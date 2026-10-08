//! THE PARITY INSTRUMENT.
//!
//! Prints, as JSON, the answer every pure decision gives for a FIXED corpus of inputs. The
//! TypeScript prints the same document (`parity/ts.mjs`, which requires the built `bin/summrise.js`
//! the npm suite already uses), and `parity/compare.mjs` diffs them and reports the count.
//!
//! WHY IT EXISTS: a port whose two implementations are never driven from the same input is a port
//! nobody has checked. The oracle proves the Rust answers the cases the TypeScript suite names; this
//! proves the two answer the SAME THING on inputs neither suite mentions — including the PowerShell
//! the CLI generates, compared byte for byte.
//!
//! It is a binary rather than a test so the same run can be pointed at the npm package after the
//! cutover, when the TypeScript is gone and the Rust build is the one on the device.

use serde_json::{json, Value};
use summrise_cli::components::{component_fetch_url, component_key, component_url, STAGE_ADVICE};
use summrise_cli::config::{parse_agent_port, parse_device_token, parse_target_arg};
use summrise_cli::dispatch::print_monitors_json;
use summrise_cli::host::RealHost;
use summrise_cli::monitors::{ascii_json, fmt_duration, monitors_json, probe_line, target_line};
use summrise_cli::ps::{ps_argv, psq};
use summrise_cli::psgen::{
    autostart_argv, boot_task_ps, busy_marker_ps, desk_shortcut_repair_ps, desktop_start_ps,
    desktop_task_ps, ensure_desktop_ps, firewall_ps, lnk_identity, migrate_layout_ps,
    playwright_probe_ps, start_desktop_ps, uninstall_reg_body_ps, uninstall_version_ps,
    update_busy_path, update_receipt_ps, BOOT_TASKS, DESKTOP_AUMID,
};
use summrise_cli::status::{status_report, StatusFacts};
use summrise_cli::update::{
    behind_by, busy_is_fresh, is_behind, newest_of, release_marker_verdict, rollback_version_ok,
    update_would_not_move, ReleaseMarkerCheck, Verb,
};

fn host() -> RealHost {
    RealHost::new()
}

