//! The session's document, rendered by the device at `/activity`.
//!
//! The page is one session's work, not the machine's audit window: the goal is the title, that
//! session's transactions are the ledger, and everything else — the machine-scope and unreceipted
//! transactions, the rows with no transaction — sits in a collapsed secondary
//! section. The machine-wide counts are the header's quiet subtitle, and the call-by-call transcript
//! is a second collapsed disclosure. [`selected_session`] is the one rule that decides which session
//! the document is about; [`VIEW_SCRIPT`] still groups and counts nothing: every row carries the
//! group, status, turn and label this module decided, and the script only filters, follows and
//! expands a run through `/api/runs/:id`.
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
pub(crate) const STYLES: &str = r##"
  body.activity-page { font-size:var(--fs-md); line-height:var(--lh-body); font-variant-numeric:tabular-nums; }
  :where(body.activity-page) a { color:var(--accent); }
  :where(body.activity-page) a:hover { color:var(--accent-hover); }

  .activity-brand { display:flex; align-items:baseline; flex-wrap:wrap; gap:var(--sp-2); }
  .activity-brand .brand-name { font-family:var(--font); font-size:17px; font-weight:650; }
  .stem, .activity-release { color:var(--muted); font-size:12px; }
  .activity-release { display:flex; align-items:center; flex-wrap:wrap; gap:var(--sp-2); }
  #activity-state { font-size:13px; }
  .activity-head .spacer { flex:1; }
  .activity-pill, .release-state { display:inline-flex; align-items:center; gap:var(--sp-2); }
  .activity-pill { min-width:0; font-size:13px; }
  .dot, .session-dot { display:inline-block; flex:none; width:7px; height:7px; border-radius:50%; background:var(--muted); }
  .dot, .session-dot.live { background:var(--accent); }
  .host-name, .mono { font-family:var(--font-mono); overflow-wrap:anywhere; }
  .terminal-link { min-height:32px; display:inline-flex; align-items:center; font-size:13px; }
  a:focus-visible, button:focus-visible, select:focus-visible, summary:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  @media (forced-colors: active) { a:focus-visible, button:focus-visible, select:focus-visible, summary:focus-visible { outline: 2px solid CanvasText; } }

  .document { display:grid; gap:var(--sp-5); min-width:0; }
  .document > *, .session-work > *, .ledger-entry > * { min-width:0; }
  .ledger-rail { font-size:13px; }
  .rail-label { margin:0 0 var(--sp-3); color:var(--ink); font-size:var(--fs-lg); line-height:var(--lh-section); font-weight:600; }
  .session-selector { display:grid; gap:var(--sp-2); }
  .rail-session { display:flex; flex:0 1 auto; align-items:baseline; flex-wrap:wrap; gap:var(--sp-2); min-width:0; max-width:100%; padding:var(--sp-2) var(--sp-3); border:1px solid var(--line); border-radius:var(--radius-xs); background:var(--surface); }
  .rail-session:has([aria-current="page"]) { border-color:var(--accent); background:var(--accent-soft); }
  .rail-session-name { min-width:0; font-weight:600; overflow-wrap:anywhere; }
  .rail-session-name a { display:inline-block; min-height:32px; padding:4px 0; overflow-wrap:anywhere; }
  .rail-session-name a[aria-current="page"] { color:var(--accent-on-soft); }
  .rail-holder { max-width:100%; color:var(--muted); font-size:12px; overflow-wrap:anywhere; }
  .rail-session:has([aria-current="page"]) .rail-holder { color:var(--sidebar-muted); }
  .record-guide { margin-top:var(--sp-2); color:var(--muted); }
  .record-guide summary { font-size:13px; }
  .rail-note, .rail-none, .rail-record, .rail-legend { margin:var(--sp-2) 0; font-size:13px; color:var(--muted); }
  .session-work { min-width:0; }
  .goal { padding:0 0 var(--sp-5); border-bottom:1px solid var(--line); }
  .goal-label { margin:0 0 var(--sp-2); color:var(--muted); font-size:12px; font-weight:600; letter-spacing:.05em; }
  .goal-title { margin:0 0 var(--sp-3); font-family:var(--font); font-size:var(--fs-xl); font-weight:var(--fw-display); line-height:var(--lh-page); letter-spacing:-.025em; overflow-wrap:anywhere; }
  .goal-title.none, .goal-none, .goal-missing { margin:0 0 var(--sp-2); font-family:var(--font); font-size:13px; font-weight:500; line-height:1.55; }
  .goal-missing { color:var(--error); }
  .goal-meta { margin:0; color:var(--muted); font-size:13px; overflow-wrap:anywhere; }
  .goal-totals { margin:var(--sp-3) 0 0; font-size:13px; font-weight:500; }
  .goal-source { margin:var(--sp-2) 0 0; color:var(--muted); font-size:12px; }
  .ledger { margin-top:var(--sp-5); padding:var(--sp-5); border:1px solid var(--line); border-radius:var(--radius); background:var(--surface); }
  .ledger-group-head { margin:0 0 var(--sp-3); font-size:13px; color:var(--muted); font-weight:600; }
  .ledger-group-note { font-weight:400; overflow-wrap:anywhere; }
  .ledger-entry { padding:var(--sp-4) 0; border-bottom:1px solid var(--line); }
  .ledger-entry:last-child { border-bottom:0; }
  .ledger-entry[data-expanded="true"] { padding-top:0; }
  .ledger-line { display:flex; align-items:baseline; flex-wrap:wrap; gap:var(--sp-2); min-width:0; }
  .ledger-time { color:var(--muted); font-family:var(--font-mono); font-size:12px; }
  .ledger-purpose { margin:0; flex:1 1 16rem; min-width:0; font-family:var(--font); font-size:var(--fs-purpose); font-weight:600; line-height:1.45; overflow-wrap:anywhere; }
  span.ledger-purpose { font-size:13px; }
  .ledger-meta { flex:1 1 100%; color:var(--muted); font-size:12px; overflow-wrap:anywhere; }
  .ledger-receipt-link { display:inline-block; min-height:32px; padding:4px 0; font-size:13px; }
  .chip { display:inline-flex; align-items:center; padding:2px var(--sp-2); border:1px solid var(--line); border-radius:var(--radius-xs); font-size:13px; overflow-wrap:anywhere; }
  .chip.status-failed, .chip.decision-deny, .state-aborted { color:var(--error); background:var(--error-soft); }
  .chip.status-ok, .state-committed { color:var(--accent-on-soft); background:var(--accent-soft); }
  .chip.status-running, .state-working { color:var(--muted); background:var(--panel-2); }
  .ledger-spark { display:inline-flex; gap:3px; }
  .ledger-spark i { display:block; width:5px; height:5px; border-radius:50%; background:var(--accent); }
  .ledger-spark .bad { background:var(--error); }
  .ledger-spark .work { background:var(--muted); }
  .current-refusal { margin:var(--sp-3) 0; padding:var(--sp-3); border-left:2px solid var(--error); background:var(--error-soft); color:var(--error); font-size:13px; }
  .work-observation { margin-top:var(--sp-3); }
  .effects, .observed { margin:var(--sp-2) 0; color:var(--muted); font-size:13px; }
  .effects-label { display:block; color:var(--ink); font-weight:600; margin-bottom:var(--sp-1); }
  .work-evidence { margin-top:var(--sp-3); }
  .work-evidence > summary { color:var(--accent); font-size:13px; }
  .ledger-spread { display:grid; grid-template-columns:minmax(0,1fr); gap:var(--sp-5); margin-top:var(--sp-4); }
  .said, .did, .left { min-width:0; }
  .ledger-col-label { margin:0 0 var(--sp-2); font-size:13px; font-weight:600; }
  .claim-text, .claim-by, .claim-note { margin:var(--sp-2) 0; font-size:var(--fs-md); overflow-wrap:anywhere; }
  .claim-by, .claim-note { color:var(--muted); }
  .ledger-steps { display:grid; gap:var(--sp-2); }
  .step { min-width:0; padding:var(--sp-3); background:var(--panel-2); border:1px solid var(--line); border-radius:var(--radius-xs); }
  .step[data-status="failed"] { border-left:2px solid var(--error); }
  .step[data-kind="unknown"] { border-style:dashed; }
  .step-tool, .step-args { font-family:var(--font-mono); font-size:13px; overflow-wrap:anywhere; }
  .step-args, .step-outcome { margin-top:var(--sp-1); color:var(--muted); font-size:13px; }
  .receipt { border-left:2px solid var(--line-strong); padding-left:var(--sp-3); font-size:13px; }
  .receipt-big { font-family:var(--font); font-size:13px; font-weight:600; }
  .receipt-declared, .receipt-observed, .receipt-drift { display:flex; flex-wrap:wrap; align-items:baseline; gap:var(--sp-2); margin:0 0 var(--sp-2); }
  .receipt-label, .receipt-drift-label { font-size:13px; color:var(--muted); }
  .receipt-summary, .receipt-dim, .receipt-pending, .receipt-reason { margin:var(--sp-2) 0; font-size:13px; overflow-wrap:anywhere; }
  .receipt-dim { color:var(--muted); }
  .receipt-pending, .receipt-drift { color:var(--error); font-size:13px; }
  .work-history, .older-activity, .transcript { padding:var(--sp-4) var(--sp-5); border:1px solid var(--line); border-radius:var(--radius); background:var(--surface); }
  .work-history > summary, .older-activity > summary, .transcript > summary { color:var(--ink); font-size:13px; font-weight:600; }
  .work-history { margin-top:var(--sp-5); padding:var(--sp-3) 0 0; background:transparent; border-width:1px 0 0; border-radius:0; }
  .older-why, .ledger-note, .ledger-none, .ledger-empty, .ledger-missing { color:var(--muted); font-size:13px; }
  .ledger-missing { padding:var(--sp-4) var(--sp-5); margin:0; }
  .ledger-empty b { display:block; }
  .ledger-unknown-row { display:flex; flex-wrap:wrap; gap:var(--sp-2); padding:var(--sp-3) 0; border-bottom:1px solid var(--line); }
  .ledger-tool, .ledger-args { font-family:var(--font-mono); font-size:13px; overflow-wrap:anywhere; }
  .ledger-outcome { color:var(--muted); font-size:13px; }
  .activity-machine { margin:var(--sp-2) 0 var(--sp-4); color:var(--muted); font-size:12px; }
  .activity-toolbar { display:flex; flex-wrap:wrap; align-items:center; gap:var(--sp-3); margin:var(--sp-3) 0; }
  .activity-count { font-size:12px; color:var(--muted); }
  .activity-filter { min-height:32px; background:var(--surface); border:1px solid var(--line-strong); padding:0 var(--sp-2); }
  .activity-turn { margin:var(--sp-4) 0; }
  .activity-turn-label { margin:0 0 var(--sp-2); font-size:12px; color:var(--muted); }
  .activity-row { padding:var(--sp-3) 0; border-top:1px solid var(--line); min-width:0; }
  .activity-line { display:flex; flex-wrap:wrap; align-items:baseline; gap:var(--sp-2); min-width:0; }
  .activity-tool, .activity-args { font-family:var(--font-mono); font-size:13px; overflow-wrap:anywhere; }
  .activity-args { flex:1 1 100%; color:var(--muted); }
  .activity-verb, .activity-duration, .activity-time { font-size:12px; color:var(--muted); }
  .activity-glyph.status-failed { color:var(--error); }
  .activity-detail { margin-top:var(--sp-2); }
  .activity-kv { display:grid; grid-template-columns:6rem minmax(0,1fr); gap:var(--sp-1) var(--sp-3); margin:var(--sp-2) 0; font-size:13px; }
  .activity-kv dt { color:var(--muted); }
  .activity-kv dd { margin:0; font-family:var(--font-mono); overflow-wrap:anywhere; }
  .activity-run summary { color:var(--accent); font-size:13px; }
  .activity-preview { padding:var(--sp-3); background:var(--panel-2); border:1px solid var(--line); border-radius:var(--radius-xs); white-space:pre-wrap; overflow-wrap:anywhere; font-size:13px; }
  .activity-preview-label { font-size:12px; color:var(--muted); }
  .activity-none, .activity-hint { margin:var(--sp-4) 0; color:var(--muted); font-size:13px; }
  .activity-none b { display:block; color:var(--ink); }
  .activity-foot { display:flex; flex-wrap:wrap; gap:var(--sp-3); padding:var(--sp-4) var(--device-gutter); border-top:1px solid var(--line); color:var(--muted); font-size:12px; }
  .activity-foot .spacer { flex:1; }
  kbd { padding:1px 4px; border:1px solid var(--line-strong); border-radius:3px; font-size:12px; }
  @media (max-width:900px) { .ledger-spread { grid-template-columns:1fr; } }
  @media (max-width:600px) {
    .activity-head { gap:var(--sp-2); }
    .activity-head .device-nav { flex-basis:100%; }
    .activity-head .spacer { display:none; }
    .activity-release { flex-basis:100%; }
    .activity-main { padding-top:var(--sp-4); }
    .ledger, .older-activity, .transcript, .ledger-missing { padding:var(--sp-4); }
    .rail-session { flex-basis:100%; }
    .ledger-purpose { flex-basis:100%; }
    .activity-kv { grid-template-columns:4.5rem minmax(0,1fr); }
  }
"##;

