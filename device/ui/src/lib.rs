//! The design tokens every Summrise surface renders through.
//!
//! **One palette, one vocabulary.** The console (``device/console``) and the device's own panel
//! (``device/device/src/panel.rs``) were two stylesheets that had to be kept in step by hand, and the
//! operator saw the result: a black terminal in a light page, on a product whose legacy panel had a
//! white one. The colours now live here, once; the panel's older names are aliases, not a second
//! palette.
//!
//! **The product has one theme: light.** The dark palette and the machinery that chose it are
//! deleted, not hidden — there is no media query, no `data-theme`, and no colour with a second
//! value. ``every_text_pair_reads_on_the_surface_it_sits_on`` in the console measures the contrast
//! of these tokens as a browser would.

use maud::{html, Markup};

/// The token block: one `:root`, one light palette, and the scale (type, weights, spacing, radii,
/// fonts) — which was already theme-free and is declared once.
pub const TOKENS: &str = r#":root {
  color-scheme: light;
  /* A3: navigation, context and work area have distinct visual planes. */
  --bg:#f4f7fb; --panel:#ffffff;
  --panel-2:color-mix(in srgb, var(--ink) 3%, var(--panel));
  --panel-3:color-mix(in srgb, var(--ink) 5%, var(--panel));
  --ink:#172c38; --muted:#4c5d73; --faint:#60718b;
  --line:#e1e7f0; --line-strong:#8190a5;
  --line-soft:color-mix(in srgb, var(--ink) 7%, var(--panel));
  --accent:#2559cc; --accent-solid:#2559cc; --accent-hover:#1c47a8;
  /* Concrete wash and selection colors also serve the terminal consumer. */
  --accent-soft:#edf2fc; --accent-fg:#ffffff; --accent-on-soft:#1c47a8;
  --ok:#23704b; --warn:#80590e; --danger:#a53a37;
  --ok-soft:color-mix(in srgb, var(--ok) 8%, var(--panel));
  --warn-soft:color-mix(in srgb, var(--warn) 9%, var(--panel));
  --danger-soft:color-mix(in srgb, var(--danger) 8%, var(--panel));
  --focus-ring:#2559cc;
  --sidebar:#fff; --sidebar-ink:#172c38; --sidebar-muted:#60718b;
  --sidebar-selected:#edf2ff; --sidebar-focus:#2559cc;
  --dur-interaction:120ms;
  --shadow:0 1px 3px rgba(23,44,56,.05);
  --shadow-pop:0 8px 32px -8px rgba(16,29,48,.2),0 2px 8px rgba(16,29,48,.08);
  --shadow-1:var(--shadow); --shadow-2:0 8px 24px -16px rgba(16,29,48,.2);
  --elev-segment:0 1px 3px rgba(23,44,56,.12);
  --elev-2:0 24px 64px -20px rgba(16,29,48,.3),0 2px 8px rgba(16,29,48,.08);
  --ring:0 0 0 2px var(--focus-ring);
  --radius:12px; --radius-sm:10px; --radius-sm2:8px; --radius-xs:6px;
  --radius-lg:16px; --radius-pill:999px;
  --font:system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Hiragino Sans GB","Microsoft YaHei",sans-serif;
  --font-serif:var(--font);
  --mono:ui-monospace,"SF Mono",Consolas,"Cascadia Mono",monospace;
  --font-mono:var(--mono);
  --fs-2xs:12px; --fs-xs:12px; --fs-sm:12px; --fs-base:13px;
  --fs-md:14px; --fs-lg:18px; --fs-xl:28px; --fs-metric:32px;
  --fw-regular:400; --fw-medium:500; --fw-semi:600; --fw-bold:700; --fw-display:650;
  --tracking-display:-0.025em; --tracking-tight:-0.015em;
  --tracking-label:0.06em; --tracking-eyebrow:0.08em; --tracking-none:normal;
  --sp-0-5:2px; --sp-1:4px; --sp-1-5:6px; --sp-2:8px; --sp-2-5:10px;
  --sp-3:12px; --sp-3-5:14px; --sp-4:16px; --sp-5:24px; --sp-6:32px;
  --control-h:32px; --control-pad-y:5px;
  --scrim:color-mix(in srgb, #101d30 36%, transparent);
  --lh-body:1.5; --lh-tight:1.3; --lh-mark:1;
  --lh-page:36px; --lh-section:26px; --lh-metric:40px;
  --measure:66ch;
  --term-canvas:#ffffff; --term-selection:rgba(37,89,204,.2);
  --term-black:#1d1d1f; --term-white:#44403c;
  --term-red:#b3261e; --term-green:#1d7a3c; --term-yellow:#a8620a;
  --term-blue:#1f5fbf; --term-magenta:#8a3ffc; --term-cyan:#0e7490;
  --surface:var(--panel); --surface-chip:var(--panel-3);
  --surface-recessed:color-mix(in srgb, var(--ink) 7%, var(--panel));
  --ink-muted:var(--muted); --ink-faint:var(--faint); --error:var(--danger);
  --error-soft:var(--danger-soft); --hover:var(--panel-2);
}
"#;

