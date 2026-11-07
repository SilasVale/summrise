//! What machine is this: the facts an operator needs before a device's report means anything.
//!
//! The console shows a device as "online, version N", which answers "is the app running" and not
//! "what am I looking at". The facts here are the other half: the operating system and uptime, the
//! memory and disk the device is sharing, and the sessions — including the one the helper runs in,
//! because "the desktop cannot be captured" is a property of a session, not of a device.
//!
//! Everything is best-effort. A fact that cannot be read is reported as zero, an empty list or null
//! rather than failing the call: a device that answers with five of six facts is more useful than one
//! that refuses to say anything.
//!
//! The `update` block is the launcher's records — its three baseline generations, its last swap and
//! its repairs — read for display. Nothing here verifies a signature or replays a swap: the launcher
//! wrote these files, and the facts report what it wrote.
//!
//! The frame's top-level `update` section is the device's own story instead: the version this process
//! runs, the verdict it reached at its last start, and the updater's last answer, read from
//! `<data>/state.json`. It is separate from [`facts`]' `update` block on purpose — that one is the
//! launcher's files, this one is the state document the running app writes — and a device whose
//! state is unreadable omits the section rather than sending a story it does not have.

use std::io::{BufRead, BufReader};
use std::path::Path;

use summrise_core::{State, UpdateOutcome, VerdictOutcome};

use crate::update::{swap_result_path, SwapOutcome, SwapResult};

/// The launcher's baseline layout, relative to the install root. The writer's spelling of these
/// names is `device/launch/src/paths.rs`.
const BASELINE_DIR: &str = "baseline";
const BASELINE_SIDECAR: &str = "baseline.json";

/// How many of the launcher's repairs the facts keep. A repair is rare; five is enough to show a
/// device that keeps coming back, and it bounds what a long log costs the facts.
const RECENT_REPAIRS: usize = 5;

/// The machine facts, as the wire sees them.
pub fn facts(data: &std::path::Path) -> serde_json::Value {
    let mut document = platform_facts();
    // This app's own boots, newest first: the half of "why did it restart" the launcher does not
    // keep. An unreadable journal contributes an empty list rather than failing the facts.
    document["restarts"] =
        serde_json::json!(crate::restarts::recent(data, crate::restarts::IN_FACTS)
            .into_iter()
            .map(|entry| serde_json::json!({
                "at_ms": entry.at_ms,
                "version": entry.version,
                "channel": entry.channel,
                "pid": entry.pid,
            }))
            .collect::<Vec<_>>());
    if let Some(helper) = crate::helper::current() {
        let pid = helper.pid();
        let registered = helper.port().is_some();
        let session = pid.and_then(session_of_process);
        document["helper"] = serde_json::json!({
            "registered": registered,
            "pid": pid,
            "session": session,
        });
    } else {
        document["helper"] = serde_json::json!({ "registered": false });
    }
    // The install root is where this executable was started from: an installed app lives at
    // `<install>/summrise-device.exe`. An executable that cannot name its directory reports the
    // update facts as unknown rather than failing the facts.
    let install = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    // What this build can do beyond request/answer. The console offers a terminal stream only to
    // a device that names this capability: an older build would fail to parse the subscribe frame
    // and answer nothing at all, which is a hung stream rather than a fallback.
    document["caps"] = serde_json::json!([CAP_TERMINAL_STREAM]);
    document["update"] = match install.as_deref() {
        Some(install) => update_state(install),
        None => serde_json::json!({
            "current": null,
            "previous": null,
            "pending": null,
            "last_swap": null,
            "repairs": [],
        }),
    };
    // The tray half's file, as this install has it. The canary update does NOT carry the shell, so
    // its digest is the one fact that tells an operator whether the icon on the machine is the build
    // they think it is; without it, "the tray is old" is indistinguishable from "the tray is right"
    // until someone reports a missing menu entry.
    document["shell"] = match install.as_deref() {
        Some(install) => shell_state(install),
        None => serde_json::json!({ "present": false }),
    };
    document
}

/// The installed `summrise-shell.exe`'s digest and size, for the console to show beside the app's own
/// release facts. Absent on a machine whose tray was never installed.
fn shell_state(install: &Path) -> serde_json::Value {
    let path = install.join("summrise-shell.exe");
    let Ok(bytes) = std::fs::read(&path) else {
        return serde_json::json!({ "present": false });
    };
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(&bytes);
    serde_json::json!({
        "present": true,
        "sha256": digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        "bytes": bytes.len(),
    })
}

/// The capability a device advertises when its channel can stream a terminal's output.
pub const CAP_TERMINAL_STREAM: &str = "terminal.stream";

/// The frame the device sends on its channel: the console reads `host` and stores it per device, and
/// reads the top-level `update` as the device's own account of its last start.
///
/// The `update` key is omitted when `<data>/state.json` is missing or unreadable. A section with
/// `null` fields would be a claim where the device has no fact, and the console is told to draw
/// nothing new for a device that sent nothing.
pub fn frame(data: &std::path::Path) -> String {
    let mut frame = serde_json::json!({ "host": facts(data) });
    if let Some(update) = update_story(data) {
        frame["update"] = update;
    }
    frame.to_string()
}