/// The page's only script: poll, append/update, filter, follow and run expand.
pub(crate) const VIEW_SCRIPT: &str = r##"(function () {
  "use strict";
  var feed = document.getElementById("feed");
  if (!feed) return;
  var stateLine = document.getElementById("activity-state");
  var filter = document.getElementById("activity-filter");
  var meta = document.querySelector('meta[name="summrise-panel"]');
  var token = meta ? meta.getAttribute("content") : "";
  var boot = feed.getAttribute("data-boot") || "";
  var cursor = feed.getAttribute("data-next") || "";
  var follow = true;
  var polling = false;
  var failures = 0;
  var reloading = false;
  var seen = new Map();
  var active = true;
  var viewUrl = window.location.href.split("#")[0];
  var pollTimer;
  var durationTimer;
  function currentView() {
    return active && !reloading && feed.isConnected && window.location.href.split("#")[0] === viewUrl;
  }
  window.addEventListener("pagehide", function () {
    active = false;
    window.clearInterval(pollTimer);
    window.clearInterval(durationTimer);
  });
  window.addEventListener("pageshow", function (event) {
    if (event.persisted) reloadPage();
  });

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
    if (!currentView() || !body.isConnected) return;
    try {
      var response = await fetch("/api/runs/" + encodeURIComponent(id), {
        headers: { "x-summrise-panel": token },
        cache: "no-store"
      });
      if (!currentView() || !body.isConnected) return;
      if (response.status === 401 || response.status === 403) {
        reloadPage();
        return;
      }
      if (!response.ok) throw new Error("status " + response.status);
      var answer = await response.json();
      if (!currentView() || !body.isConnected) return;
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
      if (!currentView() || !body.isConnected) return;
      body.textContent = "run " + id + " unavailable (" + error.message + ")";
    }
  }

  // The call list is a disclosure at the document's end. Following its tail only makes sense while
  // that disclosure is open: a closed list must never drag the reader away from the goal.
  function following() {
    var details = feed.closest ? feed.closest("details") : null;
    return follow && (!details || details.open);
  }

  async function tick() {
    if (!currentView() || polling || !cursor || feed.getAttribute("data-state") === "unreadable") return;
    polling = true;
    try {
      var response = await fetch("/api/activity?after=" + encodeURIComponent(cursor) + "&limit=200", {
        headers: { "x-summrise-panel": token },
        cache: "no-store"
      });
      if (!currentView()) return;
      if (response.status === 401 || response.status === 403) {
        reloadPage();
        return;
      }
      if (!response.ok) throw new Error("status " + response.status);
      var answer = await response.json();
      if (!currentView()) return;
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
      if (following()) window.scrollTo(0, document.body.scrollHeight);
    } catch (error) {
      if (!currentView()) return;
      failures += 1;
      if (failures >= 3) {
        reloadPage();
        return;
      }
      if (stateLine) stateLine.textContent = "stale · retrying";
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
    pollTimer = window.setInterval(function () { void tick(); }, 1000);
  }
  durationTimer = window.setInterval(function () {
    if (currentView()) refreshRunningDurations();
  }, 1000);
})();"##;

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
    /// The session `?session=<id>` names, before [`selected_session`] checks it against the record.
    pub(crate) selected_session: Option<String>,
    /// This machine's own name, read from the device's environment (`COMPUTERNAME`, `HOSTNAME`), or
    /// `this machine` when the process cannot read one. It is never guessed from a client's report.
    pub(crate) host: String,
    /// The running release's version: the launcher's signed facts, never `CARGO_PKG_VERSION`.
    pub(crate) version: String,
    /// The release channel the booted sidecar named.
    pub(crate) channel: String,
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
    /// A policy refusal, or a legacy non-process capability refusal. An ambiguous historical
    /// `runs.exec` refusal does not establish a policy refusal or whether a process ran.
    refused: bool,
    process: ProcessExecution,
    unknown: bool,
}

#[derive(Clone, Copy, Debug)]
enum ProcessExecution {
    Ran,
    NotRun,
    Unknown,
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

    /// The receipt as the drift rule may read it: the declared ending and the device's own counts.
    ///
    /// A receipt this process does not hold declared no ending, so `retained` travels with the
    /// reading and the rule returns no sentence for it.
    fn reading(&self) -> ReceiptReading<'_> {
        ReceiptReading {
            state: &self.state,
            calls: self.calls,
            failures: self.failures,
            refusals: self.refusals,
            retained: self.retained,
        }
    }
}

/// What the drift sentence may read: the declared ending and the device's own counts.
///
/// The receipt also carries a purpose, a reason and the goals at its two ends. Those are text its
/// author wrote, and the sentence is arithmetic over what the device observed, so they are not on
/// this type: there is nothing here for a later edit to read. `reason` is drawn beside the receipt
/// in its own words and is never reworded into the hint.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ReceiptReading<'a> {
    pub(crate) state: &'a str,
    pub(crate) calls: u64,
    pub(crate) failures: u64,
    pub(crate) refusals: u64,
    /// Whether this process still holds the receipt. An orphan declared no ending to compare.
    pub(crate) retained: bool,
}

/// The one drift rule: the declared ending against the calls the device observed.
///
/// `None` is the normal case — a clean commit, a working transaction, an abort over a failure or a
/// refusal, and a receipt this process no longer holds. The sentence names the ending and counts
/// only the side that disagrees; it never repeats the receipt's total and never says what the
/// author meant.
pub(crate) fn drift(group: &ReceiptReading<'_>) -> Option<String> {
    if !group.retained {
        return None;
    }
    match group.state {
        "committed" if group.failures > 0 => Some(format!(
            "Committed over {}.",
            call_noun(group.failures, "failed")
        )),
        "committed" if group.refusals > 0 => Some(format!(
            "Committed over {}.",
            call_noun(group.refusals, "refused")
        )),
        "aborted" if group.failures == 0 && group.refusals == 0 && group.calls > 0 => {
            Some(format!(
                "Aborted after {} — the ending is the author's, not the record's.",
                call_noun(group.calls, "clean")
            ))
        }
        "committed" | "aborted" if group.calls == 0 => {
            Some("Ended with no calls recorded.".to_owned())
        }
        _ => None,
    }
}

/// `1 failed call`, `2 failed calls`: the drift sentence's adjective travels with the noun.
fn call_noun(count: u64, adjective: &str) -> String {
    count_noun(count, &format!("{adjective} call"))
}

/// The ledger's answer from one read: the transaction groups and the rows that rode no
/// transaction. An unclassifiable row stays visible in the section that draws it, counted there.
struct Ledger {
    groups: Vec<LedgerGroup>,
    loose: Vec<LedgerStep>,
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

    let mut groups: Vec<LedgerGroup> = groups.into_values().collect();
    groups.sort_by(|left, right| {
        right
            .started_at_ms
            .cmp(&left.started_at_ms)
            .then_with(|| right.id.cmp(&left.id))
    });

    Ledger { groups, loose }
}

