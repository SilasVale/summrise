//! EVERY FIELD ONE END READS MUST BE A FIELD THE OTHER END SPEAKS — THE SAME RULE AT THREE LAYERS.
//!
//! WHY THIS EXISTS (rounds 82, 100, 101 of the standing goal). The class of defect this session kept
//! paying for is a field name that only one end knows:
//!
//!   * `prov-dot` rendered nothing because the Devices page looked a device's health up by `h.id`
//!     while the fixture wrote `prefix:`;
//!   * the console's crash row never appeared because the fixture said `verdict:` where the page
//!     reads `last_boot_kind` (so `sig-dot.err` had no surface in 136 sweeps);
//!   * the panel's `boot-mark info` took seven rounds, one of whose suspects was exactly this;
//!   * and the gateway forwards a device's answer FIELD BY FIELD, BY HAND — no generated contract
//!     reaches across that boundary — so a name the device stopped sending becomes `undefined` on
//!     the console with every suite on both sides still green.
//!
//! Every time, one end was reading a field, the other was carrying a different one, and nothing
//! compared the two. This is that comparison, at the three places the two languages meet.
//!
//! THE THREE LAYERS, each with its own producer corpus — and each with its own floor, because a scan
//! that read nothing must not pass:
//!
//!   panel    `useAgentVitals`/`useCommandEvents`/`usePlugins`/`useSessions`/`evicted`/`useMonitors`
//!            → the sweep's stubbed device AND the shared fixtures AND the agent's Rust or the gateway
//!   console  every non-test `.tsx?` under `gateway/ui/src`
//!            → the gateway's own sources (`gateway/src/**`), and NOT the render smokes or the sweep
//!              table, which are fixtures: a field only they carry renders in a test and answers
//!              `undefined` against the deployed worker
//!   gateway  `plugins/mcp.ts`, `device-fetch.ts`, `plugins/devices.ts`
//!            → the agent's Rust or the shared fixtures
//!
//! WHAT IT DOES NOT CHECK: types, values, which endpoint a field arrives on (that is the harness's own
//! business, and round 73 records what it cost), or whether a name is spelled the same way twice.
//! Only that the NAME exists where the other end would have to spell it.
//!
//! ONE SCANNER, NOT THREE (the point of moving these three together). The JS versions were three
//! copies of the same `decomment` helper — a fourth copy of the rule that helper exists to state,
//! written by the gates that enforce it — and three separate regexes whose receiver widths had already
//! drifted (`{1,8}` for the panel and the gateway, `{1,10}` for the console). The widths are now an
//! ARGUMENT rather than three literals, so the drift is visible at the call site, and the comment
//! strip has one definition.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28):
//!   * `node scripts/test/wire-field-check.mjs` → "wire-field: 30 field(s) read by 6 panel parser(s) —
//!     every one spoken by the harness or a fixture AND by a producer (the agent's Rust or the
//!     gateway), so none of them renders only in a stub"
//!   * `node scripts/test/console-wire-field-check.mjs` → "console-wire-field: 10 field(s) read
//!     across 26 console module(s) — every one spoken by the gateway ITSELF"
//!   * `node scripts/test/gateway-device-field-check.mjs` → "gateway-device-field: 7 device field(s)
//!     read across 3 gateway module(s), every one spelled by the agent or a fixture"
//!   * this file → the SAME three sentences with the SAME three counts, character for character.
//!   * a planted `const x = probe.no_such_field_anywhere;` in the gateway's `device-fetch.ts` → the
//!     JS gate exits 1 naming `gateway/src/device-fetch.ts: reads "no_such_field_anywhere", which the
//!     agent's sources and fixtures never spell`, and this file fails with the identical sentence.
//!   * a field named ONLY in a comment (`// only_a_comment_field`) → BOTH STAY GREEN on the field and
//!     the JS floor is what fails, which is the honest measurement: `decomment` is a FALSE-NEGATIVE
//!     guard (a comment must not satisfy a producer), so removing the producer and leaving the prose
//!     is exactly the case it was written for — and it is reproduced in the scanner's own proof below.
//!
//! MUTATION: read a snake_case field from a device answer that no producer spells — add
//!           `const x = probe.no_such_field_anywhere;` to `gateway/src/device-fetch.ts`.
//! RESULT:   fails, printing the file, the field name, and the sentence that says which corpus is
//!           missing it — "the agent's sources and fixtures never spell" for the gateway layer.