/// The device's own update story, read from `<data>/state.json`, or `None` when it cannot be read.
///
/// The facts are the ones an operator cannot get from the console otherwise:
///
/// * `current` — the version this process actually runs, from `install.version` (the launcher's
///   `--release-version` argv, not this build's compile-time version);
/// * `verdict` — `lastVerdict`'s outcome and detail, the sentence the device wrote about its last
///   start or the updater's last answer;
/// * `last_update` — `update.to`/`outcome`/`lastResultAtMs`, present once the app has recorded a
///   result: a folded swap answer, or the version change its own claimed start proves (the versioned
///   handoff writes no result file). A staged release is not a result, and `to` alone would read as
///   one;
/// * `last_check` — `update.lastCheck`'s `atMs` and `trigger`, present when the state records a
///   check. It is a separate fact from `last_update`: a machine that looked and found nothing has
///   checked without producing a result, and the console must be able to say when it last looked;
/// * `rolled_back_to` — present only when that last answer was `rolled_back`, and naming the version
///   the device is now running: the swap result names the candidate that failed, and the version
///   that took its place is the one this process started from.
///
/// Nothing is inferred beyond that: a field the state does not have is absent, and a document that
/// does not parse yields no section at all.
pub fn update_story(data: &Path) -> Option<serde_json::Value> {
    let document = std::fs::read_to_string(data.join(crate::store::STATE_FILE)).ok()?;
    let state = State::parse(&document).ok()?;
    let mut story = serde_json::Map::new();
    let current = state.install.version;
    story.insert("current".to_owned(), current.clone().into());
    story.insert(
        "verdict".to_owned(),
        serde_json::json!({
            "outcome": verdict_outcome(state.last_verdict.outcome),
            "detail": state.last_verdict.detail,
        }),
    );
    if let (Some(outcome), Some(at_ms)) = (state.update.outcome, state.update.last_result_at_ms) {
        let mut last = serde_json::Map::new();
        if let Some(to) = state.update.to {
            last.insert("to".to_owned(), to.into());
        }
        last.insert("outcome".to_owned(), update_outcome(outcome).into());
        last.insert("at_ms".to_owned(), at_ms.into());
        story.insert("last_update".to_owned(), serde_json::Value::Object(last));
        if outcome == UpdateOutcome::RolledBack {
            story.insert("rolled_back_to".to_owned(), current.into());
        }
    }
    // The last check is separate from the last result: a device that checked today and found
    // nothing new has no result to fold in, and its panel/console must still say it looked. Absent
    // when the state document predates the field, so "not recorded" is not drawn as "never".
    if let Some(check) = state.update.last_check {
        story.insert(
            "last_check".to_owned(),
            serde_json::json!({
                "at_ms": check.at_ms,
                "trigger": check.trigger.as_str(),
            }),
        );
    }
    Some(serde_json::Value::Object(story))
}

/// The state document's verdict outcome, in the wire words `summrise-core` writes.
///
/// Spelled here rather than serialized through `serde` so a new variant in the core enum is a
/// compile error in this frame instead of a silently missing word on the wire.
fn verdict_outcome(outcome: VerdictOutcome) -> &'static str {
    match outcome {
        VerdictOutcome::Ok => "ok",
        VerdictOutcome::Repaired => "repaired",
        VerdictOutcome::NeedsAttention => "needs_attention",
    }
}

/// The state document's update outcome, in the wire words `summrise-core` writes.
fn update_outcome(outcome: UpdateOutcome) -> &'static str {
    match outcome {
        UpdateOutcome::Applied => "applied",
        UpdateOutcome::RolledBack => "rolled_back",
        UpdateOutcome::Refused => "refused",
    }
}

/// What the launcher's records say, for display.
///
/// `current` is the generation the launcher boots, `pending` a pair staged and not yet proven, and
/// `previous` the pair a failed update returns to. Only `version` and `channel` are read from each
/// `<install>/baseline/<generation>/baseline.json`.
///
/// `last_swap` is the launcher's last swap result and `repairs` the newest restores it made from a
/// baseline generation; both are read from the launcher's files under the install root.
///
/// This does not verify the sidecar's signature. The launcher verified it before it wrote the pair;
/// this reports what the launcher wrote, a fact for the console and not a trust decision. A missing,
/// unreadable or malformed sidecar is `null` for that generation, and one bad file never hides the
/// other two.
fn update_state(install: &Path) -> serde_json::Value {
    serde_json::json!({
        "current": generation_state(install, "current"),
        "previous": generation_state(install, "previous"),
        "pending": generation_state(install, "pending"),
        "last_swap": last_swap(install),
        "repairs": repairs(install),
    })
}

