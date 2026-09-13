//! Run journal — what the PREVIOUS run of the agent did, and how it ended.
//!
//! WHY THIS EXISTS. On 2026-09-13 the agent on d1 died and left NOTHING behind: no exit
//! line, no panic text, not even a gap an operator could measure. `startup.log` (the boot
//! journal) only gains a block when the NEXT run starts, so between two runs the file
//! simply ends — and a Rust panic writes to stderr, which a boot-task process does not
//! have. "The agent restarted every one to two hours" was therefore a pattern nobody could
//! explain, and an investigation of it could not even begin. That is the whole defect this
//! module closes: **a process that can die silently makes every other diagnosis guesswork**.
//!
//! IT IS THREE FACTS, not a logging framework: when a run started, when it was last seen
//! alive, and whether it exited on purpose. A heartbeat makes the middle one meaningful —
//! without it, "started at 22:55" says nothing about whether the process lived one second
//! or nine hours.
//!
//! THE PURE HALF IS SEPARATED ON PURPOSE. `describe_previous` turns the stored state into
//! the one line an operator reads, and it is a function of `(previous, now)` with no file
//! and no clock of its own, so the wording — which is the part that has to be RIGHT when
//! somebody is debugging at 3am — is tested directly.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// One run's footprint. `last` is refreshed by the heartbeat, so a process that was killed
/// mid-flight is distinguishable from one that exited on purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunState {
    /// Unix seconds when this run started.
    pub started: u64,
    /// Unix seconds of the most recent heartbeat.
    pub last: u64,
    /// True once the run wrote its clean-exit marker.
    pub exited: bool,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Parse the stored state. Anything unreadable is `None` rather than an error: a corrupt
/// journal must never stop the agent from starting.
pub fn parse(text: &str) -> Option<RunState> {
    let mut started = None;
    let mut last = None;
    let mut exited = false;
    for line in text.lines() {
        let (k, v) = line.split_once('=')?;
        match k.trim() {
            "started" => started = v.trim().parse::<u64>().ok(),
            "last" => last = v.trim().parse::<u64>().ok(),
            "exited" => exited = v.trim() == "1",
            _ => {}
        }
    }
    Some(RunState {
        started: started?,
        last: last.or(started)?,
        exited,
    })
}

/// Render the state in the format [`parse`] reads.
pub fn render(state: &RunState) -> String {
    format!(
        "started={}\nlast={}\nexited={}\n",
        state.started,
        state.last,
        if state.exited { 1 } else { 0 }
    )
}

/// THE LINE AN OPERATOR READS. `None` means there is no previous run (a first start, or a
/// journal that was cleared) — which is a fact worth stating too, because "no previous run"
/// and "a previous run that left no trace" are different situations and the second one is
/// the one that used to be invisible.
pub fn describe_previous(previous: Option<&RunState>, now: u64) -> String {
    let Some(prev) = previous else {
        return "run journal: no previous run on record (first start, or the journal was cleared)"
            .into();
    };
    let uptime = prev.last.saturating_sub(prev.started);
    let since = now.saturating_sub(prev.last);
    if prev.exited {
        format!(
            "run journal: previous run started {since_start}s ago, ran {uptime}s, exited cleanly",
            since_start = now.saturating_sub(prev.started),
        )
    } else {
        // NO CLEAN-EXIT MARKER: the process died without getting a chance to say goodbye —
        // a panic, a kill, a power loss, or the update swap replacing it mid-flight. The
        // uptime is what separates "died immediately at boot" from "ran for hours", and it
        // is exactly what nobody could measure before this module existed.
        format!(
            "run journal: previous run DID NOT EXIT CLEANLY — started {since_start}s ago, \
             last heartbeat {since}s before this start, survived {uptime}s",
            since_start = now.saturating_sub(prev.started),
        )
    }
}

/// Where the journal lives (under DataDir, beside the logs).
pub fn state_path(data_dir: &Path) -> PathBuf {
    data_dir.join("logs").join("run-state.txt")
}

/// Read the previous run's state. Never fails: a missing or corrupt journal is `None`.
pub fn load(data_dir: &Path) -> Option<RunState> {
    let text = std::fs::read_to_string(state_path(data_dir)).ok()?;
    parse(&text)
}

