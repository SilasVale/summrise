//! THREE COPIES OF ONE FIELD, HELD TO ONE SET OF FACTS.
//!
//! `scripts/test/particles-check.mjs` (179 lines), transliterated. The ninth gate to move into
//! `agent/tests/*.rs`, and the first whose subject is THREE FILES IN TWO LANGUAGES — which is why
//! its numeric reader has a Rust arm as well as a JavaScript one.
//!
//! WHY IT EXISTS (round 15 of the standing goal). The ambient particle field exists three times —
//! the panel and the console as TypeScript modules (a canvas behind every surface), the landing in
//! RUST compiled to wasm (2026-09-28: that page has no JS left at all). The objective's spine says
//! no surface should compute its own version of the same fact, and this is the one place where the
//! copies are unavoidable; so the FACTS they must share are checked instead of the code being
//! shared.
//!
//! THEY HAD ALREADY DRIFTED INTO THE SAME TWO DEFECTS, one round apart:
//!
//!   * **THE RETIRED PALETTE.** Both modules asked for `--aura-1/3/4` — tokens the rebrand REMOVED
//!     — read empty, and fell back to 190 cyan / 280 violet / 330 pink: the background of the whole
//!     product in the colours the brand had abandoned.
//!   * **DRAWING BY HUE.** All three converted a token to a hue and drew `hsla(h 90% 62%)`, so even
//!     a correct token would not have been the colour on screen. A palette entry is a COLOUR, and
//!     the field draws it as one.
//!
//! ── THE FIVE FACTS, each checkable in the source and each the shape of a real defect ────────────
//!
//!   1. IT READS BRAND TOKENS, and no retired one.
//!   2. EVERY HEX IT FALLS BACK TO IS ONE OF THE BRAND'S. A fallback is what people see when a token
//!      is missing — which is exactly how the cyan arrived — so a fallback that is not a brand
//!      colour is the defect itself.
//!   3. IT DRAWS WITH `rgba(` FROM A TRIPLE, not `hsla(` from a hue.
//!   4. IT REFUSES TO RUN AT ALL UNDER `prefers-reduced-motion`. A canvas loop is not a CSS
//!      animation, so `motion_check.rs` cannot see it: it reads the sheets, and this is JavaScript
//!      painting every 1/30th of a second forever. All three copies return before creating their
//!      canvas, which is the honest fallback — the static wash stays, the motion does not — and
//!      VERIFIED ON THE DEVICE: with reduced motion emulated, rAF 0/s, clears 0/s, fills 0/s and no
//!      canvas in the DOM at all.
//!   5. **THE THREE COPIES DRAW THE SAME FIELD** — the same cap, the same density and the same peak
//!      alpha. This is a fact of its own because 1-4 are about WHERE THE COLOUR COMES FROM, and all
//!      three copies can agree on every one of them while drawing visibly different fields: a
//!      90-mote cap here, 140 there. The numbers happened to agree when this was written
//!      (90 / 5.5 per 100000 / MAX_ALPHA * twinkle * 0.35), which is exactly the state that does not
//!      survive unenforced — nothing in this repository compared them.
//!
//! ── THE DENSITY IS WRITTEN IN TWO UNITS AND THEY ARE THE SAME NUMBER ───────────────────────────
//!
//! The modules say `DENSITY = 0.55` and apply it as `(w * h / 100_000) * DENSITY * 10`; the landing
//! says `(w * h / 100000) * 5.5`. A check that compared the LITERALS would report a divergence that
//! is not there, so this resolves the named constant and compares the EFFECTIVE per-100000 value.
//!
//! AND THE RUST ARM IS NOT A CONVENIENCE: Rust spells the same four numbers differently — a `const`
//! carries a type between its name and its `=` (`const MAX_MOTES: usize = 90`) and a minimum is a
//! method call (`MAX_MOTES.min(…)`). Without that arm every expression reads `None` for the landing,
//! and the guard at the bottom reports it as "a comparison that cannot see all three" — the right
//! FAILURE but a useless check. The facts are unchanged; only the reader follows the language.
//!
//! ── MIGRATION-TIME EQUIVALENCE, MEASURED (both implementations, one tree, 2026-09-29) ──────────
//!
//! The `.mjs` was restored from `main`, both were run over the same tree, and `cmp` was applied to
//! the two streams. SIX CASES, EVERY ONE BYTE-IDENTICAL:
//!
//!   case                                 js / rust            body bytes   the copy mutated
//!   ────────────────────────────────────────────────────────────────────────────────────────────
//!   clean tree                           accept / accept      198          (none)
//!   A  cap 90 → 140                      exit 1 / exit 101    185          the landing's Rust
//!   B  a brand token → `--aura-1`        exit 1 / exit 101    196          the landing's Rust
//!   C  a fallback → `#00ffff`            exit 1 / exit 101    262          the panel's TypeScript
//!   D  the reduced-motion refusal gone   exit 1 / exit 101    222          the landing's Rust
//!   E  every `rgba(` → `hsla(`           exit 1 / exit 101    201          the landing's Rust
//!
//! A, B, D AND E ARE THE RUST COPY AND C IS THE TYPESCRIPT ONE, so both readers are exercised in the
//! biting direction — including the Rust arm that reads `const MAX_MOTES: usize = 90` where the
//! JavaScript arm reads `MAX_MOTES = 90`. **AND TWO OF THESE CASES WERE WRONG BEFORE THEY WERE
//! RIGHT**: `prefers-reduced-motion` and `rgba(` each appear THREE times in the landing, and a
//! `replace(…, 1)` mutation left the other two in place, so both implementations ACCEPTED a tree the
//! mutation was supposed to break. Both said so in the same way — which is why a mutation that does
//! not bite has to be read as a fact about the mutation, not about the gate.
//!
//! MUTATION: put the retired palette back (`colour_of("--aura-1", "#00ffff")` in the landing), or
//!           give a copy a fallback that is not a brand colour, or change one copy's cap
//!           (`const MAX_MOTES: usize = 140;` in the landing).
//! RESULT:   exit 101 either way: "index/landing/src/interactive.rs reads the RETIRED --aura-1 —
//!           that is the palette the rebrand removed"; "falls back to #00ffff, which is not one of
//!           the brand's colours — a fallback is what people SEE when a token is missing"; and
//!           "index/landing/src/interactive.rs draws a different field: cap is 140 where the others
//!           are 90". **THE FIX IS IN THE MESSAGE** — each sentence names the file, the value it
//!           found and the value the others have. Rendered evidence for the landing copy, for the
//!           next reader: mote cores measure 255,210,60 (= `#ffd43b` exactly), 243,156,0 and
//!           232,87,12, with ZERO blue-dominant pixels.

