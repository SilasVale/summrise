//! ONE AGE WINDOW, RESOLVED ONCE — the rule the device's records were each
//! spelling out for themselves.
//!
//! WHY THIS MODULE EXISTS. `lib.rs` declined to add one, in as many words: "a
//! shared `retention.rs` would be a primitive with one consumer, which the
//! repo's PROMOTION rule rejects". THE PREMISE WAS FALSE, and it is recorded
//! here because it is what stopped anyone looking. Age-bounding is implemented
//! FOUR times, and the copies had DRIFTED:
//!
//! | family                         | clock                            | unit |
//! |--------------------------------|----------------------------------|------|
//! | `evidence::prune`              | each file's mtime                | ms   |
//! | `runs::trim`                   | each record's `ts_ms`            | ms   |
//! | `SessionLogger::prune_stale`   | each file's `createdAt`, mtime   | s    |
//! | `memory::store`                | each record's `updated_at`       | s    |
//!
//! Two carried the 1-day floor, the audit trail carried none, the memory store
//! carried a CAP the others lacked, `86_400_000` / `86_400` was spelled out at
//! four sites, and a `DAY_MS` test constant was declared three times. A rule
//! with four spellings cannot be checked — which is why the exclusivity of the
//! boundary had to be re-asserted, in prose, by the tests of two families, each
//! citing `prune_stale` as the semantics it mirrored: the third copy of one rule.
//!
//! WHAT THIS MODULE OWNS: the window — the floor, the cap, the day conversion
//! and the exclusive boundary — and NOTHING ELSE. No IO, no directory walk, no
//! per-family predicate, no clock read: each family keeps its own clock, its own
//! reason to delete a thing, and its own count returned to its caller. It is a
//! module of arithmetic, and it is deep in the only way that matters here: the
//! decision is one call, and the four call sites cannot disagree because there
//! is one place left for them to agree with.
//!
//! NOT A CLIENT: `runstate::prune_history()` bounds `BOOT_HISTORY_MAX` boot
//! records by COUNT, not by age. There is no window there to resolve, and
//! counting it as a fifth consumer would be a category error — stated where a
//! reader looking for the missing caller will look.
//!
//! THE FLOOR IS A CORRECTNESS PROPERTY, NOT A POLICY. A prune that removes the
//! record an in-flight operation JUST wrote is worse than a full disk, so the
//! guarantee is structural rather than a promise about the configured window:
//! whatever a caller passes — a `retention_days: 0` typo, a caller that skipped
//! `RetentionConfig::effective`, a future re-wiring — the cutoff is never closer
//! to now than yesterday. A window that would otherwise be honoured is clamped
//! UP, never down.
//!
//! THE CAP IS NOT A POLICY CHOICE EITHER; IT IS THE WRAP. `days` arrives from
//! config and from `PUT /api/settings` with no upper bound: in DEBUG a large
//! value panics on the multiply, and in RELEASE it WRAPS — `days = 2^57` is
//! exactly 0 mod 2^64, so the cutoff became NOW and an entire store was retired
//! by a setting that meant "keep everything". A century is far past any real
//! retention and makes the arithmetic total. That incident is preserved verbatim
//! at its own call site (`plugins/memory/store.rs`), which is where it did the
//! damage; this module is where its constant now lives.
//!
//! THE BOUNDARY IS EXCLUSIVE: a stamp exactly ON the cutoff is KEPT. All three
//! age-based families already agreed on this — it is the same boundary as
//! `prune_stale`'s `age > max_age`, expressed the other way round — and the
//! duplication is why nothing could check that they do. [`Cutoff::excludes`] is
//! that check now, and the truth table at the bottom of this file is the only
//! place the rule is asserted.

/// The smallest window this device keeps, whatever the config says.
pub(crate) const MIN_RETENTION_DAYS: u64 = 1;

/// The largest window that means anything. `days` arrives from config and from `PUT /api/settings`
/// with no upper bound, and an unbounded multiply is not merely large — see the module doc.
pub(crate) const MAX_RETENTION_DAYS: u64 = 36_500;

/// Milliseconds in a day. THE conversion, so no call site spells it out again.
pub(crate) const DAY_MS: u64 = 86_400_000;

/// Seconds in a day — the same conversion for the two families whose clock
/// (and whose stored stamps) are in seconds.
pub(crate) const DAY_SECS: u64 = 86_400;

/// ONE WINDOW, RESOLVED.
///
/// Deliberately opaque: callers read the boundary through [`Cutoff::excludes`]
/// (the ms-clock families), [`Cutoff::cutoff_ms`] and [`Cutoff::cutoff_secs`]
/// (the s-clock families), so none of them can re-derive the arithmetic this
/// module exists to own. Deriving it differently is exactly the drift the four
/// former copies demonstrated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cutoff {
    /// The resolved boundary, in ms.
    cutoff_ms: u64,
}

impl Cutoff {
    /// `days` floored at [`MIN_RETENTION_DAYS`] and capped at [`MAX_RETENTION_DAYS`], applied to
    /// `now_ms`. Total: the multiply cannot wrap and the subtraction cannot go below zero.
    pub(crate) fn days_before(days: u64, now_ms: u64) -> Cutoff {
        let days = days.clamp(MIN_RETENTION_DAYS, MAX_RETENTION_DAYS);
        Cutoff {
            cutoff_ms: now_ms.saturating_sub(days.saturating_mul(DAY_MS)),
        }
    }

