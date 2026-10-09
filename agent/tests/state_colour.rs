//! STATE COLOUR IS FOR STATE, AND THE CHROME STAYS NEUTRAL.
//!
//! `scripts/test/state-colour-check.mjs` (150 lines), transliterated. The twelfth gate to move into
//! `agent/tests/*.rs`, and the first to pass ALL THREE of the census's constraints (the plan's
//! "P1 的普查" sections): it reads DATA (two stylesheets), it spawns nothing, and its proof is a
//! mutation of those sheets — which lives in this repository.
//!
//! WHY IT EXISTS (round 12 of the standing goal). The objective's clause is "the chrome neutral and
//! still, with colour and motion reserved to that state layer", and nothing measured it. Measured
//! first, on both front ends: 122 rules in the panel and 41 in the console paint with a state colour,
//! and essentially every one of them is either the state layer itself, a DESTRUCTIVE action (red means
//! this deletes), a CATEGORY lane (colour is the second channel there, deliberately), or a token
//! definition. That is a clean result — and a clean result that nothing enforces is one commit away
//! from not being true.
//!
//! HOW IT JUDGES. Every rule that paints with a state colour must match one of the PURPOSES, each of
//! which carries its reason. A rule that matches none FAILS and names itself, which is the moment to
//! ask whether the colour is carrying state or decorating chrome — and, if it is carrying state, to add
//! the purpose here with the reason. **That is the whole design: the list is not a suppression file,
//! it is the set of things state colour is FOR.**
//!
//! AND IT DELIBERATELY DOES NOT JUDGE `--accent`. An accent button is an ACTION, not a state, and
//! dressing every primary button in grey to satisfy a slogan would be a worse interface.
//!
//! ── THE TOOL IS REWRITTEN; THE SUBJECT IS NOT (the census's constraint ②) ──────────────────────
//!
//! The `.mjs` imports `loudnessOf` from `agent/scripts/lib/design-sweep.mjs` and calls it. That is a
//! TOOL, not the subject — the subject is the two stylesheets — so this file carries its own copy,
//! transliterated below. (Contrast that with `agent/tests/contrast_probe.rs`, whose SUBJECT is the probe
//! module's arithmetic: **that one DID move, and the split is why** — its rules are a Rust transliteration
//! pinned by the gate's own numbers, and `contrast_probe_emitted.rs` separately pins that the MODULE still
//! carries those functions, verbatim, and that the artifact the sweeps inject compiles.)
//!
//! ── MIGRATION-TIME EQUIVALENCE, MEASURED (both implementations, one tree, 2026-09-29) ──────────
//!
//! The `.mjs` was restored from `main`, both were run over the same tree, and `cmp` was applied to the
//! two streams. FOUR CASES, THE VERDICT IDENTICAL ON EVERY ONE, 153 state-colour uses read in each:
//!
//!   case                                     js / rust            body bytes   what was mutated
//!   ────────────────────────────────────────────────────────────────────────────────────────────
//!   clean tree                               accept / accept      116          (none)
//!   A  `.side-row { color: var(--state-fail) }`  exit 1 / exit 101   see note    the `.mjs`'s own
//!      appended to the BUILT panel sheet                           below          mutation
//!   B  the console's dark `--info-bg` set to     exit 1 / exit 101   483          a LOUD surface —
//!      the historical `#1a3a5c`                                                  the second half
//!   C  `.session-status-line` with a state       accept / accept      116          **must NOT bite**
//!      colour, a name the list already covers
//!
//! **AND THE ONE DELIBERATE DIFFERENCE IS A PATH IN A MESSAGE.** A's body is identical after
//! substituting `scripts/test/state-colour-check.mjs` → `agent/tests/state_colour.rs`, because the
//! failure says "it belongs on the list in <file> with its reason" — and that file is about to be
//! deleted. Pointing a reader at a file that no longer exists is worse than a message that differs
//! from its predecessor, so the Rust names the list's new home. The differential normalises it:
//!
//!   sed 's#scripts/test/state-colour-check.mjs#agent/tests/state_colour.rs#' <the js stderr>
//!
//! **B IS THE HALF THAT WOULD HAVE BEEN EASY TO MISS**: it is not a selector at all, it is a TOKEN
//! VALUE, judged by `loudnessOf` — the function the probe itself evaluates — and it needs the theme
//! split (`data-theme="dark"`) and "the LAST declaration wins" to see it at all. Its mutation also had
//! to go to the CONSOLE's sheet rather than the panel's, which is how the first attempt at it found
//! nothing to break: `--info-bg` is defined in `gateway/ui/src/styles/globals.css`.
//!
//! MUTATION: paint a neutral element with a state colour — `.side-row { color: var(--state-fail) }` on
//!           the BUILT panel sheet — or make a state surface loud (`--info-bg: #1a3a5c`).
//! RESULT:   exit 101: "state-colour-check: FAILED — state colour on something that is not state:" and
//!           "panel: .side-row paints with a state colour, and matches no purpose on the list"; and
//!           for the second, "state-colour-check: FAILED — 1 state surface(s) are LOUD, and a status
//!           surface is a tint" naming the token, its hex, saturation and lightness. **THE FIX IS IN
//!           EACH MESSAGE**: the first names the selector and says the choice is "state, so put it on
//!           the list with its reason" or "chrome wearing a colour that means something it does not",
//!           and the second states the band and what to do instead.

