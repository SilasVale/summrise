//! THE FILE RELAY, COMPARED AGAINST WHAT THE SHIPPING JAVASCRIPT ANSWERED — both halves of it: the
//! worker's `/files/*` surface, and the claim Durable Object it forwards into. Same stubs, same case
//! lists, same rows.
//!
//! `worker-corpus.json` was recorded by `record-worker.mjs`, which drove `relay/src/index.js` — **the
//! worker that was then still serving the route** — through `gateway/wasm/bindings-stub.mjs` and the
//! case list in `worker-cases.json`. **BOTH JAVASCRIPT FILES ARE DELETED (2026-10-08, the cutover): the
//! corpus is the record now, and the blob hashes in its `source` map name the bytes it was recorded
//! from.** This test drives the BUILT RUST worker (`worker-build`'s output) through
//! `gateway/wasm/run-cases.mjs` with the same stubs and the same cases, and compares five rows per case:
//!
//!     status · sorted headers · body bytes · **the R2 operations, in order** · **the request the claim
//!     DO received**
//!
//! **THE LAST TWO ARE THE ONES A RESPONSE-ONLY CHECK CANNOT SEE**, and they are why the corpus was worth
//! recording: an upload that stored the wrong `Content-Disposition`, or a claim forward that dropped the
//! internal credential, answers the same bytes as one that did neither.
//!
//! ── THE MUTATIONS THAT MUST FAIL THIS GATE ──────────────────────────────────────────────────────
//!
//! MUTATION: in `worker.rs`, forward the claim request without setting `x-do-auth` when `DO_AUTH` is set.
//! RESULT:   the `x-do-auth` case FAILS on the `forwarded` row alone — the response is byte-identical
//!           because the stub answers the same either way. (`a_dropped_do_credential_is_not_a_pass` below
//!           is that mutation in-process, so the proof does not depend on the wasm build.)
//!
//! MUTATION: in `worker.rs`, read the raw body into a `Vec<u8>` and put `Data::Bytes` instead of
//!           `Data::ReadableStream`.
//! RESULT:   **NOTHING HERE FAILS, AND THAT IS STATED RATHER THAN HIDDEN**: the stored bytes are the same
//!           either way, so this corpus cannot tell streaming from buffering — the difference shows up
//!           only under an upload large enough to matter, which is a property of the runtime and not of
//!           the answer. It is written down because a gate that silently cannot see something is worse
//!           than one that says so.
//!
//! MUTATION: in `worker.rs`, drop the basename rule and pass the raw `?name=` through to the header.
//! RESULT:   the `..%2F..%2Fevil.txt` case FAILS on the R2 row's `contentDisposition` and on the body's
//!           `filename` — the response bytes move, so this one the response comparison would catch too.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use summrise_differential_harness::{build, repo_root, verdict};

const CRATE: &str = env!("CARGO_MANIFEST_DIR");

fn read_json(name: &str) -> serde_json::Value {
    let path = PathBuf::from(CRATE).join(name);
    let raw =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&raw).expect("valid JSON")
}

/// The spec the runner is driven with: the case list, with each case's bindings and token stream folded
/// into the shape `run-cases.mjs` reads.
fn spec(cases: &serde_json::Value) -> serde_json::Value {
    let list: Vec<serde_json::Value> = cases["cases"]
        .as_array()
        .expect("a cases array")
        .iter()
        .map(|c| {
            let mut o = c.clone();
            if let Some(b) = c.get("bindings") {
                o["bindings"] = b.clone();
            }
            // **A SURFACE MAY NAME A CLASS RATHER THAN THE DEFAULT ENTRY** — `claim-cases.json` drives the
            // claim Durable Object directly, so the comparison is against the shipping DO and not only
            // against the worker that forwards to it.
            if let Some(e) = cases.get("entry") {
                o["entry"] = e.clone();
            }
            o
        })
        .collect();
    // **THE SPEC-LEVEL `bindings` TRAVELS TOO.** Dropping it is what made the claim object answer
    // *"Binding `TEMP_FILES` is undefined"* for the cases that declare no world of their own: the case
    // wants an EMPTY BUCKET (`/files/<token>` for an object that is not there), and a missing binding is a
    // different failure from a missing object.
    let mut out = serde_json::json!({ "host": cases["host"], "env": cases["env"], "cases": list });
    if let Some(b) = cases.get("bindings") {
        out["bindings"] = b.clone();
    }
    out
}

