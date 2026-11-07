#!/usr/bin/env node
// The console's accessibility audit, measured in the browser the page actually draws in.
//
//   node scripts/a11y-audit.mjs
//
// `scripts/screenshot.mjs` already renders every destination against a stubbed console, but it is a
// top-level script: importing it would run the whole screenshot pass. This file therefore copies the
// minimal part of that harness — one loopback server for `dist/index.html`, the same Playwright
// resolution, and the small JSON surface the pages read — and adds the measurement.
//
// For each page state it collects, from the real DOM:
//
//   * `role="button"`, a click handler, or a non-negative `tabindex` on something that is not a
//     native control (a scrollable named region is the one allowed tab stop, because it is not a
//     control: the tab stop is how a keyboard user scrolls it);
//   * every `aria-expanded`, and whether the element is a `<button>`;
//   * every interactive element whose accessible name is empty;
//   * the tab order (positive tabindex first, then DOM order) against the visual reading order
//     (top-to-bottom, left-to-right) for `main` and any open dialog;
//   * every icon-only button whose `aria-label`/`title` is missing.
//
// One line per finding: page, kind, selector, detail. Exit 1 when there is one.

import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import http from 'node:http';
import { createRequire } from 'node:module';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const WEB = path.resolve(HERE, '..');
const DIST_INDEX = path.join(WEB, 'dist', 'index.html');

// ---------------------------------------------------------------------------------------------
// Playwright resolution, the same candidates `scripts/screenshot.mjs` uses. The browser is a tool
// on the machine, not an application dependency, so this script never installs one.
// ---------------------------------------------------------------------------------------------

const require = createRequire(import.meta.url);
const PLAYWRIGHT_CANDIDATES = [
  process.env.PLAYWRIGHT_CORE_DIR &&
    path.join(process.env.PLAYWRIGHT_CORE_DIR, 'node_modules', 'playwright-core'),
  process.env.PLAYWRIGHT_CORE_DIR,
  'playwright-core',
  'playwright',
  '/home/zss/summrise/agent/resources/panel-react/node_modules/playwright-core',
  path.join(WEB, 'node_modules', 'playwright-core')
].filter(Boolean);

let playwright = null;
for (const candidate of PLAYWRIGHT_CANDIDATES) {
  try {
    playwright = require(candidate);
    break;
  } catch {
    /* try the next candidate */
  }
}
if (!playwright) {
  console.error(
    'a11y-audit: no Playwright found. Set PLAYWRIGHT_CORE_DIR to the package directory, or NODE_PATH.'
  );
  console.error(`a11y-audit: tried:\n  ${PLAYWRIGHT_CANDIDATES.join('\n  ')}`);
  process.exit(2);
}

// ---------------------------------------------------------------------------------------------
// Fixtures. The audit does not photograph the pages, so this is the smallest console that renders
// them: two machines (one offline), a mixed audit with an openable row, the tool catalogue, one
// plugin, two tokens, and empty device answers. The ids are stable so a selector in the report can
// be reproduced.
// ---------------------------------------------------------------------------------------------

const NOW = Date.now();
const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const GIB = 1024 ** 3;

function hexId(label) {
  return createHash('sha256').update(`summrise-a11y-audit:${label}`).digest('hex');
}

const DEVICE = {
  workshop: hexId('device/workshop'),
  attic: hexId('device/attic'),
  studio: hexId('device/studio')
};
const TOKEN = {
  workshop: hexId('token/workshop'),
  fleet: hexId('token/fleet')
};

const TOOL_NAMES = [
  'files.read',
  'files.write',
  'files.mkdir',
  'files.list',
  'memory.append',
  'memory.search',
  'terminal.create',
  'terminal.write',
  'terminal.read',
  'terminal.close',
  'terminal.list',
  'terminal.screen',
  'browser.list',
  'browser.open',
  'browser.navigate',
  'browser.eval',
  'browser.screenshot',
  'browser.close',
  'browser.shutdown',
  'monitor.add',
  'monitor.remove',
  'monitor.list',
  'monitor.probe',
  'monitor.status',
  'runs.exec',
  'runs.list',
  'runs.get',
  'system.process_list',
  'system.process_kill',
  'system.net_test',
  'host.facts',
  'screen.capture',
  'state.get',
  'plugins.list',
  'plugins.inspect',
  'plugins.add',
  'plugins.remove',
  'plugins.restart',
  'plugins.logs'
];

const DESTRUCTIVE = new Set([
  'files.write',
  'terminal.create',
  'terminal.write',
  'browser.open',
  'browser.navigate',
  'browser.eval',
  'runs.exec',
  'system.process_kill',
  'plugins.add',
  'plugins.remove',
  'plugins.restart'
]);

