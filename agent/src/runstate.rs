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
//! THE PURE HALF IS SEPARATED ON PURPOSE. `classify` decides WHAT HAPPENED and `describe_previous`
//! turns that decision into the one line an operator reads; both are functions of
//! `(previous, now, host boot time)` with no file and no clock of their own, so the wording —
//! which is the part that has to be RIGHT when somebody is debugging at 3am — and the
//! machine-readable verdict are tested directly, and tested against EACH OTHER.

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

/// How recently the previous run must have beaten for its death to be read as a REPLACEMENT
/// rather than a crash.
///
/// MEASURED, not chosen: on d1 (round 244) the 1.2.364 -> 1.2.365 update killed the agent tree
/// and the new process began **6 s** after the old one's last heartbeat — the swap kills the
/// process and restarts the task, so `since` lands at a few seconds. A crash is different in
/// kind: nothing takes over until the 60 s watchdog notices, so `since` is at least a minute and
/// usually much more. One minute sits between the two by a wide margin on both sides.
const REPLACED_WITHIN_SECS: u64 = 60;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Unix seconds when the HOST booted, or `None` when this build cannot ask.
///
/// WHY THE VERDICT NEEDS IT. Without it, "the run stopped heartbeating and nothing took over
/// within a minute" has two causes that look identical and mean opposite things: the AGENT died
/// (a fault) or the MACHINE went down with it (a reboot or a power cut — routine, and the run
/// had no chance to say goodbye). Every device that reboots would otherwise report a crash on
/// every start, and a warning that fires on routine events is one an operator learns to ignore —
/// which would cost the surface the only case it exists for.
///
/// `None` is not "no reboot": it is "cannot tell", and the classifier stays with the conservative
/// answer (a crash) rather than inventing a benign explanation. Windows has `GetTickCount64`
/// (boot-relative milliseconds, no privileges); every other host returns `None` — the shipped
/// agent is the Windows one, and a host that cannot answer must not guess.
#[cfg(windows)]
fn machine_boot_secs() -> Option<u64> {
    let uptime_ms = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    Some(now_secs().saturating_sub(uptime_ms / 1000))
}

#[cfg(not(windows))]
fn machine_boot_secs() -> Option<u64> {
    None
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

/// WHICH of the five things happened to the previous run — the machine-readable half of the line
/// [`describe_previous`] writes.
///
/// WHY IT IS A TYPE AND NOT A SUBSTRING. The prose line exists for a human at 3am; the panel and
/// the console have to DECIDE something with it (shout, or stay quiet), and a consumer that
/// decides by matching English is a consumer that breaks the next time the wording is improved.
/// Round 254 taught `describe_previous` to tell a REPLACEMENT from a CRASH; round 256 makes that
/// answer travel as data, so no reader has to re-derive it — and the two can never disagree,
/// because both come out of [`classify`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootKind {
    /// No previous run on record.
    FirstRun,
    /// The previous run wrote its clean-exit marker.
    CleanExit,
    /// It died without a marker, but something took over at once — an update swap, or a task
    /// restart. Normal, and the reason this distinction exists at all.
    Replaced,
    /// It died without a marker because THE HOST went down with it: the machine's own boot time
    /// is later than the run's last heartbeat, so a reboot (or a power cut) ended it, not a fault.
    MachineRestart,
    /// It died without a marker and nothing took over until the watchdog noticed.
    Crashed,
}

impl BootKind {
    /// The wire spelling: `/api/status` carries it as `last_boot_kind` and the panel and the
    /// console branch on it, so this is a contract rather than a debug string — lowercase and
    /// hyphenated like every other enum this API spells out.
    pub fn as_str(self) -> &'static str {
        match self {
            BootKind::FirstRun => "first-run",
            BootKind::CleanExit => "clean-exit",
            BootKind::Replaced => "replaced",
            BootKind::MachineRestart => "machine-restart",
            BootKind::Crashed => "crashed",
        }
    }

    /// The inverse of [`as_str`](Self::as_str) — `None` for anything this build does not know,
    /// which a reader must treat as "no machine answer on record" rather than as a crash.
    pub fn parse(s: &str) -> Option<BootKind> {
        match s.trim() {
            "first-run" => Some(BootKind::FirstRun),
            "clean-exit" => Some(BootKind::CleanExit),
            "replaced" => Some(BootKind::Replaced),
            "machine-restart" => Some(BootKind::MachineRestart),
            "crashed" => Some(BootKind::Crashed),
            _ => None,
        }
    }
}

