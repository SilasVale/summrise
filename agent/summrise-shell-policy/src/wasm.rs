//! THE EXPORTED SURFACE — one `#[wasm_bindgen]` function per question `main.ts` asks.
//!
//! `wasm-pack build --target nodejs` writes a CommonJS glue module next to the `.wasm`, and `main.ts`
//! requires that glue. Five things about this file are deliberate, and the first four are the ones
//! `agent/summrise-url-policy/src/wasm.rs` records — this crate inherits them rather than re-deriving
//! them:
//!
//! **1. `js_name` ON EVERY EXPORT, SPELLED THE WAY THE TYPESCRIPT SPELLED IT.** wasm-bindgen does NOT
//! camel-case by default, so each export names its `js_name` explicitly and `main.ts`'s call sites read
//! the way they did before the port. `test/shell-policy-wasm.test.mjs` pins the whole name list, so a
//! missing or renamed export is a failing suite rather than a `TypeError` in a shell nobody runs on this
//! box.
//!
//! **2. THE WRAPPERS DECIDE NOTHING.** Each one calls the crate's own function — the one `src/tests.rs`
//! exercises — so the tested program and the shipped program are the same program. The wrappers do
//! exactly one other job: the JS→Rust boundary conditions of §4 below.
//!
//! **3. `--target nodejs` IS WHY THERE IS NO `init()` HERE.** The main process is Node behind Electron,
//! so the glue loads the module SYNCHRONOUSLY (`fs.readFileSync` + a compiled `WebAssembly.Module`) and
//! the first decision may run immediately — which this crate needs, because the FIRST thing the shell
//! does is resolve the agent's port and pin the url-policy crate's predicates.
//!
//! **4. EVERY TEXT PARAMETER ARRIVES AS A `JsValue` AND IS COERCED HERE, BECAUSE A WASM `&str` PARAMETER
//! IS NOT THE TYPESCRIPT'S PARAMETER.** wasm-bindgen's glue hands a `&str` argument to
//! `passStringToWasm0`, which reads `.length` off it — url-policy's first build TRAPPED with
//! `RuntimeError: memory access out of bounds` on a non-string. Several of these channels are reachable
//! from a RENDERER (`browser-session:open`, `embedded-browser:zoom`, `embedded-dsh:go`,
//! `embedded-browser:place`) and two are reachable from the NETWORK (`req.url`, `req.headers.origin`),
//! so the coercion is not a formality in either direction. `unchecked_param_type` keeps the generated
//! `.d.ts` saying `string` rather than `any`.
//!
//! **5. THE NUMERIC COERCION IS `Number()`, CALLED RATHER THAN IMITATED.** `zoomFactor` is
//! `Math.min(3, Math.max(0.5, Number(factor) || 1))` and `resolveAgentPort` starts from
//! `Number(process.env.SUMMRISE_AGENT_PORT)`: both need JavaScript's string→number rules (leading and
//! trailing whitespace, `""` as 0, `0x10` as 16, `Infinity`), and a hand-rolled parser would be a second
//! answer to a question the platform already answers. So the JavaScript `Number` FUNCTION is declared
//! below and called — the same move url-policy makes with `String`. A `f64` wasm-bindgen parameter would
//! instead have THROWN a `TypeError` from the glue's own assertion, which is a third answer.

use js_sys::JsString;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// THE JAVASCRIPT `String(...)` FUNCTION ITSELF — not `x.toString()`, which throws on `null` and
    /// `undefined`, and not a Rust `format!`. The TypeScript wrote `String(x)` in the coercions this
    /// crate inherits, and `String` is the operation that transliterates exactly, an object with a
    /// `toString` included.
    #[wasm_bindgen(js_name = String)]
    fn js_string_of(value: &JsValue) -> JsString;

    /// THE JAVASCRIPT `Number(...)` FUNCTION ITSELF. See §5 above.
    #[wasm_bindgen(js_name = Number)]
    fn js_number_of(value: &JsValue) -> f64;
}

/// `String(value)`.
fn text(value: &JsValue) -> String {
    String::from(&js_string_of(value))
}