const DEVICES = [
  {
    id: DEVICE.workshop,
    name: 'workshop',
    online: true,
    version: '0.3.42',
    last_seen_ms: NOW - 12_000,
    last_frame_ms: NOW - 12_000,
    registered_at_ms: NOW - 41 * 24 * HOUR,
    // The day `/fleet` counted in the audit's newest window: the row states it on its muted line.
    today: { client_calls: 283, failed: 38 },
    last_attempt: null,
    // The device's own account of its last start, as the host frame carries it. The line it draws
    // is text, not a control: the audit still finds no unnamed button and no extra tab stop.
    update: {
      current: '0.3.42',
      verdict: { outcome: 'ok', detail: 'started 0.3.42' },
      last_update: { to: '0.3.42', outcome: 'applied', at_ms: NOW - 2 * 86_400 }
    },
    host: {
      os: { name: 'Windows 11 Pro', version: '10.0.26100', arch: 'x86_64' },
      machine: 'DESKTOP-WORKSHOP',
      uptime_s: 3 * 86_400 + 4 * 3_600,
      cpu: { cores: 16 },
      memory: { total_bytes: 64 * GIB, available_bytes: 27 * GIB },
      disks: [{ name: 'C:', total_bytes: 1024 * GIB, free_bytes: 312 * GIB }],
      sessions: [{ id: 2, user: 'operator', state: 'active' }],
      helper: { registered: true, pid: 4812, session: 2 },
      // This app's own boot journal: its newest entry is when the running process started. The
      // applied answer two days ago (above) belongs to this start, so the row draws the start with
      // its age and the answer's age beside it; the studio machine has no journal and draws no
      // start rather than borrow a time.
      restarts: [
        { at_ms: NOW - 3 * 86_400_000, version: '0.3.42', channel: 'stable', pid: 4210 }
      ],
      update: null
    }
  },
  {
    id: DEVICE.attic,
    name: 'attic',
    online: true,
    version: '0.3.42',
    last_seen_ms: NOW - 38_000,
    last_frame_ms: NOW - 38_000,
    registered_at_ms: NOW - 12 * 24 * HOUR,
    today: { client_calls: 11, failed: 0 },
    last_attempt: null,
    host: {
      os: { name: 'Ubuntu', version: '24.04.1 LTS', arch: 'x86_64' },
      machine: 'attic',
      uptime_s: 11 * 86_400,
      cpu: { cores: 8 },
      memory: { total_bytes: 16 * GIB, available_bytes: 9 * GIB },
      disks: [{ name: '/', total_bytes: 512 * GIB, free_bytes: 241 * GIB }],
      sessions: [],
      helper: { registered: false, pid: null, session: null },
      update: null
    }
  },
  {
    id: DEVICE.studio,
    name: 'studio',
    online: false,
    version: '0.3.30',
    last_seen_ms: NOW - 3 * HOUR,
    last_frame_ms: NOW - 3 * HOUR,
    registered_at_ms: NOW - 80 * 24 * HOUR,
    today: { client_calls: 84, failed: 2 },
    last_attempt: null,
    // The incident's shape: a machine went silent, and its own last story says what happened to the
    // update. It sent no host facts, so there is no boot journal to prove the refusal belongs to
    // this start; the page shows the silence and draws no undated update sentence beside it.
    update: {
      current: '0.3.30',
      verdict: { outcome: 'ok', detail: 'started 0.3.30' },
      last_update: { to: '0.3.31', outcome: 'refused', at_ms: NOW - 3 * HOUR - 60_000 }
    },
    host: null
  }
];

function liveToolsFor(deviceId) {
  const names = deviceId === DEVICE.studio ? [] : TOOL_NAMES;
  return names.map((name) => ({
    name,
    description: `${name} on this machine`,
    inputSchema: { type: 'object', properties: {}, additionalProperties: false },
    source: 'builtin',
    capability: name.split('.')[0],
    destructive: DESTRUCTIVE.has(name)
  }));
}

const PLUGINS = [
  {
    id: 'com.example.hello',
    namespace: 'hello',
    name: 'Hello',
    version: '0.1.0',
    description: 'Two friendly tools, and the reference for writing a plugin.',
    state: 'running',
    tools: 2,
    path: 'C:\\dev\\plugins\\hello',
    last_error: null
  },
  {
    id: 'com.example.image',
    namespace: 'image',
    name: 'Image',
    version: '0.2.1',
    description: 'Resize an image on the machine.',
    state: 'failed',
    tools: 0,
    path: 'C:\\work\\image-tools\\plugin',
    last_error: 'initialize timed out after 10s'
  }
];

const TOKENS = [
  {
    id: TOKEN.workshop,
    name: 'workshop-dsh',
    device_id: DEVICE.workshop,
    tools: ['files.list', 'files.read', 'terminal.list', 'runs.list'],
    allow_destructive: false,
    created_at_ms: NOW - 12 * 24 * HOUR,
    last_used_ms: NOW - 4 * MINUTE,
    expires_at_ms: null
  },
  {
    id: TOKEN.fleet,
    name: 'dsh-fleet',
    device_id: '',
    device_ids: [DEVICE.workshop, DEVICE.attic],
    tools: ['files.list', 'runs.list'],
    allow_destructive: false,
    created_at_ms: NOW - 2 * 24 * HOUR,
    last_used_ms: null,
    expires_at_ms: null
  }
];

/** One audit line. The evidence on two of them is what makes the row a disclosure. */
function auditLine(agoMs, overrides) {
  return {
    ts_ms: NOW - agoMs,
    kind: 'mcp',
    status: 200,
    device_id: DEVICE.workshop,
    sentence: 'Used a tool',
    // A proxied row is a client's call unless it says otherwise; the operator actions below name
    // no caller at all.
    caller: 'client',
    ...overrides
  };
}

const AUDIT = [
  // The console's own read: in the record and in the timeline, marked with its quiet word.
  auditLine(30_000, {
    tool: 'terminal.list',
    sentence: 'Listed the terminal sessions',
    caller: 'console'
  }),
  auditLine(40_000, {
    tool: 'runs.exec',
    detail: 'run r-1 exited 0 in 42 ms',
    sentence: 'Ran `echo hello`',
    live: true,
    evidence: { command: 'echo', args: ['hello'], cwd: '.', output: 'hello\n' }
  }),
  auditLine(2 * MINUTE, {
    tool: 'terminal.write',
    sentence: 'Typed into a terminal',
    live: true,
    evidence: { session: 'shot-pty-1', input: 'dir\n' }
  }),
  auditLine(5 * MINUTE, {
    tool: 'files.list',
    detail: 'error',
    sentence: 'Listed a folder',
    evidence: { path: '' }
  }),
  auditLine(8 * MINUTE, {
    tool: 'memory.search',
    detail: 'jsonrpc error -32002',
    status: 200,
    sentence: 'Searched its memory',
    evidence: { query: 'deploy', output: 'no memory matches' }
  }),
  auditLine(13 * MINUTE, {
    tool: 'state.get',
    sentence: 'Read the machine\u2019s state',
    evidence: { output: '{"state":"ok"}' }
  }),
  auditLine(21 * MINUTE, {
    kind: 'grant',
    tool: null,
    detail: 'allow_destructive=false',
    sentence: 'Issued a grant',
    caller: null
  })
];

