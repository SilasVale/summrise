//! THE OBSERVATION RING — WHAT A SCHEDULED CHECK SAW, AND THE VERDICT READ FROM IT.
//!
//! **WHY A RING AND NOT A COUNTER.** The plan's hardest constraint: a Worker's isolates do not share memory, and a KV
//! read-modify-write can be LOST or REORDERED under concurrency — so a counter that increments per run would drift
//! the first time two runs overlapped, and it would drift in the direction of retiring a model. **This module never
//! increments anything.** A run APPENDS one entry describing what it saw, and the verdict is a PURE FUNCTION of the
//! last few entries. Two runs racing produce one of two orders; neither can accumulate.
//!
//! **`K = 2` (`DEMOTE_AFTER`), AND IT IS EXACT.** Two conclusive runs in a row that both fail to see a row are
//! enough to demote it; one is not. The plan forbids rounding it to a "sensible" default, because the number IS the
//! policy: one run is a blip (an upstream's replica lagging), two is a pattern.
//!
//! **AND THE RING IS NOT `model_store::Lifecycle`.** That type is the STATE a row carries (what a check concluded);
//! this module is the HISTORY a channel carries (what the last runs saw). The state is written from a verdict; the
//! history is what the verdict is read from, and keeping them apart is what makes the second one testable in-process.
use serde::{Deserialize, Serialize};

/// How many consecutive conclusive misses demote a row. **Exact, not a default** — see the module header.
pub const DEMOTE_AFTER: usize = 2;

/// One run's observation of ONE channel: what it asked, what came back, and whether the answer was evidence at all.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    /// The run's own id, so a reader can tell two entries apart and see which run wrote which.
    pub run_id: String,
    /// Unix seconds — what a person reads a history by.
    pub at: i64,
    /// **THE FLOOR'S ANSWER** (`models_probe::conclusive`). An inconclusive run is recorded rather than dropped: it
    /// is what BREAKS a streak, and a ring that quietly skipped it would demote on two runs that were not adjacent.
    pub conclusive: bool,
    /// The ids the listing carried, as the upstream spells them.
    pub seen: Vec<String>,
    /// The curated ids of this channel that the listing did NOT carry — computed by the caller, which knows the
    /// channel's rows, so this module stays a ring rather than a lookup.
    pub missing: Vec<String>,
}

/// **APPEND, AND KEEP ONLY WHAT A VERDICT CAN READ.** The ring holds `DEMOTE_AFTER + 1` entries: enough for the
/// streak AND for the newest "seen" that ends it, which is the most the verdict below ever looks at.
pub fn push(ring: &[Observation], observation: Observation) -> Vec<Observation> {
    let keep = DEMOTE_AFTER + 1;
    let mut out: Vec<Observation> = ring.to_vec();
    out.push(observation);
    if out.len() > keep {
        out.drain(0..out.len() - keep);
    }
    out
}

/// **WHICH CURATED ROWS A CONCLUSIVE LISTING DID NOT CARRY** — the input a ring entry's `missing` is built from.
///
/// The caller has already asked `models_probe::conclusive` whether the listing is evidence at all; **this function
/// assumes it is** and answers the narrower question. Both sides are normalised by the caller's `normalise`, so a
/// wire-remapped or differently-cased id is the same row.
///
/// It returns them in the order the CURATED list gave, not the listing's, because the result is used to name rows in
/// a KV write and in a console message — **an order that came from an upstream's response would shuffle a report
/// every run.**
pub fn absent_from(
    curated: &[String],
    offered: &[String],
    normalise: impl Fn(&str) -> String,
) -> Vec<String> {
    let have: Vec<String> = offered.iter().map(|id| normalise(id)).collect();
    curated
        .iter()
        .filter(|id| !have.iter().any(|seen| *seen == normalise(id)))
        .cloned()
        .collect()
}