/// The launcher's last swap, from `<install>/swap.result.json`, or `null`.
///
/// The four fields the console shows are copied; `sha256` stays in the launcher's file. The result
/// is the launcher's answer to one request, and a result it did not write — missing, torn, or an
/// outcome this build does not know — is `null` rather than an inferred restart.
fn last_swap(install: &Path) -> serde_json::Value {
    let Ok(text) = std::fs::read_to_string(swap_result_path(install)) else {
        return serde_json::Value::Null;
    };
    let Ok(result) = serde_json::from_str::<SwapResult>(&text) else {
        return serde_json::Value::Null;
    };
    let mut swap = serde_json::Map::new();
    swap.insert(
        "outcome".to_owned(),
        match result.outcome {
            SwapOutcome::Applied => "applied",
            SwapOutcome::RolledBack => "rolled_back",
            SwapOutcome::Refused => "refused",
        }
        .into(),
    );
    swap.insert("at_ms".to_owned(), result.at_ms.into());
    if let Some(version) = result.version {
        swap.insert("version".to_owned(), version.into());
    }
    if let Some(reason) = result.reason {
        swap.insert("reason".to_owned(), reason.into());
    }
    serde_json::Value::Object(swap)
}

/// One line of `<install>/repair.log`, as the launcher writes it.
///
/// `kind` must be `"repaired"` and `from` must be a generation this build knows; the app's own
/// reader (`store::read_new_repairs`) applies the same predicate, because a line from a newer
/// writer or a damaged line is not a repair this build can read. `kind` is not copied to the facts
/// — the file's name already says why the line is there — and `ts_ms` is reported as `at_ms`.
#[derive(serde::Deserialize)]
struct RepairLine {
    ts_ms: u64,
    kind: String,
    #[serde(rename = "from")]
    from: String,
    sha256: String,
}

/// The launcher's repairs, newest first, at most [`RECENT_REPAIRS`] of them.
///
/// `<install>/repair.log` is one JSON line per restore, appended by the launcher. It is read a line
/// at a time and only the newest few are kept, so a log that has grown for years costs one line of
/// memory. A missing or unreadable log is an empty list. A line that does not parse — the tail of an
/// append still in flight — and a line whose kind or source this build does not know are skipped
/// rather than guessed at.
fn repairs(install: &Path) -> serde_json::Value {
    let Ok(file) = std::fs::File::open(install.join(crate::store::REPAIR_LOG_FILE)) else {
        return serde_json::Value::Array(Vec::new());
    };
    let mut newest: Vec<RepairLine> = Vec::with_capacity(RECENT_REPAIRS);
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            return serde_json::Value::Array(Vec::new());
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<RepairLine>(&line) else {
            continue;
        };
        if record.kind != "repaired" || !matches!(record.from.as_str(), "baseline" | "previous") {
            continue;
        }
        if newest.len() == RECENT_REPAIRS {
            newest.remove(0);
        }
        newest.push(record);
    }
    newest.reverse();
    let repairs: Vec<serde_json::Value> = newest
        .into_iter()
        .map(|record| {
            serde_json::json!({
                "at_ms": record.ts_ms,
                "from": record.from,
                "sha256": record.sha256,
            })
        })
        .collect();
    serde_json::Value::Array(repairs)
}

/// The two fields of a generation's sidecar that the console shows.
///
/// The launcher writes `sha256` and `signature` beside them; a document that names only these two
/// leaves the rest unread rather than copying the launcher's evidence into the facts.
#[derive(serde::Deserialize)]
struct SidecarVersion {
    version: String,
    channel: String,
}

/// One generation's `{version, channel}`, or `null` when the sidecar cannot be read or parsed.
fn generation_state(install: &Path, generation: &str) -> serde_json::Value {
    let path = install
        .join(BASELINE_DIR)
        .join(generation)
        .join(BASELINE_SIDECAR);
    let Ok(text) = std::fs::read_to_string(path) else {
        return serde_json::Value::Null;
    };
    let Ok(sidecar) = serde_json::from_str::<SidecarVersion>(&text) else {
        return serde_json::Value::Null;
    };
    serde_json::json!({
        "version": sidecar.version,
        "channel": sidecar.channel,
    })
}

/// The session a process is in — where the helper landed, and what a capture would target.
pub fn session_of_process(pid: u32) -> Option<u32> {
    #[cfg(windows)]
    {
        summrise_winsys::session_of_process(pid)
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        None
    }
}

#[cfg(windows)]
fn platform_facts() -> serde_json::Value {
    use serde_json::json;

    match summrise_winsys::host_facts() {
        Ok(facts) => json!({
            "os": {
                "name": "Windows",
                "release": facts.os_release,
                "version": facts.os_version,
                "arch": std::env::consts::ARCH,
            },
            "machine": facts.machine,
            "uptime_s": facts.uptime_s,
            "cpu": {
                "cores": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
                // Sampled over a short interval: a count of cores says what the machine has, this says
                // what it is doing, which is the question an operator asks before trusting a slow call.
                "percent_used": facts.cpu_percent,
            },
            "memory": {
                "total_bytes": facts.memory_total_bytes,
                "available_bytes": facts.memory_available_bytes,
            },
            "disks": facts.volumes.iter().map(|volume| json!({
                "name": volume.name,
                "total_bytes": volume.total_bytes,
                "free_bytes": volume.free_bytes,
            })).collect::<Vec<_>>(),
            "sessions": facts.sessions.iter().map(|session| json!({
                "id": session.id,
                "user": session.user,
                "state": session.state,
            })).collect::<Vec<_>>(),
        }),
        Err(error) => json!({ "error": error.to_string() }),
    }
}

