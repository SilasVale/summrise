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
  last_seen_ms: NOW,
  last_frame_ms: NOW - 12_000,
  // The device's own update story, so the gate photographs the verdict line beside the state line
  // and the a11y audit measures the same row.
  update: {
    current: '0.3.42',
    verdict: { outcome: 'ok', detail: 'started 0.3.42' },
    last_update: { to: '0.3.43', outcome: 'refused', at_ms: NOW - 60_000 }
  }
};
// THE OFFLINE ROW'S AGE. The measured DESKTOP-14RJCR8 case: no frame since the console restarted,
// the registry's last channel open is older than the close, and the pair in `last_outage` belongs to
// an earlier one-second absence. The row must age the silence from `last_left_ms` — 1 h 49 m — at
// both widths; the menu must not date the stale pair.
const OFFLINE_DEVICE = {
  id: 'gate-asleep-0123456789abcdef',
  name: 'gate-asleep',
  online: false,
  version: '0.3.42',
  last_seen_ms: NOW - 2 * 60 * 60_000,
  // 108 m 30 s, so the rounded row reads "1h 49m" for the gate's whole run (its two widths are read
  // minutes apart, and a fixture exactly on 109 m would tick to 1h 50m between them).
  last_left_ms: NOW - (109 * 60_000 - 30_000),
  last_outage: {
    away_ms: 1_009,
    back_at_ms: NOW - 2 * 60 * 60_000 - 12 * 60_000
  }
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
// The machine's one Tools answer needs a plugin row beside the catalogue; without `plugins.list` the
// page would show the older-build sentence, which is a different thing to photograph.
const PLUGIN = {
  id: 'com.example.hello',
  namespace: 'hello',
  name: 'Hello',
  version: '0.1.0',
  description: 'Two friendly tools',
  state: 'running',
  tools: 2,
  path: 'C:\\dev\\plugins\\hello',
  last_error: null
};
// THE RECORD'S TWO AUDIENCES. The run is an AI client's call, five minutes old; the console's own
// `terminal.list` read is twenty seconds old. The Now band must stay Quiet with only the latter
// recent, and both rows stay in the table with the console's marked.
const AUDIT_LINE = {
  ts_ms: NOW - 5 * 60_000,
  kind: 'mcp',
  device_id: DEVICE.id,
  token_id: null,
  tool: 'runs.exec',
  detail: 'run r-1 exited 0 in 42 ms',
  status: 200,
  caller: 'client'
};
const CONSOLE_LINE = {
  ts_ms: NOW - 20_000,
  kind: 'mcp',
  device_id: DEVICE.id,
  token_id: null,
  tool: 'terminal.list',
  detail: null,
  status: 200,
  caller: 'console'
};
// **THE HUB'S DAY IS WIDER THAN ONE SCREEN.** These 75 lines are today's at any hour — they sit one
// second into the local day, and the distinct tool names keep them distinct even at one instant — so
// the fold has a remainder to offer without the two rows the other legs assert on. They are the
// console's calls: the Now band counts the AI and must stay Quiet with them in the day.
const DAY_START = new Date(NOW);
DAY_START.setHours(0, 0, 0, 0);
const HISTORIC_LINES = Array.from({ length: 75 }, (_, index) => ({
  ts_ms: DAY_START.getTime() + 1_000,
  kind: 'mcp',
  device_id: DEVICE.id,
  token_id: null,
  tool: `history.${index}`,
  detail: null,
  status: 200,
  caller: 'console'
}));
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
  'terminal.close',
  'plugins.list'
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

/** The open `/events` responses: a leg below pushes one started and one finished frame. */
const eventStreams = new Set();

/** Write one SSE frame to every page watching the activity stream. */
function pushEvent(name, data) {
  for (const stream of eventStreams) {
    stream.write(`event: ${name}\ndata: ${JSON.stringify(data)}\n\n`);
  }
}

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
  if (method === 'GET' && pathname === '/fleet') return json([DEVICE, OFFLINE_DEVICE]);
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
  // Newest first, as the console serves it: the console's own read is the newer row. `?device=`
  // narrows during the read, exactly as the console does; the gate's one machine owns both rows.
  if (method === 'GET' && pathname === '/audit') {
    const device = url.searchParams.get('device');
    const lines = [CONSOLE_LINE, AUDIT_LINE, ...HISTORIC_LINES]
      .filter((line) => device === null || line.device_id === device)
      // Newest first, as the console serves it.
      .sort((left, right) => right.ts_ms - left.ts_ms);
    return json(lines);
  }
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
    if (name === 'plugins.list') {
      return json({ result: { content: [{ type: 'text', text: JSON.stringify([PLUGIN]) }] } });
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
    // Oldest first, as the console serves a snapshot.
    response.write(
      `event: snapshot\ndata: ${JSON.stringify({ lines: [AUDIT_LINE, CONSOLE_LINE], running: [] })}\n\n`
    );
    eventStreams.add(response);
    const timer = setInterval(() => response.write(': ping\n\n'), 25_000);
    request.on('close', () => {
      eventStreams.delete(response);
      clearInterval(timer);
    });
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

  // **AN OFFLINE MACHINE'S AGE IS THE NEWEST EVIDENCE.** No frame survived the console restart; the
  // registry's connect time is two hours old, the open close is 1 h 49 m old, and the pair in
  // `last_outage` is from an earlier absence. The row must read the open close, at the desktop width.
  await page.setViewportSize({ width: 1440, height: 960 });
  await page.waitForTimeout(200);
  const offlineRow1440 = page.getByRole('button', { name: /^Open gate-asleep/ });
  const offlineState1440 = (await offlineRow1440.locator('.fleet-state').textContent())?.trim();
  if (offlineState1440 !== 'offline · last seen 1h 49m ago') {
    throw new Error(`the offline row read "${offlineState1440}" at 1440`);
  }
  step(`at 1440 the offline row says "${offlineState1440}"`);
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.waitForTimeout(200);

  await page.locator('.fleet-open').first().click();
  await page.getByRole('heading', { name: 'Home' }).waitFor({ timeout: 10_000 });
  step('opened gate-workshop; Home states the hub');

  // THE NOW BAND COUNTS THE AI, NOT THE CONSOLE'S OWN READS. The activity stream above serves one
  // `terminal.list` the console made twenty seconds ago and one client call five minutes ago; a
  // console read under ninety seconds must not make an idle machine read "Active now".
  try {
    await page.waitForFunction(
      () => document.querySelector('.live-state')?.textContent?.trim() === 'Quiet',
      null,
      { timeout: 10_000 }
    );
  } catch {
    const shown = (await page.locator('.live-state').first().textContent().catch(() => ''))?.trim();
    throw new Error(`the Now band read "${shown}" with only the console's own read recent`);
  }
  const rate = (await page.locator('.live-rate').first().textContent())?.trim();
  if (!rate || !rate.includes('no calls in the last minute')) {
    throw new Error(`the Now rate counted the console's own read: "${rate}"`);
  }
  // And the row is still in the record, marked so a reader can tell it from the AI's calls.
  await page.locator('.timeline-caller').first().waitFor({ state: 'visible', timeout: 10_000 });
  step('the Now band stays Quiet, and the console\'s own row is marked in the timeline');

  // THE DAY IS SCANNABLE WITHOUT HIDING THE RECORD. Seventy-seven rows are today's; the hub draws
  // the newest sixty and offers the rest by count. The count is read from the control and the
  // expansion is checked against it, so the leg does not invent an expected remainder.
  const more = page.getByRole('button', { name: /^Show the rest of today \(\d+ more\)$/ });
  await more.waitFor({ state: 'visible', timeout: 10_000 });
  const foldedRows = await page.locator('.timeline-row').count();
  const moreText = (await more.textContent())?.trim() ?? '';
  const hidden = Number(/\((\d+) more\)/.exec(moreText)?.[1] ?? 0);
  if (foldedRows !== 60 || hidden === 0) {
    throw new Error(`the hub drew ${foldedRows} rows and offered "${moreText}"`);
  }
  await more.click();
  await page.waitForFunction(
    (expected) => document.querySelectorAll('.timeline-row').length === expected,
    60 + hidden,
    { timeout: 10_000 }
  );
  if ((await more.count()) !== 0) throw new Error('the fold control stayed after the day expanded');
  step(`the hub drew 60 rows and expanded the rest of today (${hidden} more)`);

  // A CALL IN FLIGHT IS NOT AN AUDIT ROW. The proxy announces it before the device answers; the
  // band must name it while the timeline still has nothing, and the finished frame must clear it.
  pushEvent('started', {
    started: {
      device_id: DEVICE.id,
      tool: 'runs.exec',
      id: 'gate-call-in-flight',
      at_ms: NOW - 72_000
    },
    caller: 'client'
  });
  await page.getByText(/running runs\.exec · \d+m \d+s/).waitFor({ timeout: 10_000 });
  step('the Now band names the call that is running before any audit row exists');
  pushEvent('finished', { finished: { id: 'gate-call-in-flight' } });
  await page
    .locator('.live-running-entry')
    .waitFor({ state: 'detached', timeout: 10_000 });
  step('the finished frame clears the running line');

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

  // THE DAY'S COUNT, AND ONE CHIP TO SEE THE AI ALONE. The fixture is one client call and 76
  // console reads, so the header's count is the day's proof (the client call is today unless the
  // gate runs in the five minutes after midnight) and the chip narrows 77 rows to that one call.
  const midnight = new Date(NOW);
  midnight.setHours(0, 0, 0, 0);
  const clientToday = AUDIT_LINE.ts_ms >= midnight.getTime() ? 1 : 0;
  const expectedCount = `Today ${clientToday} client ${
    clientToday === 1 ? 'call' : 'calls'
  } · 0 failed · 0 unattributed`;
  const countText = (await page.locator('.activity-today').textContent())?.trim();
  if (countText !== expectedCount) {
    throw new Error(`the Activity header stated "${countText}", expected "${expectedCount}"`);
  }
  const allActivityRows = await page.locator('.timeline-row').count();
  const clientsChip = page.getByRole('button', { name: 'Clients only · 1' });
  if ((await clientsChip.getAttribute('aria-pressed')) !== 'false') {
    throw new Error('the Clients only chip was pressed before it was chosen');
  }
  await clientsChip.click();
  await page.waitForFunction(
    () => document.querySelectorAll('.timeline-row').length === 1,
    null,
    { timeout: 10_000 }
  );
  if ((await clientsChip.getAttribute('aria-pressed')) !== 'true') {
    throw new Error('the Clients only chip did not report its pressed state');
  }
  // THE HEADER FOLLOWS THE CHIP. The chip hides the console's and the unknown rows, so the count
  // beside it must be the client subset the list draws — not the whole window's sentence over two
  // rows. This is the item's contract in the browser, not only in jsdom.
  const narrowedCount = (await page.locator('.activity-today').textContent())?.trim();
  const expectedNarrowed = `Today ${clientToday} client ${
    clientToday === 1 ? 'call' : 'calls'
  } · 0 failed`;
  if (narrowedCount !== expectedNarrowed) {
    throw new Error(`the chip-on header stated "${narrowedCount}", expected "${expectedNarrowed}"`);
  }
  const narrowed = (await page.locator('.timeline-row').first().textContent()) ?? '';
  if (!narrowed.includes('runs.exec')) {
    throw new Error(`the Clients only chip kept a row that is not the AI's call: "${narrowed}"`);
  }
  await clientsChip.click();
  await page.waitForFunction(
    (expected) => document.querySelectorAll('.timeline-row').length === expected,
    allActivityRows,
    { timeout: 10_000 }
  );
  step(
    `the header counted ${clientToday} client call, and Clients only narrowed ` +
      `${allActivityRows} rows to the AI's one`
  );

  // Activity opens on the timeline; the audit rows are the table view.
  await page.getByRole('tab', { name: 'Table' }).click();
  const auditRow = page.locator('.audit-row').filter({ hasText: 'runs.exec' }).first();
  await auditRow.waitFor({ timeout: 10_000 });
  step('opened Activity and its Table view; the run\'s row is the one opened, not the console\'s');
  // The record is disclosed by its own button, so a keyboard reaches it too.
  await auditRow.getByRole('button', { name: /Show the record for/ }).click();
  await page.getByRole('button', { name: 'Open in Runs' }).click();
  await page.getByRole('heading', { name: 'Runs' }).waitFor({ timeout: 10_000 });
  await page.getByText('r-1', { exact: true }).first().waitFor({ timeout: 10_000 });
  if (await page.locator('.terminal-panel').isVisible()) {
    throw new Error('the terminal panel stayed visible on the Runs page');
  }
  step('opened r-1 from its audit line; the terminal panel stayed hidden');
  // RUNS IS THE HISTORY NOW. The exec form moved to Tools, so this page must not carry a second way
  // to run a command; the gate would otherwise keep passing on a page with the old form on it.
  if ((await page.locator('.runs-form, #runs-command').count()) > 0) {
    throw new Error('the Runs page still carries the exec form; it belongs under Tools');
  }
  await page.screenshot({ path: path.join(OUT, '04-activity-run.png') });

  // ONE TAB FOR WHAT THE MACHINE CAN DO. The run form, the catalogue and the plugin list are the
  // same answer, so the gate checks all three render on the same page before it moves on — and that
  // the form, the most-used write, is drawn before the 46-deep catalogue a person would scroll past.
  await page.getByRole('button', { name: 'Tools' }).click();
  await page.getByRole('heading', { name: 'Tools', exact: true }).waitFor({ timeout: 10_000 });
  await page.locator('.tool-row').first().waitFor({ timeout: 10_000 });
  await page.locator('.plugin-name').filter({ hasText: 'Hello' }).first().waitFor({ timeout: 10_000 });
  const toolsText = await page.locator('.tools-tab').innerText();
  if (!toolsText.includes('Tools on this machine') || !toolsText.includes('plugins.list')) {
    throw new Error('the Tools tab did not draw the catalogue');
  }
  const runSection = page.locator('section[aria-labelledby="tools-run"]');
  await runSection
    .getByRole('heading', { name: 'Run a command on this machine', exact: true })
    .waitFor({ timeout: 10_000 });
  if ((await runSection.locator('#runs-command').count()) === 0) {
    throw new Error('the exec form is not under Tools');
  }
  const runFirst = await page.evaluate(() => {
    const form = document.querySelector('section[aria-labelledby="tools-run"]');
    const catalogue = document.querySelector('section[aria-labelledby="tools-on-this-machine"]');
    return (
      form !== null &&
      catalogue !== null &&
      (form.compareDocumentPosition(catalogue) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0
    );
  });
  if (!runFirst) {
    throw new Error('the Tools tab draws the catalogue before the exec form');
  }
  step(
    'opened Device → Tools; the run form precedes the catalogue, the plugin list and the exec form are one answer'
  );
  await page.screenshot({ path: path.join(OUT, '05-tools.png') });

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
  await page.screenshot({ path: path.join(OUT, '06-connect-tokens.png') });

  // ITEM 5, MEASURED AT 1280. The sheet is 600 px wide on desktop, so the token row is measured
  // against the sheet, not the viewport: the old sixth column put Revoke past its right edge.
  await page.waitForTimeout(400);
  const sheet1280 = await page.locator('.sheet').boundingBox();
  const revoke1280 = await tokenRows.first().getByRole('button', { name: 'Revoke' }).boundingBox();
  if (!sheet1280 || !revoke1280) throw new Error('the token sheet or its Revoke has no box');
  const inside = (box, outer) =>
    box.x >= outer.x - 0.5 && box.x + box.width <= outer.x + outer.width + 0.5;
  console.log(
    `browser-gate: tokens at 1280: sheet x ${Math.round(sheet1280.x)}..${Math.round(
      sheet1280.x + sheet1280.width
    )}, Revoke x ${Math.round(revoke1280.x)}..${Math.round(revoke1280.x + revoke1280.width)}`
  );
  if (!inside(revoke1280, sheet1280)) {
    throw new Error('the first token\'s Revoke is outside the sheet at 1280');
  }

  // THE PHONE HALF. Resize the same page: the audit table must drop its sixth column, put the
  // disclosure in the first cell, and fit the 390 px content without a sideways scroll.
  await page.keyboard.press('Escape');
  await page.waitForTimeout(250);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(400);
  // The same sentence at 390: the state line takes its own row on the card, but it still states the
  // silence's age and never drops it at the narrow width. The fleet list is Home with no machine
  // chosen, so clear the choice through the header's own menu first.
  await page.locator('details.machine-menu > summary').click();
  await page.getByRole('button', { name: 'All machines' }).click();
  await page.locator('nav.rail button[title="Home"]').click();
  await page.getByRole('heading', { name: 'Machines', exact: true }).waitFor({ timeout: 10_000 });
  await page.getByRole('button', { name: /^Open gate-asleep/ }).waitFor({ timeout: 10_000 });
  const offlineState390 = (
    await page.getByRole('button', { name: /^Open gate-asleep/ }).locator('.fleet-state').textContent()
  )?.trim();
  if (offlineState390 !== 'offline · last seen 1h 49m ago') {
    throw new Error(`the offline row read "${offlineState390}" at 390`);
  }
  step(`at 390 the offline row says "${offlineState390}"`);
  await page.locator('nav.rail button[title="Activity"]').click();
  await page.getByRole('heading', { name: 'Activity' }).waitFor({ timeout: 10_000 });
  await page.getByRole('tab', { name: 'Table' }).click();
  const firstAuditRow = page.locator('.audit-row').first();
  await firstAuditRow.waitFor({ timeout: 10_000 });
  const audit390 = await page.evaluate(() => {
    const row = document.querySelector('.audit-row');
    const button = row?.querySelector('.audit-disclosure');
    const scroller = row?.closest('.table-scroll');
    const rect = (element) => {
      const box = element.getBoundingClientRect();
      return {
        left: Math.round(box.left),
        right: Math.round(box.right),
        width: Math.round(box.width)
      };
    };
    return {
      viewport: document.documentElement.clientWidth,
      documentScrollWidth: document.documentElement.scrollWidth,
      firstCellHasButton: row?.cells[0]?.contains(button ?? null) ?? false,
      disclosure: button ? rect(button) : null,
      scroller: scroller
        ? {
            ...rect(scroller),
            scrollWidth: scroller.scrollWidth,
            clientWidth: scroller.clientWidth
          }
        : null
    };
  });
  console.log(
    `browser-gate: audit at 390: disclosure ${JSON.stringify(
      audit390.disclosure
    )}, viewport ${audit390.viewport}/${audit390.documentScrollWidth}, scroller ${
      audit390.scroller?.scrollWidth
    }/${audit390.scroller?.clientWidth}, first-cell ${audit390.firstCellHasButton}`
  );
  if (!audit390.firstCellHasButton) {
    throw new Error('the audit disclosure is not in the row\'s first cell at 390');
  }
  if (
    !audit390.disclosure ||
    audit390.disclosure.left < 0 ||
    audit390.disclosure.right > audit390.viewport
  ) {
    throw new Error('the audit disclosure is outside the viewport at 390');
  }
  if (
    audit390.documentScrollWidth > audit390.viewport ||
    !audit390.scroller ||
    audit390.scroller.scrollWidth > audit390.scroller.clientWidth
  ) {
    throw new Error('the audit table still scrolls sideways at 390');
  }
  // The measured opener is the real one: it opens the record with no horizontal scroll.
  const disclosure390 = firstAuditRow.locator('button.audit-disclosure');
  if ((await disclosure390.getAttribute('aria-expanded')) === 'true') {
    await disclosure390.click();
    await page.waitForTimeout(150);
  }
  await disclosure390.click();
  await page.locator('.audit-record-row').first().waitFor({ state: 'visible', timeout: 5_000 });
  await page.screenshot({ path: path.join(OUT, '07-audit-390.png') });
  step('at 390 the audit opener is in the first cell; the table fits and the record opens');

  // The Connect sheet at 390: the same measured promise for the token row.
  await page.locator('.account > summary').click();
  await page.locator('.account').getByRole('button', { name: 'Tokens', exact: true }).click();
  await page.getByRole('dialog', { name: 'Tokens' }).waitFor({ timeout: 10_000 });
  const tokenRow390 = page.locator('.token-table tbody tr').first();
  await tokenRow390.waitFor({ timeout: 10_000 });
  await page.waitForTimeout(400);
  const sheet390 = await page.locator('.sheet').boundingBox();
  const revoke390 = await tokenRow390.getByRole('button', { name: 'Revoke' }).boundingBox();
  if (!sheet390 || !revoke390) throw new Error('the phone token sheet or its Revoke has no box');
  console.log(
    `browser-gate: tokens at 390: sheet x ${Math.round(sheet390.x)}..${Math.round(
      sheet390.x + sheet390.width
    )}, Revoke x ${Math.round(revoke390.x)}..${Math.round(revoke390.x + revoke390.width)}`
  );
  if (!inside(revoke390, sheet390)) {
    throw new Error('the first token\'s Revoke is outside the sheet at 390');
  }
  await page.screenshot({ path: path.join(OUT, '08-tokens-390.png') });
  step('at 390 the first token\'s Revoke is inside the sheet, under the token\'s name');

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
