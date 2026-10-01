//! THE WIRE VOCABULARY, CHECKED AT BOTH ENDS.
//!
//! WHY THIS EXISTS (round 44 of the standing goal). Every string in `agent/contract-vocabulary.json`
//! is written by the DEVICE and switched on by an INTERFACE, and until that round each end spelled
//! them independently: `"monitor-change"` in `monitor.rs` and again in the panel's `useMonitors`,
//! `"crashed"` in `runstate.rs` and again in `bootNotice.ts`, the command end reasons in `exec.rs`
//! and again in `stateFromEnd`. Two hand-written copies of one fact is the exact shape the objective
//! exists to remove, and each copy was kept in step by tests that read ONE side.
//!
//! THE SOURCE OF TRUTH IS RUST (`agent/src/vocabulary.rs`); the artifacts are generated from it by
//! `SUMMRISE_REFRESH_CONTRACT=1 cargo test contract_vocabulary_snapshot`. This gate checks the
//! DIRECTIONS that catch real drift, by name and by file:
//!
//!   1. the two artifacts agree with each other — a hand-edited one is caught;
//!   2. every `"ev"` literal in the agent's Rust sources is a listed frame — a NEW frame cannot be
//!      invented silently;
//!   3. every boot kind the enum spells is listed, and every kind listed is spelled by the enum —
//!      the fifth kind (`machine-restart`) was missing from the first draft of the list, which is
//!      how this direction earned its place;
//!   4. the PANEL's readers only compare against listed values (`bootNotice.ts` kinds,
//!      `stateFromEnd` reasons) — a comparison against a string the device never writes is a state
//!      that silently never renders;
//!   5. every declared value is one the device actually WRITES (round 156), and
//!   6. every declared value is READ by an interface (round 157) — the two symmetric halves of the
//!      artifact's own claim, "the strings the device writes and the interfaces read". Without them
//!      a value declared and emitted nowhere agrees with itself perfectly.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28):
//!   * `node scripts/test/contract-vocabulary-check.mjs` → the seven `ok` lines ("FRAMES: both
//!     artifacts carry the same 5 value(s)", "END_REASONS … 6", "10 frame
//!     literal(s) in the agent, every one listed (5 known)", "5 boot kind(s) agree in both
//!     directions", "7 boot kind(s) in bootNotice.ts", "6 end reason(s) in the derivation's table")
//!     then "contract-vocabulary: the device and the interfaces spell one vocabulary".
//!   * this file → the SAME seven lines and the SAME closing line, character for character.
//!   * a planted `frames.push(("ev", "not-a-listed-frame"));` in `agent/src/monitor.rs` → the JS gate
//!     exits 1 with `agent/src/monitor.rs:<line> pushes frame "not-a-listed-frame", which
//!     contract-vocabulary.json does not list — add it to agent/src/vocabulary.rs`, and this file
//!     fails with the identical sentence.
//!   * a frame named ONLY in a comment (`// "ev": "comment-only-frame"`) → BOTH STAY GREEN, which is
//!     the honest measurement rather than a divergence: `decomment` is what stops prose from
//!     satisfying the scan, and removing the producer while leaving the comment is exactly the case
//!     it was written for.
//!
//! MUTATION: push an `"ev"` frame the contract does not list, or hand-edit one of the generated
//!           artifacts so the two disagree.
//! RESULT:   fails, naming the file, the LINE and the frame, and telling the reader to add it to
//!           `agent/src/vocabulary.rs` rather than to the artifact.

mod common;

use common::{decomment, files_under, git_ls_files, read, rel_to_repo};
use std::collections::BTreeSet;

const JSON: &str = "agent/contract-vocabulary.json";
const TS_GEN: &str = "agent/resources/panel-react/src/lib/contract.gen.ts";

/// The arrays the two artifacts both carry, with the name each has in the generated TS.
/// THE ONE LIST `contract.gen.ts` STILL EMITS, after 2026-09-30.
///
/// `end_reasons` left this table when the derivation that was its only reader moved into
/// `panel-logic/src/path.rs`, and `boot_kinds` left it the same way one round later: the boot notice
/// is `panel-logic/src/boot_notice.rs` now, so the generator stopped emitting `BOOT_KINDS` (the TYPE
/// stays — two panel signatures take it) and the crate's own copy is compared against the JSON by
/// `crate_boot_kinds` further down. A list nobody imports is a promise nobody asked for, and
/// `exports-check` refused the emitted one on exactly that ground.
const ARRAYS: [(&str, &str); 1] = [("frames", "FRAMES")];