mod common;

use common::repo;
use std::fs;

/// Every copy of the field, and the reason it is its own copy.
const FIELDS: [(&str, &str); 3] = [
    (
        "agent/resources/panel-react/src/lib/particles.ts",
        "the panel's — a module, bundled",
    ),
    (
        "gateway/ui/src/lib/particles.ts",
        "the console's — a module, bundled",
    ),
    (
        "index/landing/src/interactive.rs",
        "the landing's — RUST now, compiled to wasm; the page has no JS left to inline it into",
    ),
];

/// The brand's own values, across all three surfaces: the white-text gradient pair, the accent, and
/// the mark.
const BRAND_HEXES: [&str; 6] = [
    "#c2410c", "#9a3412", "#bf3a0a", "#f59f00", "#e8590c", "#ffd43b",
];

/// Only the FIELD's own code: the file may mention a token in a comment explaining why it left.
///
/// The `.mjs` ran four replacements in this order: `/* … */` (non-greedy), then `^\s*//.*$`, then
/// `^\s*\*.*$`, then `^\s*//.*$` AGAIN. The last one is redundant and is kept as a no-op rather
/// than tidied, because the order is the behaviour and a port that "cleaned it up" would be a
/// different function.
fn field_code(src: &str) -> String {
    // 1. `/* … */`, non-greedy — two block comments on one line are two removals.
    let mut out = String::with_capacity(src.len());
    let s: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < s.len() {
        if i + 1 < s.len() && s[i] == '/' && s[i + 1] == '*' {
            match (i + 2..s.len().saturating_sub(1)).find(|&k| s[k] == '*' && s[k + 1] == '/') {
                Some(k) => {
                    i = k + 2;
                    continue;
                }
                None => break,
            }
        }
        out.push(s[i]);
        i += 1;
    }
    // 2, 3 and 4: whole LINES that are a `//` comment or a `*` continuation, in that order.
    let mut kept: Vec<String> = Vec::new();
    for line in out.split('\n') {
        let t = line.trim_start();
        if t.starts_with("//") || t.starts_with('*') {
            continue;
        }
        kept.push(line.to_string());
    }
    kept.join("\n")
}

