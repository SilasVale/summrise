//! THE CONSOLE'S STATE DOTS MUST DIFFER IN SHAPE, AND EVERY SHAPE MUST BE LEGIBLE.
//!
//! `scripts/test/console-marks-check.mjs` (229 lines), transliterated. The tenth gate to move into
//! `agent/tests/*.rs`, and the second whose subject is a STYLESHEET (`motion_check.rs` was the
//! first) — with a WCAG contrast computation on top, which is the part that has to be exact.
//!
//! WHY IT EXISTS (round 11 of the standing goal). The console carried three `.sig-dot` states — ok,
//! err, off — as THREE IDENTICAL 7px CIRCLES distinguished only by fill colour. That is the
//! arrangement the PANEL retired in round 245, still live on the other front end, and the panel's own
//! guards cannot see it: they read the panel's sheet. Measured on this one, the "off" dot was also
//! **2.46:1 on `--bg` in the light theme** — under the 3:1 a graphic needs — so it was not merely
//! shapeless, it was barely there. The word beside each dot is what kept the row readable, which is
//! exactly why nobody noticed: the row reads fine and the DOT says nothing.
//!
//! TWO RULES, both the panel's own, restated where the console can be held to them:
//!
//!   1. **A SHAPE PER STATE.** With the colour stripped out, two states that draw the same silhouette
//!      are one state. The signature is (border-radius, border-style, transform, kind) — the
//!      properties that make a circle a ring, a diamond or a dash, and the ones a colour-blind reader
//!      depends on.
//!   2. **LEGIBLE INK, BOTH THEMES.** A dot is a graphic: 3:1 against the surface it lands on.
//!      `--text-faint` failed this in light at 2.46, which is why the fix moved the `off` dot to
//!      `--text-muted` (4.63 light / 6.58 dark).
//!
//! IT READS THE SOURCE SHEETS, because the console's `dist/` is a build artifact that is pruned and
//! not committed — unlike the panel, whose build output IS the tracked mirror. The styles are plain
//! CSS, so what is concatenated here is what the bundler emits.
//!
//! ── MIGRATION-TIME EQUIVALENCE, MEASURED (both implementations, one tree, 2026-09-29) ──────────
//!
//! The `.mjs` was restored from `main`, both were run over the same tree, and `cmp` was applied to
//! the two streams. SIX CASES, EVERY ONE BYTE-IDENTICAL:
//!
//!   case                                js / rust            body bytes   the sheet mutated
//!   ────────────────────────────────────────────────────────────────────────────────────────────
//!   clean tree                          accept / accept      125          (none)
//!   A  `off`'s ink → `--text-faint`     exit 1 / exit 101    121          the failing ink, 2.46:1
//!   B  `off` → a plain filled circle    exit 1 / exit 101    167          two states, one silhouette
//!   C  `.dot.err` loses `box-shadow: none` exit 1 / exit 101 224          a FILL inside a RING
//!   D  a state's rule renamed away      exit 1 / exit 101     87          a missing state
//!   E  `off`'s ink → `--text`           accept / accept      125          the case that must NOT bite
//!
//! **AND E IS THE ONE THAT MATTERS MOST**: a state's ink moving to a STRONGER token leaves the shape
//! alone and raises the ratio, so a gate that refused it would be refusing a fix — and the console's
//! own history is that fix (`--text-faint` 2.46 → `--text-muted` 4.63 light / 6.58 dark).
//!
//! **AND ONE OF THESE CASES FOUND A REAL DIFFERENCE IN THE PORT.** The first Rust version trimmed the
//! border colour it read out of `borderDecl`, and the JavaScript does not: `borderDecl` is
//! `[val("border"), val("border-style")].join(" ")`, so a rule that sets `border` and no
//! `border-style` produces a string with a TRAILING SPACE that `[^;]+$` captures. The ratio is the
//! same either way (both `resolve` and `parseColour` trim); the MESSAGE is not — `var(--text-faint)
//!  on` against `var(--text-faint) on` — and case A is what showed it. A port that "cleaned up" the
//! trailing space would have been a port that quietly changed a gate's output.
//!
//! MUTATION: give two console signal states the same silhouette (put `off` back to a plain circle),
//!           or put the failing ink back (`--text-faint` for `off`), or make a mark a FILL inside a
//!           RING.
//! RESULT:   exit 101 either way: ".sig-dot.off draws the same shape as .sig-dot.ok (50%|none|none|
//!           solid) — strip the colour and they are one state"; ".sig-dot.off [light] var(--text-
//!           faint) on --bg = 2.46 (a mark is a graphic; 3 is the bar)"; and "… is a FILL inside a
//!           RING — the vocabulary is solid / ring / halo / empty, and a mark that is two of them is
//!           neither". **THE FIX IS IN EACH MESSAGE**: the first names the shape both states draw,
//!           the second names the token, the ratio and the bar, and the third names the four kinds.

