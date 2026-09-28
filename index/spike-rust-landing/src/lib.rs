//! Spike: can this product's TypeScript surfaces become Rust?
//!
//! The smallest surface is the landing page. This crate answers with a working
//! page rather than an argument:
//!
//! * `page` — renders the HTML in Rust. Runs on the HOST (build time / worker),
//!   so every word is in the HTML and nothing waits on a binary.
//! * `interactive` — the two behaviours, in Rust, compiled to wasm and loaded
//!   after the page has already painted.
//!
//! See README.md for the three numbers and the trade.

pub mod page;

#[cfg(target_arch = "wasm32")]
pub mod interactive;
