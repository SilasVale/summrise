//! THE JUDGE HALF OF THE TWO SATELLITE DIFFERENTIALS — data in, JSON out, bytes compared.
//!
//! WHY THIS CRATE EXISTS. Each satellite had a 400-line `verify.mjs` that did three jobs at once: run the
//! built wasm module (which needs a JS engine — see `gateway/wasm/run-cases.mjs`), hold the case list and
//! the upstream answers, and judge the bytes. **ONLY THE FIRST OF THOSE NEEDS JAVASCRIPT.** The other two
//! are data and logic, and they live here — one copy, used by both satellites, so a rule stated once
//! cannot drift between them.
//!
//! **IT IS A `[dev-dependencies]` CRATE AND THAT IS LOAD-BEARING**: it spawns a process and reads files,
//! neither of which exists on `wasm32-unknown-unknown`, and a normal dependency would be compiled into
//! the worker the moment some other crate enabled a feature that reached it. A dev-dependency is linked
//! into the TEST binaries only.
//!
//! THE SHAPE OF A RUN: the judge owns the cases and the upstream answers; it writes them as a spec JSON,
//! spawns `node run-cases.mjs --module <build/index.js> --spec <spec.json>`, and compares what came back
//! against `shipping-answers.json` — what the SHIPPING JavaScript answered, recorded by the harness that
//! drove it before it was deleted. The comparison rows are the ones that harness used: status, sorted
//! headers, body bytes, body, and **the upstream request**, which is the only row that can see a header
//! the worker sends upstream (the session id and both credentials live there).

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The upstream's answer for one path, or a single case's override of it.
#[derive(Clone, Debug)]
pub struct Answer {
    pub status: u16,
    pub headers: &'static [(&'static str, &'static str)],
    pub body: &'static str,
}

/// One request the differential drives.
#[derive(Clone, Debug)]
pub struct Case {
    pub label: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub headers: &'static [(&'static str, &'static str)],
    pub body: Option<&'static str>,
    /// When present, the upstream answers THIS for the case, whatever path it is asked about — how a 4xx
    /// and a 5xx are exercised on a path the `answers` table also serves.
    pub upstream: Option<Answer>,
}

/// Everything a run needs: where, with what env, and what the upstream says.
pub struct Spec {
    pub host: &'static str,
    pub env: &'static [(&'static str, &'static str)],
    pub answers: &'static [(&'static str, Answer)],
    pub cases: &'static [Case],
}

