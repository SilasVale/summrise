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

/// The token block: `:root` for the light theme and the same names again inside the dark media query.
pub const TOKENS: &str = r#":root {
  color-scheme: light dark;
  --bg:#f6f6f7; --panel:#ffffff; --panel-2:#f4f4f5; --panel-3:#ececef;
  --ink:#18181b; --muted:#5d5e67; --faint:#74757f;
  --line:#e6e6e9; --line-strong:#d5d5da;
  --accent:#bf3a0a; --accent-solid:#b03a0a; --accent-hover:#963008;
  --accent-soft:#fff0e6; --accent-fg:#ffffff;
  --ok:#1d7a3c; --warn:#a8620a; --ok-soft:#e7f6ec;
  --danger:#b3261e; --danger-soft:#fdecea;
  --rail:#17181c; --rail-line:rgba(255,255,255,.08); --rail-ink:#9a9ba4;
  --rail-ink-hover:#f4f4f5; --rail-focus:#ffb86b;
  --shadow:0 1px 2px rgba(24,24,27,.04), 0 10px 30px -24px rgba(24,24,27,.30);
  --radius:12px; --radius-sm:10px; --radius-xs:8px;
  --font:-apple-system,"SF Pro Text","PingFang SC","Hiragino Sans GB","Microsoft YaHei","Segoe UI",Roboto,sans-serif;
  --mono:"SF Mono",ui-monospace,"JetBrains Mono",Consolas,"Cascadia Mono",monospace;
  /* The terminal is a canvas that follows the page, the way summrise-legacy's does (`--term-canvas-bg`).
     An ANSI palette tuned for a light canvas is not the dark one with the colours swapped: these are
     the legacy's, so a `ls` or a `git status` reads the same in both products. */
  --term-canvas:#ffffff; --term-selection:rgba(191,58,10,.18);
  --term-red:#b3261e; --term-green:#1d7a3c; --term-yellow:#a8620a;
  --term-blue:#1f5fbf; --term-magenta:#8a3ffc; --term-cyan:#0e7490;
  /* ---- one vocabulary, two names ---- */
  /* The device's own panel was written against older names (`--surface`, `--ink-muted`, `--error`).
     They are aliases now rather than a second palette: a token defined twice is a token that drifts. */
  --surface:var(--panel); --surface-recessed:var(--panel-2); --surface-chip:var(--panel-3);
  --ink-muted:var(--muted); --ink-faint:var(--faint); --error:var(--danger);
  --error-soft:var(--danger-soft); --warn-soft:#fff4e5; --ok-soft:var(--ok-soft);
  --font-mono:var(--mono); --radius-lg:16px;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg:#101115; --panel:#191a1f; --panel-2:#212228; --panel-3:#2a2b32;
    --ink:#ececf0; --muted:#a6a7b1; --faint:#8e909a;
    --line:rgba(255,255,255,.08); --line-strong:rgba(255,255,255,.15);
    --accent:#ffa36b; --accent-solid:#b03a0a; --accent-hover:#c2410c;
    --accent-soft:#33211a; --accent-fg:#ffffff;
    --ok:#6edc8b; --warn:#e0a44a; --ok-soft:rgba(46,160,73,.16);
    --danger:#ff8b84; --danger-soft:rgba(190,40,30,.18);
    --rail:#0b0c0e; --rail-line:rgba(255,255,255,.07); --rail-ink:#8f9099;
    --rail-ink-hover:#ffffff; --rail-focus:#ffb86b;
    --shadow:0 1px 2px rgba(0,0,0,.45), 0 16px 36px -26px rgba(0,0,0,.9);
    --term-canvas:#131418; --term-selection:rgba(255,163,107,.24);
    --term-red:#ff8b84; --term-green:#6edc8b; --term-yellow:#e0a44a;
    --term-blue:#7fb0ff; --term-magenta:#c9a3ff; --term-cyan:#6fd3e6;
  }
}
"#;

/// The tokens followed by a surface's own component rules.
pub fn style(components: &str) -> String {
    let mut css = String::with_capacity(TOKENS.len() + components.len() + 1);
    css.push_str(TOKENS);
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
