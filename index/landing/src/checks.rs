//! `scripts/test/landing-check.mjs`, AS TESTS — the download landing page is a SURFACE, and nothing was
//! measuring it.
//!
//! WHY (round 239). The token contract mentions the landing only because it shares three token names with
//! the panel; the design suite covers the panel, the console and the extension. The landing is the page at
//! the product's front door, hand-written with its own `--dsw-alias-*` vocabulary — and no check had ever
//! asked whether its TEXT is readable. The gate was renamed in round 242 when it grew the structure and
//! layout sections, because a name covering a third of what it does stops the next reader looking.
//!
//! **IT IS A TEST IN THIS CRATE BECAUSE THE RENDERER IS HERE**: the sections read the RENDERED document,
//! and rendering it needs no toolchain, no browser and no spawn — which is why this one of the eight
//! remaining `.mjs` gates could move and the other six could not.
//!
//! THE MUTATIONS THAT MUST FAIL, from the gate's own header, and each is asserted below:
//!
//!   * lighten a label (`#71717a` -> `#9a9aa2`) -> "measures 2.68, under the 4.5 AA wants for text";
//!   * take the heading away (`<h1>` back to a `<div>`) -> "expected exactly 1 h1, found 0";
//!   * give a card a fixed width (`.card { width: 480px }`) -> "wider than the 320px a 1.4.10 reflow
//!     test uses".

use crate::colour::{contrast_ratio, parse_colour};
use crate::page::render;

const CONSOLE_URL: &str = "https://ai.saisi.online";
const INSTALLER_URL: &str = "https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz";
const SETUP_URL: &str = "summrise setup";

/// The page with real URLs, for the sections that read the document rather than the slots.
fn page(setup: Option<&str>) -> String {
    render(CONSOLE_URL, INSTALLER_URL, setup)
}

/// The `<style>` block of a rendered page.
fn style_of(html: &str) -> String {
    let start = html.find("<style>").expect("a <style> block") + "<style>".len();
    let end = html[start..].find("</style>").expect("a closing </style>") + start;
    html[start..end].to_string()
}

