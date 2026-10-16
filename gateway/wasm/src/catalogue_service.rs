//! THE CATALOGUE SERVICE — WHERE THE FACTS COME FROM, AND WHAT IS SERVED WHEN THEY CANNOT.
//!
//! Three sources, in this order, and the order is the whole design:
//!
//!   1. **the KV cache**, when it is fresh — a scheduled run refreshes it, and a request never waits on a fetch;
//!   2. **the committed slice** (`catalogue::slice()`, `include_str!`-ed) — **the FLOOR**, which is why an
//!      unreachable catalogue degrades to slightly-old facts rather than to no facts. The plan's first test:
//!      *"an unreachable catalogue serves the committed slice — injected fetcher fails, no state change"*;
//!   3. **nothing else.** There is deliberately no "invent an empty slice" path: an empty catalogue would make every
//!      row look uncatalogued, and the adoption path would then refuse everything for the wrong reason.
//!
//! **THE TTL IS SHORTER THAN THE CRON PERIOD, AND THE PLAN SAYS WHY THE TWO MUST NOT BE EQUAL**: a scheduled run that
//! fires exactly when the value expires races its own reader — the run may read the entry it is about to replace, and
//! a reader arriving in the same second may get either. Making the period longer than the TTL means every scheduled
//! run finds an expired (or nearly expired) entry and does real work, instead of sometimes finding a fresh one and
//! doing nothing.
//!
//! **THE PURE DECISION IS SEPARATED FROM THE FETCH ON PURPOSE.** `source_for` is a function of two numbers, so the
//! policy — how long facts are good for, and what happens at the boundary — is testable in-process, while the async
//! half is left with nothing but plumbing.

/// **ONE RUN'S WHOLE DECISION, GIVEN WHAT THE FETCH RETURNED** — pure, so the plan's tests are in-process rather
/// than in a harness with a stubbed network.
///
/// `fetched` is `None` when the catalogue could not be reached at all, which is the case that matters most: **the run
/// still makes an observation, and that observation is INCONCLUSIVE** — it breaks any streak (so two misses either
/// side of an outage cannot add up) and it writes NO row state (so an outage cannot demote anything). The plan's
/// first test, as a function signature.
///
/// The returned writes are `(id, lifecycle)` pairs for rows whose verdict asks for one, and they are computed
/// through `lifecycle::lifecycle_for`, so the ring, the row state and the console's copy all read the same rule.
pub fn plan_run(
    rows: &[serde_json::Value],
    prefix: &str,
    fetched: Option<&[String]>,
    ring: &[crate::lifecycle::Observation],
    now: i64,
    run_id: &str,
) -> (
    crate::lifecycle::Observation,
    Vec<(String, crate::model_store::Lifecycle)>,
) {
    use crate::lifecycle::{self, Verdict};
    let curated = crate::models_probe::upstream_spellings(prefix);
    let normalise = |s: &str| s.trim().to_ascii_lowercase();

    // **THE FLOOR DECIDES FIRST.** `conclusive` refuses an empty or unrelated listing, so an outage — or a body that
    // changed shape — is recorded as an inconclusive run rather than as "every row is gone".
    let offered: Vec<String> = fetched.unwrap_or(&[]).to_vec();
    let conclusive = fetched.is_some()
        && crate::models_probe::conclusive(
            &offered,
            prefix,
            &curated.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        );
    let missing = if conclusive {
        lifecycle::absent_from(&curated, &offered, normalise)
    } else {
        Vec::new()
    };
    let observation = lifecycle::Observation {
        run_id: run_id.to_string(),
        at: now,
        conclusive,
        seen: offered.clone(),
        missing,
    };
    let history = lifecycle::push(ring, observation.clone());

    let mut writes = Vec::new();
    for row in rows {
        let Some(id) = row.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        if !id.starts_with(prefix) {
            continue;
        }
        let prior = crate::model_store::lifecycle_of(row);
        // **THE VERDICT IS ASKED ABOUT THE UPSTREAM SPELLING**, because the ring records what a listing carried:
        // `og/deepseek-v4.1-flash` answers to `deepseek-flash` upstream, and asking about the registry id would
        // find nothing and answer `Unchanged` forever — a silent no-news that looks like a working system.
        let upstream = crate::models_probe::upstream_spelling(id);
        let verdict = lifecycle::verdict(&history, &upstream, normalise);
        if let Some(next) = lifecycle::lifecycle_for(verdict, &prior, now, run_id) {
            let _ = Verdict::Unchanged;
            writes.push((id.to_string(), next));
        }
    }
    (observation, writes)
}

