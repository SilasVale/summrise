//! page.rs — the landing page's document, rendered by Rust at BUILD time.
//!
//! THE TEXT IS IN THE HTML. This renderer runs on the HOST, when the landing is
//! built, so the crawler and the first paint both get a complete document and no
//! byte of wasm is on the critical path. `src/interactive.rs` then hydrates the
//! two behaviours. That is the whole design, and it is the only one that keeps a
//! static page fast — measured with the binary held back two seconds, first paint
//! is unchanged (92 ms against 96 ms for the JavaScript page).
//!
//! # The contract with the caller
//!
//! `render` takes THREE URL STRINGS AND INTERPOLATES THEM VERBATIM. It does not
//! whitelist them and it does not escape them, and that is deliberate: the three
//! values do not exist at build time. Two are built from the request's own origin
//! and the third comes from a per-deployment var, so the policy on what may be put
//! in them can only run where they arrive — the worker's request boundary, in
//! `index/src/page.js`, which is the ONE copy of `safePageUrl` + `escHtml`.
//!
//! The spike carried a WHATWG whitelist here, ported with the `url` crate, and it
//! was right for a renderer that runs per request. This one does not: at build
//! time it would be handed a placeholder token, and `safe_page_url(token)` passes
//! a token through, because a token resolves to https against the base — a check
//! that checked nothing. So `gen` passes TOKENS and ASSERTS they survived
//! verbatim (a mangled placeholder fails the build), and `verify.mjs` passes the
//! real values the worker computes, which is where the bytes are compared against
//! the page this replaces.
//!
//! Fidelity bar: for the same three values this emits the same bytes as
//! `index/src/page.js` did, except at the two places the migration is *about*
//! (the theme-toggle attribute and the closing script block). `verify.mjs`
//! asserts that, so "the same numbers" is not a claim — it is a diff.

use std::fmt::Write as _;

/// The slot `gen` leaves for the console URL, and the worker fills.
pub const CONSOLE_SLOT: &str = "{{CONSOLE_URL}}";
/// The slot for the release host's versionless tgz alias.
pub const INSTALLER_SLOT: &str = "{{INSTALLER_URL}}";
/// The slot for the Windows installer — present only in the arm that has one.
pub const SETUP_SLOT: &str = "{{SETUP_URL}}";

/// The page's CSS. Carried as an asset, the way any implementation would.
pub const CSS: &str = include_str!("../assets/page.css");
/// The brand mark, as the data-URI the page already inlines.
pub const FAVICON: &str = include_str!("../assets/favicon.txt");
/// The "the third step was not a step" note. Prose that documents a decision.
pub const STEP_NOTE: &str = include_str!("../assets/step-note.html");

/// The pre-paint theme bootstrap.
///
/// THIS ONE STAYS INLINE, SYNCHRONOUS JAVASCRIPT, AND THAT IS NOT A COMPROMISE —
/// it is the finding. It reads localStorage and sets the theme attribute BEFORE
/// the body paints; a wasm module cannot do that, because a module script is
/// deferred and its binary has to arrive first. Moving this to Rust would buy
/// 433 bytes and cost a flash of the wrong theme on every dark-mode visit.
pub const THEME_INIT_JS: &str = r#"<script>
// DSH-style theme init: respect system preference, allow manual toggle
(function() {
  var stored = localStorage.getItem('summrise-theme');
  var systemDark = stored === null
    && typeof matchMedia !== 'undefined'
    && matchMedia('(prefers-color-scheme: dark)').matches;
  var dark = stored === 'dark' || (stored === null && systemDark);
  if (dark) document.body.setAttribute('data-ds-dark-theme', '');
})();
</script>"#;

/// The wasm loader, in place of the page's old inline particle + toggle script.
///
/// The import is an ABSOLUTE path under the release host's own asset prefix, not
/// `./pkg/…`: the page is served at `/` and again at `/index.html`, and a relative
/// specifier would resolve differently at the two. The pair is staged beside every
/// other served asset (`index/public/summrise-agent/landing/`), so it is served by
/// the same ASSETS binding that serves the tgz and needs no worker route.
pub const WASM_LOADER_JS: &str = r#"<script type="module">
// The page is already complete and painted; this only adds the behaviour.
// The timestamp is what makes "the text did not wait for the binary" measurable
// rather than asserted — see measure.mjs.
import init from '/summrise-agent/landing/summrise_landing.js';
init().then(() => { window.__wasmReadyMs = performance.now(); });
</script>"#;

