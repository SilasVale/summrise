//! COMMAND DISPATCH, ARGV PARSING, AND THE DECISIONS THE VERBS MAKE.
//!
//! Ported from the `commands` object and the `require.main` block at the end of
//! `agent/summrise-agent-npm/src/summrise.ts`.
//!
//! Every verb here returns an [`Outcome`] — lines and an exit code — rather than printing. That is
//! the whole point of the slice: the TypeScript's `start`/`stop`/`restart` bodies are INLINE and
//! execute `schtasks`, so the oracle could only pin their SHAPE by scanning the built file. Here the
//! decision is a function over [`crate::host::Host`], and the case that used to be a source scan
//! EXECUTES the branch instead.

use crate::config::agent_port;
use crate::host::{Host, RunResult, Tri};
use crate::monitors::{ascii_json, monitors_json};
use crate::paths::{release_marker_path, Layout};
use crate::ps::{ps_argv, psq};
use crate::psgen::{autostart_argv, register_desktop_task_ps, BOOT_TASKS};
use crate::status::{status_report, StatusFacts};
use crate::update::{
    await_release_marker, busy_is_fresh, is_behind, latest_release_version, release_marker_verdict,
    update_would_not_move, AwaitOpts, ReleaseMarkerCheck, Verb,
};
use crate::version::package_version;
use serde_json::{json, Value};
use std::path::Path;

/// The verbs, in the order the help prints them — derived from ONE list so the help cannot promise a
/// verb the dispatcher does not have. (`report` and `watch` were still advertised after their
/// removal; a usage line is a contract, and one that lies is worse than none.)
pub const VERBS: [&str; 13] = [
    "setup",
    "monitor",
    "desktop",
    "status",
    "start",
    "stop",
    "restart",
    "autostart",
    "update",
    "rollback",
    "uninstall",
    "run",
    "tunnel",
];

/// What a verb decided: the exit code, the lines for stdout, and the lines for stderr.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    pub exit: i32,
    pub out: Vec<String>,
    pub err: Vec<String>,
}

impl Outcome {
    pub fn ok() -> Self {
        Outcome::default()
    }

    pub fn fail(code: i32, msg: impl Into<String>) -> Self {
        Outcome {
            exit: code,
            out: Vec::new(),
            err: vec![msg.into()],
        }
    }

    pub fn say(mut self, msg: impl Into<String>) -> Self {
        self.out.push(msg.into());
        self
    }

    pub fn warn(mut self, msg: impl Into<String>) -> Self {
        self.err.push(msg.into());
        self
    }

    pub fn exit(mut self, code: i32) -> Self {
        self.exit = code;
        self
    }
}

/// The help: one header line and one line per verb.
pub fn usage_lines() -> Vec<String> {
    let mut v = vec![format!(
        "summrise <{}> -- Summrise Agent control",
        VERBS.join("|")
    )];
    for k in VERBS {
        v.push(format!("  {k}"));
    }
    v
}

/// The `--json` printer, and the ONE place `ascii_json` is reached from for that output.
///
/// Split out so the wiring itself is testable: the TypeScript's case had to read the shipped file to
/// prove the call site went through the escaping helper, because the helper's own test passed while
/// the printer kept calling `JSON.stringify` directly.
pub fn print_monitors_json(
    device: &str,
    asked_at_ms: i64,
    payload: &Value,
    only: Option<&str>,
) -> String {
    let projected = monitors_json(device, asked_at_ms, Some(payload), only);
    ascii_json(&projected, None)
}

/// `schtasks /<Action> /TN SummriseAgent` — **and the RESULT is returned**.
///
/// It used to be discarded, which is why `stop` printed "stopped" and exited 0 for a missing task or
/// an access-denied, and why `start`/`restart` were silent either way.
pub fn svc(host: &dyn Host, action: &str) -> RunResult {
    host.run(
        &[
            "schtasks".to_string(),
            format!("/{action}"),
            "/TN".to_string(),
            "SummriseAgent".to_string(),
        ],
        None,
    )
}

