//! `summrise monitor` — THE DEVICE'S REACHABILITY INSTRUMENT, in the terminal the operator is
//! already working in.
//!
//! The panel draws the same numbers. This runs where the ssh session is, needs no browser, and is
//! the only way an operator can act on a watch without one. Ported from the `monitor` verb in
//! `agent/summrise-agent-npm/src/summrise.ts`; the FORMATTING it prints is
//! [`crate::monitors`], which the npm suite's oracle already pins.
//!
//! **THE DEVICE OWNS THE RULES AND THIS VERB PASSES THEM ON.** `probe` refuses a target the device
//! is not watching (one rule, one place); the CLI does not quietly add a watch to make the probe
//! succeed, it prints the refusal WITH the command that fixes it. And a device that could not be
//! reached is a NON-ZERO EXIT with the reason on stderr — never a JSON body pretending everything
//! is fine, which is the shape `--json` would otherwise invite.

use crate::config::parse_target_arg;
use crate::device::device_api;
use crate::dispatch::Outcome;
use crate::host::Host;
use crate::monitors::{ascii_json, monitors_json, probe_line, target_line};
use crate::paths::Layout;
use serde_json::{json, Value};

const USAGE_LIST: &str = "usage: summrise monitor [list [--json] | add <host:port[/path]> [--expect <text>] | probe <host:port[/path]> | rm <host:port[/path]>]";
const USAGE_ADD: &str =
    "usage: summrise monitor add <host:port[/path]>   (e.g. 192.168.1.1:80/ or 192.168.1.1:22)";
const USAGE_PROBE: &str = "usage: summrise monitor probe <host:port[/path]>";

/// `--expect <text>`, and the positional arguments with the flag AND its value removed.
///
/// The filter is the TypeScript's, transcribed: index 0 is the sub-verb and is dropped, the flag
/// itself is dropped wherever it appears, and the value at `expect_at + 1` is dropped — which is
/// what keeps `monitor add host:80 --expect "hello world"` from reading `hello world` as a target.
pub fn split_expect(args: &[String]) -> (String, Vec<String>, bool) {
    let at = args.iter().position(|a| a == "--expect");
    let expect = at
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let positional: Vec<String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| *i > 0 && *a != "--expect" && Some(*i) != at.map(|x| x + 1))
        .map(|(_, a)| a.clone())
        .collect();
    (expect, positional, at.is_some())
}