/// `value || undefined` — the falsy test on the UNCOERCED value, which is the distinction url-policy's
/// `controlOriginOk` records: `[]` is TRUTHY, so it coerces to `""` and reaches the parse, while a
/// literal `""` never does.
fn opt_text(value: Option<JsValue>) -> Option<String> {
    match value {
        Some(inner) if inner.is_truthy() => Some(text(&inner)),
        _ => None,
    }
}

/// The same test where the parameter is REQUIRED rather than optional.
///
/// It exists because wasm-bindgen refuses a required parameter after an optional one, so a function whose
/// absent-value case is identical to its falsy-value case — which is true wherever the answer goes
/// through [`opt_text`] — takes a plain `JsValue` and lets `undefined` arrive as itself. The alternative,
/// reordering the arguments, would have made the call site read differently from the TypeScript for no
/// behavioural reason.
fn truthy_text(value: &JsValue) -> Option<String> {
    if value.is_truthy() {
        Some(text(value))
    } else {
        None
    }
}

/// `value.width` / `value.local_port` — a property read that answers `undefined` for a missing property
/// AND for a receiver that is not an object.
///
/// The second half is why this is a function: JavaScript's `Reflect.get` THROWS on a primitive receiver
/// (`Reflect.get(5, "width")` is a `TypeError`), while `(5).width` is `undefined`. The TypeScript's
/// optional chaining was the latter, so the guard is what makes the two agree.
fn field(value: &JsValue, key: &str) -> JsValue {
    if value.is_object() {
        js_sys::Reflect::get(value, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
    } else {
        JsValue::UNDEFINED
    }
}

// ── boot ────────────────────────────────────────────────────────────────────────────────────────────

#[wasm_bindgen(js_name = deviceToken)]
pub fn device_token_js(
    #[wasm_bindgen(unchecked_optional_param_type = "string | null")] config_yaml: Option<JsValue>,
) -> Option<String> {
    // `String(raw || "")`: a missing config file is the empty document, which names no token.
    crate::boot::device_token(&opt_text(config_yaml).unwrap_or_default())
}

#[wasm_bindgen(js_name = tokenCacheFresh)]
pub fn token_cache_fresh_js(cached_at: f64, now: f64) -> bool {
    crate::boot::token_cache_fresh(cached_at, now)
}

#[wasm_bindgen(js_name = authorization)]
pub fn authorization_js(
    #[wasm_bindgen(unchecked_optional_param_type = "string | null")] token: Option<JsValue>,
) -> Option<String> {
    let token = opt_text(token);
    crate::boot::authorization(token.as_deref())
}

#[wasm_bindgen(js_name = resolveAgentPort)]
pub fn resolve_agent_port_js(
    #[wasm_bindgen(unchecked_optional_param_type = "string | null")] env: Option<JsValue>,
    #[wasm_bindgen(unchecked_optional_param_type = "number | undefined")] config_port: Option<f64>,
) -> u16 {
    // `Number(process.env.SUMMRISE_AGENT_PORT)` — the JavaScript conversion, for the reason §5 gives.
    let env_number = env.as_ref().map(js_number_of);
    // `parseAgentPort(raw)`'s answer, which the host evaluated. An out-of-range or absent one is not a
    // port, so it is dropped here rather than being allowed to win the precedence.
    let config_port = config_port.and_then(crate::boot::valid_port);
    crate::boot::resolve_agent_port(env_number, config_port)
}

#[wasm_bindgen(js_name = resolveDshPort)]
pub fn resolve_dsh_port_js(
    #[wasm_bindgen(unchecked_optional_param_type = "string | null")] env: Option<JsValue>,
) -> u16 {
    crate::boot::resolve_dsh_port(env.as_ref().map(js_number_of))
}

#[wasm_bindgen(js_name = trayIconName)]
pub fn tray_icon_name_js(
    #[wasm_bindgen(unchecked_param_type = "string")] platform: JsValue,
) -> String {
    crate::boot::tray_icon_name(&text(&platform)).to_string()
}

#[wasm_bindgen(js_name = windowIconName)]
pub fn window_icon_name_js() -> String {
    crate::boot::window_icon_name().to_string()
}

#[wasm_bindgen(js_name = usesAppUserModelId)]
pub fn uses_app_user_model_id_js(
    #[wasm_bindgen(unchecked_param_type = "string")] platform: JsValue,
) -> bool {
    crate::boot::uses_app_user_model_id(&text(&platform))
}

#[wasm_bindgen(js_name = aumidReport)]
pub fn aumid_report_js(
    #[wasm_bindgen(unchecked_param_type = "string")] platform: JsValue,
) -> String {
    crate::boot::aumid_report(&text(&platform)).to_string()
}

#[wasm_bindgen(js_name = agentHostLabel)]
pub fn agent_host_label_js(
    #[wasm_bindgen(unchecked_param_type = "string")] agent_base: JsValue,
) -> String {
    crate::boot::agent_host_label(&text(&agent_base))
}

#[wasm_bindgen(js_name = shellConstants)]
pub fn shell_constants_js() -> String {
    crate::boot::shell_constants()
}

#[wasm_bindgen(js_name = forbiddenFrame)]
pub fn forbidden_frame_js() -> String {
    crate::boot::forbidden_frame_json()
}

// ── the native menu ─────────────────────────────────────────────────────────────────────────────────

#[wasm_bindgen(js_name = appMenuJson)]
pub fn app_menu_json_js(
    #[wasm_bindgen(unchecked_param_type = "string")] platform: JsValue,
    #[wasm_bindgen(unchecked_param_type = "string")] app_name: JsValue,
) -> String {
    crate::menu::app_menu_json(&text(&platform), &text(&app_name))
}

// ── the browser sessions and the two embedded views ─────────────────────────────────────────────────

/// `browserOpen`'s plan: `{"reuse": id|null, "evict": id|null}`.
#[wasm_bindgen(js_name = planBrowserOpen)]
pub fn plan_browser_open_js(
    #[wasm_bindgen(unchecked_param_type = "string")] existing_json: JsValue,
    #[wasm_bindgen(unchecked_param_type = "string")] target: JsValue,
    cap: f64,
) -> String {
    crate::sessions::plan_browser_open_json(&text(&existing_json), &text(&target), cap)
}

#[wasm_bindgen(js_name = browserId)]
pub fn browser_id_js(now_ms: f64) -> String {
    crate::sessions::browser_id(now_ms)
}

#[wasm_bindgen(js_name = cdpEndpoint)]
pub fn cdp_endpoint_js(port: u16) -> String {
    crate::sessions::cdp_endpoint(port)
}

/// `!bounds || bounds.width < 50 || bounds.height < 50`.
#[wasm_bindgen(js_name = slotTooSmall)]
pub fn slot_too_small_js(bounds: JsValue) -> bool {
    if !bounds.is_truthy() {
        return true;
    }
    let width = js_number_of(&field(&bounds, "width"));
    let height = js_number_of(&field(&bounds, "height"));
    crate::sessions::slot_too_small(width, height)
}

/// The DROP rule for a popup the load door refused. The argument is the DECIDED target.
#[wasm_bindgen(js_name = embeddedPopupTarget)]
pub fn embedded_popup_target_js(
    #[wasm_bindgen(unchecked_param_type = "string")] target: JsValue,
) -> Option<String> {
    crate::sessions::embedded_popup_target(&text(&target))
}

/// `admitted` is `isDshUrl(raw)`, evaluated by the host.
#[wasm_bindgen(js_name = dshPopupTarget)]
pub fn dsh_popup_target_js(
    #[wasm_bindgen(unchecked_param_type = "string")] raw: JsValue,
    admitted: bool,
) -> Option<String> {
    crate::sessions::dsh_popup_target(&text(&raw), admitted)
}

/// `admitted` is `isDshUrl(raw)`, evaluated by the host.
#[wasm_bindgen(js_name = dshTarget)]
pub fn dsh_target_js(
    #[wasm_bindgen(unchecked_param_type = "string")] raw: JsValue,
    admitted: bool,
) -> String {
    crate::sessions::dsh_target(&text(&raw), admitted)
}

#[wasm_bindgen(js_name = dshHome)]
pub fn dsh_home_js(#[wasm_bindgen(unchecked_param_type = "string")] dsh_base: JsValue) -> String {
    crate::sessions::dsh_home(&text(&dsh_base))
}

/// `Math.min(3, Math.max(0.5, Number(factor) || 1))` — and the `|| 1` is applied HERE, on the value
/// `Number()` produced, because that is the order the JavaScript evaluated it in.
#[wasm_bindgen(js_name = zoomFactor)]
pub fn zoom_factor_js(factor: JsValue) -> f64 {
    let number = js_number_of(&factor);
    crate::sessions::zoom_factor(number)
}

#[wasm_bindgen(js_name = embeddedRecoverUrl)]
pub fn embedded_recover_url_js(
    #[wasm_bindgen(unchecked_optional_param_type = "string | null")] last: Option<JsValue>,
) -> String {
    let last = opt_text(last);
    crate::sessions::embedded_recover_url(last.as_deref())
}

#[wasm_bindgen(js_name = shownUrl)]
pub fn shown_url_js(
    #[wasm_bindgen(unchecked_param_type = "string | null | undefined")] live: JsValue,
    #[wasm_bindgen(unchecked_param_type = "string")] fallback: JsValue,
) -> String {
    crate::sessions::shown_url(truthy_text(&live).as_deref(), &text(&fallback))
}

#[wasm_bindgen(js_name = viewVisible)]
pub fn view_visible_js(want_visible: bool, live_contents: bool) -> bool {
    crate::sessions::view_visible(want_visible, live_contents)
}

#[wasm_bindgen(js_name = goBackwards)]
pub fn go_backwards_js(delta: f64) -> bool {
    crate::sessions::go_backwards(delta)
}

/// THE ADMISSIBLE HARNESS DOORS, in the order the answer named them.
///
/// The argument is the ALREADY-PARSED `answer.harnesses` array, because `Array.isArray` and
/// `row?.local_port` are JavaScript operations on a JavaScript value; what is decided here is which
/// numbers pass the range test. The host then calls `addDshPort` — the url-policy crate's — for each,
/// because admitting a door is that crate's decision over its own list and this one must not hold a
/// second copy of it (see `Cargo.toml`). So the shell's harness table is: **the agent names the ports,
/// this crate says which are usable, and the url policy says which doors exist.**
#[wasm_bindgen(js_name = harnessDoors)]
pub fn harness_doors_js(rows: JsValue) -> Vec<u16> {
    if !js_sys::Array::is_array(&rows) {
        return Vec::new();
    }
    let numbers: Vec<Option<f64>> = js_sys::Array::from(&rows)
        .iter()
        .map(|row| {
            let raw = field(&row, "local_port");
            if raw.is_undefined() {
                None
            } else {
                Some(js_number_of(&raw))
            }
        })
        .collect();
    crate::sessions::harness_doors(&numbers)
}

// ── the loopback control server ─────────────────────────────────────────────────────────────────────

#[wasm_bindgen(js_name = controlPath)]
pub fn control_path_js(
    #[wasm_bindgen(unchecked_optional_param_type = "string | null")] raw_url: Option<JsValue>,
) -> Option<String> {
    // `req.url || "/"` — the falsy test is the caller's, so `""` and `undefined` both mean "no path".
    let raw_url = opt_text(raw_url);
    crate::control::control_path(raw_url.as_deref())
}

#[wasm_bindgen(js_name = controlQuery)]
pub fn control_query_js(
    #[wasm_bindgen(unchecked_param_type = "string | null | undefined")] raw_url: JsValue,
    #[wasm_bindgen(unchecked_param_type = "string")] key: JsValue,
) -> Option<String> {
    // `req.url || "/"` — the falsy test is the caller's, so `""` and `undefined` both mean "no path".
    crate::control::control_query(truthy_text(&raw_url).as_deref(), &text(&key))
}

#[wasm_bindgen(js_name = controlIsPreflight)]
pub fn control_is_preflight_js(
    #[wasm_bindgen(unchecked_param_type = "string")] method: JsValue,
) -> bool {
    crate::control::control_is_preflight(&text(&method))
}

#[wasm_bindgen(js_name = controlRoute)]
pub fn control_route_js(
    #[wasm_bindgen(unchecked_param_type = "string")] method: JsValue,
    #[wasm_bindgen(unchecked_param_type = "string")] pathname: JsValue,
) -> String {
    crate::control::control_route(&text(&method), &text(&pathname))
        .as_str()
        .to_string()
}

#[wasm_bindgen(js_name = corsAllowOrigin)]
pub fn cors_allow_origin_js(
    #[wasm_bindgen(unchecked_optional_param_type = "string | null")] origin: Option<JsValue>,
) -> String {
    // `req.headers.origin || "*"` — a REFLECTED value, so a truthy non-string is coerced rather than
    // passed through, and an absent or empty header is the wildcard.
    let origin = opt_text(origin);
    crate::control::cors_allow_origin(origin.as_deref())
}

// ── the agent lifecycle and the shell's self-protection ─────────────────────────────────────────────

#[wasm_bindgen(js_name = autoLaunchPlan)]
pub fn auto_launch_plan_js(enabled: bool, exists: bool) -> String {
    crate::lifecycle::auto_launch_plan(enabled, exists)
        .as_str()
        .to_string()
}

#[wasm_bindgen(js_name = schtasksQueryArgs)]
pub fn schtasks_query_args_js(
    #[wasm_bindgen(unchecked_param_type = "string")] task: JsValue,
) -> Vec<String> {
    crate::lifecycle::schtasks_query_args(&text(&task))
}

#[wasm_bindgen(js_name = schtasksRunArgs)]
pub fn schtasks_run_args_js(
    #[wasm_bindgen(unchecked_param_type = "string")] task: JsValue,
) -> Vec<String> {
    crate::lifecycle::schtasks_run_args(&text(&task))
}

#[wasm_bindgen(js_name = schtasksEndArgs)]
pub fn schtasks_end_args_js(
    #[wasm_bindgen(unchecked_param_type = "string")] task: JsValue,
) -> Vec<String> {
    crate::lifecycle::schtasks_end_args(&text(&task))
}

#[wasm_bindgen(js_name = schtasksDeleteArgs)]
pub fn schtasks_delete_args_js(
    #[wasm_bindgen(unchecked_param_type = "string")] task: JsValue,
) -> Vec<String> {
    crate::lifecycle::schtasks_delete_args(&text(&task))
}

/// THE ARGUMENT LIST WITH THE DOCUMENTED QUOTING TRAP IN IT — the `/tr` value carries its own inner
/// quotes, because `schtasks` re-parses it as a command line.
#[wasm_bindgen(js_name = schtasksCreateArgs)]
pub fn schtasks_create_args_js(
    #[wasm_bindgen(unchecked_param_type = "string")] task: JsValue,
    #[wasm_bindgen(unchecked_param_type = "string")] script: JsValue,
) -> Vec<String> {
    crate::lifecycle::schtasks_create_args(&text(&task), &text(&script))
}

#[wasm_bindgen(js_name = watchdogShouldStart)]
pub fn watchdog_should_start_js(misses: f64, last_start_at: f64, now: f64) -> bool {
    crate::lifecycle::watchdog_should_start(misses, last_start_at, now)
}

#[wasm_bindgen(js_name = watchdogLog)]
pub fn watchdog_log_js(
    ok: bool,
    #[wasm_bindgen(unchecked_optional_param_type = "string | null")] error: Option<JsValue>,
) -> String {
    let error = opt_text(error);
    crate::lifecycle::watchdog_log(ok, error.as_deref())
}

#[wasm_bindgen(js_name = nextRetryMs)]
pub fn next_retry_ms_js(current: f64) -> f64 {
    crate::lifecycle::next_retry_ms(current)
}

#[wasm_bindgen(js_name = shouldRetryLoad)]
pub fn should_retry_load_js(is_main_frame: bool, error_code: f64) -> bool {
    crate::lifecycle::should_retry_load(is_main_frame, error_code)
}

#[wasm_bindgen(js_name = isWaitPage)]
pub fn is_wait_page_js(#[wasm_bindgen(unchecked_param_type = "string")] url: JsValue) -> bool {
    crate::lifecycle::is_wait_page(&text(&url))
}

#[wasm_bindgen(js_name = statusIsAlive)]
pub fn status_is_alive_js(status_code: Option<f64>) -> bool {
    crate::lifecycle::status_is_alive(status_code)
}

/// `desktop_spa` is `isDesktopSpaUrl(url)`, evaluated by the host.
#[wasm_bindgen(js_name = tripwireAllows)]
pub fn tripwire_allows_js(
    #[wasm_bindgen(unchecked_param_type = "string")] url: JsValue,
    desktop_spa: bool,
) -> bool {
    crate::lifecycle::tripwire_allows(&text(&url), desktop_spa)
}

#[wasm_bindgen(js_name = tripwireLog)]
pub fn tripwire_log_js(#[wasm_bindgen(unchecked_param_type = "string")] url: JsValue) -> String {
    crate::lifecycle::tripwire_log(&text(&url))
}

#[wasm_bindgen(js_name = cdpUserAgent)]
pub fn cdp_user_agent_js(
    #[wasm_bindgen(unchecked_param_type = "string")] version_json: JsValue,
) -> String {
    crate::lifecycle::cdp_user_agent(&text(&version_json))
}

#[wasm_bindgen(js_name = cdpUserAgentOurs)]
pub fn cdp_user_agent_ours_js(
    #[wasm_bindgen(unchecked_optional_param_type = "string | null")] user_agent: Option<JsValue>,
) -> bool {
    let user_agent = opt_text(user_agent);
    crate::lifecycle::cdp_user_agent_is_ours(user_agent.as_deref())
}

#[wasm_bindgen(js_name = cdpSelfCheckOwnsPort)]
pub fn cdp_self_check_owns_port_js(
    #[wasm_bindgen(unchecked_param_type = "string")] version_json: JsValue,
) -> bool {
    crate::lifecycle::cdp_self_check_owns_port(&text(&version_json))
}

#[wasm_bindgen(js_name = cdpSelfCheckOk)]
pub fn cdp_self_check_ok_js(port: u16) -> String {
    crate::lifecycle::cdp_self_check_ok(port)
}

#[wasm_bindgen(js_name = cdpWarningForeign)]
pub fn cdp_warning_foreign_js(
    port: u16,
    #[wasm_bindgen(unchecked_param_type = "string")] user_agent: JsValue,
) -> String {
    crate::lifecycle::cdp_warning_foreign(port, &text(&user_agent))
}

#[wasm_bindgen(js_name = cdpWarningNotResponding)]
pub fn cdp_warning_not_responding_js(port: u16) -> String {
    crate::lifecycle::cdp_warning_not_responding(port)
}

#[wasm_bindgen(js_name = cdpWarningUnreachable)]
pub fn cdp_warning_unreachable_js(port: u16) -> String {
    crate::lifecycle::cdp_warning_unreachable(port)
}

// ── the tray ────────────────────────────────────────────────────────────────────────────────────────

#[wasm_bindgen(js_name = fmtUptime)]
pub fn fmt_uptime_js(secs: f64) -> String {
    crate::tray::fmt_uptime(secs)
}

/// `new Date(at).toTimeString().slice(0, 8)` — with the host's time-zone offset passed in, because wasm
/// has no clock and no zone database.
#[wasm_bindgen(js_name = fmtClock)]
pub fn fmt_clock_js(at_ms: f64, tz_offset_min: f64) -> String {
    crate::tray::fmt_clock(at_ms, tz_offset_min)
}

/// ONE POLL'S WHOLE DECISION: the parse, the keep-last rule, the vitals line and the tooltip.
#[wasm_bindgen(js_name = refreshTrayHealth)]
pub fn refresh_tray_health_js(
    #[wasm_bindgen(unchecked_param_type = "string")] request_json: JsValue,
) -> String {
    crate::tray::refresh_tray_health(&text(&request_json))
}

/// `base_origin` is `isBaseOrigin(mainUrl)`, evaluated by the host — and only when the window is live,
/// which keeps the TypeScript's short-circuit.
#[wasm_bindgen(js_name = trayShouldWatch)]
pub fn tray_should_watch_js(
    running: bool,
    watch_active: bool,
    window_live: bool,
    base_origin: bool,
) -> bool {
    crate::tray::tray_should_watch(running, watch_active, window_live, base_origin)
}
