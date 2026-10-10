//! summrise-shell-policy — the Electron shell's remaining DECISIONS, in Rust.
//!
//! Landing 6b of `docs/superpowers/specs/2026-10-08-every-decision-is-rust-design.md`. It replaces the
//! decisions of `agent/summrise-desktop-electron/src/main.ts` (1,468 lines) and is loaded by the shell's
//! main process as a **wasm module built with `wasm-pack --target nodejs`**, exactly as its sibling
//! `agent/summrise-url-policy` (landing 6a) is.
//!
//! # THE TWO CRATES ARE SIBLINGS AND NEITHER DEPENDS ON THE OTHER, WHICH IS A MEASURED CONSTRAINT
//!
//! The first build of this crate DID depend on `summrise-url-policy`, and the generated glue showed why
//! it cannot: **wasm-bindgen exports every `#[wasm_bindgen]` item in the whole crate GRAPH**, so the
//! shell-policy glue came out with 75 exports — the 18 url-policy names among them. That is not a
//! duplicated name, it is a duplicated MODULE: each glue compiles its own `.wasm`, so url-policy's
//! `thread_local!` port state would exist TWICE, and `setAgentPort(7740)` through one glue would leave
//! the other at 18080 — a custom-port install where the DSH door admits the wrong origin and the
//! main-window tripwire snaps the panel back. Silent, device-only.
//!
//! So the two communicate THROUGH THE HOST: where a decision here composes one of url-policy's
//! predicates it takes that predicate's ANSWER as an argument (`tripwire_allows(url, desktop_spa)`,
//! `tray_should_watch(…, base_origin)`, `dsh_target(raw, admitted)`, `embedded_popup_target(target)`,
//! `resolve_agent_port(env, config_port)`), and main.ts — which holds exactly one instance of each glue —
//! evaluates it. Nothing is re-implemented and there is no state to share. `Cargo.toml` carries the
//! measurement; `test/shell-policy-wasm.test.mjs` PINS it by failing if any of the 18 names ever appears
//! in this crate's exports again.
//!
//! # What is here, and what stayed
//!
//! `main.ts` is now the **native host**: it creates windows, owns the tray, wires `ipcMain`, opens CDP,
//! spawns `schtasks`, reads `config.yaml` and makes HTTP requests. Every question those surfaces ASK is
//! answered here. The classification was made before a line of Rust was written, and it is the six
//! groups below — the first four are the ones the boundary manifest named ("token/auth, the loopback
//! control server, agent lifecycle, window policy") and the last two are the ones it did not, recorded
//! here because a list that is merely believed is how a decision stays behind:
//!
//! | group | what it decides |
//! |---|---|
//! | [`boot`] | the device token's line pattern and its cache TTL, the auth header, the agent and DSH port PRECEDENCE, the icon file per platform, the AUMID, the IPC door's refusal, the shell's policy constants |
//! | [`menu`] | the native application menu — every label, every accelerator and every command id |
//! | [`sessions`] | the browser-session reuse/eviction plan, the window id, the CDP endpoint, the embedded and DSH views' doors, placement threshold, zoom clamp and recovery URL |
//! | [`control`] | the loopback control server's request parse, route table, preflight rule and CORS headers |
//! | [`lifecycle`] | the `schtasks` argument lists and the auto-launch plan, the watchdog gate, the retry backoff, the tripwire's allow-list and the CDP self-check |
//! | [`tray`] | the `/api/status` parse with its keep-last rule, and the formatters the tooltip is built from |
//!
//! # THE HOST STAYS TYPESCRIPT, AND `CDP :9333` IS WHY THE LINE IS WHERE IT IS
//!
//! Port 9333 is a product contract: `agent/tests/mcp_autoselect_integration.rs` binds it to test MCP
//! auto-select, the CLI probes it, playwright's `connectOverCDP` drives the operator's visible page, and
//! the design sweep's attached mode depends on it. Nothing here changes WHEN or WHETHER it is opened —
//! `app.commandLine.appendSwitch("remote-debugging-port", …)` and the self-check's HTTP request are both
//! effects and both stay in `main.ts`; this crate owns only the self-check's QUESTION.
//!
//! # The rule every function here obeys
//!
//! It is the TypeScript it replaces, transliterated: the same predicates in the same order, the same
//! carve-outs, the same refusals — including the ones that look like accidents, because they are
//! decisions somebody made and the comments in `main.ts` say who and why. Two deliberate differences are
//! named where they occur and are the only two: a malformed `req.url` cannot crash the main process
//! ([`control::control_path`]), and a `slice` that lands inside a surrogate pair drops the character
//! ([`js::truncate_utf16`]).
//!
//! # The state, and why there is almost none
//!
//! The url-policy crate keeps the agent's and the DSH's ports in `thread_local!`s because the TypeScript
//! did. This crate keeps NOTHING of its own: every function is a pure function of its arguments, and the
//! only state it reads is the url-policy crate's port bindings (`agent_base`, `dsh_base`, `is_dsh_url`),
//! which is where the shell sets them. The session table, the tray's facts and the watchdog's counters
//! are the HOST's state — it is the only thing that can observe them — and they cross this boundary as
//! arguments and as JSON. That is what makes the ported cases ordinary `#[test]`s with no guard struct
//! and no ordering hazard.

pub mod boot;
pub mod control;
pub mod js;
pub mod lifecycle;
pub mod menu;
pub mod sessions;
pub mod tray;

#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(test)]
mod tests;

pub use boot::*;
pub use control::*;
pub use lifecycle::*;
pub use menu::*;
pub use sessions::*;
pub use tray::*;