/// The readers the panel's clauses are about.
/// THE BOOT NOTICE IS THE CRATE'S FILE NOW (2026-09-30), so the scan below reads where the kinds are
/// actually spelled — the same move `wire_fields.rs` made when a family left the TypeScript.
const BOOT_NOTICE: &str = "agent/resources/panel-logic/src/boot_notice.rs";
const PATH_RS: &str = "agent/resources/panel-logic/src/path.rs";
const CRATE_VOCAB: &str = "agent/resources/panel-logic/src/vocabulary.rs";
const RUNSTATE: &str = "agent/src/runstate.rs";

/// The interface trees whose sources must READ every declared value.
const INTERFACE_DIRS: [&str; 3] = [
    "agent/resources/panel-react/src",
    "gateway/src",
    "gateway/ui/src",
];

struct Out {
    lines: Vec<String>,
    fail: usize,
}

impl Out {
    fn new() -> Self {
        Out {
            lines: Vec::new(),
            fail: 0,
        }
    }
    fn ok(&mut self, m: impl AsRef<str>) {
        self.lines.push(format!("ok   {}", m.as_ref()));
    }
    fn bad(&mut self, m: impl AsRef<str>) {
        self.fail += 1;
        self.lines.push(format!("FAIL {}", m.as_ref()));
    }
    fn bad_many(&mut self, n: usize, m: impl AsRef<str>) {
        self.fail += n;
        self.lines.push(format!("FAIL {}", m.as_ref()));
    }
}

/// `export const NAME = (\[[^\]]*\]) as const;` — through `serde_json`, exactly as the JS puts it
/// through `JSON.parse`. (The parentheses in that JS regex are GROUP delimiters, not literals.)
fn ts_array(gen: &str, name: &str) -> Option<Vec<String>> {
    let anchor = format!("export const {name} = ");
    let rest = &gen[gen.find(&anchor)? + anchor.len()..];
    let open = rest.find('[')?;
    let close = rest[open..].find(']')? + open;
    let v: Vec<String> = serde_json::from_str(&rest[open..=close]).ok()?;
    if !rest[close + 1..].starts_with(" as const;") {
        return None;
    }
    Some(v)
}

fn json_compact(v: &[String]) -> String {
    serde_json::to_string(v).expect("a Vec<String> serialises")
}

/// `"ev"\s*:\s*"([a-z-]+)"` — the first frame literal on a line.
fn frame_on_line(line: &str) -> Option<String> {
    let c: Vec<char> = line.chars().collect();
    let needle: Vec<char> = "\"ev\"".chars().collect();
    let mut i = 0;
    while i + needle.len() <= c.len() {
        if c[i..i + needle.len()] != needle[..] {
            i += 1;
            continue;
        }
        let mut j = i + needle.len();
        while j < c.len() && c[j].is_whitespace() {
            j += 1;
        }
        if c.get(j) != Some(&':') {
            i += 1;
            continue;
        }
        j += 1;
        while j < c.len() && c[j].is_whitespace() {
            j += 1;
        }
        if c.get(j) != Some(&'"') {
            i += 1;
            continue;
        }
        j += 1;
        let start = j;
        while j < c.len() && (c[j].is_ascii_lowercase() || c[j] == '-') {
            j += 1;
        }
        if j == start || c.get(j) != Some(&'"') {
            i += 1;
            continue;
        }
        return Some(c[start..j].iter().collect());
    }
    None
}

/// `BootKind::\w+ => "([a-z-]+)"`, every occurrence.
fn boot_kind_arms(src: &str) -> Vec<String> {
    let c: Vec<char> = src.chars().collect();
    let needle: Vec<char> = "BootKind::".chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + needle.len() <= c.len() {
        if c[i..i + needle.len()] != needle[..] {
            i += 1;
            continue;
        }
        let mut j = i + needle.len();
        let w = j;
        while j < c.len() && common::is_word(c[j]) {
            j += 1;
        }
        if j == w {
            i += 1;
            continue;
        }
        let arrow: Vec<char> = " => \"".chars().collect();
        if j + arrow.len() > c.len() || c[j..j + arrow.len()] != arrow[..] {
            i += 1;
            continue;
        }
        j += arrow.len();
        let start = j;
        while j < c.len() && (c[j].is_ascii_lowercase() || c[j] == '-') {
            j += 1;
        }
        if j == start || c.get(j) != Some(&'"') {
            i += 1;
            continue;
        }
        out.push(c[start..j].iter().collect());
        i = j + 1;
    }
    out
}