/// **THE ROW STATE A VERDICT WRITES** — the one place a `Verdict` becomes a `model_store::Lifecycle`, so the timer,
/// the admin state and the console's copy cannot disagree about what a verdict means.
///
/// The rules, and each exists because its absence would be a lie:
///
///   * `Demote` writes `Deprecated` — **and keeps `verifiedAt`/`verifiedBy`**, because the stamp is WHEN the
///     conclusion was reached and a demotion is a conclusion like any other. A row with no stamp reads as never
///     checked, which would make the demotion unattributable.
///   * `Active` writes `Active` with a fresh stamp, which is also what RESTORES a row: the plan's third review focus
///     is that a row which comes back upstream returns without a human, and this is where that happens.
///   * `Unchanged` writes **nothing** (`None`) — an inconclusive run must not touch the row at all, because a write
///     would move `verifiedAt` and make an unknown look like a check.
///   * **`ephemeral` and `assumed` are CARRIED, never invented**: they are the adoption path's facts (Task 7), and a
///     scheduled observation has no business changing them. A row that was ephemeral stays ephemeral while it is
///     demoted, so the console can still say why it was there.
pub fn lifecycle_for(
    verdict: Verdict,
    prior: &crate::model_store::Lifecycle,
    now: i64,
    run_id: &str,
) -> Option<crate::model_store::Lifecycle> {
    use crate::model_store::{Lifecycle, Status};
    match verdict {
        Verdict::Unchanged => None,
        Verdict::Active => Some(Lifecycle {
            status: Status::Active,
            ephemeral: prior.ephemeral,
            verified_at: Some(now),
            verified_by: Some(run_id.to_string()),
            assumed: prior.assumed.clone(),
        }),
        Verdict::Demote => Some(Lifecycle {
            status: Status::Deprecated,
            ephemeral: prior.ephemeral,
            verified_at: Some(now),
            verified_by: Some(run_id.to_string()),
            assumed: prior.assumed.clone(),
        }),
    }
}

/// What the history says about one id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The newest run saw it: it is served, whatever the runs before it said.
    Active,
    /// The last `DEMOTE_AFTER` runs were all conclusive AND all failed to see it.
    Demote,
    /// Not enough evidence either way — the default, and the safe one.
    Unchanged,
}

/// **THE VERDICT, READ FROM THE RING.** The order of the two questions is the whole rule:
///
///   1. **DID THE NEWEST RUN SEE IT?** If so it is `Active` — a row that comes back upstream returns without a
///      human, which is the plan's third review focus. Note this is checked FIRST, so `[miss, miss, seen]` is
///      `Active` even though the two entries before it were misses.
///   2. **ARE THE LAST `K` RUNS CONCLUSIVE AND ALL MISSING?** Then `Demote`. An inconclusive run anywhere in that
///      window makes the answer `Unchanged` — it breaks the streak, which is why the ring records those runs rather
///      than skipping them.
///
/// `normalise` maps an id to the spelling both sides are compared in — the caller's business, because it knows the
/// channel's wire names (`models_probe::upstream_spellings`).
pub fn verdict(ring: &[Observation], id: &str, normalise: impl Fn(&str) -> String) -> Verdict {
    let wanted = normalise(id);
    let seen_in = |observation: &Observation| {
        observation
            .seen
            .iter()
            .any(|candidate| normalise(candidate) == wanted)
    };
    let missing_in = |observation: &Observation| {
        observation
            .missing
            .iter()
            .any(|candidate| normalise(candidate) == wanted)
    };

    if ring.last().is_some_and(seen_in) {
        return Verdict::Active;
    }
    if ring.len() < DEMOTE_AFTER {
        return Verdict::Unchanged;
    }
    let window = &ring[ring.len() - DEMOTE_AFTER..];
    let all_missing = window
        .iter()
        .all(|observation| observation.conclusive && missing_in(observation));
    if all_missing {
        Verdict::Demote
    } else {
        Verdict::Unchanged
    }
}

#[cfg(test)]
mod tests {

    /// **THE MISSING LIST IS BUILT FROM THE CURATED ORDER, NOT THE LISTING'S**: it names rows in a KV write and in a
    /// console message, and an order that came from an upstream's response would shuffle a report every run.
    #[test]
    fn the_absent_list_keeps_the_curated_order() {
        let curated: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let offered: Vec<String> = ["c", "  B "].iter().map(|s| s.to_string()).collect();
        assert_eq!(absent_from(&curated, &offered, normalise), vec!["a"]);
        assert!(absent_from(&curated, &curated, normalise).is_empty());
    }

    /// **A DEMOTION CARRIES ITS STAMP**: the conclusion is attributable, and `Unchanged` writes NOTHING — an
    /// inconclusive run that touched `verifiedAt` would make an unknown look like a check.
    #[test]
    fn only_a_conclusion_writes_a_row_state() {
        let prior = crate::model_store::Lifecycle {
            ephemeral: true,
            assumed: vec!["usEgress".to_string()],
            ..Default::default()
        };
        assert!(lifecycle_for(Verdict::Unchanged, &prior, 7, "run-1").is_none());

        let demoted =
            lifecycle_for(Verdict::Demote, &prior, 7, "run-1").expect("a demotion writes");
        assert_eq!(demoted.status, crate::model_store::Status::Deprecated);
        assert_eq!(demoted.verified_at, Some(7));
        assert_eq!(demoted.verified_by.as_deref(), Some("run-1"));
        // The adoption path's facts are CARRIED, never invented by an observation.
        assert!(demoted.ephemeral);
        assert_eq!(demoted.assumed, vec!["usEgress".to_string()]);

        let restored =
            lifecycle_for(Verdict::Active, &demoted, 9, "run-2").expect("a return writes");
        assert_eq!(restored.status, crate::model_store::Status::Active);
        assert_eq!(
            restored.verified_at,
            Some(9),
            "a return is a fresh conclusion"
        );
        assert!(restored.ephemeral, "a return does not un-mark a free tier");
    }
    use super::*;

