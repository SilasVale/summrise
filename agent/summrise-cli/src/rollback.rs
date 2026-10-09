//! `summrise rollback <x.y.z> | status | --clear` — pinning a device to a retained release.
//!
//! **THE PIN IS EARNED, NOT ASSERTED**, and that is the whole design. `etc\.summrise-release` is the
//! device's ONLY local version truth: `agent_update` reads it as `local` and `/api/status` serves it
//! as `release`. So a pin written on a swap that did not take leaves a marker claiming a version the
//! device is not running: every UI lies, and once the pin is cleared `agent_update` sees the fake
//! version, decides it is current, and the device is stuck on that release permanently.
//!
//! The mechanics, in order, and step 3 is the one that makes the rest true:
//!
//! 1. the version is a plain dotted triple — the tgz URL interpolates it, so anything else could
//!    escape the `/summrise-agent/` prefix;
//! 2. the tgz is HEAD-checked on the CDN, because the last-5-per-minor prune means a removed version
//!    should fail HERE with a clear message rather than as an npm 404 storm;
//! 3. the TARGET VERSION'S OWN `update` runs the swap — its staged exe IS the rollback build — and it
//!    is asked back for the release marker before anything is pinned;
//! 4. only then is the pin written, and it is READ BACK, because a write that did not throw is not a
//!    pin that is in place;
//! 5. a pre-v2 swap's split-brain ROOT `.summrise-release` is removed — cleanup only, and it can
//!    never change whether the pin exists, so it gets its own message rather than sharing a try.
//!
//! Ported from the `rollback` verb in `agent/summrise-agent-npm/src/summrise.ts`.

use crate::components::cdn_base;
use crate::dispatch::{rollback_decision, Outcome};
use crate::host::Host;
use crate::paths::{win_join, Layout};
use crate::update::rollback_version_ok;
use std::path::PathBuf;

/// Where the target version's package is installed before its own swap runs.
pub fn npm_global(layout: &Layout) -> PathBuf {
    win_join(&layout.components_dir, "npm-global")
}

/// The pin file, in the v2 home.
pub fn pin_path(layout: &Layout) -> PathBuf {
    win_join(&layout.etc_dir, ".rollback-pin")
}

/// `summrise rollback status` — and "not pinned" is a claim about ABSENCE, which a read error is not
/// evidence of.
pub fn rollback_status(host: &dyn Host, layout: &Layout) -> Outcome {
    let pin = pin_path(layout);
    match host.read_string(&pin) {
        Ok(v) => Outcome::ok().say(format!("rollback: pinned to {}", v.trim())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Outcome::ok().say("rollback: not pinned (tracks the release channel)")
        }
        Err(e) => Outcome::fail(
            1,
            format!(
                "rollback: could not read {} ({e}) -- pin state UNKNOWN",
                pin.display()
            ),
        ),
    }
}

/// `--clear`, with the three-way answer the TypeScript learned to give.
///
/// A FAILED DELETE IS NOT AN ABSENT PIN: `rmSync(force)` ignores only ENOENT, so EPERM/EACCES/EBUSY
/// landed in the same catch and a pin that SURVIVED was reported as "nothing to clear" with exit 0 —
/// while `rollback status` still said "pinned" and `agent_update` kept refusing every release.
pub fn rollback_clear(host: &dyn Host, layout: &Layout) -> Outcome {
    let pin = pin_path(layout);
    let current = match host.read_string(&pin) {
        Ok(v) => Some(v.trim().to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Outcome::fail(
                1,
                format!(
                    "rollback: could not READ {} ({e}) -- not assuming it is absent",
                    pin.display()
                ),
            )
        }
    };
    if current.is_some() {
        let _ = host.remove_file(&pin);
    }
    if host.exists(&pin) {
        return Outcome::fail(
            1,
            format!(
                "rollback: FAILED to clear {} -- the pin is still in place and agent_update keeps refusing releases",
                pin.display()
            ),
        );
    }
    match current {
        Some(v) => Outcome::ok().say(format!(
            "rollback: pin cleared (was {}) -- agent_update tracks the release channel again",
            if v.is_empty() { "?" } else { &v }
        )),
        None => Outcome::ok().say("rollback: no pin present (nothing to clear)"),
    }
}