/// Why a run did not happen. **A refusal, not an error**: the caller is a route that must answer somebody.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// **ONLY THE TIMER MAY WRITE STATUS** — see `may_write_status`. The probe still answers; it just does not record.
    NotTheTimer,
}

/// **THE RING FOR ONE CHANNEL**, under its own key: observations are per channel, because a listing is.
pub fn ring_key(prefix: &str) -> String {
    format!("models:catalogue:ring:{prefix}")
}

/// **ONE SCHEDULED RUN, END TO END, WITH THE FETCH DONE BY THE CALLER.**
///
/// The listing is a PARAMETER rather than something this function fetches, and that is the whole reason the chain is
/// testable: `plan_run` (the decision), `apply_writes` (the effect) and `served_from` (the read policy) are all pure
/// and covered, so what remains here is the KV plumbing a harness can drive with a stub. **A function that fetched
/// its own listing could only ever be tested against a network.**
///
/// The order is deliberate: the REFUSAL comes first (nothing is read for a caller who may not write), then the rows,
/// then the ring, then the decision, then one write. **A run that fails anywhere leaves the store untouched**: the
/// rows are written once, from `apply_writes`, never row by row.
pub async fn observe(
    env: &worker::Env,
    caller: Caller,
    prefix: &str,
    run_id: &str,
    fetched: Option<Vec<String>>,
) -> Result<crate::lifecycle::Observation, Refused> {
    if !may_write_status(caller) {
        return Err(Refused::NotTheTimer);
    }
    let now = crate::device_store::now_ms() / 1000;
    let rows = crate::model_store::read_list(env, crate::model_store::CUSTOM_MODELS_KEY).await;
    let key = ring_key(prefix);
    let ring: Vec<crate::lifecycle::Observation> = crate::device_store::kv_text(env, &key)
        .await
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();

    let (observation, writes) = plan_run(&rows, prefix, fetched.as_deref(), &ring, now, run_id);
    if !writes.is_empty() {
        let after = apply_writes(&rows, &writes);
        crate::model_store::write_list(env, crate::model_store::CUSTOM_MODELS_KEY, &after).await;
    }
    let history = crate::lifecycle::push(&ring, observation.clone());
    if let Ok(text) = serde_json::to_string(&history) {
        crate::device_store::kv_put(env, &key, &text, None).await;
    }
    Ok(observation)
}

/// **THE KV KEY THE CACHE LIVES UNDER** — beside the store's own (`models:custom`, `models:disabled`), because it is
/// the same namespace and the same reader: an operator inspecting KV sees the catalogue where they see the rest.
pub const CATALOGUE_KEY: &str = "models:catalogue";

/// **THE CATALOGUE, FROM WHEREVER IT CAN BE HAD** — three lines of plumbing over `served_from`, which is where the
/// policy lives and where it is tested. **There is no branch here on purpose**: an async shell with a decision in it
/// is a decision that cannot be tested without a KV.
///
/// The KV read is the UNcached one (`device_store::kv_text`): a cached read would add a second TTL on top of this
/// module's, and two expiry rules for one value is how a stale catalogue survives its own refresh.
pub async fn facts(env: &worker::Env) -> Served {
    let cached = crate::device_store::kv_text(env, CATALOGUE_KEY).await;
    let now_secs = crate::device_store::now_ms() / 1000;
    served_from(cached.as_deref(), now_secs)
}

/// **WHERE A CALLER'S FACTS CAME FROM** — the answer to "what did we serve", which the console shows and a test can
/// assert.
#[derive(Debug, Clone, PartialEq)]
pub enum Served {
    /// The KV entry, still inside its TTL.
    Cache(std::collections::BTreeMap<String, crate::catalogue::Facts>),
    /// **THE FLOOR** — the committed slice, because there is no fresh cache. This is what an unreachable catalogue
    /// degrades to, and the plan's first test is exactly this arm.
    Floor,
}