/// The download door. A URL is passed in only when the release manifest
/// advertises an installer, so a tgz-only publish never offers a button that
/// would serve the PREVIOUS build. There is deliberately no fallback here.
///
/// THE COST COMES BEFORE THE CLICK. The sentence naming this page's only
/// precondition sits ABOVE the button, so the reader is told what pressing needs
/// before being invited to press. Order is load-bearing: measured on the live page
/// at 1280x900, the button's top was 183.7 and the sentence's 235.7 before the two
/// blocks were swapped, and `verify.mjs` asserts the order rather than trusting it.
fn setup_block(setup_url: Option<&str>) -> String {
    match setup_url {
        Some(u) => {
            format!(
                r#"<span class="hint">Easiest path: one setup.exe (needs admin + internet, no Node.js required). Or the manual channel below.</span>
        <a class="btn-primary" href="{u}">Download Windows installer</a>"#
            )
        }
        None => r#"<span class="hint">No Windows installer is published for this release — use the npm channel below: it installs the same agent and updates itself.</span>"#.to_string(),
    }
}

/// Render the landing page. `setup_url` is `None` when the release has no installer.
///
/// THE THREE VALUES ARE ALREADY SAFE — whitelisted and HTML-escaped by the caller.
/// See the module header: this runs at build time, where the real values do not
/// exist yet, so the policy lives where they arrive (`index/src/page.js`).
pub fn render(console_url: &str, installer_url: &str, setup_url: Option<&str>) -> String {
    let console = console_url;
    let installer = installer_url;

    let mut h = String::with_capacity(32 * 1024);
    h.push_str(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Summrise Agent</title>
<link rel="icon" href=""#,
    );
    h.push_str(FAVICON);
    h.push_str("\">\n<style>");
    h.push_str(CSS);
    h.push_str("</style>\n</head>\n<body>\n");
    h.push_str(THEME_INIT_JS);
    h.push_str(
        r#"

<div class="app">
  <button class="theme-toggle" aria-label="Toggle theme">
    <svg class="icon-moon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M13.36 10.06A6 6 0 0 1 5.94 2.64 6 6 0 1 0 13.36 10.06Z"/></svg>
    <svg class="icon-sun" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="8" r="3"/><path d="M8 1v2M8 13v2M1 8h2M13 8h2M3.05 3.05l1.41 1.41M11.54 11.54l1.41 1.41M3.05 12.95l1.41-1.41M11.54 4.46l1.41-1.41"/></svg>
  </button>

  <main class="main">
    <div class="aside">
      <div class="brand">
        <img class="brand-mark" src=""#,
    );
    h.push_str(FAVICON);
    h.push_str(
        r#"" alt="Summrise">
        <div class="brand-text">
          <h1 class="brand-name">Summrise Agent</h1>
          <div class="brand-tag">device agent</div>
        </div>
      </div>

      <p class="desc">Summrise Agent is a device command center (serial / terminal / browser + MCP) that runs on a Windows machine. Each device is exposed over a Cloudflare Tunnel and managed from the <a href=""#,
    );
    h.push_str(console);
    h.push_str(
        r#"">Summrise console</a>.</p>
    </div>

    <div class="card">
      <div class="actions">
        "#,
    );
    h.push_str(&setup_block(setup_url));
    h.push_str(
        r#"
        <code class="cmd">npx summrise-agent setup</code>
        <span class="hint">Run on the Windows machine connected to the device. Requires Node.js + admin rights.</span>
      </div>

      <div class="steps">
        <div class="step">
          <div class="step-num">1</div>
          <div class="step-body">Install and set up in one command: <code>npx summrise-agent setup</code>. The device registers itself with the <a href=""#,
    );
    h.push_str(console);
    h.push_str(
        r#"">Summrise console</a> on first start — a no-key install is not a local-only one. <b>Behind a locked-down network</b> that cannot reach the npm registry, install from the release host instead: <code>npm i -g "#,
    );
    h.push_str(installer);
    h.push_str(
        r#"</code></div>
        </div>
        <div class="step">
          <div class="step-num">2</div>
          <div class="step-body">Setup installs the agent service, fetches the boxed components it needs (about 200MB: cloudflared 54MB, playwright 30MB, the desktop runtime 115MB) and starts the agent. For the panel URL run <code>summrise status</code> (<code>summrise.cmd status</code> in PowerShell) — <code>setup</code> prints neither a URL nor a token.</div>
        </div>
      </div>
      "#,
    );
    h.push_str(STEP_NOTE);
    h.push_str(
        r#"

      <p class="hint"><b>Later updates need <code>--prefix</code></b>: a plain <code>npm i -g</code> writes npm's default global prefix, not the one <code>summrise</code> lives in, so <code>summrise update</code> runs the OLD CLI and stages the OLD build while npm reports success. In PowerShell: <code>npm i -g --prefix (Split-Path (Get-Command summrise).Source) &lt;url&gt;</code>. Install from the URL above, never the bare package name.</p>
    </div>
  </main>
</div>

"#,
    );
    h.push_str(WASM_LOADER_JS);
    h.push_str("\n</body>\n</html>");
    let _ = write!(h, "");
    h
}

