//! THE FILE RELAY'S DECISION HALF, COMPARED AGAINST WHAT THE SHIPPING JAVASCRIPT ANSWERED.
//!
//! `pure-corpus.json` was recorded by `record-pure.mjs` — which imported `relay/src/index.js` and
//! `relay/src/claim.js` and asked them 31 filenames, 220 claim states and 10 digests — and this test
//! drives the Rust functions over the same inputs and compares the answers. **THE ORACLE WAS THE
//! IMPLEMENTATION STILL SERVING THE FILE RELAY'S ROUTE**, which is the strongest form this comparison
//! could take; **the cutover deleted that JavaScript on 2026-10-08 and the corpus is the record now**,
//! the way `shipping-answers.json` is for the satellites.
//!
//! ── THE THREE MUTATIONS THAT MUST FAIL THIS GATE (all measured; the crate's own header argues them)
//!
//! MUTATION: `js_trim` → `str::trim`.
//! RESULT:   `"\u{85}nel.txt"` FAILS — the shipping worker does NOT trim U+0085 (Rust does), so its
//!           header keeps the character in `filename*=UTF-8''%C2%85nel.txt` and the port would not.
//!
//! MUTATION: `decide_claim` returns `Serve` where `!ts.is_finite()`.
//! RESULT:   the `"abc"`, `"NaN"` and `{}` rows FAIL — the shipping rule fails CLOSED on an unparseable
//!           deadline, and the difference is an unbounded download where the worker answers 410.
//!
//! MUTATION: `gen_token_from`'s `RANGE` → `256` (i.e. `byte % 62` with no rejection sampling).
//! RESULT:   `the_alphabet_is_uniform_and_not_overweighted` FAILS: A–H come out ~4% more often than the
//!           rest, which is the bias the shipping implementation's comment describes.
//!
//! AND ONE THAT MUST NOT FAIL, WHICH IS WHY IT IS WRITTEN DOWN: the corpus's `source` blobs pin the two
//! JavaScript files this was recorded from, and those files are DELETED — so the pin is a pointer into
//! this repository's history rather than at the tree (`git cat-file blob 3afc456c…`). **A RECORDER THAT
//! DRIFTED USED TO BE CAUGHT BY `node record-pure.mjs --check`, WHICH NO LONGER EXISTS**: the
//! comparison it ran was against the implementation it recorded from, and that implementation is gone.
//! From here the corpus changes only by a deliberate re-record, and the pin is what says from which
//! bytes.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use summrise_relay_worker::{
    build_content_disposition, decide_claim, encode_uri_component, gen_token_from, js_number,
    js_trim, sha256_matches, Claim, TOKEN_CHARS, UNAVAILABLE,
};

fn corpus() -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("pure-corpus.json");
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e} — the corpus is the record since the 2026-10-08 cutover; it is \
             regenerated deliberately, never by a script that no longer exists",
            path.display()
        )
    });
    serde_json::from_str(&raw).expect("pure-corpus.json is JSON")
}

