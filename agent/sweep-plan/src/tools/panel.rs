//! THE PANEL'S PLAN — every surface `agent/scripts/lib/sweep/panel-run.cjs` visits, in visit order.
//!
//! The order is not cosmetic: the report's `rows`, `surfaces`, `names`, `themeChecks` and `sse` arrays
//! are appended in visit order, and 2b's acceptance bar is a report that is byte-identical to the one
//! the JavaScript produced. So this file is a transcription of the payload's loop structure, and the
//! parity harness (`parity/compare.mjs`) executes the pre-change payload under a stub browser and
//! compares the ordered trace of navigations with what this builds.

use crate::plan::{vp, PassSpec, Plan, Surface, Tool};
use serde_json::json;

/// The two densities, in the order every loop that walks them uses.
const DENSITIES: &[(&str, &str, u32, u32)] = &[
    ("panel", "/panel/", 1280, 860),
    ("desktop", "/desktop/", 1440, 900),
];

const THEMES: &[&str] = &["light", "dark"];

/// A small builder, because a surface is fourteen fields and most of them are the same in a block.
/// `q` takes ordered pairs and is the only place a query is spelled.
struct B {
    s: Surface,
}

impl B {
    #[allow(clippy::too_many_arguments)]
    fn new(
        kind: &'static str,
        id: String,
        density: &'static str,
        path: &str,
        viewport: crate::plan::Viewport,
        theme: &str,
        mode: impl Into<String>,
        query: Vec<(String, String)>,
    ) -> B {
        B {
            s: Surface {
                id,
                kind,
                density: Some(density),
                path: Some(path.to_string()),
                query: Some(query),
                viewport,
                theme: Some(theme.to_string()),
                mode: Some(mode.into()),
                page: None,
                hash: None,
                media: None,
                dynamic: None,
                reload: true,
                navigate: true,
                set_viewport: true,
                pre: Vec::new(),
                post: Vec::new(),
                vars: serde_json::Map::new(),
                passes: Vec::new(),
            },
        }
    }

    fn page(mut self, page: impl Into<String>) -> B {
        self.s.page = Some(page.into());
        self
    }

