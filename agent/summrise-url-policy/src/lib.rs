//! summrise-url-policy — the Electron shell's URL, origin and certificate policy, in Rust.
//!
//! Landing 6 of `docs/superpowers/specs/2026-10-08-every-decision-is-rust-design.md`. It replaces
//! `agent/summrise-desktop-electron/src/url-policy.ts` (222 lines, 11 cases in its `.mjs` suite) and
//! is loaded by the shell's main process as a **wasm module built with `wasm-pack --target nodejs`**.
//!
//! # Why wasm and not a napi addon
//!
//! The alternative was a napi native addon, which the Windows cross-build the release already runs
//! would produce for free. It cannot be EXECUTED on the Linux box this repository is developed on, or
//! in CI — so its correctness could only ever be discovered on the device, by the operator, after a
//! release. wasm runs everywhere, which is the property that makes this port's own tests run at all:
//! `cargo test -p summrise-url-policy` runs the 11 ported cases natively, and
//! `agent/summrise-desktop-electron/test/url-policy-wasm.test.mjs` loads the COMMITTED artifact in
//! Node and calls every export, so the shipped bytes are executed by a suite rather than only by
//! Electron on Windows. The cost is the artifact itself, and it is paid explicitly: the glue and the
//! `.wasm` are committed, named in `agent/summrise-agent-npm/required-in-tgz.txt` and in that
//! package's `files[]`, and rebuilt-and-compared by
//! `scripts/test/url-policy-wasm-freshness.bash`.
//!
//! # THE HOST STAYS TYPESCRIPT — this crate ports DECISIONS, not the shell
//!
//! `CDP :9333` is a product contract (`agent/tests/mcp_autoselect_integration.rs` binds it, the CLI
//! probes it, playwright `connectOverCDP` drives the operator's visible page, the design sweep's
//! attached mode depends on it), so `main.ts` keeps its windows, its IPC door, its CDP self-check and
//! its agent lifecycle. What moves here is every predicate those surfaces ASK — and `main.ts` asks
//! them through this module rather than keeping a second copy.
//!
//! # The rule every function here obeys
//!
//! It is the TypeScript it replaces, transliterated: the same predicates in the same order, the same
//! carve-outs, the same refusals — including the ones that look like accidents, because they are
//! decisions somebody made (`controlOriginOk` allows an ABSENT origin deliberately: `curl` and native
//! tooling send none; `dshOrigins()` returns raw strings, so a DSH port of 80 does not match, exactly
//! as the JavaScript did). Boundary conditions are transliterated too, which is what [`js`] is for.
//!
//! # The state, and why it is thread-local
//!
//! The TypeScript kept the agent's and the DSH's ports in module-level bindings, mutable through
//! `setAgentPort` / `setDshPort` / `addDshPort`, and reset by tests. This crate keeps the same three
//! pieces of state; the difference is that they are `thread_local!` here.
//!
//! In a wasm module that distinction does not exist — wasm without threads is one instance with one
//! thread, so a thread-local IS the module global the shell configured. It matters only to
//! `cargo test`, which runs the ported cases on several threads at once: a process-wide static would
//! let the DSH case's `setDshPort(19999)` race the second DSH case's `setDshPort(18081)`, and the
//! suite would fail intermittently for a reason that has nothing to do with the policy. The JS suite
//! was single-threaded and restored its ports in a `finally`; these cases restore them in a `Drop`
//! guard, so the suite is order-independent either way.

use std::cell::{Cell, RefCell};

pub mod js;

use url::Url;

#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(test)]
mod tests;

/// The agent's canonical port (the TypeScript's `18080`), used whenever none was configured.
pub const DEFAULT_AGENT_PORT: u16 = 18080;
/// The DSH component's default port (the TypeScript's `18081`).
pub const DEFAULT_DSH_PORT: u16 = 18081;

/// The port range every setter applies: `Number.isInteger(port) && port > 0 && port < 65536`, on an
/// `f64` because that is what JavaScript passes. `Number.isInteger` is NOT "a number": it is finite,
/// integral, and not NaN — and an integral `f64` is what a wasm `f64` parameter must be re-checked
/// for, since a non-integral value would otherwise be silently truncated at the boundary.
fn valid_port(port: f64) -> Option<u16> {
    if port.is_finite() && port.fract() == 0.0 && port > 0.0 && port < 65536.0 {
        Some(port as u16)
    } else {
        None
    }
}