/// `^\s*case "([a-z-]+)":` — one per line; `^\s*` always succeeds at a line start.
fn boot_notice_cases(src: &str) -> Vec<String> {
    // THE CRATE'S SHAPE: `match kind.as_string().as_deref() { Some("first-run") => … }`. The scan
    // looks for `Some("…")` followed by `=>`, so a string that is merely mentioned in a comment or a
    // comparison elsewhere is not counted as a case.
    let mut out = Vec::new();
    for line in src.lines() {
        let t = line.trim_start();
        let Some(rest) = t.strip_prefix("Some(\"") else {
            continue;
        };
        let end = rest
            .char_indices()
            .take_while(|(_, c)| c.is_ascii_lowercase() || *c == '-')
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        // `")` IS TWO CHARACTERS, and the first version advanced by one — so the `=>` test looked at
        // the closing quote and the scan found ZERO kinds on a file that spells five.
        if end > 0
            && rest[end..].starts_with("\")")
            && rest[end + 2..].trim_start().starts_with("=>")
        {
            out.push(rest[..end].to_string());
        }
    }
    out
}

/// `^\s+([a-z]+):\s*"(muted|warn|bg|ok|fail|running)",` — the derivation's end-state table.
///
/// `^\s+` requires whitespace at a line start, but `\s` includes a newline, so ANY line that is not
/// the first can be reached from the preceding line's start. The per-line rule is therefore
/// "has leading whitespace, OR is not the first line" — and the entries in this table are indented
/// anyway, which is why the JS's own comment records that "the first version of this scan asked for
/// exactly two spaces and found nothing".
fn end_state_table(src: &str) -> Vec<String> {
    const STATES: [&str; 6] = ["muted", "warn", "bg", "ok", "fail", "running"];
    let mut out = Vec::new();
    for (idx, line) in src.lines().enumerate() {
        let has_ws = line.starts_with([' ', '\t', '\n', '\r', '\x0c', '\x0b']);
        if idx > 0 && !has_ws {
            continue;
        }
        let t = line.trim_start();
        let key_end = t
            .char_indices()
            .take_while(|(_, c)| c.is_ascii_lowercase())
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        if key_end == 0 {
            continue;
        }
        let rest = &t[key_end..];
        let Some(rest) = rest.strip_prefix(':') else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('"') else {
            continue;
        };
        for s in STATES {
            if let Some(after) = rest.strip_prefix(s) {
                if after.starts_with("\",") {
                    out.push(t[..key_end].to_string());
                    break;
                }
            }
        }
    }
    out
}

/// THE SAME READ, OVER THE RUST TABLE — because the table moved there (2026-09-29, P2).
///
/// `end_state_table` above reads the TypeScript `Record<EndReason, PathState>`; the derivation it
/// belonged to is `panel-logic/src/path.rs` now, whose table is `const END_STATE: [(&str, &str); 6]`.
/// **THE GATE FOLLOWS ITS SUBJECT, WHICH IS THE THIRD TIME IT HAS DONE SO** (the other two: the
/// `laneClass` families in the console, and this table's own move), and the reason is the one the
/// header already states: a contract whose subject moved is not a contract that was broken, and a
/// reader left on the old home would scan nothing and report a clean run.
fn end_state_table_rust(src: &str) -> Vec<String> {
    const STATES: [&str; 6] = ["muted", "warn", "bg", "ok", "fail", "running"];
    let mut out = Vec::new();
    for line in src.lines() {
        let t = line.trim();
        // `("interrupted", "warn"),`
        let Some(rest) = t.strip_prefix("(\"") else {
            continue;
        };
        let Some((key, rest)) = rest.split_once("\",") else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(value) = rest.strip_prefix("\"") else {
            continue;
        };
        for s in STATES {
            if value.starts_with(s) {
                out.push(key.to_string());
                break;
            }
        }
    }
    out
}

