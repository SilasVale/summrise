//! THE PORTED CASES — `agent/summrise-desktop-electron/test/url-policy.test.mjs`, case for case.
//!
//! **THE JAVASCRIPT SUITE HAD 11 `test(...)` BLOCKS AND THIS FILE HAS 11 `#[test]` FUNCTIONS**, one per
//! block, each with that block's own name in its doc comment so a reader can match them by grep:
//!
//! ```text
//!   cargo test -p summrise-url-policy                    # 11 ported cases + 2 for the JS classes
//!   grep -c '^test(' agent/summrise-desktop-electron/test/url-policy.test.mjs   # deleted by this landing
//! ```
//!
//! The bodies are the same assertions in the same order, including the ones that only carry a message
//! — a case that was worth a sentence in the JavaScript is worth the same sentence here, because the
//! sentence is what tells the next reader why the input is in the list at all.
//!
//! The `.mjs` file is DELETED in this landing rather than left to rot: it imported
//! `../src/url-policy.js`, which no longer exists. What still executes the SHIPPED artifact is
//! `agent/summrise-desktop-electron/test/url-policy-wasm.test.mjs` — a wiring test, not a second copy
//! of these cases (they would be two implementations of one oracle, which is how oracles drift).

use crate::*;

/// The JavaScript suite's `finally { setAgentPort(18080); setDshPort(18081) }`, as a guard.
///
/// It runs even when an assertion panics, so the cases that mutate the configured ports stay
/// order-independent — the same property the `.mjs` suite got from `try`/`finally`, and the reason a
/// failing case cannot leave the next one looking at a port nobody set.
struct PortsRestored;

impl Drop for PortsRestored {
    fn drop(&mut self) {
        set_agent_port(f64::from(DEFAULT_AGENT_PORT));
        set_dsh_port(f64::from(DEFAULT_DSH_PORT));
        clear_extra_dsh_ports();
    }
}

/// JS: `test("isBaseOrigin: userinfo-trick bypass stays closed (IPC audit #1)")`
#[test]
fn is_base_origin_userinfo_trick_bypass_stays_closed() {
    assert!(is_base_origin("http://127.0.0.1:18080/desktop/"));
    assert!(
        !is_base_origin("http://127.0.0.1:18080@evil.com/x"),
        "userinfo prefix must NOT pass"
    );
    assert!(!is_base_origin("http://evil.com/?u=http://127.0.0.1:18080"));
    assert!(!is_base_origin("http://127.0.0.1:18081/"), "port matters");
    assert!(!is_base_origin("not a url"));
    assert!(!is_base_origin(""));
}

/// JS: `test("frameUrlOk: only pinned-origin frames reach the IPC bridge (audit #2)")`
#[test]
fn frame_url_ok_only_pinned_origin_frames_reach_the_ipc_bridge() {
    assert!(frame_url_ok("http://127.0.0.1:18080/panel/"));
    assert!(
        !frame_url_ok("data:text/html,hi"),
        "wait-page frames get no bridge"
    );
    assert!(!frame_url_ok("file:///etc/passwd"));
    assert!(!frame_url_ok(""));
}

/// JS: `test("sanitizeBrowserUrl: http/https/about:blank only")`
#[test]
fn sanitize_browser_url_http_https_about_blank_only() {
    assert_eq!(
        sanitize_browser_url(Some("file:///C:/Windows/win.ini")),
        "about:blank"
    );
    assert_eq!(
        sanitize_browser_url(Some("javascript:alert(1)")),
        "about:blank"
    );
    assert_eq!(
        sanitize_browser_url(Some("chrome://settings")),
        "about:blank"
    );
    assert_eq!(
        sanitize_browser_url(Some("https://ok.example/a?x=1")),
        "https://ok.example/a?x=1"
    );
    assert_eq!(sanitize_browser_url(None), "about:blank");
}