/// WHICH of the five happened — the ONE place the discriminator is applied. `describe_previous`
/// phrases this answer; `/api/status` ships it as data; nothing re-derives it.
///
/// `boot_secs` is the host's boot time ([`machine_boot_secs`]); the ORDER of the middle two
/// checks is the point, not an accident. A reboot that took less than a minute looks exactly
/// like an update swap by heartbeat freshness alone, so the host's own boot time is asked FIRST:
/// it is positive evidence about what happened, where `since` is only an inference from what did
/// not happen.
pub fn classify(previous: Option<&RunState>, now: u64, boot_secs: Option<u64>) -> BootKind {
    match previous {
        None => BootKind::FirstRun,
        Some(prev) if prev.exited => BootKind::CleanExit,
        Some(prev) => {
            let since = now.saturating_sub(prev.last);
            if boot_secs.is_some_and(|boot| boot > prev.last) {
                BootKind::MachineRestart
            } else if since <= REPLACED_WITHIN_SECS {
                // A fresh heartbeat means something replaced the process; a stale one means it
                // died and nothing took over. See REPLACED_WITHIN_SECS for why a minute sits
                // between the two.
                BootKind::Replaced
            } else {
                BootKind::Crashed
            }
        }
    }
}

/// THE LINE AN OPERATOR READS. `None` means there is no previous run (a first start, or a
/// journal that was cleared) — which is a fact worth stating too, because "no previous run"
/// and "a previous run that left no trace" are different situations and the second one is
/// the one that used to be invisible.
///
/// The wording is branched on [`classify`]'s answer, so the sentence and the kind can never
/// disagree; `boot_secs` is the host's boot time, for the reboot case.
pub fn describe_previous(previous: Option<&RunState>, now: u64, boot_secs: Option<u64>) -> String {
    let kind = classify(previous, now, boot_secs);
    let Some(prev) = previous else {
        return "run journal: no previous run on record (first start, or the journal was cleared)"
            .into();
    };
    let uptime = prev.last.saturating_sub(prev.started);
    let since = now.saturating_sub(prev.last);
    if kind == BootKind::CleanExit {
        return format!(
            "run journal: previous run started {since_start}s ago, ran {uptime}s, exited cleanly",
            since_start = now.saturating_sub(prev.started),
        );
    }
    // NO CLEAN-EXIT MARKER: the process died without getting a chance to say goodbye —
    // a panic, a kill, a power loss, or the update swap replacing it mid-flight. The
    // uptime is what separates "died immediately at boot" from "ran for hours", and it
    // is exactly what nobody could measure before this module existed.
    // THE LINE USED TO HAND THE OPERATOR THREE NUMBERS AND NO READING OF THEM. Round 244
    // read this on d1 and had to work out by hand that "DID NOT EXIT CLEANLY ... last
    // heartbeat 6s before this start" is a NORMAL update swap, not a fault — the journal
    // for that round records the reasoning ("the line reads alarming and is not"). The
    // discriminator is `since`, so the function applies it; round 256 added the third case,
    // because "nothing took over for a minute" is also what a REBOOT looks like and calling
    // that a crash would teach an operator to ignore the one line that matters. The
    // "DID NOT EXIT CLEANLY" marker is preserved in every branch — a reader that keys on it
    // (a log grep, a test, a person) keeps working.
    let verdict = match kind {
        BootKind::Replaced => {
            "REPLACED by a restart (an update swap or a task restart killed it mid-flight)"
        }
        BootKind::MachineRestart => {
            "the MACHINE RESTARTED under it (the host went down before it could beat again — a \
             reboot or a power cut, not an agent fault)"
        }
        _ => "CRASHED or was killed (it stopped heartbeating and nothing took over)",
    };
    format!(
        "run journal: previous run DID NOT EXIT CLEANLY — {verdict}; started {since_start}s \
         ago, last heartbeat {since}s before this start, survived {uptime}s",
        since_start = now.saturating_sub(prev.started),
    )
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

/// Where the last boot's verdict is kept so the HTTP surface can report it.
///
/// `begin` computes the one line an operator reads at the next boot and hands it to the binary's
/// logger — which is where round 244 found it on d1, in `logs\startup.log`, on the device. **A
/// field engineer reading a log is not the same audience as the panel or any API client**, so the
/// same line is persisted here and served by `/api/status`. Kept BESIDE the run journal rather
/// than inside it: `run-state.txt`'s three fields are a contract with every installed build, and
/// this needs no part of it.
pub fn verdict_path(data_dir: &Path) -> PathBuf {
    data_dir.join("logs").join("last-boot.txt")
}

/// The persisted verdict: the prose line, plus the [`BootKind`] when the build that wrote the file
/// recorded one.
///
/// `kind` is an `Option` for one measured reason: 1.2.366 shipped prose and no kind, so a device
/// that has not rebooted since carries a file this reader must still be able to use — and the
/// honest answer for that file is "the line is here, the machine answer is not", never a guess
/// made by matching English in a UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastBoot {
    pub kind: Option<BootKind>,
    pub detail: String,
}

