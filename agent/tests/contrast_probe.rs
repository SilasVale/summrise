//! `scripts/test/contrast-probe-check.mjs`, MINUS THE FOUR ASSERTIONS ABOUT THE EMITTED PROBE.
//!
//! WHAT THIS GATE IS FOR, in its own header: four rounds of contrast sweeps used an ad-hoc snippet
//! retyped each time, and it was wrong twice. Both defects are assertions here, with the numbers they
//! cost:
//!
//!   1. **TRANSLUCENT BACKGROUNDS.** Reading `rgba(255,255,255,0.07)` as if it were white reported 2.51
//!      for text that actually sits on a composited `rgb(44,45,49)` and measures 5.49 — twenty of fifty
//!      findings were this.
//!   2. **A SKIP RULE THAT DISABLED THE SWEEP.** Skipping anything with a `background-image` ancestor
//!      skipped EVERY node in the panel and reported `checked=0, underAA=0`, which reads exactly like a
//!      pass.
//!
//! # WHY THE `.mjs` STILL EXISTS, AND WHY IT IS NOW FOUR ASSERTIONS
//!
//! `PROBE_SOURCE` is the probe the DESIGN SWEEPS INJECT INTO A BROWSER, and the plan's carve-out list
//! keeps that JavaScript: `browser_run_script` takes a JS file, and the measurement runs in the DOM. Its
//! four assertions — that the source is syntactically valid, that it COMPILES as the artifact it is, that
//! it keeps its regex escapes, and that it carries these functions rather than a paraphrase — are about
//! that artifact and cannot move until the probe itself is not JavaScript. They are named in the commit
//! and in that file's header, which is the plan's rule for a gate that cannot follow its subject.
//!
//! # THE COLOUR CORE IS A SECOND COPY, AND IT SAYS SO
//!
//! `parse_colour`/`contrast_ratio` are also in `index/landing/src/colour.rs` (the landing's own checks
//! need them and the two crates cannot depend on each other). This one is held to the shipping
//! JavaScript by the assertions below — the gate's own numbers, not a paraphrase of them — and the day
//! either copy moves it must name the other.

use std::collections::BTreeSet;

/// A parsed colour. The components are `f64` because `color(srgb …)` carries fractions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colour {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Colour {
    fn rgb(r: f64, g: f64, b: f64) -> Self {
        Colour { r, g, b, a: 1.0 }
    }
}

/// `parseColour(c)`. The two rules the JavaScript records are load-bearing: HEX FIRST (the numeric
/// scrape reads `#f4f4f5` as 4, 4 and 5), and `color(srgb …)` scaled only when EVERY component fits 0-1.
pub fn parse_colour(c: &str) -> Option<Colour> {
    let s = c.trim();
    if let Some(digits) = s.strip_prefix('#') {
        if (3..=8).contains(&digits.len()) && digits.chars().all(|d| d.is_ascii_hexdigit()) {
            let chars: Vec<char> = digits.chars().collect();
            let hex = |d: char| d.to_digit(16).map(f64::from);
            let expand = |d: char| hex(d).map(|v| v * 16.0 + v);
            let pair = |a: char, b: char| match (hex(a), hex(b)) {
                (Some(a), Some(b)) => Some(a * 16.0 + b),
                _ => None,
            };
            return match digits.len() {
                3 | 4 => Some(Colour {
                    r: expand(chars[0])?,
                    g: expand(chars[1])?,
                    b: expand(chars[2])?,
                    a: if digits.len() == 4 {
                        expand(chars[3])? / 255.0
                    } else {
                        1.0
                    },
                }),
                6 | 8 => Some(Colour {
                    r: pair(chars[0], chars[1])?,
                    g: pair(chars[2], chars[3])?,
                    b: pair(chars[4], chars[5])?,
                    a: if digits.len() == 8 {
                        pair(chars[6], chars[7])? / 255.0
                    } else {
                        1.0
                    },
                }),
                _ => None,
            };
        }
    }
    let srgb = s.starts_with("color(") && s["color(".len()..].trim_start().starts_with("srgb");
    let numbers = scrape_numbers(s);
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

/// WCAG relative-luminance contrast ratio, rounded to 2dp the way `toFixed(2)` does it.
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

/// Composite a stack of layers, INNERMOST LAST, over `base`. Order matters and is the thing both
/// defects got wrong: the list runs from the element upward, so it is applied from the end backwards.
pub fn composite_stack(stack: &[Colour], base: Colour) -> Colour {
    let mut out = base;
    for c in stack.iter().rev() {
        out = Colour {
            r: c.r * c.a + out.r * (1.0 - c.a),
            g: c.g * c.a + out.g * (1.0 - c.a),
            b: c.b * c.a + out.b * (1.0 - c.a),
            a: 1.0,
        };
    }
    out
}

/// The WCAG AA bar for TEXT at this size/weight. 18px bold is NOT large — the boundary is 18.66, and
/// rounding it to 18 would let a real failure through.
pub fn aa_threshold(font_size: f64, font_weight: f64) -> f64 {
    let large = font_size >= 24.0 || (font_size >= 18.66 && font_weight >= 700.0);
    if large {
        3.0
    } else {
        4.5
    }
}

/// One row of a sweep's report, in the fields these rules read.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub cr: Option<f64>,
    pub inactive: bool,
    pub need: Option<f64>,
    pub size: f64,
    pub weight: f64,
    pub kind: String,
    pub paint: String,
    pub surface: String,
}