/// `["'`](--[a-z0-9-]+)["'`]` — a token in quotes.
///
/// The quote characters are NOT required to match, which is the `.mjs`'s own behaviour: the
/// character class is `["'` + backtick + `]` at both ends, so `'--brand-x"` would match. It cannot
/// occur in these files and the shape is kept rather than tightened, because tightening it is a
/// change to the gate's subject.
fn quoted_tokens(code: &str) -> Vec<String> {
    let s: Vec<char> = code.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if !is_quote(s[i]) {
            i += 1;
            continue;
        }
        // `--[a-z0-9-]+`
        let start = i + 1;
        if start + 2 <= s.len() && s[start] == '-' && s[start + 1] == '-' {
            let mut k = start + 2;
            while k < s.len() && (s[k].is_ascii_lowercase() || s[k].is_ascii_digit() || s[k] == '-')
            {
                k += 1;
            }
            if k > start + 2 && k < s.len() && is_quote(s[k]) {
                out.push(s[start..k].iter().collect());
                i = k + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// `["'`](#[0-9a-f]{6})["'`]` with the `i` flag, lower-cased — a hex fallback in quotes.
fn quoted_hexes(code: &str) -> Vec<String> {
    let s: Vec<char> = code.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if !is_quote(s[i]) {
            i += 1;
            continue;
        }
        let start = i + 1;
        if start < s.len() && s[start] == '#' {
            let mut k = start + 1;
            while k < s.len() && s[k].is_ascii_hexdigit() {
                k += 1;
            }
            // EXACTLY six, which is what `{6}` says: a seven-digit run is not a match at all.
            if k == start + 7 && k < s.len() && is_quote(s[k]) {
                out.push(s[start..k].iter().collect::<String>().to_lowercase());
                i = k + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn is_quote(c: char) -> bool {
    c == '"' || c == '\'' || c == '`'
}

/// Does `hay` contain `needle` at `at`?
fn at(hay: &[char], at: usize, needle: &str) -> bool {
    let n: Vec<char> = needle.chars().collect();
    at + n.len() <= hay.len() && hay[at..at + n.len()] == n[..]
}

/// The four numbers one copy states, as the `.mjs` read them.
#[derive(Default, Clone, Copy)]
struct Field {
    cap: Option<f64>,
    alpha: Option<f64>,
    density: Option<f64>,
    alpha_factor: Option<f64>,
}

/// `(?:const |var |let )?NAME\s*=\s*([0-9.]+)` — the JavaScript declaration shape.
fn js_num(code: &[char], name: &str) -> Option<f64> {
    let mut i = 0;
    while i < code.len() {
        if at(code, i, name) {
            // The name must not be preceded by a word character, or `MY_MAX_MOTES` would match.
            let before_ok = i == 0 || !(code[i - 1].is_alphanumeric() || code[i - 1] == '_');
            let mut k = i + name.chars().count();
            if before_ok && (k >= code.len() || !(code[k].is_alphanumeric() || code[k] == '_')) {
                while k < code.len() && code[k].is_whitespace() {
                    k += 1;
                }
                if k < code.len() && code[k] == '=' {
                    k += 1;
                    while k < code.len() && code[k].is_whitespace() {
                        k += 1;
                    }
                    let start = k;
                    while k < code.len() && (code[k].is_ascii_digit() || code[k] == '.') {
                        k += 1;
                    }
                    if k > start {
                        return code[start..k]
                            .iter()
                            .collect::<String>()
                            .parse::<f64>()
                            .ok();
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// `(?:const|static)\s+NAME\s*:\s*[A-Za-z0-9_<>]+\s*=\s*([0-9.]+)` — the Rust declaration shape.
fn rs_num(code: &[char], name: &str) -> Option<f64> {
    let mut i = 0;
    while i < code.len() {
        if at(code, i, name) {
            let before_ok = i == 0 || !(code[i - 1].is_alphanumeric() || code[i - 1] == '_');
            let mut k = i + name.chars().count();
            if before_ok {
                while k < code.len() && code[k].is_whitespace() {
                    k += 1;
                }
                if k < code.len() && code[k] == ':' {
                    k += 1;
                    while k < code.len() && code[k].is_whitespace() {
                        k += 1;
                    }
                    while k < code.len()
                        && (code[k].is_alphanumeric()
                            || code[k] == '_'
                            || code[k] == '<'
                            || code[k] == '>')
                    {
                        k += 1;
                    }
                    while k < code.len() && code[k].is_whitespace() {
                        k += 1;
                    }
                    if k < code.len() && code[k] == '=' {
                        k += 1;
                        while k < code.len() && code[k].is_whitespace() {
                            k += 1;
                        }
                        let start = k;
                        while k < code.len() && (code[k].is_ascii_digit() || code[k] == '.') {
                            k += 1;
                        }
                        if k > start {
                            return code[start..k]
                                .iter()
                                .collect::<String>()
                                .parse::<f64>()
                                .ok();
                        }
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// `fieldOf(rel, code)` — the four numbers, read in the language the copy is written in.
fn field_of(rel: &str, code: &str) -> Field {
    let c: Vec<char> = code.chars().collect();
    if rel.ends_with(".rs") {
        // The USE is checked as well as the value, exactly as `Math.min(MAX_MOTES` checks it for
        // the three JavaScript copies: a constant nobody multiplies by is not a density.
        let used = code.contains("MAX_MOTES.min(");
        let peak = code.contains("MAX_ALPHA * tw * TWINKLE");
        return Field {
            cap: if used { rs_num(&c, "MAX_MOTES") } else { None },
            alpha: rs_num(&c, "MAX_ALPHA"),
            density: if used { rs_num(&c, "DENSITY") } else { None },
            alpha_factor: if peak { rs_num(&c, "TWINKLE") } else { None },
        };
    }

    let cap = js_num(&c, "MAX_MOTES");
    let alpha = js_num(&c, "MAX_ALPHA");

    // NO REGEX FOR THE FRAGILE HALF. The first version of the `.mjs` parsed the density with
    // expressions like `/\*\s*10\s*$/` and a heredoc double-escaped them into matching a literal
    // backslash — so two of the three copies read as unparseable and the check (correctly) refused
    // to compare. Splitting on characters cannot be mis-escaped, and that is kept here.
    let mut density: Option<f64> = None;
    let want = code.find("Math.min(MAX_MOTES");
    if let Some(want) = want {
        // `code.slice(want, code.indexOf(";", want))`, then everything after `Math.round(`.
        let semi = code[want..]
            .find(';')
            .map(|k| want + k)
            .unwrap_or(code.len());
        let slice: Vec<char> = code[want..semi].chars().collect();
        let after_round = find_seq(&slice, "Math.round(").map(|k| k + "Math.round(".len());
        let arg: Vec<char> = match after_round {
            Some(k) => slice[k..].to_vec(),
            None => Vec::new(),
        };
        // `arg.replace(/\)+\s*$/, "").trim()` — trailing closing parens and whitespace off.
        let mut end = arg.len();
        while end > 0 && (arg[end - 1] == ')' || arg[end - 1].is_whitespace()) {
            end -= 1;
        }
        let tail: String = arg[..end].iter().collect();
        let parts: Vec<&str> = tail.split('*').map(|x| x.trim()).collect();
        let last = parts.last().copied().unwrap_or("");
        let prev = if parts.len() >= 2 {
            parts[parts.len() - 2]
        } else {
            ""
        };
        // The density is the SECOND-to-last factor when there is a trailing `* 10`, and the last one
        // otherwise.
        let named = if prev == "DENSITY" || last == "DENSITY" {
            js_num(&c, "DENSITY")
        } else {
            None
        };
        density = match named {
            Some(n) => Some(if last == "10" { n * 10.0 } else { n }),
            None => last.parse::<f64>().ok(),
        };
    }

    let mut alpha_factor: Option<f64> = None;
    if let Some(use_at) = code.find("MAX_ALPHA *") {
        let close = code[use_at..]
            .find(')')
            .map(|k| use_at + k)
            .unwrap_or(code.len());
        let parts: Vec<&str> = code[use_at..close].split('*').map(|x| x.trim()).collect();
        alpha_factor = parts.last().and_then(|x| x.parse::<f64>().ok());
    }

    Field {
        cap,
        // `Number.isFinite(density) ? density : null` — and `parse::<f64>` already refuses NaN and
        // the infinities this can produce, so the two agree.
        alpha,
        density: density.filter(|d| d.is_finite()),
        alpha_factor: alpha_factor.filter(|d| d.is_finite()),
    }
}

/// The first index at or after 0 where `pat` sits.
fn find_seq(s: &[char], pat: &str) -> Option<usize> {
    let p: Vec<char> = pat.chars().collect();
    (0..=s.len().saturating_sub(p.len())).find(|&i| s[i..i + p.len()] == p[..])
}

#[test]
fn three_copies_of_one_field_agree() {
    let root = repo();
    let mut failures: Vec<String> = Vec::new();
    let mut fields = 0;

    for (rel, why) in FIELDS {
        let src =
            fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("cannot read {rel}: {e}"));
        let code = field_code(&src);
        fields += 1;

        // ── 1. brand tokens, and no retired one
        let tokens = quoted_tokens(&code);
        let brand: Vec<&String> = tokens
            .iter()
            .filter(|t| t.starts_with("--brand-grad-") || t.starts_with("--brand-mark-"))
            .collect();
        let retired: Vec<&String> = tokens.iter().filter(|t| t.starts_with("--aura-")).collect();
        if brand.len() < 2 {
            failures.push(format!(
                "{rel} ({why}) reads {} brand token(s) — the field must take its palette from the brand",
                brand.len()
            ));
        }
        if !retired.is_empty() {
            let list = retired
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            failures.push(format!(
                "{rel} reads the RETIRED {list} — that is the palette the rebrand removed"
            ));
        }

        // ── 2. the fallbacks are the brand's
        let fallbacks = quoted_hexes(&code);
        for hex in &fallbacks {
            if !BRAND_HEXES.contains(&hex.as_str()) {
                failures.push(format!(
                    "{rel} falls back to {hex}, which is not one of the brand's colours — a fallback is what people SEE when a token is missing"
                ));
            }
        }
        if fallbacks.len() < 2 {
            failures.push(format!(
                "{rel} has {} hex fallback(s) — a reader with no fallback draws nothing sensible",
                fallbacks.len()
            ));
        }

        // ── 3. drawn as a colour, not as a hue
        if draws_from_a_hue(&code) {
            failures.push(format!(
                "{rel} draws hsla() from a hue — a token is a COLOUR, and the field must draw that colour"
            ));
        }
        if !code.contains("rgba(") {
            failures.push(format!(
                "{rel} never draws rgba() — the palette is supposed to arrive as a colour triple"
            ));
        }

        // ── 4. it refuses to run at all under prefers-reduced-motion
        if !code.contains("prefers-reduced-motion") {
            failures.push(format!(
                "{rel} runs without checking prefers-reduced-motion — a canvas loop is motion the CSS gate cannot see"
            ));
        }
    }

    // ── 5. THE SAME FIELD: cap, density and peak alpha agree across the three copies.
    let seen: Vec<(String, Field)> = FIELDS
        .iter()
        .map(|(rel, _)| {
            let src = fs::read_to_string(root.join(rel))
                .unwrap_or_else(|e| panic!("cannot read {rel}: {e}"));
            (rel.to_string(), field_of(rel, &src))
        })
        .collect();

    for (key, get) in [
        ("cap", (|f: &Field| f.cap) as fn(&Field) -> Option<f64>),
        ("alpha", |f: &Field| f.alpha),
        ("density", |f: &Field| f.density),
        ("alphaFactor", |f: &Field| f.alpha_factor),
    ] {
        let values: Vec<f64> = seen.iter().filter_map(|(_, f)| get(f)).collect();
        if values.len() < 3 {
            failures.push(format!(
                "only {} of 3 copies state their {key} in a form this check can read — a comparison that cannot see all three proves nothing",
                values.len()
            ));
            continue;
        }
        let first = values[0];
        for (rel, f) in &seen {
            if let Some(v) = get(f) {
                if v != first {
                    failures.push(format!(
                        "{rel} draws a different field: {key} is {v} where the others are {first}"
                    ));
                }
            }
        }
    }

    // A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN.
    assert!(
        fields >= 3,
        "particles-check: FAILED — read {fields} field(s), and there are three"
    );

    if !failures.is_empty() {
        let report = failures
            .iter()
            .map(|f| format!("  {f}"))
            .collect::<Vec<_>>()
            .join("\n");
        panic!(
            "particles-check: FAILED — the copies of the ambient field no longer agree on the facts:\n{report}"
        );
    }

    let f0 = &seen[0].1;
    println!(
        "particles-check: ok — {fields} copies of the ambient field: brand tokens as colours, brand fallbacks, no motion under prefers-reduced-motion, and one set of numbers (cap {}, {} per 100000, alpha x{})",
        f0.cap.unwrap_or(f64::NAN),
        f0.density.unwrap_or(f64::NAN),
        f0.alpha_factor.unwrap_or(f64::NAN)
    );
}

/// `/hsla\(\s*['"]?\s*\+?\s*(m\.)?hue/` OR `/hsla\(['"]\s*\+\s*m\.hue/` — the two shapes the `.mjs`
/// refused, written as one scan: `hsla(` … `hue` with nothing but quotes, `+` and whitespace between
/// them, and the `.mjs`'s own two alternatives are the two ways that can be spelled.
fn draws_from_a_hue(code: &str) -> bool {
    let s: Vec<char> = code.chars().collect();
    let mut i = 0;
    while i < s.len() {
        if at(&s, i, "hsla(") {
            let mut k = i + 5;
            // quotes, `+`, whitespace and an optional `m.` — then `hue`.
            while k < s.len() && (is_quote(s[k]) || s[k] == '+' || s[k].is_whitespace()) {
                k += 1;
            }
            if at(&s, k, "m.") {
                k += 2;
            }
            while k < s.len() && s[k].is_whitespace() {
                k += 1;
            }
            if at(&s, k, "hue") {
                return true;
            }
        }
        i += 1;
    }
    false
}