/// A recorded header is a string or `null` (the shipping function returns `null` when nothing survives).
fn recorded_header(v: &serde_json::Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

#[test]
fn every_recorded_filename_builds_the_same_content_disposition() {
    let c = corpus();
    let cases = c["disposition"].as_array().expect("a disposition array");
    assert!(
        cases.len() >= 30,
        "only {} case(s) in the corpus",
        cases.len()
    );
    let mut bad = 0;
    for case in cases {
        let raw = case["raw"].as_str().expect("a raw name");
        let want = recorded_header(&case["header"]);
        let got = build_content_disposition(raw);
        if got != want {
            bad += 1;
            println!("  FAIL {raw:?}");
            println!("    recorded: {want:?}");
            println!("    rust:     {got:?}");
        }
    }
    assert_eq!(
        bad,
        0,
        "{bad} of {} filename(s) build a different header",
        cases.len()
    );
    println!(
        "relay-pure: {} filename(s) build the recorded Content-Disposition",
        cases.len()
    );
}

#[test]
fn every_recorded_claim_state_decides_the_same_way() {
    let c = corpus();
    let cases = c["claims"].as_array().expect("a claims array");
    assert!(
        cases.len() >= 200,
        "only {} case(s) in the corpus",
        cases.len()
    );
    let mut bad = 0;
    for case in cases {
        let exists = case["exists"].as_bool().expect("exists");
        // **A MISSING KEY IS `undefined`** — `JSON.stringify` drops it, which is exactly how JavaScript
        // spells the branch the shipping rule fails OPEN on.
        let raw = case.get("expiresAtRaw");
        let now_ms = case["nowMs"].as_f64().expect("nowMs");
        let want = match case["decision"].as_str().expect("a decision") {
            "gone" => Claim::Gone,
            "serve" => Claim::Serve,
            "expired" => Claim::Expired,
            other => panic!("unknown recorded decision {other:?}"),
        };
        let got = decide_claim(exists, raw, now_ms);
        if got != want {
            bad += 1;
            println!(
                "  FAIL exists={exists} expiresAtRaw={raw:?} nowMs={now_ms} — recorded {want:?}, rust {got:?}"
            );
        }
    }
    assert_eq!(
        bad,
        0,
        "{bad} of {} claim state(s) decide differently",
        cases.len()
    );
    println!(
        "relay-pure: {} claim state(s) decide the recorded way",
        cases.len()
    );
}

#[test]
fn the_digest_shape_and_the_unavailable_envelope_match() {
    let c = corpus();
    for case in c["sha256"].as_array().expect("a sha256 array") {
        let input = case["input"].as_str().expect("an input");
        assert_eq!(
            sha256_matches(input),
            case["matches"].as_bool().expect("matches"),
            "SHA256_RE on {input:?}"
        );
    }
    let env = &c["envelopes"][0];
    assert_eq!(env["name"].as_str(), Some("unavailableResponse"));
    assert_eq!(
        UNAVAILABLE.status,
        env["status"].as_u64().unwrap_or_default() as u16
    );
    assert_eq!(UNAVAILABLE.body, env["body"].as_str().unwrap_or_default());
    let headers: Vec<String> = env["headers"]
        .as_array()
        .expect("headers")
        .iter()
        .filter_map(|h| h.as_str().map(str::to_string))
        .collect();
    assert_eq!(
        headers,
        vec![format!("content-type: {}", UNAVAILABLE.content_type)],
        "the envelope's headers"
    );
    println!("relay-pure: the digest shape and the 503 envelope match");
}

// ── the properties a corpus CANNOT record, and the reason they are here instead ────────────────────

#[test]
fn the_alphabet_is_uniform_and_not_overweighted() {
    // A uniform byte stream is what `crypto.getRandomValues` gives. **THE REJECTION SAMPLING IS THE
    // DECISION THIS TESTS**: with `RANGE = 248`, bytes 248..=255 are redrawn, so all 62 symbols are
    // equally likely; with `%62` and no rejection, A–H get 5 draws each instead of 4 and come out ~4%
    // heavy over 256 draws. 200,000 samples make that difference ~40 standard errors wide.
    let bytes: Vec<u8> = (0..200_000u32).map(|i| (i % 256) as u8).collect();
    let token = gen_token_from(bytes.into_iter(), 100_000);
    assert_eq!(token.len(), 100_000);
    let mut counts: BTreeMap<char, usize> = BTreeMap::new();
    for ch in token.chars() {
        *counts.entry(ch).or_default() += 1;
    }
    assert_eq!(counts.len(), 62, "the alphabet is not fully covered");
    let expected = 100_000.0 / 62.0;
    let (min, max) = (
        *counts.values().min().unwrap() as f64,
        *counts.values().max().unwrap() as f64,
    );
    assert!(
        min > expected * 0.97 && max < expected * 1.03,
        "the distribution is not uniform: min {min}, max {max}, expected {expected:.1} — \
         `RANGE` is the rejection threshold, and `%62` without it overweights A–H"
    );
    // The rejection itself, stated directly: 248..=255 are skipped, so a stream of them produces nothing.
    assert_eq!(gen_token_from((248u8..=255).cycle().take(100), 5), "");
    assert_eq!(
        gen_token_from([0u8, 247, 248, 62].into_iter(), 3),
        "A9A",
        "0 → A, 247 → 9 (247 % 62 = 61, the last symbol), 248 → skipped, 62 → A"
    );
    println!("relay-pure: 100,000 draws over 62 symbols, uniform within 3%");
}

#[test]
fn the_three_code_points_where_the_two_languages_disagree_are_handled_javascripts_way() {
    // **THE MUTATION IN THE HEADER, IN PROCESS.** These are the code points where `str::trim` and
    // JavaScript's `trim` disagree, and each one changes a header:
    assert_eq!(
        js_trim("\u{85}nel"),
        "\u{85}nel",
        "U+0085 is NOT whitespace in JavaScript (Rust: is)"
    );
    assert_eq!(
        js_trim("\u{feff}bom"),
        "bom",
        "U+FEFF IS whitespace in JavaScript (Rust: is not)"
    );
    assert_eq!(
        js_trim("\u{a0}nbsp"),
        "nbsp",
        "U+00A0 IS whitespace in JavaScript (Rust: is not)"
    );
    // ...and the header they produce, which is what a browser decodes:
    assert_eq!(
        build_content_disposition("\u{85}nel.txt").as_deref(),
        Some("attachment; filename=\"nel.txt\"; filename*=UTF-8''%C2%85nel.txt")
    );
    assert_eq!(
        build_content_disposition("\u{feff}bom.txt").as_deref(),
        Some("attachment; filename=\"bom.txt\"")
    );
}

#[test]
fn number_coercion_matches_javascripts() {
    // The claim rule is `Number.isFinite(Number(raw))`, so these all matter.
    for (raw, want) in [
        ("0x10", 16.0),
        (" 42 ", 42.0),
        ("1e3", 1000.0),
        ("-5", -5.0),
        ("", 0.0),
        ("   ", 0.0),
        ("Infinity", f64::INFINITY),
    ] {
        assert_eq!(
            js_number(Some(&serde_json::Value::String(raw.into()))),
            want,
            "Number({raw:?})"
        );
    }
    for raw in ["abc", "infinity", "-0x10", "NaN", "1.2.3"] {
        assert!(
            js_number(Some(&serde_json::Value::String(raw.into()))).is_nan(),
            "Number({raw:?}) should be NaN"
        );
    }
    assert_eq!(js_number(Some(&serde_json::json!([]))), 0.0);
    assert_eq!(js_number(Some(&serde_json::json!([7]))), 7.0);
    assert!(js_number(Some(&serde_json::json!([1, 2]))).is_nan());
    assert!(js_number(Some(&serde_json::json!({}))).is_nan());
    assert_eq!(js_number(Some(&serde_json::json!(true))), 1.0);
    // The strict comparison: a deadline exactly at now SERVES.
    assert_eq!(
        decide_claim(true, Some(&serde_json::json!(1000)), 1000.0),
        Claim::Serve
    );
    assert_eq!(
        decide_claim(true, Some(&serde_json::json!(999)), 1000.0),
        Claim::Expired
    );
}

#[test]
fn encode_uri_component_leaves_javascripts_unreserved_set_alone() {
    assert_eq!(encode_uri_component("a-_.!~*'()"), "a-_.!~*'()");
    assert_eq!(encode_uri_component("a b&c=d"), "a%20b%26c%3Dd");
    assert_eq!(encode_uri_component("报告"), "%E6%8A%A5%E5%91%8A");
    assert_eq!(encode_uri_component("café"), "caf%C3%A9");
    // The alphabet the two share, for the record: 62 symbols, and `TOKEN_CHARS` is the shipping order.
    assert_eq!(TOKEN_CHARS.len(), 62);
    assert!(
        TOKEN_CHARS.starts_with("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789")
    );
}