/// `readVars`: `--name: #hex;` pairs, FIRST WINS, comments stripped.
fn read_vars(css: &str) -> Vec<(String, String)> {
    let stripped = strip_comments(css);
    let mut out: Vec<(String, String)> = Vec::new();
    for (at, _) in stripped.match_indices("--") {
        let rest = &stripped[at..];
        let Some(colon) = rest.find(':') else {
            continue;
        };
        let name = &rest[..colon];
        if !name
            .chars()
            .all(|c| c == '-' || c.is_ascii_lowercase() || c.is_ascii_digit())
        {
            continue;
        }
        let value = rest[colon + 1..].trim_start();
        let Some(hash) = value.strip_prefix('#') else {
            continue;
        };
        let digits: String = hash.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        if !(3..=8).contains(&digits.len()) || !hash[digits.len()..].trim_start().starts_with(';') {
            continue;
        }
        if !out.iter().any(|(n, _)| n == name) {
            out.push((name.to_string(), format!("#{digits}")));
        }
    }
    out
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(at) = rest.find("/*") {
        out.push_str(&rest[..at]);
        match rest[at..].find("*/") {
            Some(end) => rest = &rest[at + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// The variable a theme defines, or a panic naming the file — the JavaScript's `v(name)`.
fn var<'a>(vars: &'a [(String, String)], name: &str, theme: &str) -> &'a str {
    vars.iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
        .unwrap_or_else(|| {
            panic!("index/landing/assets/page.css does not define {name} for {theme} — the check is reading the wrong thing")
        })
}

#[test]
fn the_landing_is_readable_in_both_themes() {
    let html = page(Some(SETUP_URL));
    let style = style_of(&html);
    let dark_start = style
        .find("body[data-ds-dark-theme]")
        .expect("no dark block in the landing's stylesheet — the theme toggle would do nothing");
    let light = read_vars(&style[..dark_start]);
    let dark: Vec<(String, String)> = {
        let mut merged = light.clone();
        for (n, v) in read_vars(&style[dark_start..]) {
            match merged.iter_mut().find(|(name, _)| *name == n) {
                Some(slot) => slot.1 = v,
                None => merged.push((n, v)),
            }
        }
        merged
    };

    // THE PAIRS THE PAGE ACTUALLY PAINTS, read from the file rather than copied here: a check that
    // hardcodes what it is checking tests itself.
    const PAIRS: [(&str, &str, &str); 8] = [
        (
            "label-primary on bg-base",
            "--dsw-alias-label-primary",
            "--dsw-alias-bg-base",
        ),
        (
            "label-secondary on bg-base",
            "--dsw-alias-label-secondary",
            "--dsw-alias-bg-base",
        ),
        (
            "label-tertiary on bg-base",
            "--dsw-alias-label-tertiary",
            "--dsw-alias-bg-base",
        ),
        (
            "label-tertiary on layer-1",
            "--dsw-alias-label-tertiary",
            "--dsw-alias-bg-layer-1",
        ),
        (
            "label-secondary on layer-2",
            "--dsw-alias-label-secondary",
            "--dsw-alias-bg-layer-2",
        ),
        (
            "button foreground on its fill",
            "--dsw-alias-button-primary-foreground",
            "--dsw-alias-button-primary-fill",
        ),
        (
            "button foreground on its hover",
            "--dsw-alias-button-primary-foreground",
            "--dsw-alias-button-primary-hover",
        ),
        (
            "business state on a white card",
            "--dsw-alias-state-business-primary",
            "--dsw-alias-bg-layer-1",
        ),
    ];
    const AA_TEXT: f64 = 4.5;

    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for (theme, vars) in [("light", &light), ("dark", &dark)] {
        for (label, fg_name, bg_name) in PAIRS {
            let (fg, bg) = (var(vars, fg_name, theme), var(vars, bg_name, theme));
            let ratio = contrast_ratio(
                parse_colour(fg).unwrap_or_else(|| panic!("{fg} is not a colour")),
                parse_colour(bg).unwrap_or_else(|| panic!("{bg} is not a colour")),
            );
            checked += 1;
            if ratio < AA_TEXT {
                failures.push(format!(
                    "{theme}: {label}: {fg} on {bg} measures {ratio:.2}, under the {AA_TEXT} AA wants for text"
                ));
            }
        }
    }
    // A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN: a floor on the tokens found, so a rename fails loudly
    // instead of silently checking nothing.
    assert!(
        light.len() >= 10 && dark.len() >= 10,
        "{} light / {} dark colours read from index/landing/assets/page.css, so this proves nothing",
        light.len(),
        dark.len()
    );
    assert!(
        failures.is_empty(),
        "landing contrast: FAILED\n  {}",
        failures.join("\n  ")
    );
    assert_eq!(checked, 16, "8 pairs in BOTH themes");
}

#[test]
fn the_landing_has_one_heading_that_agrees_with_its_title() {
    let html = page(Some(SETUP_URL));
    let body = strip_tags(&html, "<style", "</style>");
    let body = strip_tags(&body, "<script", "</script>");

    let h1s: Vec<String> = text_between(&body, "<h1", "</h1>");
    let title = text_between(&body, "<title>", "</title>")
        .into_iter()
        .next();
    let mut problems: Vec<String> = Vec::new();
    if h1s.len() != 1 {
        problems.push(format!(
            "expected exactly 1 h1, found {}{}",
            h1s.len(),
            if h1s.is_empty() {
                String::new()
            } else {
                format!(": {}", h1s.join(", "))
            }
        ));
    }
    match (h1s.as_slice(), &title) {
        ([_], None) => problems.push("no <title>".to_string()),
        ([h1], Some(t)) if h1 != t => problems.push(format!(
            "the h1 says \"{h1}\" and the title says \"{t}\" — a page whose heading and title disagree announces two names"
        )),
        _ => {}
    }
    if !body.contains("<html") || !body[..body.find('>').unwrap_or(0) + 1].contains("lang=") {
        // The `lang` attribute is on the opening `<html …>` tag.
        let open = body.find("<html").map(|i| &body[i..]).unwrap_or("");
        let tag = &open[..open.find('>').map(|i| i + 1).unwrap_or(open.len())];
        if !tag.contains("lang=") {
            problems.push("no lang on <html>".to_string());
        }
    }
    if !body.contains("name=\"viewport\"") {
        problems.push("no viewport meta".to_string());
    }
    let mains = body.match_indices("<main").count();
    if mains != 1 {
        problems.push(format!("{mains} main landmarks, expected 1"));
    }
    assert!(
        problems.is_empty(),
        "landing structure: FAILED\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn the_landing_reflows_at_the_narrowest_viewport() {
    const NARROWEST: f64 = 320.0;
    let style = style_of(&page(Some(SETUP_URL)));
    let mut problems: Vec<String> = Vec::new();
    let mut widths = 0usize;

    for (selector, declarations) in css_rules(&style) {
        for (property, value) in declarations {
            if property != "min-width" && property != "width" {
                continue;
            }
            // Percentages, `clamp()`, `min()` and `calc()` all shrink; only a bare pixel value sets a
            // floor.
            let Some(px) = value.strip_suffix("px").and_then(|n| n.parse::<f64>().ok()) else {
                continue;
            };
            widths += 1;
            if px > NARROWEST {
                problems.push(format!(
                    "{selector} {{ {property}: {value} }} — wider than the {NARROWEST}px a 1.4.10 reflow test uses; a max-width would cap it instead of setting a floor"
                ));
            }
        }
    }
    let breakpoints = count_breakpoints(&style);
    if breakpoints == 0 {
        problems.push(
            "no max-width breakpoint at all, so nothing about this page can reflow".to_string(),
        );
    }
    if widths == 0 {
        problems.push(
            "no pixel width was found to check — the scan is reading the wrong thing".to_string(),
        );
    }
    assert!(
        problems.is_empty(),
        "landing layout: FAILED\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn the_landing_never_tells_a_visitor_to_install_the_bare_package() {
    // AGENTS.md: "A BARE `npm i -g summrise-agent` CAN INSTALL NOTHING WHILE REPORTING SUCCESS" — a stale
    // cached `latest` prints "changed 1 package" and leaves the old CLI in place. The criterion is about
    // the command a VISITOR COPIES, so the RENDERED DOCUMENT is the input, and BOTH arms are read: a
    // tgz-only release must not start naming the bare package either.
    let src = format!("{}\n{}", page(Some(SETUP_URL)), page(None));
    let mut problems: Vec<String> = Vec::new();
    let mut at = 0usize;
    let mut seen = 0usize;
    while let Some(i) = src[at..].find("npm i -g") {
        let start = at + i;
        seen += 1;
        // A bare package name is one with no URL and no `--prefix` in front of it.
        let tail = &src[start + "npm i -g".len()..];
        let tail = tail
            .strip_prefix(" --prefix")
            .map(|t| &t[t.find(' ').unwrap_or(t.len())..])
            .unwrap_or(tail);
        if tail.trim_start().starts_with("summrise-agent")
            && !tail.trim_start().starts_with("summrise-agent-latest")
            && !tail.trim_start().starts_with("summrise-agent@")
        {
            problems.push(format!(
                "a bare package name has a resolution step: npm i -g{}",
                &tail[..tail.len().min(40)]
            ));
        }
        at = start + "npm i -g".len();
    }
    if seen < 2 {
        problems.push(format!(
            "read {seen} \"npm i -g\" occurrence(s); the scan is reading the wrong thing"
        ));
    }
    assert!(
        problems.is_empty(),
        "landing install: FAILED\n  {}",
        problems.join("\n  ")
    );
}

// ── the small scanners the sections above use ───────────────────────────────────────────────────

fn strip_tags(html: &str, open: &str, close: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find(open) {
        out.push_str(&rest[..at]);
        match rest[at..].find(close) {
            Some(end) => rest = &rest[at + end + close.len()..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

fn text_between(html: &str, open: &str, close: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find(open) {
        let after = &rest[at..];
        let Some(gt) = after.find('>') else { break };
        let Some(end) = after[gt..].find(close) else {
            break;
        };
        out.push(after[gt + 1..gt + end].trim().to_string());
        rest = &after[gt + end + close.len()..];
    }
    out
}

/// `([^{}]+)\{([^{}]*)\}` — the selector (its LAST line) and its declarations.
fn css_rules(css: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out = Vec::new();
    let mut rest = css;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        let selector = rest[..open].lines().last().unwrap_or("").trim().to_string();
        let mut declarations = Vec::new();
        for decl in rest[open + 1..open + close].split(';') {
            if let Some(colon) = decl.find(':') {
                declarations.push((
                    decl[..colon].trim().to_string(),
                    decl[colon + 1..].trim().to_string(),
                ));
            }
        }
        out.push((selector, declarations));
        rest = &rest[open + close + 1..];
    }
    out
}

fn count_breakpoints(style: &str) -> usize {
    let mut count = 0usize;
    let mut at = 0usize;
    while let Some(i) = style[at..].find("@media") {
        let start = at + i;
        let head = &style[start
            ..style[start..]
                .find('{')
                .map(|e| start + e)
                .unwrap_or(style.len())];
        if head.contains("max-width") {
            count += 1;
        }
        at = start + "@media".len();
    }
    count
}
