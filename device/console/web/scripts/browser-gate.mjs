#!/usr/bin/env node
// The page's first evidence path: sign in, pick a device, open a pty, type, read it back.
//
//   node scripts/browser-gate.mjs
//
// The console's HTTP routes are answered by Playwright; `/mcp` is a deliberate ECHO, not a shell.
// It proves the page's write goes out and the read comes back into xterm — the seam the jsdom suite
// cannot see — while saying plainly that no device executed anything.
//
// The browser is the Playwright runtime already on this machine (same resolution as the screenshot
// harness); no dependency is added to package.json.

import { existsSync } from 'node:fs';
import { mkdir, readFile } from 'node:fs/promises';
import http from 'node:http';
import { createRequire } from 'node:module';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const WEB = path.resolve(HERE, '..');
const DIST_INDEX = path.join(WEB, 'dist', 'index.html');
const OUT = path.resolve(process.env.CONSOLE_GATE_ROOT || '/tmp/console-browser-gate');
const TOKEN = 'browser-gate-operator-token';
const MARKER = 'gate-marker-7f3a';

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
    const runtime = require(candidate);
    // The `playwright` package is the CI candidate and is installed WITH its browser; a dev box may
    // have it without one, so that candidate is checked. `playwright-core` is the legacy runtime on
    // this box and can launch the headless shell without a full chrome at executablePath().
    if (path.basename(candidate) === 'playwright') {
      const executable = runtime.chromium?.executablePath?.();
      if (executable && !existsSync(executable)) continue;
    }
    playwright = runtime;
    break;
  } catch {
    /* the next candidate */
  }
}
if (!playwright) {
  console.error('browser-gate: playwright-core not found; set PLAYWRIGHT_CORE_DIR');
  process.exit(2);
}

const ICON_PNG = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
  'base64'
);

const indexHtml = await readFile(DIST_INDEX).catch((error) => {
  console.error(`browser-gate: cannot read ${DIST_INDEX}: ${error.message}`);
  console.error('browser-gate: build the page first: cd device/console/web && npm run build');
  process.exit(2);
});

const NOW = Date.now();
const DEVICE = {
  id: 'gate-device-0123456789abcdef',
  name: 'gate-workshop',
  online: true,
  version: '0.3.42',
  last_seen_ms: NOW
};
const RUN = {
  id: 'r-1',
  command: 'echo',
  args: ['hello'],
  cwd: '.',
  exit_code: 0,
  timed_out: false,
  duration_ms: 42,
  started_at_ms: NOW - 5 * 60_000,
  stdout_bytes: 6,
  stderr_bytes: 0
};
const AUDIT_LINE = {
  ts_ms: NOW - 30_000,
  kind: 'mcp',
  device_id: DEVICE.id,
  token_id: null,
  tool: 'runs.exec',
  detail: 'run r-1 exited 0 in 42 ms',
  status: 200
};
// The whole registry, not the selected machine's slice: one token for this machine, one for a
// machine the gate never selects, and one aggregate. The Tokens leg counts the rows against this.
const TOKENS = [
  {
    id: 'gate-token-on-gate-device',
    name: 'gate-laptop',
    device_id: DEVICE.id,
    tools: ['files.read', 'terminal.read'],
    allow_destructive: false,
    created_at_ms: NOW - 2 * 60 * 60_000
  },
  {
    id: 'gate-token-elsewhere',
    name: 'attic-ops',
    device_id: 'gate-device-attic-0123456789',
    tools: ['runs.list'],
    allow_destructive: false,
    created_at_ms: NOW - 3 * 60 * 60_000
  },
  {
    id: 'gate-token-fleet',
    name: 'gate-fleet',
    device_id: '',
    device_ids: [DEVICE.id, 'gate-device-attic-0123456789'],
    tools: ['files.read', 'runs.exec'],
    allow_destructive: true,
    created_at_ms: NOW - 4 * 60 * 60_000
  }
];
const TOOL_NAMES = [
  'terminal.create',
  'terminal.list',
  'terminal.read',
  'terminal.resize',
  'terminal.write',
  'terminal.close'
];

// The only state the gate needs: whether the operator signed in, whether a pty was opened, what the
// page wrote, and whether the echo has been handed back.
const state = {
  signedIn: false,
  opened: false,
  writes: '',
  echoed: false,
  cursor: 0,
  /** Whether the page subscribed to the session's stream: the push path, not the polling one. */
  streamOpened: false,
  /** What the stream has already echoed, so a reconnecting client is not told twice. */
  streamEchoed: false
};

