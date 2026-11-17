//! The operations transcript, rendered by the device at `/activity`.
//!
//! The page is the old Electron transcript moved into Rust: one row per audited call, oldest first,
//! split into turns where the audit went quiet for more than eight seconds, with the tool groups,
//! counts and the light/dark theme computed here. The first paint is server-rendered from the panel's
//! bounded audit tail; [`VIEW_SCRIPT`] only filters, follows, applies the stored theme and expands a
//! run through `/api/runs/:id`. It groups and counts nothing: every row carries the group, status,
//! turn and label this module decided.
//!
//! The audit writes two lines per call, `begin` and `end`, under one `id`. An `end` whose `begin` is
//! outside the bounded window is still a fact — the call ended — so it becomes an `unknown` row with
//! the outcome the line wrote, exactly as the old transcript's addition of a lone `end` did. A
//! `begin` whose `end` has not arrived is a running row; the page does not invent an outcome for it.

use maud::{html, Markup, PreEscaped, DOCTYPE};
use serde::Serialize;
use serde_json::Value;

use crate::panel;

/// The break between two turns, in milliseconds: the old transcript's `8000`, unchanged.
const TURN_BREAK_MS: u64 = 8_000;

/// The newest audit lines the transcript reads: the panel's bound, so the two surfaces of the same
/// machine cannot disagree about how much of the record they show.
const AUDIT_LINES: usize = panel::TIMELINE_LINES;

/// The largest new-begin page size (or newest-window size without a cursor). The audit window is
/// 200 lines, so one answer still carries at most that many rows; a query cannot ask for an
/// unbounded document.
pub(crate) const MAX_ROWS: usize = 200;

/// The page size a request without a `limit` gets. The first paint asks for [`MAX_ROWS`] because it
/// draws the whole bounded tail; the route's default is the smaller amount a poll actually needs.
pub(crate) const DEFAULT_LIMIT: usize = 50;

/// The transcript's stylesheet, rendered through [`summrise_ui::style`] after the shared tokens.
pub(crate) const STYLES: &str = r##"  * { box-sizing: border-box; }
  body.activity-page {
    margin: 0; padding: 0; background: var(--bg); color: var(--ink);
    font-family: var(--font); font-size: var(--fs-md); line-height: 1.55;
    font-variant-numeric: tabular-nums; -webkit-font-smoothing: antialiased;
  }
  .activity-head {
    position: sticky; top: 0; z-index: 5; display: flex; align-items: center; flex-wrap: wrap;
    gap: var(--sp-2); padding: var(--sp-2) max(var(--sp-4), calc((100% - 70rem) / 2 + var(--sp-4)));
    background: color-mix(in srgb, var(--surface) 88%, transparent);
    backdrop-filter: blur(10px); border-bottom: 1px solid var(--line);
  }
  .activity-brand { display: flex; align-items: baseline; gap: var(--sp-1-5); }
  .activity-brand h1 { margin: 0; font-size: var(--fs-lg); letter-spacing: var(--tracking-tight); }
  .activity-brand .stem { color: var(--ink-faint); font-size: var(--fs-base); font-weight: 500; }
  .activity-pill {
    display: inline-flex; align-items: center; gap: var(--sp-1-5); height: 24px; padding: 0 var(--sp-2);
    border: 1px solid var(--line); border-radius: var(--radius-pill); color: var(--muted);
    font-size: var(--fs-sm); background: var(--surface);
  }
  .activity-pill .dot {
    width: 7px; height: 7px; border-radius: 50%; background: var(--ok);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--ok) 16%, transparent);
  }
  .activity-head .spacer { flex: 1; }
  .activity-counts { display: flex; align-items: center; gap: var(--sp-2); color: var(--muted); font-size: var(--fs-sm); }
  .activity-window {
    padding: 0 var(--sp-1-5); height: 20px; display: inline-flex; align-items: center;
    border: 1px solid var(--line); border-radius: var(--radius-xs); color: var(--ink-faint); font-size: var(--fs-xs);
    letter-spacing: var(--tracking-label);
  }
  .activity-counts b { color: var(--ink); font-weight: 600; }
  .activity-count { color: var(--ink-muted); }
  .activity-filter {
    height: 26px; border: 1px solid var(--line); border-radius: var(--radius-xs);
    background: var(--surface); color: var(--ink); font: inherit; font-size: var(--fs-sm); padding: 0 var(--sp-1-5);
  }
  .activity-theme {
    width: 28px; height: 28px; display: grid; place-items: center; cursor: pointer;
    border: 1px solid var(--line); border-radius: var(--radius-xs); background: var(--surface); color: var(--ink);
  }
  .activity-main { flex: 1; padding: var(--sp-3) 0 var(--sp-6); }
  .activity-feed { max-width: 60rem; margin: 0 auto; padding: 0 var(--sp-4); }
  .activity-turn { margin: var(--sp-4) 0 var(--sp-1); }
  .activity-turn-label {
    display: flex; align-items: center; gap: var(--sp-2); margin: 0 0 var(--sp-1-5);
    color: var(--ink-faint); font-size: var(--fs-xs); font-weight: 600;
    letter-spacing: var(--tracking-label); text-transform: uppercase;
  }
  .activity-turn-label::after { content: ""; flex: 1; height: 1px; background: var(--line); }
  .activity-row {
    display: grid; gap: var(--sp-0-5); padding: var(--sp-1-5) var(--sp-2);
    border: 1px solid transparent; border-radius: var(--radius-xs);
  }
  .activity-row:hover { background: var(--panel-2); border-color: var(--line); }
  .activity-line { display: flex; align-items: baseline; gap: var(--sp-2); min-width: 0; white-space: nowrap; }
  .activity-glyph { flex: none; width: 1rem; text-align: center; color: var(--ink-faint); }
  .activity-glyph.status-ok { color: var(--ok); }
  .activity-glyph.status-failed { color: var(--error); }
  .activity-glyph.status-running { color: var(--warn); }
  .activity-tool { font-family: var(--font-mono); font-size: var(--fs-sm); font-weight: 500; }
  .activity-verb { color: var(--ink-muted); font-size: var(--fs-sm); }
  .activity-args {
    flex: 1 1 0; min-width: 0; overflow: hidden; text-overflow: ellipsis;
    color: var(--ink-faint); font-family: var(--font-mono); font-size: var(--fs-sm);
  }
  .activity-duration, .activity-time { color: var(--ink-faint); font-size: var(--fs-sm); font-variant-numeric: tabular-nums; }
  .activity-duration { margin-left: auto; }
  .activity-time { font-family: var(--font-mono); }
  .chip {
    display: inline-flex; align-items: center; flex: none; height: 19px; padding: 0 var(--sp-1-5);
    border-radius: var(--radius-xs); font-size: var(--fs-xs); font-weight: 500; white-space: nowrap;
  }
  .chip.status-ok { color: var(--ok); background: var(--ok-soft); }
  .chip.status-failed { color: var(--error); background: var(--error-soft); }
  .chip.status-running { color: var(--warn); background: var(--warn-soft); }
  .chip.decision-allow { color: var(--accent-on-soft); background: var(--accent-soft); }
  .chip.decision-deny { color: var(--error); background: var(--error-soft); }
  .chip.outcome { color: var(--muted); background: var(--panel-2); border: 1px solid var(--line); }
  .activity-detail { padding: 0 0 var(--sp-1) 1.5rem; }
  .activity-kv { display: grid; grid-template-columns: 7rem 1fr; gap: var(--sp-0-5) var(--sp-3); margin: var(--sp-1) 0; }
  .activity-kv dt { color: var(--ink-faint); font-size: var(--fs-sm); }
  .activity-kv dd { margin: 0; overflow-wrap: anywhere; font-family: var(--font-mono); font-size: var(--fs-sm); }
  details.activity-run { margin-top: var(--sp-1); }
  details.activity-run summary { cursor: pointer; color: var(--accent); font-size: var(--fs-sm); }
  .activity-preview-label { margin: var(--sp-2) 0 var(--sp-0-5); color: var(--ink-faint); font-size: var(--fs-xs); text-transform: uppercase; letter-spacing: var(--tracking-label); }
  .activity-preview {
    margin: 0; padding: var(--sp-2); white-space: pre-wrap; word-break: break-word;
    background: var(--surface-recessed); border: 1px solid var(--line); border-radius: var(--radius-xs);
    font-family: var(--font-mono); font-size: var(--fs-sm); color: var(--ink);
  }
  .activity-none { max-width: 60rem; margin: var(--sp-6) auto; padding: 0 var(--sp-4); color: var(--ink-faint); font-size: var(--fs-md); }
  .activity-none b { display: block; color: var(--ink-muted); font-weight: 600; font-size: var(--fs-lg); margin-bottom: var(--sp-1); }
  .activity-hint { max-width: 60rem; margin: var(--sp-2) auto 0; padding: 0 var(--sp-4); color: var(--ink-faint); font-size: var(--fs-sm); }
  .activity-foot {
    display: flex; align-items: center; gap: var(--sp-4); padding: var(--sp-1-5) max(var(--sp-4), calc((100% - 70rem) / 2 + var(--sp-4)));
    border-top: 1px solid var(--line); background: var(--surface); color: var(--ink-faint); font-size: var(--fs-sm);
  }
  kbd {
    border: 1px solid var(--line); border-bottom-width: 2px; border-radius: 5px; padding: 0 5px;
    font-family: var(--font-mono); font-size: var(--fs-xs); color: var(--ink-muted); background: var(--panel-2);
  }
  /* The work ledger: a machine group, then a group per session; the rail is the page's margin. */
  .terminal-link { color: var(--ink-muted); text-decoration: none; border-bottom: 1px solid var(--line-strong); padding-bottom: 1px; }
  .terminal-link:hover { color: var(--accent); border-color: var(--accent); }
  .ledger-layout {
    display: grid; grid-template-columns: 17rem minmax(0, 1fr); gap: var(--sp-5);
    max-width: 70rem; margin: 0 auto; padding: var(--sp-4) var(--sp-4) var(--sp-2);
  }
  .ledger { min-width: 0; }
  .ledger-group-head {
    display: flex; align-items: baseline; gap: var(--sp-2); margin: var(--sp-3) 0 var(--sp-1);
    font-size: var(--fs-xs); font-weight: 600; letter-spacing: var(--tracking-label);
    text-transform: uppercase; color: var(--ink-faint);
  }
  .ledger-group-head::after { content: ""; flex: 1; height: 1px; background: var(--line); }
  .ledger-group-note { color: var(--ink-faint); font-size: var(--fs-2xs); letter-spacing: var(--tracking-label); text-transform: none; }
  .ledger-entry { padding: var(--sp-2) 0; border-bottom: 1px solid var(--line); }
  .ledger-entry:first-of-type { border-top: 1px solid var(--line); }
  .ledger-line { display: flex; align-items: baseline; flex-wrap: wrap; gap: var(--sp-2); }
  .ledger-time { flex: none; width: 3.2rem; font-family: var(--font-mono); font-size: var(--fs-sm); color: var(--ink-faint); }
  .ledger-purpose { min-width: 0; font-size: var(--fs-md); font-weight: 500; overflow-wrap: anywhere; }
  .ledger-chip { flex: none; }
  .ledger-chip.state-working { color: var(--warn); background: var(--warn-soft); }
  .ledger-chip.state-committed { color: var(--ok); background: var(--ok-soft); }
  .ledger-chip.state-aborted { color: var(--error); background: var(--error-soft); }
  .ledger-chip.state-unknown { color: var(--muted); background: var(--panel-2); border: 1px solid var(--line); }
  .ledger-spark { display: inline-flex; gap: 3px; align-items: center; }
  .ledger-spark i { width: 5px; height: 5px; border-radius: 50%; background: var(--ok); display: block; }
  .ledger-spark i.bad { background: var(--error); }
  .ledger-spark i.work { background: var(--warn); }
  .ledger-meta { margin-left: auto; color: var(--ink-faint); font-family: var(--font-mono); font-size: var(--fs-xs); }
  .ledger-receipt-link { color: var(--ink-muted); text-decoration: none; border-bottom: 1px solid var(--line-strong); }
  .ledger-receipt-link:hover { color: var(--accent); border-color: var(--accent); }
  .ledger-spread {
    display: grid; grid-template-columns: 1fr 1.35fr 1fr; gap: var(--sp-4);
    margin: var(--sp-2) 0 var(--sp-1) 3.2rem; align-items: start;
  }
  .ledger-col-label {
    margin: 0 0 var(--sp-1); font-size: var(--fs-2xs); font-weight: 600;
    letter-spacing: var(--tracking-label); text-transform: uppercase; color: var(--ink-faint);
  }
  .said, .did, .left { min-width: 0; }
  .claim-text { margin: 0; font-size: var(--fs-md); line-height: 1.55; }
  .claim-by { margin: var(--sp-1) 0 0; color: var(--ink-muted); font-size: var(--fs-sm); }
  .claim-note { margin: var(--sp-1) 0 0; color: var(--ink-faint); font-size: var(--fs-xs); font-style: italic; }
  .claim-badge {
    display: inline-flex; align-items: center; height: 18px; padding: 0 var(--sp-1);
    border: 1px solid var(--accent); border-radius: var(--radius-xs);
    color: var(--accent); font-size: var(--fs-2xs); font-weight: 600;
    letter-spacing: var(--tracking-label); text-transform: uppercase;
  }
  .ledger-steps { display: flex; flex-wrap: wrap; gap: var(--sp-1); }
  .step {
    min-width: 8rem; max-width: 16rem; padding: var(--sp-1); border: 1px solid var(--line);
    border-radius: var(--radius-xs); background: var(--surface);
  }
  .step[data-status="failed"] { border-color: var(--error); background: var(--error-soft); }
  .step[data-status="running"] { border-color: var(--warn); background: var(--warn-soft); }
  .step[data-kind="unknown"] { border-style: dashed; }
  .step-tool { font-family: var(--font-mono); font-size: var(--fs-xs); color: var(--ink); overflow-wrap: anywhere; }
  .step-args { margin-top: 2px; color: var(--ink-faint); font-family: var(--font-mono); font-size: var(--fs-2xs); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .step-outcome { margin-top: var(--sp-0-5); color: var(--ink-muted); font-size: var(--fs-2xs); }
  .effects { margin: var(--sp-2) 0 0; color: var(--ink-muted); font-size: var(--fs-xs); }
  .receipt { border-left: 2px solid var(--line-strong); padding-left: var(--sp-2); font-size: var(--fs-sm); color: var(--ink-muted); }
  .receipt-big { font-size: var(--fs-base); font-weight: 600; color: var(--ink); }
  .receipt-dim { margin: var(--sp-1) 0 0; color: var(--ink-faint); font-size: var(--fs-xs); }
  .receipt-pending { margin: var(--sp-1) 0 0; color: var(--warn); font-size: var(--fs-xs); }
  .receipt-summary { margin: var(--sp-1) 0 0; color: var(--ink); font-size: var(--fs-sm); }
  .receipt-reason { margin: var(--sp-1) 0 0; }
  .receipt-counts { display: grid; grid-template-columns: auto 1fr; gap: var(--sp-0-5) var(--sp-2); margin: var(--sp-1) 0 0; }
  .receipt-counts dt { color: var(--ink-faint); font-size: var(--fs-xs); }
  .receipt-counts dd { margin: 0; font-family: var(--font-mono); font-size: var(--fs-sm); }
  .ledger-none, .ledger-note, .ledger-missing, .ledger-empty { color: var(--ink-muted); font-size: var(--fs-sm); }
  .ledger-empty { padding: var(--sp-4) 0; }
  .ledger-empty b { display: block; color: var(--ink); font-size: var(--fs-md); }
  .ledger-unknown-row { display: flex; flex-wrap: wrap; align-items: baseline; gap: var(--sp-2); padding: var(--sp-1) 0; border-bottom: 1px dotted var(--line); }
  .ledger-unknown-row[data-kind="unknown"] { border-left: 2px dashed var(--line-strong); padding-left: var(--sp-2); }
  .ledger-tool { font-family: var(--font-mono); font-size: var(--fs-sm); }
  .ledger-args { color: var(--ink-faint); font-family: var(--font-mono); font-size: var(--fs-xs); }
  .ledger-outcome { margin-left: auto; color: var(--ink-muted); font-size: var(--fs-xs); }
  .ledger-rail {
    align-self: start; padding: var(--sp-3); border: 1px solid var(--line);
    border-radius: var(--radius); background: var(--panel); font-size: var(--fs-sm);
  }
  .rail-label { margin: var(--sp-2) 0 var(--sp-1); font-size: var(--fs-2xs); font-weight: 600; letter-spacing: var(--tracking-label); text-transform: uppercase; color: var(--ink-faint); }
  .rail-label:first-child { margin-top: 0; }
  .rail-session { display: flex; align-items: baseline; gap: var(--sp-1-5); padding: var(--sp-1) 0; border-bottom: 1px dotted var(--line); }
  .rail-session:last-child { border-bottom: 0; }
  .session-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--faint); display: inline-block; flex: none; }
  .session-dot.live { background: var(--ok); box-shadow: 0 0 0 3px color-mix(in srgb, var(--ok) 16%, transparent); }
  .rail-session-name { font-weight: 600; overflow-wrap: anywhere; }
  .rail-holder { margin-left: auto; color: var(--ink-faint); font-size: var(--fs-xs); text-align: right; }
  .rail-rule { margin: var(--sp-3) 0; border-top: 1px solid var(--line); }
  .rail-legend { display: flex; align-items: baseline; gap: var(--sp-1-5); margin: 0 0 var(--sp-1); color: var(--ink-muted); font-size: var(--fs-xs); }
  .legend-swatch { width: 8px; height: 8px; border-radius: 2px; display: inline-block; flex: none; }
  .legend-swatch.claim { background: var(--accent); }
  .legend-swatch.fact { background: var(--ok); }
  .rail-note, .rail-none { margin: var(--sp-1) 0 0; color: var(--ink-faint); font-size: var(--fs-xs); line-height: 1.6; }
  .rail-record { margin: 0; color: var(--ink-muted); font-size: var(--fs-xs); }
  .rail-record b { color: var(--ink); }
  details.transcript { max-width: 60rem; margin: var(--sp-4) auto 0; padding: 0 var(--sp-4); }
  details.transcript summary { cursor: pointer; color: var(--ink-muted); font-size: var(--fs-sm); font-weight: 600; }
  @media (max-width: 60rem) {
    .ledger-layout { grid-template-columns: 1fr; }
    .ledger-spread { grid-template-columns: 1fr; }
    .ledger-rail { order: -1; }
  }
  @media (max-width: 40rem) {
    .activity-head { padding: var(--sp-2) var(--sp-3); }
    .activity-count { display: none; }
    .activity-args { display: none; }
  }
