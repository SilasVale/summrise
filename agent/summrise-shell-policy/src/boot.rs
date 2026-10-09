//! BOOT: the token, the ports, the icons, the AUMID and the shell's policy constants.
//!
//! Everything here is asked ONCE, before any window or probe exists — which is why `main.ts`'s comments
//! say a predicate that ran before it "would be pinned to a port nothing is listening on". The decisions
//! are the questions; the reads (`fs.readFileSync`, `process.env`, `process.platform`, `__dirname`) are
//! the host's and stay there.

use regex::Regex;
use serde_json::json;
use std::sync::OnceLock;

/// The agent's canonical port (the TypeScript's `18080`), used whenever neither the environment nor the
/// device's `config.yaml` names one.
pub const DEFAULT_AGENT_PORT: u16 = 18080;
/// The DSH component's default port (the TypeScript's `18081`).
pub const DEFAULT_DSH_PORT: u16 = 18081;

/// The device token's cache lifetime: `Date.now() - _tokenCache.at < 60_000`.
pub const TOKEN_CACHE_MS: f64 = 60_000.0;

/// `Number.isInteger(port) && port > 0 && port < 65536`, on an `f64` because that is what JavaScript
/// passes. `Number.isInteger` is NOT "a number": it is finite, integral and not NaN — and the same test
/// is what `agent/summrise-url-policy` applies in its own setters, which is why it is written here in
/// the same shape rather than imported through a `pub(crate)`.
pub fn valid_port(port: f64) -> Option<u16> {
    if port.is_finite() && port.fract() == 0.0 && port > 0.0 && port < 65536.0 {
        Some(port as u16)
    } else {
        None
    }
}

/// `device_token` out of the agent's `config.yaml`, or `None`.
///
/// THE PATTERN IS DELIBERATELY NARROWER THAN THE TWO OTHER PARSERS OF THE SAME LINE, and `main.ts` says
/// so where it uses this: the CLI's (`bin/summrise.js`, `[A-Za-z0-9._-]+`) and the agent's Rust recovery
/// both accept more than lowercase hex. Measured against a live device on 2026-09-24, the token is 64 hex
/// characters with no space before the colon, so all three agree — and the failure mode if that ever
/// stops being true is SILENT: no credential, a 401, a title that stays "Summrise" and vitals that stay
/// blank. The three-way divergence is not fixed here (that would be a behaviour change nobody measured);
/// it is made visible, which is what a port can honestly do.
///
/// The pattern is JavaScript's `/device_token:\s*"?([0-9a-f]{16,})"?/`, with `\s` written out from
/// [`crate::js::JS_WS`] because the regex crate's `\s` is a different set. `exec` finds the LEFTMOST
/// match anywhere in the document, and so does `Regex::captures` — including the consequence that a
/// first `device_token:` line whose value is too short is skipped in favour of a later one.
pub fn device_token(config_yaml: &str) -> Option<String> {
    fn pattern() -> &'static Regex {
        static R: OnceLock<Regex> = OnceLock::new();
        R.get_or_init(|| {
            Regex::new(&format!(
                r#"device_token:{ws}*"?(?P<tok>[0-9a-f]{{16,}})"?"#,
                ws = crate::js::JS_WS
            ))
            .expect("static pattern")
        })
    }
    pattern()
        .captures(config_yaml)
        .map(|caps| caps["tok"].to_string())
}

/// `Date.now() - _tokenCache.at < 60_000`: is the cached token still usable?
///
/// The cache exists because the shell reads `config.yaml` on every vitals poll and the file is on disk;
/// the TTL is the decision, the read is the host's. `_tokenCache` starts at `{ at: 0 }`, so the first
/// call is always a read.
pub fn token_cache_fresh(cached_at_ms: f64, now_ms: f64) -> bool {
    now_ms - cached_at_ms < TOKEN_CACHE_MS
}

/// `Bearer <token>`, or `None` where the TypeScript answered `{}`.
///
/// The falsy test is on the TOKEN, not on the answer: `t ? { authorization: … } : {}`, and a token of
/// `""` is falsy in JavaScript, so an empty credential sends NO header rather than `Bearer `.
pub fn authorization(token: Option<&str>) -> Option<String> {
    match token {
        Some(t) if !t.is_empty() => Some(format!("Bearer {t}")),
        _ => None,
    }
}

/// `resolveAgentPort()` — the PRECEDENCE is the decision: the environment first, then `server.port` out
/// of the device's own `config.yaml`, then the canonical port.
///
/// `env` is the value `Number(process.env.SUMMRISE_AGENT_PORT)` produced — an `f64` INCLUDING the NaN a
/// missing variable yields, which [`valid_port`] refuses — so the JavaScript `Number()` coercion happens
/// at the wasm boundary where JavaScript can do it, and the range test and the fallback happen here.
///
/// `config_port` is `summrise_url_policy::parse_agent_port`'s ANSWER, evaluated by the host. That is the
/// shape the two crates' independence forces and it is the better one anyway: **the YAML walk stays in
/// the crate that owns it**, with its `server:` section rule and its `\s`/`.` boundary conditions, and
/// this function owns only the ORDER the two sources are consulted in — which is the part that was
/// written down in `main.ts` and tested nowhere. See `Cargo.toml` for why a dependency is not available.
pub fn resolve_agent_port(env: Option<f64>, config_port: Option<u16>) -> u16 {
    if let Some(port) = env.and_then(valid_port) {
        return port;
    }
    if let Some(port) = config_port {
        return port;
    }
    DEFAULT_AGENT_PORT
}