/// Render the verdict file. The kind is a `kind=` line FIRST so the prose stays readable on its own
/// (a human who opens the file sees the same sentence as before, one line down) and so a reader
/// never has to parse a sentence to find it.
pub fn render_verdict(kind: BootKind, line: &str) -> String {
    format!("kind={}\n{}\n", kind.as_str(), line.trim())
}

/// Read a verdict file. `None` for a missing, empty or whitespace-only file — a reader must never
/// render a blank warning. An UNKNOWN `kind=` value (a newer build's spelling) degrades to the
/// whole text as detail with no kind, rather than to a wrong answer.
pub fn parse_verdict(text: &str) -> Option<LastBoot> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let first = t.lines().next().unwrap_or("").trim();
    if let Some(raw) = first.strip_prefix("kind=") {
        // The prose is everything under the first line — absent when the file is ONLY a kind,
        // which is not a verdict (there is nothing to show a reader) and reads as no verdict.
        let rest = t.split_once('\n').map(|(_, r)| r.trim()).unwrap_or("");
        return match BootKind::parse(raw) {
            Some(kind) if !rest.is_empty() => Some(LastBoot {
                kind: Some(kind),
                detail: rest.to_string(),
            }),
            Some(_) => None,
            // A spelling this build does not know (a newer one): hand back the prose exactly as
            // written and claim no kind, rather than guessing what the word meant.
            None => Some(LastBoot {
                kind: None,
                detail: t.to_string(),
            }),
        };
    }
    Some(LastBoot {
        kind: None,
        detail: t.to_string(),
    })
}

/// Persist the boot verdict, best-effort — a journal that cannot be written must not take the
/// agent down with it, the same rule `save` follows.
pub fn save_verdict(data_dir: &Path, kind: BootKind, line: &str) {
    let path = verdict_path(data_dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, render_verdict(kind, line));
}

/// The last boot's verdict, or `None` when this install has never booted a build that wrote one.
/// Never fails: a missing or unreadable file is `None`, never an error and never an empty string.
pub fn last_boot(data_dir: &Path) -> Option<LastBoot> {
    parse_verdict(&std::fs::read_to_string(verdict_path(data_dir)).ok()?)
}