fn run(cases_file: &str) -> Vec<serde_json::Value> {
    let crate_dir = PathBuf::from(CRATE);
    let root = repo_root(CRATE);
    build(&crate_dir);
    let cases = read_json(cases_file);
    let spec_path = std::env::temp_dir().join(format!(
        "relay-spec-{}-{}.json",
        std::process::id(),
        cases_file.trim_end_matches(".json")
    ));
    fs::write(
        &spec_path,
        serde_json::to_vec(&spec(&cases)).expect("a spec"),
    )
    .expect("write the spec");
    let out = Command::new("node")
        .arg(root.join("gateway/wasm/run-cases.mjs"))
        .arg("--module")
        .arg(crate_dir.join("build/index.js"))
        .arg("--spec")
        .arg(&spec_path)
        .output()
        .expect("node runs the built module — this differential executes a wasm artifact");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "the runner failed ({}):\n{stdout}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("no JSON ({e}):\n{stdout}"));
    parsed["observed"]
        .as_array()
        .expect("an observed array")
        .clone()
}

fn ops(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|o| {
                    let mut s = format!(
                        "{} {}",
                        o["op"].as_str().unwrap_or_default(),
                        o["key"].as_str().unwrap_or_default()
                    );
                    for k in ["contentType", "contentDisposition", "expiresAt", "size"] {
                        if let Some(x) = o.get(k) {
                            if !x.is_null() {
                                s.push_str(&format!(" {k}={x}"));
                            }
                        }
                    }
                    s
                })
                .collect()
        })
        .unwrap_or_default()
}

