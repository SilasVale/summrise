//! THE CONSOLE'S PLAN — every surface `agent/scripts/lib/sweep/console-run.cjs` visits, in visit order.
//!
//! The console is an SPA driven by `location.hash`, so a surface is `(page label, hash, width, theme)`
//! rather than a path — the payload navigates to the origin and then SETS THE HASH, and the trace the
//! parity harness compares records both calls.
//!
//! The live origin is in here because that is where this sweep has always measured; the console is the
//! one tool with no local fixture, which is also why CI cannot run it.

use crate::plan::{vp, PassSpec, Plan, Surface, Tool};
use serde_json::json;

/// The console's own route table (`gateway/ui/src/App.tsx`) — every authenticated page it has.
pub const PAGES: &[(&str, &str)] = &[
    ("overview", "#/"),
    ("devices", "#/devices"),
    ("models", "#/models"),
    ("keys", "#/keys"),
    ("routes", "#/routes"),
    ("users", "#/users"),
];

/// THE FIVE WIDTHS, and why 320 is one of them: WCAG 1.4.10 names 320 CSS pixels, and the sweep used
/// to test 1440/900/720 — a "WCAG reflow" claim measured at a width the criterion does not mention.
/// The sheet's breakpoints are 396/400/560/768/900/980/1080/1140, and every one has a measured width
/// on each side.
const WIDTHS: &[u32] = &[1440, 900, 720, 640, 320];

/// WHERE THE PASSES THAT NEED THE WIDEST LAYOUT RUN. One width, not five: hover styles are per-class
/// and the cost of the rest is a full-DOM probe per surface.
const WIDE: u32 = 1440;