mod common;

use common::repo;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

/// A family of marks: its base selector and the states that have rules of their own.
struct Mark {
    what: &'static str,
    base: &'static str,
    states: &'static [&'static str],
}

/// EVERY MARK THE CONSOLE DRAWS A STATE WITH. `.sig-dot` was the first family and the only one this
/// check knew; the LED families below kept the pre-round-11 arrangement — identical circles told
/// apart by fill colour — for thirteen rounds because nothing looked at them (round 24). A family
/// names its base selector and each state's modifier; the empty string is the base rule itself.
///
/// THE STATES A FAMILY LISTS ARE THE STATES THAT HAVE RULES OF THEIR OWN; a class that resolves to
/// the base IS the base. That is why `.dot` lists two and not four (round 44: `online` had no
/// producer and `offline` restated the base, so both were pruned) and why `.dev-led` has no `off`
/// (round 76, learned by trying it: the DOM renders `dev-led off` and the BASE paints it, so adding
/// `off` reports the base and the arm as "one state" the moment the arm is removed — which it was,
/// because the arm was the defect).
const MARKS: [Mark; 5] = [
    Mark {
        what: "device signal",
        base: ".sig-dot",
        states: &["ok", "err", "off"],
    },
    Mark {
        what: "key LED",
        base: ".ov-keyled",
        states: &["", "on"],
    },
    Mark {
        what: "connection dot",
        base: ".dot",
        states: &["ok", "err"],
    },
    Mark {
        what: "device LED",
        base: ".dev-led",
        states: &["", "on"],
    },
    Mark {
        what: "mini LED",
        base: ".dev-mini-led",
        states: &["", "on"],
    },
];

/// The console's stylesheets, concatenated the way the `.mjs` did it — with `\n` between files, and
/// SORTED (the `.mjs` used `readdirSync` order, which is unspecified; the verdict is order-independent
/// because each lookup is anchored on a selector, but a message that lists failures would not be).
fn sheet() -> String {
    let dir = repo().join("gateway/ui/src/styles");
    let mut files: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("css"))
        .collect();
    files.sort();
    let joined = files
        .iter()
        .map(|p| {
            fs::read_to_string(p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
        })
        .collect::<Vec<_>>()
        .join("\n");
    strip_comments(&joined)
}

/// `\/\*[\s\S]*?\*\/` — comments out, non-greedy.
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