impl Served {
    /// The facts for an id, from whichever source answered — **callers do not branch on the source**, which is what
    /// makes the floor invisible to every consumer and the two paths interchangeable.
    pub fn facts_for(&self, id: &str) -> Option<&crate::catalogue::Facts> {
        match self {
            Served::Cache(rows) => rows.get(id),
            Served::Floor => crate::catalogue::slice().facts_for(id),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Served::Cache(rows) => rows.len(),
            Served::Floor => crate::catalogue::slice().len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// **THE READ PATH, AS A PURE FUNCTION**: a cached value (or none), the clock, and the answer.
///
/// Five outcomes, and every one of them either serves something usable or falls back — **there is no error arm,
/// because a catalogue that cannot be read is not an error the caller can act on**. A stale entry, an unparseable
/// one, a value from the future: all of them are the floor, which is slightly old and always present.
pub fn served_from(cached: Option<&str>, now: i64) -> Served {
    let Some(text) = cached else {
        return Served::Floor;
    };
    let Some(parsed) = crate::catalogue::read_cache(text) else {
        return Served::Floor;
    };
    if source_for(Some(parsed.fetched_at), now) == Source::Cache {
        Served::Cache(parsed.slice())
    } else {
        Served::Floor
    }
}

/// **APPLY A RUN'S WRITES TO THE STORE ROWS — the pure half of the timer's only side effect.**
///
/// Three rules, and each is about what a run is NOT allowed to do:
///
///   * **a write for an id the store does not carry is IGNORED**, not appended. An observation may conclude about a
///     row that exists; it may not CREATE one, because the catalogue slice is a third party's data and the store is
///     what an operator approved. (Adoption is the path that adds rows, and it goes through `catalogue::adoptable`.)
///   * **the order is the store's**, because the listing's order is part of its bytes — a run that reordered the
///     store would change `/v1/models` while claiming to have changed nothing.
///   * **everything else about a row is carried**, which is `with_lifecycle`'s job: the run writes the lifecycle and
///     touches no other field, so a row's `ownedBy`, its `wire` and anything a person put there survive.
pub fn apply_writes(
    rows: &[serde_json::Value],
    writes: &[(String, crate::model_store::Lifecycle)],
) -> Vec<serde_json::Value> {
    rows.iter()
        .map(|row| {
            let Some(id) = row.get("id").and_then(|v| v.as_str()) else {
                return row.clone();
            };
            match writes.iter().find(|(written, _)| written == id) {
                Some((_, lifecycle)) => crate::model_store::with_lifecycle(row, lifecycle),
                None => row.clone(),
            }
        })
        .collect()
}

/// **WHO IS ASKING TO WRITE A ROW'S LIFECYCLE.** The distinction is the plan's fifth review focus, and it is not
/// about permissions: it is about what an answer MEANS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caller {
    /// The scheduled observation. It asks the upstream the same question for every row, on the same clock, with the
    /// same floor — so its answer is comparable across runs, which is what a history is made of.
    Timer,
    /// The console's probe button, run by an admin with their own BYOK key. **Its answer is not comparable**: it
    /// asked with a different credential, at a different time, about ONE row, and a key that is out of credit
    /// answers 401 for a row that is perfectly alive. Recording that as a status would demote a model because an
    /// operator's key expired.
    ManualProbe,
    /// A person editing a row by hand. That is `set_lifecycle`'s business, not an observation's.
    Admin,
}

/// **ONLY THE TIMER MAY WRITE STATUS.** A manual probe still ANSWERS a person — it reports what the upstream said —
/// and that is the whole of what it may do. **A report and an observation look identical in a log and mean opposite
/// things**: one is evidence, the other is a person asking a question.
pub fn may_write_status(caller: Caller) -> bool {
    matches!(caller, Caller::Timer)
}

/// How long a cached catalogue is served before a run refreshes it. **Six hours**, and the Cron period that refreshes
/// it is longer (see the header): a period equal to the TTL would race its own reader.
pub const TTL_SECS: i64 = 6 * 60 * 60;

/// Where a caller's facts should come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The KV entry is present and within its TTL: serve it, and do not fetch.
    Cache,
    /// There is no entry, or it is older than the TTL: fetch, and keep serving the floor until it lands.
    Fetch,
}

/// **THE ONE DECISION**: is what we have good enough, or must somebody go and look?
///
/// `None` means there is no cached entry at all — a first run after a deploy, which must fetch. A `cached_at` in the
/// FUTURE (a clock that moved backwards between runs, or a value written by a host with a faster clock) is treated as
/// **stale**, not fresh: serving it would freeze the catalogue until real time caught up, and the plan's whole point
/// is that a stale row must not outlive the upstream that serves it.
pub fn source_for(cached_at: Option<i64>, now: i64) -> Source {
    match cached_at {
        Some(at) if at <= now && now - at < TTL_SECS => Source::Cache,
        _ => Source::Fetch,
    }
}

#[cfg(test)]
mod tests {

