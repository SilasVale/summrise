//! THE LANDING'S PLAN — every surface `agent/scripts/lib/sweep/landing-run.cjs` visits, in visit order.
//!
//! The landing is the smallest of the three and the only one whose THEME is an emulated media feature
//! rather than a URL parameter or an attribute, so its surfaces carry `media` and the payload calls
//! `page.emulateMedia` where the plan says to.

use crate::plan::{vp, PassSpec, Plan, Surface, Tool};
use serde_json::json;

/// THE TWO STATES A RELEASE CAN PRODUCE, and the reason a rendered arm is worth its cost: when a
/// release publishes no installer the page swaps the primary button for a hint — a different element,
/// a different size, a different contrast question.
const PAGES: &[&str] = &["installer", "npm-only"];

/// BOTH COLOUR SCHEMES. The landing's theme is `prefers-color-scheme`, so it is emulated.
const SCHEMES: &[&str] = &["light", "dark"];

/// The widths WCAG 1.4.10 names, measured on both pages.
const REFLOW_WIDTHS: &[u32] = &[640, 320];

fn surf(
    kind: &'static str,
    id: String,
    label: &str,
    viewport: crate::plan::Viewport,
    theme: &str,
    passes: &[&'static str],
) -> Surface {
    Surface {
        id,
        kind,
        density: Some("landing"),
        // AN ABSOLUTE URL PATH, always: the payload concatenates origin + path, and a path
        // without its leading slash produced `http://summrise.testinstaller.html`.
        path: Some(format!("/{label}.html")),
        query: None,
        viewport,
        theme: Some(theme.to_string()),
        mode: None,
        page: Some(label.to_string()),
        hash: None,
        media: None,
        dynamic: None,
        reload: false,
        navigate: true,
        set_viewport: true,
        pre: Vec::new(),
        vars: Default::default(),
        post: Vec::new(),
        passes: passes.to_vec(),
    }
}

pub fn build(spec: PassSpec) -> Plan {
    let mut plan = Plan::new(Tool::Landing, spec);
    plan.caps = json!({
        "focus_tabs": 14,
        "press_curated": [".btn-primary", "a", ".theme-toggle"],
        "reflow_widths": REFLOW_WIDTHS,
        "reflow_height": 800,
        "schemes": SCHEMES,
        "pages": PAGES,
    });

    let w = |name: &str| plan.wants(name);
    let mut out: Vec<Surface> = Vec::new();

    // ── A. BOTH STATES, BOTH SCHEMES ─────────────────────────────────────────────────────────────────
    // `emulateMedia` is set once per scheme, not once per page: the second page of a scheme inherits it.
    for scheme in SCHEMES {
        for (i, label) in PAGES.iter().enumerate() {
            let mut passes: Vec<&'static str> = Vec::new();
            if w("contrast") {
                passes.push("contrast");
            }
            if w("names") {
                passes.push("names");
            }
            if w("focus") {
                passes.push("focus");
            }
            if w("idle") {
                passes.push("idle");
            }
            if w("unstyled") {
                passes.push("unstyled");
            }
            if w("targets") {
                passes.push("targets");
            }
            if w("press") {
                passes.push("press");
            }
            let mut s = surf(
                "page",
                format!("page.{label}.{scheme}"),
                label,
                vp(1440, 900),
                scheme,
                &passes,
            );
            // THE REPORT'S OWN LABEL, not the bare file name: the payload used to build
            // `label + "@1440" + (dark ? "-dark" : "")` at the call site, and a label the report
            // carries is a fact about the surface.
            s.page = Some(format!(
                "{label}@1440{}",
                if *scheme == "dark" { "-dark" } else { "" }
            ));
            if i == 0 {
                // THE SCHEME IS EMULATED ONCE, BEFORE THE FIRST PAGE OF THE SCHEME — and `pre` is what
                // says "before", because the payload's own order is `emulateMedia` then `goto`, with
                // the viewport set between them.
                s.pre
                    .push(json!({ "emulateMedia": { "colorScheme": scheme } }));
            }
            out.push(s);
        }
    }

    // ── B. THE REFLOW WIDTHS, BOTH PAGES ─────────────────────────────────────────────────────────────
    // The viewport is set once per WIDTH and the two pages are walked at it, which is why only the
    // first surface of each width carries `set_viewport`.
    if w("reflow") {
        for width in REFLOW_WIDTHS {
            for (i, label) in PAGES.iter().enumerate() {
                let mut s = surf(
                    "reflow",
                    format!("reflow.{width}.{label}"),
                    label,
                    vp(*width, 800),
                    "light",
                    &["reflow"],
                );
                s.page = Some(format!("{label}@{width}"));
                s.set_viewport = i == 0;
                out.push(s);
            }
        }
    }

    // ── C. MOTION: THE SAME PAGE, TWICE ──────────────────────────────────────────────────────────────
    // Once with the preference unset and once with it emulated, because "nothing animates under
    // reduce" is only evidence if something animates without it. NO VIEWPORT IS SET: the pass measures
    // at whatever the previous block left, which is what the payload has always done.
    if w("motion") {
        let renders = [
            (json!({"reducedMotion": null}), "motion"),
            (json!({"reducedMotion": "reduce"}), "motion-reduced"),
        ];
        for (i, (media, mode)) in renders.iter().enumerate() {
            let mut s = surf(
                "motion",
                format!("motion.{mode}"),
                "installer",
                vp(1440, 900),
                "light",
                &["motion"],
            );
            s.mode = Some((*mode).to_string());
            s.media = Some(media.clone());
            s.set_viewport = false;
            if i == renders.len() - 1 {
                // THE RESET AFTER THE LOOP. `motionPass` leaves the emulation off, and the panel's
                // payload does it explicitly; both are one step here.
                s.post
                    .push(json!({ "emulateMedia": { "reducedMotion": null } }));
            }
            out.push(s);
        }
    }

    for s in out {
        plan.push(s);
    }
    plan
}