/// The scheduled task's State (`Running`, `Ready`, `Disabled`, …) or **`None` when it could not be
/// read**.
///
/// THREE STATES, NOT ONE. `None` means "I could not ask", and callers report that as itself instead
/// of as a verdict. `Get-ScheduledTask` rather than parsing `schtasks /Query`, whose headers are
/// LOCALIZED.
pub fn task_state(host: &dyn Host, name: &str) -> Option<String> {
    let r = host.run(
        &ps_argv(&format!(
            "(Get-ScheduledTask -TaskName '{name}' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty State)"
        )),
        None,
    );
    if r.status != Some(0) {
        return None;
    }
    let s = r.stdout.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// TELL THE AGENT IT IS ABOUT TO BE STOPPED ON PURPOSE, before anything kills it.
///
/// `stop` and `restart` end the process from OUTSIDE, so it writes no clean-exit marker — and the run
/// journal then reports the operator's own action as `crashed` whenever the revival takes longer than
/// the heartbeat window. The device has one place that knows how to mark it
/// (`POST /api/run/mark-exit`), so the CLI ASKS rather than writing the file itself: one rule, one
/// implementation.
///
/// BEST-EFFORT BY DESIGN: it runs BEFORE the kill, and a device whose agent is already gone must
/// still be stoppable. A failure is a note, never a refusal to stop.
///
/// THE REASON DECIDES THE VERDICT: `update` makes the next start report `replaced` (the swap was
/// deliberate and normal) while `stop` reports `clean-exit`. Neither can be inferred from timing —
/// a measured update swap left a 61 s heartbeat gap while the boot task revives a real crash within
/// ~60 s, so an unmarked update lands as a CRASH.
pub fn mark_deliberate_stop(host: &dyn Host, layout: &Layout, reason: &str) -> Vec<String> {
    let Some(token) = crate::config::device_token(host, &layout.etc_dir) else {
        return vec![format!(
            "  (note: could not mark this stop as deliberate -- no device token in {}\\config.yaml; the next start may report a crash)",
            layout.etc_dir
        )];
    };
    let port = agent_port(host, &layout.etc_dir);
    let body = ascii_json(&json!({ "reason": reason }), None);
    let argv: Vec<String> = vec![
        "curl".into(),
        "-sS".into(),
        "-m".into(),
        "15".into(),
        "-X".into(),
        "POST".into(),
        "-H".into(),
        format!("Authorization: Bearer {token}"),
        "-H".into(),
        "content-type: application/json".into(),
        "-d".into(),
        body,
        format!("http://127.0.0.1:{port}/api/run/mark-exit"),
    ];
    let r = host.run(&argv, Some(20_000));
    if r.status != Some(0) {
        return vec![format!(
            "  (note: could not mark this stop as deliberate -- device unreachable on 127.0.0.1:{port}; the next start may report a crash)"
        )];
    }
    Vec::new()
}

/// `summrise status` — gather the facts, render the report.
///
/// THE EXIT CODE IS 0 EVEN WHEN THE REPORT SAYS "UNKNOWN", AND THAT IS THE CONVENTION: `status` is
/// asked for a REPORT, it produces one, and the verdict lives in the text. A script that needs a
/// verdict must not read this exit code.
pub fn status_decision(
    host: &dyn Host,
    layout: &Layout,
    latest_version: Option<String>,
) -> Outcome {
    let agent = host.process_running("summrise-agent.exe");
    let release = host
        .read_string(&release_marker_path(&layout.dir))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let marker = crate::psgen::update_busy_path();
    let marker_path = Path::new(&marker);
    let (update_marker_ms, unreadable) = if host.exists(marker_path) {
        match host.mtime_ms(marker_path) {
            Some(ms) => (Some(ms), false),
            // The marker EXISTS and could not be read: `None` then means "could not look", NOT
            // "nothing in flight".
            None => (None, true),
        }
    } else {
        (None, false)
    };
    let facts = StatusFacts {
        agent_running: agent.as_bool(),
        install_dir: layout.dir.clone(),
        exe_exists: host.exists(Path::new(&layout.exe_dst)),
        port: agent_port(host, &layout.etc_dir),
        release_version: release,
        update_marker_ms,
        update_marker_unreadable: unreadable,
        package_version: package_version(),
        latest_version,
        now_ms: host.now_ms(),
    };
    Outcome {
        exit: 0,
        out: status_report(&facts),
        err: Vec::new(),
    }
}

/// `summrise start` — and the status is CHECKED, like `stop` does.
///
/// The comment on `svc` says the return value exists because discarding it made `start`/`restart`
/// "silent either way" — and two of the four call sites still discarded it, including this one, which
/// is step 2 of every documented journey. An operator whose agent never came up got exit 0 and no
/// sentence at all.
pub fn start_decision(host: &dyn Host, _layout: &Layout) -> Outcome {
    let r = svc(host, "Run");
    match r.status {
        Some(0) => Outcome::ok(),
        other => Outcome::fail(
            1,
            format!(
                "summrise start: schtasks /Run failed (status {}) -- the agent is NOT running",
                other.map(|c| c.to_string()).unwrap_or_else(|| "?".into())
            ),
        ),
    }
}

/// `summrise stop`.
///
/// Say WHY before the process goes (see [`mark_deliberate_stop`]), then ASK whether the task is still
/// running instead of inferring it from `/End`'s exit code — which answers neither question. `stop`
/// used to print "the agent may still be running" for a task that had already stopped, and, before
/// that, "stopped" and exit 0 for a missing task or an access-denied.
pub fn stop_decision(host: &dyn Host, layout: &Layout) -> Outcome {
    let mut out = Outcome::ok();
    for line in mark_deliberate_stop(host, layout, "stop") {
        out = out.warn(line);
    }
    let r = svc(host, "End");
    if r.status != Some(0) {
        let status = r
            .status
            .map(|c| c.to_string())
            .unwrap_or_else(|| "?".into());
        match task_state(host, "SummriseAgent") {
            // Could not read it — say that, rather than guessing either way (the three-state rule).
            None => {
                return out
                    .warn(format!(
                        "summrise stop: schtasks /End failed (status {status}) and the task's state could not be read"
                    ))
                    .exit(1);
            }
            Some(st) if st == "Running" => {
                return out
                    .warn(format!(
                        "summrise stop: schtasks /End failed (status {status}) and the task is still Running"
                    ))
                    .exit(1);
            }
            Some(_) => {
                out = out.warn(format!(
                    "summrise stop: schtasks /End returned {status}, but the task is not Running -- nothing to stop"
                ));
            }
        }
    }
    out.say(
        "stopped -- revives via 'summrise start' or the 5-min watchdog ('summrise autostart off' opts out of autostart)",
    )
}

/// `summrise restart`.
///
/// THE STOP IS REPORTED BUT NOT FATAL: a task that was not running has nothing to end, so a non-zero
/// `/End` is the ordinary case for "restart a stopped agent", and the operator asked for the START.
pub fn restart_decision(host: &dyn Host, layout: &Layout) -> Outcome {
    let mut out = Outcome::ok();
    for line in mark_deliberate_stop(host, layout, "stop") {
        out = out.warn(line);
    }
    let end = svc(host, "End");
    if end.status != Some(0) {
        out = out.warn(format!(
            "summrise restart: schtasks /End failed (status {}) -- the old process may still be running",
            end.status.map(|c| c.to_string()).unwrap_or_else(|| "?".into())
        ));
    }
    host.sleep_ms(2000); // `timeout /t 2`, with no shell to run it in
    let run = svc(host, "Run");
    if run.status != Some(0) {
        return out
            .warn(format!(
                "summrise restart: schtasks /Run failed (status {}) -- the agent is NOT running",
                run.status
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "?".into())
            ))
            .exit(1);
    }
    out
}

