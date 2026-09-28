//! panel-logic — the panel's LOGIC, in Rust, compiled to wasm and called from React.
//!
//! P2 of `docs/superpowers/plans/2026-09-28-the-product-moves-to-rust.md`, whose inventory is
//! `docs/superpowers/p0/README.md`: the panel holds **156 LOGIC exports across 46 files** and this
//! crate is where they move, one family per commit. The React tree, Radix and Tailwind stay exactly
//! where they are — this is a crate of FUNCTIONS, not a second component tree (P0's framework
//! decision, and the reason there is no `yew`/`leptos`/`sycamore` in Cargo.toml).
//!
//! # The two rules every function in here obeys
//!
//! **1. It is a MIRROR, not a reimplementation.** Each function is the TypeScript it replaces,
//! transliterated, including the sentences it throws and the exact predicate it tests — the JS
//! `typeof`, the JS `trim()`, `Number.isFinite`. `js_sys` is used at that level deliberately: a
//! serde derive would be shorter and would answer DIFFERENTLY for the shapes this panel actually
//! receives (`{state:{}}` is an absence in `lib/archive.ts`, and a `#[derive(Deserialize)]` would
//! call it a struct with default fields). The panel's parsers are total by design and their
//! boundaries are part of their behaviour, so the boundary conditions are transliterated too.
//!
//! **2. It is proved against the JS it replaces, not against a rewrite of its tests.** The
//! TypeScript tests that were written against `lib/archive.ts` run UNCHANGED against this crate
//! (the loader in `panel-react/src/wasm/panelLogic.ts` hands the same artifact to vitest as to the
//! browser), and the corpus in the commit message is every combination of the shapes those
//! functions can be handed, both implementations, compared on the VALUES AND THE MESSAGES.
//!
//! # Why the FFI is `JsValue` and not JSON
//!
//! Measured, same machine, same `wasm-opt -Oz`: serde_json costs 43,824 gz where this crate's whole
//! surface costs a fraction of it (Cargo.toml carries the three-way measurement). The panel's logic
//! is already holding JS values — the parse of `GET /api/sessions` receives the parsed body — so
//! serializing it to text and parsing it back inside the wasm would pay ~37 KB gz, 13.5% of the
//! panel's entire first-load payload, for a round trip nobody asked for.

use wasm_bindgen::prelude::*;

mod archive;
mod boot;

pub use archive::archive_entries;
pub use boot::parse_boot_history;