#[derive(Debug, PartialEq)]
pub struct Upstream {
    pub method: String,
    pub path: String,
    pub headers: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub struct Observed {
    pub label: String,
    pub status: u16,
    pub headers: Vec<String>,
    pub bytes: usize,
    pub body: String,
    pub upstream: Option<Upstream>,
}

/// One recorded answer — what the shipping JavaScript answered, as `shipping-answers.json` holds it.
#[derive(Debug)]
pub struct Recorded {
    pub method: String,
    pub path: String,
    pub status: u16,
    pub headers: Vec<String>,
    pub bytes: usize,
    pub body: String,
    pub upstream: Option<Upstream>,
}

/// The repo root, from a crate's `CARGO_MANIFEST_DIR` — **FOUND, NOT COUNTED**.
///
/// It counted three levels up, which is where `proxies/<satellite>/worker` sits. `relay/worker` sits two
/// levels down, and the count then walked ABOVE the repository: the first run of the file relay's
/// differential asked node for `/home/zss/gateway/wasm/run-cases.mjs`. Walking up to the `.git` entry is
/// the same answer for every crate and cannot be broken by a crate moving.
pub fn repo_root(manifest_dir: &str) -> PathBuf {
    Path::new(manifest_dir)
        .ancestors()
        .find(|p| p.join(".git").exists())
        .unwrap_or_else(|| {
            panic!("no .git above {manifest_dir} — is this crate inside the repository?")
        })
        .to_path_buf()
}

/// Build the module the differential drives — **ALWAYS, AND THE FIRST RUN OF THIS JUDGE IS WHY.**
///
/// It skipped the build when `build/index.js` existed, and the mutation that proves this gate (drop the
/// session header from the `/responses` arm) then PASSED: the judge drove the PREVIOUS build and reported
/// `4 passed`. **A DIFFERENTIAL THAT READS A STALE ARTIFACT IS A DIFFERENTIAL THAT VALIDATES THE LAST
/// BUILD**, which is this repository's own rule one level down — "when a test reads a built artifact,
/// `build` must run before `test`" — and `scripts/test/rust-byte-checks.bash` carries the same lesson from
/// 2026-10-05, when a mutated `build/` made `zen-go` report "1 of 17 DIFFER" against a correct tree.
///
/// So this always builds. `worker-build` is incremental, so the cost after the first run is a few seconds,
/// and the alternative is a green gate that measured something other than the tree.
///
/// **AND ONE BUILD AT A TIME.** `cargo test` runs a binary's tests on parallel threads, and the file
/// relay's two differentials each call this — measured 2026-10-08 on a cold tree: two concurrent
/// `worker-build` runs in one directory clobbered `build/.tmp`, one exited non-zero, and the harness
/// reported `worker-build failed in relay/worker`, which is a red CI that is NOT a defect. A process-wide
/// lock is enough because cargo runs test BINARIES one after another; the poison case is recovered rather
/// than propagated, since a panic in the other test says nothing about this build.
pub fn build(crate_dir: &Path) {
    static BUILD: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one_at_a_time = BUILD.lock().unwrap_or_else(|e| e.into_inner());
    println!("  building with worker-build --release …");
    let status = Command::new("worker-build")
        .arg("--release")
        .current_dir(crate_dir)
        .status()
        .unwrap_or_else(|e| {
            panic!(
                "worker-build is not runnable ({e}) — the module is gitignored, so a checkout without \
                 this build has no main to drive. Install it: cargo install worker-build --locked"
            )
        });
    assert!(
        status.success(),
        "worker-build failed in {}",
        crate_dir.display()
    );
}

fn answer_json(a: &Answer) -> Value {
    json!({
        "status": a.status,
        "headers": a.headers.iter().copied().collect::<BTreeMap<_, _>>(),
        "body": a.body,
    })
}

fn spec_json(spec: &Spec) -> Value {
    let cases: Vec<Value> = spec
        .cases
        .iter()
        .map(|c| {
            let mut o = Map::new();
            o.insert("label".into(), json!(c.label));
            o.insert("method".into(), json!(c.method));
            o.insert("path".into(), json!(c.path));
            if !c.headers.is_empty() {
                o.insert(
                    "headers".into(),
                    json!(c.headers.iter().copied().collect::<BTreeMap<_, _>>()),
                );
            }
            if let Some(b) = c.body {
                o.insert("body".into(), json!(b));
            }
            if let Some(a) = &c.upstream {
                o.insert("override".into(), answer_json(a));
            }
            Value::Object(o)
        })
        .collect();
    json!({
        "host": spec.host,
        "env": spec.env.iter().copied().collect::<BTreeMap<_, _>>(),
        "answers": spec.answers.iter().map(|(p, a)| ((*p).to_string(), answer_json(a))).collect::<BTreeMap<_, _>>(),
        "cases": cases,
    })
}

/// Run the cases through the built module, in the JS runner, and return what it saw.
pub fn run(spec: &Spec, module: &Path, runner: &Path) -> Vec<Observed> {
    let spec_path = std::env::temp_dir().join(format!(
        "differential-spec-{}-{}.json",
        std::process::id(),
        spec.cases.len()
    ));
    fs::write(
        &spec_path,
        serde_json::to_vec(&spec_json(spec)).expect("a spec"),
    )
    .unwrap_or_else(|e| panic!("cannot write {}: {e}", spec_path.display()));

    let out = Command::new("node")
        .arg(runner)
        .arg("--module")
        .arg(module)
        .arg("--spec")
        .arg(&spec_path)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "cannot run node ({e}) — this differential executes the wasm artifact, \
                                    and that needs a JS engine"
            )
        });
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() {
        panic!(
            "the runner failed ({}):\n{}\n{}",
            out.status,
            stdout,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let parsed: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("the runner printed no JSON ({e}):\n{stdout}"));
    parsed["observed"]
        .as_array()
        .expect("an observed array")
        .iter()
        .map(|o| Observed {
            label: o["label"].as_str().unwrap_or_default().to_string(),
            status: o["status"].as_u64().unwrap_or_default() as u16,
            headers: strings(&o["headers"]),
            bytes: o["bytes"].as_u64().unwrap_or_default() as usize,
            body: o["body"].as_str().unwrap_or_default().to_string(),
            upstream: upstream_of(&o["upstream"]),
        })
        .collect()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn upstream_of(v: &Value) -> Option<Upstream> {
    if v.is_null() {
        return None;
    }
    Some(Upstream {
        method: v["method"].as_str().unwrap_or_default().to_string(),
        path: v["path"].as_str().unwrap_or_default().to_string(),
        headers: strings(&v["headers"]),
    })
}

/// `shipping-answers.json`, as the recorded reference.
pub fn recorded(fixture: &Path) -> Vec<Recorded> {
    let raw = fs::read_to_string(fixture)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", fixture.display()));
    let parsed: Value = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("{} is not JSON: {e}", fixture.display()));
    parsed["cases"]
        .as_array()
        .expect("a cases array")
        .iter()
        .map(|c| Recorded {
            method: c["method"].as_str().unwrap_or_default().to_string(),
            path: c["path"].as_str().unwrap_or_default().to_string(),
            status: c["status"].as_u64().unwrap_or_default() as u16,
            headers: strings(&c["headers"]),
            bytes: c["bytes"].as_u64().unwrap_or_default() as usize,
            body: c["body"].as_str().unwrap_or_default().to_string(),
            upstream: upstream_of(&c["upstream"]),
        })
        .collect()
}