/// The same document where `/proc` is the only source. It exists so the shape and the tool can be
/// tested on the machines CI runs on; the device only ships on Windows.
#[cfg(not(windows))]
/// The CPU busy share over [`CPU_SAMPLE_MS`] from `/proc/stat`, or `None` when it cannot be read.
///
/// The first `cpu` line holds the counters in jiffies since boot: user, nice, system, idle, iowait,
/// irq, softirq, steal. Busy is everything except idle and iowait, which is the same definition the
/// Windows sampler uses (kernel time minus idle).
#[cfg(not(windows))]
fn cpu_percent() -> Option<u8> {
    fn sample() -> Option<(u64, u64)> {
        let text = std::fs::read_to_string("/proc/stat").ok()?;
        let line = text.lines().next()?;
        let mut fields = line.split_whitespace();
        if fields.next()? != "cpu" {
            return None;
        }
        let values: Vec<u64> = fields.filter_map(|value| value.parse().ok()).collect();
        if values.len() < 4 {
            return None;
        }
        let idle = values[3] + values.get(4).copied().unwrap_or(0);
        Some((values.iter().sum::<u64>(), idle))
    }

    let (total_before, idle_before) = sample()?;
    std::thread::sleep(std::time::Duration::from_millis(200));
    let (total_after, idle_after) = sample()?;
    let total = total_after.saturating_sub(total_before);
    let idle = idle_after.saturating_sub(idle_before);
    if total == 0 {
        return None;
    }
    Some((((total - idle) * 100 + total / 2) / total).min(100) as u8)
}

#[cfg(not(windows))]
fn platform_facts() -> serde_json::Value {
    use serde_json::json;

    let uptime = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|text| text.split_whitespace().next()?.parse::<f64>().ok())
        .map(|seconds| seconds as u64)
        .unwrap_or(0);
    let memory = std::fs::read_to_string("/proc/meminfo").ok().map(|text| {
        let field = |name: &str| -> u64 {
            text.lines()
                .find(|line| line.starts_with(name))
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|value| value.parse::<u64>().ok())
                .map(|kilobytes| kilobytes * 1024)
                .unwrap_or(0)
        };
        (field("MemTotal:"), field("MemAvailable:"))
    });
    json!({
        "os": {
            "name": std::env::consts::OS,
            "version": std::fs::read_to_string("/proc/sys/kernel/osrelease")
                .map(|text| text.trim().to_owned())
                .unwrap_or_default(),
            "arch": std::env::consts::ARCH,
        },
        "machine": std::fs::read_to_string("/proc/sys/kernel/hostname")
            .map(|text| text.trim().to_owned())
            .unwrap_or_default(),
        "uptime_s": uptime,
        "cpu": {
            "cores": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
            "percent_used": cpu_percent(),
        },
        "memory": {
            "total_bytes": memory.map(|(total, _)| total).unwrap_or(0),
            "available_bytes": memory.map(|(_, available)| available).unwrap_or(0),
        },
        "disks": [],
        "sessions": [],
    })
}

#[cfg(test)]
mod tests {

    /// The CPU sampler answers on a machine that has a `/proc/stat`, and the value is a percentage.
    #[cfg(not(windows))]
    #[test]
    fn the_cpu_sample_is_a_percentage_of_a_real_machine() {
        let percent = cpu_percent().expect("this machine has /proc/stat");
        assert!(percent <= 100, "a share cannot exceed the whole: {percent}");
    }
    use super::*;

    use crate::test_support::TempDir;

    /// Write one generation's sidecar under a temp install.
    fn write_sidecar(install: &Path, generation: &str, sidecar: &str) {
        let directory = install.join("baseline").join(generation);
        std::fs::create_dir_all(&directory).expect("the generation directory is creatable");
        std::fs::write(directory.join("baseline.json"), sidecar).expect("the sidecar is writable");
    }

    /// Write the launcher's swap result under a temp install.
    fn write_swap_result(install: &Path, result: &str) {
        std::fs::create_dir_all(install).expect("the install directory is creatable");
        std::fs::write(crate::update::swap_result_path(install), result)
            .expect("the swap result is writable");
    }

    /// Write the launcher's repair log under a temp install.
    fn write_repair_log(install: &Path, log: &str) {
        std::fs::create_dir_all(install).expect("the install directory is creatable");
        std::fs::write(install.join(crate::store::REPAIR_LOG_FILE), log)
            .expect("the repair log is writable");
    }