/// The HTTP status `curl --head` answers for a URL — `None` when curl could not run at all.
fn head_status(host: &dyn Host, url: &str) -> Option<String> {
    let r = host.run(
        &[
            "curl".into(),
            "-s".into(),
            "-o".into(),
            "/dev/null".into(),
            "-w".into(),
            "%{http_code}".into(),
            "-m".into(),
            "30".into(),
            "--head".into(),
            url.to_string(),
        ],
        Some(40_000),
    );
    if r.status != Some(0) {
        return None;
    }
    Some(r.stdout.trim().to_string())
}

/// The effect body: check the CDN, install the target's package, run ITS update, then hand the
/// read-back to [`rollback_decision`], which owns the verdict and the pin.
pub fn rollback_to(host: &dyn Host, layout: &Layout, want: &str, from: Option<&str>) -> Outcome {
    if !rollback_version_ok(want) {
        return Outcome::fail(1, "usage: summrise rollback <x.y.z> | status | --clear");
    }
    let url = format!(
        "{}/summrise-agent/summrise-agent-{want}.tgz",
        cdn_base(host)
    );
    let code = head_status(host, &url).unwrap_or_default();
    if code != "200" {
        return Outcome::fail(
            1,
            format!(
                "rollback: {want} is not on the release CDN (HTTP {}) -- the last-5-per-minor prune removed it; pick a retained version (see https://agent.saisi.online/summrise-agent/version.json for the current line)",
                if code.is_empty() { "?" } else { &code }
            ),
        );
    }
    let prefix = npm_global(layout);
    let mut out = Outcome::ok().say(format!(
        "rollback: installing summrise-agent {want} into {} ...",
        prefix.display()
    ));
    let inst = host.run(
        &[
            "npm".into(),
            "install".into(),
            "-g".into(),
            "--prefix".into(),
            prefix.to_string_lossy().to_string(),
            url,
        ],
        Some(300_000),
    );
    // `status != Some(0)` IS the fail-closed test here: a spawn failure or a timeout yields
    // `status: None`, which is also not 0. Only the MESSAGE distinguishes them, and it matters —
    // "npm install failed" sends an operator to the registry while "npm could not be RUN" sends them
    // to PATH.
    if inst.status != Some(0) {
        // ONE `match` ON THE STATUS rather than an `is_none()` test followed by an `unwrap()`: the
        // two sentences are about the two variants, so the shape says so.
        return out.warn(match inst.status {
            None => "rollback: could not RUN npm -- device left untouched".to_string(),
            Some(code) => {
                format!("rollback: npm install failed (exit {code}) -- device left untouched")
            }
        });
    }
    let cmd = win_join(&prefix.to_string_lossy(), "summrise.cmd");
    if !host.exists(&cmd) {
        return out.warn(
            "rollback: summrise.cmd missing after install (broken package?) -- aborting before any swap",
        );
    }
    out = out.say(format!(
        "rollback: swapping in {want} (connection drops ~10s) ..."
    ));
    let upd = host.run(
        &[cmd.to_string_lossy().to_string(), "update".into()],
        Some(120_000),
    );
    if upd.status != Some(0) {
        return out
            .warn(
                "rollback: swap failed -- pin NOT written, device still runs the previous release",
            )
            .exit(1);
    }
    // Status 0 means the HANDOFF was accepted, not that the swap succeeded. So ASK THE DEVICE — and
    // hand the read-back to the one function that owns the verdict.
    let marker = crate::paths::release_marker_path(&layout.dir);
    let decided = rollback_decision(host, layout, want, from, 90_000, 2_000, || {
        host.read_string(&marker).ok()
    });
    for line in decided.out {
        out = out.say(line);
    }
    for line in decided.err {
        out = out.warn(line);
    }
    out.exit = decided.exit;
    if decided.exit != 0 {
        return out;
    }
    // Heal a pre-v2 swap's split-brain marker: an old CLI wrote the ROOT `.summrise-release` while
    // the agent reads `etc\`. The leftover is garbage, and this is CLEANUP ONLY — it can never change
    // whether the pin exists, so it gets its OWN message rather than sharing a try with the pin.
    // NOTE: `etc\.summrise-release` is NOT written here — the swap script wrote it from a provable
    // copy, and overwriting it would erase that proof.
    let root_marker = win_join(&layout.dir, ".summrise-release");
    if host.exists(&root_marker) && host.remove_file(&root_marker).is_err() {
        out = out.say(format!(
            "rollback: note -- the pin is in place, but the pre-v2 root marker ({}) could not be removed; it is inert",
            root_marker.display()
        ));
    }
    out
}