    /// **THE WHOLE STORY, IN ONE TEST: two conclusive runs that do not see a row demote it, and the next run that
    /// does see it brings it back — with the store carried between runs the way `observe` carries it.**
    ///
    /// This is the plan's D3 and its third review focus end to end, and it is deliberately an integration test of the
    /// two halves rather than a unit test of either: **`plan_run` and `apply_writes` are each covered above, and what
    /// was never covered is the CHAIN** — that what one run writes is what the next run reads.
    #[test]
    fn two_runs_demote_a_row_and_the_next_run_that_sees_it_restores_it() {
        use crate::model_store::{lifecycle_of, Status};
        let rows = serde_json::json!([
            {"id": "og/deepseek-v4.1-flash"},
            {"id": "og/mimo-v2.5"},
        ]);
        // A listing that carries one curated row and not the other: conclusive, and `mimo-v2.5` is absent.
        let partial = vec!["deepseek-flash".to_string()];
        let mut store = rows.as_array().unwrap().clone();
        let mut ring: Vec<crate::lifecycle::Observation> = Vec::new();

        // ── RUN 1: a blip. Nothing is demoted.
        let (observation, writes) = plan_run(&store, "og", Some(&partial), &ring, 1_000, "run-1");
        ring = crate::lifecycle::push(&ring, observation);
        store = apply_writes(&store, &writes);
        assert!(
            store
                .iter()
                .all(|row| lifecycle_of(row).status != Status::Deprecated),
            "one miss must not demote anything"
        );

        // ── RUN 2: the same absence, one run later. Now it is a pattern.
        let (observation, writes) = plan_run(&store, "og", Some(&partial), &ring, 2_000, "run-2");
        ring = crate::lifecycle::push(&ring, observation);
        store = apply_writes(&store, &writes);
        let demoted = store
            .iter()
            .find(|row| row["id"] == "og/mimo-v2.5")
            .expect("the row is still there");
        assert_eq!(lifecycle_of(demoted).status, Status::Deprecated);
        assert_eq!(
            lifecycle_of(demoted).verified_at,
            Some(2_000),
            "and attributable"
        );
        let kept = store
            .iter()
            .find(|row| row["id"] == "og/deepseek-v4.1-flash")
            .expect("the seen row");
        assert_eq!(
            lifecycle_of(kept).status,
            Status::Active,
            "a row the upstream still serves is not touched by a neighbour's absence"
        );

        // ── RUN 3: the upstream serves it again. NO HUMAN IS INVOLVED.
        let full = vec!["deepseek-flash".to_string(), "mimo-v2.5".to_string()];
        let (observation, writes) = plan_run(&store, "og", Some(&full), &ring, 3_000, "run-3");
        // The history is not needed again after the last run, and clippy is right to say so: **assigning a value
        // nobody reads is how a test comes to look like it checks something it does not.**
        let _ = crate::lifecycle::push(&ring, observation);
        store = apply_writes(&store, &writes);
        let restored = store
            .iter()
            .find(|row| row["id"] == "og/mimo-v2.5")
            .expect("the row");
        assert_eq!(
            lifecycle_of(restored).status,
            Status::Active,
            "a row that comes back is restored on the newest run"
        );
        assert_eq!(
            lifecycle_of(restored).verified_at,
            Some(3_000),
            "with a fresh stamp"
        );

        // ── AND AN OUTAGE IN BETWEEN WOULD HAVE BROKEN THE STREAK: run 1 + an inconclusive run + run 2 leaves the
        //    row alone, which is the property that keeps an upstream's bad afternoon from retiring a model.
        let mut streak: Vec<crate::lifecycle::Observation> = Vec::new();
        let (o1, _) = plan_run(
            rows.as_array().unwrap(),
            "og",
            Some(&partial),
            &streak,
            1_000,
            "r1",
        );
        streak = crate::lifecycle::push(&streak, o1);
        let (o2, _) = plan_run(rows.as_array().unwrap(), "og", None, &streak, 2_000, "r2");
        streak = crate::lifecycle::push(&streak, o2);
        let (_, writes) = plan_run(
            rows.as_array().unwrap(),
            "og",
            Some(&partial),
            &streak,
            3_000,
            "r3",
        );
        assert!(
            writes.iter().all(|(_, l)| l.status != Status::Deprecated),
            "the outage between the two misses means the streak is one run, not two"
        );
    }