/// The rules for the components this crate renders, so the two surfaces draw them the same way.
///
/// A component whose markup lives here but whose CSS lived in each surface would drift exactly the way
/// the palettes did; the pair travels together.
pub const COMPONENTS: &str = r#"
/* Shared status, section and shell components; surface-specific geometry follows. */
.chip { display:inline-flex; align-items:center; gap:var(--sp-1-5); min-height:24px; padding:var(--sp-0-5) var(--sp-2); border:1px solid transparent; border-radius:var(--radius-pill); background:var(--panel-2); color:var(--muted); font-size:var(--fs-base); font-weight:var(--fw-medium); line-height:var(--lh-tight); white-space:nowrap; }
.chip .dot { width:6px; height:6px; border-radius:50%; background:currentColor; }
.chip.ok, .chip.online { background:var(--ok-soft); color:var(--ok); }
.chip.warn { background:var(--warn-soft); color:var(--warn); }
.chip.off, .chip.offline, .chip.muted { background:var(--panel-2); color:var(--muted); }
.chip.fail, .chip.alert, .chip.failed { background:var(--danger-soft); color:var(--danger); }
.chip.brand { background:var(--accent-soft); color:var(--accent-on-soft); }
.chip.plain { padding:0; background:none; color:var(--muted); }
.card { margin:0 0 var(--sp-5); border:1px solid var(--line); border-radius:var(--radius); background:var(--panel); box-shadow:none; }
.card-head { display:flex; align-items:center; flex-wrap:wrap; gap:var(--sp-2); min-height:48px; padding:var(--sp-3) var(--sp-4); border-bottom:1px solid var(--line); }
.card-head h2 { margin:0; color:var(--ink); font-size:var(--fs-lg); line-height:var(--lh-section); font-weight:var(--fw-semi); letter-spacing:var(--tracking-display); }
.card-head .actions { flex:none; display:flex; align-items:center; gap:var(--sp-2); margin-left:auto; }
.card-body { padding:var(--sp-4); }
.card-body > :last-child { margin-bottom:0; }
.empty { display:flex; flex-direction:column; align-items:flex-start; justify-content:center; gap:var(--sp-3); padding:var(--sp-5) var(--sp-4); color:var(--muted); font-size:var(--fs-base); text-align:left; }
.empty-icon { display:grid; place-items:center; width:38px; height:38px; border-radius:var(--radius-sm); background:var(--panel-2); color:var(--faint); }
.empty p { max-width:var(--measure); }
.empty .btn { margin-top:var(--sp-0-5); }
/* Console chooses a rail beside its column; Device chooses a column shell. */
.app { display:flex; min-height:100vh; background:var(--bg); }
.topbar { flex:none; display:flex; align-items:center; gap:var(--sp-3); min-height:64px; padding:0 var(--sp-5); background:var(--panel); border-bottom:1px solid var(--line); }
.topbar h1 { margin:0; flex:none; font-size:var(--fs-lg); font-weight:var(--fw-semi); letter-spacing:var(--tracking-display); white-space:nowrap; }
.topbar .context { display:inline-flex; align-items:center; gap:var(--sp-1-5); min-width:0; max-width:36ch; padding:var(--sp-1) var(--sp-2); border:1px solid var(--line); border-radius:var(--radius-pill); background:var(--panel-2); color:var(--muted); font-size:var(--fs-base); }
.topbar .context .dot { flex:none; width:6px; height:6px; border-radius:50%; background:var(--ok); }
.topbar .context.offline .dot { background:var(--faint); }
.topbar .spacer { flex:1; }
.statusbar { flex:none; display:flex; align-items:center; gap:var(--sp-2); min-height:32px; padding:0 var(--sp-4); background:var(--panel); border-top:1px solid var(--line); color:var(--faint); font-size:var(--fs-base); white-space:nowrap; }
.statusbar .middle { min-width:0; overflow:hidden; text-overflow:ellipsis; color:var(--muted); }
.statusbar .spacer { flex:1; }
.statusbar .sep { color:var(--line-strong); }
.statusbar .side { color:var(--faint); font-size:var(--fs-sm); }
/* Device SSR: a local workspace, scoped away from the Console frame. */
.local-workspace { --device-gutter:32px; --device-content-max:1440px; --fs-goal:28px; --fs-purpose:18px; }
body.local-workspace { display:grid; grid-template-columns:232px minmax(0,1fr); margin:0; min-height:100vh; background:var(--bg); color:var(--ink); font:var(--fs-md)/var(--lh-body) var(--font); overflow-wrap:anywhere; }
.local-workspace *, .local-workspace *::before, .local-workspace *::after { box-sizing:border-box; }
.local-workspace [hidden] { display:none !important; }
.local-workspace-body { min-width:0; display:flex; flex-direction:column; }
.local-sidebar { position:sticky; top:0; align-self:start; min-height:100vh; padding:var(--sp-5) var(--sp-4); background:var(--sidebar); color:var(--sidebar-ink); border-right:1px solid var(--line); }
.local-brand { display:flex; align-items:center; gap:var(--sp-3); margin-bottom:var(--sp-6); font-size:var(--fs-lg); font-weight:var(--fw-display); }
.local-brand svg { width:28px; height:28px; flex:none; }
.local-scope { margin:var(--sp-5) var(--sp-3); color:var(--sidebar-muted); font-size:var(--fs-sm); }
.local-nav-label { padding:0 var(--sp-3); color:var(--sidebar-muted); font-size:var(--fs-sm); letter-spacing:var(--tracking-label); text-transform:uppercase; }
.local-nav { display:flex; flex-direction:column; gap:var(--sp-2); }
.local-nav a { display:flex; align-items:center; gap:var(--sp-3); min-height:40px; padding:var(--sp-2) var(--sp-3); border:1px solid transparent; border-radius:var(--radius-xs); color:var(--sidebar-muted); font-size:var(--fs-md); font-weight:var(--fw-medium); text-decoration:none; }
.local-nav a:hover { background:var(--sidebar-selected); color:var(--sidebar-ink); }
.local-nav a[aria-current="page"] { background:var(--sidebar-selected); color:var(--accent); }
.local-nav-icon { display:grid; place-items:center; width:20px; flex:none; font-family:var(--font-mono); font-size:var(--fs-base); }
.local-mobile-menu { display:none; }
.local-workspace .device-skip { position:absolute; width:1px; height:1px; margin:-1px; overflow:hidden; clip-path:inset(50%); white-space:nowrap; }
.local-workspace .device-skip:focus { position:fixed; top:8px; left:8px; z-index:100; width:auto; height:auto; margin:0; padding:var(--sp-3) var(--sp-4); overflow:visible; clip-path:none; background:var(--panel); color:var(--accent); border:1px solid var(--line-strong); border-radius:var(--radius-xs); }
.local-workspace :focus-visible { outline:2px solid var(--focus-ring); outline-offset:2px; }
.local-sidebar :focus-visible { outline-color:var(--sidebar-focus); }
.local-workspace .local-topbar { display:flex; flex-wrap:wrap; align-items:center; gap:var(--sp-3); min-height:64px; padding:var(--sp-3) var(--device-gutter); border-bottom:1px solid var(--line); background:var(--panel); }
.local-breadcrumb { display:flex; flex-wrap:wrap; gap:var(--sp-2); align-items:center; color:var(--muted); font-size:var(--fs-base); }
.local-breadcrumb strong { color:var(--ink); font-weight:var(--fw-semi); }
.local-workspace main { width:100%; min-width:0; max-width:calc(var(--device-content-max) + 2 * var(--device-gutter)); margin:0 auto; padding:var(--sp-6) var(--device-gutter); }
.local-workspace .page-head { display:flex; align-items:start; justify-content:space-between; flex-wrap:wrap; gap:var(--sp-5); margin-bottom:var(--sp-6); }
.local-workspace h1 { font-size:var(--fs-xl); line-height:var(--lh-page); font-weight:var(--fw-display); letter-spacing:var(--tracking-display); }
.local-workspace h2 { font-size:var(--fs-lg); line-height:var(--lh-section); }
.local-task-layout { display:grid; grid-template-columns:minmax(0,1fr) minmax(280px,340px); align-items:start; gap:var(--sp-6); }
.local-task-layout > * { min-width:0; }
.local-primary, .local-support { display:grid; gap:var(--sp-5); }
.local-support { padding-left:var(--sp-5); border-left:1px solid var(--line); }
.local-workspace button, .local-workspace select, .local-workspace textarea, .local-workspace input:not([type="hidden"]):not([type="checkbox"]):not([type="radio"]), .local-workspace .button { min-height:var(--control-h); }
.local-workspace button, .local-workspace input, .local-workspace select, .local-workspace textarea { font:inherit; }
.local-workspace summary { min-height:var(--control-h); cursor:pointer; font-size:var(--fs-base); }
.local-workspace .terminal-link, .local-workspace .ledger-receipt-link, .local-workspace .rail-session-name a, .local-workspace .activity-filter { min-height:var(--control-h); }
.local-workspace a { text-underline-offset:.18em; }
.local-workspace pre { max-width:100%; }
@media (max-width:1200px) { .local-task-layout { grid-template-columns:minmax(0,1fr); } .local-support { padding-left:0; padding-top:var(--sp-5); border-left:0; border-top:1px solid var(--line); } }
@media (min-width:701px) and (max-width:1000px) {
  body.local-workspace { grid-template-columns:72px minmax(0,1fr); }
  .local-sidebar { padding:var(--sp-5) var(--sp-2); }
  .local-brand { justify-content:center; }
  .local-sidebar .local-brand-name, .local-sidebar .local-nav-text, .local-sidebar .local-nav-label, .local-sidebar .local-scope { position:absolute; width:1px; height:1px; overflow:hidden; clip-path:inset(50%); }
  .local-nav a { justify-content:center; padding:var(--sp-3) var(--sp-2); }
}
@media (max-width:700px) {
  .local-workspace { --device-gutter:16px; --control-h:40px; }
  body.local-workspace { display:block; }
  .local-sidebar { display:none; }
  .local-mobile-menu { display:block; padding:var(--sp-3) var(--sp-4); background:var(--sidebar); color:var(--sidebar-ink); border-bottom:1px solid var(--line); }
  .local-mobile-menu > summary { min-height:40px; padding:var(--sp-2) 0; font-size:var(--fs-md); font-weight:var(--fw-semi); cursor:pointer; }
  .local-mobile-menu .local-nav { padding:var(--sp-3) 0; }
  .local-workspace main { padding-top:var(--sp-5); }
  .local-workspace .page-head { margin-bottom:var(--sp-5); gap:var(--sp-4); }
}
@media (forced-colors:active) {
  .local-workspace :focus-visible { outline:2px solid CanvasText; outline-offset:2px; }
  .local-nav a[aria-current="page"] { border-color:CanvasText; }
}
@media (prefers-reduced-motion:reduce) { .local-workspace *, .local-workspace *::before, .local-workspace *::after { animation:none !important; transition:none !important; scroll-behavior:auto !important; } }
"#;

