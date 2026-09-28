//! The Summrise landing page, in Rust.
//!
//! * `page` — renders the document. It runs on the HOST, at build time, so every
//!   word is in the HTML the browser receives and nothing waits on a binary.
//! * `interactive` — the page's two behaviours (the theme toggle and the particle
//!   field), compiled to wasm and loaded after the page has already painted.
//!
//! The architecture is the transferable result: **render in Rust at build time,
//! ship the text as HTML, hydrate the behaviours from wasm after paint.** Measured
//! with the binary held back two seconds, first paint is unchanged (92 ms against
//! 96 ms for the JavaScript page) because the text is not produced by the binary.
//!
//! `build.sh` builds both halves and prints the sizes; `verify.mjs` proves the
//! rendered document is byte-identical to the page it replaces outside the two
//! places the migration is about; `measure.mjs` measures the first render.

pub mod page;

#[cfg(target_arch = "wasm32")]
pub mod interactive;