"##;

/// The page's only script: poll, append/update, filter, theme, follow and run expand.
pub(crate) const VIEW_SCRIPT: &str = r##"(function () {
  "use strict";
  var feed = document.getElementById("feed");
  if (!feed) return;
  var stateLine = document.getElementById("activity-state");
  var filter = document.getElementById("activity-filter");
  var themeButton = document.getElementById("activity-theme");
  var meta = document.querySelector('meta[name="summrise-panel"]');
  var token = meta ? meta.getAttribute("content") : "";
  var boot = feed.getAttribute("data-boot") || "";
  var cursor = feed.getAttribute("data-next") || "";
  var follow = true;
  var polling = false;
  var failures = 0;
  var reloading = false;
  var seen = new Map();

  function el(tag, cls, text) {
    var node = document.createElement(tag);
    if (cls) node.className = cls;
    if (text !== undefined && text !== null) node.textContent = String(text);
    return node;
  }

  function fmtDur(ms) {
    return ms < 1000 ? Math.round(ms) + "ms" : (ms / 1000).toFixed(1) + "s";
  }

  function addKv(parent, key, value, cls) {
    parent.append(el("dt", "", key));
    parent.append(el("dd", cls || "", value));
  }

  function reloadPage() {
    if (reloading) return;
    reloading = true;
    window.location.reload();
  }

  function applyCounts(counts) {
    if (!counts) return;
    var box = document.querySelector(".activity-counts");
    if (box) {
      box.setAttribute("data-total", counts.total);
      box.setAttribute("data-failed", counts.failed);
    }
    var total = document.querySelector("[data-count-total]");
    if (total) {
      total.textContent = String(counts.total);
      total.setAttribute("data-count-total", counts.total);
    }
    var failed = document.querySelector("[data-count-failed]");
    if (failed) {
      failed.textContent = String(counts.failed);
      failed.setAttribute("data-count-failed", counts.failed);
    }
    var groups = counts.groups || {};
    var spans = document.querySelectorAll(".activity-count[data-group]");
    for (var i = 0; i < spans.length; i += 1) {
      var key = spans[i].getAttribute("data-group");
      var value = groups[key];
      if (typeof value !== "number") continue;
      spans[i].setAttribute("data-count", value);
      var number = spans[i].querySelector("b");
      if (number) number.textContent = String(value);
    }
  }

  function rowMeta(node) {
    var end = node.getAttribute("data-end-seq");
    return {
      status: node.getAttribute("data-status") || "",
      end_seq: end && end !== "null" ? Number(end) : null
    };
  }

  function seedSeen() {
    var nodes = feed.querySelectorAll(".activity-row[data-id]");
    for (var i = 0; i < nodes.length; i += 1) {
      var id = Number(nodes[i].getAttribute("data-id"));
      if (!isNaN(id)) seen.set(id, rowMeta(nodes[i]));
    }
  }

  function refreshRunningDurations() {
    var now = Date.now();
    var rows = feed.querySelectorAll('[data-status="running"][data-started]');
    for (var i = 0; i < rows.length; i += 1) {
      var started = Number(rows[i].getAttribute("data-started"));
      if (!isFinite(started)) continue;
      var text = fmtDur(Math.max(0, now - started));
      var duration = rows[i].querySelector(".activity-duration");
      if (duration) duration.textContent = text;
      var detail = rows[i].querySelector(".activity-detail-duration");
      if (detail) detail.textContent = text;
    }
  }

  function applyTheme(theme) {
    document.documentElement.dataset.theme = theme;
    if (themeButton) {
      themeButton.textContent = theme === "dark" ? "\u2600" : "\u263e";
      themeButton.title = theme === "dark" ? "Switch to light" : "Switch to dark";
    }
  }

  var queryTheme = new URLSearchParams(location.search).get("theme");
  var saved = "light";
  if (queryTheme === "dark" || queryTheme === "light") {
    saved = queryTheme;
  } else {
    try { saved = localStorage.getItem("summrise-theme") || "light"; } catch (e) {}
  }
  applyTheme(saved === "dark" ? "dark" : "light");
  if (themeButton) {
    themeButton.addEventListener("click", function () {
      var next = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
      applyTheme(next);
      try { localStorage.setItem("summrise-theme", next); } catch (e) {}
    });
  }

  function applyFilter() {
    var value = filter ? filter.value : "all";
    var rows = feed.querySelectorAll(".activity-row[data-group]");
    for (var i = 0; i < rows.length; i += 1) {
      rows[i].hidden = value !== "all" && rows[i].getAttribute("data-group") !== value;
    }
  }
  if (filter) filter.addEventListener("change", applyFilter);
  applyFilter();

  function lastTurn() {
    var turns = feed.querySelectorAll(".activity-turn");
    return turns.length ? turns[turns.length - 1] : null;
  }

  function loadRunBlock(details) {
    if (!details.open || details.getAttribute("data-loaded")) return;
    var id = details.getAttribute("data-run");
    var body = details.querySelector(".activity-run-body");
    if (!id || !body) return;
    details.setAttribute("data-loaded", "1");
    loadRun(body, id);
  }

  function watchRun(details) {
    details.addEventListener("toggle", function () { loadRunBlock(details); });
  }

  function runBlock(id) {
    var details = el("details", "activity-run");
    details.setAttribute("data-run", id);
    details.append(el("summary", "", "Run " + id));
    details.append(el("div", "activity-run-body"));
    watchRun(details);
    return details;
  }

  function watchRuns() {
    var blocks = feed.querySelectorAll("details[data-run]");
    for (var i = 0; i < blocks.length; i += 1) watchRun(blocks[i]);
  }

  function rowNode(row) {
    var node = el("article", "activity-row status-" + row.status);
    node.setAttribute("data-id", String(row.id));
    node.setAttribute("data-seq", String(row.seq));
    node.setAttribute("data-group", row.group);
    node.setAttribute("data-status", row.status);
    node.setAttribute("data-started", String(row.at_ms));
    node.setAttribute(
      "data-end-seq",
      row.end_seq === null || row.end_seq === undefined ? "null" : String(row.end_seq)
    );

    var line = el("div", "activity-line");
    var glyph = row.running ? "\u2026" : (row.failed ? "\u2717" : "\u2713");
    line.append(el("span", "activity-glyph status-" + row.status, glyph));
    line.append(el("span", "activity-tool", row.tool));
    if (row.verb) line.append(el("span", "activity-verb", row.verb));
    if (row.decision) line.append(el("span", "chip decision-" + row.decision, row.decision));
    line.append(el("span", "chip status-" + row.status, row.status));
    if (row.args) line.append(el("span", "activity-args", row.args));
    if (row.outcome) line.append(el("span", "chip outcome", row.outcome));
    line.append(el("span", "activity-duration", fmtDur(row.duration_ms)));
    var time = el("time", "activity-time", row.time);
    time.setAttribute("datetime", row.started);
    time.setAttribute("title", row.started);
    line.append(time);
    node.append(line);

    var detail = el("div", "activity-detail");
    var kv = el("dl", "activity-kv");
    addKv(kv, "call", "#" + row.id);
    addKv(kv, "decision", row.decision || "\u2014");
    addKv(kv, "duration", fmtDur(row.duration_ms), "activity-detail-duration");
    addKv(kv, "outcome", row.running ? "running" : (row.status + (row.outcome ? " \u00b7 " + row.outcome : "")));
    if (row.caller) addKv(kv, "via", row.caller);
    if (row.error) addKv(kv, "error", row.error);
    if (row.session) addKv(kv, "session", row.session);
    detail.append(kv);
    if (row.run_id) detail.append(runBlock(row.run_id));
    node.append(detail);

    return node;
  }

  function setRowsState() {
    if (feed.getAttribute("data-state") === "rows") return;
    var none = feed.querySelector(".activity-none");
    if (none) none.remove();
    feed.setAttribute("data-state", "rows");
  }

  function insertRow(row) {
    var id = Number(row.id);
    var incoming = {
      status: row.status,
      end_seq: row.end_seq === null || row.end_seq === undefined ? null : Number(row.end_seq)
    };
    var existing = feed.querySelector('.activity-row[data-id="' + row.id + '"]');
    var meta = seen.get(id) || (existing ? rowMeta(existing) : null);
    // The route keeps re-sending a completed row until the cursor passes it. The page already
    // has that fact: replacing the node would close an open run detail every second.
    if (existing && meta && meta.status === incoming.status && meta.end_seq === incoming.end_seq) {
      return;
    }
    var node = rowNode(row);
    seen.set(id, incoming);
    if (existing) {
      var reopen = null;
      var open = existing.querySelectorAll("details[data-run]");
      for (var i = 0; i < open.length; i += 1) {
        if (open[i].open) { reopen = open[i].getAttribute("data-run"); break; }
      }
      existing.replaceWith(node);
      if (reopen !== null) {
        var replacements = node.querySelectorAll("details[data-run]");
        for (var j = 0; j < replacements.length; j += 1) {
          if (replacements[j].getAttribute("data-run") === reopen) {
            replacements[j].open = true;
            loadRunBlock(replacements[j]);
            break;
          }
        }
      }
      applyFilter();
      return;
    }
    setRowsState();
    if (row.turn || !lastTurn()) {
      var section = el("section", "activity-turn");
      section.append(el("p", "activity-turn-label", row.turn_label));
      section.append(node);
      feed.append(section);
    } else {
      lastTurn().append(node);
    }
    applyFilter();
  }

  async function loadRun(body, id) {
    try {
      var response = await fetch("/api/runs/" + encodeURIComponent(id), {
        headers: { "x-summrise-panel": token },
        cache: "no-store"
      });
      if (response.status === 401 || response.status === 403) {
        reloadPage();
        return;
      }
      if (!response.ok) throw new Error("status " + response.status);
      var answer = await response.json();
      var run = answer.run || {};
      var argv = Array.isArray(run.args) && run.args.length ? " " + run.args.join(" ") : "";
      var code = run.exit_code === null || run.exit_code === undefined
        ? (run.timed_out ? "timed out" : "no exit code")
        : "exit " + run.exit_code;
      body.textContent = "";
      var kv = el("dl", "activity-kv");
      addKv(kv, "command", (run.command || "") + argv);
      addKv(kv, "cwd", run.cwd || "\u2014");
      addKv(kv, "result", code + " \u00b7 " + fmtDur(run.duration_ms || 0));
      addKv(kv, "output", "stdout " + (run.stdout_bytes == null ? "?" : run.stdout_bytes) + " B \u00b7 stderr " + (run.stderr_bytes == null ? "?" : run.stderr_bytes) + " B");
      body.append(kv);
      if (run.stdout_preview) {
        body.append(el("p", "activity-preview-label", "stdout"));
        body.append(el("pre", "activity-preview", run.stdout_preview));
      }
      if (run.stderr_preview) {
        body.append(el("p", "activity-preview-label", "stderr"));
        body.append(el("pre", "activity-preview", run.stderr_preview));
      }
    } catch (error) {
      body.textContent = "run " + id + " unavailable (" + error.message + ")";
    }
  }

  async function tick() {
    if (polling || !cursor || feed.getAttribute("data-state") === "unreadable") return;
    polling = true;
    try {
      var response = await fetch("/api/activity?after=" + encodeURIComponent(cursor) + "&limit=200", {
        headers: { "x-summrise-panel": token },
        cache: "no-store"
      });
      if (response.status === 401 || response.status === 403) {
        reloadPage();
        return;
      }
      if (!response.ok) throw new Error("status " + response.status);
      var answer = await response.json();
      failures = 0;
      applyCounts(answer.counts);
      var rows = Array.isArray(answer.rows) ? answer.rows : [];
      for (var i = 0; i < rows.length; i += 1) insertRow(rows[i]);
      if (typeof answer.next === "number") cursor = boot + "." + answer.next;
      if (stateLine) stateLine.textContent = "live";
      if (answer.more) {
        polling = false;
        await tick();
        return;
      }
      if (follow) window.scrollTo(0, document.body.scrollHeight);
    } catch (error) {
      failures += 1;
      if (failures >= 3) {
        reloadPage();
        return;
      }
      if (stateLine) stateLine.textContent = "reconnecting";
    } finally {
      polling = false;
    }
  }

  document.addEventListener("keydown", function (event) {
    if (event.key !== "f" || event.metaKey || event.ctrlKey || event.altKey) return;
    var tag = event.target && event.target.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
    follow = !follow;
  });

  seedSeen();
  watchRuns();
  if (feed.getAttribute("data-state") !== "unreadable") {
    void tick();
    window.setInterval(function () { void tick(); }, 1000);
  }
  window.setInterval(refreshRunningDurations, 1000);
})();"##;