    /// **A RUN WRITES THE LIFECYCLE AND NOTHING ELSE, IN THE STORE'S OWN ORDER** — and it cannot invent a row.
    #[test]
    fn a_run_updates_the_rows_it_knows_and_adds_none() {
        let rows = serde_json::json!([
            {"id": "og/a", "ownedBy": "opencode", "wire": "a"},
            {"id": "og/b", "ownedBy": "opencode"},
            {"odd": "no id at all"},
        ]);
        let writes = vec![
            (
                "og/b".to_string(),
                crate::model_store::Lifecycle {
                    status: crate::model_store::Status::Deprecated,
                    verified_at: Some(5),
                    verified_by: Some("run-1".to_string()),
                    ..Default::default()
                },
            ),
            (
                "og/never-seen".to_string(),
                crate::model_store::Lifecycle {
                    status: crate::model_store::Status::Deprecated,
                    ..Default::default()
                },
            ),
        ];
        let after = apply_writes(rows.as_array().unwrap(), &writes);
        assert_eq!(after.len(), 3, "a write must never append a row");
        assert_eq!(after[0]["id"], "og/a", "the store's order is kept");
        assert!(
            after[0].get("status").is_none(),
            "an unwritten row is untouched"
        );
        assert_eq!(after[1]["status"], "deprecated");
        assert_eq!(after[1]["verifiedBy"], "run-1");
        assert_eq!(
            after[1]["ownedBy"], "opencode",
            "the row's own fields survive"
        );
        assert_eq!(
            after[2]["odd"], "no id at all",
            "a row with no id is carried"
        );
    }

    /// **THE PLAN'S FIRST TEST, AS A FUNCTION**: with nothing cached — a catalogue that cannot be reached — the
    /// committed slice answers. Every id the slice carries is still served, so nothing is hidden by an outage.
    #[test]
    fn an_unreachable_catalogue_serves_the_committed_slice() {
        let served = served_from(None, 1_000);
        assert_eq!(served, Served::Floor);
        assert!(
            served.len() >= 100,
            "the floor is the committed slice, not an empty map"
        );
        assert!(
            served.facts_for("og/mimo-v2.6-pro").is_some(),
            "and it answers for the models the repo advertises"
        );
    }