/// The two destinations served by this machine's own loopback UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevicePage {
    Machine,
    Activity,
}

/// Local navigation has no fleet or access controls and never carries a panel token.
/// The compact menu is a native disclosure: it remains operable without JavaScript.
pub fn device_navigation(active: DevicePage, brand: Markup) -> Markup {
    let skip = match active {
        DevicePage::Machine => "#content",
        DevicePage::Activity => "#main",
    };
    let links = html! {
        a href="/panel" aria-current=[(active == DevicePage::Machine).then_some("page")] title="This machine" {
            span class="local-nav-icon" aria-hidden="true" { "M" }
            span class="local-nav-text" { "This machine" }
        }
        a href="/activity" aria-current=[(active == DevicePage::Activity).then_some("page")] title="Activity" {
            span class="local-nav-icon" aria-hidden="true" { "A" }
            span class="local-nav-text" { "Activity" }
        }
    };
    html! {
        a class="device-skip" href=(skip) { "Skip to content" }
        aside class="local-sidebar" aria-label="This machine workspace" {
            div class="local-brand" { (brand) span class="local-brand-name" { "Summrise" } }
            p class="local-nav-label" { "Local workspace" }
            nav class="local-nav" aria-label="Local navigation" { (&links) }
            p class="local-scope" { "Only this machine" br; "Loopback access" }
        }
        details class="local-mobile-menu" {
            summary { "Summrise · Menu" }
            nav class="local-nav" aria-label="Local navigation" { (links) }
        }
    }
}