    fn passes(mut self, passes: &[&'static str]) -> B {
        self.s.passes = passes.to_vec();
        self
    }

    fn var(mut self, key: &str, value: serde_json::Value) -> B {
        self.s.vars.insert(key.to_string(), value);
        self
    }

    fn media(mut self, media: serde_json::Value) -> B {
        self.s.media = Some(media);
        self
    }

    fn dynamic(mut self, what: &'static str) -> B {
        self.s.dynamic = Some(what);
        self
    }

    /// `flag` is a `key=value` fixture flag, appended as its own query pair — written this way so the
    /// pair order matches the URL the payload builds, which is what the parity harness compares.
    fn flag(mut self, flag: &str) -> B {
        let (k, v) = flag.split_once('=').expect("a fixture flag is key=value");
        if let Some(q) = self.s.query.as_mut() {
            q.push((k.to_string(), v.to_string()));
        }
        self
    }

    fn done(self, out: &mut Vec<Surface>) {
        out.push(self.s);
    }
}

/// A query in the payloads' own order: `theme`, `mode`, `sessions`, then the block's own flags.
fn base(theme: &str, mode: &str, sessions: &str) -> Vec<Vec<(String, String)>> {
    vec![vec![
        ("theme".to_string(), theme.to_string()),
        ("mode".to_string(), mode.to_string()),
        ("sessions".to_string(), sessions.to_string()),
    ]]
}

fn qs(theme: &str, mode: &str, sessions: &str) -> Vec<(String, String)> {
    base(theme, mode, sessions).remove(0)
}

pub fn build(spec: PassSpec) -> Plan {
    let mut plan = Plan::new(Tool::Panel, spec);
    plan.caps = json!({
        "focus_tabs": 14,
        "ack_budget_ms": 100,
        "ack_settings_targets": [".monitor-btn", ".monitor-add .btn"],
        "ack_memory_discover": 4,
        "press_curated": [".rail-btn", ".desktop-rail-btn", ".tab", ".dtab", ".side-row", ".side-add"],
        "press_rail_discover": 16,
        "press_skip": [".rail-btn", ".desktop-rail-btn", ".tab", ".dtab", ".side-row", ".side-add"],
        "reflow_widths": [640, 320],
        "reflow_height": 800,
    });

    let w = |name: &str| plan.wants(name);
    let pages = w("pages");
    let mut out: Vec<Surface> = Vec::new();

    // ── A. THE MATRIX ────────────────────────────────────────────────────────────────────────────────
    // `needsPage` is the loop's own gate, and it is wider than `pages`: any of these six axes renders
    // the page, because the focus, idle, press and targets blocks live INSIDE this loop. Running
    // `--passes=focus` alone used to run it ZERO times and report `focus: []` — clean because nothing
    // ran — which is why the gate names every axis that needs a rendered page.
    let needs_page = ["pages", "focus", "timing", "hover", "press", "idle"]
        .iter()
        .any(|n| w(n));
    if needs_page {
        // TWO MODES WHEN THE PAGES ARE WANTED, one otherwise: `relaxed` is the state the pages pass
        // measures, and the axes that live inside this loop do not need it.
        let modes: &[&str] = if pages {
            &["idle", "relaxed"]
        } else {
            &["idle"]
        };
        for (density, path, width, height) in DENSITIES {
            for theme in THEMES {
                for mode in modes {
                    let mut passes: Vec<&'static str> = Vec::new();
                    if pages {
                        passes.push("pages");
                    }
                    passes.push("timing");
                    passes.push("focus");
                    if w("idle") {
                        passes.push("idle");
                    }
                    if w("press") {
                        passes.push("press");
                    }
                    if w("targets") {
                        passes.push("targets");
                    }
                    B::new(
                        "matrix",
                        format!("matrix.{density}.{theme}.{mode}"),
                        density,
                        path,
                        vp(*width, *height),
                        theme,
                        *mode,
                        qs(theme, mode, "4"),
                    )
                    .passes(&passes)
                    .done(&mut out);
                }
            }
        }
    }

    // ── B. THE DESKTOP'S EMPTY STATE ─────────────────────────────────────────────────────────────────
    // Desktop only, because in the PANEL harness the rail receives connected=false and the surface
    // measured "Sessions unavailable" while calling itself empty. The panel block was pruned; the
    // desktop one is the real state.
    if pages {
        for theme in THEMES {
            B::new(
                "desktop-empty",
                format!("desktop-empty.desktop.{theme}"),
                "desktop",
                "/desktop/",
                vp(1440, 900),
                theme,
                "relaxed",
                qs(theme, "relaxed", "0"),
            )
            .page("Desktop-empty")
            .passes(&["pages", "theme"])
            .done(&mut out);
        }
    }

    // ── C. THE NEW-SESSION MENU, WHICH ONLY A CLICK RENDERS ──────────────────────────────────────────
    if pages {
        for theme in THEMES {
            B::new(
                "new-menu",
                format!("new-menu.desktop.{theme}"),
                "desktop",
                "/desktop/",
                vp(1440, 900),
                theme,
                "menu",
                qs(theme, "idle", "4"),
            )
            .page(format!("Desktop-NewMenu-{theme}"))
            .passes(&["pages", "theme"])
            .done(&mut out);
        }
    }

    // ── D. THE PANEL DENSITY'S EMPTY STATE ───────────────────────────────────────────────────────────
    if pages {
        for theme in THEMES {
            B::new(
                "panel-empty",
                format!("panel-empty.panel.{theme}"),
                "panel",
                "/panel/",
                vp(1280, 860),
                theme,
                "relaxed",
                qs(theme, "relaxed", "0"),
            )
            .page("Panel-empty")
            .passes(&["pages", "theme"])
            .done(&mut out);
        }
    }

    // ── E. THE UPDATE CARD MID-RELEASE, BOTH DENSITIES ───────────────────────────────────────────────
    if pages {
        for (density, path, width, height) in DENSITIES {
            for theme in THEMES {
                let label = if *density == "desktop" {
                    "Desktop-settings-busy"
                } else {
                    "Settings-busy"
                };
                B::new(
                    "settings-busy",
                    format!("settings-busy.{density}.{theme}"),
                    density,
                    path,
                    vp(*width, *height),
                    theme,
                    "busy",
                    qs(theme, "idle", "3"),
                )
                .flag("busy=1")
                .page(label)
                .passes(&["pages", "theme"])
                .done(&mut out);
            }
        }
    }

    // ── F. EVERY RAIL PAGE, BY WALKING THE RAIL ──────────────────────────────────────────────────────
    // THE ONE GENUINELY DYNAMIC SURFACE: the page names come from the DOM (the walk clicks each rail
    // button and reads back which one the app reports active), so the plan carries ONE surface per
    // density and theme and says so with `dynamic`. A named exception rather than a silent gap — the
    // parity harness's stub page has no DOM, so neither side can enumerate it, and both say so.
    if pages {
        for (density, path, width, height) in DENSITIES {
            for theme in THEMES {
                let mut passes = vec!["pages", "theme"];
                if w("targets") {
                    passes.push("targets");
                }
                if w("press") {
                    passes.push("press");
                }
                B::new(
                    "rail",
                    format!("rail.{density}.{theme}"),
                    density,
                    path,
                    vp(*width, *height),
                    theme,
                    "rail",
                    qs(theme, "idle", "4"),
                )
                .dynamic("rail-labels")
                .passes(&passes)
                .done(&mut out);
            }
        }
    }

    // ── G. SIXTEEN SESSIONS, THE OPERATOR'S OWN COUNT ────────────────────────────────────────────────
    if pages {
        for (density, path, width, height) in DENSITIES {
            for theme in THEMES {
                let label = if *density == "desktop" {
                    "Desktop-16-sessions"
                } else {
                    "Terminal-16-sessions"
                };
                B::new(
                    "sessions-16",
                    format!("sessions-16.{density}.{theme}"),
                    density,
                    path,
                    vp(*width, *height),
                    theme,
                    "overflow",
                    qs(theme, "idle", "16"),
                )
                .page(label)
                .passes(&["pages"])
                .done(&mut out);
            }
        }
    }

    // ── H. THE FIXTURE SURFACES: the failure surfaces and a down monitor ─────────────────────────────
    // FOUR FIXTURES PER DENSITY. The fixture list is built by spreading the two themes of each flag
    // together, so within one density the order is fail-light, fail-dark, monitor-light,
    // monitor-dark — and the density loop is OUTSIDE it. Transcribing that order is what the
    // byte-for-byte bar requires.
    if pages {
        for (density, path, width, height) in DENSITIES {
            for (page_name, flag) in [
                ("Terminal-fail", "fail=1"),
                ("Settings-monitor-down", "monitor=down"),
            ] {
                for theme in THEMES {
                    let pname = if *density == "desktop" {
                        format!("Desktop-{page_name}-{theme}")
                    } else {
                        format!("{page_name}-{theme}")
                    };
                    let lands = if page_name.starts_with("Settings") {
                        "Settings"
                    } else {
                        "Terminal"
                    };
                    B::new(
                        "fixture",
                        format!("fixture.{density}.{page_name}.{theme}"),
                        density,
                        path,
                        vp(*width, *height),
                        theme,
                        "fixture",
                        qs(theme, "idle", "3"),
                    )
                    .flag(flag)
                    .var("lands", json!(lands))
                    .page(pname)
                    .passes(&["pages", "theme"])
                    .done(&mut out);
                }
            }
        }
    }

    // ── I..K. THE PLUGIN AND MONITOR STATES, PANEL DENSITY ONLY ──────────────────────────────────────
    // The desktop shell renders its own tab strip and no side list, so these states have no desktop
    // surface to photograph — measured, not assumed.
    if pages {
        for theme in THEMES {
            B::new(
                "plugins-running",
                format!("plugins-running.panel.{theme}"),
                "panel",
                "/panel/",
                vp(1280, 860),
                theme,
                "plugin-running",
                qs(theme, "idle", "3"),
            )
            .flag("pwrun=1")
            .page(format!("PluginRunning-{theme}"))
            .passes(&["pages", "theme"])
            .done(&mut out);
        }
        for theme in THEMES {
            B::new(
                "plugins-start-fail",
                format!("plugins-start-fail.panel.{theme}"),
                "panel",
                "/panel/",
                vp(1280, 860),
                theme,
                "plugin-fail",
                qs(theme, "idle", "3"),
            )
            .flag("pwstart=fail")
            .page(format!("PluginStartFail-{theme}"))
            .passes(&["pages", "theme"])
            .done(&mut out);
        }
        for theme in THEMES {
            for (dir, label) in [("up", "MonitorUp"), ("down", "MonitorDown")] {
                B::new(
                    "monitor",
                    format!("monitor.{dir}.panel.{theme}"),
                    "panel",
                    "/panel/",
                    vp(1280, 860),
                    theme,
                    format!("monitor-{dir}"),
                    qs(theme, "idle", "3"),
                )
                .flag(&format!("monitorchange={dir}"))
                .var("dir", json!(dir))
                .page(format!("{label}-{theme}"))
                .passes(&["pages", "theme"])
                .done(&mut out);
            }
        }
        for theme in THEMES {
            B::new(
                "boot-replaced",
                format!("boot-replaced.panel.{theme}"),
                "panel",
                "/panel/",
                vp(1280, 860),
                theme,
                "boot-replaced",
                qs(theme, "idle", "3"),
            )
            .flag("boot=replaced")
            .page(format!("BootReplaced-{theme}"))
            .passes(&["pages", "theme"])
            .done(&mut out);
        }
    }

    // ── L..P. THE APPROVAL GATE, THE LAST COMMAND, THE WARNED LOGS ───────────────────────────────────
    if pages {
        for theme in THEMES {
            B::new(
                "approval-off",
                format!("approval-off.panel.{theme}"),
                "panel",
                "/panel/",
                vp(1280, 860),
                theme,
                "approval-off",
                qs(theme, "idle", "3"),
            )
            .flag("appr=off")
            .page(format!("ApprovalOff-{theme}"))
            .passes(&["pages", "theme"])
            .done(&mut out);
        }
        for theme in THEMES {
            B::new(
                "exit-fail",
                format!("exit-fail.panel.{theme}"),
                "panel",
                "/panel/",
                vp(1280, 860),
                theme,
                "exit-fail",
                qs(theme, "pending", "6"),
            )
            .flag("exitfail=1")
            .page(format!("LastFail-{theme}"))
            .passes(&["pages", "theme"])
            .done(&mut out);
        }
        for theme in THEMES {
            B::new(
                "exit-fail-active",
                format!("exit-fail-active.panel.{theme}"),
                "panel",
                "/panel/",
                vp(1280, 860),
                theme,
                "exit-fail-active",
                qs(theme, "idle", "3"),
            )
            .flag("exitfail=active")
            .page(format!("LastFailActive-{theme}"))
            .passes(&["pages", "theme"])
            .done(&mut out);
        }
        for theme in THEMES {
            B::new(
                "logs-warn",
                format!("logs-warn.panel.{theme}"),
                "panel",
                "/panel/",
                vp(1280, 860),
                theme,
                "logs-warn",
                qs(theme, "idle", "3"),
            )
            .flag("logs=warn")
            .page(format!("LogsWarn-{theme}"))
            .passes(&["pages", "theme"])
            .done(&mut out);
        }
    }

    // ── Q. THE RECORD VIEW: two tabs, and the panel's second one TRIMMED ─────────────────────────────
    if pages {
        for (density, path, width, height) in DENSITIES {
            for theme in THEMES {
                let trimmeds: &[bool] = if *density == "panel" {
                    &[false, true]
                } else {
                    &[false]
                };
                for tab in ["Trajectory", "Path"] {
                    for trimmed in trimmeds {
                        let prefix = if *density == "desktop" {
                            "Desktop-"
                        } else {
                            ""
                        };
                        let page = format!(
                            "{prefix}{tab}-{theme}{}",
                            if *trimmed { "-trimmed" } else { "" }
                        );
                        let mut b = B::new(
                            "record",
                            format!("record.{density}.{tab}.{theme}.{trimmed}"),
                            density,
                            path,
                            vp(*width, *height),
                            theme,
                            "record",
                            qs(theme, "idle", "3"),
                        );
                        if *trimmed {
                            b = b.flag("trimmed=1");
                        }
                        b.var("tab", json!(tab))
                            .var("trimmed", json!(*trimmed))
                            .page(page)
                            .passes(&["pages"])
                            .done(&mut out);
                    }
                }
            }
        }
    }

    // ── R. THE ARCHIVE, TWO VIEWS ON ONE SURFACE ─────────────────────────────────────────────────────
    // The rows render and the trail is opened by a CLICK on the same page, so there is one surface here
    // and two measurements — not two navigations.
    if pages {
        for theme in THEMES {
            B::new(
                "archive",
                format!("archive.panel.{theme}"),
                "panel",
                "/panel/",
                vp(1280, 860),
                theme,
                "archive",
                qs(theme, "idle", "3"),
            )
            .flag("rows=50")
            .page(format!("ArchiveRows-{theme}"))
            .passes(&["pages", "theme"])
            .done(&mut out);
        }
    }

    // ── S. THE RUN JOURNAL ───────────────────────────────────────────────────────────────────────────
    if pages {
        for theme in THEMES {
            B::new(
                "runs",
                format!("runs.panel.{theme}"),
                "panel",
                "/panel/",
                vp(1280, 860),
                theme,
                "runs",
                qs(theme, "idle", "3"),
            )
            .page(format!("HistoryRuns-{theme}"))
            .passes(&["pages", "theme"])
            .done(&mut out);
        }
    }

    // ── T. THE ACKNOWLEDGEMENT AXIS: a curated pair, then a page where discovery is honest ───────────
    if w("ack") {
        for (density, path, width, height) in DENSITIES {
            for theme in THEMES {
                let label = if *density == "desktop" {
                    format!("Desktop-Settings-ack-{theme}")
                } else {
                    format!("Settings-ack-{theme}")
                };
                B::new(
                    "ack-settings",
                    format!("ack-settings.{density}.{theme}"),
                    density,
                    path,
                    vp(*width, *height),
                    theme,
                    "ack",
                    qs(theme, "idle", "3"),
                )
                .flag("slowms=900")
                .page(label)
                .passes(&["ack"])
                .done(&mut out);
            }
        }
        for theme in THEMES {
            B::new(
                "ack-memory",
                format!("ack-memory.panel.{theme}"),
                "panel",
                "/panel/",
                vp(1280, 860),
                theme,
                "ack",
                qs(theme, "idle", "3"),
            )
            .flag("slowms=900")
            .page(format!("Memory-ack-{theme}"))
            .passes(&["ack"])
            .done(&mut out);
        }
    }

    // ── U..W. THE GOAL BAR'S THREE STATES ────────────────────────────────────────────────────────────
    if pages {
        for (kind, flag, mode, label) in [
            ("no-goal", "goal=none", "no-goal", "NoGoal"),
            ("held", "held=1", "held", "Held"),
        ] {
            for (density, path, width, height) in DENSITIES {
                for theme in THEMES {
                    let page = if *density == "desktop" {
                        format!("Desktop-{label}-{theme}")
                    } else {
                        format!("{label}-{theme}")
                    };
                    B::new(
                        kind,
                        format!("{kind}.{density}.{theme}"),
                        density,
                        path,
                        vp(*width, *height),
                        theme,
                        mode,
                        qs(theme, "idle", "4"),
                    )
                    .flag(flag)
                    .page(page)
                    .passes(&["pages"])
                    .done(&mut out);
                }
            }
        }
        for (density, path, width, height) in DENSITIES {
            for theme in THEMES {
                let page = if *density == "desktop" {
                    format!("Desktop-Closed-{theme}")
                } else {
                    format!("Closed-{theme}")
                };
                B::new(
                    "closed",
                    format!("closed.{density}.{theme}"),
                    density,
                    path,
                    vp(*width, *height),
                    theme,
                    "closed",
                    qs(theme, "pending", "4"),
                )
                .page(page)
                .passes(&["pages"])
                .done(&mut out);
            }
        }
    }

    // ── X. TARGET SIZE, ONE RENDER PER DENSITY ───────────────────────────────────────────────────────
    // WCAG 2.5.8's own number, in the state the page rests in. Light only: a box does not change with
    // the theme, and the payload's comment says so.
    if pages {
        for (density, path, width, height) in DENSITIES {
            B::new(
                "targets",
                format!("targets.{density}"),
                density,
                path,
                vp(*width, *height),
                "light",
                "rest",
                qs("light", "relaxed", "3"),
            )
            .passes(&["targets"])
            .done(&mut out);
        }
    }

    // ── Y..Z. THE SINGLE-PASS BLOCKS AFTER THE BODY ──────────────────────────────────────────────────
    if w("unstyled") {
        for (density, path, width, height) in DENSITIES {
            B::new(
                "unstyled",
                format!("unstyled.{density}"),
                density,
                path,
                vp(*width, *height),
                "light",
                "relaxed",
                qs("light", "relaxed", "3"),
            )
            .page(*density)
            .passes(&["unstyled"])
            .done(&mut out);
        }
    }
    if w("hover") {
        for (density, path, width, height) in DENSITIES {
            for theme in THEMES {
                B::new(
                    "hover",
                    format!("hover.{density}.{theme}"),
                    density,
                    path,
                    vp(*width, *height),
                    theme,
                    "relaxed",
                    qs(theme, "relaxed", "3"),
                )
                .passes(&["hover"])
                .done(&mut out);
            }
        }
    }
    // THE MOTION PASS MEASURES THE SAME PAGE TWICE — once with the preference unset, once with it
    // emulated — because "nothing animates under reduce" is only evidence if something animates
    // without it. Two surfaces per density, not one.
    // THE RESET AFTER THE MOTION BLOCK, at a FIXED POINT IN THE PROGRAM: line 1170 of the payload
    // runs whether or not the motion pass was wanted, and BEFORE the reflow block. So it belongs to
    // whatever surface comes next — and to the plan's tail when nothing does, which is reachable:
    // `--passes=` on the panel wants no pass at all and still performs this step.
    let mut pending: Vec<serde_json::Value> = Vec::new();
    if w("motion") {
        for (density, path, width, height) in DENSITIES {
            for (i, mode) in ["motion", "motion-reduced"].into_iter().enumerate() {
                let media = if i == 0 {
                    json!({"reducedMotion": null})
                } else {
                    json!({"reducedMotion": "reduce"})
                };
                let mut b = B::new(
                    "motion",
                    format!("motion.{density}.{mode}"),
                    density,
                    path,
                    vp(*width, *height),
                    "light",
                    mode,
                    qs("light", "idle", "3"),
                )
                .media(media)
                .passes(&["motion"]);
                // THE SECOND RENDER IS A RELOAD OF THE PAGE THE FIRST ONE LOADED, not a fresh
                // navigation: the emulated preference only takes effect on a fresh style resolution,
                // and the payload resizes once per density, before the pair.
                if i == 1 {
                    b.s.set_viewport = false;
                    b.s.navigate = false;
                }
                b.done(&mut out);
            }
        }
    }
    pending.push(json!({"emulateMedia": {"reducedMotion": null}}));

    // REFLOW AT THE TWO WIDTHS WCAG 1.4.10 NAMES, panel density only: the desktop density is the
    // Electron window's shell rather than a browser viewport at 400% zoom, and its tab strip is the
    // standard's own toolbar exception.
    if w("reflow") {
        for width in [640u32, 320u32] {
            // The reset goes to the FIRST reflow surface when there is one — even when no surface
            // preceded it, which is what `--passes=reflow` produces.
            let pre = std::mem::take(&mut pending);
            let mut b = B::new(
                "reflow",
                format!("reflow.panel.{width}"),
                "panel",
                "/panel/",
                vp(width, 800),
                "light",
                "idle",
                qs("light", "idle", "3"),
            )
            .passes(&["reflow"]);
            b.s.pre = pre;
            b.done(&mut out);
        }
    }
    plan.post = pending;

    for s in out {
        plan.push(s);
    }
    plan
}