/// The page's heading script: the query's theme wins over the stored one, as the old transcript's did.
const THEME_SCRIPT: &str = r#"try {
  var q = new URLSearchParams(location.search).get("theme");
  if (q === "light" || q === "dark") {
    document.documentElement.dataset.theme = q;
  } else {
    var t = localStorage.getItem("summrise-theme");
    if (t === "light" || t === "dark") document.documentElement.dataset.theme = t;
  }
} catch (e) {}"#;

/// Which of the page's two themes the request asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Theme {
    Light,
    Dark,
}

impl Theme {
    /// `?theme=dark` chooses the dark theme; every other spelling, and no query, is light.
    pub(crate) fn from_query(theme: Option<&str>) -> Theme {
        match theme {
            Some("dark") => Theme::Dark,
            _ => Theme::Light,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }
}

/// Everything the transcript shows: the bounded audit tail and the facts the page and its polls need.
#[derive(Debug)]
pub(crate) struct ActivityFacts {
    /// The audit's newest lines, oldest first. `None` is "the audit could not be read"; `Some(vec![])`
    /// is "the audit has no lines", which the page says differently.
    pub(crate) audit: Option<Vec<Value>>,
    /// True when the audit held more lines than the bound, so the page says older calls are missing.
    pub(crate) truncated: bool,
    /// The instant a running call's duration is measured to. A request reads it once.
    pub(crate) now_ms: u64,
    /// This boot's id, carried in every cursor so a cursor from a previous process cannot name a
    /// row in this one's numbering.
    pub(crate) boot: u64,
    /// This boot's panel token; the page carries it only in its meta tag.
    pub(crate) panel_token: String,
    /// The transactions the audit names: this process's records and, with `retained: false`, a group
    /// whose receipt is gone. Both pages read the same list, so they cannot disagree.
    pub(crate) txns: Vec<panel::TxnFact>,
    /// The live terminal sessions, with their holders and goals, for the rail.
    pub(crate) sessions: Vec<panel::SessionFacts>,
    /// The transaction `?txn=<id>` names, when the query carried one.
    pub(crate) selected_txn: Option<String>,
}

/// One transcribed call: the row the server draws and the JSON `/api/activity` streams.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ActivityRow {
    /// The audit's call id. Two lines of one call share it, and a poll updates a row by it.
    pub(crate) id: u64,
    /// The order of the row's `begin` line: the call's place in the oldest-first transcript. A
    /// cursor names this, so paging cannot skip a call whose `end` lands after a later call began.
    /// It is not the row's index in the window: a window position stops at 200 and shifts as lines
    /// arrive, which would stop the live stream.
    pub(crate) seq: u64,
    /// The order of the row's `end` line, or `None` while the call is running. A poll uses it to
    /// re-deliver the outcome of a call before the cursor and the view uses it to recognise an
    /// update it already has, so a completed row is not churned every second.
    pub(crate) end_seq: Option<u64>,
    /// The `begin` line's epoch milliseconds (the `end`'s when the begin is outside the window).
    pub(crate) at_ms: u64,
    /// `HH:MM:SS` in this machine's timezone.
    pub(crate) time: String,
    /// The exact UTC instant, for the `datetime`/`title` attributes.
    pub(crate) started: String,
    /// From `begin` to `end`, or from `begin` to the read's instant while running.
    pub(crate) duration_ms: u64,
    /// No `end` line is in the window. The page does not invent an outcome for it.
    pub(crate) running: bool,
    /// The tool the `begin` line named, or `unknown` for an orphan `end`.
    pub(crate) tool: String,
    /// The old transcript's one-line description of the tool.
    pub(crate) verb: String,
    /// `runs`, `terminal`, `files` or `other`: the filter's own taxonomy, computed here.
    pub(crate) group: String,
    /// The `begin` line's authorization decision.
    pub(crate) decision: String,
    /// `ok`, `failed` or `running`: the word the audit's facts support.
    pub(crate) status: String,
    /// The `end` line said `ok: false`. A missing end is not a failure.
    pub(crate) failed: bool,
    /// The transport the call arrived through.
    pub(crate) caller: String,
    /// The redacted, capped summary the `begin` line carried.
    pub(crate) args: String,
    /// The `end` line's one-line outcome.
    pub(crate) outcome: String,
    /// The `end` line's closed-set error kind.
    pub(crate) error: String,
    /// The run `runs.exec` produced, when its `end` said so.
    pub(crate) run_id: String,
    /// The terminal session a `terminal.*` call named, when its arguments said so.
    pub(crate) session: String,
    /// True when this row starts a turn: the first row, or one more than [`TURN_BREAK_MS`] after
    /// the previous row.
    pub(crate) turn: bool,
    /// The clock label of this row's turn, so a client adding a row can name the turn it starts
    /// without computing where a turn begins.
    pub(crate) turn_label: String,
}

/// One answer of `/api/activity`: the rows, the cursor to ask after them, whether the record
/// continues, and the server-computed counts of the whole window so the header never goes stale.
#[derive(Debug, Serialize)]
pub(crate) struct ActivityPage {
    pub(crate) rows: Vec<ActivityRow>,
    /// The greatest begin-`seq` among the rows returned after the caller's cursor; the caller's
    /// `after` when this page added no new begin (an update-only answer does not move it).
    pub(crate) next: u64,
    /// True when more new begins were cut, or (without a cursor) when older rows exist. A page with
    /// a cursor can carry more rows than `limit` when late-end updates accompany the new begins.
    pub(crate) more: bool,
    /// The counts of the whole audit window, not only of the rows on this page.
    pub(crate) counts: GroupCounts,
    /// The boot this window belongs to; not part of the wire answer.
    #[serde(skip)]
    pub(crate) boot: u64,
    /// Whether the tail was cut; not part of the wire answer.
    #[serde(skip)]
    pub(crate) truncated: bool,
}

/// The counts over the audit window, one field per filter value. The route serializes this as
/// `counts: {total, failed, groups: {runs, terminal, files, other}}`; the view applies the numbers
/// to the header and counts nothing itself.
#[derive(Debug, Default, Serialize)]
pub(crate) struct GroupCounts {
    pub(crate) total: usize,
    pub(crate) failed: usize,
    pub(crate) groups: GroupTally,
}

/// The per-group half of [`GroupCounts`].
#[derive(Debug, Default, Serialize)]
pub(crate) struct GroupTally {
    pub(crate) runs: usize,
    pub(crate) terminal: usize,
    pub(crate) files: usize,
    pub(crate) other: usize,
}

impl GroupCounts {
    fn add(&mut self, row: &ActivityRow) {
        self.total += 1;
        if row.failed {
            self.failed += 1;
        }
        match row.group.as_str() {
            "runs" => self.groups.runs += 1,
            "terminal" => self.groups.terminal += 1,
            "files" => self.groups.files += 1,
            _ => self.groups.other += 1,
        }
    }
}

/// The filter group of a tool, the taxonomy the old transcript used.
pub(crate) fn tool_group(tool: &str) -> &'static str {
    if tool.starts_with("runs.") {
        "runs"
    } else if tool.starts_with("terminal.") {
        "terminal"
    } else if tool.starts_with("files.") {
        "files"
    } else {
        "other"
    }
}

/// The old transcript's one-line description of a tool; an unknown tool has none.
pub(crate) fn tool_verb(tool: &str) -> &'static str {
    match tool {
        "runs.exec" => "run a command",
        "runs.get" => "read a run",
        "runs.list" => "list runs",
        "terminal.open" => "open a terminal",
        "terminal.create" => "open a terminal",
        "terminal.write" => "type into a terminal",
        "terminal.read" => "read a terminal",
        "terminal.close" => "close a terminal",
        "files.read" => "read a file",
        "files.write" => "write a file",
        "files.list" => "list files",
        "screen.capture" => "look at the screen",
        "state.get" => "read device state",
        "browser.open" => "open a browser",
        "browser.navigate" => "navigate a browser",
        "monitor.add" => "watch an endpoint",
        "monitor.list" => "list watches",
        "monitor.probe" => "probe an endpoint",
        _ => "",
    }
}

/// The transcript's rows from one bounded audit window, oldest first.
///
/// The pairing is the panel's (one `begin` and its `end`), and the order of the row's `begin` line
/// is its `seq`. An `end` the pairing left over is still a fact and becomes an `unknown` row, as the
/// old transcript's addition of a lone `end` did.
pub(crate) fn rows(audit: &[Value], now_ms: u64) -> Vec<ActivityRow> {
    let calls = panel::call_pairs(audit);
    let seqs = line_seqs(audit);
    let mut consumed = vec![false; audit.len()];
    let mut rows = Vec::with_capacity(calls.len());

    for (begin, end) in &calls {
        let Some(begin_index) = line_index(audit, begin) else {
            continue;
        };
        let end_index = end.and_then(|end| line_index(audit, end));
        if let Some(index) = end_index {
            consumed[index] = true;
        }
        let begin_seq = seqs[begin_index];
        let end_seq = end_index.map(|index| seqs[index]);
        if let Some(row) = call_row(begin, *end, begin_seq, end_seq, now_ms) {
            rows.push(row);
        }
    }

    for (index, line) in audit.iter().enumerate() {
        if consumed[index] {
            continue;
        }
        if line.get("event").and_then(Value::as_str) != Some("end") {
            continue;
        }
        if let Some(row) = orphan_row(line, seqs[index], Some(seqs[index])) {
            rows.push(row);
        }
    }

    rows.sort_by_key(|row| row.at_ms);
    assign_turns(&mut rows);

    rows
}

/// How wide one instant's order is: 500 call ids and both halves of each fit under it. The instant
/// is kept in milliseconds and the stride keeps a `seq` under JavaScript's exact-integer range, so
/// the page can round-trip a cursor through JSON without losing it (safe until the year 2255).
const SEQ_STRIDE: u64 = 1_000;