/// The crate's own `END_REASONS` list, read the same way — the copy the derivation now keys its
/// table on. It is the list `agent/src/vocabulary.rs` generates into `contract.gen.ts`, and this
/// assertion is what makes the crate's copy a checked one rather than a fourth unverified spelling.
fn crate_end_reasons(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in src.lines() {
        let t = line.trim();
        if t.starts_with("pub const END_REASONS") {
            inside = true;
            continue;
        }
        if inside {
            if t.starts_with(']') {
                break;
            }
            let v = t.trim_end_matches(',').trim();
            if let Some(inner) = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                out.push(inner.to_string());
            }
        }
    }
    out
}

/// The crate's `BOOT_KINDS`, read the same way as `crate_end_reasons`.
fn crate_boot_kinds(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in src.lines() {
        let t = line.trim();
        if t.starts_with("pub const BOOT_KINDS") {
            inside = true;
            continue;
        }
        if inside {
            if t.starts_with(']') {
                break;
            }
            let v = t.trim_end_matches(',').trim();
            if let Some(inner) = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                out.push(inner.to_string());
            }
        }
    }
    out
}

fn check() -> Result<String, String> {
    let json_raw = read(JSON);
    // The artifact carries `//` header lines, which JSON does not allow — the JS strips them by
    // LINE PREFIX, so only a line whose very first character is `/` is dropped.
    let json = json_raw
        .split('\n')
        .filter(|l| !l.starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let contract: serde_json::Value =
        serde_json::from_str(&json).unwrap_or_else(|e| panic!("cannot parse {JSON}: {e}"));
    let ts_gen = read(TS_GEN);

    let mut o = Out::new();

    // ── the artifacts ──────────────────────────────────────────────────────────────────────────
    for (key, ts_name) in ARRAYS {
        let want = contract[key]
            .as_array()
            .unwrap_or_else(|| panic!("{JSON} has no array {key}"))
            .iter()
            .map(|v| v.as_str().expect("a string").to_string())
            .collect::<Vec<_>>();
        let Some(from_ts) = ts_array(&ts_gen, ts_name) else {
            o.bad(format!(
                "contract.gen.ts has no {ts_name} — regenerate with SUMMRISE_REFRESH_CONTRACT=1 cargo test contract_vocabulary_snapshot"
            ));
            continue;
        };
        if json_compact(&from_ts) != json_compact(&want) {
            o.bad(format!(
                "contract.gen.ts {ts_name} and contract-vocabulary.json disagree: {} vs {}",
                json_compact(&from_ts),
                json_compact(&want)
            ));
        } else {
            o.ok(format!(
                "{ts_name}: both artifacts carry the same {} value(s)",
                want.len()
            ));
        }
    }
    // `EXITED_PREFIX` WAS ALSO AN `export const` ONCE, and it left with `END_REASONS`. The JSON still
    // names it, so the check that keeps it honest now reads the file the derivation reads.
    let prefix = contract["exited_prefix"].as_str().expect("a string");
    let crate_vocab_early = read(CRATE_VOCAB);
    if !crate_vocab_early.contains(&format!(
        "EXITED_PREFIX: &str = {}",
        serde_json::to_string(prefix).expect("a string serialises")
    )) {
        o.bad(format!(
            "panel-logic/src/vocabulary.rs EXITED_PREFIX is not {}",
            serde_json::to_string(prefix).expect("a string serialises")
        ));
    }

    // ── every frame literal in the Rust sources is listed ──────────────────────────────────────
    let rust_paths = files_under("agent/src", &|n| n.ends_with(".rs"));
    let frames: BTreeSet<String> = contract["frames"]
        .as_array()
        .expect("an array")
        .iter()
        .map(|v| v.as_str().expect("a string").to_string())
        .collect();
    let mut saw_frames = 0usize;
    for p in &rust_paths {
        let rel = rel_to_repo(p);
        let text = decomment(
            &std::fs::read_to_string(p)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display())),
        );
        for (i, line) in text.lines().enumerate() {
            let Some(frame) = frame_on_line(line) else {
                continue;
            };
            saw_frames += 1;
            if !frames.contains(&frame) {
                o.bad(format!(
                    "{rel}:{} pushes frame \"{frame}\", which contract-vocabulary.json does not list — add it to agent/src/vocabulary.rs",
                    i + 1
                ));
            }
        }
    }
    if saw_frames < 4 {
        o.bad(format!(
            "only {saw_frames} frame literal(s) found in agent/src — the scan has gone stale, so this proves nothing"
        ));
    } else {
        o.ok(format!(
            "{saw_frames} frame literal(s) in the agent, every one listed ({} known)",
            frames.len()
        ));
    }

    // ── the boot kinds, both directions ────────────────────────────────────────────────────────
    let runstate = read(RUNSTATE);
    let enum_kinds = boot_kind_arms(&runstate);
    let listed: Vec<String> = contract["boot_kinds"]
        .as_array()
        .expect("an array")
        .iter()
        .map(|v| v.as_str().expect("a string").to_string())
        .collect();
    if enum_kinds.len() < 5 {
        o.bad(format!(
            "read {} boot kind(s) out of runstate.rs — expected at least 5, so this proves nothing",
            enum_kinds.len()
        ));
    } else {
        for k in &enum_kinds {
            if !listed.contains(k) {
                o.bad(format!(
                    "runstate.rs spells boot kind \"{k}\" and the contract does not list it"
                ));
            }
        }
        for k in &listed {
            if !enum_kinds.contains(k) {
                o.bad(format!(
                    "the contract lists boot kind \"{k}\" and runstate.rs never spells it"
                ));
            }
        }
        o.ok(format!(
            "{} boot kind(s) agree in both directions",
            enum_kinds.len()
        ));
    }

    // ── the panel's readers only compare against listed values ─────────────────────────────────
    let boot_notice = read(BOOT_NOTICE);
    let kinds: BTreeSet<&String> = listed.iter().collect();
    // A SWITCH, LIKE THE DERIVATION — and the first version of this scan looked for `kind === "…"`,
    // found NOTHING, and passed silently: renaming a case to a kind the device never writes did not
    // bite. The floor below is the fix, and it is the same lesson the derivation's scan taught one
    // screen up: a scan that reads nothing must say so, or it certifies an empty set.
    let cases = boot_notice_cases(&boot_notice);
    for k in &cases {
        if !kinds.contains(k) {
            o.bad(format!(
                "boot_notice.rs spells kind \"{k}\", which the device never writes"
            ));
        }
    }
    if cases.len() < 2 {
        o.bad(format!(
            "read {} boot kind(s) out of boot_notice.rs — expected at least 2, so this proves nothing",
            cases.len()
        ));
    } else {
        o.ok(format!(
            "{} boot kind(s) in boot_notice.rs, all named by contract-vocabulary.json",
            cases.len()
        ));
    }

    // THE DERIVATION'S TABLE, AND IT IS RUST NOW (2026-09-29, P2): `stateFromEnd` and its `END_STATE`
    // table moved into `panel-logic/src/path.rs` when `lib/path.ts`'s five functions did, so this reads
    // THAT file. The vocabulary the table is keyed by moved with it — `panel-logic/src/vocabulary.rs`
    // now carries `END_REASONS` and `EXITED_PREFIX` — and the next check is what pins that copy.
    let path_rs = read(PATH_RS);
    let reasons: BTreeSet<String> = contract["end_reasons"]
        .as_array()
        .expect("an array")
        .iter()
        .map(|v| v.as_str().expect("a string").to_string())
        .collect();
    let table = end_state_table_rust(&path_rs);
    if table.is_empty() {
        o.bad("path.rs has no end-state table — the derivation moved and this scan proves nothing");
    } else {
        let named: BTreeSet<&String> = table.iter().collect();
        for r in &reasons {
            if !named.contains(r) {
                o.bad(format!(
                    "path.rs's table does not name the end reason \"{r}\" the device writes"
                ));
            }
        }
        for r in &named {
            if !reasons.contains(*r) {
                o.bad(format!(
                    "path.rs's table names \"{r}\", which contract-vocabulary.json does not list"
                ));
            }
        }
        if !path_rs.contains("EXITED_PREFIX") {
            o.bad("path.rs no longer reads the generated EXITED_PREFIX");
        }
        o.ok(format!(
            "{} end reason(s) in the derivation's table, every one in the vocabulary (and the type is generated)",
            table.len()
        ));
    }

    // ── AND THE WASM CRATE'S COPY OF THE VOCABULARY, WHICH NOBODY WAS CHECKING ──────────────────
    // `panel-logic/src/vocabulary.rs` has carried a hand-written `BOOT_KINDS` since `boot.rs` moved,
    // and its own header PROMISED that "the drift is caught by a gate" — while the gate it meant read
    // `agent/src/vocabulary.rs` and the two generated artifacts, never the crate. **A promise about a
    // check that does not exist is a claim nobody checked**, so this is the check: both lists the
    // crate carries are compared against the source of truth, and `END_REASONS` (moved there with
    // `path.rs`) is in the set.
    let crate_vocab = read(CRATE_VOCAB);
    for (label, got, want) in [
        (
            "END_REASONS",
            crate_end_reasons(&crate_vocab),
            contract["end_reasons"]
                .as_array()
                .expect("an array")
                .iter()
                .map(|v| v.as_str().expect("a string").to_string())
                .collect::<Vec<_>>(),
        ),
        (
            "BOOT_KINDS",
            crate_boot_kinds(&crate_vocab),
            contract["boot_kinds"]
                .as_array()
                .expect("an array")
                .iter()
                .map(|v| v.as_str().expect("a string").to_string())
                .collect::<Vec<_>>(),
        ),
    ] {
        if got.is_empty() {
            o.bad(format!(
                "panel-logic/src/vocabulary.rs has no {label} list — the crate's copy is unchecked now"
            ));
        } else if got != want {
            o.bad(format!(
                "panel-logic/src/vocabulary.rs {label} is {got:?} and contract-vocabulary.json says \
                 {want:?} — the crate is a hand-written copy, so this is the check that keeps it one"
            ));
        } else {
            o.ok(format!(
                "{label}: the crate's copy is the vocabulary's, {} value(s)",
                got.len()
            ));
        }
    }
    if !crate_vocab.contains("EXITED_PREFIX: &str = \"exited:\"") {
        o.bad("panel-logic/src/vocabulary.rs no longer carries the EXITED_PREFIX the derivation reads");
    } else {
        o.ok("EXITED_PREFIX: the crate's copy is the vocabulary's prefix");
    }

    // ── AND EVERY DECLARED VALUE IS ONE THE DEVICE ACTUALLY WRITES (round 156) ─────────────────
    // THE FILE LIST COMES FROM GIT, the way `production-host-check` does it: a hand-rolled walk
    // recursed until the stack blew (a symlink, most likely), and `git ls-files` is authoritative,
    // ordered and cannot loop.
    let rust_sources = git_ls_files("agent/src")
        .into_iter()
        .filter(|f| f.ends_with(".rs"))
        .map(|f| read(&f))
        .collect::<Vec<_>>()
        .join("\n");
    let (never_written, never_read) =
        declared_but_unspoken(&contract, &rust_sources, &interface_sources());

    if !never_written.is_empty() {
        o.bad_many(
            never_written.len(),
            format!(
                "{} declared value(s) the device never writes — a state that silently never renders:\n  {}\n\nEither the device emits it somewhere in agent/src, or it is a name only the interfaces know.",
                never_written.len(),
                never_written.join("\n  ")
            ),
        );
    }
    if !never_read.is_empty() {
        o.bad_many(
            never_read.len(),
            format!(
                "{} declared value(s) no interface reads — vocabulary the device speaks to nobody:\n  {}\n\nEither an interface switches on it, or the device has no reason to emit it.",
                never_read.len(),
                never_read.join("\n  ")
            ),
        );
    }

    let mut out = o.lines.join("\n");
    if o.fail > 0 {
        // The JS's final line is `console.error("\ncontract-vocabulary: …")`, and each `ok` line
        // already ended with a newline — so the stream carries a BLANK LINE before it.
        out.push_str(&format!(
            "\n\ncontract-vocabulary: {} problem(s) — the two ends do not spell one vocabulary",
            o.fail
        ));
        return Err(out);
    }
    out.push_str("\n\ncontract-vocabulary: the device and the interfaces spell one vocabulary");
    Ok(out)
}