/// `resolveDshPort()` — the environment, else the component's default.
///
/// IT HAS NO `config.yaml` FALLBACK, AND THAT IS NOT AN OVERSIGHT TO FIX: the DSH is a separate
/// component whose port the deployment sets, and the agent's `config.yaml` says nothing about it. The
/// asymmetry with [`resolve_agent_port`] is the original behaviour.
pub fn resolve_dsh_port(env: Option<f64>) -> u16 {
    env.and_then(valid_port).unwrap_or(DEFAULT_DSH_PORT)
}

/// The tray's icon on Windows — `.ico`, which is what the Windows shell requires.
pub const TRAY_ICON_WIN: &str = "icon.ico";
/// The tray's icon everywhere else, and the WINDOW's icon on every platform.
pub const TRAY_ICON_OTHER: &str = "icon.png";
/// The main window's icon: `.png` on ALL platforms, deliberately.
///
/// `main.ts` carries the measurement: Skia decodes PNG reliably while Chromium's ICO parser has choked
/// on PNG-compressed 256px entries, silently falling back to the stock `electron.exe` icon — device-caught.
pub const WINDOW_ICON: &str = "icon.png";

/// `appIcon()`'s choice: the win32 tray needs `.ico`, everything else takes `.png`.
pub fn tray_icon_name(platform: &str) -> &'static str {
    if platform == "win32" {
        TRAY_ICON_WIN
    } else {
        TRAY_ICON_OTHER
    }
}

/// `windowIcon()`'s name. A function rather than a bare constant so the wasm surface has one name per
/// decision, and so a future platform carve-out has a place to be written.
pub fn window_icon_name() -> &'static str {
    WINDOW_ICON
}

/// The `AppUserModelID` set on the main window. **IT IS A COPY OF THE CLI's `DESKTOP_AUMID` AND THE TWO
/// MUST STAY IN STEP** (`agent/summrise-agent-npm/src/summrise.ts`, which also writes
/// `System.AppUserModel.RelaunchIconResource`), and `main.ts` says why the literal is a copy: the file is
/// emitted to plain JS in two packages and cannot import the CLI's module.
///
/// THE VALUE IS NOT THE ONE THE FILE USED BEFORE (`…summrise.agent`) and that is the measured half of
/// the fix rather than a rename: on `desktop-14rjcr8`, with that string set on the window AND read back
/// off the shortcut, the taskbar still drew the Electron logo, because releases up to 1.2.489 had
/// already resolved that AUMID against the backing executable and kept the answer.
pub const DESKTOP_AUMID: &str = "online.saisi.summrise.desktop";
/// What the icon report says about the AUMID on a platform that has none.
pub const AUMID_NON_WINDOWS: &str = "(non-windows)";
/// What the icon report says when `setAppUserModelId` threw.
pub const AUMID_SET_FAILED: &str = "(set-failed)";

/// Does this platform set an `AppUserModelID` at all? The shell's one platform test for it.
pub fn uses_app_user_model_id(platform: &str) -> bool {
    platform == "win32"
}

/// The value the icon report records for the AUMID on this platform.
pub fn aumid_report(platform: &str) -> &'static str {
    if uses_app_user_model_id(platform) {
        DESKTOP_AUMID
    } else {
        AUMID_NON_WINDOWS
    }
}

/// `agentBase().replace("http://", "")` — the `host:port` the wait page prints twice.
///
/// The BASE ORIGIN is the host's to evaluate (`agentBase()` is the url-policy crate's, and it is where
/// the configured port lives), so it arrives as an argument; the formatting is this crate's.
/// `String.prototype.replace` with a string pattern replaces the FIRST occurrence only.
pub fn agent_host_label(agent_base: &str) -> String {
    agent_base.replacen("http://", "", 1)
}

/// THE IPC DOOR'S ONE REFUSAL — `{ ok: false, error: "forbidden frame" }`, as JSON.
///
/// **IT IS A DECISION BECAUSE THE SPA READS IT.** `main.ts` records the history: the refusal used to have
/// TWO shapes — this one for seven handlers and a bare `{ ok: false }` for the rest — "which the SPA
/// cannot tell apart from a dead view (`j?.ok` is falsy either way, and only this shape carries the
/// reason)". The preload's `invoke` callers were written against THIS shape, so every handler answers it.
///
/// It is built here rather than written at the call site so that "one refusal, stated once" is a property
/// of the source and not a coincidence: the string crosses the boundary as JSON, and `main.ts` freezes
/// what it parses, which is the `Object.freeze` the TypeScript had.
pub const FORBIDDEN_FRAME_ERROR: &str = "forbidden frame";

