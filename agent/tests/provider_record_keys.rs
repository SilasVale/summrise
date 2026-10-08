//! THE PROVIDER RECORD, IN THE THREE PLACES IT IS WRITTEN — and the two seams TypeScript cannot check.
//!
//! ── WHAT B2 ASKED FOR, AND WHY IT IS A GATE ─────────────────────────────────────────────────────
//!
//! The catalogue plan's second landing step says: *the console's provider form and the record `ui-logic`
//! reads must be the same shape; today's form is already that shape, only the CHECK is missing.* A check
//! that is not written down is a check nobody runs, so this is the check.
//!
//! THE SHAPE IS WRITTEN THREE TIMES:
//!
//!     gateway/src/store/providers.ts   ProviderSpec + ProviderModel   what the admin API VALIDATES and stores
//!     gateway/ui/src/api/client.ts     ProviderDraft                  what the FORM submits
//!     gateway/wasm/src/store.rs        provider_*()                   what the WORKER reads back out
//!
//! **`tsc` CHECKS ONE OF THE TWO SEAMS AND CANNOT CHECK THE OTHER.** `gateway/ui` is its own package with
//! its own tsconfig, so `ProviderDraft` cannot import `ProviderSpec` — the file says *"Mirrors the server's
//! `parseProviderSpec`"* and that sentence is the entire link between them. And nothing on the TypeScript
//! side knows the Rust reader exists at all. **A RENAME ON EITHER SIDE IS SILENT**: `apiKeyEnv` → `keyEnv`
//! in the worker leaves every provider answering "no provider key" while both TypeScript projects compile.
//!
//! **AND THE RECORD HAS TWO SPELLINGS, WHICH THE FIRST RUN OF THIS GATE IS WHAT FOUND.** The form sends
//! `input: ["text", "image"]` and `parseProviderModel` validates it against `MODEL_FIELDS`, while the STORED
//! record — the one the Rust reads — declares `vision: boolean` instead. `input` is the WIRE spelling;
//! `vision` is what is stored, DERIVED from it. So "the record" is not one list of keys, and a gate that
//! compared two of these four places directly would report a rename where there is a conversion.
//!
//! SO THE TWO ASSERTIONS ARE THE TWO SEAMS, EACH AGAINST THE LIST THAT SIDE ACTUALLY USES:
//!   1. **the worker ← the stored record**: every key the Rust READS is declared by
//!      `ProviderSpec`/`ProviderModel`. A read of a field nobody writes is a read of `null`, forever.
//!   2. **the form → the parser**: every key the FORM sends is one `parseProviderSpec`/`parseProviderModel`
//!      actually accepts — the provider-level `body.<field>` accesses, plus `MODEL_FIELDS` for a model. A
//!      field the parser does not read is dropped on the floor, so the console would show a control that
//!      stores nothing.
//!
//! The lists are READ FROM THE SOURCES rather than restated here: a gate that hardcoded them would be a
//! fifth copy of the shape, drifting exactly like the four it is checking.
//!
//! MUTATION: rename a key in `gateway/src/store/providers.ts` — `apiKeyEnv` → `keyEnv` in `ProviderSpec`.
//! RESULT:   exit 101, naming the seam and the key:
//!             gateway/wasm/src/store.rs reads `apiKeyEnv`, and gateway/src/store/providers.ts does not
//!             declare it — the worker reads a field nothing writes.
//!
//! MUTATION (the other seam): rename `apiKeyEnv` → `keyEnv` in `ProviderDraft`.
//! RESULT:   exit 101 —
//!             gateway/ui/src/api/client.ts's `ProviderDraft` sends `keyEnv`, and
//!             gateway/src/store/providers.ts does not accept it — the server drops it on the floor, so the
//!             console shows a control that stores nothing.
//!
//! MUTATION (the field the first run found): drop `"input"` from `MODEL_FIELDS` in
//!           `gateway/src/store/providers.ts`.
//! RESULT:   the same refusal, naming `input` — and the consequence is the one worth stating: `input` is
//!           the wire spelling `vision` is DERIVED from, so every model the console saves would silently
//!           lose its vision flag while the form kept offering the control.

mod common;

use std::collections::BTreeSet;

/// The Rust reader: `gateway/wasm/src/store.rs`'s `provider_*` functions.
const RUST: &str = "gateway/wasm/src/store.rs";
/// The record the admin API validates and stores.
const SPEC: &str = "gateway/src/store/providers.ts";
/// What the console's form submits.
const DRAFT: &str = "gateway/ui/src/api/client.ts";

