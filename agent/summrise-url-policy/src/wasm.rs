//! THE EXPORTED SURFACE — one `#[wasm_bindgen]` function per name `url-policy.ts` exported.
//!
//! `wasm-pack build --target nodejs` writes a CommonJS glue module next to the `.wasm`, and `main.ts`
//! requires that glue. Four things about this file are deliberate:
//!
//! **1. `js_name` ON EVERY EXPORT, SPELLED THE WAY THE TYPESCRIPT SPELLED IT.** wasm-bindgen does NOT
//! camel-case by default — `agent/resources/panel-react/src/wasm/panel_logic.d.ts` is the measurement,
//! where the exports are `adopt_needs_another_page` and friends. So each export names its `js_name`
//! explicitly, `main.ts`'s `import { isBaseOrigin, … }` is unchanged from the day it imported
//! `./url-policy`, and `test/url-policy-wasm.test.mjs` pins the whole 18-name list so a missing or
//! renamed export is a failing suite rather than a `TypeError` in a shell nobody runs on this box.
//!
//! **2. THE WRAPPERS DECIDE NOTHING.** Each one calls the crate's own function — the one the 11 ported
//! cases in `src/tests.rs` exercise — so the tested program and the shipped program are the same
//! program. The wrappers do exactly one other job: the JS→Rust boundary conditions of §4 below.
//!
//! **3. `--target nodejs` IS WHY THERE IS NO `init()` HERE.** The main process is Node behind Electron,
//! so the glue loads the module SYNCHRONOUSLY (`fs.readFileSync` + a compiled `WebAssembly.Module`) and
//! the first predicate may run immediately. `--target web` would need an async `init()` awaited before
//! any decision, which is the constraint the panel's migration had to design around
//! (`panel-react/src/wasm/panelLogic.ts`); this landing chose the target that removes it.
//!
//! **4. EVERY PARAMETER ARRIVES AS A `JsValue` AND IS COERCED HERE, BECAUSE A WASM `&str` PARAMETER IS
//! NOT THE TYPESCRIPT'S PARAMETER.** wasm-bindgen's glue hands a `&str` argument to `passStringToWasm0`,
//! which reads `.length` off it: the first build of this crate TRAPPED with
//! `RuntimeError: memory access out of bounds` on `sanitizeBrowserUrl(42)`, where the TypeScript
//! answered `"about:blank"`. That case is not hypothetical — the stage-n preload audit added exactly
//! that coercion ("a renderer passing a number/object would throw TypeError in the main process (DoS)"),
//! and a renderer reaches `sanitizeBrowserUrl` through `browser-session:open`. So each parameter is
//! `JsValue`, converted with the same operation the TypeScript performed (`String(x)` for text, the
//! `Number.isInteger` typeof gate for ports, the JS falsy test for `x || ""`), and
//! `unchecked_param_type` keeps the generated `.d.ts` saying `string`/`number` rather than `any` —
//! the declaration stays honest while the runtime stays the TypeScript's.

use js_sys::JsString;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// THE JAVASCRIPT `String(...)` FUNCTION ITSELF — not `x.toString()` (which throws on
    /// `null`/`undefined`) and not a Rust `format!`: `url-policy.ts` wrote `String(url || "about:blank")`,
    /// and `String` is the operation that transliterates exactly, an object with a `toString` included.
    #[wasm_bindgen(js_name = String)]
    fn js_string_of(value: &JsValue) -> JsString;
}

/// `String(value)`.
fn js_string(value: &JsValue) -> String {
    String::from(&js_string_of(value))
}

#[wasm_bindgen(js_name = isBaseOrigin)]
pub fn is_base_origin_js(#[wasm_bindgen(unchecked_param_type = "string")] url: JsValue) -> bool {
    // `new URL(url)` ToStrings its argument, so `undefined` becomes the string "undefined" and fails to
    // parse on both sides. `is_base_origin` takes the coerced text.
    crate::is_base_origin(&js_string(&url))
}

#[wasm_bindgen(js_name = frameUrlOk)]
pub fn frame_url_ok_js(#[wasm_bindgen(unchecked_param_type = "string")] url: JsValue) -> bool {
    crate::frame_url_ok(&js_string(&url))
}

#[wasm_bindgen(js_name = controlOriginOk)]
pub fn control_origin_ok_js(
    #[wasm_bindgen(unchecked_optional_param_type = "string | null")] origin: Option<JsValue>,
) -> bool {
    // `if (!origin || origin === "null") return true;` — and the FALSY TEST IS ON THE UNCOERCED VALUE,
    // then the coercion, then the parse. In that order: `[]` is truthy, so the TypeScript coerces it to
    // `""` and FAILS to parse it, where a literal `""` never reaches the parse. Coercing first and
    // testing falsiness afterwards answers `true` for `[]` — measured, and the one difference a
    // differential over 927 inputs found in the whole port.
    match origin {
        Some(value) if value.is_truthy() => crate::control_origin_allowed(&js_string(&value)),
        _ => crate::control_origin_ok(None),
    }
}

