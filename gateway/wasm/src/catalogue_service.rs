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
