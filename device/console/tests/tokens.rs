//! The React page's visual contract, checked by Rust.
//!
//! The token sheet is a product decision — seven type sizes, one spacing rhythm, the legacy radii
//! and the dark surfaces restated — not a stylesheet someone edits by feel. The web app's own tests
//! cover React behaviour; this one covers the values, and it belongs here because it is a pure
//! string comparison, not a browser test. The file is the same one Vite builds into the page.

const TOKENS: &str = include_str!("../web/src/tokens.css");

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