/// `summrise autostart on|off|status` — the only real "don't start at boot" control.
///
/// `summrise stop` is one-shot and the watchdog revives it. Missing task = skipped with a note (never
/// fatal — headless installs have no SummriseDesktop). State is read via `Get-ScheduledTask`
/// (locale-independent enum, unlike the localized `schtasks /Query` headers).
pub fn autostart_decision(host: &dyn Host, _layout: &Layout, sub: &str) -> Outcome {
    let sub = if sub.is_empty() { "status" } else { sub }.to_lowercase();
    let mut out = Outcome::ok();
    if sub == "status" {
        for t in BOOT_TASKS {
            let r = host.run(
                &ps_argv(&format!(
                    "(Get-ScheduledTask -TaskName '{t}' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty State -ErrorAction SilentlyContinue)"
                )),
                None,
            );
            let s = r.stdout.trim().to_string();
            if r.status != Some(0) {
                // THREE STATES, NOT ONE: every failure to READ the state used to land on the same
                // output as a genuinely absent task, so an operator asking "is autostart on?" was
                // sent to re-run setup by an access-denied.
                let why = if r.stderr.trim().is_empty() {
                    format!(
                        "exit {}",
                        r.status
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "?".into())
                    )
                } else {
                    r.stderr.trim().to_string()
                };
                out = out.say(format!("{t}: state UNKNOWN -- could not read it ({why})"));
            } else {
                out = out.say(format!(
                    "{t}: {}",
                    if s.is_empty() { "(not installed)" } else { &s }
                ));
            }
        }
        return out;
    }
    if sub != "on" && sub != "off" {
        return Outcome::fail(1, "usage: summrise autostart <on|off|status>");
    }
    let mut failed = false;
    let mut skipped = 0;
    for t in BOOT_TASKS {
        // ASK WHETHER THE TASK EXISTS FIRST. The comment above this loop has said "Missing task =
        // skipped with a note" all along, and the code made a missing task FATAL — so a headless
        // install could not turn autostart off at all.
        let exists = host
            .run(
                &[
                    "schtasks".into(),
                    "/Query".into(),
                    "/TN".into(),
                    t.to_string(),
                ],
                None,
            )
            .status
            == Some(0);
        if !exists {
            out = out.say(format!(
                "autostart: {t} not installed -- skipped (headless install)"
            ));
            skipped += 1;
            continue;
        }
        let argv = autostart_argv(t, &sub);
        let r = host.run(&argv, None);
        if r.status != Some(0) {
            out = out.warn(format!(
                "autostart: {t} {sub} FAILED on an existing task (status {}) -- the change did not take",
                r.status.map(|c| c.to_string()).unwrap_or_else(|| "?".into())
            ));
            failed = true;
        } else {
            out = out.say(format!(
                "autostart: {t} {}",
                if sub == "on" { "enabled" } else { "disabled" }
            ));
        }
    }
    if skipped == BOOT_TASKS.len() {
        return out
            .warn("autostart: no boot tasks found -- nothing to switch. `summrise setup` registers the agent task; the desktop task comes from the installer.")
            .exit(1);
    }
    // Only claim the durable outcome when every EXISTING task actually changed.
    if sub == "off" && !failed {
        out = out.say(
            "autostart: off -- tasks stay disabled across reboot until 'summrise autostart on'",
        );
    }
    if failed {
        out = out.exit(1);
    }
    out
}

/// The desktop task registration step, as its own decision.
///
/// `setup` runs the registration script and must report the status it ACTUALLY got. It used to print
/// "SummriseDesktop registered (logon + a 5-minute watchdog) and started" unconditionally, with the
/// status discarded — and the enclosing try/catch could not fire, because the spawn helper RETURNS
/// rather than throws.
pub fn register_desktop_task(host: &dyn Host, layout: &Layout) -> (bool, String) {
    let path = format!("{}\\register-desktop-task.ps1", layout.scripts_dir);
    let body = register_desktop_task_ps(&psq(&layout.dir)).join("\r\n");
    if host.write_bytes(Path::new(&path), body.as_bytes()).is_err() {
        return (
            false,
            format!("setup: WARNING -- could not register SummriseDesktop; run: powershell -File \"{path}\""),
        );
    }
    let r = host.run(
        &[
            "powershell".into(),
            "-NoProfile".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-File".into(),
            path.clone(),
        ],
        None,
    );
    if r.status == Some(0) {
        (
            true,
            "setup: SummriseDesktop registered (logon + a 5-minute watchdog) and started"
                .to_string(),
        )
    } else {
        (
            false,
            format!(
                "setup: WARNING -- registering SummriseDesktop failed (status {}); run: powershell -File \"{path}\"",
                r.status.map(|c| c.to_string()).unwrap_or_else(|| "spawn error".into())
            ),
        )
    }
}

/// The default device hostname: `--hostname` beats `SUMMRISE_HOSTNAME` beats THIS MACHINE'S NAME.
///
/// **THE DEFAULT USED TO BE THE DEVELOPER'S OWN DEVICE — A HARD-CODED `d1` HOST — AND EVERY FRESH
/// INSTALL INHERITED IT.** (The literal lives in `endpoints::DEVICE_HOST_SUFFIX`'s own note, where
/// the rule that forbids it is one line away.) Everything downstream followed from that one string: the console
/// registered the new machine as `d1`, and the tunnel was named `summrise-agent-d1`, colliding with
/// the real one. A default that names somebody else's machine is not a default.
pub fn device_host(host: &dyn Host, args: &[String]) -> String {
    if let Some(i) = args.iter().position(|a| a == "--hostname") {
        if let Some(v) = args.get(i + 1) {
            return v.clone();
        }
    }
    if let Some(v) = host.env("SUMMRISE_HOSTNAME").filter(|s| !s.is_empty()) {
        return v;
    }
    let machine: String = host
        .hostname()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("{machine}{}", crate::endpoints::DEVICE_HOST_SUFFIX)
}

/// `summrise setup` — THE LOCAL INSTALL. The body lives in [`crate::setup`], because it stages the
/// same files the swap does and a second copy of that list is how the two come to disagree.
pub use crate::setup::setup_decision;

