//! page.rs — the landing page's HTML, rendered by Rust.
//!
//! THE TEXT IS IN THE HTML. This renderer runs at BUILD time (or in the worker),
//! not in the browser, so the crawler and the first paint both get a complete
//! document and no byte of wasm is on the critical path. `src/interactive.rs`
//! then hydrates the two behaviours. That is the whole design, and it is the
//! only one that keeps a static page fast.
//!
//! Fidelity bar: for the same three URLs this must emit the same bytes as
//! `index/src/page.js` does, except at the two places the migration is *about*
//! (the theme-toggle attribute and the closing script block). `build.sh`
//! asserts that, so "the same numbers" is not a claim — it is a diff.

use std::fmt::Write as _;
use url::Url;

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
pub const WASM_LOADER_JS: &str = r#"<script type="module">
// The page is already complete and painted; this only adds the behaviour.
// The timestamp is what makes "the text did not wait for the binary" measurable
// rather than asserted — see measure.mjs.
import init from './pkg/spike_rust_landing.js';
init().then(() => { window.__wasmReadyMs = performance.now(); });
</script>"#;

/// P2-8: https-only URL whitelist, http allowed solely for loopback dev.
///
/// Ported with the reference WHATWG parser rather than a prefix match, because
/// `new URL(u, base)` resolves a RELATIVE string against the base and then finds
/// it https — so `/summrise-agent/x.tgz` is a legitimate value here, and a
/// prefix check would have rejected it.
pub fn safe_page_url(u: &str, fallback: &str) -> String {
    let base = Url::parse("https://placeholder.local").expect("static base parses");
    let parsed = match Url::parse(u) {
        Ok(p) => p,
        Err(_) => match base.join(u) {
            Ok(p) => p,
            Err(_) => return fallback.to_string(),
        },
    };
    if parsed.scheme() == "https" {
        return u.to_string();
    }
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    if parsed.scheme() == "http" && matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]") {
        return u.to_string();
    }
    fallback.to_string()
}

/// P2-8: HTML-escape at every interpolation point. `&` must go first.
pub fn esc_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The download door. A URL is passed in only when the release manifest
/// advertises an installer, so a tgz-only publish never offers a button that
/// would serve the PREVIOUS build. There is deliberately no fallback here.
fn setup_block(setup_url: Option<&str>) -> String {
    match setup_url {
        Some(u) => {
            let href = esc_html(&safe_page_url(u, "/summrise-agent/SummriseAgent-Setup.exe"));
            // THE COST COMES BEFORE THE CLICK. The sentence naming this page's only
            // precondition sits ABOVE the button, so the reader is told what pressing
            // needs before being invited to press. Order is load-bearing here.
            format!(
                r#"<span class="hint">Easiest path: one setup.exe (needs admin + internet, no Node.js required). Or the manual channel below.</span>
        <a class="btn-primary" href="{href}">Download Windows installer</a>"#
            )
        }
        None => r#"<span class="hint">No Windows installer is published for this release — use the npm channel below: it installs the same agent and updates itself.</span>"#.to_string(),
    }
}

/// Render the landing page. `setup_url` is `None` when the release has no installer.
pub fn render(console_url: &str, installer_url: &str, setup_url: Option<&str>) -> String {
    let console = esc_html(&safe_page_url(console_url, "/"));
    let installer = esc_html(&safe_page_url(
        installer_url,
        "/summrise-agent/summrise-agent-latest.tgz",
    ));

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
    h.push_str(&console);
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
    h.push_str(&console);
    h.push_str(
        r#"">Summrise console</a> on first start — a no-key install is not a local-only one. <b>Behind a locked-down network</b> that cannot reach the npm registry, install from the release host instead: <code>npm i -g "#,
    );
    h.push_str(&installer);
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
