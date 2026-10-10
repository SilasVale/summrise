#!/usr/bin/env node
// Summrise Agent E2E suite — THE DRIVER.
//
// ── WHAT THIS FILE IS, AND WHAT IT IS NOT ────────────────────────────────────────────────────────────────────
//
// IT IS THE PLATFORM HALF, AND IT DECIDES NOTHING. It opens PTY sessions over the product's own HTTP surface,
// drives the desktop SPA over CDP 9333, hands `browser_run_script` its payloads, talks to playwright-mcp on two
// transports, and reads and writes the device's own files. Then it writes down what it ASKED and what came
// back, and stops.
//
// IT IS NOT THE VERDICT AND IT IS NOT THE PLAN.
//
//   * THE PLAN is Rust (`agent/e2e-judge/src/plan.rs`): which sections run, in what order, what `--only` may
//     name, every command and marker the suite types or requires, and the policy caps and floors (poll counts,
//     retry counts and the waits between them). It arrives as DATA — `--plan <file>`, or the constant
//     `BUNDLED_PLAN` below, which `summrise-e2e-judge --bundle` fills in when it writes the copy a device
//     fetches. This file has no fixture of its own and no section ladder of its own.
//   * THE JUDGE is Rust too (`--rows`): the conditions, the sentences a reader triages a failure from, the two
//     skip arms and the exit code.
//
// WHY THE SPLIT, IN THE REPOSITORY'S OWN WORDS. A decision is Rust; a platform call that decides nothing stays
// JavaScript and says which call it performs. This file used to hold BOTH halves: 75 `check(name, cond, detail)`
// calls were decisions (`.some(...)`, `rs === 'OK'`, `text.includes('E2E-SESSION-OK')`, `state === 'partial'`)
// sitting beside the fetches that fed them — and the design sweeps had already made this exact cut, their
// in-page COLLECTOR staying JavaScript while the JUDGE and the PLAN became Rust. `--record` existed for this
// port; the record is now the only output there is.
//
// ── THE TWO THINGS THIS FILE IS STILL ALLOWED, AND WHY NEITHER IS A VERDICT ──────────────────────────────────
//
//   1. A POLL'S TERMINATION CONDITION. A driver must know when to stop driving: whether the shell has produced
//      a prompt, whether the embedded view has arrived, whether the address bar has caught up. Those are
//      built out of the plan's own data (`panel_marker_prefix`, `mcp_click_targets`, `browser_bar_fragment`)
//      and the series they produce is RE-JUDGED in Rust — which is why the series is recorded rather than
//      collapsed into a boolean here.
//   2. A HANDLE READ OUT OF AN ANSWER, so the next call can be made at all: the session id `terminal_open`
//      answers, the `[ref=…]` a snapshot carries for `browser_click`. A driver that could not read the handle
//      could not drive.
//
// What is never computed here is what an answer MEANS.
//
// ── TWO THINGS A READER OF A DEVICE TRANSCRIPT NEEDS TO KNOW ────────────────────────────────────────────────
//
//   1. THE DRIVER'S EXIT CODE SAYS WHETHER IT COLLECTED, NOT WHETHER THE DEVICE PASSED. 0 = the rows were
//      written; 1 = no token; 2 = it could not run at all (no plan, or a section it does not implement). A run
//      that aborted mid-way still writes its rows, and those rows carry `driver.abort`, which the judge reports
//      as a failure. Run `summrise-e2e-judge --rows <file>` for the verdict — this process does not have one.
//   2. EVERY CHECK IS JUDGED FROM THE ROWS, SO THE ROWS MUST LEAVE THE DEVICE. On a box with a toolchain that
//      is the next command. On the device it means carrying the file, which is the same problem the recorded
//      corpus has; it is named in `agent/scripts/e2e/README.md` rather than left to be discovered.
//
// Usage:
//   node e2e.js --token <agent-token> [--base http://127.0.0.1:18080] [--rows e2e-rows.json]
//   node e2e.js --plan plan.json --token <t> --rows out.json      # a subset: the plan is data
//
// Requires: agent running on the device; for the CDP sections also the Electron desktop (9333) and the bundled
// Playwright. The emitted bundle (`summrise-e2e-judge --bundle`) carries the plan and needs only `--token`.

const fs = require('node:fs');

// ── THE PLAN ────────────────────────────────────────────────────────────────────────────────────────────────
//
// THE LINE BELOW IS THE EMITTER'S MARKER, and it is EXACT: `summrise-e2e-judge --bundle` refuses to emit unless
// it appears exactly once, and replaces it with the plan as JSON. The source carries `null` on purpose, so
// running the source with no `--plan` REFUSES rather than inventing a section order that Rust would then judge
// as though the caller had asked for it.
const BUNDLED_PLAN = null; // @e2e-plan

const argv = process.argv.slice(2);
const flag = (name) => {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : null;
};
const has = (name) => argv.includes(name);

const TOKEN = flag('--token') || process.env.SUMMRISE_AGENT_TOKEN;
const BASE = flag('--base') || 'http://127.0.0.1:18080';
const ROWS_PATH = flag('--rows') || 'e2e-rows.json';
const PLAN_PATH = flag('--plan');

// **`--only` AND `--no-browser` ARE REFUSED HERE, AND THAT IS THE POINT.** Both are decisions about WHICH
// sections run, and the vocabulary they are checked against lives in Rust — the guard that refuses a name
// selecting nothing was written because `--only governance-typo` used to be indistinguishable from success.
// Re-implementing the filter here would put that decision back in JavaScript. Ask the judge for a plan instead;
// the message says how.
if (has('--only') || has('--no-browser')) {
  console.error(
    'the section list is DATA and this file cannot re-derive it: --only and --no-browser are resolved by the\n' +
      'judge, not by the driver.\n' +
      '  summrise-e2e-judge --plan --only <a,b> [--no-browser] > plan.json\n' +
      '  node e2e.js --plan plan.json --rows out.json --token <token>\n' +
      'The bundled copy a device runs already carries the full plan; use --plan for a subset.',
  );
  process.exit(2);
}

