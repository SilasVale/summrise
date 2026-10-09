//! THE PORTED CASES.
//!
//! **WHERE THEY COME FROM, EXACTLY.** Two JavaScript files carried the shell's decision tests and both
//! SURVIVE this landing, because what remains in them is not a decision:
//!
//! * `agent/summrise-desktop-electron/test/ipc-door.test.mjs` (145 lines, 3 cases) — two of its cases are
//!   pure CALL-SITE COUNTS over `main.ts`'s own text (`ipcMain.handle` exactly once, `sanitizeBrowserUrl`
//!   exactly once, `loadTarget` used at least four times) and one asserts the channels the preload
//!   invokes equal the channels the door registers. Those are properties of the ELECTRON HOST: they can
//!   only be checked by reading `main.ts`, they compute nothing, and no Rust can hold them.
//! * `agent/summrise-desktop-electron/test/embedded-bridge.test.mjs` (73 lines, 2 cases) — the preload's
//!   exposed member names against the panel's fixture, and that three subscribing members return an
//!   unsubscribe function. Same class: `contextBridge` name and shape pins on the host's side of an
//!   Electron boundary.
//!
//! ONE ASSERTION IN THE FIRST FILE WAS A DECISION and it is here: the refusal a forbidden frame gets,
//! `{ ok: false, error: "forbidden frame" }`, which the SPA reads and which used to have TWO shapes —
//! see [`crate::boot::forbidden_frame_json`]. The `.mjs` case keeps its counts and no longer restates the
//! string.
//!
//! **EVERYTHING ELSE IN THIS FILE IS NEW, AND IT IS THE POINT OF THE LANDING.** The shell's decisions had
//! NO tests at all: `main.ts` imports `electron`, which this repository deliberately does not install in
//! the shell's package, so `node --test` could not import it and a source grep was the only instrument.
//! The 58 decision sites the classification found are now ordinary `#[test]`s over pure functions —
//! including the ones whose only previous record was a comment explaining an incident.

use crate::*;
use serde_json::Value;

// ── boot ────────────────────────────────────────────────────────────────────────────────────────────