const SESSION = { authenticated: true, email: 'operator@summrise.test', via: 'token' };
const HEALTH = { version: '0.3.218', devices: DEVICES.length };
const MESH = { configured: true, broker: 'https://mesh.summrise.test' };
const CONNECTIONS = { [DEVICE.workshop]: [], [DEVICE.attic]: [], [DEVICE.studio]: [] };

const ICON_B64 =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=';
const iconPng = Buffer.from(ICON_B64, 'base64');

const indexHtml = await readFile(DIST_INDEX).catch((error) => {
  console.error(`a11y-audit: cannot read ${DIST_INDEX}: ${error.message}`);
  console.error('a11y-audit: build the page first: cd device/console/web && npm run build');
  process.exit(2);
});

async function startServer() {
  const server = http.createServer((request, response) => {
    const url = new URL(request.url ?? '/', 'http://127.0.0.1');
    if (request.method === 'GET' && (url.pathname === '/' || url.pathname === '/index.html')) {
      response.writeHead(200, {
        'content-type': 'text/html; charset=utf-8',
        'cache-control': 'no-store'
      });
      response.end(indexHtml);
      return;
    }
    if (
      request.method === 'GET' &&
      (url.pathname === '/assets/icon.png' || url.pathname === '/favicon.ico')
    ) {
      response.writeHead(200, { 'content-type': 'image/png', 'cache-control': 'no-store' });
      response.end(iconPng);
      return;
    }
    response.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' });
    response.end('not found');
  });

  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      server.off('error', reject);
      resolve();
    });
  });
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('the loopback server has no port');
  return { server, origin: `http://127.0.0.1:${address.port}` };
}

async function stopServer(server) {
  if (!server) return;
  await new Promise((resolve) => {
    server.close(() => resolve());
    if (typeof server.closeAllConnections === 'function') server.closeAllConnections();
  });
}

function json(body, status = 200) {
  return { status, body };
}

function bodyOf(request) {
  try {
    return request.postDataJSON() ?? {};
  } catch {
    return {};
  }
}

function replyFor(request, url) {
  const method = request.method();
  const pathname = url.pathname;

  if (method === 'GET' && pathname === '/session') return json(SESSION);
  if (method === 'GET' && pathname === '/health') return json(HEALTH);
  if (method === 'GET' && pathname === '/fleet') return json(DEVICES);
  // The fleet's installer door, so the audit measures the link and the muted facts line the fleet
  // actually draws (a fixture hole would hide the block and audit a state the page never shows).
  if (method === 'GET' && pathname === '/installer') {
    return json({
      available: true,
      filename: 'SummriseSetup-0.3.280.exe',
      version: '0.3.280',
      size_bytes: 6_638_339,
      sha256: '26d39c12' + 'ab'.repeat(28)
    });
  }
  if (method === 'GET' && pathname === '/audit/day') {
    // The audit's day, by the page's four buckets: boundaries are not calls, a poll is its own
    // bucket, and a failed row is status 400+ or a JSON-RPC error inside a 200.
    const start = new Date(NOW);
    start.setHours(0, 0, 0, 0);
    const day = {
      client_calls: 0,
      failed: 0,
      console: 0,
      console_failed: 0,
      poll: 0,
      poll_failed: 0,
      unattributed: 0,
      unattributed_failed: 0
    };
    for (const line of AUDIT) {
      if (line.ts_ms < start.getTime()) continue;
      const failed =
        line.status >= 400 || String(line.detail ?? '').startsWith('jsonrpc error');
      if (line.kind === 'poll') {
        day.poll += 1;
        if (failed) day.poll_failed += 1;
      } else if (line.caller === 'client' || (line.caller == null && line.token_id != null)) {
        day.client_calls += 1;
        if (failed) day.failed += 1;
      } else if (line.caller === 'console') {
        day.console += 1;
        if (failed) day.console_failed += 1;
      } else {
        day.unattributed += 1;
        if (failed) day.unattributed_failed += 1;
      }
    }
    return json(day);
  }
  // The file the audit window reads. The page draws one muted line from it beside the cap
  // sentence; the audit measures the same line a person reads.
  if (method === 'GET' && pathname === '/audit/source') {
    return json({
      file: 'audit.jsonl',
      bytes: 8_808_038,
      oldest_ts_ms: NOW - 3 * 24 * HOUR,
      newest_ts_ms: NOW,
      max_bytes: 8 * 1024 * 1024,
      rotations: 1,
      rotated: { file: 'audit.1.jsonl', oldest_ts_ms: NOW - 5 * 24 * HOUR }
    });
  }
  if (method === 'GET' && pathname === '/audit') {
    const raw = Number(url.searchParams.get('limit') ?? 20);
    const limit = Number.isFinite(raw) ? Math.max(0, Math.min(Math.trunc(raw), 1000)) : 20;
    return json(AUDIT.slice(0, limit));
  }
  if (method === 'GET' && pathname === '/events') {
    const lines = AUDIT.slice(0, 6).reverse();
    return { status: 200, sse: `event: snapshot\ndata: ${JSON.stringify({ lines })}\n\n` };
  }
  if (method === 'GET' && pathname === '/denials') return json([]);
  // The static registry carries each tool's destructive flag. The fleet token form has no live
  // catalogue to read, so these are the flags it counts.
  if (method === 'GET' && pathname === '/tools')
    return json(TOOL_NAMES.map((name) => ({ name, destructive: DESTRUCTIVE.has(name) })));
  if (method === 'GET' && pathname === '/mesh') return json(MESH);
  if (pathname === '/tokens' && method === 'GET') return json(TOKENS);
  if (/^\/tokens\/[^/]+$/.test(pathname) && method === 'DELETE') return { status: 204 };

  const liveTools = /^\/fleet\/([^/]+)\/tools$/.exec(pathname);
  if (liveTools && method === 'GET') {
    const id = decodeURIComponent(liveTools[1]);
    if (id === DEVICE.studio) {
      return json(
        { error: 'device_offline', message: 'this machine is not connected' },
        503
      );
    }
    return json({ tools: liveToolsFor(id) });
  }

  const fleet = /^\/fleet\/([^/]+)(?:\/(name|connections|mesh))?$/.exec(pathname);
  if (fleet) {
    const deviceId = decodeURIComponent(fleet[1]);
    const leaf = fleet[2];
    if (method === 'DELETE' && !leaf) return { status: 204 };
    if (leaf === 'connections' && method === 'GET') return json(CONNECTIONS[deviceId] ?? []);
    if (leaf === 'name' && method === 'POST') {
      return json({ id: deviceId, name: bodyOf(request).name ?? null });
    }
    if (leaf === 'mesh' && method === 'GET') {
      return json({ configured: true, reachable: true, candidates: 2 });
    }
    return null;
  }

  if (method === 'POST' && pathname === '/mcp') {
    const body = bodyOf(request);
    const deviceId = body?.device_id;
    const name = body?.request?.params?.name;
    if (deviceId === DEVICE.studio) {
      return json({ error: 'device reconnecting', retry_after_ms: 1000 }, 503);
    }
    if (name === 'terminal.list') {
      return json({ result: { content: [{ type: 'text', text: '[]' }] } });
    }
    if (name === 'system.process_list') {
      return json({ result: { content: [{ type: 'text', text: '[]' }] } });
    }
    if (name === 'browser.list') {
      return json({
        result: {
          content: [
            {
              type: 'text',
              text: JSON.stringify([
                { id: 'tab-1', url: 'https://example.test/', title: 'Example', type: 'page' }
              ])
            }
          ]
        }
      });
    }
    if (name === 'browser.screenshot') {
      return json({
        result: {
          content: [
            {
              type: 'text',
              text: JSON.stringify({ data: ICON_B64, bytes: 68, width: 1, height: 1 })
            }
          ]
        }
      });
    }
    if (name === 'files.list') {
      return json({
        result: {
          content: [
            {
              type: 'text',
              text: JSON.stringify([
                { name: 'docs', kind: 'directory', len: 0 },
                { name: 'notes.txt', kind: 'file', len: 12 }
              ])
            }
          ]
        }
      });
    }
    if (name === 'runs.list') {
      return json({
        result: {
          content: [
            {
              type: 'text',
              text: JSON.stringify([
                {
                  id: 'r-1',
                  command: 'echo',
                  args: ['hello'],
                  cwd: '.',
                  exit_code: 0,
                  timed_out: false,
                  duration_ms: 42,
                  started_at_ms: NOW - 5 * MINUTE,
                  stdout_bytes: 6,
                  stderr_bytes: 0
                }
              ])
            }
          ]
        }
      });
    }
    if (name === 'plugins.list') {
      const plugins = deviceId === DEVICE.workshop ? PLUGINS : [];
      return json({ result: { content: [{ type: 'text', text: JSON.stringify(plugins) }] } });
    }
    return null;
  }

  return null;
}

