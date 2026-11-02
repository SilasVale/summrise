//! The React page's visual contract, checked by Rust.
//!
//! The token sheet is a product decision — seven type sizes, one spacing rhythm, the legacy radii
//! and the dark surfaces restated — not a stylesheet someone edits by feel. The web app's own tests
//! cover React behaviour; this one covers the values, and it belongs here because it is a pure
//! string comparison, not a browser test. The file is the same one Vite builds into the page.

const TOKENS: &str = include_str!("../web/src/tokens.css");
const APP: &str = include_str!("../web/src/app.css");

/// The value of `--name` in a token block, parsed line by line: comments are ignored because a
/// declaration always starts at the beginning of its own line.
fn value<'a>(block: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("--{name}:");
    block.lines().find_map(|line| {
        let line = line.trim();
        line.strip_prefix(&prefix)
            .map(|rest| rest.trim().trim_end_matches(';').trim())
    })
}

fn block(css: &str, start: usize) -> &str {
    &css[start..]
}

#[test]
fn seven_type_sizes_and_a_thirteen_pixel_body() {
    let sizes: Vec<&str> = TOKENS
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if !line.starts_with("--fs-") {
                return None;
            }
            let (_, value) = line.split_once(':')?;
            Some(value.trim().trim_end_matches(';').trim())
        })
        .collect();
    assert_eq!(
        sizes,
        ["10px", "11px", "12px", "13px", "14px", "17px", "22px"],
        "the page has one seven-step type scale"
    );
    assert_eq!(value(TOKENS, "fs-base"), Some("13px"));
}

#[test]
fn one_spacing_rhythm_and_the_legacy_radii() {
    let spacing: Vec<&str> = [
        "sp-0-5", "sp-1", "sp-1-5", "sp-2", "sp-2-5", "sp-3", "sp-3-5", "sp-4", "sp-5",
    ]
    .into_iter()
    .map(|name| value(TOKENS, name).unwrap_or_else(|| panic!("--{name} is missing")))
    .collect();
    assert_eq!(
        spacing,
        ["2px", "4px", "6px", "8px", "10px", "12px", "14px", "16px", "24px"]
    );

    let radii: Vec<&str> = [
        "radius-xs",
        "radius-sm2",
        "radius-sm",
        "radius",
        "radius-lg",
    ]
    .into_iter()
    .map(|name| value(TOKENS, name).unwrap_or_else(|| panic!("--{name} is missing")))
    .collect();
    assert_eq!(radii, ["6px", "8px", "10px", "14px", "20px"]);
    assert_eq!(value(TOKENS, "accent"), Some("#bf3a0a"));
    assert_eq!(value(TOKENS, "accent-solid"), Some("#b03a0a"));
}

#[test]
fn the_dark_surfaces_are_restated_not_inherited() {
    let dark = TOKENS
        .find("@media")
        .map(|index| block(TOKENS, index))
        .expect("the token sheet has a dark block");
    assert_eq!(value(dark, "bg"), Some("#131418"));
    assert_eq!(value(dark, "surface"), Some("#1c1d22"));
    assert_eq!(value(dark, "ink"), Some("#ecedef"));
    assert_eq!(value(dark, "accent"), Some("#ffa36b"));
}

// ── THE REACT PAGE'S OWN COLOUR PAIRS ─────────────────────────────────────────────────────────────
//
// The maud fallback has its own measured AA test in `src/http/page.rs`; the React page's sheet had
// none, so `--ink-faint` shipped as #a1a1aa — 2.46:1 on white, used at 11px — and nothing failed.
// This test reads the SAME `web/src/tokens.css` Vite builds, resolves the two themes, and measures
// every text-on-surface pair the page actually draws. A token changed to a pretty value fails here.

/// A `#rrggbb` token as linear-ish sRGB channels in 0..=1. Alpha tokens are not admitted: a pair
/// measured against a colour it is not composited on is a number, not a verdict.
fn rgb(hex: &str) -> (f64, f64, f64) {
    let body = hex
        .strip_prefix('#')
        .unwrap_or_else(|| panic!("{hex} is not a #rrggbb token"));
    assert_eq!(body.len(), 6, "{hex} is not a #rrggbb token");
    let channel = |index: usize| {
        u8::from_str_radix(&body[index..index + 2], 16).expect("a hex channel") as f64 / 255.0
    };
    (channel(0), channel(2), channel(4))
}