    /// One line of the launcher's repair log, as `append_repair_record` writes it.
    fn repair_line(ts_ms: u64, from: &str, sha256: &str) -> String {
        format!(r#"{{"ts_ms":{ts_ms},"kind":"repaired","from":"{from}","sha256":"{sha256}"}}"#)
    }

    #[test]
    fn the_three_generations_report_their_version_and_channel() {
        let dir = TempDir::new("update-state-generations");
        let install = dir.join("install");
        write_sidecar(
            &install,
            "current",
            r#"{"version":"0.3.29","channel":"stable","sha256":"00","signature":"00"}"#,
        );
        write_sidecar(
            &install,
            "previous",
            r#"{"version":"0.3.28","channel":"stable","sha256":"00","signature":"00"}"#,
        );
        write_sidecar(
            &install,
            "pending",
            r#"{"version":"0.3.30","channel":"canary","sha256":"00","signature":"00"}"#,
        );

        // The whole document equals: the sha256 and signature are not copied to the facts.
        assert_eq!(
            update_state(&install),
            serde_json::json!({
                "current": { "version": "0.3.29", "channel": "stable" },
                "previous": { "version": "0.3.28", "channel": "stable" },
                "pending": { "version": "0.3.30", "channel": "canary" },
                "last_swap": null,
                "repairs": [],
            })
        );
    }

    #[test]
    fn one_malformed_sidecar_is_null_while_the_others_report() {
        let dir = TempDir::new("update-state-malformed");
        let install = dir.join("install");
        write_sidecar(
            &install,
            "current",
            r#"{"version":"0.3.29","channel":"stable"}"#,
        );
        write_sidecar(
            &install,
            "previous",
            r#"{"version":"0.3.28","channel":"stable"}"#,
        );
        // A torn write: the file is there, the document is not.
        write_sidecar(&install, "pending", r#"{"version":"0.3.3"#);

        let update = update_state(&install);
        assert_eq!(update["current"]["version"], "0.3.29");
        assert_eq!(update["previous"]["version"], "0.3.28");
        assert!(update["pending"].is_null(), "{update}");
    }

    #[test]
    fn a_sidecar_without_a_channel_is_null() {
        let dir = TempDir::new("update-state-missing-field");
        let install = dir.join("install");
        write_sidecar(&install, "current", r#"{"version":"0.3.29"}"#);

        let update = update_state(&install);
        assert!(update["current"].is_null(), "{update}");
    }

    #[test]
    fn a_missing_baseline_directory_is_three_nulls() {
        let dir = TempDir::new("update-state-no-baseline");
        let install = dir.join("install");
        std::fs::create_dir_all(&install).expect("the install directory is creatable");

        assert_eq!(
            update_state(&install),
            serde_json::json!({
                "current": null,
                "previous": null,
                "pending": null,
                "last_swap": null,
                "repairs": [],
            })
        );
    }

    #[test]
    fn an_install_path_that_does_not_exist_does_not_panic() {
        let dir = TempDir::new("update-state-nonexistent");
        let update = update_state(&dir.join("not-an-install"));
        assert!(update["current"].is_null(), "{update}");
        assert!(update["previous"].is_null(), "{update}");
        assert!(update["pending"].is_null(), "{update}");
    }

    #[test]
    fn an_applied_swap_result_parses_without_its_digest() {
        let dir = TempDir::new("last-swap-applied");
        let install = dir.join("install");
        write_swap_result(
            &install,
            r#"{"outcome":"applied","version":"0.3.30","sha256":"aa","at_ms":1700000000000}"#,
        );

        let update = update_state(&install);
        assert_eq!(
            update["last_swap"],
            serde_json::json!({
                "outcome": "applied",
                "version": "0.3.30",
                "at_ms": 1700000000000_u64,
            })
        );
        assert!(
            update["last_swap"].get("sha256").is_none(),
            "the launcher's digest is not copied to the facts: {update}"
        );
    }

    #[test]
    fn a_rolled_back_swap_result_carries_its_version_and_reason() {
        let dir = TempDir::new("last-swap-rolled-back");
        let install = dir.join("install");
        write_swap_result(
            &install,
            concat!(
                r#"{"outcome":"rolled_back","version":"0.3.30","sha256":"aa","at_ms":1700000000001,"#,
                r#""reason":"the new build did not answer the health probe"}"#,
            ),
        );

        assert_eq!(
            update_state(&install)["last_swap"],
            serde_json::json!({
                "outcome": "rolled_back",
                "version": "0.3.30",
                "at_ms": 1700000000001_u64,
                "reason": "the new build did not answer the health probe",
            })
        );
    }

    #[test]
    fn a_refused_swap_result_keeps_its_reason_and_no_version() {
        let dir = TempDir::new("last-swap-refused");
        let install = dir.join("install");
        write_swap_result(
            &install,
            concat!(
                r#"{"outcome":"refused","sha256":"aa","at_ms":1700000000002,"#,
                r#""reason":"the staged sidecar was not signed for this channel"}"#,
            ),
        );

        let update = update_state(&install);
        assert_eq!(
            update["last_swap"],
            serde_json::json!({
                "outcome": "refused",
                "at_ms": 1700000000002_u64,
                "reason": "the staged sidecar was not signed for this channel",
            })
        );
        assert!(
            update["last_swap"].get("version").is_none(),
            "a refusal names no version: {update}"
        );
    }

    #[test]
    fn a_torn_swap_result_is_null_while_the_generations_report() {
        let dir = TempDir::new("last-swap-torn");
        let install = dir.join("install");
        write_sidecar(
            &install,
            "current",
            r#"{"version":"0.3.29","channel":"stable"}"#,
        );
        // The launcher writes the result atomically; half a document is not an answer.
        write_swap_result(&install, r#"{"outcome":"applied","version":"0.3.3"#);

        let update = update_state(&install);
        assert!(update["last_swap"].is_null(), "{update}");
        assert_eq!(update["current"]["version"], "0.3.29");
        assert!(update["previous"].is_null(), "{update}");
        assert_eq!(update["repairs"], serde_json::json!([]));
    }

    #[test]
    fn three_repairs_report_newest_first() {
        let dir = TempDir::new("repairs-three");
        let install = dir.join("install");
        let log = [
            repair_line(1700000000000, "baseline", "aa"),
            repair_line(1700000000100, "previous", "bb"),
            repair_line(1700000000200, "baseline", "cc"),
        ]
        .join("\n")
            + "\n";
        write_repair_log(&install, &log);

        assert_eq!(
            update_state(&install)["repairs"],
            serde_json::json!([
                { "at_ms": 1700000000200_u64, "from": "baseline", "sha256": "cc" },
                { "at_ms": 1700000000100_u64, "from": "previous", "sha256": "bb" },
                { "at_ms": 1700000000000_u64, "from": "baseline", "sha256": "aa" },
            ])
        );
    }

    #[test]
    fn a_half_written_last_repair_is_not_counted() {
        let dir = TempDir::new("repairs-torn");
        let install = dir.join("install");
        let mut log = String::new();
        log.push_str(&repair_line(1700000000000, "baseline", "aa"));
        log.push('\n');
        log.push_str(&repair_line(1700000000100, "previous", "bb"));
        log.push('\n');
        // An append in flight: the line is there, its newline is not.
        log.push_str(r#"{"ts_ms":1700000000200,"kind":"repaired","from":"baseline","sha256":"c"#);
        write_repair_log(&install, &log);

        assert_eq!(
            update_state(&install)["repairs"],
            serde_json::json!([
                { "at_ms": 1700000000100_u64, "from": "previous", "sha256": "bb" },
                { "at_ms": 1700000000000_u64, "from": "baseline", "sha256": "aa" },
            ])
        );
    }

    #[test]
    fn ten_repairs_report_the_newest_five() {
        let dir = TempDir::new("repairs-ten");
        let install = dir.join("install");
        let log = (0..10)
            .map(|index| repair_line(1700000000000 + index, "baseline", &format!("{index:02}")))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        write_repair_log(&install, &log);

        let repairs = update_state(&install)["repairs"].clone();
        assert_eq!(
            repairs.as_array().map(|records| records.len()),
            Some(5),
            "{repairs}"
        );
        let expected: Vec<serde_json::Value> = (5..10)
            .rev()
            .map(|index| {
                serde_json::json!({
                    "at_ms": 1700000000000_u64 + index,
                    "from": "baseline",
                    "sha256": format!("{index:02}"),
                })
            })
            .collect();
        assert_eq!(repairs, serde_json::Value::Array(expected));
    }

    #[test]
    fn a_repair_line_with_another_kind_or_source_is_skipped() {
        let dir = TempDir::new("repairs-unknown-kind");
        let install = dir.join("install");
        let log = [
            r#"{"ts_ms":1700000000100,"kind":"something-else","from":"baseline","sha256":"aa"}"#
                .to_owned(),
            r#"{"ts_ms":1700000000200,"kind":"repaired","from":"elsewhere","sha256":"bb"}"#
                .to_owned(),
            // A repair the app's own reader accepts still counts beside them.
            repair_line(1700000000300, "previous", "cc"),
        ]
        .join("\n")
            + "\n";
        write_repair_log(&install, &log);

        assert_eq!(
            update_state(&install)["repairs"],
            serde_json::json!([
                { "at_ms": 1700000000300_u64, "from": "previous", "sha256": "cc" },
            ])
        );
    }

    #[test]
    fn the_facts_report_the_installed_shell_and_its_digest() {
        let dir = std::env::temp_dir().join(format!("summrise-shell-facts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the fixture install is creatable");

        // No shell beside the app: the fact says so rather than naming an empty digest.
        let absent = shell_state(&dir);
        assert_eq!(absent["present"], serde_json::json!(false), "{absent}");

        std::fs::write(dir.join("summrise-shell.exe"), b"a tray binary")
            .expect("the fixture shell is writable");
        let present = shell_state(&dir);
        assert_eq!(present["present"], serde_json::json!(true), "{present}");
        assert_eq!(
            present["bytes"],
            serde_json::json!(13),
            "the size is the file's: {present}"
        );
        assert_eq!(
            present["sha256"].as_str().map(str::len),
            Some(64),
            "a digest, in the same hex the release documents use: {present}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_facts_report_the_update_state() {
        let dir = std::env::temp_dir().join(format!("summrise-facts-{}", std::process::id()));
        let facts = facts(&dir);
        let update = &facts["update"];
        assert!(update.is_object(), "{facts}");
        for generation in ["current", "previous", "pending"] {
            let value = &update[generation];
            assert!(
                value.is_null() || (value["version"].is_string() && value["channel"].is_string()),
                "{facts}"
            );
        }
        let last_swap = &update["last_swap"];
        assert!(
            last_swap.is_null()
                || (last_swap["outcome"].is_string()
                    && last_swap["at_ms"].is_u64()
                    && last_swap.get("sha256").is_none()),
            "{facts}"
        );
        assert!(update["repairs"].is_array(), "{facts}");
    }

    #[test]
    fn the_facts_are_a_document_a_console_can_render() {
        let facts = facts(&std::env::temp_dir());
        assert!(facts["os"]["name"].is_string(), "{facts}");
        assert!(facts["os"]["arch"].is_string(), "{facts}");
        assert!(facts["uptime_s"].is_u64(), "{facts}");
        assert!(facts["cpu"]["cores"].is_u64(), "{facts}");
        assert!(facts["memory"]["total_bytes"].is_u64(), "{facts}");
        assert!(facts["disks"].is_array(), "{facts}");
        assert!(facts["sessions"].is_array(), "{facts}");
        // Without a helper in this process, the document says so rather than omitting the key.
        assert_eq!(facts["helper"]["registered"], serde_json::json!(false));
    }

    /// A state document with only the facts the update story reads, so a test names one fact and
    /// leaves the rest of the document at values the schema accepts.
    fn state_document(version: &str, verdict: &str, update: &str) -> String {
        format!(
            concat!(
                r#"{{"schema":1,"#,
                r#""install":{{"dir":"C:/Summrise","version":"{version}","channel":"canary"}},"#,
                r#""baseline":{{"version":"{version}","sha256":"aa","exe":"baseline/current/summrise-device.exe"}},"#,
                r#""service":{{"state":"running","pid":1,"startedAt":"2026-11-05T08:21:00Z"}},"#,
                r#""lastVerdict":{verdict},"#,
                r#""sessions":[],"#,
                r#""update":{update}}}"#,
            ),
            version = version,
            verdict = verdict,
            update = update,
        )
    }

    fn write_state(data: &Path, document: &str) {
        std::fs::create_dir_all(data).expect("the data directory is creatable");
        std::fs::write(data.join(crate::store::STATE_FILE), document)
            .expect("the state document is writable");
    }

    /// **THE DEVICE'S OWN STORY, NOT THE CONSOLE'S GUESS.** The state document records the version
    /// this process runs, the verdict it reached at its last start, and the updater's last answer.
    /// The frame carries all three so an operator reading a machine that just came back can tell an
    /// update that was refused (and left the pointer where it was) from one that was applied.
    #[test]
    fn the_update_story_reports_the_running_version_verdict_and_last_update() {
        let dir = TempDir::new("update-story-refused");
        let data = dir.join("data");
        write_state(
            &data,
            &state_document(
                "0.3.275",
                r#"{"ts":"2026-11-05T08:21:00Z","outcome":"ok","detail":"started 0.3.275"}"#,
                r#"{"phase":"idle","to":"0.3.276","outcome":"refused","lastResultAtMs":1762330860000}"#,
            ),
        );

        assert_eq!(
            update_story(&data),
            Some(serde_json::json!({
                "current": "0.3.275",
                "verdict": { "outcome": "ok", "detail": "started 0.3.275" },
                "last_update": {
                    "to": "0.3.276",
                    "outcome": "refused",
                    "at_ms": 1762330860000_u64,
                },
            }))
        );
    }

    /// The last check is a fact the device keeps in its own state (`update.lastCheck`) and the
    /// panel reads it; the frame carries it too, so the console's Machine tab can say when the
    /// machine last looked rather than treating the last applied target as the current one. The
    /// trigger is the device's own word for what asked for the check.
    #[test]
    fn the_update_story_reports_the_last_check_and_what_asked_for_it() {
        let dir = TempDir::new("update-story-last-check");
        let data = dir.join("data");
        write_state(
            &data,
            &state_document(
                "0.3.284",
                r#"{"ts":"2026-11-06T17:24:00Z","outcome":"ok","detail":"started 0.3.284"}"#,
                r#"{"phase":"idle","to":"0.3.273","outcome":"applied","lastResultAtMs":1762330860000,"lastCheck":{"atMs":1793985962342,"trigger":"channel"}}"#,
            ),
        );

        let story = update_story(&data).expect("the state document is readable");
        assert_eq!(
            story["last_check"],
            serde_json::json!({ "at_ms": 1793985962342_u64, "trigger": "channel" }),
            "the last check is the state's own moment and trigger: {story}"
        );
    }

    /// An updater that has not answered yet leaves no `last_update`: the stage a release reached is
    /// not a result, and `to` alone would read as one. The verdict is still the device's story.
    #[test]
    fn a_last_update_appears_only_after_the_updater_has_answered() {
        let dir = TempDir::new("update-story-staged");
        let data = dir.join("data");
        write_state(
            &data,
            &state_document(
                "0.3.275",
                r#"{"ts":"2026-11-05T08:21:00Z","outcome":"ok","detail":"started 0.3.275"}"#,
                r#"{"phase":"staged","to":"0.3.276","outcome":null,"lastResultAtMs":null}"#,
            ),
        );

        let story = update_story(&data).expect("the state document is readable");
        assert!(
            story.get("last_update").is_none(),
            "a staged release is not a result: {story}"
        );
        assert_eq!(story["current"], serde_json::json!("0.3.275"));
        assert_eq!(
            story["verdict"]["detail"],
            serde_json::json!("started 0.3.275")
        );
    }

    /// A rollback names the version the device returned to. The swap result names the candidate that
    /// failed; after the rollback the running version is the one that took its place, so the field is
    /// present only when the updater's last answer was `rolled_back`.
    #[test]
    fn a_rollback_names_the_version_the_device_is_running() {
        let dir = TempDir::new("update-story-rolled-back");
        let data = dir.join("data");
        write_state(
            &data,
            &state_document(
                "0.3.275",
                r#"{"ts":"2026-11-05T08:21:00Z","outcome":"needs_attention","detail":"started 0.3.275"}"#,
                r#"{"phase":"idle","to":"0.3.276","outcome":"rolled_back","lastResultAtMs":1762330860000}"#,
            ),
        );

        let story = update_story(&data).expect("the state document is readable");
        assert_eq!(story["rolled_back_to"], serde_json::json!("0.3.275"));
        assert_eq!(
            story["last_update"]["outcome"],
            serde_json::json!("rolled_back")
        );
        assert_eq!(story["last_update"]["to"], serde_json::json!("0.3.276"));
    }

    /// A last answer that was not a rollback has no version it rolled back to: a refusal installed
    /// nothing and an applied swap moved forward, so the key is absent rather than naming `current`.
    #[test]
    fn only_a_rollback_names_the_version_it_returned_to() {
        let dir = TempDir::new("update-story-applied");
        let data = dir.join("data");
        write_state(
            &data,
            &state_document(
                "0.3.276",
                r#"{"ts":"2026-11-05T08:30:00Z","outcome":"ok","detail":"started 0.3.276"}"#,
                r#"{"phase":"idle","to":"0.3.276","outcome":"applied","lastResultAtMs":1762330860000}"#,
            ),
        );

        let story = update_story(&data).expect("the state document is readable");
        assert_eq!(
            story["last_update"]["outcome"],
            serde_json::json!("applied")
        );
        assert!(
            story.get("rolled_back_to").is_none(),
            "an applied update rolled nothing back: {story}"
        );
    }

    /// An unreadable state document is not a story. The frame omits the section whole rather than
    /// sending `current: null` or an empty verdict for the console to render as a fact.
    #[test]
    fn a_frame_without_a_readable_state_omits_the_update_section() {
        let dir = TempDir::new("update-story-absent");
        let data = dir.join("data");
        std::fs::create_dir_all(&data).expect("the data directory is creatable");

        let frame: serde_json::Value =
            serde_json::from_str(&frame(&data)).expect("the frame is JSON");
        assert!(frame["host"].is_object(), "{frame}");
        assert!(
            frame.get("update").is_none(),
            "a device with no state has no story: {frame}"
        );
    }

    #[test]
    fn the_frame_carries_the_update_story_beside_the_host_facts() {
        let dir = TempDir::new("update-story-frame");
        let data = dir.join("data");
        write_state(
            &data,
            &state_document(
                "0.3.275",
                r#"{"ts":"2026-11-05T08:21:00Z","outcome":"ok","detail":"started 0.3.275"}"#,
                r#"{"phase":"idle","to":"0.3.276","outcome":"refused","lastResultAtMs":1762330860000}"#,
            ),
        );

        let frame: serde_json::Value =
            serde_json::from_str(&frame(&data)).expect("the frame is JSON");
        assert!(frame["host"].is_object(), "{frame}");
        assert_eq!(frame["update"]["current"], serde_json::json!("0.3.275"));
        assert_eq!(
            frame["update"]["verdict"]["detail"],
            serde_json::json!("started 0.3.275")
        );
        assert_eq!(
            frame["update"]["last_update"]["outcome"],
            serde_json::json!("refused")
        );
    }

    #[test]
    fn the_frame_carries_the_facts_under_host() {
        let frame: serde_json::Value =
            serde_json::from_str(&frame(&std::env::temp_dir())).expect("the frame is JSON");
        assert!(frame["host"]["os"]["name"].is_string(), "{frame}");
    }

    #[test]
    fn the_uptime_is_a_positive_number_on_any_real_machine() {
        let facts = facts(&std::env::temp_dir());
        let uptime = facts["uptime_s"].as_u64().unwrap_or(0);
        assert!(uptime > 0, "{facts}");
    }
}
