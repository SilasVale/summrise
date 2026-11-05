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
  name: 'DESKTOP-9VH92BJ',
  online: true,
  version: '0.3.42',
  last_seen_ms: NOW,
  last_frame_ms: NOW - 12_000,
  // The day the console's newest audit window counted for this machine: 283 calls, 38 failed. The
  // row states it on its muted line; the failed half disappears at zero (see OFFLINE_DEVICE).
  today: { client_calls: 283, failed: 38 },
  // The attempt before this channel: a count and a reason, and no moment of its own. The row must
  // not draw a sentence about it (C9), so the gate carries the measured one and asserts it is absent.
  last_attempt: {
    attempts: 1,
    reason: 'the console accepted the channel and then dropped it'
  },
  // The device's own update story, so the gate photographs the verdict line beside the state line
  // and the a11y audit measures the same row.
  update: {
    current: '0.3.42',
    verdict: { outcome: 'ok', detail: 'started 0.3.42' },
    last_update: { to: '0.3.43', outcome: 'refused', at_ms: NOW - 60_000 }
  },
  // The platform the run form chooses its example from. Only `os` and the boot journal are here:
  // the form needs the OS fact, and the journal's newest entry is the moment this process started,
  // which is what makes the refused answer at NOW - 60 s belong to it (ITEM 5).
  host: {
    os: { name: 'Windows', version: '10.0.26100', arch: 'x86_64' },
    restarts: [{ at_ms: NOW - 60 * 60_000, version: '0.3.42', channel: 'stable', pid: 4200 }]
  }
};
// THE OFFLINE ROW'S AGE. The measured DESKTOP-14RJCR8 case: no frame since the console restarted,
// the registry's last channel open is older than the close, and the pair in `last_outage` belongs to
// an earlier one-second absence. The row must age the silence from `last_left_ms` — 1 h 49 m — at
// both widths; the menu must not date the stale pair.
const OFFLINE_DEVICE = {
  id: 'gate-asleep-0123456789abcdef',
  name: 'DESKTOP-14RJCR8',
  online: false,
  version: '0.3.42',
  last_seen_ms: NOW - 2 * 60 * 60_000,
  // The quiet machine's day at zero failures: the row states the calls and omits `· 0 failed`.
  today: { client_calls: 11, failed: 0 },
  // 108 m 30 s, so the rounded row reads "1h 49m" for the gate's whole run (its two widths are read
  // minutes apart, and a fixture exactly on 109 m would tick to 1h 50m between them).
  last_left_ms: NOW - (109 * 60_000 - 30_000),
  last_outage: {
    away_ms: 1_009,
    back_at_ms: NOW - 2 * 60 * 60_000 - 12 * 60_000
  },
  // The measured hub notice: the machine is offline and the console's last attempt failed after
  // the channel was accepted. At 390 the reason used to be cut to one line with an ellipsis.
  last_attempt: {
    attempts: 1,
    reason: 'the console accepted the channel and then dropped it'
  }
};
// The third machine: the fleet compares more than two rows, and this one's day has a failure. It
// carries no update story, so its row stays the measured 45 px at 1440 — the height the day must not
// add to.
const QUIET_DEVICE = {
  id: 'gate-quiet-0123456789abcdef',
  name: 'DESKTOP-CPH4KB2',
  online: true,
  version: '0.3.42',
  last_seen_ms: NOW - 30_000,
  last_frame_ms: NOW - 30_000,
  today: { client_calls: 84, failed: 2 },
  host: null
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
// the fold has a remainder to offer without the two rows the other legs assert on. Three of them are
// client calls, so the newest sixty rows are the measured shape: 4 client calls and 56 console
// reads. The rest are the console's own calls: its 73 reads are folded behind the hub's one chip,
// and the Now band counts the AI and must stay Quiet with them in the day.
const DAY_START = new Date(NOW);
DAY_START.setHours(0, 0, 0, 0);
const HISTORIC_LINES = Array.from({ length: 75 }, (_, index) => ({
  ts_ms: DAY_START.getTime() + 1_000,
  kind: 'mcp',
  device_id: DEVICE.id,
  token_id: null,
  tool: index < 3 ? `runs.${index}` : `history.${index}`,
  detail: null,
  status: 200,
  caller: index < 3 ? 'client' : 'console'
}));
// The fixture's own day counts, derived from its clock. The two recent rows are today except in the
// minutes after local midnight, so every expected number is computed instead of hardcoded: the hub
// draws the client calls, the chip counts the console reads, and Activity draws every row.
const FIXTURE_LINES = [CONSOLE_LINE, AUDIT_LINE, ...HISTORIC_LINES];
const TODAY_LINES = FIXTURE_LINES.filter((line) => line.ts_ms >= DAY_START.getTime());
const TODAY_CLIENT = TODAY_LINES.filter((line) => line.caller === 'client').length;
const TODAY_CONSOLE = TODAY_LINES.filter((line) => line.caller === 'console').length;
const ALL_CLIENT = FIXTURE_LINES.filter((line) => line.caller === 'client').length;
// The newest client row in the window is the one a narrowed list draws first. It is the run's line
// at a normal hour and a historic call in the five minutes after midnight, when `AUDIT_LINE` is
// yesterday; the leg names whichever it is rather than racing the clock.
const FIRST_CLIENT = [...FIXTURE_LINES]
  .filter((line) => line.caller === 'client')
  .sort((left, right) => right.ts_ms - left.ts_ms)[0];
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
  if (method === 'GET' && pathname === '/fleet') return json([DEVICE, OFFLINE_DEVICE, QUIET_DEVICE]);
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
  const offlineRow1440 = page.getByRole('button', { name: /^Open DESKTOP-14RJCR8/ });
  const offlineState1440 = (await offlineRow1440.locator('.fleet-state').textContent())?.trim();
  if (offlineState1440 !== 'offline · last seen 1h 49m ago') {
    throw new Error(`the offline row read "${offlineState1440}" at 1440`);
  }
  step(`at 1440 the offline row says "${offlineState1440}"`);
  // **A RECONNECT IS NOT SHOWN AS NOW (C9).** The online row carries the measured `last_attempt` —
  // one attempt, the console's channel drop — and neither row may draw a sentence about it: both
  // live rows said "Reconnected after 1 attempt — …" at 10:07 about a 09:41 restart, with no date.
  if ((await page.locator('article.fleet-row .fleet-attempt').count()) !== 0) {
    throw new Error('a fleet row drew an undated reconnect sentence');
  }
  step('the online row carries a last attempt and draws no reconnect sentence');
  // **ITEM 5: THE VERSION ONCE, AND THE UPDATE AS THE NEW FACT.** The chip is the row's one
  // version. The refused answer is one minute old and the journal says the process started an hour
  // ago, so it belongs to this start and is drawn as the updater's own sentence — not as
  // "started 0.3.42 (updated)", which repeated the chip and dated nothing.
  const onlineRow = page.getByRole('button', { name: /^Open DESKTOP-9VH92BJ/ });
  const onlineRowText = (await onlineRow.textContent()) ?? '';
  const versionCopies = (onlineRowText.match(/0\.3\.42/g) ?? []).length;
  if (versionCopies !== 1) {
    throw new Error(`the online row carried ${versionCopies} copies of its version: "${onlineRowText}"`);
  }
  const verdict = (await onlineRow.locator('.fleet-verdict').textContent())?.trim();
  if (verdict !== 'update to 0.3.43 refused') {
    throw new Error(`the online row's update line read "${verdict}"`);
  }
  step(`the online row states "${verdict}" and the version once`);
  // **THE FLEET COMPARES THE WORK, NOT ONLY THE PRESENCE.** Every row states the day the console's
  // newest audit window counted: `283 calls · 38 failed`, the failed half gone at zero, and nothing
  // for a machine the window never names. The counts are on the state's muted line, so a row with no
  // verdict line keeps the measured 45 px.
  const fleet1440 = await page.evaluate(() =>
    [...document.querySelectorAll('article.fleet-row')].map((row) => {
      const name = row.querySelector('.fleet-name');
      const state = row.querySelector('.fleet-state');
      const work = row.querySelector('.fleet-work');
      const height = Math.round(row.getBoundingClientRect().height);
      return {
        name: name?.textContent?.trim() ?? '',
        state: state?.textContent?.trim() ?? '',
        work: work?.textContent?.trim() ?? null,
        height,
        stateTop: state ? Math.round(state.getBoundingClientRect().top) : null,
        workTop: work ? Math.round(work.getBoundingClientRect().top) : null
      };
    })
  );
  const workBy = new Map(fleet1440.map((row) => [row.name, row.work]));
  const expectedWork = new Map([
    ['DESKTOP-9VH92BJ', '283 calls · 38 failed'],
    ['DESKTOP-14RJCR8', '11 calls'],
    ['DESKTOP-CPH4KB2', '84 calls · 2 failed']
  ]);
  for (const [name, expected] of expectedWork) {
    if (workBy.get(name) !== expected) {
      throw new Error(`the ${name} row read "${workBy.get(name)}" instead of "${expected}"`);
    }
  }
  const plainRows = fleet1440.filter(
    (row) => row.name === 'DESKTOP-14RJCR8' || row.name === 'DESKTOP-CPH4KB2'
  );
  for (const row of plainRows) {
    // 45 px is the measured row on the live console; a browser whose font metrics round it to 44
    // still meets the budget — what must not happen is a second line, which `workTop` rules out.
    if (row.height > 45) {
      throw new Error(`the ${row.name} row grew to ${row.height} px at 1440; the row is 45 px`);
    }
    if (row.workTop !== row.stateTop) {
      throw new Error(`the ${row.name} row's day is not on its state's line at 1440`);
    }
  }
  step(
    `the three fleet rows state their days (${[...expectedWork.values()].join(' | ')}); ` +
      `the rows without a verdict are ${plainRows.map((row) => `${row.name} ${row.height}px`).join(', ')} ` +
      `at 1440 (45 px is the measured live row)`
  );
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.waitForTimeout(200);

  await page.locator('.fleet-open').first().click();
  await page.getByRole('heading', { name: 'Home' }).waitFor({ timeout: 10_000 });
  step('opened DESKTOP-9VH92BJ; Home states the hub');

  // THE NOW BAND COUNTS THE AI, NOT THE CONSOLE'S OWN READS. The activity stream above serves one
  // `terminal.list` the console made twenty seconds ago and one client call five minutes ago; a
  // console read under ninety seconds must not make an idle machine read "Active now". The three
  // historic client calls are today's at 00:00:01, so in the first ninety seconds of the day they
  // are legitimately recent and the band is right to say Active; the leg does not claim otherwise.
  const historicAge = Date.now() - (DAY_START.getTime() + 1_000);
  if (historicAge >= 90_000) {
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
    step('the Now band stays Quiet with the console\'s own read in the day');
  } else {
    step('the Now band leg skipped: the day is younger than its newest client call');
  }

  // **THE HUB IS THE AI'S WORK; THE CONSOLE'S READS ARE ONE CHIP AWAY.** The fixture's newest sixty
  // rows are the measured shape — 4 client calls and 56 console reads — above a day line that counts
  // the four. The hub draws the four: the console's own polling is counted on one chip, not drawn
  // as the machine's work. Clicking the chip adds it; the fold then offers the rest of today, and
  // nothing is dropped. The heights are measured, not asserted, so the change has a before/after.
  const hubMeasure = () =>
    page.evaluate(() => {
      const main = document.querySelector('main');
      return {
        rows: document.querySelectorAll('.timeline-row').length,
        height: main ? main.scrollHeight : 0
      };
    });
  const hubChip = page.getByRole('button', { name: `Console reads · ${TODAY_CONSOLE}` });
  await hubChip.waitFor({ state: 'visible', timeout: 10_000 });
  const todayLine = (await page.locator('.home-today').textContent())?.trim();
  const todayCalls = `Today ${TODAY_CLIENT} client ${TODAY_CLIENT === 1 ? 'call' : 'calls'}`;
  if (!todayLine?.startsWith(todayCalls)) {
    throw new Error(`the hub's day line read "${todayLine}", expected "${todayCalls}"`);
  }
  await page.setViewportSize({ width: 1440, height: 960 });
  await page.waitForTimeout(500);
  const hub1440 = await hubMeasure();
  console.log(
    `browser-gate: hub at 1440: ${hub1440.rows} rows, main ${hub1440.height} px` +
      ` (${TODAY_CONSOLE} console reads folded)`
  );
  if (hub1440.rows !== TODAY_CLIENT) {
    throw new Error(`the hub drew ${hub1440.rows} rows; ${TODAY_CLIENT} are the AI's calls`);
  }
  if ((await page.locator('.timeline-caller').count()) !== 0) {
    throw new Error('the hub drew the console\'s own reads before they were asked for');
  }
  if ((await hubChip.getAttribute('aria-pressed')) !== 'false') {
    throw new Error('the console-reads chip was pressed before it was chosen');
  }
  await hubChip.click();
  const foldedRows = Math.min(TODAY_CLIENT + TODAY_CONSOLE, 60);
  await page.waitForFunction(
    (expected) => document.querySelectorAll('.timeline-row').length === expected,
    foldedRows,
    { timeout: 10_000 }
  );
  if ((await hubChip.getAttribute('aria-pressed')) !== 'true') {
    throw new Error('the console-reads chip did not report its pressed state');
  }
  await page.locator('.timeline-caller').first().waitFor({ state: 'visible', timeout: 10_000 });
  const hubWithConsole = await hubMeasure();
  console.log(
    `browser-gate: hub with console reads at 1440: ${hubWithConsole.rows} rows, ` +
      `main ${hubWithConsole.height} px`
  );
  // The rest of today is still behind the fold the console chip just widened.
  const more = page.getByRole('button', { name: /^Show the rest of today \(\d+ more\)$/ });
  const hidden = Math.max(0, TODAY_CLIENT + TODAY_CONSOLE - 60);
  if (hidden > 0) {
    await more.waitFor({ state: 'visible', timeout: 10_000 });
    const moreText = (await more.textContent())?.trim() ?? '';
    if (Number(/\((\d+) more\)/.exec(moreText)?.[1] ?? -1) !== hidden) {
      throw new Error(`the hub offered "${moreText}" for ${hidden} rows behind the fold`);
    }
    await more.click();
    await page.waitForFunction(
      (expected) => document.querySelectorAll('.timeline-row').length === expected,
      TODAY_CLIENT + TODAY_CONSOLE,
      { timeout: 10_000 }
    );
    if ((await more.count()) !== 0) throw new Error('the fold control stayed after the day expanded');
  }
  // The toggle is a view, not a stored preference: off again draws only the AI's calls.
  await hubChip.click();
  await page.waitForFunction(
    (expected) => document.querySelectorAll('.timeline-row').length === expected,
    TODAY_CLIENT,
    { timeout: 10_000 }
  );
  step(
    `the hub drew the AI's ${TODAY_CLIENT} calls, not the ${TODAY_CONSOLE} console reads, ` +
      `and the chip restored all ${TODAY_CLIENT + TODAY_CONSOLE} in one click`
  );
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(500);
  const hub390 = await hubMeasure();
  console.log(
    `browser-gate: hub at 390: ${hub390.rows} rows, main ${hub390.height} px`
  );
  if (hub390.rows !== TODAY_CLIENT) {
    throw new Error(`the hub drew ${hub390.rows} rows at 390; ${TODAY_CLIENT} are the AI's calls`);
  }
  // The first screen is the AI's work: the first row is the newest client call, not a console read.
  const firstHubRow = (await page.locator('.timeline-row').first().textContent()) ?? '';
  if (!firstHubRow.includes(FIRST_CLIENT.tool)) {
    throw new Error(`the hub's first row was not the AI's call: "${firstHubRow}"`);
  }
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.waitForTimeout(200);

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

  // THE DAY'S COUNT, AND ONE CHIP TO SEE THE AI ALONE. Activity is the audit: every caller is drawn
  // by default — all 77 rows — and the hub's console-reads chip must not appear here. Its own
  // "Clients only" chip is the way to narrow, and the header then states the subset it drew.
  const expectedCount = `Today ${TODAY_CLIENT} client ${
    TODAY_CLIENT === 1 ? 'call' : 'calls'
  } · 0 failed · 0 unattributed`;
  const countText = (await page.locator('.activity-today').textContent())?.trim();
  if (countText !== expectedCount) {
    throw new Error(`the Activity header stated "${countText}", expected "${expectedCount}"`);
  }
  const allActivityRows = await page.locator('.timeline-row').count();
  if (allActivityRows !== FIXTURE_LINES.length) {
    throw new Error(
      `Activity drew ${allActivityRows} rows; the audit's default is all ${FIXTURE_LINES.length}`
    );
  }
  if ((await page.locator('.timeline-console-reads').count()) !== 0) {
    throw new Error('Activity drew the hub\'s console-reads chip');
  }
  const clientsChip = page.getByRole('button', { name: `Clients only · ${ALL_CLIENT}` });
  if ((await clientsChip.getAttribute('aria-pressed')) !== 'false') {
    throw new Error('the Clients only chip was pressed before it was chosen');
  }
  await clientsChip.click();
  await page.waitForFunction(
    (expected) => document.querySelectorAll('.timeline-row').length === expected,
    ALL_CLIENT,
    { timeout: 10_000 }
  );
  if ((await clientsChip.getAttribute('aria-pressed')) !== 'true') {
    throw new Error('the Clients only chip did not report its pressed state');
  }
  // THE HEADER FOLLOWS THE CHIP. The chip hides the console's and the unknown rows, so the count
  // beside it must be the client subset the list draws — not the whole window's sentence over two
  // rows. This is the item's contract in the browser, not only in jsdom.
  const narrowedCount = (await page.locator('.activity-today').textContent())?.trim();
  const expectedNarrowed = `Today ${TODAY_CLIENT} client ${
    TODAY_CLIENT === 1 ? 'call' : 'calls'
  } · 0 failed`;
  if (narrowedCount !== expectedNarrowed) {
    throw new Error(`the chip-on header stated "${narrowedCount}", expected "${expectedNarrowed}"`);
  }
  const narrowed = (await page.locator('.timeline-row').first().textContent()) ?? '';
  if (!narrowed.includes(FIRST_CLIENT.tool)) {
    throw new Error(`the Clients only chip kept a row that is not the AI's call: "${narrowed}"`);
  }
  await clientsChip.click();
  await page.waitForFunction(
    (expected) => document.querySelectorAll('.timeline-row').length === expected,
    allActivityRows,
    { timeout: 10_000 }
  );
  step(
    `the header counted ${TODAY_CLIENT} client calls, Activity kept all ${allActivityRows} rows ` +
      `and Clients only narrowed them to ${ALL_CLIENT}`
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
  // THE EXAMPLE IS ONE THE MACHINE CAN RUN (C8). The old `echo hello` was a shell builtin typed
  // into argv mode: the device answered "runs.exec: program not found" in 4 ms. The fixture's OS
  // fact says Windows, so the field must name cmd.exe — a program in System32, not a builtin.
  const runExample = (await runSection.locator('#runs-command').getAttribute('placeholder')) ?? '';
  if (runExample !== 'cmd.exe /c echo hello') {
    throw new Error(`the run form's example is "${runExample}" on a Windows device`);
  }
  step(
    'opened Device → Tools; the run form precedes the catalogue, the plugin list and the exec form are one answer'
  );
  step(`the run form's example is "${runExample}", a program this Windows machine has`);
  await page.screenshot({ path: path.join(OUT, '05-tools.png') });

  // **ITEM 5: THE MACHINE TAB DOES NOT RE-STATE THE BUILD.** The header says `Running · v0.3.42`
  // once; the measured `build 0.3.42 · online` here was a third copy. The line keeps reachability
  // and drops the build.
  await page.getByRole('button', { name: 'Machine', exact: true }).click();
  await page.getByRole('heading', { name: 'Machine', exact: true }).waitFor({ timeout: 10_000 });
  const machinePageText = await page.locator('.system-page').innerText();
  if (machinePageText.includes('build ')) {
    throw new Error('the Machine tab repeated the build the header already shows');
  }
  step('the Machine tab states reachability without a second build line');

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
  await page.getByRole('button', { name: /^Open DESKTOP-14RJCR8/ }).waitFor({ timeout: 10_000 });
  const offlineState390 = (
    await page.getByRole('button', { name: /^Open DESKTOP-14RJCR8/ }).locator('.fleet-state').textContent()
  )?.trim();
  if (offlineState390 !== 'offline · last seen 1h 49m ago') {
    throw new Error(`the offline row read "${offlineState390}" at 390`);
  }
  step(`at 390 the offline row says "${offlineState390}"`);
  // **AT 390 THE COUNTS GET THEIR OWN LINE, NOT THE NAME'S.** The status cell takes the full width
  // below dot and name, and the day wraps under the state: every name keeps the first line's width,
  // and the page grows downward instead of sideways.
  const fleet390 = await page.evaluate(() => {
    const rows = [...document.querySelectorAll('article.fleet-row')].map((row) => {
      const name = row.querySelector('.fleet-name');
      const work = row.querySelector('.fleet-work');
      const nameBox = name?.getBoundingClientRect();
      const workBox = work?.getBoundingClientRect();
      return {
        name: name?.textContent?.trim() ?? '',
        work: work?.textContent?.trim() ?? null,
        nameWidth: nameBox ? Math.round(nameBox.width) : 0,
        nameRight: nameBox ? Math.round(nameBox.right) : 0,
        nameBottom: nameBox ? Math.round(nameBox.bottom) : 0,
        workTop: workBox ? Math.round(workBox.top) : null,
        height: Math.round(row.getBoundingClientRect().height)
      };
    });
    return {
      viewport: window.innerWidth,
      scrollWidth: document.documentElement.scrollWidth,
      rows
    };
  });
  if (fleet390.rows.length !== 3) {
    throw new Error(`the fleet drew ${fleet390.rows.length} rows at 390`);
  }
  for (const row of fleet390.rows) {
    if (!row.work) {
      throw new Error(`the ${row.name} row lost its day at 390`);
    }
    if (row.nameWidth < 120) {
      throw new Error(`the ${row.name} name is only ${row.nameWidth} px wide at 390`);
    }
    if (row.nameRight > fleet390.viewport) {
      throw new Error(`the ${row.name} name is outside the viewport at 390`);
    }
    if (row.workTop === null || row.workTop < row.nameBottom) {
      throw new Error(`the ${row.name} row's day is not below the name at 390`);
    }
  }
  if (fleet390.scrollWidth > fleet390.viewport) {
    throw new Error(
      `the fleet page scrolls sideways at 390: ${fleet390.scrollWidth} > ${fleet390.viewport}`
    );
  }
  const heights390 = fleet390.rows.map((row) => `${row.name} ${row.height}px`).join(', ');
  step(
    `at 390 the three rows carry their counts below the name ` +
      `(${heights390}); no sideways scroll`
  );
  await page.locator('nav.rail button[title="Activity"]').click();
  await page.getByRole('heading', { name: 'Activity' }).waitFor({ timeout: 10_000 });
  // **A PAUSED PAGE RESUMES; IT DOES NOT REFRESH (C10).** The measured button reissued the poll's
  // read (`/fleet`, `/audit?limit=1000&device=…`, `/tokens`), and a page paused at 955 rows stayed
  // 955 after the click. The poll and Resume own the two jobs.
  const activityActions = await page.locator('.page-actions button').allTextContents();
  if (activityActions.some((text) => text.trim() === 'Refresh')) {
    throw new Error(`Activity still offers Refresh: ${JSON.stringify(activityActions)}`);
  }
  await page.getByRole('button', { name: 'Pause' }).click();
  const resume390 = page.getByRole('button', { name: /^Resume/ });
  await resume390.waitFor({ timeout: 5_000 });
  await resume390.click();
  step('Activity offers no Refresh: Pause/Resume is the paused page\'s one way back');
  await page.getByRole('tab', { name: 'Table' }).click();
  const firstAuditRow = page.locator('.audit-row').first();
  await firstAuditRow.waitFor({ timeout: 10_000 });
  const audit390 = await page.evaluate(() => {
    const row = document.querySelector('.audit-row');
    const runRow = [...document.querySelectorAll('.audit-row')].find((entry) =>
      entry.textContent?.includes('runs.exec')
    );
    const button = row?.querySelector('.audit-disclosure');
    const scroller = row?.closest('.table-scroll');
    const deviceCell = row?.cells[1];
    const runDeviceCell = runRow?.cells[1];
    const short = deviceCell?.querySelector('.audit-device-short');
    const rect = (element) => {
      const box = element.getBoundingClientRect();
      return {
        left: Math.round(box.left),
        right: Math.round(box.right),
        width: Math.round(box.width)
      };
    };
    const rowBox = row?.getBoundingClientRect();
    const deviceBox = deviceCell?.getBoundingClientRect();
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
        : null,
      device: {
        short: short?.textContent?.trim() ?? null,
        text: deviceCell?.textContent?.trim() ?? null,
        title: deviceCell?.getAttribute('title') ?? null,
        runTitle: runDeviceCell?.getAttribute('title') ?? null,
        cellWidth: Math.round(deviceBox?.width ?? 0),
        cellHeight: Math.round(deviceBox?.height ?? 0),
        rowHeight: Math.round(rowBox?.height ?? 0)
      }
    };
  });
  console.log(
    `browser-gate: audit at 390: disclosure ${JSON.stringify(
      audit390.disclosure
    )}, viewport ${audit390.viewport}/${audit390.documentScrollWidth}, scroller ${
      audit390.scroller?.scrollWidth
    }/${audit390.scroller?.clientWidth}, first-cell ${audit390.firstCellHasButton}, ` +
      `device ${JSON.stringify(audit390.device)}`
  );
  // **THE PHONE'S DEVICE CELL NAMES THE MACHINE IN ONE OR TWO LINES.** The measured cell held
  // "DESKTOP-9VH92BJ console" in 67 px and grew the row to 91 px. The full name stays the cell's
  // accessible name and its title; the drawn string is the tail the three machines do not share.
  if (audit390.device.short !== '9VH92BJ') {
    throw new Error(
      `the 390 device cell drew ${JSON.stringify(
        audit390.device.short ?? audit390.device.text
      )} instead of the tail "9VH92BJ"`
    );
  }
  if (!audit390.device.title?.includes('DESKTOP-9VH92BJ')) {
    throw new Error(`the 390 device cell's title is ${JSON.stringify(audit390.device.title)}`);
  }
  if (!audit390.device.runTitle?.includes('DESKTOP-9VH92BJ')) {
    throw new Error(`the run row's device title is ${JSON.stringify(audit390.device.runTitle)}`);
  }
  if (audit390.device.rowHeight > 70) {
    throw new Error(
      `the 390 audit row is ${audit390.device.rowHeight} px; the device cell is ` +
        `${audit390.device.cellHeight} px, so the row is still more than two lines`
    );
  }
  const fullNameCells = await page.getByRole('cell', { name: 'DESKTOP-9VH92BJ', exact: true }).count();
  if (fullNameCells === 0) {
    throw new Error('the full machine name is not the device cell\'s accessible name at 390');
  }
  step(
    `at 390 the device cell draws "${audit390.device.short}" with the full name in its title ` +
      `and accessible name; the row is ${audit390.device.rowHeight} px ` +
      `(cell ${audit390.device.cellWidth}×${audit390.device.cellHeight}, was 67×91)`
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

  // **THE OFFLINE REASON WRAPS INSTEAD OF LOSING ITS LAST WORD.** The measured notice cut
  // "…and then dropped it" to "…and then dro…": 283 px of text in a 265 px line, with the rest only
  // in a title a phone cannot hover. The reason must be whole on screen and flow onto a second line.
  await page.locator('nav.rail button[title="Home"]').click();
  await page.getByRole('button', { name: /^Open DESKTOP-14RJCR8/ }).click();
  await page.getByRole('heading', { name: 'Home', exact: true }).waitFor({ timeout: 10_000 });
  const reason390 = await page.evaluate(() => {
    const element = document.querySelector('.attempt-reason');
    if (!element) return null;
    const style = getComputedStyle(element);
    const box = element.getBoundingClientRect();
    return {
      text: element.textContent?.trim() ?? '',
      title: element.getAttribute('title'),
      whiteSpace: style.whiteSpace,
      textOverflow: style.textOverflow,
      overflow: style.overflow,
      lines: element.getClientRects().length,
      width: Math.round(box.width),
      right: Math.round(box.right),
      viewport: document.documentElement.clientWidth
    };
  });
  console.log(`browser-gate: offline reason at 390: ${JSON.stringify(reason390)}`);
  if (!reason390) throw new Error('the offline hub drew no reason at 390');
  const reasonText = 'the console accepted the channel and then dropped it';
  if (reason390.text !== reasonText) {
    throw new Error(`the offline reason read ${JSON.stringify(reason390.text)}`);
  }
  if (reason390.textOverflow === 'ellipsis' || reason390.whiteSpace === 'nowrap') {
    throw new Error(
      `the offline reason is still cut to one line: ${JSON.stringify({
        whiteSpace: reason390.whiteSpace,
        textOverflow: reason390.textOverflow
      })}`
    );
  }
  if (reason390.lines < 2) {
    throw new Error(`the offline reason drew ${reason390.lines} line; the 283 px sentence must wrap`);
  }
  if (reason390.right > reason390.viewport) {
    throw new Error(`the offline reason extends past the viewport at 390: ${reason390.right}`);
  }
  await page.screenshot({ path: path.join(OUT, '07b-offline-390.png') });
  step(
    `at 390 the offline reason is whole across ${reason390.lines} lines ` +
      `(${reason390.width} px), not one clipped line`
  );

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