mod common;

use common::{
    decomment, decomment_line, decommented_corpus, files_under, is_word, read, rel_to_repo, repo,
};

use std::collections::BTreeSet;
use std::fs;

fn is_lower_or_digit(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}

/// `[a-z][a-z0-9]*(?:_[a-z0-9]+)+` over `s[start..end]`.
fn is_field_name(s: &[char], start: usize, end: usize) -> bool {
    if end <= start + 1 || !s[start].is_ascii_lowercase() {
        return false;
    }
    let body = &s[start..end];
    if !body.iter().all(|&c| is_lower_or_digit(c) || c == '_') {
        return false;
    }
    if !is_lower_or_digit(body[body.len() - 1]) {
        return false;
    }
    // At least one `_`, and no two in a row (a `_` must open a non-empty [a-z0-9]+ group).
    let mut groups = 0;
    let mut k = 0;
    while k < body.len() {
        if body[k] == '_' {
            if k + 1 >= body.len() || !is_lower_or_digit(body[k + 1]) {
                return false;
            }
            groups += 1;
        }
        k += 1;
    }
    groups > 0
}

/// `\b\w{1,max}\??\.([a-z][a-z0-9]*(?:_[a-z0-9]+)+)\b`, in order, de-duplicated.
///
/// `max` is the receiver width, and it is an ARGUMENT because the three JS gates had already
/// drifted: the panel's and the gateway's copies read `{1,8}`, the console's read `{1,10}`.
fn snake_reads(text: &str, max: usize) -> Vec<String> {
    let s: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut i = 0;
    while i < s.len() {
        // `\b`, and `\w{1,max}` must start on a word character.
        if !is_word(s[i]) || (i > 0 && is_word(s[i - 1])) {
            i += 1;
            continue;
        }
        let mut run_end = i;
        while run_end < s.len() && is_word(s[run_end]) && run_end - i < max {
            run_end += 1;
        }
        let mut hit: Option<(String, usize)> = None;
        // `\w{1,max}` is greedy: longest receiver first, then shorter.
        for len in (1..=run_end - i).rev() {
            let j = i + len;
            // `\??` is greedy: with the `?` first, then without.
            for skip_q in [true, false] {
                let mut k = j;
                if skip_q {
                    if s.get(k) != Some(&'?') {
                        continue;
                    }
                    k += 1;
                }
                if s.get(k) != Some(&'.') {
                    continue;
                }
                k += 1;
                // The capture, greedy, with `\b` after it — so the longest valid end whose next
                // character is not a word character (or which ends the text).
                if !s.get(k).is_some_and(|c| c.is_ascii_lowercase()) {
                    continue;
                }
                let mut max_end = k + 1;
                while max_end < s.len() && (is_lower_or_digit(s[max_end]) || s[max_end] == '_') {
                    max_end += 1;
                }
                for e in (k + 2..=max_end).rev() {
                    if !is_field_name(&s, k, e) {
                        continue;
                    }
                    if e < s.len() && is_word(s[e]) {
                        continue;
                    }
                    // A FIELD IS READ, WHILE A WASM EXPORT IS CALLED. The panel's migrated
                    // parsers are `await panelLogic()` and then `logic.parse_monitors(j)` — a
                    // CALL on the glue, whose export names are snake_case BECAUSE RUST IS. This
                    // scanner matched `parse_monitors` and reported it as a field the panel reads
                    // that no fixture carries, which is true and irrelevant: A DEVICE SENDS DATA,
                    // NEVER A FUNCTION. The JS gate carried this rule as `(?!\s*\()`; the port
                    // did not, and deleting the .mjs deleted the rule with it — which is why this
                    // is a rule of the SCANNER and not an exemption in a list.
                    let mut after = e;
                    while after < s.len() && s[after].is_whitespace() {
                        after += 1;
                    }
                    if s.get(after) == Some(&'(') {
                        continue;
                    }
                    hit = Some((s[k..e].iter().collect(), e));
                    break;
                }
                if hit.is_some() {
                    break;
                }
            }
            if hit.is_some() {
                break;
            }
        }
        match hit {
            Some((field, end)) => {
                if seen.insert(field.clone()) {
                    out.push(field);
                }
                i = end;
            }
            None => i += 1,
        }
    }
    out
}

