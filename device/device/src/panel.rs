//! The device's own panel, rendered in Rust.
//!
//! The page is a function of [`PanelFacts`]: the HTTP handler reads every fact once, then renders
//! it, so a browser reload is a fresh read and an enrollment is a plain form POST followed by a
//! redirect. Only the terminal is scripted, because a browser terminal needs an emulator.

use maud::{html, Markup, PreEscaped, DOCTYPE};

use crate::console::RetryNote;

/// The page's whole style, copied from the HTML page this replaces: the panel and the console are
/// two surfaces of one product, so the move into Rust must not restyle either.
///
/// Rendered through `PreEscaped` because a `<style>` element is raw text in HTML: escaping the
/// quotes in the font stacks would put literal `&quot;` into the CSS.
const STYLE: &str = r##"
  :root {
    color-scheme: light dark;
    --accent: #bf3a0a;
    --accent-soft: #ffefe5;
    --bg: #fafafa;
    --surface: #ffffff;
    --surface-recessed: #fafafa;
    --surface-chip: #f4f4f5;
    --ink: #18181b;
    --ink-muted: #71717a;
    --ink-faint: #a1a1aa;
    --line: #e4e4e7;
    --line-strong: #d4d4d8;
    --ok: #128a4b;
    --ok-soft: #e7f6ee;
    --warn: #a8620a;
    --warn-soft: #fff4e5;
    --error: #c02a1b;
    --radius: 10px;
    --radius-lg: 14px;
    --term-canvas: #ffffff; --term-selection: rgba(191,58,10,.18);
    --term-red: #b3261e; --term-green: #1d7a3c; --term-yellow: #a8620a;
    --term-blue: #1f5fbf; --term-magenta: #8a3ffc; --term-cyan: #0e7490;
    --font: ui-sans-serif, system-ui, -apple-system, "Segoe UI", Roboto, "Helvetica Neue", sans-serif;
    --font-mono: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace;
    --shadow: 0 1px 2px rgba(16, 23, 33, .06);
  }
  @media (prefers-color-scheme: dark) {
    :root {
      --accent-soft: #2a1c12;
      --term-canvas: #131418; --term-selection: rgba(255,163,107,.24);
      --term-red: #ff8b84; --term-green: #6edc8b; --term-yellow: #e0a44a;
      --term-blue: #7fb0ff; --term-magenta: #c9a3ff; --term-cyan: #6fd3e6;
      --bg: #0c1014;
      --surface: #121820;
      --surface-recessed: #0e1319;
      --surface-chip: #1a222c;
      --ink: #e7edf4;
      --ink-muted: #98a5b4;
      --ink-faint: #7b8794;
      --line: #222c37;
      --line-strong: #2e3a47;
      --ok: #3fcf7f;
      --ok-soft: #10261a;
      --warn: #e0a44a;
      --warn-soft: #2a1e0b;
      --error: #ff8b7e;
      --shadow: 0 1px 2px rgba(0, 0, 0, .4);
    }
  }
  * { box-sizing: border-box; }
  body {
    margin: 0; padding: 1.5rem 1.25rem 3rem;
    background: var(--bg); color: var(--ink);
    font: 0.9375rem / 1.55 var(--font);
    font-variant-numeric: tabular-nums;
    -webkit-font-smoothing: antialiased;
  }
  main { max-width: 54rem; margin: 0 auto; display: grid; gap: 1rem; }
  header { display: flex; align-items: center; gap: 0.6rem; margin-bottom: 0.25rem; }
  header img { display: block; }
  h1 { font-size: 1.15rem; font-weight: 650; margin: 0; letter-spacing: -0.012em; }
  header span { color: var(--ink-faint); font-size: 0.85rem; }
  .lede { color: var(--ink-muted); margin: 0 0 0.25rem; font-size: 0.9rem; }
  header button {
    margin-left: auto; font: inherit; font-size: 0.82rem; font-weight: 550;
    padding: 0.3rem 0.65rem; border-radius: 8px; cursor: pointer;
    border: 1px solid var(--line-strong); background: var(--surface); color: var(--ink);
  }
  header button:hover { border-color: var(--accent); }
  header button.copied { border-color: var(--ok); color: var(--ok); }

  section {
    background: var(--surface); border: 1px solid var(--line);
    border-radius: var(--radius-lg); padding: 1rem 1.1rem; box-shadow: var(--shadow);
  }
  section h2 {
    font-size: 0.95rem; font-weight: 650; margin: 0 0 0.7rem;
    display: flex; align-items: center; gap: 0.5rem;
  }
  section h2 .chip { margin-left: auto; }
  dl { display: grid; grid-template-columns: 11rem 1fr; gap: 0.3rem 1rem; margin: 0; }
  dt { color: var(--ink-muted); font-size: 0.88rem; }
  dd { margin: 0; overflow-wrap: anywhere; font-size: 0.88rem; }
  .mono { font-family: var(--font-mono); font-size: 0.78rem; }

  .chip {
    display: inline-block; padding: 0.05rem 0.5rem; border-radius: 999px;
    font-size: 0.75rem; font-weight: 600; line-height: 1.6; white-space: nowrap;
  }
  .chip-ok { background: var(--ok-soft); color: var(--ok); }
  .chip-warn { background: var(--warn-soft); color: var(--warn); }
  .chip-off { background: var(--surface-chip); color: var(--ink-muted); }

  table { width: 100%; border-collapse: collapse; }
  th, td { text-align: left; padding: 0.35rem 0.45rem; border-bottom: 1px solid var(--line); font-size: 0.85rem; }
  th { font-size: 0.72rem; text-transform: uppercase; letter-spacing: 0.05em; color: var(--ink-faint); font-weight: 600; }
  tr:last-child td { border-bottom: 0; }
  td.kind { color: var(--ink-muted); }
  .none { color: var(--ink-muted); font-size: 0.88rem; }

  /* The enroll form: the same fields and vocabulary as the console's "Add a device", because the
     person pastes the code from there into here. Field and button rules only; the tokens above are
     the panel's. */
  form {
    display: grid; gap: 0.45rem; margin-top: 0.9rem; padding-top: 0.9rem;
    border-top: 1px solid var(--line);
  }
  form h3 { font-size: 0.9rem; font-weight: 650; margin: 0 0 0.1rem; }
  form label { font-size: 0.85rem; color: var(--ink-muted); }
  form input[type="text"] {
    font: inherit; font-size: 0.88rem; color: var(--ink);
    background: var(--surface-recessed); border: 1px solid var(--line-strong);
    border-radius: 8px; padding: 0.4rem 0.6rem; min-width: 0;
  }
  form .check { display: flex; align-items: flex-start; gap: 0.45rem; color: var(--ink); }
  form .check input { margin: 0.25rem 0 0; }
  form .hint { margin: 0; color: var(--ink-faint); font-size: 0.8rem; }
  form button {
    justify-self: start; font: inherit; font-size: 0.85rem; font-weight: 600;
    padding: 0.4rem 0.9rem; border-radius: 8px; cursor: pointer;
    border: 1px solid transparent; background: var(--accent); color: #fff;
  }
  form button:disabled { opacity: 0.6; cursor: progress; }
  form input:focus-visible, form button:focus-visible {
    outline: 2px solid var(--accent); outline-offset: 1px;
  }
  .enroll-status { margin: 0; min-height: 1.35em; font-size: 0.85rem; color: var(--ink-muted); }
  .enroll-status.ok { color: var(--ok); }
  .enroll-status.error { color: var(--error); }

  /* The local MCP instructions: the config is the thing to copy, so it gets the free space and a
     button, while the token itself is named by file and never printed. */
  /* The terminal canvas, the same token the console uses: white in the light theme, near-black in the
     dark one. A black block inside a light page is what made the two surfaces read as two products. */
  .term-host { height: 20rem; background: var(--term-canvas); border: 1px solid var(--line); border-radius: 8px; padding: 6px; }
  .xterm .xterm-viewport { background-color: var(--term-canvas); }
  .config {
    margin: 0.5rem 0 0.6rem; padding: 0.7rem 0.8rem; overflow-x: auto;
    background: var(--surface-recessed); border: 1px solid var(--line); border-radius: 8px;
    font-family: var(--font-mono); font-size: 0.78rem; line-height: 1.5; color: var(--ink);
  }
  #copy-local {
    font: inherit; font-size: 0.85rem; font-weight: 600; padding: 0.35rem 0.8rem;
    border-radius: 8px; border: 1px solid var(--line-strong);
    background: var(--surface); color: var(--ink); cursor: pointer;
  }
  #copy-local.copied { border-color: var(--ok); color: var(--ok); }
  code { font-family: var(--font-mono); font-size: 0.82rem; }

  /* A phone, or a console window dragged narrow: an 11 rem label column leaves the value column about
     ten characters wide, so the two stack instead. */
  @media (max-width: 40rem) {
    body { padding: 1rem 0.85rem 2rem; }
    dl { grid-template-columns: 1fr; gap: 0.1rem; }
    dt { margin-top: 0.5rem; }
    header { flex-wrap: wrap; }
    header button { margin-left: 0; }
  }
