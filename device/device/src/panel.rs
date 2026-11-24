//! The device's own panel, rendered in Rust.
//!
//! The page is a function of [`PanelFacts`]: the HTTP handler reads every fact once, then renders
//! it, so a browser reload is a fresh read and an enrollment is a plain form POST followed by a
//! redirect. Browser scripts render terminal bytes and copy text already present in the document.

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

/// Device-local component rules; shared palette, typography, radii and focus come from summrise_ui.
pub(crate) const COMPONENTS: &str = r##"  body.app { flex-direction: column; }
  body { padding: 0; font-variant-numeric: tabular-nums; -webkit-font-smoothing: antialiased; }
  header.topbar { height:auto; }
  .topbar > svg { width: 28px; height: 28px; flex: none; }
  .brand-name { font-size: var(--fs-lg); font-weight: 650; letter-spacing: var(--tracking-tight); }
  .device-nav { margin-left: auto; }
  .copy-tools { display: flex; flex-wrap: wrap; align-items: center; gap: var(--sp-2); }
  .copy-status { color: var(--muted); font-size: var(--fs-base); }
  main > *, .connection-grid > *, .maintenance-grid > *, .card-body > * { min-width: 0; }
  main .card { margin-bottom: 0; }
  .panel-group { display: grid; gap: var(--sp-3); }
  .group-label, .eyebrow { margin: 0; color: var(--faint); font-size: var(--fs-xs); font-weight: 600; letter-spacing: var(--tracking-label); text-transform: uppercase; }
  .connection-grid { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:var(--sp-5); align-items:start; }
  .maintenance-grid { display:grid; gap:var(--sp-4); }
  #terminal { grid-column: 1 / -1; }
  .hero { padding:0; border:0; background:transparent; box-shadow:none; }
  .hero-identity { min-width: 0; }
  .hero h1 { margin: var(--sp-2) 0 0; font-size: var(--fs-xl); line-height: var(--lh-page); letter-spacing: var(--tracking-display); font-weight: 650; }
  .hero .meta { margin:var(--sp-3) 0 0; color:var(--muted); font-size:var(--fs-base); }
  .lede { margin:var(--sp-3) 0 0; color:var(--muted); font-size:var(--fs-md); max-width:var(--measure); }
  .hero .actions { display:flex; flex-wrap:wrap; gap:var(--sp-2); }
  .copy-tools { margin-left:auto; }
  .local-support .card { border:0; background:transparent; }
  .local-support .card-head, .local-support .card-body { padding-left:0; padding-right:0; }
  .card-head { min-height: 48px; padding: var(--sp-3) var(--sp-4); }
  .card-head h2 { color: var(--ink); font-size: var(--fs-lg); text-transform: none; letter-spacing: normal; }
  .card-body { padding: var(--sp-4); }
  .card .card-body > * + * { margin-top: var(--sp-3); }
  .hint, .none { color: var(--muted); font-size: var(--fs-md); line-height: 1.6; }
  p.hint, p.none { margin: 0; }
  dl { display: grid; grid-template-columns: minmax(7rem, .4fr) minmax(0, 1fr); gap: var(--sp-2) var(--sp-3); margin: 0; }
  dt { color: var(--muted); font-size: var(--fs-base); }
  dd { margin: 0; overflow-wrap: anywhere; font-size: var(--fs-base); }
  .mono, code { font-family: var(--font-mono); font-size: var(--fs-base); }
  .button, header button, #copy-local, #term-pty, #term-serial, #term-close {
    display: inline-flex; align-items: center; justify-content: center; min-height: var(--control-h);
    font-family: var(--font); font-size: var(--fs-base); font-weight: 600; line-height: 1.3;
    padding: var(--sp-2) var(--sp-3); border: 1px solid var(--line-strong); border-radius: var(--radius-xs);
    background: var(--surface); color: var(--ink); cursor: pointer; text-decoration: none;
  }
  .button:hover, header button:hover, #copy-local:hover, #term-pty:hover, #term-serial:hover, #term-close:hover { background: var(--panel-2); border-color: var(--accent); }
  .button.primary, form button { background: var(--accent-solid); color: var(--accent-fg); border-color: transparent; }
  .button.primary:hover, form button:hover { background: var(--accent-hover); color: var(--accent-fg); }
  header button.copied, #copy-local.copied { border-color: var(--ok); color: var(--ok); }
  .chip, .claim-badge { border-radius: var(--radius-xs); }
  .chip { height: auto; min-height: 24px; white-space: normal; font-size: var(--fs-base); }
  form { display: grid; gap: var(--sp-2); margin-top: var(--sp-4); padding-top: var(--sp-4); border-top: 1px solid var(--line); }
  form h3 { margin: 0 0 var(--sp-1); font-size: var(--fs-base); font-weight: 600; }
  form label { color: var(--muted); font-size: var(--fs-base); }
  form input[type="text"], #term-port, #term-baud {
    width: 100%; min-width: 0; font: inherit; font-size: var(--fs-base); color: var(--ink);
    padding: var(--sp-2); border: 1px solid var(--line-strong); border-radius: var(--radius-xs); background: var(--panel);
  }
  form .check { display: flex; align-items: flex-start; gap: var(--sp-2); min-height: var(--control-h); color: var(--ink); }
  form .check input { flex: none; margin: var(--sp-1) 0 0; }
  form .hint { margin: 0; color: var(--muted); font-size: var(--fs-base); }
  form button { justify-self: start; min-height: var(--control-h); font: inherit; font-size: var(--fs-base); font-weight: 600; padding: var(--sp-2) var(--sp-3); border: 1px solid transparent; border-radius: var(--radius-xs); cursor: pointer; }
  form button:disabled { background: var(--panel-2); color: var(--faint); border-color: var(--line); cursor: progress; }
  .enroll-status { margin: 0; min-height: 1.35em; font-size: var(--fs-base); color: var(--muted); }
  .enroll-status.ok { color: var(--ok); }
  .enroll-status.error { color: var(--error); }
  details.secondary, details.diagnostics { border: 1px solid var(--line); border-radius: var(--radius); background: var(--panel); padding: var(--sp-3) var(--sp-4); }
  details.secondary summary, details.diagnostics summary { display: list-item; min-height: var(--control-h); padding: var(--sp-1) 0; font-size: var(--fs-base); font-weight: 600; color: var(--ink); cursor: pointer; }
  details.secondary[open] summary, details.diagnostics[open] summary { margin-bottom: var(--sp-3); }
  details.secondary form { margin-top: 0; }
  .config { margin: var(--sp-2) 0; max-width: 100%; padding: var(--sp-3); white-space: pre-wrap; overflow-wrap: anywhere; background: var(--surface-recessed); border: 1px solid var(--line); border-radius: var(--radius-xs); font-family: var(--font-mono); font-size: var(--fs-base); line-height: 1.6; color: var(--ink); }
  #diagnostics:not([hidden]) { margin: var(--sp-4) var(--device-gutter); padding: var(--sp-4); white-space: pre-wrap; overflow-wrap: anywhere; background: var(--panel); border: 1px solid var(--line); border-radius: var(--radius); }
  .term-controls { display: flex; flex-wrap: wrap; align-items: end; gap: var(--sp-3); }
  .term-new, .term-serial-fields { display: flex; flex-wrap: wrap; align-items: end; gap: var(--sp-2); }
  .term-serial-fields { padding-left: var(--sp-3); border-left: 1px solid var(--line); }
  .term-field { display: grid; gap: var(--sp-1); width: 8rem; max-width: 100%; }
  .term-field label { color: var(--muted); font-size: var(--fs-base); }
  .term-close { margin-left: auto; }
  /* The terminal sizes itself from the box this pane offers it, so that box must be the
     content area: a border-box height hides this pane's own padding and border from the
     measurement, the terminal then renders one row more than the pane can show, and that
     last row is cut off. content-box keeps the measured height equal to the space the
     terminal really has; the height is compensated so the pane still occupies 20rem. */
  .term-host { box-sizing: content-box; height: calc(20rem - 2 * var(--sp-2) - 2px); min-width: 0; overflow: hidden; background: var(--term-canvas); border: 1px solid var(--line); border-radius: var(--radius-xs); padding: var(--sp-2); }
  .term-host:empty::before { content: "No session attached"; display: grid; place-items: center; height: 100%; color: var(--muted); font-size: var(--fs-base); }
  .xterm .xterm-viewport { background-color: var(--term-canvas); }
  .term-sessions { margin-top: var(--sp-3); padding-top: var(--sp-3); border-top: 1px solid var(--line); }
  .session-card { padding: var(--sp-3) 0; border-bottom: 1px solid var(--line); }
  .session-card:last-child { border-bottom: 0; }
  .session-head { display: flex; flex-wrap: wrap; align-items: baseline; gap: var(--sp-2); }
  .session-dot { width: 7px; height: 7px; flex: none; border-radius: 50%; background: var(--faint); display: inline-block; }
  .session-dot.live { background: var(--ok); }
  .session-kind { font-weight: 600; }
  .session-id, .session-holder { color: var(--muted); }
  .session-holder { margin-left: auto; font-size: var(--fs-sm); }
  .session-shell { margin: var(--sp-1) 0 0; color: var(--muted); }
  .session-goal, .session-txn, .session-call { margin: var(--sp-2) 0 0; font-size: var(--fs-base); }
  .session-goal { font-family: var(--font-serif); }
  .session-goal .claim-badge, .session-goal-by { font-family: var(--font); }
  .session-goal.none { font-family: var(--font); }
  .session-goal-by { color: var(--faint); font-size: var(--fs-sm); }
  .claim-badge { display: inline-flex; padding: 0 var(--sp-1); border: 1px solid var(--accent); color: var(--accent); font-size: var(--fs-xs); font-weight: 600; }
  .timeline-day-label { margin: 0 0 var(--sp-2); font-size: var(--fs-xs); font-weight: 600; text-transform: uppercase; letter-spacing: var(--tracking-label); color: var(--faint); }
  .timeline-rows { list-style: none; margin: 0; padding: 0; }
  .timeline-row { display: grid; grid-template-columns: 3.5rem minmax(0, 1fr) auto auto; gap: var(--sp-2); align-items: baseline; padding: var(--sp-2) 0; border-bottom: 1px solid var(--line); }
  .timeline-row:last-child { border-bottom: 0; }
  .timeline-time, .timeline-tool { font-family: var(--font-mono); font-size: var(--fs-base); overflow-wrap: anywhere; }
  .timeline-time, .timeline-caller { color: var(--muted); }
  .timeline-caller { font-size: var(--fs-sm); }
  .timeline-error { grid-column: 1 / -1; font-size: var(--fs-base); color: var(--error); }
  .timeline-txn { padding: var(--sp-3) 0; }
  .timeline-txn + .timeline-txn { border-top: 1px solid var(--line); }
  .timeline-txn-head { display: flex; flex-wrap: wrap; align-items: baseline; gap: var(--sp-2); margin: 0 0 var(--sp-2); }
  .timeline-txn-purpose { font-weight: 600; font-size: var(--fs-base); font-family: var(--font-serif); }
  .timeline-txn-receipt { color: var(--muted); font-size: var(--fs-base); }
  .timeline-txn-declared, .timeline-txn-observed { display: inline-flex; flex-wrap: wrap; align-items: baseline; gap: var(--sp-1); }
  .receipt-label { color: var(--faint); font-size: var(--fs-sm); }
  .receipt-drift { display: flex; flex-wrap: wrap; align-items: baseline; gap: var(--sp-2); margin: 0 0 var(--sp-2); color: var(--warn); font-size: var(--fs-base); }
  .receipt-drift-label, .timeline-txn-by { color: var(--faint); font-size: var(--fs-sm); }
  details.timeline-rest { padding-top: var(--sp-3); border-top: 1px solid var(--line); }
  details.timeline-rest summary { cursor: pointer; font-size: var(--fs-base); font-weight: 600; color: var(--ink); }
  details.timeline-rest[open] summary { margin-bottom: var(--sp-3); }
  .timeline-rest-body > * + * { margin-top: var(--sp-3); }
  .diagnostics-body { display: grid; gap: var(--sp-4); }
  .diagnostics-body .card { border: 0; box-shadow: none; }
  .diagnostics-body .card-head, .diagnostics-body .card-body { padding-left: 0; padding-right: 0; }
  table { width: 100%; border-collapse: collapse; table-layout: fixed; }
  th, td { text-align: left; padding: var(--sp-2); border-bottom: 1px solid var(--line); font-size: var(--fs-base); overflow-wrap: anywhere; vertical-align: top; }
  th { color: var(--faint); font-size: var(--fs-sm); font-weight: 600; }
  th:first-child { width: 35%; }
  tr:last-child td { border-bottom: 0; }
  .statusbar { height: auto; min-height: 32px; padding: var(--sp-2) var(--device-gutter); white-space: normal; }
  .statusbar .sep { color: var(--muted); opacity: 1; }
  .statusbar .side { font-size: var(--fs-sm); }
  :focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  @media (forced-colors: active) { :focus-visible { outline: 2px solid CanvasText; } }
  @media (max-width: 800px) {
    .hero { flex-direction: column; align-items: start; }
    .connection-grid, .maintenance-grid { grid-template-columns: minmax(0, 1fr); }
    dl { grid-template-columns: minmax(7rem, .35fr) minmax(0, 1fr); }
  }
  @media (max-width: 600px) {
    header.topbar { gap: var(--sp-2); padding: var(--sp-3) var(--device-gutter); }
    .device-nav { margin-left: auto; }
    .copy-tools { width: 100%; }
    .hero { padding: var(--sp-4); gap: var(--sp-4); }
    .hero .actions { width: 100%; }
    dl { grid-template-columns: minmax(0, 1fr); gap: var(--sp-1); }
    dt:not(:first-child) { margin-top: var(--sp-2); }
    .term-serial-fields { width: 100%; padding-left: 0; padding-top: var(--sp-3); border-left: 0; border-top: 1px solid var(--line); }
    .term-close { margin-left: 0; }
    .timeline-row { grid-template-columns: 3.5rem minmax(0, 1fr) auto; }
    .timeline-caller { grid-column: 2 / -1; }
  }
