//! The smallest thing that proves the Rust -> wasm -> JavaScript toolchain works in THIS
//! repository. It is not a template for the migration and nothing imports it: it exists so that
//! "wasm-pack works here" is a measurement instead of a belief.
//!
//! TWO FUNCTIONS, BECAUSE THE BOUNDARY HAS TWO SHAPES. A number in / number out proves the
//! module instantiates and the glue finds the export; a string in / string out proves the
//! `--target web` glue allocates and copies across the linear memory, which is the shape every
//! real migration step will use (JSON in, JSON out) and the one that fails silently if it does
//! not work. Sizes of both are in README.md.
use wasm_bindgen::prelude::*;

/// The number boundary.
#[wasm_bindgen]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

/// The string boundary — the shape a migrated `parse`/`derive`/`format` export will have.
#[wasm_bindgen]
pub fn echo(s: &str) -> String {
    format!("wasm:{s}")
}
