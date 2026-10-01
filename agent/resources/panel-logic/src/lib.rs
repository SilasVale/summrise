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

// NO `use wasm_bindgen::prelude::*;` HERE, and it is not an oversight: this file declares the
// modules and re-exports their functions, and each module imports the prelude for its own
// `#[wasm_bindgen]` attribute. The line was here and unused — the build said so on every run, which
// is how a warning becomes furniture.
mod actions;
mod adopt;
mod agent_vitals;
mod archive;
mod attention;
mod boot;
mod boot_notice;
mod events;
mod evicted;
mod idle;
mod js;
mod liveness;
mod marks;
mod monitors;
mod notify;
mod path;
mod recipe;
mod runs;
mod session_labels;
mod spark;
mod update;
mod trail;
mod vitals;
mod version;
mod vocabulary;

pub use actions::action_verdict;
pub use adopt::{adopt_needs_another_page, adopt_page_exceeded, split_write_slices};
pub use agent_vitals::parse_last_boot;
pub use archive::archive_entries;
pub use attention::{badge_icon, title_for};
pub use boot::parse_boot_history;
pub use boot_notice::{boot_kind_label, boot_notice, is_crash};
pub use events::{group_events, group_rounds, strip_ansi, terminal_status};
pub use evicted::parse_evicted;
pub use idle::{human_idle, idle_offer_text, idle_sessions, prune_session_views};
pub use marks::{monitor_mark_class, monitor_modifier};
pub use liveness::{
    any_command_running, device_liveness, liveness_of, session_active, session_failed,
    session_liveness, session_waiting,
};
pub use monitors::{parse_monitor_change, parse_monitors};
pub use notify::{permission_hint, read_permission};
pub use path::{attention_steps, card_state, derive_path, state_from_end, summarize_path};
pub use recipe::{build_recipe, recipe_warnings, suggested_title};
pub use runs::{group_operation, operation_rows};
pub use session_labels::disambiguate_labels;
pub use spark::{load_notice, series_stats, spark_segments};
pub use update::{attempt_age, checked_age, diagnose_update, parse_attempt, parse_update_status};
pub use trail::trail_read_notice;
pub use vitals::parse_vitals_series;