"##;

/// Canonical UI prefix, shared device presentation, then this surface's components.
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
    try {
      area.select();
      return document.execCommand("copy");
    } catch (_) {
      return false;
    } finally {
      area.remove();
    }
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
      const copied = await copy(source.textContent).catch(function () { return false; });
      if (!copied) {
        source.hidden = false;
        const range = document.createRange();
        range.selectNodeContents(source);
        const selection = window.getSelection();
        if (selection) {
          selection.removeAllRanges();
          selection.addRange(range);
        }
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
    /// The door that opened the session, as a person reads it (`token srm_7f`, `the panel`).
    pub(crate) holder: String,
    /// The session's umbrella goal: a claim with its author, never an instruction.
    pub(crate) goal: Option<String>,
    pub(crate) goal_by: Option<String>,
    /// The transaction that is working on this session now, and the purpose it claims.
    pub(crate) open_txn: Option<String>,
    pub(crate) open_purpose: Option<String>,
    /// The newest call the bounded audit tail records for this session.
    pub(crate) last_call: Option<String>,
    pub(crate) last_call_at_ms: Option<u64>,
    /// Whether the far end is still producing: the registry's own fact.
    pub(crate) alive: bool,
}

/// One transaction as both Rust pages show it: the declaration, the device's observation, and
/// whether the process still holds the record.
///
/// `retained` is the honest answer to "does a receipt exist here?": a group read only from the
/// audit survives a restart, but its receipt does not. The counters are the device's totals (the
/// registry's own), never a count of the rows a bounded window happens to carry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TxnFact {
    pub(crate) id: String,
    pub(crate) purpose: String,
    pub(crate) author: String,
    pub(crate) session: Option<String>,
    pub(crate) state: String,
    pub(crate) reason: Option<String>,
    pub(crate) started_at_ms: u64,
    pub(crate) ended_at_ms: Option<u64>,
    pub(crate) calls: u64,
    pub(crate) failures: u64,
    pub(crate) refusals: u64,
    pub(crate) duration_ms: u64,
    pub(crate) retained: bool,
}

