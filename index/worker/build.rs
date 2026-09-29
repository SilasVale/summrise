//! The two generated landing documents are an INPUT to this crate, and a stale one would ship a
//! stale landing page — so cargo is told to re-run when either changes.
//!
//! **THE ARMS ARE EMBEDDED FROM THE COMMITTED MODULES, NOT COPIED.** `index/landing/build.sh` writes
//! `index/src/landing/setup.js` and `npm-only.js`, the JS worker imports those two files, and this
//! crate `include_str!`s the same two. One source, two consumers — and a regeneration is picked up by
//! the next build of BOTH, which is the alternative to a generated `.rs` that can be committed stale
//! (the failure `contract.gen.ts` and its gate exist to prevent).

fn main() {
    for arm in ["../src/landing/setup.js", "../src/landing/npm-only.js"] {
        println!("cargo:rerun-if-changed={arm}");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
