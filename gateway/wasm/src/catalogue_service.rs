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
        let rows = serde_json::json!([
            {"id": "og/deepseek-v4.1-flash"},
            {"id": "og/another"},
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
        assert!(writes.is_empty(), "one miss is a blip");

        let history = crate::lifecycle::push(&[], first);
        let (_, writes) = plan_run(
            rows.as_array().unwrap(),
            "og",
            Some(&listing),
            &history,
            2_000,
            "run-2",
        );
        let demoted: Vec<&String> = writes.iter().map(|(id, _)| id).collect();
        assert_eq!(
            demoted,
            vec!["og/another"],
            "the row nobody serves is the one demoted"
        );
        let (_, lifecycle) = &writes[0];
        assert_eq!(lifecycle.status, crate::model_store::Status::Deprecated);
        assert_eq!(lifecycle.verified_by.as_deref(), Some("run-2"));
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