"##;

/// The brand mark, the same SVG every surface of the product shows. It is markup, not data.
const BRAND_MARK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48" viewBox="0 0 48 48">
  <!-- Summrise brand mark: the summrise at sunrise — near hill, far ridge, signal over the pass.
       THE ONE MARK. It is 16–30px in real life (Windows tray 16, rail 20/26, console 30),
       so it is drawn for that: three gradients, no scene. The iridescent variant that
       replaced it for three days put twelve gradients and a whole sky into the same
       space; at 16–26px its ribbons collapsed into a smear. Reverted.
       Copies kept in step by src/ui/__tests__/Icon.test.tsx: this file, the panel's
       BrandMark, the console favicon and the landing page's data-URI. -->
  <defs>
    <linearGradient id="summrise-sky" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#f59f00"/>
      <stop offset="1" stop-color="#e8590c"/>
    </linearGradient>
    <radialGradient id="summrise-glow" cx=".5" cy=".5" r=".5">
      <stop offset="0" stop-color="#fff8e1" stop-opacity=".55"/>
      <stop offset="1" stop-color="#ffe8a3" stop-opacity="0"/>
    </radialGradient>
    <!-- The material: light gathers at the top edge, the base sits in its own shadow.
         This is what makes the tile read as an object rather than a sticker. -->
    <linearGradient id="summrise-sheen" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#ffffff" stop-opacity=".25"/>
      <stop offset=".45" stop-color="#ffffff" stop-opacity="0"/>
      <stop offset="1" stop-color="#7c2d12" stop-opacity=".10"/>
    </linearGradient>
  </defs>

  <rect width="48" height="48" rx="11" fill="url(#summrise-sky)"/>

  <!-- signal rising over the pass -->
  <circle cx="21" cy="14" r="7.5" fill="url(#summrise-glow)"/>
  <circle cx="21" cy="14" r="4" fill="#fff8e1"/>

  <!-- far ridge (haze) -->
  <path fill="#ffffff" opacity=".78" d="M14 41Q26 16 44 41Z"/>
  <!-- near hill (solid) -->
  <path fill="#ffffff" d="M2 41Q12 20 24 41Z"/>

  <rect width="48" height="48" rx="11" fill="url(#summrise-sheen)"/>