/// The order of every audit line: its instant, then its call id, then the half (a `begin` sorts
/// before the `end` that shares its id).
///
/// This is monotonic along the file, so a cursor survives the 200-line window shifting and a call's
/// later `end` always sorts after the `begin` a page has already drawn. A line's position in the
/// window would not: it is at most 200 and moves down as new lines arrive.
fn line_seqs(audit: &[Value]) -> Vec<u64> {
    let mut seqs = Vec::with_capacity(audit.len());
    for line in audit {
        let event = line.get("event").and_then(Value::as_str);
        let id = line.get("id").and_then(Value::as_u64).unwrap_or(0);
        let half = u64::from(event != Some("begin"));
        let local = (id % 500) * 2 + half;
        let at_ms = line.get("ts_ms").and_then(Value::as_u64).unwrap_or(0);
        seqs.push(
            at_ms
                .saturating_mul(SEQ_STRIDE)
                .saturating_add(local)
                .saturating_add(1),
        );
    }

    seqs
}

/// The index of one audit line in the window, by identity: `call_pairs` returned references into it.
fn line_index(audit: &[Value], line: &Value) -> Option<usize> {
    audit
        .iter()
        .position(|candidate| std::ptr::eq(candidate, line))
}

/// One paired call as a row, or `None` when the begin line carries no tool, instant or local time.
fn call_row(
    begin: &Value,
    end: Option<&Value>,
    seq: u64,
    end_seq: Option<u64>,
    now_ms: u64,
) -> Option<ActivityRow> {
    let tool = begin
        .get("tool")
        .and_then(Value::as_str)
        .filter(|tool| !tool.is_empty())?;
    let at_ms = begin.get("ts_ms").and_then(Value::as_u64)?;
    let at = panel::local_at(at_ms)?;
    let args = text(begin, "args");
    let (status, failed, duration_ms, outcome, error, run_id, running) = match end {
        Some(end) => {
            let ok = end.get("ok").and_then(Value::as_bool).unwrap_or(false);
            let end_ms = end.get("ts_ms").and_then(Value::as_u64).unwrap_or(at_ms);
            (
                if ok { "ok" } else { "failed" },
                !ok,
                end_ms.saturating_sub(at_ms),
                text(end, "outcome"),
                text(end, "error_kind"),
                text(end, "run_id"),
                false,
            )
        }
        // The instant is the read's, not the row's: the page says "running", never an outcome.
        None => (
            "running",
            false,
            now_ms.saturating_sub(at_ms),
            String::new(),
            String::new(),
            String::new(),
            true,
        ),
    };

    Some(ActivityRow {
        id: begin.get("id").and_then(Value::as_u64)?,
        seq,
        end_seq,
        at_ms,
        time: at.format("%H:%M:%S").to_string(),
        started: crate::store::format_utc(at_ms / 1_000),
        duration_ms,
        running,
        tool: tool.to_owned(),
        verb: tool_verb(tool).to_owned(),
        group: tool_group(tool).to_owned(),
        decision: text(begin, "decision"),
        status: status.to_owned(),
        failed,
        caller: text(begin, "transport"),
        session: session_of(tool, &args),
        args,
        outcome,
        error,
        run_id,
        turn: false,
        turn_label: String::new(),
    })
}

/// The `end` the pairing could not attach to a `begin` in the window.
///
/// Its tool is genuinely unknown — the begin line that named it is not here — so the row says
/// `unknown` and groups under `other`. The outcome fields are the end line's own; nothing is inferred.
fn orphan_row(line: &Value, seq: u64, end_seq: Option<u64>) -> Option<ActivityRow> {
    let id = line.get("id").and_then(Value::as_u64)?;
    let at_ms = line.get("ts_ms").and_then(Value::as_u64)?;
    let at = panel::local_at(at_ms)?;
    let ok = line.get("ok").and_then(Value::as_bool).unwrap_or(false);

    Some(ActivityRow {
        id,
        seq,
        end_seq,
        at_ms,
        time: at.format("%H:%M:%S").to_string(),
        started: crate::store::format_utc(at_ms / 1_000),
        duration_ms: 0,
        running: false,
        tool: "unknown".to_owned(),
        verb: String::new(),
        group: "other".to_owned(),
        decision: String::new(),
        status: if ok { "ok" } else { "failed" }.to_owned(),
        failed: !ok,
        caller: text(line, "transport"),
        args: String::new(),
        outcome: text(line, "outcome"),
        error: text(line, "error_kind"),
        run_id: text(line, "run_id"),
        session: String::new(),
        turn: false,
        turn_label: String::new(),
    })
}

/// One string field of an audit line, or the empty string when it did not write one.
fn text(line: &Value, key: &str) -> String {
    line.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

/// The session a `terminal.*` begin line named, if any.
fn session_of(tool: &str, args: &str) -> String {
    if tool.starts_with("terminal.") {
        args.strip_prefix("session=").unwrap_or("").to_owned()
    } else {
        String::new()
    }
}

/// Marks the rows that start a turn and gives every row its turn's label.
///
/// The break is the old transcript's: a call more than eight seconds after the turn's first call
/// opens the next turn. The first row always opens one.
fn assign_turns(rows: &mut [ActivityRow]) {
    let mut turn_start: Option<u64> = None;
    let mut label = String::new();
    for row in rows {
        let starts = match turn_start {
            None => true,
            Some(start) => row.at_ms.saturating_sub(start) > TURN_BREAK_MS,
        };
        if starts {
            turn_start = Some(row.at_ms);
            label = row.time.clone();
            row.turn = true;
        }
        row.turn_label = label.clone();
    }
}

/// The counts over a set of rows, per group and in total.
pub(crate) fn group_counts(rows: &[ActivityRow]) -> GroupCounts {
    let mut counts = GroupCounts::default();
    for row in rows {
        counts.add(row);
    }
    counts
}

/// The rows of one turn, oldest first.
struct Turn<'a> {
    label: &'a str,
    started: &'a str,
    rows: Vec<&'a ActivityRow>,
}

fn turns(rows: &[ActivityRow]) -> Vec<Turn<'_>> {
    let mut turns: Vec<Turn<'_>> = Vec::new();
    for row in rows {
        if row.turn || turns.is_empty() {
            turns.push(Turn {
                label: &row.turn_label,
                started: &row.started,
                rows: Vec::new(),
            });
        }
        if let Some(turn) = turns.last_mut() {
            turn.rows.push(row);
        }
    }

    turns
}

/// One answer of [`page_of`]: the rows of a window, the cursor to ask after them, and the counts
/// of the whole window.
///
/// Without `after`, the answer is the newest `limit` rows, oldest first, and `more` says older rows
/// exist: the initial read of a client that did not start from the page's first paint.
///
/// With `after` (the greatest begin-`seq` the caller has), the answer is two sets, oldest first:
/// the first `limit` rows whose `begin` comes after it, and **every** row at or before it whose
/// `end` comes after it. `limit` is the new-begin page size, not a total-rows cap: a late `end` is
/// always included, so a running call's outcome cannot be skipped by new begins that arrived
/// before the next poll. The second set is bounded by the audit window, not by `limit`; a page
/// re-reads rows by `id`, so a row in both sets is one row to the reader. `next` is the greatest
/// begin-`seq` in the new-begin set, or the caller's `after` when that set was empty. `more` is true
/// when new begins were cut. `boot` and `truncated` travel with the answer for the page's render and
/// are not part of the JSON the route returns.
pub(crate) fn page_of(
    rows: &[ActivityRow],
    after: Option<u64>,
    limit: usize,
    boot: u64,
    truncated: bool,
) -> ActivityPage {
    let limit = limit.clamp(1, MAX_ROWS);
    let counts = group_counts(rows);

    let (page, next, more) = match after {
        None => {
            let start = rows.len().saturating_sub(limit);
            let page = rows[start..].to_vec();
            let next = page.iter().map(|row| row.seq).max().unwrap_or(0);
            // "More" means the record continues in the older direction: either rows the page left
            // out of this window, or rows the 200-line tail cut off before this window started.
            (page, next, start > 0 || truncated)
        }
        Some(after) => {
            let new_begins: Vec<&ActivityRow> = rows.iter().filter(|row| row.seq > after).collect();
            let more = new_begins.len() > limit;
            let mut page: Vec<ActivityRow> = new_begins
                .iter()
                .take(limit)
                .map(|row| (*row).clone())
                .collect();
            // Every late end, outside the new-begin page size: dropping one would leave the row
            // "running" forever once newer begins moved the cursor past its end.
            page.extend(
                rows.iter()
                    .filter(|row| row.seq <= after && row.end_seq.is_some_and(|end| end > after))
                    .cloned(),
            );
            page.sort_by_key(|row| row.seq);
            let next = page
                .iter()
                .filter(|row| row.seq > after)
                .map(|row| row.seq)
                .max()
                .unwrap_or(after);
            (page, next, more)
        }
    };

    ActivityPage {
        rows: page,
        next,
        more,
        counts,
        boot,
        truncated,
    }
}

/// The cursor spelling: this boot's id, a dot, then the row's `seq`.
pub(crate) fn cursor_string(boot: u64, seq: u64) -> String {
    format!("{boot}.{seq}")
}

/// The `(boot, seq)` a cursor spells, or `None` when it does not spell one.
pub(crate) fn parse_cursor(text: &str) -> Option<(u64, u64)> {
    let (boot, seq) = text.split_once('.')?;
    Some((boot.parse().ok()?, seq.parse().ok()?))
}

/// One observed call inside a ledger entry. It is the row the page draws, never a summary.
#[derive(Clone, Debug)]
struct LedgerStep {
    tool: String,
    args: String,
    outcome: String,
    status: String,
    duration_ms: u64,
    at_ms: u64,
    time: String,
    /// The end line's `error_kind` was `refused`: the call was turned away, not attempted.
    refused: bool,
    unknown: bool,
}

impl LedgerStep {
    fn spark_class(&self) -> &'static str {
        match self.status.as_str() {
            "failed" => "bad",
            "running" => "work",
            _ => "ok",
        }
    }
}

/// One transaction, as the record can answer it: the claim (purpose and author), the calls the
/// bounded window observed, and the receipt the device still holds.
#[derive(Clone, Debug)]
struct LedgerGroup {
    id: String,
    purpose: String,
    author: String,
    session: Option<String>,
    state: String,
    reason: Option<String>,
    retained: bool,
    started_at_ms: u64,
    duration_ms: u64,
    calls: u64,
    failures: u64,
    refusals: u64,
    steps: Vec<LedgerStep>,
    window_calls: usize,
    window_failures: usize,
    window_refusals: usize,
}

impl LedgerGroup {
    fn purpose_or_unknown(&self) -> &str {
        if self.purpose.is_empty() {
            "(purpose not retained)"
        } else {
            &self.purpose
        }
    }

    fn start_time(&self) -> String {
        panel::local_at(self.started_at_ms)
            .map(|at| at.format("%H:%M").to_string())
            .unwrap_or_default()
    }

    fn started_utc(&self) -> String {
        crate::store::format_utc(self.started_at_ms / 1_000)
    }

    fn duration_text(&self) -> String {
        duration_text(self.duration_ms)
    }

    fn is_working(&self) -> bool {
        self.state == "working"
    }
}

/// The ledger's answer from one read: the transaction groups, the rows that rode no transaction,
/// and how many rows this build could not classify (visible and counted, never dropped).
struct Ledger {
    groups: Vec<LedgerGroup>,
    loose: Vec<LedgerStep>,
    unknown: usize,
}

/// Build the ledger from the registry's transactions and the audit's rows.
///
/// **The receipt is the registry's record, and the row's `txn` field is the only grouping.** A
/// transaction the registry still holds carries its counts and its declared ending; one it does not
/// (a restart, or eviction past the retention window) is still drawn from the audit, but the page
/// says its receipt is not retained and invents neither counts nor an outcome. Its claim is the one
/// `http::txn_facts` joined from the transaction's own `txn.begin` row in this tail, so a purpose is
/// shown only when the record still holds it — an absent one says so. A call whose row carries no
/// txn is not guessed into a transaction: it is drawn under "Rows with no transaction".
fn ledger(facts: &ActivityFacts) -> Ledger {
    use std::collections::BTreeMap;

    let audit = facts.audit.as_deref().unwrap_or(&[]);
    let mut groups: BTreeMap<String, LedgerGroup> = BTreeMap::new();
    for fact in &facts.txns {
        groups.insert(
            fact.id.clone(),
            LedgerGroup {
                id: fact.id.clone(),
                purpose: fact.purpose.clone(),
                author: fact.author.clone(),
                session: fact.session.clone(),
                state: fact.state.clone(),
                reason: fact.reason.clone(),
                retained: fact.retained,
                started_at_ms: fact.started_at_ms,
                duration_ms: fact.duration_ms,
                calls: fact.calls,
                failures: fact.failures,
                refusals: fact.refusals,
                steps: Vec::new(),
                window_calls: 0,
                window_failures: 0,
                window_refusals: 0,
            },
        );
    }

    let mut loose = Vec::new();
    for (begin, end) in panel::call_pairs(audit) {
        let tool = begin
            .get("tool")
            .and_then(Value::as_str)
            .filter(|tool| !tool.is_empty())
            .unwrap_or("unknown");
        let row_txn = begin
            .get("txn")
            .or_else(|| end.and_then(|end| end.get("txn")))
            .and_then(Value::as_str)
            .filter(|txn| !txn.is_empty());
        let step = ledger_step(tool, begin, end, facts.now_ms);
        // The declaration itself is not a step of the work; its call rows still carry the txn.
        if tool.starts_with("txn.") {
            continue;
        }
        match row_txn.and_then(|id| groups.get_mut(id)) {
            Some(group) => {
                if step.status == "failed" {
                    group.window_failures += 1;
                    if step.refused {
                        group.window_refusals += 1;
                    }
                }
                group.window_calls += 1;
                group.steps.push(step);
            }
            None => loose.push(step),
        }
    }

    let unknown = loose.iter().filter(|step| step.unknown).count()
        + groups
            .values()
            .flat_map(|group| group.steps.iter())
            .filter(|step| step.unknown)
            .count();

    let mut groups: Vec<LedgerGroup> = groups.into_values().collect();
    groups.sort_by(|left, right| {
        right
            .started_at_ms
            .cmp(&left.started_at_ms)
            .then_with(|| right.id.cmp(&left.id))
    });

    Ledger {
        groups,
        loose,
        unknown,
    }
}