fn main() {
    let mut cases: Vec<Value> = Vec::new();
    let mut add = |id: String, value: Value| cases.push(json!({"id": id, "value": value}));

    // ── quoting ────────────────────────────────────────────────────────────────
    for s in [
        "C:\\Program Files\\Summrise\\a'b",
        "/plain/path",
        "",
        "'",
        "a'b'c",
        "it''s",
    ] {
        add(format!("psq:{s}"), json!(psq(s)));
    }
    let script = "[$(Get-Date -Format o)] update requested 1.2.322 -> 1.2.323 \" | Out-File 'D:\\Summrise\\logs\\summrise-update.log' -Append; Write-Output \"a<b & c|d ^ e%f\"";
    add(
        "ps_argv:script".into(),
        json!(ps_argv(script).join("\u{1f}")),
    );

    // ── config parsing ─────────────────────────────────────────────────────────
    for yaml in [
        "server:\n  host: \"0.0.0.0\"\n  port: 7740\n",
        "server:\n  port: 18080\n",
        "server:\n  host: \"127.0.0.1\"\n",
        "serial:\n  port: 1234\n",
        "server:\n  port: 0\n",
        "server:\n  port: 99999\n",
        "",
        "server:\n  port: \"7740\"  # a comment\n",
        "server:\r\n  port: 8080\r\n",
        "server:\n  port:7740\n",
    ] {
        add(
            format!(
                "parse_agent_port:{}",
                yaml.replace('\n', "\\n").replace('\r', "\\r")
            ),
            json!(parse_agent_port(yaml)),
        );
    }
    for yaml in [
        "server:\n  device_token: abc.DEF-123_x\n",
        "server:\n  device_token: \"tok\"\n",
        "serial:\n  device_token: nope\n",
        "server:\n  host: x\n",
        "",
    ] {
        add(
            format!("parse_device_token:{}", yaml.replace('\n', "\\n")),
            json!(parse_device_token(yaml)),
        );
    }
    for arg in [
        "192.168.1.1:80/health",
        "192.168.1.1:80",
        "h:0",
        "h:70000",
        "no-port",
        "h:80/x:y",
        "",
        "  h:22  ",
    ] {
        let t = parse_target_arg(arg);
        add(
            format!("parse_target_arg:{arg}"),
            match t {
                Some(t) => json!({"host": t.host, "port": t.port, "path": t.path, "id": t.id}),
                None => Value::Null,
            },
        );
    }

    // ── components ─────────────────────────────────────────────────────────────
    for name in [
        "summrise-playwright.zip",
        "summrise-playwright-mcp.tgz",
        "electron-win32-x64.zip",
        "cloudflared.exe",
        "fix-tunnel.ps1",
        "",
    ] {
        add(format!("component_key:{name}"), json!(component_key(name)));
        add(
            format!("component_url:{name}"),
            json!(component_url(&host(), name)),
        );
    }
    let pins = json!({
        "cloudflared": {"url": "https://mirror.test/summrise-agent/cloudflared.exe", "sha256": "a".repeat(64)},
        "playwright": {"url": "   ", "sha256": "b".repeat(64)},
        "electron": {"sha256": "c".repeat(64)}
    });
    for name in [
        "cloudflared.exe",
        "electron-win32-x64.zip",
        "summrise-playwright.zip",
        "fix-tunnel.ps1",
    ] {
        add(
            format!("component_fetch_url:{name}"),
            json!(component_fetch_url(&host(), name, &pins)),
        );
    }
    add("stage_advice".into(), json!(STAGE_ADVICE));

    // ── durations, lines, JSON ─────────────────────────────────────────────────
    for ms in [
        0i64,
        999,
        1000,
        59_999,
        60_000,
        3_599_999,
        3_600_000,
        86_400_000,
        200_000_000,
        -5,
    ] {
        add(format!("fmt_duration:{ms}"), json!(fmt_duration(ms)));
    }
    let now = 1_789_000_000_000i64;
    let targets = [
        json!({"id": "a:22", "summary": {"up_now": true, "since_ms": now - 1000, "drops": 1}}),
        json!({"id": "a:22", "summary": {"up_now": true, "since_ms": now - 1000, "drops": 2}}),
        json!({"id": "a:22", "summary": {"up_now": true, "since_ms": now - 1000, "drops": 0}}),
        json!({"id": "h:80/", "summary": {"up_now": false, "since_ms": now - 1000, "last_status": 200, "last_expect_ok": false}}),
        json!({"id": "h:80/", "summary": {"up_now": true, "since_ms": now - 1000, "last_status": 200, "last_expect_ok": true}}),
        json!({"id": "h:80/", "summary": {"up_now": true, "since_ms": now - 1000, "last_status": 200, "last_expect_ok": null}}),
        json!({"id": "x:1", "summary": {"up_now": null, "probes": 0}}),
        json!({"id": "long-host-name.example.com:1234", "summary": {"up_now": true, "since_ms": now - 90_000, "up_pct": 99, "latency": {"avg": 9}, "drops": 3}, "note": {"text": "I rebooted it \u{2014} ok"}}),
    ];
    for (i, t) in targets.iter().enumerate() {
        add(format!("target_line:{i}"), json!(target_line(t, now, 30)));
    }
    let payload = json!({
        "ok": true,
        "interval_secs": 15,
        "targets": [
            {
                "id": "192.168.1.1:22", "host": "192.168.1.1", "port": 22, "path": null, "expect": null,
                "summary": {"probes": 12, "up": 12, "down": 0, "up_pct": 100, "up_now": true,
                    "since_ms": 111, "drops": 0, "latency": {"min": 7, "avg": 9, "max": 16},
                    "last_status": null, "last_expect_ok": null},
                "transitions": [{"at_ms": 100, "up": true, "lasted_ms": 39_000}],
                "series": [{"ts_ms": 1, "ok": true, "ms": 9}]
            },
            {
                "id": "h:80/", "host": "h", "port": 80, "path": "/", "expect": "OpenWrt",
                "summary": {"probes": 4, "up": 2, "down": 2, "up_pct": 50, "up_now": false,
                    "since_ms": 222, "drops": 1, "latency": null, "last_status": 200,
                    "last_expect_ok": false},
                "transitions": []
            }
        ]
    });
    add(
        "monitors_json:all".into(),
        json!(ascii_json(
            &monitors_json("d1", 1_789_000_000_000, Some(&payload), None),
            None
        )),
    );
    add(
        "monitors_json:one".into(),
        json!(ascii_json(
            &monitors_json("d1", 1, Some(&payload), Some("h:80/")),
            None
        )),
    );
    add(
        "print_monitors_json".into(),
        json!(print_monitors_json("d1", 1_789_000_000_000, &payload, None)),
    );
    let unknown = json!({"targets": [{"id": "x:1", "host": "x", "port": 1, "summary": {"up_now": null, "probes": 0}}]});
    add(
        "monitors_json:unknown".into(),
        json!(ascii_json(
            &monitors_json("d1", 1, Some(&unknown), None),
            None
        )),
    );
    add(
        "monitors_json:null".into(),
        json!(ascii_json(&monitors_json("d1", 1, None, None), None)),
    );
    for v in [
        json!({"id": "h:22", "text": "I rebooted it \u{2014} not a fault"}),
        json!({"a": 1, "b": true, "c": null}),
        json!({}),
        json!({"t": "\u{4e2d}\u{6587} / \u{65e5}\u{672c}\u{8a9e} / \u{e9}moji \u{1f680}"}),
        json!([1, 2.5, "x", null, true]),
        json!({"nested": {"deep": ["\u{1f600}", {"k": "\u{2014}"}]}}),
    ] {
        add(
            format!("ascii_json:{}", serde_json::to_string(&v).unwrap()),
            json!(ascii_json(&v, None)),
        );
        add(
            format!("ascii_json_pretty:{}", serde_json::to_string(&v).unwrap()),
            json!(ascii_json(&v, Some(2))),
        );
    }
    add(
        "probe_line".into(),
        json!(probe_line(
            &json!({"id": "h:80/", "expect": "OpenWrt"}),
            &json!({"ok": false, "status": 200, "expect_ok": false, "ms": 12}),
            now
        )),
    );

    // ── the update decision's facts ────────────────────────────────────────────
    for (m, n) in [
        (1_700_000_000_000i64 - 9 * 60_000, 1_700_000_000_000i64),
        (1_700_000_000_000 - 11 * 60_000, 1_700_000_000_000),
        (1_700_000_000_000, 1_700_000_000_000),
        (1_700_000_000_000 - 10 * 60_000 - 1, 1_700_000_000_000),
    ] {
        add(format!("busy_is_fresh:{m}:{n}"), json!(busy_is_fresh(m, n)));
    }
    for (a, b) in [
        ("1.2.9", "1.2.12"),
        ("1.2.9", "1.2.10"),
        ("1.1.9", "1.2.0"),
        ("garbage", "1.2.0"),
        ("1.2.0", "garbage"),
        ("1.2.3", "1.2.3"),
        ("1.2.5", "1.2.1"),
    ] {
        add(format!("behind_by:{a}:{b}"), json!(behind_by(a, b)));
        add(format!("is_behind:{a}:{b}"), json!(is_behind(a, b)));
        add(
            format!("newest_of:{a}:{b}"),
            json!(newest_of(Some(a), Some(b))),
        );
    }
    for a in ["1.2.452", "garbage", ""] {
        add(
            format!("newest_of:null:{a}"),
            json!(newest_of(None, Some(a))),
        );
        add(
            format!("newest_of:{a}:null"),
            json!(newest_of(Some(a), None)),
        );
    }
    add("newest_of:null:null".into(), json!(newest_of(None, None)));
    for (a, b) in [
        ("1.2.438", "1.2.438"),
        ("1.2.437", "1.2.438"),
        ("", "1.2.438"),
        ("1.2.438", ""),
    ] {
        add(
            format!("update_would_not_move:{a}:{b}"),
            json!(update_would_not_move(a, b)),
        );
    }
    for v in [
        "1.2.307",
        "0.0.1",
        "1.2",
        "1.2.3.4",
        "1.2.307/../../evil",
        "--clear",
        "",
    ] {
        add(
            format!("rollback_version_ok:{v}"),
            json!(rollback_version_ok(v)),
        );
    }
    add("update_busy_path".into(), json!(update_busy_path()));
    add("busy_marker_ps".into(), json!(busy_marker_ps()));
    for (dq, from, to) in [
        ("D:\\Summrise", "1.2.321", "1.2.322"),
        ("D:\\it''s\\Summrise", "1.0.0", "1.0.1"),
    ] {
        add(
            format!("update_receipt_ps:{dq}"),
            json!(update_receipt_ps(dq, from, to).join("\n")),
        );
    }

    // ── the status report's content ────────────────────────────────────────────
    let base = |agent: Option<bool>,
                release: Option<&str>,
                latest: Option<&str>,
                marker: Option<i64>| StatusFacts {
        agent_running: agent,
        install_dir: "D:\\Summrise".to_string(),
        exe_exists: true,
        port: 18080,
        release_version: release.map(String::from),
        update_marker_ms: marker,
        update_marker_unreadable: false,
        package_version: "1.2.359".to_string(),
        latest_version: latest.map(String::from),
        now_ms: 1_700_000_000_000,
    };
    let now = 1_700_000_000_000i64;
    for (id, facts) in [
        (
            "unknown-process",
            base(None, Some("1.2.359"), Some("1.2.359"), None),
        ),
        (
            "running",
            base(Some(true), Some("1.2.359"), Some("1.2.359"), None),
        ),
        (
            "stopped",
            base(Some(false), Some("1.2.359"), Some("1.2.359"), None),
        ),
        (
            "stalled",
            base(
                Some(true),
                Some("1.2.321"),
                Some("1.2.322"),
                Some(now - 27 * 60_000),
            ),
        ),
        (
            "in-flight",
            base(
                Some(true),
                Some("1.2.321"),
                Some("1.2.322"),
                Some(now - 30_000),
            ),
        ),
        ("no-marker", base(Some(false), None, Some("1.2.322"), None)),
        (
            "behind",
            base(Some(true), Some("1.2.340"), Some("1.2.345"), None),
        ),
        (
            "current",
            base(Some(true), Some("1.2.340"), Some("1.2.340"), None),
        ),
        ("cdn-silent", base(Some(true), Some("1.2.340"), None, None)),
    ] {
        add(
            format!("status_report:{id}"),
            json!(status_report(&facts).join("\n")),
        );
    }
    let mut unreadable = base(Some(true), Some("1.2.340"), Some("1.2.340"), None);
    unreadable.update_marker_unreadable = true;
    add(
        "status_report:marker-unreadable".into(),
        json!(status_report(&unreadable).join("\n")),
    );

    add(
        "release_marker_verdict:proven".into(),
        json!(verdict_json(&ReleaseMarkerCheck {
            ok: true,
            saw: Some("1.2.322".into()),
            waited_ms: 0,
        })),
    );
    add(
        "release_marker_verdict:unproven".into(),
        json!(verdict_json(&ReleaseMarkerCheck {
            ok: false,
            saw: Some("1.2.321".into()),
            waited_ms: 90_000,
        })),
    );
    add(
        "release_marker_verdict:blind".into(),
        json!(verdict_json(&ReleaseMarkerCheck {
            ok: false,
            saw: None,
            waited_ms: 90_000,
        })),
    );

    // ── the PowerShell it generates, byte for byte ─────────────────────────────
    add("desktop_aumid".into(), json!(DESKTOP_AUMID));
    add(
        "lnk_identity".into(),
        json!(
            lnk_identity("D:\\Summrise\\components\\summrise-desktop-electron\\icon.ico").0
                + "|"
                + &lnk_identity("D:\\Summrise\\components\\summrise-desktop-electron\\icon.ico").1
        ),
    );
    add(
        "desk_shortcut_repair_ps".into(),
        json!(desk_shortcut_repair_ps(
            "D:\\Summrise\\scripts",
            "D:\\Summrise\\components\\summrise-desktop-electron",
            "Write-Host"
        )
        .join("\n")),
    );
    add(
        "start_desktop_ps".into(),
        json!(start_desktop_ps(
            "D:\\Summrise\\components\\summrise-desktop-electron",
            "C:\\ProgramData\\Summrise\\logs"
        )
        .join("\n")),
    );
    add(
        "ensure_desktop_ps".into(),
        json!(
            ensure_desktop_ps("D:\\Summrise\\scripts", "C:\\ProgramData\\Summrise\\logs")
                .join("\n")
        ),
    );
    add(
        "desktop_task_ps".into(),
        json!(desktop_task_ps(
            "C:\\Program Files\\Summrise",
            "C:\\ProgramData\\Summrise\\logs"
        )
        .join("\n")),
    );
    add(
        "desktop_start_ps".into(),
        json!(desktop_start_ps("C:\\Summrise").join("\n")),
    );
    add("firewall_ps".into(), json!(firewall_ps(7740).join("\n")));
    add(
        "boot_task_ps:false".into(),
        json!(boot_task_ps(
            "C:\\V\\summrise-agent.exe",
            "C:\\V\\etc\\config.yaml",
            false
        )
        .join("\n")),
    );
    add(
        "boot_task_ps:true".into(),
        json!(
            boot_task_ps("C:\\V\\summrise-agent.exe", "C:\\V\\etc\\config.yaml", true).join("\n")
        ),
    );
    add(
        "migrate_layout_ps".into(),
        json!(migrate_layout_ps("D:\\Summrise", "C:\\ProgramData\\Summrise").join("\n")),
    );
    add(
        "uninstall_reg_body_ps".into(),
        json!(uninstall_reg_body_ps("D:\\Summrise", "1.2.307").join("\n")),
    );
    add(
        "uninstall_version_ps".into(),
        json!(uninstall_version_ps("C:\\Program Files\\Summrise", "1.2.307").join("\n")),
    );
    add(
        "uninstall_reg_body_ps:empty".into(),
        json!(uninstall_reg_body_ps("D:\\Summrise", "").join("\n")),
    );
    add(
        "playwright_probe_ps".into(),
        json!(playwright_probe_ps().join("\n")),
    );
    for t in BOOT_TASKS {
        for a in ["on", "off"] {
            add(
                format!("autostart_argv:{t}:{a}"),
                json!(autostart_argv(t, a).join(" ")),
            );
        }
    }

    println!(
        "{}",
        serde_json::to_string(&json!({"cases": cases})).unwrap()
    );
}

fn verdict_json(c: &ReleaseMarkerCheck) -> Value {
    let v = release_marker_verdict(c, "1.2.322", Verb::Rollback, None);
    json!({"writePin": v.write_pin, "exitCode": v.exit_code, "message": v.message})
}