/// Every `get("…")` inside a `pub fn provider_…` block — the fields the worker actually reads.
fn rust_reads() -> BTreeSet<String> {
    let text = common::read(RUST);
    let mut out = BTreeSet::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with("pub fn provider_") {
            inside = true;
        } else if inside && line.starts_with('}') {
            inside = false;
        }
        if !inside {
            continue;
        }
        // `.get("apiKeyEnv")` — and nothing else on the line is a key.
        let mut rest = line;
        while let Some(i) = rest.find(".get(\"") {
            rest = &rest[i + 6..];
            if let Some(end) = rest.find('"') {
                out.insert(rest[..end].to_string());
                rest = &rest[end..];
            }
        }
    }
    assert!(
        out.len() >= 5,
        "only {} key(s) found in {RUST}'s provider functions — the reader this gate is built on moved or \
         was renamed, and a gate reading nothing must not pass",
        out.len()
    );
    out
}

/// The property names of one `<export> interface <name> { … }` block.
fn interface_keys(path: &str, name: &str) -> BTreeSet<String> {
    let text = common::read(path);
    // **`export` IS OPTIONAL, AND THE FIRST RUN OF THIS GATE IS WHY**: `ProviderDraft` is internal to the
    // client module and declared as a bare `interface`, while `ProviderSpec` is exported — anchoring on
    // `export interface` reported a rename that had not happened, which is a gate failing on its own parse.
    let anchor = format!("interface {name} {{");
    let start = text.find(&anchor).unwrap_or_else(|| {
        panic!("{path} has no `{anchor}` — the record this gate pins was renamed")
    });
    let body = &text[start + anchor.len()..];
    let end = body.find("\n}").expect("a closed interface");
    let mut out = BTreeSet::new();
    for line in body[..end].lines() {
        let line = line.trim_start();
        if line.starts_with("//") || line.starts_with('*') || line.starts_with("/*") {
            continue;
        }
        let Some((name, _)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().trim_end_matches('?');
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            out.insert(name.to_string());
        }
    }
    assert!(
        out.len() >= 3,
        "only {} key(s) parsed out of `{name}` in {path} — the parse is broken, not the record",
        out.len()
    );
    out
}

#[test]
fn the_worker_can_only_read_fields_the_record_declares() {
    let reads = rust_reads();
    let spec: BTreeSet<String> = interface_keys(SPEC, "ProviderSpec")
        .union(&interface_keys(SPEC, "ProviderModel"))
        .cloned()
        .collect();
    let missing: Vec<&String> = reads.difference(&spec).collect();
    assert!(
        missing.is_empty(),
        "{RUST} reads {}, and {SPEC} does not declare it — **the worker reads a field nothing writes**, \
         which is a field that is `null` forever. FIX: rename it on one side, or declare it in \
         `ProviderSpec`/`ProviderModel`.",
        missing.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ")
    );
    println!(
        "provider-record: the worker reads {} key(s), all declared in {SPEC}",
        reads.len()
    );
}

/// The wire fields the admin parser accepts: `body.<name>` inside `parseProviderSpec`, plus
/// `MODEL_FIELDS` for one model.
fn parser_fields() -> BTreeSet<String> {
    let text = common::read(SPEC);
    let anchor = "export function parseProviderSpec(";
    let start = text.find(anchor).unwrap_or_else(|| {
        panic!("{SPEC} has no `{anchor}` — the parser this seam pins was renamed")
    });
    let body = &text[start..];
    let end = body.find("\n}\n").expect("a closed function");
    let mut out = BTreeSet::new();
    let mut rest = &body[..end];
    while let Some(i) = rest.find("body.") {
        rest = &rest[i + 5..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            out.insert(name);
        }
    }
    // `MODEL_FIELDS` is the model-level half of the same list, and it is an array literal.
    let fields = text
        .find("const MODEL_FIELDS = [")
        .unwrap_or_else(|| panic!("{SPEC} has no `MODEL_FIELDS` — the model field list moved"));
    let line = &text[fields..text[fields..].find(']').expect("a closed array") + fields];
    for quoted in line.split('"').skip(1).step_by(2) {
        out.insert(quoted.to_string());
    }
    assert!(
        out.len() >= 6,
        "only {} wire field(s) parsed out of {SPEC} — the parse is broken, not the form",
        out.len()
    );
    out
}

#[test]
fn the_form_can_only_send_fields_the_parser_accepts() {
    let draft = interface_keys(DRAFT, "ProviderDraft");
    let accepted = parser_fields();
    let missing: Vec<&String> = draft.difference(&accepted).collect();
    assert!(
        missing.is_empty(),
        "{DRAFT}'s `ProviderDraft` sends {}, and {SPEC} does not accept it — **the server drops it on the \
         floor**, so the console shows a control that stores nothing. FIX: accept it in the parser (and \
         store it), or stop sending it.",
        missing.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ")
    );
    println!(
        "provider-record: the form sends {} key(s), all accepted by {SPEC}",
        draft.len()
    );
}