/// Write the state, best-effort. A journal that cannot be written must not take the agent
/// down with it — the same rule the audit trail follows.
pub fn save(data_dir: &Path, state: &RunState) {
    let path = state_path(data_dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, render(state));
}

/// Begin a run: describe what the last one did, then claim the journal for this one.
///
/// The order matters: the previous state must be READ before it is overwritten, and the
/// description is returned rather than logged so the binary's own `log_line` (which knows
/// where stdout-less output belongs) stays the only writer.
pub fn begin(data_dir: &Path) -> (Option<RunState>, String) {
    let previous = load(data_dir);
    let now = now_secs();
    let line = describe_previous(previous.as_ref(), now);
    save(
        data_dir,
        &RunState {
            started: now,
            last: now,
            exited: false,
        },
    );
    (previous, line)
}

/// Refresh the heartbeat. Called on a timer while the run is alive.
pub fn beat(data_dir: &Path, started: u64) {
    save(
        data_dir,
        &RunState {
            started,
            last: now_secs(),
            exited: false,
        },
    );
}

/// Record a clean exit, so the NEXT start can say so instead of reporting a mystery.
pub fn mark_exited(data_dir: &Path, started: u64) {
    save(
        data_dir,
        &RunState {
            started,
            last: now_secs(),
            exited: true,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("vale-runstate-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

    #[test]
    fn roundtrip_is_stable() {
        let s = RunState {
            started: 100,
            last: 250,
            exited: true,
        };
        assert_eq!(parse(&render(&s)), Some(s));
    }

    #[test]
    fn a_missing_last_falls_back_to_started() {
        // An older journal, or one written between the start and the first heartbeat.
        let s = parse("started=100\nexited=0\n").expect("parsed");
        assert_eq!(s.last, 100);
    }

    #[test]
    fn garbage_is_none_rather_than_an_error() {
        // A corrupt journal must never stop the agent from starting.
        assert_eq!(parse("not a journal"), None);
        assert_eq!(parse("started=abc\n"), None);
        assert!(load(&dir("missing")).is_none());
    }

    #[test]
    fn no_previous_run_says_so() {
        // "Nothing on record" and "a run that left no trace" are different situations.
        assert!(describe_previous(None, 1_000).contains("no previous run"));
    }

    #[test]
    fn a_clean_exit_is_reported_as_one() {
        let prev = RunState {
            started: 100,
            last: 400,
            exited: true,
        };
        let line = describe_previous(Some(&prev), 500);
        assert!(line.contains("exited cleanly"), "{line}");
        assert!(line.contains("ran 300s"), "{line}");
    }

    #[test]
    fn a_run_that_left_no_exit_marker_is_called_out_with_its_uptime() {
        // THE CASE THIS MODULE EXISTS FOR. Uptime is what separates "died at boot" from
        // "ran for hours", and it is what nobody could measure before.
        let prev = RunState {
            started: 1_000,
            last: 1_061,
            exited: false,
        };
        let line = describe_previous(Some(&prev), 9_000);
        assert!(line.contains("DID NOT EXIT CLEANLY"), "{line}");
        assert!(line.contains("survived 61s"), "{line}");
        assert!(
            line.contains("last heartbeat 7939s before this start"),
            "{line}"
        );
    }

    #[test]
    fn begin_reports_then_claims_the_journal() {
        let d = dir("begin");
        // First start: nothing on record.
        let (prev, line) = begin(&d);
        assert!(prev.is_none());
        assert!(line.contains("no previous run"), "{line}");
        // A second start sees the first, and does not call it clean.
        let (prev2, line2) = begin(&d);
        assert!(prev2.is_some());
        assert!(line2.contains("DID NOT EXIT CLEANLY"), "{line2}");
    }

    #[test]
    fn a_clean_exit_is_visible_to_the_next_start() {
        let d = dir("clean");
        let (_, _) = begin(&d);
        let started = load(&d).expect("journal written").started;
        mark_exited(&d, started);
        let (_, line) = begin(&d);
        assert!(line.contains("exited cleanly"), "{line}");
    }

    #[test]
    fn the_heartbeat_moves_last_without_touching_started() {
        let d = dir("beat");
        let (_, _) = begin(&d);
        let started = load(&d).expect("journal").started;
        beat(&d, started);
        let s = load(&d).expect("journal");
        assert_eq!(s.started, started);
        assert!(!s.exited);
    }
}