fn surf(
    kind: &'static str,
    id: String,
    label: &str,
    viewport: crate::plan::Viewport,
    hash: Option<&str>,
    theme: &str,
    passes: &[&'static str],
) -> Surface {
    Surface {
        id,
        kind,
        density: Some("console"),
        path: Some("/".to_string()),
        query: None,
        viewport,
        theme: Some(theme.to_string()),
        mode: Some(if theme == "dark" { "dark" } else { "light" }.to_string()),
        page: Some(label.to_string()),
        hash: hash.map(str::to_string),
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
    let mut plan = Plan::new(Tool::Console, spec);
    plan.caps = json!({
        "focus_tabs": 16,
        "ack_budget_ms": 100,
        "press_curated": [".rail-btn", ".btn", ".icon-btn", ".lang-btn", ".auth-tab", ".btn-dashed",
                          ".card-link", ".dev-mini", ".rail-avatar", ".user-pop-logout"],
        "press_discover": 8,
        "press_skip": [".rail-btn", ".lang-btn", ".avatar", "a"],
        "wide_width": WIDE,
        "reflow_widths": WIDTHS,
        "default_widths": [WIDE],
        "motion_widths": [WIDE],
        "pages": PAGES.iter().map(|(l, h)| json!([l, h])).collect::<Vec<_>>(),
    });

    let w = |name: &str| plan.wants(name);
    let mut out: Vec<Surface> = Vec::new();

    // ── A. THE DARK THEME, ON EVERY PAGE ─────────────────────────────────────────────────────────────
    // The console has had a dark theme for as long as it has existed, and every section of this sweep
    // hardcoded `theme: 'light'`. One width, because `1440` is where the console is used.
    if w("dark") {
        for (label, hash) in PAGES {
            out.push(surf(
                "dark",
                format!("dark.{label}"),
                label,
                vp(WIDE, 900),
                Some(hash),
                "dark",
                &["dark"],
            ));
        }
    }

    // ── B. THE WIDTH SWEEP: every page at every width, with the wide-only passes inside it ───────────
    // `--passes=reflow` is what widens this loop; without it the sweep renders ONE width, which is also
    // what makes a press-only run affordable.
    let widths: Vec<u32> = if w("reflow") {
        WIDTHS.to_vec()
    } else {
        vec![WIDE]
    };
    for width in widths {
        for (i, (label, hash)) in PAGES.iter().enumerate() {
            let mut passes: Vec<&'static str> = Vec::new();
            if w("reflow") {
                passes.push("reflow");
            }
            if w("contrast") {
                passes.push("contrast");
            }
            // THE WIDE-ONLY AXES. They are per-class or per-layout rather than per-width, so measuring
            // them five times would be the same measurement five times over.
            if width == WIDE {
                if w("names") {
                    passes.push("names");
                }
                if w("unstyled") {
                    passes.push("unstyled");
                }
                passes.push("hover");
                if w("focus") {
                    passes.push("focus");
                }
                if w("press") {
                    passes.push("press");
                }
                if w("idle") {
                    passes.push("idle");
                }
                if w("ack") {
                    passes.push("ack");
                }
            }
            let mut s = surf(
                "widths",
                format!("widths.{width}.{label}"),
                label,
                vp(width, 900),
                Some(hash),
                "light",
                &passes,
            );
            // THE VIEWPORT IS SET ONCE PER WIDTH, then the six pages are walked at it — which is what
            // the payload has always done, and what the parity harness compares.
            s.set_viewport = i == 0;
            out.push(s);
        }
    }

    // ── C. REDUCED MOTION, BOTH STATES, ON THE CONSOLE'S OWN OVERVIEW ────────────────────────────────
    // The preference only takes effect on a fresh style resolution, so the render reloads the page and
    // re-applies the route — which is why this is two surfaces rather than one.
    if w("motion") {
        let renders = [
            (json!({"reducedMotion": null}), "motion"),
            (json!({"reducedMotion": "reduce"}), "motion-reduced"),
        ];
        for (i, (media, mode)) in renders.iter().enumerate() {
            let mut s = surf(
                "motion",
                format!("motion.{mode}"),
                "overview",
                vp(WIDE, 900),
                Some("#/"),
                "light",
                &["motion"],
            );
            s.mode = Some((*mode).to_string());
            s.media = Some(media.clone());
            s.set_viewport = i == 0;
            if i == renders.len() - 1 {
                s.post
                    .push(json!({"emulateMedia": {"reducedMotion": null}}));
            }
            out.push(s);
        }
    }

    // ── D. TARGET SIZE, TWO PAGES ────────────────────────────────────────────────────────────────────
    if w("targets") {
        for (label, hash) in [("overview", "#/"), ("devices", "#/devices")] {
            out.push(surf(
                "targets",
                format!("targets.{label}"),
                label,
                vp(WIDE, 900),
                Some(hash),
                "light",
                &["targets"],
            ));
        }
    }

    // ── E. THE UNSTYLED CENSUS ON THE EMPTY OVERVIEW ─────────────────────────────────────────────────
    // The one class this console declares unstyled-by-design fires when a tone is `off`, which happens
    // on the Overview's stats when there is nothing to report — so the census has to visit the EMPTY
    // Overview or its own declaration is never exercised.
    if w("unstyled") {
        for theme in ["light", "dark"] {
            let label = if theme == "dark" {
                "overview-empty-dark"
            } else {
                "overview-empty"
            };
            out.push(surf(
                "unstyled-empty",
                format!("unstyled-empty.{theme}"),
                label,
                vp(WIDE, 900),
                Some("#/"),
                theme,
                &["unstyled"],
            ));
        }
    }

    // ── F. THE EMPTY FLEET, AS SURFACES OF ITS OWN ───────────────────────────────────────────────────
    {
        for theme in ["light", "dark"] {
            for (label, hash) in [
                ("overview-empty", "#/"),
                ("devices-empty", "#/devices"),
                ("keys-empty", "#/keys"),
            ] {
                let name = format!("{label}{}", if theme == "dark" { "-dark" } else { "" });
                out.push(surf(
                    "empty",
                    format!("empty.{label}.{theme}"),
                    &name,
                    vp(WIDE, 900),
                    Some(hash),
                    theme,
                    &["contrast", "names", "theme"],
                ));
            }
        }
    }

    // ── G. EVERY PAGE'S FAILURE STATE ────────────────────────────────────────────────────────────────
    {
        for theme in ["light", "dark"] {
            for (label, hash) in PAGES {
                let name = format!("{label}-fail{}", if theme == "dark" { "-dark" } else { "" });
                out.push(surf(
                    "fail",
                    format!("fail.{label}.{theme}"),
                    &name,
                    vp(WIDE, 900),
                    Some(hash),
                    theme,
                    &["contrast", "names", "theme"],
                ));
            }
        }
    }

    // ── H. THE LOGIN PAGE, IN BOTH THEMES ────────────────────────────────────────────────────────────
    // It is the one surface an operator sees before anything else works, and it is a CARD — colour,
    // type and a form — so a light-only render leaves a dark regression unmeasured. No hash: the app
    // renders the login screen instead of any route.
    for theme in ["light", "dark"] {
        let name = if theme == "dark" {
            "login-dark"
        } else {
            "login"
        };
        out.push(surf(
            "login",
            format!("login.{theme}"),
            name,
            vp(WIDE, 900),
            None,
            theme,
            &["contrast", "names", "theme"],
        ));
    }

    for s in out {
        plan.push(s);
    }
    plan
}