</svg>"##;

/// The page's only inline JavaScript: the copy buttons. It moves text the server rendered into the
/// clipboard and decides nothing; everything that reads the device is Rust.
const COPY_SCRIPT: &str = r##"// The page is server-rendered; these buttons only copy text already in the document.
(function () {
  function legacyCopy(text) {
    const area = document.createElement("textarea");
    area.value = text;
    document.body.append(area);
    area.select();
    const copied = document.execCommand("copy");
    area.remove();
    return copied;
  }

  function copy(text) {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      return navigator.clipboard.writeText(text).then(
        function () { return true; },
        function () { return legacyCopy(text); }
      );
    }
    return Promise.resolve(legacyCopy(text));
  }

  for (const button of document.querySelectorAll("[data-copy]")) {
    button.addEventListener("click", async function () {
      const source = document.getElementById(button.dataset.copy);
      if (!source) return;
      const status = button.dataset.status
        ? document.getElementById(button.dataset.status)
        : null;
      const copied = await copy(source.textContent);
      if (!copied) {
        if (status && button.dataset.refused) status.textContent = button.dataset.refused;
        button.textContent = "Copy refused";
        setTimeout(function () { button.textContent = button.dataset.label; }, 1600);
        return;
      }
      if (status && button.dataset.copied) status.textContent = button.dataset.copied;
      button.textContent = "Copied";
      button.classList.add("copied");
      setTimeout(function () {
        button.textContent = button.dataset.label;
        button.classList.remove("copied");
      }, 1600);
    });
  }
})();"##;

/// One live terminal session, reduced to the facts the page prints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SessionFacts {
    /// `pty`, `ssh` or `serial`, the registry's own spelling.
    pub(crate) kind: String,
    /// The program, `user@host:port`, or the serial port path.
    pub(crate) shell: String,
    /// The session id, `session-<number>`.
    pub(crate) id: String,
}

/// Everything the panel shows, read from the same sources `/api/panel` reads.
#[derive(Debug)]
pub(crate) struct PanelFacts {
    pub(crate) version: String,
    pub(crate) channel: String,
    pub(crate) sha256: String,
    pub(crate) pid: u32,
    /// Where the device writes its token and state; the page derives the token path from it.
    pub(crate) data_dir: String,
    /// The loopback MCP endpoint a client on this machine connects to.
    pub(crate) mcp_endpoint: String,
    pub(crate) token_file: String,
    pub(crate) token_command: String,
    /// `None` is "the device could not read its state document", not "there is no state".
    pub(crate) state: Option<serde_json::Value>,
    pub(crate) events: Vec<serde_json::Value>,
    pub(crate) console_attempt: Option<RetryNote>,
    pub(crate) sessions: Vec<SessionFacts>,
    /// This boot's panel token: the page carries it in its meta tag and its form.
    pub(crate) panel_token: String,
}