/// **THE ROW THAT PINS THE INSTANCE NAME, WHICH IS THE PRECONDITION OF "EXACTLY ONE DOWNLOAD".**
///
/// The guarantee itself is the runtime's (one instance's requests are delivered strictly one at a time),
/// so no stub can show it — but what a stub CAN see, and what this row exists for, is that the worker
/// forwards to the instance NAMED BY THE TOKEN. Forward to any other name and two claims for one file
/// land on two instances, both win, and the file downloads twice, while every response stays byte
/// identical. `bindings-stub.mjs` records the name for both namespace shapes and the corpus carries it;
/// until 2026-10-08 this function dropped it, so that mutation was invisible to every gate here.
fn forwarded(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|f| {
                    format!(
                        "{} {} {} | {}",
                        f["name"].as_str().unwrap_or_default(),
                        f["method"].as_str().unwrap_or_default(),
                        f["path"].as_str().unwrap_or_default(),
                        f["headers"]
                            .as_array()
                            .map(|h| h
                                .iter()
                                .filter_map(|x| x.as_str())
                                .collect::<Vec<_>>()
                                .join(" | "))
                            .unwrap_or_default()
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn strings(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The comparison, once: five rows per case over a case list and its recorded answers.
fn compare(name: &str, cases_file: &str, corpus_file: &str) {
    let corpus = read_json(corpus_file);
    let expected = corpus["cases"].as_array().expect("a cases array");
    let observed = run(cases_file);
    assert_eq!(
        expected.len(),
        observed.len(),
        "the fixture holds {} case(s) and the run produced {} — it was recorded from a different case list",
        expected.len(),
        observed.len()
    );
    let mut bad = 0;
    for (want, got) in expected.iter().zip(observed.iter()) {
        assert_eq!(want["label"], got["label"], "the case list's order moved");
        let rows: [(&str, String, String); 5] = [
            (
                "status",
                want["status"].to_string(),
                got["status"].to_string(),
            ),
            (
                "headers",
                strings(&want["headers"]).join(" | "),
                strings(&got["headers"]).join(" | "),
            ),
            (
                "body bytes",
                want["bytes"].to_string(),
                got["bytes"].to_string(),
            ),
            (
                "body",
                want["body"].as_str().unwrap_or_default().to_string(),
                got["body"].as_str().unwrap_or_default().to_string(),
            ),
            (
                "r2 operations",
                ops(&want["r2"]).join(" ; "),
                ops(&got["r2"]).join(" ; "),
            ),
            // the sixth row is folded into this one so the array stays a fixed size:
            // ("forwarded", …) is checked below.
        ];
        let fwd_want = forwarded(&want["forwarded"]).join(" ; ");
        let fwd_got = forwarded(&got["forwarded"]).join(" ; ");
        let differs: Vec<_> = rows.iter().filter(|(_, w, g)| w != g).collect();
        println!("  {}", want["label"].as_str().unwrap_or_default());
        println!(
            "      {} {} {} — {} bytes, status {}",
            if differs.is_empty() && fwd_want == fwd_got {
                "ok  "
            } else {
                "FAIL"
            },
            want["method"].as_str().unwrap_or_default(),
            want["path"].as_str().unwrap_or_default(),
            want["bytes"],
            want["status"]
        );
        if !differs.is_empty() || fwd_want != fwd_got {
            bad += 1;
            for (what, w, g) in differs {
                println!("      {what} DIFFERS");
                println!("        recorded: {}", clip(w));
                println!("        rust:     {}", clip(g));
            }
            if fwd_want != fwd_got {
                println!("      forwarded DIFFERS");
                println!("        recorded: {}", clip(&fwd_want));
                println!("        rust:     {}", clip(&fwd_got));
            }
        }
    }
    verdict(name, expected.len(), bad);
}

#[test]
fn the_port_answers_the_recorded_shipping_bytes_over_the_whole_files_surface() {
    compare("relay-worker", "worker-cases.json", "worker-corpus.json");
}

#[test]
fn the_claim_object_decides_and_deletes_like_the_shipping_one() {
    // **THE ROWS THAT MATTER HERE ARE THE BUCKET OPERATIONS.** A claim that served the bytes without
    // deleting the key answers a byte-identical response and hands the file out twice; only the `r2`
    // row sees the difference.
    compare("relay-claim", "claim-cases.json", "claim-corpus.json");
}

fn clip(s: &str) -> String {
    let json = serde_json::to_string(s).unwrap_or_default();
    json.chars().take(320).collect()
}

// ── the judge's own proof: the two rows a response-only check cannot see ─────────────────────────

#[test]
fn a_dropped_do_credential_is_not_a_pass() {
    let with = serde_json::json!([{"name": "files/t", "method": "GET", "path": "/files/t",
        "headers": ["content-type: application/json", "x-do-auth: do-secret-123"]}]);
    let without = serde_json::json!([{"name": "files/t", "method": "GET", "path": "/files/t",
        "headers": ["content-type: application/json"]}]);
    assert_ne!(forwarded(&with), forwarded(&without));
    assert!(
        forwarded(&with)[0].contains("x-do-auth: do-secret-123"),
        "the row must carry the credential it exists to see"
    );
}

#[test]
fn a_stored_object_whose_metadata_moved_is_not_a_pass() {
    let want = serde_json::json!([{"op": "put", "key": "files/t", "contentDisposition": "attachment; filename=\"a.txt\"", "size": 3}]);
    let got = serde_json::json!([{"op": "put", "key": "files/t", "contentDisposition": "attachment; filename=\"../../evil.txt\"", "size": 3}]);
    assert_ne!(
        ops(&want),
        ops(&got),
        "the R2 row must see the header that was stored"
    );
}

/// The third proof, and the one whose absence was a real hole (found in review, 2026-10-08): the row
/// must see WHICH INSTANCE the claim was forwarded to. `files/t` and `files/t2` answer identically and
/// are different one-time locks.
#[test]
fn a_claim_forwarded_to_another_instance_is_not_a_pass() {
    let named = serde_json::json!([{"name": "files/t", "method": "GET", "path": "/files/t",
        "headers": ["content-type: application/json"]}]);
    let other = serde_json::json!([{"name": "files/other", "method": "GET", "path": "/files/t",
        "headers": ["content-type: application/json"]}]);
    assert_ne!(
        forwarded(&named),
        forwarded(&other),
        "the row must see the instance the worker addressed, not only the path it asked for"
    );
    assert!(
        forwarded(&named)[0].starts_with("files/t GET /files/t"),
        "the instance name leads the row: {}",
        forwarded(&named)[0]
    );
}