thread_local! {
    /// The configured agent port; `None` is the TypeScript's `null` ("follow the default").
    static AGENT_PORT: Cell<Option<u16>> = const { Cell::new(None) };
    /// The configured DSH port; `None` is the default.
    static DSH_PORT: Cell<Option<u16>> = const { Cell::new(None) };
    /// The extra DSH doors, in the order they were added — a `Vec` and not a `BTreeSet`, because the
    /// TypeScript's `Set<number>` is INSERTION-ordered and `dshOrigins()` reports that order.
    static EXTRA_DSH_PORTS: RefCell<Vec<u16>> = const { RefCell::new(Vec::new()) };
}

/// `setAgentPort`: an invalid port is IGNORED, never a panic and never a reset to the default. The
/// shell calls this once at boot with an environment value, a `config.yaml` value or `18080`, and a
/// predicate that ran before it would otherwise be pinned to a port nothing is listening on.
pub fn set_agent_port(port: f64) {
    if let Some(p) = valid_port(port) {
        AGENT_PORT.with(|c| c.set(Some(p)));
    }
}

/// `getAgentPort`.
pub fn get_agent_port() -> u16 {
    AGENT_PORT.with(|c| c.get()).unwrap_or(DEFAULT_AGENT_PORT)
}

/// `agentBase` — the origin the shell probes and loads, as a string.
pub fn agent_base() -> String {
    format!("http://127.0.0.1:{}", get_agent_port())
}

/// `new URL(agentBase()).origin`, through the parser rather than by formatting.
///
/// THIS IS NOT THE SAME STRING AS `agent_base()`, and the difference is the reason it is computed
/// this way: for a port of 80 the WHATWG serializer ELIDES the port, so the origin of
/// `http://127.0.0.1:80` is `http://127.0.0.1`. The TypeScript compared `new URL(url).origin` against
/// `new URL(agentBase()).origin`, so a configured port of 80 matched `http://127.0.0.1/` and not
/// `http://127.0.0.1:80/`; formatting the string here would have inverted that without a test.
fn agent_origin() -> String {
    Url::parse(&agent_base())
        .expect("agentBase() builds a URL this module owns")
        .origin()
        .ascii_serialization()
}

/// `new URL(url).origin`, or `None` where the JavaScript constructor THROWS.
///
/// An opaque origin answers `Some("null")`, which is what `new URL("data:…").origin` answers too —
/// so a caller comparing it to a tuple origin refuses it for the same reason on both sides.
fn origin_of(url: &str) -> Option<String> {
    Url::parse(url)
        .ok()
        .map(|u| u.origin().ascii_serialization())
}

/// `isBaseOrigin`: is this URL on the agent's own origin?
///
/// The comparison is the PARSED origin, which is the fix IPC audit #1 records: the string prefix
/// `http://127.0.0.1:18080` is a prefix of `http://127.0.0.1:18080@evil.com/x`, whose host Chromium
/// parses as `evil.com`.
pub fn is_base_origin(url: &str) -> bool {
    origin_of(url).as_deref() == Some(agent_origin().as_str())
}

/// `frameUrlOk`: may a frame at this URL invoke the IPC bridge?
///
/// The TypeScript forwarded `url || ""`, so a missing URL was the empty string and the empty string
/// did not parse. A `&str` parameter cannot be missing, and "" does not parse here either — the
/// door's answer is unchanged, and `main.ts` still passes `e.senderFrame?.url || ""`.
pub fn frame_url_ok(url: &str) -> bool {
    is_base_origin(url)
}

/// `controlOriginOk` — the loopback control API's origin veto (port 9444).
///
/// An ABSENT origin (or the literal `"null"` a `data:` page sends) is ALLOWED DELIBERATELY: the API is
/// also driven by `curl` and native tooling, which send none, and `main.ts` documents that carve-out.
/// This is a nuisance barrier for browser pages, NOT authentication — the IPC bridge stays separately
/// gated on the frame's own origin. An UNPARSEABLE value is not allowed: a value the policy cannot
/// read must never be treated as one it recognises.
pub fn control_origin_ok(origin: Option<&str>) -> bool {
    match origin {
        None => true,
        Some(origin) if origin.is_empty() || origin == "null" => true,
        Some(origin) => control_origin_allowed(origin),
    }
}

/// The TypeScript's body AFTER its `if (!origin || origin === "null") return true;`.
///
/// IT IS A SEPARATE FUNCTION BECAUSE THE FALSY TEST BELONGS TO THE UNCOERCED VALUE, and the differential
/// over 927 inputs is what found that out: `controlOriginOk([])` is `false` in the TypeScript — `[]` is
/// TRUTHY, so it reaches `new URL([])`, whose ToString is the empty string, which does not parse — while
/// a literal `""` never reaches the parse at all and is allowed. A boundary that coerced first and then
/// applied the falsy test answered `true` for `[]`, and the 11 ported cases could not see it: every one
/// of them passes a string.
pub(crate) fn control_origin_allowed(origin: &str) -> bool {
    match Url::parse(origin) {
        Ok(u) => {
            if u.scheme() == "file" {
                return true;
            }
            if u.scheme() != "http" && u.scheme() != "https" {
                return false;
            }
            matches!(u.host_str(), Some("127.0.0.1") | Some("localhost"))
        }
        Err(_) => false,
    }
}

