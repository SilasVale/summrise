//! THE COLOUR CORE THE LANDING'S CHECKS NEED — `parseColour` and `contrastRatio`, ported from
//! `agent/scripts/lib/contrast-probe.mjs`.
//!
//! WHY THIS IS HERE RATHER THAN SHARED. That library is used by the panel's design sweeps as well as by
//! this page's checks, and the two live in different crates that cannot depend on each other — so this is
//! one of two copies, and the day the panel's copy moves it must NAME this one. Until then the corpus in
//! `fixtures/colour-corpus.json` is what holds this copy to the shipping JavaScript.
//!
//! TWO RULES IN THE JAVASCRIPT ARE LOAD-BEARING AND BOTH ARE HERE:
//!
//!   * **HEX IS PARSED FIRST, AND EXPLICITLY.** The numeric scrape below reads `#f4f4f5` as the numbers
//!     4, 4 and 5 — a real colour, silently wrong — and `#71717a` as nothing at all, because the `a` ends
//!     the run. It went unnoticed for rounds because the RENDERED sweep only ever sees `rgb()` forms.
//!   * **`color(srgb …)` IS NOT A 0-255 TRIPLE.** Its components are 0-1 floats, and the scrape turned the
//!     light rail's `color(srgb 0.956863 …)` into `rgb(1,1,1)` — judging every mark on the rail against a
//!     near-black surface, which produced six false findings in CI. The scale is applied only when EVERY
//!     component fits 0-1.

/// A parsed colour. The components are `f64` because `color(srgb …)` carries fractions, and the
/// JavaScript keeps them as they were written.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colour {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

/// `parseColour(c)` — `#rgb`/`#rrggbb`/`rgb()`/`rgba()`/`color(srgb …)` into a colour, or nothing.
///
/// **AND THE NUMERIC SCRAPE DOES NOT LOOK AT THE PREFIX**, which is the JavaScript's behaviour rather than
/// an accident of the port: `parseColour("foo 1 2 3")` is `rgb(1,2,3)`. The corpus carries such an input so
/// the quirk is pinned rather than discovered later.
pub fn parse_colour(c: &str) -> Option<Colour> {
    let s = c.trim();
    // ── hex, first and explicitly ────────────────────────────────────────────────────────────────
    if let Some(digits) = s.strip_prefix('#') {
        if (3..=8).contains(&digits.len()) && digits.chars().all(|d| d.is_ascii_hexdigit()) {
            let expand = |d: &str| -> Option<f64> {
                let d = if d.len() == 1 {
                    format!("{d}{d}")
                } else {
                    d.to_string()
                };
                u8::from_str_radix(&d, 16).ok().map(f64::from)
            };
            let byte = |a: &str, b: &str| -> Option<f64> {
                u8::from_str_radix(&format!("{a}{b}"), 16)
                    .ok()
                    .map(f64::from)
            };
            let chars: Vec<char> = digits.chars().collect();
            return match digits.len() {
                3 | 4 => Some(Colour {
                    r: expand(&chars[0].to_string())?,
                    g: expand(&chars[1].to_string())?,
                    b: expand(&chars[2].to_string())?,
                    a: if digits.len() == 4 {
                        expand(&chars[3].to_string())? / 255.0
                    } else {
                        1.0
                    },
                }),
                6 | 8 => Some(Colour {
                    r: byte(&chars[0].to_string(), &chars[1].to_string())?,
                    g: byte(&chars[2].to_string(), &chars[3].to_string())?,
                    b: byte(&chars[4].to_string(), &chars[5].to_string())?,
                    a: if digits.len() == 8 {
                        byte(&chars[6].to_string(), &chars[7].to_string())? / 255.0
                    } else {
                        1.0
                    },
                }),
                // 5 or 7 digits is not a colour.
                _ => None,
            };
        }
        // A `#` that is not a colour falls through to the scrape, as it does in the JavaScript.
    }
    let srgb = s.starts_with("color(") && s["color(".len()..].trim_start().starts_with("srgb");
    let numbers: Vec<f64> = scrape_numbers(s);
    if numbers.len() < 3 {
        return None;
    }
    let (r, g, b) = (numbers[0], numbers[1], numbers[2]);
    let scale = if srgb && r <= 1.0 && g <= 1.0 && b <= 1.0 {
        255.0
    } else {
        1.0
    };
    Some(Colour {
        r: r * scale,
        g: g * scale,
        b: b * scale,
        a: if numbers.len() > 3 { numbers[3] } else { 1.0 },
    })
}

/// `s.match(/[\d.]+/g)` — runs of digits and dots, each read as a number. A run that is only dots
/// (`".."`) is `NaN` in the JavaScript, and `NaN` is kept rather than dropped.
fn scrape_numbers(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut run = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            run.push(ch);
        } else if !run.is_empty() {
            out.push(run.parse::<f64>().unwrap_or(f64::NAN));
            run.clear();
        }
    }
    if !run.is_empty() {
        out.push(run.parse::<f64>().unwrap_or(f64::NAN));
    }
    out
}