async function installPage(page, origin) {
  await page.route('**/*', async (route) => {
    const request = route.request();
    let url;
    try {
      url = new URL(request.url());
    } catch {
      await route.abort('blockedbyclient').catch(() => {});
      return;
    }
    if (url.origin !== origin) {
      await route.abort('blockedbyclient').catch(() => {});
      return;
    }
    if (
      url.pathname === '/' ||
      url.pathname === '/index.html' ||
      url.pathname === '/assets/icon.png' ||
      url.pathname === '/favicon.ico'
    ) {
      await route.continue();
      return;
    }
    const reply = replyFor(request, url);
    if (reply) {
      const headers = { 'cache-control': 'no-store' };
      if (reply.status === 204) {
        await route.fulfill({ status: 204, headers });
      } else if (reply.sse !== undefined) {
        await route.fulfill({
          status: reply.status,
          headers: { ...headers, 'content-type': 'text/event-stream' },
          body: reply.sse
        });
      } else {
        await route.fulfill({
          status: reply.status,
          headers: { ...headers, 'content-type': 'application/json; charset=utf-8' },
          body: JSON.stringify(reply.body)
        });
      }
      return;
    }
    // An API route this audit did not stub is a hole in the fixture, not a page state: fail loudly
    // rather than audit a page that is stuck on "Reading…".
    const api =
      /^\/(session|auth|logout|fleet|audit|denials|health|tokens|tools|mesh|enroll|grants|mcp|mcp\/all|events)(\/|$)/.test(
        url.pathname
      );
    if (api) {
      console.error(`a11y-audit: no fixture for ${request.method()} ${url.pathname}`);
      await route.fulfill({
        status: 500,
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ error: 'a11y_audit_fixture_missing' })
      });
      return;
    }
    await route.continue();
  });
}

// ---------------------------------------------------------------------------------------------
// Navigation. The same rail titles the screenshot harness uses; a page is only measured once its
// own content is on screen, not while it still says "Reading…".
// ---------------------------------------------------------------------------------------------

const settle = (page, ms = 250) => page.waitForTimeout(ms);

async function rail(page, label) {
  await page.locator(`nav.rail button[title=${JSON.stringify(label)}]`).click();
  await page.waitForFunction(
    ({ label }) => {
      const active = document.querySelector('nav.rail .rail-btn[aria-current="page"]');
      return active !== null && active.getAttribute('title') === label;
    },
    { label },
    { timeout: 5_000 }
  );
  await settle(page);
}

async function deviceTab(page, label) {
  await page.locator('nav.device-tabs').getByRole('button', { name: label, exact: true }).click();
  await settle(page);
}

async function boot(page, origin) {
  const response = await page.goto(origin + '/', {
    waitUntil: 'domcontentloaded',
    timeout: 30_000
  });
  if (!response || !response.ok()) {
    throw new Error(`GET / answered ${response ? response.status() : 'nothing'}`);
  }
  await page.locator('nav.rail').waitFor({ state: 'visible', timeout: 10_000 });
  await page.getByRole('heading', { name: 'Machines', exact: true, level: 1 }).waitFor({
    state: 'visible',
    timeout: 10_000
  });
  await page.waitForFunction(
    () => document.querySelectorAll('.fleet-row').length > 0,
    null,
    { timeout: 10_000 }
  );
  await settle(page, 400);
}