impl Row {
    fn new(cr: Option<f64>) -> Self {
        Row {
            cr,
            inactive: false,
            need: None,
            size: 12.0,
            weight: 400.0,
            kind: "text".to_string(),
            paint: String::new(),
            surface: String::new(),
        }
    }
}

/// `failures(rows)` — each row's OWN bar, never a flat 4.5.
pub fn failures(rows: &[Row]) -> Vec<Row> {
    rows.iter()
        .filter(|r| {
            r.cr.is_some_and(|cr| {
                !r.inactive && cr < r.need.unwrap_or_else(|| aa_threshold(r.size, r.weight))
            })
        })
        .cloned()
        .collect()
}

/// `inactive(rows)` — WCAG 1.4.3 exempts inactive UI components, so they are counted separately.
pub fn inactive(rows: &[Row]) -> Vec<Row> {
    rows.iter()
        .filter(|r| r.inactive && r.cr.is_some())
        .cloned()
        .collect()
}

/// `unmeasurable(rows)` — a gradient row must not vanish: a sweep that measured nothing reads as a
/// clean pass.
pub fn unmeasurable(rows: &[Row]) -> Vec<Row> {
    rows.iter().filter(|r| r.cr.is_none()).cloned().collect()
}

/// `graphics(rows)`.
pub fn graphics(rows: &[Row]) -> Vec<Row> {
    rows.iter()
        .filter(|r| r.kind == "graphic")
        .cloned()
        .collect()
}