/// The body of ONE rule, matched at a SELECTOR BOUNDARY.
///
/// The first version of the `.mjs` was a substring match, so `blockOf(".dot")` found `.badge .dot {`
/// — an earlier rule that happens to CONTAIN the selector — and read its body: no box-shadow there,
/// so the mark's ink came out as `transparent` and the check reported a dot it could not measure. A
/// boundary is the start of the sheet or a `}`/newline that ended the previous rule.
fn block_of(sheet: &str, sel: &str) -> Option<String> {
    let s: Vec<char> = sheet.chars().collect();
    let selc: Vec<char> = sel.chars().collect();
    let mut i = 0;
    while i + selc.len() <= s.len() {
        // The boundary: start of the sheet, `}`, or a newline — then optional whitespace.
        let boundary_ok = i == 0 || {
            // walk BACK over whitespace to the boundary character
            let mut b = i;
            while b > 0 && s[b - 1].is_whitespace() && s[b - 1] != '\n' {
                b -= 1;
            }
            b == 0 || s[b - 1] == '}' || s[b - 1] == '\n'
        };
        if boundary_ok && s[i..i + selc.len()] == selc[..] {
            let mut k = i + selc.len();
            while k < s.len() && s[k].is_whitespace() {
                k += 1;
            }
            if k < s.len() && s[k] == '{' {
                let start = k + 1;
                let mut e = start;
                while e < s.len() && s[e] != '}' && s[e] != '{' {
                    e += 1;
                }
                if e < s.len() && s[e] == '}' {
                    return Some(s[start..e].iter().collect());
                }
            }
        }
        i += 1;
    }
    None
}