/// How an enrollment form POST ended, as the redirect query spells it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EnrollOutcome {
    /// The console link and token were written.
    Enrolled,
    /// The enrollment was refused; the value is the stable code the redirect carries.
    Failed(String),
}

/// The token file's path and the command that reads it, for a client on this machine.
///
/// The command is the platform's shell spelling: the panel is read by a person at the machine, and
/// the page has no other way to know which shell they have.
pub(crate) fn token_facts(data_dir: &str) -> (String, String) {
    let separator = if data_dir.contains('\\') { '\\' } else { '/' };
    let base = data_dir.trim_end_matches(['/', '\\']);
    let token_file = format!("{base}{separator}token");
    let command = if separator == '\\' {
        format!("Get-Content \"{token_file}\"")
    } else {
        format!("cat \"{token_file}\"")
    };

    (token_file, command)
}

/// The panel with no enrollment result: a plain read of the machine.
pub(crate) fn page(facts: &PanelFacts) -> Markup {
    render(facts, None)
}

/// The same page with the enrollment form's outcome at the top of its own section.
///
/// A failed enrollment is not an error page: the operator still needs the console link's facts and
/// the form, so the banner is a variant of the page rather than a response that replaces it.
pub(crate) fn enroll_result(facts: &PanelFacts, outcome: EnrollOutcome) -> Markup {
    render(facts, Some(&outcome))
}