mod common;

use common::repo;
use std::fs;

/// The tokens that MEAN a state, as opposed to the accent, which means an action.
///
/// `/--(state-ok|state-warn|state-fail|state-running|success|error|danger|warn-ink|danger-on-ink|danger-on-soft|danger-soft|danger-ink|ok-ink)\b/`
/// — CASE-SENSITIVE (no `i` flag), unlike every pattern below.
const STATE_TOKENS: [&str; 13] = [
    "state-ok",
    "state-warn",
    "state-fail",
    "state-running",
    "success",
    "error",
    "danger",
    "warn-ink",
    "danger-on-ink",
    "danger-on-soft",
    "danger-soft",
    "danger-ink",
    "ok-ink",
];

/// What state colour is FOR: `[needles, reason]`, and a selector matching NONE of them is a finding.
///
/// THE `\b` NEEDLES ARE SEPARATE, and that is not a detail: `ok\b` must not match `token`, and `arm\b`
/// must not match `armed`. The `.mjs` writes them inside its alternations; here they are their own
/// lists so the word-end rule is applied to exactly the needles that carry it.
struct Purpose {
    needles: &'static [&'static str],
    word_end: &'static [&'static str],
    prefix: bool,
    /// THE REASON IS NEVER READ BY THE CHECK, AND THAT IS THE DESIGN RATHER THAN AN OVERSIGHT — the
    /// `.mjs` never read its own either. The list is not a suppression file; it is the set of things
    /// state colour is FOR, and the reason is what the person who wants to add an entry has to write
    /// down. `#[allow(dead_code)]` because a compiler warning about it would invite deleting the one
    /// field that makes the list a decision instead of an exemption.
    #[allow(dead_code)]
    reason: &'static str,
}