let PLAN = BUNDLED_PLAN;
if (PLAN_PATH) {
  try {
    PLAN = JSON.parse(fs.readFileSync(PLAN_PATH, 'utf8'));
  } catch (e) {
    console.error('cannot read the plan ' + PLAN_PATH + ': ' + e.message);
    process.exit(2);
  }
}
if (!PLAN || !Array.isArray(PLAN.sections) || !PLAN.fixtures || !PLAN.budgets) {
  console.error(
    'no plan. This driver runs what it is given and refuses to invent an order:\n' +
      '  summrise-e2e-judge --plan > plan.json && node e2e.js --plan plan.json --token <token>\n' +
      'or run the emitted bundle, which carries the plan inside it.',
  );
  process.exit(2);
}
const FX = PLAN.fixtures;
const B = PLAN.budgets;

// The two runtime overrides the suite has always carried, with their DEFAULTS in the plan: a device that keeps
// its evidence or its Playwright somewhere else says so in the environment, and the driver just reads it.
const EVIDENCE_DIR = process.env.SUMMRISE_EVIDENCE_DIR || FX.evidence_dir;
const PW_DIR = process.env.SUMMRISE_PW_DIR || FX.pw_dir;

if (!TOKEN) {
  console.error('missing token: pass --token or SUMMRISE_AGENT_TOKEN');
  process.exit(1);
}

const H = { Authorization: 'Bearer ' + TOKEN, 'Content-Type': 'application/json' };
const START = Date.now();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const jstr = (v) => JSON.stringify(v);

// ── THE RECORD ──────────────────────────────────────────────────────────────────────────────────────────────
//
// One entry per question asked. `args` is what was asked, `result` is what came back, `error` is the transport
// failure that used to `throw` out of the suite. Nothing here is derived: no condition is evaluated and no
// detail string is composed, because both of those are the judge's.
const observations = [];
function record(id, fields) {
  const o = { id, at_ms: Date.now() - START };
  if (fields.args !== undefined) o.args = fields.args;
  if (fields.result !== undefined) o.result = fields.result;
  if (fields.error !== undefined) o.error = fields.error;
  observations.push(o);
  console.log('obs ' + id);
  return fields.result;
}

/** A run-stopping failure. The JS suite threw out of its one `try` and reported `suite: <message>`; the judge
 *  reports the same thing from `driver.abort`, so the message stays the raw one rather than a new invention. */
class Stop extends Error {}

/**
 * ONE CALL TO THE PRODUCT'S OWN TOOL SURFACE, in the shape external AI clients use: POST /api/tools/{name}
 * with the args object as the body (no `{tool,args}` wrapper).
 *
 * `bestEffort` is the JS suite's `.catch(() => {})` — cleanup it does not depend on. Everything else stops the
 * run, which is what the suite did: one `try` wrapped every section, so a failed call ended it.
 */
async function tool(id, name, body, opts) {
  const args = body || {};
  let result;
  try {
    const r = await fetch(BASE + '/api/tools/' + name, {
      method: 'POST',
      headers: H,
      body: JSON.stringify(args),
    });
    if (!r.ok) throw new Error('tool ' + name + ' HTTP ' + r.status);
    const j = await r.json();
    if (j && j.ok === false) throw new Error('tool ' + name + ' failed: ' + jstr(j).slice(0, 200));
    result = j && j.result !== undefined ? j.result : j;
  } catch (e) {
    const message = String((e && e.message) || e);
    record(id, { args: (opts && opts.recordArgs) || args, error: message });
    if (opts && opts.bestEffort) return null;
    throw new Stop(message);
  }
  return record(id, { args: (opts && opts.recordArgs) || args, result });
}

/** A session-control call: the JS read these as `{status, json}` and the checks assert on the STATUS too. */
async function ctl(id, sessionId, body) {
  const path = '/api/sessions/' + encodeURIComponent(sessionId) + '/control';
  const r = await fetch(BASE + path, { method: 'POST', headers: H, body: JSON.stringify(body) });
  return record(id, { args: body, result: { status: r.status, json: await jsonOrNull(r) } });
}

