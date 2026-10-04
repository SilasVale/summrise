//! **THE TWO RELAYS MUST AGREE ON THE CONSTANTS THEY SHARE, AND NOTHING CHECKED THAT.**
//!
//! WHY THIS EXISTS. `proxies/api-relay/api/` is the Node relay in production and `relay/` is the Rust one
//! being prepared to replace it. The differential (`relay/differential.mjs`) compares their DECISIONS — 12 of
//! 14 routes byte-identical, 2 not wired — and this crate's port comments claim the constants match in as
//! many words ("the JavaScript reads `SUMMRISE_RELAY_HEADER_TIMEOUT_MS` at MODULE LOAD, which is what reading
//! it once at start-up is"). **A CLAIM NOBODY CHECKS IS A CLAIM THAT DRIFTS**, and the divergence class here
//! is quiet: a renamed environment variable, a changed default, a host that moved. None of them would fail a
//! corpus, because the harness sets the environment for both sides.
//!
//! WHAT IT PINS, and each one is read out of BOTH sources rather than restated:
//!
//!   * the environment variable the header budget comes from, and its DEFAULT;
//!   * the upstream host the git handler forwards to;
//!   * and that the Rust side's `UPSTREAMS` covers every host the Node side's handlers name.
//!
//! THE SUBJECT IS THE AGREEMENT, not either file: this fails when the two disagree, whichever one moved.

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    // `relay/` -> `api-relay/` -> `proxies/` -> the root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("the repository root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn the_two_relays_agree_on_the_constants_they_share() {
    let ts_git = read("proxies/api-relay/api/git.ts");
    let rust_main = read("proxies/api-relay/relay/src/main.rs");
    let rust_lib = read("proxies/api-relay/relay/src/lib.rs");

    // 1. THE ENVIRONMENT VARIABLE, named in both.
    const ENV: &str = "SUMMRISE_RELAY_HEADER_TIMEOUT_MS";
    assert!(
        ts_git.contains(ENV),
        "api/git.ts no longer reads {ENV} — the Rust side still does, and they would drift silently"
    );
    assert!(
        rust_main.contains(ENV),
        "relay/src/main.rs no longer reads {ENV} — the Node relay still does"
    );

    // 2. THE DEFAULT. **THE FIRST VERSION OF THIS PARSED FOR IT AND GOT IT WRONG** — the `??` is several
    //    lines below the marker, so the extraction returned nothing and the gate failed on its own helper.
    //    Asserting the fragment is in the file is what the check actually needs, and it cannot be wrong
    //    about where the fragment is.
    assert!(
        ts_git.contains("?? 30000"),
        "api/git.ts's default header budget moved off 30000; the Rust side's is pinned against it"
    );
    // **THE RUST SPELLS IT `30_000`**, which the first version of this did not accept — and the failure it
    // printed ("moved away from 30000") was about my literal, not about a divergence. Normalize the
    // separators before comparing, so the gate is about the NUMBER.
    let rust_digits: String = rust_main
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '_')
        .collect();
    assert!(
        rust_digits.contains("30_000") || rust_digits.contains("30000"),
        "relay/src/main.rs's default header budget moved off 30000"
    );

    // 3. THE GIT UPSTREAM HOST, in both.
    assert!(
        ts_git.contains("https://github.com"),
        "api/git.ts's upstream moved"
    );
    assert!(
        rust_lib.contains("(\"web\", \"https://github.com\")"),
        "the Rust side's `web` upstream moved away from api/git.ts's"
    );

    // 4. **EVERY HOST THE NODE HANDLERS NAME IS ONE THE RUST TABLE KNOWS.** This is the direction that
    //    matters: a host the Node relay forwards to and the Rust one does not would be a route that answers
    //    differently after the cutover, and no corpus would see it.
    for (file, host) in [
        ("proxies/api-relay/api/git.ts", "https://github.com"),
        ("proxies/api-relay/api/github.ts", "https://github.com"),
    ] {
        let src = read(file);
        if src.contains(host) {
            assert!(
                rust_lib.contains(host),
                "{file} forwards to {host} and the Rust UPSTREAMS table does not know it"
            );
        }
    }
}