/// `gradientStops(image)` — the colour stops of a gradient, `transparent` included as alpha 0.
///
/// **ONE PASS, IN ORDER**, because the JavaScript is `matchAll(/(rgba?\([^)]*\)|transparent)/g)` and a
/// port that collected the `rgb()` forms and then the `transparent` ones would REORDER the stops. The
/// order is observable — the gate's own assertion reads `[1].a` — and the first version of this function
/// had exactly that bug: it passed the case the gate carries (which happens to have `rgba` first) and
/// would have failed `linear-gradient(transparent, rgb(0,0,0))`.
pub fn gradient_stops(image: &str) -> Vec<Colour> {
    let text = image.trim();
    if text.is_empty() || text == "none" {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < text.len() {
        let next_rgb = text[at..].find("rgb(").map(|i| at + i);
        let next_rgba = text[at..].find("rgba(").map(|i| at + i);
        let next_transparent = text[at..].find("transparent").map(|i| at + i);
        let next = [next_rgb, next_rgba, next_transparent]
            .into_iter()
            .flatten()
            .min();
        let Some(start) = next else { break };
        if next_transparent == Some(start) {
            out.push(Colour {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            });
            at = start + "transparent".len();
            continue;
        }
        let Some(close) = text[start..].find(')') else {
            break;
        };
        if let Some(c) = parse_colour(&text[start..start + close + 1]) {
            out.push(c);
        }
        at = start + close + 1;
    }
    out
}

/// `worstOverGradient(fg, stops, base)` — the LOWEST contrast against any stop, each composited over
/// `base`. The worst stop decides, never the best: the number reported is the one that can hurt somebody.
pub fn worst_over_gradient(fg: Colour, stops: &[Colour], base: Colour) -> Option<f64> {
    let mut worst: Option<f64> = None;
    for stop in stops {
        let bg = composite_stack(&[*stop], base);
        let ratio = contrast_ratio(fg, bg);
        if worst.is_none_or(|w| ratio < w) {
            worst = Some(ratio);
        }
    }
    worst
}

/// `svgRootPaints(rootFill, rootStroke, shapePaints)` — the paint an SVG root ACTUALLY puts on screen.
///
/// THE ROOT DRAWS NOTHING ITSELF: `fill` and `stroke` are INHERITED, so a root's value is evidence only
/// when a shape below it computes that same paint. The brand mark reports `rgb(0,0,0)` — the initial
/// value of an inherited property — while all three of its shapes override it, and reporting that as a
/// colour filed ten findings in CI that read "svg — painted rgb(0,0,0) (fill) … needs 3".
pub fn svg_root_paints(
    root_fill: &str,
    root_stroke: &str,
    shape_paints: &[&str],
) -> Vec<(Colour, String)> {
    let key = |c: Colour| {
        format!(
            "{},{},{},{}",
            c.r.round(),
            c.g.round(),
            c.b.round(),
            (c.a * 1000.0).round()
        )
    };
    let mut painted: BTreeSet<String> = BTreeSet::new();
    for raw in shape_paints {
        if let Some(c) = parse_colour(raw) {
            if c.a > 0.05 {
                painted.insert(key(c));
            }
        }
    }
    let mut out = Vec::new();
    for (raw, from) in [(root_fill, "fill"), (root_stroke, "stroke")] {
        let Some(c) = parse_colour(raw) else { continue };
        if c.a <= 0.05 || !painted.contains(&key(c)) {
            continue;
        }
        out.push((c, from.to_string()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **THE BACKTICK GUARD EXISTS BY VIRTUE OF AN IMPORT, SO THE IMPORT IS WHAT TO PIN.**
    ///
    /// `contrast-probe-check.mjs` says of itself: "NO SEPARATE BACKTICK CHECK, and the reason is worth
    /// keeping. I wrote one three times and every version had a wrong premise … Then I noticed the guard
    /// ALREADY EXISTS: **this file IMPORTS the module**, so a stray backtick inside the template makes the
    /// import throw a SyntaxError and the whole test file fails loudly. **A hand-rolled parser for a case the
    /// import already catches is a check that can only be wrong.**"
    ///
    /// **A CHECK THAT EXISTS BY ACCIDENT OF AN IMPORT IS A CHECK THAT CAN BE DELETED BY ACCIDENT.** Remove
    /// the import — or move `PROBE_SOURCE` to a module this file does not import — and the backtick guard
    /// disappears without a single test changing colour. This is what says so.
    #[test]
    fn the_javascript_still_imports_the_probe_source() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the repository root")
            .join("scripts/test/contrast-probe-check.mjs");
        let js =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            js.contains("PROBE_SOURCE"),
            "the .mjs no longer mentions PROBE_SOURCE at all"
        );
        let imports_it = js.lines().any(|l| {
            l.trim_start().starts_with("import ")
                && l.contains("PROBE_SOURCE")
                && l.contains("contrast-probe.mjs")
        });
        assert!(
            imports_it,
            "scripts/test/contrast-probe-check.mjs no longer IMPORTS PROBE_SOURCE from \
             agent/scripts/lib/contrast-probe.mjs — and that import IS its backtick guard: a stray backtick \
             inside the template makes the import throw a SyntaxError. Without it, a broken probe passes \
             silently. The file's own note explains why a hand-rolled replacement is worse."
        );
    }

    /// The gate's own assertions, ported one for one — these ARE the equivalence evidence, because the
    /// JavaScript version asserts exactly these numbers.
    #[test]
    fn a_translucent_white_over_a_dark_base_composites_and_is_not_white() {
        let bg = composite_stack(
            &[Colour {
                r: 255.0,
                g: 255.0,
                b: 255.0,
                a: 0.07,
            }],
            Colour::rgb(28.0, 29.0, 34.0),
        );
        assert!(
            bg.r > 40.0 && bg.r < 50.0,
            "expected a dark grey, got {bg:?}"
        );
        assert_eq!(contrast_ratio(Colour::rgb(162.0, 163.0, 172.0), bg), 5.49);
        assert_eq!(
            contrast_ratio(
                Colour::rgb(162.0, 163.0, 172.0),
                Colour::rgb(255.0, 255.0, 255.0)
            ),
            2.51
        );
    }

    #[test]
    fn the_stack_applies_innermost_last() {
        let over = composite_stack(
            &[
                Colour {
                    r: 255.0,
                    g: 255.0,
                    b: 255.0,
                    a: 0.5,
                },
                Colour {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
            ],
            Colour::rgb(255.0, 255.0, 255.0),
        );
        assert_eq!(over.r, 127.5);
        assert_eq!(over.g, 127.5);
        assert_eq!(over.b, 127.5);
        // An opaque layer ends the walk regardless of what is beneath.
        let ended = composite_stack(
            &[
                Colour {
                    r: 19.0,
                    g: 20.0,
                    b: 24.0,
                    a: 1.0,
                },
                Colour {
                    r: 255.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
            ],
            Colour::rgb(255.0, 255.0, 255.0),
        );
        assert_eq!((ended.r, ended.g, ended.b), (19.0, 20.0, 24.0));
    }

    #[test]
    fn wcag_ratios_match_the_reference_values() {
        assert_eq!(
            contrast_ratio(Colour::rgb(255.0, 255.0, 255.0), Colour::rgb(0.0, 0.0, 0.0)),
            21.0
        );
        assert_eq!(
            contrast_ratio(Colour::rgb(0.0, 0.0, 0.0), Colour::rgb(0.0, 0.0, 0.0)),
            1.0
        );
        // The two numbers this repo argues about, so they cannot be re-derived wrongly.
        assert_eq!(
            contrast_ratio(
                Colour::rgb(162.0, 161.0, 170.0),
                Colour::rgb(255.0, 255.0, 255.0)
            ),
            2.56
        );
        assert_eq!(
            contrast_ratio(
                Colour::rgb(82.0, 82.0, 91.0),
                Colour::rgb(255.0, 255.0, 255.0)
            ),
            7.73
        );
    }

    #[test]
    fn the_aa_bar_depends_on_size_and_weight() {
        assert_eq!(aa_threshold(12.0, 400.0), 4.5);
        assert_eq!(aa_threshold(24.0, 400.0), 3.0);
        assert_eq!(aa_threshold(18.66, 700.0), 3.0);
        assert_eq!(aa_threshold(18.0, 700.0), 4.5, "18px bold is NOT large");
    }

    #[test]
    fn colours_parse_in_every_form_the_app_emits() {
        assert_eq!(
            parse_colour("rgb(29, 29, 31)"),
            Some(Colour::rgb(29.0, 29.0, 31.0))
        );
        assert_eq!(
            parse_colour("rgba(255, 255, 255, 0.07)"),
            Some(Colour {
                r: 255.0,
                g: 255.0,
                b: 255.0,
                a: 0.07
            })
        );
        assert_eq!(parse_colour("transparent"), None);
        assert_eq!(parse_colour(""), None);
    }

    #[test]
    fn failures_use_each_rows_own_bar_and_exempt_inactive_controls() {
        let mut large = Row::new(Some(3.5));
        large.size = 30.0;
        large.need = Some(3.0);
        let mut body = Row::new(Some(3.5));
        body.need = Some(4.5);
        assert_eq!(failures(&[large, body.clone()]).len(), 1);
        assert_eq!(failures(&[body]).len(), 1);

        // AN INACTIVE CONTROL IS EXEMPT, NOT A FAILURE: the panel's disabled Start button measures a
        // truthful 2.1:1 through its opacity chain, which is a real reading of a control nobody can use.
        let mut off = Row::new(Some(2.1));
        off.inactive = true;
        off.need = Some(4.5);
        let mut on = Row::new(Some(2.1));
        on.need = Some(4.5);
        assert_eq!(failures(&[off.clone(), on.clone()]).len(), 1);
        assert_eq!(inactive(&[off, on]).len(), 1);
    }

    #[test]
    fn a_gradient_row_is_unmeasurable_not_a_failure_and_not_a_pass() {
        let gradient = Row::new(None);
        let mut real = Row::new(Some(2.0));
        real.need = Some(4.5);
        assert_eq!(failures(&[gradient.clone(), real.clone()]).len(), 1);
        assert_eq!(failures(&[gradient.clone(), real])[0].cr, Some(2.0));
        assert_eq!(unmeasurable(&[gradient]).len(), 1);
    }

    #[test]
    fn a_gradients_stops_are_read_and_an_unreadable_background_stays_unreadable() {
        let g2 =
            gradient_stops("linear-gradient(135deg, rgb(250, 250, 250) 0%, rgb(0, 0, 0) 100%)");
        assert_eq!(g2.len(), 2);
        assert_eq!(g2[0].r, 250.0);
        assert_eq!(
            gradient_stops("linear-gradient(rgba(255,255,255,0.5), transparent)")[1].a,
            0.0
        );
        assert_eq!(gradient_stops("url(\"x.png\")").len(), 0);
        assert_eq!(gradient_stops("none").len(), 0);
        // THE ORDER IS THE STOPS' ORDER, which the first version of this function got wrong by
        // collecting every `rgb()` form before every `transparent` one.
        let reversed = gradient_stops("linear-gradient(transparent, rgb(0, 0, 0))");
        assert_eq!(reversed.len(), 2);
        assert_eq!(reversed[0].a, 0.0, "the transparent stop comes FIRST");
        assert_eq!(reversed[1].r, 0.0);
        assert_eq!(reversed[1].a, 1.0);
    }

    #[test]
    fn the_worst_stop_decides_never_the_best() {
        let bw = gradient_stops("linear-gradient(rgb(255,255,255), rgb(0,0,0))");
        let base = Colour::rgb(255.0, 255.0, 255.0);
        assert_eq!(
            worst_over_gradient(Colour::rgb(255.0, 255.0, 255.0), &bw, base)
                .map(|v| format!("{v:.2}")),
            Some("1.00".to_string())
        );
        assert_eq!(
            worst_over_gradient(Colour::rgb(0.0, 0.0, 0.0), &bw, base).map(|v| format!("{v:.2}")),
            Some("1.00".to_string())
        );
        assert_eq!(
            worst_over_gradient(Colour::rgb(0.0, 0.0, 0.0), &[], base),
            None
        );
    }

    #[test]
    fn an_svg_roots_paint_is_reported_only_when_a_shape_computes_it() {
        // The panel's Icon: fill="none" stroke="currentColor", and every shape inherits exactly that.
        let icon = svg_root_paints(
            "none",
            "rgb(162, 163, 172)",
            &["none", "rgb(162, 163, 172)"],
        );
        assert_eq!(
            icon.iter().map(|(_, f)| f.as_str()).collect::<Vec<_>>(),
            vec!["stroke"]
        );
        assert_eq!(icon[0].0.r.round(), 162.0);
        // An icon that inherits a lane colour must produce a row, carrying THAT colour.
        let lane = svg_root_paints("none", "rgb(77, 171, 247)", &["none", "rgb(77, 171, 247)"]);
        assert_eq!(lane.len(), 1);
        assert_eq!(lane[0].0.b, 247.0);
        // The brand mark: the root reports rgb(0,0,0) and all three shapes override it, so NOTHING paints
        // black and nothing is reported.
        let brand = svg_root_paints(
            "rgb(0, 0, 0)",
            "none",
            &[
                "url(\"#summrise-sky\")",
                "rgb(255, 248, 225)",
                "rgb(255, 255, 255)",
            ],
        );
        assert!(brand.is_empty(), "{brand:?}");
        // ...and the row it used to produce was the 1.18 CI filed.
        assert_eq!(
            format!(
                "{:.2}",
                contrast_ratio(
                    parse_colour("rgb(0, 0, 0)").unwrap(),
                    parse_colour("rgb(23, 24, 29)").unwrap()
                )
            ),
            "1.18",
            "the removed finding's own number — if this moves, the probe's comment is stale"
        );
        // A root value a shape DOES inherit still counts, and an empty svg paints nothing at all.
        assert_eq!(
            svg_root_paints("rgb(191, 58, 10)", "none", &["rgb(191, 58, 10)"]).len(),
            1
        );
        assert!(svg_root_paints("rgb(0, 0, 0)", "none", &[]).is_empty());
    }

    #[test]
    fn color_srgb_components_are_zero_to_one_floats_not_channels() {
        let c = parse_colour("color(srgb 0.956863 0.956863 0.960784 / 0.88)").expect("a colour");
        assert!((c.r - 0.956863 * 255.0).abs() < 1e-9, "{c:?}");
        assert!((c.a - 0.88).abs() < 1e-9);
        // The scale is applied only when EVERY component fits 0-1.
        assert_eq!(parse_colour("color(srgb 2 2 2)").map(|c| c.r), Some(2.0));
    }

    #[test]
    fn hex_equals_its_rgb_form() {
        assert_eq!(parse_colour("#1d1d1f"), parse_colour("rgb(29, 29, 31)"));
        assert_eq!(
            contrast_ratio(
                parse_colour("#a2a1aa").unwrap(),
                parse_colour("#ffffff").unwrap()
            ),
            contrast_ratio(
                parse_colour("rgb(162, 161, 170)").unwrap(),
                parse_colour("rgb(255, 255, 255)").unwrap()
            )
        );
        // 3- and 4-digit hex expand, and 8-digit hex carries alpha.
        assert_eq!(parse_colour("#fff"), Some(Colour::rgb(255.0, 255.0, 255.0)));
        assert_eq!(
            parse_colour("#ffff"),
            Some(Colour {
                r: 255.0,
                g: 255.0,
                b: 255.0,
                a: 1.0
            })
        );
        assert!((parse_colour("#f4f4f5cc").unwrap().a - 204.0 / 255.0).abs() < 1e-9);
        // A malformed hex is null, not a colour.
        assert_eq!(parse_colour("#12345"), None);
        assert_eq!(parse_colour("#gggggg"), None);
        assert_eq!(parse_colour("#"), None);
    }

    #[test]
    fn a_graphic_row_carries_the_evidence_its_number_came_from() {
        let mut row = Row::new(Some(6.45));
        row.kind = "graphic".to_string();
        row.paint = "rgb(146, 64, 14) (border)".to_string();
        row.surface = "rgb(244, 244, 245)".to_string();
        row.size = 7.0;
        row.need = Some(3.0);
        assert_eq!(
            failures(&[row.clone()]).len(),
            0,
            "6.45 clears the 3:1 a graphic needs"
        );
        let mut low = row.clone();
        low.cr = Some(2.9);
        assert_eq!(failures(&[low]).len(), 1, "2.9 does not");
        assert_eq!(graphics(&[row.clone()]).len(), 1);
        assert!(row.paint.contains("rgb(") && row.surface.contains("rgb("));
        assert!(
            [
                "(border)",
                "(background)",
                "(fill)",
                "(stroke)",
                "(ring)",
                "(::"
            ]
            .iter()
            .any(|w| row.paint.contains(w)),
            "the painter must say WHERE the colour came from"
        );
    }
}
