//! `GATEWAY_DISPATCHED_CODES` and `gateway/src/mcp.ts` are two halves of ONE
//! contract, and each half was pinned only against ITSELF.
//!
//! The agent's `error.rs` pins the three strings as literals, and its test message
//! says "gateway/src/mcp.ts matches exactly these three" — a claim about a file that
//! test never opens. The gateway's own suite exercises the mapping through a stubbed
//! agent response, so it pins the three arms IT knows about.
//!
//! Two directions are therefore covered — rename on either side fails that side's
//! tests — and ONE IS NOT: **if the gateway adds a fourth dispatched code, the
//! gateway's tests still pass, and so does the agent's**, because the agent file did
//! not change. At that moment `GATEWAY_DISPATCHED_CODES` understates the real
//! contract, and `error.rs`'s documented "deliberately NOT dispatched" list — whose
//! first entry is `human_in_control` — silently becomes false. Nothing refuses it.
//!
//! This reads BOTH files as data and compares them, so neither side's literals are
//! restated here and the assertion cannot agree with itself.

use std::path::PathBuf;

fn repo_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("agent/ has a parent")
        .to_path_buf()
}

/// The strings in `pub const GATEWAY_DISPATCHED_CODES: &[&str] = &[ … ];`
fn agent_dispatched_codes(rust: &str) -> Vec<String> {
    // Anchor on the whole `= &[` tail: a bare `find('[')` lands inside the TYPE
    // `&[&str]`, whose `]` closes immediately — which yields an EMPTY list, and an
    // empty list is a statement about this parser rather than about the contract.
    // (That is exactly how this test first failed, and the empty `left` was the tell.)
    const ANCHOR: &str = "GATEWAY_DISPATCHED_CODES: &[&str] = &[";
    let start = rust
        .find(ANCHOR)
        .expect("error.rs declares GATEWAY_DISPATCHED_CODES")
        + ANCHOR.len();
    let rest = &rust[start..];
    let close = rest.find(']').expect("the array is closed");
    rest[..close]
        .split(',')
        .filter_map(|piece| {
            let q = piece.find('"')?;
            let tail = &piece[q + 1..];
            Some(tail[..tail.find('"')?].to_string())
        })
        .collect()
}

/// The literals in `data?.code === "…"` inside the `!ok || data.ok === false` arm
/// of the terminal-tool call — the arm that maps a device failure onto a gateway
/// class. Scoped to that arm deliberately: `code ===` may legitimately appear
/// elsewhere in the file, and widening the search would report phantom drift.
fn gateway_matched_codes(ts: &str) -> Vec<String> {
    let arm = ts
        .find("if (!ok || data.ok === false)")
        .expect("mcp.ts has the !ok arm");
    let seg = &ts[arm..];
    let seg = &seg[..seg.find("\n  }").map(|e| e + 1).unwrap_or(seg.len())];
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(hit) = seg[cursor..].find("data?.code === \"") {
        let at = cursor + hit + "data?.code === \"".len();
        match seg[at..].find('"') {
            Some(end) => {
                out.push(seg[at..at + end].to_string());
                cursor = at + end;
            }
            None => break,
        }
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn the_agent_declares_exactly_the_codes_the_gateway_dispatches() {
    let rust = std::fs::read_to_string(repo_dir().join("agent/vale-command-core/src/error.rs"))
        .expect("error.rs is readable");
    let ts = std::fs::read_to_string(repo_dir().join("gateway/src/mcp.ts"))
        .expect("gateway/src/mcp.ts is readable");

    let mut declared = agent_dispatched_codes(&rust);
    declared.sort();
    let dispatched = gateway_matched_codes(&ts);

    assert_eq!(
        declared, dispatched,
        "The agent's GATEWAY_DISPATCHED_CODES and the codes gateway/src/mcp.ts actually \
         matches have drifted apart. Each side's own tests pin only its own literals, so \
         a change on the GATEWAY side — especially ADDING a code — passes both suites while \
         the agent's declared contract, and the `deliberately NOT dispatched` list that \
         documents the omissions, silently become false. Fix whichever side is wrong; if \
         the gateway genuinely gained a class, the agent's declaration and its not-dispatched \
         list both need to say so in the same commit.",
    );
}