fn luminance(value: (f64, f64, f64)) -> f64 {
    let linear = |channel: f64| {
        if channel <= 0.040_45 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(value.0) + 0.7152 * linear(value.1) + 0.0722 * linear(value.2)
}

fn contrast(ink: &str, surface: &str) -> f64 {
    let a = luminance(rgb(ink));
    let b = luminance(rgb(surface));
    let (light, dark) = if a >= b { (a, b) } else { (b, a) };
    (light + 0.05) / (dark + 0.05)
}

fn theme(dark: bool) -> &'static str {
    if dark {
        TOKENS
            .find("@media")
            .map(|index| block(TOKENS, index))
            .expect("the token sheet has a dark block")
    } else {
        TOKENS
    }
}

#[test]
fn every_text_pair_reads_on_the_surface_it_sits_on() {
    // (ink, surface): the pairs a reader actually sees, named by the use rather than the page.
    let pairs: &[(&str, &str, &str)] = &[
        ("body on a card", "ink", "surface"),
        ("body on the page", "ink", "bg"),
        ("hint on a card", "ink-muted", "surface"),
        ("hint on the page", "ink-muted", "bg"),
        ("faint label on a card", "ink-faint", "surface"),
        ("faint label on the page", "ink-faint", "bg"),
        ("faint label on a chip", "ink-faint", "surface-chip"),
        ("primary button label", "accent-fg", "accent-solid"),
        ("success on its wash", "ok", "ok-soft"),
        ("warning on its wash", "warn", "warn-soft"),
        ("error on its wash", "error", "error-soft"),
    ];
    for dark in [false, true] {
        let sheet = theme(dark);
        for (use_name, ink, surface) in pairs {
            // The dark success wash is a translucent green; it is measured by the runtime probe, not
            // compared against a hex it is not the background of.
            if dark && (*ink == "ok") {
                continue;
            }
            let ink_value = value(sheet, ink).unwrap_or_else(|| panic!("--{ink} is missing"));
            let surface_value =
                value(sheet, surface).unwrap_or_else(|| panic!("--{surface} is missing"));
            let measured = contrast(ink_value, surface_value);
            assert!(
                measured >= 4.5,
                "{use_name}: --{ink} {ink_value} on --{surface} {surface_value} measures {measured:.2}:1 in {} (AA needs 4.5)",
                if dark { "dark" } else { "light" }
            );
        }
    }
}

/// A declaration anywhere on a line. `device/ui` packs several tokens per line, so the line-based
/// `value` above would return the rest of the line for the first one.
fn declared<'a>(css: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("--{name}:");
    let start = css.find(&needle)? + needle.len();
    let rest = &css[start..];
    let end = rest.find(';').unwrap_or(rest.len());
    Some(rest[..end].trim())
}

#[test]
fn the_terminal_palette_is_the_canonical_one() {
    // `device/ui` is the single palette; the React sheet is its consumer. The canvas and the six
    // ANSI colours are the ones an operator reads a shell through, so they are compared by name in
    // both themes rather than trusted to a copy that already drifted once.
    use summrise_ui::TOKENS as UI;
    let names = [
        "term-canvas",
        "term-black",
        "term-red",
        "term-green",
        "term-yellow",
        "term-blue",
        "term-magenta",
        "term-cyan",
        "term-white",
    ];
    for dark in [false, true] {
        let react = theme(dark);
        let canonical = if dark {
            UI.find("@media")
                .map(|index| block(UI, index))
                .expect("the canonical sheet has a dark block")
        } else {
            UI
        };
        for name in names {
            assert_eq!(
                declared(react, name),
                declared(canonical, name),
                "--{name} differs between the React sheet and device/ui in {}",
                if dark { "dark" } else { "light" }
            );
        }
    }
}

#[test]
fn every_weight_is_one_of_four_tokens() {
    // The sheet had 26 literal weights across five values (450/500/550/600/650). The family is now
    // four documented steps; a new literal, or a renamed token, fails here instead of drifting.
    for (name, expected) in [
        ("fw-regular", "400"),
        ("fw-medium", "500"),
        ("fw-semi", "600"),
        ("fw-bold", "700"),
    ] {
        assert_eq!(value(TOKENS, name), Some(expected), "--{name}");
    }
    for line in APP.lines() {
        let Some(index) = line.find("font-weight:") else {
            continue;
        };
        let weight = line[index + "font-weight:".len()..]
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        assert!(
            weight.starts_with("var(--fw-"),
            "literal font weight: {line}"
        );
    }
}

#[test]
fn every_type_size_comes_from_the_seven_step_scale() {
    // Two declarations had escaped the scale (0.95rem for a close glyph, 0.65rem for a status pill).
    // A raw length here means a new size nobody measured against the body's 13px.
    for line in APP.lines() {
        let Some(index) = line.find("font-size:") else {
            continue;
        };
        let size = line[index + "font-size:".len()..]
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        assert!(
            size.starts_with("var(--fs-"),
            "font size outside the scale: {line}"
        );
    }
}