#[wasm_bindgen(js_name = isDesktopSpaUrl)]
pub fn is_desktop_spa_url_js(
    #[wasm_bindgen(unchecked_param_type = "string")] url: JsValue,
) -> bool {
    crate::is_desktop_spa_url(&js_string(&url))
}

#[wasm_bindgen(js_name = isPrivateHost)]
pub fn is_private_host_js(
    #[wasm_bindgen(unchecked_param_type = "string")] hostname: JsValue,
) -> bool {
    // `String(hostname || "")` — a falsy hostname is the empty string, which the core refuses.
    let text = if hostname.is_truthy() {
        js_string(&hostname)
    } else {
        String::new()
    };
    crate::is_private_host(&text)
}

#[wasm_bindgen(js_name = certBypassAllowed)]
pub fn cert_bypass_allowed_js(
    #[wasm_bindgen(unchecked_param_type = "string")] url: JsValue,
) -> bool {
    crate::cert_bypass_allowed(&js_string(&url))
}

#[wasm_bindgen(js_name = sanitizeBrowserUrl)]
pub fn sanitize_browser_url_js(
    #[wasm_bindgen(unchecked_optional_param_type = "string | null")] url: Option<JsValue>,
) -> String {
    // `String(url || "about:blank")` — a missing, empty or otherwise falsy url is the blank page.
    let text = match url {
        Some(value) if value.is_truthy() => js_string(&value),
        _ => String::new(),
    };
    crate::sanitize_browser_url(Some(&text))
}

/// `Number.isInteger(port)` is a TYPEOF test as well as a range test, and `JsValue::as_f64` is exactly
/// that test: it answers `None` for a string, a BigInt, `null`, `undefined` and an object, and `Some` —
/// `NaN` included, which the range check then refuses — only for a JavaScript number.
fn port_of(value: &JsValue) -> Option<f64> {
    value.as_f64()
}

#[wasm_bindgen(js_name = setAgentPort)]
pub fn set_agent_port_js(#[wasm_bindgen(unchecked_param_type = "number")] port: JsValue) {
    if let Some(port) = port_of(&port) {
        crate::set_agent_port(port);
    }
}

#[wasm_bindgen(js_name = getAgentPort)]
pub fn get_agent_port_js() -> u16 {
    crate::get_agent_port()
}

#[wasm_bindgen(js_name = agentBase)]
pub fn agent_base_js() -> String {
    crate::agent_base()
}

#[wasm_bindgen(js_name = setDshPort)]
pub fn set_dsh_port_js(#[wasm_bindgen(unchecked_param_type = "number")] port: JsValue) {
    if let Some(port) = port_of(&port) {
        crate::set_dsh_port(port);
    }
}

#[wasm_bindgen(js_name = getDshPort)]
pub fn get_dsh_port_js() -> u16 {
    crate::get_dsh_port()
}

#[wasm_bindgen(js_name = dshBase)]
pub fn dsh_base_js() -> String {
    crate::dsh_base()
}

/// The one wrapper with a decision of its own: the JavaScript threw a real `Error`, so a thrown STRING
/// would not be the same refusal (a caller printing `err.message` or matching a regex sees the
/// difference). The message is the TypeScript's, `${port}` included — which is why a non-number is
/// rendered with `String(value)` rather than dropped: `addDshPort("abc")` must say `not a port: abc`.
#[wasm_bindgen(js_name = addDshPort)]
pub fn add_dsh_port_js(
    #[wasm_bindgen(unchecked_param_type = "number")] port: JsValue,
) -> Result<(), JsValue> {
    match port_of(&port) {
        Some(port) => {
            crate::add_dsh_port(port).map_err(|message| js_sys::Error::new(&message).into())
        }
        None => Err(js_sys::Error::new(&format!("not a port: {}", js_string(&port))).into()),
    }
}

#[wasm_bindgen(js_name = clearExtraDshPorts)]
pub fn clear_extra_dsh_ports_js() {
    crate::clear_extra_dsh_ports();
}

#[wasm_bindgen(js_name = dshOrigins)]
pub fn dsh_origins_js() -> Vec<String> {
    crate::dsh_origins()
}

#[wasm_bindgen(js_name = isDshUrl)]
pub fn is_dsh_url_js(#[wasm_bindgen(unchecked_param_type = "string")] url: JsValue) -> bool {
    crate::is_dsh_url(&js_string(&url))
}

/// `Option<u16>` rather than a `JsValue` holding `null`: the generated `.d.ts` then says
/// `number | undefined` instead of `any`, and the shell's one call site is `if (port) return port;` — a
/// truthiness test, which `undefined` and `null` answer the same way. The TypeScript answered `null`;
/// this is the one place the JS-visible answer is spelled differently, and nothing can observe it.
#[wasm_bindgen(js_name = parseAgentPort)]
pub fn parse_agent_port_js(
    #[wasm_bindgen(unchecked_param_type = "string")] yaml_text: JsValue,
) -> Option<u16> {
    // `String(yamlText || "")` — a falsy argument is the empty document, which finds no port.
    let text = if yaml_text.is_truthy() {
        js_string(&yaml_text)
    } else {
        String::new()
    };
    crate::parse_agent_port(&text)
}