fn render(facts: &PanelFacts, outcome: Option<&EnrollOutcome>) -> Markup {
    let attempt = facts
        .console_attempt
        .as_ref()
        .filter(|note| note.attempts > 0);
    let digest = if facts.sha256.is_empty() {
        "unverified"
    } else {
        facts.sha256.as_str()
    };
    let digest_short: String = digest.chars().take(16).collect();
    let chip = match attempt {
        Some(note) => html! {
            span id="console-chip" class="chip chip-warn" { "reconnected after " (note.attempts) }
        },
        None => html! {
            span id="console-chip" class="chip chip-ok" { "connected" }
        },
    };
    let state_rows = state_rows(facts.state.as_ref());
    let event_rows = event_rows(&facts.events);
    let config = local_config(facts);

    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="summrise-panel" content=(facts.panel_token);
                title { "Summrise — this machine" }
                link rel="stylesheet" href="/panel/xterm.css";
                style { (PreEscaped(STYLE)) }
            }
            body {
                main {
                    header {
                        (PreEscaped(BRAND_MARK))
                        h1 { "Summrise" }
                        span { "this machine" }
                        button id="copy" type="button" data-copy="diagnostics" data-label="Copy diagnostics" {
                            "Copy diagnostics"
                        }
                    }
                    p class="lede" {
                        "The device's own panel. Loopback only, no login: if you can read this, you are on the box."
                    }

                    section {
                        h2 { "Use this machine" }
                        p class="hint" {
                            "A client on this machine reaches the device's MCP endpoint with the install token. Nothing below leaves the machine, and the token is not printed here: the page is served to anyone on loopback, while the token file is protected to the service account and administrators. Read it yourself and paste it into the config."
                        }
                        dl id="local" {
                            dt { "MCP endpoint" }
                            dd id="local-endpoint" class="mono" { (facts.mcp_endpoint) }
                            dt { "Install token" }
                            dd id="local-token-file" class="mono" { (facts.token_file) }
                            dt { "Read it" }
                            dd id="local-token-command" class="mono" { (facts.token_command) }
                        }
                        pre class="config" id="local-config" { (config) }
                        button
                            id="copy-local"
                            type="button"
                            data-copy="local-config"
                            data-status="local-copy-status"
                            data-label="Copy local MCP config"
                            data-copied="Config copied. Paste it into your MCP client and add the token."
                            data-refused="The clipboard refused; select the config and copy it."
                        {
                            "Copy local MCP config"
                        }
                        p class="hint" id="local-copy-status" role="status" {}
                    }

                    section {
                        h2 { "Terminal" }
                        p class="hint" {
                            "Open a pty or a serial line on this machine and type into it. Loopback only, the same registry the MCP's terminal.* tools use. A session does not survive a device restart."
                        }
                        p {
                            button id="term-pty" type="button" { "New pty" }
                            input id="term-port" type="text" placeholder="COM3" value="COM3" style="width:7rem";
                            input id="term-baud" type="number" placeholder="115200" value="115200" style="width:7rem";
                            button id="term-serial" type="button" { "New serial" }
                            button id="term-close" type="button" { "Close" }
                        }
                        div id="term-host" class="term-host" {}
                        p class="hint" id="term-status" role="status" { "choose a session above" }
                    }

                    section {
                        h2 { "Terminal sessions" }
                        p class="hint" {
                            "The same sessions an MCP client created on this machine, live. The interactive terminal lives in the console; this list is what an operator standing here can check without it."
                        }
                        div id="terminal-sessions" class="hint" {
                            @if facts.sessions.is_empty() {
                                "No live terminal sessions."
                            } @else {
                                dl {
                                    @for session in &facts.sessions {
                                        dt { (session.kind) }
                                        dd { (format!("{} · {}", session.shell, session.id)) }
                                    }
                                }
                            }
                        }
                    }

                    section {
                        h2 { "Build" }
                        dl id="build" {
                            dt { "Version" }
                            dd { (facts.version) }
                            dt { "Channel" }
                            dd { (facts.channel) }
                            dt { "Binary sha256" }
                            dd class="mono" title=(digest) { (digest_short) }
                            dt { "Process" }
                            dd { "pid " (facts.pid) }
                        }
                    }

                    section {
                        h2 { "Console link " (chip) }
                        dl id="console" {
                            @if let Some(note) = attempt {
                                dt { "Link" }
                                dd {
                                    "reconnected after " (note.attempts) " " (failure_noun(note.attempts))
                                }
                                dt { "Reason" }
                                dd {
                                    @if note.reason.is_empty() {
                                        "(none given)"
                                    } @else {
                                        (note.reason)
                                    }
                                }
                            } @else {
                                dt { "Link" }
                                dd { "connected on the first try" }
                            }
                        }
                        form id="enroll-form" method="post" action="/panel/enroll" autocomplete="off" {
                            input type="hidden" name="panel_token" value=(facts.panel_token);
                            h3 { "Enrol this machine" }
                            p class="hint" {
                                "Paste the console URL and the one-time code from the console's “Add a device”."
                            }
                            label for="enroll-console" { "Console URL" }
                            input
                                id="enroll-console"
                                name="console_url"
                                type="text"
                                spellcheck="false"
                                placeholder="https://console.example"
                                required;
                            label for="enroll-code" { "One-time code" }
                            input
                                id="enroll-code"
                                name="code"
                                type="text"
                                spellcheck="false"
                                autocomplete="off"
                                required;
                            label class="check" {
                                input id="enroll-force" name="force" type="checkbox" value="on";
                                " Replace an existing enrollment"
                            }
                            p class="hint" {
                                "Replacing rotates this machine's console token; the token the console issued before stops working."
                            }
                            label class="check" {
                                input id="enroll-plaintext" name="allow_plaintext" type="checkbox" value="on";
                                " Allow plaintext HTTP to a console on a trusted tunnel"
                            }
                            p class="hint" {
                                "Only for a console reached over a private tunnel (WireGuard, Tailscale, an SSH forward): the code and the returned token cross this connection."
                            }
                            button id="enroll-submit" type="submit" { "Enroll" }
                            @if let Some(outcome) = outcome {
                                (banner(outcome))
                            } @else {
                                p id="enroll-status" role="status" {}
                            }
                        }
                    }

                    section {
                        h2 { "State" }
                        dl id="state" {
                            @for (key, value, mono) in &state_rows {
                                dt { (key) }
                                @if *mono {
                                    dd class="mono" { (value) }
                                } @else {
                                    dd { (value) }
                                }
                            }
                        }
                    }

                    section {
                        h2 { "Recent events" }
                        table {
                            thead {
                                tr {
                                    th { "when" }
                                    th { "event" }
                                }
                            }
                            tbody id="events" {
                                @for row in &event_rows {
                                    tr {
                                        td { (row.when) }
                                        td {
                                            (row.what)
                                            @if let Some(kind) = &row.kind {
                                                span class="kind" { (kind) }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        p class="none" id="no-events" hidden[!facts.events.is_empty()] {
                            "No event has been recorded on this device."
                        }
                    }
                }
                // The diagnostics text is rendered here so the clipboard button copies a fact, not a
                // second reading of the API; it is the same report the inline script used to build.
                pre id="diagnostics" hidden { (diagnostics(facts)) }
                script src="/panel/xterm.js" {}
                script src="/panel/addon-fit.js" {}
                script src="/panel/panel-terminal.js" {}
                script { (PreEscaped(COPY_SCRIPT)) }
            }
        }
    }
}

fn banner(outcome: &EnrollOutcome) -> Markup {
    match outcome {
        EnrollOutcome::Enrolled => html! {
            p class="enroll-status ok" id="enroll-status" role="status" {
                "Enrolled. If the device is restarting to connect, reload this panel in a few seconds; otherwise restart the SummriseLaunch service and reload."
            }
        },
        EnrollOutcome::Failed(code) => html! {
            p class="enroll-status error" id="enroll-status" role="status" {
                "Enrolment failed (" (code) "): " (failure_sentence(code))
            }
        },
    }
}

/// The stable redirect code, in the operator's words. The code is what travels in the URL; this is
/// what the form's reader sees, and the enrollment module's longer sentence stays on `/api/enroll`.
fn failure_sentence(code: &str) -> &'static str {
    match code {
        "bootstrap" => "console-bootstrap.json is not readable as a bootstrap",
        "usage" => "the form is missing the console URL or the one-time code",
        "bad_url" => {
            "the console URL must be an absolute https URL (plain http only for a loopback host)"
        }
        "data_dir" => "the device could not create its data directory",
        "identity" => "the device could not read or mint its identity",
        "console_unreachable" => "the device could not reach the console",
        "console_status" => "the console answered an unexpected status",
        "console_refused" => "the console refused the one-time code or the signature",
        "bad_response" => "the console answered a body the device could not read",
        "console_file" => "the existing console link could not be read",
        "token_conflict" => {
            "a console token already exists; tick “Replace an existing enrollment” to rotate it"
        }
        "write" => "the device could not write the console link",
        "protect" => "the device could not protect the token file",
        "unauthorized" => "this page did not carry this boot's panel token",
        _ => "the device refused the enrollment",
    }
}

fn failure_noun(attempts: u8) -> &'static str {
    if attempts == 1 {
        "failed attempt"
    } else {
        "failed attempts"
    }
}

fn local_config(facts: &PanelFacts) -> String {
    [
        "# Local MCP (this machine)".to_owned(),
        "- id: mcp-summrise-local".to_owned(),
        "  name: '@deepseek-ai/dsh-mcp-client'".to_owned(),
        "  config:".to_owned(),
        "    serverName: summrise-local".to_owned(),
        "    transport: streamable-http".to_owned(),
        format!("    url: {}", facts.mcp_endpoint),
        "    headers:".to_owned(),
        format!(
            "      Authorization: Bearer <contents of {}>",
            facts.token_file
        ),
    ]
    .join("\n")
}

/// `(label, value, is-structured)`: the state document as the table the page prints. A structured
/// value is shown as compact JSON, which is what the HTML page's `JSON.stringify` produced.
fn state_rows(state: Option<&serde_json::Value>) -> Vec<(String, String, bool)> {
    match state {
        None => vec![("state.json".to_owned(), "unreadable".to_owned(), false)],
        Some(serde_json::Value::Object(entries)) => entries
            .iter()
            .map(|(key, value)| {
                let structured = matches!(
                    value,
                    serde_json::Value::Array(_)
                        | serde_json::Value::Object(_)
                        | serde_json::Value::Null
                );
                let text = match value {
                    serde_json::Value::String(text) => text.clone(),
                    other => other.to_string(),
                };
                (key.clone(), text, structured)
            })
            .collect(),
        Some(other) => vec![("state.json".to_owned(), other.to_string(), true)],
    }
}

struct EventRow {
    when: String,
    what: String,
    kind: Option<String>,
}

/// The event tail as rows. A tool call writes two lines — `begin` with the tool, `end` with the
/// outcome — and the outcome replaces its call's first text instead of becoming a row that says
/// only "end", exactly as the HTML page merged them.
fn event_rows(events: &[serde_json::Value]) -> Vec<EventRow> {
    let mut rows: Vec<EventRow> = Vec::new();
    for event in events {
        if event.get("event").and_then(serde_json::Value::as_str) == Some("end") {
            if let Some(row) = rows.last_mut() {
                if row.kind.is_some() {
                    let ok = event
                        .get("ok")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    row.kind = Some(format!(" {}", if ok { "ok" } else { "failed" }));
                }
            }
            continue;
        }
        let when = event_time(event);
        let (what, kind) = if let Some(tool) = event.get("tool").and_then(serde_json::Value::as_str)
        {
            let kind = event
                .get("decision")
                .and_then(serde_json::Value::as_str)
                .or_else(|| event.get("event").and_then(serde_json::Value::as_str))
                .unwrap_or("");
            (tool.to_owned(), Some(format!(" {kind}")))
        } else {
            let what = event
                .get("event")
                .and_then(serde_json::Value::as_str)
                .or_else(|| event.get("kind").and_then(serde_json::Value::as_str))
                .map(str::to_owned)
                .unwrap_or_else(|| event.to_string());
            (what, None)
        };
        rows.push(EventRow { when, what, kind });
    }

    rows
}

fn event_time(event: &serde_json::Value) -> String {
    event
        .get("at_ms")
        .and_then(serde_json::Value::as_u64)
        // The page printed `at_ms || ts_ms`, so a zero `at_ms` is not a timestamp.
        .filter(|milliseconds| *milliseconds != 0)
        .or_else(|| event.get("ts_ms").and_then(serde_json::Value::as_u64))
        .map(|milliseconds| crate::store::format_utc(milliseconds / 1_000))
        .unwrap_or_default()
}

/// The text behind "Copy diagnostics": the HTML page built it from `/api/panel`; the page carries
/// the same facts already, so the server builds it once and the browser only copies it.
fn diagnostics(facts: &PanelFacts) -> String {
    let mut lines = vec![
        "Summrise device diagnostics".to_owned(),
        format!(
            "version {}  channel {}  sha256 {}",
            facts.version,
            facts.channel,
            if facts.sha256.is_empty() {
                "unverified"
            } else {
                facts.sha256.as_str()
            }
        ),
        format!("pid {}", facts.pid),
        format!("data directory: {}", facts.data_dir),
    ];

    match facts
        .console_attempt
        .as_ref()
        .filter(|note| note.attempts > 0)
    {
        Some(note) => {
            let reason = if note.reason.is_empty() {
                "(no reason)"
            } else {
                note.reason.as_str()
            };
            lines.push(format!(
                "console link: reconnected after {} {} — {}",
                note.attempts,
                failure_noun(note.attempts),
                reason
            ));
        }
        None => lines.push("console link: connected on the first try".to_owned()),
    }

    if let Some(verdict) = facts
        .state
        .as_ref()
        .and_then(|state| state.get("lastVerdict"))
    {
        if let Some(outcome) = verdict.get("outcome").and_then(serde_json::Value::as_str) {
            let detail = verdict
                .get("detail")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let timestamp = verdict
                .get("ts")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            lines.push(format!("last verdict: {outcome} {detail} at {timestamp}"));
        }
    }

    lines.push("recent events:".to_owned());
    let start = facts.events.len().saturating_sub(10);
    let mut last_tool_line: Option<usize> = None;
    for event in &facts.events[start..] {
        if event.get("event").and_then(serde_json::Value::as_str) == Some("end") {
            if let Some(line) = last_tool_line {
                let ok = event
                    .get("ok")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                lines[line].push_str(if ok { " → ok" } else { " → failed" });
            }
            continue;
        }
        let when = event_time(event);
        if let Some(tool) = event.get("tool").and_then(serde_json::Value::as_str) {
            let decision = event
                .get("decision")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            last_tool_line = Some(lines.len());
            lines.push(format!("  {when}  {tool} {decision}").trim_end().to_owned());
        } else {
            last_tool_line = None;
            let what = event
                .get("event")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| event.to_string());
            lines.push(format!("  {when}  {what}"));
        }
    }

    lines.join("\n")
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn facts() -> PanelFacts {
        PanelFacts {
            version: "2.0.3".to_owned(),
            channel: "canary".to_owned(),
            sha256: "b1946ac92492d2347c6235b4d2611184".to_owned(),
            pid: 4242,
            data_dir: "/var/lib/summrise".to_owned(),
            mcp_endpoint: "http://127.0.0.1:18080/mcp".to_owned(),
            token_file: "/var/lib/summrise/token".to_owned(),
            token_command: "cat \"/var/lib/summrise/token\"".to_owned(),
            state: Some(json!({
                "schema": 1,
                "lastVerdict": {
                    "outcome": "verified",
                    "detail": "the baseline booted",
                    "ts": "2026-10-27T11:00:00Z"
                }
            })),
            events: vec![
                json!({
                    "ts_ms": 1_761_566_400_000u64,
                    "event": "begin",
                    "tool": "terminal.create",
                    "decision": "allow"
                }),
                json!({ "ts_ms": 1_761_566_401_000u64, "event": "end", "ok": true }),
                json!({ "ts_ms": 1_761_566_402_000u64, "event": "repair" }),
            ],
            console_attempt: Some(RetryNote {
                attempts: 2,
                reason: "the tunnel was down".to_owned(),
            }),
            sessions: vec![SessionFacts {
                kind: "pty".to_owned(),
                shell: "bash".to_owned(),
                id: "session-1".to_owned(),
            }],
            panel_token: "a".repeat(64),
        }
    }

    #[test]
    fn the_page_renders_every_section_from_the_facts() {
        let html = page(&facts()).into_string();

        for fact in [
            "Use this machine",
            "Terminal",
            "Terminal sessions",
            "Build",
            "Console link",
            "State",
            "Recent events",
            "2.0.3",
            "canary",
            "b1946ac92492d2347c6235b4d2611184",
            "pid 4242",
            "http://127.0.0.1:18080/mcp",
            "/var/lib/summrise/token",
            "cat &quot;/var/lib/summrise/token&quot;",
            "lastVerdict",
            "verified",
            "reconnected after 2 failed attempts",
            "the tunnel was down",
            "pty",
            "bash",
            "session-1",
        ] {
            assert!(
                html.contains(fact),
                "{fact:?} is missing from the page: {html}"
            );
        }
        assert!(
            html.contains(&format!(
                r#"<meta name="summrise-panel" content="{}">"#,
                "a".repeat(64)
            )),
            "the script and the form read this boot's token from the meta tag"
        );
        assert!(
            html.contains(r#"<pre id="diagnostics" hidden>"#),
            "the diagnostics text is rendered by the server and only copied by the browser"
        );
        assert!(
            html.contains(r#"<script src="/panel/panel-terminal.js"></script>"#),
            "the page loads the terminal glue as an asset"
        );
        assert!(html.contains(r#"action="/panel/enroll""#));
        assert!(html.contains(r#"name="panel_token""#));
        assert!(html.contains("/panel/panel-terminal.js"));
        assert!(html.contains("/panel/xterm.js"));
        assert!(html.contains("/panel/addon-fit.js"));
        assert!(
            html.contains(r#"id="no-events" hidden"#),
            "a recorded event hides the empty-state line"
        );
        assert!(
            html.contains("--accent: #bf3a0a;"),
            "the page keeps the panel's existing style"
        );
        assert!(
            html.contains("&lt;contents of /var/lib/summrise/token&gt;"),
            "the config block shows the placeholder where the token goes"
        );
    }

    #[test]
    fn a_tool_end_row_joins_its_call_instead_of_printing_end() {
        let html = page(&facts()).into_string();

        assert!(html.contains("terminal.create"));
        assert!(
            html.contains(r#"<span class="kind"> ok</span>"#),
            "the outcome is the call's own row: {html}"
        );
        assert!(!html.contains(">end<"), "the raw end row is not printed");
    }

    #[test]
    fn an_empty_page_says_there_are_no_sessions_events_or_readable_state() {
        let mut facts = facts();
        facts.sessions.clear();
        facts.events.clear();
        facts.state = None;
        facts.console_attempt = None;

        let html = page(&facts).into_string();

        assert!(html.contains("No live terminal sessions."));
        assert!(html.contains("No event has been recorded on this device."));
        assert!(
            !html.contains(r#"id="no-events" hidden"#),
            "the empty-state line is visible when there is no event"
        );
        assert!(html.contains("unreadable"));
        assert!(html.contains("connected on the first try"));
        assert!(html.contains(r#"<span id="console-chip" class="chip chip-ok">"#));
    }

    #[test]
    fn the_enrollment_banner_reports_success_and_names_the_failure_code() {
        let enrolled = enroll_result(&facts(), EnrollOutcome::Enrolled).into_string();
        assert!(enrolled.contains(r#"class="enroll-status ok""#));
        assert!(enrolled.contains("Enrolled."));

        let failed = enroll_result(&facts(), EnrollOutcome::Failed("token_conflict".to_owned()))
            .into_string();
        assert!(failed.contains(r#"class="enroll-status error""#));
        assert!(failed.contains("Enrolment failed (token_conflict)"));
        assert!(failed.contains("Replace an existing enrollment"));
    }

    #[test]
    fn the_token_path_and_read_command_follow_the_data_directory() {
        let (file, command) = token_facts("/var/lib/summrise/");
        assert_eq!(file, "/var/lib/summrise/token");
        assert_eq!(command, "cat \"/var/lib/summrise/token\"");

        let (file, command) = token_facts(r"C:\ProgramData\Summrise\data\\");
        assert_eq!(file, r"C:\ProgramData\Summrise\data\token");
        assert_eq!(
            command,
            "Get-Content \"C:\\ProgramData\\Summrise\\data\\token\""
        );
    }

    #[test]
    fn the_diagnostics_copy_is_a_report_of_the_same_facts() {
        let text = diagnostics(&facts());

        assert!(
            text.contains("version 2.0.3  channel canary  sha256 b1946ac92492d2347c6235b4d2611184")
        );
        assert!(text.contains("pid 4242"));
        assert!(text
            .contains("console link: reconnected after 2 failed attempts — the tunnel was down"));
        assert!(text.contains("last verdict: verified the baseline booted at 2026-10-27T11:00:00Z"));
        assert!(text.contains("terminal.create allow → ok"));
        assert!(text.contains("  repair"));
    }
}
