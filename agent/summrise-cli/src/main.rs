//! The `summrise-cli` binary: the ported decisions, on the real machine.
//!
//! It is NOT the npm package's `bin` yet — the cutover is a later landing, and repointing it before
//! the port is proven would put an unproven binary on devices in the field. Until then this binary
//! is what `cargo test -p summrise-cli` drives.

use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let host = summrise_cli::host::RealHost::new();
    exit(summrise_cli::dispatch::run(&host, &args));
}