// ── THE TWO ARMS, RENDERED HERE SO THE BUILD AND ITS TEST CALL THE SAME CODE ────────────────────
//
// `gen slots` writes these into `index/src/landing/`, where the worker's `page.js` substitutes the
// three URLs into them per request. **AND THE TEST BELOW COMPARES THEM WITH WHAT IS TRACKED**, which is
// the freshness rule: the arms are rendered, committed and SERVED, so a change to this file that is not
// regenerated ships the previous document — the same shape the panel's sheet carries, checked here
// without spawning anything because the renderer IS this crate.

/// The module wrapper: the document as a JS default export, with the three characters that would end a
/// template literal escaped.
pub fn as_module(html: &str) -> String {
    let mut out = String::with_capacity(html.len() + 256);
    out.push_str(
        "// GENERATED by index/landing/build.sh — do not edit.\n\
         // The document is rendered by index/landing/src/page.rs; the three {{…_URL}}\n\
         // slots are filled per request by index/src/page.js, which owns the URL policy.\n\
         export default `",
    );
    let mut chars = html.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '`' => out.push_str("\\`"),
            '\\' => out.push_str("\\\\"),
            // Only `${` opens an interpolation; a lone `$` is an ordinary character
            // and escaping every one of them would make the document unreadable.
            '$' if chars.peek() == Some(&'{') => out.push_str("\\$"),
            c => out.push(c),
        }
    }
    out.push_str("`;\n");
    out
}

/// `(file name, contents)` for both arms, in the order the build writes them.
///
/// THE SLOTS ARE CHECKED RATHER THAN ASSUMED. `render` interpolates verbatim, so this can only fail if a
/// slot string is edited into something the document rewrites — and the failure mode is a reader seeing
/// a placeholder. The npm-only arm additionally must NOT offer an installer door: a tgz-only release
/// must not link the alias, which serves the PREVIOUS build.
pub fn arms() -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    for (name, setup) in [("setup.js", Some(SETUP_SLOT)), ("npm-only.js", None)] {
        let html = render(CONSOLE_SLOT, INSTALLER_SLOT, setup);
        let want: &[&str] = match setup {
            Some(s) => &[CONSOLE_SLOT, INSTALLER_SLOT, s],
            None => &[CONSOLE_SLOT, INSTALLER_SLOT],
        };
        for slot in want {
            assert!(
                html.contains(slot),
                "{name}: the slot {slot} did not survive rendering — the worker would serve it literally"
            );
        }
        if setup.is_none() {
            // `class="btn-primary"`, not `btn-primary`: the stylesheet defines the button's rules in
            // BOTH arms (it is one stylesheet), so an assertion on the bare name fails on a correct
            // page — which is how this one failed the first time it ran.
            assert!(
                !html.contains(SETUP_SLOT) && !html.contains(r#"class="btn-primary""#),
                "npm-only.js offers an installer door — a tgz-only release must not link the alias, which serves the PREVIOUS build"
            );
        }
        out.push((name, as_module(&html)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// THE TRACKED ARMS ARE WHAT THIS CRATE RENDERS. A change to `page.rs` that is not regenerated
    /// ships the previous document — the arms are rendered, committed and served — and this is the
    /// check that refuses it. It needs no toolchain beyond `cargo test`, because the renderer is here.
    ///
    /// MUTATION: change one word of `page.rs` (or of an asset it includes) and do not regenerate.
    /// RESULT:   fails, naming the file and the first line that differs.
    #[test]
    fn the_tracked_arms_are_what_this_crate_renders() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/landing");
        for (name, want) in arms() {
            let path = dir.join(name);
            let got = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            if got != want {
                let (gl, wl) = (got.lines().count(), want.lines().count());
                let first = got
                    .lines()
                    .zip(want.lines())
                    .position(|(a, b)| a != b)
                    .map(|i| i + 1);
                panic!(
                    "{}: the tracked arm is not what this crate renders \
                     (tracked {gl} line(s), rendered {wl}, first difference at line {first:?}). \
                     Regenerate with `index/landing/build.sh` and commit the arms.",
                    path.display()
                );
            }
        }
    }
}
