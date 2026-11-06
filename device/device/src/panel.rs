//! The device's own panel, rendered in Rust.
//!
//! The page is a function of [`PanelFacts`]: the HTTP handler reads every fact once, then renders
//! it, so a browser reload is a fresh read and an enrollment is a plain form POST followed by a
//! redirect. Only the terminal is scripted, because a browser terminal needs an emulator.

use chrono::{Datelike, Local, TimeZone};
use maud::{html, Markup, PreEscaped, DOCTYPE};

use crate::console::RetryNote;

/// The newest audit lines the Timeline reads. The device's audit can be 64 MiB; a panel reload is a
/// glance, not a log export, so it reads a bounded tail and says when older lines exist.
pub(crate) const TIMELINE_LINES: usize = 200;

/// The newest Timeline rows the page draws. Drawn whole, the bounded record is 3,500 px at 1440 and
/// 6,200 px at 390, which put Build, Updates and Connect thousands of pixels below it. The rest is
/// one server-rendered `<details>`: the browser opens it without a script and every row stays on the
/// page.
pub(crate) const TIMELINE_VISIBLE_ROWS: usize = 20;

/// The page's whole style, copied from the HTML page this replaces: the panel and the console are
/// two surfaces of one product, so the move into Rust must not restyle either.
///
/// Rendered through `PreEscaped` because a `<style>` element is raw text in HTML: escaping the
/// quotes in the font stacks would put literal `&quot;` into the CSS.
/// The panel's own component rules; the tokens they read from are [`summrise_ui::TOKENS`], the same
/// block the console renders, so a colour cannot mean two things on two surfaces.
const COMPONENTS: &str = r##"  * { box-sizing: border-box; }
  /* The panel is one column: a top bar, the content, a status strip. The console's shell is a row of
     rail + column, so the direction belongs here and not in the shared block. */
  body.app { flex-direction: column; }
  body {
    margin: 0; padding: 0;
    background: var(--bg); color: var(--ink);
    font-family: var(--font); font-size: var(--fs-md); line-height: 1.55;
    font-variant-numeric: tabular-nums;
    -webkit-font-smoothing: antialiased;
  }
  main { flex: 1; width: 100%; max-width: 54rem; margin: 0 auto; padding: var(--sp-5) var(--sp-4) var(--sp-6); display: grid; gap: var(--sp-4); }
  /* The shared card carries a bottom margin; in a grid that owns the gap it doubled the space
     between siblings (16 + 20 = 36px). One owner of the gap. */
  main .card { margin-bottom: 0; }
  /* A grid item refuses to shrink below its content unless told to; the cards are the items. */
  main > * { min-width: 0; }
  .term-controls { display: flex; flex-wrap: wrap; align-items: center; gap: var(--sp-1-5); }
  .term-controls input { width: 7rem; min-width: 0; }
  .term-sep { flex: none; width: 1px; height: 18px; margin: 0 var(--sp-1); background: var(--line); }
  /* Close is the end of the row, not the neighbour of New serial: a gap that grows keeps it there
     when the row wraps without shoving it onto its own line. */
  .term-close { margin-left: auto; }
  .lede { color: var(--ink-muted); margin: 0 0 var(--sp-1); font-size: var(--fs-md); }
  header button {
    margin-left: auto; font: inherit; font-size: var(--fs-base); font-weight: 600;
    padding: var(--sp-1) var(--sp-3); border-radius: var(--radius-xs); cursor: pointer;
    border: 1px solid var(--line-strong); background: var(--surface); color: var(--ink);
  }
  header button:hover { border-color: var(--accent); }
  header button.copied { border-color: var(--ok); color: var(--ok); }

  /* A section is a card from `summrise_ui`: the container, the heading row and the body are the shared
     rules, so this page no longer styles sections at all. Only the spacing inside a body is local. */
  .card .card-body > * + * { margin-top: var(--sp-3); }
  dl { display: grid; grid-template-columns: 11rem 1fr; gap: var(--sp-1) var(--sp-4); margin: 0; }
  dt { color: var(--ink-muted); font-size: var(--fs-md); }
  dd { margin: 0; overflow-wrap: anywhere; font-size: var(--fs-md); }
  .mono { font-family: var(--font-mono); font-size: var(--fs-sm); }


  table { width: 100%; border-collapse: collapse; }
  th, td { text-align: left; padding: var(--sp-1-5) var(--sp-2); border-bottom: 1px solid var(--line); font-size: var(--fs-base); }
  th { font-size: var(--fs-xs); text-transform: uppercase; letter-spacing: var(--tracking-label); color: var(--ink-faint); font-weight: 600; }
  tr:last-child td { border-bottom: 0; }
  td.kind { color: var(--ink-muted); }
  .none { color: var(--ink-muted); font-size: var(--fs-md); }

  /* **THE RECORD, NOT A TABLE.** The Timeline is one row per call, grouped by the local day and
     newest first: time, tool, outcome and the door the call came through. The status tone is the
     audit's own (a failed `end` line), never a second opinion. */
  .timeline-day-label {
    margin: 0 0 var(--sp-1-5); font-size: var(--fs-xs); font-weight: 600; text-transform: uppercase;
    letter-spacing: var(--tracking-label); color: var(--ink-faint);
  }
  .timeline-rows { list-style: none; margin: 0; padding: 0; }
  .timeline-row {
    display: grid; grid-template-columns: 3.5rem minmax(0, 1fr) auto auto; gap: var(--sp-2);
    align-items: baseline; padding: var(--sp-1-5) 0; border-bottom: 1px solid var(--line);
  }
  .timeline-row:last-child { border-bottom: 0; }
  .timeline-time { font-family: var(--font-mono); font-size: var(--fs-sm); color: var(--ink-muted); }
  .timeline-tool { font-family: var(--font-mono); font-size: var(--fs-sm); overflow-wrap: anywhere; }
  .timeline-status { font-size: var(--fs-xs); font-weight: 600; color: var(--ink-muted); }
  .timeline-status.ok { color: var(--ok); }
  .timeline-status.failed { color: var(--error); }
  .timeline-caller { font-size: var(--fs-sm); color: var(--ink-faint); }
  .timeline-error { grid-column: 1 / -1; font-size: var(--fs-sm); color: var(--error); }

  /* **THE RECORD FOLDS AFTER THE NEWEST ROWS.** The card holds the machine's newest 200 audit lines;
     drawn whole it buried Build, Updates and Connect. The disclosure is the browser's own, so the
     rest opens without a script, and the seam says what is behind it. */
  details.timeline-rest { padding-top: var(--sp-3); border-top: 1px solid var(--line); }
  details.timeline-rest summary { cursor: pointer; font-size: var(--fs-base); font-weight: 600; color: var(--ink); }
  details.timeline-rest[open] summary { margin-bottom: var(--sp-3); }
  .timeline-rest-body > * + * { margin-top: var(--sp-3); }

  /* The enroll form: the same fields and vocabulary as the console's "New enrollment code", because the
     person pastes the code from there into here. Field and button rules only; the tokens above are
     the panel's. */
  form {
    display: grid; gap: var(--sp-2); margin-top: var(--sp-4); padding-top: var(--sp-4);
    border-top: 1px solid var(--line);
  }
  form h3 { font-size: var(--fs-md); font-weight: 600; margin: 0 0 var(--sp-1); }
  form label { font-size: var(--fs-base); color: var(--ink-muted); }
  form input[type="text"] {
    font: inherit; font-size: var(--fs-md); color: var(--ink);
    background: var(--surface-recessed); border: 1px solid var(--line-strong);
    border-radius: var(--radius-xs); padding: var(--sp-1-5) var(--sp-2); min-width: 0;
  }
  form .check { display: flex; align-items: flex-start; gap: var(--sp-2); color: var(--ink); }
  form .check input { margin: 0.25rem 0 0; }
  form .hint { margin: 0; color: var(--ink-faint); font-size: var(--fs-sm); }
  form button {
    justify-self: start; font: inherit; font-size: var(--fs-base); font-weight: 600;
    padding: var(--sp-1-5) var(--sp-3); border-radius: var(--radius-xs); cursor: pointer;
    /* `--accent` is a FOREGROUND in the dark theme (#ffa36b); as a fill it put white on it at 1.96:1.
       `--accent-solid`/`--accent-fg` is the pair the tokens were measured for. */
    border: 1px solid transparent; background: var(--accent-solid); color: var(--accent-fg);
  }
  form button:disabled { background: var(--panel-2); color: var(--faint); border-color: var(--line); cursor: progress; }
  :focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .enroll-status { margin: 0; min-height: 1.35em; font-size: var(--fs-base); color: var(--ink-muted); }
  .enroll-status.ok { color: var(--ok); }
  .enroll-status.error { color: var(--error); }

  /* The local MCP instructions: the config is the thing to copy, so it gets the free space and a
     button, while the token itself is named by file and never printed. */
  /* The terminal canvas, the same token the console uses: white in the light theme, near-black in the
     dark one. A black block inside a light page is what made the two surfaces read as two products. */
  /* **FIVE CONTROLS USED TO BE THE BROWSER'S, NOT OURS.** `#term-pty`, `#term-port`, `#term-baud`,
     `#term-serial` and `#term-close` matched no selector (they are not inside a form), so they drew
     at the UA's 13.33px/2px radius — the one place the type scale visibly broke. */
  #term-pty, #term-serial, #term-close {
    font-family: var(--font); font-size: var(--fs-base); font-weight: 600;
    padding: var(--sp-1-5) var(--sp-3); border: 1px solid var(--line-strong); border-radius: var(--radius-xs);
    background: var(--panel); color: var(--ink); cursor: pointer;
  }
  #term-pty:hover, #term-serial:hover, #term-close:hover { background: var(--panel-2); }
  #term-port, #term-baud {
    font-family: var(--font); font-size: var(--fs-base); width: 7rem;
    padding: var(--sp-1-5) var(--sp-2); border: 1px solid var(--line-strong); border-radius: var(--radius-xs);
    background: var(--surface-recessed); color: var(--ink);
  }
  /* The session list lives under the canvas now, so it reads as part of the terminal rather than a
     second card that claims the terminal is somewhere else. */
  .term-sessions { margin-top: var(--sp-2-5); padding-top: var(--sp-2); border-top: 1px solid var(--line); }
  .term-sessions dl { grid-template-columns: 4rem 1fr; gap: var(--sp-0-5) var(--sp-2); }
  .term-host { height: 20rem; background: var(--term-canvas); border: 1px solid var(--line); border-radius: var(--radius-xs); padding: var(--sp-1-5); }
  /* **AN EMPTY 20 REM BOX READS AS A BROKEN TERMINAL.** Until a session is attached the canvas is
     blank and the only explanation was a hint underneath it. The placeholder is CSS, so it vanishes
     the moment xterm mounts a child. */
  .term-host:empty::before {
    content: "No session attached";
    display: grid; place-items: center; height: 100%;
    color: var(--muted); font-size: var(--fs-base);
  }
  .xterm .xterm-viewport { background-color: var(--term-canvas); }
  .config {
    margin: var(--sp-2) 0; padding: var(--sp-3); overflow-x: auto;
    /* The config is one long JSON line and the button copies all of it; a phone should not have to
       scroll sideways to read what it is about to paste. */
    white-space: pre-wrap; overflow-wrap: anywhere;
    background: var(--surface-recessed); border: 1px solid var(--line); border-radius: var(--radius-xs);
    font-family: var(--font-mono); font-size: var(--fs-sm); line-height: 1.5; color: var(--ink);
  }
  #copy-local {
    font: inherit; font-size: var(--fs-base); font-weight: 600; padding: var(--sp-1-5) var(--sp-3);
    border-radius: var(--radius-xs); border: 1px solid var(--line-strong);
    background: var(--surface); color: var(--ink); cursor: pointer;
  }
  #copy-local.copied { border-color: var(--ok); color: var(--ok); }

  /* The first screen is a sentence and two actions, not a config block. */
  .hero { display: grid; gap: var(--sp-0-5); padding: var(--sp-0-5) 0 var(--sp-1-5); }
  .hero .eyebrow { margin: 0; color: var(--ink-faint); font-size: var(--fs-xs); font-weight: 600; letter-spacing: var(--tracking-eyebrow); text-transform: uppercase; }
  .hero h2 { margin: var(--sp-0-5) 0 0; font-size: var(--fs-xl); line-height: 1.15; letter-spacing: var(--tracking-tight); }
  .hero .meta { margin: var(--sp-1-5) 0 0; color: var(--ink-muted); font-size: var(--fs-base); }
  .hero .lede { margin: var(--sp-1-5) 0 0; }
  .hero .actions { display: flex; flex-wrap: wrap; gap: var(--sp-2); margin-top: var(--sp-2-5); }
  .button { display: inline-flex; align-items: center; justify-content: center; font: inherit; font-size: var(--fs-base); font-weight: 600; line-height: 1.2; padding: var(--sp-1-5) var(--sp-3); border-radius: var(--radius-xs); cursor: pointer; border: 1px solid var(--line-strong); background: var(--surface); color: var(--ink); text-decoration: none; }
  .button:hover { background: var(--panel-2); border-color: var(--accent); }
  .button.primary { background: var(--accent-solid); border-color: transparent; color: var(--accent-fg); }
  .button.primary:hover { background: var(--accent-hover); color: var(--accent-fg); }
  details.secondary, details.diagnostics { border: 1px solid var(--line); border-radius: var(--radius); background: var(--panel); padding: var(--sp-3) var(--sp-3-5); }
  details.secondary summary, details.diagnostics summary { cursor: pointer; font-size: var(--fs-md); font-weight: 600; color: var(--ink); }
  details.secondary[open] summary, details.diagnostics[open] summary { margin-bottom: var(--sp-2-5); }
  .diagnostics-body { display: grid; gap: var(--sp-3); padding-top: var(--sp-0-5); }
  .diagnostics-body .card { border: 0; padding: 0; box-shadow: none; }
  .diagnostics-body .card .card-head { padding-left: 0; padding-right: 0; }
  .diagnostics-body .card .card-body { padding-left: 0; padding-right: 0; }
  code { font-family: var(--font-mono); font-size: var(--fs-base); }

  /* **THE TOP BAR SITS ON THE SAME COLUMN AS THE CARDS.** It used to be padded 24 px from the window
     edge while `main` is a centred 54 rem column, so on a 1280 px screen the brand started at 24 px
     and the first card at 224 — two different pages stacked. The padding is the column's own offset
     plus its gutter, which falls back to the plain gutter on a window with no margins left to give. */
  header.topbar { padding: var(--sp-2) max(var(--sp-4), calc((100% - 54rem) / 2 + var(--sp-4))); }

  /* A phone, or a console window dragged narrow: an 11 rem label column leaves the value column about
     ten characters wide, so the two stack instead. */
  @media (max-width: 40rem) {
    body { padding: var(--sp-4) var(--sp-3) var(--sp-6); }
    dl { grid-template-columns: 1fr; gap: var(--sp-1); }
    dt { margin-top: var(--sp-2); }
    /* **A WRAPPED BUTTON NEEDS A HEADER THAT GROWS.** The shared `.topbar` is a fixed 56 px, so the
       action that wraps to its own line at 390 px overflowed it by 10 px and printed on top of the
       first paragraph (measured: button bottom 106, paragraph top 96). */
    /* `.topbar` is the class that sets the fixed height, so the selector that lifts it must carry the
       class too: a bare `header` rule loses to `.topbar` on specificity. */
    header.topbar { flex-wrap: wrap; height: auto; min-height: 56px; padding: var(--sp-2) var(--sp-3); row-gap: var(--sp-1-5); }
    header button { margin-left: 0; }
    /* The caller moves under the tool and the outcome, so the time and the outcome stay on the
       first line instead of the four-column row shrinking the tool to a few characters. */
    .timeline-row { grid-template-columns: 3.5rem minmax(0, 1fr) auto; }
    .timeline-caller { grid-column: 2 / -1; }
  }