/** The one route that is a stream: the page subscribes to it and the device's bytes arrive on it. */
const streamRoute = /^\/terminal\/[^/]+\/[^/]+\/stream$/;

const json = (body, status = 200) => ({ status, body });
const bodyOf = (request) => {
  try {
    return request.postDataJSON() ?? {};
  } catch {
    return {};
  }
};

function apiReply(request, url) {
  const method = request.method();
  const pathname = url.pathname;

  if (method === 'POST' && pathname === '/auth') {
    if (bodyOf(request).token !== TOKEN) return json({ error: 'invalid_token' }, 401);
    state.signedIn = true;
    return json({ token: TOKEN, expires_at_ms: NOW + 60 * 60 * 1000 });
  }
  if (method === 'GET' && pathname === '/session') {
    return json(
      state.signedIn
        ? { authenticated: true, email: 'operator@gate.test', via: 'token' }
        : { authenticated: false, access: { configured: false } }
    );
  }
  if (method === 'POST' && pathname === '/logout') {
    state.signedIn = false;
    // The console's contract: 200 and a JSON body. `access_logout` is the edge's logout URL to
    // follow after the console's own cookie is cleared; null when there is no edge.
    return json({ access_logout: null });
  }
  if (method === 'GET' && pathname === '/health') {
    return json({ status: 'ok', devices: 1, online: 1, version: 'browser-gate' });
  }
  if (method === 'GET' && pathname === '/fleet') return json([DEVICE]);
  if (method === 'GET' && /^\/fleet\/[^/]+\/tools$/.test(pathname)) {
    return json({
      tools: TOOL_NAMES.map((name) => ({
        name,
        description: `${name} on the gate machine`,
        inputSchema: { type: 'object', properties: {} },
        source: 'builtin',
        capability: name.split('.')[0],
        destructive: name === 'terminal.write' || name === 'terminal.create'
      }))
    });
  }
  if (method === 'GET' && /^\/fleet\/[^/]+\/connections$/.test(pathname)) return json([]);
  if (method === 'GET' && pathname === '/tokens') return json(TOKENS);
  if (method === 'GET' && pathname === '/audit') return json([AUDIT_LINE]);
  if (method === 'GET' && pathname === '/denials') return json([]);
  if (method === 'GET' && pathname === '/tools') return json(TOOL_NAMES.map((name) => ({ name })));

  if (method === 'POST' && pathname === '/mcp') {
    const call = bodyOf(request)?.request?.params;
    const name = call?.name;
    const args = call?.arguments ?? {};
    if (name === 'terminal.list') {
      return json({ result: { content: [{ type: 'text', text: '[]' }] } });
    }
    if (name === 'terminal.create') {
      state.opened = true;
      return json({ result: { content: [{ type: 'text', text: JSON.stringify({ id: 'gate-pty-1' }) }] } });
    }
    if (name === 'terminal.resize') {
      return json({ result: { content: [{ type: 'text', text: JSON.stringify({ rows: 24, cols: 80 }) }] } });
    }
    if (name === 'terminal.write') {
      state.writes += String(args.data ?? '');
      if (state.writes.includes(MARKER)) state.echoed = true;
      return json({ result: { content: [{ type: 'text', text: '{}' }] } });
    }
    if (name === 'terminal.read') {
      // THE ECHO. The marker the page typed is what comes back on the next read; this is not a
      // shell and the script says so in its PASS line.
      if (state.echoed && state.cursor === 0) {
        state.cursor = MARKER.length + 2;
        return json({
          result: { content: [{ type: 'text', text: JSON.stringify({ data: MARKER + '\r\n', cursor: state.cursor }) }] }
        });
      }
      return json({
        result: { content: [{ type: 'text', text: JSON.stringify({ data: '', cursor: state.cursor }) }] }
      });
    }
    if (name === 'runs.list') {
      return json({ result: { content: [{ type: 'text', text: JSON.stringify([RUN]) }] } });
    }
    if (name === 'runs.get') {
      return json({ result: { content: [{ type: 'text', text: JSON.stringify(RUN) }] } });
    }
    if (name === 'terminal.close') {
      return json({ result: { content: [{ type: 'text', text: '{}' }] } });
    }
    return json({ error: { code: -32601, message: `browser-gate has no ${name}` } }, 200);
  }
  return json({ error: 'not_found' }, 404);
}