/// What a chip is saying.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Serving, connected, healthy.
    Ok,
    /// Up but degraded, or a note worth reading.
    Warn,
    /// Not connected; not an error.
    Off,
    /// A failure the operator must see.
    Alert,
    /// The product's own accent: a count, a new thing.
    Brand,
}

impl Tone {
    /// The class this tone renders as — `.chip.<class>`.
    pub fn class(self) -> &'static str {
        match self {
            Tone::Ok => "ok",
            Tone::Warn => "warn",
            Tone::Off => "off",
            Tone::Alert => "alert",
            Tone::Brand => "brand",
        }
    }
}

/// A status pill: a dot and a label, escaped by construction.
pub fn chip(tone: Tone, label: &str) -> Markup {
    html! {
        span class=(format!("chip {}", tone.class())) {
            span class="dot" aria-hidden="true" {}
            (label)
        }
    }
}

/// A card's heading row: a title, whatever belongs beside it, and its actions.
///
/// The three parts are separate arguments because the console puts a counter chip and a button in that
/// row, and the panel puts only a chip; a component that took one bucket would be a layout nobody owns.
pub fn card_head(title: &str, meta: Markup, actions: Markup) -> Markup {
    html! {
        div class="card-head" {
            h2 { (title) }
            (meta)
            div class="actions" { (actions) }
        }
    }
}