/// `isDesktopSpaUrl` — the main window's tripwire allow-list: the desktop SPA subtree of the base
/// origin, by PARSED origin and PARSED pathname. `/desktopx` is a different path, not the SPA mount.
pub fn is_desktop_spa_url(url: &str) -> bool {
    match Url::parse(url) {
        Ok(u) => {
            if u.origin().ascii_serialization() != agent_origin() {
                return false;
            }
            let path = u.path();
            path == "/desktop" || path.starts_with("/desktop/")
        }
        Err(_) => false,
    }
}

/// `isPrivateHost` — may this host's certificate errors be bypassed?
///
/// A STRING predicate on purpose, exactly as the TypeScript was: `main.ts` feeds it the raw hostname
/// it already has, and `certBypassAllowed` feeds it the PARSED one. `.local` and the RFC1918 blocks
/// plus loopback and link-local; the public internet keeps full validation, because a bypassed MITM
/// on a lab LAN is contained and on the open web it is not. `192.168.1.1.evil.com` is a suffix
/// lookalike and is refused — the parts must BE four numbers, not merely end in some.
pub fn is_private_host(hostname: &str) -> bool {
    let h = js::trim(hostname).to_lowercase();
    if h.is_empty() {
        return false;
    }
    if h == "localhost" || h == "::1" || h == "[::1]" {
        return true;
    }
    if h.ends_with(".local") || h.ends_with(".local.") {
        return true;
    }
    let parts: Vec<&str> = h.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    let mut nums = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        // `^\d{1,3}$` with JAVASCRIPT's `\d`, which is `[0-9]`; a Unicode `\d` would accept `٣` and
        // `Number("٣")` would not, so the digit test and the parse must agree on ASCII.
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        match part.parse::<u16>() {
            Ok(n) if n <= 255 => nums[i] = n as u8,
            _ => return false,
        }
    }
    let [a, b, _, _] = nums;
    a == 10
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || a == 127
        || (a == 169 && b == 254)
}

/// `certBypassAllowed` — the gate for `app.on("certificate-error")`: `true` bypasses (the shell
/// `preventDefault`s and calls back `true`), `false` denies. `http`/`https` on private hosts only:
/// public hosts, other schemes and unparseable input stay fully validated.
pub fn cert_bypass_allowed(url: &str) -> bool {
    match Url::parse(url) {
        Ok(u) => {
            if u.scheme() != "http" && u.scheme() != "https" {
                return false;
            }
            // `u.hostname`: for an IPv6 URL the WHATWG accessor keeps its brackets, and so does
            // `Url::host_str()`, which is why this comparison is the same one on both sides.
            is_private_host(u.host_str().unwrap_or(""))
        }
        Err(_) => false,
    }
}

/// `sanitizeBrowserUrl` — the ONE load door's decision. `http`/`https`/`about:blank` only; `file://`
/// and every other scheme collapse to `about:blank`, because a loadable `file://` would hand the AI a
/// local-file read primitive through the shared CDP endpoint.
pub fn sanitize_browser_url(url: Option<&str>) -> String {
    const BLANK: &str = "about:blank";
    // `String(url || "about:blank").trim()`: a missing OR EMPTY url is the blank page, and the trim
    // is JavaScript's (js::trim), not Rust's.
    let t = js::trim(match url {
        Some(u) if !u.is_empty() => u,
        _ => BLANK,
    });
    if t == BLANK {
        return BLANK.to_string();
    }
    match Url::parse(t) {
        Ok(u) if u.scheme() == "http" || u.scheme() == "https" => u.to_string(),
        _ => BLANK.to_string(),
    }
}

/// `setDshPort`. The DSH's port is deliberately NOT the agent's: the agent's origin is where the panel
/// lives, and a view that could load the panel could also load `/api/*` with the panel's authority.
pub fn set_dsh_port(port: f64) {
    if let Some(p) = valid_port(port) {
        DSH_PORT.with(|c| c.set(Some(p)));
    }
}

/// `getDshPort`.
pub fn get_dsh_port() -> u16 {
    DSH_PORT.with(|c| c.get()).unwrap_or(DEFAULT_DSH_PORT)
}