/// One call pair as a ledger step, with the audit's own outcome and duration.
fn ledger_step(tool: &str, begin: &Value, end: Option<&Value>, now_ms: u64) -> LedgerStep {
    let at_ms = begin.get("ts_ms").and_then(Value::as_u64).unwrap_or(0);
    // Policy denies and unanswered approvals never dispatch. A parser failure also never starts a
    // process, but is an admitted failure rather than a policy refusal. Older runs used `refused`
    // for both spawn errors and executed failures, so their execution must remain unknown.
    let decision = begin.get("decision").and_then(Value::as_str);
    let denied = matches!(decision, Some("deny" | "ask"));
    let error_kind = end.map(|end| text(end, "error_kind")).unwrap_or_default();
    let process = if tool != "runs.exec" || denied {
        ProcessExecution::NotRun
    } else {
        match end {
            Some(end) if end.get("ok").and_then(Value::as_bool) == Some(true) => {
                ProcessExecution::Ran
            }
            Some(_) => match error_kind.as_str() {
                "executed_failure" => ProcessExecution::Ran,
                "arguments" | "run_not_started" | "approval_required" => ProcessExecution::NotRun,
                _ => ProcessExecution::Unknown,
            },
            None if decision == Some("allow") => ProcessExecution::Ran,
            None => ProcessExecution::Unknown,
        }
    };
    let (status, duration_ms, outcome, refused) = match end {
        Some(end) => {
            let ok = end.get("ok").and_then(Value::as_bool).unwrap_or(false);
            let end_ms = end.get("ts_ms").and_then(Value::as_u64).unwrap_or(at_ms);
            (
                if ok { "ok" } else { "failed" },
                end_ms.saturating_sub(at_ms),
                text(end, "outcome"),
                denied || (!ok && error_kind == "refused" && tool != "runs.exec"),
            )
        }
        None => (
            "running",
            now_ms.saturating_sub(at_ms),
            String::new(),
            denied,
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
        process,
        unknown: kind == summrise_core::RowKind::Unknown,
    }
}

/// The ledger's one expanded entry: `?txn` when it is among `groups`, else the working transaction,
/// else the newest. A `?txn` the given groups do not hold expands nothing: a stale link must not
/// show another transaction as if it were the named one.
fn expanded_id(facts: &ActivityFacts, groups: &[&LedgerGroup]) -> Option<String> {
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

/// The session the document is about: the query's when the record knows it, else the live session
/// with an open transaction, else the newest live session, else the newest session the ledger names.
///
/// One rule, five steps. A `?session` the record does not know selects nothing: the caller says it
/// is not in this record rather than substituting another session for it.
pub(crate) fn selected_session(facts: &ActivityFacts) -> Option<String> {
    if let Some(id) = &facts.selected_session {
        return record_names_session(facts, id).then(|| id.clone());
    }
    if let Some(session) = newest_live_session(facts, true) {
        return Some(session.id.clone());
    }
    if let Some(session) = newest_live_session(facts, false) {
        return Some(session.id.clone());
    }
    facts
        .txns
        .iter()
        .filter(|txn| txn.session.is_some())
        .max_by(|left, right| {
            left.started_at_ms
                .cmp(&right.started_at_ms)
                .then_with(|| left.id.cmp(&right.id))
        })
        .and_then(|txn| txn.session.clone())
}

/// Whether the record knows a session: a live session's own id, or a ledger group naming it.
fn record_names_session(facts: &ActivityFacts, id: &str) -> bool {
    facts.sessions.iter().any(|session| session.id == id)
        || facts
            .txns
            .iter()
            .any(|txn| txn.session.as_deref() == Some(id))
}

/// The live session with the newest `last_call_at_ms`; with `working_only`, one that has an open
/// transaction. A session with no recorded call is the oldest, and the id breaks the tie.
fn newest_live_session(facts: &ActivityFacts, working_only: bool) -> Option<&panel::SessionFacts> {
    facts
        .sessions
        .iter()
        .filter(|session| session.alive)
        .filter(|session| !working_only || session.open_txn.is_some())
        .max_by(|left, right| {
            left.last_call_at_ms
                .unwrap_or(0)
                .cmp(&right.last_call_at_ms.unwrap_or(0))
                .then_with(|| left.id.cmp(&right.id))
        })
}

/// The one plural rule: `1 transaction`, `2 transactions`; `1 row`, `73 rows`.
///
/// `singular` is the whole noun phrase, so an adjective travels with it: `count_noun(1, "failed
/// call")` is `1 failed call`. Every count the page prints goes through this function; the effects
/// line is the one deliberate exception, because it names the label first and the count after it.
fn count_noun(count: u64, singular: &str) -> String {
    format!("{count} {}", plural_noun(count, singular))
}

/// The noun alone, plural unless the count is one: `call`/`calls`, `row`/`rows`.
///
/// For a count site that already prints its number in its own element — the machine-wide line's
/// `<b data-count-total>`, which the view script rewrites — the noun must not carry a second copy.
fn plural_noun(count: u64, singular: &str) -> String {
    if count == 1 {
        singular.to_owned()
    } else {
        format!("{singular}s")
    }
}

/// The work one transaction's steps did, as its expanded entry's effects line prints it.
#[derive(Clone, Copy, Debug, Default)]
struct Effects {
    /// `files.write` calls that ended `ok`: a failed write wrote nothing.
    files_written: usize,
    /// Successful or typed executed-failure `runs.exec` calls, plus allowed calls still running
    /// under the accepted provisional contract.
    processes_run: usize,
    /// Historical ambiguous failures and unclassified outcomes cannot establish execution.
    processes_unknown: usize,
    /// `forward.add` calls that ended `ok`: a forwarding rule was recorded. This does not prove
    /// its listener bound or is reachable. `monitor.add` is never counted here.
    ports: usize,
    /// `browser.open` calls that ended `ok`: one window per successful open.
    windows: usize,
}

/// Count a transaction's effects over its own steps — the rows its expanded entry shows in Did.
///
/// `files.read` is not an effect (a read leaves nothing), and `terminal.*` calls are session I/O,
/// not processes: neither takes a branch. The line prints all four, zero included.
fn effects(steps: &[LedgerStep]) -> Effects {
    let mut effects = Effects::default();
    for step in steps {
        match step.tool.as_str() {
            "files.write" if step.status == "ok" => effects.files_written += 1,
            "runs.exec" => match step.process {
                ProcessExecution::Ran => effects.processes_run += 1,
                ProcessExecution::Unknown => effects.processes_unknown += 1,
                ProcessExecution::NotRun => {}
            },
            "forward.add" if step.status == "ok" => effects.ports += 1,
            "browser.open" if step.status == "ok" => effects.windows += 1,
            _ => {}
        }
    }
    effects
}

/// The selected session's four numbers, under the goal: its transactions, the receipts' calls and
/// refusals, and the successful writes its own steps show in this window.
struct SessionTotals {
    transactions: usize,
    calls: u64,
    refusals: u64,
    files_written: usize,
}

/// Sum the selected session's groups. Calls and refusals are each retained receipt's own totals;
/// a group without a retained receipt contributes only what this window observed.
fn session_totals(groups: &[&LedgerGroup]) -> SessionTotals {
    let mut totals = SessionTotals {
        transactions: groups.len(),
        calls: 0,
        refusals: 0,
        files_written: 0,
    };
    for group in groups {
        if group.retained {
            totals.calls += group.calls;
            totals.refusals += group.refusals;
        } else {
            // The plan pins this window fallback as the missing-receipt contract, even though
            // today's orphan groups carry no session and so never reach it.
            totals.calls += group.window_calls as u64;
            totals.refusals += group.window_refusals as u64;
        }
        totals.files_written += effects(&group.steps).files_written;
    }
    totals
}

/// The goal block: the document's title.
///
/// A session with a goal gets it as the title with its author; a session without
/// one says so in the body face; and a machine with no session says which absence it is — never a
/// transaction's purpose promoted to a goal. A selected session also gets its totals under the goal
/// meta: the four numbers its own transactions, receipts and writes add up to.
fn goal_block(
    facts: &ActivityFacts,
    selected: Option<&str>,
    missing: Option<&str>,
    totals: Option<&SessionTotals>,
) -> Markup {
    let live = selected.and_then(|id| facts.sessions.iter().find(|session| session.id == id));
    let goal = live.and_then(|session| session.goal.as_deref());
    let state = if selected.is_none() {
        "no-session"
    } else if goal.is_some() {
        "set"
    } else if live.is_none() {
        "unavailable"
    } else {
        "none"
    };
    html! {
        section
            class="goal"
            data-goal=(state)
            data-session=[selected]
            data-missing-session=[missing]
        {
            @if state == "no-session" {
                (no_session_line(facts, missing))
            } @else if let Some(goal) = goal {
                p class="goal-label" { "GOAL OF THE SESSION" }
                h1 class="goal-title" { (goal) }
                p class="goal-meta" {
                    "Goal"
                    @if let Some(by) = live.and_then(|session| session.goal_by.as_deref()) {
                        " set by " (by)
                    }
                    @if let Some(session) = live {
                        " · " (session.kind) " " (session.id) " · "
                        (if session.alive { "live" } else { "not live" })
                        " · holder " (session.holder)
                    }
                }
                p class="goal-source" data-text-role="meta" { "An authored session goal; observed calls and receipt totals appear below." }
            } @else {
                h1 class="goal-title none" {
                    @if state == "unavailable" {
                        "The goal for this session is not retained."
                    } @else {
                        "No goal set for this session."
                    }
                }
                @if let Some(session) = live {
                    p class="goal-meta" {
                        (session.kind) " " (session.id) " · "
                        (if session.alive { "live" } else { "not live" })
                        " · holder " (session.holder)
                    }
                } @else if let Some(id) = selected {
                    p class="goal-meta" { "session " (id) " · not live" }
                }
            }
            @if let Some(totals) = totals {
                p
                    class="goal-totals"
                    title="calls and refusals are the receipts' totals; files written are the successful `files.write` calls in this window."
                {
                    (count_noun(totals.transactions as u64, "transaction")) " · "
                    (count_noun(totals.calls, "call")) " · "
                    (totals.refusals) " refused · "
                    (count_noun(totals.files_written as u64, "file")) " written"
                }
                p class="goal-source" data-text-role="meta" {
                    "Calls and refusals are receipt totals; files written are successful writes observed in this window."
                }
            }
        }
    }
}

/// The four no-session states, each said: an unknown `?session`, an unreadable record, a record
/// with rows that names no session, and a machine where nothing has been run at all.
///
/// The statement is the document's `h1`, so the outline always starts with what the page is about —
/// the goal, or the honest reason there is none — and never with the ledger's `h2`.
fn no_session_line(facts: &ActivityFacts, missing: Option<&str>) -> Markup {
    let has_rows = facts
        .audit
        .as_deref()
        .is_some_and(|lines| !lines.is_empty());
    html! {
        @if let Some(id) = missing {
            h1 class="goal-missing" { "Session " (id) " is not in this record." }
        } @else if facts.audit.is_none() {
            h1 class="goal-none" { "The record is unreadable." }
        } @else if has_rows {
            h1 class="goal-none" {
                "No session on this machine — the work in this record names no session."
            }
        } @else {
            h1 class="goal-none" { "No session on this machine — nothing has been run yet." }
        }
    }
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

/// The receipt link's target: the transaction, and the session the document is about when one is
/// selected. Without the session, reloading re-runs [`selected_session`] and can land on another
/// session's document, so the named transaction would be neither drawn nor said to be missing.
fn receipt_href(txn: &str, selected: Option<&str>) -> String {
    match selected {
        Some(session) => format!("?session={session}&txn={txn}"),
        None => format!("?txn={txn}"),
    }
}

/// One transaction's line and keyboard-accessible evidence. `selected` keeps receipt links in
/// the same session; an explicit receipt deep link reveals that entry's evidence.
fn ledger_entry(
    group: &LedgerGroup,
    expanded: bool,
    selected: Option<&str>,
    reveal: bool,
) -> Markup {
    let state = ledger_state(group);
    let state_class = format!("chip ledger-chip state-{state}");
    let failures = if group.retained {
        group.failures
    } else {
        group.window_failures as u64
    };
    let refusals = if group.retained {
        group.refusals
    } else {
        group.window_refusals as u64
    };
    html! {
        article
            class="ledger-entry"
            data-txn=(group.id)
            data-expanded=(if expanded { "true" } else { "false" })
            data-state=(state)
            data-retained=(if group.retained { "true" } else { "false" })
        {
            div class="ledger-line" {
                time class="ledger-time" datetime=(group.started_utc()) title=(group.started_utc()) { (group.start_time()) }
                @if expanded {
                    h2 class="ledger-purpose" { (group.purpose_or_unknown()) }
                } @else {
                    span class="ledger-purpose" { (group.purpose_or_unknown()) }
                }
                span class=(state_class) { (state) }
                span class="ledger-spark" aria-hidden="true" {
                    @for step in &group.steps { i class=(step.spark_class()) {} }
                }
                span class="ledger-meta" {
                    @if group.retained {
                        (group.duration_text()) " · " (count_noun(group.calls, "call")) " · "
                        (count_noun(failures, "failure")) " · " (refusals) " refused · "
                        a class="ledger-receipt-link" href=(receipt_href(&group.id, selected)) { "receipt →" }
                    } @else {
                        (count_noun(group.window_calls as u64, "call"))
                        " in this window · receipt not retained · "
                        a class="ledger-receipt-link" href=(receipt_href(&group.id, selected)) { "Open recorded work →" }
                    }
                }
            }
            @if expanded && refusals > 0 {
                p class="current-refusal" { (refusals) " refused · " (count_noun(failures, "failure")) " recorded for this work." }
            }
            @if expanded {
                (work_observation(group))
            }
            details class="work-evidence" open[reveal] {
                summary { "Evidence — Said, Did, Left" }
                @if !expanded { (work_observation(group)) }
                (ledger_spread(group))
            }
        }
    }
}

/// Display the existing Rust effects and counts without interpreting generic success as proof.
fn work_observation(group: &LedgerGroup) -> Markup {
    let counted = effects(&group.steps);
    html! {
        div class="work-observation" {
            p class="effects-label" { "Observed in this work" }
            p class="effects"
                data-effects=(format!("files:{} processes:{} ports:{} windows:{}", counted.files_written, counted.processes_run, counted.ports, counted.windows))
                title="counted from the calls shown in this window"
            {
                "files " (counted.files_written) " written · processes " (counted.processes_run)
                @if counted.processes_unknown > 0 {
                    " confirmed run (" (counted.processes_unknown) " execution" (if counted.processes_unknown == 1 { "" } else { "s" }) " unknown)"
                } @else { " run" }
                " · forwarding rules recorded " (counted.ports) " · windows " (counted.windows)
            }
            @if let Some(step) = group.steps.last() {
                p class="observed" {
                    "Latest observation: " (step.time) " · " (step.tool) " · " (step.status)
                    @if !step.outcome.is_empty() { " · " (step.outcome) }
                }
            }
            p class="observed" {
                "Observed in this window: " (count_noun(group.window_calls as u64, "call")) " · "
                (count_noun(group.window_failures as u64, "failure")) " · " (group.window_refusals) " refused"
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
                    "Declared purpose by "
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
                p class="claim-note" { "An authored purpose, shown in the author's own words." }
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
                                (count_noun(group.window_calls as u64, "call"))
                                " in this window · "
                                (count_noun(group.window_failures as u64, "failure"))
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
                            (count_noun(group.failures, "failure"))
                            " so far · " (group.refusals) " refused"
                        }
                    }
                } @else {
                    div class="receipt" data-state=(state_name_for_receipt(group)) {
                        // The receipt's two parts: what the transaction declared, and what the
                        // device observed. The hint below compares them and adds no fact of its own.
                        p class="receipt-declared" {
                            span class="receipt-label" { "Declared" }
                            span class="receipt-big" {
                                @if group.state == "committed" { "Committed." } @else { "Aborted." }
                            }
                        }
                        @if let Some(reason) = &group.reason {
                            p class="receipt-reason" { "Reason: " (reason) }
                        }
                        p class="receipt-observed" {
                            span class="receipt-label" { "Observed" }
                            span class="receipt-summary" {
                                (count_noun(group.calls, "call")) " · "
                                (count_noun(group.failures, "failure")) " · "
                                (group.refusals) " refused · " (group.duration_text())
                            }
                        }
                        @if let Some(sentence) = drift(&group.reading()) {
                            p class="receipt-drift" {
                                span class="receipt-drift-label" { "drift" }
                                " " (sentence)
                            }
                        }
                        p class="receipt-dim" {
                            "Committed and Aborted are the author's declarations, not a verdict on the session goal. The counts are the device's totals for the transaction; the steps beside them are the rows in this window."
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

/// The machine-wide counts as the header's quiet subtitle.
///
/// It is the `.activity-counts` box the view script updates, so the `data-count-*` attributes it
/// reads live here. They are on the two `<b>`s, not on the sentence: the script replaces the text of
/// the first `[data-count-total]`, and a whole-sentence element would lose its words.
fn machine_line(counts: &GroupCounts) -> Markup {
    html! {
        p
            class="activity-machine activity-counts"
            data-total=(counts.total)
            data-failed=(counts.failed)
            data-window=(format!("last {AUDIT_LINES}"))
            title=(format!(
                "These counts cover the newest {AUDIT_LINES} audit lines, not every call the page has appended."
            ))
        {
            "machine-wide · "
            a href="#all-activity" { "last " (AUDIT_LINES) " audit lines" }
            " · " b data-count-total=(counts.total) { (counts.total) }
            " " (plural_noun(counts.total as u64, "call"))
            " · " b data-count-failed=(counts.failed) { (counts.failed) }
            " failed"
        }
    }
}

/// The session selector: every session the record knows, linked by `?session=`, the claim/fact legend, and
/// the record's bound. The live sessions come first; an id only a ledger group names is still a
/// session the record can select, so it is listed too.
fn ledger_rail(facts: &ActivityFacts, selected: Option<&str>) -> Markup {
    let rows = facts.audit.as_ref().map(Vec::len).unwrap_or(0);
    let mut known: Vec<(&str, Option<&panel::SessionFacts>)> = Vec::new();
    for session in &facts.sessions {
        known.push((session.id.as_str(), Some(session)));
    }
    for txn in &facts.txns {
        if let Some(session) = txn.session.as_deref() {
            if !known.iter().any(|(id, _)| *id == session) {
                known.push((session, None));
            }
        }
    }
    html! {
        aside class="ledger-rail local-support" aria-label="Session navigation and sources" {
            h2 class="rail-label" { "Device sessions" }
            div class="session-selector" {
            @if known.is_empty() {
                p class="rail-none" { "No session on this machine." }
            } @else {
                @for (id, session) in &known {
                    @let live = session.is_some_and(|session| session.alive);
                    div class="rail-session" data-session=(id) {
                        span
                            class=(if live { "session-dot live" } else { "session-dot" })
                            aria-hidden="true"
                        {}
                        span class="rail-session-name" {
                            a
                                href=(format!("?session={id}"))
                                aria-current=[(Some(*id) == selected).then_some("page")]
                            {
                                @if let Some(session) = session {
                                    (session.kind) " " (id)
                                } @else {
                                    (id)
                                }
                            }
                        }
                        span class="rail-holder" {
                            @if let Some(session) = session {
                                (session.holder)
                            } @else {
                                "not live"
                            }
                        }
                    }
                }
            }
            }
            details class="record-guide" {
            summary { "How to read this record" }
            h3 class="rail-label" { "Sources and provenance" }
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
            h3 class="rail-label" { "The record" }
            p class="rail-record" data-record-rows=(rows) {
                (count_noun(rows as u64, "row")) " in this bounded tail"
            }
            p class="rail-note" {
                "The newest " (AUDIT_LINES) " audit lines. Older rows are not retained on this page."
                @if facts.truncated { " The tail is cut; older rows exist." }
            }
            p class="rail-note" {
                "A purpose and goal are authored claims; every step is a row the device wrote."
            }
            }
        }
    }
}

/// The document's ledger: only the selected session's transactions, newest first, the working one
/// (else the newest) expanded. Everything else the machine did is secondary and lives in
/// [`other_activity`]; it is never interleaved here.
fn session_ledger(
    facts: &ActivityFacts,
    members: &[&LedgerGroup],
    selected: Option<&str>,
    expanded: Option<&str>,
) -> Markup {
    let Some(id) = selected else {
        return html! {};
    };
    let state = match (&facts.audit, members.is_empty()) {
        (None, _) => "unreadable",
        (Some(_), true) => "empty",
        (Some(_), false) => "rows",
    };
    let live = facts.sessions.iter().find(|session| session.id == id);
    let earlier: Vec<_> = members
        .iter()
        .filter(|group| expanded != Some(group.id.as_str()))
        .collect();
    let failures: u64 = earlier
        .iter()
        .map(|group| {
            if group.retained {
                group.failures
            } else {
                group.window_failures as u64
            }
        })
        .sum();
    html! {
        section id="ledger" class="ledger" data-state=(state) data-session=(id) data-transactions=(members.len()) {
            p class="ledger-group-head" {
                "Current work · Session " span class="mono" { (id) }
                @if let Some(session) = live { " · " span class="ledger-group-note" { (session.holder) } }
            }
            @if state == "unreadable" {
                p class="ledger-empty" {
                    b { "The record is unreadable." }
                    " The device's audit could not be read, so this page cannot say what the session did."
                }
            } @else if members.is_empty() {
                p class="ledger-empty" { "No transaction recorded for this session yet." }
            } @else {
                @for group in members.iter().filter(|group| expanded == Some(group.id.as_str())) {
                    (ledger_entry(group, true, Some(id), facts.selected_txn.as_deref() == Some(group.id.as_str())))
                }
                @if !earlier.is_empty() {
                    details class="work-history" data-transactions=(earlier.len()) data-failures=(failures) {
                        summary { "Earlier work · " (count_noun(earlier.len() as u64, "transaction")) " · " (count_noun(failures, "failure")) }
                        @for group in earlier { (ledger_entry(group, false, Some(id), false)) }
                    }
                }
            }
        }
    }
}

/// The other work on this machine — the device-scope transactions, the unreceipted transactions
/// whose scope is unknown, and the rows with no transaction. Collapsed,
/// because it is not the session's ledger; the summary says how much it holds so a reader decides.
fn other_activity(
    facts: &ActivityFacts,
    groups: &[&LedgerGroup],
    loose: &[LedgerStep],
    selected: Option<&str>,
) -> Markup {
    // Device-scope records this process still holds are the machine's work. An orphan read only
    // from the audit may have been session-scoped, so it is not guessed into the machine group.
    let machine: Vec<&LedgerGroup> = groups
        .iter()
        .filter(|group| group.retained)
        .copied()
        .collect();
    let unscoped: Vec<&LedgerGroup> = groups
        .iter()
        .filter(|group| !group.retained)
        .copied()
        .collect();
    let transactions = machine.len() + unscoped.len();
    if transactions == 0 && loose.is_empty() {
        return html! {};
    }
    let loose_unknown = loose.iter().filter(|step| step.unknown).count();
    let unknown = loose_unknown
        + machine
            .iter()
            .chain(unscoped.iter())
            .flat_map(|group| group.steps.iter())
            .filter(|step| step.unknown)
            .count();
    let expanded = expanded_id(facts, groups);
    // A `?txn` link into this section opens it: the page it reloads must show the entry it named.
    let opened = facts
        .selected_txn
        .as_deref()
        .is_some_and(|id| transactions > 0 && groups.iter().any(|group| group.id == id));
    let mut summary = String::from("Other activity on this machine");
    if transactions > 0 {
        summary.push_str(" · ");
        summary.push_str(&count_noun(transactions as u64, "transaction"));
    }
    if !loose.is_empty() {
        summary.push_str(" · ");
        summary.push_str(&count_noun(loose.len() as u64, "row"));
        summary.push_str(" with no transaction");
    }
    html! {
        details
            class="older-activity"
            data-group="older"
            data-transactions=(transactions)
            data-unknown=(unknown)
            open[opened]
        {
            summary { (summary) }
            p class="older-why" {
                "Not this session’s ledger: machine-scope work belongs to the console, the panel"
                " or the install door. Rows with no transaction may be session creation, exempt calls,"
                " pre-dispatch refusals or older records."
            }
            @if !machine.is_empty() {
                section class="ledger-machine" data-group="machine" {
                    h2 class="ledger-group-head" {
                        "Machine"
                        span class="ledger-group-note" {
                            "the console, the panel, the install door — and receipts from before this rule"
                        }
                    }
                    @for group in &machine {
                        (ledger_entry(group, expanded.as_deref() == Some(group.id.as_str()), selected, facts.selected_txn.as_deref() == Some(group.id.as_str())))
                    }
                }
            }
            @if !unscoped.is_empty() {
                section class="ledger-unscoped" data-group="unscoped" {
                    h2 class="ledger-group-head" {
                        "No session recorded "
                        span class="ledger-group-note" { "the audit kept the work, not its scope" }
                    }
                    p class="ledger-note" {
                        "These transactions cannot say whether they worked on a session or on the machine: the receipt that carried the scope is not retained."
                    }
                    @for group in &unscoped {
                        (ledger_entry(group, expanded.as_deref() == Some(group.id.as_str()), selected, facts.selected_txn.as_deref() == Some(group.id.as_str())))
                    }
                }
            }
            @if !loose.is_empty() {
                section class="ledger-unattributed" data-unknown=(loose_unknown) {
                    h3 { "Rows with no transaction · " (count_noun(loose.len() as u64, "row")) }
                    p class="ledger-note" {
                        "A row whose record carries no `txn` is not guessed into a transaction. An unclassifiable row is drawn as unknown and counted, never dropped."
                    }
                    @for step in loose {
                        (loose_row(step))
                    }
                }
            }
        }
    }
}

/// `GET /activity`: the session's document, server-rendered from the audited facts.
pub(crate) fn page(facts: &ActivityFacts) -> Markup {
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
    let selected = selected_session(facts);
    // A `?session` the record does not know is named, not silently replaced by another session.
    let missing_session = facts
        .selected_session
        .as_deref()
        .filter(|_| selected.is_none());
    // A `?txn` the window does not hold is named, not silently replaced by another transaction.
    let missing_txn = facts
        .selected_txn
        .as_ref()
        .filter(|id| !ledger.groups.iter().any(|group| &group.id == *id));
    let other_session_txn = facts.selected_txn.as_ref().and_then(|id| {
        ledger
            .groups
            .iter()
            .find(|group| &group.id == id && group.session.is_some() && group.session != selected)
    });
    // The document's ledger is the selected session's; a group naming another session is not this
    // document's work (the rail links to it), and a group naming none is the secondary section's.
    let members: Vec<&LedgerGroup> = match &selected {
        Some(id) => ledger
            .groups
            .iter()
            .filter(|group| group.session.as_deref() == Some(id.as_str()))
            .collect(),
        None => Vec::new(),
    };
    let mut other: Vec<&LedgerGroup> = ledger
        .groups
        .iter()
        .filter(|group| group.session.is_none())
        .collect();
    // The selected session's four numbers. A machine with no session says no totals at all, and an
    // unreadable record has nothing to sum: it must not print zeros above its own honest line.
    let totals = match (&facts.audit, &selected) {
        (Some(_), Some(_)) => Some(session_totals(&members)),
        _ => None,
    };
    // The secondary section's own default entry: the working one, else the newest.
    other.sort_by(|left, right| {
        right
            .started_at_ms
            .cmp(&left.started_at_ms)
            .then_with(|| right.id.cmp(&left.id))
    });
    let session_expanded = expanded_id(facts, &members);
    let tallies = [
        ("runs", "Runs", counts.groups.runs),
        ("terminal", "Terminal", counts.groups.terminal),
        ("files", "Files", counts.groups.files),
        ("other", "Other", counts.groups.other),
    ];
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="summrise-panel" content=(facts.panel_token);
                title { "Summrise — operations" }
                style { (PreEscaped(summrise_ui::style(STYLES))) }
            }
            body class="activity-page local-workspace" {
                (summrise_ui::device_navigation(summrise_ui::DevicePage::Activity, Markup::default()))
                div class="local-workspace-body" {
                header
                    class="activity-head local-topbar"
                    data-host=(facts.host)
                    data-version=(facts.version)
                    data-channel=(facts.channel)
                {
                    div class="activity-brand local-breadcrumb" {
                        span class="stem" data-text-role="meta" { "Local workspace" }
                        span aria-hidden="true" { "/" }
                        strong { "Activity" }
                    }
                    span
                        class="activity-pill"
                        title="This machine's own name, as the device reads it"
                    {
                        span class="dot" aria-hidden="true" {}
                        span class="host-name" { (facts.host) }
                    }
                    span class="spacer" {}
                    div class="activity-release" data-text-role="meta" {
                        span class="release-channel" { (facts.channel) }
                        " · "
                        span class="release-version" { (facts.version) }
                        " · "
                        span class="release-state" {
                            span class="dot" aria-hidden="true" {}
                            span id="activity-state" role="status" aria-live="polite" { (if facts.audit.is_some() { "live" } else { "stale · record unreadable" }) }
                        }
                        " · "
                        a class="terminal-link" href="/panel#terminal" { "terminal ↗" }
                    }
                }
                main id="main" class="activity-main" tabindex="-1" {
                    div class="document" {
                        div class="local-task-layout" {
                        article class="local-primary" aria-label="Session work" {
                        div class="session-work" {
                        (goal_block(
                            facts,
                            selected.as_deref(),
                            missing_session,
                            totals.as_ref(),
                        ))
                        @if let Some(missing) = missing_txn {
                            p class="ledger-missing" data-missing-txn=(missing) {
                                b { "That transaction is not in this window." }
                                " No retained receipt, working record or audit row here names "
                                span class="mono" { (missing) }
                                ". It may have ended before this record's bound."
                            }
                        }
                        @if let Some(group) = other_session_txn {
                            @if let Some(session) = &group.session {
                                p class="ledger-missing" {
                                    "Transaction " (group.id) " belongs to session " (session) ". "
                                    a href=(format!("?session={session}&txn={}", group.id)) {
                                        "Open its session ledger."
                                    }
                                }
                            }
                        }
                        (session_ledger(
                            facts,
                            &members,
                            selected.as_deref(),
                            session_expanded.as_deref(),
                        ))
                        }
                        }
                        (ledger_rail(facts, selected.as_deref()))
                        }
                        (other_activity(facts, &other, &ledger.loose, selected.as_deref()))
                        details id="all-activity" class="transcript" {
                            summary {
                                "All activity on this machine — last " (AUDIT_LINES) " audit lines"
                            }
                            (machine_line(counts))
                            div class="activity-toolbar" {
                                @for (group, label, count) in tallies {
                                    span class="activity-count" data-group=(group) data-count=(count) {
                                        (label) " " b { (count) }
                                    }
                                }
                                span class="spacer" {}
                                select id="activity-filter" class="activity-filter" aria-label="Tool filter" {
                                    option value="all" { "All tools" }
                                    option value="runs" { "Runs" }
                                    option value="terminal" { "Terminal" }
                                    option value="files" { "Files" }
                                    option value="other" { "Other" }
                                }
                            }
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
                }
                footer class="activity-foot" data-text-role="meta" {
                    span { "the ledger is a fresh read · the call list polls every second" }
                    span class="ledger-window-note" {
                        "A line and its steps are the rows in this bounded window; a receipt's counts are the device's totals for that transaction."
                    }
                    span class="spacer" {}
                    span { kbd { "f" } " follow" }
                    span { "loopback only" }
                }
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
            selected_session: None,
            host: "DESKTOP-9VH92BJ".to_owned(),
            version: "2.0.15".to_owned(),
            channel: "canary".to_owned(),
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

    /// A live session with a goal, for the page whose title is that goal.
    fn session_with_goal(id: &str, holder: &str, goal: &str, by: &str) -> panel::SessionFacts {
        let mut session = session_facts(id, holder, true);
        session.goal = Some(goal.to_owned());
        session.goal_by = Some(by.to_owned());
        session
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
            selected_session: None,
            host: "DESKTOP-9VH92BJ".to_owned(),
            version: "2.0.15".to_owned(),
            channel: "canary".to_owned(),
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

    /// A failed end that was attempted, not refused: the call ran and failed.
    fn tagged_failure(id: u64, at: u64, txn: &str) -> Value {
        json!({
            "id": id,
            "ts_ms": at,
            "event": "end",
            "ok": false,
            "error_kind": "tool_failed",
            "transport": "loopback",
            "txn": txn,
        })
    }

    /// A call the gate refused before it ran: the begin carries `decision: "deny"` and the end the
    /// gate's own `error_kind`, which is never `"refused"`.
    fn denied_call(id: u64, at: u64, tool: &str, error_kind: &str, txn: &str) -> [Value; 2] {
        [
            json!({
                "id": id,
                "ts_ms": at,
                "event": "begin",
                "tool": tool,
                "decision": "deny",
                "transport": "loopback",
                "txn": txn,
            }),
            json!({
                "id": id,
                "ts_ms": at + 100,
                "event": "end",
                "ok": false,
                "error_kind": error_kind,
                "transport": "loopback",
                "txn": txn,
            }),
        ]
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
    fn a3_local_workspace_keeps_session_navigation_beside_the_work_record() {
        let html = page(&ledger(
            vec![],
            vec![txn(
                "txn-work",
                "Inspect runtime",
                Some("session-1"),
                "working",
                BOOT,
            )],
            vec![session_with_goal(
                "session-1",
                "you",
                "Repair this machine",
                "you",
            )],
            None,
        ))
        .into_string();
        let body = html.split_once("<body").unwrap().1;
        let navigation = body
            .split_once("aria-label=\"Local navigation\"")
            .expect("local navigation landmark")
            .1
            .split_once("</nav>")
            .unwrap()
            .0;
        assert!(navigation.contains("href=\"/activity\" aria-current=\"page\""));
        assert!(navigation.contains("href=\"/panel\""));
        let primary = body
            .split_once("aria-label=\"Session work\"")
            .expect("work record region")
            .1
            .split_once("</article>")
            .unwrap()
            .0;
        assert!(primary.contains("Repair this machine"));
        assert!(primary.contains("id=\"ledger\""));
        let support = body
            .split_once("aria-label=\"Session navigation and sources\"")
            .expect("supporting landmark")
            .1
            .split_once("</aside>")
            .unwrap()
            .0;
        assert!(support.contains("?session=session-1"));
        assert!(support.contains("How to read this record"));
        assert!(body.contains("id=\"all-activity\""));
        assert!(body.contains("id=\"feed\""));
        assert_eq!(body.matches("<h1 ").count(), 1);
    }

    /// A goal must remain the one page title; totals and the current purpose must be reachable
    /// without opening historical or evidence disclosures.
    #[test]
    fn session_work_keeps_goal_purpose_and_visible_totals() {
        let at = BOOT + 10_000;
        let html = page(&ledger(
            vec![],
            vec![txn(
                "txn-current",
                "核实安装路径 /var/lib/summrise",
                Some("session-1"),
                "working",
                at,
            )],
            vec![session_with_goal(
                "session-1",
                "the operator",
                "让设备工作清楚可见",
                "you",
            )],
            None,
        ))
        .into_string();
        assert_eq!(html.matches("<h1 ").count(), 1);
        assert!(html.contains(r#"<h1 class="goal-title">让设备工作清楚可见</h1>"#));
        assert!(html.contains(r#"<h2 class="ledger-purpose">核实安装路径 /var/lib/summrise</h2>"#));
        let body = html.split_once("<body").unwrap().1;
        let nav = body.find(r##"href="#main""##).expect("skip is first");
        assert!(nav < body.find(r#"href="/activity""#).unwrap());
        assert!(
            body.contains(r#"<main id="main" class="activity-main" tabindex="-1">"#),
            "skip target can receive keyboard focus without entering the tab sequence"
        );
        let surface = body
            .find(r#"class="session-work""#)
            .expect("one integrated work surface");
        let goal = body.find(r#"class="goal""#).unwrap();
        let totals = body.find(r#"class="goal-totals""#).unwrap();
        let current = body.find(r#"id="ledger""#).unwrap();
        assert!(surface < goal && goal < totals && totals < current);
        assert!(
            !body[surface..current].contains("<details"),
            "goal and totals are visible"
        );
        assert!(body.contains("set by you"));
        assert!(body.contains("2 calls"));
        assert!(!body.contains(">CLAIM<"));
    }

    /// A failed historical declaration must not erase the refusal in current work or conceal
    /// missing-receipt / execution-unknown evidence behind a success label.
    #[test]
    fn work_history_preserves_failure_and_unknown_evidence() {
        let at = BOOT + 10_000;
        let mut old = txn(
            "txn-old",
            "earlier check",
            Some("session-1"),
            "committed",
            at,
        );
        old.failures = 2;
        let mut current = txn(
            "txn-current",
            "current check",
            Some("session-1"),
            "working",
            at + 20_000,
        );
        current.failures = 1;
        current.refusals = 1;
        let audit = denied_call(11, at + 21_000, "files.write", "denied", "txn-current").to_vec();
        let html = page(&ledger(
            audit,
            vec![old, current],
            vec![session_facts("session-1", "you", true)],
            None,
        ))
        .into_string();
        let current = html
            .split_once(r#"data-txn="txn-current""#)
            .unwrap()
            .1
            .split_once("</article>")
            .unwrap()
            .0;
        let refusal = current
            .find(r#"class="current-refusal""#)
            .expect("current refusal is visible");
        assert!(current[..refusal].find("<details").is_none());
        assert!(current[refusal..].contains("1 refused"));
        let history = html
            .split_once(r#"class="work-history""#)
            .expect("Earlier work disclosure")
            .1;
        let summary = history.split_once("</summary>").unwrap().0;
        assert!(summary.contains("Earlier work") && summary.contains("2 failures"));
        assert!(history.contains("Committed.") && history.contains("Declared"));

        let mut orphan = txn("txn-orphan", "old command", None, "unknown", at);
        orphan.retained = false;
        orphan.calls = 0;
        let html = page(&ledger(
            vec![
                tagged_begin(21, at, "runs.exec", None, "txn-orphan"),
                tagged_end(21, at + 100, false, "txn-orphan"),
            ],
            vec![orphan],
            vec![],
            None,
        ))
        .into_string();
        assert!(html.contains("receipt not retained"));
        assert!(html.contains("processes 0 confirmed run (1 execution unknown)"));
        assert!(html.contains("Observed in this work"));
        assert!(html.contains("forwarding rules recorded"));
        assert!(!html.contains("goal succeeded"));
    }

    #[test]
    fn receipt_deep_link_opens_the_named_evidence() {
        let facts = ledger(
            vec![],
            vec![txn(
                "txn-selected",
                "inspect receipt",
                Some("session-1"),
                "committed",
                BOOT,
            )],
            vec![session_facts("session-1", "you", true)],
            Some("txn-selected"),
        );
        let html = page(&facts).into_string();
        let entry = html
            .split_once(r#"data-txn="txn-selected""#)
            .unwrap()
            .1
            .split_once("</article>")
            .unwrap()
            .0;
        assert!(
            entry.contains(r#"<details class="work-evidence" open>"#),
            "the receipt deep link reveals its evidence"
        );
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
        let html = page(&facts).into_string();

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
        let cut = page(&facts).into_string();
        assert!(
            cut.contains("data-truncated"),
            "a cut tail says older lines exist: {cut}"
        );

        let empty = page(&transcript(vec![], NOW)).into_string();
        assert!(
            empty.contains(r#"data-state="empty""#),
            "an audit with no lines is empty: {empty}"
        );
        assert!(empty.contains("No call has been recorded"));

        let mut unreadable = transcript(vec![], NOW);
        unreadable.audit = None;
        // A live session still selects the document, but an unreadable record has no totals to say.
        unreadable.sessions = vec![session_facts("session-1", "the operator", true)];
        let unreadable = page(&unreadable).into_string();
        assert!(
            unreadable.contains(r#"data-state="unreadable""#),
            "an audit that cannot be read is not empty: {unreadable}"
        );
        assert!(unreadable.contains("could not be read"));
        assert!(
            !unreadable.contains(r#"<p class="goal-totals""#),
            "an unreadable record prints no totals: {unreadable}"
        );
    }

    #[test]
    fn the_transcript_page_escapes_the_tool_name_it_reads() {
        let at = BOOT + 1_000;
        let audit = vec![
            begin(1, at, "<img src=x onerror=alert(1)>", None),
            end(1, at + 100, true, None),
        ];
        let html = page(&transcript(audit, at + 200)).into_string();

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
        let html = page(&transcript(audit, at + 5_000)).into_string();

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
    fn the_activity_page_has_one_palette() {
        let html = page(&transcript(vec![], NOW)).into_string();
        assert!(
            !html.contains("data-theme"),
            "the page declares no theme attribute"
        );
        assert!(
            !html.contains(r#"id="theme""#),
            "the page offers no theme button"
        );
        assert!(!html.contains("summrise-theme"), "the page stores no theme");
        assert!(
            !html.contains(r#":root[data-theme="dark"]"#),
            "the shared sheet carries no dark block"
        );
        assert!(
            html.contains("color-scheme: light"),
            "the one palette is light"
        );
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
        let html = page(&transcript(audit, at + 2_500)).into_string();

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
        let html = page(&transcript(vec![], NOW)).into_string();

        assert!(
            html.contains(r#"data-window="last 200""#),
            "the counts carry their window: {html}"
        );
        assert!(
            html.contains(r##"<a href="#all-activity">last 200 audit lines</a>"##),
            "the machine line says the counts are the window and links to the call list: {html}"
        );
        assert!(
            html.contains("These counts cover the newest 200 audit lines"),
            "the title explains the window: {html}"
        );
        assert!(
            html.contains("machine-wide · "),
            "the subtitle names its scope instead of posing as the document's total: {html}"
        );
        assert!(
            html.contains(
                r##"machine-wide · <a href="#all-activity">last 200 audit lines</a> · <b data-count-total="0">0</b> calls · <b data-count-failed="0">0</b> failed"##
            ),
            "the machine line prints each number once, then its noun: {html}"
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
        let html = page(&ledger(audit, txns, vec![], None)).into_string();

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
            html.contains(r#"href="?session=session-1&amp;txn=txn-2""#),
            "a receipt link keeps the document's session, so a reload lands on the same one: {html}"
        );
        // The working session entry is the document's one expanded entry; the machine transaction
        // in the secondary section has its own default expansion, which the reader only sees after
        // opening that section.
        let ledger_start = html.find(r#"id="ledger""#).expect("the ledger is drawn");
        let ledger_end = html
            .find(r#"class="older-activity""#)
            .expect("the secondary section follows the ledger");
        let session_ledger = &html[ledger_start..ledger_end];
        assert!(
            session_ledger.contains(r#"href="?session=session-1&amp;txn=txn-1""#),
            "the session ledger's receipt link carries both the session and the transaction: {session_ledger}"
        );
        assert_eq!(
            session_ledger.matches(r#"class="ledger-spread""#).count(),
            1,
            "one entry of the session's ledger is expanded: {session_ledger}"
        );
    }

    #[test]
    fn a_device_scope_transaction_sits_in_the_collapsed_other_activity() {
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
            page(&ledger(
                audit,
                txns,
                vec![session_facts("session-1", "the operator", true)],
                None,
            ))
            .into_string()
        };

        let ledger = html
            .find(r#"id="ledger""#)
            .expect("the session's ledger exists");
        let older = html
            .find(r#"class="older-activity""#)
            .expect("the secondary section exists");
        let transcript = html
            .find(r#"class="transcript""#)
            .expect("the call list exists");
        assert!(
            html[ledger..older].contains(r#"data-txn="txn-2""#)
                && !html[ledger..older].contains(r#"data-txn="txn-1""#),
            "the session's ledger holds only the session's work: {html}"
        );
        assert!(
            older < transcript && html[older..transcript].contains("bump the release pointer"),
            "the device-scope transaction is in the secondary section after the ledger: {html}"
        );
        let open_tag_end = html[older..].find('>').expect("the details' tag ends") + older;
        assert!(
            !html[older..=open_tag_end].contains(" open"),
            "the secondary section is collapsed by default: {}",
            &html[older..=open_tag_end]
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
        let html = page(&ledger(audit, txns, vec![], None)).into_string();

        assert!(
            html.contains(r#"data-txn="txn-1" data-expanded="true""#),
            "the working transaction is expanded: {html}"
        );
        assert!(
            !html.contains(r#"data-txn="txn-2" data-expanded="true""#),
            "a committed transaction is not the default entry: {html}"
        );
    }

    /// The session ledger expands the working transaction; with none working, the newest one.
    #[test]
    fn the_session_ledger_expands_the_working_transaction_else_the_newest() {
        let at = BOOT + 10_000;
        let session = session_facts("session-1", "the operator", true);
        let mut txns = vec![
            txn(
                "txn-2",
                "verify the suite",
                Some("session-1"),
                "committed",
                at + 30_000,
            ),
            txn("txn-1", "fix the parser", Some("session-1"), "working", at),
        ];
        let html = page(&ledger(
            Vec::new(),
            txns.clone(),
            vec![session.clone()],
            None,
        ))
        .into_string();
        let session_ledger = session_ledger_of(&html);
        assert!(
            session_ledger.contains(r#"data-txn="txn-1" data-expanded="true""#),
            "the working transaction is expanded even though it is the older one: {session_ledger}"
        );
        assert!(
            !session_ledger.contains(r#"data-txn="txn-2" data-expanded="true""#),
            "the newer committed transaction is not: {session_ledger}"
        );

        txns[1].state = "committed".to_owned();
        txns[1].ended_at_ms = Some(at + 60_000);
        let html = page(&ledger(Vec::new(), txns, vec![session], None)).into_string();
        let session_ledger = session_ledger_of(&html);
        assert!(
            session_ledger.contains(r#"data-txn="txn-2" data-expanded="true""#),
            "with nothing working, the newest session entry is expanded: {session_ledger}"
        );
        assert!(
            !session_ledger.contains(r#"data-txn="txn-1" data-expanded="true""#),
            "the older one is not: {session_ledger}"
        );
    }

    /// The selection rule's precedence: the working session, then the newest call, then the record.
    #[test]
    fn the_selection_rule_prefers_the_working_session_then_the_newest_then_the_record() {
        let at = BOOT + 10_000;
        let mut working = session_facts("session-working", "the operator", true);
        working.open_txn = Some("txn-1".to_owned());
        working.last_call_at_ms = Some(at);
        let mut idle = session_facts("session-idle", "you", true);
        idle.last_call_at_ms = Some(at + 60_000);
        let txns = vec![
            txn(
                "txn-1",
                "fix the parser",
                Some("session-working"),
                "working",
                at,
            ),
            txn(
                "txn-2",
                "read the record",
                Some("session-record"),
                "committed",
                at + 30_000,
            ),
        ];
        let mut facts = ledger(Vec::new(), txns, vec![working, idle], None);

        let html = page(&facts).into_string();
        assert!(
            html.contains(r#"data-goal="none" data-session="session-working""#),
            "the live session with an open transaction beats the newer idle one: {html}"
        );

        // Nothing is working: the newest last_call_at_ms wins.
        facts.sessions[0].open_txn = None;
        let html = page(&facts).into_string();
        assert!(
            html.contains(r#"data-goal="none" data-session="session-idle""#),
            "with no open transaction the session with the newest call wins: {html}"
        );

        // A `?session` only a ledger group names is still a session the record knows.
        facts.selected_session = Some("session-record".to_owned());
        let html = page(&facts).into_string();
        assert!(
            html.contains(r#"data-goal="unavailable" data-session="session-record""#),
            "a session the record only names in a ledger group is selectable: {html}"
        );
    }

    #[test]
    fn a_transaction_query_in_another_session_explains_its_location() {
        let at = BOOT + 10_000;
        for explicit in [None, Some("session-b".to_owned())] {
            let mut facts = ledger(
                vec![],
                vec![
                    txn(
                        "txn-a",
                        "first session work",
                        Some("session-a"),
                        "committed",
                        at,
                    ),
                    txn(
                        "txn-b",
                        "second session work",
                        Some("session-b"),
                        "committed",
                        at + 1_000,
                    ),
                ],
                vec![],
                Some("txn-a"),
            );
            facts.selected_session = explicit;
            let html = page(&facts).into_string();
            assert!(
                html.contains(r#"data-session="session-b""#),
                "the selected session remains B"
            );
            assert!(
                html.contains("Transaction txn-a belongs to session session-a."),
                "{html}"
            );
            assert!(
                html.contains(r#"href="?session=session-a&amp;txn=txn-a""#),
                "a link opens the named transaction in its own session"
            );
            assert!(
                !html.contains(r#"data-txn="txn-a" data-expanded="true""#),
                "the query does not replace the current session"
            );
        }
    }

    /// The document's order, and the rail's own state for the selected session.
    #[test]
    fn the_document_reads_work_session_sources_then_other_activity() {
        let at = BOOT + 10_000;
        let mut audit = Vec::new();
        audit.extend(txn_begin(10, at, "fix the parser", "txn-1"));
        audit.push(tagged_begin(11, at + 1_000, "files.read", None, "txn-1"));
        audit.push(tagged_end(11, at + 1_100, true, "txn-1"));
        audit.extend(txn_begin(20, at + 30_000, "verify the suite", "txn-2"));
        audit.push(tagged_begin(21, at + 31_000, "files.list", None, "txn-2"));
        audit.push(tagged_end(21, at + 31_100, true, "txn-2"));
        audit.push(begin(30, at + 40_000, "files.list", None));
        audit.push(end(30, at + 40_100, true, None));
        let txns = vec![
            txn("txn-1", "fix the parser", Some("session-1"), "working", at),
            txn("txn-2", "verify the suite", None, "committed", at + 30_000),
        ];
        let html = page(&ledger(
            audit,
            txns,
            vec![session_with_goal(
                "session-1",
                "the operator",
                "make the installer's config readable by the operator",
                "you",
            )],
            None,
        ))
        .into_string();

        let order = [
            r#"<section class="goal""#,
            r#"<section id="ledger""#,
            r#"<aside class="ledger-rail local-support""#,
            r#"<details class="older-activity""#,
            r#"<details id="all-activity""#,
            r#"<p class="activity-machine activity-counts""#,
        ];
        let mut last = 0;
        for needle in order {
            let offset = html
                .find(needle)
                .unwrap_or_else(|| panic!("the document draws {needle}: {html}"));
            assert!(
                last < offset,
                "{needle} comes after the section before it: {html}"
            );
            last = offset;
        }
        assert!(
            html.contains(r#"<a href="?session=session-1" aria-current="page">"#),
            "the rail links the selected session and marks it as the current page: {html}"
        );
    }

    #[test]
    fn a_query_transaction_selects_another_expanded_entry() {
        let at = BOOT + 10_000;
        let txns = vec![
            txn("txn-2", "verify the suite", None, "committed", at + 30_000),
            txn("txn-1", "fix the parser", None, "working", at),
        ];
        let html = page(&ledger(Vec::new(), txns, vec![], Some("txn-2"))).into_string();

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
        let html = page(&ledger(audit, txns, vec![], None)).into_string();

        assert!(html.contains(r#"class="said""#), "the claim column: {html}");
        assert!(
            html.contains("Declared purpose by the install door"),
            "the purpose remains attributed to its author: {html}"
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
        let html = page(&ledger(Vec::new(), txns, vec![], None)).into_string();

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

    /// The receipt's declared ending and the device's own counts, as the drift rule reads them.
    fn reading(state: &str, calls: u64, failures: u64, refusals: u64) -> ReceiptReading<'_> {
        ReceiptReading {
            state,
            calls,
            failures,
            refusals,
            retained: true,
        }
    }

    /// The receipt's drift element, when the page drew one.
    fn receipt_drift(html: &str) -> Option<String> {
        let open = r#"<p class="receipt-drift">"#;
        let start = html.find(open)?;
        let close = html[start..].find("</p>")? + start + "</p>".len();
        Some(html[start..close].to_owned())
    }

    #[test]
    fn a_committed_transaction_with_a_failure_says_so() {
        // The declared ending against the observed calls: the author committed, and one call
        // failed. The noun is the receipt's, singular at one.
        assert_eq!(
            drift(&reading("committed", 5, 1, 0)).as_deref(),
            Some("Committed over 1 failed call.")
        );
        assert_eq!(
            drift(&reading("committed", 5, 3, 0)).as_deref(),
            Some("Committed over 3 failed calls.")
        );

        let at = BOOT + 10_000;
        let mut txns = vec![txn("txn-1", "publish the release", None, "committed", at)];
        txns[0].calls = 5;
        txns[0].failures = 1;
        let html = page(&ledger(Vec::new(), txns, vec![], None)).into_string();
        let line = receipt_drift(&html).expect("a committed receipt over a failure draws the hint");
        assert!(
            line.contains(r#"<span class="receipt-drift-label">drift</span>"#),
            "the element carries the visible drift label: {line}"
        );
        assert!(
            line.contains("Committed over 1 failed call."),
            "the receipt's own sentence is drawn: {line}"
        );
    }

    #[test]
    fn an_aborted_transaction_with_no_failure_says_the_ending_is_its_authors() {
        // An abort the record cannot justify: no call failed and none was refused, so the ending
        // is the author's decision, not something the device observed.
        assert_eq!(
            drift(&reading("aborted", 2, 0, 0)).as_deref(),
            Some("Aborted after 2 clean calls — the ending is the author's, not the record's.")
        );
        // The wire omits a counter that was zero; the receipt's u64s carry it explicitly, so a
        // receipt that retained only `calls: 2` is exactly this reading.
        assert_eq!(
            drift(&ReceiptReading {
                state: "aborted",
                calls: 2,
                failures: 0,
                refusals: 0,
                retained: true,
            })
            .as_deref(),
            Some("Aborted after 2 clean calls — the ending is the author's, not the record's.")
        );
        assert_eq!(
            drift(&reading("aborted", 1, 0, 0)).as_deref(),
            Some("Aborted after 1 clean call — the ending is the author's, not the record's.")
        );
        // An abort after a failure is consistent: the failure is the record's, and there is no
        // drift.
        assert_eq!(drift(&reading("aborted", 2, 1, 0)), None);

        // The rendered receipt, not only the rule: the page draws the same sentence.
        let at = BOOT + 10_000;
        let txns = vec![txn("txn-1", "stop before the release", None, "aborted", at)];
        let html = page(&ledger(Vec::new(), txns, vec![], None)).into_string();
        let line = receipt_drift(&html).expect("an abort over clean calls draws the hint");
        assert!(
            line.contains(
                "Aborted after 2 clean calls — the ending is the author's, not the record's."
            ),
            "the abort's own sentence is drawn: {line}"
        );
    }

    #[test]
    fn an_empty_transaction_says_it_recorded_no_calls() {
        // A receipt that closed without observing a single call: whatever the purpose said, the
        // record holds no work to compare it with.
        assert_eq!(
            drift(&reading("committed", 0, 0, 0)).as_deref(),
            Some("Ended with no calls recorded.")
        );
        assert_eq!(
            drift(&reading("aborted", 0, 0, 0)).as_deref(),
            Some("Ended with no calls recorded.")
        );

        // The rendered receipt, not only the rule: an empty commit draws the same sentence.
        let at = BOOT + 10_000;
        let mut txns = vec![txn("txn-1", "an empty transaction", None, "committed", at)];
        txns[0].calls = 0;
        txns[0].duration_ms = 0;
        let html = page(&ledger(Vec::new(), txns, vec![], None)).into_string();
        let line = receipt_drift(&html).expect("an empty receipt draws the hint");
        assert!(
            line.contains("Ended with no calls recorded."),
            "the empty receipt's own sentence is drawn: {line}"
        );
    }

    #[test]
    fn a_clean_committed_transaction_has_no_drift_line() {
        // A clean commit is the normal ending; it earns no sentence at all.
        assert_eq!(drift(&reading("committed", 5, 0, 0)), None);
        // A working transaction has no declared ending to compare; an orphan has no receipt.
        assert_eq!(
            drift(&ReceiptReading {
                state: "working",
                calls: 2,
                failures: 0,
                refusals: 0,
                retained: true,
            }),
            None
        );
        assert_eq!(
            drift(&ReceiptReading {
                state: "committed",
                calls: 5,
                failures: 1,
                refusals: 0,
                retained: false,
            }),
            None
        );

        let at = BOOT + 10_000;
        let mut txns = vec![txn("txn-1", "verify the suite", None, "committed", at)];
        txns[0].calls = 5;
        let html = page(&ledger(Vec::new(), txns, vec![], None)).into_string();
        assert!(
            receipt_drift(&html).is_none(),
            "a clean receipt draws no hint: {html}"
        );
    }

    #[test]
    fn the_drift_line_names_both_sides() {
        let sentence = drift(&reading("committed", 4, 2, 0)).expect("the line exists");
        assert_eq!(sentence, "Committed over 2 failed calls.");
        // The declaration (`Committed`) and the observation (`2 failed calls`) are both in the line.
        assert!(
            sentence.contains("Committed"),
            "the declared ending: {sentence}"
        );
        assert!(
            sentence.contains("2 failed calls"),
            "the observed calls: {sentence}"
        );
        // The receipt's total is not the drift: only the side that disagrees is counted.
        assert!(
            !sentence.contains("4 calls"),
            "the line does not repeat the total: {sentence}"
        );
        // Refusals are the same arithmetic, and the noun agrees with the receipt's count.
        assert_eq!(
            drift(&reading("committed", 4, 0, 2)).as_deref(),
            Some("Committed over 2 refused calls.")
        );
        assert_eq!(
            drift(&reading("committed", 4, 0, 1)).as_deref(),
            Some("Committed over 1 refused call.")
        );
    }

    #[test]
    fn the_drift_line_invents_no_fact() {
        let at = BOOT + 10_000;
        let mut txns = vec![txn("txn-1", "fix the failing test", None, "committed", at)];
        txns[0].calls = 9;
        txns[0].failures = 1;
        txns[0].reason = Some("the build was red".to_owned());
        let html = page(&ledger(Vec::new(), txns, vec![], None)).into_string();
        let line = receipt_drift(&html).expect("the receipt draws its hint");
        let sentence = line
            .split_once("</span>")
            .map(|(_, rest)| rest.trim_end_matches("</p>").trim().to_owned())
            .expect("the drift element carries its label");
        // The sentence is the receipt's arithmetic and nothing else: not the purpose, not the
        // reason, not the total, not the duration.
        assert_eq!(sentence, "Committed over 1 failed call.");
        for claim in [
            "fix the failing test",
            "the build was red",
            "9 calls",
            "1m 0s",
        ] {
            assert!(
                !sentence.contains(claim),
                "the hint invents no fact from {claim}: {sentence}"
            );
        }
    }

    #[test]
    fn the_receipt_is_declared_then_observed() {
        let at = BOOT + 10_000;
        let mut txns = vec![txn("txn-1", "verify the suite", None, "committed", at)];
        txns[0].calls = 9;
        txns[0].failures = 1;
        txns[0].refusals = 2;
        let html = page(&ledger(Vec::new(), txns, vec![], None)).into_string();
        let start = html.find(r#"class="receipt""#).expect("the receipt exists");
        let close = html[start..].find("</div>").expect("the receipt closes") + start;
        let receipt = &html[start..close];

        let declared = receipt
            .find(r#"<span class="receipt-label">Declared</span>"#)
            .unwrap_or_else(|| panic!("the declaration is labelled: {receipt}"));
        let observed = receipt
            .find(r#"<span class="receipt-label">Observed</span>"#)
            .unwrap_or_else(|| panic!("the observation is labelled: {receipt}"));
        let drift = receipt
            .find(r#"<p class="receipt-drift">"#)
            .unwrap_or_else(|| panic!("the hint sits under the two parts: {receipt}"));
        assert!(
            declared < observed && observed < drift,
            "the receipt reads Declared, then Observed, then the hint: {receipt}"
        );
        assert!(
            receipt.contains(r#"<span class="receipt-big">Committed.</span>"#),
            "the declaration is the receipt's state word: {receipt}"
        );
        assert!(
            receipt.contains("9 calls")
                && receipt.contains("1 failure")
                && receipt.contains("2 refused"),
            "the observation is the receipt's own counts: {receipt}"
        );
        assert!(
            receipt.contains("Committed over 1 failed call."),
            "the hint compares the two parts: {receipt}"
        );
    }

    #[test]
    fn the_terminal_is_a_link_not_a_pane() {
        let html = page(&ledger(Vec::new(), Vec::new(), vec![], None)).into_string();

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
        let html = page(&ledger(Vec::new(), Vec::new(), vec![session], None)).into_string();

        assert!(
            html.contains(r#"class="ledger-rail local-support""#),
            "the rail: {html}"
        );
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
        assert!(
            html.contains(
                r#"<p class="rail-record" data-record-rows="0">0 rows in this bounded tail</p>"#
            ),
            "the rail prints its count once, not twice: {html}"
        );
    }

    #[test]
    fn a_long_token_id_is_attributed_by_its_first_eight_characters() {
        const TOKEN: &str = "21b2931637748caebf7128f87bcd500bc509e239938df85c2ec64fe57b7cc209";
        let holder = summrise_core::Holder {
            kind: summrise_core::HolderKind::Token,
            token_id: Some(TOKEN.to_owned()),
        };
        let label = panel::holder_label(&holder);
        let session = session_with_goal("session-1", &label, "Round 27 live proof", &label);
        let mut txn = txn(
            "txn-1",
            "Round 27 live proof",
            Some("session-1"),
            "committed",
            BOOT + 10_000,
        );
        txn.author = label;
        let html = page(&ledger(Vec::new(), vec![txn], vec![session], None)).into_string();

        let rail = html
            .find(r#"<span class="rail-holder">"#)
            .expect("the rail names the holder");
        assert!(
            html[rail..].starts_with(r#"<span class="rail-holder">token 21b29316</span>"#),
            "the rail prints the token's first eight characters, not its hash: {}",
            &html[rail..]
        );
        assert!(
            !html.contains(TOKEN),
            "the 64-character token id is nowhere on the page: {html}"
        );
    }

    #[test]
    fn a_short_token_id_is_unchanged_in_the_rail() {
        let holder = summrise_core::Holder {
            kind: summrise_core::HolderKind::Token,
            token_id: Some("srm_7f".to_owned()),
        };
        let label = panel::holder_label(&holder);
        let html = page(&ledger(
            Vec::new(),
            Vec::new(),
            vec![session_facts("session-1", &label, true)],
            None,
        ))
        .into_string();

        assert!(
            html.contains(r#"<span class="rail-holder">token srm_7f</span>"#),
            "a short id is shown whole, not shortened: {html}"
        );
    }

    #[test]
    fn an_unknown_row_is_visible_and_counted() {
        let at = BOOT + 10_000;
        let audit = vec![
            begin(1, at, "mystery.tool", None),
            end(1, at + 100, true, None),
        ];
        let html = page(&ledger(audit, vec![], vec![], None)).into_string();

        assert!(
            html.contains(r#"class="older-activity" data-group="older" data-transactions="0" data-unknown="1""#),
            "the unknown row is counted in the section that draws it: {html}"
        );
        assert!(
            html.contains(r#"class="ledger-unknown-row""#)
                && html.contains(r#"data-kind="unknown""#)
                && html.contains("mystery.tool"),
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
        let html = page(&ledger(
            audit,
            vec![orphan],
            vec![session_facts("session-1", "the operator", true)],
            None,
        ))
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
        let html = page(&ledger(audit, txns, vec![], None)).into_string();

        assert!(
            html.contains("Observed in this window: 1 call · 1 failure · 1 refused"),
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
        let html = page(&ledger(
            audit,
            txns,
            vec![session_facts("session-1", "the operator", true)],
            None,
        ))
        .into_string();

        let ledger = html
            .find(r#"id="ledger""#)
            .expect("the session's ledger is drawn");
        let older = html
            .find(r#"class="older-activity""#)
            .expect("the secondary section exists");
        assert!(
            !html[ledger..older].contains(r#"data-txn="txn-1""#),
            "the argument does not place the transaction on the session's ledger: {html}"
        );
        assert!(
            html[older..].contains(r#"class="ledger-machine""#)
                && html[older..].contains("bump the release pointer"),
            "the record's device scope wins, in the secondary section: {html}"
        );
    }

    /// Session navigation is drawn once beside the focused work, before the full call transcript.
    #[test]
    fn the_session_selector_is_drawn_once_beside_the_work() {
        let html = page(&ledger(
            Vec::new(),
            Vec::new(),
            vec![session_facts("session-1", "the operator", true)],
            None,
        ))
        .into_string();
        assert_eq!(
            html.matches(r#"class="ledger-rail local-support""#).count(),
            1,
            "the sessions, legend and record are drawn once, never duplicated beneath the ledger: {html}"
        );
        let rail = html
            .find(r#"class="ledger-rail local-support""#)
            .expect("the rail is drawn");
        let transcript = html
            .find(r#"class="transcript""#)
            .expect("the call list is drawn");

        let goal = html
            .find(r#"<section class="goal""#)
            .expect("the goal exists");
        assert!(
            goal < rail && rail < transcript,
            "session selection is beside the focused work and before the transcript"
        );
        assert!(html[rail..transcript].contains(r#"href="?session=session-1""#));
    }

    /// A stale `?txn` link used to expand another transaction. The page says the named transaction is
    /// not in this window, and expands nothing.
    #[test]
    fn a_stale_query_transaction_says_it_is_not_in_this_window() {
        let at = BOOT + 10_000;
        let txns = vec![txn("txn-1", "fix the parser", None, "working", at)];
        let html = page(&ledger(Vec::new(), txns, vec![], Some("txn-gone"))).into_string();

        assert!(
            html.contains("txn-gone") && html.contains("is not in this window"),
            "the page names the missing transaction instead of falling back: {html}"
        );
        assert!(
            !html
                .split_once("<body")
                .unwrap()
                .1
                .contains(r#"data-expanded="true""#),
            "a stale link must not expand the working or newest transaction: {html}"
        );
    }

    /// The secondary section's `data-unknown` is the count of the rows it holds, not the row count of
    /// its loose list alone, and `.ledger-unattributed` counts only its own rows.
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
        let html = page(&ledger(audit, txns, vec![], None)).into_string();

        assert!(
            html.contains(
                r#"class="older-activity" data-group="older" data-transactions="1" data-unknown="2""#
            ),
            "the section's total counts the unknown row inside its machine transaction too: {html}"
        );
        assert!(
            html.contains(r#"class="ledger-unattributed" data-unknown="1""#),
            "the unattributed section counts only its own row: {html}"
        );
    }

    /// The page's own stylesheet, out of the rendered document.
    fn css_of(html: &str) -> &str {
        let start = html.find("<style>").expect("the page carries its sheet") + "<style>".len();
        let end = html[start..].find("</style>").expect("the sheet closes") + start;
        &html[start..end]
    }

    /// One rule by its opening selector: the declarations between its `{` and `}`, inclusive.
    fn rule<'a>(css: &'a str, selector: &str) -> &'a str {
        let at = css
            .find(selector)
            .unwrap_or_else(|| panic!("the sheet styles {selector}: {css}"));
        let open = css[at..].find('{').expect("the rule opens") + at;
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

    /// **THE DOCUMENT FACE IS A ROLE, NOT A NEW SIZE (the light contract).** The transaction's purpose,
    /// the receipt's headline and the page's brand word are the document; the test reads the named
    /// rules in the rendered sheet, because a computed face is not a question a string can answer.
    #[test]
    fn the_ledger_purpose_is_set_in_the_document_face() {
        let at = BOOT + 1_000;
        let html = page(&ledger(
            vec![
                tagged_begin(1, at, "files.read", None, "txn-1"),
                tagged_end(1, at + 100, true, "txn-1"),
            ],
            vec![txn("txn-1", "fix the parser", None, "committed", at)],
            vec![],
            None,
        ))
        .into_string();
        assert!(
            html.contains(r#"<h2 class="ledger-purpose">fix the parser</h2>"#),
            "the purpose the claim declared is the line under its time: {html}"
        );
        let css = css_of(&html);
        for selector in [
            ".ledger-purpose",
            ".receipt-big",
            ".activity-brand .brand-name",
        ] {
            let declarations = rule(css, selector);
            assert!(
                declarations.contains("font-family:var(--font)"),
                "{selector} is the document face: {declarations}"
            );
        }
    }

    /// **EVERY CONTROL CARRIES THE VISIBLE ACCENT OUTLINE, ON BOTH RUST PAGES.** A link, a button, the
    /// filter select and each disclosure summary of the ledger must show where the keyboard is. The
    /// shipped 2 px outline (`--accent`, 4.84:1 on the paper) is the ring; the refraction halo alone
    /// composites to 1.37:1 and is not a focus indicator. The panel's universal `:focus-visible`
    /// declares the same block, and both keep a `CanvasText` fallback for forced-colors sessions,
    /// which drop shadows and sometimes the author's colours.
    #[test]
    fn the_ledger_has_a_focus_ring_on_every_control() {
        let at = BOOT + 1_000;
        let html = page(&ledger(
            vec![
                tagged_begin(1, at, "files.read", None, "txn-1"),
                tagged_end(1, at + 100, true, "txn-1"),
            ],
            vec![txn("txn-1", "fix the parser", None, "committed", at)],
            vec![],
            None,
        ))
        .into_string();
        let css = css_of(&html);
        let focus = rule(css, "a:focus-visible");
        for selector in [
            "a:focus-visible",
            "button:focus-visible",
            "select:focus-visible",
            "summary:focus-visible",
        ] {
            assert!(
                focus.contains(selector),
                "{selector} is one of the controls the ring covers: {focus}"
            );
        }
        assert!(
            focus.contains("outline: 2px solid var(--accent)")
                && focus.contains("outline-offset: 2px"),
            "the focus ring is the shipped accent outline: {focus}"
        );
        assert!(
            !focus.contains("outline: none"),
            "the outline is not removed in favour of a 1.37:1 halo: {focus}"
        );
        let forced = forced_colors(css);
        assert!(
            forced.contains("outline: 2px solid CanvasText"),
            "a forced-colors session keeps a visible ring: {forced}"
        );
        for control in ["<a ", "<select", "<summary"] {
            assert!(
                html.contains(control),
                "the page draws the {control} control the ring must cover"
            );
        }
        // One system: the panel's own sheet declares exactly the same ring and fallback.
        let panel = rule(crate::panel::COMPONENTS, ":focus-visible");
        let declarations = |block: &str| {
            let open = block.find('{').expect("the rule opens");
            let close = block.rfind('}').expect("the rule closes");
            block[open + 1..close]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert_eq!(
            declarations(focus),
            declarations(panel),
            "the ledger and the panel draw one focus system: {focus} / {panel}"
        );
        assert!(
            forced_colors(crate::panel::COMPONENTS).contains("outline: 2px solid CanvasText"),
            "the panel keeps the same fallback"
        );
    }

    /// The rendered session ledger, from `#ledger` to the secondary section (or the transcript when
    /// the page has no secondary section).
    fn session_ledger_of(html: &str) -> &str {
        let start = html
            .find(r#"id="ledger""#)
            .expect("the session's ledger is drawn");
        let end = html
            .find(r#"class="older-activity""#)
            .or_else(|| html.find(r#"class="transcript""#))
            .expect("the document continues after the ledger");
        &html[start..end]
    }

    /// The rendered `<header class="activity-head">` element, for the identity line's assertions.
    fn header_of(html: &str) -> &str {
        let start = html
            .find(r#"<header class="activity-head local-topbar""#)
            .expect("the page draws its identity header");
        let end =
            html[start..].find("</header>").expect("the header closes") + start + "</header>".len();
        &html[start..end]
    }

    /// A session's goal is the document's title, with its claim and author above the ledger.
    #[test]
    fn a_sessions_goal_is_the_title_with_its_claim_and_author() {
        let at = BOOT + 10_000;
        let goal = "make the installer's config readable by the operator";
        let mut audit = Vec::new();
        audit.extend(txn_begin(10, at, "fix the parser", "txn-1"));
        audit.push(tagged_begin(11, at + 1_000, "files.read", None, "txn-1"));
        audit.push(tagged_end(11, at + 1_100, true, "txn-1"));
        let txns = vec![txn(
            "txn-1",
            "fix the parser",
            Some("session-1"),
            "working",
            at,
        )];
        let html = page(&ledger(
            audit,
            txns,
            vec![session_with_goal("session-1", "the operator", goal, "you")],
            None,
        ))
        .into_string();

        assert!(
            html.contains(r#"data-goal="set""#),
            "the document says its goal is set: {html}"
        );
        assert_eq!(
            html.matches(r#"<h1 class="goal-title">"#).count(),
            1,
            "the goal is the page's one h1, not a toolbar count: {html}"
        );
        assert!(
            html.contains(&format!(r#"<h1 class="goal-title">{goal}</h1>"#)),
            "the goal itself is the title: {html}"
        );
        assert!(
            html.contains("An authored session goal"),
            "the goal remains explicitly attributed as authored: {html}"
        );
        assert!(
            html.contains("set by you"),
            "the goal names its author: {html}"
        );
        let goal_at = html.find(goal).expect("the goal is drawn");
        let txn_at = html
            .find(r#"data-txn="txn-1""#)
            .expect("the session's transaction is a line");
        assert!(
            goal_at < txn_at,
            "the goal sits above the ledger it introduces: {html}"
        );
    }

    /// No goal is said, never filled in with the transaction's purpose.
    #[test]
    fn a_session_without_a_goal_says_so_and_never_borrows_the_purpose() {
        let at = BOOT + 10_000;
        let purpose = "fix the parser";
        let mut audit = Vec::new();
        audit.extend(txn_begin(10, at, purpose, "txn-1"));
        audit.push(tagged_begin(11, at + 1_000, "files.read", None, "txn-1"));
        audit.push(tagged_end(11, at + 1_100, true, "txn-1"));
        let txns = vec![txn("txn-1", purpose, Some("session-1"), "working", at)];
        let html = page(&ledger(
            audit,
            txns,
            vec![session_facts("session-1", "the operator", true)],
            None,
        ))
        .into_string();

        assert!(
            html.contains(r#"data-goal="none""#),
            "the document says the session has no goal: {html}"
        );
        assert!(
            html.contains(r#"<h1 class="goal-title none">No goal set for this session.</h1>"#),
            "the absence is said in the body face: {html}"
        );
        let goal_at = html
            .find(r#"class="goal""#)
            .expect("the goal block is drawn");
        let ledger_at = html
            .find(r#"id="ledger""#)
            .expect("the session's ledger follows the goal");
        let goal_block = &html[goal_at..ledger_at];
        assert!(
            !goal_block.contains(purpose),
            "the transaction's purpose stays in its own row, never the title: {goal_block}"
        );
        let css = css_of(&html);
        assert!(
            rule(css, ".goal-title").contains("font-family:var(--font)"),
            "a real goal is the document face: {}",
            rule(css, ".goal-title")
        );
        assert!(
            rule(css, ".goal-title.none").contains("font-family:var(--font);"),
            "the placeholder is the body face, never a serif headline: {}",
            rule(css, ".goal-title.none")
        );
    }

    /// No session is one line, not seventy-three rows.
    #[test]
    fn no_session_is_one_honest_line_and_the_empty_record_says_nothing_ran() {
        let html = page(&transcript(vec![], NOW)).into_string();

        assert!(
            html.contains(r#"data-goal="no-session""#),
            "the document has no session: {html}"
        );
        assert!(
            html.contains("No session on this machine — nothing has been run yet."),
            "the empty record says exactly that: {html}"
        );
        assert_eq!(
            html.matches("<h1 ").count(),
            1,
            "the honest statement is the document's one h1: {html}"
        );
        assert!(
            !html.contains(r#"class="ledger-entry""#),
            "an empty record draws no entry: {html}"
        );
    }

    /// The untransacted history is secondary and collapsed, never interleaved with the ledger.
    #[test]
    fn untransacted_rows_sit_in_a_collapsed_secondary_section() {
        let at = BOOT + 10_000;
        let mut audit = Vec::new();
        audit.extend(txn_begin(10, at, "fix the parser", "txn-1"));
        audit.push(tagged_begin(11, at + 1_000, "files.read", None, "txn-1"));
        audit.push(tagged_end(11, at + 1_100, true, "txn-1"));
        for index in 0..73u64 {
            let id = 100 + index * 2;
            let loose_at = at + 2_000 + index * 1_000;
            audit.push(begin(id, loose_at, "files.list", None));
            audit.push(end(id, loose_at + 50, true, None));
        }
        let txns = vec![txn(
            "txn-1",
            "fix the parser",
            Some("session-1"),
            "working",
            at,
        )];
        let html = page(&ledger(
            audit,
            txns,
            vec![session_facts("session-1", "the operator", true)],
            None,
        ))
        .into_string();

        let ledger = html
            .find(r#"id="ledger""#)
            .expect("the session's ledger is drawn");
        let older = html
            .find(r#"<details class="older-activity""#)
            .expect("the untransacted history is a secondary section");
        assert!(
            ledger < older,
            "the session's ledger closes before the secondary section: {html}"
        );
        let tag_end = html[older..].find('>').expect("the details' tag ends") + older;
        assert!(
            !html[older..=tag_end].contains(" open"),
            "the summary is the only line until a reader opens it: {}",
            &html[older..=tag_end]
        );
        let transcript = html
            .find(r#"class="transcript""#)
            .expect("the call list follows");
        let older_html = &html[older..transcript];
        assert!(
            older_html.contains("Other activity on this machine · 73 rows with no transaction"),
            "the summary says how much it holds and calls it secondary: {older_html}"
        );
        assert!(
            older_html.contains("Rows with no transaction") && older_html.contains("· 73 rows"),
            "the loose rows are named as a group of 73: {older_html}"
        );
        assert!(
            older_html.contains(
                "Not this session’s ledger: machine-scope work belongs to the console, the panel or the install door. Rows with no transaction may be session creation, exempt calls, pre-dispatch refusals or older records."
            ),
            "one sentence says why they are not the session's ledger: {older_html}"
        );
        assert_eq!(
            older_html.matches(r#"class="ledger-unknown-row""#).count(),
            73,
            "the rows themselves are still there: {older_html}"
        );
    }

    /// A record with rows but no session does not claim nothing ran.
    #[test]
    fn no_session_with_a_record_names_the_record_not_a_fresh_machine() {
        let at = BOOT + 10_000;
        let mut audit = Vec::new();
        for index in 0..73u64 {
            let id = 100 + index * 2;
            let loose_at = at + index * 1_000;
            audit.push(begin(id, loose_at, "files.list", None));
            audit.push(end(id, loose_at + 50, true, None));
        }
        let html = page(&transcript(audit, NOW)).into_string();

        assert!(
            html.contains("No session on this machine — the work in this record names no session."),
            "a record with rows is not a fresh machine: {html}"
        );
        assert!(
            !html.contains("nothing has been run yet"),
            "the page never claims nothing ran above a record of rows: {html}"
        );
    }

    /// The header names the machine and the running release, and keeps live honest.
    #[test]
    fn the_header_names_the_host_the_release_and_the_live_state() {
        let html = page(&transcript(vec![], NOW)).into_string();
        let header = header_of(&html);

        assert!(
            header.contains(r#"data-host="DESKTOP-9VH92BJ""#)
                && header.contains(r#"data-version="2.0.15""#)
                && header.contains(r#"data-channel="canary""#),
            "the header carries the machine's own name and the launcher's release facts: {header}"
        );
        assert!(
            header.contains("DESKTOP-9VH92BJ") && header.contains("2.0.15"),
            "the host pill and the release are drawn: {header}"
        );
        assert!(
            header.contains(
                r#"<span id="activity-state" role="status" aria-live="polite">live</span>"#
            ),
            "the live state is the span the view script rewrites: {header}"
        );
        assert!(
            !header.contains("last 200"),
            "the header is identity, not the machine-wide toolbar: {header}"
        );
    }

    #[test]
    fn an_empty_period_says_nothing_has_been_run() {
        let html = page(&ledger(Vec::new(), Vec::new(), vec![], None)).into_string();

        assert!(
            html.contains(r#"data-state="empty""#),
            "an audit with no lines is empty: {html}"
        );
        assert!(
            html.contains(r#"data-goal="no-session""#)
                && html.contains("No session on this machine — nothing has been run yet."),
            "the document says the machine has run nothing: {html}"
        );
        assert!(
            !html.contains(r#"class="ledger-entry""#)
                && !html.contains(r#"class="older-activity""#)
                && !html.contains("No receipt yet"),
            "an empty period invents no entry and no secondary section: {html}"
        );
    }

    /// **THE EFFECTS TABLE IS THE CONTRACT.** Each of the ten steps below exercises one row of it:
    /// a failed write wrote nothing, a running command still ran while a refused one did not,
    /// `monitor.add` watches a port without binding one, and `files.read` and `terminal.*` are not
    /// effects at all. The line is the transaction's own steps, all four printed, zero included.
    ///
    /// The real failed-child path is covered through the dispatcher and HTTP route in `http`.
    /// This table also retains the accepted provisional count for an allowed running command.
    #[test]
    fn the_effects_line_is_derived_from_the_transactions_own_steps() {
        let at = BOOT + 10_000;
        let mut audit = Vec::new();
        audit.extend(txn_begin(10, at, "make the importer idempotent", "txn-1"));
        audit.extend([
            tagged_begin(11, at + 1_000, "files.write", Some("path=a.toml"), "txn-1"),
            tagged_end(11, at + 1_100, true, "txn-1"),
            tagged_begin(12, at + 2_000, "files.write", Some("path=b.toml"), "txn-1"),
            tagged_failure(12, at + 2_100, "txn-1"),
            tagged_begin(13, at + 3_000, "files.read", Some("path=a.toml"), "txn-1"),
            tagged_end(13, at + 3_100, true, "txn-1"),
            tagged_begin(14, at + 4_000, "runs.exec", Some("cargo test"), "txn-1"),
            tagged_end(14, at + 4_500, true, "txn-1"),
            // No end line: still running, and it still ran.
            tagged_begin(15, at + 5_000, "runs.exec", Some("cargo fmt"), "txn-1"),
            // The policy denied this command before dispatch.
            denied_call(16, at + 6_000, "runs.exec", "denied", "txn-1")[0].clone(),
            denied_call(16, at + 6_000, "runs.exec", "denied", "txn-1")[1].clone(),
            tagged_begin(17, at + 7_000, "forward.add", None, "txn-1"),
            tagged_end(17, at + 7_100, true, "txn-1"),
            tagged_begin(18, at + 8_000, "monitor.add", None, "txn-1"),
            tagged_end(18, at + 8_100, true, "txn-1"),
            tagged_begin(19, at + 9_000, "browser.open", None, "txn-1"),
            tagged_end(19, at + 9_100, true, "txn-1"),
            tagged_begin(20, at + 10_000, "browser.open", None, "txn-1"),
            tagged_failure(20, at + 10_100, "txn-1"),
        ]);
        let html = page(&ledger(
            audit,
            vec![txn(
                "txn-1",
                "make the importer idempotent",
                Some("session-1"),
                "working",
                at,
            )],
            vec![session_facts("session-1", "the operator", true)],
            None,
        ))
        .into_string();

        assert!(
            html.contains(
                r#"<p class="effects" data-effects="files:1 processes:2 ports:1 windows:1" title="counted from the calls shown in this window">files 1 written · processes 2 run · forwarding rules recorded 1 · windows 1</p>"#
            ),
            "the effects line counts the transaction's own steps by the table: {html}"
        );
        assert!(
            html.contains("Observed in this window: 10 calls · 3 failures · 1 refused"),
            "the observed line counts every step, refusals included: {html}"
        );
    }

    #[test]
    fn an_old_refused_run_reports_execution_uncertainty_without_a_gate_refusal() {
        let at = BOOT + 10_000;
        let audit = vec![
            tagged_begin(11, at, "runs.exec", Some("old command"), "txn-1"),
            tagged_end(11, at + 100, false, "txn-1"),
        ];
        let html = page(&ledger(
            audit,
            vec![txn(
                "txn-1",
                "retained work",
                Some("session-1"),
                "committed",
                at,
            )],
            vec![],
            None,
        ))
        .into_string();
        assert!(
            html.contains("processes 0 confirmed run (1 execution unknown)"),
            "{html}"
        );
        assert!(
            html.contains("Observed in this window: 1 call · 1 failure · 0 refused"),
            "{html}"
        );
    }

    /// **A DOOR-REFUSED COMMAND DID NOT RUN.** The gate's refusals (`txn_required`,
    /// `txn_session_mismatch`, `txn_ended`) deny the begin and never reach the command, and their
    /// end carries the gate's `error_kind`, not `"refused"`. `processes` counts what ran, so the
    /// denied call is zero processes while the row itself stays drawn.
    #[test]
    fn a_door_refused_command_is_not_a_process() {
        let at = BOOT + 10_000;
        let mut audit = Vec::new();
        audit.extend(txn_begin(10, at, "publish the release", "txn-1"));
        audit.extend(denied_call(
            11,
            at + 1_000,
            "runs.exec",
            "txn_required",
            "txn-1",
        ));
        let html = page(&ledger(
            audit,
            vec![txn(
                "txn-1",
                "publish the release",
                Some("session-1"),
                "working",
                at,
            )],
            vec![session_facts("session-1", "the operator", true)],
            None,
        ))
        .into_string();

        assert!(
            html.contains(
                "files 0 written · processes 0 run · forwarding rules recorded 0 · windows 0"
            ),
            "a command the gate refused is not a process: {html}"
        );
        assert!(
            html.contains(r#"<div class="step" data-kind="fact" data-status="failed">"#)
                && html.contains("runs.exec"),
            "the refused call is still drawn as its row: {html}"
        );
    }

    /// **THE TOTALS ARE THE SESSION'S OWN.** Transactions are the selected session's groups; calls
    /// and refusals are each retained receipt's totals; files written are the successful writes in
    /// the session's groups. There is no port count here: a port is one transaction's effect.
    #[test]
    fn the_goal_totals_are_the_sessions_own() {
        let at = BOOT + 10_000;
        let mut audit = Vec::new();
        audit.extend(txn_begin(10, at, "fix the parser", "txn-1"));
        audit.extend(txn_begin(20, at + 30_000, "verify the suite", "txn-2"));
        audit.extend([
            tagged_begin(11, at + 1_000, "files.write", Some("path=a.toml"), "txn-1"),
            tagged_end(11, at + 1_100, true, "txn-1"),
        ]);
        let mut first = txn(
            "txn-1",
            "fix the parser",
            Some("session-1"),
            "committed",
            at,
        );
        first.calls = 2;
        let mut second = txn(
            "txn-2",
            "verify the suite",
            Some("session-1"),
            "committed",
            at + 30_000,
        );
        second.calls = 2;
        second.refusals = 1;
        let html = page(&ledger(
            audit,
            vec![first, second],
            vec![session_facts("session-1", "the operator", true)],
            None,
        ))
        .into_string();

        assert!(
            html.contains(
                r#"<p class="goal-totals" title="calls and refusals are the receipts' totals; files written are the successful `files.write` calls in this window.">2 transactions · 4 calls · 1 refused · 1 file written</p>"#
            ),
            "the totals are the session's receipts and its own effects: {html}"
        );
    }

    /// **ONE CALL IS ONE CALL EVERYWHERE.** Every count site in the module speaks one plural rule;
    /// the label-first effects line is the only deliberate exception.
    #[test]
    fn one_call_is_one_call_everywhere() {
        let at = BOOT + 10_000;
        let mut audit = Vec::new();
        audit.extend(txn_begin(10, at, "fix the parser", "txn-1"));
        audit.extend([
            tagged_begin(11, at + 1_000, "runs.exec", Some("cargo test"), "txn-1"),
            tagged_end(11, at + 1_100, false, "txn-1"),
            begin(30, at + 2_000, "files.list", None),
            end(30, at + 2_100, true, None),
        ]);
        let mut only = txn("txn-1", "fix the parser", Some("session-1"), "working", at);
        only.calls = 1;
        only.failures = 1;
        only.refusals = 0;
        let html = page(&ledger(
            audit,
            vec![only],
            vec![session_facts("session-1", "the operator", true)],
            None,
        ))
        .into_string();

        for line in [
            "Observed in this window: 1 call · 1 failure · 0 refused",
            "1 failure so far · 0 refused",
            "1 transaction · 1 call",
            "1 row with no transaction",
        ] {
            assert!(html.contains(line), "the page says {line}: {html}");
        }
        for wrong in [
            "1 calls",
            "1 failures",
            "1 rows",
            "1 transactions",
            "1 files",
            "1 refusals",
        ] {
            assert!(!html.contains(wrong), "the page says {wrong}: {html}");
        }
    }
}