/// Begin a run: describe what the last one did, then claim the journal for this one.
///
/// The order matters: the previous state must be READ before it is overwritten, and the
/// description is returned rather than logged so the binary's own `log_line` (which knows
/// where stdout-less output belongs) stays the only writer.
pub fn begin(data_dir: &Path) -> (Option<RunState>, String) {
    let previous = load(data_dir);
    let now = now_secs();
    // The host's boot time is read HERE, once, and handed to both halves of the verdict — the
    // sentence logged to `startup.log` and the kind persisted for `/api/status`. Reading it in
    // two places is how the reboot case would come to be classified one way and described
    // another.
    let boot = machine_boot_secs();
    let kind = classify(previous.as_ref(), now, boot);
    let line = describe_previous(previous.as_ref(), now, boot);
    // Also persist it for the HTTP surface — see `verdict_path`. The KIND rides along, computed
    // here rather than re-derived by whoever reads the file.
    save_verdict(data_dir, kind, &line);
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
        assert!(describe_previous(None, 1_000, None).contains("no previous run"));
    }

    #[test]
    fn a_clean_exit_is_reported_as_one() {
        let prev = RunState {
            started: 100,
            last: 400,
            exited: true,
        };
        let line = describe_previous(Some(&prev), 500, None);
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
        let line = describe_previous(Some(&prev), 9_000, None);
        assert!(line.contains("DID NOT EXIT CLEANLY"), "{line}");
        assert!(line.contains("survived 61s"), "{line}");
        assert!(
            line.contains("last heartbeat 7939s before this start"),
            "{line}"
        );
    }

    #[test]
    fn a_swap_is_called_a_replacement_and_a_gap_is_called_a_crash() {
        // THE REAL NUMBERS FROM d1, round 244: the 1.2.364 -> 1.2.365 update left
        // "started 31991s ago, last heartbeat 6s before this start, survived 31985s", and the
        // journal for that round records the operator's problem with it — the line "reads
        // alarming and is not". These two cases are the ones the old text could not tell apart.
        let swapped = RunState {
            started: 100_000,
            last: 131_985, // heartbeating until 6 s before the new process began
            exited: false,
        };
        let line = describe_previous(Some(&swapped), 131_991, None);
        assert!(line.contains("DID NOT EXIT CLEANLY"), "{line}");
        assert!(line.contains("REPLACED by a restart"), "{line}");
        assert!(!line.contains("CRASHED"), "{line}");

        // Same shape, but nothing took over for an hour: a crash, and the uptime is what says
        // so — the case the module was written for.
        let crashed = RunState {
            started: 100_000,
            last: 131_985,
            exited: false,
        };
        let line = describe_previous(Some(&crashed), 135_585, None);
        assert!(line.contains("DID NOT EXIT CLEANLY"), "{line}");
        assert!(line.contains("CRASHED or was killed"), "{line}");
        assert!(!line.contains("REPLACED"), "{line}");
    }

    #[test]
    fn the_prose_and_the_kind_come_from_the_same_rule() {
        // THE AGREEMENT TEST. The two halves of a verdict — the sentence a human reads and the
        // enum a UI branches on — are produced by `describe_previous` and `classify` separately,
        // so nothing but this test stops them from disagreeing. Each case pins BOTH: the kind, and
        // the phrase that kind's sentence must contain. A future edit that reclassifies a case
        // without rephrasing it (or the reverse) fails here instead of shipping a UI that says
        // "crashed" over a line that says "replaced".
        let swapped = RunState {
            started: 100_000,
            last: 131_985, // 6 s before the new process began — d1's real update numbers
            exited: false,
        };
        let crashed = RunState {
            started: 100_000,
            last: 131_985,
            exited: false,
        };
        let clean = RunState {
            started: 100,
            last: 400,
            exited: true,
        };
        // A run that ended WITH the host: the machine booted after its last heartbeat.
        let rebooted = RunState {
            started: 100_000,
            last: 131_985,
            exited: false,
        };
        // A named case rather than a five-field tuple array: the tuple type was the only thing
        // clippy had to say about this test, and the case IS the concept being tested.
        struct Case<'a> {
            prev: Option<&'a RunState>,
            now: u64,
            boot: Option<u64>,
            kind: BootKind,
            phrase: &'a str,
        }
        let cases = [
            Case {
                prev: None,
                now: 1_000,
                boot: None,
                kind: BootKind::FirstRun,
                phrase: "no previous run",
            },
            Case {
                prev: Some(&clean),
                now: 500,
                boot: None,
                kind: BootKind::CleanExit,
                phrase: "exited cleanly",
            },
            Case {
                prev: Some(&swapped),
                now: 131_991,
                boot: Some(90_000),
                kind: BootKind::Replaced,
                phrase: "REPLACED",
            },
            Case {
                prev: Some(&rebooted),
                now: 135_585,
                boot: Some(135_000),
                kind: BootKind::MachineRestart,
                phrase: "MACHINE RESTARTED",
            },
            Case {
                prev: Some(&crashed),
                now: 135_585,
                boot: Some(90_000),
                kind: BootKind::Crashed,
                phrase: "CRASHED",
            },
        ];
        for Case {
            prev,
            now,
            boot,
            kind,
            phrase,
        } in cases
        {
            assert_eq!(
                classify(prev, now, boot),
                kind,
                "classify({prev:?}, {now}, {boot:?})"
            );
            let line = describe_previous(prev, now, boot);
            assert!(
                line.contains(phrase),
                "the {kind:?} sentence must contain {phrase:?}: {line}",
            );
            // Every "did not exit cleanly" verdict keeps round 254's marker, so a reader that
            // greps for it (a log search, a field engineer, another test) keeps its handle. The
            // clean exit is the one case that never had it.
            if !matches!(kind, BootKind::CleanExit | BootKind::FirstRun) {
                assert!(line.contains("DID NOT EXIT CLEANLY"), "{line}");
            }
        }
    }

    #[test]
    fn a_reboot_is_not_reported_as_a_crash() {
        // THE FALSE ALARM THIS CASE EXISTS TO PREVENT. Every device reboots eventually, and a
        // reboot looks EXACTLY like a crash to a heartbeat check: the run stopped beating and
        // nothing took over for minutes. What separates them is positive evidence about the host —
        // the machine's own boot time is later than the run's last heartbeat, so the run ended
        // WITH the host. Without it, every routine restart would raise the panel's crash chip, and
        // a warning that fires on routine events is how an operator learns to ignore the one that
        // matters.
        let prev = RunState {
            started: 100_000,
            last: 131_985, // the last beat before the machine went down
            exited: false,
        };
        let now = 135_585; // the agent came back 3600 s later, at boot
        assert_eq!(
            classify(Some(&prev), now, Some(135_000)),
            BootKind::MachineRestart,
        );
        let line = describe_previous(Some(&prev), now, Some(135_000));
        assert!(line.contains("MACHINE RESTARTED"), "{line}");
        assert!(!line.contains("CRASHED"), "{line}");

        // THE SAME NUMBERS WITH THE HOST UP THE WHOLE TIME: the run died on its own, which is the
        // crash this module was written for. The pair is what makes the reboot case a
        // discrimination rather than an excuse.
        assert_eq!(classify(Some(&prev), now, Some(90_000)), BootKind::Crashed);

        // AND WHEN THE HOST CANNOT BE ASKED (`None` — a non-Windows build), the answer stays the
        // conservative one: "I cannot tell" must never manufacture a benign explanation.
        assert_eq!(classify(Some(&prev), now, None), BootKind::Crashed);

        // Positive evidence OUTRANKS the freshness heuristic: a machine that rebooted and came
        // back in six seconds reads as a reboot, not as the update swap those numbers otherwise
        // look exactly like.
        assert_eq!(
            classify(Some(&prev), 131_991, Some(131_990)),
            BootKind::MachineRestart,
        );
    }

    #[test]
    fn a_verdict_file_carries_its_kind_and_older_files_still_read() {
        // The wire spelling is a contract, so it is pinned rather than assumed.
        assert_eq!(BootKind::Crashed.as_str(), "crashed");
        assert_eq!(BootKind::parse("replaced"), Some(BootKind::Replaced));
        assert_eq!(BootKind::parse("melted"), None);

        let text = render_verdict(BootKind::Crashed, "run journal: CRASHED or was killed");
        let back = parse_verdict(&text).expect("parsed");
        assert_eq!(back.kind, Some(BootKind::Crashed));
        assert_eq!(back.detail, "run journal: CRASHED or was killed");

        // THE FILE 1.2.366 ALREADY WROTE ON A DEVICE: prose only, no kind line. It must still be
        // readable — with an honest `None` rather than a guess made by matching English.
        let legacy = "run journal: previous run DID NOT EXIT CLEANLY — REPLACED by a restart\n";
        let back = parse_verdict(legacy).expect("legacy file parses");
        assert_eq!(back.kind, None, "a file with no kind must not invent one");
        assert!(back.detail.contains("REPLACED by a restart"), "{back:?}");

        // An unknown kind from a NEWER build degrades to prose, never to a wrong answer.
        let future = "kind=melted\nrun journal: something new\n";
        let back = parse_verdict(future).expect("parsed");
        assert_eq!(back.kind, None);
        assert!(back.detail.contains("something new"), "{back:?}");

        // Blank is absence, not a verdict.
        assert_eq!(parse_verdict("   \n  "), None);
        assert_eq!(parse_verdict("kind=crashed\n\n"), None);
    }

    #[test]
    fn the_boot_verdict_survives_for_the_http_surface_to_read_back() {
        // The product half of round 254's change: the line was computed and logged, and the only
        // way to see it was to read logs/startup.log on the device. It is now readable too — and
        // since round 256 it carries the KIND, so the surfaces that act on it need no prose.
        let dir = std::env::temp_dir().join(format!("vale-verdict-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(last_boot(&dir), None, "no verdict before any boot");

        let (_, line) = begin(&dir);
        let boot = last_boot(&dir).expect("a verdict after the first boot");
        assert_eq!(
            boot.kind,
            Some(BootKind::FirstRun),
            "a fresh install's first boot"
        );
        assert_eq!(
            boot.detail, line,
            "the verdict begin() returns is the one a reader gets back"
        );

        // A second boot inside the journal's own lifetime: still nothing exited cleanly, and the
        // new run started seconds later — the update-swap case, which is `replaced`.
        let (_, line) = begin(&dir);
        let boot = last_boot(&dir).expect("a verdict");
        assert_eq!(boot.kind, Some(BootKind::Replaced));
        assert_eq!(boot.detail, line);

        // An empty file is `None`, not `Some("")` — a reader must not render a blank warning.
        save_verdict(&dir, BootKind::Crashed, "   \n  ");
        assert_eq!(last_boot(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
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