    fn observation(conclusive: bool, seen: &[&str], missing: &[&str]) -> Observation {
        Observation {
            run_id: "r".into(),
            at: 0,
            conclusive,
            seen: seen.iter().map(|s| (*s).to_string()).collect(),
            missing: missing.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    const ID: &str = "og/deepseek-v4.1-flash";
    fn normalise(s: &str) -> String {
        s.trim().to_ascii_lowercase()
    }

    /// **ONE MISS IS A BLIP.** An upstream's replica lagging for one run must not retire a model.
    #[test]
    fn one_missing_run_is_not_enough() {
        let ring = vec![observation(true, &[], &[ID])];
        assert_eq!(verdict(&ring, ID, normalise), Verdict::Unchanged);
        assert_eq!(DEMOTE_AFTER, 2, "the policy number is exact");
    }

    /// Two consecutive conclusive misses demote.
    #[test]
    fn two_conclusive_missing_runs_demote() {
        let ring = vec![observation(true, &[], &[ID]), observation(true, &[], &[ID])];
        assert_eq!(verdict(&ring, ID, normalise), Verdict::Demote);
    }

    /// **AN INCONCLUSIVE RUN BREAKS THE STREAK** — the empty 200 that must hide nothing is recorded, and it is what
    /// stops two non-adjacent misses from adding up.
    #[test]
    fn an_inconclusive_run_breaks_the_streak() {
        let ring = vec![
            observation(true, &[], &[ID]),
            observation(false, &[], &[ID]),
            observation(true, &[], &[ID]),
        ];
        assert_eq!(verdict(&ring, ID, normalise), Verdict::Unchanged);
    }

    /// **A ROW THAT COMES BACK RETURNS WITHOUT A HUMAN**, and it returns on the NEWEST run — even though the two
    /// entries before it were misses.
    #[test]
    fn a_seen_row_returns_to_active() {
        let ring = vec![
            observation(true, &[], &[ID]),
            observation(true, &[], &[ID]),
            observation(true, &[ID], &[]),
        ];
        assert_eq!(verdict(&ring, ID, normalise), Verdict::Active);
    }

    /// **A LOST WRITE CHANGES NO VERDICT, BECAUSE NOTHING IS INCREMENTED**: replaying the same run over the same
    /// ring is the same ring, and the verdict is a function of what is there rather than of how many times a write
    /// landed. This is the property a counter could not have.
    #[test]
    fn a_lost_write_changes_no_verdict() {
        let ring = push(&[], observation(true, &[], &[ID]));
        let replayed = push(&ring, observation(true, &[], &[ID]));
        // The same entry pushed twice is two entries — the point is that the VERDICT is read from them, not
        // accumulated: a third push of a "seen" run must return Active regardless of what was lost before it.
        assert_eq!(verdict(&ring, ID, normalise), Verdict::Unchanged);
        assert_eq!(verdict(&replayed, ID, normalise), Verdict::Demote);
        let seen = push(&replayed, observation(true, &[ID], &[]));
        assert_eq!(verdict(&seen, ID, normalise), Verdict::Active);
    }

    /// The comparison is normalised: a wire-remapped or differently-cased id is the same row.
    #[test]
    fn the_comparison_is_normalised() {
        let ring = vec![
            observation(true, &["  DeepSeek-Flash "], &[]),
            observation(true, &["deepseek-flash"], &[]),
        ];
        assert_eq!(verdict(&ring, "DeepSeek-Flash", normalise), Verdict::Active);
        let ring = vec![
            observation(true, &[], &["DeepSeek-Flash"]),
            observation(true, &[], &["deepseek-flash"]),
        ];
        assert_eq!(
            verdict(&ring, "  deepseek-FLASH", normalise),
            Verdict::Demote
        );
    }

    /// **THE RING KEEPS EXACTLY WHAT A VERDICT CAN READ** — `K + 1`, no more: enough for the streak and for the
    /// newest "seen" that ends it.
    #[test]
    fn the_ring_keeps_k_plus_one_entries() {
        let mut ring = Vec::new();
        for i in 0..5 {
            ring = push(
                &ring,
                Observation {
                    run_id: format!("r{i}"),
                    at: i,
                    conclusive: true,
                    seen: Vec::new(),
                    missing: vec![ID.to_string()],
                },
            );
        }
        assert_eq!(ring.len(), DEMOTE_AFTER + 1);
        assert_eq!(
            ring.last().unwrap().run_id,
            "r4",
            "the newest entry is kept"
        );
    }
}