// ---------------------------------------------------------------------------------------------
// The measurement. Everything here runs inside the page so a selector is built from the DOM the
// audit actually read, not from a second copy of the markup.
// ---------------------------------------------------------------------------------------------

function collectInPage(regionSelector) {
  const norm = (value) => String(value ?? '').replace(/\s+/g, ' ').trim();
  const findings = [];
  const allowed = [];

  const selectorFor = (element) => {
    const parts = [];
    let node = element;
    while (node && node.nodeType === 1) {
      if (node.id) {
        parts.unshift('#' + CSS.escape(node.id));
        break;
      }
      let part = node.tagName.toLowerCase();
      const classes = [...node.classList]
        .filter((name) => !name.startsWith('is-') && name !== '')
        .slice(0, 2);
      if (classes.length > 0) part += '.' + classes.map((name) => CSS.escape(name)).join('.');
      parts.unshift(part);
      if (node === document.body) break;
      node = node.parentElement;
    }
    return parts.join(' > ');
  };

  const add = (kind, element, detail = '') => {
    const selector = selectorFor(element);
    const key = `${kind}::${selector}`;
    if (findings.some((entry) => entry.key === key)) return;
    findings.push({ key, kind, selector, detail });
  };

  const reactProps = (element) => {
    for (const key of Object.keys(element)) {
      if (key.startsWith('__reactProps$')) return element[key];
    }
    return null;
  };

  const isVisible = (element) => {
    if (!(element instanceof HTMLElement) && element.tagName !== 'SVG') return false;
    if (element.closest('[hidden]') !== null) return false;
    if (element.getAttribute('aria-hidden') === 'true') return false;
    if (typeof element.checkVisibility === 'function') {
      try {
        if (
          !element.checkVisibility({
            checkOpacity: true,
            checkVisibilityCSS: true,
            contentVisibilityAuto: true
          })
        ) {
          return false;
        }
      } catch {
        /* an older engine: fall through to the rect */
      }
    }
    const rect = element.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  };

  const nativeControl = (element) =>
    ['BUTTON', 'A', 'INPUT', 'SELECT', 'TEXTAREA', 'SUMMARY'].includes(element.tagName);

  const interactive = (element) =>
    ['BUTTON', 'A', 'INPUT', 'SELECT', 'TEXTAREA', 'SUMMARY'].includes(element.tagName) ||
    element.getAttribute('role') === 'button' ||
    element.getAttribute('tabindex') === '0';

  const inNativeControl = (element) =>
    element.closest('button, a[href], input, select, textarea, summary') !== null;

  const nameOf = (element) => {
    const labelledby = element.getAttribute('aria-labelledby');
    if (labelledby) {
      const text = labelledby
        .split(/\s+/)
        .map((id) => document.getElementById(id)?.textContent ?? '')
        .join(' ');
      if (norm(text) !== '') return norm(text);
    }
    const label = norm(element.getAttribute('aria-label'));
    if (label !== '') return label;
    if (element.tagName === 'INPUT' || element.tagName === 'SELECT' || element.tagName === 'TEXTAREA') {
      const labels = element.labels ? [...element.labels] : [];
      const text = labels.map((node) => node.textContent).join(' ');
      if (norm(text) !== '') return norm(text);
      if (element.tagName === 'INPUT' && (element.type === 'submit' || element.type === 'button')) {
        if (norm(element.value) !== '') return norm(element.value);
      }
    }
    if (element.tagName === 'IMG') {
      const alt = norm(element.getAttribute('alt'));
      if (alt !== '') return alt;
    }
    const text = norm(element.textContent);
    if (text !== '') return text;
    return norm(element.getAttribute('title'));
  };

  // Visible text, ignoring the parts the author hid from the accessibility tree (an icon's `<svg>`
  // is usually aria-hidden; a glyph is visible text and is treated as an icon below).
  const visibleText = (element) => {
    let text = '';
    const walk = (node) => {
      if (node.nodeType === 3) {
        text += node.nodeValue ?? '';
        return;
      }
      if (node.nodeType !== 1) return;
      if (node.getAttribute('aria-hidden') === 'true') return;
      if (node.tagName === 'SVG') return;
      for (const child of node.childNodes) walk(child);
    };
    walk(element);
    return norm(text);
  };

  const GLYPH_ONLY = /^[\s×✕✖✗⋯…›‹▾▸⌄⌃←→↑↓⏶⏷▴▾]+$/;

  // 1. A control that is not a control.
  for (const element of document.querySelectorAll('[role="button"]')) {
    if (element.tagName !== 'BUTTON') {
      add('role-button-on-non-button', element, `role="button" on <${element.tagName.toLowerCase()}>`);
    }
  }
  for (const element of document.querySelectorAll('*')) {
    if (!isVisible(element)) continue;
    if (element.tagName === 'BUTTON' || element.tagName === 'A') continue;
    if (inNativeControl(element)) continue;
    const handler = reactProps(element)?.onClick;
    if (typeof handler === 'function') {
      add(
        'click-on-non-control',
        element,
        `onClick on <${element.tagName.toLowerCase()}> with no button or link role`
      );
    }
  }
  for (const element of document.querySelectorAll('[tabindex]')) {
    const value = Number(element.getAttribute('tabindex'));
    if (!Number.isFinite(value) || value < 0) continue;
    if (nativeControl(element)) continue;
    // A named scrollable region is the one allowed non-control tab stop: its tab stop is how a
    // keyboard user scrolls it. It is not a button and must not be reported as one.
    if (element.getAttribute('role') === 'region' && norm(nameOf(element)) !== '') {
      allowed.push({ selector: selectorFor(element), detail: 'named scrollable region' });
      continue;
    }
    add(
      'tabindex-on-non-control',
      element,
      `tabindex="${value}" on <${element.tagName.toLowerCase()}>`
    );
  }

  // 2. A disclosure is a button.
  for (const element of document.querySelectorAll('[aria-expanded]')) {
    if (element.tagName !== 'BUTTON') {
      add(
        'aria-expanded-not-a-button',
        element,
        `aria-expanded on <${element.tagName.toLowerCase()}>`
      );
    }
  }

  // 3. Every interactive element has a name.
  for (const element of document.querySelectorAll(
    'button, a[href], input:not([type="hidden"]), select, textarea, summary, [role="button"], [role="tab"], [role="link"], [role="switch"], [role="checkbox"]'
  )) {
    if (!isVisible(element)) continue;
    if (nameOf(element) === '') {
      add(
        'unnamed-control',
        element,
        `<${element.tagName.toLowerCase()}${
          element.getAttribute('type') ? ` type="${element.getAttribute('type')}"` : ''
        }> has no accessible name`
      );
    }
  }

  // 4. Icon-only buttons are named by aria-label or title, not by a glyph.
  for (const element of document.querySelectorAll('button')) {
    if (!isVisible(element)) continue;
    const explicit = norm(element.getAttribute('aria-label')) !== '' ||
      norm(element.getAttribute('aria-labelledby')) !== '' ||
      norm(element.getAttribute('title')) !== '';
    if (explicit) continue;
    const text = visibleText(element);
    const icon = element.querySelector('svg, img') !== null || GLYPH_ONLY.test(text);
    if (icon && (text === '' || GLYPH_ONLY.test(text))) {
      add(
        'icon-only-button-unnamed',
        element,
        `icon-only <button> "${text}" has neither aria-label nor title`
      );
    }
  }

  // 5. The tab order follows the visual reading order inside the region the operator is using.
  const tabbable = (element) => {
    if (!isVisible(element)) return false;
    if (element.disabled === true) return false;
    if (element.closest('[inert]') !== null) return false;
    if (element.getAttribute('aria-hidden') === 'true') return false;
    const tag = element.tagName;
    const tabindex = element.getAttribute('tabindex');
    if (tabindex !== null) {
      const value = Number(tabindex);
      return Number.isFinite(value) && value >= 0;
    }
    if (tag === 'BUTTON' || tag === 'SELECT' || tag === 'TEXTAREA' || tag === 'SUMMARY') return true;
    if (tag === 'A') return element.hasAttribute('href');
    if (tag === 'INPUT') return element.type !== 'hidden';
    return false;
  };

  let debugOrder = [];
  const regions = [...document.querySelectorAll(regionSelector)].filter(isVisible);
  if (regions.length > 0) {
    const region = regions[regions.length - 1];
    const scope = region === document.body ? document.body : region;
    // A tabbable container (a named scroll region) encloses the controls it scrolls. Its own stop
    // is a scrolling affordance, not a sibling in reading order: comparing it with its contents
    // would put every control inside the container on one visual line. Read the leaves.
    const allTabbable = [...scope.querySelectorAll('*')].filter(tabbable);
    const items = allTabbable.filter(
      (element) => !allTabbable.some((other) => other !== element && element.contains(other))
    );
    const rect = (element) => {
      const box = element.getBoundingClientRect();
      return {
        top: box.top + window.scrollY,
        left: box.left + window.scrollX,
        height: box.height,
        element
      };
    };
    const positioned = items.map(rect);
    // Reading order is line by line. A line starts at the topmost unread control and takes every
    // control that shares its band: cells of one table row, controls of one single-line flex or
    // grid row, or boxes that overlap vertically. Within the line, reading is left-to-right; a
    // control aligned to the bottom of a form row still belongs beside the submit button.
    const tableRow = (element) => element.closest('tr');
    const tableCell = (element) => element.closest('td, th');
    const layoutParent = (element) => {
      let node = element.parentElement;
      while (node) {
        const display = getComputedStyle(node).display;
        // A one-child wrapper (a checkbox's inline-flex label) is not a row of its own: the control
        // belongs to the first real layout line above it.
        if (
          ['flex', 'inline-flex', 'grid', 'inline-grid'].includes(display) &&
          [...node.children].length > 1
        ) {
          return node;
        }
        node = node.parentElement;
      }
      return null;
    };
    const parentLayout = new Map();
    const singleLine = (container) => {
      if (container === null) return false;
      if (parentLayout.has(container)) return parentLayout.get(container);
      const children = [...container.children].filter(
        (child) => child.getBoundingClientRect().height > 0
      );
      let oneLine = false;
      if (children.length > 0) {
        const tallest = Math.max(...children.map((child) => child.getBoundingClientRect().height));
        const gap = Number.parseFloat(getComputedStyle(container).rowGap) || 0;
        oneLine = container.getBoundingClientRect().height <= tallest + gap + 12;
      }
      parentLayout.set(container, oneLine);
      return oneLine;
    };
    const sameBand = (existing, candidate) => {
      const existingRow = tableRow(existing.element);
      const candidateRow = tableRow(candidate.element);
      if (existingRow !== null && candidateRow !== null) {
        if (existingRow !== candidateRow) return false;
        if (tableCell(existing.element) !== tableCell(candidate.element)) {
          // A table row usually draws its cells on one line, but the token list reflows a row into
          // a grid card at a narrow sheet (name, scope/tools, actions, dates). Two cells of the
          // same row can therefore sit on different visual lines; measure before calling them one.
          return (
            Math.abs(
              candidate.top + candidate.height / 2 - (existing.top + existing.height / 2)
            ) <= 16 ||
            (candidate.top < existing.top + existing.height - 2 &&
              candidate.top + candidate.height > existing.top + 2)
          );
        }
      }
      const existingLayout = layoutParent(existing.element);
      const candidateLayout = layoutParent(candidate.element);
      if (
        existingLayout !== null &&
        existingLayout === candidateLayout &&
        singleLine(existingLayout)
      ) {
        return true;
      }
      return (
        Math.abs(
          candidate.top + candidate.height / 2 - (existing.top + existing.height / 2)
        ) <= 16 ||
        (candidate.top < existing.top + existing.height - 2 &&
          candidate.top + candidate.height > existing.top + 2)
      );
    };
    const visual = [];
    const remaining = [...positioned].sort(
      (a, b) => a.top - b.top || a.left - b.left || items.indexOf(a.element) - items.indexOf(b.element)
    );
    while (remaining.length > 0) {
      const line = [remaining.shift()];
      for (let index = 0; index < remaining.length; ) {
        const item = remaining[index];
        if (line.some((member) => sameBand(member, item))) {
          line.push(item);
          remaining.splice(index, 1);
        } else {
          index += 1;
        }
      }
      line.sort((a, b) => a.left - b.left || a.top - b.top);
      visual.push(...line);
    }
    const rank = new Map(visual.map((entry, index) => [entry.element, index]));
    const positive = items
      .filter((element) => Number(element.getAttribute('tabindex')) > 0)
      .sort(
        (a, b) =>
          Number(a.getAttribute('tabindex')) - Number(b.getAttribute('tabindex')) ||
          items.indexOf(a) - items.indexOf(b)
      );
    const effective = [...positive, ...items.filter((element) => !positive.includes(element))];
    let high = -1;
    let highElement = null;
    for (const element of effective) {
      const visualRank = rank.get(element);
      if (visualRank === undefined) continue;
      if (visualRank < high) {
        add(
          'focus-order',
          element,
          `tab order reaches "${nameOf(element) || element.tagName}" after "${
            nameOf(highElement) || highElement.tagName
          }" but it reads earlier on screen`
        );
      }
      if (visualRank > high) {
        high = visualRank;
        highElement = element;
      }
    }
    debugOrder = effective.map((element) => {
      const entry = positioned.find((candidate) => candidate.element === element);
      const box = element.getBoundingClientRect();
      return {
        name: nameOf(element).slice(0, 40) || element.tagName,
        top: Math.round(entry?.top ?? box.top + window.scrollY),
        left: Math.round(entry?.left ?? box.left + window.scrollX),
        height: Math.round(box.height),
        layout:
          layoutParent(element) === null
            ? ''
            : `${layoutParent(element).tagName.toLowerCase()}.${
                typeof layoutParent(element).className === 'string'
                  ? layoutParent(element).className
                  : ''
              }${singleLine(layoutParent(element)) ? ' [one line]' : ''}`,
        visual: (rank.get(element) ?? -1) + 1
      };
    });
  }

  return { findings, allowed, region: regionSelector, debugOrder };
}