/// **THE COMPARISON, AND ITS ROWS ARE THE POINT.** A response-only check cannot see a header the worker
/// sends UPSTREAM; that row is where the session id and both credentials live, and it is the row whose
/// absence once made a real defect invisible (the harness's own mutation proof recorded it: dropping the
/// session header changed no response byte at all).
pub fn judge(name: &str, cases: &[Case], expected: &[Recorded], observed: &[Observed]) -> usize {
    assert_eq!(
        cases.len(),
        expected.len(),
        "{name}: the fixture holds {} case(s) and the table has {} — it was recorded from a different \
         list, and re-recording needs the shipping half back from git history",
        expected.len(),
        cases.len()
    );
    let mut bad = 0;
    for (i, c) in cases.iter().enumerate() {
        let want = &expected[i];
        let got = &observed[i];
        assert_eq!(
            (want.method.as_str(), want.path.as_str()),
            (c.method, c.path),
            "{name}: fixture entry {i} is {} {} and the table's is {} {}",
            want.method,
            want.path,
            c.method,
            c.path
        );
        let rows: [(&str, String, String); 5] = [
            ("status", want.status.to_string(), got.status.to_string()),
            ("headers", want.headers.join(" | "), got.headers.join(" | ")),
            ("body bytes", want.bytes.to_string(), got.bytes.to_string()),
            ("body", want.body.clone(), got.body.clone()),
            (
                "upstream request",
                upstream_row(&want.upstream),
                upstream_row(&got.upstream),
            ),
        ];
        let differs: Vec<_> = rows.iter().filter(|(_, w, g)| w != g).collect();
        println!("  {}", c.label);
        println!(
            "      {} {} {} — {} bytes, status {}",
            if differs.is_empty() { "ok  " } else { "FAIL" },
            c.method,
            c.path,
            want.bytes,
            want.status
        );
        if !differs.is_empty() {
            bad += 1;
            for (what, w, g) in differs {
                println!("      {what} DIFFERS");
                println!("        recorded: {}", clip(w));
                println!("        rust:     {}", clip(g));
            }
        }
    }
    bad
}

fn upstream_row(u: &Option<Upstream>) -> String {
    match u {
        Some(u) => format!("{} {} | {}", u.method, u.path, u.headers.join(" | ")),
        None => "(none)".to_string(),
    }
}

fn clip(s: &str) -> String {
    let json = serde_json::to_string(s).unwrap_or_default();
    json.chars().take(300).collect()
}

/// The closing line every harness in this repository prints, so `rust-byte-checks.bash` and a reader can
/// both read the verdict without parsing the rows.
pub fn verdict(name: &str, total: usize, bad: usize) {
    println!(
        "\n{name} verify: {}",
        if bad == 0 {
            format!("{total} case(s) byte-identical, upstream requests included")
        } else {
            format!("{bad} of {total} case(s) DIFFER")
        }
    );
    assert_eq!(bad, 0, "{name}: {bad} of {total} case(s) differ");
}