/// One call pair as a ledger step, with the audit's own outcome and duration.
fn ledger_step(tool: &str, begin: &Value, end: Option<&Value>, now_ms: u64) -> LedgerStep {
    let at_ms = begin.get("ts_ms").and_then(Value::as_u64).unwrap_or(0);
    let (status, duration_ms, outcome, refused) = match end {
        Some(end) => {
            let ok = end.get("ok").and_then(Value::as_bool).unwrap_or(false);
            let end_ms = end.get("ts_ms").and_then(Value::as_u64).unwrap_or(at_ms);
            (
                if ok { "ok" } else { "failed" },
                end_ms.saturating_sub(at_ms),
                text(end, "outcome"),
                !ok && text(end, "error_kind") == "refused",
            )
        }
        None => (
            "running",
            now_ms.saturating_sub(at_ms),
            String::new(),
            false,
        ),
    };
    let action = text(begin, "args");
    let action = action
        .split_whitespace()
        .find_map(|part| part.strip_prefix("action="))
        .unwrap_or("");
    let event = begin
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or("begin");
    let kind = summrise_core::RowKind::of_row(tool, Some(action), event);

    LedgerStep {
        tool: tool.to_owned(),
        args: text(begin, "args"),
        outcome,
        status: status.to_owned(),
        duration_ms,
        at_ms,
        time: panel::local_at(at_ms)
            .map(|at| at.format("%H:%M:%S").to_string())
            .unwrap_or_default(),
        refused,
        unknown: kind == summrise_core::RowKind::Unknown,
    }
}

/// The ledger's one expanded entry: `?txn` when the window holds it, else the working transaction,
/// else the newest. A `?txn` the window does not hold expands nothing: a stale link must not show
/// another transaction as if it were the named one.
fn expanded_id(facts: &ActivityFacts, groups: &[LedgerGroup]) -> Option<String> {
    if let Some(selected) = &facts.selected_txn {
        return groups
            .iter()
            .any(|group| &group.id == selected)
            .then(|| selected.clone());
    }
    groups
        .iter()
        .find(|group| group.is_working())
        .or_else(|| groups.first())
        .map(|group| group.id.clone())
}

/// The state word the ledger's chip carries. A transaction without a retained receipt may not know
/// its ending: the word lived in the receipt, so the chip says `unknown`.
fn ledger_state(group: &LedgerGroup) -> &str {
    if !group.retained || group.state.is_empty() {
        "unknown"
    } else {
        &group.state
    }
}

/// One transaction's line, and its three-column spread when it is the expanded entry.
fn ledger_entry(group: &LedgerGroup, expanded: bool) -> Markup {
    let state = ledger_state(group);
    let state_class = format!("chip ledger-chip state-{state}");
    html! {
        article
            class="ledger-entry"
            data-txn=(group.id)
            data-expanded=(if expanded { "true" } else { "false" })
            data-state=(state)
            data-retained=(if group.retained { "true" } else { "false" })
        {
            div class="ledger-line" {
                time
                    class="ledger-time"
                    datetime=(group.started_utc())
                    title=(group.started_utc())
                { (group.start_time()) }
                span class="ledger-purpose" { (group.purpose_or_unknown()) }
                span class=(state_class) { (state) }
                span class="ledger-spark" aria-hidden="true" {
                    @for step in &group.steps {
                        i class=(step.spark_class()) {}
                    }
                }
                span class="ledger-meta" {
                    @if group.retained {
                        (group.duration_text()) " · " (group.calls) " calls · "
                        a class="ledger-receipt-link" href=(format!("?txn={}", group.id)) {
                            "receipt →"
                        }
                    } @else {
                        (group.window_calls) " calls in this window · receipt not retained"
                    }
                }
            }
            @if expanded {
                (ledger_spread(group))
            }
        }
    }
}

/// The expanded entry: Said (the claim), Did (the observed rows), Left (the receipt or its absence).
fn ledger_spread(group: &LedgerGroup) -> Markup {
    html! {
        div class="ledger-spread" {
            section class="said" {
                h3 class="ledger-col-label" { "Said" }
                p class="claim-text" { "“" (group.purpose_or_unknown()) "”" }
                p class="claim-by" {
                    span class="claim-badge" { "CLAIM" }
                    " "
                    @if group.author.is_empty() {
                        "author not retained"
                    } @else {
                        (group.author)
                    }
                    " · "
                    time datetime=(group.started_utc()) title=(group.started_utc()) {
                        (group.start_time())
                    }
                }
                p class="claim-note" { "a claim, shown as its author's" }
            }
            section class="did" {
                h3 class="ledger-col-label" { "Did" }
                @if group.steps.is_empty() {
                    p class="ledger-none" { "No call in this window is tagged with this transaction." }
                } @else {
                    div class="ledger-steps" {
                        @for step in &group.steps {
                            div
                                class="step"
                                data-kind=(if step.unknown { "unknown" } else { "fact" })
                                data-status=(step.status)
                            {
                                div class="step-tool" { (step.tool) }
                                @if !step.args.is_empty() {
                                    div class="step-args" { (step.args) }
                                }
                                div class="step-outcome" {
                                    @if step.status == "running" {
                                        "running"
                                    } @else if step.outcome.is_empty() {
                                        (step.status)
                                    } @else {
                                        (step.status) " · " (step.outcome)
                                    }
                                    " · " (duration_text(step.duration_ms))
                                }
                            }
                        }
                    }
                }
                p class="effects" {
                    "Observed in this window: " (group.window_calls) " calls · "
                    (group.window_failures) " " (failure_noun(group.window_failures as u64))
                    " · " (group.window_refusals) " refused"
                }
            }
            section class="left" {
                h3 class="ledger-col-label" { "Left" }
                @if !group.retained {
                    div class="receipt" {
                        div class="receipt-big" { "receipt not retained" }
                        p class="receipt-dim" {
                            "This transaction's calls are still in the audit, but this process holds no receipt for it (it ended before a restart, or the receipt was evicted)."
                        }
                        @if group.window_calls > 0 {
                            p class="receipt-pending" {
                                (group.window_calls) " calls in this window · "
                                (group.window_failures) " " (failure_noun(group.window_failures as u64))
                            }
                        }
                    }
                } @else if group.is_working() {
                    div class="receipt" {
                        div class="receipt-big" { "No receipt yet." }
                        p class="receipt-dim" {
                            "The transaction is still working. Nothing is closed on the page until the AI commits or aborts."
                        }
                        p class="receipt-pending" {
                            (group.failures) " " (failure_noun(group.failures))
                            " so far · " (group.refusals) " refused"
                        }
                    }
                } @else {
                    div class="receipt" data-state=(state_name_for_receipt(group)) {
                        div class="receipt-big" {
                            @if group.state == "committed" { "Committed." } @else { "Aborted." }
                        }
                        @if let Some(reason) = &group.reason {
                            p class="receipt-reason" { "Reason: " (reason) }
                        }
                        p class="receipt-summary" {
                            (group.calls) " calls · " (group.failures) " "
                            (failure_noun(group.failures)) " · "
                            (group.refusals) " refused · " (group.duration_text())
                        }
                        dl class="receipt-counts" {
                            dt { "calls" }
                            dd { (group.calls) }
                            dt { "failures" }
                            dd { (group.failures) }
                            dt { "refused" }
                            dd { (group.refusals) }
                            dt { "duration" }
                            dd { (group.duration_text()) }
                        }
                        p class="receipt-dim" {
                            "The counts are the device's totals for the transaction; the steps beside them are the rows in this window."
                        }
                    }
                }
            }
        }
    }
}

/// The receipt's data attribute: the declared ending, never a verdict.
fn state_name_for_receipt(group: &LedgerGroup) -> &'static str {
    if group.state == "committed" {
        "committed"
    } else if group.state == "aborted" {
        "aborted"
    } else {
        "unknown"
    }
}

/// `1 failure`, `2 failures`.
fn failure_noun(count: u64) -> &'static str {
    if count == 1 {
        "failure"
    } else {
        "failures"
    }
}

/// One row that rode no transaction: shown, and counted as unknown when this build cannot place it.
fn loose_row(step: &LedgerStep) -> Markup {
    html! {
        div
            class="ledger-unknown-row"
            data-kind=(if step.unknown { "unknown" } else { "fact" })
            data-status=(step.status)
        {
            time class="ledger-time" datetime=(crate::store::format_utc(step.at_ms / 1_000)) {
                (step.time)
            }
            span class="ledger-tool" { (step.tool) }
            @if !step.args.is_empty() {
                span class="ledger-args" { (step.args) }
            }
            span class="ledger-outcome" { (step.status) }
        }
    }
}

/// The side rail: the live sessions with their holders, the claim/fact legend, and the record's
/// bound. It says what the page is drawing from, not a total it does not have.
fn ledger_rail(facts: &ActivityFacts) -> Markup {
    let rows = facts.audit.as_ref().map(Vec::len).unwrap_or(0);
    html! {
        aside class="ledger-rail" {
            h2 class="rail-label" { "Sessions" }
            @if facts.sessions.is_empty() {
                p class="rail-none" { "No live session on this machine." }
            } @else {
                @for session in &facts.sessions {
                    div class="rail-session" data-session=(session.id) {
                        span
                            class=(if session.alive { "session-dot live" } else { "session-dot" })
                            aria-hidden="true"
                        {}
                        span class="rail-session-name" { (session.kind) " " (session.id) }
                        span class="rail-holder" { (session.holder) }
                    }
                }
            }
            div class="rail-rule" {}
            h2 class="rail-label" { "How to read this" }
            p class="rail-legend" {
                span class="legend-swatch claim" aria-hidden="true" {}
                "claim — what the AI says it is doing"
            }
            p class="rail-legend" {
                span class="legend-swatch fact" aria-hidden="true" {}
                "fact — what the device observed"
            }
            p class="rail-note" {
                "A transaction is one piece of work. Its receipt compares the claim with the facts; it never rewrites them."
            }
            div class="rail-rule" {}
            h2 class="rail-label" { "The record" }
            p class="rail-record" data-record-rows=(rows) {
                b { (rows) } " rows in this bounded tail"
            }
            p class="rail-note" {
                "The newest " (AUDIT_LINES) " audit lines. Older rows are not retained on this page."
                @if facts.truncated { " The tail is cut; older rows exist." }
            }
            p class="rail-note" {
                "Nothing on this page is the AI's narration: a purpose is a badged claim, and every step is a row the device wrote."
            }
        }
    }
}

