//! The design tokens every Summrise surface renders through.
//!
//! **One palette, two vocabularies.** The console (``device/console``) and the device's own panel
//! (``device/device/src/panel.rs``) were two stylesheets that had to be kept in step by hand, and the
//! operator saw the result: a black terminal in a light page, on a product whose legacy panel had a
//! white one. The colours now live here, once; the panel's older names are aliases, not a second
//! palette.
//!
//! Every colour has a dark value, because the operator's desktop follows the system theme and a token
//! with only a light value would keep its surface light in the dark. ``every_text_colour_reads_on_the
//! _surface_it_sits_on`` in the console measures the contrast of these tokens as a browser would.

use maud::{html, Markup};

/// The token block: `:root` for the light theme, with every colour and the terminal palette restated
/// inside the dark media query. The scale (type, weights, spacing, radii, fonts) is theme-free and
/// therefore declared once.
pub const TOKENS: &str = r#":root {
  color-scheme: light dark;
  --bg:#fafafa; --panel:#ffffff; --panel-2:#fafafa; --panel-3:#f4f4f5;
  --ink:#18181b; --muted:#71717a; --faint:#696a74;
  --line:rgba(0,0,0,.08); --line-strong:rgba(0,0,0,.14); --line-soft:rgba(0,0,0,.05);
  --accent:#bf3a0a; --accent-solid:#b03a0a; --accent-hover:#963008;
  --accent-soft:#fff0e6; --accent-fg:#ffffff;
  --ok:#1d7a3c; --warn:#a05d08; --ok-soft:#e7f6ec; --warn-soft:#fff4e5;
  --danger:#b3261e; --danger-soft:#fdecea; --focus-ring:#bf3a0a;
  --rail:#17181c; --rail-line:rgba(255,255,255,.08); --rail-ink:#9a9ba4;
  --rail-ink-hover:#f4f4f5; --rail-focus:#ffb86b;
  --shadow:0 1px 3px rgba(0,0,0,.06),0 1px 2px rgba(0,0,0,.04);
  --shadow-lg:0 12px 32px rgba(0,0,0,.12);
  --shadow-1:var(--shadow);
  --shadow-2:0 6px 24px -18px rgba(16,23,33,.45);
  --radius:14px; --radius-sm:10px; --radius-sm2:8px; --radius-xs:6px;
  --radius-lg:20px; --radius-pill:999px;
  --font:-apple-system,"SF Pro Text","PingFang SC","Hiragino Sans GB","Microsoft YaHei","Segoe UI",Roboto,sans-serif;
  --mono:"SF Mono",ui-monospace,"JetBrains Mono",Consolas,"Cascadia Mono",monospace;
  --font-mono:var(--mono);

  /* Seven type sizes, one weight family, one spacing rhythm. The React sheet used to carry its
     own copies of these; they live here now so the two surfaces cannot choose different numbers. */
  --fs-2xs:10px; --fs-xs:11px; --fs-sm:12px; --fs-base:13px;
  --fs-md:14px; --fs-lg:17px; --fs-xl:22px;
  --fw-regular:400; --fw-medium:500; --fw-semi:600; --fw-bold:700;
  --tracking-display:-0.011em; --tracking-micro:0.014em;
  --sp-0-5:2px; --sp-1:4px; --sp-1-5:6px; --sp-2:8px; --sp-2-5:10px;
  --sp-3:12px; --sp-3-5:14px; --sp-4:16px; --sp-5:24px;

  /* The terminal is a canvas that follows the page, the way summrise-legacy's does (`--term-canvas-bg`).
     An ANSI palette tuned for a light canvas is not the dark one with the colours swapped: these are
     the legacy's, so a `ls` or a `git status` reads the same in both products. */
  --term-canvas:#ffffff; --term-selection:rgba(191,58,10,.18);
  --term-black:#1d1d1f; --term-white:#44403c;
  --term-red:#b3261e; --term-green:#1d7a3c; --term-yellow:#a8620a;
  --term-blue:#1f5fbf; --term-magenta:#8a3ffc; --term-cyan:#0e7490;
  /* ---- one vocabulary, two names ---- */
  /* The device's own panel was written against older names (`--surface`, `--ink-muted`, `--error`).
     They are aliases now rather than a second palette: a token defined twice is a token that drifts. */
  --surface:var(--panel); --surface-recessed:var(--panel-2); --surface-chip:var(--panel-3);
  --ink-muted:var(--muted); --ink-faint:var(--faint); --error:var(--danger);
  --error-soft:var(--danger-soft); --hover:var(--panel-2);
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg:#131418; --panel:#1c1d22; --panel-2:#16171b; --panel-3:#23242a;
    --ink:#ecedef; --muted:#a2a3ac; --faint:#8e909a;
    --line:rgba(255,255,255,.08); --line-strong:rgba(255,255,255,.14); --line-soft:rgba(255,255,255,.05);
    --accent:#ffa36b; --accent-solid:#b03a0a; --accent-hover:#c2410c;
    --accent-soft:#3d2817; --accent-fg:#ffffff;
    --ok:#6edc8b; --warn:#e0a44a; --ok-soft:rgba(46,160,73,.16);
    --warn-soft:#3a2a14; --danger:#ff8b84; --danger-soft:#2a1512;
    --focus-ring:#ffa36b;
    --rail:#0b0c0e; --rail-line:rgba(255,255,255,.07); --rail-ink:#8f9099;
    --rail-ink-hover:#ffffff; --rail-focus:#ffb86b;
    --shadow:0 1px 2px rgba(0,0,0,.4);
    --shadow-lg:0 12px 32px rgba(0,0,0,.45);
    --shadow-1:var(--shadow);
    --shadow-2:0 8px 24px -18px rgba(0,0,0,.8);
    --hover:#2a2b32;
    --term-canvas:#131418; --term-selection:rgba(255,163,107,.24);
    --term-black:#2a2c33; --term-white:#e8e9ec;
    --term-red:#ff8b84; --term-green:#6edc8b; --term-yellow:#e0a44a;
    --term-blue:#7fb0ff; --term-magenta:#c9a3ff; --term-cyan:#6fd3e6;
  }
}
"#;