/// `summrise update` — THE GUARDS, in the order the defects were measured, and then the swap.
///
/// Every refusal here is a measured defect: a CLI too old to deliver the version on the channel, an
/// update that would stamp the device with the version it already has, and a second update racing the
/// first through `Copy-Item` on `*.new`. Past them the work is [`crate::swap::update_swap`] — the
/// staging, the generated swap script, the WMI handoff and the read-back that says whether it took.
pub fn update_decision(host: &dyn Host, layout: &Layout, latest: Option<&str>) -> Outcome {
    let self_version = package_version();
    let mut out = Outcome::ok();

    // THE CLI CANNOT DELIVER A VERSION IT DOES NOT CARRY (round 201, measured on the operator's
    // device): "update requested 1.2.438 -> 1.2.438", "copy ok=True", and the release unchanged —
    // while `summrise status` kept saying "THIS DEVICE IS BEHIND ... run 'summrise update'".
    if let Some(latest) = latest {
        if !self_version.is_empty() && is_behind(&self_version, latest) {
            return Outcome::fail(
                1,
                format!(
                    "update: this CLI is {self_version} and the release channel has {latest}.\n  \
                     Updating from here would stamp the device with {self_version} and change nothing, because the device reads <install>/.summrise-release (written by this package) as its version.\n  \
                     Install the new CLI first, then update again:\n    npm i -g summrise-agent\n    summrise update"
                ),
            );
        }
    }

    // AND THE SAME REFUSAL WITHOUT THE NETWORK.
    let device_now = host
        .read_string(&release_marker_path(&layout.dir))
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if update_would_not_move(&device_now, &self_version) {
        return Outcome::fail(
            1,
            format!(
                "update: this CLI is {self_version} and the device is ALREADY on {device_now}.\n  \
                 Updating from here would stamp the device with the version it already has and change nothing,\n  \
                 because the device reads <install>/.summrise-release (written by this package) as its version.\n  \
                 If the device is meant to be newer, this CLI is too old to deliver it: install the new one first.\n    npm i -g summrise-agent\n    summrise update"
            ),
        );
    }

    // npm audit #10: no mutual exclusion — two updates (or setup racing a swap) interleave Copy-Item
    // on *.new, leaving a half-written exe "ok". The marker is created EXCLUSIVELY so two racing
    // updaters cannot BOTH pass the freshness check.
    let busy = crate::psgen::update_busy_path();
    let busy_path = Path::new(&busy);
    let now = host.now_ms();
    if host.exists(busy_path) {
        match host.mtime_ms(busy_path) {
            Some(ms) if busy_is_fresh(ms, now) => {
                return Outcome::fail(
                    1,
                    format!("update: another update looks in progress ({busy} <10 min old) -- wait, or delete the marker after a mid-swap reboot"),
                );
            }
            // Stale marker — overwrite it.
            Some(_) => {
                let _ = host.write_bytes(busy_path, now.to_string().as_bytes());
            }
            None => {
                return Outcome::fail(
                    1,
                    format!("update: another update looks in progress (cannot stat {busy})"),
                );
            }
        }
    } else {
        let _ = host.mkdirs(busy_path.parent().unwrap_or(Path::new(".")));
        let _ = host.write_bytes(busy_path, now.to_string().as_bytes());
    }

    out = out.say(format!(
        "update: {} -> {} -- staging, the connection will drop",
        if device_now.is_empty() {
            "unknown"
        } else {
            &device_now
        },
        if self_version.is_empty() {
            "unknown"
        } else {
            &self_version
        }
    ));
    for line in mark_deliberate_stop(host, layout, "update") {
        out = out.warn(line);
    }
    // THE RECEIPT, before anything irreversible happens. It is written into the swap's OWN log sink,
    // because a receipt written to a different file than the swap writes is worse than none: it would
    // look like the swap never started. A failed write is a WARNING rather than an abort — `ps()`
    // RETURNS its spawn result in the TypeScript, so the try/catch around it could never fire and a
    // failed receipt was SILENT; it is best-effort, and the cost of silence is named.
    if !crate::swap::write_receipt(host, layout, &device_now, &self_version) {
        out = out.warn(
            "update: WARNING -- could not write the receipt to summrise-update.log; if this swap fails, the log will not distinguish it from a command that never arrived",
        );
    }
    // ...and past here the swap itself. The busy marker is released by the swap (its generated
    // script removes it) or by a staging failure inside `update_swap`; either way a later run is not
    // refused for a swap that never started.
    let swap = crate::swap::update_swap(
        host,
        layout,
        &host.package_dir(),
        &device_now,
        &self_version,
        90_000,
        2_000,
    );
    out.out.extend(swap.out);
    out.err.extend(swap.err);
    out.exit(swap.exit)
}

/// `summrise rollback <x.y.z> | --clear` — the pin is EARNED, not asserted.
///
/// `etc\.summrise-release` is the device's ONLY local version truth: `agent_update` reads it as
/// `local` and `/api/status` serves it as `release`. `rollback` used to write it UNCONDITIONALLY once
/// `update` returned status 0 — but status 0 means the WMI handoff was ACCEPTED, not that the swap
/// succeeded. A rollback whose swap died therefore left a marker claiming a version the device was
/// not running: every UI lies, and once the pin is cleared `agent_update` sees the fake version,
/// decides it is current, and the device is stuck on the old release permanently.
///
/// The oracle could only pin this with a SOURCE SCAN, because the call site does real I/O. Here the
/// branch EXECUTES: `read_marker` is injected, and the pin is written only on `verdict.write_pin`.
pub fn rollback_decision<R>(
    host: &dyn Host,
    layout: &Layout,
    want: &str,
    from: Option<&str>,
    timeout_ms: i64,
    interval_ms: i64,
    read_marker: R,
) -> Outcome
where
    R: FnMut() -> Option<String>,
{
    if want == "--clear" {
        let pin = Path::new(&layout.etc_dir).join(".rollback-pin");
        // A FAILED DELETE IS NOT AN ABSENT PIN.
        if host.exists(&pin) && host.remove_file(&pin).is_err() {
            return Outcome::fail(
                1,
                format!(
                    "rollback: could not remove {} -- the pin is still in force",
                    pin.display()
                ),
            );
        }
        return Outcome::ok().say("rollback: pin cleared -- auto-upgrade is allowed again");
    }
    if !crate::update::rollback_version_ok(want) {
        return Outcome::fail(
            1,
            format!("usage: summrise rollback <x.y.z|--clear> (got '{want}')"),
        );
    }
    let opts = AwaitOpts {
        want: want.to_string(),
        timeout_ms,
        interval_ms,
    };
    let now = || host.now_ms();
    let mut slept = 0i64;
    let check: ReleaseMarkerCheck = await_release_marker(
        &opts,
        &mut { read_marker },
        |ms| {
            slept += ms;
            host.sleep_ms(ms.max(0) as u64);
        },
        &now,
    );
    let _ = slept;
    let verdict = release_marker_verdict(&check, want, Verb::Rollback, from);
    if !verdict.write_pin {
        // THE PIN IS GATED ON THE VERDICT. The CLI never writes `etc\.summrise-release` itself — only
        // the swap script may, and only from a provable copy.
        return Outcome::fail(verdict.exit_code, verdict.message);
    }
    let pin = Path::new(&layout.etc_dir).join(".rollback-pin");
    if host.write_bytes(&pin, want.as_bytes()).is_err() {
        return Outcome::fail(
            1,
            format!(
                "rollback: could not write {} -- the release is not pinned",
                pin.display()
            ),
        );
    }
    Outcome {
        exit: verdict.exit_code,
        out: vec![verdict.message],
        err: Vec::new(),
    }
    .say(format!(
        "rollback: staged {want} and pinned it at {}",
        pin.display()
    ))
}

/// `summrise desktop` — ask for the window, and report the FACT the script answered.
pub fn desktop_decision(host: &dyn Host, layout: &Layout) -> Outcome {
    let path = format!("{}\\start-desktop.ps1", layout.scripts_dir);
    let body =
        crate::psgen::start_desktop_ps(&psq(&layout.desk_dir), &psq(&layout.logs_dir)).join("\r\n");
    if host.write_bytes(Path::new(&path), body.as_bytes()).is_err() {
        return Outcome::fail(1, format!("desktop: could not write {path}"));
    }
    let r = host.run(
        &[
            "powershell".into(),
            "-NoProfile".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-File".into(),
            path,
        ],
        None,
    );
    let word = r.stdout.trim().to_string();
    match word.as_str() {
        "already-running" => Outcome::ok().say("desktop: the shell is already running"),
        "started" => Outcome::ok().say("desktop: the shell is starting"),
        _ => Outcome::fail(
            1,
            format!(
                "desktop: not-started (the launcher answered {:?})",
                if word.is_empty() { "" } else { &word }
            ),
        ),
    }
}

