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

// The second pty's stream stands in for a device whose ring no longer holds the page's cursor: it
// refuses the first connection with the device's too-old sentence. The page must then PAGE the
// retained output with `terminal.read`'s `limit` (answered by the MCP stub below), draw the page,
// and only then subscribe again at the cursor that page reached. The stream at that reached cursor
// carries nothing: the retained bytes came from the limited read, not from the subscribe frame.
const RETAINED_SESSION = 'gate-pty-2';
const RETAINED_FROM = 1024;
const RETAINED_TEXT = 'gate-retained-after-drop\r\n';
const RETAINED_END = RETAINED_FROM + RETAINED_TEXT.length;

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
  // The local day this machine's audit fixture holds. It is filled in from `TODAY_CLIENT` below,
  // because the fixture's two recent lines can be yesterday in the first minutes after midnight;
  // the fleet row and the hub's day line must both say the day the gate actually serves.
  today: { client_calls: 0, failed: 0 },
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
    last_update: { to: '0.3.43', outcome: 'refused', at_ms: NOW - 60_000 },
    // The device's last check, and what woke it. The Machine tab's update line must draw this
    // rather than treating the refused target as the current one.
    last_check: { at_ms: NOW - 20 * 60_000, trigger: 'channel' }
  },
  // The platform the run form chooses its example from. Only `os` and the boot journal are here:
  // the form needs the OS fact, and the journal's newest entry is the moment this process started,
  // which is what makes the refused answer at NOW - 60 s belong to it (ITEM 5).
  host: {
    os: { name: 'Windows', version: '10.0.26100', arch: 'x86_64' },
    // The per-user helper registered in session 2, and session 2 is disconnected: the Screen page
    // must state that before the press instead of inviting the measured `os error 5`.
    sessions: [
      { id: 0, state: 'disconnected' },
      { id: 2, user: 'Administrator', state: 'disconnected' },
      { id: 5, state: 'connected' }
    ],
    helper: { registered: true, pid: 4812, session: 2 },
    restarts: [{ at_ms: NOW - 60 * 60_000, version: '0.3.42', channel: 'stable', pid: 4200 }],
    // The launcher's current generation carries the channel the update line names when the fleet
    // reports one. No other surface in this gate needs the launcher's update block.
    update: {
      current: { version: '0.3.42', channel: 'stable' },
      previous: null,
      pending: null,
      last_swap: null,
      repairs: []
    }
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
// **THE MEASURED HUB WINDOW HAS ITS OWN MACHINE.** The end of the gate opens this machine's hub to
// prove the sentence the hub states is the 974 rows its fold and two chips can reach — 283 client
// calls, 580 console reads and 111 rows no caller was recorded for — under a 1000-line window whose
// remaining 26 fetched lines are 9 channel boundaries and 17 repeated keys. The day's 283 calls are
// the number its fleet row states, so the page and the fleet compare agree.
const BIG_DEVICE = {
  id: 'gate-bigwindow-0123456789abcdef',
  name: 'DESKTOP-7QK3MN2',
  online: true,
  version: '0.3.42',
  last_seen_ms: NOW - 45_000,
  last_frame_ms: NOW - 45_000,
  today: { client_calls: 283, failed: 0 },
  host: null
};
// **THE INSTALLER THE CONSOLE WAS GIVEN.** The item's door is a same-origin link, so the gate
// stubs the facts the page reads once and never follows the download: the assertion is that the
// fleet's zero state offers the file the console holds, with the numbers the route answered.
const INSTALLER = {
  available: true,
  filename: 'SummriseSetup-0.3.280.exe',
  version: '0.3.280',
  size_bytes: 6_638_339,
  sha256: '26d39c12' + 'ab'.repeat(28)
};
const INSTALLER_SENTENCE =
  'No installer is published for this console yet. Install it from the release, then add the machine for its enrollment command.'
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
// **THE LONG COMMAND IS THE AUDIT'S SHAPE.** The measured record was a 4,590-character
// `powershell.exe -NoProfile -EncodedCommand …` line: 1,190 px of a 1,551 px card at 1440 and
// 1,866 px of 2,299 px at 390, with Exit/Duration/Started starting at y 1,393/2,114. The history
// truncates it to `shortCommand`'s 110 characters; the record now paints the same short form with
// the facts above it, and the full line is behind `Show full command`.
const LONG_PAYLOAD = 'JABF' + 'A'.repeat(4_544);
const LONG_LINE = 'powershell.exe -NoProfile -EncodedCommand ' + LONG_PAYLOAD;
// `shortCommand`'s shape: the first 109 characters and one ellipsis.
const LONG_SHORT = LONG_LINE.slice(0, 109) + '…';
const LONG_RUN = {
  id: 'r-2',
  command: 'powershell.exe',
  args: ['-NoProfile', '-EncodedCommand', LONG_PAYLOAD],
  cwd: 'C:\\Users\\operator',
  exit_code: 1,
  timed_out: false,
  duration_ms: 61_000,
  started_at_ms: NOW - 3 * 60 * 60_000,
  stdout_bytes: 2048,
  stderr_bytes: 12
};
if (LONG_LINE.length !== 4_590) {
  throw new Error(`the gate's long fixture is ${LONG_LINE.length} characters, not the audit's 4,590`);
}
if (LONG_SHORT.length !== 110) {
  throw new Error(`the gate's short fixture is ${LONG_SHORT.length} characters, not 110`);
}
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
// One day, one number: the fleet's `today` is the day this fixture's `/audit/day` would answer.
DEVICE.today = { client_calls: TODAY_CLIENT, failed: 0 };
// The newest client row in the window is the one a narrowed list draws first. It is the run's line
// at a normal hour and a historic call in the five minutes after midnight, when `AUDIT_LINE` is
// yesterday; the leg names whichever it is rather than racing the clock.
const FIRST_CLIENT = [...FIXTURE_LINES]
  .filter((line) => line.caller === 'client')
  .sort((left, right) => right.ts_ms - left.ts_ms)[0];
// **THE MEASURED WINDOW: 1004 FETCHED, 974 THE CONTROLS CAN REACH.** A 1000-line answer for
// BIG_DEVICE: 9 channel boundaries, 17 repeated keys, and 974 rows the hub draws — 283 client calls,
// 580 console reads and 111 rows no caller was recorded for. All of it inside today's first twenty
// minutes, so the day filter keeps every row at any hour the gate runs.
const BIG_CLIENT = 283;
const BIG_CONSOLE = 580;
const BIG_UNATTRIBUTED = 111;
const BIG_BOUNDARIES = 9;
const BIG_REPEATS = 17;
const BIG_DRAWABLE = BIG_CLIENT + BIG_CONSOLE + BIG_UNATTRIBUTED;
let bigTs = DAY_START.getTime() + 1_000 * (BIG_CLIENT + BIG_CONSOLE + BIG_UNATTRIBUTED + BIG_BOUNDARIES + 1);
const bigLine = (overrides) => ({
  ts_ms: (bigTs -= 1_000),
  kind: 'mcp',
  device_id: BIG_DEVICE.id,
  token_id: null,
  detail: null,
  status: 200,
  caller: 'client',
  ...overrides
});
const BIG_UNIQUE = [
  ...Array.from({ length: BIG_BOUNDARIES }, (_, index) =>
    bigLine({
      caller: 'console',
      tool: index % 2 === 0 ? 'device.left' : 'device.joined',
      sentence: "The console's channel boundary"
    })
  ),
  ...Array.from({ length: BIG_CONSOLE }, (_, index) =>
    bigLine({ caller: 'console', tool: `big.console.${index}`, sentence: `Console read ${index}` })
  ),
  ...Array.from({ length: BIG_CLIENT }, (_, index) =>
    bigLine({ caller: 'client', tool: `big.client.${index}`, sentence: `Client call ${index}` })
  ),
  ...Array.from({ length: BIG_UNATTRIBUTED }, (_, index) =>
    bigLine({
      caller: undefined,
      tool: `big.unknown.${index}`,
      sentence: `Unattributed ${index}`
    })
  )
];
const BIG_WINDOW_LINES = [
  ...BIG_UNIQUE,
  // Lines the file wrote twice: the same instant, tool and status, so the merge draws each once.
  ...BIG_UNIQUE.slice(BIG_BOUNDARIES, BIG_BOUNDARIES + BIG_REPEATS).map((line) => ({ ...line }))
];
if (BIG_WINDOW_LINES.length !== 1000 || BIG_DRAWABLE !== 974) {
  throw new Error(
    `the measured hub fixture is ${BIG_WINDOW_LINES.length} fetched / ${BIG_DRAWABLE} drawable`
  );
}
// The whole registry, not the selected machine's slice: one token for this machine, one for a
// machine the gate never selects, and one aggregate. The Tokens leg counts the rows against this.
// **THE AGGREGATE IS THE AUDIT'S THREE-MACHINE SHAPE.** At 390 its scope chip was 337 px and the
// name cell 0 px, so `zss-harness-20261106-fleet` broke into a 26 x 176 px sliver and the row
// stood 414 px tall. The phone card must give the name the card's width and put that scope
// sentence under it, so the fixture carries three enrolled machine names and the measured name.
const TOKENS = [
  {
    id: 'gate-token-on-gate-device',
    name: 'gate-laptop',
    kind: 'device',
    device_id: DEVICE.id,
    tools: ['files.read', 'terminal.read'],
    allow_destructive: false,
    created_at_ms: NOW - 2 * 60 * 60_000,
    // The date cells' three shapes: a used moment, an expiring token, and (below) a token with
    // neither, which must keep `never used` / `never`. The gate pins their ISO text and titles.
    last_used_ms: NOW - 90 * 60_000,
    expires_at_ms: NOW + 30 * 86400_000
  },
  {
    id: 'gate-token-elsewhere',
    name: 'attic-ops',
    kind: 'device',
    device_id: 'gate-device-attic-0123456789',
    tools: ['runs.list'],
    allow_destructive: false,
    created_at_ms: NOW - 3 * 60 * 60_000,
    last_used_ms: NOW - 3 * 86400_000,
    expires_at_ms: NOW + 90 * 86400_000
  },
  {
    id: 'gate-token-fleet',
    name: 'zss-harness-20261106-fleet',
    kind: 'fleet',
    device_id: '',
    device_ids: [DEVICE.id, QUIET_DEVICE.id, BIG_DEVICE.id],
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
// The registry's destructive flags: the fleet form has no machine to ask, so this is the count it
// must read. Two of the seven tools change the machine.
const DESTRUCTIVE_TOOLS = new Set(['terminal.create', 'terminal.write']);

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
  /** How many ptys the page opened, so the retained-start session has an id of its own. */
  created: 0,
  /** What the stream has already echoed, so a reconnecting client is not told twice. */
  streamEchoed: false,
  /** How many times the retained-start stream refused a cursor as older than its ring. */
  oldCursorRefusals: 0,
  /** How many limited reads the page made, and the first cursor one asked for. */
  limitedReads: 0,
  limitedFrom: null,
  /** The `from` the page resumed the stream at. */
  retainedFrom: null,
  /** How many `/audit?from=&to=` requests the page made, and the last one's moments and count. */
  rangeRequests: 0,
  lastRange: null,
  /** How many `/audit/export` requests the page made, and the bounds the last one asked for. */
  exportRequests: 0,
  lastExport: null,
  /** Whether the next `/audit/export` is answered as a refusal (the alert's state). */
  refuseExport: false,
  /**
   * Whether the offline machine's `/fleet/<id>/tools` is answered 503, the measured console's
   * answer for 14R. Armed for the offline leg, where the token form reads it on purpose.
   */
  offlineToolsFails: false,
  /** Whether the console's deployment staged an installer, and how often the page asked. */
  installerAvailable: true,
  installerRequests: 0
};

/**
 * The local calendar day for the lines one route serves, by the page's four buckets.
 *
 * The gate's `/audit/day` answers this, so Activity's header counts the same day the console would;
 * a channel boundary is not a call, a poll is its own bucket, and a failure is status 400+ or a
 * JSON-RPC error carried inside a 200.
 */
function dayCounts(lines) {
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
  for (const line of lines) {
    if (line.tool === 'device.left' || line.tool === 'device.joined') continue;
    const failed =
      Number(line.status) >= 400 ||
      line.ok === false ||
      String(line.detail ?? '').startsWith('jsonrpc error');
    if (line.kind === 'poll') {
      day.poll += 1;
      if (failed) day.poll_failed += 1;
    } else if (
      line.caller === 'client' ||
      (line.caller == null && line.token_id != null)
    ) {
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
  return day;
}

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
/** A reply that is not JSON: the export's JSON Lines body and its download headers. */
const ndjson = (body, headers, status = 200) => ({ status, body, raw: body, headers });
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
  if (method === 'GET' && pathname === '/fleet')
    return json([DEVICE, OFFLINE_DEVICE, QUIET_DEVICE, BIG_DEVICE]);
  // THE INSTALLER READ. One answer per fleet render; the page must not poll it, and the gate
  // counts the requests so a poll would show up here.
  if (method === 'GET' && pathname === '/installer') {
    state.installerRequests += 1;
    return json(state.installerAvailable ? INSTALLER : { available: false });
  }
  if (method === 'GET' && /^\/fleet\/[^/]+\/tools$/.test(pathname)) {
    // The measured console with 14R away: `/fleet/<id>/tools` answers 503 for an offline machine,
    // while `/tools` still serves the registry. The offline token-form leg below reads this.
    if (state.offlineToolsFails && pathname.split('/')[2] === OFFLINE_DEVICE.id) {
      return json({ error: 'device reconnecting', retry_after_ms: 1000 }, 503);
    }
    return json({
      tools: TOOL_NAMES.map((name) => ({
        name,
        description: `${name} on the gate machine`,
        inputSchema: { type: 'object', properties: {} },
        source: 'builtin',
        capability: name.split('.')[0],
        destructive: DESTRUCTIVE_TOOLS.has(name)
      }))
    });
  }
  if (method === 'GET' && /^\/fleet\/[^/]+\/connections$/.test(pathname)) return json([]);
  if (method === 'GET' && pathname === '/tokens') return json(TOKENS);
  // Newest first, as the console serves it: the console's own read is the newer row. `?device=`
  // narrows during the read, exactly as the console does; the gate's one machine owns both rows.
  // **THE RANGE AS A FILE.** The page's Export fetches this route with the bounds the Time
  // control drew; the gate answers chronological JSON Lines and records the query, so the leg
  // below proves the button asked for the range rather than saving whatever happened to be drawn.
  if (method === 'GET' && pathname === '/audit/export') {
    if (state.refuseExport) {
      state.exportRequests += 1;
      return json({ error: 'the range holds more lines than the console will scan' }, 413);
    }
    const fromMs = Number(url.searchParams.get('from'));
    const toMs = Number(url.searchParams.get('to'));
    const device = url.searchParams.get('device');
    const source =
      device === BIG_DEVICE.id ? BIG_WINDOW_LINES : [CONSOLE_LINE, AUDIT_LINE, ...HISTORIC_LINES];
    const lines = source
      .filter((line) => device === null || line.device_id === device)
      .filter((line) => line.ts_ms >= fromMs && line.ts_ms < toMs)
      .sort((left, right) => left.ts_ms - right.ts_ms);
    state.exportRequests += 1;
    state.lastExport = { from: fromMs, to: toMs, device, count: lines.length };
    const compact = (ms) => new Date(ms).toISOString().replace(/[-:]/g, '');
    const filename = `summrise-audit-${compact(fromMs)}_to_${compact(toMs)}.ndjson`;
    return ndjson(lines.map((line) => JSON.stringify(line)).join('\n') + (lines.length ? '\n' : ''), {
      'content-type': 'application/x-ndjson',
      'content-disposition': `attachment; filename="${filename}"`,
      'x-audit-range': `from=${fromMs}; to=${toMs}`
    });
  }
  if (method === 'GET' && pathname === '/audit') {
    const device = url.searchParams.get('device');
    // **THE END LEG'S WINDOW IS A MACHINE'S OWN 1000 LINES.** The measured shape needs a full answer
    // under a sentence that claims only the rows the controls reach; the other machines keep the
    // small day the earlier legs assert on.
    const source =
      device === BIG_DEVICE.id ? BIG_WINDOW_LINES : [CONSOLE_LINE, AUDIT_LINE, ...HISTORIC_LINES];
    const scoped = source.filter((line) => device === null || line.device_id === device);
    // **A RANGE IS THE TABLE'S REQUEST.** The Time control sends `from`/`to`; the gate serves the
    // same `[from, to)` rule the console does and records the request the page made, so the leg
    // below proves a preset fired it rather than filtering a window in the browser.
    const from = url.searchParams.get('from');
    const to = url.searchParams.get('to');
    if (from !== null || to !== null) {
      const fromMs = Number(from);
      const toMs = Number(to);
      const lines = scoped
        .filter((line) => line.ts_ms >= fromMs && line.ts_ms < toMs)
        .sort((left, right) => right.ts_ms - left.ts_ms);
      state.rangeRequests += 1;
      state.lastRange = { from: fromMs, to: toMs, count: lines.length };
      return json({ lines, lines_read: scoped.length, at_limit: false });
    }
    // Newest first, as the console serves it.
    return json(scoped.sort((left, right) => right.ts_ms - left.ts_ms));
  }
  // **THE FILE THE WINDOW IS READ FROM.** The page states this beside its cap sentence; the gate
  // serves the measured shape (an 8.4 MiB current file and one older generation) so the line it
  // asserts is the sentence the console would produce, not a fixture with no rotation.
  if (method === 'GET' && pathname === '/audit/source') {
    return json({
      file: 'audit.jsonl',
      bytes: 8_808_038,
      oldest_ts_ms: DAY_START.getTime() + 1_000,
      newest_ts_ms: NOW,
      max_bytes: 8_388_608,
      rotations: 1,
      rotated: {
        file: 'audit.1.jsonl',
        oldest_ts_ms: DAY_START.getTime() - 6 * 60 * 60_000
      }
    });
  }
  // The day, not the window: Activity's header counts this; the window sentence keeps its own rows.
  if (method === 'GET' && pathname === '/audit/day') {
    const device = url.searchParams.get('device');
    const source = device === BIG_DEVICE.id ? BIG_UNIQUE : TODAY_LINES;
    return json(
      dayCounts(source.filter((line) => device === null || line.device_id === device))
    );
  }
  if (method === 'GET' && pathname === '/denials') return json([]);
  // The static registry, with the same destructive flags the live catalogue carries. The fleet
  // token form reads this one: no machine is selected to ask.
  if (method === 'GET' && pathname === '/tools')
    return json(TOOL_NAMES.map((name) => ({ name, destructive: DESTRUCTIVE_TOOLS.has(name) })));

  if (method === 'POST' && pathname === '/mcp') {
    const call = bodyOf(request)?.request?.params;
    const name = call?.name;
    const args = call?.arguments ?? {};
    if (name === 'terminal.list') {
      return json({ result: { content: [{ type: 'text', text: '[]' }] } });
    }
    if (name === 'terminal.create') {
      state.opened = true;
      state.created += 1;
      return json({
        result: { content: [{ type: 'text', text: JSON.stringify({ id: `gate-pty-${state.created}` }) }] }
      });
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
      // **THE PAGED CATCH-UP.** The second pty's ring no longer holds the page's cursor, so its
      // recovery must page the retained output through `terminal.read` with `limit`, draw it, and
      // subscribe again only at the cursor the page reached. One short page is the ring's end.
      if (args.id === RETAINED_SESSION && typeof args.limit === 'number') {
        state.limitedReads += 1;
        const cursor = Number(args.cursor);
        if (state.limitedFrom === null) state.limitedFrom = cursor;
        if (cursor === RETAINED_FROM) {
          return json({
            result: {
              content: [
                {
                  type: 'text',
                  text: JSON.stringify({ data: RETAINED_TEXT, cursor: RETAINED_END })
                }
              ]
            }
          });
        }
        return json({
          result: { content: [{ type: 'text', text: JSON.stringify({ data: '', cursor }) }] }
        });
      }
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
      return json({
        result: { content: [{ type: 'text', text: JSON.stringify([LONG_RUN, RUN]) }] }
      });
    }
    if (name === 'runs.get') {
      // The record the id names. The audit's long row is a record of its own, so the gate can open
      // it and measure what the record paints, not the short row the Activity link opened.
      const record = args.id === LONG_RUN.id ? LONG_RUN : RUN;
      return json({ result: { content: [{ type: 'text', text: JSON.stringify(record) }] } });
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
    const session = url.pathname.split('/')[3];
    const from = Number(url.searchParams.get('from') ?? '0');
    response.writeHead(200, {
      'content-type': 'text/event-stream; charset=utf-8',
      'cache-control': 'no-store',
      'x-accel-buffering': 'no'
    });
    response.write(': stream open\n\n');
    // **A CURSOR TOO OLD IS REFUSED HERE, AND ONLY HERE.** The page must page the retained bytes
    // through the MCP read above and open a NEW connection at the cursor that page reached; a page
    // that only moves its map never draws the retained bytes below.
    if (session === RETAINED_SESSION) {
      if (from < RETAINED_FROM) {
        state.oldCursorRefusals += 1;
        response.write(
          `event: fault\ndata: ${JSON.stringify({
            sentence: `terminal.read: the cursor ${from} is older than the retained output, which starts at ${RETAINED_FROM}`,
            status: 200
          })}\n\n`
        );
      } else {
        // The retained bytes arrived on the limited read. This connection is the page's resume at
        // the cursor that page reached, so it carries no output of its own; a later reconnect after
        // real output would move the fact, but the gate sends none.
        if (state.retainedFrom === null) state.retainedFrom = from;
      }
    }
    const timer = setInterval(() => {
      if (session !== RETAINED_SESSION && state.echoed && !state.streamEchoed) {
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
  if (message.type() !== 'error') return;
  // The gate deliberately answers one export with a 413; the browser logs the refused fetch as a
  // resource error, and that refusal is the state under test, not a page fault.
  if (state.refuseExport && message.text().includes('413')) return;
  // The same for the offline machine's tools read: its 503 is the measured answer the offline
  // token-form leg exists to show, not a fault of the page.
  if (state.offlineToolsFails && message.text().includes('503')) return;
  errors.push(message.text());
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
    headers: {
      ...headers,
      'content-type': 'application/json; charset=utf-8',
      ...(reply.headers ?? {})
    },
    body: reply.raw ?? JSON.stringify(reply.body)
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
  // **THE DOOR IS WHERE THE PERSON ARRIVES.** The fleet's zero state is where Add-a-machine starts,
  // and the enrollment command it prints assumes the runtime is installed. The block must name the
  // file the console holds — its filename, size and short sha, all from `/installer` — and its
  // link must be the same-origin download, not a fetch the page turns into a blob.
  await page.getByRole('heading', { name: 'Install the device runtime' }).waitFor({ timeout: 10_000 });
  const installerLink = page.getByRole('link', { name: 'Download Windows installer' });
  if ((await installerLink.getAttribute('href')) !== '/installer/download') {
    throw new Error(`the installer link points at ${await installerLink.getAttribute('href')}`);
  }
  if ((await installerLink.getAttribute('download')) === null) {
    throw new Error('the installer link is not a download');
  }
  const installerLine = (await page.locator('.installer .hint').textContent())?.trim();
  const expectedInstallerLine =
    'SummriseSetup-0.3.280.exe · 6.3 MB · sha256 26d39c12…';
  if (installerLine !== expectedInstallerLine) {
    throw new Error(
      `the installer line read ${JSON.stringify(installerLine)}, expected ${JSON.stringify(expectedInstallerLine)}`
    );
  }
  if (state.installerRequests !== 1) {
    throw new Error(`the fleet read /installer ${state.installerRequests} times, not once`);
  }
  step(
    `the fleet's zero state offers ${INSTALLER.filename} (${installerLine}) from /installer/download`
  );
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
  // **THE START IS THE ROW'S NEW FACT, EVEN WHEN ITS VERSION REPEATS THE CHIP.** The journal says
  // the process started an hour ago; the verdict is "started 0.3.42", the same version the chip
  // carries. The row must draw the start with its age (and the refusal that belongs to it), not
  // drop it as a duplicate — the moment is what the chip cannot state. The exact instant is in the
  // line's title. The chip remains the build's one copy; the device's own sentence repeats the
  // version because that is the device's sentence.
  const onlineRow = page.getByRole('button', { name: /^Open DESKTOP-9VH92BJ/ });
  const onlineRowText = (await onlineRow.textContent()) ?? '';
  const versionCopies = (onlineRowText.match(/0\.3\.42/g) ?? []).length;
  if (versionCopies !== 2) {
    throw new Error(`the online row carried ${versionCopies} copies of its version: "${onlineRowText}"`);
  }
  // **THE INSTALLER IS THE FLEET'S OTHER VERSION.** Every gate machine runs 0.3.42 and the staged
  // file is 0.3.280, so each row must say it is behind that file beside its own build — the
  // measured fleet had 84T3LIN on 0.3.280 beside the 0.3.284 installer and said nothing.
  const build = (await onlineRow.locator('.fleet-build').textContent())?.trim();
  if (build !== 'v0.3.42 · behind 0.3.280') {
    throw new Error(`the online row's build read "${build}", not its own version and the staged one`);
  }
  step(`the online row's build states "${build}"`);
  const verdict = (await onlineRow.locator('.fleet-verdict').textContent())?.trim();
  if (verdict !== 'started 0.3.42 · 1h ago (update to 0.3.43 refused)') {
    throw new Error(`the online row's start line read "${verdict}"`);
  }
  const startInstant = new Date(NOW - 60 * 60_000).toISOString();
  const startTitle = (await onlineRow.locator('.fleet-verdict').getAttribute('title')) ?? '';
  if (!startTitle.includes(`(${startInstant})`)) {
    throw new Error(`the row's start title "${startTitle}" does not carry the exact instant`);
  }
  step(`the online row states "${verdict}" with the start's exact instant in its title`);
  // **THE ROW NAMES THE CHANNEL THE RUNNING PAIR CAME FROM.** The build cell says what runs and what
  // is staged; it cannot say which release channel the running pair belongs to. The newest restart
  // entry names it (the launcher's current generation is the fallback), so "channel not reported"
  // is impossible while a restart entry names one.
  const rowChannel = (await onlineRow.locator('.fleet-channel').textContent())?.trim();
  if (rowChannel !== 'channel stable') {
    throw new Error(`the online row's channel line read "${rowChannel}", not its restart's channel`);
  }
  step('the online row names the channel its newest restart ran: "channel stable"');
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
  const dayCalls = `${TODAY_CLIENT} ${TODAY_CLIENT === 1 ? 'call' : 'calls'}`;
  const expectedWork = new Map([
    ['DESKTOP-9VH92BJ', dayCalls],
    ['DESKTOP-14RJCR8', '11 calls'],
    ['DESKTOP-CPH4KB2', '84 calls · 2 failed'],
    // The measured window's machine: the same 283 client calls its hub draws behind the fold.
    ['DESKTOP-7QK3MN2', '283 calls']
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
    `the four fleet rows state their days (${[...expectedWork.values()].join(' | ')}); ` +
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

  // **A CURSOR TOO OLD PAGES IN A REAL BROWSER.** The gate's stream for the second pty refuses its
  // first connection, and its MCP stub answers the page's limited reads with the retained output.
  // The page must draw that page, then open the stream at the cursor the page reached — not at the
  // refused cursor, and not at the retained start with a whole-ring subscribe frame.
  await page.getByRole('button', { name: '+ New session' }).click();
  await page
    .getByRole('dialog', { name: 'New terminal session' })
    .getByRole('button', { name: 'Open' })
    .click();
  await page.locator('.term-host:not([hidden]) .xterm').waitFor({ state: 'visible', timeout: 10_000 });
  await page.waitForFunction(
    (text) => {
      const rows = document.querySelector('.term-host:not([hidden]) .xterm-rows');
      return rows?.textContent?.includes(text) ?? false;
    },
    RETAINED_TEXT.trim(),
    { timeout: 10_000 }
  );
  if (state.oldCursorRefusals < 1) {
    throw new Error('the stream never refused a cursor as older than the retained output');
  }
  if (state.limitedReads < 1) {
    throw new Error('the page never paged the retained output with terminal.read limit');
  }
  if (state.limitedFrom !== RETAINED_FROM) {
    throw new Error(`the page paged from ${state.limitedFrom}, not the retained start ${RETAINED_FROM}`);
  }
  if (state.limitedReads !== 1) {
    throw new Error(
      `the page made ${state.limitedReads} limited reads; one short page is the ring's end`
    );
  }
  if (state.retainedFrom !== RETAINED_END) {
    throw new Error(
      `the page resumed the stream at ${state.retainedFrom}, not the reached cursor ${RETAINED_END}`
    );
  }
  if (await page.locator('.terminal-fault').count()) {
    throw new Error('the too-old refusal left its sentence on the tab after the retained bytes arrived');
  }
  step('a cursor too old paged the ring, drew it and resumed the stream at the reached cursor');
  await page.screenshot({ path: path.join(OUT, '04-terminal-retained.png') });

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
  // THE SOURCE LINE, ONCE, BESIDE THE WINDOW SENTENCE. It says what is read, not what is drawn:
  // the file's size must be the server's, and a rotation the page was told about must be stated.
  const sourceLine = page.locator('.audit-source');
  await sourceLine.waitFor({ timeout: 10_000 });
  const sourceText = (await sourceLine.textContent())?.trim() ?? '';
  if (!sourceText.startsWith('Reading audit.jsonl — 8.4 MiB, lines since ')) {
    throw new Error(`the source line did not state the file and its size: "${sourceText}"`);
  }
  if (!sourceText.includes('rotated once, older lines start ')) {
    throw new Error(`the source line did not state the rotation: "${sourceText}"`);
  }
  step(`the window states the file it reads: ${sourceText}`);
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

  // **THE DESKTOP HISTORY IS THE TABLE IT WAS.** The phone card below must be a media query, not a
  // second markup path: at 1440 the header is drawn and the first command column is the wide one the
  // audit measured (679 px), with a 57 px row. This is measured before the phone resize so a rule
  // that leaked upward cannot pass.
  await page.setViewportSize({ width: 1440, height: 960 });
  await page.waitForTimeout(300);
  const runs1440 = await page.evaluate(() => {
    const head = document.querySelector('.runs-history thead');
    const row = document.querySelector('.runs-row');
    const button = row?.querySelector('button.runs-open');
    const headBox = head?.getBoundingClientRect();
    const buttonBox = button?.getBoundingClientRect();
    const rowBox = row?.getBoundingClientRect();
    return {
      viewport: document.documentElement.clientWidth,
      rows: document.querySelectorAll('.runs-row').length,
      headerDisplay: head ? getComputedStyle(head).display : null,
      headerWidth: Math.round(headBox?.width ?? 0),
      headerHeight: Math.round(headBox?.height ?? 0),
      commandWidth: Math.round(buttonBox?.width ?? 0),
      commandHeight: Math.round(buttonBox?.height ?? 0),
      rowHeight: Math.round(rowBox?.height ?? 0),
      documentScrollWidth: document.documentElement.scrollWidth
    };
  });
  console.log(`browser-gate: runs at 1440: ${JSON.stringify(runs1440)}`);
  if (runs1440.rows !== 2) {
    throw new Error(`the 1440 history drew ${runs1440.rows} rows, not the fixture's 2`);
  }
  if (runs1440.headerDisplay === 'none' || runs1440.headerWidth === 0) {
    throw new Error('the 1440 history lost its table header');
  }
  if (runs1440.commandWidth < 300) {
    throw new Error(`the 1440 command column is ${runs1440.commandWidth} px, not the wide table cell`);
  }
  if (runs1440.rowHeight > 90) {
    throw new Error(`the 1440 history row is ${runs1440.rowHeight} px tall`);
  }
  if (runs1440.documentScrollWidth > runs1440.viewport) {
    throw new Error(
      `the 1440 Runs page scrolls sideways: ${runs1440.documentScrollWidth} > ${runs1440.viewport}`
    );
  }
  step(
    `at 1440 the Runs history keeps its table: header ${runs1440.headerWidth} px, command ` +
      `${runs1440.commandWidth} px, row ${runs1440.rowHeight} px`
  );
  await page.screenshot({ path: path.join(OUT, '04-activity-run.png') });

  // **AT 390 THE HISTORY IS A CARD LIST, NOT A 320 PX TABLE.** The audited phone drew the command in
  // an 80 px column and broke `powershell.exe -NoProfile …` every few characters, with a 52 px
  // header cell and a 5,660 px column for 20 runs. The card must put the command full width and
  // wrapping, then exit, duration, started and the output sizes as labelled lines, with the header
  // gone and no sideways scroll — while the row's one control stays its command button.
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(400);
  const runs390 = await page.evaluate(() => {
    const scroller = document.querySelector('.runs-history');
    const head = document.querySelector('.runs-history thead');
    const headStyle = head ? getComputedStyle(head) : null;
    const headBox = head?.getBoundingClientRect();
    const rows = [...document.querySelectorAll('.runs-row')].map((row) => {
      const button = row.querySelector('button.runs-open');
      const box = button?.getBoundingClientRect();
      const rowBox = row.getBoundingClientRect();
      return {
        command: button?.textContent?.trim() ?? '',
        commandWidth: Math.round(box?.width ?? 0),
        commandHeight: Math.round(box?.height ?? 0),
        commandRight: Math.round(box?.right ?? 0),
        commandBottom: Math.round(box?.bottom ?? 0),
        commandScrollWidth: button?.scrollWidth ?? 0,
        commandClientWidth: button?.clientWidth ?? 0,
        rowWidth: Math.round(rowBox.width),
        labels: [...row.querySelectorAll('.runs-fact-label')].map((label) => ({
          text: label.textContent?.trim() ?? '',
          visible:
            getComputedStyle(label).display !== 'none' && label.getClientRects().length > 0
        })),
        factTops: [...row.querySelectorAll('td.runs-fact')].map((cell) =>
          Math.round(cell.getBoundingClientRect().top)
        )
      };
    });
    return {
      viewport: document.documentElement.clientWidth,
      documentScrollWidth: document.documentElement.scrollWidth,
      scrollerScrollWidth: scroller?.scrollWidth ?? 0,
      scrollerClientWidth: scroller?.clientWidth ?? 0,
      headerDisplay: headStyle?.display ?? null,
      headerWidth: Math.round(headBox?.width ?? 0),
      headerHeight: Math.round(headBox?.height ?? 0),
      focusables: document.querySelectorAll(
        '.runs-history button, .runs-history a[href], .runs-history input, .runs-history select, ' +
          '.runs-history textarea, .runs-history [tabindex]:not([tabindex="-1"])'
      ).length,
      rows
    };
  });
  console.log(`browser-gate: runs at 390: ${JSON.stringify(runs390)}`);
  if (runs390.rows.length !== 2) {
    throw new Error(`the 390 history drew ${runs390.rows.length} cards, not the fixture's 2`);
  }
  if (runs390.headerDisplay !== 'none' || runs390.headerWidth > 1 || runs390.headerHeight > 1) {
    throw new Error(
      `the 390 history still draws its header: display ${runs390.headerDisplay}, ` +
        `${runs390.headerWidth}×${runs390.headerHeight}`
    );
  }
  for (const card of runs390.rows) {
    if (card.commandWidth <= 200) {
      throw new Error(
        `the 390 command is only ${card.commandWidth} px wide: ${JSON.stringify(card.command)}`
      );
    }
    if (card.commandRight > runs390.viewport) {
      throw new Error(`the 390 command is outside the viewport at ${card.commandRight} px`);
    }
    if (card.commandWidth < card.rowWidth - 40) {
      throw new Error(
        `the 390 command is ${card.commandWidth} px in a ${card.rowWidth} px card, not full width`
      );
    }
    if (card.commandScrollWidth > card.commandClientWidth + 1) {
      throw new Error(
        `the 390 command is ${card.commandScrollWidth} px in a ${card.commandClientWidth} px box; ` +
          `it is cut instead of wrapped`
      );
    }
    if (card.labels.map((label) => label.text).join('|') !== 'Exit|Duration|Started|stdout / stderr') {
      throw new Error(
        `the 390 card's fact labels are ${JSON.stringify(card.labels.map((label) => label.text))}`
      );
    }
    if (card.labels.some((label) => !label.visible)) {
      throw new Error(`a 390 fact label is not visible: ${JSON.stringify(card.labels)}`);
    }
    if (card.factTops.length !== 4 || card.factTops.some((top) => top < card.commandBottom)) {
      throw new Error(
        `the 390 card does not put its facts below the command: ${JSON.stringify({
          commandBottom: card.commandBottom,
          factTops: card.factTops
        })}`
      );
    }
  }
  // The long fixture command is the audit's shape: it must wrap onto more than one line, not be
  // clipped or broken by the card.
  const longCard = runs390.rows[0];
  if (longCard.commandHeight < 30) {
    throw new Error(`the 390 long command is ${longCard.commandHeight} px tall; it did not wrap`);
  }
  if (runs390.documentScrollWidth > runs390.viewport) {
    throw new Error(
      `the 390 Runs page scrolls sideways: ${runs390.documentScrollWidth} > ${runs390.viewport}`
    );
  }
  if (runs390.scrollerScrollWidth > runs390.scrollerClientWidth) {
    throw new Error(
      `the 390 history scrolls sideways: ${runs390.scrollerScrollWidth} > ${runs390.scrollerClientWidth}`
    );
  }
  // ONE TAB STOP PER ROW, THE COMMAND: the labels are text, not controls, and the card adds no
  // second way to open a record.
  if (runs390.focusables !== runs390.rows.length) {
    throw new Error(
      `the 390 history has ${runs390.focusables} focusable elements for ${runs390.rows.length} rows`
    );
  }
  // The screenshot is the evidence; scroll the cards into the frame (the measured record above is
  // open and taller than the phone).
  await page.locator('.runs-history').scrollIntoViewIfNeeded();
  await page.waitForTimeout(200);
  await page.screenshot({ path: path.join(OUT, '04b-runs-390.png') });
  step(
    `at 390 the Runs history is ${runs390.rows.length} cards: commands ` +
      `${runs390.rows.map((card) => `${card.commandWidth}×${card.commandHeight}`).join(', ')} px ` +
      `with ${runs390.rows[0].labels.length} labelled facts each, header hidden, no sideways scroll`
  );

  // **THE RECORD PAINTS A SHORT COMMAND, NOT 4,590 CHARACTERS OF BASE64.** Audit 36 F3 measured the
  // full `-EncodedCommand` line at 1,190 px of a 1,551 px card at 1440 and 1,866 px of 2,299 px at
  // 390, with Exit/Duration/Started beginning at y 1,393/2,114 — the facts were below the first
  // screen at both widths. The record must put the facts first, paint the short form with the count
  // it hides, and keep the exact full line behind one `Show full command` toggle.
  const recordMetrics = (startedAt) =>
    page.evaluate((startedAt) => {
      const card = document.querySelector('.runs-detail');
      const command = document.querySelector('#runs-detail-command-text');
      const facts = card?.querySelector('dl.facts');
      const startedTime = facts?.querySelector('time[datetime]');
      const fact = (label) => {
        const dt = [...(facts?.querySelectorAll('dt') ?? [])].find(
          (node) => node.textContent === label
        );
        const dd = dt?.nextElementSibling;
        const box = dd?.getBoundingClientRect();
        return box
          ? {
              text: (dd?.textContent ?? '').trim(),
              top: Math.round(box.top),
              bottom: Math.round(box.bottom)
            }
          : null;
      };
      const cardBox = card?.getBoundingClientRect();
      const commandBox = command?.getBoundingClientRect();
      const toggle = document.querySelector('.runs-detail-command-more button');
      return {
        // The Started fact is an instant, not an age: the `<time>`'s text is the local day and
        // clock, `dateTime` is the exact UTC instant and `title` carries both clocks.
        started: {
          text: (startedTime?.textContent ?? '').trim(),
          dateTime: startedTime?.getAttribute('datetime') ?? null,
          title: startedTime?.getAttribute('title') ?? null,
          exact: new Date(startedAt).toISOString()
        },
        viewport: { width: window.innerWidth, height: window.innerHeight },
        card: cardBox
          ? { top: Math.round(cardBox.top), height: Math.round(cardBox.height) }
          : null,
        command: commandBox
          ? {
              top: Math.round(commandBox.top),
              height: Math.round(commandBox.height),
              text: command?.textContent ?? ''
            }
          : null,
        facts: { exit: fact('Exit'), duration: fact('Duration'), started: fact('Started') },
        toggle: toggle?.textContent ?? null,
        expanded: toggle?.getAttribute('aria-expanded') ?? null
      };
    }, startedAt);
  const factsAboveCommand = () =>
    page.evaluate(() => {
      const facts = document.querySelector('.runs-detail dl.facts');
      const command = document.querySelector('#runs-detail-command-text');
      return Boolean(
        facts &&
          command &&
          facts.compareDocumentPosition(command) & Node.DOCUMENT_POSITION_FOLLOWING
      );
    });
  const checkRecord = (where, record) => {
    if (!record.card || !record.command) {
      throw new Error(`${where}: the record or its command did not draw`);
    }
    if (record.command.text !== LONG_SHORT) {
      throw new Error(
        `${where}: the record paints ${JSON.stringify(record.command.text.slice(0, 60))}… ` +
          `(${record.command.text.length} chars), not the short command`
      );
    }
    if (record.toggle !== 'Show full command' || record.expanded !== 'false') {
      throw new Error(
        `${where}: the command toggle is ${JSON.stringify({ toggle: record.toggle, expanded: record.expanded })}`
      );
    }
    if (record.command.height > 120) {
      throw new Error(`${where}: the short command is ${record.command.height} px tall`);
    }
    for (const [name, fact] of Object.entries(record.facts)) {
      if (!fact) throw new Error(`${where}: the ${name} fact did not draw`);
      if (fact.top < 0 || fact.bottom > record.viewport.height) {
        throw new Error(
          `${where}: ${name} is at y ${fact.top}..${fact.bottom} in a ${record.viewport.height} px viewport`
        );
      }
      if (fact.bottom > record.command.top) {
        throw new Error(`${where}: ${name} is y ${fact.bottom}, below the command at ${record.command.top}`);
      }
    }
    if (!record.facts.exit.text.startsWith('exit ')) {
      throw new Error(`${where}: the exit fact is ${JSON.stringify(record.facts.exit.text)}`);
    }
    // **THE START IS AN INSTANT.** Audit 36 F6 measured `21m 04s ago` become `21m 11s ago` six
    // seconds later, with no title or datetime. The `<time>` now carries the exact UTC in
    // `dateTime`, both clocks in its title, the local day and clock as its text, and the age beside
    // it — at both widths.
    if (!record.started.dateTime || !record.started.title || !record.started.text) {
      throw new Error(`${where}: the Started fact draws no <time> with dateTime/title`);
    }
    if (record.started.dateTime !== record.started.exact) {
      throw new Error(
        `${where}: Started dateTime is ${JSON.stringify(record.started.dateTime)}, not ${record.started.exact}`
      );
    }
    if (!record.started.title.includes(`(${record.started.exact})`)) {
      throw new Error(
        `${where}: Started title is ${JSON.stringify(record.started.title)}, not the exact instant`
      );
    }
    // The visible text is `YYYY-MM-DD HH:MM` in whatever timezone the browser runs, not a locale
    // form; the age is the fact beside it, not instead of it.
    if (!/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/.test(record.started.text)) {
      throw new Error(`${where}: Started text is ${JSON.stringify(record.started.text)}`);
    }
    if (!record.facts.started.text.startsWith(`${record.started.text} · `)) {
      throw new Error(
        `${where}: Started reads ${JSON.stringify(record.facts.started.text)}, not the instant and the age beside it`
      );
    }
    if (!record.facts.started.text.endsWith(' ago')) {
      throw new Error(`${where}: Started carries no age: ${JSON.stringify(record.facts.started.text)}`);
    }
  };
  const expandRecord = async (where) => {
    await page.getByRole('button', { name: 'Show full command' }).click();
    await page.waitForTimeout(150);
    const expanded = await recordMetrics(LONG_RUN.started_at_ms);
    if (expanded.command?.text !== LONG_LINE) {
      throw new Error(
        `${where}: Show full command painted ${expanded.command?.text.length} chars, not ${LONG_LINE.length}`
      );
    }
    if (expanded.toggle !== 'Hide full command' || expanded.expanded !== 'true') {
      throw new Error(
        `${where}: the expanded toggle is ${JSON.stringify({
          toggle: expanded.toggle,
          expanded: expanded.expanded
        })}`
      );
    }
    if (!(await factsAboveCommand())) {
      throw new Error(`${where}: the full command is not after the facts in the DOM`);
    }
    await page.getByRole('button', { name: 'Hide full command' }).click();
    await page.waitForTimeout(100);
  };

  const longOpener = page
    .locator('.runs-row')
    .filter({ hasText: 'powershell.exe' })
    .getByRole('button');
  await longOpener.click();
  await page.getByRole('button', { name: 'Show full command' }).waitFor({ timeout: 10_000 });
  await page.evaluate(() => document.querySelector('.runs-detail')?.scrollIntoView({ block: 'start' }));
  await page.waitForTimeout(300);
  const record390 = await recordMetrics(LONG_RUN.started_at_ms);
  checkRecord('390', record390);
  await page.screenshot({ path: path.join(OUT, '04c-runs-record-390.png') });

  // **THE INSTANT DOES NOT MOVE.** The audit's record read `21m 04s ago` and `21m 11s ago` six
  // seconds apart. Refresh re-reads the history and re-renders the record (so the age is read
  // again); the `<time>`'s text, dateTime and title must be exactly what they were.
  await page.getByRole('button', { name: 'Refresh history' }).click();
  await page.waitForTimeout(1_200);
  const record390Again = await recordMetrics(LONG_RUN.started_at_ms);
  if (JSON.stringify(record390Again.started) !== JSON.stringify(record390.started)) {
    throw new Error(
      `the Started instant moved on refresh: ${JSON.stringify(record390.started)} -> ` +
        JSON.stringify(record390Again.started)
    );
  }
  step(
    `the record's Started is the instant ${record390.started.text} ` +
      `(${record390.started.dateTime} in dateTime, ${JSON.stringify(record390.started.title)} in ` +
      `its title) and reads ${JSON.stringify(record390.facts.started.text)} before and after the ` +
      `1.2 s refresh`
  );
  await expandRecord('390');

  await page.setViewportSize({ width: 1440, height: 960 });
  await page.waitForTimeout(400);
  await page.evaluate(() => document.querySelector('.runs-detail')?.scrollIntoView({ block: 'start' }));
  await page.waitForTimeout(250);
  const record1440 = await recordMetrics(LONG_RUN.started_at_ms);
  checkRecord('1440', record1440);
  await page.screenshot({ path: path.join(OUT, '04d-runs-record-1440.png') });
  await expandRecord('1440');
  step(
    `the long record paints a ${record390.command.height}/${record1440.command.height} px short ` +
      `command with Exit/Duration/Started above it at y ` +
      `${record1440.facts.exit.top}/${record1440.facts.duration.top}/${record1440.facts.started.top} ` +
      `(1440) and ${record390.facts.exit.top}/${record390.facts.duration.top}/${record390.facts.started.top} ` +
      `(390), all on the first screen; the full 4,590 characters are behind Show full command`
  );
  console.log(
    `browser-gate: long record at 1440: ${JSON.stringify(record1440)}; at 390: ${JSON.stringify(record390)}`
  );

  await page.setViewportSize({ width: 1280, height: 900 });
  await page.waitForTimeout(300);

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
  // **THE DEVICE'S OWN UPDATE STORY IS THE LINE THE TAB WAS MISSING.** It is not a second build
  // chip: it names the version its state document records, the channel the fleet reports, the last
  // check and the updater's last outcome. All four are the gate fixture's facts, not the header's
  // chip repeated.
  const updateLine = (await page.locator('.system-update').innerText()).trim();
  for (const fact of ['running v0.3.42', 'channel stable', 'last check', 'last outcome refused to 0.3.43']) {
    if (!updateLine.includes(fact)) {
      throw new Error(`the Machine update line read "${updateLine}", missing ${JSON.stringify(fact)}`);
    }
  }
  step(`the Machine tab's update line states "${updateLine}"`);
  // **THE START IS ON THE MACHINE TAB, WITHOUT OPENING THE DEVICE'S OWN PANEL.** The same journal
  // entry the fleet row reads dates this line, which repeats the header's version because that is
  // what the device said it started; the exact instant is in the title.
  const machineStart = (await page.locator('.started-line').innerText()).trim();
  if (machineStart !== 'started 0.3.42 · 1h ago (update to 0.3.43 refused)') {
    throw new Error(`the Machine tab's start line read "${machineStart}"`);
  }
  const machineStartTitle = (await page.locator('.started-line').getAttribute('title')) ?? '';
  if (!machineStartTitle.includes(`(${new Date(NOW - 60 * 60_000).toISOString()})`)) {
    throw new Error(`the Machine start title "${machineStartTitle}" does not carry the exact instant`);
  }
  step(`the Machine tab states "${machineStart}" with its exact instant in the title`);
  // **THE CONSOLE LINK'S ATTEMPT AND THE LOCAL CHECK ARE NAMED.** The device's own retry note
  // reached the console, so the Machine tab draws its count and the device's reason; and because the
  // console has no update-check tool, the page names the panel's own control instead of offering one.
  const machineChannel = (await page.locator('.system-channel').innerText()).trim();
  const attemptSentence =
    "The device's last console attempt: retried after 1 failed attempt — " +
    'the console accepted the channel and then dropped it.';
  if (machineChannel !== attemptSentence) {
    throw new Error(`the Machine tab's console-link line read "${machineChannel}"`);
  }
  const machineCheck = (await page.locator('.system-check').innerText()).trim();
  if (
    !machineCheck.includes('no update-check tool') ||
    !machineCheck.includes('"Check for updates now"')
  ) {
    throw new Error(`the Machine tab's check line read "${machineCheck}"`);
  }
  step('the Machine tab names the console link\'s last attempt and the panel\'s local check');
  step('the Machine tab states reachability without a second build line');

  // **THE SCREEN PAGE NAMES THE HELPER'S SESSION BEFORE THE PRESS (audit 37 F3).** The fleet's
  // helper fact is session 2 and session 2 is disconnected; the measured live page offered the
  // button and answered the device's `os error 5`. The control now carries the fact and the way
  // out, disabled: a press the page already knows the answer to is not an invitation.
  await page
    .locator('nav.device-tabs')
    .getByRole('button', { name: 'Screen', exact: true })
    .click();
  await page
    .getByRole('heading', { name: 'Screen', exact: true, level: 1 })
    .waitFor({ timeout: 10_000 });
  const helperSessionLine = (await page.locator('#screen-helper-session').innerText()).trim();
  for (const fact of ['session 2, which is disconnected', 'sign in at the machine']) {
    if (!helperSessionLine.includes(fact)) {
      throw new Error(
        `the Screen helper line read "${helperSessionLine}", missing ${JSON.stringify(fact)}`
      );
    }
  }
  const captureButton = page.getByRole('button', { name: 'Capture screen', exact: true });
  if (!(await captureButton.isDisabled())) {
    throw new Error('Capture screen is enabled while the helper\'s session is disconnected');
  }
  if (!((await captureButton.getAttribute('title')) ?? '').includes('session 2')) {
    throw new Error('the disabled Capture screen does not name the helper\'s session');
  }
  step(`the Screen page states "${helperSessionLine}" and disables Capture screen`);
  await page.screenshot({ path: path.join(OUT, '05b-screen-disconnected.png') });

  // **THE MACHINE KEEPS ITS CONNECT DOOR.** The button the audit measured at 1440 (59×28 at the
  // head's right edge) is still the desktop door, named for the sheet it opens. The phone leg below
  // measures the same button at 390, where the old rule removed it.
  await page.setViewportSize({ width: 1440, height: 960 });
  await page.waitForTimeout(300);
  const deviceConnect1440 = await page.evaluate(() => {
    const host = document.querySelector('.device-connect');
    const button = host?.querySelector('button');
    const box = button?.getBoundingClientRect();
    return {
      viewport: document.documentElement.clientWidth,
      scrollWidth: document.documentElement.scrollWidth,
      display: host ? getComputedStyle(host).display : null,
      visible: button != null && button.offsetParent !== null,
      width: Math.round(box?.width ?? 0),
      height: Math.round(box?.height ?? 0),
      right: Math.round(box?.right ?? 0)
    };
  });
  console.log(`browser-gate: device connect at 1440: ${JSON.stringify(deviceConnect1440)}`);
  if (!deviceConnect1440.visible || deviceConnect1440.display === 'none') {
    throw new Error('the Device head has no visible Connect button at 1440');
  }
  if (deviceConnect1440.width < 50 || deviceConnect1440.height < 28) {
    throw new Error(
      `the 1440 Connect button is ${deviceConnect1440.width}×${deviceConnect1440.height}, ` +
        'not the measured 59×28 control'
    );
  }
  if (
    deviceConnect1440.right > deviceConnect1440.viewport ||
    deviceConnect1440.scrollWidth > deviceConnect1440.viewport
  ) {
    throw new Error('the 1440 Device head does not fit its viewport');
  }
  step(
    `at 1440 the Device head keeps its ${deviceConnect1440.width}×${deviceConnect1440.height} ` +
      `Connect button (right ${deviceConnect1440.right} px) named "Connect a client"`
  );
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.waitForTimeout(300);

  // THE CREDENTIAL LIST IS THE WHOLE REGISTRY. `GET /tokens` is the only list route; a row for
  // another machine is still a real token and hidden rows made an occupied registry read "none".
  await page.getByRole('button', { name: 'Connect a client', exact: true }).click();
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

  // **THE MACHINE FORM COUNTS THE LIVE CATALOGUE.** The same seven tools carry two destructive
  // flags on the gate machine; the sentence under them must state that count, not a blank.
  const machineCount = connect.getByText('7 tools · 2 can change the machine', { exact: true });
  await machineCount.waitFor({ timeout: 10_000 });
  step(`the machine token form counts its live catalogue: "${(await machineCount.innerText()).trim()}"`);

  // **THE TOKEN SHEET'S DATES ARE THE CONSOLE'S, NOT THE BROWSER'S.** The cells used
  // `toLocaleDateString()`, so an en-US browser read `11/4/2026` and no other locale could be sure
  // of the order. They are ISO now (the audit's own shape), the exact moment is in the cell's
  // title, and a token minted without a lifetime still reads `never`.
  const isoDay = (ms) => {
    const at = new Date(ms);
    const pad = (value) => String(value).padStart(2, '0');
    return `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}`;
  };
  const exactMoment = (ms) => {
    const at = new Date(ms);
    const pad = (value) => String(value).padStart(2, '0');
    return (
      `${isoDay(ms)} ${pad(at.getHours())}:${pad(at.getMinutes())}:${pad(at.getSeconds())}` +
      ` (${at.toISOString()})`
    );
  };
  const dated = tokenRows.first();
  const createdCell = dated.locator('.token-created time');
  const usedCell = dated.locator('.token-name .hint time');
  const expiresCell = dated.locator('.token-expires time');
  for (const [cell, atMs, label] of [
    [createdCell, TOKENS[0].created_at_ms, 'created'],
    [usedCell, TOKENS[0].last_used_ms, 'used'],
    [expiresCell, TOKENS[0].expires_at_ms, 'expires']
  ]) {
    const day = await cell.innerText();
    if (day !== isoDay(atMs)) {
      throw new Error(`the token's ${label} date reads "${day}", not ${isoDay(atMs)}`);
    }
    const title = await cell.getAttribute('title');
    if (title !== exactMoment(atMs)) {
      throw new Error(`the token's ${label} title reads "${title}", not the exact moment`);
    }
  }
  // Every absolute date the sheet states is the audit's shape; a locale's slash form fails here.
  for (const day of await page.locator('.token-table time').allInnerTexts()) {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(day)) {
      throw new Error(`the token sheet drew a non-ISO date: "${day}"`);
    }
  }
  // A token with neither a use nor a lifetime keeps the words; no element pretends to a date.
  const neverToken = tokenRows.filter({ hasText: 'zss-harness-20261106-fleet' });
  const neverExpiry = await neverToken.locator('.token-expires').innerText();
  // At the sheet's 600 px container width the cell's own `Expires` label is inline, so the value is
  // the cell's suffix; a digit anywhere would mean a date was drawn for a token that has none.
  if (!neverExpiry.endsWith('never') || /\d/.test(neverExpiry)) {
    throw new Error(`a token with no expiry read ${JSON.stringify(neverExpiry)}, not never`);
  }
  if ((await neverToken.locator('.token-expires time').count()) !== 0) {
    throw new Error('a token with no expiry drew a date');
  }
  const neverUsed = await neverToken.locator('.token-name .hint').innerText();
  if (neverUsed !== 'never used') {
    throw new Error(`a token that was never used read ${JSON.stringify(neverUsed)}, not never used`);
  }
  step(
    'the token sheet dates created/used/expires in ISO with the exact moment in the title, and never for a token with no expiry'
  );
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
  // **THE PHONE KEEPS THE MACHINE'S CONNECT DOOR (audit 25 F6).** At 390 the old rule removed the
  // button outright (`display: none`, `offsetParent === null`). The same button now sits on the
  // title's row, named "Connect a client"; its click opens the same sheet on this machine, the tab
  // bar keeps its own row below, and the page does not scroll sideways.
  const deviceConnect390 = page.getByRole('button', { name: 'Connect a client', exact: true });
  const connectBox390 = await deviceConnect390.boundingBox();
  const connectFacts390 = await page.evaluate(() => {
    const host = document.querySelector('.device-connect');
    const button = host?.querySelector('button');
    const box = button?.getBoundingClientRect();
    const h1 = document.querySelector('.device-head h1')?.getBoundingClientRect();
    const tabs = document.querySelector('.device-head .device-tabs')?.getBoundingClientRect();
    return {
      viewport: document.documentElement.clientWidth,
      scrollWidth: document.documentElement.scrollWidth,
      display: host ? getComputedStyle(host).display : null,
      offsetParent: button ? button.offsetParent !== null : false,
      width: Math.round(box?.width ?? 0),
      height: Math.round(box?.height ?? 0),
      right: Math.round(box?.right ?? 0),
      titleTop: Math.round(h1?.top ?? 0),
      buttonTop: Math.round(box?.top ?? 0),
      tabsTop: Math.round(tabs?.top ?? 0)
    };
  });
  console.log(`browser-gate: device connect at 390: ${JSON.stringify(connectFacts390)}`);
  if (!connectBox390 || !connectFacts390.offsetParent || connectFacts390.display === 'none') {
    throw new Error('the 390 Device head still hides its Connect button');
  }
  if (connectFacts390.right > connectFacts390.viewport) {
    throw new Error(`the 390 Connect button is outside the viewport at ${connectFacts390.right} px`);
  }
  if (connectFacts390.scrollWidth > connectFacts390.viewport) {
    throw new Error(
      `the Device page scrolls sideways at 390: ${connectFacts390.scrollWidth} > ` +
        `${connectFacts390.viewport}`
    );
  }
  if (connectFacts390.tabsTop <= connectFacts390.buttonTop) {
    throw new Error('the 390 Connect button does not sit on the title row above the tab bar');
  }
  await page.screenshot({ path: path.join(OUT, '06d-device-connect-390.png') });
  await deviceConnect390.click();
  const connect390 = page.getByRole('dialog', { name: 'Connect a client' });
  await connect390.waitFor({ state: 'visible', timeout: 10_000 });
  const endpoint390 = (await connect390.locator('.endpoint').innerText()).trim();
  if (endpoint390 !== `${base}/mcp/${DEVICE.id}`) {
    throw new Error(
      `the 390 Connect sheet opened ${JSON.stringify(endpoint390)}, not this machine's endpoint`
    );
  }
  await page.waitForTimeout(250);
  await page.screenshot({ path: path.join(OUT, '06e-device-connect-sheet-390.png') });
  await page.locator('.sheet-close').first().click();
  await connect390.waitFor({ state: 'hidden', timeout: 5_000 });
  if (!(await deviceConnect390.evaluate((button) => button === document.activeElement))) {
    throw new Error('the 390 Connect sheet did not return focus to its button');
  }
  step(
    `at 390 the Device head keeps a ${connectFacts390.width}×${connectFacts390.height} Connect ` +
      `button (right ${connectFacts390.right} px); its click opens this machine's Connect sheet`
  );
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
  if (fleet390.rows.length !== 4) {
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
    `at 390 the four rows carry their counts below the name ` +
      `(${heights390}); no sideways scroll`
  );
  // **THE CREDENTIAL HAS A DOOR FROM THE FLEET.** Tokens used to live only under Account → Tokens,
  // so the person adding a machine never met the thing a client needs. The fleet header's secondary
  // control must open the console's one credential sheet on its Tokens section, and the sheet must
  // state the fixed scope before a mint — measured at the phone width the operator's colleague used.
  await page.getByRole('button', { name: 'Tokens', exact: true }).click();
  const tokensDoor = page.getByRole('dialog', { name: 'Tokens' });
  await tokensDoor.waitFor({ timeout: 10_000 });
  const tokensDoorTab = tokensDoor.getByRole('tab', { name: 'Tokens', exact: true });
  if ((await tokensDoorTab.getAttribute('aria-selected')) !== 'true') {
    throw new Error("the fleet's Tokens door opened the sheet on another section");
  }
  const fixedScope = tokensDoor.getByText(
    "A token's device scope is fixed when it is minted. Rotate keeps it; to cover different " +
      'machines, mint a new token.'
  );
  await fixedScope.waitFor({ state: 'visible', timeout: 5_000 });
  // **THE FLEET FORM COUNTS THE REGISTRY, NOT A FALSE ZERO.** No machine is selected here, so the
  // only catalogue is `/tools`: seven tools, two destructive. The measured bug printed
  // `0 can change the machine` over exactly this form while the machine form printed the real count.
  const fleetCount = tokensDoor.getByText(
    '7 tools · 2 can change the machine · one list for every machine in the token\'s scope',
    { exact: true }
  );
  await fleetCount.waitFor({ timeout: 10_000 });
  step(`the fleet token form counts the registry: "${(await fleetCount.innerText()).trim()}"`);
  // **THE COVERAGE SENTENCE MARKS THE MACHINES IT WILL COVER.** The fleet row behind this sheet read
  // 14RJCR8's silence as 1h 49m; the sentence a person choosing "all machines" reads must state the
  // same thing about the same machine, or the dead machine looks live. The state is a statement about
  // the scope, not a filter of it: the sentence still names every enrolled machine.
  const coverage = (
    await tokensDoor.getByText(/This token will cover the \d+ machines enrolled now:/).innerText()
  ).trim();
  if (!coverage.includes(`${DEVICE.name} (online)`)) {
    throw new Error(`the coverage sentence did not mark the live machine: ${JSON.stringify(coverage)}`);
  }
  const markedOffline = `${OFFLINE_DEVICE.name} (${offlineState390.replace(' · ', ', ')})`;
  if (!coverage.includes(markedOffline)) {
    throw new Error(
      `the coverage sentence did not mark the silent machine as ${JSON.stringify(markedOffline)}: ` +
        JSON.stringify(coverage)
    );
  }
  step(`the fleet coverage sentence marks each machine: ${coverage}`);
  // **THE SCOPE CELL NAMES THE MACHINES.** A count alone sent the operator minting another token to
  // answer "does this one cover 84T3LIN?"; the fleet row must name the gate machine it reaches.
  const fleetScopeRow = tokensDoor.getByRole('row', { name: /zss-harness-20261106-fleet/ });
  const fleetScopeText = await fleetScopeRow.innerText();
  if (!fleetScopeText.includes(DEVICE.name)) {
    throw new Error(`the fleet token's scope read ${JSON.stringify(fleetScopeText)}`);
  }
  // **THE SCOPE CELL NAMES THE DOOR, NOT ONLY THE MACHINES.** A token whose scope names a machine
  // is still refused at that machine's URL when it is a fleet token; the cell must say which door
  // the credential opens, in the same words the 401 uses.
  if (!fleetScopeText.includes('fleet · /mcp/all')) {
    throw new Error(`the fleet token's scope did not name its door: ${JSON.stringify(fleetScopeText)}`);
  }
  const deviceScopeText = await tokensDoor
    .getByRole('row', { name: /gate-laptop/ })
    .innerText();
  if (!deviceScopeText.includes('device · /mcp/')) {
    throw new Error(`the device token's scope did not name its door: ${JSON.stringify(deviceScopeText)}`);
  }
  // **AT 390 THE NAME IS THE CARD'S WIDTH, NOT A 26 PX SLIVER.** The measured phone put the
  // three-machine scope (337 px) in the card's `auto` column beside the name and squeezed the name
  // cell to 0 px: `zss-harness-20261106-fleet` drew as `zs / har / nes / …` in 26 x 176 px and the
  // row was 414 px tall. Below 640 px the card is one column: the name takes the whole card, the
  // scope sentence wraps under it, then the tools, the action row and the dates. The 1440 card is
  // untouched (the rule is a viewport query), and the name must be whole at both widths.
  await page.waitForTimeout(200);
  const tokenCard390 = await tokensDoor.evaluate((sheet) => {
    const scroller = sheet.querySelector('.token-table-scroll');
    const box = (element) => {
      const rect = element?.getBoundingClientRect();
      return rect
        ? {
            top: Math.round(rect.top),
            right: Math.round(rect.right),
            bottom: Math.round(rect.bottom),
            width: Math.round(rect.width),
            height: Math.round(rect.height)
          }
        : null;
    };
    return {
      viewport: document.documentElement.clientWidth,
      documentScrollWidth: document.documentElement.scrollWidth,
      sheetRight: Math.round(sheet.getBoundingClientRect().right),
      scrollerScrollWidth: scroller?.scrollWidth ?? 0,
      scrollerClientWidth: scroller?.clientWidth ?? 0,
      rows: [...sheet.querySelectorAll('.token-table tbody tr')].map((row) => {
        const nameCell = row.cells[0];
        const nameBox = row.querySelector('.token-name');
        const strong = nameBox?.querySelector('strong');
        const scopeCell = row.cells[1];
        const actions = row.querySelector('.token-actions');
        // A Range counts the text's line boxes; `strong.getClientRects()` returns the inline's
        // union box, which stays one rect even when the text is a 26 px-wide 176 px-tall sliver.
        let lineRects = [];
        if (strong) {
          const range = document.createRange();
          range.selectNodeContents(strong);
          lineRects = [...range.getClientRects()];
        }
        return {
          name: strong?.textContent?.trim() ?? '',
          cell: box(nameCell),
          nameBox: box(nameBox),
          strong: box(strong),
          strongLines: lineRects.length,
          strongWidths: lineRects.map((rect) => Math.round(rect.width)),
          scope: box(scopeCell),
          row: box(row),
          actionLabels: actions
            ? [...actions.querySelectorAll('button')].map(
                (button) => button.textContent?.trim() ?? ''
              )
            : [],
          actionsDisplay: actions ? getComputedStyle(actions).display : null
        };
      })
    };
  });
  console.log(`browser-gate: tokens at 390: ${JSON.stringify(tokenCard390)}`);
  if (tokenCard390.rows.length !== TOKENS.length) {
    throw new Error(
      `the 390 token card list drew ${tokenCard390.rows.length} rows for ${TOKENS.length} registry tokens`
    );
  }
  const drawnNames = tokenCard390.rows.map((card) => card.name).sort();
  const expectedNames = TOKENS.map((token) => token.name).sort();
  if (drawnNames.join('|') !== expectedNames.join('|')) {
    throw new Error(`the 390 token list dropped a name: ${JSON.stringify(drawnNames)}`);
  }
  // The name is the one field that identifies the credential: its box is the card's width and its
  // text is one line. A 26 px sliver fails both, and it is measured before the row's other boxes.
  for (const card of tokenCard390.rows) {
    if (!card.nameBox || card.nameBox.width <= 200) {
      throw new Error(
        `the 390 token name's box is ${card.nameBox?.width ?? 0} px wide: ${JSON.stringify(card.name)}`
      );
    }
    if (card.strongLines !== 1 || !card.strong || card.strong.height > 30) {
      throw new Error(
        `the 390 token name is drawn in ${card.strongLines} line(s), ` +
          `${card.strong?.width ?? 0}x${card.strong?.height ?? 0} px: ${JSON.stringify(card.name)}`
      );
    }
  }
  for (const card of tokenCard390.rows) {
    if (!card.scope || !card.nameBox || card.scope.top < card.nameBox.bottom) {
      throw new Error(`the 390 scope is not under the name: ${JSON.stringify(card)}`);
    }
    if (card.actionLabels.join('|') !== 'Rotate|Revoke' || card.actionsDisplay === 'none') {
      throw new Error(
        `the 390 card lost its action row: ${JSON.stringify(card.actionLabels)} ` +
          `(${card.actionsDisplay})`
      );
    }
    if (!card.row || card.row.right > tokenCard390.viewport + 1) {
      throw new Error(`the 390 token card is outside the viewport at ${card.row?.right} px`);
    }
  }
  if (tokenCard390.documentScrollWidth > tokenCard390.viewport) {
    throw new Error(
      `the 390 Tokens sheet scrolls sideways: ${tokenCard390.documentScrollWidth} > ` +
        `${tokenCard390.viewport}`
    );
  }
  if (tokenCard390.scrollerScrollWidth > tokenCard390.scrollerClientWidth) {
    throw new Error(
      `the 390 token list scrolls sideways: ${tokenCard390.scrollerScrollWidth} > ` +
        `${tokenCard390.scrollerClientWidth}`
    );
  }
  step(
    `at 390 the token cards are one column: names ` +
      `${tokenCard390.rows.map((card) => `${card.nameBox?.width}px`).join(', ')} wide on one line, ` +
      `scope under each, actions kept, no sideways scroll`
  );
  // The sheet's entrance is 180 ms; the screenshot is the evidence, so let it settle.
  await page.waitForTimeout(250);
  await page.screenshot({ path: path.join(OUT, '06b-fleet-tokens-390.png') });
  // The three cards are the measured evidence: the name whole, the scope sentence under it.
  await page.locator('.token-table').screenshot({
    path: path.join(OUT, '06c-token-cards-390.png')
  });
  step(
    'from the fleet at 390 the Tokens door opens the sheet on Tokens and states the fixed device scope'
  );
  await page.keyboard.press('Escape');
  await tokensDoor.waitFor({ state: 'detached', timeout: 5_000 });
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

  // **A PRESET IS A REQUEST, NOT A LOCAL FILTER.** The Time control used to narrow the 1,000-line
  // window in the browser and fire nothing. `Last hour` must ask `/audit?from=&to=`, and the
  // sentence above the rows must name the range and the count the answer actually held.
  const rangeRequestsBefore = state.rangeRequests;
  await page.getByLabel('Time').selectOption('hour');
  await page.waitForFunction(
    () => document.querySelector('.audit-cap')?.textContent?.startsWith('Showing '),
    null,
    { timeout: 10_000 }
  );
  if (state.rangeRequests !== rangeRequestsBefore + 1) {
    throw new Error(
      `the preset fired ${state.rangeRequests - rangeRequestsBefore} range requests, not one`
    );
  }
  if (state.lastRange.to - state.lastRange.from !== 3_600_000) {
    throw new Error(`the preset asked for ${JSON.stringify(state.lastRange)}, not one hour`);
  }
  const rangeSentence390 = (await page.locator('.audit-cap').textContent())?.trim() ?? '';
  const rangeMatch = /^Showing \d{2}:\d{2}–\d{2}:\d{2} of \d{2}-\d{2}, (\d+) lines?$/.exec(
    rangeSentence390
  );
  if (!rangeMatch) {
    throw new Error(`the range sentence read ${JSON.stringify(rangeSentence390)}`);
  }
  const rangeRows = await page.locator('.audit-row').count();
  if (Number(rangeMatch[1]) !== state.lastRange.count || rangeRows !== state.lastRange.count) {
    throw new Error(
      `the sentence and rows counted ${rangeMatch[1]}/${rangeRows}, the answer held ` +
        `${state.lastRange.count}`
    );
  }
  const rangeHash = await page.evaluate(() => window.location.hash);
  if (rangeHash !== `#activity?from=${state.lastRange.from}&to=${state.lastRange.to}`) {
    throw new Error(`the range is not in the address: ${rangeHash}`);
  }
  await page.screenshot({ path: path.join(OUT, '07e-range-390.png') });
  step(
    `at 390 "Last hour" asked /audit?from=&to= and stated ${JSON.stringify(rangeSentence390)} ` +
      `over ${rangeRows} rows; the range is in the fragment`
  );

  // **THE RANGE THAT WAS DRAWN IS THE RANGE THAT LEAVES.** Export is beside Time and asks the
  // console's own route for the same two bounds; it must not save a different window or filter.
  const exportRequestsBefore = state.exportRequests;
  const exportAnswered = page.waitForResponse(
    (response) => new URL(response.url()).pathname === '/audit/export',
    { timeout: 10_000 }
  );
  await page
    .getByRole('button', { name: /^Export JSONL: the drawn range/ })
    .click();
  const exportResponse = await exportAnswered;
  if (state.exportRequests !== exportRequestsBefore + 1) {
    throw new Error(
      `Export made ${state.exportRequests - exportRequestsBefore} export requests, not one`
    );
  }
  if (
    state.lastExport.from !== state.lastRange.from ||
    state.lastExport.to !== state.lastRange.to
  ) {
    throw new Error(
      `Export asked for ${JSON.stringify(state.lastExport)}, the drawn range is ` +
        `${JSON.stringify(state.lastRange)}`
    );
  }
  const exportDisposition = exportResponse.headers()['content-disposition'] ?? '';
  if (
    exportResponse.status() !== 200 ||
    !exportDisposition.includes('summrise-audit-') ||
    !exportDisposition.endsWith('.ndjson"')
  ) {
    throw new Error(
      `Export answered ${exportResponse.status()} with ${JSON.stringify(exportDisposition)}`
    );
  }
  step(
    `at 390 Export asked /audit/export for the drawn ${state.lastExport.from}–` +
      `${state.lastExport.to} and received ${JSON.stringify(exportDisposition)}`
  );

  // **A REFUSAL IS AN ALERT, AND IT ENDS WITH ITS RANGE.** The route's 413 is a sentence the
  // operator must see: the gate answers one, and the page must draw it in the console's alert
  // surface with a Dismiss — not the muted hint that outlived the range. Clear resets the range and
  // must take the sentence with it.
  state.refuseExport = true;
  await page.getByRole('button', { name: /^Export JSONL: the drawn range/ }).click();
  const refusal = page.locator('.alert-inline[role="alert"]');
  await refusal.waitFor({ state: 'visible', timeout: 10_000 });
  const refusalText = (await refusal.innerText()).trim();
  if (!refusalText.includes('The console refused this export:')) {
    throw new Error(`the refusal alert read ${JSON.stringify(refusalText)}`);
  }
  if ((await refusal.getAttribute('class') ?? '').includes('hint')) {
    throw new Error('the refusal still wears the hint surface');
  }
  if ((await page.getByRole('button', { name: 'Dismiss' }).count()) === 0) {
    throw new Error('the refusal alert has no Dismiss');
  }
  await page.screenshot({ path: path.join(OUT, '07f-export-refusal-390.png') });
  await page.getByRole('button', { name: 'Clear' }).first().click();
  await page.waitForTimeout(800);
  if ((await page.locator('.alert-inline[role="alert"]').count()) !== 0) {
    throw new Error('the refusal alert survived the range it answered for');
  }
  state.refuseExport = false;
  step('a refused export draws an alert, and Clear ends it with its range');

  // **THE OFFLINE REASON WRAPS INSTEAD OF LOSING ITS LAST WORD.** The measured notice cut
  // "…and then dropped it" to "…and then dro…": 283 px of text in a 265 px line, with the rest only
  // in a title a phone cannot hover. The reason must be whole on screen and flow onto a second line.
  await page.locator('nav.rail button[title="Home"]').click();
  // From here the offline machine's tools read is the measured 503; the console error listener
  // above knows that refusal belongs to this leg.
  state.offlineToolsFails = true;
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

  // **THE OFFLINE TOKEN FORM DOES NOT CLAIM THE CONSOLE'S COUNT.** `/fleet/<id>/tools` is 503 for
  // this machine, so the count comes from `/tools`. The form must name that catalogue, cover the
  // machine with the row's own silence, and draw the static-fallback note on the default preset —
  // the measured bug hid the note behind Choose tools… and printed 46 as the machine's.
  await page.locator('.account > summary').click();
  await page.locator('.account').getByRole('button', { name: 'Tokens', exact: true }).click();
  const offlineTokens = page.getByRole('dialog', { name: 'Tokens' });
  await offlineTokens.waitFor({ timeout: 10_000 });
  const offlineCount = offlineTokens.getByText(
    '7 tools (console registry; the machine is offline) · 2 can change the machine',
    { exact: true }
  );
  await offlineCount.waitFor({ timeout: 10_000 });
  step(
    `the offline token form names the registry: "${(await offlineCount.innerText()).trim()}"`
  );
  const offlineCoverage = offlineTokens.getByText(
    `This token will cover ${OFFLINE_DEVICE.name} (${offlineState390.replace(' · ', ', ')}).`,
    { exact: true }
  );
  await offlineCoverage.waitFor({ timeout: 10_000 });
  const offlineFallback = offlineTokens.getByText(
    /The machine is offline\. This form counts the static fallback; the console refuses a plugin tool it cannot verify\./
  );
  await offlineFallback.waitFor({ timeout: 10_000 });
  // The default preset is All tools; Read-only keeps the same note because it is about the
  // catalogue, not the checklist.
  await offlineTokens.getByRole('button', { name: 'Read-only' }).click();
  await offlineFallback.waitFor({ timeout: 10_000 });
  await page.screenshot({ path: path.join(OUT, '07c-offline-tokens-390.png') });
  step(
    `the offline token form covers "${OFFLINE_DEVICE.name} (${offlineState390.replace(' · ', ', ')})" ` +
      `and draws the static-fallback note on All tools and Read-only`
  );
  await page.locator('.sheet-close').first().click();
  // The sheet opened from the account menu leaves that `<details>` open; collapse it so the next
  // leg's machine menu is not under the popup.
  await page.locator('.account > summary').click();
  state.offlineToolsFails = false;

  // **THE HUB'S SENTENCE NAMES ONLY ROWS ITS CONTROLS REACH.** The end leg opens the machine whose
  // 1000-line window holds the measured 974-row day: 283 client calls, 580 console reads and 111
  // rows no caller was recorded for, with 9 channel boundaries and 17 repeated keys withheld. The
  // sentence may not print the fetched 1000 (or 1004) as a number of lines: expanding and turning on
  // both chips is the only way to 974, and this leg measures that those four numbers are the rows
  // the controls actually draw at both widths.
  await page.locator('details.machine-menu > summary').click();
  await page.getByRole('button', { name: 'All machines' }).click();
  await page.getByRole('button', { name: /^Open DESKTOP-7QK3MN2/ }).click();
  await page.getByRole('heading', { name: 'Home', exact: true }).waitFor({ timeout: 10_000 });
  const bigSentence390 = await page.locator('.timeline-capped').textContent();
  const expectedBigSentence = (shown) =>
    `Showing ${shown === BIG_DRAWABLE ? 'all' : `the newest ${shown} of`} ${BIG_DRAWABLE} lines ` +
    `(${BIG_CLIENT} client calls, ${BIG_CONSOLE} console reads, ${BIG_UNATTRIBUTED} unattributed; ` +
    `${BIG_BOUNDARIES} channel events, ${BIG_REPEATS} repeats not drawn); ` +
    `older ones are not on this page.`;
  if (bigSentence390?.trim() !== expectedBigSentence(60)) {
    throw new Error(
      `the measured hub's sentence read ${JSON.stringify(bigSentence390?.trim())}, expected ` +
        `${JSON.stringify(expectedBigSentence(60))}`
    );
  }
  for (const digits of bigSentence390.match(/\d+/g) ?? []) {
    if (Number(digits) > BIG_DRAWABLE) {
      throw new Error(
        `the measured hub's sentence states ${digits}, more than the ${BIG_DRAWABLE} rows its controls draw`
      );
    }
  }
  // One rule: the sentence's client bucket and the day line above the rows are the same count.
  const bigToday = (await page.locator('.home-today').textContent())?.trim();
  if (!bigToday?.startsWith(`Today ${BIG_CLIENT} client calls`)) {
    throw new Error(`the measured hub's day line read ${JSON.stringify(bigToday)}`);
  }
  const bigRows = () => page.locator('.timeline-row').count();
  const bigMore = (more) =>
    page.getByRole('button', { name: `Show the rest of today (${more} more)` });
  if ((await bigRows()) !== 60) {
    throw new Error(`the measured hub drew ${await bigRows()} rows before the fold, not 60`);
  }
  const consoleChip = page.getByRole('button', { name: `Console reads · ${BIG_CONSOLE}` });
  const unattributedChip = page.getByRole('button', {
    name: `Unattributed · ${BIG_UNATTRIBUTED}`
  });
  await bigMore(BIG_CLIENT - 60).waitFor({ state: 'visible', timeout: 10_000 });
  await consoleChip.click();
  await unattributedChip.click();
  const folded = await bigMore(BIG_DRAWABLE - 60).textContent();
  const more390 = Number(/\((\d+) more\)/.exec(folded ?? '')?.[1] ?? -1);
  const reachable390 = (await bigRows()) + more390;
  if (reachable390 !== BIG_DRAWABLE) {
    throw new Error(`at 390 the controls reach ${reachable390} rows, not ${BIG_DRAWABLE}`);
  }
  const document390 = await page.evaluate(() => ({
    viewport: document.documentElement.clientWidth,
    scrollWidth: document.documentElement.scrollWidth
  }));
  console.log(
    `browser-gate: measured hub at 390: ${await bigRows()} rows drawn, ${reachable390} reachable, ` +
      `document ${document390.viewport}/${document390.scrollWidth}`
  );
  await page.screenshot({ path: path.join(OUT, '07c-measured-hub-390.png') });
  await page.setViewportSize({ width: 1440, height: 960 });
  await page.waitForTimeout(400);
  await bigMore(BIG_DRAWABLE - 60).click();
  await page.waitForFunction(
    (expected) => document.querySelectorAll('.timeline-row').length === expected,
    BIG_DRAWABLE,
    { timeout: 10_000 }
  );
  const reachable1440 = await bigRows();
  const allSentence = (await page.locator('.timeline-capped').textContent())?.trim();
  if (allSentence !== expectedBigSentence(BIG_DRAWABLE)) {
    throw new Error(
      `after expanding, the measured hub's sentence read ${JSON.stringify(allSentence)}`
    );
  }
  const lastBigRow = (await page.locator('.timeline-row').last().textContent())?.trim() ?? '';
  if (!lastBigRow.includes('Unattributed 110')) {
    throw new Error(`the expanded day's last row was ${JSON.stringify(lastBigRow)}`);
  }
  await page.screenshot({ path: path.join(OUT, '07d-measured-hub-1440.png') });
  console.log(
    `browser-gate: measured hub at 1440: ${reachable1440} rows drawn after the fold, ` +
      `the sentence now says all ${BIG_DRAWABLE}`
  );
  if (reachable1440 !== BIG_DRAWABLE) {
    throw new Error(`at 1440 the controls reached ${reachable1440} rows, not ${BIG_DRAWABLE}`);
  }
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(400);
  const reachable390After = await bigRows();
  if (reachable390After !== BIG_DRAWABLE) {
    throw new Error(
      `the expanded measured day lost rows at 390: ${reachable390After} of ${BIG_DRAWABLE}`
    );
  }
  step(
    `the measured hub states only the ${BIG_DRAWABLE} rows its fold and chips reach ` +
      `(${BIG_CLIENT} client calls, ${BIG_CONSOLE} console reads, ${BIG_UNATTRIBUTED} unattributed); ` +
      `at 390 ${reachable390} and at 1440 ${reachable1440} are reachable, and the fold reaches the last row`
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

  // **A RELEASE WITH NO INSTALLER SAYS SO, AND DRAWS NO DOOR.** The page reads `/installer` once
  // when the console boots, so a fresh boot after the stub stops holding the file is the
  // unavailable state. The stored machine is cleared first so this boot lands on the fleet (the
  // page remembers the chosen machine across reloads); the session itself is still the gate's.
  state.installerAvailable = false;
  await page.evaluate(() => localStorage.removeItem('summrise.selected.device'));
  await page.goto(base + '/', { waitUntil: 'domcontentloaded' });
  await page.getByRole('heading', { name: 'Machines', exact: true }).waitFor({ timeout: 10_000 });
  await page.getByText(INSTALLER_SENTENCE).waitFor({ timeout: 10_000 });
  if ((await page.getByRole('link', { name: 'Download Windows installer' }).count()) !== 0) {
    throw new Error('the unavailable installer still drew a download link');
  }
  step('with no installer staged the fleet states the sentence and draws no link');

  if (errors.length > 0) throw new Error('console errors: ' + errors.join(' ;; '));
  console.log(
    `browser-gate: PASS — ${steps.length} steps, ${state.writes.length} bytes written, marker echoed, ` +
      `too-old cursor refused ${state.oldCursorRefusals}x, paged ${state.limitedReads} limited read(s) ` +
      `from ${state.limitedFrom}, stream resumed at ${state.retainedFrom}`
  );
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
