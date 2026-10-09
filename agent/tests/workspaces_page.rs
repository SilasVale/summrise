//! THE WORKSPACES PAGE, ON THE BYTES THE AGENT SERVES.
//!
//! ── WHAT THIS REPLACES ──────────────────────────────────────────────────────────────────────────────
//! `agent/scripts/verify-workspaces-page.mjs` fetched the served `panel.js` and `panel.css` and then
//! applied a set of judgements over their TEXT: five class names must appear in the bundle, each must have
//! a rule that paints it, the file row must carry its `:active` press state, four strings the page says out
//! loud must be present, and the page must carry an `sr-only` heading. NOTHING INVOKED IT.
//!
//! **AND PORTING IT FOUND THAT ITS HAND-KEPT LIST HAD ROTTED — which is the argument for this gate rather
//! than a footnote to it.** The first run failed on two of its own constants: `workspaces-machine` is in
//! neither the bundle nor the stylesheet, and is not in the VIEWS either (so the class was renamed or
//! removed, and the script would have failed for anyone who ran it), and one of the four strings,
//! `"Workspaces"`, is not in the bundle — the page's copy is translated, so a literal English substring was
//! never the invariant it claimed to be. So the list is not repaired here, it is DELETED: the classes are
//! DERIVED from the two view files, which cannot rot, and the string check is dropped with that reason
//! written down rather than silently weakened. "The page says what it should" is a RENDERED property, and
//! rendering needs the DOM, which is the device half named below.
//!
//! **THE FETCH IS THE DRIVER; THE JUDGEMENT IS DATA.** That is the whole reason this can move: the script
//! read the served bytes because a device has no browser, and the bytes the agent serves ARE
//! `agent/resources/panel/panel.{js,css}` — the same committed build, embedded with `include_str!`. So the
//! judgements run here, in `cargo test`, where they are enforced on every change instead of when somebody
//! remembers to run a script.
//!
//! ── WHAT IS NOT HERE, NAMED SO IT IS NOT MISTAKEN FOR COVERAGE ──────────────────────────────────────
//! The LIVE half: that the DEVICE is serving this build at all. That is a deploy question — the file the
//! agent embeds is this one — and it is checked where deploys are checked, not here.
//!
//! ── THE MUTATION THAT MUST FAIL THIS GATE ───────────────────────────────────────────────────────────
//! MUTATION: rename a class in the BUILT bundle (`workspaces-entry-file` -> `workspaces-entry-fileX`).
//! RESULT:   **exit 0 — IT DID NOT BITE, and it found a weakness in the check rather than in the gate's
//!            idea.** The renamed class still CONTAINS the old name, so a substring test passed on a page
//!            whose class no longer exists — the same weakness the script this replaces had. The check is a
//!            TOKEN match now (neither side may continue as an identifier), and the same mutation bites:
//!            exit 101 — "workspaces-entry-file: in the bundle false, painted by css true".
//! MUTATION: delete the `:active` rule for the file row from `panel.css`.
//! RESULT:   exit 101 — "the file row's press state: .workspaces-entry-file:active".
//! AND THE MUTATION THAT MUST NOT: the committed build passes and PRINTS what it measured — the number of
//! classes it derived, the byte sizes of both files — so the gate is visibly measuring rather than echoing
//! itself.

use std::collections::BTreeSet;
use std::path::PathBuf;

mod common;

/// WHERE THE PAGE'S MARKUP IS WRITTEN. The class names are DERIVED from these files rather than listed
/// here — see the header for why that is the whole point of this gate.
const VIEWS: [&str; 2] = [
    "agent/resources/panel-react/src/components/WorkspacesPanel.tsx",
    "agent/resources/panel-react/src/components/TerminalWorkspace.tsx",
];

/// The press state the feedback gate demanded: a hover with no `:active` is a failure, not a style.
const PRESS_RULE: &str = ".workspaces-entry-file:active";

fn panel() -> (String, String) {
    let dir = common::repo().join("agent/resources/panel");
    let js = std::fs::read_to_string(dir.join("panel.js"))
        .expect("panel.js must exist — it is the build the agent embeds");
    let css = std::fs::read_to_string(dir.join("panel.css"))
        .expect("panel.css must exist — it is the build the agent embeds");
    (js, css)
}

/// Every `workspaces-…` token the views render, in order and without duplicates.
fn marks_the_views_render() -> BTreeSet<String> {
    let repo = common::repo();
    let mut out = BTreeSet::new();
    for rel in VIEWS {
        let src = std::fs::read_to_string(repo.join(rel))
            .unwrap_or_else(|e| panic!("the view {rel} must be readable: {e}"));
        for word in src.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
            if let Some(rest) = word.strip_prefix("workspaces-") {
                if !rest.is_empty() {
                    out.insert(format!("workspaces-{rest}"));
                }
            }
        }
    }
    out
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

/// A CLASS NAME IS A TOKEN, NOT A SUBSTRING — and the first mutation proved the difference: renaming
/// `workspaces-entry-file` to `workspaces-entry-fileX` in the bundle left the substring in place, so a
/// `contains` check passed on a page whose class no longer exists. The script this replaces had the same
/// weakness. A match only counts when neither side continues as an identifier.
fn has_token(haystack: &str, token: &str) -> bool {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    haystack.match_indices(token).any(|(i, _)| {
        let before = haystack[..i].chars().next_back();
        let after = haystack[i + token.len()..].chars().next();
        !before.is_some_and(ident) && !after.is_some_and(ident)
    })
}

#[test]
fn every_class_the_workspaces_views_render_is_painted() {
    let (js, css) = panel();
    let marks = marks_the_views_render();
    assert!(
        marks.len() >= 10,
        "read only {} workspaces-… class(es) from the views — the views moved, so this gate would prove \
         nothing",
        marks.len()
    );

    let mut failures: Vec<String> = Vec::new();
    for mark in &marks {
        let in_js = has_token(&js, mark);
        let in_css = has_token(&css, &format!(".{mark}"));
        if !in_js || !in_css {
            failures.push(format!(
                "{mark}: in the bundle {in_js}, painted by css {in_css}"
            ));
        }
    }
    if count(&css, PRESS_RULE) == 0 {
        failures.push(format!("the file row's press state: {PRESS_RULE}"));
    }
    // The h1 contract: the page renders one, sr-only, and nothing else claims to be the page's name.
    if count(&js, "sr-only") == 0 {
        failures.push("the page carries no sr-only heading".to_string());
    }

    assert!(
        failures.is_empty(),
        "the built panel does not carry every class the workspaces views render:\n  {}",
        failures.join("\n  ")
    );
    println!(
        "workspaces-page: {} class(es) derived from {} view(s), all painted — panel.js {} bytes, panel.css {} bytes",
        marks.len(),
        VIEWS.len(),
        js.len(),
        css.len()
    );
}

#[test]
fn the_derivation_reads_a_view_and_not_an_empty_set() {
    let marks = marks_the_views_render();
    // The floor above is the real guard; this states the two properties the derivation must have, so a
    // change that made it return nothing (or return fragments) fails here with its own name.
    assert!(
        marks.iter().all(|m| m.starts_with("workspaces-")),
        "{marks:?}"
    );
    assert!(
        marks.contains("workspaces-entry-file"),
        "the pressed row must be in the set: {marks:?}"
    );
    assert!(
        !marks.contains("workspaces-machine"),
        "the class the old list named is gone: {marks:?}"
    );
}