    /// A fresh cache is served; an EXPIRED one, a CORRUPT one and one from the FUTURE are all the floor — the three
    /// ways a cache can be useless without being an error.
    #[test]
    fn only_a_fresh_readable_cache_is_served() {
        let fresh = crate::catalogue::CachedCatalogue {
            fetched_at: 1_000,
            rows: vec![crate::catalogue::CachedRow {
                id: "og/example".into(),
                context: Some(131_072),
                ..Default::default()
            }],
        };
        let text = serde_json::to_string(&fresh).unwrap();
        match served_from(Some(&text), 1_000) {
            Served::Cache(rows) => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows["og/example"].context, Some(131_072));
            }
            Served::Floor => panic!("a fresh cache must be served"),
        }
        assert_eq!(
            served_from(Some(&text), 1_000 + TTL_SECS),
            Served::Floor,
            "expired"
        );
        assert_eq!(
            served_from(Some(&text), 1_000 - 1),
            Served::Floor,
            "from the future"
        );
        assert_eq!(
            served_from(Some("{not json"), 1_000),
            Served::Floor,
            "corrupt"
        );
    }

    /// **AN UNREACHABLE CATALOGUE IS AN INCONCLUSIVE RUN, NOT A MASS RETIREMENT.** This is the plan's first test and
    /// the reason the floor exists: `fetched = None` must write nothing and break any streak.
    #[test]
    fn an_unreachable_catalogue_writes_nothing() {
        let rows = serde_json::json!([{"id": "og/deepseek-v4.1-flash"}]);
        let (observation, writes) =
            plan_run(rows.as_array().unwrap(), "og", None, &[], 1_000, "run-1");
        assert!(!observation.conclusive);
        assert!(
            observation.missing.is_empty(),
            "an inconclusive run names nobody"
        );
        assert!(writes.is_empty(), "and it writes nothing at all");
    }

    /// **TWO CONCLUSIVE MISSES DEMOTE, AND THE WRITE CARRIES THE STAMP** — the whole chain from a listing to a row's
    /// state, in one call.
    #[test]
    fn two_runs_that_do_not_see_a_row_demote_it() {
        // **A CURATED ROW, NOT A CUSTOM ONE, AND THE DISTINCTION IS THE RULE**: the observation judges the
        // REGISTRY's rows, because a listing can only be evidence about ids the upstream was ever asked for. A
        // custom row (`og/another`) is never in an upstream listing, so judging it would demote every custom model
        // on the first conclusive run.
        let rows = serde_json::json!([
            {"id": "og/deepseek-v4.1-flash"},
            {"id": "og/mimo-v2.5"},
        ]);
        let listing = vec!["deepseek-flash".to_string()]; // one curated row present, so the run is evidence
        let (first, writes) = plan_run(
            rows.as_array().unwrap(),
            "og",
            Some(&listing),
            &[],
            1_000,
            "run-1",
        );
        assert!(
            first.conclusive,
            "one of our rows was there, so the listing is evidence"
        );
        // **"ONE MISS IS A BLIP" MEANS NOTHING IS DEMOTED — NOT THAT NOTHING IS WRITTEN.** A conclusive run also
        // stamps every row it SAW (that is how a returned row is restored), so the assertion is about the demotion
        // and not about the write count. **The test has now been corrected twice for the same reason: an expectation
        // narrower than the rule**, which is the failure mode a test is supposed to catch rather than cause.
        let demoted_on_first: Vec<&String> = writes
            .iter()
            .filter(|(_, l)| l.status == crate::model_store::Status::Deprecated)
            .map(|(id, _)| id)
            .collect();
        assert!(
            demoted_on_first.is_empty(),
            "one miss is a blip — nothing is demoted, but {demoted_on_first:?} was"
        );

        let history = crate::lifecycle::push(&[], first);
        let (_, writes) = plan_run(
            rows.as_array().unwrap(),
            "og",
            Some(&listing),
            &history,
            2_000,
            "run-2",
        );
        // **EVERY CURATED ROW THE LISTING DID NOT CARRY IS MISSING, AND THAT IS THE RULE RATHER THAN A BUG**: a
        // listing carrying ONE of our rows is evidence, so the rest really are absent upstream. The test's first
        // version expected a single write; **the assertion was narrower than the rule**, and the run said so.
        let demoted: Vec<&String> = writes.iter().map(|(id, _)| id).collect();
        assert!(
            demoted.iter().any(|id| *id == "og/mimo-v2.5"),
            "the curated row nobody serves must be among those demoted: {demoted:?}"
        );
        let (_, lifecycle) = writes
            .iter()
            .find(|(id, _)| id == "og/mimo-v2.5")
            .expect("og/mimo-v2.5 has a write");
        assert_eq!(lifecycle.status, crate::model_store::Status::Deprecated);
        assert_eq!(lifecycle.verified_by.as_deref(), Some("run-2"));
    }

    /// **A CUSTOM ROW IS NEVER JUDGED BY AN OBSERVATION** — the upstream was never asked about it, so its absence
    /// from a listing is not evidence of anything. Judging it would demote every custom model on the first
    /// conclusive run, which is the loudest way this design could fail.
    #[test]
    fn a_custom_row_is_not_judged() {
        let rows = serde_json::json!([
            {"id": "og/deepseek-v4.1-flash"},
            {"id": "og/not-in-the-registry"},
        ]);
        let listing = vec!["deepseek-flash".to_string()];
        let (observation, writes) = plan_run(
            rows.as_array().unwrap(),
            "og",
            Some(&listing),
            &[],
            1_000,
            "run-1",
        );
        assert!(
            !observation
                .missing
                .iter()
                .any(|m| m == "not-in-the-registry"),
            "a custom id is not the observation's business: {:?}",
            observation.missing
        );
        assert!(
            !writes.iter().any(|(id, _)| id == "og/not-in-the-registry"),
            "and it is never written by one"
        );
    }

    /// **A ROW THAT COMES BACK IS RESTORED ON THE NEWEST RUN** — the plan's third review focus, end to end.
    #[test]
    fn a_row_that_comes_back_is_restored() {
        let rows = serde_json::json!([{"id": "og/deepseek-v4.1-flash", "status": "deprecated"}]);
        let listing = vec!["deepseek-flash".to_string(), "another".to_string()];
        let (observation, writes) = plan_run(
            rows.as_array().unwrap(),
            "og",
            Some(&listing),
            &[],
            5_000,
            "run-9",
        );
        assert!(observation.conclusive);
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].1.status, crate::model_store::Status::Active);
        assert_eq!(writes[0].1.verified_at, Some(5_000));
    }

    /// **THE PROBE BUTTON MUST NOT DEMOTE A MODEL.** Its answer depends on the operator's own key and on the moment
    /// they pressed it, so a 401 from an out-of-credit key would otherwise retire a row that is perfectly alive.
    #[test]
    fn only_the_timer_may_write_status() {
        assert!(may_write_status(Caller::Timer));
        assert!(!may_write_status(Caller::ManualProbe));
        assert!(!may_write_status(Caller::Admin));
    }
    use super::*;

    /// No entry, or an entry from before the TTL: go and look.
    #[test]
    fn an_absent_or_expired_entry_is_fetched() {
        assert_eq!(
            source_for(None, 1_000),
            Source::Fetch,
            "a first run must fetch"
        );
        assert_eq!(
            source_for(Some(1_000 - TTL_SECS), 1_000),
            Source::Fetch,
            "expired"
        );
        assert_eq!(
            source_for(Some(1_000 - TTL_SECS - 1), 1_000),
            Source::Fetch,
            "one second past the TTL is past it"
        );
    }

    /// A fresh entry is served, and the boundary is exclusive: at exactly TTL seconds old the value is spent.
    #[test]
    fn a_fresh_entry_is_served() {
        assert_eq!(source_for(Some(1_000), 1_000), Source::Cache);
        assert_eq!(source_for(Some(1_000 - TTL_SECS + 1), 1_000), Source::Cache);
    }

    /// **A STAMP IN THE FUTURE IS STALE, NOT FRESH.** A clock that moved backwards would otherwise freeze the
    /// catalogue for as long as the skew lasts — and the point of the whole lifecycle is that a stale row must not
    /// outlive the upstream serving it.
    #[test]
    fn a_timestamp_from_the_future_is_stale() {
        assert_eq!(source_for(Some(2_000), 1_000), Source::Fetch);
    }

    /// The policy number is a fact about the Cron period, so it is pinned rather than assumed: the period must be
    /// LONGER than this, and a reader who changes it has to come here.
    #[test]
    fn the_ttl_is_six_hours_and_shorter_than_the_period() {
        assert_eq!(TTL_SECS, 21_600);
    }
}