/// JS: `test("isDesktopSpaUrl: parsed origin + /desktop subtree only")`
#[test]
fn is_desktop_spa_url_parsed_origin_and_desktop_subtree_only() {
    assert!(is_desktop_spa_url("http://127.0.0.1:18080/desktop"));
    assert!(is_desktop_spa_url(
        "http://127.0.0.1:18080/desktop/settings"
    ));
    assert!(
        !is_desktop_spa_url("http://127.0.0.1:18080/desktopx"),
        "/desktop must be a path segment"
    );
    assert!(!is_desktop_spa_url("http://127.0.0.1:18080/"));
    assert!(
        !is_desktop_spa_url("http://127.0.0.1:18080.evil.com/desktop"),
        "sibling-host lookalike"
    );
    assert!(
        !is_desktop_spa_url("http://127.0.0.1:18080@evil.com/desktop"),
        "userinfo trick"
    );
    assert!(
        !is_desktop_spa_url("https://127.0.0.1:18080/desktop"),
        "scheme is part of the origin"
    );
    assert!(!is_desktop_spa_url("data:text/html,wait"));
    assert!(!is_desktop_spa_url("about:blank"));
    assert!(!is_desktop_spa_url("not a url"));
}

/// JS: `test("isPrivateHost: RFC1918 + loopback + .local only")`
#[test]
fn is_private_host_rfc1918_loopback_and_local_only() {
    assert!(is_private_host("192.168.1.1"), "ONT lab net");
    assert!(is_private_host("10.0.0.5"));
    assert!(is_private_host("172.16.0.1"));
    assert!(is_private_host("172.31.255.255"));
    assert!(is_private_host("127.0.0.1"));
    assert!(is_private_host("localhost"));
    assert!(is_private_host("printer.local"));
    assert!(!is_private_host("172.15.0.1"), "just outside 172.16/12");
    assert!(!is_private_host("172.32.0.1"), "just outside 172.16/12");
    assert!(!is_private_host("8.8.8.8"), "public stays strict");
    assert!(!is_private_host("example.com"));
    assert!(!is_private_host("192.168.1.1.evil.com"), "suffix lookalike");
    assert!(!is_private_host(""));
}

/// JS: `test("certBypassAllowed: private http(s) only")`
#[test]
fn cert_bypass_allowed_private_http_s_only() {
    assert!(
        cert_bypass_allowed("https://192.168.1.1:8000/?Role=Gpon"),
        "ONT web UI"
    );
    assert!(cert_bypass_allowed("http://10.1.2.3/"));
    assert!(
        !cert_bypass_allowed("https://example.com/"),
        "public internet stays validated"
    );
    assert!(
        !cert_bypass_allowed("file:///C:/Windows/win.ini"),
        "schemes stay gated"
    );
    assert!(!cert_bypass_allowed("not a url"));
}

/// JS: `test("agent port: predicates follow setAgentPort, default stays 18080")`
#[test]
fn agent_port_predicates_follow_set_agent_port_default_stays_18080() {
    let _restored = PortsRestored;
    assert_eq!(get_agent_port(), 18080, "default is canonical");
    assert_eq!(agent_base(), "http://127.0.0.1:18080");
    set_agent_port(7740.0);
    assert_eq!(get_agent_port(), 7740);
    assert_eq!(agent_base(), "http://127.0.0.1:7740");
    assert!(
        is_base_origin("http://127.0.0.1:7740/desktop/"),
        "bridge follows the port"
    );
    assert!(
        !is_base_origin("http://127.0.0.1:18080/desktop/"),
        "old port no longer matches"
    );
    assert!(is_desktop_spa_url("http://127.0.0.1:7740/desktop/settings"));
    set_agent_port(0.0);
    set_agent_port(99999.0);
    set_agent_port(f64::NAN);
    assert_eq!(get_agent_port(), 7740, "invalid ports are ignored");
    set_agent_port(f64::from(DEFAULT_AGENT_PORT));
    assert!(
        is_base_origin("http://127.0.0.1:18080/desktop/"),
        "default restored"
    );
}