/// `GET /activity`: the transcript's first paint, server-rendered from the audited facts.
pub(crate) fn page(facts: &ActivityFacts, theme: Theme) -> Markup {
    let rows = facts
        .audit
        .as_deref()
        .map(|audit| rows(audit, facts.now_ms))
        .unwrap_or_default();
    let page = page_of(&rows, None, MAX_ROWS, facts.boot, facts.truncated);
    let state = match (&facts.audit, page.rows.is_empty()) {
        (None, _) => "unreadable",
        (Some(_), true) => "empty",
        (Some(_), false) => "rows",
    };
    let counts = &page.counts;
    let turns = turns(&page.rows);
    let next = cursor_string(page.boot, page.next);
    let ledger = ledger(facts);
    let expanded = expanded_id(facts, &ledger.groups);
    // A `?txn` the window does not hold is named, not silently replaced by another transaction.
    let missing_txn = facts
        .selected_txn
        .as_ref()
        .filter(|id| !ledger.groups.iter().any(|group| &group.id == *id));
    // The section's own count: the rows it lists, not the window's total.
    let loose_unknown = ledger.loose.iter().filter(|step| step.unknown).count();
    // Device-scope records this process still holds are the machine's work. An orphan read only
    // from the audit may have been session-scoped, so it is not guessed into the machine group.
    let machine: Vec<&LedgerGroup> = ledger
        .groups
        .iter()
        .filter(|group| group.session.is_none() && group.retained)
        .collect();
    let unscoped: Vec<&LedgerGroup> = ledger
        .groups
        .iter()
        .filter(|group| group.session.is_none() && !group.retained)
        .collect();
    // The rail's order first (the live sessions), then any session the audit still names.
    let mut session_ids: Vec<String> = facts
        .sessions
        .iter()
        .map(|session| session.id.clone())
        .collect();
    for group in &ledger.groups {
        if let Some(session) = &group.session {
            if !session_ids.contains(session) {
                session_ids.push(session.clone());
            }
        }
    }
    let groups_empty = ledger.groups.is_empty() && ledger.loose.is_empty();
    let ledger_state = match (&facts.audit, groups_empty) {
        (None, _) => "unreadable",
        (Some(_), true) => "empty",
        (Some(_), false) => "rows",
    };
    let theme_name = theme.as_str();
    let theme_glyph = match theme {
        Theme::Light => "\u{263e}",
        Theme::Dark => "\u{2600}",
    };

    html! {
        (DOCTYPE)
        html lang="en" data-theme=(theme_name) {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="summrise-panel" content=(facts.panel_token);
                title { "Summrise — operations" }
                style { (PreEscaped(summrise_ui::style(STYLES))) }
                script { (PreEscaped(THEME_SCRIPT)) }
            }
            body class="activity-page" {
                header class="activity-head" {
                    div class="activity-brand" {
                        h1 { "Summrise" }
                        span class="stem" { "operations" }
                    }
                    span class="activity-pill" {
                        span class="dot" aria-hidden="true" {}
                        span id="activity-state" { "live" }
                    }
                    span class="spacer" {}
                    div
                        class="activity-counts"
                        data-total=(counts.total)
                        data-failed=(counts.failed)
                        data-window=(format!("last {AUDIT_LINES}"))
                        title=(format!(
                            "These counts cover the newest {AUDIT_LINES} audit lines, not every call the page has appended."
                        ))
                    {
                        span
                            class="activity-window"
                            title="The audit is read as a bounded tail; older calls are not in these numbers."
                        {
                            "last " (AUDIT_LINES)
                        }
                        span class="activity-summary" {
                            b data-count-total=(counts.total) { (counts.total) }
                            " calls · "
                            b data-count-failed=(counts.failed) { (counts.failed) }
                            " failed"
                        }
                        span class="activity-count" data-group="runs" data-count=(counts.groups.runs) {
                            "Runs " b { (counts.groups.runs) }
                        }
                        span class="activity-count" data-group="terminal" data-count=(counts.groups.terminal) {
                            "Terminal " b { (counts.groups.terminal) }
                        }
                        span class="activity-count" data-group="files" data-count=(counts.groups.files) {
                            "Files " b { (counts.groups.files) }
                        }
                        span class="activity-count" data-group="other" data-count=(counts.groups.other) {
                            "Other " b { (counts.groups.other) }
                        }
                    }
                    select id="activity-filter" class="activity-filter" aria-label="Tool filter" {
                        option value="all" { "All tools" }
                        option value="runs" { "Runs" }
                        option value="terminal" { "Terminal" }
                        option value="files" { "Files" }
                        option value="other" { "Other" }
                    }
                    a class="terminal-link" href="/panel#terminal" { "terminal ↗" }
                    button
                        id="activity-theme"
                        class="activity-theme"
                        type="button"
                        title="Toggle light/dark"
                    {
                        (theme_glyph)
                    }
                }
                main id="main" class="activity-main" {
                    div class="ledger-layout" {
                        (ledger_rail(facts))
                        section
                            id="ledger"
                            class="ledger"
                            data-state=(ledger_state)
                            data-transactions=(ledger.groups.len())
                            data-unknown=(ledger.unknown)
                        {
                            @if let Some(missing) = missing_txn {
                                p class="ledger-missing" data-missing-txn=(missing) {
                                    b { "That transaction is not in this window." }
                                    " No retained receipt, working record or audit row here names "
                                    span class="mono" { (missing) }
                                    ". It may have ended before this record's bound."
                                }
                            }
                            @if ledger_state == "unreadable" {
                                p class="ledger-empty" {
                                    b { "The record is unreadable." }
                                    " The device's audit could not be read, so this page cannot say what the machine did."
                                }
                            } @else if groups_empty {
                                p class="ledger-empty" {
                                    b { "No transaction has been recorded." }
                                    " No line in this period names a unit of work."
                                }
                            } @else {
                                @if !machine.is_empty() {
                                    section class="ledger-machine" data-group="machine" {
                                        h2 class="ledger-group-head" {
                                            "Machine"
                                            span class="ledger-group-note" { "device-scope transactions" }
                                        }
                                        @for group in &machine {
                                            (ledger_entry(group, expanded.as_deref() == Some(group.id.as_str())))
                                        }
                                    }
                                }
                                @if !unscoped.is_empty() {
                                    section class="ledger-unscoped" data-group="unscoped" {
                                        h2 class="ledger-group-head" {
                                            "No session recorded"
                                            span class="ledger-group-note" { "the audit kept the work, not its scope" }
                                        }
                                        p class="ledger-note" {
                                            "These transactions cannot say whether they worked on a session or on the machine: the receipt that carried the scope is not retained."
                                        }
                                        @for group in &unscoped {
                                            (ledger_entry(group, expanded.as_deref() == Some(group.id.as_str())))
                                        }
                                    }
                                }
                                @for session_id in &session_ids {
                                    @let members: Vec<&LedgerGroup> = ledger
                                        .groups
                                        .iter()
                                        .filter(|group| group.session.as_deref() == Some(session_id.as_str()))
                                        .collect();
                                    @if !members.is_empty() {
                                        section class="ledger-session" data-group="session" data-session=(session_id) {
                                            h2 class="ledger-group-head" {
                                                "Session " span class="mono" { (session_id) }
                                                @if let Some(session) = facts.sessions.iter().find(|session| &session.id == session_id) {
                                                    span class="ledger-group-note" { (session.holder) }
                                                }
                                            }
                                            @for group in members {
                                                (ledger_entry(group, expanded.as_deref() == Some(group.id.as_str())))
                                            }
                                        }
                                    }
                                }
                                @if !ledger.loose.is_empty() {
                                    section class="ledger-unattributed" data-unknown=(loose_unknown) {
                                        h2 class="ledger-group-head" {
                                            "Rows with no transaction"
                                            span class="ledger-group-note" { (ledger.loose.len()) " row(s) in this window" }
                                        }
                                        p class="ledger-note" {
                                            "A row whose record carries no `txn` is not guessed into a transaction. An unclassifiable row is drawn as unknown and counted, never dropped."
                                        }
                                        @for step in &ledger.loose {
                                            (loose_row(step))
                                        }
                                    }
                                } @else if ledger.unknown > 0 {
                                    p class="ledger-note" {
                                        (ledger.unknown) " unknown row(s) in this window; they are shown where their transaction is."
                                    }
                                }
                            }
                        }
                    }
                    details class="transcript" {
                        summary { "The record, call by call" }
                        div
                            id="feed"
                        class="activity-feed"
                        data-state=(state)
                        data-boot=(page.boot)
                        data-next=(next)
                        data-truncated[page.truncated]
                    {
                        @if state == "unreadable" {
                            p class="activity-none" {
                                b { "The record is unreadable" }
                                "The device's audit could not be read."
                            }
                        } @else if state == "empty" {
                            p class="activity-none" {
                                b { "Waiting for the AI to act" }
                                "No call has been recorded on this device."
                            }
                        } @else {
                            @if page.truncated {
                                p class="activity-hint" {
                                    "The newest " (AUDIT_LINES) " lines; older calls are not shown."
                                }
                            }
                            @for turn in &turns {
                                section class="activity-turn" {
                                    p class="activity-turn-label" {
                                        time datetime=(turn.started) title=(turn.started) { (turn.label) }
                                    }
                                    @for row in &turn.rows {
                                        (row_markup(row))
                                    }
                                }
                            }
                        }
                        }
                    }
                }
                footer class="activity-foot" {
                    span { "the ledger is a fresh read · the call list polls every second" }
                    span class="ledger-window-note" {
                        "A line and its steps are the rows in this bounded window; a receipt's counts are the device's totals for that transaction."
                    }
                    span class="spacer" {}
                    span { kbd { "f" } " follow" }
                    span { "loopback only" }
                }
                script { (PreEscaped(VIEW_SCRIPT)) }
            }
        }
    }
}

/// One row, drawn from the facts [`rows`] computed. Every interpolation is escaped by `maud`.
fn row_markup(row: &ActivityRow) -> Markup {
    let class = format!("activity-row status-{}", row.status);
    let glyph_class = format!("activity-glyph status-{}", row.status);
    let status_class = format!("chip status-{}", row.status);
    let decision_class = format!("chip decision-{}", row.decision);
    let glyph = if row.running {
        "\u{2026}"
    } else if row.failed {
        "\u{2717}"
    } else {
        "\u{2713}"
    };

    html! {
        article
            class=(class)
            data-id=(row.id)
            data-seq=(row.seq)
            data-group=(row.group)
            data-status=(row.status)
            data-started=(row.at_ms)
            data-end-seq=(row.end_seq.map_or_else(|| "null".to_owned(), |seq| seq.to_string()))
        {
            div class="activity-line" {
                span class=(glyph_class) aria-hidden="true" { (glyph) }
                span class="activity-tool" { (row.tool) }
                @if !row.verb.is_empty() {
                    span class="activity-verb" { (row.verb) }
                }
                @if !row.decision.is_empty() {
                    span class=(decision_class) { (row.decision) }
                }
                span class=(status_class) { (row.status) }
                @if !row.args.is_empty() {
                    span class="activity-args" { (row.args) }
                }
                @if !row.outcome.is_empty() {
                    span class="chip outcome" { (row.outcome) }
                }
                span class="activity-duration" { (duration_text(row.duration_ms)) }
                time class="activity-time" datetime=(row.started) title=(row.started) {
                    (row.time)
                }
            }
            div class="activity-detail" {
                dl class="activity-kv" {
                    dt { "call" }
                    dd { "#" (row.id) }
                    dt { "decision" }
                    dd {
                        @if row.decision.is_empty() { "—" } @else { (row.decision) }
                    }
                    dt { "duration" }
                    dd class="activity-detail-duration" { (duration_text(row.duration_ms)) }
                    dt { "outcome" }
                    dd {
                        @if row.running { "running" } @else { (row.status) }
                        @if !row.outcome.is_empty() { " · " (row.outcome) }
                    }
                    @if !row.caller.is_empty() {
                        dt { "via" }
                        dd { (row.caller) }
                    }
                    @if !row.error.is_empty() {
                        dt { "error" }
                        dd { (row.error) }
                    }
                    @if !row.session.is_empty() {
                        dt { "session" }
                        dd { (row.session) }
                    }
                }
                @if !row.run_id.is_empty() {
                    details class="activity-run" data-run=(row.run_id) {
                        summary { "Run " (row.run_id) }
                        div class="activity-run-body" {}
                    }
                }
            }
        }
    }
}