/// `summrise monitor [list [--json] | add … | probe … | rm …]`.
pub fn monitor_decision(host: &dyn Host, layout: &Layout, args: &[String]) -> Outcome {
    let sub = args
        .first()
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "list".to_string());
    let (expect, positional, expect_given) = split_expect(args);
    // AN `--expect` WITH NO TEXT IS A USAGE ERROR, not an empty content check: `--expect ""` cannot
    // mean "the body must contain nothing", because every page contains nothing.
    if expect_given && expect.is_empty() {
        return Outcome::fail(
            1,
            "usage: summrise monitor add <host:port[/path]> --expect \"<text>\"",
        );
    }

    if sub == "add" || sub == "rm" || sub == "remove" {
        let Some(t) = positional.first().and_then(|a| parse_target_arg(a)) else {
            return Outcome::fail(1, USAGE_ADD);
        };
        if sub == "add" {
            let r = device_api(
                host,
                &layout.etc_dir,
                "POST",
                "/api/monitors/add",
                Some(&json!({"host": t.host, "port": t.port, "path": t.path, "expect": expect})),
            );
            if !r.ok {
                return Outcome::fail(1, format!("monitor add: {}", r.error()));
            }
            let body = r.body.clone().unwrap_or(Value::Null);
            if body.get("ok") == Some(&Value::Bool(false)) {
                return Outcome::fail(
                    1,
                    format!(
                        "monitor add: {}",
                        body.get("error").and_then(|e| e.as_str()).unwrap_or("")
                    ),
                );
            }
            // PROBE ONCE, so the operator sees a reading instead of "no readings yet" for one 15 s
            // cycle — the same courtesy the panel gives. Its result is deliberately NOT read: the
            // watch exists now, and a first probe that failed is a reading, not a failure to add.
            let _ = device_api(
                host,
                &layout.etc_dir,
                "POST",
                "/api/monitors/probe",
                Some(&json!({"id": t.id})),
            );
            let kind = if t.path.is_empty() {
                " (TCP connect)"
            } else {
                " (HTTP GET, status code recorded)"
            };
            let want = if expect.is_empty() {
                String::new()
            } else {
                format!(" requiring the body to contain \"{expect}\"")
            };
            return Outcome::ok().say(format!("watching {}{kind}{want}", t.id));
        }
        let r = device_api(
            host,
            &layout.etc_dir,
            "POST",
            "/api/monitors/remove",
            Some(&json!({"id": t.id})),
        );
        if !r.ok {
            return Outcome::fail(1, format!("monitor rm: {}", r.error()));
        }
        let removed = r
            .body
            .as_ref()
            .and_then(|b| b.get("removed"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        return Outcome::ok().say(if removed {
            format!("stopped watching {}", t.id)
        } else {
            format!("monitor rm: {} was not being watched", t.id)
        });
    }

    if sub == "probe" {
        let Some(t) = positional.first().and_then(|a| parse_target_arg(a)) else {
            return Outcome::fail(1, USAGE_PROBE);
        };
        let r = device_api(
            host,
            &layout.etc_dir,
            "POST",
            "/api/tools/monitor_probe",
            Some(&json!({"id": t.id})),
        );
        if !r.ok {
            return Outcome::fail(1, format!("monitor probe: {}", r.error()));
        }
        let payload = r.body.clone().unwrap_or(Value::Null);
        if payload.get("ok") != Some(&Value::Bool(true)) {
            // The device refuses to probe what it is not watching, and the CLI passes that on WITH
            // the way out instead of quietly adding a watch.
            let why = payload
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("the device refused the probe");
            return Outcome {
                exit: 1,
                out: Vec::new(),
                err: vec![
                    format!("monitor probe: {why}"),
                    format!("  to watch it: summrise monitor add {}", t.id),
                ],
            };
        }
        let result = payload.get("result").cloned().unwrap_or(Value::Null);
        let probe = result.get("probe").cloned().unwrap_or(Value::Null);
        // The device sends the CRITERION with the answer (`expect`), so "no match" can name the text
        // it looked for — an unreadable verdict is a verdict nobody can act on.
        let target = json!({
            "id": t.id,
            "expect": result.get("expect").cloned().unwrap_or(Value::Null),
        });
        return Outcome::ok().say(probe_line(&target, &probe, host.now_ms()));
    }

    if sub != "list" {
        return Outcome::fail(1, USAGE_LIST);
    }

    let r = device_api(host, &layout.etc_dir, "GET", "/api/monitors", None);
    if !r.ok {
        return Outcome::fail(1, format!("monitor: {}", r.error()));
    }
    let body = r.body.clone().unwrap_or(Value::Null);
    if args.iter().any(|a| a == "--json") {
        // ASCII-escaped for the same reason the request body is: a PIPE is an encoding boundary
        // too. PowerShell decodes a child's output with the console code page (CP936 on d1), so raw
        // UTF-8 through a pipe came back as mojibake while the same text printed directly read fine.
        let projected = monitors_json(&host.hostname(), host.now_ms(), Some(&body), None);
        return Outcome::ok().say(ascii_json(&projected, Some(2)));
    }
    let targets = body
        .get("targets")
        .and_then(|t| t.as_array())
        .cloned()
        .unwrap_or_default();
    if targets.is_empty() {
        return Outcome::ok()
            .say("watching nothing. `summrise monitor add 192.168.1.1:22` starts one;")
            .say("add a path to check a web UI instead of a port: `summrise monitor add 192.168.1.1:80/`");
    }
    let interval = body
        .get("interval_secs")
        .and_then(|v| v.as_i64())
        .filter(|v| *v != 0)
        .unwrap_or(15);
    let now = host.now_ms();
    let mut out = Outcome::ok().say(format!(
        "watching {} target(s), probed every {interval}s:",
        targets.len()
    ));
    for t in &targets {
        out = out.say(target_line(t, now, 30));
    }
    out
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

    fn host() -> FakeHost {
        FakeHost::new().with_file(
            &config_path("D:\\Summrise\\etc").to_string_lossy(),
            "server:\n  port: 18080\n  device_token: tok.A\n",
        )
    }

    fn answers(bodies: &[&str]) -> Vec<RunResult> {
        bodies
            .iter()
            .map(|b| RunResult {
                status: Some(0),
                stdout: (*b).to_string(),
                ..Default::default()
            })
            .collect()
    }

    /// THE FLAG'S VALUE IS NOT A TARGET. This is the whole reason the filter is a filter and not an
    /// `args[1]`: `monitor add host:80 --expect "500"` must not read `500` as a target id.
    #[test]
    fn the_expect_value_is_never_read_as_a_target() {
        let args: Vec<String> = ["add", "192.168.1.1:80/", "--expect", "hello world"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (expect, positional, given) = split_expect(&args);
        assert_eq!(expect, "hello world");
        assert_eq!(positional, vec!["192.168.1.1:80/".to_string()]);
        assert!(given);

        // ...and a flag in the FIRST position after the sub-verb still leaves nothing positional.
        let args: Vec<String> = ["add", "--expect", "x"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (_, positional, _) = split_expect(&args);
        assert!(positional.is_empty());
    }

    /// An `--expect` with no text is refused: an empty content check cannot fail.
    #[test]
    fn an_empty_expect_is_a_usage_error() {
        let args: Vec<String> = ["add", "1.2.3.4:80", "--expect"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let r = monitor_decision(&host(), &layout(), &args);
        assert_eq!(r.exit, 1);
        assert!(r.err[0].contains("--expect"), "{:?}", r.err);
    }

    /// A device that did not answer is a NON-ZERO EXIT with the reason — never a JSON body that
    /// looks like a successful empty watch list.
    #[test]
    fn an_unreachable_device_is_a_failure_not_an_empty_list() {
        let h = host().script_runs(vec![RunResult {
            status: Some(7),
            ..Default::default()
        }]);
        let r = monitor_decision(&h, &layout(), &["list".into()]);
        assert_eq!(r.exit, 1);
        assert!(
            r.err[0].contains("monitor: device unreachable"),
            "{:?}",
            r.err
        );

        let h = host().script_runs(vec![RunResult {
            status: Some(7),
            ..Default::default()
        }]);
        let r = monitor_decision(&h, &layout(), &["list".into(), "--json".into()]);
        assert_eq!(r.exit, 1, "--json must not turn a failure into a document");
        assert!(r.out.is_empty(), "{:?}", r.out);
    }

    /// `add` probes ONCE so the operator sees a reading instead of "no readings yet" — and a FAILED
    /// first probe does not undo the watch that was just created.
    #[test]
    fn add_probes_once_and_reports_the_watch_it_created() {
        let h = host().script_runs(answers(&[
            "{\"ok\":true}",
            "{\"ok\":false,\"error\":\"offline\"}",
        ]));
        let args: Vec<String> = ["add", "192.168.1.1:80/", "--expect", "Summrise"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let r = monitor_decision(&h, &layout(), &args);
        assert_eq!(r.exit, 0, "{r:?}");
        assert_eq!(
            r.out[0],
            "watching 192.168.1.1:80/ (HTTP GET, status code recorded) requiring the body to contain \"Summrise\""
        );
        let runs = h.runs();
        assert_eq!(runs.len(), 2, "the probe must follow the add");
        // THE ENDPOINT IS THE TAIL OF THE URL, because `device_api` hands curl ONE argv element for
        // the whole address — the same shape the TypeScript builds (`args.push(\`http://127.0.0.1:
        // ${port}${pathname}\`)`). Asserting an element EQUAL to the path asserted a request shape
        // the CLI has never made; the assertion that means something is that the SECOND request went
        // to the probe endpoint rather than repeating the add.
        assert!(
            runs[1].iter().any(|a| a.ends_with("/api/monitors/probe")),
            "{:?}",
            runs[1]
        );
        assert!(
            !runs[1].iter().any(|a| a.ends_with("/api/monitors/add")),
            "the probe must not repeat the add: {:?}",
            runs[1]
        );
    }

    /// A device that REFUSES the add is reported with the device's own reason, and nothing is
    /// probed afterwards.
    #[test]
    fn a_refused_add_does_not_probe() {
        let h = host().script_runs(answers(&["{\"ok\":false,\"error\":\"port out of range\"}"]));
        let args: Vec<String> = ["add", "1.2.3.4:70000"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // 70000 is not a valid port, so this is caught before any HTTP at all.
        let r = monitor_decision(&h, &layout(), &args);
        assert_eq!(r.exit, 1);
        assert!(h.runs().is_empty(), "no request for an unparseable target");

        let h = host().script_runs(answers(&["{\"ok\":false,\"error\":\"port out of range\"}"]));
        let args: Vec<String> = ["add", "1.2.3.4:22"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let r = monitor_decision(&h, &layout(), &args);
        assert_eq!(r.exit, 1);
        assert!(r.err[0].contains("port out of range"), "{:?}", r.err);
        assert_eq!(h.runs().len(), 1);
    }

    /// `probe` on a target nobody watches carries the way OUT with the refusal, rather than adding
    /// a watch behind the operator's back.
    #[test]
    fn a_refused_probe_carries_the_command_that_fixes_it() {
        let h = host().script_runs(answers(&["{\"ok\":false,\"error\":\"not watched\"}"]));
        let args: Vec<String> = ["probe", "10.0.0.5:443"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let r = monitor_decision(&h, &layout(), &args);
        assert_eq!(r.exit, 1);
        assert!(r.err[0].contains("not watched"), "{:?}", r.err);
        assert_eq!(r.err[1], "  to watch it: summrise monitor add 10.0.0.5:443");
    }

    /// `rm` distinguishes "removed" from "was not being watched" from the DEVICE's answer — not
    /// from the exit code, which cannot tell them apart.
    #[test]
    fn rm_reports_what_the_device_said_it_removed() {
        let h = host().script_runs(answers(&["{\"removed\":true}"]));
        let args: Vec<String> = ["rm", "1.2.3.4:22"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            monitor_decision(&h, &layout(), &args).out[0],
            "stopped watching 1.2.3.4:22"
        );

        let h = host().script_runs(answers(&["{\"removed\":false}"]));
        assert_eq!(
            monitor_decision(&h, &layout(), &args).out[0],
            "monitor rm: 1.2.3.4:22 was not being watched"
        );
        // `remove` is the same sub-verb.
        let args: Vec<String> = ["remove", "1.2.3.4:22"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let h = host().script_runs(answers(&["{\"removed\":true}"]));
        assert_eq!(
            monitor_decision(&h, &layout(), &args).out[0],
            "stopped watching 1.2.3.4:22"
        );
    }

    /// The `--json` document carries the device's numbers plus only what the CLI knows, and it goes
    /// through the ASCII escaper.
    #[test]
    fn the_json_listing_is_the_projection_not_the_devices_bytes() {
        let h = host().script_runs(answers(&[
            "{\"interval_secs\":15,\"targets\":[{\"id\":\"1.2.3.4:22\",\"summary\":{\"up\":true}}]}",
        ]));
        let r = monitor_decision(&h, &layout(), &["list".into(), "--json".into()]);
        assert_eq!(r.exit, 0);
        let v: Value = serde_json::from_str(&r.out[0]).unwrap();
        assert!(v["asked_at_ms"].is_number(), "{v}");
        assert_eq!(v["targets"][0]["id"], json!("1.2.3.4:22"));
    }

    /// An unknown sub-verb prints the full usage and exits 1.
    #[test]
    fn an_unknown_sub_verb_prints_the_usage() {
        let r = monitor_decision(&host(), &layout(), &["watch".into()]);
        assert_eq!(r.exit, 1);
        assert!(r.err[0].contains("summrise monitor [list"), "{:?}", r.err);
    }
}