/// A whole card: the container, its heading row, and a body.
///
/// The body is markup, not a string, and the caller writes it inline — a section that only has a body
/// is `card(title, Markup::default(), Markup::default(), html! { … })`, and a page that has seven of
/// them reads as seven cards because that is what it renders.
pub fn card(title: &str, meta: Markup, actions: Markup, body: Markup) -> Markup {
    html! {
        section class="card" {
            (card_head(title, meta, actions))
            div class="card-body" { (body) }
        }
    }
}

/// The one empty state: a muted icon, a sentence, and at most one action.
pub fn empty_state(icon: Markup, message: &str, action: Markup) -> Markup {
    html! {
        div class="empty" {
            div class="empty-icon" aria-hidden="true" { (icon) }
            p { (message) }
            (action)
        }
    }
}

/// The tokens followed by a surface's own component rules.
pub fn style(components: &str) -> String {
    let mut css = String::with_capacity(TOKENS.len() + COMPONENTS.len() + components.len() + 1);
    css.push_str(TOKENS);
    css.push_str(COMPONENTS);
    css.push_str(components);
    css
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both vocabularies are present: the console's names and the panel's aliases.
    #[test]
    fn one_block_names_both_vocabularies() {
        for token in [
            "--panel",
            "--surface",
            "--ink",
            "--ink-muted",
            "--error",
            "--term-canvas",
        ] {
            assert!(TOKENS.contains(token), "{token} is declared");
        }
    }

    /// The sheet declares one palette and one scheme; the dark machinery is gone.
    #[test]
    fn the_sheet_declares_one_palette() {
        assert!(TOKENS
            .lines()
            .any(|line| line.trim() == "color-scheme: light;"));
        assert!(!TOKENS.contains("prefers-color-scheme"));
        assert!(!TOKENS.contains("data-theme"));
        assert_eq!(TOKENS.matches(":root").count(), 1);
        assert!(TOKENS.contains("--font-serif:"));
    }
}