await mkdir(OUT, { recursive: true });
const server = http.createServer((request, response) => {
  const url = new URL(request.url ?? '/', 'http://127.0.0.1');
  if (request.method === 'GET' && (url.pathname === '/' || url.pathname === '/index.html')) {
    response.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' });
    response.end(indexHtml);
    return;
  }
  if (request.method === 'GET' && url.pathname === '/favicon.ico') {
    response.writeHead(200, { 'content-type': 'image/png', 'cache-control': 'no-store' });
    response.end(ICON_PNG);
    return;
  }
  // THE STREAM IS ANSWERED BY THIS SERVER, NOT BY Playwright's route stub: a stub response is one
  // complete body, and a stream that ends immediately is the failure this gate must not disguise.
  // The echo is written the moment `terminal.write` saw the marker, which is what the page reads
  // back through xterm.
  if (request.method === 'GET' && streamRoute.test(url.pathname)) {
    state.streamOpened = true;
    response.writeHead(200, {
      'content-type': 'text/event-stream; charset=utf-8',
      'cache-control': 'no-store',
      'x-accel-buffering': 'no'
    });
    response.write(': stream open\n\n');
    const timer = setInterval(() => {
      if (state.echoed && !state.streamEchoed) {
        state.streamEchoed = true;
        response.write(
          `event: output\ndata: ${JSON.stringify({ data: MARKER + '\r\n', cursor: MARKER.length + 2 })}\n\n`
        );
        return;
      }
      response.write(': ping\n\n');
    }, 25);
    request.on('close', () => clearInterval(timer));
    return;
  }
  // THE ACTIVITY STREAM, ANSWERED THE SAME WAY. The page follows /events for its newest lines; a
  // 404 there is a console error and the gate fails on it at the end. One snapshot is enough, and
  // the stream stays open so EventSource does not reconnect in a loop.
  if (request.method === 'GET' && url.pathname === '/events') {
    response.writeHead(200, {
      'content-type': 'text/event-stream; charset=utf-8',
      'cache-control': 'no-store',
      'x-accel-buffering': 'no'
    });
    response.write(`event: snapshot\ndata: ${JSON.stringify({ lines: [AUDIT_LINE] })}\n\n`);
    const timer = setInterval(() => response.write(': ping\n\n'), 25_000);
    request.on('close', () => clearInterval(timer));
    return;
  }
  response.writeHead(404, { 'content-type': 'text/plain' });
  response.end('not found');
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const base = `http://127.0.0.1:${server.address().port}`;
console.log(`browser-gate: page at ${base} (echo MCP, not a shell)`);

const browser = await playwright.chromium.launch();
const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
const errors = [];
page.on('console', (message) => {
  if (message.type() === 'error') errors.push(message.text());
});
page.on('pageerror', (error) => errors.push('pageerror: ' + error.message));
page.on('response', (response) => {
  if (response.status() >= 400) console.error('browser-gate: HTTP', response.status(), response.url());
});

await page.route('**/*', async (route) => {
  const url = new URL(route.request().url());
  if (url.origin !== base) return route.continue();
  if (url.pathname === '/' || url.pathname === '/index.html') return route.continue();
  if (url.pathname === '/events') return route.continue();
  if (streamRoute.test(url.pathname)) return route.continue();
  const reply = apiReply(route.request(), url);
  const headers = { 'cache-control': 'no-store' };
  if (reply.status === 204) return route.fulfill({ status: 204, headers });
  return route.fulfill({
    status: reply.status,
    headers: { ...headers, 'content-type': 'application/json; charset=utf-8' },
    body: JSON.stringify(reply.body)
  });
});

const steps = [];
function step(name) {
  steps.push(name);
  console.log(`browser-gate: ${name}`);
}

try {
  await page.goto(base + '/', { waitUntil: 'domcontentloaded' });
  await page.getByRole('heading', { name: 'Sign in to the console' }).waitFor({ timeout: 10_000 });
  step('login form rendered from an unauthenticated /session');
  await page.screenshot({ path: path.join(OUT, '01-login.png') });

  await page.getByLabel('Operator token').fill(TOKEN);
  await page.getByRole('button', { name: 'Sign in' }).click();
  await page.getByRole('heading', { name: 'Machines', exact: true }).waitFor({ timeout: 10_000 });
  step('signed in through /auth; the fleet is Home\'s zero state');
  await page.screenshot({ path: path.join(OUT, '02-machines.png') });

  await page.locator('.fleet-open').first().click();
  await page.getByRole('heading', { name: 'Home' }).waitFor({ timeout: 10_000 });
  step('opened gate-workshop; Home states the hub');

  await page.getByRole('button', { name: 'Device' }).click();
  await page.getByRole('heading', { name: 'Terminal', exact: true }).waitFor({ timeout: 10_000 });
  step('opened Device → Terminal with a session-scoped grant');

  await page.getByRole('button', { name: '+ New session' }).click();
  const dialog = page.getByRole('dialog', { name: 'New terminal session' });
  await dialog.getByRole('button', { name: 'Open' }).click();
  await page.locator('.term-host .xterm').waitFor({ state: 'visible', timeout: 10_000 });
  step('opened one pty; xterm mounted');

  await page.locator('.term-host .xterm').click();
  await page.keyboard.type(MARKER, { delay: 15 });
  await page.waitForFunction(
    (marker) => document.querySelector('.term-host .xterm-rows')?.textContent?.includes(marker) ?? false,
    MARKER,
    { timeout: 10_000 }
  );
  if (!state.writes.includes(MARKER)) throw new Error('the marker never reached terminal.write');
  if (!state.streamOpened) throw new Error('the page never subscribed to the terminal stream');
  step('typed a marker; it reached terminal.write and returned down the open stream');
  await page.screenshot({ path: path.join(OUT, '03-terminal-marker.png') });

  await page.getByRole('button', { name: 'Activity' }).click();
  await page.getByRole('heading', { name: 'Activity' }).waitFor({ timeout: 10_000 });
  // Activity opens on the timeline; the audit rows are the table view.
  await page.getByRole('tab', { name: 'Table' }).click();
  const auditRow = page.locator('.audit-row').first();
  await auditRow.waitFor({ timeout: 10_000 });
  step('opened Activity and its Table view; the audit window carries the run');
  // The record is disclosed by its own button, so a keyboard reaches it too.
  await auditRow.getByRole('button', { name: /Show the record for/ }).click();
  await page.getByRole('button', { name: 'Open in Runs' }).click();
  await page.getByRole('heading', { name: 'Runs' }).waitFor({ timeout: 10_000 });
  await page.getByText('r-1', { exact: true }).first().waitFor({ timeout: 10_000 });
  if (await page.locator('.terminal-panel').isVisible()) {
    throw new Error('the terminal panel stayed visible on the Runs page');
  }
  step('opened r-1 from its audit line; the terminal panel stayed hidden');
  await page.screenshot({ path: path.join(OUT, '04-activity-run.png') });

  // THE CREDENTIAL LIST IS THE WHOLE REGISTRY. `GET /tokens` is the only list route; a row for
  // another machine is still a real token and hidden rows made an occupied registry read "none".
  await page.getByRole('button', { name: 'Connect', exact: true }).click();
  const connect = page.getByRole('dialog', { name: 'Connect a client' });
  await connect.waitFor({ timeout: 10_000 });
  await connect.getByRole('tab', { name: 'Tokens' }).click();
  const tokenRows = page.locator('.token-table tbody tr');
  await tokenRows.first().waitFor({ timeout: 10_000 });
  const drawn = await tokenRows.count();
  if (drawn !== TOKENS.length) {
    throw new Error(`the token list drew ${drawn} rows for ${TOKENS.length} registry tokens`);
  }
  const listed = await page.locator('.token-table tbody').innerText();
  for (const token of TOKENS) {
    if (!listed.includes(token.name)) throw new Error(`the token list dropped ${token.name}`);
  }
  step('opened Connect → Tokens; one row per registry token, including another machine\'s');
  await page.screenshot({ path: path.join(OUT, '05-connect-tokens.png') });

  if (errors.length > 0) throw new Error('console errors: ' + errors.join(' ;; '));
  console.log(`browser-gate: PASS — ${steps.length} steps, ${state.writes.length} bytes written, marker echoed`);
  console.log(`browser-gate: screenshots in ${OUT}`);
  process.exitCode = 0;
} catch (error) {
  console.error('browser-gate: FAIL —', error.message);
  try {
    await page.screenshot({ path: path.join(OUT, 'failure.png') });
  } catch {
    /* the page is already gone */
  }
  process.exitCode = 1;
} finally {
  await browser.close();
  server.close();
}