const PURPOSES: [Purpose; 15] = [
    Purpose {
        needles: &[":root", "body[data-theme", "@media", "*"],
        word_end: &[],
        prefix: true,
        reason: "a token DEFINITION, not a use of one",
    },
    Purpose {
        needles: &[
            "dot", "mark", "led", "badge", "chip", "signal", "indicator", "swatch",
        ],
        word_end: &[
            "dot", "mark", "led", "badge", "chip", "signal", "indicator", "swatch",
        ],
        prefix: false,
        reason: "the state layer itself — a mark carrying state",
    },
    Purpose {
        needles: &[
            "status", "state", "verdict", "result", "health", "alert", "notice", "toast", "msg",
            "message", "banner", "warn", "error", "fail", "urgent", "expired", "evicted", "crash",
            "blocked", "offline", "online",
        ],
        word_end: &["ok"],
        prefix: false,
        reason: "a status surface: the thing the state is about",
    },
    Purpose {
        needles: &["approval", "revoke", "grant", "refuse", "expire"],
        word_end: &["arm"],
        prefix: false,
        reason: "the approval gate — a pending question IS the state",
    },
    Purpose {
        needles: &[
            "close", "clear", "archive", "remove", "delete", "purge", "evict", "danger", "logout",
            "kill", "stop", "disconnect", "forget", "discard",
        ],
        word_end: &[],
        prefix: false,
        reason: "a DESTRUCTIVE action: the colour says what the click does",
    },
    Purpose {
        needles: &["confirm"],
        word_end: &[],
        prefix: false,
        reason: "the confirm affordance's own text — the destructive branch stated in words before you take it",
    },
    Purpose {
        needles: &["lane-", "provider", "prefix", "model-"],
        word_end: &[],
        prefix: false,
        reason: "a CATEGORY lane — colour as the second channel, deliberately not a state",
    },
    Purpose {
        needles: &["traj", "cmd-", "run-", "path-", "step-", "exit"],
        word_end: &[],
        prefix: false,
        reason: "a trajectory or command outcome",
    },
    Purpose {
        needles: &[
            "progress", "stream", "spinner", "pulse", "live", "sync", "conn", "plug", "socket",
            "series",
        ],
        word_end: &[],
        prefix: false,
        reason: "a live or in-flight surface",
    },
    // TRIAGED from this check's first run (round 12): nine rules that paint with a state colour and
    // match no other purpose. Every one is a state surface whose NAME does not contain the word
    // "state", which is exactly the case the list exists to make somebody write down.
    Purpose {
        needles: &["tone", "dial", "gauge"],
        word_end: &[],
        prefix: false,
        reason: "a dial's tone IS the state it renders (data-tone=ok|warn|crit)",
    },
    Purpose {
        needles: &["monitor"],
        word_end: &[],
        prefix: false,
        reason: "a monitor that did not answer: the card and its log lines",
    },
    Purpose {
        needles: &["timeout", "is-down", "offline"],
        word_end: &["down"],
        prefix: false,
        reason: "a session or probe that timed out or is down",
    },
    Purpose {
        needles: &["available", "latest"],
        word_end: &[],
        prefix: false,
        reason: "the update card's two states: an update is available, or this is the latest",
    },
    Purpose {
        needles: &["activity-row"],
        word_end: &[],
        prefix: false,
        reason: "an activity row's outcome",
    },
    Purpose {
        needles: &["browser-action"],
        word_end: &[],
        prefix: false,
        reason: "the failure detail of a browser action",
    },
];

/// `/:hover|:active|:focus/` — a destructive action is only "destructive" when it answers an
/// interaction; a static red border on a container is not a button and does not earn the colour.
const DESTRUCTIVE_STATE: [&str; 3] = [":hover", ":active", ":focus"];
const DESTRUCTIVE_NAMES: [&str; 12] = [
    "close", "clear", "archive", "remove", "delete", "purge", "evict", "logout", "kill", "stop",
    "discard", "forget",
];

const SURFACE_TOKENS: [&str; 5] = [
    "--info-bg",
    "--success-bg",
    "--warn-bg",
    "--danger-bg",
    "--err-bg",
];

/// A rule's selector and body, from `([^{}]+)\{([^{}]*)\}`.
struct Rule {
    sel: String,
    body: String,
}