/// JS: `test("parseAgentPort: server.port only, strict")`
#[test]
fn parse_agent_port_server_port_only_strict() {
    assert_eq!(
        parse_agent_port("server:\n  host: \"0.0.0.0\"\n  port: 7740\n"),
        Some(7740)
    );
    assert_eq!(parse_agent_port("server:\n  port: 18080\n"), Some(18080));
    assert_eq!(
        parse_agent_port("server:\n  host: \"127.0.0.1\"\n"),
        None,
        "absent port"
    );
    assert_eq!(
        parse_agent_port("serial:\n  port: 1234\n"),
        None,
        "non-server section ignored"
    );
    assert_eq!(
        parse_agent_port("server:\n  port: 0\n"),
        None,
        "ephemeral rejected"
    );
    assert_eq!(
        parse_agent_port("server:\n  port: 99999\n"),
        None,
        "out of range rejected"
    );
    assert_eq!(
        parse_agent_port("server:\n  port: abc\n"),
        None,
        "non-numeric rejected"
    );
    assert_eq!(parse_agent_port(""), None);
}

/// JS: `test("controlOriginOk: the loopback control API's origin veto")`
#[test]
fn control_origin_ok_the_loopback_control_apis_origin_veto() {
    // Allowed: the desktop SPA and any loopback port.
    assert!(control_origin_ok(Some("http://127.0.0.1:9444")));
    assert!(control_origin_ok(Some("http://127.0.0.1:18080")));
    assert!(control_origin_ok(Some("http://localhost:9444")));
    assert!(control_origin_ok(Some("file:///C:/x/wait.html")));
    // ABSENT and "null" are deliberate: curl and native tooling send no Origin, and the data: wait
    // page sends "null".
    assert!(control_origin_ok(None));
    assert!(control_origin_ok(Some("")));
    assert!(control_origin_ok(Some("null")));

    // THE LOOKALIKES THE OLD REGEX ADMITTED — each one a page an attacker hosts.
    assert!(
        !control_origin_ok(Some("http://127.0.0.1.evil.com")),
        "sibling-host lookalike"
    );
    assert!(
        !control_origin_ok(Some("http://localhost.evil.com")),
        "sibling-host lookalike"
    );
    assert!(
        !control_origin_ok(Some("http://127.0.0.1x")),
        "trailing character"
    );
    assert!(!control_origin_ok(Some("http://127.0.0.1.evil.com:9444")));
    // The userinfo trick from IPC audit #1, applied to this path.
    assert!(
        !control_origin_ok(Some("http://127.0.0.1:9444@evil.com")),
        "userinfo trick"
    );
    assert!(
        control_origin_ok(Some("https://127.0.0.1:9444")),
        "https loopback is still loopback"
    );
    assert!(!control_origin_ok(Some("http://evil.com")));
    assert!(
        !control_origin_ok(Some("http://192.168.1.5:9444")),
        "not loopback"
    );
    assert!(!control_origin_ok(Some("chrome-extension://abcd")));
    // A value the policy cannot READ must never be treated as one it recognises.
    assert!(!control_origin_ok(Some("not a url")));
}

