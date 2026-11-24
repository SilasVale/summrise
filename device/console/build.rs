//! The version this console was built as, written where `include_str!` can find it.
//!
//! `--version` alone drifted: the launcher script carried a hand-written label and the page reported a
//! release that had been superseded. `option_env!` would do the same job in one line, but a build script
//! is the reliable place for it — it can also tell cargo to rebuild when the value changes, which is
//! exactly what makes a release label trustworthy.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=SUMMRISE_BUILD_VERSION");
    let version = env::var("SUMMRISE_BUILD_VERSION").unwrap_or_else(|_| {
        env::var("CARGO_PKG_VERSION").expect("cargo always sets CARGO_PKG_VERSION")
    });
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo always sets OUT_DIR"));
    fs::write(out.join("version.txt"), version.trim()).expect("write version.txt");
}