#[test]
fn the_device_token_pattern_is_the_narrow_one_main_ts_documented() {
    // The 16-character floor is the pattern's, not the device's: a live token is 64 hex characters.
    assert_eq!(
        device_token("device_token: 0123456789abcdef").as_deref(),
        Some("0123456789abcdef")
    );
    assert_eq!(
        device_token("other: 1\ndevice_token: 0123456789abcdef\n").as_deref(),
        Some("0123456789abcdef")
    );
    // The quotes are optional and are not part of the capture.
    assert_eq!(
        device_token(r#"device_token: "0123456789abcdef""#).as_deref(),
        Some("0123456789abcdef")
    );
    // TOO SHORT IS NOT A MATCH, and the engine then looks FURTHER RIGHT — a later line can supply it.
    assert_eq!(device_token("device_token: abcdef").as_deref(), None);
    assert_eq!(
        device_token("device_token: abcdef\ndevice_token: fedcba9876543210").as_deref(),
        Some("fedcba9876543210")
    );
    // UPPERCASE HEX IS NOT THIS PATTERN. `main.ts` records that the CLI's and the agent's parsers accept
    // more than lowercase hex and that all three agree today "because hex is what the worker issues" —
    // and that the failure mode when they stop agreeing is SILENT. This pins which side of that
    // divergence this crate is on.
    assert_eq!(
        device_token("device_token: 0123456789ABCDEF").as_deref(),
        None
    );
    // `\s` is JAVASCRIPT's set: U+00A0 is whitespace, U+0085 is not (the difference `js::JS_WS` names).
    assert_eq!(
        device_token("device_token:\u{00A0}0123456789abcdef").as_deref(),
        Some("0123456789abcdef")
    );
    assert_eq!(
        device_token("device_token:\u{0085}0123456789abcdef").as_deref(),
        None
    );
    assert_eq!(device_token("").as_deref(), None);
}

#[test]
fn the_token_cache_is_a_sixty_second_window_and_starts_cold() {
    // `_tokenCache` starts at `{ at: 0, tok: null }` and the clock starts at ~1.76e12, so the difference
    // is far past the window and the first call always READS rather than trusting the null it holds.
    assert!(
        !token_cache_fresh(0.0, 1_759_123_456_789.0),
        "the cold sentinel `{{ at: 0 }}` must never look fresh against a real clock"
    );
    assert!(token_cache_fresh(1_000.0, 1_000.0));
    assert!(token_cache_fresh(1_000.0, 60_999.0));
    assert!(
        !token_cache_fresh(1_000.0, 61_000.0),
        "the window is exclusive at 60 s"
    );
}

#[test]
fn authorization_is_absent_for_a_missing_or_empty_token() {
    assert_eq!(authorization(Some("abc")).as_deref(), Some("Bearer abc"));
    assert_eq!(
        authorization(Some("")),
        None,
        "`t ? … : {{}}` — an empty token sends no header"
    );
    assert_eq!(authorization(None), None);
}

#[test]
fn resolve_agent_port_prefers_the_environment_then_the_devices_own_config() {
    // The environment wins.
    assert_eq!(resolve_agent_port(Some(7740.0), Some(9000)), 7740);
    // `Number(undefined)` is NaN, which is not an integer — so a missing variable falls through.
    assert_eq!(resolve_agent_port(Some(f64::NAN), Some(9000)), 9000);
    // ...and so do 0, a non-integral value and anything out of range.
    assert_eq!(resolve_agent_port(Some(0.0), Some(9000)), 9000);
    assert_eq!(resolve_agent_port(Some(8080.5), Some(9000)), 9000);
    assert_eq!(resolve_agent_port(Some(70_000.0), None), DEFAULT_AGENT_PORT);
    assert_eq!(resolve_agent_port(None, Some(7740)), 7740);
    // `parseAgentPort("serial:\n  port: 1234\n")` answers undefined — another section's port is not the
    // agent's — and the walk that decides that is the url-policy crate's own case. What this pins is that
    // an ABSENT config answer is the DEFAULT rather than a zero.
    assert_eq!(resolve_agent_port(None, None), DEFAULT_AGENT_PORT);
}

#[test]
fn resolve_dsh_port_has_no_config_fallback_and_that_is_the_original_behaviour() {
    assert_eq!(resolve_dsh_port(Some(19000.0)), 19000);
    assert_eq!(resolve_dsh_port(Some(f64::NAN)), DEFAULT_DSH_PORT);
    assert_eq!(resolve_dsh_port(Some(0.0)), DEFAULT_DSH_PORT);
    assert_eq!(resolve_dsh_port(None), DEFAULT_DSH_PORT);
}

#[test]
fn the_tray_icon_is_an_ico_on_windows_and_a_png_everywhere_else() {
    assert_eq!(tray_icon_name("win32"), "icon.ico");
    assert_eq!(tray_icon_name("darwin"), "icon.png");
    assert_eq!(tray_icon_name("linux"), "icon.png");
    // The WINDOW takes a PNG on every platform, Windows included — Chromium's ICO parser has choked on
    // PNG-compressed 256px entries and silently fallen back to the stock electron.exe icon.
    assert_eq!(window_icon_name(), "icon.png");
}

#[test]
fn the_aumid_is_the_string_the_cli_also_writes() {
    assert_eq!(DESKTOP_AUMID, "online.saisi.summrise.desktop");
    assert!(uses_app_user_model_id("win32"));
    assert!(!uses_app_user_model_id("darwin"));
    assert_eq!(aumid_report("win32"), DESKTOP_AUMID);
    assert_eq!(aumid_report("linux"), "(non-windows)");
    assert_eq!(AUMID_SET_FAILED, "(set-failed)");
}

#[test]
fn the_host_label_is_the_base_origin_without_its_scheme() {
    assert_eq!(
        agent_host_label("http://127.0.0.1:18080"),
        "127.0.0.1:18080"
    );
    assert_eq!(agent_host_label("http://127.0.0.1:7740"), "127.0.0.1:7740");
    // `replace` with a string pattern takes the FIRST occurrence, and a base with no scheme is unchanged.
    assert_eq!(agent_host_label("127.0.0.1:18080"), "127.0.0.1:18080");
}

#[test]
fn the_forbidden_frame_refusal_is_stated_once_and_carries_its_reason() {
    // THE ASSERTION THAT MOVED OUT OF `test/ipc-door.test.mjs`: the SPA reads `j?.ok`, so the bare
    // `{ ok: false }` the other six handlers used to answer was indistinguishable from a dead view.
    let refusal: Value =
        serde_json::from_str(&forbidden_frame_json()).expect("the refusal is JSON");
    assert_eq!(refusal["ok"], Value::Bool(false));
    assert_eq!(
        refusal["error"],
        Value::String("forbidden frame".to_string())
    );
    assert_eq!(
        refusal,
        serde_json::json!({ "ok": false, "error": FORBIDDEN_FRAME_ERROR })
    );
}

#[test]
fn shell_constants_carry_the_policy_numbers_the_shell_was_writing_inline() {
    let constants: Value =
        serde_json::from_str(&shell_constants()).expect("the constants are JSON");
    // 9333 is the product contract `agent/tests/mcp_autoselect_integration.rs` binds.
    assert_eq!(constants["CDP_PORT"], 9333);
    assert_eq!(constants["CTRL_PORT"], 9444);
    assert_eq!(constants["MAX_BROWSER_WINDOWS"], 8);
    assert_eq!(constants["AUTOSTART_TASK"], "SummriseDesktop");
    assert_eq!(constants["AGENT_TASK"], "SummriseAgent");
    assert_eq!(constants["AUTO_START_AFTER_MISSES"], 5.0);
    assert_eq!(constants["AUTO_START_MIN_GAP_MS"], 300_000.0);
    assert_eq!(constants["RETRY_START_MS"], 2_000.0);
    assert_eq!(constants["RETRY_CAP_MS"], 30_000.0);
    assert_eq!(constants["CDP_UA_MARKER"], "summrise-desktop-electron");
    assert_eq!(constants["ABOUT_BLANK"], "about:blank");
    assert_eq!(constants["FORBIDDEN_ORIGIN"], "forbidden origin");
    assert_eq!(constants["NOT_FOUND"], "not found");
    // Every key the shell destructures must be present — a missing one would be `undefined` in
    // `main.js` and would reach a comparison as a silent no-match rather than an error.
    for key in [
        "MIN_SLOT_PX",
        "SCHTASKS_TIMEOUT_MS",
        "TRIPWIRE_BACKOFF_MS",
        "LOG_URL_UNITS",
        "CDP_UA_UNITS",
        "AGENT_PROBE_BOOT_MS",
        "AGENT_PROBE_TRAY_MS",
        "AGENT_PROBE_DEFAULT_MS",
        "STATUS_FETCH_MS",
        "HARNESS_FETCH_MS",
        "CDP_CHECK_MS",
        "MENU_FLUSH_MS",
        "TRAY_POLL_MS",
        "CORS_ALLOW_METHODS",
        "CORS_ALLOW_HEADERS",
        "CORS_VARY",
        "FORBIDDEN_FRAME_ERROR",
    ] {
        assert!(
            !constants[key].is_null(),
            "shellConstants() must carry {key}"
        );
    }
}

// ── the native menu ─────────────────────────────────────────────────────────────────────────────────

#[test]
fn the_menu_is_the_same_table_everywhere_except_the_apple_application_menu() {
    let mac = app_menu("darwin", "Summrise");
    let win = app_menu("win32", "Summrise");
    assert_eq!(win.len(), 4, "File, Edit, View, Session");
    assert_eq!(mac.len(), 5, "…plus the application menu");
    assert_eq!(
        mac[0].label, "Summrise",
        "the app menu is labelled with app.name"
    );
    assert_eq!(win[0].label, "File");
    assert_eq!(mac[1].label, "File");
    let labels: Vec<&str> = win.iter().map(|section| section.label.as_str()).collect();
    assert_eq!(labels, vec!["File", "Edit", "View", "Session"]);

    // The File menu's LAST row is the platform branch: mac closes the window, the others quit the app.
    let mac_last = mac[1].items.last().expect("File has rows");
    assert_eq!(mac_last.label, None);
    assert_eq!(mac_last.action, MenuAction::Role("close"));
    let win_last = win[0].items.last().expect("File has rows");
    assert_eq!(win_last.label, Some("Exit"));
    assert_eq!(win_last.action, MenuAction::Role("quit"));

    // THE TWO ROWS THAT MUST NOT COME BACK: "List Sessions" dispatched a command with no SPA handler and
    // "Export Session Log…" duplicated the per-tab export mark. The absence is a decision, so it is
    // pinned rather than left to a reader's memory.
    let all_commands: Vec<&str> = win
        .iter()
        .flat_map(|section| section.items.iter())
        .filter_map(|item| match item.action {
            MenuAction::Command(id) => Some(id),
            _ => None,
        })
        .collect();
    assert!(!all_commands.contains(&"list-sessions"));
    assert!(!all_commands.contains(&"export-session"));
    assert_eq!(
        all_commands,
        vec![
            "new-pty",
            "new-ssh",
            "new-serial",
            "new-browser",
            "close-session",
            "toggle-trajectory",
            "next-session",
            "prev-session",
        ]
    );
}

#[test]
fn every_command_row_carries_an_accelerator_and_the_separators_and_roles_do_not() {
    for section in app_menu("win32", "Summrise")
        .iter()
        .chain(app_menu("darwin", "Summrise").iter())
    {
        for item in &section.items {
            match item.action {
                MenuAction::Command(_) => assert!(
                    item.accelerator.is_some() && item.label.is_some(),
                    "a command row the SPA receives must be labelled and reachable by its chord"
                ),
                MenuAction::Separator => {
                    assert!(item.label.is_none() && item.accelerator.is_none());
                }
                MenuAction::Role(_) => {
                    assert!(
                        item.accelerator.is_none(),
                        "Electron owns a role's accelerator"
                    );
                }
                // Reload is the ONE row that is neither a command nor a role: it calls a host method and
                // it DOES carry its chord, because nothing else would give it one.
                MenuAction::Reload => {
                    assert!(item.accelerator.is_some() && item.label.is_some());
                }
            }
        }
    }
}

#[test]
fn the_menu_json_is_what_main_ts_maps_onto_electrons_template() {
    let parsed: Value = serde_json::from_str(&app_menu_json("win32", "Summrise")).expect("JSON");
    let sections = parsed.as_array().expect("an array of sections");
    assert_eq!(sections.len(), 4);
    assert_eq!(sections[0]["label"], "File");
    assert_eq!(sections[0]["items"][0]["label"], "New Terminal");
    assert_eq!(sections[0]["items"][0]["accelerator"], "CmdOrCtrl+Shift+T");
    assert_eq!(sections[0]["items"][0]["command"], "new-pty");
    assert!(sections[0]["items"][0]["role"].is_null());
    assert_eq!(sections[0]["items"][4]["separator"], true);
    assert_eq!(sections[0]["items"][7]["role"], "quit");
    assert_eq!(sections[0]["items"][7]["label"], "Exit");
    // The one row that is neither a command nor a role.
    assert_eq!(sections[2]["items"][2]["label"], "Reload");
    assert_eq!(sections[2]["items"][2]["reload"], true);
    assert!(sections[2]["items"][2]["command"].is_null());
    // The ellipsis is U+2026 in the row the SPA never sees, and the ids are unchanged.
    assert_eq!(
        sections[0]["items"][1]["label"],
        "New SSH Connection\u{2026}"
    );
    // The mac application menu's rows have no label at all — Electron labels a role itself.
    let mac: Value = serde_json::from_str(&app_menu_json("darwin", "Summrise")).expect("JSON");
    assert_eq!(mac[0]["label"], "Summrise");
    assert!(mac[0]["items"][0]["label"].is_null());
    assert_eq!(mac[0]["items"][0]["role"], "about");
}

// ── the browser sessions and the two embedded views ─────────────────────────────────────────────────

fn sessions(rows: &[(&str, &str, bool)]) -> Vec<BrowserSession> {
    rows.iter()
        .map(|(id, target, destroyed)| BrowserSession {
            id: (*id).to_string(),
            target: (*target).to_string(),
            destroyed: *destroyed,
        })
        .collect()
}

#[test]
fn browser_open_reuses_the_window_opened_on_the_same_initial_target() {
    let existing = sessions(&[
        ("browser-1", "https://a.example/", false),
        ("browser-2", "https://b.example/", false),
    ]);
    assert_eq!(
        plan_browser_open(&existing, "https://b.example/", 8),
        OpenPlan::Reuse("browser-2".to_string())
    );
    assert_eq!(
        plan_browser_open(&existing, "https://c.example/", 8),
        OpenPlan::New { evict: None }
    );
    // THE MATCH IS ON THE INITIAL TARGET, which is why the "reused" case cannot be reached by asking
    // `webContents.getURL()`: it answers `about:blank` while a load is in flight, so a same-URL reopen
    // right after the first open would miss and stack a duplicate.
    let navigated = sessions(&[("browser-1", "about:blank", false)]);
    assert_eq!(
        plan_browser_open(&navigated, "https://a.example/", 8),
        OpenPlan::New { evict: None }
    );
}

#[test]
fn a_destroyed_window_is_skipped_for_reuse_but_still_counts_against_the_cap() {
    let with_dead = sessions(&[
        ("browser-1", "https://a.example/", true),
        ("browser-2", "https://b.example/", false),
    ]);
    // The destroyed window's target is NOT reused...
    assert_eq!(
        plan_browser_open(&with_dead, "https://a.example/", 8),
        OpenPlan::New { evict: None }
    );
    // ...but it is still a row of the map, so it is evicted first when the cap is reached.
    assert_eq!(
        plan_browser_open(&with_dead, "https://c.example/", 2),
        OpenPlan::New {
            evict: Some("browser-1".to_string())
        }
    );
}

#[test]
fn the_cap_evicts_the_oldest_window_and_never_the_least_recently_used() {
    let full = sessions(&[
        ("browser-1", "https://a.example/", false),
        ("browser-2", "https://b.example/", false),
        ("browser-3", "https://c.example/", false),
    ]);
    assert_eq!(
        plan_browser_open(&full, "https://d.example/", 3),
        OpenPlan::New {
            evict: Some("browser-1".to_string())
        },
        "insertion order, so the FIRST row — not the one least recently focused"
    );
    assert_eq!(
        plan_browser_open(&full, "https://d.example/", 4),
        OpenPlan::New { evict: None }
    );
}

#[test]
fn the_open_plan_crosses_as_json_with_both_keys_always_present() {
    let existing = r#"[{"id":"browser-9","target":"https://a.example/","destroyed":false}]"#;
    let reuse: Value =
        serde_json::from_str(&plan_browser_open_json(existing, "https://a.example/", 8.0))
            .expect("JSON");
    assert_eq!(reuse["reuse"], "browser-9");
    assert!(reuse["evict"].is_null());
    let fresh: Value =
        serde_json::from_str(&plan_browser_open_json("[]", "https://a.example/", 8.0))
            .expect("JSON");
    assert!(fresh["reuse"].is_null());
    assert!(fresh["evict"].is_null());
    let evicting: Value =
        serde_json::from_str(&plan_browser_open_json(existing, "https://b.example/", 1.0))
            .expect("JSON");
    assert_eq!(evicting["evict"], "browser-9");
}

#[test]
fn the_window_id_is_the_epoch_millisecond_and_the_cdp_endpoint_is_the_contract_port() {
    assert_eq!(browser_id(1_759_123_456_789.0), "browser-1759123456789");
    assert_eq!(cdp_endpoint(9333), "http://127.0.0.1:9333");
}

#[test]
fn a_slot_under_fifty_pixels_is_not_shown() {
    assert!(slot_too_small(49.0, 400.0));
    assert!(slot_too_small(400.0, 49.0));
    assert!(
        !slot_too_small(50.0, 50.0),
        "the threshold is a floor, not an exclusive bound"
    );
    assert!(!slot_too_small(1200.0, 761.0));
    // A MISSING dimension is `undefined < 50` in JavaScript — false — so an object without the fields
    // places rather than hides. That is what a NaN answers here.
    assert!(!slot_too_small(f64::NAN, 400.0));
}

#[test]
fn a_refused_popup_is_dropped_rather_than_blanking_the_page_being_read() {
    // THE ARGUMENT IS THE LOAD DOOR'S ANSWER, which the host evaluates — so the inputs here are what
    // `sanitizeBrowserUrl` returns: an admitted URL, or `about:blank` for everything it refuses. Which
    // schemes it refuses is that crate's own case; what is pinned here is that a REFUSAL becomes a DROP
    // and not a navigation to the blank page.
    assert_eq!(
        embedded_popup_target("https://baidu.com/s?wd=x"),
        Some("https://baidu.com/s?wd=x".to_string()),
        "target=_blank is the norm on real sites and must navigate the same view"
    );
    assert_eq!(embedded_popup_target("about:blank"), None);
    // The DSH view's own door, the same shape: one origin, so an external link is dropped.
    assert_eq!(
        dsh_popup_target("http://127.0.0.1:18081/docs", true),
        Some("http://127.0.0.1:18081/docs".to_string())
    );
    assert_eq!(dsh_popup_target("https://example.com/", false), None);
    assert_eq!(dsh_popup_target("about:blank", true), None);
}

#[test]
fn the_dsh_door_serializes_what_the_url_policy_admitted_and_blanks_the_rest() {
    assert_eq!(
        dsh_target("http://127.0.0.1:18081/api/remote.mux", true),
        "http://127.0.0.1:18081/api/remote.mux"
    );
    // The SERIALIZATION is this crate's half — and it is the WHATWG one, so a path that needs normalizing
    // is normalized the way `new URL(raw).toString()` normalizes it.
    assert_eq!(
        dsh_target("http://127.0.0.1:18081/a/../b", true),
        "http://127.0.0.1:18081/b"
    );
    // `isDshUrl` is the host's, and its answers are the url-policy crate's own cases. A `false` here is
    // the refusal, whatever the URL looks like.
    assert_eq!(
        dsh_target("http://127.0.0.1:18080/panel/", false),
        "about:blank"
    );
    assert_eq!(dsh_target("https://127.0.0.1:18081/", false), "about:blank");
    assert_eq!(dsh_target("not a url", false), "about:blank");
    // ...and a target the door ADMITTED that will not parse is still refused rather than panicking.
    assert_eq!(dsh_target("http://[::1", true), "about:blank");
    assert_eq!(
        dsh_home("http://127.0.0.1:18081"),
        "http://127.0.0.1:18081/"
    );
}

#[test]
fn zoom_is_clamped_and_a_zero_means_one_hundred_percent() {
    assert_eq!(zoom_factor(2.0), 2.0);
    assert_eq!(zoom_factor(0.0), 1.0, "`Number(f) || 1` — 0 is falsy");
    assert_eq!(zoom_factor(f64::NAN), 1.0);
    assert_eq!(zoom_factor(9.0), 3.0);
    assert_eq!(zoom_factor(0.1), 0.5);
    assert_eq!(
        zoom_factor(-4.0),
        0.5,
        "a negative zoom is clamped, not refused"
    );
    assert_eq!(zoom_factor(0.5), 0.5);
    assert_eq!(zoom_factor(3.0), 3.0);
}

#[test]
fn a_crashed_view_recovers_to_the_blank_page_and_never_to_a_search_engine() {
    assert_eq!(
        embedded_recover_url(Some("https://ok.example/a")),
        "https://ok.example/a"
    );
    assert_eq!(embedded_recover_url(Some("about:blank")), "about:blank");
    assert_eq!(embedded_recover_url(Some("")), "about:blank");
    assert_eq!(embedded_recover_url(None), "about:blank");
}

#[test]
fn the_state_record_falls_back_when_the_live_url_is_empty() {
    assert_eq!(
        shown_url(Some("https://ok.example/a"), "about:blank"),
        "https://ok.example/a"
    );
    assert_eq!(
        shown_url(Some(""), "https://last.example/"),
        "https://last.example/"
    );
    assert_eq!(
        shown_url(None, "https://last.example/"),
        "https://last.example/"
    );
    assert!(view_visible(true, true));
    assert!(
        !view_visible(true, false),
        "a dead webContents is not visible"
    );
    assert!(
        !view_visible(false, true),
        "hidden is hidden even with live contents"
    );
}

#[test]
fn the_harness_doors_are_the_ports_the_range_check_admits_in_order() {
    assert_eq!(
        harness_doors(&[
            Some(7801.0),
            None,
            Some(0.0),
            Some(70_000.0),
            Some(8080.5),
            Some(18_082.0),
        ]),
        vec![7801, 18_082]
    );
    assert_eq!(harness_doors(&[]), Vec::<u16>::new());
    assert_eq!(harness_door(Some(7801.0)), Some(7801));
    assert_eq!(harness_door(None), None);
}

#[test]
fn going_backwards_is_the_sign_of_the_delta() {
    assert!(go_backwards(-1.0));
    assert!(!go_backwards(1.0));
    assert!(go_backwards(-2.0), "`delta < 0`, not `delta === -1`");
    assert!(!go_backwards(0.0));
}

// ── the loopback control server ─────────────────────────────────────────────────────────────────────

#[test]
fn the_control_router_keeps_the_method_asymmetries_the_typescript_had() {
    // POST-only routes fall through to the 404 on any other method...
    assert_eq!(
        control_route("POST", "/api/browser-session/open"),
        ControlRoute::BrowserOpen
    );
    assert_eq!(
        control_route("GET", "/api/browser-session/open"),
        ControlRoute::NotFound
    );
    assert_eq!(
        control_route("POST", "/api/browser-session/close"),
        ControlRoute::BrowserClose
    );
    assert_eq!(
        control_route("GET", "/api/browser-session/close"),
        ControlRoute::NotFound
    );
    assert_eq!(
        control_route("POST", "/api/shell/start-agent"),
        ControlRoute::StartAgent
    );
    assert_eq!(
        control_route("GET", "/api/shell/start-agent"),
        ControlRoute::NotFound
    );
    // ...and the read routes have no method guard at all, so a POST reaches the reader.
    assert_eq!(
        control_route("GET", "/api/browser-session/list"),
        ControlRoute::BrowserList
    );
    assert_eq!(
        control_route("POST", "/api/browser-session/list"),
        ControlRoute::BrowserList
    );
    assert_eq!(
        control_route("GET", "/api/shell/agent-status"),
        ControlRoute::AgentStatus
    );
    assert_eq!(
        control_route("POST", "/api/shell/agent-status"),
        ControlRoute::AgentStatus
    );
    assert_eq!(
        control_route("GET", "/api/shell/icon-status"),
        ControlRoute::IconStatus
    );
    // The fall-through, and the fact that a path is matched exactly — `/desktopx` is a different path.
    assert_eq!(control_route("GET", "/"), ControlRoute::NotFound);
    assert_eq!(
        control_route("GET", "/api/shell/agent-status/"),
        ControlRoute::NotFound
    );
    assert_eq!(
        control_route("GET", "/api/shell/agent-statusx"),
        ControlRoute::NotFound
    );
    for route in [
        ControlRoute::BrowserOpen,
        ControlRoute::BrowserClose,
        ControlRoute::BrowserList,
        ControlRoute::StartAgent,
        ControlRoute::AgentStatus,
        ControlRoute::IconStatus,
        ControlRoute::NotFound,
    ] {
        assert!(!route.as_str().is_empty());
    }
}

#[test]
fn a_preflight_is_a_method_check_of_its_own() {
    assert!(control_is_preflight("OPTIONS"));
    assert!(!control_is_preflight("GET"));
    assert!(
        !control_is_preflight("options"),
        "the comparison is case-sensitive, as HTTP methods are"
    );
}

#[test]
fn the_path_is_resolved_against_the_loopback_base() {
    assert_eq!(
        control_path(Some("/api/shell/icon-status")).as_deref(),
        Some("/api/shell/icon-status")
    );
    assert_eq!(
        control_path(Some("/api/shell/icon-status?x=1")).as_deref(),
        Some("/api/shell/icon-status")
    );
    assert_eq!(
        control_path(None).as_deref(),
        Some("/"),
        "`req.url || \"/\"`"
    );
    assert_eq!(
        control_path(Some("")).as_deref(),
        Some("/"),
        "an empty request target is the root"
    );
    // A protocol-relative request line resolves to a DIFFERENT HOST and the SAME path — so the router
    // does not care which host the request line named, exactly as before.
    assert_eq!(
        control_path(Some("//evil.example/api/shell/icon-status")).as_deref(),
        Some("/api/shell/icon-status")
    );
    assert_eq!(
        control_path(Some("http://evil.example/x")).as_deref(),
        Some("/x")
    );
}

#[test]
fn a_malformed_request_url_is_a_404_and_not_a_crash() {
    // THE ONE NAMED DIVERGENCE: `new URL(req.url, base)` throws out of the request listener in
    // `main.ts`, taking the Electron main process with it. This answers `None`, which the caller turns
    // into a 404 — a fix, recorded in `control::control_url` rather than slipped in.
    assert_eq!(control_path(Some("http://[::1")), None);
    assert_eq!(control_url(Some("http://[::1")), None);
}

#[test]
fn a_query_parameter_is_percent_decoded_the_way_url_search_params_decodes_it() {
    let request = Some("/api/browser-session/open?url=https%3A%2F%2Fok.example%2Fa%3Fx%3D1");
    assert_eq!(
        control_query(request, "url").as_deref(),
        Some("https://ok.example/a?x=1"),
        "the decode IS the load door's input"
    );
    // `+` is a space, and the FIRST value for a repeated key wins.
    assert_eq!(
        control_query(Some("/x?a=1+2&a=3"), "a").as_deref(),
        Some("1 2")
    );
    // A key with no `=` answers the empty string, and an absent key answers nothing at all.
    assert_eq!(control_query(Some("/x?flag"), "flag").as_deref(), Some(""));
    assert_eq!(control_query(Some("/x?a=1"), "b"), None);
    assert_eq!(control_query(None, "url"), None);
}

#[test]
fn the_reflected_origin_falls_back_to_the_wildcard() {
    assert_eq!(
        cors_allow_origin(Some("http://127.0.0.1:18080")),
        "http://127.0.0.1:18080"
    );
    assert_eq!(
        cors_allow_origin(Some("null")),
        "null",
        "a data: page's origin is a value, not an absence"
    );
    assert_eq!(cors_allow_origin(None), "*");
    assert_eq!(cors_allow_origin(Some("")), "*");
    assert_eq!(CORS_ALLOW_METHODS, "GET, POST, OPTIONS");
    assert_eq!(CORS_ALLOW_HEADERS, "content-type, authorization");
    assert_eq!(CORS_VARY, "Origin");
}

// ── the agent lifecycle and the shell's self-protection ─────────────────────────────────────────────

#[test]
fn the_auto_launch_plan_is_idempotent_and_never_reports_a_no_op_as_a_failure() {
    assert_eq!(auto_launch_plan(true, false), AutoLaunchPlan::Create);
    assert_eq!(auto_launch_plan(false, true), AutoLaunchPlan::Remove);
    assert_eq!(auto_launch_plan(true, true), AutoLaunchPlan::Nothing);
    assert_eq!(auto_launch_plan(false, false), AutoLaunchPlan::Nothing);
    assert_eq!(AutoLaunchPlan::Create.as_str(), "create");
    assert_eq!(AutoLaunchPlan::Remove.as_str(), "remove");
    assert_eq!(AutoLaunchPlan::Nothing.as_str(), "nothing");
}

#[test]
fn the_task_argument_lists_are_exact_including_the_create_quoting_trap() {
    assert_eq!(
        schtasks_query_args(AUTOSTART_TASK),
        ["/query", "/tn", "SummriseDesktop"]
    );
    assert_eq!(
        schtasks_run_args(AGENT_TASK),
        ["/run", "/tn", "SummriseAgent"]
    );
    assert_eq!(
        schtasks_end_args(AUTOSTART_TASK),
        ["/end", "/tn", "SummriseDesktop"]
    );
    assert_eq!(
        schtasks_delete_args(AUTOSTART_TASK),
        ["/delete", "/tn", "SummriseDesktop", "/f"]
    );
    let create = schtasks_create_args(
        AUTOSTART_TASK,
        r"C:\Program Files\Summrise\scripts\start-desktop.ps1",
    );
    assert_eq!(
        create,
        vec![
            "/create".to_string(),
            "/tn".to_string(),
            "SummriseDesktop".to_string(),
            "/tr".to_string(),
            // THE INNER QUOTES ARE THE POINT: `schtasks` re-parses the /tr VALUE as a command line, and
            // the spawn array keeps the outer argument intact so these two reach it.
            r#"powershell -NoProfile -ExecutionPolicy Bypass -File \"C:\Program Files\Summrise\scripts\start-desktop.ps1\""#.to_string(),
            "/sc".to_string(),
            "onlogon".to_string(),
            "/ru".to_string(),
            "Administrator".to_string(),
            "/f".to_string(),
        ]
    );
    assert_eq!(SCHTASKS_TIMEOUT_MS, 15_000.0);
}

#[test]
fn the_watchdog_needs_five_misses_and_a_five_minute_gap() {
    let now = 1_700_000_000_000.0;
    assert!(
        !watchdog_should_start(4.0, 0.0, now),
        "four misses is not the gate"
    );
    assert!(
        watchdog_should_start(5.0, 0.0, now),
        "and a last-start of 0 is always past the gap"
    );
    assert!(watchdog_should_start(9.0, now - 300_001.0, now));
    assert!(
        !watchdog_should_start(9.0, now - 300_000.0, now),
        "`> MIN_GAP`, so exactly five minutes is still inside the gap"
    );
}

#[test]
fn the_watchdog_log_is_the_only_evidence_it_fired() {
    assert_eq!(
        watchdog_log(true, None),
        "[summrise] agent watchdog: schtasks /run SummriseAgent \u{2192} ok"
    );
    assert_eq!(
        watchdog_log(false, Some("schtasks exit 1")),
        "[summrise] agent watchdog: schtasks /run SummriseAgent \u{2192} failed: schtasks exit 1"
    );
}

#[test]
fn the_retry_backoff_doubles_and_stops_at_thirty_seconds() {
    assert_eq!(RETRY_START_MS, 2_000.0);
    assert_eq!(next_retry_ms(2_000.0), 4_000.0);
    assert_eq!(next_retry_ms(4_000.0), 8_000.0);
    assert_eq!(next_retry_ms(16_000.0), 30_000.0, "32 s is capped");
    assert_eq!(next_retry_ms(30_000.0), 30_000.0);
    assert_eq!(RETRY_CAP_MS, 30_000.0);
}

#[test]
fn only_a_real_main_frame_failure_schedules_a_retry() {
    assert!(
        should_retry_load(true, -105.0),
        "ERR_NAME_NOT_RESOLVED on the main frame"
    );
    assert!(
        !should_retry_load(true, -3.0),
        "ERR_ABORTED is a same-URL reload — retrying it stacked a second loadDesktop chain"
    );
    assert!(
        !should_retry_load(false, -105.0),
        "a subframe failure is not the main document's"
    );
}

#[test]
fn the_wait_page_guard_matches_the_document_the_shell_itself_wrote() {
    assert!(is_wait_page(
        "data:text/html;charset=utf-8,%3C!doctype%20html%3E"
    ));
    assert!(is_wait_page("data:text/html,"));
    assert!(
        !is_wait_page("data:application/json,{}"),
        "another data: document is not the wait page"
    );
    assert!(!is_wait_page("http://127.0.0.1:18080/desktop/"));
    assert!(!is_wait_page(""));
}

#[test]
fn any_http_answer_proves_the_accept_loop_is_alive() {
    // NOT 200: `/api/status` is token-gated, so a HEALTHY agent answers 401 to the shell's
    // credential-less probe, and requiring 200 "made a live agent look dead and sent the watchdog into a
    // restart loop".
    assert!(status_is_alive(Some(401.0)));
    assert!(status_is_alive(Some(200.0)));
    assert!(status_is_alive(Some(500.0)));
    assert!(
        !status_is_alive(Some(0.0)),
        "a status of 0 is not a response"
    );
    assert!(
        !status_is_alive(None),
        "`typeof r.statusCode === \"number\"`"
    );
    assert!(!status_is_alive(Some(f64::NAN)));
}

#[test]
fn the_tripwire_allows_the_spa_the_wait_page_and_the_blank_page() {
    // `desktop_spa` is `isDesktopSpaUrl(url)`'s answer, evaluated by the host; which URLs that predicate
    // admits — the parsed origin, the `/desktop` path segment, the userinfo and sibling-host lookalikes —
    // is the url-policy crate's own case. This pins the COMPOSITION the shell actually ran.
    assert!(tripwire_allows("http://127.0.0.1:18080/desktop/", true));
    assert!(
        !tripwire_allows("http://127.0.0.1:18080/desktop/", false),
        "a URL the SPA predicate did not vouch for is not allowed by looking like one"
    );
    // The two literal carve-outs, which are THIS crate's and do not go through the predicate.
    assert!(tripwire_allows("data:text/html;charset=utf-8,x", false));
    assert!(tripwire_allows("about:blank", false));
    // THE ROUND-258 INCIDENT: a CDP-driven `browser_navigate` hijacked the main window to qq.com and the
    // panel vanished. It bypasses `will-navigate`, so this is the only thing that sees it.
    assert!(!tripwire_allows("https://qq.com/", false));
    assert!(
        !tripwire_allows("http://127.0.0.1:18080/", false),
        "the origin's root is not the SPA"
    );
    assert!(
        !tripwire_allows("datax:text/html", false),
        "the carve-out is a PREFIX, not a substring"
    );
}

#[test]
fn the_tripwire_log_truncates_in_javascript_units() {
    let long = "x".repeat(200);
    assert_eq!(
        tripwire_log(&long),
        format!(
            "[summrise] main-window tripwire: blocked stray navigation to {}",
            "x".repeat(80)
        )
    );
    assert_eq!(
        tripwire_log("https://qq.com/"),
        "[summrise] main-window tripwire: blocked stray navigation to https://qq.com/"
    );
}

#[test]
fn the_cdp_self_check_recognises_its_own_user_agent() {
    assert!(cdp_user_agent_is_ours(Some(
        "Mozilla/5.0 summrise-desktop-electron/1.2.510"
    )));
    assert!(!cdp_user_agent_is_ours(Some(
        "Mozilla/5.0 Chrome/141.0.0.0"
    )));
    assert!(
        !cdp_user_agent_is_ours(None),
        "`j[\"User-Agent\"] || \"\"` — an absent one matches nothing"
    );
    assert!(!cdp_user_agent_is_ours(Some("")));
    assert_eq!(
        cdp_user_agent(r#"{"Browser":"Chrome/141","User-Agent":"summrise-desktop-electron"}"#),
        "summrise-desktop-electron"
    );
    // A field that is present but not a string is not a user agent.
    assert_eq!(cdp_user_agent(r#"{"User-Agent":42}"#), "");
    assert_eq!(cdp_user_agent(r#"{"User-Agent":null}"#), "");
    assert_eq!(cdp_user_agent("{}"), "");
    assert_eq!(
        cdp_user_agent("not json"),
        "",
        "a body that does not parse owns nothing"
    );
    assert!(cdp_self_check_owns_port(
        r#"{"User-Agent":"x summrise-desktop-electron x"}"#
    ));
    assert!(!cdp_self_check_owns_port(r#"{"User-Agent":"Chrome/141"}"#));
}

#[test]
fn the_three_cdp_messages_name_the_port_and_keep_their_wording() {
    assert_eq!(
        cdp_self_check_ok(9333),
        "[summrise] CDP self-check OK: 127.0.0.1:9333 is this app"
    );
    assert_eq!(
        cdp_warning_foreign(9333, "Chrome/141"),
        "[summrise] CDP WARNING: port 9333 answered by another browser (UA=Chrome/141\u{2026}) \u{2014} AI driving may target the wrong process"
    );
    assert_eq!(
        cdp_warning_not_responding(9333),
        "[summrise] CDP WARNING: 9333 not responding \u{2014} remote debugging may be off"
    );
    assert_eq!(
        cdp_warning_unreachable(9333),
        "[summrise] CDP WARNING: could not reach 9333 \u{2014} remote debugging may be off"
    );
    // The foreign warning truncates the UA at 60 units, in JavaScript's counting.
    let long = "u".repeat(200);
    assert!(cdp_warning_foreign(9333, &long).contains(&format!("UA={}\u{2026}", "u".repeat(60))));
}

// ── the tray ────────────────────────────────────────────────────────────────────────────────────────

#[test]
fn uptime_is_compact_and_every_boundary_is_exact() {
    assert_eq!(fmt_uptime(0.0), "0s");
    assert_eq!(fmt_uptime(59.0), "59s");
    assert_eq!(fmt_uptime(60.0), "1m 0s");
    assert_eq!(fmt_uptime(204.0), "3m 24s");
    assert_eq!(fmt_uptime(3_599.0), "59m 59s");
    assert_eq!(fmt_uptime(3_600.0), "1h 0m");
    assert_eq!(fmt_uptime(3_923.0), "1h 5m");
    assert_eq!(fmt_uptime(86_399.0), "23h 59m");
    assert_eq!(fmt_uptime(86_400.0), "1d 0h");
    assert_eq!(fmt_uptime(183_600.0), "2d 3h");
}

#[test]
fn the_clock_is_local_and_the_host_supplies_its_offset() {
    // 2026-01-01T00:00:00Z.
    const AT: f64 = 1_767_225_600_000.0;
    assert_eq!(fmt_clock(AT, 0.0), "00:00:00");
    // `getTimezoneOffset()` is minutes to ADD to local to get UTC, so UTC+8 is -480.
    assert_eq!(fmt_clock(AT, -480.0), "08:00:00");
    assert_eq!(fmt_clock(AT, 480.0), "16:00:00");
    // A half-hour zone, and the day rollover in both directions.
    assert_eq!(fmt_clock(AT, -330.0), "05:30:00");
    assert_eq!(fmt_clock(AT - 60_000.0, -480.0), "07:59:00");
    assert_eq!(fmt_clock(AT + 86_400_000.0, 0.0), "00:00:00");
}

#[test]
fn a_failed_poll_keeps_the_last_facts_and_a_partial_one_keeps_the_fields_it_omits() {
    let previous = StatusFacts {
        version: "1.2.500".to_string(),
        uptime: "1h 0m".to_string(),
        sessions: 3.0,
        cpu: Some(5.0),
        mem: Some(10.0),
    };
    assert_eq!(apply_status("not json", &previous), previous);
    assert_eq!(apply_status("null", &previous), previous);
    assert_eq!(apply_status("[]", &previous), previous);
    let partial = apply_status(r#"{"mem_pct":42}"#, &previous);
    assert_eq!(partial.version, "1.2.500");
    assert_eq!(partial.uptime, "1h 0m");
    assert_eq!(partial.sessions, 3.0);
    assert_eq!(partial.cpu, Some(5.0));
    assert_eq!(partial.mem, Some(42.0));
}

#[test]
fn the_version_prefers_the_npm_release_and_falls_through_the_way_or_does() {
    let previous = StatusFacts {
        version: "1.2.500".to_string(),
        ..Default::default()
    };
    assert_eq!(
        apply_status(r#"{"release":"1.2.510","version":"1.0.145"}"#, &previous).version,
        "1.2.510"
    );
    assert_eq!(
        apply_status(r#"{"release":"","version":"1.0.145"}"#, &previous).version,
        "1.0.145"
    );
    assert_eq!(
        apply_status(r#"{"release":null,"version":"1.0.145"}"#, &previous).version,
        "1.0.145"
    );
    assert_eq!(
        apply_status(r#"{"release":"","version":""}"#, &previous).version,
        "1.2.500",
        "neither is truthy, so the last known version stays"
    );
}

#[test]
fn the_four_numeric_fields_are_guarded_by_typeof() {
    let previous = StatusFacts {
        sessions: 3.0,
        mem: Some(10.0),
        ..Default::default()
    };
    // A string is not a number, and neither is a bool — `typeof` on both sides.
    let stringy = apply_status(
        r#"{"live_sessions":"7","cpu_pct":"4","mem_pct":true}"#,
        &previous,
    );
    assert_eq!(stringy.sessions, 3.0);
    assert_eq!(stringy.cpu, None);
    assert_eq!(stringy.mem, Some(10.0));
    // ...and a real number is taken, with the uptime stored ALREADY FORMATTED.
    let numeric = apply_status(
        r#"{"uptime_secs":3723,"live_sessions":7,"cpu_pct":4.4,"mem_pct":12.6}"#,
        &previous,
    );
    assert_eq!(numeric.uptime, "1h 2m");
    assert_eq!(numeric.sessions, 7.0);
    assert_eq!(numeric.cpu, Some(4.4));
    assert_eq!(numeric.mem, Some(12.6));
}

/// 2026-01-01T00:00:00Z, so every expected string below reads as a clock face.
const AT: f64 = 1_767_225_600_000.0;

#[test]
fn the_health_line_names_the_observation_and_never_a_bare_verdict() {
    let facts = StatusFacts {
        version: "1.2.510".to_string(),
        uptime: "3m 24s".to_string(),
        sessions: 2.0,
        cpu: Some(4.4),
        mem: Some(12.6),
    };
    assert_eq!(
        tray_health(true, &facts, AT, 0.0, None),
        // `Math.round`: 4.4 -> 4 and 12.6 -> 13.
        "answered 00:00:00 \u{00b7} v1.2.510, up 3m 24s, 2 sessions, CPU 4% \u{00b7} MEM 13%"
    );
    // THE SENTENCE THE WHOLE FILE EXISTS FOR: nothing has ever answered, and it says so rather than
    // asserting "Agent stopped".
    assert_eq!(
        tray_health(false, &facts, AT, 0.0, None),
        "reachability NOT VERIFIED \u{00b7} no reply since launch"
    );
    // It answered once and has not since — and the WHEN is the last reply, not the failed probe.
    assert_eq!(
        tray_health(false, &facts, AT, 0.0, Some((AT - 3_600_000.0, 0.0))),
        "not answering \u{00b7} last reply 23:00:00"
    );
    // It IS answering, so the time is this observation's.
    assert_eq!(
        tray_health(true, &facts, AT, 0.0, Some((AT - 3_600_000.0, 0.0))),
        "answered 00:00:00 \u{00b7} v1.2.510, up 3m 24s, 2 sessions, CPU 4% \u{00b7} MEM 13%"
    );
}

#[test]
fn the_health_line_omits_what_the_answer_did_not_carry() {
    // Nothing but a reply: no version, no uptime, no sessions, no vitals.
    assert_eq!(
        tray_health(true, &StatusFacts::default(), AT, 0.0, None),
        "answered 00:00:00 \u{00b7} v?"
    );
    // Half a vitals line is not printed: no memory figure means NO vitals, even with a CPU figure.
    let cpu_only = StatusFacts {
        cpu: Some(4.0),
        ..Default::default()
    };
    assert_eq!(
        tray_health(true, &cpu_only, AT, 0.0, None),
        "answered 00:00:00 \u{00b7} v?"
    );
    // A memory figure with no CPU figure prints the CPU as `?` rather than dropping it.
    let mem_only = StatusFacts {
        mem: Some(12.0),
        ..Default::default()
    };
    assert_eq!(
        tray_health(true, &mem_only, AT, 0.0, None),
        "answered 00:00:00 \u{00b7} v?, CPU ?% \u{00b7} MEM 12%"
    );
    // A version that arrived but is not a string is rendered the way a template literal renders it.
    let numeric_version = StatusFacts {
        version: "510".to_string(),
        ..Default::default()
    };
    assert_eq!(
        tray_health(true, &numeric_version, AT, 0.0, None),
        "answered 00:00:00 \u{00b7} v510"
    );
}

#[test]
fn the_session_count_is_pluralised_and_a_zero_is_omitted_entirely() {
    let one = StatusFacts {
        sessions: 1.0,
        ..Default::default()
    };
    assert_eq!(
        tray_health(true, &one, AT, 0.0, None),
        "answered 00:00:00 \u{00b7} v?, 1 session"
    );
    let two = StatusFacts {
        sessions: 2.0,
        ..Default::default()
    };
    assert_eq!(
        tray_health(true, &two, AT, 0.0, None),
        "answered 00:00:00 \u{00b7} v?, 2 sessions"
    );
    let none = StatusFacts {
        sessions: 0.0,
        ..Default::default()
    };
    assert_eq!(
        tray_health(true, &none, AT, 0.0, None),
        "answered 00:00:00 \u{00b7} v?",
        "0 is falsy in the TypeScript, so the whole clause is dropped"
    );
}

#[test]
fn the_tray_re_enters_the_load_loop_only_from_the_panels_own_origin() {
    // The last argument is `isBaseOrigin(mainUrl)`'s answer, which the host evaluates ONLY when the
    // window is live — the TypeScript's short-circuit, preserved at the call site.
    assert!(tray_should_watch(false, false, true, true));
    assert!(
        !tray_should_watch(true, false, true, true),
        "a live agent needs no watch"
    );
    assert!(
        !tray_should_watch(false, true, true, true),
        "the loop guard, so polls cannot stack retries"
    );
    assert!(
        !tray_should_watch(false, false, false, true),
        "no window, nothing to swap"
    );
    // THE ORIGIN HALF IS THE POINT: the wait page is a `data:` URL and the blank page is `about:blank`,
    // and neither is on the panel's origin — so neither is swapped for the wait page again.
    assert!(!tray_should_watch(false, false, true, false));
}

#[test]
fn one_poll_crosses_the_boundary_as_one_decision() {
    let request = format!(
        r#"{{"running":true,"statusText":"{{\"release\":\"1.2.510\",\"uptime_secs\":204,\"live_sessions\":2,\"cpu_pct\":4.4,\"mem_pct\":12.6}}","prev":{{"version":"","uptime":"","sessions":0,"cpu":null,"mem":null}},"observedAt":{AT},"lastAnsweredAt":null,"tzOffsetMin":0}}"#
    );
    let answered: Value = serde_json::from_str(&refresh_tray_health(&request)).expect("JSON");
    assert_eq!(
        answered["health"],
        "answered 00:00:00 \u{00b7} v1.2.510, up 3m 24s, 2 sessions, CPU 4% \u{00b7} MEM 13%"
    );
    assert_eq!(answered["lastAnsweredAt"].as_f64(), Some(AT));
    assert_eq!(answered["facts"]["version"], "1.2.510");

    // The agent has gone: no fetch, so the facts survive and the time is the LAST REPLY's.
    let lapsed = format!(
        r#"{{"running":false,"statusText":"","prev":{},"observedAt":{AT},"lastAnsweredAt":{},"tzOffsetMin":0}}"#,
        answered["facts"],
        AT - 3_600_000.0
    );
    let lapsed: Value = serde_json::from_str(&refresh_tray_health(&lapsed)).expect("JSON");
    assert_eq!(
        lapsed["health"],
        "not answering \u{00b7} last reply 23:00:00"
    );
    assert_eq!(
        lapsed["facts"]["version"], "1.2.510",
        "the keep-last rule survives the round trip"
    );
    assert_eq!(
        lapsed["lastAnsweredAt"].as_f64(),
        Some(AT - 3_600_000.0),
        "a failed poll does not move the last reply"
    );

    // Nothing has ever answered.
    let never = format!(
        r#"{{"running":false,"statusText":"","prev":null,"observedAt":{AT},"lastAnsweredAt":null,"tzOffsetMin":0}}"#
    );
    let never: Value = serde_json::from_str(&refresh_tray_health(&never)).expect("JSON");
    assert_eq!(
        never["health"],
        "reachability NOT VERIFIED \u{00b7} no reply since launch"
    );
    assert!(never["lastAnsweredAt"].is_null());
}