/// The refusal above, as the JSON object the door returns.
pub fn forbidden_frame_json() -> String {
    json!({ "ok": false, "error": FORBIDDEN_FRAME_ERROR }).to_string()
}

/// EVERY POLICY NUMBER THE SHELL USES, in one place, as JSON.
///
/// These were `const` declarations scattered through 1,468 lines of `main.ts`: the CDP and control ports,
/// the browser-window cap, the watchdog's two thresholds, eight probe and fetch budgets, the `schtasks`
/// timeout, the two scheduled-task names and the CDP marker. None of them is a computation, and all of
/// them are POLICY — the kind of number that has to move together with the code that reads it and cannot
/// be found once it is spread over a file.
///
/// WHY JSON RATHER THAN TWENTY EXPORTS: the wasm module's exported surface is a contract that a Node
/// suite pins name by name (`test/shell-policy-wasm.test.mjs`), and twenty one-line getters would make
/// that list mostly noise. The call site destructures it once, so a reader of `main.ts` still sees
/// `K.MAX_BROWSER_WINDOWS` rather than a literal.
pub fn shell_constants() -> String {
    json!({
        // ── the two doors ────────────────────────────────────────────────────────────────────────────
        "CDP_PORT": 9333,
        "CTRL_PORT": 9444,
        // ── the browser-session windows ──────────────────────────────────────────────────────────────
        "MAX_BROWSER_WINDOWS": crate::sessions::MAX_BROWSER_WINDOWS,
        "MIN_SLOT_PX": crate::sessions::MIN_SLOT_PX,
        // ── the agent lifecycle ──────────────────────────────────────────────────────────────────────
        "AUTOSTART_TASK": crate::lifecycle::AUTOSTART_TASK,
        "AGENT_TASK": crate::lifecycle::AGENT_TASK,
        "SCHTASKS_TIMEOUT_MS": crate::lifecycle::SCHTASKS_TIMEOUT_MS,
        "AUTO_START_AFTER_MISSES": crate::lifecycle::AUTO_START_AFTER_MISSES,
        "AUTO_START_MIN_GAP_MS": crate::lifecycle::AUTO_START_MIN_GAP_MS,
        "RETRY_START_MS": crate::lifecycle::RETRY_START_MS,
        "RETRY_CAP_MS": crate::lifecycle::RETRY_CAP_MS,
        "TRIPWIRE_BACKOFF_MS": crate::lifecycle::TRIPWIRE_BACKOFF_MS,
        "LOG_URL_UNITS": crate::lifecycle::LOG_URL_UNITS,
        "CDP_UA_UNITS": crate::lifecycle::CDP_UA_UNITS,
        "CDP_UA_MARKER": crate::lifecycle::CDP_UA_MARKER,
        // ── the probe and fetch budgets ──────────────────────────────────────────────────────────────
        "AGENT_PROBE_BOOT_MS": crate::lifecycle::AGENT_PROBE_BOOT_MS,
        "AGENT_PROBE_TRAY_MS": crate::lifecycle::AGENT_PROBE_TRAY_MS,
        "AGENT_PROBE_DEFAULT_MS": crate::lifecycle::AGENT_PROBE_DEFAULT_MS,
        "STATUS_FETCH_MS": crate::lifecycle::STATUS_FETCH_MS,
        "HARNESS_FETCH_MS": crate::lifecycle::HARNESS_FETCH_MS,
        "CDP_CHECK_MS": crate::lifecycle::CDP_CHECK_MS,
        "MENU_FLUSH_MS": crate::lifecycle::MENU_FLUSH_MS,
        "TRAY_POLL_MS": crate::lifecycle::TRAY_POLL_MS,
        // ── the control server's answer headers and refusals ─────────────────────────────────────────
        "CORS_ALLOW_METHODS": crate::control::CORS_ALLOW_METHODS,
        "CORS_ALLOW_HEADERS": crate::control::CORS_ALLOW_HEADERS,
        "CORS_VARY": crate::control::CORS_VARY,
        "FORBIDDEN_ORIGIN": crate::control::FORBIDDEN_ORIGIN,
        "NOT_FOUND": crate::control::NOT_FOUND,
        "FORBIDDEN_FRAME_ERROR": FORBIDDEN_FRAME_ERROR,
        // ── the blank page every refusing door collapses to ──────────────────────────────────────────
        "ABOUT_BLANK": crate::sessions::ABOUT_BLANK,
        // ── the app-user-model id and the two strings its report can carry instead ───────────────────
        "DESKTOP_AUMID": DESKTOP_AUMID,
        "AUMID_SET_FAILED": AUMID_SET_FAILED,
        "AUMID_NON_WINDOWS": AUMID_NON_WINDOWS,
        // ── the control server's own quick probe, distinct from the tray's ───────────────────────────
        // `GET /api/shell/agent-status` answers "is it up RIGHT NOW" to the wait page's 2 s poll, so its
        // budget is the short one. It happens to equal the tray's today; it is a separate number because
        // the two answer different questions and either could move.
        "CONTROL_PROBE_MS": 1_000.0,
    })
    .to_string()
}