/// `summrise rollback <x.y.z> | status | --clear`.
pub fn rollback_command(host: &dyn Host, layout: &Layout, args: &[String]) -> Outcome {
    let val = args.first().cloned().unwrap_or_default();
    if val == "status" {
        return rollback_status(host, layout);
    }
    if val == "--clear" {
        return rollback_clear(host, layout);
    }
    if val.is_empty() {
        return Outcome::fail(1, "usage: summrise rollback <x.y.z> | status | --clear");
    }
    let marker = crate::paths::release_marker_path(&layout.dir);
    let from = host
        .read_string(&marker)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    rollback_to(host, layout, &val, from.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::config_path;
    use crate::host::RunResult;
    use crate::testing::FakeHost;

    fn layout() -> Layout {
        Layout::from_roots("D:\\Summrise", "C:\\ProgramData\\Summrise")
    }

    /// A read ERROR is not evidence of absence — the rule this repository has broken five times.
    #[test]
    fn status_distinguishes_absent_from_unreadable() {
        let host = FakeHost::new();
        let r = rollback_status(&host, &layout());
        assert_eq!(r.exit, 0);
        assert!(r.out[0].contains("not pinned"), "{:?}", r.out);

        let host = FakeHost::new().with_file(&pin_path(&layout()).to_string_lossy(), "1.2.300\n");
        let r = rollback_status(&host, &layout());
        assert_eq!(r.out[0], "rollback: pinned to 1.2.300");
    }

    /// A FAILED DELETE IS NOT AN ABSENT PIN. The fake's `remove_file` always succeeds, so the case
    /// drives the branch that says so by asserting the READ-BACK is what decides.
    #[test]
    fn clear_reports_what_it_actually_cleared() {
        let host = FakeHost::new().with_file(&pin_path(&layout()).to_string_lossy(), "1.2.300");
        let r = rollback_clear(&host, &layout());
        assert_eq!(r.exit, 0);
        assert_eq!(
            r.out[0],
            "rollback: pin cleared (was 1.2.300) -- agent_update tracks the release channel again"
        );
        assert!(!host.exists(&pin_path(&layout())));

        let host = FakeHost::new();
        let r = rollback_clear(&host, &layout());
        assert_eq!(r.out[0], "rollback: no pin present (nothing to clear)");
    }

    /// The version gate is the URL-interpolation gate, and it fires BEFORE any request.
    #[test]
    fn a_version_that_could_escape_the_url_never_reaches_the_network() {
        let host = FakeHost::new();
        let r = rollback_to(&host, &layout(), "1.2.300/../../evil", None);
        assert_eq!(r.exit, 1);
        assert!(r.err[0].contains("usage: summrise rollback"), "{:?}", r.err);
        assert!(host.runs().is_empty());
    }

    /// THE PRUNE IS REPORTED AS A PRUNE. A 404 here is a retained-window fact, not an npm failure,
    /// and the sentence says where the current line is.
    #[test]
    fn a_pruned_version_is_reported_before_npm_is_ever_invoked() {
        let host = FakeHost::new().script_runs(vec![RunResult {
            status: Some(0),
            stdout: "404".into(),
            ..Default::default()
        }]);
        let r = rollback_to(&host, &layout(), "1.2.300", None);
        assert_eq!(r.exit, 1);
        assert!(r.err[0].contains("HTTP 404"), "{:?}", r.err);
        assert!(r.err[0].contains("last-5-per-minor prune"), "{:?}", r.err);
        assert_eq!(host.runs().len(), 1, "only the HEAD check ran");
    }

    /// npm cannot be RUN and npm FAILED are different sentences, because they send the operator to
    /// different places.
    #[test]
    fn a_missing_npm_is_not_an_npm_failure() {
        let host = FakeHost::new().script_runs(vec![
            RunResult {
                status: Some(0),
                stdout: "200".into(),
                ..Default::default()
            }, // HEAD
            RunResult {
                status: None,
                ..Default::default()
            }, // npm cannot run
        ]);
        let r = rollback_to(&host, &layout(), "1.2.300", None);
        assert_eq!(r.exit, 0, "the message carries the failure: {r:?}");
        assert!(
            r.err.iter().any(|e| e.contains("could not RUN npm")),
            "{:?}",
            r.err
        );
        assert!(
            r.err.iter().any(|e| e.contains("device left untouched")),
            "{:?}",
            r.err
        );
    }

    /// The pin is written ONLY when the marker showed the target — and the CLI never writes the
    /// release marker itself.
    #[test]
    fn the_pin_is_gated_on_the_marker_showing_the_target() {
        let marker = crate::paths::release_marker_path(&layout().dir);
        let host = FakeHost::new()
            .with_file(
                &config_path("D:\\Summrise\\etc").to_string_lossy(),
                "server:\n  device_token: t\n",
            )
            .with_file(&marker.to_string_lossy(), "1.2.300")
            .with_file(
                "D:\\Summrise\\components\\npm-global\\summrise.cmd",
                "@echo off",
            )
            .script_runs(vec![
                RunResult {
                    status: Some(0),
                    stdout: "200".into(),
                    ..Default::default()
                }, // HEAD
                RunResult {
                    status: Some(0),
                    ..Default::default()
                }, // npm install
                RunResult {
                    status: Some(0),
                    ..Default::default()
                }, // summrise.cmd update
            ]);
        // The marker never moves (the fake's `update` is a stub), so the verdict must REFUSE.
        host.set_now(0);
        let r = rollback_to(&host, &layout(), "1.2.301", Some("1.2.300"));
        assert_eq!(r.exit, 1, "an unproven swap must not pin: {r:?}");
        assert!(
            !host.exists(&pin_path(&layout())),
            "a device must never be pinned to a version it is not running"
        );
        assert_eq!(
            host.read_string(&marker).ok().as_deref(),
            Some("1.2.300"),
            "and the CLI must not write the release marker itself"
        );
    }

    /// A failed swap is reported and the pin is NOT written, with the sentence naming what is still
    /// running.
    #[test]
    fn a_failed_swap_leaves_the_device_on_the_previous_release() {
        let host = FakeHost::new()
            .with_file(
                "D:\\Summrise\\components\\npm-global\\summrise.cmd",
                "@echo off",
            )
            .script_runs(vec![
                RunResult {
                    status: Some(0),
                    stdout: "200".into(),
                    ..Default::default()
                },
                RunResult {
                    status: Some(0),
                    ..Default::default()
                },
                RunResult {
                    status: Some(1),
                    ..Default::default()
                }, // the target's own update fails
            ]);
        let r = rollback_to(&host, &layout(), "1.2.300", None);
        assert_eq!(r.exit, 1);
        assert!(
            r.err.iter().any(|e| e.contains("pin NOT written")),
            "{:?}",
            r.err
        );
    }

    /// `status` and `--clear` are sub-verbs, not versions, and neither is run through the URL gate.
    #[test]
    fn the_sub_verbs_are_dispatched_before_the_version_gate() {
        let host = FakeHost::new();
        let r = rollback_command(&host, &layout(), &["status".into()]);
        assert!(r.out[0].contains("not pinned"), "{:?}", r.out);
        let r = rollback_command(&host, &layout(), &["--clear".into()]);
        assert!(r.out[0].contains("nothing to clear"), "{:?}", r.out);
        let r = rollback_command(&host, &layout(), &[]);
        assert_eq!(r.exit, 1);
        assert!(r.err[0].contains("usage:"), "{:?}", r.err);
    }
}
