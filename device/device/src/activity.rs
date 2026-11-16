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
                footer class="activity-foot" {
                    span { "polls every second" }
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

/// A duration the way the transcript prints it: milliseconds under a second, else seconds.
fn duration_text(duration_ms: u64) -> String {
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
}