/// JS: `test("isDshUrl: the DSH view reaches its own loopback port and nothing else")`
#[test]
fn is_dsh_url_the_dsh_view_reaches_its_own_loopback_port_and_nothing_else() {
    let _restored = PortsRestored;
    assert_eq!(
        get_dsh_port(),
        18081,
        "the default is the port the component listens on"
    );
    assert_eq!(dsh_base(), "http://127.0.0.1:18081");
    assert!(is_dsh_url("http://127.0.0.1:18081/"));
    assert!(is_dsh_url("http://127.0.0.1:18081/api/remote.mux"));
    // The agent's own port is a DIFFERENT origin: the DSH view must not be able to load the panel.
    assert!(
        !is_dsh_url("http://127.0.0.1:18080/panel/"),
        "the agent port is not the DSH port"
    );
    // The lookalikes this module exists to kill, applied to this door.
    assert!(
        !is_dsh_url("http://127.0.0.1:18081@evil.com/"),
        "userinfo trick"
    );
    assert!(
        !is_dsh_url("http://127.0.0.1.evil.com:18081/"),
        "sibling-host lookalike"
    );
    assert!(
        !is_dsh_url("https://127.0.0.1:18081/"),
        "scheme matters: the DSH is plain http on loopback"
    );
    assert!(!is_dsh_url("http://evil.com/"));
    assert!(!is_dsh_url("not a url"));
    assert!(!is_dsh_url(""));
    // A configured port is followed, and the default comes back (no state leaks between tests).
    set_dsh_port(19999.0);
    assert!(is_dsh_url("http://127.0.0.1:19999/"));
    assert!(!is_dsh_url("http://127.0.0.1:18081/"));
    set_dsh_port(f64::from(DEFAULT_DSH_PORT));
}

/// JS: `test("isDshUrl: a second host's harness is its own door, and a third thing is still not")`
///
/// The `.mjs` block carried this MUTATION line, which is kept because it is the reason the case exists:
/// *make `addDshPort` push into the allow-list without the port range check and this test sees
/// `http://127.0.0.1:70000/` accepted — an out-of-range "origin" no forward can hold.*
#[test]
fn is_dsh_url_a_second_hosts_harness_is_its_own_door_and_a_third_thing_is_still_not() {
    let _restored = PortsRestored;
    set_dsh_port(f64::from(DEFAULT_DSH_PORT));
    clear_extra_dsh_ports();

    // the first host: this device's own harness, unchanged
    assert_eq!(dsh_base(), "http://127.0.0.1:18081");
    assert!(is_dsh_url("http://127.0.0.1:18081/"));

    // a second host's forward
    add_dsh_port(7801.0).expect("7801 is a port");
    assert!(
        is_dsh_url("http://127.0.0.1:7801/"),
        "the second host's harness is reachable"
    );
    assert!(
        is_dsh_url("http://127.0.0.1:7801/api/remote.mux"),
        "and its own API"
    );
    assert!(
        is_dsh_url("http://127.0.0.1:18081/"),
        "the first host is still reachable: selecting another must not un-reach this one"
    );

    // and the refusals the single-port test already pins, on the NEW door too
    assert!(
        !is_dsh_url("http://127.0.0.1:7801@evil.com/"),
        "userinfo trick, on a second door"
    );
    assert!(
        !is_dsh_url("http://127.0.0.1.evil.com:7801/"),
        "sibling-host lookalike, ditto"
    );
    assert!(
        !is_dsh_url("https://127.0.0.1:7801/"),
        "scheme matters, ditto"
    );
    assert!(
        !is_dsh_url("http://127.0.0.1:18080/panel/"),
        "the agent port is never a harness"
    );
    assert!(
        !is_dsh_url("http://127.0.0.1:7802/"),
        "a port nobody added is not a door"
    );

    // a port outside the range is refused BY NAME rather than added — the JavaScript's `Error`
    // message, which the wasm wrapper re-throws as a real `Error` (pinned in the Node test).
    assert_eq!(add_dsh_port(0.0).unwrap_err(), "not a port: 0");
    assert_eq!(add_dsh_port(70000.0).unwrap_err(), "not a port: 70000");
    assert!(!is_dsh_url("http://127.0.0.1:70000/"));

    // forgetting a host's door takes it away again — the list is not append-only
    clear_extra_dsh_ports();
    assert!(
        !is_dsh_url("http://127.0.0.1:7801/"),
        "a host that was dropped is no longer a door"
    );
    assert!(
        is_dsh_url("http://127.0.0.1:18081/"),
        "and the primary door is untouched"
    );

    set_dsh_port(f64::from(DEFAULT_DSH_PORT));
}
