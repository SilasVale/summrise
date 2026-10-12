//! THE BYOK CHANNEL TABLE, IN THE TWO PLACES IT IS WRITTEN — and the seam nothing was comparing.
//!
//! ── WHY THIS IS A GATE ───────────────────
//!
//! `gateway/src/store/byok.ts`'s `BYOK_CHANNELS` is the shared source: five fields per channel
//! (`prefix`, `kind`, `userKey`, `envKey`, `shape`) and a comment saying *"a second naming of channels is how two
//! tables drift apart."* `gateway/wasm/src/byok.rs` carries a PROJECTION of it — its own comment says *"the two
//! things this module needs from it"* — the `kind -> userKey` map and the `kind -> envKey` map, where `nv` and `gmi`
//! deliberately have **no** deployment fallback and the `null` is carried rather than inferred.
//!
//! **AND NOTHING COMPARED THEM.** `provider_record_keys.rs` already gates the provider record across the same
//! seam, so the pattern and the reason exist; this table had no equivalent. A rename of a key on either side is
//! silent: the worker reads `bearer_key_for`'s answer, and a `userKey` the TypeScript no longer declares leaves
//! every caller of that channel answering "no provider key" while both sides compile.
//!
//! **THE ASSERTION IS AGAINST THE LIST EACH SIDE ACTUALLY USES**: every `(kind, userKey)` the Rust carries must be
//! declared by the TypeScript, and every `envKey` must agree — including the `None`, because a channel that gains a
//! deployment fallback in one place and not the other changes which key a caller is billed to.
//!
//! MUTATION: rename a `userKey` in `gateway/src/store/byok.ts` — `userKey: "DEEPSEEK_API_KEY"` ->
//!           `userKey: "DEEPSEEK_KEY"`.
//! RESULT:   exit 101 — `gateway/wasm/src/byok.rs` carries `("deepseek", "DEEPSEEK_API_KEY", …)`, and
//!           `gateway/src/store/byok.ts` declares kind `deepseek` with `userKey` `DEEPSEEK_KEY`.
//!
//! MUTATION (the env seam): give `nv` an `envKey` on one side only — `envKey: "NVAPI_KEY"` in the TypeScript.
//! RESULT:   exit 101 — the Rust says kind `nvidia` has NO deployment fallback and the TypeScript now gives it
//!           one, which changes which key a caller is billed to.

mod common;

use std::collections::BTreeMap;

/// The projection: `(kind, userKey, envKey)` — `envKey` is `None` for the two channels with no deployment
/// fallback, which is information and not an omission.
const RUST: &str = "gateway/wasm/src/byok.rs";
/// The shared source.
const SPEC: &str = "gateway/src/store/byok.ts";

/// `(kind, userKey, envKey)` out of the Rust const's tuple rows.
fn rust_rows() -> BTreeMap<String, (String, Option<String>)> {
    let text = common::read(RUST);
    let start = text
        .find("pub(crate) const BYOK_CHANNELS")
        .unwrap_or_else(|| {
            panic!("{RUST} no longer declares BYOK_CHANNELS — the projection moved")
        });
    let body = &text[start..];
    let end = body.find("];").expect("a closed const");
    let body = &body[..end];
    let mut out = BTreeMap::new();
    // **ONLY A ROW STARTS WITH A QUOTED KIND.** `split('('` also lands on `Some("…")` inside a row and
    // on the TYPE ANNOTATION `[(&str, &str, Option<&str>); 9]`, and the first version of this gate read
    // those as rows — reporting `&str` as a kind and every key name after it. **A parse that admits the
    // wrong address reports about something else**, which is this session's recurring shape.
    // **MASK `Some(` BEFORE SPLITTING.** Every remaining `(` then opens a ROW, which is the only exact
    // discriminator: the source's formatting is MIXED — some rows break the line after `(` and some are
    // single-line (`("gmi", "GMI_API_KEY", None)`), so neither "starts with a quote" nor "starts with a
    // newline" separates a row from a `Some(` inside one. The type annotation `[(&str, …); 9]` also
    // survives, and the quote check below drops it.
    // Delete `Some(` rather than masking it: the row then reads
    // `("deepseek", "DEEPSEEK_API_KEY", "DEEPSEEK_API_KEY")`, so the third arm is a plain quoted string
    // and the `None` rows keep their `None` — no marker left inside a field.
    for chunk in body.replace("Some(", "").split('(').skip(1) {
        let Some((inner, _)) = chunk.split_once(')') else {
            continue;
        };
        if !inner.trim_start().starts_with('"') {
            continue;
        }
        // `"deepseek", "DEEPSEEK_API_KEY", Some("DEEPSEEK_API_KEY")` — the third arm is `Some(..)` or `None`.
        let mut parts = inner.split(',').map(|p| p.trim()).filter(|p| !p.is_empty());
        let kind = unquote(parts.next().unwrap_or_default());
        let user = unquote(parts.next().unwrap_or_default());
        // `flatten`, because the third arm's ABSENCE (a two-tuple row) and its `None` are the same
        // answer: no deployment fallback.
        let env = parts
            .next()
            .map(|p| {
                if p == "None" {
                    None
                } else {
                    Some(unquote(p.trim_start_matches("Some(").trim_end_matches(')')))
                }
            })
            .flatten();
        if kind.is_empty() {
            continue;
        }
        out.insert(kind, (user, env));
    }
    assert!(
        out.len() >= 8,
        "only {} row(s) parsed out of {RUST}'s BYOK_CHANNELS — the parse is broken, not the table",
        out.len()
    );
    out
}