/** An endpoint the JS read with a bare `.json()`: approval, grants, the session trail, the run timeline. */
async function jsonCall(id, method, path, body) {
  const r = await fetch(BASE + path, {
    method,
    headers: H,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  return record(id, {
    args: body === undefined ? { method, path } : { method, path, body },
    result: await jsonOrNull(r),
  });
}

async function jsonOrNull(resp) {
  try {
    return await resp.json();
  } catch (e) {
    return null;
  }
}

/** The CDP target list the SPA and the embedded view are found in. */
async function cdpTargets(id) {
  const r = await fetch('http://127.0.0.1:9333/json/list');
  return record(id, { args: { url: 'http://127.0.0.1:9333/json/list' }, result: await r.json() });
}

/** A GENERATED VALUE IS AN OBSERVATION TOO. The marker suffixes are the run's own clock; the plan carries only
 *  the prefix, so the judge learns the value the same way it learns every other fact — from the record. */
const mint = (id, value) => record(id, { result: value });

/** `Write-Output "<marker>"` and friends: the TEMPLATE is the plan's, the value is the run's. */
const fill = (template, values) =>
  String(template).replace(/\{(\w+)\}/g, (_, k) => (values[k] === undefined ? '' : String(values[k])));

// ── CDP, OVER THE SPA'S OWN DEBUGGER SOCKET ────────────────────────────────────────────────────────────────
//
// The evaluate round trip and the close handshake are platform calls. The EXPRESSIONS are page-side source
// strings — the half the sweeps also left in JavaScript — and what they return is read here only far enough to
// decide whether to keep polling.

async function evalInSpa(ws, id, expression, timeoutMs) {
  const r = await new Promise((resolve) => {
    const t = setTimeout(() => resolve(null), timeoutMs);
    const onMsg = (m) => {
      let o;
      try {
        o = JSON.parse(m.data);
      } catch (e) {
        return;
      }
      if (o.id === id) {
        clearTimeout(t);
        ws.removeEventListener('message', onMsg);
        resolve(o);
      }
    };
    ws.addEventListener('message', onMsg);
    ws.send(JSON.stringify({ id, method: 'Runtime.evaluate', params: { expression, returnByValue: true } }));
  });
  const raw = r && r.result && r.result.result && r.result.result.value;
  return raw === undefined || raw === null ? null : raw;
}

/** Open a socket to the desktop SPA target, if there is one. */
async function spaSocket() {
  const list = await (await fetch('http://127.0.0.1:9333/json/list')).json();
  const spa = (list || []).find((t) => t.url.includes('/desktop/'));
  if (!spa) return null;
  const ws = new WebSocket(spa.webSocketDebuggerUrl);
  await new Promise((res, rej) => {
    ws.onopen = res;
    ws.onerror = rej;
  });
  return ws;
}

/** Closing a CDP WebSocket immediately after `send` can trip Node's `UV_HANDLE_CLOSING` assert on Windows, so
 *  the close is AWAITED rather than fired and forgotten. */
async function closeSocket(ws) {
  await new Promise((resolve) => {
    ws.onclose = resolve;
    setTimeout(() => {
      try {
        ws.close();
      } catch (e) {}
    }, B.ws_close_immediate_ms);
    setTimeout(resolve, B.ws_close_grace_ms);
  });
}

// ── §1 terminal: session execute + background collect ───────────────────────────────────────────────────────

async function sectionTerminal() {
  const sid = await tool('terminal.open', 'terminal_open', { kind: 'pty' });
  const sessionId = typeof sid === 'string' ? sid : sid && sid.sid;
  if (!sessionId) throw new Stop('terminal_open answered no session id');

  // A REAL GATE, NOT A SLEEP WEARING ONE'S NAME (round 113). A fixed wait is not a gate: under load the first
  // prompt takes longer, the command lands in a shell still starting, and the device is reported broken when
  // nothing is wrong with it.
  for (let i = 0; i < B.prompt_polls; i++) {
    await sleep(B.prompt_interval_ms);
    const sc = await tool('terminal.prompt.' + i, 'terminal_read', { session_id: sessionId, offset: 0 });
    const txt = (sc && (sc.text || sc)) || '';
    if (String(txt).trim().length > 0) break;
  }

  await tool('terminal.list.sessions', 'terminal_list', {});
  await tool('terminal.resize', 'terminal_resize', { session_id: sessionId, rows: 30, cols: 120 });

  // TWO ATTEMPTS, because a command that arrives while the shell is still settling can legitimately come back
  // partial — and the proof stays strict: the marker must actually appear.
  for (let attempt = 0; attempt < B.execute_attempts; attempt++) {
    if (attempt > 0) await sleep(B.execute_retry_ms);
    const ex = await tool('terminal.execute.' + attempt, 'terminal_execute', {
      command: FX.session_command,
      session_id: sessionId,
      timeout_secs: 30,
    });
    if (ex && ex.state === 'done' && String(ex.text || '').includes(FX.session_marker)) break;
  }

  await tool('terminal.row.after_exec', 'terminal_list', {});
  await tool('terminal.exec.fail', 'terminal_execute', {
    command: FX.failing_command,
    session_id: sessionId,
    timeout_secs: 20,
  });
  await tool('terminal.row.after_fail', 'terminal_list', {});

  await tool('terminal.exec.bg', 'terminal_execute', {
    command: FX.background_command,
    session_id: sessionId,
    run_in_background: true,
  });
  await sleep(B.background_collect_ms);
  await tool('terminal.read.bg', 'terminal_read', { session_id: sessionId });

  // The keystroke path (not execute): raw text + CRLF, the shell runs it, and the tail view shows the echo.
  const wmarker = mint('mint.terminal_write', FX.write_marker_prefix + Date.now());
  await tool('terminal.write', 'terminal_write', {
    session_id: sessionId,
    data: fill(FX.write_command_template, { marker: wmarker }),
  });
  await sleep(B.screen_settle_ms);
  await tool('terminal.screen', 'terminal_screen', { session_id: sessionId });

  // An unknown session must report `evicted` — not throw, and not empty-ok.
  await tool('terminal.read.unknown', 'terminal_read', { session_id: FX.unknown_session });
  await tool('terminal.close', 'terminal_close', { session_id: sessionId }, { bestEffort: true });
  await tool('terminal.history', 'terminal_history', {});
}

// ── §2 file: stat + append-up + paged raw read down ────────────────────────────────────────────────────────

async function sectionFile() {
  const path = EVIDENCE_DIR + '\\' + FX.transfer_name;
  const chunk = Buffer.alloc(FX.transfer_page_bytes, FX.transfer_fill).toString('base64');
  for (let page = 0; page < FX.transfer_pages; page++) {
    const body = { path, data: chunk };
    if (page > 0) body.append = true;
    await tool('file.write.page.' + page, 'system_file_write', body);
  }
  await tool('file.stat', 'system_file_stat', { path });
  await tool('file.read.raw', 'system_file_read', { path, offset: 0, limit: 1048576, raw: true });
  await tool('file.list', 'system_file_list', { path: EVIDENCE_DIR });

  // A missing path is a data-shaped `{ok:false}`, not a transport error.
  await tool('file.stat.missing', 'system_file_stat', { path: EVIDENCE_DIR + '\\' + FX.missing_name });

  // Text mode, not only base64 pages.
  const tpath = EVIDENCE_DIR + '\\' + FX.text_name;
  await tool('file.text.write', 'system_file_write', { path: tpath, text: FX.text_payload });
  await tool('file.text.read', 'system_file_read', { path: tpath });

  // The suite must not litter the device.
  for (const p of [path, tpath]) {
    try {
      fs.unlinkSync(p);
    } catch (e) {}
  }
}

// ── §3 workflow: the cross-plugin chain ─────────────────────────────────────────────────────────────────────

async function sectionWorkflow() {
  await tool('workflow.process_list', 'system_process_list', { name: 'electron' });
  await tool('workflow.execute', 'terminal_execute', { command: FX.workflow_command, timeout_secs: 15 });

  const cfg = jstr({ e2e: true, ts: Date.now() });
  mint('mint.workflow_config', cfg);
  const wpath = EVIDENCE_DIR + '\\' + FX.workflow_name;
  await tool('workflow.file.write', 'system_file_write', { path: wpath, text: cfg });
  await tool('workflow.file.stat', 'system_file_stat', { path: wpath });

  const tok = mint('mint.memory_token', FX.memory_token_prefix + Date.now());
  const ms = await tool('workflow.memory.save', 'memory_save', {
    title: FX.memory_title,
    content: tok,
    tags: FX.memory_tags,
  });
  await tool('workflow.memory.search', 'memory_search', { query: FX.memory_title });
  await tool('workflow.memory.list', 'memory_list', { limit: FX.memory_list_limit });

  // Update the entry, then prove the NEW token is searchable.
  const tok2 = mint('mint.memory_update', FX.memory_update_prefix + Date.now());
  await tool('workflow.memory.update', 'memory_update', { id: ms && ms.id, content: tok2 });
  await tool('workflow.memory.search.updated', 'memory_search', { query: tok2 });
  await tool('workflow.memory.export', 'memory_export', {});

  // Self-clean so repeated runs do not accumulate entries (device-caught round 294), and then search the token
  // again: whether the delete TOOK is a verdict and it is the judge's; issuing it is driving.
  if (ms && ms.id) {
    await tool('workflow.memory.delete', 'memory_delete', { id: ms.id }, { bestEffort: true });
  }
  await tool('workflow.memory.search.deleted', 'memory_search', { query: tok2 });
  try {
    fs.unlinkSync(wpath);
  } catch (e) {}
}

// ── §4b panel: does the desktop SPA actually SHOW what the AI wrote? ────────────────────────────────────────

async function sectionPanel() {
  const marker = mint('mint.panel_marker', FX.panel_marker_prefix + Date.now());
  const sid = await tool('panel.open', 'terminal_open', { kind: 'pty' });
  const sessionId = typeof sid === 'string' ? sid : sid && sid.sid;
  if (!sessionId) throw new Stop('terminal_open answered no session id');
  await sleep(B.panel_open_settle_ms);
  await tool('panel.exec', 'terminal_execute', {
    command: fill(FX.panel_command_template, { marker }),
    session_id: sessionId,
    timeout_secs: 20,
  });

  // Read the SPA's xterm DOM: the marker must be visible in the panel.
  await sleep(B.panel_list_settle_ms);
  const targets = await cdpTargets('panel.cdp.targets');
  const spa = (targets || []).find((t) => t.url.includes('/desktop/'));
  if (!spa) {
    await tool('panel.close', 'terminal_close', { session_id: sessionId }, { bestEffort: true });
    return;
  }

  let ws = null;
  try {
    ws = new WebSocket(spa.webSocketDebuggerUrl);
    await new Promise((res, rej) => {
      ws.onopen = res;
      ws.onerror = rej;
    });
    // Make sure the SPA is on the Terminal page, then click the LAST session tab — the session just opened is
    // the newest, and the visible xterm follows the ACTIVE tab. SCOPED TO `.dtab`, because `role=tab` is ALSO
    // the view switch's (Terminal / Trajectory / Path) and an unscoped `[length-1]` clicks the Path PAGE.
    mint(
      'panel.rail.click',
      await evalInSpa(
        ws,
        200,
        "(function(){ var bs = document.querySelectorAll('button, [role=button], [class*=rail] > *'); for (var i=0;i<bs.length;i++){ var el = bs[i]; var t = (el.getAttribute('aria-label')||el.title||el.textContent||'').trim(); if (t === 'Terminal' || t.indexOf('Terminal') === 0 && t.length < 12) { el.click(); return 'clicked'; } } return 'no-rail'; })()",
        B.ws_eval_timeout_ms,
      ),
    );
    await sleep(B.panel_rail_settle_ms);
    mint(
      'panel.tab.click',
      await evalInSpa(
        ws,
        205,
        "(function(){ var ts = document.querySelectorAll('.dtab[role=tab]'); if (!ts.length) return 'no-tabs'; ts[ts.length - 1].click(); return 'clicked-last-session'; })()",
        B.ws_eval_timeout_ms,
      ),
    );
    await sleep(B.panel_tab_settle_ms);

    // The visible xterm's text (round-264 method: a visible `.term-host`'s `.xterm-rows` spans — hidden hosts
    // exist per session). The stop rule is "the marker FAMILY is on screen"; WHICH marker, and whether it is
    // the right one, is decided in Rust from the recorded series.
    for (let attempt = 0; attempt < B.panel_xterm_polls; attempt++) {
      const raw = await evalInSpa(
        ws,
        201 + attempt,
        "(function(){ var hosts = document.querySelectorAll('.term-host'); var vis = null; for (var i=0;i<hosts.length;i++){ var r = hosts[i].getBoundingClientRect(); if (r.width > 50 && r.height > 50) { vis = hosts[i]; break; } } if (!vis) return 'NO_VISIBLE'; var rows = vis.querySelectorAll('.xterm-rows > div'); var all = ''; for (var j=0;j<rows.length;j++){ all += rows[j].textContent + String.fromCharCode(10); } return all; })()",
        B.ws_eval_timeout_ms,
      );
      record('panel.xterm.' + attempt, { result: raw });
      if (typeof raw === 'string' && raw.includes(FX.panel_marker_prefix)) break;
      await sleep(B.panel_xterm_interval_ms);
    }
  } finally {
    await tool('panel.close', 'terminal_close', { session_id: sessionId }, { bestEffort: true });
    if (ws) await closeSocket(ws);
  }
}

// ── §4c2 mcp: connect auto-selects the embedded view (round-281) ────────────────────────────────────────────
//
// Shared: connect -> immediate navigate -> the embedded view must reach the marker URL with NO manual tab
// select, the SPA must stay intact, and a real snapshot-ref CLICK must drive it too.

async function mcpProbe(tag, connArgs) {
  // round-306: start CLEAN — a leftover connection from a previous run makes connect return already_connected
  // and the check fails spuriously (observed repeatedly on long-running devices).
  await tool('mcp.' + tag + '.disconnect', 'mcp_client_disconnect', {}, { bestEffort: true });
  await sleep(B.mcp_disconnect_settle_ms);
  const marker = mint('mcp.' + tag + '.mint', tag + '-autoselect-' + Date.now());
  const c = await tool('mcp.' + tag + '.connect', 'mcp_client_connect', connArgs);
  if (!c || c.status !== 'connected') return;

  await tool('mcp.' + tag + '.navigate', 'mcp_client_call', {
    tool: 'browser_navigate',
    arguments: { url: fill(FX.marker_url_template, { marker }) },
  });

  // Poll rather than sleep: after a transport switch the view can still show the previous probe's page for
  // several seconds (device-caught: an http probe kept seeing the stdio probe's landing past a fixed 6 s).
  let embedded = null;
  for (let i = 0; i < B.mcp_view_polls; i++) {
    await sleep(B.mcp_view_interval_ms);
    let list = null;
    try {
      list = await (await fetch('http://127.0.0.1:9333/json/list')).json();
    } catch (e) {
      record('mcp.' + tag + '.view.' + i, {
        args: { url: 'http://127.0.0.1:9333/json/list' },
        error: String((e && e.message) || e),
      });
      continue;
    }
    record('mcp.' + tag + '.view.' + i, { args: { url: 'http://127.0.0.1:9333/json/list' }, result: list });
    embedded = (list || []).find((t) => !t.url.includes('/desktop/'));
    if (embedded && embedded.url.includes(marker)) break;
  }

  // The marker URL above is example.com/<marker> (a 404 page with no links), so navigate to the HOMEPAGE first,
  // then inject a BIG same-origin link and click THAT: a real snapshot-ref mouse click driving a real
  // navigation. The external "Learn more" link is the fallback, because its cross-origin redirect chain flakes
  // under load (device-caught: ok clicks with valid geometry that never commit, while same-origin ones land).
  await tool('mcp.' + tag + '.navigate.home', 'mcp_client_call', {
    tool: 'browser_navigate',
    arguments: { url: FX.mcp_home_url },
  });
  await sleep(B.mcp_nav_settle_ms);
  const inject =
    "() => { document.body.innerHTML = '<a id=e2e href=/" +
    FX.mcp_click_targets[0] +
    " style=display:block;font-size:40px;padding:60px>" +
    FX.mcp_link_labels[0] +
    "</a>'; return 'INJECTED'; }";
  await tool(
    'mcp.' + tag + '.inject',
    'mcp_client_call',
    { tool: 'browser_evaluate', arguments: { function: inject } },
    { bestEffort: true },
  );
  await sleep(B.mcp_inject_settle_ms);

  // Snapshot + click + poll, up to `mcp_click_attempts` with a FRESH snapshot each time: under contention a
  // click can land while the view is mid-navigation and silently do nothing.
  let emb2 = null;
  const drove = () => !!(emb2 && FX.mcp_click_targets.some((t) => emb2.url.includes(t)));
  for (let attempt = 0; attempt < B.mcp_click_attempts && !drove(); attempt++) {
    if (attempt > 0) await sleep(B.mcp_click_retry_ms);
    const snap = await tool('mcp.' + tag + '.snapshot.' + attempt, 'mcp_client_call', {
      tool: 'browser_snapshot',
      arguments: {},
    });
    // The ref is read out of the snapshot because the click NEEDS it — a handle, not a verdict. The snapshot is
    // JSON.stringify'd, so inner quotes are escaped and the ref is matched after the label without depending on
    // the escaping.
    const snapTxt = jstr(snap);
    let ref = null;
    let label = FX.mcp_link_labels[0];
    for (const candidate of FX.mcp_link_labels) {
      const at = snapTxt.indexOf(candidate);
      const m = at >= 0 ? /\[ref=(\w+)\]/.exec(snapTxt.slice(at, at + 200)) : null;
      if (m) {
        ref = m[1];
        label = candidate;
        break;
      }
    }
    if (ref) {
      await tool(
        'mcp.' + tag + '.click.' + attempt,
        'mcp_client_call',
        { tool: 'browser_click', arguments: { target: ref } },
        { recordArgs: { target: ref, label } },
      );
    }
    for (let i = 0; i < B.mcp_view_polls; i++) {
      await sleep(B.mcp_view_interval_ms);
      let list = null;
      try {
        list = await (await fetch('http://127.0.0.1:9333/json/list')).json();
      } catch (e) {
        record('mcp.' + tag + '.view2.' + attempt + '.' + i, {
          args: { url: 'http://127.0.0.1:9333/json/list' },
          error: String((e && e.message) || e),
        });
        continue;
      }
      record('mcp.' + tag + '.view2.' + attempt + '.' + i, {
        args: { url: 'http://127.0.0.1:9333/json/list' },
        result: list,
      });
      emb2 = (list || []).find((t) => !t.url.includes('/desktop/'));
      if (drove()) break;
    }
  }

  // Geometry triage (DIAGNOSTIC ONLY, never a check): a physical click that misses for viewport reasons looks
  // identical to a broken click path, so the viewport and the link's rect are recorded beside the failure. The
  // judge prints them only when the click did not drive.
  if (!drove()) record('mcp.' + tag + '.geometry', { result: await clickGeom() });
  await tool('mcp.' + tag + '.disconnect.2', 'mcp_client_disconnect', {}, { bestEffort: true });
}

/** Best-effort CDP geometry snapshot for click-miss triage. Never throws, and it says WHICH of the two states
 *  that used to read identically as `geometry: null` it saw: the link not being in the view, or the probe
 *  answering nothing. Those need opposite responses from the next reader. */
async function clickGeom() {
  if (typeof WebSocket === 'undefined') return 'no-websocket (node < 22)';
  let ws = null;
  try {
    const l = await (await fetch('http://127.0.0.1:9333/json/list')).json();
    const t = (l || []).find((x) => !x.url.includes('/desktop/'));
    if (!t) return 'no-view';
    ws = new WebSocket(t.webSocketDebuggerUrl);
    await Promise.race([
      new Promise((res, rej) => {
        ws.addEventListener('open', res, { once: true });
        ws.addEventListener('error', rej, { once: true });
      }),
      sleep(B.ws_eval_timeout_ms).then(() => {
        throw new Error('ws-timeout');
      }),
    ]);
    let seq = 0;
    const pend = new Map();
    ws.addEventListener('message', (ev) => {
      const m = JSON.parse(String(ev.data));
      if (m.id && pend.has(m.id)) {
        pend.get(m.id)(m);
        pend.delete(m.id);
      }
    });
    const send = (method, params) =>
      new Promise((res, rej) => {
        seq += 1;
        pend.set(seq, res);
        ws.send(JSON.stringify({ id: seq, method, params }));
        sleep(B.ws_eval_timeout_ms).then(() => {
          if (pend.has(seq)) {
            pend.delete(seq);
            rej(new Error('cdp-timeout'));
          }
        });
      });
    const r = await send('Runtime.evaluate', {
      expression:
        "JSON.stringify((() => { const a = [...document.querySelectorAll('a')].find((x) => x.textContent.trim() === 'Learn more'); if (!a) return null; const b = a.getBoundingClientRect(); return { x: Math.round(b.x), y: Math.round(b.y), w: Math.round(b.width), h: Math.round(b.height), vw: window.innerWidth, vh: window.innerHeight }; })())",
      returnByValue: true,
    });
    ws.close();
    ws = null;
    const value = r.result && r.result.result && r.result.result.value;
    if (value === undefined || value === null)
      return 'no-value-from-cdp (the probe answered, the expression produced nothing)';
    if (value === 'null') return 'no-link-in-view (the Learn more link is not in the embedded view right now)';
    return value;
  } catch (e) {
    try {
      ws && ws.close();
    } catch (e2) {}
    return 'geom-error:' + String((e && e.message) || e).slice(0, 80);
  }
}

async function sectionMcp() {
  for (const t of FX.mcp_transports) await mcpProbe(t.tag, t.args);
}

// ── §4c evidence: an AI screenshot lands in pwout and shows in pwshots ──────────────────────────────────────

/** Switch the desktop SPA to the named rail page, so the embedded view has non-zero bounds: the
 *  WebContentsView is placed only while the Browser page is active (device-caught round-291 — a screenshot of
 *  a hidden view fails with "Cannot take screenshot with 0 width"). */
async function spaRailClick(label) {
  const ws = await spaSocket();
  if (!ws) return false;
  try {
    const raw = await evalInSpa(
      ws,
      1,
      "(function(){ var bs = document.querySelectorAll('button, [role=button]'); for (var i=0;i<bs.length;i++){ var el = bs[i]; var t = (el.getAttribute('aria-label')||el.title||el.textContent||'').trim(); if (t === '" +
        label +
        "' || (t.indexOf('" +
        label +
        "') === 0 && t.length < 15)) { el.click(); return 'clicked'; } } return 'no-rail'; })()",
      B.ws_eval_timeout_ms,
    );
    await sleep(B.spa_rail_settle_ms);
    return raw === 'clicked';
  } finally {
    await closeSocket(ws);
  }
}

async function sectionEvidence() {
  mint('evidence.rail.click', await spaRailClick('Browser'));
  await sleep(B.evidence_rail_settle_ms);

  // The AI takes a screenshot of the embedded view through `browser_run_script`, into the pwout directory —
  // the round-252 evidence path.
  const name = mint('mint.evidence_shot', FX.evidence_shot_prefix + Date.now() + FX.evidence_shot_suffix);
  const script = [
    "const { chromium } = require('" + PW_DIR.replace(/\\/g, '/') + "/node_modules/playwright');",
    '(async () => {',
    "  const browser = await chromium.connectOverCDP('http://127.0.0.1:9333', { timeout: 10000 });",
    '  const pages = browser.contexts().flatMap(c => c.pages());',
    "  const view = pages.find(p => !p.url().includes('/desktop/'));",
    "  if (!view) { console.log('NO_VIEW'); await browser.close(); return; }",
    "  await view.screenshot({ path: '" + EVIDENCE_DIR.replace(/\\/g, '\\\\') + '\\\\' + name + "' });",
    "  console.log('SHOT_SAVED');",
    '  await browser.close();',
    "})().catch(e => { console.log('FAIL:' + String(e).slice(0, 200)); process.exit(1); });",
  ].join('\n');
  await tool('evidence.screenshot', 'browser_run_script', { script });

  // The pwshots API — the Evidence drawer's data source — must list it. No Content-Type here: this is a plain
  // GET, the way the JS suite made it.
  await sleep(B.evidence_shots_settle_ms);
  const shotsResp = await fetch(BASE + '/api/browser/pwshots', { headers: { Authorization: 'Bearer ' + TOKEN } });
  record('evidence.pwshots', {
    args: { method: 'GET', path: '/api/browser/pwshots' },
    result: shotsResp.ok ? await jsonOrNull(shotsResp) : null,
  });

  // Self-clean (same policy as the file and terminal sections — the suite must not litter pwout).
  try {
    fs.unlinkSync(EVIDENCE_DIR + '\\' + name);
  } catch (e) {}
}

// ── §4 browser: browser_run_script drives the view; the SPA bar follows ─────────────────────────────────────

async function sectionBrowser() {
  await tool('browser.pw_info', 'browser_pw_info', {});

  // The FAIL path: a throwing script must report exit_code != 0, so the bounded runner surfaces the error
  // instead of hanging until its timeout.
  await tool('browser.script.fail', 'browser_run_script', {
    script:
      "(async () => { console.log('E2E-FAIL-PATH'); throw new Error('e2e-intentional'); })().catch(e => { console.log('FAIL:' + e.message); process.exit(1); });",
    timeout_secs: 60,
  });

  const drive = [
    "const { chromium } = require('" + PW_DIR.replace(/\\/g, '/') + "/node_modules/playwright');",
    '(async () => {',
    "  const browser = await chromium.connectOverCDP('http://127.0.0.1:9333', { timeout: 10000 });",
    '  const pages = browser.contexts().flatMap(c => c.pages());',
    "  const view = pages.find(p => !p.url().includes('/desktop/'));",
    "  if (!view) { console.log('NO_VIEW'); await browser.close(); return; }",
    "  await view.goto('" + FX.browser_home_url + "');",
    "  console.log('TITLE=' + await view.title());",
    '  await browser.close();',
    "})().catch(e => { console.log('FAIL:' + String(e).slice(0, 200)); process.exit(1); });",
  ].join('\n');
  await tool('browser.script.drive', 'browser_run_script', { script: drive });

  // Right after a Playwright-driven navigation the SPA can answer `Runtime.evaluate` without a value for
  // several seconds (Electron CDP quirk, observed ~3 s and fine at ~8 s), so poll generously rather than
  // sleeping once.
  await sleep(B.browser_bar_settle_ms);
  const targets = await cdpTargets('browser.cdp.targets');
  const spa = (targets || []).find((t) => t.url.includes('/desktop/'));
  if (!spa) return;

  let ws = null;
  try {
    ws = new WebSocket(spa.webSocketDebuggerUrl);
    await new Promise((res, rej) => {
      ws.onopen = res;
      ws.onerror = rej;
    });
    const bar =
      "JSON.stringify({ onBrowser: !!document.querySelector('.browser-url'), url: (document.querySelector('.browser-url')||{}).value || '' })";
    let onBrowser = false;
    for (let attempt = 0; attempt < B.browser_bar_polls; attempt++) {
      const raw = await evalInSpa(ws, 100, bar, B.ws_eval_timeout_ms);
      record('browser.bar.poll.' + attempt, { result: raw });
      const parsed = raw ? safeParse(raw) : null;
      if (parsed) onBrowser = !!parsed.onBrowser;
      if (parsed && parsed.url && parsed.url.includes(FX.browser_bar_fragment)) break;
      if (parsed && !parsed.onBrowser) break;
      await sleep(B.browser_bar_interval_ms);
    }
    if (!onBrowser) {
      // The SPA is not on the Browser page, so click the rail — the driver may do that; what the bar says
      // afterwards is the judge's to read.
      record(
        'browser.rail.click',
        {
          result: await evalInSpa(
            ws,
            101,
            "var bs = document.querySelectorAll('button'); for (var i=0;i<bs.length;i++){ if (bs[i].getAttribute('aria-label')==='Browser' || bs[i].textContent.trim()==='Browser') { bs[i].click(); break; } } 'ok'",
            B.ws_eval_timeout_ms,
          ),
        },
      );
      await sleep(B.browser_rail_settle_ms);
      record('browser.bar.after_rail', { result: await evalInSpa(ws, 100, bar, B.ws_eval_timeout_ms) });
    }

    // Focus-trap regression: FOCUS the address bar without typing (the stuck state is focus-without-blur — the
    // operator clicked the bar and then the native view, whose clicks never fire SPA blur), drive a navigation,
    // and require the bar to follow WITHOUT re-entering the panel.
    const marker = mint('mint.focus_marker', FX.focus_marker_prefix + Date.now());
    record(
      'browser.trap.focus',
      {
        result: await evalInSpa(
          ws,
          300,
          "(function(){ var el = document.querySelector('.browser-url'); if (!el) return 'no-bar'; el.focus(); return 'focused'; })()",
          B.ws_eval_timeout_ms,
        ),
      },
    );
    const focusScript = [
      "const { chromium } = require('" + PW_DIR.replace(/\\/g, '/') + "/node_modules/playwright');",
      '(async () => {',
      "  const browser = await chromium.connectOverCDP('http://127.0.0.1:9333', { timeout: 10000 });",
      '  const pages = browser.contexts().flatMap(c => c.pages());',
      "  const view = pages.find(p => !p.url().includes('/desktop/'));",
      "  if (!view) { console.log('NO_VIEW'); await browser.close(); return; }",
      "  await view.goto('" + fill(FX.marker_url_template, { marker }) + "');",
      "  console.log('NAV-OK');",
      '  await browser.close();',
      "})().catch(e => { console.log('FAIL:' + String(e).slice(0, 200)); process.exit(1); });",
    ].join('\n');
    await tool('browser.trap.nav', 'browser_run_script', { script: focusScript });
    for (let attempt = 0; attempt < B.browser_trap_polls; attempt++) {
      const raw = await evalInSpa(
        ws,
        310 + attempt,
        "JSON.stringify({ url: (document.querySelector('.browser-url')||{}).value || '' })",
        B.ws_eval_timeout_ms,
      );
      record('browser.trap.bar.' + attempt, { result: raw });
      const parsed = raw ? safeParse(raw) : null;
      if (parsed && String(parsed.url || '').includes(marker)) break;
      await sleep(B.browser_trap_interval_ms);
    }
    // Restore: blur the bar so later runs and sections start from a clean state.
    record(
      'browser.trap.blur',
      {
        result: await evalInSpa(
          ws,
          399,
          "(function(){ var el = document.querySelector('.browser-url'); if (el) el.blur(); return 'ok'; })()",
          B.ws_eval_timeout_ms,
        ),
      },
    );
  } finally {
    if (ws) await closeSocket(ws);
  }
}

function safeParse(text) {
  try {
    return JSON.parse(text);
  } catch (e) {
    return null;
  }
}

// ── §8 governance: goal / approval gate / grants / intent ───────────────────────────────────────────────────

async function sectionGovernance() {
  const sid = await tool('gov.open', 'terminal_open', { kind: 'pty', rows: 24, cols: 80 });
  const sessionId = typeof sid === 'string' ? sid : sid && sid.sid;
  if (!sessionId) throw new Stop('terminal_open answered no session id');
  const apath = (suffix) => '/api/sessions/' + encodeURIComponent(sessionId) + suffix;

  // The operator states intent; the goal is a partial patch and must not clear what it did not mention.
  await ctl('gov.control.goal', sessionId, { goal: FX.gov_goal });
  await tool('gov.list.after_goal', 'terminal_list', {});

  // Arm the gate TWICE: the repeat must not double-record.
  await ctl('gov.control.arm.0', sessionId, { approval_required: true });
  await ctl('gov.control.arm.1', sessionId, { approval_required: true });

  // An execute BLOCKS at the gate carrying its intent. It is ISSUED here and collected after the approval,
  // which is what makes it a park rather than a failure — and its rejection is caught so that the RECORD, not
  // an exception, is what the judge reads.
  const pending = tool('gov.exec.result', 'terminal_execute', {
    session_id: sessionId,
    command: FX.gov_command,
    intent: FX.gov_intent,
    considered: FX.gov_considered,
  }).then(
    (r) => ({ landed: true, r }),
    (e) => ({ landed: false, e: String((e && e.message) || e) }),
  );

  for (let i = 0; i < B.gov_pending_polls; i++) {
    await sleep(B.gov_pending_interval_ms);
    const parked = await tool('gov.pending.' + i, 'terminal_list', {});
    const r = (Array.isArray(parked) ? parked : []).find((x) => x.id === sessionId);
    if (r && r.pending_approval && r.pending_approval.id) break;
  }

  // The approval carries the id off the row the poll just read — a handle, read because the call needs it.
  const last = observations[observations.length - 1];
  const parkedList = (last && last.result) || [];
  const parkedRow = (Array.isArray(parkedList) ? parkedList : []).find((x) => x.id === sessionId);
  const apid = (parkedRow && parkedRow.pending_approval && parkedRow.pending_approval.id) || '';
  await jsonCall('gov.approval', 'POST', apath('/approval'), { id: apid, approve: true, grant: true });

  await pending; // the parked execute settles once the gate decides
  await tool('gov.list.after_exec', 'terminal_list', {});

  // The granted family does NOT ask again — and HOW LONG it took is a fact the judge compares to the plan.
  const t0 = Date.now();
  await tool('gov.exec.sibling', 'terminal_execute', { session_id: sessionId, command: FX.gov_sibling_command });
  mint('gov.sibling_ms', Date.now() - t0);

  await jsonCall('gov.grants.revoke', 'POST', apath('/grants'), { all: true });
  await ctl('gov.control.disarm', sessionId, { approval_required: false });

  // THE TRAIL: does it explain why a command ran unasked?
  await jsonCall('gov.session', 'GET', apath(''));
  await tool('gov.close', 'terminal_close', { session_id: sessionId }, { bestEffort: true });
}

// ── §8 runs: one AI execution gets a name, and the work wears it ────────────────────────────────────────────

async function sectionRuns() {
  const sid = await tool('runs.open', 'terminal_open', { kind: 'pty', rows: 24, cols: 80 });
  const sessionId = typeof sid === 'string' ? sid : sid && sid.sid;
  if (!sessionId) throw new Stop('terminal_open answered no session id');

  // A UNIQUE label, so the assertions cannot match an earlier run still inside the retention window on a
  // device that has been used before.
  const marker = mint('mint.runs_label', FX.runs_label_prefix + Date.now());
  const begun = await tool('runs.begin', 'run_begin', { label: marker, goal: FX.runs_goal });
  const runId = begun && begun.run_id;

  // Work INSIDE the run, carrying the id... and work OUTSIDE it, which must stay unattributed rather than
  // being absorbed into whatever run happens to precede it.
  await tool('runs.exec.in', 'terminal_execute', {
    session_id: sessionId,
    command: FX.runs_inside_command,
    intent: 'the work the run names',
    run_id: runId,
  });
  await tool('runs.exec.loose', 'terminal_execute', { session_id: sessionId, command: FX.runs_loose_command });

  await jsonCall('runs.operation.before_end', 'GET', '/api/operation');
  await tool('runs.end', 'run_end', { run_id: runId, outcome: 'done' });
  await jsonCall('runs.operation.after_end', 'GET', '/api/operation');

  // A LABEL, NEVER A CREDENTIAL: an id that was never minted is ACCEPTED and reported as `known:false`.
  await tool('runs.end.unknown', 'run_end', { run_id: FX.runs_unknown_id, outcome: 'done' });
  await tool('runs.close', 'terminal_close', { session_id: sessionId }, { bestEffort: true });
}

// ── THE RUN ─────────────────────────────────────────────────────────────────────────────────────────────────

const SECTIONS = {
  terminal: sectionTerminal,
  file: sectionFile,
  workflow: sectionWorkflow,
  panel: sectionPanel,
  mcp: sectionMcp,
  evidence: sectionEvidence,
  browser: sectionBrowser,
  governance: sectionGovernance,
  runs: sectionRuns,
};

(async () => {
  // A plan naming a section this driver does not implement is a REFUSAL, not a silent omission: the judge would
  // otherwise report a verdict about a run that never happened.
  const unknown = PLAN.sections.filter((s) => !SECTIONS[s]);
  if (unknown.length) {
    console.error('the plan names section(s) this driver does not implement: ' + unknown.join(', '));
    console.error('EXIT 2: THE SUITE DID NOT RUN.');
    process.exit(2);
  }

  try {
    for (const name of PLAN.sections) {
      console.log('-- ' + name);
      await SECTIONS[name]();
    }
  } catch (e) {
    // ONE `try` AROUND EVERY SECTION, which is what the JS suite always did: a failed call ends the run, and
    // the checks that already ran still count. The judge reports this as `suite`.
    record('driver.abort', { error: String((e && e.message) || e) });
  }

  const doc = { schema: 1, base: BASE, plan: PLAN, observations };
  try {
    fs.writeFileSync(ROWS_PATH, JSON.stringify(doc));
  } catch (e) {
    console.error('could not write ' + ROWS_PATH + ': ' + e.message);
    process.exit(2);
  }
  console.log('wrote ' + observations.length + ' observation(s) to ' + ROWS_PATH);
  // THE EXIT CODE IS NOT THE VERDICT, and saying so is cheaper than a reader assuming otherwise.
  console.log("the verdict is the judge's: summrise-e2e-judge --rows " + ROWS_PATH);
  process.exit(0);
})();