/// `\bfield\b` — the field name is snake_case, so it holds no metacharacter.
fn spoken(corpus: &str, field: &str) -> bool {
    let c: Vec<char> = corpus.chars().collect();
    let f: Vec<char> = field.chars().collect();
    if f.is_empty() || f.len() > c.len() {
        return false;
    }
    for start in 0..=c.len() - f.len() {
        if c[start..start + f.len()] != f[..] {
            continue;
        }
        let before_ok = start == 0 || !is_word(c[start - 1]);
        let after = start + f.len();
        let after_ok = after == c.len() || !is_word(c[after]);
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

fn fixtures_corpus() -> String {
    let dir = repo().join("agent/tests/fixtures");
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|e| {
            e.expect("a readable entry")
                .file_name()
                .to_string_lossy()
                .to_string()
        })
        .filter(|n| n.ends_with(".json"))
        .collect();
    names.sort();
    names
        .iter()
        .map(|n| read(&format!("agent/tests/fixtures/{n}")))
        .collect::<Vec<_>>()
        .join("\n")
}

// ── LAYER 1: the panel's wire-parsing hooks ─────────────────────────────────────────────────────

/// The hooks that parse a device answer. Named, so a new one is added deliberately rather than swept
/// in.
const PANEL_PARSERS: [&str; 6] = [
    "agent/resources/panel-react/src/hooks/useAgentVitals.ts",
    "agent/resources/panel-react/src/hooks/useCommandEvents.ts",
    "agent/resources/panel-react/src/hooks/usePlugins.ts",
    "agent/resources/panel-react/src/hooks/useSessions.ts",
    // THE ONE THE SESSION-ROW GATE HANDS OVER (round 228). That gate reads useSessions.ts and its own
    // comment says the panel's other modules are "the wire-field gate's business" — while this list
    // did not include evicted.ts, which reads r.idle_ms with its own coercion. A disclaimer that names
    // a gate is only honest if that gate is actually looking.
    "agent/resources/panel-react/src/lib/evicted.ts",
    "agent/resources/panel-react/src/hooks/useMonitors.ts",
];

/// Field reads that are NOT device fields, declared with the reason.
const NOT_DEVICE_FIELDS: [&str; 2] = ["session_id", "ev"];

/// THE PARSERS THAT HAVE MOVED, WHICH THIS GATE COULD NOT SEE — and the way it went blind is the
/// reason this list is here rather than a note. `archive.rs`, `boot.rs`, `monitors.rs` and `runs.rs`
/// each moved a family's parse out of TypeScript and into this crate, and each one took its fields
/// out of a scan that only ever looked at the six files above. NOTHING SAID SO: the floor below was
/// 20, the TypeScript that remained read more than that, and the gate went on printing "every one
/// spoken by the harness" while covering a smaller and smaller share of the panel. **A RULE THAT
/// ONLY HOLDS FOR THE FILES THAT HAVE NOT MOVED YET IS A RULE THAT EXPIRES** — and it expired
/// silently, one verified migration at a time.
///
/// THE SIXTH FAMILY IS WHAT MADE IT SAY SO: `lib/evicted.ts` and `useVitalsSeries.ts` lost their
/// parses, the count fell to 19, and the floor refused with "the parsers moved, so this proves
/// nothing" — which is exactly what had happened, four families earlier than the gate noticed.
///
/// **AND A FAMILY MOVE NOW ADDS ITS FILE HERE IN THE SAME COMMIT**, which is the rule that keeps this
/// from happening again: the parse and its scan move together, or the next move re-opens the same
/// hole one family smaller. `agent_vitals.rs` was added this way on 2026-09-29, in the commit that
/// created it.
const PANEL_LOGIC: [&str; 7] = [
    "agent/resources/panel-logic/src/agent_vitals.rs",
    "agent/resources/panel-logic/src/archive.rs",
    "agent/resources/panel-logic/src/boot.rs",
    "agent/resources/panel-logic/src/evicted.rs",
    "agent/resources/panel-logic/src/monitors.rs",
    "agent/resources/panel-logic/src/runs.rs",
    "agent/resources/panel-logic/src/vitals.rs",
];

/// THE RUST PARSERS READ A WIRE KEY AS A STRING, not as a property, so `snake_reads` cannot see it:
/// a moved family calls `prop(&raw, "idle_ms")`. This is the same invariant in the shape the port
/// gave it — and THE FIELD RULE IS THE SAME ONE (`is_field_name`: at least one `_`, lowercase and
/// digits only), so `prop(&detail, "ev")` and `prop(&j, "targets")` are not wire fields here either,
/// for the same reason they are not in the TypeScript.
fn prop_reads(text: &str) -> Vec<String> {
    let s: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut i = 0;
    while i < s.len() {
        // `prop(`, and then the FIRST string literal before the call closes — that literal is the key.
        if !(i + 5 <= s.len() && s[i..i + 5].iter().collect::<String>() == "prop(") {
            i += 1;
            continue;
        }
        let mut j = i + 5;
        while j < s.len() && s[j] != ')' && s[j] != '"' {
            j += 1;
        }
        if j >= s.len() || s[j] != '"' {
            i += 1;
            continue;
        }
        let start = j + 1;
        let mut k = start;
        while k < s.len() && s[k] != '"' {
            k += 1;
        }
        if k >= s.len() {
            break;
        }
        let name: String = s[start..k].iter().collect();
        let cs: Vec<char> = name.chars().collect();
        if is_field_name(&cs, 0, cs.len()) && seen.insert(name.clone()) {
            out.push(name);
        }
        i = k + 1;
    }
    out
}

/// THE HARNESS'S FIELDS NOW COME FROM TWO FILES (round 270). The emitter builds the page; the stubbed
/// device API it inlines — every fixture response the panel reads — is `lib/sweep/panel-stub.cjs`.
/// Reading only the emitter left this corpus empty and reported `first_seq` as a field the panel reads
/// and "neither the device harness nor any fixture carries", which the stub carries twice.
const HARNESS_SOURCES: [&str; 2] = [
    "agent/scripts/panel-render-audit.mjs",
    "agent/scripts/lib/sweep/panel-stub.cjs",
];

fn panel_check() -> Result<String, String> {
    let harness = HARNESS_SOURCES
        .iter()
        .map(|rel| read(rel))
        .collect::<Vec<_>>()
        .join("\n");
    if harness.len() < 20000 {
        return Err(format!(
            "wire-field: read only {} bytes of harness — the stub or the emitter moved, so this proves nothing",
            harness.len()
        ));
    }
    let other_end = format!("{harness}\n{}", fixtures_corpus());

    // THE PRODUCERS, which are what actually matters (round 108). `otherEnd` above is the TEST side —
    // a stub and fixtures — and a field that exists ONLY there is a field the device may not send at
    // all. A wire field has to be spelled by something that SENDS it: the agent's Rust or the
    // gateway's TypeScript.
    let mut producers_paths = files_under("agent/src", &|n| n.ends_with(".rs"));
    producers_paths.extend(files_under("gateway/src", &|n| n.ends_with(".ts")));
    let producers = decommented_corpus(&producers_paths);

    let mut reads = 0;
    let mut missing = Vec::new();
    // THE TYPESCRIPT THAT HAS NOT MOVED, by property access.
    for rel in PANEL_PARSERS {
        let text = decomment(&read(rel));
        for field in snake_reads(&text, 8) {
            if NOT_DEVICE_FIELDS.contains(&field.as_str()) {
                continue;
            }
            reads += 1;
            if !spoken(&other_end, &field) {
                missing.push(format!(
                    "{rel}: reads \"{field}\", which neither the device harness nor any fixture carries"
                ));
            } else if !spoken(&producers, &field) {
                missing.push(format!(
                    "{rel}: reads \"{field}\", which only a fixture carries — no Rust or gateway source sends it"
                ));
            }
        }
    }
    // AND THE RUST THAT HAS — the same two questions, asked of the parsers this gate could not see.
    for rel in PANEL_LOGIC {
        let text = decomment(&read(rel));
        for field in prop_reads(&text) {
            if NOT_DEVICE_FIELDS.contains(&field.as_str()) {
                continue;
            }
            reads += 1;
            if !spoken(&other_end, &field) {
                missing.push(format!(
                    "{rel}: reads \"{field}\", which neither the device harness nor any fixture carries"
                ));
            } else if !spoken(&producers, &field) {
                missing.push(format!(
                    "{rel}: reads \"{field}\", which only a fixture carries — no Rust or gateway source sends it"
                ));
            }
        }
    }

    // THE FLOOR IS 40, MEASURED RATHER THAN CARRIED (2026-09-29). It was 20, and 20 was one field
    // above what the SIX TYPESCRIPT FILES ALONE still read (19) — so the number that was supposed to
    // catch a collapse was sitting on the exact value a collapse produces, and it only said so
    // because the sixth family happened to take the count one below it. With the moved families in
    // the scan the count is **47**, so 40 is the same kind of margin this file's siblings use: below
    // anything a working tree produces, above anything a single-language scan can reach (19 from
    // TypeScript alone, 28 from the Rust alone).
    if reads < 40 {
        return Err(format!(
            "FAIL read only {reads} wire field(s) — the parsers moved, so this proves nothing"
        ));
    }
    if !missing.is_empty() {
        return Err(format!(
            "wire-field: {} field(s) the panel reads and no fixture speaks:\n  {}\n\nThree of these cost rounds in this session: a page reading `h.id` against a fixture saying `prefix:`, a\ncrash row waiting on `last_boot_kind` against a fixture saying `verdict:`, and `prov-dot` rendering nothing at\nall for three rounds. Add the field to the harness stub or a fixture, or declare it in NOT_DEVICE_FIELDS with a\nreason.",
            missing.len(),
            missing.join("\n  ")
        ));
    }
    Ok(format!(
        "wire-field: {reads} field(s) read by {} panel parser(s) — {} TypeScript file(s) that still parse and {} Rust \
         file(s) that have taken over — every one spoken by the harness or a fixture AND by a producer (the agent's \
         Rust or the gateway), so none of them renders only in a stub",
        PANEL_PARSERS.len() + PANEL_LOGIC.len(),
        PANEL_PARSERS.len(),
        PANEL_LOGIC.len()
    ))
}

// ── LAYER 2: the console's own readers ──────────────────────────────────────────────────────────

fn console_check() -> Result<String, String> {
    // WHAT THE GATEWAY ITSELF SENDS — the producer, and the only source that counts (round 122). The
    // first version also accepted the render smokes and the console sweep, which are FIXTURES: a field
    // only they carry renders in a test and answers `undefined` against the deployed worker, which is
    // the `prov-dot`/`verdict:` class this gate exists for.
    let producers = decommented_corpus(&files_under("gateway/src", &|n| n.ends_with(".ts")));

    // The console's readers, excluding its own tests (they assert, they do not parse the wire) and
    // its GENERATED wasm declarations.
    //
    // `.d.ts` IS EXCLUDED, AND IT IS NOT AN EXEMPTION — a declaration file has no expressions, so it
    // cannot read a wire field at all; the gate would be counting a file class that cannot contain the
    // thing it looks for. Found by this gate REFUSING: block ③ added `gateway/ui/src/wasm/ui_logic.d.ts`
    // (wasm-pack's own output, committed beside the glue), `n.ends_with(".ts")` matched it, and the
    // pinned module count went 26 -> 27 with the message claiming a module that reads nothing. The
    // count is pinned precisely so a change in the denominator is loud, and it was.
    let readers = files_under("gateway/ui/src", &|n| {
        (n.ends_with(".ts") || n.ends_with(".tsx"))
            && !n.contains(".test.")
            && !n.ends_with(".d.ts")
    });

    let mut reads = 0;
    let mut missing = Vec::new();
    for f in &readers {
        let rel = rel_to_repo(f);
        let text = decomment(
            &fs::read_to_string(f).unwrap_or_else(|e| panic!("cannot read {}: {e}", f.display())),
        );
        for field in snake_reads(&text, 10) {
            reads += 1;
            if !spoken(&producers, &field) {
                missing.push(format!(
                    "{rel}: reads \"{field}\", which neither the gateway nor any console fixture carries"
                ));
            }
        }
    }

    // A LOWER FLOOR THAN THE PANEL'S, and the reason is a fact about the console: it maps a device
    // answer to camelCase earlier (`deviceState.ts`), so far fewer snake_case reads reach its views.
    if reads < 5 {
        return Err(format!(
            "FAIL read only {reads} wire field(s) from the console — the tree moved, so this proves nothing"
        ));
    }
    if !missing.is_empty() {
        return Err(format!(
            "console-wire-field: {} field(s) the console reads and nothing produces:\n  {}\n\nTwo of this session's defects were exactly this — a page looking a device's health up by `h.id` against a\nfixture saying `prefix:`, and a crash row waiting on `last_boot_kind` against `verdict:`. Add the field to a\nproducer or a fixture, or take the read out.",
            missing.len(),
            missing.join("\n  ")
        ));
    }
    Ok(format!(
        "console-wire-field: {reads} field(s) read across {} console module(s) — every one spoken by the gateway ITSELF, \
         so none of them renders only in a smoke or a sweep fixture",
        readers.len()
    ))
}

// ── LAYER 3: the gateway forwarding a device answer ─────────────────────────────────────────────

/// THE MODULES WHOSE ANSWERS COME FROM A DEVICE, named because the alternative was measured and is
/// wrong (round 123). The obvious widening — walk every TypeScript file under the gateway's sources —
/// reported 35 fields, and reading them settled it: `tool_use_id`, `media_type`, `max_tokens`,
/// `prompt_tokens`, `cache_read_input_tokens` are the vocabulary of the UPSTREAM LLM PROVIDERS, not
/// of the agent. Three vocabularies share one field syntax, which is the same lesson
/// `one-derivation-check`'s first run taught about four vocabularies sharing a word: a broad pattern
/// does not become a rule by matching more.
const GATEWAY_CONSUMERS: [&str; 3] = [
    "gateway/src/plugins/mcp.ts",
    "gateway/src/device-fetch.ts",
    "gateway/src/plugins/devices.ts",
];

fn gateway_check() -> Result<String, String> {
    let mut device_side_paths = files_under("agent/src", &|n| n.ends_with(".rs"));
    device_side_paths.extend(files_under("agent/tests/fixtures", &|n| {
        n.ends_with(".json")
    }));
    let device_side = decommented_corpus(&device_side_paths);

    let mut reads = 0;
    let mut missing = Vec::new();
    for rel in GATEWAY_CONSUMERS {
        let text = decomment(&read(rel));
        for field in snake_reads(&text, 8) {
            reads += 1;
            if !spoken(&device_side, &field) {
                missing.push(format!(
                    "{rel}: reads \"{field}\" from a device answer, which the agent's sources and fixtures never spell"
                ));
            }
        }
    }

    // LOW, AND THE REASON IS THE SAME ONE THE CONSOLE'S GATE RECORDS: these files map a device answer
    // to camelCase early (`DeviceProbeState`), so only a handful of snake_case reads reach them.
    if reads < 4 {
        return Err(format!(
            "FAIL read only {reads} device field(s) in the gateway — the tree moved, so this proves nothing"
        ));
    }
    if !missing.is_empty() {
        return Err(format!(
            "gateway-device-field: {} forwarded field(s) the device does not spell:\n  {}\n\nThe gateway forwards a device's answer field by field, by hand — so a name the device stopped sending, or\nspells differently, becomes `undefined` on the console with every suite on both sides still green. That is the\nshape of the two defects the same rule caught one layer in (rounds 82 and 100).",
            missing.len(),
            missing.join("\n  ")
        ));
    }
    Ok(format!(
        "gateway-device-field: {reads} device field(s) read across {} gateway module(s), every one spelled by the agent \
         or a fixture",
        GATEWAY_CONSUMERS.len()
    ))
}

// ── the gates ──────────────────────────────────────────────────────────────────────────────────

#[test]
fn panel_wire_fields() {
    let msg = panel_check().unwrap_or_else(|e| panic!("{e}"));
    println!("{msg}");
    assert!(msg.contains("field(s) read by 13 panel parser(s)"), "{msg}");
}

#[test]
fn console_wire_fields() {
    let msg = console_check().unwrap_or_else(|e| panic!("{e}"));
    println!("{msg}");
    assert!(
        msg.contains("field(s) read across 26 console module(s)"),
        "{msg}"
    );
}

#[test]
fn gateway_device_fields() {
    let msg = gateway_check().unwrap_or_else(|e| panic!("{e}"));
    println!("{msg}");
    assert!(
        msg.contains("device field(s) read across 3 gateway module(s)"),
        "{msg}"
    );
}

// ── the scanner's own proof: a parser that reads nothing must not pass ─────────────────────────

#[test]
fn the_scanner_reads_a_field_off_any_receiver() {
    assert_eq!(
        snake_reads("const x = j.first_seq;", 8),
        vec!["first_seq".to_string()]
    );
    assert_eq!(
        snake_reads("const x = s?.last_exit_code;", 8),
        vec!["last_exit_code".to_string()]
    );
    // ANY receiver, because the hooks name their answers differently (`j`, `d`, `st`, `row`, `next`).
    assert_eq!(
        snake_reads("const x = row.idle_ms;", 8),
        vec!["idle_ms".to_string()]
    );
    // camelCase is the panel's own vocabulary, not a wire field.
    assert!(snake_reads("const x = row.lastExitCode;", 8).is_empty());
    // A trailing capital is not a `\b`: `last_exit_codeX` matches nothing.
    assert!(snake_reads("const x = j.last_exit_codeX;", 8).is_empty());
    // `a__b` is not `[a-z][a-z0-9]*(?:_[a-z0-9]+)+`.
    assert!(snake_reads("const x = j.a__b;", 8).is_empty());
    assert!(snake_reads("const x = j.a_;", 8).is_empty());
    // Two reads on one line, in order, de-duplicated.
    assert_eq!(
        snake_reads("f(a.b_c, a.b_c, a.d_e);", 8),
        vec!["b_c".to_string(), "d_e".to_string()]
    );
}

#[test]
fn the_receiver_width_is_an_argument_because_the_three_copies_had_drifted() {
    // The panel's and the gateway's JS copies read `{1,8}`; the console's read `{1,10}`. Nine
    // characters is the boundary that tells them apart, and it is the whole reason this is a
    // parameter rather than a literal in three places.
    let line = "const x = ninechars.some_field;";
    assert!(
        snake_reads(line, 8).is_empty(),
        "8 must not reach a 9-character receiver"
    );
    assert_eq!(snake_reads(line, 10), vec!["some_field".to_string()]);
}

#[test]
fn a_comment_is_not_a_producer() {
    // The measured case: a harness field plus a hook read plus `// only_a_comment_field …` in a Rust
    // source passed the JS gate with rc=0. A deleted producer can be kept alive by a comment, which
    // is the opposite of what these gates are for.
    let src = "fn main() {}\n// only_a_comment_field is gone\nlet a = 1;\n";
    assert!(!spoken(&decomment(src), "only_a_comment_field"));
    assert!(
        spoken(src, "only_a_comment_field"),
        "the raw text still carries it"
    );
    // A URL's `//` is preceded by a colon and must survive.
    assert_eq!(
        decomment_line("let u = \"https://x/y\";"),
        "let u = \"https://x/y\";"
    );
    // A trailing comment goes, and a whole-line one goes entirely.
    assert_eq!(decomment_line("let a = 1; // why"), "let a = 1; ");
    assert_eq!(decomment_line("   // why"), "");
    assert_eq!(decomment("/* block */ a"), " a");
}