fn rules(css: &str) -> Vec<Rule> {
    // Comments out, non-greedy — the same strip the other sheet gates use.
    let clean = strip_comments(css);
    let c: Vec<char> = clean.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '{' {
            // walk back for the selector
            let mut s = i;
            while s > 0 && c[s - 1] != '}' && c[s - 1] != '{' {
                s -= 1;
            }
            let sel: String = c[s..i].iter().collect();
            let mut e = i + 1;
            while e < c.len() && c[e] != '}' && c[e] != '{' {
                e += 1;
            }
            if e < c.len() && c[e] == '}' {
                let body: String = c[i + 1..e].iter().collect();
                let sel = sel.split_whitespace().collect::<Vec<_>>().join(" ");
                out.push(Rule { sel, body });
                i = e + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn strip_comments(css: &str) -> String {
    let s: Vec<char> = css.chars().collect();
    let mut out = String::with_capacity(css.len());
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
    out
}

/// Case-insensitive substring — the `.mjs`'s `/needle/i`.
fn ci_contains(hay: &str, needle: &str) -> bool {
    let (h, n) = (hay.to_lowercase(), needle.to_lowercase());
    h.contains(&n)
}

/// `\b` after a needle: the substring must END at a non-word character or the end of the string.
/// `/ok\b/i` must not match `token`, and `/arm\b/i` must not match `armed`.
fn ci_contains_word_end(hay: &str, needle: &str) -> bool {
    let (h, n) = (hay.to_lowercase(), needle.to_lowercase());
    let hb = h.as_bytes();
    let mut from = 0;
    while let Some(at) = h[from..].find(&n) {
        let end = from + at + n.len();
        let boundary = end >= hb.len() || !(hb[end].is_ascii_alphanumeric() || hb[end] == b'_');
        if boundary {
            return true;
        }
        from = from + at + 1;
    }
    false
}

/// `--{token}\b`, case-SENSITIVE.
fn has_state_token(body: &str) -> bool {
    STATE_TOKENS
        .iter()
        .any(|t| ci_contains_word_end_sensitive(body, &format!("--{t}")))
}

fn ci_contains_word_end_sensitive(hay: &str, needle: &str) -> bool {
    let hb = hay.as_bytes();
    let mut from = 0;
    while let Some(at) = hay[from..].find(needle) {
        let end = from + at + needle.len();
        let boundary = end >= hb.len() || !(hb[end].is_ascii_alphanumeric() || hb[end] == b'_');
        if boundary {
            return true;
        }
        from = from + at + 1;
    }
    false
}

fn purpose_matches(p: &Purpose, sel: &str) -> bool {
    if p.prefix {
        return p.needles.iter().any(|n| sel.starts_with(n));
    }
    p.needles.iter().any(|n| ci_contains(sel, n))
        || p.word_end.iter().any(|n| ci_contains_word_end(sel, n))
}

/// `loudnessOf`, transliterated from `agent/scripts/lib/design-sweep.mjs` — the function the probe
/// itself evaluates, and the `.mjs` imports rather than copies. THE BANDS ARE MEASURED, NOT CHOSEN.
///
/// HSL saturation is `d / (1 - |2l - 1|)`, which is the form the probe has always used — and the one
/// the probe's own gate checks.
fn loudness_of(r: u32, g: u32, b: u32) -> (f64, f64, bool) {
    let (rf, gf, bf) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let mx = rf.max(gf).max(bf);
    let mn = rf.min(gf).min(bf);
    let l = (mx + mn) / 2.0;
    let d = mx - mn;
    let sat = if d == 0.0 {
        0.0
    } else {
        d / (1.0 - (2.0 * l - 1.0).abs())
    };
    let loud = sat.is_finite() && l.is_finite() && sat >= 0.35 && (0.2..=0.9).contains(&l);
    (sat, l, loud)
}

/// The sheets this judges: the BUILT panel sheet, and the console's source sheets concatenated.
fn sheets() -> Vec<(&'static str, String)> {
    let panel = fs::read_to_string(repo().join("agent/resources/panel/panel.css"))
        .expect("cannot read the built panel sheet");
    let dir = repo().join("gateway/ui/src/styles");
    let mut files: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("css"))
        .collect();
    files.sort();
    let console = files
        .iter()
        .map(|p| {
            fs::read_to_string(p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
        })
        .collect::<Vec<_>>()
        .join("\n");
    vec![("panel", panel), ("console", console)]
}

#[test]
fn state_colour_is_only_where_state_is() {
    let sheets = sheets();
    let mut failures: Vec<String> = Vec::new();
    let mut judged = 0usize;

    for (name, css) in &sheets {
        for r in rules(css) {
            if !has_state_token(&r.body) {
                continue;
            }
            judged += 1;
            if PURPOSES.iter().any(|p| purpose_matches(p, &r.sel)) {
                continue;
            }
            if DESTRUCTIVE_STATE.iter().any(|s| r.sel.contains(s))
                && DESTRUCTIVE_NAMES.iter().any(|n| ci_contains(&r.sel, n))
            {
                continue;
            }
            failures.push(format!(
                "{name}: {} paints with a state colour, and matches no purpose on the list",
                r.sel
            ));
        }
    }

    // A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN. The two sheets carried 163 such rules when this
    // was written, and the floor is the `.mjs`'s own 100.
    assert!(
        judged >= 100,
        "state-colour-check: FAILED — judged {judged} rules, which is too few to be reading both sheets"
    );

    if !failures.is_empty() {
        let mut msg = String::from(
            "state-colour-check: FAILED — state colour on something that is not state:",
        );
        for f in &failures {
            msg.push_str(&format!("\n  {f}"));
        }
        msg.push_str(
            "\n\n  Either this is state and it belongs on the list in agent/tests/state_colour.rs with its\n  \
             reason, or the chrome is wearing a colour that means something it does not mean.",
        );
        panic!("{msg}");
    }

    // ── A STATUS SURFACE IS A TINT, JUDGED WITH THE PROBE'S OWN RULE (round 78) ──────────────────
    //
    // The rendered axis found the dark info chip as a second focal point on the Users page:
    // `--info-bg: #1a3a5c`, saturation 0.559. The light chip is a pale tint the same rule skips by
    // design (lightness above 0.9), and `--success-bg` was already a quiet tint — so one token had
    // missed what its siblings got, and only the one page that renders that badge could see it.
    let mut loud: Vec<String> = Vec::new();
    for (name, css) in &sheets {
        let parts: Vec<&str> = css.split("data-theme=\"dark\"").collect();
        let light = parts[0];
        let dark_owned = parts[1..].join("");
        let dark: &str = if dark_owned.is_empty() {
            css
        } else {
            &dark_owned
        };
        for (theme, text) in [("light", light), ("dark", dark)] {
            for token in SURFACE_TOKENS {
                // The LAST declaration in the block wins, which is what the cascade does.
                let mut last: Option<String> = None;
                let mut from = 0;
                while let Some(at) = text[from..].find(token) {
                    let rest = &text[from + at + token.len()..];
                    let after = rest.trim_start_matches([' ', '\t']);
                    if let Some(after) = after.strip_prefix(':') {
                        let after = after.trim_start_matches([' ', '\t']);
                        if let Some(hex) = after.strip_prefix('#') {
                            let digits: String =
                                hex.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
                            if digits.len() == 6 {
                                last = Some(format!("#{digits}"));
                            }
                        }
                    }
                    from = from + at + token.len();
                }
                let Some(hex) = last else { continue };
                let n = |a: usize, b: usize| u32::from_str_radix(&hex[a..b], 16).unwrap_or(0);
                let (sat, l, is_loud) = loudness_of(n(1, 3), n(3, 5), n(5, 7));
                if is_loud {
                    loud.push(format!(
                        "{name} {theme} {token} = {hex} — saturation {sat:.2}, lightness {l:.2}: a saturated block on chrome"
                    ));
                }
            }
        }
    }
    if !loud.is_empty() {
        let mut msg = format!(
            "state-colour-check: FAILED — {} state surface(s) are LOUD, and a status surface is a tint",
            loud.len()
        );
        for f in &loud {
            msg.push_str(&format!("\n  {f}"));
        }
        msg.push_str(
            "\n\n  The rule: saturation >= 0.35 with lightness between 0.2 and 0.9 is what an operator reads as\n  \
             something shouting. Colour belongs to the state LAYER (a mark, an ink, a lane) — a surface a badge\n  \
             sits on is chrome, and chrome is neutral. Pick a tint of the same hue, as --success-bg is.",
        );
        panic!("{msg}");
    }

    println!(
        "state-colour-check: ok — {judged} uses of a state colour across both sheets, every one on a surface that carries state"
    );
}