/// `--version` answers the installer's acceptance check.
///
/// It is NOT a verb, so it used to fall through to the usage branch, print the verb list and exit 1 —
/// while the installer's own checklist BEGINS with "`summrise --version` / the panel opens". A fresh
/// install that worked therefore reported a failure in the one place the operator is told to look.
pub fn version_outcome() -> Outcome {
    Outcome::ok().say(package_version())
}

/// Parse argv and decide. No printing: [`run`] is the thin shell over this.
pub fn dispatch(host: &dyn Host, args: &[String]) -> Outcome {
    let layout = Layout::resolve(host);
    let Some(cmd) = args.first().map(|s| s.as_str()) else {
        return Outcome {
            exit: 0,
            out: usage_lines(),
            err: Vec::new(),
        };
    };
    if cmd == "--version" || cmd == "-v" || cmd == "version" {
        return version_outcome();
    }
    if !VERBS.contains(&cmd) {
        // The list is DERIVED from the verb table, so the help cannot promise a verb that was pruned.
        return Outcome {
            exit: 1,
            out: usage_lines(),
            err: Vec::new(),
        };
    }
    let rest = &args[1..];
    match cmd {
        "setup" => setup_decision(host, &layout, rest),
        "monitor" => crate::monitor::monitor_decision(host, &layout, rest),
        "status" => {
            let latest = latest_release_version(host);
            status_decision(host, &layout, latest)
        }
        "start" => start_decision(host, &layout),
        "stop" => stop_decision(host, &layout),
        "restart" => restart_decision(host, &layout),
        "autostart" => autostart_decision(
            host,
            &layout,
            rest.first().map(|s| s.as_str()).unwrap_or("status"),
        ),
        "update" => {
            let latest = latest_release_version(host);
            update_decision(host, &layout, latest.as_deref())
        }
        "rollback" => crate::rollback::rollback_command(host, &layout, rest),
        "uninstall" => crate::uninstall::uninstall_decision(host, &layout, rest),
        "run" => crate::swap::run_decision(host, &layout, rest),
        "tunnel" => crate::tunnel::tunnel_decision(host, &layout, rest),
        "desktop" => desktop_decision(host, &layout),
        // UNREACHABLE BY CONSTRUCTION: `VERBS` is the list the guard above matched, so a verb added
        // there without an arm here is a compile error rather than a run-time surprise.
        other => Outcome::fail(
            3,
            format!("summrise {other}: no dispatcher arm -- add it to the verb table's match"),
        ),
    }
}

/// The real entry point: decide, print, and return the exit code.
pub fn run(host: &dyn Host, args: &[String]) -> i32 {
    let outcome = dispatch(host, args);
    for line in &outcome.out {
        println!("{line}");
    }
    for line in &outcome.err {
        eprintln!("{line}");
    }
    outcome.exit
}