/// A duration the way both Rust pages print it: milliseconds under a second, else seconds.
pub(crate) fn duration_text(duration_ms: u64) -> String {
    if duration_ms < 1_000 {
        format!("{duration_ms}ms")
    } else {
        format!("{:.1}s", duration_ms as f64 / 1_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const BOOT: u64 = 1_761_566_400_000;
    const NOW: u64 = BOOT + 600_000;

    fn transcript(audit: Vec<Value>, now: u64) -> ActivityFacts {
        ActivityFacts {
            audit: Some(audit),
            truncated: false,
            now_ms: now,
            boot: BOOT,
            panel_token: "0123456789abcdef".repeat(4),
            txns: Vec::new(),
            sessions: Vec::new(),
            selected_txn: None,
        }
    }

    fn begin(id: u64, at: u64, tool: &str, args: Option<&str>) -> Value {
        json!({
            "id": id,
            "ts_ms": at,
            "event": "begin",
            "tool": tool,
            "decision": "allow",
            "transport": "loopback",
            "args": args,
        })
    }

    fn end(id: u64, at: u64, ok: bool, run_id: Option<&str>) -> Value {
        json!({
            "id": id,
            "ts_ms": at,
            "event": "end",
            "ok": ok,
            "error_kind": if ok { None } else { Some("refused") },
            "transport": "loopback",
            "run_id": run_id,
        })
    }

    /// A transaction as the registry answers it, for a ledger that has a live record.
    fn txn(
        id: &str,
        purpose: &str,
        session: Option<&str>,
        state: &str,
        started: u64,
    ) -> panel::TxnFact {
        panel::TxnFact {
            id: id.to_owned(),
            purpose: purpose.to_owned(),
            author: "the install door".to_owned(),
            session: session.map(str::to_owned),
            state: state.to_owned(),
            reason: None,
            started_at_ms: started,
            ended_at_ms: (state != "working").then_some(started + 60_000),
            calls: 2,
            failures: 0,
            refusals: 0,
            duration_ms: 60_000,
            retained: true,
        }
    }

    fn session_facts(id: &str, holder: &str, alive: bool) -> panel::SessionFacts {
        panel::SessionFacts {
            kind: "pty".to_owned(),
            shell: "bash".to_owned(),
            id: id.to_owned(),
            holder: holder.to_owned(),
            goal: None,
            goal_by: None,
            open_txn: None,
            open_purpose: None,
            last_call: None,
            last_call_at_ms: None,
            alive,
        }
    }

    /// Facts for the ledger: the audit tail, the registry's transactions, the live sessions and
    /// the transaction `?txn` selected.
    fn ledger(
        audit: Vec<Value>,
        txns: Vec<panel::TxnFact>,
        sessions: Vec<panel::SessionFacts>,
        selected: Option<&str>,
    ) -> ActivityFacts {
        ActivityFacts {
            audit: Some(audit),
            truncated: false,
            now_ms: NOW,
            boot: BOOT,
            panel_token: "0123456789abcdef".repeat(4),
            txns,
            sessions,
            selected_txn: selected.map(str::to_owned),
        }
    }

    /// A begin line tagged with its transaction, as the device writes it for a call.
    fn tagged_begin(id: u64, at: u64, tool: &str, args: Option<&str>, txn: &str) -> Value {
        json!({
            "id": id,
            "ts_ms": at,
            "event": "begin",
            "tool": tool,
            "decision": "allow",
            "transport": "loopback",
            "args": args,
            "txn": txn,
        })
    }

    /// An end line tagged with its transaction.
    fn tagged_end(id: u64, at: u64, ok: bool, txn: &str) -> Value {
        json!({
            "id": id,
            "ts_ms": at,
            "event": "end",
            "ok": ok,
            "error_kind": if ok { None } else { Some("refused") },
            "transport": "loopback",
            "txn": txn,
        })
    }

    /// The `txn.begin` pair: its begin line carries the purpose, its end line the minted id.
    fn txn_begin(id: u64, at: u64, purpose: &str, txn: &str) -> [Value; 2] {
        [
            json!({
                "id": id,
                "ts_ms": at,
                "event": "begin",
                "tool": "txn.begin",
                "decision": "allow",
                "transport": "loopback",
                "args": format!("purpose={purpose}"),
            }),
            json!({
                "id": id,
                "ts_ms": at + 5,
                "event": "end",
                "ok": true,
                "outcome": "started",
                "transport": "loopback",
                "txn": txn,
            }),
        ]
    }

    /// One `<article>` row, for assertions about its own attributes.
    fn row(html: &str, id: u64) -> String {
        let marker = format!(r#"data-id="{id}""#);
        let at = html.find(&marker).expect("the row exists");
        let start = html[..at].rfind("<article").expect("the row opens");
        let close = "</article>";
        let end = html[start..].find(close).expect("the row closes") + start + close.len();

        html[start..end].to_owned()
    }

    #[test]
    fn the_transcript_page_renders_calls_and_turns_from_the_audit() {
        let at = BOOT + 1_000;
        let audit = vec![
            begin(1, at, "files.list", None),
            end(1, at + 100, true, None),
            begin(2, at + 10_000, "runs.exec", Some("echo one")),
            end(2, at + 10_400, true, Some("run-1")),
            // The begin is outside the bounded window: the end cannot become a call with a tool.
            end(3, at + 10_500, false, None),
        ];
        let mut facts = transcript(audit, at + 20_000);
        let html = page(&facts, Theme::Light).into_string();

        assert_eq!(
            html.matches(r#"class="activity-turn""#).count(),
            2,
            "two calls ten seconds apart are two turns: {html}"
        );
        let first = html
            .find(r#"data-id="1""#)
            .expect("the first call is a row");
        let second = html
            .find(r#"data-id="2""#)
            .expect("the second call is a row");
        assert!(first < second, "the rows are oldest first: {html}");
        assert!(!html.contains("timeline-day"), "no day groups: {html}");
        assert!(
            html.contains(r#"<span class="activity-tool">unknown</span>"#),
            "an orphan end is a row with no tool: {html}"
        );
        let orphan = row(&html, 3);
        assert!(
            orphan.contains(r#"data-group="other""#) && orphan.contains(r#"data-status="failed""#),
            "the orphan end is an other-group failed row: {orphan}"
        );
        assert!(
            !html.contains("data-truncated"),
            "an uncut tail says nothing about older lines: {html}"
        );

        facts.truncated = true;
        let cut = page(&facts, Theme::Light).into_string();
        assert!(
            cut.contains("data-truncated"),
            "a cut tail says older lines exist: {cut}"
        );

        let empty = page(&transcript(vec![], NOW), Theme::Light).into_string();
        assert!(
            empty.contains(r#"data-state="empty""#),
            "an audit with no lines is empty: {empty}"
        );
        assert!(empty.contains("No call has been recorded"));

        let mut unreadable = transcript(vec![], NOW);
        unreadable.audit = None;
        let unreadable = page(&unreadable, Theme::Light).into_string();
        assert!(
            unreadable.contains(r#"data-state="unreadable""#),
            "an audit that cannot be read is not empty: {unreadable}"
        );
        assert!(unreadable.contains("could not be read"));
    }

    #[test]
    fn the_transcript_page_escapes_the_tool_name_it_reads() {
        let at = BOOT + 1_000;
        let audit = vec![
            begin(1, at, "<img src=x onerror=alert(1)>", None),
            end(1, at + 100, true, None),
        ];
        let html = page(&transcript(audit, at + 200), Theme::Light).into_string();

        assert!(
            html.contains("&lt;img src=x onerror=alert(1)&gt;"),
            "the tool is escaped: {html}"
        );
        assert!(
            !html.contains("<img src=x"),
            "the raw attribute never reaches the page: {html}"
        );
    }

    #[test]
    fn the_activity_page_carries_the_tool_groups_and_the_counts() {
        let at = BOOT + 1_000;
        let audit = vec![
            begin(1, at, "runs.exec", Some("echo one")),
            end(1, at + 100, true, Some("run-1")),
            begin(2, at + 1_000, "runs.list", None),
            end(2, at + 1_100, false, None),
            begin(3, at + 2_000, "terminal.write", Some("session=session-1")),
            end(3, at + 2_100, true, None),
            begin(4, at + 3_000, "files.read", None),
            end(4, at + 3_100, true, None),
            begin(5, at + 4_000, "screen.capture", None),
            end(5, at + 4_100, true, None),
        ];
        let html = page(&transcript(audit, at + 5_000), Theme::Light).into_string();

        assert!(html.contains(r#"data-total="5""#), "five rows: {html}");
        assert!(
            html.contains(r#"data-failed="1""#),
            "one end says failed: {html}"
        );
        assert!(
            html.contains(r#"data-count-total="5""#) && html.contains(r#"data-count-failed="1""#),
            "the numbers the view applies to the header are server-rendered: {html}"
        );
        let ended = row(&html, 1);
        assert!(
            ended.contains(r#"data-end-seq=""#) && !ended.contains(r#"data-end-seq="null""#),
            "a completed row's page metadata carries its end sequence: {ended}"
        );
        for (group, count) in [("runs", 2), ("terminal", 1), ("files", 1), ("other", 1)] {
            assert!(
                html.contains(&format!(r#"data-group="{group}" data-count="{count}""#)),
                "the {group} count is {count}: {html}"
            );
        }
        for value in ["all", "runs", "terminal", "files", "other"] {
            assert!(
                html.contains(&format!(r#"<option value="{value}""#)),
                "the filter offers {value}: {html}"
            );
        }
    }

    #[test]
    fn the_activity_page_carries_the_light_and_dark_theme() {
        let light = page(&transcript(vec![], NOW), Theme::Light).into_string();
        assert!(
            light.contains(r#"<html lang="en" data-theme="light">"#),
            "the page defaults to the light theme: {light}"
        );
        assert!(
            light.contains(r#":root[data-theme="dark"]"#),
            "the shared tokens carry the dark theme: {light}"
        );

        let dark = page(&transcript(vec![], NOW), Theme::Dark).into_string();
        assert!(
            dark.contains(r#"<html lang="en" data-theme="dark">"#),
            "the query chooses the dark theme: {dark}"
        );
        assert!(dark.contains(r#":root[data-theme="dark"]"#));
        assert_eq!(Theme::from_query(Some("dark")), Theme::Dark);
        assert_eq!(Theme::from_query(Some("light")), Theme::Light);
        assert_eq!(Theme::from_query(None), Theme::Light);
    }
    #[test]
    fn a_begin_without_an_end_is_a_running_row() {
        let at = BOOT + 1_000;
        let audit = vec![begin(1, at, "runs.exec", Some("sleep 5"))];
        let calls = rows(&audit, at + 2_500);
        assert!(
            calls[0].end_seq.is_none(),
            "a running row has no end sequence"
        );
        let html = page(&transcript(audit, at + 2_500), Theme::Light).into_string();

        let running = row(&html, 1);
        assert!(
            running.contains(r#"data-status="running""#),
            "a begin with no end is running: {running}"
        );
        assert!(
            running.contains(r#"data-end-seq="null""#),
            "the page metadata says the row has no end yet: {running}"
        );
        assert!(
            running.contains(r#"class="chip status-running">running"#),
            "the row says running, never an outcome: {running}"
        );
        assert!(
            !running.contains("status-failed"),
            "a missing end is not a failure: {running}"
        );
        assert!(
            running.contains("2.5s"),
            "the duration measures to the read instant: {running}"
        );
    }

    /// The controller's running-B case: the first page shows B running. B then ends while C and D
    /// begin before the next poll. With a new-begin page size of two, that poll must return C and D
    /// and still carry B's late end; otherwise B stays "running" forever because D's begin moved
    /// the cursor past B's end.
    #[test]
    fn overlapping_calls_keep_the_next_page_and_deliver_a_late_end() {
        let at = BOOT + 1_000;

        // The audit as the first page saw it: A finished, B still running.
        let first_audit = vec![
            begin(1, at, "files.read", None),
            end(1, at + 100, true, None),
            begin(2, at + 1_000, "runs.exec", Some("slow")),
        ];
        let first_calls = rows(&first_audit, at + 1_500);
        let first = page_of(&first_calls, Some(0), 2, BOOT, false);
        assert_eq!(
            first.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            [1, 2],
            "the first page is A and the running B"
        );
        let b = first
            .rows
            .iter()
            .find(|row| row.id == 2)
            .expect("B is on the first page");
        assert!(b.running, "B is running on the first page");
        assert!(b.end_seq.is_none(), "B has no end yet");
        let after = first.next;
        assert_eq!(after, first_calls[1].seq, "next is B's begin");

        // The audit at the next poll: B ended late, after C and D had begun.
        let later_audit = vec![
            begin(1, at, "files.read", None),
            end(1, at + 100, true, None),
            begin(2, at + 1_000, "runs.exec", Some("slow")),
            begin(3, at + 2_000, "files.read", None),
            end(3, at + 2_100, true, None),
            begin(4, at + 3_000, "terminal.write", None),
            end(4, at + 3_100, true, None),
            end(2, at + 8_000, true, None),
        ];
        let calls = rows(&later_audit, at + 20_000);
        let second = page_of(&calls, Some(after), 2, BOOT, false);
        let ids: Vec<u64> = second.rows.iter().map(|row| row.id).collect();
        let new_begins = second.rows.iter().filter(|row| row.seq > after).count();
        assert_eq!(
            new_begins, 2,
            "limit bounds the new begins; C and D both fit: {ids:?}"
        );
        assert!(
            second.rows.len() > 2,
            "B's late end is outside the new-begin page size: {ids:?}"
        );
        assert!(
            ids.contains(&3) && ids.contains(&4),
            "C and D are not lost: {ids:?}"
        );
        let b = second
            .rows
            .iter()
            .find(|row| row.id == 2)
            .expect("B's late end is delivered");
        assert_eq!(b.status, "ok", "B is no longer running");
        assert!(
            b.end_seq.is_some_and(|end| end > after),
            "B's end is why the row is re-delivered"
        );
        assert_eq!(
            second.next, calls[3].seq,
            "the cursor advances to D's begin"
        );
        assert!(
            !second.more,
            "exactly the two new begins fit the new-begin page size"
        );
    }

    #[test]
    fn the_page_size_is_clamped_to_one_and_two_hundred() {
        let mut audit = Vec::with_capacity(500);
        for index in 0..250u64 {
            let at = BOOT + index * 1_000;
            audit.push(begin(index, at, "files.list", None));
            audit.push(end(index, at + 100, true, None));
        }
        let calls = rows(&audit, BOOT + 300_000);
        assert_eq!(calls.len(), 250);

        let newest_one = page_of(&calls, None, 0, BOOT, false);
        assert_eq!(newest_one.rows.len(), 1, "limit 0 clamps up to 1");
        assert_eq!(
            newest_one.rows[0].id, 249,
            "no cursor returns the newest rows"
        );
        assert!(newest_one.more, "older rows remain");

        let all = page_of(&calls, None, 999, BOOT, false);
        assert_eq!(all.rows.len(), 200, "limit 999 clamps down to 200");
        assert_eq!(all.rows.first().map(|row| row.id), Some(50));
        assert_eq!(all.rows.last().map(|row| row.id), Some(249));
        assert!(all.more, "the 50 older rows are behind the page");
        assert_eq!(all.counts.total, 250, "the counts cover the whole window");
    }

    #[test]
    fn the_activity_counts_say_they_are_the_window() {
        let html = page(&transcript(vec![], NOW), Theme::Light).into_string();

        assert!(
            html.contains(r#"data-window="last 200""#),
            "the counts carry their window: {html}"
        );
        assert!(
            html.contains(">last 200<"),
            "the header says the counts are the window, not the whole feed: {html}"
        );
        assert!(
            html.contains("These counts cover the newest 200 audit lines"),
            "the title explains the window: {html}"
        );
    }

    /// The ledger's one line per transaction, newest first, with the working one expanded.
    #[test]
    fn the_ledger_lists_one_line_per_transaction() {
        let at = BOOT + 10_000;
        let mut audit = Vec::new();
        audit.extend(txn_begin(10, at, "fix the parser", "txn-1"));
        audit.push(tagged_begin(
            11,
            at + 1_000,
            "terminal.run",
            Some("session=session-1"),
            "txn-1",
        ));
        audit.push(tagged_end(11, at + 2_000, true, "txn-1"));
        audit.extend(txn_begin(20, at + 30_000, "verify the suite", "txn-2"));
        audit.push(tagged_begin(21, at + 31_000, "files.list", None, "txn-2"));
        audit.push(tagged_end(21, at + 31_100, true, "txn-2"));
        let txns = vec![
            txn("txn-2", "verify the suite", None, "committed", at + 30_000),
            txn("txn-1", "fix the parser", Some("session-1"), "working", at),
        ];
        let html = page(&ledger(audit, txns, vec![], None), Theme::Light).into_string();

        assert_eq!(
            html.matches(r#"class="ledger-entry""#).count(),
            2,
            "one line per transaction: {html}"
        );
        assert!(
            html.contains("fix the parser") && html.contains("verify the suite"),
            "each line names its transaction's purpose: {html}"
        );
        assert!(
            html.contains(r#"href="?txn=txn-2""#),
            "a collapsed line links to its receipt: {html}"
        );
        assert_eq!(
            html.matches(r#"class="ledger-spread""#).count(),
            1,
            "one entry is expanded: {html}"
        );
    }

    #[test]
    fn a_device_scope_transaction_sits_in_a_machine_group_above_the_sessions() {
        let html = {
            let at = BOOT + 10_000;
            let mut audit = Vec::new();
            audit.extend(txn_begin(10, at, "bump the release pointer", "txn-1"));
            audit.extend(txn_begin(20, at + 30_000, "verify the suite", "txn-2"));
            let txns = vec![
                txn("txn-1", "bump the release pointer", None, "working", at),
                txn(
                    "txn-2",
                    "verify the suite",
                    Some("session-1"),
                    "working",
                    at + 30_000,
                ),
            ];
            page(
                &ledger(
                    audit,
                    txns,
                    vec![session_facts("session-1", "the operator", true)],
                    None,
                ),
                Theme::Light,
            )
            .into_string()
        };

        let machine = html
            .find(r#"class="ledger-machine""#)
            .expect("a machine group exists");
        let session = html
            .find(r#"class="ledger-session""#)
            .expect("a session group exists");
        let rail = html
            .find(r#"class="ledger-rail""#)
            .expect("the side rail exists");
        assert!(
            machine < session,
            "the machine group is above the session groups: {html}"
        );
        assert!(
            rail < machine,
            "the rail is drawn first, on the left of the ledger: {html}"
        );
        assert!(
            html[machine..session].contains("bump the release pointer"),
            "the device-scope transaction is in the machine group: {html}"
        );
    }

    #[test]
    fn the_working_transaction_is_the_expanded_entry() {
        let at = BOOT + 10_000;
        let audit = Vec::new();
        // The working record is older; the committed one is newer. Recency must not win.
        let txns = vec![
            txn("txn-2", "verify the suite", None, "committed", at + 30_000),
            txn("txn-1", "fix the parser", None, "working", at),
        ];
        let html = page(&ledger(audit, txns, vec![], None), Theme::Light).into_string();

        assert!(
            html.contains(r#"data-txn="txn-1" data-expanded="true""#),
            "the working transaction is expanded: {html}"
        );
        assert!(
            !html.contains(r#"data-txn="txn-2" data-expanded="true""#),
            "a committed transaction is not the default entry: {html}"
        );
    }

    #[test]
    fn a_query_transaction_selects_another_expanded_entry() {
        let at = BOOT + 10_000;
        let txns = vec![
            txn("txn-2", "verify the suite", None, "committed", at + 30_000),
            txn("txn-1", "fix the parser", None, "working", at),
        ];
        let html = page(
            &ledger(Vec::new(), txns, vec![], Some("txn-2")),
            Theme::Light,
        )
        .into_string();

        assert!(
            html.contains(r#"data-txn="txn-2" data-expanded="true""#),
            "?txn=txn-2 expands the named entry: {html}"
        );
        assert!(
            !html.contains(r#"data-txn="txn-1" data-expanded="true""#),
            "the working default yields to the query: {html}"
        );
    }

    #[test]
    fn an_expanded_entry_has_said_did_and_left() {
        let at = BOOT + 10_000;
        let mut audit = Vec::new();
        audit.extend(txn_begin(10, at, "fix the parser", "txn-1"));
        audit.push(tagged_begin(
            11,
            at + 1_000,
            "terminal.run",
            Some("session=session-1"),
            "txn-1",
        ));
        audit.push(tagged_end(11, at + 1_500, false, "txn-1"));
        let mut txns = vec![txn(
            "txn-1",
            "fix the parser",
            Some("session-1"),
            "working",
            at,
        )];
        txns[0].failures = 1;
        let html = page(&ledger(audit, txns, vec![], None), Theme::Light).into_string();

        assert!(html.contains(r#"class="said""#), "the claim column: {html}");
        assert!(
            html.contains(r#"class="claim-badge">CLAIM"#),
            "the purpose is badged as a claim: {html}"
        );
        assert!(
            html.contains("the install door"),
            "the claim names its author: {html}"
        );
        assert!(
            html.contains(r#"class="did""#),
            "the observed-steps column: {html}"
        );
        assert!(
            html.contains(r#"class="step""#) && html.contains("terminal.run"),
            "the steps are the audit's rows: {html}"
        );
        assert!(
            html.contains(r#"class="left""#),
            "the receipt column: {html}"
        );
        assert!(
            html.contains("No receipt yet") && html.contains("1 failure so far"),
            "a working transaction has no receipt, and its failure count is the window's: {html}"
        );
    }

    #[test]
    fn a_committed_receipt_does_not_hide_a_failure() {
        let at = BOOT + 10_000;
        let mut txns = vec![txn("txn-1", "publish the release", None, "committed", at)];
        txns[0].calls = 5;
        txns[0].failures = 1;
        txns[0].refusals = 2;
        let html = page(&ledger(Vec::new(), txns, vec![], None), Theme::Light).into_string();

        assert!(
            html.contains(">committed<"),
            "the ending is the declared state: {html}"
        );
        assert!(
            html.contains("1 failure") && html.contains("2 refused"),
            "the receipt reports the device's failures and refusals: {html}"
        );
        assert!(
            !html.contains("No receipt yet"),
            "a committed transaction has a receipt: {html}"
        );
    }

    #[test]
    fn the_terminal_is_a_link_not_a_pane() {
        let html = page(&ledger(Vec::new(), Vec::new(), vec![], None), Theme::Light).into_string();

        assert!(
            html.contains(r#"<a class="terminal-link" href="/panel#terminal">terminal ↗</a>"#),
            "the header links to the terminal's own page: {html}"
        );
        assert!(
            !html.contains("term-host") && !html.contains("xterm"),
            "the activity page embeds no terminal pane: {html}"
        );
    }

    #[test]
    fn the_side_rail_names_sessions_holders_and_the_legend() {
        let mut session = session_facts("session-1", "the operator", true);
        session.open_txn = Some("txn-1".to_owned());
        let html = page(
            &ledger(Vec::new(), Vec::new(), vec![session], None),
            Theme::Light,
        )
        .into_string();

        assert!(html.contains(r#"class="ledger-rail""#), "the rail: {html}");
        assert!(
            html.contains("session-1") && html.contains("the operator"),
            "the rail names the session and its holder: {html}"
        );
        assert!(
            html.contains(r#"class="session-dot live""#),
            "a live session carries the live dot: {html}"
        );
        assert!(
            html.contains("claim — what the AI says it is doing")
                && html.contains("fact — what the device observed"),
            "the rail's legend tells claims from facts: {html}"
        );
        assert!(
            html.contains(r#"data-record-rows=""#) && html.contains("bounded tail"),
            "the rail states the record's bound: {html}"
        );
    }

    #[test]
    fn an_unknown_row_is_visible_and_counted() {
        let at = BOOT + 10_000;
        let audit = vec![
            begin(1, at, "mystery.tool", None),
            end(1, at + 100, true, None),
        ];
        let html = page(&ledger(audit, vec![], vec![], None), Theme::Light).into_string();

        assert!(
            html.contains(r#"data-unknown="1""#),
            "the unknown row is counted: {html}"
        );
        assert!(
            html.contains(r#"class="ledger-unknown-row""#) && html.contains("mystery.tool"),
            "the unknown row stays visible: {html}"
        );
        assert!(
            html.contains("Rows with no transaction"),
            "the page says where an ungrouped row sits: {html}"
        );
    }

    /// The production shape of an orphan: `http::txn_facts` enumerates the audit's transaction ids
    /// and marks a registry-less one `retained: false`, with no counts and no purpose. The page must
    /// carry that flag instead of fabricating a receipt with zero calls, and must not guess the
    /// transaction's scope from a call's `session=` argument.
    #[test]
    fn an_orphan_group_says_receipt_not_retained() {
        let at = BOOT + 10_000;
        let mut audit = Vec::new();
        audit.extend(txn_begin(10, at, "read the installer's config", "txn-9"));
        audit.push(tagged_begin(
            11,
            at + 1_000,
            "terminal.run",
            Some("session=session-1"),
            "txn-9",
        ));
        audit.push(tagged_end(11, at + 1_100, true, "txn-9"));
        // The production shape: `txn_facts` joins the txn id to its `txn.begin` row in the same
        // tail, so the claim survives while the receipt does not.
        let mut orphan = txn("txn-9", "read the installer's config", None, "unknown", at);
        orphan.retained = false;
        orphan.calls = 0;
        orphan.failures = 0;
        orphan.refusals = 0;
        orphan.duration_ms = 0;
        let html = page(
            &ledger(
                audit,
                vec![orphan],
                vec![session_facts("session-1", "the operator", true)],
                None,
            ),
            Theme::Light,
        )
        .into_string();

        assert!(
            html.contains(r#"data-txn="txn-9""#) && html.contains(r#"data-retained="false""#),
            "the orphan group is drawn from the audit: {html}"
        );
        assert!(
            html.contains("receipt not retained"),
            "the orphan says its receipt is gone: {html}"
        );
        assert!(
            !html.contains("Committed.") && !html.contains("Aborted."),
            "a receipt the process does not hold declares no outcome: {html}"
        );
        assert!(
            !html.contains("0 calls · 0 failures"),
            "no count is invented for a missing receipt: {html}"
        );
        // The claim is retained in the tail even when the receipt is not: the purpose and its
        // author are shown, and only the receipt is missing.
        let ledger_start = html.find(r#"id="ledger""#).expect("the ledger is drawn");
        let ledger_end = html
            .find(r#"class="transcript""#)
            .expect("the call list follows the ledger");
        let ledger_html = &html[ledger_start..ledger_end];
        assert!(
            ledger_html.contains("read the installer's config")
                && ledger_html.contains("the install door")
                && !ledger_html.contains("(purpose not retained)"),
            "the claim from the audit's begin row keeps the header: {ledger_html}"
        );
        assert!(
            !html.contains(r#"data-group="machine""#) && html.contains(r#"data-group="unscoped""#),
            "an orphan's scope is unknown, so it is not guessed into Machine: {html}"
        );
    }

    /// A refused ending (`error_kind = "refused"`) is a failure the page says it observed; a printed
    /// "0 refused" beside it was a number nothing derived.
    #[test]
    fn a_refused_end_is_counted_in_the_window() {
        let at = BOOT + 10_000;
        let audit = vec![
            tagged_begin(
                11,
                at + 1_000,
                "terminal.run",
                Some("session=session-1"),
                "txn-1",
            ),
            tagged_end(11, at + 1_500, false, "txn-1"),
        ];
        let txns = vec![txn(
            "txn-1",
            "fix the parser",
            Some("session-1"),
            "working",
            at,
        )];
        let html = page(&ledger(audit, txns, vec![], None), Theme::Light).into_string();

        assert!(
            html.contains("Observed in this window: 1 calls · 1 failure · 1 refused"),
            "the refused end is counted where the page prints the window: {html}"
        );
    }

    /// A transaction's session is the registry record's own fact. A step's `session=` argument is the
    /// caller's flattened input, and it never moves a device-scope transaction into a session group.
    #[test]
    fn a_transactions_session_is_the_records_not_an_argument() {
        let at = BOOT + 10_000;
        let audit = vec![
            tagged_begin(
                11,
                at + 1_000,
                "terminal.run",
                Some("session=session-1"),
                "txn-1",
            ),
            tagged_end(11, at + 1_500, true, "txn-1"),
        ];
        let txns = vec![txn(
            "txn-1",
            "bump the release pointer",
            None,
            "working",
            at,
        )];
        let html = page(
            &ledger(
                audit,
                txns,
                vec![session_facts("session-1", "the operator", true)],
                None,
            ),
            Theme::Light,
        )
        .into_string();

        assert!(
            !html.contains(r#"data-group="session""#),
            "the argument does not place the transaction on the session: {html}"
        );
        assert!(
            html.contains(r#"class="ledger-machine""#),
            "the record's device scope wins: {html}"
        );
    }

    /// The design and the brief put the rail on the left, so it is emitted before the ledger and the
    /// grid draws it there.
    #[test]
    fn the_rail_is_drawn_once_on_the_left_of_the_ledger() {
        let html = page(&ledger(Vec::new(), Vec::new(), vec![], None), Theme::Light).into_string();
        assert_eq!(
            html.matches(r#"class="ledger-rail""#).count(),
            1,
            "the sessions, legend and record are drawn once, never duplicated beneath the ledger: {html}"
        );
        let rail = html
            .find(r#"class="ledger-rail""#)
            .expect("the rail is drawn");
        let ledger = html.find(r#"id="ledger""#).expect("the ledger is drawn");

        assert!(
            rail < ledger,
            "the rail precedes the ledger in the grid, which is what puts it on the left: {html}"
        );
    }

    /// A stale `?txn` link used to expand another transaction. The page says the named transaction is
    /// not in this window, and expands nothing.
    #[test]
    fn a_stale_query_transaction_says_it_is_not_in_this_window() {
        let at = BOOT + 10_000;
        let txns = vec![txn("txn-1", "fix the parser", None, "working", at)];
        let html = page(
            &ledger(Vec::new(), txns, vec![], Some("txn-gone")),
            Theme::Light,
        )
        .into_string();

        assert!(
            html.contains("txn-gone") && html.contains("is not in this window"),
            "the page names the missing transaction instead of falling back: {html}"
        );
        assert!(
            !html.contains(r#"data-expanded="true""#),
            "a stale link must not expand the working or newest transaction: {html}"
        );
    }

    /// The unattributed section's `data-unknown` is the count of the rows it lists, not the window's
    /// total (which `#ledger` carries).
    #[test]
    fn the_unattributed_count_is_the_rows_it_lists() {
        let at = BOOT + 10_000;
        let audit = vec![
            tagged_begin(11, at + 1_000, "mystery.tool", None, "txn-1"),
            tagged_end(11, at + 1_100, true, "txn-1"),
            begin(21, at + 2_000, "mystery.loose", None),
            end(21, at + 2_100, true, None),
            begin(22, at + 3_000, "files.list", None),
            end(22, at + 3_100, true, None),
        ];
        let txns = vec![txn("txn-1", "fix the parser", None, "working", at)];
        let html = page(&ledger(audit, txns, vec![], None), Theme::Light).into_string();

        assert!(
            html.contains(r#"data-transactions="1" data-unknown="2""#),
            "the window's total counts both unknown rows: {html}"
        );
        assert!(
            html.contains(r#"class="ledger-unattributed" data-unknown="1""#),
            "the unattributed section counts only its own row: {html}"
        );
    }

    #[test]
    fn an_empty_period_renders_an_empty_ledger() {
        let html = page(&ledger(Vec::new(), Vec::new(), vec![], None), Theme::Light).into_string();

        assert!(
            html.contains(r#"data-state="empty""#),
            "an audit with no lines is empty: {html}"
        );
        assert!(
            html.contains("No transaction has been recorded"),
            "the empty ledger says so: {html}"
        );
        assert!(
            !html.contains(r#"class="ledger-entry""#) && !html.contains("No receipt yet"),
            "an empty period invents no entry: {html}"
        );
    }
}