// ---------------------------------------------------------------------------------------------
// Page states, in the order a person meets them. The Home fleet is first because it is the page's
// default; opening a machine gives the hub and selects that machine for every later state.
// ---------------------------------------------------------------------------------------------

const results = [];
const pageFailures = [];
const measuredPages = new Set();
const allowedStops = [];

async function measure(page, label, region = 'main') {
  const result = await page.evaluate(collectInPage, region);
  measuredPages.add(label);
  for (const finding of result.findings) {
    results.push({ page: label, ...finding });
  }
  for (const stop of result.allowed) {
    allowedStops.push({ page: label, ...stop });
  }
  if (process.env.A11Y_DEBUG) {
    console.log(`a11y-audit: tab order for ${label} (${region})`);
    for (const entry of result.debugOrder) {
      console.log(
        `  #${String(entry.visual).padStart(2)}  ${String(entry.top).padStart(4)}px  ` +
          `${String(entry.left).padStart(4)}px  h${String(entry.height).padStart(3)}  ` +
          `${entry.layout ?? ''}  ${entry.name}`
      );
    }
  }
  return result;
}

function report() {
  const sorted = [...results].sort(
    (a, b) =>
      a.page.localeCompare(b.page) ||
      a.kind.localeCompare(b.kind) ||
      a.selector.localeCompare(b.selector)
  );
  for (const finding of sorted) {
    console.log(`${finding.page}\t${finding.kind}\t${finding.selector}\t${finding.detail}`);
  }
  const byKind = new Map();
  for (const finding of sorted) {
    byKind.set(finding.kind, (byKind.get(finding.kind) ?? 0) + 1);
  }
  console.log('');
  console.log(
    `a11y-audit: ${sorted.length} finding(s) across ${measuredPages.size} page state(s)` +
      (byKind.size > 0
        ? ` (${[...byKind.entries()].map(([kind, count]) => `${kind} ${count}`).join(', ')})`
        : '')
  );
  if (allowedStops.length > 0) {
    console.log(
      `a11y-audit: ${allowedStops.length} allowed tab stop(s): named scrollable regions ` +
        `(${[...new Set(allowedStops.map((stop) => stop.page))].join(', ')})`
    );
  }
  if (sorted.length > 0) process.exitCode = 1;
}