/// WCAG relative-luminance contrast ratio, **ROUNDED TO 2dp THE WAY `toFixed(2)` DOES IT** — half away
/// from zero, then read back as a number. The rounding is part of the value: the gate's messages print it
/// and the design sweep's thresholds are compared against it.
pub fn contrast_ratio(fg: Colour, bg: Colour) -> f64 {
    let f = |v: f64| {
        let v = v / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let lum = |c: Colour| 0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b);
    let (a, b) = (lum(fg), lum(bg));
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    let x = (hi + 0.05) / (lo + 0.05);
    (x * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// THE SHIPPING JAVASCRIPT'S OWN ANSWERS, replayed: `index/landing/oracle.mjs colour` drove
    /// `agent/scripts/lib/contrast-probe.mjs` over 45 colour spellings and 225 pairs and wrote
    /// `fixtures/colour-corpus.json`.
    ///
    /// MUTATION: change one expected ratio in `fixtures/colour-corpus.json`.
    /// RESULT:   fails, naming the pair.
    #[test]
    fn the_typescript_colour_corpus() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/colour-corpus.json");
        let raw = std::fs::read_to_string(path).expect("the fixture is committed");
        let doc: serde_json::Value = serde_json::from_str(&raw).expect("the fixture is JSON");

        let parse = |v: &serde_json::Value| -> Option<Colour> {
            v.as_object().map(|o| Colour {
                r: o["r"].as_f64().unwrap_or(0.0),
                g: o["g"].as_f64().unwrap_or(0.0),
                b: o["b"].as_f64().unwrap_or(0.0),
                a: o["a"].as_f64().unwrap_or(1.0),
            })
        };

        let mut parsed = 0usize;
        let mut refused = 0usize;
        for case in doc["colours"].as_array().expect("a colours array") {
            let input = case["input"].as_str().expect("an input");
            let got = parse_colour(input);
            let want = parse(&case["parsed"]);
            match (got, want) {
                (Some(g), Some(w)) => {
                    assert!(
                        (g.r - w.r).abs() < 1e-9
                            && (g.g - w.g).abs() < 1e-9
                            && (g.b - w.b).abs() < 1e-9
                            && (g.a - w.a).abs() < 1e-9,
                        "parseColour({input:?}): got {g:?}, want {w:?}"
                    );
                    parsed += 1;
                }
                (None, None) => refused += 1,
                (got, want) => panic!("parseColour({input:?}): got {got:?}, want {want:?}"),
            }
        }

        let mut ratios = 0usize;
        let mut unmeasurable = 0usize;
        for case in doc["pairs"].as_array().expect("a pairs array") {
            let (fg, bg) = (
                case["fg"].as_str().expect("a foreground"),
                case["bg"].as_str().expect("a background"),
            );
            let (f, b) = (parse_colour(fg), parse_colour(bg));
            match case["ratio"].as_f64() {
                Some(want) => {
                    let (f, b) = (f.expect("parsed fg"), b.expect("parsed bg"));
                    let got = contrast_ratio(f, b);
                    assert!(
                        (got - want).abs() < 1e-9,
                        "contrastRatio({fg:?}, {bg:?}): got {got}, want {want}"
                    );
                    ratios += 1;
                }
                None => {
                    assert!(
                        f.is_none() || b.is_none(),
                        "{fg:?} on {bg:?} should be unmeasurable"
                    );
                    unmeasurable += 1;
                }
            }
        }
        assert!(
            parsed >= 15 && refused >= 5 && ratios >= 50 && unmeasurable >= 20,
            "the fixture moved: {parsed} parsed, {refused} refused, {ratios} ratios, {unmeasurable} unmeasurable"
        );
    }

    #[test]
    fn the_two_rules_the_javascript_records_are_kept() {
        // HEX IS PARSED FIRST, because the numeric scrape reads `#f4f4f5` as the numbers 4, 4 and 5.
        assert_eq!(
            parse_colour("#f4f4f5"),
            Some(Colour {
                r: 244.0,
                g: 244.0,
                b: 245.0,
                a: 1.0
            })
        );
        assert_eq!(parse_colour("#71717a").map(|c| c.r), Some(113.0));
        // AND `color(srgb …)` IS NOT A 0-255 TRIPLE: 0-1 floats, scaled only when EVERY component fits.
        assert_eq!(
            parse_colour("color(srgb 0.956863 0.956863 0.960784 / 0.88)").map(|c| c.r),
            Some(0.956863 * 255.0)
        );
        assert_eq!(parse_colour("color(srgb 2 2 2)").map(|c| c.r), Some(2.0));
        // The scrape ignores the prefix — the quirk, pinned rather than discovered later.
        assert_eq!(
            parse_colour("foo 1 2 3"),
            Some(Colour {
                r: 1.0,
                g: 2.0,
                b: 3.0,
                a: 1.0
            })
        );
        // Five or seven hex digits is not a colour, and neither is a `#` with no digits.
        assert_eq!(parse_colour("#12345"), None);
        assert_eq!(parse_colour("#1234567"), None);
        assert_eq!(parse_colour("#"), None);
    }
}
