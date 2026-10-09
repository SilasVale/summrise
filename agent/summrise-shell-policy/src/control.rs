//! THE LOOPBACK CONTROL SERVER (port 9444) — its request parse, its router, its preflight and its CORS.
//!
//! This is the shell's fallback path for when the SPA runs in a plain browser rather than under the
//! preload bridge, and `main.ts` records what that costs: "loopback is exempt from mixed-content
//! blocking", so ANY page open in ANY local browser could previously POST `/api/shell/start-agent`.
//! The origin VETO is already Rust (`summrise_url_policy::control_origin_ok`, landing 6a); what moves
//! here is everything around it — which route a request is, what a preflight is answered with before the
//! veto runs, and which CORS headers the answer carries.
//!
//! `http.createServer`, the `res` calls and the async handler bodies are effects and stay in `main.ts`.

/// The base `new URL(req.url, …)` resolves against. A base is required because `req.url` is a PATH.
const CONTROL_BASE: &str = "http://127.0.0.1";

/// The preflight answer's methods list.
pub const CORS_ALLOW_METHODS: &str = "GET, POST, OPTIONS";
/// The preflight answer's headers list.
pub const CORS_ALLOW_HEADERS: &str = "content-type, authorization";
/// The `Vary` the answer carries, because the allow-origin header is reflected.
pub const CORS_VARY: &str = "Origin";

/// The refusal a foreign origin gets on the control API.
pub const FORBIDDEN_ORIGIN: &str = "forbidden origin";
/// The refusal an unknown route gets.
pub const NOT_FOUND: &str = "not found";

/// Which handler a request reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlRoute {
    BrowserOpen,
    BrowserClose,
    BrowserList,
    StartAgent,
    AgentStatus,
    IconStatus,
    NotFound,
}

impl ControlRoute {
    /// The name that crosses the wasm boundary.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BrowserOpen => "browser-open",
            Self::BrowserClose => "browser-close",
            Self::BrowserList => "browser-list",
            Self::StartAgent => "start-agent",
            Self::AgentStatus => "agent-status",
            Self::IconStatus => "icon-status",
            Self::NotFound => "not-found",
        }
    }
}

/// `new URL(req.url || "/", "http://127.0.0.1")`, or `None` where the JavaScript would have THROWN.
///
/// **THE ONE DELIBERATE DIVERGENCE IN THIS CRATE, AND IT IS NAMED RATHER THAN SLIPPED IN.** In `main.ts`
/// the parse sits OUTSIDE the request handler's `try`, so a malformed `req.url` (a header line smuggling
/// `http://[::1` into the request line, say) throws out of the listener and takes the Electron MAIN
/// PROCESS with it — every window, the tray and the agent's control channel at once. This answers `None`,
/// the route lookup finds nothing, and the request gets a 404. A crash is not one of the decisions
/// somebody made; it is what a port is allowed to fix, and the fix is recorded here so the difference is
/// auditable rather than discovered.
pub fn control_url(raw_url: Option<&str>) -> Option<url::Url> {
    let base = url::Url::parse(CONTROL_BASE).ok()?;
    let path = match raw_url {
        Some(url) if !url.is_empty() => url,
        _ => "/",
    };
    base.join(path).ok()
}

/// `u.pathname` — or `None`, which the caller turns into a 404.
pub fn control_path(raw_url: Option<&str>) -> Option<String> {
    control_url(raw_url).map(|parsed| parsed.path().to_string())
}

/// `u.searchParams.get(key)` — the FIRST value for the key, percent-decoded, with `+` as a space.
///
/// The decoding matters and is JavaScript's: `embedded-browser:navigate` is reachable over this server
/// and its `url` parameter arrives percent-encoded, so the search-parameter decode IS the load door's
/// input. `Url::query_pairs` and `URLSearchParams` agree on `+` (both decode it to a space) and on an
/// invalid escape (both substitute U+FFFD) — checked rather than assumed, because a decoder that
/// answered the raw text would hand `sanitizeBrowserUrl` a different URL than Chromium's would.
pub fn control_query(raw_url: Option<&str>, key: &str) -> Option<String> {
    let parsed = control_url(raw_url)?;
    parsed
        .query_pairs()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.into_owned())
}

/// `req.method === "OPTIONS"` — a PREFLIGHT, answered before anything else.
///
/// IT IS A FUNCTION BECAUSE OF WHERE IT SITS, not because the comparison is hard: in `main.ts` the
/// preflight is answered BEFORE the foreign-origin veto, so the veto does not apply to it. Moving the
/// check without moving that fact is how a CORS preflight from a legitimate page would start getting a
/// 403 and the plain-browser path would stop working with no other symptom.
pub fn control_is_preflight(method: &str) -> bool {
    method == "OPTIONS"
}

/// The route table, in the TypeScript's order.
///
/// TWO ASYMMETRIES ARE THE ORIGINAL BEHAVIOUR AND ARE PRESERVED ON PURPOSE:
///
///   * `/api/browser-session/list`, `/api/shell/agent-status` and `/api/shell/icon-status` have NO
///     method guard, so a POST reaches the same read handler a GET does.
///   * `/api/browser-session/open`, `…/close` and `/api/shell/start-agent` require POST, so a GET to one
///     of them falls through to the 404 rather than reaching the handler.
///
/// A port that "tidied" this into a `(method, path)` table with a uniform rule would change which
/// requests the shell answers. The `Not Found` arm is last, exactly as the fall-through was.
pub fn control_route(method: &str, pathname: &str) -> ControlRoute {
    match pathname {
        "/api/browser-session/open" if method == "POST" => ControlRoute::BrowserOpen,
        "/api/browser-session/close" if method == "POST" => ControlRoute::BrowserClose,
        "/api/browser-session/list" => ControlRoute::BrowserList,
        "/api/shell/start-agent" if method == "POST" => ControlRoute::StartAgent,
        "/api/shell/agent-status" => ControlRoute::AgentStatus,
        "/api/shell/icon-status" => ControlRoute::IconStatus,
        _ => ControlRoute::NotFound,
    }
}

/// `req.headers.origin || "*"` — the ORIGIN THE ANSWER REFLECTS.
///
/// It used to be `Access-Control-Allow-Origin: *` with no check at all, which is the defect review #8
/// records. The reflection is what replaced it: the veto above decides WHETHER to answer, and this
/// decides what the answer says. An absent `Origin` (native tooling, `curl`) is not a header a browser
/// will enforce, and `*` is what the original sent in that case.
pub fn cors_allow_origin(origin: Option<&str>) -> String {
    match origin {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => "*".to_string(),
    }
}