/// `kind -> (userKey, envKey)` out of the TypeScript's array of records. One entry per channel, and
/// `kind` is unique: the derived `BYOK_KEY_FOR_KIND` is a `Record`, which is what makes a duplicate a
/// silent overwrite rather than an error.
fn ts_rows() -> BTreeMap<String, (String, Option<String>)> {
    let text = common::read(SPEC);
    let start = text
        // ANCHOR ON THE DECLARATION, NOT THE NAME: the module’s comment mentions
        // `BYOK_CHANNELS` long before `const BYOK_CHANNELS`, and the first version of this gate anchored on the
        // bare name, found 0 rows, and reported a broken parse — a gate that reads the wrong address is a gate
        // that answers about something else.
        .find("const BYOK_CHANNELS")
        .unwrap_or_else(|| {
            panic!("{SPEC} no longer declares BYOK_CHANNELS — the shared source moved")
        });
    let body = &text[start..];
    // **`]` IS NOT THE END: the type annotation is `ByokChannel[]`, so the FIRST `]` is inside the
    // declaration and every row after it is outside the range.** Anchor on `= [` and `];`.
    let body = &body[body.find("= [").expect("the array's opening bracket")..];
    let end = body.find("];").expect("a closed array");
    let body = &body[..end];
    let mut out = BTreeMap::new();
    for entry in body.split('{').skip(1) {
        let Some((inner, _)) = entry.split_once('}') else {
            continue;
        };
        let mut kind = String::new();
        let mut user = String::new();
        let mut env = None;
        // **SPLIT ON COMMAS, NOT LINES.** The single-line entries (`{ prefix: "nv", kind: "nvidia", … }`)
        // put every field on one line, so a line-anchored parse reads `prefix` and stops — which is how
        // the first version of this gate found 5 of the 9 channels and called its own parse broken.
        for line in inner.split(',') {
            let Some((k, v)) = line.split_once(':') else {
                continue;
            };
            let v = v.trim().trim_end_matches(',');
            match k.trim() {
                "kind" => kind = unquote(v),
                "userKey" => user = unquote(v),
                "envKey" => env = if v == "null" { None } else { Some(unquote(v)) },
                _ => {}
            }
        }
        if kind.is_empty() {
            continue;
        }
        if out
            .insert(kind.clone(), (user.clone(), env.clone()))
            .is_some()
        {
            panic!("{SPEC} declares kind `{kind}` twice — `BYOK_KEY_FOR_KIND` is a Record, so one of them is \
                    silently overwritten");
        }
    }
    assert!(
        out.len() >= 8,
        "only {} row(s) parsed out of {SPEC}'s BYOK_CHANNELS — the parse is broken, not the table",
        out.len()
    );
    out
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').to_string()
}

#[test]
fn the_byok_table_is_the_same_table_in_both_places() {
    let rust = rust_rows();
    let ts = ts_rows();

    let mut failures = Vec::new();
    for (kind, (user, env)) in &rust {
        let Some((ts_user, ts_env)) = ts.get(kind) else {
            failures.push(format!(
                "gateway/wasm/src/byok.rs carries kind `{kind}` (userKey `{user}`), and \
                 gateway/src/store/byok.ts does not declare that kind — the worker answers a key the \
                 shared source no longer names"
            ));
            continue;
        };
        if ts_user != user {
            failures.push(format!(
                "gateway/wasm/src/byok.rs carries kind `{kind}` with userKey `{user}`, and \
                 gateway/src/store/byok.ts declares `{ts_user}` for it — a rename on either side is silent: \
                 every caller of that channel answers \"no provider key\" while both sides compile"
            ));
        }
        if ts_env != env {
            failures.push(format!(
                "kind `{kind}`'s deployment fallback differs: the worker says {env:?} and the shared source \
                 says {ts_env:?} — which key a caller is billed to is not the same on both sides"
            ));
        }
    }
    // And the other direction: a channel the shared source names that the projection lost.
    for kind in ts.keys() {
        if !rust.contains_key(kind) {
            failures.push(format!(
                "gateway/src/store/byok.ts declares kind `{kind}`, and gateway/wasm/src/byok.rs does not \
                 carry it — the projection dropped a channel the shared source still names"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "the BYOK channel table is written in two places and they disagree:\n  {}",
        failures.join("\n  ")
    );
}