    /// IS THIS STAMP OLDER THAN THE WINDOW? THE BOUNDARY IS EXCLUSIVE — a stamp exactly ON the
    /// cutoff is KEPT. All three age-based families already agree on this; the duplication is why
    /// nothing could check that they do.
    pub(crate) fn excludes(&self, stamp_ms: u64) -> bool {
        stamp_ms < self.cutoff_ms()
    }

    pub(crate) fn cutoff_ms(&self) -> u64 {
        self.cutoff_ms
    }

    /// The same window for the two families whose clock is in SECONDS.
    ///
    /// Floored at second resolution, so the boundary can only move AWAY from now
    /// — a reader that reconstructs a window from it can never be handed a
    /// cutoff a millisecond inside the one the ms families would keep.
    pub(crate) fn cutoff_secs(&self) -> u64 {
        self.cutoff_ms() / 1000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed "now" for the whole table, so every row is exact rather than
    /// drifting with the wall clock (the same discipline the families' own
    /// seeded tests follow).
    const NOW: u64 = 1_800_000_000_000;

    /// THE TRUTH TABLE — the ONLY place the window rule is asserted.
    ///
    /// The families' own boundary tests still exist, but they are IO tests now:
    /// they prove the family consults this boundary with ITS clock, and they
    /// point here for the rule. That split is the point of the module — before
    /// it, each family re-asserted the arithmetic and one of them asserted a
    /// boundary no code shared.
    #[test]
    fn cutoff_truth_table() {
        // THE FLOOR: 0 days is lifted to the minimum, not honoured. A window of
        // zero would delete everything not stamped this very millisecond —
        // including the screenshot an action just wrote.
        let floored = Cutoff::days_before(0, NOW);
        assert_eq!(
            floored.cutoff_ms(),
            NOW - DAY_MS,
            "a zero-day window must resolve to the 1-day floor"
        );
        assert_eq!(
            Cutoff::days_before(MIN_RETENTION_DAYS, NOW),
            floored,
            "the floor IS the minimum window — same rule at its own value"
        );

        // THE CAP: 10 million days would be ~27,000 years; it resolves to the
        // cap's own boundary, never to the value asked for. With any `now` this
        // device will see before the 2060s that boundary is 0 — a window longer
        // than the epoch SATURATES (the last row below) rather than wrapping to
        // NOW, which is the failure the cap exists to make impossible.
        let capped = Cutoff::days_before(10_000_000, NOW);
        assert_eq!(
            capped,
            Cutoff::days_before(MAX_RETENTION_DAYS, NOW),
            "10 million days must resolve to the cap, not to a value of its own"
        );
        assert_ne!(
            capped.cutoff_ms(),
            NOW,
            "the WRAP made the cutoff NOW — a setting that meant \"keep \
             everything\" retired a whole store that way, and the cap exists so \
             it cannot happen again"
        );
        assert!(
            !capped.excludes(NOW - DAY_MS),
            "and the safe direction must survive the absurd value: yesterday's \
             record is KEPT under a 10-million-day window"
        );
        assert_eq!(
            Cutoff::days_before(u64::MAX, NOW),
            capped,
            "the cap is TOTAL: `u64::MAX` days must resolve like every other \
             value above it — 2^57 days is exactly 0 mod 2^64 in an \
             unsaturating multiply, which is the incident the cap records"
        );
        // At a `now` far enough out that a century FITS, the cap's arithmetic is
        // exact — the boundary is the cap, not the 10 million days requested.
        let far = MAX_RETENTION_DAYS * DAY_MS + 5;
        assert_eq!(
            Cutoff::days_before(10_000_000, far).cutoff_ms(),
            5,
            "the capped window is exactly MAX_RETENTION_DAYS days"
        );

        // THE BOUNDARY IS EXCLUSIVE. One ms is the smallest unit any family's
        // clock here resolves, so "exactly on the cutoff" is exactly testable.
        let c = Cutoff::days_before(30, NOW);
        assert!(
            !c.excludes(c.cutoff_ms()),
            "a stamp exactly ON the cutoff is KEPT — the boundary is exclusive"
        );
        assert!(
            c.excludes(c.cutoff_ms() - 1),
            "one ms older than the cutoff is excluded"
        );
        assert!(
            !c.excludes(c.cutoff_ms() + 1),
            "one ms newer than the cutoff is kept"
        );

        // SATURATION, not wrap: a window longer than the epoch the caller was
        // measured from leaves the cutoff at 0, which excludes NOTHING.
        let saturated = Cutoff::days_before(30, 5);
        assert_eq!(
            saturated.cutoff_ms(),
            0,
            "now_ms smaller than the window must saturate at 0, never wrap"
        );
        assert!(
            !saturated.excludes(0),
            "a saturated cutoff excludes nothing — the safe direction"
        );
    }

    /// The seconds form is the SAME window: the conversion must not shift the
    /// boundary by a day (a `/ 86_400` applied to a ms value, or the reverse).
    #[test]
    fn cutoff_secs_agrees_with_cutoff_ms() {
        let c = Cutoff::days_before(30, NOW);
        assert_eq!(
            c.cutoff_secs(),
            c.cutoff_ms() / 1000,
            "cutoff_secs must be the same instant as cutoff_ms"
        );
        assert_eq!(
            c.cutoff_secs(),
            NOW / 1000 - 30 * DAY_SECS,
            "and it must be the window in SECONDS, not in millis or in days"
        );
        assert_eq!(
            DAY_MS,
            DAY_SECS * 1000,
            "the two conversions must describe the same day"
        );
    }
}