"##;

/// The panel's stylesheet: the shared tokens, then its own component rules.
fn style_sheet() -> String {
    summrise_ui::style(COMPONENTS)
}

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
    /// The audit's newest lines, oldest first, for the Timeline. `None` is "the audit could not be
    /// read"; `Some(vec![])` is "the audit has no lines", which the page says differently.
    pub(crate) audit: Option<Vec<serde_json::Value>>,
    /// True when the audit held more lines than [`TIMELINE_LINES`], so the page says older lines are
    /// not shown instead of presenting the tail as the whole record.
    pub(crate) audit_truncated: bool,
    /// The instant the day labels ("Today", "Yesterday") are relative to. A request reads it once;
    /// a test pins it instead of racing the wall clock.
    pub(crate) rendered_at_ms: u64,
    pub(crate) console_attempt: Option<RetryNote>,
    /// The console this machine is linked to, when `console.json` names one.
    pub(crate) console_url: Option<String>,
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
    let linked = facts.console_url.as_deref().filter(|url| !url.is_empty());
    let digest = if facts.sha256.is_empty() {
        "unverified"
    } else {
        facts.sha256.as_str()
    };
    let digest_short: String = digest.chars().take(16).collect();
    let chip = match (linked, attempt) {
        (Some(_), _) => html! {
            span id="console-chip" class="chip ok" { "linked" }
        },
        (None, Some(note)) => html! {
            span id="console-chip" class="chip warn" { "retried after " (note.attempts) }
        },
        // **`None` IS NOT "CONNECTED".** A note is recorded at the START of each attempt, so no
        // note means either "this process never tried" or "it was refused and will not retry" —
        // and a green "connected" was shown for both. Neutral is the honest colour.
        (None, None) => html! {
            span id="console-chip" class="chip muted" { "not linked" }
        },
    };
    let state_rows = state_rows(facts.state.as_ref());
    let update = facts.state.as_ref().and_then(|state| state.get("update"));
    let update_phase = update
        .and_then(|update| update.get("phase"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("not recorded");
    let update_target = update
        .and_then(|update| update.get("to"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("none");
    let update_outcome = update
        .and_then(|update| update.get("outcome"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("none");
    let update_last = update
        .and_then(|update| update.get("lastResultAtMs"))
        .and_then(serde_json::Value::as_u64)
        .and_then(local_moment);
    // **THE LAST CHECK IS A FACT OF ITS OWN.** `/api/panel` has carried `lastCheck` all along, but
    // the card never drew it; a build newer than the applied target then read as a stale updater
    // when the device had checked that same day. A document that has no check (an older build, or a
    // device that never checked) says so instead of inventing a time.
    let update_last_check = update
        .and_then(|update| update.get("lastCheck"))
        .and_then(|check| check.get("atMs"))
        .and_then(serde_json::Value::as_u64)
        .and_then(local_moment);
    let event_rows = event_rows(&facts.events);
    let timeline = facts.audit.as_deref().map(|audit| {
        fold_timeline(
            timeline_days(audit, facts.rendered_at_ms),
            TIMELINE_VISIBLE_ROWS,
        )
    });
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
                style { (PreEscaped(style_sheet())) }
            }
            body class="app" {
                header class="topbar" {
                    (PreEscaped(BRAND_MARK))
                    h1 { "Summrise" }
                    span class="context" {
                        span class="dot" aria-hidden="true" {}
                        "this machine"
                    }
                    span class="spacer" {}
                    button id="copy" type="button" data-copy="diagnostics" data-label="Copy diagnostics" {
                        "Copy diagnostics"
                    }
                }
                main id="content" {
                    section class="hero" {
                        p class="eyebrow" { "This machine" }
                        h2 { "Running" }
                        p class="meta" {
                            "Version " (facts.version) " · " (facts.channel) " · pid " (facts.pid) " · loopback only"
                        }
                        @if let Some(url) = facts.console_url.as_deref().filter(|url| !url.is_empty()) {
                            p class="lede" { "Linked to " span class="mono" { (url) } ". The console can reach this machine from anywhere." }
                        } @else {
                            p class="lede" { "Not linked to a console. Enrol it once to reach this machine from anywhere." }
                        }
                        div class="actions" {
                            @if let Some(url) = facts.console_url.as_deref().filter(|url| !url.is_empty()) {
                                a class="button primary" href=(url) { "Open console" }
                            } @else {
                                a class="button primary" href="#console" { "Connect to a console" }
                            }
                            a class="button" href="#terminal" { "Open terminal" }
                        }
                    }

                    (summrise_ui::card(
                        "Console",
                        chip,
                        Markup::default(),
                        html! {
                        @if let Some(url) = linked {
                            dl id="console" {
                                dt { "Console" }
                                dd class="mono" { (url) }
                            }
                            @if let Some(note) = attempt {
                                p class="hint" {
                                    "Last attempt: retried after " (note.attempts) " " (failure_noun(note.attempts)) " — " (note.reason)
                                }
                            }
                            details class="secondary" {
                                summary { "Replace the console link" }
                                (enroll_form(facts, outcome))
                            }
                        } @else {
                            // The chip beside the card's title already says "not linked", and the lede
                            // above says it once more; the hint's job is the instruction, not the third
                            // statement of the same fact.
                            p class="hint" {
                                "In the console, create a one-time enrollment code; paste the URL and the code here."
                            }
                            // **A ROW ABOUT A LINK THAT DOES NOT EXIST.** Unlinked, the card printed
                            // "Link: no failed attempt since this process started" — a reassuring
                            // answer to a question the reader has not asked yet, above the form that
                            // is the actual content. The row appears when there is a failure to
                            // report and stays away when there is not.
                            @if let Some(note) = attempt {
                                dl id="console" {
                                    dt { "Link" }
                                    dd { "retried after " (note.attempts) " " (failure_noun(note.attempts)) }
                                    dt { "Reason" }
                                    dd {
                                        @if note.reason.is_empty() { "(none given)" } @else { (note.reason) }
                                    }
                                }
                            }
                            (enroll_form(facts, outcome))
                        }
                        }
                    ))

                    (summrise_ui::card(
                        "Build",
                        Markup::default(),
                        Markup::default(),
                        html! {
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
                    ))


                    (summrise_ui::card(
                        "Updates",
                        Markup::default(),
                        Markup::default(),
                        html! {
                        dl id="updates" {
                            dt { "Phase" }
                            dd { (update_phase) }
                            dt { "Last target" }
                            dd { (update_target) }
                            dt { "Last outcome" }
                            dd { (update_outcome) }
                            dt { "Last result" }
                            dd {
                                @if let Some((local, exact)) = &update_last {
                                    time datetime=(exact) title=(exact) { (local) }
                                } @else {
                                    "never"
                                }
                            }
                            dt { "Last check" }
                            dd {
                                @if let Some((local, exact)) = &update_last_check {
                                    time datetime=(exact) title=(exact) { (local) }
                                } @else {
                                    "not recorded"
                                }
                            }
                        }
                        form method="post" action="/api/panel/update" {
                            input type="hidden" name="panel_token" value=(facts.panel_token);
                            button id="update-now" type="submit" { "Check for updates now" }
                        }
                        p class="hint" {
                            "The device checks its configured update source on its own. A release is applied only after the signature, digest, channel and revocation checks pass."
                        }
                        }
                    ))


                    (summrise_ui::card(
                        "Connect a client",
                        Markup::default(),
                        Markup::default(),
                        html! {
                        p class="hint" {
                            "For an MCP client on this machine. The token is never printed; read it from the protected file."
                        }
                        dl id="local" {
                            dt { "MCP endpoint" }
                            dd id="local-endpoint" class="mono" { (facts.mcp_endpoint) }
                            dt { "Install token" }
                            dd id="local-token-file" class="mono" { (facts.token_file) }
                            dt { "Read it" }
                            dd id="local-token-command" class="mono" { (facts.token_command) }
                        }
                        details class="secondary" {
                            summary { "Client configuration" }
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
                        }
                    ))

                    (summrise_ui::card(
                        "Timeline",
                        Markup::default(),
                        Markup::default(),
                        html! {
                        // **THE RECORD OF WHAT THIS MACHINE DID, NOT A RAW TABLE.** Every line here
                        // is the device's own audit: one row per call, grouped by the local day and
                        // newest first. A call the bound cut says so; an audit that holds nothing
                        // says that instead of drawing an empty table. The newest rows are drawn and
                        // the rest is one disclosure, so the record cannot put the machine's own
                        // facts behind a 200-line wall.
                        @if let Some(fold) = &timeline {
                            p class="hint" {
                                "The calls this machine recorded, newest first, in this machine's local time."
                            }
                            @if facts.audit_truncated {
                                p class="hint" id="timeline-bound" {
                                    "The newest " (TIMELINE_LINES) " lines; older lines are not shown."
                                }
                            }
                            @if fold.visible.is_empty() && fold.rest.is_empty() {
                                p class="none" id="timeline-empty" {
                                    "No call has been recorded on this device."
                                }
                            } @else {
                                @for day in &fold.visible {
                                    (timeline_day(day))
                                }
                                @if fold.rest_rows > 0 {
                                    details class="timeline-rest" {
                                        summary {
                                            "Show the other " (fold.rest_rows) " "
                                            @if fold.rest_rows == 1 { "call" } @else { "calls" }
                                        }
                                        div class="timeline-rest-body" {
                                            @for day in &fold.rest {
                                                (timeline_day(day))
                                            }
                                        }
                                    }
                                }
                            }
                        } @else {
                            p class="none" id="timeline-unreadable" {
                                "The device's audit could not be read."
                            }
                        }
                        }
                    ))

                    (summrise_ui::card(
                        "Terminal",
                        html! { span class="chip muted" { (facts.sessions.len()) } },
                        Markup::default(),
                        html! {
                        p class="hint" {
                            "Open a pty or a serial line on this machine and type into it. Loopback only, the same registry the MCP's terminal.* tools use. A session does not survive a device restart."
                        }
                        // Five controls in a plain <p> had no whitespace between them, so they formed
                        // one unbreakable inline run and forced the whole grid column wider than the
                        // window on a narrow screen. A wrapping row breaks between them.
                        // **THE ROW SAYS WHICH CONTROL BELONGS WITH WHICH.** Five controls in one run read
                        // as five equal actions: the two boxes had no visible owner and "Close" sat
                        // beside "New serial" as if it opened something. A rule divides the serial
                        // group, and Close is pushed to the far end, away from the actions that create.
                        div class="term-controls" {
                            button id="term-pty" type="button" { "New pty" }
                            span class="term-sep" aria-hidden="true" {}
                            input id="term-port" type="text" placeholder="COM3" value="COM3" aria-label="Serial port" title="Serial port";
                            input id="term-baud" type="number" placeholder="115200" value="115200" aria-label="Baud rate" title="Baud rate";
                            button id="term-serial" type="button" { "New serial" }
                            button id="term-close" class="term-close" type="button" { "Close" }
                        }
                        div id="term-host" class="term-host" {}
                        p class="hint" id="term-status" role="status" {
                            "No session is attached. Open one with the buttons above, or attach one from the list below."
                        }
                        // **ONE CARD ABOUT THE TERMINAL, NOT TWO.** The list used to sit in a second
                        // card whose own sentence said the interactive terminal was elsewhere — a
                        // denial printed directly beneath the terminal. The count is in the head now
                        // and the live rows are here, where the sessions are opened.
                        div id="terminal-sessions" class="term-sessions hint" {
                            @if facts.sessions.is_empty() {
                                "No live session on this machine."
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
                    ))


                    details class="diagnostics" {
                        summary { "Diagnostics" }
                        div class="diagnostics-body" {
                        (summrise_ui::card(
                            "State",
                            Markup::default(),
                            Markup::default(),
                            html! {
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
                        ))


                        (summrise_ui::card(
                            "Recent events",
                            Markup::default(),
                            Markup::default(),
                            html! {
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
                        ))
                    }
                    // The diagnostics text is rendered here so the clipboard button copies a fact, not a
                    // second reading of the API; it is the same report the inline script used to build.

                        }
                    }

                footer class="statusbar" {
                    span { "v" (facts.version) }
                    span class="sep" { "·" }
                    span class="middle" { (facts.channel) }
                    span class="spacer" {}
                    span class="side" { "panel" }
                }
                pre id="diagnostics" hidden { (diagnostics(facts)) }
                script src="/panel/xterm.js" {}
                script src="/panel/addon-fit.js" {}
                script src="/panel/panel-terminal.js" {}
                script { (PreEscaped(COPY_SCRIPT)) }
            }
        }
    }
}

/// The enrollment form, shown directly when unlinked and inside the replace disclosure when linked.
fn enroll_form(facts: &PanelFacts, outcome: Option<&EnrollOutcome>) -> Markup {
    html! {
    form id="enroll-form" method="post" action="/panel/enroll" autocomplete="off" {
        input type="hidden" name="panel_token" value=(facts.panel_token);
        h3 { "Enrol this machine" }
        p class="hint" {
            "Paste the console URL and the one-time code from the console's “New enrollment code”."
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

/// One call in the Timeline: its `begin` line and, when the audit window carries it, its `end`.
#[derive(Debug)]
pub(crate) struct TimelineRow {
    /// The `begin` line's epoch milliseconds; the day group and clock are this instant locally.
    at_ms: u64,
    /// `HH:MM` in this machine's timezone.
    time: String,
    tool: String,
    /// The `end` line's `ok`/`failed`, or the `begin` line's decision when no result is in the
    /// window. Never a word the audit did not write.
    status: String,
    failed: bool,
    /// The audit's own `transport`: the door the call came through.
    caller: String,
    /// The `end` line's `error_kind`, when the call failed.
    error: Option<String>,
}

/// One local day of calls, newest first.
#[derive(Debug)]
pub(crate) struct TimelineDay {
    label: String,
    rows: Vec<TimelineRow>,
}

/// The calls the audit records, grouped by this machine's local day and newest first.
///
/// The audit writes two lines per call, `begin` and `end`, under one `id`; the panel shows one row
/// per call, with the `end` line's outcome and error. An `end` whose `begin` is outside the bounded
/// window carries no tool, so it cannot become a row: that is the one case the pairing drops. A
/// `begin` whose `end` has not been written yet keeps the decision as its status.
pub(crate) fn timeline_days(audit: &[serde_json::Value], now_ms: u64) -> Vec<TimelineDay> {
    use std::collections::{BTreeMap, HashMap};

    let mut calls: Vec<(&serde_json::Value, Option<&serde_json::Value>)> = Vec::new();
    let mut pending: HashMap<u64, usize> = HashMap::new();
    for line in audit {
        let Some(object) = line.as_object() else {
            continue;
        };
        let Some(id) = object.get("id").and_then(serde_json::Value::as_u64) else {
            continue;
        };
        match object.get("event").and_then(serde_json::Value::as_str) {
            Some("begin") => {
                // An id can be reused after a restart; the earlier call stays a row without an end
                // rather than borrowing the new call's result.
                pending.insert(id, calls.len());
                calls.push((line, None));
            }
            Some("end") => {
                if let Some(index) = pending.remove(&id) {
                    if let Some(call) = calls.get_mut(index) {
                        call.1 = Some(line);
                    }
                }
            }
            _ => {}
        }
    }

    let today = local_at(now_ms).map(|at| at.date_naive());
    let mut days: BTreeMap<chrono::NaiveDate, Vec<TimelineRow>> = BTreeMap::new();
    for (begin, end) in calls.iter().rev() {
        let Some(at_ms) = begin.get("ts_ms").and_then(serde_json::Value::as_u64) else {
            continue;
        };
        let Some(at) = local_at(at_ms) else {
            continue;
        };
        let Some(row) = timeline_row(begin, *end, at_ms, &at) else {
            continue;
        };
        days.entry(at.date_naive()).or_default().push(row);
    }

    days.into_iter()
        .rev()
        .map(|(date, mut rows)| {
            // The window is newest first already; the sort is the guarantee, and a stable one keeps
            // two calls in the same millisecond in the order the audit wrote them.
            rows.sort_by_key(|row| std::cmp::Reverse(row.at_ms));
            TimelineDay {
                label: day_label(date, today),
                rows,
            }
        })
        .collect()
}

/// Where the Timeline stops and folds: the newest rows stay drawn, the rest goes behind one
/// disclosure. A day the fold cuts keeps its label on both sides, and the folded side says
/// `(continued)` so two headings cannot read as two different days.
#[derive(Debug)]
pub(crate) struct TimelineFold {
    /// The day groups above the fold, newest first.
    pub(crate) visible: Vec<TimelineDay>,
    /// The day groups behind it; every row here is older than every visible row.
    pub(crate) rest: Vec<TimelineDay>,
    /// How many calls are behind the fold, for the summary's count.
    pub(crate) rest_rows: usize,
}

fn fold_timeline(days: Vec<TimelineDay>, visible_rows: usize) -> TimelineFold {
    let mut visible = Vec::new();
    let mut rest = Vec::new();
    let mut rest_rows = 0;
    let mut budget = visible_rows;
    for mut day in days {
        if budget == 0 {
            rest_rows += day.rows.len();
            rest.push(day);
            continue;
        }
        if day.rows.len() <= budget {
            budget -= day.rows.len();
            visible.push(day);
            continue;
        }
        let folded = day.rows.split_off(budget);
        budget = 0;
        rest_rows += folded.len();
        let label = format!("{} (continued)", day.label);
        visible.push(day);
        rest.push(TimelineDay {
            label,
            rows: folded,
        });
    }
    TimelineFold {
        visible,
        rest,
        rest_rows,
    }
}

/// One local day of the Timeline, drawn the same way on either side of the fold.
fn timeline_day(day: &TimelineDay) -> Markup {
    html! {
        section class="timeline-day" {
            h2 class="timeline-day-label" { (day.label) }
            ol class="timeline-rows" {
                @for row in &day.rows {
                    li class="timeline-row" {
                        time
                            class="timeline-time"
                            datetime=(crate::store::format_utc(row.at_ms / 1_000))
                            title=(crate::store::format_utc(row.at_ms / 1_000))
                        { (row.time) }
                        span class="timeline-tool" { (row.tool) }
                        @if row.failed {
                            span class="timeline-status failed" { (row.status) }
                        } @else {
                            span class="timeline-status" { (row.status) }
                        }
                        @if !row.caller.is_empty() {
                            span class="timeline-caller" { (row.caller) }
                        }
                        @if let Some(error) = &row.error {
                            span class="timeline-error" { (error) }
                        }
                    }
                }
            }
        }
    }
}

fn timeline_row(
    begin: &serde_json::Value,
    end: Option<&serde_json::Value>,
    at_ms: u64,
    at: &chrono::DateTime<Local>,
) -> Option<TimelineRow> {
    let tool = begin
        .get("tool")
        .and_then(serde_json::Value::as_str)
        .filter(|tool| !tool.is_empty())?;
    let decision = begin
        .get("decision")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let (status, failed) = match end {
        Some(end) => {
            let ok = end
                .get("ok")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            (if ok { "ok" } else { "failed" }, !ok)
        }
        None => (decision, decision == "deny"),
    };
    let error = end
        .and_then(|end| end.get("error_kind"))
        .and_then(serde_json::Value::as_str)
        .filter(|kind| !kind.is_empty())
        .map(str::to_owned);

    Some(TimelineRow {
        at_ms,
        time: at.format("%H:%M").to_string(),
        tool: tool.to_owned(),
        status: status.to_owned(),
        failed,
        caller: begin
            .get("transport")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_owned(),
        error,
    })
}

/// The instant in this machine's timezone, or `None` when it is not a representable local time.
fn local_at(milliseconds: u64) -> Option<chrono::DateTime<Local>> {
    let milliseconds = i64::try_from(milliseconds).ok()?;
    Local.timestamp_millis_opt(milliseconds).single()
}

/// One moment as the page draws it: the machine's local clock, and the exact UTC instant for the
/// title. The Timeline and the Updates card read the same clock; only the reader's timezone differs
/// from the audit's UTC spelling, which stays available on hover.
fn local_moment(milliseconds: u64) -> Option<(String, String)> {
    local_at(milliseconds).map(|at| {
        (
            at.format("%Y-%m-%d %H:%M:%S").to_string(),
            crate::store::format_utc(milliseconds / 1_000),
        )
    })
}

const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// `Today`, `Yesterday`, or the date, exactly as the console's Activity timeline labels a day.
fn day_label(day: chrono::NaiveDate, today: Option<chrono::NaiveDate>) -> String {
    let today = today.unwrap_or(day);
    if day == today {
        return "Today".to_owned();
    }
    if today.pred_opt() == Some(day) {
        return "Yesterday".to_owned();
    }
    let weekday = WEEKDAYS[day.weekday().num_days_from_sunday() as usize];
    let month = day.month();
    let day_of_month = day.day();
    if day.year() == today.year() {
        format!("{weekday}, {month}/{day_of_month}")
    } else {
        format!("{weekday}, {month}/{day_of_month}/{}", day.year())
    }
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
                "console link: retried after {} {} — {}",
                note.attempts,
                failure_noun(note.attempts),
                reason
            ));
        }
        None => lines.push("console link: no failed attempt recorded".to_owned()),
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
        let audit = vec![
            json!({
                "id": 1,
                "ts_ms": 1_761_566_400_000u64,
                "event": "begin",
                "tool": "terminal.create",
                "decision": "allow",
                "transport": "panel"
            }),
            json!({
                "id": 1,
                "ts_ms": 1_761_566_401_000u64,
                "event": "end",
                "ok": true,
                "transport": "panel"
            }),
            json!({ "ts_ms": 1_761_566_402_000u64, "event": "repair" }),
            json!({
                "id": 2,
                "ts_ms": 1_761_566_402_000u64,
                "event": "begin",
                "tool": "runs.exec",
                "decision": "allow",
                "transport": "console"
            }),
            json!({
                "id": 2,
                "ts_ms": 1_761_566_403_000u64,
                "event": "end",
                "ok": false,
                "error_kind": "refused",
                "transport": "console"
            }),
        ];
        let events = audit.clone();
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
                "update": {
                    "phase": "idle",
                    "to": null,
                    "outcome": null,
                    "lastResultAtMs": null
                },
                "lastVerdict": {
                    "outcome": "verified",
                    "detail": "the baseline booted",
                    "ts": "2026-10-27T11:00:00Z"
                }
            })),
            events,
            audit: Some(audit),
            audit_truncated: false,
            rendered_at_ms: 1_761_566_404_000,
            console_attempt: Some(RetryNote {
                attempts: 2,
                reason: "the tunnel was down".to_owned(),
            }),
            console_url: Some("https://console.example.test".to_owned()),
            sessions: vec![SessionFacts {
                kind: "pty".to_owned(),
                shell: "bash".to_owned(),
                id: "session-1".to_owned(),
            }],
            panel_token: "a".repeat(64),
        }
    }

    /// **THE PANEL IS ON THE SAME TYPE SCALE, BY NAME.** Eighteen declarations resolved to seven
    /// sizes no reader could rank, because the panel used `rem` while the console used `px` and no
    /// gate read either. The first fix pinned the pixels; this one pins the shared steps themselves
    /// (`--fs-2xs` … `--fs-xl`), so the two surfaces cannot drift apart again — a raw pixel value is
    /// now the failure, not a value outside a list.
    #[test]
    fn the_panel_uses_the_shared_type_scale() {
        const ALLOWED: [&str; 7] = [
            "--fs-2xs",
            "--fs-xs",
            "--fs-sm",
            "--fs-base",
            "--fs-md",
            "--fs-lg",
            "--fs-xl",
        ];
        let mut rest = COMPONENTS;
        let mut seen = std::collections::BTreeSet::new();
        while let Some(at) = rest.find("font-size:") {
            let after = &rest[at + "font-size:".len()..];
            let end = after.find([';', '}']).unwrap_or(after.len());
            let value = after[..end].trim();
            if !value.starts_with("inherit") {
                seen.insert(value.to_owned());
                assert!(
                    ALLOWED.iter().any(|token| value.contains(token)),
                    "font-size:{value} is not one of the shared steps {ALLOWED:?}"
                );
            }
            rest = &after[end..];
        }
        assert!(!seen.is_empty(), "the panel declares no font sizes at all");
    }

    /// **SPACING IS THE RHYTHM, AND THE RHYTHM IS THE TOKENS.** Sixty-one padding, margin and gap
    /// declarations used ten pixel values; nine of them were exactly the shared steps and the tenth
    /// (32) was a step the scale was missing. They are tokens now, and a bare length in one of those
    /// properties is the failure.
    #[test]
    fn the_panel_uses_the_shared_spacing_steps() {
        const PROPERTIES: [&str; 5] = ["padding", "margin", "gap", "row-gap", "column-gap"];
        let mut rest = COMPONENTS;
        let mut seen = 0;
        while let Some(at) = PROPERTIES
            .iter()
            .filter_map(|name| rest.find(&format!("{name}:")))
            .min()
        {
            let after = &rest[at..];
            let end = after.find([';', '}']).unwrap_or(after.len());
            let declaration = &after[..end];
            seen += 1;
            assert!(
                !declaration.contains("px"),
                "spacing is written in pixels: {declaration}"
            );
            rest = &after[end..];
        }
        assert!(seen > 0, "the panel declares no spacing at all");
    }

    /// **A TRACKING VALUE IS A STEP, NOT A NUMBER.** The panel set 0.08, 0.05 and -0.02 by hand while
    /// the console set 0.05, 0.04 and -0.011: six numbers for three intents, and nothing read either
    /// sheet. The steps are tokens now (`--tracking-eyebrow`, `--tracking-label`, `--tracking-tight`),
    /// and a bare length here is the failure.
    #[test]
    fn the_panel_uses_the_shared_tracking_steps() {
        let mut rest = COMPONENTS;
        let mut seen = 0;
        while let Some(at) = rest.find("letter-spacing:") {
            let after = &rest[at + "letter-spacing:".len()..];
            let end = after.find([';', '}']).unwrap_or(after.len());
            let value = after[..end].trim();
            seen += 1;
            assert!(
                value.starts_with("var(--tracking-") || value == "normal" || value == "inherit",
                "letter-spacing:{value} is not a shared step"
            );
            rest = &after[end..];
        }
        assert!(seen > 0, "the panel declares no letter spacing at all");
    }

    /// The panel's primary action and its console chip were the two places the surface claimed
    /// something the tokens or the process could not back up.
    #[test]
    fn the_primary_action_and_the_console_chip_tell_the_truth() {
        let css = COMPONENTS;
        assert!(
            css.contains("background: var(--accent-solid); color: var(--accent-fg)"),
            "the submit button must be the measured pair: --accent is a foreground in the dark theme"
        );
        assert!(
            !css.contains("background: var(--accent); color: #fff"),
            "white on the dark --accent measured 1.96:1"
        );
        assert!(
            css.contains("#term-pty, #term-serial, #term-close"),
            "the terminal's five controls must not fall back to UA defaults"
        );
        let html = page(&facts()).into_string();
        assert!(
            html.contains("retried after 2 failed attempts"),
            "a recorded failure is named: {html}"
        );

        let mut first = facts();
        first.console_attempt = None;
        first.console_url = None;
        let html = page(&first).into_string();
        assert!(
            !html.contains("no failed attempt since this process started"),
            "an unlinked card does not print a row about a link that does not exist: {html}"
        );
        assert!(
            html.contains("In the console, create a one-time enrollment code"),
            "the card carries the instruction instead: {html}"
        );
        assert!(
            html.contains(r#"<span id="console-chip" class="chip muted">"#),
            "and the chip beside the title carries the state: {html}"
        );
        assert!(
            !html.contains("class=\"chip ok\" id=\"console-chip\""),
            "the chip must not be green when nothing was observed: {html}"
        );
    }

    /// Write the panel for a screenshot pass, the way the console's own review renderer does.
    ///
    /// Ignored by default — it writes files and exists to be looked at:
    ///   SUMMRISE_PANEL_DIR=/tmp/panel cargo test -p summrise-device --bin summrise-device \
    ///       -- --ignored render_panel_for_review
    #[test]
    #[ignore]
    fn render_panel_for_review() {
        let Some(dir) = std::env::var_os("SUMMRISE_PANEL_DIR") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("the review directory");
        std::fs::write(dir.join("panel.html"), page(&facts()).into_string())
            .expect("the panel is written");
        let mut empty = facts();
        empty.sessions.clear();
        empty.events.clear();
        empty.audit = Some(Vec::new());
        empty.state = None;
        std::fs::write(dir.join("panel-empty.html"), page(&empty).into_string())
            .expect("the empty panel is written");
        // **THE FIRST RUN IS A THIRD PAGE, NOT THE EMPTY ONE.** "Empty" above still knows a console
        // and a machine that has been updated once; a fresh install knows neither, and that screen is
        // the one an operator meets first. It is rendered here so it can be looked at.
        let mut fresh = facts();
        fresh.sessions.clear();
        fresh.events.clear();
        fresh.audit = Some(Vec::new());
        fresh.state = None;
        fresh.console_url = None;
        fresh.console_attempt = None;
        std::fs::write(dir.join("panel-fresh.html"), page(&fresh).into_string())
            .expect("the first-run panel is written");
        // The bounded record: more than two hundred audit lines, a failed call every seventeenth, so
        // the "older lines are not shown" sentence, the failed tone and the fold (the newest twenty
        // rows drawn, the rest behind the disclosure) can be looked at too. The page's facts carry
        // the last two hundred lines and the last ten events, as the route does.
        let mut bounded = facts();
        let mut lines = vec![json!({
            "id": 0,
            "ts_ms": 1_761_566_400_000u64 - 60_000,
            "event": "begin",
            "tool": "host.facts",
            "decision": "allow",
            "transport": "loopback"
        })];
        for index in 0..100u64 {
            let at = 1_761_566_400_000 + index * 60_000;
            let tool = if index % 2 == 0 {
                "host.facts"
            } else {
                "runs.exec"
            };
            lines.push(json!({
                "id": index + 1,
                "ts_ms": at,
                "event": "begin",
                "tool": tool,
                "decision": "allow",
                "transport": if index % 3 == 0 { "console" } else { "loopback" }
            }));
            lines.push(json!({
                "id": index + 1,
                "ts_ms": at + 1_000,
                "event": "end",
                "ok": index % 17 != 0,
                "error_kind": if index % 17 == 0 { Some("refused") } else { None },
                "transport": if index % 3 == 0 { "console" } else { "loopback" }
            }));
        }
        bounded.events = lines[lines.len() - 10..].to_vec();
        bounded.audit = Some(lines[lines.len() - TIMELINE_LINES..].to_vec());
        bounded.audit_truncated = true;
        bounded.rendered_at_ms = 1_761_566_400_000 + 100 * 60_000;
        std::fs::write(
            dir.join("panel-timeline.html"),
            page(&bounded).into_string(),
        )
        .expect("the bounded timeline page is written");
    }

    #[test]
    fn the_page_renders_every_section_from_the_facts() {
        let html = page(&facts()).into_string();

        for fact in [
            "Connect a client",
            "Terminal",
            "Build",
            "Console",
            "Open console",
            "Diagnostics",
            "Updates",
            "Timeline",
            "Check for updates now",
            "idle",
            "never",
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
            "retried after 2 failed attempts",
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
            html.contains(summrise_ui::TOKENS),
            "the page renders the shared tokens, so its palette is the console's"
        );
        assert!(
            html.contains(r#"<header class="topbar">"#)
                && html.contains(r#"<footer class="statusbar">"#),
            "the page shell is the console's: a top bar and a status strip"
        );
        assert!(
            html.contains("&lt;contents of /var/lib/summrise/token&gt;"),
            "the config block shows the placeholder where the token goes"
        );
    }

    /// The Updates card's two moments are the LAST ones, not a current target: `to` is the target of
    /// the last applied update, which can be older than the build above it. The labels say so, the
    /// last check the state actually carries is drawn, and both moments are local with the exact UTC
    /// instant in the title (the timeline above this card is local too).
    #[test]
    fn the_updates_card_labels_the_last_result_and_shows_the_last_check_locally() {
        let check_at = Local
            .with_ymd_and_hms(2026, 11, 6, 17, 26, 2)
            .single()
            .expect("a real local time")
            .timestamp_millis() as u64;
        let result_at = Local
            .with_ymd_and_hms(2026, 11, 4, 19, 11, 5)
            .single()
            .expect("a real local time")
            .timestamp_millis() as u64;
        let mut facts = facts();
        facts.state = Some(json!({
            "schema": 1,
            "update": {
                "phase": "idle",
                "to": "0.3.273",
                "outcome": "applied",
                "lastResultAtMs": result_at,
                "lastCheck": { "atMs": check_at, "trigger": "channel" }
            }
        }));

        let html = page(&facts).into_string();
        let card = html
            .split(r#"<dl id="updates">"#)
            .nth(1)
            .and_then(|rest| rest.split("</dl>").next())
            .expect("the Updates card is on the page");

        assert!(card.contains("<dt>Last target</dt>"), "{card}");
        assert!(card.contains("<dt>Last outcome</dt>"), "{card}");
        assert!(card.contains("<dt>Last check</dt>"), "{card}");
        assert!(
            !card.contains("<dt>Target</dt>"),
            "the applied target is never a current target: {card}"
        );
        assert!(card.contains("<dd>0.3.273</dd>"), "{card}");
        assert!(card.contains("<dd>applied</dd>"), "{card}");
        assert!(card.contains("2026-11-04 19:11:05"), "{card}");
        assert!(card.contains("2026-11-06 17:26:02"), "{card}");
        assert!(
            card.contains(&format!(
                r#"title="{}""#,
                crate::store::format_utc(check_at / 1_000)
            )),
            "the exact instant is in the last check's title: {card}"
        );
        assert!(
            card.contains(&format!(
                r#"title="{}""#,
                crate::store::format_utc(result_at / 1_000)
            )),
            "the exact instant is in the last result's title: {card}"
        );
    }

    /// A state document written before `lastCheck` existed has no check to show; the card says it
    /// was not recorded rather than inventing "never" or a time.
    #[test]
    fn the_updates_card_says_the_last_check_was_not_recorded() {
        let html = page(&facts()).into_string();
        let card = html
            .split(r#"<dl id="updates">"#)
            .nth(1)
            .and_then(|rest| rest.split("</dl>").next())
            .expect("the Updates card is on the page");

        assert!(card.contains("<dt>Last check</dt>"), "{card}");
        assert!(card.contains("not recorded"), "{card}");
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

    /// Local noon on a fixed day, in whatever timezone the test host has. The panel groups by the
    /// same local calendar, so the fixture's days stay the days under any `TZ`.
    fn local_noon(year: i32, month: u32, day: u32) -> u64 {
        let date = chrono::NaiveDate::from_ymd_opt(year, month, day).expect("a real day");
        let noon = date.and_hms_opt(12, 0, 0).expect("a real time");
        Local
            .from_local_datetime(&noon)
            .earliest()
            .expect("noon exists on this host")
            .timestamp_millis() as u64
    }

    fn begin(id: u64, at: u64, tool: &str, transport: &str) -> serde_json::Value {
        json!({
            "id": id,
            "ts_ms": at,
            "event": "begin",
            "tool": tool,
            "decision": "allow",
            "transport": transport
        })
    }

    fn end(
        id: u64,
        at: u64,
        ok: bool,
        error_kind: Option<&str>,
        transport: &str,
    ) -> serde_json::Value {
        json!({
            "id": id,
            "ts_ms": at,
            "event": "end",
            "ok": ok,
            "error_kind": error_kind,
            "transport": transport
        })
    }

    /// `count` calls on one local day, newest first, one `begin`/`end` pair each.
    fn calls_on_one_day(count: usize, noon: u64) -> Vec<serde_json::Value> {
        let mut audit = Vec::with_capacity(count * 2);
        for index in 0..count as u64 {
            let at = noon + index * 60_000;
            audit.push(begin(index + 1, at, &format!("tool-{index:02}"), "console"));
            audit.push(end(index + 1, at + 1_000, true, None, "console"));
        }
        audit
    }

    #[test]
    fn the_timeline_groups_calls_by_local_day_and_newest_first() {
        let today = local_noon(2026, 11, 6);
        let yesterday = local_noon(2026, 11, 5);
        let older = local_noon(2026, 11, 4);
        let audit = vec![
            begin(1, older + 4 * 3_600_000, "host.facts", "mesh"),
            begin(
                2,
                yesterday - 2 * 3_600_000 - 45 * 60_000,
                "files.read",
                "loopback",
            ),
            end(
                2,
                yesterday - 2 * 3_600_000 - 44 * 60_000,
                true,
                None,
                "loopback",
            ),
            begin(3, today, "terminal.create", "panel"),
            end(3, today + 1_000, true, None, "panel"),
            begin(
                4,
                today + 2 * 3_600_000 + 30 * 60_000,
                "runs.exec",
                "console",
            ),
            end(
                4,
                today + 2 * 3_600_000 + 31 * 60_000,
                false,
                Some("refused"),
                "console",
            ),
        ];

        let days = timeline_days(&audit, today + 3_600_000);

        assert_eq!(days.len(), 3, "one group per local day");
        assert_eq!(days[0].label, "Today");
        assert_eq!(
            days[0]
                .rows
                .iter()
                .map(|row| row.time.as_str())
                .collect::<Vec<_>>(),
            ["14:30", "12:00"],
            "today's calls are newest first"
        );
        assert_eq!(days[0].rows[0].tool, "runs.exec");
        assert_eq!(days[0].rows[0].status, "failed");
        assert!(days[0].rows[0].failed);
        assert_eq!(days[0].rows[0].error.as_deref(), Some("refused"));
        assert_eq!(days[0].rows[0].caller, "console");
        assert_eq!(days[0].rows[1].status, "ok");
        assert_eq!(days[1].label, "Yesterday");
        assert_eq!(days[1].rows.len(), 1);
        assert_eq!(days[1].rows[0].tool, "files.read");
        assert_eq!(days[2].label, "Wednesday, 11/4");
        assert_eq!(
            days[2].rows[0].status, "allow",
            "a call whose end is not in the window keeps the decision the begin line wrote"
        );

        // The same facts reach the page: a call row, its failed tone and the audit's error word.
        let mut facts = facts();
        facts.audit = Some(audit);
        facts.rendered_at_ms = today + 3_600_000;
        let html = page(&facts).into_string();
        assert!(html.contains(r#"<h2 class="timeline-day-label">Today</h2>"#));
        assert!(html.contains(r#"<span class="timeline-tool">runs.exec</span>"#));
        assert!(html.contains(r#"<span class="timeline-status failed">failed</span>"#));
        assert!(html.contains(r#"<span class="timeline-error">refused</span>"#));
        assert!(html.contains(r#"<span class="timeline-caller">console</span>"#));
    }

    #[test]
    fn the_timeline_escapes_the_tool_name_it_reads_from_the_audit() {
        let mut facts = facts();
        facts.audit = Some(vec![
            begin(
                1,
                1_761_566_400_000,
                "<script>alert(1)</script>",
                "loopback",
            ),
            end(1, 1_761_566_401_000, true, None, "loopback"),
        ]);
        let html = page(&facts).into_string();

        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(!html.contains("<script>alert(1)</script>"));
    }

    #[test]
    fn the_timeline_says_when_it_is_empty_bounded_or_unreadable() {
        let mut empty = facts();
        empty.audit = Some(Vec::new());
        let html = page(&empty).into_string();
        assert!(html.contains("No call has been recorded on this device."));
        assert!(
            !html.contains("older lines are not shown"),
            "an uncut audit does not claim a cut"
        );

        let mut bounded = facts();
        bounded.audit_truncated = true;
        let html = page(&bounded).into_string();
        assert!(html.contains("The newest 200 lines; older lines are not shown."));

        let mut unreadable = facts();
        unreadable.audit = None;
        let html = page(&unreadable).into_string();
        assert!(html.contains("The device's audit could not be read."));
        assert!(
            !html.contains("No call has been recorded on this device."),
            "an unreadable audit is not an empty one"
        );
    }

    /// **THE RECORD DOES NOT BURY THE MACHINE.** Drawn whole, a 200-line record is 3,500 px at 1440
    /// and 6,200 px at 390: Build, Updates and Connect sat behind it and the operator scrolled
    /// 4–8k px to read the build sha or the last check. The newest rows stay drawn and the rest is
    /// one server-rendered disclosure, so the fold opens without a script and no row is dropped.
    #[test]
    fn the_timeline_folds_after_the_newest_rows_and_keeps_every_row() {
        let noon = local_noon(2026, 11, 6);
        let mut facts = facts();
        facts.audit = Some(calls_on_one_day(30, noon));
        facts.rendered_at_ms = noon + 3_600_000;
        let html = page(&facts).into_string();

        assert_eq!(
            html.matches(r#"<li class="timeline-row">"#).count(),
            30,
            "every call is drawn exactly once: {html}"
        );
        let fold = html
            .find(r#"<details class="timeline-rest">"#)
            .expect("30 rows fold after 20");
        let visible = &html[..fold];
        let folded = &html[fold..];
        let details_end = folded.find("</details>").expect("the fold closes") + "</details>".len();
        let details = &folded[..details_end];

        assert_eq!(
            visible.matches(r#"<li class="timeline-row">"#).count(),
            TIMELINE_VISIBLE_ROWS,
            "the newest 20 rows stay out of the fold"
        );
        assert_eq!(
            details.matches(r#"<li class="timeline-row">"#).count(),
            10,
            "the other 10 are behind it"
        );
        assert!(
            details.starts_with(
                r#"<details class="timeline-rest"><summary>Show the other 10 calls</summary>"#
            ),
            "the summary says what the fold holds: {details}"
        );
        assert!(
            !details.contains("<script"),
            "the fold is server-rendered and opens without a script: {details}"
        );
        assert!(visible.contains(r#"<h2 class="timeline-day-label">Today</h2>"#));
        assert!(
            details.contains(r#"<h2 class="timeline-day-label">Today (continued)</h2>"#),
            "a day the fold cuts keeps its name: {details}"
        );

        // The record keeps its newest-first order across the seam: tool-29..tool-10 are above the
        // fold and tool-09..tool-00 below it.
        let position = |index: usize| {
            html.find(&format!(">tool-{index:02}<"))
                .unwrap_or_else(|| panic!("tool-{index:02} is missing"))
        };
        assert!(position(29) < position(10), "the newest 20 are drawn first");
        assert!(position(10) < fold, "the 20th row is the last one drawn");
        assert!(fold < position(9), "the fold opens with the 21st");
        for index in 1..30 {
            assert!(
                position(index) < position(index - 1),
                "the rows stay newest first across the fold"
            );
        }
    }

    /// The fold is a disclosure only when it has something to disclose: at the visible cap the
    /// Timeline is drawn whole, and one row behind it is "1 call", not "1 calls".
    #[test]
    fn the_timeline_folds_only_past_the_visible_rows() {
        let noon = local_noon(2026, 11, 6);
        let mut at_cap = facts();
        at_cap.audit = Some(calls_on_one_day(TIMELINE_VISIBLE_ROWS, noon));
        at_cap.rendered_at_ms = noon + 3_600_000;
        let html = page(&at_cap).into_string();
        assert!(
            !html.contains(r#"<details class="timeline-rest">"#),
            "the visible rows are drawn whole, with no empty fold: {html}"
        );
        assert_eq!(
            html.matches(r#"<li class="timeline-row">"#).count(),
            TIMELINE_VISIBLE_ROWS
        );

        let mut over = facts();
        over.audit = Some(calls_on_one_day(TIMELINE_VISIBLE_ROWS + 1, noon));
        over.rendered_at_ms = noon + 3_600_000;
        let html = page(&over).into_string();
        assert!(
            html.contains("<summary>Show the other 1 call</summary>"),
            "the summary counts in the reader's words: {html}"
        );

        // A day behind the fold that the fold did not cut keeps its own name: "(continued)" marks a
        // split day, not every day behind the disclosure.
        let mut two_days = facts();
        let mut audit = calls_on_one_day(TIMELINE_VISIBLE_ROWS, noon);
        audit.extend(calls_on_one_day(2, noon - 86_400_000));
        two_days.audit = Some(audit);
        two_days.rendered_at_ms = noon + 3_600_000;
        let html = page(&two_days).into_string();
        let details = &html[html
            .find(r#"<details class="timeline-rest">"#)
            .expect("the fold is after the visible rows")..];
        let details = &details[..details.find("</details>").expect("the fold closes")];
        assert!(
            details.contains(r#"<h2 class="timeline-day-label">Yesterday</h2>"#),
            "an uncut day behind the fold keeps its own name: {details}"
        );
        assert!(
            !details.contains("Yesterday (continued)"),
            "only a day the fold split is marked: {details}"
        );
    }

    /// **THE FACTS COME BEFORE THE RECORD.** The Timeline is the longest card; Build, Updates and
    /// Connect were under up to 200 rows of it (the audit measured Build at y 4,636 at 1440 and
    /// 7,625 at 390). The page now reads: the machine, its facts, then its record.
    #[test]
    fn the_build_updates_and_connect_facts_are_above_the_timeline() {
        let html = page(&facts()).into_string();
        let titles: Vec<&str> = html
            .split("<h2>")
            .skip(1)
            .filter_map(|rest| rest.split("</h2>").next())
            .collect();
        assert_eq!(
            titles,
            [
                "Running",
                "Console",
                "Build",
                "Updates",
                "Connect a client",
                "Timeline",
                "Terminal",
                "State",
                "Recent events",
            ],
            "the panel's reading order: the machine, its facts, its record, its tools"
        );
        let first_row = html
            .find(r#"<li class="timeline-row">"#)
            .expect("the fixture records two calls");
        for fact in [
            "<h2>Build</h2>",
            "<h2>Updates</h2>",
            "<h2>Connect a client</h2>",
        ] {
            assert!(
                html.find(fact).expect(fact) < first_row,
                "{fact} is above the Timeline's first row"
            );
        }
    }

    #[test]
    fn an_empty_page_says_there_are_no_sessions_events_or_readable_state() {
        let mut facts = facts();
        facts.sessions.clear();
        facts.events.clear();
        facts.audit = Some(Vec::new());
        facts.state = None;
        facts.console_attempt = None;
        facts.console_url = None;

        let html = page(&facts).into_string();

        assert!(html.contains("No live session on this machine."));
        assert!(html.contains("No event has been recorded on this device."));
        assert!(html.contains("No call has been recorded on this device."));
        assert!(
            !html.contains(r#"id="no-events" hidden"#),
            "the empty-state line is visible when there is no event"
        );
        assert!(html.contains("unreadable"));
        // **THE OLD TEST PINNED THE LIE**: it asserted a green "connected" chip for a page whose
        // facts carry no attempt at all. No note means nothing was observed, not that the link works.
        assert!(html.contains(r#"<span id="console-chip" class="chip muted">"#));
        assert!(!html.contains(r#"<span id="console-chip" class="chip ok">"#));
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
        assert!(
            text.contains("console link: retried after 2 failed attempts — the tunnel was down")
        );
        assert!(text.contains("last verdict: verified the baseline booted at 2026-10-27T11:00:00Z"));
        assert!(text.contains("terminal.create allow → ok"));
        assert!(text.contains("  repair"));
    }
}