impl TxnFact {
    /// The receipt as the drift rule may read it: the declared ending and the device's own counts.
    ///
    /// The purpose, the reason and the goals are the author's text; the shared rule is arithmetic
    /// over the receipt and cannot see them.
    pub(crate) fn reading(&self) -> crate::activity::ReceiptReading<'_> {
        crate::activity::ReceiptReading {
            state: &self.state,
            calls: self.calls,
            failures: self.failures,
            refusals: self.refusals,
            retained: self.retained,
        }
    }
}

/// A token id as a person reads it: the first eight characters, never the whole hash.
///
/// A credential id is 64 hex characters; printed whole it is wider than the card that names it and
/// reads as a wall of hex, not as an attribution. A short id (the fixture's `srm_7f`) is unchanged:
/// shortening it would hide a difference the person already sees.
fn token_attribution(id: &str) -> String {
    if id.chars().count() > 8 {
        id.chars().take(8).collect()
    } else {
        id.to_owned()
    }
}

/// A holder as the pages name it. Attribution only: no rank, no person, no permission.
pub(crate) fn holder_label(holder: &summrise_core::Holder) -> String {
    use summrise_core::HolderKind;

    match holder.kind {
        HolderKind::Install => "the install door".to_owned(),
        HolderKind::Panel => "the panel".to_owned(),
        HolderKind::Console => "the console operator".to_owned(),
        HolderKind::Token => match &holder.token_id {
            Some(id) => format!("token {}", token_attribution(id)),
            None => "a client token".to_owned(),
        },
        HolderKind::Grant => "a console grant".to_owned(),
        HolderKind::Mesh => "a mesh peer".to_owned(),
    }
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
    /// The transactions the audit names: this process's records (working and retained endings, the
    /// receipts the timeline shows) and, with `retained: false`, a group read only from the audit —
    /// present so the page can say the receipt is gone, never as a receipt with zero counts.
    pub(crate) txns: Vec<TxnFact>,
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

/// One live session, as the panel's own card: the holder, the umbrella goal (a claim), the open
/// transaction and the newest call the bounded record carries.
///
/// Nothing here is inferred. A session with no goal says so; a session with no working transaction
/// says so; a session the window has no call for says its last call is not in this window.
fn session_card(session: &SessionFacts) -> Markup {
    let last_call = match (&session.last_call, session.last_call_at_ms) {
        (Some(tool), Some(at_ms)) => match local_at(at_ms) {
            Some(at) => format!("{tool} · {}", at.format("%H:%M")),
            None => tool.clone(),
        },
        (Some(tool), None) => tool.clone(),
        _ => String::new(),
    };
    html! {
        article class="session-card" data-session=(session.id) {
            div class="session-head" {
                span
                    class=(if session.alive { "session-dot live" } else { "session-dot" })
                    aria-hidden="true"
                {}
                span class="session-kind" { (session.kind) }
                span class="session-id mono" { (session.id) }
                span class="session-holder" { (session.holder) }
            }
            p class="session-shell mono" { (session.shell) }
            @if let Some(goal) = &session.goal {
                p class="session-goal" {
                    span class="claim-badge" { "CLAIM" }
                    " " (goal)
                    @if let Some(by) = &session.goal_by {
                        span class="session-goal-by" { " · set by " (by) }
                    }
                }
            } @else {
                p class="session-goal none" { "No goal set." }
            }
            @if let Some(id) = &session.open_txn {
                p class="session-txn" {
                    "Open transaction " span class="mono" { (id) }
                    @if let Some(purpose) = &session.open_purpose {
                        " · " (purpose)
                    }
                }
            } @else {
                p class="session-txn none" { "No transaction is working on this session." }
            }
            @if last_call.is_empty() {
                p class="session-call none" { "No call recorded in this window." }
            } @else {
                p class="session-call" {
                    "Last call " span class="mono" { (last_call) }
                }
            }
        }
    }
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
    // Rows this build cannot classify are counted, like the ledger counts its unattributed rows.
    let unknown_rows = timeline
        .as_ref()
        .map(|fold| {
            fold.visible
                .iter()
                .chain(fold.rest.iter())
                .flat_map(|day| day.rows.iter())
                .filter(|row| row.unknown)
                .count()
        })
        .unwrap_or(0);
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
            body class="app local-workspace" {
                (summrise_ui::device_navigation(summrise_ui::DevicePage::Machine, html! { (PreEscaped(BRAND_MARK)) }))
                div class="local-workspace-body" {
                header class="topbar local-topbar" {
                    div class="local-breadcrumb" { span { "Local workspace" } span aria-hidden="true" { "/" } strong { "This machine" } }
                    div class="copy-tools" {
                        button id="copy" type="button" data-copy="diagnostics" data-status="copy-status" data-label="Copy diagnostics"
                            data-copied="Diagnostics copied." data-refused="The clipboard refused; select the diagnostics below and copy them." {
                            "Copy diagnostics"
                        }
                        span class="copy-status" id="copy-status" role="status" {}
                    }
                }
                main id="content" tabindex="-1" {
                    section class="hero page-head" {
                        div class="hero-identity" {
                            p class="eyebrow" { "Device runtime" }
                            h1 { "This machine" }
                            p class="meta" {
                                span class="chip ok" { "Running" } " · Version " (facts.version) " · " (facts.channel) " · pid " (facts.pid) " · loopback only"
                            }
                            @if let Some(url) = facts.console_url.as_deref().filter(|url| !url.is_empty()) {
                                p class="lede" { "Linked to " span class="mono" { (url) } ". The console can reach this machine from anywhere." }
                            } @else {
                                p class="lede" { "Not linked to a console. Enrol it once to reach this machine from anywhere." }
                            }
                        }
                        div class="actions" {
                            @if let Some(url) = facts.console_url.as_deref().filter(|url| !url.is_empty()) {
                                a class="button primary" href=(url) target="_blank" rel="noopener noreferrer" { "Open console" }
                            } @else {
                                a class="button primary" href="#enroll-form" { "Enroll this machine" }
                            }
                            a class="button" href="#terminal" { "Open terminal" }
                        }
                    }

                    div class="local-task-layout" {
                    article class="local-primary" aria-label="Machine tasks" {
                    div class="panel-group" aria-label="Connection and use" {
                        p class="group-label" { "Connection & use" }
                        div class="connection-grid" {
                            div id="console-settings" {
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
                                        details class="secondary" open[outcome.is_some()] {
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

                            }
                            div id="mcp" {
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

                            }
                            div id="terminal" {
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
                                        div class="term-new" role="group" aria-label="New terminal session" {
                                            button id="term-pty" type="button" { "New pty" }
                                            div class="term-serial-fields" role="group" aria-label="New serial session" {
                                                div class="term-field" {
                                                    label for="term-port" { "Serial port" }
                                                    input id="term-port" type="text" placeholder="COM3" value="COM3" aria-label="Serial port" title="Serial port";
                                                }
                                                div class="term-field" {
                                                    label for="term-baud" { "Baud rate" }
                                                    input id="term-baud" type="number" placeholder="115200" value="115200" aria-label="Baud rate" title="Baud rate";
                                                }
                                                button id="term-serial" type="button" { "New serial" }
                                            }
                                        }
                                        button id="term-close" class="term-close" type="button" { "Close" }
                                    }
                                    div id="term-host" class="term-host" {}
                                    p class="hint" id="term-status" role="status" {
                                        "Open a pty or serial session with the buttons above. This page automatically attaches to the first existing session."
                                    }
                                    // **ONE CARD ABOUT THE TERMINAL, NOT TWO.** The list used to sit in a second
                                    // card whose own sentence said the interactive terminal was elsewhere — a
                                    // denial printed directly beneath the terminal. The count is in the head now
                                    // and the live rows are here, where the sessions are opened.
                                    div id="terminal-sessions" class="term-sessions hint" {
                                        @if facts.sessions.is_empty() {
                                            "No live session on this machine."
                                        } @else {
                                            @for session in &facts.sessions {
                                                (session_card(session))
                                            }
                                        }
                                    }
                                    }
                                ))

                            }
                        }
                    }
                    }
                    aside class="local-support" aria-label="Machine details and history" {
                    div class="panel-group" id="maintenance" aria-label="Machine maintenance" {
                        p class="group-label" { "Machine maintenance" }
                        div class="maintenance-grid" {
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


                        }
                    }
                    div class="panel-group" id="history" aria-label="History" {
                        p class="group-label" { "History & diagnostics" }
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
                                    "The calls this machine recorded, newest first, in this machine's local time. A transaction group's counts are the device's totals for that transaction; the rows beneath are the rows in this window."
                                }
                                @if unknown_rows > 0 {
                                    p class="hint" id="timeline-unknown" data-unknown=(unknown_rows) {
                                        (unknown_rows) " "
                                        @if unknown_rows == 1 { "row" } @else { "rows" }
                                        " this build cannot classify are drawn as unknown and counted, never dropped."
                                    }
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
                                        (timeline_day(day, &facts.txns))
                                    }
                                    @if fold.rest_rows > 0 {
                                        details class="timeline-rest" {
                                            summary {
                                                "Show the other " (fold.rest_rows) " "
                                                @if fold.rest_rows == 1 { "call" } @else { "calls" }
                                            }
                                            div class="timeline-rest-body" {
                                                @for day in &fold.rest {
                                                    (timeline_day(day, &facts.txns))
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

                    }
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
                }
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
    /// The transaction the call rode, from the row's own `txn` field (never from its arguments).
    txn: Option<String>,
    /// The row's class from `RowKind::of_row`: a row this build cannot classify is marked unknown,
    /// drawn and counted, never dropped.
    unknown: bool,
}

/// One local day of calls, newest first.
#[derive(Debug)]
pub(crate) struct TimelineDay {
    label: String,
    rows: Vec<TimelineRow>,
}

/// The audit's calls: one `begin` line and, when the window carries it, the `end` that closes it.
///
/// The audit writes two lines per call, `begin` and `end`, under one `id`. A call whose `end` is
/// outside the bounded window has no outcome; a call whose `begin` was outside it is not a call at
/// all and is not returned here. The panel's Timeline and the transcript both pair this way.
pub(crate) fn call_pairs(
    audit: &[serde_json::Value],
) -> Vec<(&serde_json::Value, Option<&serde_json::Value>)> {
    use std::collections::HashMap;

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

    calls
}

/// The calls the audit records, grouped by this machine's local day and newest first.
///
/// The audit writes two lines per call, `begin` and `end`, under one `id`; the panel shows one row
/// per call, with the `end` line's outcome and error. An `end` whose `begin` is outside the bounded
/// window carries no tool, so it cannot become a row: that is the one case the pairing drops. A
/// `begin` whose `end` has not been written yet keeps the decision as its status.
pub(crate) fn timeline_days(audit: &[serde_json::Value], now_ms: u64) -> Vec<TimelineDay> {
    use std::collections::BTreeMap;

    let calls = call_pairs(audit);

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

/// One local day of the Timeline: its calls grouped by the transaction they rode.
///
/// The day grouping is the reader's local calendar; inside it, a transaction is the paragraph.
/// The rows keep the record's newest-first order, and a receipt (or the honest absence of one) sits
/// after the calls of its group. A row with no `txn` field is not folded into a transaction it never
/// named: it is a group of its own that says so.
fn timeline_day(day: &TimelineDay, txns: &[TxnFact]) -> Markup {
    html! {
        section class="timeline-day" {
            h2 class="timeline-day-label" { (day.label) }
            @for (txn, rows) in timeline_groups(&day.rows) {
                section class="timeline-txn" data-txn=(txn) {
                    @if txn.is_empty() {
                        p class="timeline-txn-head" {
                            span class="timeline-txn-purpose" { "Rows with no transaction" }
                        }
                        p class="hint" {
                            "This call rode no transaction; no receipt is owed for it."
                        }
                    } @else if let Some(fact) = txn_fact(txns, txn) {
                        p class="timeline-txn-head" {
                            span class="timeline-txn-purpose" {
                                @if fact.purpose.is_empty() {
                                    "(purpose not retained)"
                                } @else {
                                    (fact.purpose)
                                }
                            }
                            @if !fact.author.is_empty() {
                                span class="timeline-txn-by" { (fact.author) }
                            }
                            @if fact.retained {
                                span class="timeline-txn-declared" {
                                    span class="receipt-label" { "Declared" }
                                    span class="chip" { (fact.state) }
                                }
                                span class="timeline-txn-observed" {
                                    span class="receipt-label" { "Observed" }
                                    span class="timeline-txn-receipt" {
                                        (fact.calls) " calls · "
                                        (count_noun(fact.failures, "failure")) " · "
                                        (fact.refusals) " refused · "
                                        (crate::activity::duration_text(fact.duration_ms))
                                    }
                                }
                            } @else {
                                span class="timeline-txn-receipt" { "receipt not retained" }
                            }
                        }
                        @if !fact.retained {
                            p class="hint" {
                                "The audit still records this transaction's calls, but this process holds no receipt for it."
                            }
                        }
                        @if let Some(sentence) = crate::activity::drift(&fact.reading()) {
                            // The same sentence `/activity` draws, from the same rule: the declared
                            // ending against the calls the device observed.
                            p class="receipt-drift" {
                                span class="receipt-drift-label" { "drift" }
                                " " (sentence)
                            }
                        }
                    } @else {
                        p class="timeline-txn-head" {
                            span class="timeline-txn-purpose" { "transaction " (txn) }
                            span class="timeline-txn-receipt" { "receipt not retained" }
                        }
                        p class="hint" {
                            "The audit still records this transaction's calls, but this process holds no receipt for it."
                        }
                    }
                    ol class="timeline-rows" {
                        @for row in rows {
                            li
                                class="timeline-row"
                                data-kind=(if row.unknown { "unknown" } else { "fact" })
                                data-status=(row.status)
                            {
                                time
                                    class="timeline-time"
                                    datetime=(crate::store::format_utc(row.at_ms / 1_000))
                                    title=(crate::store::format_utc(row.at_ms / 1_000))
                                { (row.time) }
                                span class="timeline-tool" { (row.tool) }
                                @if row.failed {
                                    span class="chip fail" { (row.status) }
                                } @else {
                                    span class="chip ok" { (row.status) }
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
    }
}

/// The day's rows in transaction groups, newest group first: the order the rows already hold.
fn timeline_groups<'a>(rows: &'a [TimelineRow]) -> Vec<(&'a str, Vec<&'a TimelineRow>)> {
    let mut groups: Vec<(&'a str, Vec<&'a TimelineRow>)> = Vec::new();
    for row in rows {
        let key = row.txn.as_deref().unwrap_or("");
        match groups.iter_mut().find(|(seen, _)| *seen == key) {
            Some((_, members)) => members.push(row),
            None => groups.push((key, vec![row])),
        }
    }
    groups
}

/// The fact for one transaction, by identity. `retained` decides which head is drawn: a fact the
/// process does not hold shows its claim (when the tail kept it) and "receipt not retained", never
/// the zero counters it carries.
fn txn_fact<'a>(txns: &'a [TxnFact], id: &str) -> Option<&'a TxnFact> {
    if id.is_empty() {
        return None;
    }
    txns.iter().find(|txn| txn.id == id)
}

/// `1 failure`, `2 failures`.
fn count_noun(count: u64, noun: &str) -> String {
    if count == 1 {
        format!("{count} {noun}")
    } else {
        format!("{count} {noun}s")
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
        txn: begin
            .get("txn")
            .or_else(|| end.and_then(|end| end.get("txn")))
            .and_then(serde_json::Value::as_str)
            .filter(|txn| !txn.is_empty())
            .map(str::to_owned),
        unknown: {
            let args = begin
                .get("args")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let action = args
                .split_whitespace()
                .find_map(|part| part.strip_prefix("action="));
            let event = begin
                .get("event")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("begin");
            summrise_core::RowKind::of_row(tool, action, event) == summrise_core::RowKind::Unknown
        },
    })
}

/// The instant in this machine's timezone, or `None` when it is not a representable local time.
pub(crate) fn local_at(milliseconds: u64) -> Option<chrono::DateTime<Local>> {
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
                holder: "the panel".to_owned(),
                goal: None,
                goal_by: None,
                open_txn: None,
                open_purpose: None,
                last_call: None,
                last_call_at_ms: None,
                alive: true,
            }],
            txns: Vec::new(),
            panel_token: "a".repeat(64),
        }
    }

    #[test]
    fn a3_local_workspace_keeps_navigation_tasks_and_support_reachable() {
        let html = page(&facts()).into_string();
        let body = html.split_once("<body").unwrap().1;
        let navigation = body
            .split_once("aria-label=\"Local navigation\"")
            .expect("local navigation landmark")
            .1
            .split_once("</nav>")
            .unwrap()
            .0;
        assert!(navigation.contains("href=\"/panel\" aria-current=\"page\""));
        assert!(navigation.contains("href=\"/activity\""));
        assert!(!navigation.contains(&facts().panel_token));
        let primary = body
            .split_once("aria-label=\"Machine tasks\"")
            .expect("main task region")
            .1
            .split_once("</article>")
            .unwrap()
            .0;
        assert!(primary.contains("id=\"terminal\""));
        assert!(primary.contains("id=\"console-settings\""));
        assert!(primary.contains("id=\"mcp\""));
        let support = body
            .split_once("aria-label=\"Machine details and history\"")
            .expect("supporting landmark")
            .1
            .split_once("</aside>")
            .unwrap()
            .0;
        assert!(support.contains("id=\"maintenance\""));
        assert!(support.contains("id=\"history\""));
        assert!(support.contains("id=\"state\""));
        assert!(support.contains("id=\"update-now\""));
        assert_eq!(body.matches("<main ").count(), 1);
        assert_eq!(body.matches("<h1").count(), 1);
    }

    #[test]
    fn console_action_opens_external_destination_without_retargeting_local_actions() {
        let linked = facts();
        let html = page(&linked).into_string();
        let anchors: Vec<_> = html
            .split("<a ")
            .skip(1)
            .map(|link| link.split_once("</a>").unwrap().0)
            .collect();
        let console = anchors
            .iter()
            .find(|link| link.ends_with(">Open console"))
            .expect("linked machine has a console action");
        assert!(console.contains("href=\"https://console.example.test\""));
        assert!(
            console.contains("target=\"_blank\""),
            "external console must use the existing new-window navigation door: {console}"
        );
        assert!(console.contains("rel=\"noopener noreferrer\""));
        assert!(!console.contains(&linked.panel_token));
        for link in anchors
            .iter()
            .filter(|link| link.contains("href=\"#") || link.contains("href=\"/"))
        {
            assert!(
                !link.contains("target="),
                "local navigation stays in this page: {link}"
            );
        }
        assert!(anchors
            .iter()
            .any(|link| link.contains("href=\"#terminal\"")));

        let mut unlinked = linked;
        unlinked.console_url = None;
        let html = page(&unlinked).into_string();
        assert!(!html.contains(">Open console</a>"));
        let enroll = html
            .split("<a ")
            .skip(1)
            .map(|link| link.split_once("</a>").unwrap().0)
            .find(|link| link.ends_with(">Enroll this machine"))
            .expect("unlinked machine keeps local enrollment entry");
        assert!(enroll.contains("href=\"#enroll-form\""));
        assert!(!enroll.contains("target="));
    }

    // Removing the skip destination, serial labels, protected actions or visible primary enrollment
    // would leave keyboard operators unable to find or safely use the same machine controls.
    #[test]
    fn panel_navigation_and_forms_preserve_operator_actions() {
        let mut first = facts();
        first.console_url = None;
        first.console_attempt = None;
        let html = page(&first).into_string();
        let body = html.split("<body").nth(1).unwrap();
        let first_link = body
            .split("<a ")
            .nth(1)
            .unwrap()
            .split("</a>")
            .next()
            .unwrap();
        assert!(
            first_link.contains("href=\"#content\""),
            "skip is the first link"
        );
        assert!(html.contains("href=\"/panel\" aria-current=\"page\""));
        assert!(html.contains("href=\"/activity\""));
        assert_eq!(html.matches("<h1").count(), 1);
        assert!(html.contains("<main id=\"content\" tabindex=\"-1\">"));
        assert!(html.contains("<label for=\"term-port\">Serial port</label>"));
        assert!(html.contains("<label for=\"term-baud\">Baud rate</label>"));
        assert!(
            html.contains("class=\"button primary\" href=\"#enroll-form\">Enroll this machine</a>")
        );
        for (action, fields) in [
            (
                "/panel/enroll",
                vec![
                    "panel_token",
                    "console_url",
                    "code",
                    "force",
                    "allow_plaintext",
                ],
            ),
            ("/api/panel/update", vec!["panel_token"]),
        ] {
            let form = html
                .split("<form ")
                .skip(1)
                .find(|form| form.contains(&format!("action=\"{action}\"")))
                .unwrap()
                .split("</form>")
                .next()
                .unwrap();
            for field in fields {
                assert!(
                    form.contains(&format!("name=\"{field}\"")),
                    "{action}: {field}"
                );
            }
            assert!(form.contains(&format!("value=\"{}\"", first.panel_token)));
        }
        assert!(html.contains("id=\"copy-status\" role=\"status\""));
        let connected = page(&facts()).into_string();
        assert!(connected.contains(
            "class=\"button primary\" href=\"https://console.example.test\" target=\"_blank\" rel=\"noopener noreferrer\">Open console</a>"
        ));
        let sections: Vec<_> = [
            "id=\"console-settings\"",
            "id=\"mcp\"",
            "id=\"terminal\"",
            "id=\"maintenance\"",
            "id=\"history\"",
        ]
        .iter()
        .map(|id| html.find(id).expect(id))
        .collect();
        assert!(sections.windows(2).all(|pair| pair[0] < pair[1]));
    }

    /// **THE PANEL IS ON THE SAME TYPE SCALE, BY NAME.** Eighteen declarations resolved to seven
    /// sizes no reader could rank, because the panel used `rem` while the console used `px` and no
    /// gate read either. The first fix pinned the pixels; this one pins the shared steps themselves
    /// (`--fs-2xs` … `--fs-xl`), so the two surfaces cannot drift apart again — a raw pixel value is
    /// now the failure, not a value outside a list.
    #[test]
    fn the_panel_uses_the_shared_type_scale() {
        const ALLOWED: [&str; 9] = [
            "--fs-2xs",
            "--fs-xs",
            "--fs-sm",
            "--fs-base",
            "--fs-md",
            "--fs-lg",
            "--fs-xl",
            "--fs-goal",
            "--fs-purpose",
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
            "the submit button must be the measured pair: --accent is a foreground, not a fill"
        );
        assert!(
            !css.contains("background: var(--accent); color: #fff"),
            "a fill drawn in the accent's foreground value is not the measured pair"
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

    /// One rule by its exact opening selector, out of the panel's own sheet.
    fn rule<'a>(css: &'a str, selector: &str) -> &'a str {
        let needle = format!("{selector} {{");
        let at = css
            .find(&needle)
            .unwrap_or_else(|| panic!("the sheet styles {selector}: {css}"));
        let open = at + needle.len() - 1;
        let close = css[open..].find('}').expect("the rule closes") + open;
        &css[at..=close]
    }

    /// The forced-colors fallback block, by its media query.
    fn forced_colors(css: &str) -> &str {
        let at = css
            .find("@media (forced-colors: active)")
            .expect("the sheet has a forced-colors fallback");
        let open = css[at..].find('{').expect("the media opens") + at;
        let close = css[open..].rfind('}').expect("the media closes") + open;
        &css[at..=close]
    }

    /// **THE PANEL'S FOCUS RING IS THE SAME VISIBLE ACCENT OUTLINE, NOT A 1.37:1 HALO.** The shipped
    /// 2 px `--accent` outline measures 4.84:1 on the paper; a forced-colors session, which drops
    /// shadows and sometimes the author's colours, keeps a `CanvasText` ring.
    #[test]
    fn the_panel_focus_ring_is_the_same_outline() {
        let focus = rule(COMPONENTS, ":focus-visible");
        assert!(
            focus.contains("outline: 2px solid var(--accent)")
                && focus.contains("outline-offset: 2px"),
            "the panel focuses with the accent outline: {focus}"
        );
        assert!(
            !focus.contains("outline: none"),
            "the panel does not hide the outline behind a halo: {focus}"
        );
        let forced = forced_colors(COMPONENTS);
        assert!(
            forced.contains("outline: 2px solid CanvasText"),
            "a forced-colors session keeps a visible ring: {forced}"
        );
    }

    /// **THE GOAL SENTENCE IS THE DOCUMENT'S, AND THE TIMELINE'S PURPOSE WITH IT (the light
    /// contract).** A computed face is not a question a string can answer, so the named rules in the
    /// panel's own sheet are read: the sentence is serif, while its author badge and "set by" fact
    /// stay in the UI face.
    #[test]
    fn the_panel_goal_is_set_in_the_document_face() {
        let mut facts = facts();
        facts.sessions[0].goal = Some("ship the installer".to_owned());
        facts.sessions[0].goal_by = Some("token srm_7f".to_owned());
        let html = page(&facts).into_string();
        assert!(
            html.contains(r#"<p class="session-goal">"#),
            "the card draws the goal line: {html}"
        );
        assert!(
            html.contains("ship the installer"),
            "the claim itself is the line: {html}"
        );
        let goal = rule(COMPONENTS, ".session-goal");
        assert!(
            goal.contains("font-family: var(--font-serif)"),
            "the goal sentence is the document face: {goal}"
        );
        let by = rule(COMPONENTS, ".session-goal-by");
        assert!(
            !by.contains("font-serif"),
            "the goal's author is a fact and stays in the UI face: {by}"
        );
        let purpose = rule(COMPONENTS, ".timeline-txn-purpose");
        assert!(
            purpose.contains("font-family: var(--font-serif)"),
            "a transaction's purpose is the same document line: {purpose}"
        );
        let receipt = rule(COMPONENTS, ".timeline-txn-receipt");
        assert!(
            !receipt.contains("font-serif"),
            "the receipt's counts and duration are facts, not the document face: {receipt}"
        );
    }

    /// **THE TERMINAL'S CANVAS IS THE SHEET, EVEN WITHOUT THE SHEET.** The page reads `--term-canvas`,
    /// but a page rendered on its own must still be the device surface: the script's hard-coded fallbacks are the
    /// light palette's values, not the previous warm palette.
    #[test]
    fn the_panel_terminal_canvas_is_the_sheet() {
        let js = include_str!("../assets/panel-terminal.js");
        assert!(
            js.contains(r##"token("--term-canvas", token("--surface", "#ffffff"))"##),
            "the canvas falls back to the sheet: {js}"
        );
        assert!(
            !js.contains("#fbf9f4"),
            "the terminal's fallback canvas is not the previous warm paper: {js}"
        );
        for (token, value) in [
            ("--ink", "#22312c"),
            ("--accent", "#335f51"),
            ("--term-selection", "rgba(51,95,81,.18)"),
        ] {
            let read = format!(r#"token("{token}", "{value}")"#);
            assert!(
                js.contains(&read),
                "the {token} fallback is the the device surface value: {js}"
            );
        }
        assert!(
            js.contains("getComputedStyle(document.documentElement)"),
            "the theme still reads the page's tokens: {js}"
        );
    }

    /// Write the panel for a screenshot pass, the way the console's own review renderer does.
    ///
    /// Ignored by default — it writes files and exists to be looked at:
    ///   SUMMRISE_PANEL_DIR=/tmp/panel cargo test -p summrise-device --bin summrise-device \
    ///       -- --ignored render_panel_for_review
    #[test]
    #[ignore = "writes review pages; the CI step \"the device panel fits the window\" runs it by name with --ignored"]
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
            html.contains(r#"<header class="topbar local-topbar">"#)
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
        assert!(html.contains(r#"<span class="chip fail">failed</span>"#));
        assert!(
            html.contains(r#"<span class="timeline-error">refused</span>"#),
            "the panel names the audit's refusal"
        );
        assert!(html.contains(r#"<span class="timeline-caller">console</span>"#));
    }

    #[test]
    fn the_panel_has_no_theme_control() {
        let html = page(&facts()).into_string();
        assert!(
            !html.contains(r#"id="theme""#),
            "the panel offers no theme toggle"
        );
        assert!(
            !html.contains("summrise-theme"),
            "the panel stores no theme choice"
        );
        assert!(
            !html.contains("data-theme"),
            "the panel declares no theme attribute"
        );
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
            html.matches(r#"<li class="timeline-row""#).count(),
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
            visible.matches(r#"<li class="timeline-row""#).count(),
            TIMELINE_VISIBLE_ROWS,
            "the newest 20 rows stay out of the fold"
        );
        assert_eq!(
            details.matches(r#"<li class="timeline-row""#).count(),
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
            html.matches(r#"<li class="timeline-row""#).count(),
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
                "Console",
                "Connect a client",
                "Terminal",
                "Build",
                "Updates",
                "Timeline",
                "State",
                "Recent events",
            ],
            "the panel's reading order: the machine, its facts, its record, its tools"
        );
        let first_row = html
            .find(r#"<li class="timeline-row""#)
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
        assert!(
            enrolled.contains(r#"<details class="secondary" open>"#),
            "the linked console's outcome is visible after the redirect"
        );

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

    /// The local navigation and the skip target stay available in the same workspace.
    #[test]
    fn the_page_frame_keeps_one_main_and_reachable_local_navigation() {
        let html = page(&facts()).into_string();
        let body = html.split_once("<body").unwrap().1;
        let skip = body.find("href=\"#content\"").expect("skip target");
        let sidebar = body
            .find("class=\"local-sidebar\"")
            .expect("desktop navigation");
        let menu = body
            .find("class=\"local-mobile-menu\"")
            .expect("phone disclosure");
        let main = body
            .find("<main id=\"content\" tabindex=\"-1\">")
            .expect("focusable main");
        assert!(skip < sidebar && sidebar < menu && menu < main);
        assert_eq!(body.matches("<main ").count(), 1);
        assert!(body[sidebar..main].contains("<summary>Summrise · Menu</summary>"));
        assert!(body[sidebar..main].contains("href=\"/activity\""));
    }

    #[test]
    fn the_session_card_names_holder_goal_and_open_transaction() {
        let mut facts = facts();
        facts.sessions = vec![SessionFacts {
            kind: "pty".to_owned(),
            shell: "bash".to_owned(),
            id: "session-1".to_owned(),
            holder: "token srm_7f".to_owned(),
            goal: Some("ship the installer".to_owned()),
            goal_by: Some("token srm_7f".to_owned()),
            open_txn: Some("txn-9".to_owned()),
            open_purpose: Some("verify the suite".to_owned()),
            last_call: Some("terminal.write".to_owned()),
            last_call_at_ms: Some(1_761_566_403_000),
            alive: true,
        }];
        facts.txns = vec![TxnFact {
            id: "txn-9".to_owned(),
            purpose: "verify the suite".to_owned(),
            author: "token srm_7f".to_owned(),
            session: Some("session-1".to_owned()),
            state: "working".to_owned(),
            reason: None,
            started_at_ms: 1_761_566_402_000,
            ended_at_ms: None,
            calls: 3,
            failures: 0,
            refusals: 0,
            duration_ms: 0,
            retained: true,
        }];
        let html = page(&facts).into_string();
        let start = html
            .find(r#"class="session-card""#)
            .expect("the live session is a card");
        let end = html[start..]
            .find("</article>")
            .map(|offset| start + offset)
            .expect("the card closes");
        let card = &html[start..end];

        assert!(
            card.contains("session-1") && card.contains("token srm_7f"),
            "the card names the session and its holder: {card}"
        );
        assert!(
            card.contains("ship the installer") && card.contains("CLAIM"),
            "the goal is a badged claim, never an instruction: {card}"
        );
        assert!(
            card.contains("txn-9") && card.contains("verify the suite"),
            "the card names the open transaction and its purpose: {card}"
        );
        assert!(
            card.contains("terminal.write"),
            "the card names the last call: {card}"
        );
    }

    #[test]
    fn the_timeline_groups_calls_by_transaction_with_the_receipt() {
        let mut facts = facts();
        let at = 1_761_566_400_000u64;
        let mut begin = begin(1, at, "files.read", "loopback");
        begin["txn"] = json!("txn-1");
        let mut end = end(1, at + 1_000, true, None, "loopback");
        end["txn"] = json!("txn-1");
        facts.audit = Some(vec![begin, end]);
        facts.txns = vec![TxnFact {
            id: "txn-1".to_owned(),
            purpose: "read the installer's config".to_owned(),
            author: "install".to_owned(),
            session: None,
            state: "committed".to_owned(),
            reason: None,
            started_at_ms: at,
            ended_at_ms: Some(at + 1_000),
            calls: 7,
            failures: 1,
            refusals: 0,
            duration_ms: 1_000,
            retained: true,
        }];

        let html = page(&facts).into_string();
        let group = html
            .find(r#"class="timeline-txn""#)
            .expect("the timeline groups calls by transaction");
        let group = &html[group..];
        assert!(
            group.contains("read the installer's config"),
            "the group names the purpose: {group}"
        );
        assert!(
            group.contains("7 calls") && group.contains("1 failure"),
            "the receipt shows the device's totals, not the window's: {group}"
        );
        // The receipt's two parts are labelled with the same two words `/activity` prints, and
        // the declared word keeps its chip.
        let declared = group
            .find(r#"<span class="receipt-label">Declared</span>"#)
            .unwrap_or_else(|| panic!("the declaration is labelled: {group}"));
        let observed = group
            .find(r#"<span class="receipt-label">Observed</span>"#)
            .unwrap_or_else(|| panic!("the observation is labelled: {group}"));
        assert!(
            declared < observed,
            "the receipt reads Declared then Observed: {group}"
        );
        assert!(
            group.contains(r#"<span class="chip">committed</span>"#),
            "the declared word keeps its chip: {group}"
        );
    }

    /// The production shape of an orphan: `http::txn_facts` enumerates the audit's ids and marks a
    /// registry-less transaction `retained: false` with no counts. The page must not print those
    /// totals beside real rows; it says the receipt is not retained.
    #[test]
    fn an_orphan_timeline_row_says_receipt_not_retained() {
        let mut facts = facts();
        let at = 1_761_566_400_000u64;
        let mut begin = begin(1, at, "files.read", "loopback");
        begin["txn"] = json!("txn-gone");
        let mut end = end(1, at + 1_000, true, None, "loopback");
        end["txn"] = json!("txn-gone");
        facts.audit = Some(vec![begin, end]);
        // The production shape: the tail still holds the `txn.begin` row, so the claim survives
        // while the receipt does not.
        facts.txns = vec![TxnFact {
            id: "txn-gone".to_owned(),
            purpose: "read the installer's config".to_owned(),
            author: "the panel".to_owned(),
            session: None,
            state: "unknown".to_owned(),
            reason: None,
            started_at_ms: at,
            ended_at_ms: None,
            calls: 0,
            failures: 0,
            refusals: 0,
            duration_ms: 0,
            retained: false,
        }];

        let html = page(&facts).into_string();
        assert!(
            html.contains("read the installer's config") && html.contains("the panel"),
            "the claim from the audit keeps the group's head: {html}"
        );
        assert!(
            html.contains("receipt not retained"),
            "an orphan group is honest about its missing receipt: {html}"
        );
        assert!(
            !html.contains("0 calls · 0 failures · 0 refused"),
            "a receipt the process does not hold prints no totals: {html}"
        );
    }

    /// The day's rows carry the ledger's `data-kind`, so a row this build cannot classify is marked
    /// unknown; the Timeline counts them instead of showing them as facts.
    #[test]
    fn the_panel_timeline_marks_and_counts_an_unknown_row() {
        let mut facts = facts();
        let at = 1_761_566_402_500u64;
        facts.audit = Some(vec![
            begin(1, at, "mystery.tool", "loopback"),
            end(1, at + 100, true, None, "loopback"),
        ]);

        let html = page(&facts).into_string();

        assert!(
            html.contains(r#"<li class="timeline-row" data-kind="unknown" data-status="ok">"#),
            "an unclassifiable row is drawn as unknown: {html}"
        );
        assert!(
            html.contains(r#"data-unknown="1""#),
            "the Timeline counts the unknown row: {html}"
        );
    }

    /// The group head carries the receipt's own vocabulary: the claim's author, its duration, and
    /// the note that the counts are the device's totals while the rows are this window's.
    #[test]
    fn the_timeline_group_names_the_author_and_duration_and_scopes_the_counts() {
        let mut facts = facts();
        let at = 1_761_566_400_000u64;
        let mut begin = begin(1, at, "files.read", "loopback");
        begin["txn"] = json!("txn-1");
        let mut end = end(1, at + 1_000, true, None, "loopback");
        end["txn"] = json!("txn-1");
        facts.audit = Some(vec![begin, end]);
        facts.txns = vec![TxnFact {
            id: "txn-1".to_owned(),
            purpose: "read the installer's config".to_owned(),
            author: "the install door".to_owned(),
            session: None,
            state: "committed".to_owned(),
            reason: None,
            started_at_ms: at,
            ended_at_ms: Some(at + 1_000),
            calls: 7,
            failures: 1,
            refusals: 0,
            duration_ms: 1_000,
            retained: true,
        }];

        let html = page(&facts).into_string();
        let group = html
            .find(r#"class="timeline-txn""#)
            .expect("the timeline groups calls by transaction");
        let group = &html[group..];

        assert!(
            group.contains("the install door"),
            "the group names the claim's author: {group}"
        );
        assert!(
            group.contains("7 calls · 1 failure") && !group.contains("1 1 failure"),
            "the receipt states its failures once, beside the calls: {group}"
        );
        assert!(
            group.contains("1.0s"),
            "the receipt carries its duration beside the counts: {group}"
        );
        assert!(
            html.contains("the device's totals") && html.contains("the rows beneath"),
            "the page says the counts are the device's totals and the rows are this window's: {html}"
        );
    }

    /// The receipt's drift element, when the page drew one.
    fn receipt_drift(html: &str) -> Option<String> {
        let open = r#"<p class="receipt-drift">"#;
        let start = html.find(open)?;
        let close = html[start..].find("</p>")? + start + "</p>".len();
        Some(html[start..close].to_owned())
    }

    /// **ONE RULE, TWO PAGES (the light contract).** The panel's Timeline receipt draws the same
    /// drift sentence `/activity` draws, from the same function: the declared ending against the
    /// device's own counts.
    #[test]
    fn the_timeline_receipt_states_the_same_drift_as_the_ledger() {
        let mut facts = facts();
        let at = 1_761_566_400_000u64;
        let mut begin = begin(1, at, "files.read", "loopback");
        begin["txn"] = json!("txn-1");
        let mut end = end(1, at + 1_000, true, None, "loopback");
        end["txn"] = json!("txn-1");
        facts.audit = Some(vec![begin, end]);
        facts.txns = vec![TxnFact {
            id: "txn-1".to_owned(),
            purpose: "publish the release".to_owned(),
            author: "the install door".to_owned(),
            session: None,
            state: "committed".to_owned(),
            reason: None,
            started_at_ms: at,
            ended_at_ms: Some(at + 1_000),
            calls: 5,
            failures: 1,
            refusals: 0,
            duration_ms: 1_000,
            retained: true,
        }];

        let expected =
            crate::activity::drift(&facts.txns[0].reading()).expect("the fixture's receipt drifts");
        assert_eq!(expected, "Committed over 1 failed call.");

        let html = page(&facts).into_string();
        let line = receipt_drift(&html).expect("the panel's receipt draws the hint");
        assert!(
            line.contains(r#"<span class="receipt-drift-label">drift</span>"#),
            "the element carries the visible drift label: {line}"
        );
        assert!(
            line.contains(&expected),
            "the panel draws /activity's exact sentence: {line}"
        );

        // A clean receipt draws none, on either page.
        facts.txns[0].failures = 0;
        let clean = page(&facts).into_string();
        assert!(
            receipt_drift(&clean).is_none(),
            "a clean receipt draws no hint: {clean}"
        );
    }
}