/// The interface trees' sources, concatenated — what must READ every declared value.
fn interface_sources() -> String {
    INTERFACE_DIRS
        .iter()
        .flat_map(|d| git_ls_files(d))
        .filter(|f| f.ends_with(".ts") || f.ends_with(".tsx"))
        .map(|f| read(&f))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `JSON.stringify(v)` for a string is the quoted form, which is what the JS `includes` searches for.
fn quoted(v: &str) -> String {
    serde_json::to_string(v).expect("a string serialises")
}

/// The two symmetric halves, in the artifact's own key order (serde_json's `Map` is ordered, and
/// this artifact's keys are already alphabetical, so it is also the file's order).
fn declared_but_unspoken(
    contract: &serde_json::Value,
    rust_sources: &str,
    interface_sources: &str,
) -> (Vec<String>, Vec<String>) {
    let mut never_written = Vec::new();
    let mut never_read = Vec::new();
    let obj = contract.as_object().expect("the contract is an object");
    for (group, values) in obj {
        let Some(arr) = values.as_array() else {
            continue;
        };
        for v in arr {
            let s = v.as_str().expect("a string");
            if !rust_sources.contains(&quoted(s)) {
                never_written.push(format!("{group}: {s}"));
            }
            if !interface_sources.contains(&quoted(s)) {
                never_read.push(format!("{group}: {s}"));
            }
        }
    }
    (never_written, never_read)
}

#[test]
fn the_device_and_the_interfaces_spell_one_vocabulary() {
    let out = check().unwrap_or_else(|e| panic!("{e}"));
    for line in out.lines() {
        println!("{line}");
    }
    assert!(
        out.contains("contract-vocabulary: the device and the interfaces spell one vocabulary"),
        "{out}"
    );
}

// ── the scanner's own proof: a parser that reads nothing must not pass ─────────────────────────

#[test]
fn a_frame_literal_is_read_and_a_comment_is_not() {
    assert_eq!(
        frame_on_line(r#"  ("ev": "monitor-change"),"#).as_deref(),
        Some("monitor-change")
    );
    assert_eq!(
        frame_on_line(r#"  {"ev": "term-output"}"#).as_deref(),
        Some("term-output")
    );
    // The separator is a COLON — the shape the JS regex requires, and the only shape the Rust
    // sources use. A comma-separated tuple is not a frame literal.
    assert_eq!(
        frame_on_line(r#"("ev", "monitor-change")"#).as_deref(),
        None
    );
    // A comment naming a frame is not a frame — `decomment` is what removes it.
    assert!(frame_on_line("// \"ev\": \"comment-only-frame\"").is_some());
    assert_eq!(
        frame_on_line(&decomment("// \"ev\": \"comment-only-frame\"")).as_deref(),
        None
    );
    // An uppercase or underscored frame is not `[a-z-]+`.
    assert_eq!(frame_on_line(r#"("ev": "NotAFrame")"#).as_deref(), None);
}

#[test]
fn the_boot_kind_arms_and_the_notice_cases_have_their_own_shapes() {
    let src = "BootKind::FirstRun => \"first-run\",\nBootKind::Crashed => \"crashed\",";
    assert_eq!(
        boot_kind_arms(src),
        vec!["first-run".to_string(), "crashed".to_string()]
    );
    assert!(boot_kind_arms("BootKind::FirstRun => FirstRun").is_empty());
    // THE CRATE'S SHAPE: `Some("…") =>`, which is what the boot notice spells its arms with now.
    let notice = "        Some(\"crashed\") => {\n        Some(\"clean-exit\") => \"x\",\n";
    assert_eq!(
        boot_notice_cases(notice),
        vec!["crashed".to_string(), "clean-exit".to_string()]
    );
    // A comparison is not a case, and a kind mentioned in prose is not one either.
    assert!(boot_notice_cases("kind.as_string().as_deref() == Some(\"crashed\")").is_empty());
    assert!(boot_notice_cases("// Some(\"crashed\") => was the old shape").is_empty());
}

#[test]
fn the_end_state_table_is_read_from_its_indented_entries() {
    let src = "const END_STATE: Record<EndReason, PathState> = {\n  marker: \"ok\",\n  idle: \"muted\",\n  timeout: \"warn\",\n};\n";
    assert_eq!(
        end_state_table(src),
        vec![
            "marker".to_string(),
            "idle".to_string(),
            "timeout".to_string()
        ]
    );
    // A label table has the same keys and different values, so a STATE value is what tells them apart.
    assert!(end_state_table("  marker: \"Marker\",\n").is_empty());
    // The floor: an empty table must be reported, not passed.
    assert!(end_state_table("").is_empty());
}

#[test]
fn a_ts_array_is_parsed_the_way_json_parse_parses_it() {
    let gen = "export const FRAMES = [\"a-b\",\"c-d\"] as const;\n";
    assert_eq!(
        ts_array(gen, "FRAMES"),
        Some(vec!["a-b".to_string(), "c-d".to_string()])
    );
    assert_eq!(ts_array(gen, "BOOT_KINDS"), None);
    // The ` as const;` suffix is required, or this matched some other array.
    assert_eq!(ts_array("export const FRAMES = [\"a\"];\n", "FRAMES"), None);
}