/// `selector\s*\{([\s\S]*?)\n\}` — the token block, which the `.mjs` matched with a NON-GREEDY body
/// ending at a newline followed by `}`.
fn tokens_in(sheet: &str, selector: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(at) = sheet.find(selector) else {
        return out;
    };
    let rest = &sheet[at + selector.len()..];
    let Some(brace) = rest.find('{') else {
        return out;
    };
    // `\s*` between the selector and the brace: anything else is not this rule.
    if !rest[..brace].trim().is_empty() {
        return out;
    }
    let body_start = brace + 1;
    let Some(end) = rest[body_start..].find("\n}") else {
        return out;
    };
    let body = &rest[body_start..body_start + end];
    // `(--[\w-]+)\s*:\s*([^;]+);`
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
            let name_start = i;
            let mut k = i + 2;
            while k < chars.len()
                && (chars[k].is_alphanumeric() || chars[k] == '_' || chars[k] == '-')
            {
                k += 1;
            }
            let name: String = chars[name_start..k].iter().collect();
            while k < chars.len() && chars[k].is_whitespace() {
                k += 1;
            }
            if k < chars.len() && chars[k] == ':' {
                k += 1;
                while k < chars.len() && chars[k].is_whitespace() {
                    k += 1;
                }
                let val_start = k;
                while k < chars.len() && chars[k] != ';' {
                    k += 1;
                }
                if k < chars.len() {
                    let val: String = chars[val_start..k].iter().collect();
                    out.insert(name, val.trim().to_string());
                    i = k + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

/// `var(--token, fallback?)` resolved, recursively, with the `.mjs`'s depth limit of 6.
fn resolve(value: &str, tokens: &BTreeMap<String, String>, depth: usize) -> Option<String> {
    if depth > 6 {
        return None;
    }
    let v = value.trim();
    if let Some(inner) = v.strip_prefix("var(").and_then(|x| x.strip_suffix(')')) {
        // `--[\w-]+` then an optional `, fallback` — and the fallback is everything after the FIRST
        // comma, which is what `([^)]+)` captures inside the parens the strip already removed.
        let mut it = inner.splitn(2, ',');
        let name = it.next().unwrap_or("").trim();
        let fallback = it.next().map(|x| x.trim().to_string());
        if !name.starts_with("--") {
            return Some(v.to_string());
        }
        if let Some(next) = tokens.get(name) {
            return resolve(next, tokens, depth + 1);
        }
        return match fallback {
            Some(f) => resolve(&f, tokens, depth + 1),
            None => None,
        };
    }
    Some(v.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Colour {
    r: f64,
    g: f64,
    b: f64,
    #[allow(dead_code)]
    a: f64,
}

/// `#rrggbb` or `rgb()/rgba()` with space, comma or slash separators.
fn parse_colour(value: &str) -> Option<Colour> {
    let v = value.trim();
    if let Some(hex) = v.strip_prefix('#') {
        if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            let n = i64::from_str_radix(hex, 16).ok()?;
            return Some(Colour {
                r: ((n >> 16) & 255) as f64,
                g: ((n >> 8) & 255) as f64,
                b: (n & 255) as f64,
                a: 1.0,
            });
        }
        return None;
    }
    let inner = v
        .strip_prefix("rgba(")
        .or_else(|| v.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<f64> = inner
        .split(|c: char| c.is_whitespace() || c == ',' || c == '/')
        .filter(|x| !x.is_empty())
        .filter_map(|x| x.parse::<f64>().ok())
        .collect();
    // `parts.length >= 3 && parts.slice(0,3).every(x => !Number.isNaN(x))` — and the parse already
    // dropped the NaN-shaped pieces, so a run of three numbers is the same test.
    if parts.len() >= 3 {
        Some(Colour {
            r: parts[0],
            g: parts[1],
            b: parts[2],
            a: if parts.len() > 3 { parts[3] } else { 1.0 },
        })
    } else {
        None
    }
}

fn luminance(c: Colour) -> f64 {
    let f = |v: f64| {
        let s = v / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
}

fn contrast(a: Colour, b: Colour) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// `(?:^|[;{\s])name\s*:\s*([^;}]+)` — a declaration's value, from a block.
fn prop(name: &str, from: &str) -> String {
    let s: Vec<char> = from.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let mut i = 0;
    while i + n.len() <= s.len() {
        let boundary = i == 0 || matches!(s[i - 1], ';' | '{') || s[i - 1].is_whitespace();
        if boundary && s[i..i + n.len()] == n[..] {
            let mut k = i + n.len();
            while k < s.len() && s[k].is_whitespace() {
                k += 1;
            }
            if k < s.len() && s[k] == ':' {
                k += 1;
                while k < s.len() && s[k].is_whitespace() {
                    k += 1;
                }
                let start = k;
                while k < s.len() && s[k] != ';' && s[k] != '}' {
                    k += 1;
                }
                return s[start..k].iter().collect::<String>().trim().to_string();
            }
        }
        i += 1;
    }
    String::new()
}

/// `/(?:dashed|solid|dotted)\s+([^;]+)$/` — the border colour, ANCHORED AT THE END.
///
/// **THE CAPTURE IS NOT TRIMMED, AND THAT IS THE `.mjs`'s OWN BEHAVIOUR RATHER THAN AN OVERSIGHT.**
/// `borderDecl` is `[val("border"), val("border-style")].join(" ")`, so a rule that sets `border`
/// and no `border-style` produces a string with a TRAILING SPACE — and `[^;]+$` captures it. The
/// difference is invisible in the ratio (both `resolve` and `parseColour` trim) and visible in the
/// MESSAGE: the first port trimmed here and printed `var(--text-faint) on --bg`, one space, where the
/// JavaScript prints two. The differential caught it, which is what the differential is for.
fn border_colour(border_decl: &str) -> Option<String> {
    let s: Vec<char> = border_decl.chars().collect();
    let mut i = 0;
    while i < s.len() {
        for word in ["dashed", "solid", "dotted"] {
            if at(&s, i, word) {
                // `\s+`
                let mut k = i + word.len();
                let ws_start = k;
                while k < s.len() && s[k].is_whitespace() {
                    k += 1;
                }
                if k > ws_start {
                    let value: String = s[k..].iter().collect();
                    // `[^;]+$` — non-empty, no semicolon, to the END of the string.
                    if !value.is_empty() && !value.contains(';') {
                        return Some(value);
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// `/(var\(--[\w-]+\)|#[0-9a-f]{3,8}\b|rgba?\([^)]+\))/` — ANY colour-ish token in a shadow, wherever
/// it sits: `inset 0 0 0 1.5px var(--text-muted)` keeps its colour at the END, and the first version
/// of the `.mjs` looked for one at the start or after a space and found nothing — so a ring's ink
/// read as `transparent` and the check reported a mark it could not measure.
fn shadow_colour(shadow: &str) -> Option<String> {
    let s: Vec<char> = shadow.chars().collect();
    let mut i = 0;
    while i < s.len() {
        // `var(--x)`
        if at(&s, i, "var(--") {
            let mut k = i + 6;
            while k < s.len() && (s[k].is_alphanumeric() || s[k] == '_' || s[k] == '-') {
                k += 1;
            }
            if k < s.len() && s[k] == ')' {
                return Some(s[i..k + 1].iter().collect());
            }
        }
        // `#` + 3..8 hex
        if s[i] == '#' {
            let mut k = i + 1;
            while k < s.len() && s[k].is_ascii_hexdigit() {
                k += 1;
            }
            if (4..=9).contains(&(k - i)) {
                return Some(s[i..k].iter().collect());
            }
        }
        // `rgb(…)` / `rgba(…)`
        if at(&s, i, "rgb(") || at(&s, i, "rgba(") {
            let start = i;
            let mut k = i;
            while k < s.len() && s[k] != ')' {
                k += 1;
            }
            if k < s.len() {
                return Some(s[start..k + 1].iter().collect());
            }
        }
        i += 1;
    }
    None
}

fn at(hay: &[char], i: usize, needle: &str) -> bool {
    let n: Vec<char> = needle.chars().collect();
    i + n.len() <= hay.len() && hay[i..i + n.len()] == n[..]
}

/// `/(?:dashed|dotted|double|solid|none)/` — the first border-style word in the declaration.
fn border_style_word(border_decl: &str) -> &'static str {
    for word in ["dashed", "dotted", "double", "solid", "none"] {
        if border_decl.contains(word) {
            return match word {
                "dashed" => "dashed",
                "dotted" => "dotted",
                "double" => "double",
                "solid" => "solid",
                _ => "none",
            };
        }
    }
    "none"
}

#[test]
fn every_state_mark_has_its_own_shape_and_legible_ink() {
    let sheet = sheet();
    let light = tokens_in(&sheet, ":root");
    let mut dark = light.clone();
    for (k, v) in tokens_in(&sheet, "[data-theme=\"dark\"]") {
        dark.insert(k, v);
    }

    let mut failures: Vec<String> = Vec::new();
    let mut signatures: BTreeMap<String, String> = BTreeMap::new();

    for family in &MARKS {
        // WITHIN A FAMILY, never across: a solid mark with a halo means "on" in more than one place
        // on purpose — the vocabulary is SHARED, which is the whole point of a mark language. The
        // first version compared every signature with every other and reported two families for
        // agreeing.
        let mut family_shapes: Vec<(String, String)> = Vec::new();
        for state in family.states {
            let sel = if state.is_empty() {
                family.base.to_string()
            } else {
                format!("{}.{}", family.base, state)
            };
            let Some(block) = block_of(&sheet, &sel) else {
                failures.push(format!(
                    "{sel} is missing from the console sheet — {} has no {} state to draw",
                    family.what,
                    if state.is_empty() { "base" } else { state }
                ));
                continue;
            };
            let base = block_of(&sheet, family.base).unwrap_or_default();
            let val = |name: &str| {
                let v = prop(name, &block);
                if v.is_empty() {
                    prop(name, &base)
                } else {
                    v
                }
            };

            let bg = {
                let b = val("background");
                if b.is_empty() {
                    let c = val("background-color");
                    if c.is_empty() {
                        "transparent".to_string()
                    } else {
                        c
                    }
                } else {
                    b
                }
            };
            let shadow = val("box-shadow");
            // THE FILL KIND IS PART OF THE SHAPE: a solid mark, a ring (an inset shadow or a border
            // with no fill), and a halo (a fill plus an outer shadow) are three different things to
            // look at even when the geometry matches.
            let filled = !bg.is_empty() && !bg.contains("transparent") && !bg.contains("none");
            let inset = shadow.contains("inset");
            let outer = !shadow.is_empty() && shadow != "none" && !shadow.contains("inset");
            // A FILL AND A RING AT ONCE IS ITS OWN KIND, and reducing it to "ring" is how round 44
            // found a real defect with a mutation that did NOT bite: `.dot.err` set a fill and
            // inherited the base's inset ring, so a failing channel drew a red square INSIDE a grey
            // ring — and the signature called it a ring, distinct from `.dot.ok`'s solid, so it
            // passed. A kind that hides one of the two channels is not a silhouette.
            let kind = if inset && filled {
                "ring+fill"
            } else if inset {
                "ring"
            } else if filled && outer {
                "halo"
            } else if filled {
                "solid"
            } else {
                "empty"
            };

            if kind == "ring+fill" {
                failures.push(format!(
                    "{sel} is a FILL inside a RING — the vocabulary is solid / ring / halo / empty, and a mark that is two of them is neither. Add box-shadow: none for a fill, or drop the background for a ring"
                ));
            }

            let border_decl = format!("{} {}", val("border"), val("border-style"));
            let shape = format!(
                "{}|{}|{}|{}",
                {
                    let r = val("border-radius");
                    if r.is_empty() {
                        "0".to_string()
                    } else {
                        r
                    }
                },
                border_style_word(&border_decl),
                {
                    let t = val("transform");
                    if t.is_empty() {
                        "none".to_string()
                    } else {
                        t
                    }
                },
                kind
            );
            signatures.insert(sel.clone(), shape.clone());
            family_shapes.push((sel.clone(), shape));

            // the ink: the fill, the border colour, or the shadow's colour
            let border_c = border_colour(&border_decl);
            let shadow_c = shadow_colour(&shadow);
            let ink_raw = if filled {
                bg.clone()
            } else if inset && shadow_c.is_some() {
                shadow_c.unwrap()
            } else {
                border_c.unwrap_or(bg.clone())
            };

            for (theme, tokens) in [("light", &light), ("dark", &dark)] {
                let ink = parse_colour(&resolve(&ink_raw, tokens, 0).unwrap_or_default());
                let surface = parse_colour(
                    &resolve(
                        tokens.get("--bg").map(|s| s.as_str()).unwrap_or(""),
                        tokens,
                        0,
                    )
                    .unwrap_or_default(),
                );
                let (Some(ink), Some(surface)) = (ink, surface) else {
                    failures.push(format!(
                        "{sel} [{theme}]: could not resolve {ink_raw} and --bg (a mark that cannot be measured is not a passing mark)"
                    ));
                    continue;
                };
                let ratio = contrast(ink, surface);
                if ratio < 3.0 {
                    failures.push(format!(
                        "{sel} [{theme}] {ink_raw} on --bg = {ratio:.2} (a mark is a graphic; 3 is the bar)"
                    ));
                }
            }
        }
        let _ = family_shapes;
    }

    // TWO STATES, ONE SILHOUETTE, ONE STATE. With the colour gone they are indistinguishable, which
    // is the rule the whole mark language exists for.
    for family in &MARKS {
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        for state in family.states {
            let sel = if state.is_empty() {
                family.base.to_string()
            } else {
                format!("{}.{}", family.base, state)
            };
            let Some(shape) = signatures.get(&sel) else {
                continue;
            };
            if let Some(other) = seen.get(shape) {
                failures.push(format!(
                    "within {}: {sel} draws the same shape as {other} ({shape}) — strip the colour and they are one state",
                    family.what
                ));
            } else {
                seen.insert(shape.clone(), sel.clone());
            }
        }
    }

    // A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN.
    let expected: usize = MARKS.iter().map(|m| m.states.len()).sum();
    assert!(
        signatures.len() >= expected && light.len() >= 10,
        "console-marks-check: FAILED — read {}/{expected} marks and {} tokens, so this proves nothing",
        signatures.len(),
        light.len()
    );

    if !failures.is_empty() {
        let report = failures
            .iter()
            .map(|f| format!("  {f}"))
            .collect::<Vec<_>>()
            .join("\n");
        panic!("console-marks-check: FAILED\n{report}");
    }

    let distinct: BTreeSet<&String> = signatures.values().collect();
    println!(
        "console-marks-check: ok — {} state marks across {} families, {} distinct signatures, every ink >= 3:1 on --bg in both themes",
        signatures.len(),
        MARKS.len(),
        distinct.len()
    );
}