let server = null;
let browser = null;

try {
  const started = await startServer();
  server = started.server;
  const origin = started.origin;
  console.log(`a11y-audit: serving dist/index.html at ${origin}/`);

  try {
    browser = await playwright.chromium.launch(
      process.env.CONSOLE_BROWSER ? { executablePath: process.env.CONSOLE_BROWSER } : {}
    );
  } catch (error) {
    throw new Error(
      `chromium.launch() failed (${error instanceof Error ? error.message : String(error)}). ` +
        'Set CONSOLE_BROWSER to a Chromium executable if the Playwright browser is not installed.'
    );
  }

  const context = await browser.newContext({
    viewport: { width: 1280, height: 900 },
    colorScheme: 'light',
    deviceScaleFactor: 1
  });
  const page = await context.newPage();
  await installPage(page, origin);

  await boot(page, origin);
  await measure(page, 'home-fleet');

  // A selected machine is a section of Home, not a second page: the fleet stays and the summary
  // is measured with it. The machine's own live page is Device → Overview, measured next.
  await page.locator('.fleet-open').first().click();
  await page.locator('.machine-summary').waitFor({ state: 'visible', timeout: 5_000 });
  await settle(page, 400);
  await measure(page, 'home-selected');

  await page.getByRole('button', { name: 'Open Device', exact: true }).click();
  await page.getByRole('heading', { name: 'Overview', exact: true, level: 1 }).waitFor({
    state: 'visible',
    timeout: 5_000
  });
  await settle(page, 400);
  await measure(page, 'device-overview');

  // Activity: the timeline is the default view, and one row is opened so the evidence panel and
  // its own controls are part of the measurement.
  await rail(page, 'Activity');
  await page.locator('.timeline-row').first().waitFor({ state: 'visible', timeout: 5_000 });
  const timelineToggle = page.locator('button.timeline-line').first();
  if ((await timelineToggle.count()) > 0) {
    await timelineToggle.click();
    await page.locator('.evidence').first().waitFor({ state: 'visible', timeout: 5_000 });
    await settle(page, 200);
  }
  await measure(page, 'activity-timeline');

  // The table keeps the old row shape unless the row is fixed; open its record the way it offers.
  await page.getByRole('tab', { name: 'Table' }).click();
  await page.locator('.audit-row').first().waitFor({ state: 'visible', timeout: 5_000 });
  const rowButton = page.locator('.audit-row').first().getByRole('button').first();
  if ((await rowButton.count()) > 0) await rowButton.click();
  else await page.locator('.audit-row').first().click();
  await settle(page, 200);
  await measure(page, 'activity-table');

  // Tools: one tab answers what the machine can do. The catalogue and the plugin list are one page,
  // so both must have rendered before the state is measured; one tool's sheet is a dialog with its
  // own focus order.
  await rail(page, 'Device');
  await deviceTab(page, 'Tools');
  await page.locator('.tool-row').first().waitFor({ state: 'visible', timeout: 5_000 });
  await page.locator('.plugin-name').first().waitFor({ state: 'visible', timeout: 5_000 });
  await settle(page, 200);
  await measure(page, 'tools');
  await page.locator('.tool-row').first().click();
  await page.getByRole('dialog').first().waitFor({ state: 'visible', timeout: 5_000 });
  await settle(page, 200);
  await measure(page, 'tools-dialog', '[role="dialog"]');
  await page.locator('.sheet-close').first().click();
  await page.getByRole('dialog').first().waitFor({ state: 'hidden', timeout: 5_000 }).catch(() => {});

  await deviceTab(page, 'Terminal');
  await page.getByRole('heading', { name: 'Terminal', exact: true, level: 1 }).waitFor({
    state: 'visible',
    timeout: 5_000
  });
  await settle(page, 200);
  await measure(page, 'device-terminal');

  await deviceTab(page, 'Screen');
  await page.getByRole('heading', { name: 'Screen', exact: true, level: 1 }).waitFor({
    state: 'visible',
    timeout: 5_000
  });
  await settle(page, 200);
  await measure(page, 'device-screen');

  // Screen's second view is addressable and is a page the harness renders: the remote browser's
  // address and type fields and its picture are part of the sweep, not an unmeasured corner.
  await page
    .locator('.seg[aria-label="Screen view"]')
    .getByRole('button', { name: 'Browser', exact: true })
    .click();
  await page.locator('.browser-pane').waitFor({ state: 'visible', timeout: 5_000 });
  const refreshView = page.getByRole('button', { name: 'Refresh view', exact: true });
  await refreshView.waitFor({ state: 'visible', timeout: 5_000 });
  await page.waitForFunction(
    () => {
      const button = [...document.querySelectorAll('button')].find(
        (entry) => entry.textContent?.trim() === 'Refresh view'
      );
      return button !== undefined && !button.disabled;
    },
    null,
    { timeout: 5_000 }
  );
  await refreshView.click();
  await page.locator('.browser-shot').waitFor({ state: 'visible', timeout: 5_000 });
  await settle(page, 250);
  await measure(page, 'device-browser');

  await deviceTab(page, 'Files');
  await page.getByText('notes.txt').waitFor({ state: 'visible', timeout: 5_000 });
  await settle(page, 200);
  await measure(page, 'device-files');

  await deviceTab(page, 'Runs');
  await page.locator('.runs-row').first().waitFor({ state: 'visible', timeout: 5_000 });
  await settle(page, 200);
  await measure(page, 'device-runs');

  await deviceTab(page, 'Machine');
  await page.locator('#system-processes').waitFor({ state: 'visible', timeout: 5_000 });
  await settle(page, 200);
  await measure(page, 'device-machine');

  // Connect is a sheet, not a destination; the account menu is where it is always reachable.
  await page.locator('.account > summary').click();
  await page
    .locator('.account-pop')
    .getByRole('button', { name: 'Connect a client', exact: true })
    .click();
  const connect = page.getByRole('dialog', { name: 'Connect a client' });
  await connect.waitFor({ state: 'visible', timeout: 5_000 });
  await settle(page, 300);
  await measure(page, 'connect-endpoint');
  await measure(page, 'connect-endpoint-dialog', '[role="dialog"]');

  await connect.getByRole('tab', { name: 'Tokens', exact: true }).click();
  await connect.getByRole('heading', { name: 'Create a token' }).waitFor({
    state: 'visible',
    timeout: 5_000
  });
  await settle(page, 300);
  await measure(page, 'connect-tokens');
  await measure(page, 'connect-tokens-dialog', '[role="dialog"]');

  await page.locator('.sheet-close').first().click();
  await context.close().catch(() => {});
} catch (error) {
  pageFailures.push(error instanceof Error ? error.stack || error.message : String(error));
} finally {
  await browser?.close().catch(() => {});
  await stopServer(server);
}

report();
if (pageFailures.length > 0) {
  console.error('');
  for (const failure of pageFailures) console.error(`a11y-audit: ${failure}`);
  console.error(`a11y-audit: ${pageFailures.length} page state(s) could not be measured`);
  process.exitCode = 1;
}