/// A Yes/No/Unknown the dispatch needs, kept next to its one caller so the three-state rule is
/// visible where it is applied.
pub fn tri_of(host: &dyn Host, image: &str) -> Tri {
    host.process_running(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{RunResult, Tri};
    use crate::testing::FakeHost;

    fn host_with_layout() -> (FakeHost, Layout) {
        let host = FakeHost::new()
            .with_env("ProgramData", "C:\\ProgramData")
            .with_reg("InstallDir", "D:\\Summrise")
            .with_reg("DataDir", "C:\\ProgramData\\Summrise");
        let layout = Layout::resolve(&host);
        (host, layout)
    }

    fn ok_run() -> RunResult {
        RunResult {
            status: Some(0),
            ..Default::default()
        }
    }

    /// Oracle: cli.test.mjs:2982 — "stop and restart mark the run as deliberate before killing it".
    ///
    /// The run journal cannot see WHO ended the process, so a supervisor must say so first —
    /// otherwise `summrise restart` reads as "crashed" whenever the revival is slower than the
    /// heartbeat window. The TypeScript pinned this with a source scan because the bodies are inline;
    /// here the ORDER is observable, so the case asserts it.
    #[test]
    fn stop_and_restart_mark_the_run_before_ending_it() {
        for (name, decision) in [
            ("stop", stop_decision as fn(&dyn Host, &Layout) -> Outcome),
            (
                "restart",
                restart_decision as fn(&dyn Host, &Layout) -> Outcome,
            ),
        ] {
            let (host, layout) = host_with_layout();
            host.set_now(1_700_000_000_000);
            let host = host
                .with_file(
                    &crate::config::config_path(&layout.etc_dir).to_string_lossy(),
                    "server:\n  port: 18080\n  device_token: tok.ABC-1\n",
                )
                .script_runs(vec![ok_run(), ok_run()]);
            let _ = decision(&host, &layout);
            let runs = host.runs();
            let mark_at = runs
                .iter()
                .position(|argv| argv.iter().any(|a| a.ends_with("/api/run/mark-exit")))
                .unwrap_or_else(|| panic!("{name} must mark the run before ending it: {runs:?}"));
            let end_at = runs
                .iter()
                .position(|argv| argv.iter().any(|a| a == "/End"))
                .unwrap_or_else(|| panic!("{name} must end the task: {runs:?}"));
            assert!(
                mark_at < end_at,
                "{name}: the mark must come BEFORE the kill"
            );
            // ...and the marker carries the reason the verdict depends on.
            let mark = &runs[mark_at];
            let body = mark
                .iter()
                .position(|a| a == "-d")
                .map(|i| mark[i + 1].clone())
                .unwrap();
            assert!(
                body.contains("stop"),
                "the reason decides the verdict: {body}"
            );
        }
    }

    /// The UPDATE path marks its own swap with the reason that makes the next start say "replaced"
    /// instead of "crashed" — its timing cannot be told from a crash, because the gaps overlap.
    #[test]
    fn update_marks_its_swap_as_deliberate() {
        let (host, layout) = host_with_layout();
        let host = host
            .with_file(
                &crate::config::config_path(&layout.etc_dir).to_string_lossy(),
                "server:\n  port: 18080\n  device_token: tok.ABC-1\n",
            )
            .with_file(&release_marker_path(&layout.dir).to_string_lossy(), "1.0.0");
        let _ = update_decision(&host, &layout, None);
        let runs = host.runs();
        let mark = runs
            .iter()
            .find(|argv| argv.iter().any(|a| a.ends_with("/api/run/mark-exit")))
            .expect("update must mark its swap");
        let body = mark
            .iter()
            .position(|a| a == "-d")
            .map(|i| mark[i + 1].clone())
            .unwrap();
        assert!(body.contains("update"), "{body}");
        assert!(
            body.contains("reason"),
            "the marker carries the reason: {body}"
        );
    }

    /// Oracle: cli.test.mjs:3043 — "every schtasks action is CHECKED, so a service verb cannot fail
    /// silently".
    ///
    /// The TypeScript could only pin the SHAPE (an `svc()` call whose status is discarded) by reading
    /// the built file. Here each caller is a function, so the case drives a FAILING schtasks and
    /// asserts the verb reports it.
    #[test]
    fn every_schtasks_action_is_checked() {
        // start: a failing /Run is fatal and says the agent is NOT running.
        let (h, l) = host_with_layout();
        let h = h.script_runs(vec![RunResult {
            status: Some(1),
            ..Default::default()
        }]);
        let r = start_decision(&h, &l);
        assert_eq!(r.exit, 1);
        assert!(r.err[0].contains("schtasks /Run failed"), "{:?}", r.err);

        // restart: a failing /Run is fatal; a failing /End is only reported.
        let (h, l) = host_with_layout();
        let h = h.script_runs(vec![
            RunResult {
                status: Some(1),
                ..Default::default()
            },
            RunResult {
                status: Some(1),
                ..Default::default()
            },
        ]);
        let r = restart_decision(&h, &l);
        assert_eq!(r.exit, 1);
        assert!(
            r.err.iter().any(|e| e.contains("schtasks /End failed")),
            "the stop is reported: {:?}",
            r.err
        );
        assert!(
            r.err.iter().any(|e| e.contains("schtasks /Run failed")),
            "the start is checked: {:?}",
            r.err
        );

        // autostart: a failing /Change on an EXISTING task is fatal and says so.
        let (h, l) = host_with_layout();
        let h = h.script_runs(vec![
            ok_run(), // /Query -> the task exists
            RunResult {
                status: Some(1),
                ..Default::default()
            }, // /Change -> fails
            ok_run(), // /Query -> exists
            ok_run(), // /Change -> ok
        ]);
        let r = autostart_decision(&h, &l, "on");
        assert_eq!(r.exit, 1);
        assert!(
            r.err.iter().any(|e| e.contains("the change did not take")),
            "{:?}",
            r.err
        );
    }

    /// Oracle: cli.test.mjs:3213 — "`stop` ASKS whether the task is still running, instead of
    /// inferring it from /End".
    ///
    /// The twenty-second exploration found `stop` exiting 1 AFTER achieving its goal, printing "the
    /// agent may still be running" — a false sentence. The exit code answers neither question, so the
    /// command has to read the task's State, and branch three ways.
    #[test]
    fn stop_asks_whether_the_task_is_still_running() {
        // (a) /End failed, the state could not be read: non-zero, and it SAYS so.
        let (h, l) = host_with_layout();
        let h = h.script_runs(vec![
            RunResult {
                status: Some(1),
                ..Default::default()
            },
            RunResult {
                status: None,
                ..Default::default()
            },
        ]);
        let r = stop_decision(&h, &l);
        assert_eq!(r.exit, 1, "a failed READ must not answer either way");
        assert!(
            r.err.iter().any(|e| e.contains("could not be read")),
            "{:?}",
            r.err
        );

        // (b) /End failed and the task is genuinely Running: non-zero, and it says so.
        let (h, l) = host_with_layout();
        let h = h.script_runs(vec![
            RunResult {
                status: Some(1),
                ..Default::default()
            },
            RunResult {
                status: Some(0),
                stdout: "Running\n".into(),
                ..Default::default()
            },
        ]);
        let r = stop_decision(&h, &l);
        assert_eq!(r.exit, 1);
        assert!(
            r.err.iter().any(|e| e.contains("still Running")),
            "{:?}",
            r.err
        );

        // (c) /End failed but the task had ALREADY stopped: the goal was met — say the non-zero was
        //     harmless, and DO NOT exit non-zero (that is the round-249 defect back).
        let (h, l) = host_with_layout();
        let h = h.script_runs(vec![
            RunResult {
                status: Some(1),
                ..Default::default()
            },
            RunResult {
                status: Some(0),
                stdout: "Ready\n".into(),
                ..Default::default()
            },
        ]);
        let r = stop_decision(&h, &l);
        assert_eq!(r.exit, 0, "the goal was met: {:?}", r.err);
        assert!(
            r.err
                .iter()
                .any(|e| e.contains("the task is not Running -- nothing to stop")),
            "{:?}",
            r.err
        );
    }

    /// Oracle: cli.test.mjs:3268 — "`setup` reports the desktop task registration it actually got".
    ///
    /// It printed the success sentence unconditionally, with the status discarded — and the enclosing
    /// try/catch could not fire, because the spawn helper RETURNS rather than throws.
    #[test]
    fn setup_reports_the_desktop_registration_it_got() {
        let (h, l) = host_with_layout();
        let ok_host = h.script_runs(vec![ok_run()]);
        let (ok, msg) = register_desktop_task(&ok_host, &l);
        assert!(ok);
        assert!(
            msg.contains("SummriseDesktop registered (logon + a 5-minute watchdog) and started")
        );

        let (h2, l2) = host_with_layout();
        let bad_host = h2.script_runs(vec![RunResult {
            status: Some(1),
            ..Default::default()
        }]);
        let (ok, msg) = register_desktop_task(&bad_host, &l2);
        assert!(!ok);
        assert!(
            msg.contains("registering SummriseDesktop failed"),
            "the success sentence must sit inside a status check: {msg}"
        );
    }

    /// Oracle: cli.test.mjs:3164 — "setup must not overwrite a remapped DataDir with the literal
    /// default".
    #[test]
    fn setup_echoes_the_resolved_data_dir_into_the_registry() {
        let host = FakeHost::new()
            .with_env("ProgramData", "C:\\ProgramData")
            .with_reg("InstallDir", "D:\\Summrise")
            .with_reg("DataDir", "E:\\SummriseData");
        let layout = Layout::resolve(&host);
        let host = host.script_runs(vec![ok_run(), ok_run()]);
        let _ = setup_decision(&host, &layout, &[]);
        let effects = host.effects();
        assert!(
            effects.contains(&"reg_write:DataDir=E:\\SummriseData".to_string()),
            "the registry must echo the resolved data dir: {effects:?}"
        );
        assert!(
            !effects
                .iter()
                .any(|e| e.starts_with("reg_write:DataDir=C:\\ProgramData")),
            "and must never re-derive it from ProgramData, which discards a remap: {effects:?}"
        );
    }

    /// Oracle: cli.test.mjs:1658 — "rollback: the pin is gated on the verdict, and the CLI never
    /// writes the release marker".
    ///
    /// The ORIGINAL bug was a WIRING bug: `rollback()` wrote the marker unconditionally at its call
    /// site, and mutation testing proved that restoring it (`if (false)`) left the whole suite GREEN —
    /// because the TypeScript could only scan the source. Here the branch runs.
    #[test]
    fn rollback_pins_only_on_a_proven_swap_and_never_writes_the_marker() {
        let (host, layout) = host_with_layout();
        let marker = release_marker_path(&layout.dir);
        let host = host.with_file(&marker.to_string_lossy(), "1.2.321");

        // Proven: the marker shows the target. The pin is written; the RELEASE MARKER is untouched.
        let proven =
            rollback_decision(&host, &layout, "1.2.322", Some("1.2.321"), 5000, 10, || {
                Some("1.2.322".to_string())
            });
        assert_eq!(proven.exit, 0, "{proven:?}");
        let pin = Path::new(&layout.etc_dir).join(".rollback-pin");
        assert_eq!(
            host.read_string(&pin).ok().as_deref(),
            Some("1.2.322"),
            "a proven swap writes the pin"
        );
        assert_eq!(
            host.read_string(&marker).ok().as_deref(),
            Some("1.2.321"),
            "the CLI must NOT write etc\\.summrise-release — only the swap script may"
        );

        // Unproven: the marker never reaches the target. NO pin, non-zero exit, and the sentence
        // names what was actually seen.
        let (host2, layout2) = host_with_layout();
        let marker2 = release_marker_path(&layout2.dir);
        let host2 = host2.with_file(&marker2.to_string_lossy(), "1.2.321");
        host2.set_now(0);
        let unproven = rollback_decision(
            &host2,
            &layout2,
            "1.2.322",
            Some("1.2.321"),
            100,
            10,
            || Some("1.2.321".to_string()),
        );
        assert_eq!(unproven.exit, 1, "an unproven swap must exit non-zero");
        let pin2 = Path::new(&layout2.etc_dir).join(".rollback-pin");
        assert!(
            !host2.exists(&pin2),
            "an unproven swap must not pin the device to a version it is not running"
        );
        assert_eq!(
            host2.read_string(&marker2).ok().as_deref(),
            Some("1.2.321"),
            "and must not write the release marker either"
        );
        assert!(unproven.err[0].contains("NOT pinned"), "{:?}", unproven.err);
    }

    /// The rollback version gate is the URL-interpolation gate: a value that is not a plain dotted
    /// triple could escape the `/summrise-agent/` prefix.
    #[test]
    fn rollback_refuses_a_version_that_could_escape_the_url() {
        let (host, layout) = host_with_layout();
        let r = rollback_decision(&host, &layout, "1.2.307/../../evil", None, 100, 10, || None);
        assert_eq!(r.exit, 1);
        assert!(r.err[0].contains("usage: summrise rollback"), "{:?}", r.err);
    }

    /// `--clear` releases the pin, and a failed DELETE is not an absent pin.
    #[test]
    fn rollback_clear_releases_the_pin() {
        let (host, layout) = host_with_layout();
        let pin = Path::new(&layout.etc_dir).join(".rollback-pin");
        let host = host.with_file(&pin.to_string_lossy(), "1.2.300");
        let r = rollback_decision(&host, &layout, "--clear", None, 0, 0, || None);
        assert_eq!(r.exit, 0);
        assert!(!host.exists(&pin));
    }

    /// The three update refusals, each one a measured defect.
    #[test]
    fn update_refuses_the_three_measured_defects() {
        // (a) The CLI is behind the release channel ON THE SAME RELEASE LINE: it cannot deliver what
        //     the channel has. A cross-minor jump is deliberately NOT this guard — that is a
        //     different operation, and `isBehind` says so.
        let (h, l) = host_with_layout();
        let r = update_decision(&h, &l, Some("1.2.999"));
        assert_eq!(r.exit, 1);
        assert!(
            r.err[0].contains("the release channel has 1.2.999"),
            "{:?}",
            r.err
        );

        // (b) The device is already on this CLI's version: nothing would move.
        let (h, l) = host_with_layout();
        let self_v = package_version();
        let h = h.with_file(&release_marker_path(&l.dir).to_string_lossy(), &self_v);
        let r = update_decision(&h, &l, Some(&self_v));
        assert_eq!(r.exit, 1);
        assert!(r.err[0].contains("the device is ALREADY on"), "{:?}", r.err);
        assert!(
            !h.exists(Path::new(&crate::psgen::update_busy_path())),
            "a refusal before the marker must not leave one behind"
        );

        // (c) Another update is in flight: the marker is fresh.
        let (h, l) = host_with_layout();
        h.set_now(1_700_000_000_000);
        let busy = crate::psgen::update_busy_path();
        let h = h
            .with_file(&release_marker_path(&l.dir).to_string_lossy(), "1.0.0")
            .with_file_at(&busy, 1_700_000_000_000 - 60_000, "1");
        let r = update_decision(&h, &l, None);
        assert_eq!(r.exit, 1);
        assert!(
            r.err[0].contains("another update looks in progress"),
            "{:?}",
            r.err
        );
    }

    /// An update past its guards writes the RECEIPT into the swap's own log sink before handing off
    /// — the marker that makes "the command never ran" provable from the log alone.
    ///
    /// **THE EFFECT BODY IS PORTED (landing 4b)**, so this drives the WHOLE verb rather than stopping
    /// at a `NOT PORTED` exit 3: the guards, the receipt, the staging, the WMI handoff, and the wait
    /// for the release marker the swap itself writes. The fixture therefore has to carry a package
    /// (exe + launcher) and a device that answers, which is what the old `exit 3` assertion was
    /// standing in for.
    ///
    /// The exit is 1 and that is the SWAP's own verdict, not the port's absence: nothing on a fake
    /// runs `summrise-update.ps1`, so `.summrise-release` never moves to this CLI's version and
    /// `update` reports the device did not confirm — the honest answer, and the reason `update`
    /// reads the marker back instead of trusting `ReturnValue=0`.
    #[test]
    fn an_update_that_proceeds_writes_the_receipt() {
        let pkg = "C:\\npm\\node_modules\\summrise-agent";
        let (h, l) = host_with_layout();
        let h = h
            .with_file(&release_marker_path(&l.dir).to_string_lossy(), "1.0.0")
            .with_file(
                &crate::config::config_path(&l.etc_dir).to_string_lossy(),
                "server:\n  port: 18080\n  device_token: tok.ABC-1\n",
            )
            .with_file(&format!("{pkg}\\summrise-agent.exe"), "MZ")
            .with_file(&format!("{pkg}\\summrise-launch.exe"), "MZ")
            // Every child exits 0 — the deliberate-stop POST, the receipt's PowerShell, the WMI
            // handoff's two runs — and the handoff reads `ReturnValue: 0` out of the stdout.
            .script_runs(vec![RunResult {
                status: Some(0),
                stdout: "{\"ReturnValue\":0}".into(),
                ..Default::default()
            }]);
        let r = update_decision(&h, &l, None);
        assert!(
            !r.err.iter().any(|e| e.contains("not ported")),
            "the effect body is ported now: {r:?}"
        );
        assert_eq!(r.exit, 1, "the device never confirmed the swap: {r:?}");
        let runs = h.runs();
        let receipt_at = runs
            .iter()
            .position(|argv| argv.iter().any(|a| a.contains("update requested")))
            .expect("the receipt must be written before the handoff");
        assert!(
            runs[receipt_at]
                .iter()
                .any(|a| a.contains("update requested 1.0.0 ->")),
            "{:?}",
            runs[receipt_at]
        );
        // AND BEFORE THE HANDOFF, which is the whole point of a receipt: a marker written after the
        // WMI call could not tell "the swap never started" from "the CLI never ran".
        let handoff_at = runs
            .iter()
            .position(|argv| argv.iter().any(|a| a.contains("Win32_Process")))
            .expect("the WMI handoff");
        assert!(
            receipt_at < handoff_at,
            "the receipt ({receipt_at}) must precede the handoff ({handoff_at})"
        );
        // ...and the swap script itself reached the disk, so the handoff had something to run.
        let ps1 = crate::paths::win_join(&l.scripts_dir, "summrise-update.ps1");
        assert!(
            h.file(&ps1.to_string_lossy())
                .is_some_and(|b| !b.is_empty()),
            "the swap script must be written before the handoff"
        );
    }

    /// The help is DERIVED from the verb table, so it cannot advertise a verb the dispatcher does
    /// not have.
    #[test]
    fn the_help_lists_exactly_the_verbs_that_exist() {
        let lines = usage_lines();
        assert!(lines[0].contains("summrise <"));
        for v in VERBS {
            assert!(
                lines.iter().any(|l| l.trim() == v),
                "{v} must be in the help"
            );
        }
        for dead in ["report", "watch"] {
            assert!(
                !lines.iter().any(|l| l.trim() == dead),
                "the help advertises '{dead}', which the dispatcher does not have"
            );
        }
    }

    /// `--version` is not a verb, and it must answer instead of printing the usage list with exit 1.
    #[test]
    fn version_answers_the_installers_check() {
        let host = FakeHost::new();
        let r = dispatch(&host, &["--version".to_string()]);
        assert_eq!(r.exit, 0);
        assert_eq!(r.out.len(), 1);
        assert!(
            regex::Regex::new(r"^\d+\.\d+\.\d+$")
                .unwrap()
                .is_match(&r.out[0]),
            "and print the version, not the usage list: {:?}",
            r.out
        );
    }

    /// A bare invocation prints the help and exits 0; an UNKNOWN verb exits 1 — the TypeScript's
    /// `process.exit(cmd ? 1 : 0)`.
    #[test]
    fn a_bare_invocation_prints_the_help_and_an_unknown_verb_fails() {
        let host = FakeHost::new();
        let bare = dispatch(&host, &[]);
        assert_eq!(bare.exit, 0);
        assert!(bare.out.len() >= 10, "expected the verb list");
        let unknown = dispatch(&host, &["nonsense".to_string()]);
        assert_eq!(unknown.exit, 1);
        assert!(unknown.out[0].contains("summrise <"));
    }

    /// The `--json` printer is the only `--json` path, and it goes through the escaper.
    #[test]
    fn the_json_path_uses_the_escaping_helper() {
        let printed = print_monitors_json("d1", 1, &json!({"targets": []}), None);
        assert_eq!(
            serde_json::from_str::<Value>(&printed).unwrap()["device"],
            json!("d1")
        );
    }

    /// The status decision reads the four facts that cost the incident four hand reads.
    #[test]
    fn status_gathers_the_release_and_the_marker() {
        let (h, l) = host_with_layout();
        h.set_now(1_700_000_000_000);
        let busy = crate::psgen::update_busy_path();
        let h = h
            .with_file(&release_marker_path(&l.dir).to_string_lossy(), "1.2.321")
            .with_file("D:\\Summrise\\summrise-agent.exe", "x")
            .with_file_at(&busy, 1_700_000_000_000 - 27 * 60_000, "1")
            .with_process("summrise-agent.exe", Tri::Yes);
        let r = status_decision(&h, &l, Some("1.2.322".to_string()));
        let text = r.out.join("\n");
        assert_eq!(r.exit, 0, "a report is a report");
        assert!(text.contains("status: RUNNING"), "{text}");
        assert!(text.contains("release: 1.2.321"), "{text}");
        assert!(text.contains("DID NOT FINISH"), "{text}");
        assert!(text.contains("THIS DEVICE IS BEHIND"), "{text}");
    }

    /// ...and an UNREADABLE process list is reported as UNKNOWN, not as STOPPED.
    #[test]
    fn status_does_not_call_an_unreadable_process_list_stopped() {
        let (h, l) = host_with_layout();
        let h = h.with_process("summrise-agent.exe", Tri::Unknown);
        let text = status_decision(&h, &l, None).out.join("\n");
        assert!(text.contains("UNKNOWN"), "{text}");
        assert!(!text.contains("status: STOPPED"), "{text}");
    }

    /// `desktop` reports the FACT the launcher answered, not the step it took.
    #[test]
    fn desktop_reports_what_the_launcher_answered() {
        let (h, l) = host_with_layout();
        let h = h.script_runs(vec![RunResult {
            status: Some(0),
            stdout: "already-running\n".into(),
            ..Default::default()
        }]);
        let r = desktop_decision(&h, &l);
        assert_eq!(r.exit, 0);
        assert!(r.out[0].contains("already running"), "{:?}", r.out);

        let (h2, l2) = host_with_layout();
        let h2 = h2.script_runs(vec![RunResult {
            status: Some(1),
            stdout: "not-started\n".into(),
            ..Default::default()
        }]);
        let r = desktop_decision(&h2, &l2);
        assert_eq!(r.exit, 1);
        assert!(r.err[0].contains("not-started"), "{:?}", r.err);
    }

    /// The default device hostname is THIS machine's name, never somebody else's device.
    #[test]
    fn the_default_hostname_is_this_machine() {
        let host = FakeHost::new().with_hostname("DESKTOP-ABC123");
        let suffix = crate::endpoints::DEVICE_HOST_SUFFIX;
        assert_eq!(device_host(&host, &[]), format!("desktop-abc123{suffix}"));
        assert_eq!(
            device_host(
                &host
                    .clone()
                    .with_env("SUMMRISE_HOSTNAME", &format!("chosen{suffix}")),
                &[]
            ),
            format!("chosen{suffix}")
        );
        assert_eq!(
            device_host(&host, &["--hostname".into(), format!("explicit{suffix}")]),
            format!("explicit{suffix}")
        );
    }
}