/// `dshBase` — the primary door, as a string. NOT parsed, exactly as the TypeScript's was: see
/// `is_dsh_url` for the difference that makes.
pub fn dsh_base() -> String {
    format!("http://127.0.0.1:{}", get_dsh_port())
}

/// `addDshPort` — admit ONE host's forward as a door.
///
/// The range check is a REFUSAL THE CALLER CAN PRINT rather than a silent skip: the agent owns the
/// list (`/api/workspace/harnesses`) and the shell admits what the answer names, so a port the shell
/// cannot use must be visible rather than swallowed. The message is the TypeScript's.
pub fn add_dsh_port(port: f64) -> Result<(), String> {
    match valid_port(port) {
        Some(p) => {
            EXTRA_DSH_PORTS.with(|ports| {
                let mut ports = ports.borrow_mut();
                if !ports.contains(&p) {
                    ports.push(p);
                }
            });
            Ok(())
        }
        None => Err(format!("not a port: {port}")),
    }
}

/// `clearExtraDshPorts` — the list is not append-only: a host that was dropped stops being a door.
pub fn clear_extra_dsh_ports() {
    EXTRA_DSH_PORTS.with(|ports| ports.borrow_mut().clear());
}

/// `dshOrigins` — every origin the harness view may load, the primary door first.
///
/// The TypeScript built a `Set` seeded with the primary port and mapped it to RAW origin strings. This
/// keeps both properties: the primary port is first, insertion order follows, and the strings are not
/// run back through the parser — so a DSH port of 80 yields `http://127.0.0.1:80`, which no parsed
/// origin will ever equal. That asymmetry is the original behaviour, not a bug to fix here.
pub fn dsh_origins() -> Vec<String> {
    let mut ports = vec![get_dsh_port()];
    EXTRA_DSH_PORTS.with(|extra| {
        for port in extra.borrow().iter() {
            if !ports.contains(port) {
                ports.push(*port);
            }
        }
    });
    ports
        .into_iter()
        .map(|port| format!("http://127.0.0.1:{port}"))
        .collect()
}

/// `isDshUrl` — the DSH view's door: a SET of loopback origins (one per host's forward), and never
/// anything else. Everything the single-port rule refused stays refused on every door: the userinfo
/// trick, the sibling-host lookalike, `https`, and any port nobody added.
pub fn is_dsh_url(url: &str) -> bool {
    match origin_of(url) {
        Some(origin) => dsh_origins().contains(&origin),
        None => false,
    }
}

/// `parseAgentPort` — `server.port` out of an agent `config.yaml`, or `None`.
///
/// The TypeScript walked the lines, and so does this: a TOP-LEVEL line (`/^\S/`) decides whether the
/// following indented lines are inside a `server:` section, and the first `port:` inside one is the
/// answer. A port outside the range answers `None` IMMEDIATELY (the JavaScript `return null` inside
/// the `if (m)` block) rather than continuing to a later line; a line whose port does not MATCH the
/// pattern at all is skipped, exactly as `/…/.exec(line)` returning null did.
pub fn parse_agent_port(yaml_text: &str) -> Option<u16> {
    use regex::Regex;
    use std::sync::OnceLock;

    // The three patterns of the JavaScript, with its classes written out (see `js`).
    fn top_level() -> &'static Regex {
        static R: OnceLock<Regex> = OnceLock::new();
        R.get_or_init(|| Regex::new(&format!(r"^{}", js::JS_NOT_WS)).expect("static pattern"))
    }
    fn server_line() -> &'static Regex {
        static R: OnceLock<Regex> = OnceLock::new();
        R.get_or_init(|| Regex::new(&format!(r"^server{}*:", js::JS_WS)).expect("static pattern"))
    }
    fn port_line() -> &'static Regex {
        static R: OnceLock<Regex> = OnceLock::new();
        R.get_or_init(|| {
            Regex::new(&format!(
                r#"^{ws}*port{ws}*:{ws}*"?([0-9]{{1,5}})"?{ws}*(?:#{dot}*)?$"#,
                ws = js::JS_WS,
                dot = js::JS_DOT
            ))
            .expect("static pattern")
        })
    }

    let mut in_server = false;
    for raw in js::split_lines(yaml_text) {
        let line = js::trim_end(raw);
        if top_level().is_match(line) {
            in_server = server_line().is_match(line);
        }
        if !in_server {
            continue;
        }
        if let Some(caps) = port_line().captures(line) {
            // `[0-9]{1,5}` cannot fail to parse as a u32, and the range check is the same
            // `Number.isInteger(n) && n > 0 && n < 65536` the setters apply.
            let n: u32 = caps[1].parse().expect("1-5 ASCII digits");
            return valid_port(f64::from(n));
        }
    }
    None
}