/// The rules for the components this crate renders, so the two surfaces draw them the same way.
///
/// A component whose markup lives here but whose CSS lived in each surface would drift exactly the way
/// the palettes did; the pair travels together.
pub const COMPONENTS: &str = r#"
/* ---- chips: a status pill, the same shape on every surface ---- */
.chip { display:inline-flex; align-items:center; gap:6px; height:21px; padding:0 8px; border:1px solid transparent; border-radius:999px; background:var(--panel-2); color:var(--muted); font-size:12px; font-weight:600; white-space:nowrap; }
.chip .dot { width:6px; height:6px; border-radius:50%; background:currentColor; }
.chip.ok { background:var(--ok-soft); color:var(--ok); }
.chip.warn { background:var(--warn-soft); color:var(--warn); }
.chip.off { background:var(--panel-2); color:var(--muted); }
.chip.alert { background:var(--danger-soft); color:var(--danger); }
.chip.brand { background:var(--accent-soft); color:var(--accent); }
/* The console's historical names for the same tones. They stay as aliases while its pages move to the
   canonical classes above; a rename that breaks sixty call sites is not a design-system improvement. */
.chip.online { background:var(--ok-soft); color:var(--ok); }
.chip.offline { background:var(--panel-2); color:var(--muted); }
.chip.muted { background:var(--panel-2); color:var(--muted); }
.chip.failed { background:var(--danger-soft); color:var(--danger); }
.chip.plain { padding:0; background:none; color:var(--muted); }
/* ---- a card's heading row and the one empty state ---- */
.card { margin:0 0 20px; border:1px solid var(--line); border-radius:var(--radius); background:var(--panel); box-shadow:var(--shadow); }
.card-head { display:flex; align-items:center; gap:8px; min-height:46px; padding:12px 16px; border-bottom:1px solid var(--line); }
.card-head h2 { margin:0; color:var(--muted); font-size:13px; font-weight:600; letter-spacing:.07em; text-transform:uppercase; }
.card-head .actions { flex:none; display:flex; align-items:center; gap:8px; margin-left:auto; }
.empty { display:flex; flex-direction:column; align-items:center; justify-content:center; gap:10px; padding:40px 20px; color:var(--muted); font-size:13px; text-align:center; }
.empty-icon { display:grid; place-items:center; width:38px; height:38px; border-radius:var(--radius-sm); background:var(--panel-2); color:var(--faint); }
.card-body { padding:16px; }
.card-body > :last-child { margin-bottom:0; }
.empty p { max-width:48ch; }
/* ---- the page shell's two fixed rows ---- */
/* THE DIRECTION IS THE SURFACE'S, NOT THE SHELL'S: the console puts a rail BESIDE its column and the
   panel is a column on its own. A shared `flex-direction` here once stacked the console's rail above its
   top bar, which is the layout bug the screenshots caught on 2026-10-28. */
.app { display:flex; min-height:100vh; background:var(--bg); }
.topbar { flex:none; display:flex; align-items:center; gap:12px; height:56px; padding:0 24px; background:var(--panel); border-bottom:1px solid var(--line); }
.topbar h1 { margin:0; flex:none; font-size:18px; font-weight:600; letter-spacing:-.015em; white-space:nowrap; }
.topbar .context { display:inline-flex; align-items:center; gap:7px; min-width:0; max-width:36ch; padding:3px 9px; border:1px solid var(--line); border-radius:999px; background:var(--panel-2); color:var(--muted); font-size:12px; }
.topbar .context .dot { flex:none; width:6px; height:6px; border-radius:50%; background:var(--ok); }
.topbar .context.offline .dot { background:var(--faint); }
.topbar .spacer { flex:1; }
.statusbar { flex:none; display:flex; align-items:center; gap:8px; height:28px; padding:0 18px; background:var(--panel); border-top:1px solid var(--line); color:var(--faint); font-size:12px; white-space:nowrap; }
.statusbar .middle { min-width:0; overflow:hidden; text-overflow:ellipsis; color:var(--muted); }
.statusbar .spacer { flex:1; }
.statusbar .sep { opacity:.55; }
.statusbar .side { color:var(--faint); text-transform:uppercase; letter-spacing:.1em; font-size:11px; }
.empty .btn { margin-top:2px; }
"#;

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

    /// A dark block exists for the tokens that only got a light value.
    #[test]
    fn the_dark_block_restates_the_surfaces() {
        let dark = TOKENS
            .find("@media (prefers-color-scheme: dark)")
            .expect("a dark block");
        let block = &TOKENS[dark..];
        for token in ["--bg:", "--panel:", "--ink:"] {
            assert!(block.contains(token), "{token} is redarkened");
        }
    }
}
