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
  // The live fleet's ids are 64 hex and the Connect sentence carries one in a URL: with a short
  // fixture id the phone's sheet fitted, and audit-41 F3's clipping could not be measured.
  id: '4c718d6e44c83388c7e07bd0d811fa0b7cf22e2fced85d35d0021dd26b11f687',
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
  // The console's own channel pair: this machine's channel closed and reopened 41 minutes later.
  // The recovery line beside the start says how long it was gone (audit-44 F12); its pair is the
  // proven gap, not the device's journal (which this fixture leaves to the boot entry above).
  last_outage: { away_ms: 41 * 60_000, back_at_ms: NOW - 2 * 60 * 60_000 },
  // The platform the run form chooses its example from. Only `os` and the boot journal are here:
  // the form needs the OS fact, and the journal's newest entry is the moment this process started,
  // which is what makes the refused answer at NOW - 60 s belong to it (ITEM 5).
  host: {
    // **WHAT THE MACHINE IS (audit-44 F8).** The frame's own release/architecture/core facts; the
    // Machine tab must print them above Memory.
    os: { name: 'Windows', release: 'Windows 11', version: '10.0.26100', arch: 'x86_64' },
    cpu: { cores: 12, percent_used: 6 },
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
// second into the local day, and the distinct tool names keep them distinct even at one instant.
// Three of them are client calls, so the day line counts four with the recent call above; the rest
// are the console's own reads, which the Now band must not count and Activity still draws.
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
// minutes after local midnight, so every expected number is computed instead of hardcoded: the hub's
// day line counts the client calls, and Activity draws every row.
const FIXTURE_LINES = [CONSOLE_LINE, AUDIT_LINE, ...HISTORIC_LINES];
const TODAY_LINES = FIXTURE_LINES.filter((line) => line.ts_ms >= DAY_START.getTime());
const TODAY_CLIENT = TODAY_LINES.filter((line) => line.caller === 'client').length;
const ALL_CLIENT = FIXTURE_LINES.filter((line) => line.caller === 'client').length;
// One day, one number: the fleet's `today` is the day this fixture's `/audit/day` would answer.
DEVICE.today = { client_calls: TODAY_CLIENT, failed: 0 };
// The newest client row in the window is the one a narrowed list draws first. It is the run's line
// at a normal hour and a historic call in the five minutes after midnight, when `AUDIT_LINE` is
// yesterday; the leg names whichever it is rather than racing the clock.
const FIRST_CLIENT = [...FIXTURE_LINES]
  .filter((line) => line.caller === 'client')
  .sort((left, right) => right.ts_ms - left.ts_ms)[0];
// **THE MEASURED WINDOW: 1004 FETCHED, 974 ACTIVITY DRAWS.** A 1000-line answer for BIG_DEVICE:
// 9 channel boundaries, 17 repeated keys, and 974 rows the record draws — 283 client calls, 580
// console reads and 111 rows no caller was recorded for. All of it inside today's first twenty
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
  /** Whether the page is expected to draw a refused sign-in: its 401 is under test, not a fault. */
  refuseSignIn: false,
  opened: false,
  writes: '',
  echoed: false,
  cursor: 0,
  /** Whether the page subscribed to the session's stream: the push path, not the polling one. */
  streamOpened: false,
  /** How many ptys the page opened, so the retained-start session has an id of its own. */
  created: 0,
  /** How many `terminal.write` calls the page made; the session card joins their evidence. */
  writeCalls: 0,
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
   * answer for 14R. Armed for the offline leg, where the token form reads it on purpose. The
   * request count and the held answer are for the Retry leg: one press must be one read, and the
   * hold is how the working button is photographed before its 503 lands.
   */
  offlineToolsFails: false,
  offlineToolsRequests: 0,
  offlineToolsDelayMs: 0,
  /** Whether the console's deployment staged an installer, and how often the page asked. */
  installerAvailable: true,
  installerRequests: 0,
  /** Whether `/fleet` answers 500: the failed read whose one statement the last leg measures. */
  failFleet: false,
  /** Whether `POST /enroll` answers 500: the action failure that draws the shell banner. */
  failEnroll: false
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

/**
 * The page's range words for two instants: `09:00–17:00 of 11-05`, or both ends when the range
 * crosses a local midnight (`11-06 23:30 – 11-07 00:30`).
 *
 * The range is the request's two moments, so the gate names those instead of assuming one date. At
 * 00:30 `Last hour` is an hour wide across two local days; in the first hour after midnight a
 * single-date shape would reject a sentence the page is right to draw.
 */
function rangeText(from, to) {
  const pad = (value) => String(value).padStart(2, '0');
  const clock = (ms) => {
    const at = new Date(ms);
    return `${pad(at.getHours())}:${pad(at.getMinutes())}`;
  };
  const day = (ms) => {
    const at = new Date(ms);
    return `${pad(at.getMonth() + 1)}-${pad(at.getDate())}`;
  };
  const first = new Date(from);
  const last = new Date(to);
  const sameDay =
    first.getFullYear() === last.getFullYear() &&
    first.getMonth() === last.getMonth() &&
    first.getDate() === last.getDate();
  return sameDay
    ? `${clock(from)}–${clock(to)} of ${day(from)}`
    : `${day(from)} ${clock(from)} – ${day(to)} ${clock(to)}`;
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
  if (method === 'POST' && pathname === '/enroll' && state.failEnroll) {
    return json({ error: 'enrollment_unavailable' }, 500);
  }
  if (method === 'GET' && pathname === '/fleet') {
    if (state.failFleet) return json({ error: 'console_unavailable' }, 500);
    // A FLEET OF ZERO IS A STATE THE GATE MUST BE ABLE TO DRAW (the fortieth list). It is the page
    // a new operator meets, and it had no leg at all — which is why the installer could be the
    // page's smallest, last block for two lists without a test noticing.
    if (state.emptyFleet) return json([]);
    return json([DEVICE, OFFLINE_DEVICE, QUIET_DEVICE, BIG_DEVICE]);
  }
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
      state.offlineToolsRequests += 1;
      const reply = json({ error: 'device reconnecting', retry_after_ms: 1000 }, 503);
      return state.offlineToolsDelayMs
        ? { ...reply, delayMs: state.offlineToolsDelayMs }
        : reply;
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
    // **THE MACHINE TAB'S PROCESS TABLE NEEDS A LIST TO DRAW.** The fixture answered nothing for
    // `system.process_list`, so the gate's Machine page had no table and the leg that measures its
    // action column had nothing to measure. This is the operator's own shape: four rows the device
    // will not end (pid 0/4 and two kernel names) and four it would, one of them a reused-name
    // worker that must still be identified by pid.
    if (name === 'system.process_list') {
      return json({
        result: {
          content: [
            {
              type: 'text',
              text: JSON.stringify([
                { pid: 0, name: '[System Process]', memory_kb: 0 },
                { pid: 4, name: 'System', memory_kb: 0 },
                { pid: 224, name: 'Registry', memory_kb: 0 },
                { pid: 692, name: 'smss.exe', memory_kb: 0 },
                { pid: 828, name: 'LogonUI.exe', memory_kb: 74_496 },
                { pid: 1140, name: 'VirtualBoxVM.exe', memory_kb: 921 },
                { pid: 4242, name: 'nginx.exe', memory_kb: 82_000 },
                { pid: 9001, name: 'summrise-console.exe', memory_kb: 12_288 }
              ])
            }
          ]
        }
      });
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
      state.writeCalls += 1;
      // **THE CONSOLE PUBLISHES ONE ACTIVITY LINE PER WRITE.** Its `terminal.write` evidence is the
      // session and the input; the page's session card reads exactly that. Without it the card has
      // no last input, so the card's line is a fact of the live stream, not of the MCP answer.
      pushEvent('activity', {
        ts_ms: Date.now(),
        kind: 'mcp',
        device_id: DEVICE.id,
        status: 200,
        tool: 'terminal.write',
        caller: 'console',
        live: true,
        sentence: 'Typed into a terminal',
        evidence: { session: String(args.id ?? ''), input: String(args.data ?? '') }
      });
      // **HOLD THE FIRST REPLY SO THE PAGE'S OWN BATCHER SPLITS THE LINE.** The first chunk goes out
      // at once and the rest wait behind it; the measured 84T split `echo audit37-terminal` into two
      // writes 923 ms apart. The card must join them, not draw the newest batch (the measured
      // `cho audit37-terminal`). A run that sent one write cannot prove the join, so the leg below
      // refuses it.
      const answer = { result: { content: [{ type: 'text', text: '{}' }] } };
      return state.writeCalls === 1 ? { ...json(answer), delayMs: 400 } : json(answer);
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
    if (name === 'files.list') {
      // A root with a folder, a file and a name the filter can narrow. The Files leg measures the
      // filter's place at 390, so the listing must be non-empty (the control is drawn with it).
      return json({
        result: {
          content: [
            {
              type: 'text',
              text: JSON.stringify([
                { name: 'docs', kind: 'directory', len: 0 },
                { name: 'audit.jsonl', kind: 'file', len: 2048 },
                { name: 'notes.txt', kind: 'file', len: 12 }
              ])
            }
          ]
        }
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
  // And for the deliberately refused sign-in: the browser logs `/auth`'s 401 as a resource error,
  // while the panel's alert is what that leg measures.
  if (state.refuseSignIn && message.text().includes('401')) return;
  // The last two legs answer `/fleet` and `/enroll` with 500 on purpose: the failure state is
  // what they measure, not a page fault.
  if ((state.failFleet || state.failEnroll) && message.text().includes('500')) return;
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
  // A held reply is how the page's own write batcher is made to split one typed line (see the
  // `terminal.write` branch): the delay is on the answer, not on the pass-through.
  if (reply.delayMs) await new Promise((resolve) => setTimeout(resolve, reply.delayMs));
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

  // **A REFUSED SIGN-IN IS AN ANNOUNCED SENTENCE.** The stub refuses `wrong-token`; the panel draws
  // the console's generic sentence as a role=alert, with the machine code in the title, and starts
  // it with a capital (audit-39 F6). The real token is typed right after.
  await page.getByLabel('Operator token').fill('wrong-token');
  state.refuseSignIn = true;
  await page.getByRole('button', { name: 'Sign in' }).click();
  const signInAlert = page.getByRole('alert');
  await signInAlert.waitFor({ timeout: 10_000 });
  const signInText = (await signInAlert.textContent())?.trim();
  if (signInText !== 'The console could not sign you in.') {
    throw new Error(`the refused sign-in read ${JSON.stringify(signInText)}`);
  }
  const signInCause = await signInAlert.getAttribute('title');
  if (signInCause !== '401 invalid_token') {
    throw new Error(`the refusal's cause was ${JSON.stringify(signInCause)}`);
  }
  // The browser's own resource-error line for the 401 arrives with the response, a beat later.
  await page.waitForTimeout(300);
  state.refuseSignIn = false;
  step('a refused sign-in is an alert, its sentence capitalized, its cause in the title');
  await page.screenshot({ path: path.join(OUT, '01b-login-refused.png') });

  await page.getByLabel('Operator token').fill(TOKEN);
  await page.getByRole('button', { name: 'Sign in' }).click();
  await page.getByRole('heading', { name: 'Home', exact: true }).waitFor({ timeout: 10_000 });
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
  // **THE SHORT HASH IS A PREFIX, AND THE FULL DIGEST IS ONE GESTURE AWAY.** The strip showed only
  // `sha256 26d39c12…` with no title and no copy; the 64 hex characters are the title now and a
  // labelled control copies them.
  const installerDigest = await page.locator('.installer .hint span[title]').first().getAttribute('title');
  if (installerDigest !== INSTALLER.sha256) {
    throw new Error(`the installer digest title read ${JSON.stringify(installerDigest)}`);
  }
  if ((await page.getByRole('button', { name: 'Copy digest' }).count()) !== 1) {
    throw new Error('the installer strip has no Copy digest control');
  }
  if (state.installerRequests !== 1) {
    throw new Error(`the fleet read /installer ${state.installerRequests} times, not once`);
  }
  // **A SPARE FILE IS NOT THE PAGE'S CALL TO ACTION (the thirty-sixth list).** Measured on the live
  // page: an accent-filled `Download Windows installer` sat directly above the strip's own sentence
  // saying the fleet had already outrun that file — the page's loudest control inviting a press whose
  // answer it had given, on a fleet where the file could fix nothing. When the fleet is past the file
  // the strip says what it is for, and its download is a NEUTRAL control: the page's one accent button
  // is `Add a machine`.
  const installerState = await page.evaluate(() => {
    const strip = document.querySelector('.installer');
    const link = strip?.querySelector('a');
    const accentButtons = [...document.querySelectorAll('.page-actions a, .page-actions button')]
      .filter((element) => {
        const style = getComputedStyle(element);
        return /accent-solid|primary/.test(element.className) || style.backgroundColor.includes('176, 58, 10');
      })
      .map((element) => (element.textContent ?? '').trim());
    const style = link ? getComputedStyle(link) : null;
    return {
      heading: strip?.querySelector('h2')?.textContent?.trim() ?? '',
      linkClass: link?.className ?? '',
      linkBg: style?.backgroundColor ?? '',
      stale: /already past this file/.test(strip?.textContent ?? ''),
      accentButtons
    };
  });
  if (installerState.stale) {
    if (!installerState.linkClass.includes('installer-spare')) {
      throw new Error(
        `the stale installer's download is still the loud control: ${JSON.stringify(installerState)}`
      );
    }
    if (installerState.heading !== 'Installer for a new or returning machine') {
      throw new Error(
        `the stale installer's heading still invites an install: ${installerState.heading}`
      );
    }
  }
  // **A FAILED INSTALLER READ SAYS SO (the fortieth list).** Measured live: with `/installer`
  // answering 500 the page lost the strip entirely — 100 px gone, no sentence, no retry — because
  // the hook's `catch` set the same `null` that "not read yet" used. This leg breaks the read in the
  // page's own origin and refuses a page that answers with silence.
  await page.route('**/installer', (route) =>
    route.fulfill({ status: 500, body: JSON.stringify({ error: 'the gate broke it on purpose' }) })
  );
  await page.reload({ waitUntil: 'domcontentloaded' });
  await page.waitForTimeout(2500);
  const failedInstaller = await page.evaluate(() => {
    const strip = document.querySelector('.installer');
    if (!strip) return { present: false, text: '', retry: false, download: false };
    return {
      present: true,
      text: (strip.textContent ?? '').replace(/\s+/g, ' ').trim(),
      retry: [...strip.querySelectorAll('button')].some((b) =>
        /Try the installer again/i.test(b.textContent ?? '')
      ),
      download: [...strip.querySelectorAll('a')].some((a) =>
        /Download Windows installer/.test(a.textContent ?? '')
      )
    };
  });
  if (!failedInstaller.present) {
    throw new Error('a failed installer read drew nothing at all — the page answered with silence');
  }
  if (!/could not be read/.test(failedInstaller.text)) {
    throw new Error(`a failed installer read did not say so: "${failedInstaller.text}"`);
  }
  if (failedInstaller.download) {
    throw new Error('a failed installer read still offers the download it does not have');
  }
  if (!failedInstaller.retry) {
    throw new Error('a failed installer read offers no retry');
  }
  // **A ZERO FLEET'S NEXT STEP IS ONE CARD (the fortieth list).** Measured live at 1440 on a fleet
  // of zero: `page-head` 65, `empty-state` 100, and the installer strip 66 BELOW both — with the
  // empty state's sentence saying "start with the installer below". The page's next step was its
  // smallest block, drawn last, described by the block above it. Now there is ONE card: what to do,
  // the door, and the file's facts — and one installer door on the whole page, not two.
  // **THE PLANTED 500 BELONGS TO THE LEG ABOVE.** `page.route` registrations stack and the most
  // recent wins, so the zero-fleet leg asked `/installer`, got this leg's 500, and drew the FAILED
  // card — measured as `heading: "Add your first machine", hasDoor: false`. Detach it first; the
  // measurement above is already taken.
  await page.unroute('**/installer');
  state.emptyFleet = true;
  await page.reload({ waitUntil: 'domcontentloaded' });
  // The installer is its own request, so the card lands before its door does. Wait for the DOOR,
  // not for a fixed interval — a page still assembling is not a page that failed.
  await page
    .locator('[data-testid="first-run"] a[href="/installer/download"]')
    .waitFor({ timeout: 10_000 })
    .catch(() => {});
  await page.waitForTimeout(600);
  const firstRun = await page.evaluate(() => {
    const card = document.querySelector('[data-testid="first-run"]');
    const strip = document.querySelector('.installer.installer-strip');
    const pageEl = document.querySelector('.page');
    const doors = [...document.querySelectorAll('a[href="/installer/download"]')].filter((a) => {
      const r = a.getBoundingClientRect();
      return r.width > 0 && r.height > 0;
    }).length;
    return {
      card: card
        ? {
            h: Math.round(card.getBoundingClientRect().height),
            y: Math.round(card.getBoundingClientRect().y),
            heading: (card.querySelector('h2')?.textContent ?? '').trim(),
            hasDoor: [...card.querySelectorAll('a[href="/installer/download"]')].length > 0,
            text: (card.textContent ?? '').replace(/\s+/g, ' ').trim().slice(0, 120)
          }
        : null,
      strip: strip ? Math.round(strip.getBoundingClientRect().height) : null,
      doors,
      blocks: pageEl ? [...pageEl.children].map((k) => (k.className ?? '').toString().slice(0, 26)) : []
    };
  });
  if (firstRun.card === null) {
    throw new Error('a fleet of zero drew no first-run card');
  }
  if (!firstRun.card.hasDoor) {
    throw new Error(
      'the first-run card does not carry the installer door: ' + JSON.stringify(firstRun)
    );
  }
  if (/Add your first machine/.test(firstRun.card.heading)) {
    throw new Error(
      `the first-run card still says "${firstRun.card.heading}" while it draws the installer door`
    );
  }
  if (firstRun.strip !== null) {
    throw new Error(
      'the installer strip is drawn beside the first-run card — the page says its next step twice'
    );
  }
  if (firstRun.doors !== 1) {
    throw new Error(`a fleet of zero draws ${firstRun.doors} installer doors`);
  }
  step(
    `a fleet of zero is one card, ${firstRun.card.h} px, headed "${firstRun.card.heading}", ` +
      `with ${firstRun.doors} installer door and no second strip`
  );
  state.emptyFleet = false;
  await page.reload({ waitUntil: 'domcontentloaded' });
  await page.waitForTimeout(2200);

  step(
    `a failed installer read says so and offers the retry: "${failedInstaller.text.slice(0, 80)}"`
  );

  step(
    `the installer's state is honest: "${installerState.heading}", its download is a ` +
      `${installerState.linkClass.includes('installer-spare') ? 'secondary' : 'primary'} control, ` +
      `and the page's accent button is ${installerState.accentButtons.join(' / ') || 'none'}`
  );

  step(
    `the fleet's zero state offers ${INSTALLER.filename} (${installerLine}) from /installer/download`
  );
  await page.screenshot({ path: path.join(OUT, '02-machines.png') });

  // **NO MACHINE SELECTED: THE PICKER IS THE PAGE (audit-40 F7).** The rail's Device entry used
  // to open a page headed `Terminal` with the full tab strip above `Choose a machine`, inviting five
  // surfaces that cannot run until a machine is chosen. The title is the question now; the tabs and
  // Connect wait for the answer.
  await page.locator('nav.rail button[title="Device"]').click();
  await page
    .getByRole('heading', { name: 'Choose a machine', exact: true, level: 1 })
    .waitFor({ timeout: 10_000 });
  const pickerFacts = await page.evaluate(() => ({
    h1: document.querySelector('.device-page h1')?.textContent?.trim() ?? '',
    tabs: document.querySelectorAll('.device-tabs').length,
    visibleConnect: [...document.querySelectorAll('button')].filter(
      (button) =>
        button.getAttribute('aria-label') === 'Connect a client' &&
        button.getBoundingClientRect().width > 0
    ).length,
    rows: document.querySelectorAll('.picker li').length,
    opens: [...document.querySelectorAll('.picker button')].map((button) => ({
      label: button.getAttribute('aria-label') ?? '',
      text: button.textContent?.trim() ?? ''
    })),
    states: [...document.querySelectorAll('.picker-state')].map((state) => ({
      text: state.textContent?.trim() ?? '',
      title: state.getAttribute('title') ?? '',
      visible: state.getClientRects().length > 0
    })),
    versions: [...document.querySelectorAll('.picker .chip')].map((chip) =>
      chip.textContent?.trim() ?? ''
    )
  }));
  if (pickerFacts.h1 !== 'Choose a machine') {
    throw new Error(`Device without a machine is headed ${JSON.stringify(pickerFacts.h1)}`);
  }
  if (pickerFacts.tabs !== 0) {
    throw new Error(`Device without a machine drew ${pickerFacts.tabs} tab strip(s)`);
  }
  if (pickerFacts.visibleConnect !== 0) {
    throw new Error(
      `Device without a machine drew ${pickerFacts.visibleConnect} Connect button(s)`
    );
  }
  if (pickerFacts.rows !== 4 || pickerFacts.opens.length !== 4) {
    throw new Error(
      `the picker drew ${pickerFacts.rows} rows and ${pickerFacts.opens.length} Open buttons, not 4`
    );
  }
  // **ONE VERB (audit-44 F9).** `Use` becomes `Open`, the same verb as Home's attention row, its
  // summary and the fleet row's accessible name.
  for (const door of pickerFacts.opens) {
    if (!door.label.startsWith('Open ') || door.text !== 'Open') {
      throw new Error(`the picker's door reads ${JSON.stringify(door)}, expected Open <machine>`);
    }
  }
  // **THE STATE IS A WORD, NOT A COLOUR, AND THE VERSION KEEPS ITS V.** The words were `.sr-only`
  // under an `aria-hidden` dot, and the chip dropped the `v` Home's column carries.
  if (pickerFacts.states.length !== 4) {
    throw new Error(`the picker drew ${pickerFacts.states.length} state words, not 4`);
  }
  for (const state of pickerFacts.states) {
    if (!state.visible || (state.text !== 'online' && state.text !== 'offline')) {
      throw new Error(`the picker's state word reads ${JSON.stringify(state)}`);
    }
    if (state.text === 'online' && !/^online · seen .+ ago$/.test(state.title)) {
      throw new Error(`the online state's title reads ${JSON.stringify(state.title)}`);
    }
  }
  if (pickerFacts.versions.length !== 4 || pickerFacts.versions.some((v) => !/^v\d/.test(v))) {
    throw new Error(`the picker's versions read ${JSON.stringify(pickerFacts.versions)}`);
  }
  step(
    `Device with no machine is headed "${pickerFacts.h1}", with no tab strip and no Connect; ` +
      `the picker lists ${pickerFacts.rows} machines`
  );
  await page.screenshot({ path: path.join(OUT, '02b-device-picker.png') });
  await page.locator('nav.rail button[title="Home"]').click();
  await page.getByRole('heading', { name: 'Home', exact: true }).waitFor({ timeout: 10_000 });

  // **#signin WHILE SIGNED IN LEADS WITH BACK (audit-40 F8).** The panel was headed `Sign in to the
  // console` while its sentence said the tab already was, and the credential form was the primary.
  // The title names the state, Back is the primary action, and the token form is behind one
  // disclosure.
  await page.evaluate(() => {
    window.location.hash = '#signin';
  });
  await page
    .getByRole('heading', { name: 'You are signed in', exact: true })
    .waitFor({ timeout: 10_000 });
  const signInPanel = await page.evaluate(() => {
    const heading = document.querySelector('.login h1')?.textContent?.trim() ?? '';
    const back = [...document.querySelectorAll('.login button')].find(
      (button) => button.textContent?.trim() === 'Back to the console'
    );
    const disclosure = document.querySelector('.login details');
    const token = document.querySelector('.login #token');
    return {
      heading,
      backPrimary: back?.classList.contains('primary') ?? false,
      disclosureOpen: disclosure?.hasAttribute('open') ?? true,
      hasDisclosure: disclosure !== null,
      tokenInsideDisclosure: token !== null && disclosure?.contains(token) === true,
      signInTitle: document.title
    };
  });
  if (signInPanel.heading !== 'You are signed in') {
    throw new Error(`#signin while signed in is headed ${JSON.stringify(signInPanel.heading)}`);
  }
  if (!signInPanel.backPrimary) {
    throw new Error('#signin while signed in does not lead with a primary Back to the console');
  }
  if (
    !signInPanel.hasDisclosure ||
    signInPanel.disclosureOpen ||
    !signInPanel.tokenInsideDisclosure
  ) {
    throw new Error(
      '#signin while signed in does not keep the token form behind a closed disclosure'
    );
  }
  if (signInPanel.signInTitle !== 'Signed in · Summrise console') {
    throw new Error(`#signin's tab title read ${JSON.stringify(signInPanel.signInTitle)}`);
  }
  step(
    '#signin while signed in is headed "You are signed in", titled "Signed in", leads with a ' +
      'primary Back and keeps the token form behind a closed disclosure'
  );
  await page.screenshot({ path: path.join(OUT, '02c-signin-signed-in.png') });
  await page.getByRole('button', { name: 'Back to the console' }).click();
  await page.getByRole('heading', { name: 'Home', exact: true }).waitFor({ timeout: 10_000 });

  // **AN OFFLINE MACHINE'S AGE IS THE NEWEST EVIDENCE.** No frame survived the console restart; the
  // registry's connect time is two hours old, the open close is 1 h 49 m old, and the pair in
  // `last_outage` is from an earlier absence. The row must read the open close at the desktop width,
  // and the attention sentence must state the same age.
  await page.setViewportSize({ width: 1440, height: 960 });
  await page.waitForTimeout(200);
  const offlineRow1440 = page.locator('tr.fleet-row', {
    has: page.getByRole('button', { name: /^Open DESKTOP-14RJCR8/ })
  });
  const offlineState1440 = (await offlineRow1440.locator('.fleet-state').textContent())?.trim();
  if (offlineState1440 !== 'offline · last seen 1h 49m ago') {
    throw new Error(`the offline row read "${offlineState1440}" at 1440`);
  }
  step(`at 1440 the offline row says "${offlineState1440}"`);

  // **HOME IS THE VERDICT AND THE NEXT ACTION.** The page's first fact is one large sentence from
  // the fleet read: three of four machines are online, and every machine needs a person (14R is
  // away, and all four run behind the staged 0.3.280 file). The block below names each problem in
  // one row with the door that opens that machine.
  const verdict1440 = (await page.locator('.home-verdict').textContent())?.trim();
  if (verdict1440 !== '3 of 4 online · 4 need attention') {
    throw new Error(`Home's verdict read ${JSON.stringify(verdict1440)} at 1440`);
  }
  const attention1440 = await page.evaluate(() => {
    const block = document.querySelector('.attention');
    if (!block) return null;
    return {
      heading: block.querySelector('h2')?.textContent?.trim() ?? '',
      top: Math.round(block.getBoundingClientRect().top),
      rows: [...block.querySelectorAll('.attention-row')].map((row) => ({
        name: row.querySelector('.attention-machine')?.textContent?.trim() ?? '',
        problems: [...row.querySelectorAll('.attention-problems li')].map((problem) =>
          problem.textContent?.trim() ?? ''
        ),
        door: row.querySelector('button')?.getAttribute('aria-label') ?? '',
        doorText: row.querySelector('button')?.textContent?.trim() ?? '',
        doors: row.querySelectorAll('button').length
      }))
    };
  });
  if (attention1440 === null) {
    throw new Error('Home drew no Needs attention block with four machines that need one');
  }
  if (attention1440.heading !== 'Needs attention') {
    throw new Error(`the attention block's heading read ${JSON.stringify(attention1440.heading)}`);
  }
  // **ONE ROW PER MACHINE, ITS PROBLEMS UNDER IT, ONE DOOR (audit-40 F6).** The verdict counts
  // machines, so four machines with five problems are four rows; the two-door 14R and the four
  // identical buttons were the measured defect.
  // **EACH ROW NAMES THE DISTANCE, NOT ONLY THE VERDICT (the twenty-eighth list, item 4).** The
  // fixture's machines all run 0.3.42 against the staged 0.3.280, so every behind clause reads the
  // same numbers — which is exactly what the operator's page could not say at all: it printed
  // `Behind the release.` and left the machine's own build 445 px below, in the table.
  const expectedMachines = [
    {
      name: 'DESKTOP-9VH92BJ',
      problems: ['On v0.3.42 · behind 0.3.280.']
    },
    {
      name: 'DESKTOP-14RJCR8',
      problems: ['offline · last seen 1h 49m ago', 'On v0.3.42 · behind 0.3.280.']
    },
    {
      name: 'DESKTOP-CPH4KB2',
      problems: ['On v0.3.42 · behind 0.3.280.']
    },
    {
      name: 'DESKTOP-7QK3MN2',
      problems: ['On v0.3.42 · behind 0.3.280.']
    }
  ];
  const seenMachines = attention1440.rows.map((row) => ({
    name: row.name,
    problems: row.problems
  }));
  if (JSON.stringify(seenMachines) !== JSON.stringify(expectedMachines)) {
    throw new Error(`the attention rows read ${JSON.stringify(seenMachines)}`);
  }
  for (const row of attention1440.rows) {
    if (row.door !== `Open machine for ${row.name}`) {
      throw new Error(
        `the ${row.name} row's door reads ${JSON.stringify(row.door)}, expected one for that machine`
      );
    }
    if (row.doorText !== 'Open') {
      throw new Error(
        `the ${row.name} row's visible door reads ${JSON.stringify(row.doorText)}, not Open`
      );
    }
    if (row.doors !== 1) {
      throw new Error(`the ${row.name} row drew ${row.doors} doors, not one`);
    }
  }
  // The release is the installer's version; the console's own build is another scheme (audit-40
  // F10), so no row may call the staged 0.3.280 "the console".
  const attentionText1440 = attention1440.rows
    .flatMap((row) => [row.name, ...row.problems])
    .join(' ');
  if (/the console has/i.test(attentionText1440)) {
    throw new Error(`an attention row calls the release the console: "${attentionText1440}"`);
  }
  // **ONE RELEASE POSITION, ONE VERSION POSITION (audit-43 F2), AND EVERY ROW SAYS ITS DISTANCE
  // (the twenty-eighth list, item 4).** The block names the target release once; a row names that
  // machine's own build, so "behind" is a number the reader can see rather than a pointer to the
  // table 445 px below.
  const releaseLine = (await page.locator('.attention-release').textContent())?.trim();
  if (releaseLine !== 'The release is 0.3.280; the machines below are not on it.') {
    throw new Error(`the attention block's release line read ${JSON.stringify(releaseLine)}`);
  }
  if ((await page.locator('.attention-release').count()) !== 1) {
    throw new Error('the attention block states the release more than once');
  }
  // The old rule forbade a version in the block at all; that rule was the defect (item 4) — the row
  // must say how far behind. What still must not happen is the CONSOLE's own build being called a
  // release, or a row naming a machine's build twice.
  const rowVersions = attention1440.rows.map((row) => (row.problems.join(' ').match(/v?\d+\.\d+\.\d+/g) ?? []).length);
  if (rowVersions.some((count) => count > 2)) {
    throw new Error(`an attention row repeats a version: "${attentionText1440}"`);
  }
  // **THE RELEASE LINE NAMES THE NUMBER IT MEASURES (the thirty-third list).** Measured on
  // `0.3.273-ux129`: the fleet ran 0.3.292/0.3.275/0.3.292/0.3.291 while the installer was 0.3.291,
  // and Home said `The release is 0.3.291; the machines below are not on it.` — three of four were
  // on or past it. The block measures against the newer of the fleet's newest build and the staged
  // installer, and its sentence says which of the two it is; this leg checks both halves.
  const releaseTarget = await page.evaluate(() => {
    const parse = (text) => {
      const match = /(\d+\.\d+\.\d+)/.exec(text ?? '');
      return match ? match[1].split('.').map(Number) : null;
    };
    const compare = (a, b) => {
      for (let i = 0; i < Math.max(a.length, b.length); i += 1) {
        const diff = (a[i] ?? 0) - (b[i] ?? 0);
        if (diff !== 0) return diff;
      }
      return 0;
    };
    const line = document.querySelector('.attention-release')?.textContent?.trim() ?? '';
    const installerText = document.querySelector('.installer')?.textContent ?? '';
    const builds = [...document.querySelectorAll('.fleet-table tbody tr .fleet-build')].map(
      (node) => parse(node.textContent)
    );
    const newest = builds.filter(Boolean).sort(compare).pop() ?? null;
    return {
      line,
      installer: parse(installerText),
      newest,
      fleetForm: /in the fleet is/.test(line),
      releaseForm: /The release is/.test(line)
    };
  });
  if (releaseTarget.line === '') {
    throw new Error('the attention block draws no release line');
  }
  if (releaseTarget.newest === null || releaseTarget.installer === null) {
    throw new Error(`the release line has no number to check: ${JSON.stringify(releaseTarget)}`);
  }
  // The newer of the two is the target, and the sentence must be the form that names it. A tie
  // takes the fleet's form: the machines' own builds are the page's subject.
  const compare = (a, b) => {
    for (let i = 0; i < Math.max(a.length, b.length); i += 1) {
      const diff = (a[i] ?? 0) - (b[i] ?? 0);
      if (diff !== 0) return diff;
    }
    return 0;
  };
  const installerLeads = compare(releaseTarget.newest, releaseTarget.installer) < 0;
  const named = /(?:The release is|in the fleet is) (\d+\.\d+\.\d+)/.exec(releaseTarget.line)?.[1];
  if (installerLeads ? !releaseTarget.releaseForm : !releaseTarget.fleetForm) {
    throw new Error(
      `the release line names the wrong subject (fleet ${releaseTarget.newest} vs installer ` +
        `${releaseTarget.installer}): "${releaseTarget.line}"`
    );
  }
  const expected = installerLeads ? releaseTarget.installer : releaseTarget.newest;
  if (named !== expected.join('.')) {
    throw new Error(
      `the release line says ${named}, but the target it should name is ${expected.join('.')}: ` +
        `"${releaseTarget.line}"`
    );
  }
  step(
    `the release line names its own target: "${releaseTarget.line}" (fleet newest ` +
      `${releaseTarget.newest.join('.')}, installer ${releaseTarget.installer.join('.')})`
  );

  step(
    `the verdict is "${verdict1440}"; Needs attention lists ${attention1440.rows.length} machines ` +
      `with ${attention1440.rows.reduce((sum, row) => sum + row.problems.length, 0)} problems, ` +
      `one release line ("${releaseLine}") and one door each`
  );

  // **THE FLEET IS A TABLE, NOT A SECOND ACTIVITY.** Machine / Status / Version / Started are the
  // four facts; the names are the scale's 17 px semibold, the dot sits with its word and the version
  // is a chip.
  // No row draws `client calls`: that counter is Activity's noun and it made Home read like a copy.
  const fleet1440 = await page.evaluate(() => {
    const table = document.querySelector('table.fleet-table');
    const first = table?.querySelector('.fleet-name');
    const rows = [...document.querySelectorAll('tr.fleet-row')].map((row) => ({
      name: row.querySelector('.fleet-name')?.textContent?.trim() ?? '',
      status: row.querySelector('.fleet-state')?.textContent?.trim() ?? '',
      version: row.querySelector('.fleet-build')?.textContent?.trim() ?? '',
      started: row.querySelector('.fleet-verdict')?.textContent?.trim() ?? null,
      work: row.querySelector('.fleet-work')?.textContent?.trim() ?? null,
      cells: row.querySelectorAll('td').length,
      height: Math.round(row.getBoundingClientRect().height),
      nameSize: first ? Number.parseFloat(getComputedStyle(first).fontSize) : 0,
      nameWeight: first ? getComputedStyle(first).fontWeight : '',
      dot: (() => {
        const dot = row.querySelector('.fleet-status .dot');
        return dot ? getComputedStyle(dot).backgroundColor : null;
      })()
    }));
    const tableBox = table?.getBoundingClientRect();
    const installer = document.querySelector('.installer');
    return {
      headers: [...(table?.querySelectorAll('thead th') ?? [])].map((th) =>
        th.textContent?.trim() ?? ''
      ),
      rows,
      text: document.querySelector('.page')?.textContent ?? '',
      tableTop: tableBox ? Math.round(tableBox.top) : null,
      tableBottom: tableBox ? Math.round(tableBox.bottom) : null,
      installerTop: installer ? Math.round(installer.getBoundingClientRect().top) : null
    };
  });
  if (
    JSON.stringify(fleet1440.headers) !==
    JSON.stringify(['Machine', 'Status', 'Version', 'Started', 'Actions'])
  ) {
    throw new Error(`the fleet table's headers read ${JSON.stringify(fleet1440.headers)}`);
  }
  if (fleet1440.rows.length !== 4) {
    throw new Error(`the fleet drew ${fleet1440.rows.length} rows at 1440`);
  }
  if (fleet1440.rows[0].nameSize !== 17 || fleet1440.rows[0].nameWeight !== '600') {
    throw new Error(
      `the fleet name read ${fleet1440.rows[0].nameSize}px/${fleet1440.rows[0].nameWeight}, ` +
        "not the scale's 17px/600"
    );
  }
  for (const row of fleet1440.rows) {
    if (row.cells !== 5) throw new Error(`the ${row.name} row has ${row.cells} cells, not 5`);
    if (row.work !== null) throw new Error(`the ${row.name} row still draws a client-call counter`);
    if (row.version !== 'v0.3.42') {
      throw new Error(`the ${row.name} row's version read "${row.version}"`);
    }
    if (!row.status) throw new Error(`the ${row.name} row lost its status`);
    if (!row.dot) throw new Error(`the ${row.name} row lost its status dot`);
    if (row.height > 64) throw new Error(`the ${row.name} row grew to ${row.height} px at 1440`);
  }
  if (/\bclient calls?\b/.test(fleet1440.text)) {
    throw new Error('Home still draws a client-call counter');
  }
  const onlineRow1440 = fleet1440.rows.find((row) => row.name === 'DESKTOP-9VH92BJ');
  const startText1440 = onlineRow1440?.started ?? '';
  // **STARTED IS THE AGE; THE VERSION COLUMN OWNS THE VERSION.** The row used to read
  // `v0.3.42` and `started 0.3.42 · 1h ago` in adjacent cells; the start keeps its update answer
  // and states only when.
  if (startText1440 !== '1h ago (update to 0.3.43 refused)') {
    throw new Error(`the online row's start line read "${startText1440}"`);
  }
  if (startText1440.includes('0.3.42')) {
    throw new Error(`the row's Started cell repeats the Version cell: "${startText1440}"`);
  }
  const startTitle = await page
    .locator('tr.fleet-row', {
      has: page.getByRole('button', { name: /^Open DESKTOP-9VH92BJ/ })
    })
    .locator('.fleet-verdict')
    .getAttribute('title');
  const startInstant = new Date(NOW - 60 * 60_000).toISOString();
  if (!startTitle?.includes(`(${startInstant})`)) {
    throw new Error(`the row's start title "${startTitle}" does not carry the exact instant`);
  }
  if (
    fleet1440.installerTop === null ||
    fleet1440.tableBottom === null ||
    fleet1440.installerTop < fleet1440.tableBottom
  ) {
    throw new Error('the installer is not below the fleet table');
  }
  if (attention1440.top > (fleet1440.tableTop ?? 0)) {
    throw new Error('Needs attention is not above the fleet table');
  }
  step(
    `the fleet is a 4-row table (${fleet1440.rows
      .map((row) => `${row.name} ${row.height}px`)
      .join(', ')}), 17 px names, a version chip and a start line; the installer sits below it`
  );

  await page.setViewportSize({ width: 1280, height: 900 });
  await page.waitForTimeout(200);

  // **THE ATTENTION DOOR OPENS THE MACHINE (audit-43 F1).** The measured `Open machine` selected
  // the machine and left #home, so the one action the block exists for did nothing where the
  // operator was looking. It now selects and opens Device → Machine, where an absent machine's
  // last facts live.
  await page.getByRole('button', { name: 'Open machine for DESKTOP-CPH4KB2' }).click();
  await page.getByRole('heading', { name: 'Machine', exact: true }).waitFor({ timeout: 10_000 });
  const attentionHash = await page.evaluate(() => window.location.hash);
  if (attentionHash !== '#device/machine') {
    throw new Error(`the attention door landed on ${JSON.stringify(attentionHash)}`);
  }
  step('the attention door opened DESKTOP-CPH4KB2 on Device → Machine');

  await page.locator('nav.rail button[title="Home"]').click();
  await page.getByRole('heading', { name: 'Home', exact: true }).waitFor({ timeout: 10_000 });
  // **THE SELECTED MACHINE IS HIGHLIGHTED, AND THE CARD NAMES IT (audit-45 F1).** audit-43 F2 kept
  // the summary down to counts and doors because the row below it already named the machine; the
  // operator's brief is that a section headed "Selected machine" answers at a glance, so the card
  // states the name, `online`, and the build once each — and still repeats no AGE (the row's), no
  // second presence spelling and no third copy of the version elsewhere on the page.
  await page.locator('.machine-summary').waitFor({ state: 'visible', timeout: 10_000 });
  if ((await page.locator('.home-verdict').count()) !== 1) {
    throw new Error('opening a machine hid Home\'s verdict');
  }
  if ((await page.locator('.attention').count()) === 0) {
    throw new Error('opening a machine hid Home\'s Needs attention block');
  }
  if ((await page.locator('tr.fleet-row').count()) !== 4) {
    throw new Error('opening a machine hid the fleet table');
  }
  const selectedHome = await page.evaluate(() => {
    const summary = document.querySelector('.machine-summary');
    const current = document.querySelector('tr.fleet-row.is-current');
    const menuState = document.querySelector('.machine-menu-state');
    return {
      heading: summary?.querySelector('h2')?.textContent?.trim() ?? '',
      who: summary?.querySelector('.machine-summary-who')?.textContent?.trim() ?? '',
      text: summary?.textContent ?? '',
      currentRows: document.querySelectorAll('tr.fleet-row.is-current').length,
      currentName: current?.querySelector('.fleet-name')?.textContent?.trim() ?? '',
      menuState: menuState?.textContent?.trim() ?? ''
    };
  });
  // **THE CARD'S HEADING IS THE SELECTED MACHINE'S NAME (the thirty-fifth list).** It used to read
  // the generic `Selected machine` and then a line repeating the name, the presence word and the
  // build — three facts the topbar picker and the highlighted row already stated. The name is the
  // heading now, and it must be the machine the highlight is on: two statements of one subject are
  // only worth their height if they agree.
  // **THE SELECTED CARD IS THE CARD, TOO (the thirty-eighth list).** It was the one Home block that
  // hand-wrote its own padding — 14px 18px against the grammar's 16 — while `.card` sat beside it
  // unused. Read on the rendered element, at both widths: the phone's override sets a smaller
  // padding on purpose (a 390 px column cannot afford 16 px on each side), so the assertion is the
  // CARD's grammar, which is what the class brings, not the desktop's padding value.
  const selectedCard = await page.evaluate(() => {
    const summary = document.querySelector('.machine-summary');
    if (!summary) return null;
    const style = getComputedStyle(summary);
    return {
      card: summary.classList.contains('card'),
      radius: style.borderTopLeftRadius,
      leftEdge: style.borderLeftWidth
    };
  });
  if (selectedCard === null) throw new Error('Home draws no .machine-summary');
  if (!selectedCard.card) throw new Error("the selected-machine card is not on the card grammar");
  if (selectedCard.radius !== '14px') {
    throw new Error(`the selected-machine card's corner is ${selectedCard.radius}`);
  }
  // And its accent edge is deliberate: rule 16, the page's one accent edge, still 3 px.
  if (selectedCard.leftEdge !== '3px') {
    throw new Error(`the selected card's accent edge is ${selectedCard.leftEdge}, not 3px`);
  }
  if (selectedHome.heading !== selectedHome.currentName) {
    throw new Error(
      `the summary's heading read ${JSON.stringify(selectedHome.heading)}, the highlighted row ` +
        `names ${JSON.stringify(selectedHome.currentName)}`
    );
  }
  // An online machine says nothing else about itself: presence is the row's dot and age.
  if (/^online|online · seen/.test(selectedHome.who)) {
    throw new Error(`the summary repeats an online machine's presence: ${selectedHome.who}`);
  }
  // **THE NAME IS STATED ONCE, AND NOT AGAIN WITH ITS PRESENCE AND BUILD.** The old line read
  // `DESKTOP-CPH4KB2 · online · 0.3.42` while the row below stated the same three facts; an online
  // machine draws no such line now, so the card's whole identity is its heading.
  if (selectedHome.who !== '') {
    throw new Error(`the summary drew a second identity line: ${JSON.stringify(selectedHome.who)}`);
  }
  for (const repeated of ['last seen', 'seen ', '· v0.3.246 · v0.3.', 'Running']) {
    if (selectedHome.text.includes(repeated)) {
      throw new Error(`the summary repeats ${JSON.stringify(repeated)}: "${selectedHome.text}"`);
    }
  }
  if (selectedHome.currentRows !== 1 || selectedHome.currentName !== 'DESKTOP-CPH4KB2') {
    throw new Error(`the selected row read ${JSON.stringify(selectedHome)}`);
  }
  if (selectedHome.menuState !== 'online') {
    throw new Error(`the topbar's presence word read ${JSON.stringify(selectedHome.menuState)}`);
  }
  step(
    `the selected row is highlighted once and the summary's heading IS the machine ` +
      `("${selectedHome.heading}"), with no second identity line; the topbar says "online"`
  );

  await page
    .locator('.machine-summary')
    .getByRole('button', { name: 'Open DESKTOP-CPH4KB2' })
    .click();
  await page.getByRole('heading', { name: 'Overview', exact: true }).waitFor({ timeout: 10_000 });
  const summaryHash = await page.evaluate(() => window.location.hash);
  if (summaryHash !== '#device/overview') {
    throw new Error(`the summary's Open door landed on ${JSON.stringify(summaryHash)}`);
  }
  step('the summary door opened DESKTOP-CPH4KB2 on Device → Overview');

  await page.locator('nav.rail button[title="Home"]').click();
  await page.getByRole('heading', { name: 'Home', exact: true }).waitFor({ timeout: 10_000 });

  // **THE BUILT PAGE DRAWS THE SCALES, NOT JUST THE SHEET THAT DECLARES THEM (the twenty-sixth
  // list, item 7).** Every rule below was enforced in `tokens.rs` against `app.css`'s source, and
  // three of the defects this round fixed were invisible to that gate because a literal can be
  // checked in the file while the RENDERED value comes from a var() chain, a media query or a
  // container query. So this leg reads what the browser actually paints, on Home and on Activity,
  // at 1440 and at 390, and fails if a rendered family leaves its scale.
  //
  // Measured on the live page (0.3.273-ux114) before writing this: radii `6/8/10/14/999px` on Home
  // and `6/8/999px` on Activity, two shadow shapes, seven type sizes at 10-22 px, four weights,
  // and 15-22 distinct spacing values at each width.
  const scalePass = async (label) => {
    const measured = await page.evaluate(() => {
      const main = document.querySelector('main');
      const radii = new Set();
      const shadows = new Set();
      const sizes = new Set();
      const weights = new Set();
      const spacings = new Set();
      for (const element of main.querySelectorAll('*')) {
        const style = getComputedStyle(element);
        for (const part of (style.borderRadius || '').split(' ')) {
          if (part && !part.startsWith('0')) radii.add(part);
        }
        if (style.boxShadow !== 'none') shadows.add(style.boxShadow);
        if (element.children.length === 0 && (element.textContent ?? '').trim()) {
          sizes.add(style.fontSize);
          weights.add(style.fontWeight);
        }
        for (const side of ['Top', 'Right', 'Bottom', 'Left']) {
          const value = style[`padding${side}`];
          if (value && value !== '0px' && !value.includes('calc')) spacings.add(value);
          const margin = style[`margin${side}`];
          if (margin && margin !== '0px' && !margin.includes('calc')) spacings.add(margin);
        }
        for (const prop of ['rowGap', 'columnGap']) {
          const value = style[prop];
          if (value && value !== '0px' && !value.includes('calc')) spacings.add(value);
        }
      }
      return {
        radii: [...radii].sort(),
        shadowCount: shadows.size,
        sizes: [...sizes].sort(),
        weights: [...weights].sort(),
        spacingCount: spacings.size
      };
    });
    // The five canonical radii, and nothing else: `--radius-xs/sm2/sm/radius/lg/pill`.
    const allowedRadii = new Set(['6px', '8px', '10px', '14px', '20px', '999px']);
    const strayRadii = measured.radii.filter((value) => !allowedRadii.has(value));
    if (strayRadii.length > 0) {
      throw new Error(`${label} draws radii outside the scale: ${JSON.stringify(strayRadii)}`);
    }
    // The seven-step type scale.
    const allowedSizes = new Set(['10px', '11px', '12px', '13px', '14px', '17px', '22px']);
    const straySizes = measured.sizes.filter((value) => !allowedSizes.has(value));
    if (straySizes.length > 0) {
      throw new Error(`${label} draws type sizes outside the scale: ${JSON.stringify(straySizes)}`);
    }
    // Four weights, no more.
    const allowedWeights = new Set(['400', '500', '600', '700']);
    const strayWeights = measured.weights.filter((value) => !allowedWeights.has(value));
    if (strayWeights.length > 0) {
      throw new Error(`${label} draws font weights outside the family: ${JSON.stringify(strayWeights)}`);
    }
    // One elevation family plus rings: the count is a family size, not a per-element tally.
    if (measured.shadowCount > 6) {
      throw new Error(`${label} draws ${measured.shadowCount} distinct shadows`);
    }
    return { label, ...measured };
  };

  // **THE PAGE FRAME AND THE CARD ARE THE CONTRACT'S, IN THE BROWSER (the shared layout contract).**
  // `src/layoutContract.test.ts` proves the STYLESHEET says 1120 / 24 / 24 and 14 / 16 / 8. This leg
  // proves the RENDERED page draws them — a rule the cascade overrides, an `!important` elsewhere,
  // or a box that never gets the class all pass a source-only check. Measured before this change:
  // `.page` was 1,100 px wide (a late `min(1100px, 100%)` override from the 2026-11-04 restyle) at
  // 26/28/44 px padding and 18 px gap, while `.card` was radius 10 / padding 12.
  const frame = await page.evaluate(() => {
    const page = document.querySelector('.page');
    const style = page ? getComputedStyle(page) : null;
    const card = document.querySelector('.card, .attention, .fleet-table-wrap');
    const cardStyle = card ? getComputedStyle(card) : null;
    return page && style
      ? {
          width: Math.round(page.getBoundingClientRect().width),
          padding: style.padding,
          gap: style.rowGap,
          cardRadius: cardStyle ? cardStyle.borderTopLeftRadius : null,
          cardPad: cardStyle ? cardStyle.paddingTop : null
        }
      : null;
  });
  if (frame === null) throw new Error('Home draws no .page to measure');
  if (frame.width > 1120 + 1) {
    throw new Error(`the page column drew ${frame.width} px wide, the contract says 1120`);
  }
  if (frame.padding !== '24px') {
    throw new Error(`the page's padding drew ${frame.padding}, the contract says 24px`);
  }
  if (frame.gap !== '24px') {
    throw new Error(`the page's vertical rhythm drew ${frame.gap}, the contract says 24px`);
  }
  // **HOME'S BLOCKS ARE THE PAGE'S CARDS (the shared layout contract).** Each of `attention`,
  // `fleet-table-wrap` and `installer` wrote its own border, radius and padding while `.card` existed
  // and was used only on the machine pages — four blocks, four grammars, which is what makes a page
  // read as four stacked things. They all carry `.card` now, and this leg reads their rendered
  // corners and padding rather than their source.
  // THE STRIP ARRIVES ON ITS OWN READ, so this leg waits for the page to finish drawing itself
  // rather than measuring a Home that is still assembling — the first version of this check raced the
  // installer read and reported `Home draws no .installer` on a page that was about to draw one.
  await page
    .locator('.installer.installer-strip')
    .waitFor({ timeout: 10_000 })
    .catch(() => {});
  const homeCards = await page.evaluate(() => {
    const out = [];
    for (const sel of ['.attention', '.fleet-table-wrap', '.installer']) {
      const element = document.querySelector(sel);
      if (!element) {
        out.push({ sel, missing: true });
        continue;
      }
      const style = getComputedStyle(element);
      out.push({
        sel,
        card: element.classList.contains('card'),
        radius: style.borderTopLeftRadius,
        padLeft: style.paddingLeft,
        padTop: style.paddingTop,
        // **THE STRIP IS STILL A ROW.** `.card` sets `flex-direction: column`; a child rule that
        // sets only `display: flex` keeps the column, and the title / button / sentence stack and
        // centre themselves — measured live as a 238 px block whose four children began at x=710 /
        // 749 / 939 / 1,058. Read on the same element, in the same pass, while Home is still drawn.
        gap: style.rowGap,
        direction: style.flexDirection,
        rows: new Set(
          [...element.children].map((kid) => Math.round(kid.getBoundingClientRect().y))
        ).size
      });
    }
    return out;
  });
  for (const block of homeCards) {
    if (block.missing) throw new Error(`Home draws no ${block.sel}`);
    if (!block.card) throw new Error(`Home's ${block.sel} is not on the card grammar`);
    if (block.radius !== '14px') {
      throw new Error(`Home's ${block.sel} corner is ${block.radius}, the contract says 14px`);
    }
    // **THE TABLE'S PADDING IS ZERO ON PURPOSE, AND IT IS NOT AN EXCEPTION TO THE CARD.** A table
    // is uniformly-padded content: the card's 16 px would sit inside the wrap and push the header
    // row inward while the table's own cells carry their own gutters (9 px / `--sp-4`). The rule the
    // contract states is the CARD's grammar — corner and padding — and a content block that IS the
    // card's body declares `padding: 0` deliberately, with its own comment. Everything else must be
    // the card's.
    const padded = block.sel === '.fleet-table-wrap' ? '0px' : '16px';
    if (block.padLeft !== padded || block.padTop !== padded) {
      throw new Error(
        `Home's ${block.sel} padding is ${block.padTop} ${block.padLeft}, expected ${padded}`
      );
    }
    // The installer strip is the one block whose own addition is a ROW; a card that stacks is a
    // different surface, and it happened the moment the strip stopped declaring the direction. Its
    // children MAY wrap — the stale-file sentence is long and takes the leftover width at some
    // viewports — so the assertion is the direction, which is a property of the rule, not the run.
    // **AND EVERY BLOCK'S INNER RHYTHM IS THE CARD'S (the thirty-eighth list).** Measured before this
    // leg existed: `.attention` gap 10, `.installer` gap 14, `.machine-summary` gap 10 with a
    // `14px 18px` padding — three private scales on four cards that had just been put on one grammar.
    // The outer grammar alone is not the family; the inside has to match it too.
    if (block.sel !== '.fleet-table-wrap' && block.gap !== '8px') {
      throw new Error(`Home's ${block.sel} runs a ${block.gap} inner rhythm, the card's is 8px`);
    }
    if (block.sel === '.installer' && block.direction !== 'row') {
      throw new Error(`the installer strip stacks in a ${block.direction}`);
    }
  }
  step(
    `Home's blocks are the page's cards: ${homeCards.map((b) => b.sel).join(', ')} — each a 14 px ` +
      `corner and the card's padding, on the one grammar (the table's is 0 — it is the card's body)`
  );

  step(
    `the page frame is the shared contract's: ${frame.width} px wide, ${frame.padding} padding, ` +
      `${frame.gap} rhythm` +
      (frame.cardRadius ? `, and its first block's corner is ${frame.cardRadius}` : '')
  );

  const homeScale = await scalePass('Home at 1440');
  step(
    `Home at 1440 draws the scales: radii ${homeScale.radii.join('/')}, ${homeScale.sizes.length} type ` +
      `sizes, weights ${homeScale.weights.join('/')}, ${homeScale.shadowCount} shadows, ` +
      `${homeScale.spacingCount} spacing values`
  );

  await page.locator('nav.rail button[title="Activity"]').click();
  await page.getByRole('heading', { name: 'Activity', exact: true }).waitFor({ timeout: 10_000 });
  const activityScale = await scalePass('Activity at 1440');
  step(
    `Activity at 1440 draws the scales: radii ${activityScale.radii.join('/')}, ` +
      `${activityScale.sizes.length} type sizes, ${activityScale.shadowCount} shadows`
  );
  // The phone draws the same families: the container queries change the SHAPE of a row, never the
  // scale a value is drawn from.
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(500);
  const homeScale390 = await scalePass('Home at 390');
  step(
    `Home at 390 draws the scales: radii ${homeScale390.radii.join('/')}, ` +
      `${homeScale390.sizes.length} type sizes, weights ${homeScale390.weights.join('/')}, ` +
      `${homeScale390.spacingCount} spacing values`
  );
  await page.setViewportSize({ width: 1440, height: 960 });
  await page.waitForTimeout(500);
  await page.locator('nav.rail button[title="Home"]').click();
  await page.getByRole('heading', { name: 'Home', exact: true }).waitFor({ timeout: 10_000 });

  // **THE ROW'S NAME IS A DOOR (audit-43 F1).** It used to call `choose` and leave the page on
  // #home; the press now opens the machine's own Overview.
  await page.getByRole('button', { name: /^Open DESKTOP-9VH92BJ/ }).click();
  await page.getByRole('heading', { name: 'Overview', exact: true }).waitFor({ timeout: 10_000 });
  const rowHash = await page.evaluate(() => window.location.hash);
  if (rowHash !== '#device/overview') {
    throw new Error(`the fleet row's door landed on ${JSON.stringify(rowHash)}`);
  }
  step('the fleet row opened DESKTOP-9VH92BJ on Device → Overview; the machine\'s own live page');

  // **THE DAY IS ONE LINE; THE ROWS ARE ACTIVITY'S.** The measured page ended in a 29-row copy of
  // Activity's Today card, with the same timestamps and the same `2 calls · 17 s` clusters
  // (audit-42 F1). The machine's page keeps the day's count and span and one door; the record's
  // rows are drawn in Activity only, so this page draws none of them at either width.
  await page.locator('.home-day .home-today').waitFor({ state: 'visible', timeout: 10_000 });
  const overviewRows = await page.locator('.timeline-row').count();
  if (overviewRows !== 0) {
    throw new Error(`Overview drew ${overviewRows} of Activity's rows`);
  }
  if ((await page.locator('.timeline-day-label').count()) !== 0) {
    throw new Error("Overview still draws Activity's day heading");
  }
  if ((await page.locator('.live-now .timeline-row, .live-now .live-entry').count()) !== 0) {
    throw new Error('the Now band drew call rows');
  }
  const dayLine = (await page.locator('.home-today').textContent())?.trim() ?? '';
  const todayCalls = `Today ${TODAY_CLIENT} client ${TODAY_CLIENT === 1 ? 'call' : 'calls'}`;
  if (!dayLine.startsWith(todayCalls)) {
    throw new Error(`Overview's day line read "${dayLine}", expected "${todayCalls}"`);
  }
  // The line's third fact is the span its calls cover. The fixture's whole day is in the window,
  // so the span must be stated; only a fixture whose client calls all share one instant has none.
  const clientToday = TODAY_LINES.filter((line) => line.caller === 'client');
  const fixtureSpan =
    Math.max(...clientToday.map((line) => line.ts_ms)) -
    Math.min(...clientToday.map((line) => line.ts_ms));
  if (fixtureSpan > 0 && !/ · spanning \d/.test(dayLine)) {
    throw new Error(`the day line had no span: "${dayLine}"`);
  }
  const allActivity = page.locator('.home-day').getByRole('button', { name: /All activity/ });
  if ((await allActivity.count()) !== 1) {
    throw new Error('the day line has no single All activity door');
  }
  const overviewAt = () =>
    page.evaluate(() => {
      const main = document.querySelector('main');
      const day = document.querySelector('.home-day');
      const line = document.querySelector('.home-today');
      return {
        rows: document.querySelectorAll('.timeline-row').length,
        dayHeight: day ? Math.round(day.getBoundingClientRect().height) : 0,
        lineHeight: line ? Math.round(line.getBoundingClientRect().height) : 0,
        height: main ? main.scrollHeight : 0,
        overflow:
          document.documentElement.scrollWidth - document.documentElement.clientWidth
      };
    });
  await page.setViewportSize({ width: 1440, height: 960 });
  await page.waitForTimeout(500);
  const overview1440 = await overviewAt();
  console.log(
    `browser-gate: Overview at 1440: ${overview1440.rows} rows, day line ` +
      `${overview1440.dayHeight} px, main ${overview1440.height} px`
  );
  if (overview1440.rows !== 0) {
    throw new Error(`Overview drew ${overview1440.rows} rows at 1440`);
  }
  if (overview1440.overflow > 0) {
    throw new Error(`Overview scrolls sideways at 1440 by ${overview1440.overflow} px`);
  }
  await page.screenshot({ path: path.join(OUT, 'device-overview-1440.png') });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(500);
  const overview390 = await overviewAt();
  console.log(
    `browser-gate: Overview at 390: ${overview390.rows} rows, day line ` +
      `${overview390.dayHeight} px, document ${390 + overview390.overflow}/390`
  );
  if (overview390.rows !== 0) {
    throw new Error(`Overview drew ${overview390.rows} rows at 390`);
  }
  if (overview390.overflow > 0) {
    throw new Error(`Overview scrolls sideways at 390 by ${overview390.overflow} px`);
  }
  await page.screenshot({ path: path.join(OUT, 'device-overview-390.png') });
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.waitForTimeout(200);
  step(
    `Overview is one day line ("${dayLine}") and one All activity door; it draws none of ` +
      `Activity's rows at 1440 or 390`
  );

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

  await page.getByRole('button', { name: 'Terminal', exact: true }).click();
  await page.getByRole('heading', { name: 'Terminal', exact: true }).waitFor({ timeout: 10_000 });
  step('opened Device → Terminal with a session-scoped grant');

  await page.getByRole('button', { name: '+ New session' }).click();
  const dialog = page.getByRole('dialog', { name: 'New terminal session' });
  await dialog.getByRole('button', { name: 'Open' }).click();
  await page.locator('.term-host .xterm').waitFor({ state: 'visible', timeout: 10_000 });
  step('opened one pty; xterm mounted');

  // The card reads the console's live activity. A page that never opened `/events` would make the
  // evidence leg below pass on nothing, so the stream is a precondition, not a detail.
  if (eventStreams.size === 0) throw new Error('the page never opened its activity stream');
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

  // **THE CARD JOINS ONE LINE'S WRITES.** The first write's reply was held, so the page's batcher
  // split the marker into two calls; each call published its own evidence. The card must draw the
  // line, not the newest batch — the measured 1440 card read `cho audit37-terminal` for the same
  // split. One write would prove nothing, so it is a refusal, not a pass.
  if (state.writeCalls < 2) {
    throw new Error(
      `the page sent ${state.writeCalls} write(s); the split line this leg reads was not produced`
    );
  }
  try {
    await page.waitForFunction(
      (marker) => document.querySelector('.session-card-input')?.textContent === marker,
      MARKER,
      { timeout: 5_000 }
    );
  } catch {
    const shown = await page
      .locator('.session-card-input')
      .textContent()
      .catch(() => null);
    throw new Error(
      `the session card showed ${JSON.stringify(shown)} for a line typed in ` +
        `${state.writeCalls} writes, not ${JSON.stringify(MARKER)}`
    );
  }
  step('the session card joined the line’s writes instead of showing the newest batch');
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

  // THE DAY'S SENTENCE, AND ONE CONTROL TO SEE THE AI ALONE. Activity is the audit: every caller is
  // drawn by default — all 77 rows — and the hub's console-reads chip must not appear here. Its own
  // "Clients only" control is the way to narrow. The sentence is the day's and keeps its one
  // `client calls` noun; the control's count names its own fetched window.
  const expectedCount = `Today: ${TODAY_CLIENT} client ${
    TODAY_CLIENT === 1 ? 'call' : 'calls'
  }.`;
  const countText = (await page.locator('.activity-today').textContent())?.trim();
  if (countText !== expectedCount) {
    throw new Error(`the Activity header stated "${countText}", expected "${expectedCount}"`);
  }
  if ((countText.match(/client calls?/g) ?? []).length !== 1) {
    throw new Error(`the Activity header did not keep one client-calls noun: "${countText}"`);
  }
  const filterLabels = await page.evaluate(() => ({
    machines: document.querySelector('#timeline-machines-label')?.textContent?.trim() ?? '',
    callers: document.querySelector('#timeline-callers-label')?.textContent?.trim() ?? '',
    chipText: document.querySelector('.timeline-callers button')?.textContent?.trim() ?? ''
  }));
  // **ONE LEFT EDGE PER SUBJECT, ONE BASELINE PER ROW (the thirty-fourth list, item 2).** Measured at
  // 1440 before the grid: `MACHINES` x=314, `CALLERS` x=919, the count sentence x=1001 and
  // `Search activity` x=1070 on three baselines (241, 267, 323). The bar is one grid now; this leg
  // reads the edges and fails when a subject is off its own column.
  const filterGrid = await page.evaluate(() => {
    const box = (element) => {
      if (!element) return null;
      const rect = element.getBoundingClientRect();
      return {
        x: Math.round(rect.x),
        y: Math.round(rect.y),
        w: Math.round(rect.width),
        h: Math.round(rect.height)
      };
    };
    const byId = (id) => box(document.getElementById(id));
    const search = document.querySelector('.timeline-search');
    return {
      machinesLabel: byId('timeline-machines-label'),
      machinesChips: box(document.querySelector('.timeline-machines')),
      callersLabel: byId('timeline-callers-label'),
      callersChips: box(document.querySelector('.timeline-callers')),
      count: byId('timeline-clients-window'),
      searchLabel: search ? box(search.querySelector('span')) : null,
      searchField: search ? box(search.querySelector('input')) : null
    };
  });
  // **NOTHING IN THE BAR MAY BE WIDER THAN THE BAR.** A fixed third column overflowed it by 143 px
  // on the live page (the search's right edge at 1501 against a bar ending at 1358), which the
  // label-alignment checks above could not see: every label was still on its own column.
  const barRight = await page.evaluate(() => {
    const bar = document.querySelector('.timeline-filters');
    const search = document.querySelector('.timeline-search');
    if (!bar || !search) return null;
    return {
      bar: Math.round(bar.getBoundingClientRect().right),
      search: Math.round(search.getBoundingClientRect().right),
      field: Math.round(search.querySelector('input')?.getBoundingClientRect().right ?? 0)
    };
  });
  if (barRight && (barRight.search > barRight.bar + 1 || barRight.field > barRight.bar + 1)) {
    throw new Error(`the filter bar overflows itself: ${JSON.stringify(barRight)}`);
  }
  const off = (a, b, what) => {
    if (a === null || b === null) return `${what}: missing`;
    return Math.abs(a.x - b.x) > 1 ? `${what}: ${a.x} vs ${b.x}` : null;
  };
  const misaligned = [
    off(filterGrid.machinesLabel, filterGrid.machinesChips, 'Machines label/chips'),
    off(filterGrid.callersLabel, filterGrid.callersChips, 'Callers label/chips'),
    off(filterGrid.searchLabel, filterGrid.searchField, 'Search label/field')
  ].filter(Boolean);
  if (misaligned.length !== 0) {
    throw new Error(`the Activity filter bar has edges off its columns: ${misaligned.join('; ')}`);
  }
  // The count belongs to the Callers column, not floating where the chip row ended.
  if (filterGrid.count && filterGrid.callersChips) {
    if (Math.abs(filterGrid.count.x - filterGrid.callersChips.x) > 1) {
      throw new Error(
        `the caller count starts at ${filterGrid.count.x}, its column starts at ${filterGrid.callersChips.x}`
      );
    }
    if (filterGrid.count.y < filterGrid.callersChips.y) {
      throw new Error('the caller count sits above the chip it counts');
    }
  }
  // **THE MACHINE CHIPS STAY ON ONE LINE WHEN THE BAR CAN HOLD THEM (1440).** The first grid gave
  // that column the leftover `1fr` (444 px) and the five chips — 581 px of text — wrapped, growing
  // the bar instead of tightening it. At the gate's own 1280 width they legitimately take a second
  // line, which is what a wrapping chip row is for; the width is recorded with the measurement so
  // the number is not read as a contract the page never made.
  if (filterGrid.machinesChips) {
    const width = await page.evaluate(() => window.innerWidth);
    if (width >= 1400 && filterGrid.machinesChips.h > 34) {
      throw new Error(
        `the machine chips wrap onto a second row at ${width} px (${filterGrid.machinesChips.h} px tall)`
      );
    }
  }
  // **THE TWO SUBJECTS BESIDE EACH OTHER SHARE A BASELINE; THE SEARCH IS ITS OWN ROW.** The bar is a
  // flex row of blocks, so the machine and caller labels must agree and the right-aligned search may
  // sit lower — what may not happen is a label drifting off its own content.
  const pairs = [filterGrid.machinesLabel, filterGrid.callersLabel].filter(Boolean).map((l) => l.y);
  if (pairs.length > 1 && Math.abs(pairs[0] - pairs[1]) > 1) {
    throw new Error(`the filter bar's machine and caller labels sit on ${pairs.join(' / ')}`);
  }
  step(
    `the Activity filter bar aligns each subject with its own column: labels on y=${pairs[0]}, ` +
      `count at x=${filterGrid.count?.x} under the callers, search field at x=${filterGrid.searchField?.x}`
  );

  if (filterLabels.machines !== 'Machines' || filterLabels.callers !== 'Callers') {
    throw new Error(
      `the Activity filters are not labelled: ${JSON.stringify(filterLabels)}`
    );
  }
  // The chip's own word is `Clients only`; the count beside it now names what it counted
  // (`client calls` while the chip is off), which is the point of the sentence below.
  if (filterLabels.chipText !== 'Clients only') {
    throw new Error(
      `the caller control's own label read "${filterLabels.chipText}", not "Clients only"`
    );
  }

  // **EVERY TEXT NODE ON THE PAGE MEETS AA ON THE SURFACE IT ACTUALLY SITS ON (the twenty-ninth
  // list, the contrast read).** The Rust gate measures token pairs; it cannot see WHICH token a
  // component picked, and the chip picked the wrong one — `--ink-muted` on `--panel-3` was 4.40:1,
  // five chips on this page, while the same grey passed on the page and on a card. This leg measures
  // every leaf text node on Activity at 1440 against its nearest opaque ancestor's background.
  // The chip that regressed lives in the machine filter and the caller filter, and this is where
  // those are drawn (`#timeline-machines-label` was just read above), so the scan runs here.
  const aaScan = await page.evaluate(() => {
    const parse = (colour) => {
      const match = colour.match(/rgba?\(([^)]+)\)/);
      if (!match) return null;
      const parts = match[1].split(',').map((value) => parseFloat(value));
      return { r: parts[0], g: parts[1], b: parts[2], a: parts.length > 3 ? parts[3] : 1 };
    };
    const luminance = (colour) => {
      const channel = (value) => {
        const v = value / 255;
        return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
      };
      return 0.2126 * channel(colour.r) + 0.7152 * channel(colour.g) + 0.0722 * channel(colour.b);
    };
    const ratio = (a, b) => {
      const one = luminance(a);
      const two = luminance(b);
      return (Math.max(one, two) + 0.05) / (Math.min(one, two) + 0.05);
    };
    const surfaceOf = (element) => {
      let node = element;
      while (node && node !== document.documentElement) {
        const style = getComputedStyle(node);
        const background = parse(style.backgroundColor);
        if (background && background.a > 0.9) return background;
        node = node.parentElement;
      }
      return { r: 255, g: 255, b: 255, a: 1 };
    };
    const nodes = [...document.querySelectorAll('main *')].filter((element) => {
      if (element.children.length) return false;
      if (!(element.textContent ?? '').trim()) return false;
      const box = element.getBoundingClientRect();
      return box.width > 0 && box.height > 0;
    });
    const fails = [];
    for (const element of nodes) {
      const style = getComputedStyle(element);
      // A disabled or aria-hidden node is not text a reader is asked to read.
      if (element.closest('[aria-hidden="true"]')) continue;
      const ink = parse(style.color);
      const wash = surfaceOf(element);
      if (!ink || !wash) continue;
      const size = parseFloat(style.fontSize);
      const bold = parseInt(style.fontWeight, 10) >= 700;
      const needs = size >= 18.66 || (size >= 14 && bold) ? 3 : 4.5;
      const measured = ratio(ink, wash);
      if (measured < needs) {
        fails.push({
          text: (element.textContent ?? '').trim().slice(0, 30),
          size: Math.round(size),
          colour: style.color,
          wash: `rgb(${Math.round(wash.r)},${Math.round(wash.g)},${Math.round(wash.b)})`,
          measured: Math.round(measured * 10) / 10,
          needs
        });
      }
    }
    return { measured: nodes.length, fails: fails.slice(0, 6), total: fails.length };
  });
  if (aaScan.total !== 0) {
    throw new Error(`text on Activity fails AA: ${JSON.stringify(aaScan)}`);
  }
  step(
    `every text node on Activity meets AA on the surface it sits on (${aaScan.measured} nodes measured)`
  );

  const filterCount = page.locator('#timeline-clients-window');
  // The count's first render can be the live rows alone: the fetched window arrives on its own
  // read, and a one-line window has no span to state. Wait for the whole record so this assertion
  // measures the window the page ends with, not the empty list it started from.
  // **THE SPAN IS WITHHELD WHEN THE LIST IS NARROWED (the thirty-seventh list).** A previous leg
  // selects a machine (or types a search), and the count then names the set it counted instead of
  // joining a filtered number to the unfiltered window's span. So the wait is for a NUMBER, not for
  // 'spanning': the count is ready when it states one, and the span is asserted below only when the
  // bar is unfiltered.
  await page.waitForFunction(
    () =>
      /\d+ (client call|client calls|row|rows)/.test(
        document.querySelector('#timeline-clients-window')?.textContent ?? ''
      ),
    undefined,
    { timeout: 10_000 }
  );
  const filterCountText = (await filterCount.textContent())?.trim() ?? '';
  // **THE COUNT SAYS WHICH NOUN IT COUNTED.** The measured timeline put `353 rows in this window`
  // over a list drawing 891 rows: the number is the client-call subset, not the window's size. The
  // span's exact value depends on live rows this run adds, so the gate proves the noun and the
  // duration; the unit suite pins the arithmetic.
  // **THE SENTENCE STATES ONE SET (the thirty-seventh list).** A previous leg selects a machine, so
  // this count is narrowed: it names the machine it counted and it withholds the span, because the
  // span belongs to the whole read. Both forms are accepted here — the unfiltered one keeps its
  // span, the narrowed one names its scope — and what is refused is a number joined to a span that
  // describes a different set.
  const allClientNoun = ALL_CLIENT === 1 ? 'client call' : 'client calls';
  const countForms = [
    new RegExp(`^${ALL_CLIENT} ${allClientNoun} in this window · spanning \\d+ (s|m|h)( \\d+ (s|m))?$`),
    new RegExp(`^${ALL_CLIENT} ${allClientNoun} on \\S+ in this window$`),
    // A live run may narrow by MACHINE while the count is still the whole window's; both are one set.
    new RegExp(`^${ALL_CLIENT} ${allClientNoun} (on \\S+ )?matching the search in this window$`)
  ];
  if (!countForms.some((form) => form.test(filterCountText))) {
    throw new Error(
      `the Clients only control stated "${filterCountText}", expected its count and its OWN set ` +
        `(either "… in this window · spanning <span>" or "… on <machine> in this window")`
    );
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
  // **A DAY IS SIXTY ROWS AND ONE DOOR (backlog item 2).** The fixture is one day of 79 rows, so the
  // page draws the newest 60 and the door states the rest; pressing it draws the whole day.
  // **THE DAY'S RECORD IS A LIST, NOT A STACK OF 118 BOXES (the twenty-seventh list, item 2).**
  // Measured before this leg existed: Activity's Timeline at 1440 drew 150 bordered elements for
  // 8,270 characters, 118 of them `.timeline-row` border-tops and 25 `.timeline-caller` pills, so
  // every call was boxed off from the one above it. The rows are lines of one list now, and the
  // caller is a small-caps word rather than a pill — this leg fails if either comes back.
  const timelineBorders = await page.evaluate(() => {
    const main = document.querySelector('main');
    let bordered = 0;
    const byClass = {};
    for (const element of main.querySelectorAll('*')) {
      const style = getComputedStyle(element);
      const any = ['Top', 'Right', 'Bottom', 'Left'].some(
        (side) =>
          (parseFloat(style[`border${side}Width`]) || 0) > 0 &&
          style[`border${side}Style`] !== 'none'
      );
      if (!any) continue;
      bordered += 1;
      const key =
        typeof element.className === 'string' && element.className
          ? element.className.split(' ')[0]
          : element.tagName.toLowerCase();
      byClass[key] = (byClass[key] ?? 0) + 1;
    }
    return {
      bordered,
      chars: (main.innerText ?? '').length,
      rowRules: byClass['timeline-row'] ?? 0,
      callerPills: [...document.querySelectorAll('.timeline-caller')].filter((element) => {
        const style = getComputedStyle(element);
        return ['Top', 'Right', 'Bottom', 'Left'].some(
          (side) =>
            (parseFloat(style[`border${side}Width`]) || 0) > 0 &&
            style[`border${side}Style`] !== 'none'
        );
      }).length
    };
  });
  if (timelineBorders.rowRules !== 0) {
    throw new Error(
      `the timeline draws ${timelineBorders.rowRules} rules between its rows; the day is one list`
    );
  }
  if (timelineBorders.callerPills !== 0) {
    throw new Error(
      `the timeline draws ${timelineBorders.callerPills} caller pills; the caller is a word`
    );
  }
  step(
    `the timeline is one list: ${timelineBorders.bordered} bordered elements for ` +
      `${timelineBorders.chars} chars, no row rules, no caller pills`
  );

  const drawnRows = await page.locator('.timeline-row').count();
  const dayDoors = page.locator('.timeline-more');
  if ((await dayDoors.count()) !== 1) {
    throw new Error(`Activity drew ${await dayDoors.count()} day doors for one long day`);
  }
  const doorText = (await dayDoors.textContent())?.trim() ?? '';
  if (!/^Show the rest of today \(\d+ more\)/.test(doorText)) {
    throw new Error(`the day door read ${JSON.stringify(doorText)}`);
  }
  if (drawnRows !== 60) {
    throw new Error(`Activity drew ${drawnRows} rows before the fold, not the day's 60`);
  }
  await dayDoors.click();
  if (await page.locator('.timeline-more').count()) {
    throw new Error('the day door stayed after the day was opened');
  }
  const allActivityRows = await page.locator('.timeline-row').count();
  // **THE PAGE'S OWN WRITES ARE AUDIT ROWS.** The console records every call, so the two
  // `terminal.write` lines the session-card leg typed are drawn beside the fetched window. The
  // fixture is that window; the live rows this run added are the difference.
  const expectedActivityRows = FIXTURE_LINES.length + state.writeCalls;
  if (allActivityRows !== expectedActivityRows) {
    throw new Error(
      `Activity drew ${allActivityRows} rows; the audit's default is all ${expectedActivityRows}`
    );
  }
  if ((await page.locator('.timeline-console-reads').count()) !== 0) {
    throw new Error('Activity drew the hub\'s console-reads chip');
  }
  const clientsChip = page.getByRole('button', { name: 'Clients only' });
  if ((await clientsChip.getAttribute('aria-pressed')) !== 'false') {
    throw new Error('the Clients only control was pressed before it was chosen');
  }
  if ((await clientsChip.getAttribute('aria-describedby')) !== 'timeline-clients-window') {
    throw new Error('the Clients only control does not point at its window count');
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
  // THE HEADER DOES NOT FOLLOW THE CONTROL. It is the day's sentence; the control's own count is
  // the fetched window's subset, labelled as such. A second `client calls` number here is the
  // nineteenth item's defect, so the sentence must be unchanged while the rows narrow.
  const narrowedCount = (await page.locator('.activity-today').textContent())?.trim();
  if (narrowedCount !== expectedCount) {
    throw new Error(
      `the Clients only control changed the day's sentence to "${narrowedCount}"`
    );
  }
  const narrowedCountLabel = (await filterCount.textContent())?.trim() ?? '';
  // Pressed, the subset is the list, so the same number and window are named `rows`. **AND THE SET IS
  // STILL THE ONE IT NAMES (the thirty-seventh list):** this leg runs with a machine selected, so the
  // sentence names that machine and withholds the whole read's span.
  const pressedForms = [
    new RegExp(`^${ALL_CLIENT} rows in this window · spanning \\d+ (s|m|h)( \\d+ (s|m))?$`),
    new RegExp(`^${ALL_CLIENT} rows on \\S+ in this window$`)
  ];
  if (!pressedForms.some((form) => form.test(narrowedCountLabel))) {
    throw new Error(
      `the Clients only control lost its window count while pressed: "${narrowedCountLabel}"`
    );
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
  // THE RECORD'S SHAPE, MEASURED. The day is a heading, the verb is the scale's 14 px, the machine
  // and age are on a second line, and the technical hints are the footer after the timeline. The
  // page is not Home's card: no installer, no verdict block. The sizes are the seven-step scale's
  // tokens (`--fs-lg` 17, `--fs-xl` 22, `--fs-md` 14), not raw pixels (312da21).
  const activityDesign = await page.evaluate(() => {
    const sentence = document.querySelector('.activity-today');
    const day = document.querySelector('.timeline-day-label');
    const verb = document.querySelector('.timeline-sentence');
    const row = document.querySelector('.timeline-row');
    const timeline = document.querySelector('.timeline');
    const footer = document.querySelector('.timeline-footer');
    const facts = document.querySelector('.timeline-facts');
    const age = document.querySelector('.timeline-age');
    const machine = document.querySelector('.timeline-machine');
    const clock = document.querySelector('.timeline-row .timeline-time');
    const box = (el) => (el ? el.getBoundingClientRect() : null);
    const size = (el) => (el ? parseFloat(getComputedStyle(el).fontSize) : 0);
    return {
      sentence: size(sentence),
      day: size(day),
      verb: size(verb),
      rowHeight: box(row)?.height ?? 0,
      clockText: clock?.textContent?.trim() ?? '',
      clockTitle: clock?.getAttribute('title') ?? '',
      clockDateTime: clock?.getAttribute('datetime') ?? '',
      ageSize: size(age),
      ageDisplay: age ? getComputedStyle(age).display : '',
      machineAgeOnSecondLine:
        Boolean(facts) &&
        Boolean(age) &&
        Boolean(machine) &&
        (box(age)?.top ?? 0) > (box(verb)?.top ?? 0),
      footerAfterTimeline:
        Boolean(timeline) &&
        Boolean(footer) &&
        Boolean(timeline.compareDocumentPosition(footer) & Node.DOCUMENT_POSITION_FOLLOWING),
      footerText: footer?.textContent?.trim() ?? '',
      installer: document.querySelectorAll('.installer').length,
      verdict: document.querySelectorAll('.home-verdict, .attention').length
    };
  });
  if (activityDesign.sentence !== 17) {
    throw new Error(`the day's sentence is ${activityDesign.sentence}px, not the scale's 17px`);
  }
  if (activityDesign.day !== 22) {
    throw new Error(`the day heading is ${activityDesign.day}px, not the scale's 22px`);
  }
  if (activityDesign.verb !== 14) {
    throw new Error(`the row's verb is ${activityDesign.verb}px, not the scale's 14px`);
  }
  if (activityDesign.rowHeight < 60) {
    throw new Error(`the row is ${activityDesign.rowHeight}px tall, not a record row`);
  }
  // The Timeline's clock is the same element as the Table's and the Runs history's: `HH:MM:SS`,
  // the UTC instant in `dateTime`, the exact local moment in the title (audit-43 F8).
  if (!/^\d{2}:\d{2}:\d{2}$/.test(activityDesign.clockText)) {
    throw new Error(`the Timeline's clock reads ${JSON.stringify(activityDesign.clockText)}`);
  }
  if (!/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} \(/.test(activityDesign.clockTitle)) {
    throw new Error(`the Timeline's clock title is ${JSON.stringify(activityDesign.clockTitle)}`);
  }
  if (!activityDesign.clockDateTime || Number.isNaN(Date.parse(activityDesign.clockDateTime))) {
    throw new Error(`the Timeline's clock carries ${JSON.stringify(activityDesign.clockDateTime)}`);
  }
  if (!activityDesign.machineAgeOnSecondLine) {
    throw new Error('the machine and age are not on the row\'s second line');
  }
  if (!activityDesign.footerAfterTimeline) {
    throw new Error('the window/source hints are not after the timeline');
  }
  if (!activityDesign.footerText.includes('Reading audit.jsonl')) {
    throw new Error(`the footer does not carry the source line: "${activityDesign.footerText}"`);
  }
  if (activityDesign.installer !== 0 || activityDesign.verdict !== 0) {
    throw new Error(
      `Activity drew Home's block: ${activityDesign.installer} installer(s), ${activityDesign.verdict} verdict(s)`
    );
  }
  step(
    `Activity's sentence is ${activityDesign.sentence}px, its day ${activityDesign.day}px, ` +
      `its verbs ${activityDesign.verb}px, its rows ${Math.round(activityDesign.rowHeight)}px; ` +
      `the hints are below the timeline and Home's block is absent`
  );

  // The phone keeps the record: no sideways scroll, the age still on the second line, the verb at
  // the same scale token (14 px).
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(300);
  const activity390 = await page.evaluate(() => {
    const rail = document.querySelector('.rail')?.getBoundingClientRect() ?? null;
    const firstRow = document.querySelector('.timeline-row')?.getBoundingClientRect() ?? null;
    const clock = document.querySelector('.timeline-row .timeline-time');
    const seconds = document.querySelector('.timeline-row .timeline-clock-seconds');
    const secondsBox = seconds?.getBoundingClientRect();
    return {
      viewport: document.documentElement.clientWidth,
      scrollWidth: document.documentElement.scrollWidth,
      verb: parseFloat(getComputedStyle(document.querySelector('.timeline-sentence')).fontSize),
      clockText: clock?.textContent?.trim() ?? '',
      clockTitle: clock?.getAttribute('title') ?? '',
      secondsDisplay: seconds ? getComputedStyle(seconds).display : null,
      secondsWidth: Math.round(secondsBox?.width ?? 0),
      age: getComputedStyle(document.querySelector('.timeline-age')).display,
      oldChips: document.querySelectorAll('.timeline-machines .chip.is-old').length,
      machineChips: document.querySelectorAll('.timeline-machines .chip').length,
      firstRowTop: firstRow ? Math.round(firstRow.top) : null,
      railTop: rail ? Math.round(rail.top) : null
    };
  });
  if (activity390.scrollWidth > activity390.viewport) {
    throw new Error(
      `Activity at 390 scrolls sideways: ${activity390.viewport}/${activity390.scrollWidth}`
    );
  }
  // **THE RETIRED IDS DO NOT SPEND THE PHONE'S FIRST SCREEN (audit-40 F5).** The two dated
  // `old id` chips are already options in the Table's Device filter; at 390 they took two full rows
  // above the record. The first record must start above the bottom rail.
  if (activity390.oldChips !== 0) {
    throw new Error(`Activity kept ${activity390.oldChips} retired-id chip(s) at 390`);
  }
  if (activity390.firstRowTop === null || activity390.railTop === null) {
    throw new Error('Activity at 390 drew no record or no rail to measure against');
  }
  if (activity390.firstRowTop >= activity390.railTop) {
    throw new Error(
      `Activity's first record starts at y=${activity390.firstRowTop}, below the rail at ` +
        `y=${activity390.railTop} — the filter chrome spent the first screen`
    );
  }
  if (activity390.verb !== 14) {
    throw new Error(
      `Activity at 390 dropped its verb to ${activity390.verb}px, not the scale's 14px`
    );
  }
  // **THE PHONE KEEPS THE SECONDS (audit-43 F8).** The measured 390 clock read `08:14` because a
  // stylesheet hid the seconds span; a minute cannot separate two calls eight seconds apart. The
  // whole `HH:MM:SS` is drawn now, with the exact instant in the title.
  if (!/^\d{2}:\d{2}:\d{2}$/.test(activity390.clockText)) {
    throw new Error(`Activity at 390's clock reads ${JSON.stringify(activity390.clockText)}`);
  }
  if (activity390.secondsDisplay === 'none' || activity390.secondsWidth === 0) {
    throw new Error(
      `Activity at 390 hid the seconds: display ${activity390.secondsDisplay}, ` +
        `${activity390.secondsWidth} px`
    );
  }
  if (!/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} \(/.test(activity390.clockTitle)) {
    throw new Error(`Activity at 390's clock title is ${JSON.stringify(activity390.clockTitle)}`);
  }
  if (activity390.age === 'none') {
    throw new Error('Activity at 390 hid the age from the second line');
  }
  await page.setViewportSize({ width: 1440, height: 960 });
  await page.waitForTimeout(300);
  step(
    `Activity at 390 has no sideways scroll (${activity390.viewport}/${activity390.scrollWidth}), ` +
      `keeps the 14px verb and the age, draws ${activity390.machineChips} live machine chips and no ` +
      `retired id, and its first record starts at y=${activity390.firstRowTop} above the rail at ` +
      `y=${activity390.railTop}`
  );

  // **A COUNT'S QUALIFIER MUST BE ITS OWN SET (the thirty-seventh list).** Measured on the live page
  // after typing a search that matched nothing: `0 client calls in this window · spanning 10 h 50 m`
  // — the `0` was the SEARCH-FILTERED count and the span was the UNFILTERED window's, joined by a `·`
  // that read as "the span of the zero", under a head that still said `Today: 876 client calls`. This
  // leg types a no-match search and refuses a count that carries the read's span while the list is
  // narrowed, or that fails to say which set it counted.
  await page.locator('.timeline-search input').fill('zzzz-no-such-call');
  await page.waitForTimeout(600);
  const narrowedNumbers = await page.evaluate(() => {
    const count = document.querySelector('#timeline-clients-window')?.textContent ?? '';
    const empty = [...document.querySelectorAll('main p, main span')]
      .map((element) => (element.textContent ?? '').replace(/\s+/g, ' ').trim())
      .find((text) => /No activity matches/.test(text)) ?? null;
    const head = document.querySelector('.activity-today')?.textContent?.trim() ?? '';
    return { count: count.replace(/\s+/g, ' ').trim(), empty, head, clear: !!document.querySelector('button') };
  });
  if (narrowedNumbers.empty === null) {
    throw new Error('a search that matches nothing draws no empty state');
  }
  if (/ \u00b7 spanning /.test(narrowedNumbers.count)) {
    throw new Error(
      `the narrowed count still carries the whole read's span: "${narrowedNumbers.count}"`
    );
  }
  if (!/matching the search/.test(narrowedNumbers.count)) {
    throw new Error(`the narrowed count does not name its set: "${narrowedNumbers.count}"`);
  }
  step(
    `a no-match search states its own set: "${narrowedNumbers.count}" beside ` +
      `"${narrowedNumbers.empty}", with the day still "${narrowedNumbers.head}"`
  );
  await page.locator('.timeline-search input').fill('');
  await page.waitForTimeout(600);

  step(
    `the header counted ${TODAY_CLIENT} client calls, Activity kept all ${allActivityRows} rows ` +
      `and Clients only narrowed them to ${ALL_CLIENT}`
  );

  // Activity opens on the timeline; the audit rows are the table view.
  await page.getByRole('tab', { name: 'Table' }).click();
  const auditRow = page.locator('.audit-row').filter({ hasText: 'runs.exec' }).first();
  await auditRow.waitFor({ timeout: 10_000 });

  // **ONE FILTER ROW, ONE EXPORT, AND THE TIMELINE'S CLOCK (audit-41 F4).** The measured 1440 card
  // wrapped eight controls into two rows with `Export JSONL` in the first and `Export CSV` in the
  // second, and its When column said `8s ago` for the line the Timeline dated `04:45:03`. The row
  // must be one band (the measured two-row card was 119 px), the export one closed control with
  // both formats inside it, and the cell a real `<time>` with the clock and the age.
  const audit1440 = await page.evaluate(() => {
    const visible = (element) => {
      if (element.getClientRects().length === 0) return false;
      if (typeof element.checkVisibility !== 'function') return true;
      return element.checkVisibility({ checkVisibilityCSS: true, contentVisibilityAuto: true });
    };
    const toolbar = document.querySelector('.audit-toolbar');
    if (!toolbar) return null;
    const controls = [
      ...toolbar.querySelectorAll('.audit-field, .seg, .audit-export-summary')
    ].filter(visible);
    const menus = toolbar.querySelectorAll('details.audit-export-menu');
    const menu = menus[0] ?? null;
    const when = document.querySelector('tr.audit-row td.audit-when');
    const time = when?.querySelector('time') ?? null;
    return {
      toolbarHeight: Math.round(toolbar.getBoundingClientRect().height),
      controls: controls.length,
      controlTops: [...new Set(controls.map((entry) => Math.round(entry.getBoundingClientRect().top)))],
      menus: menus.length,
      menuClosed: menu ? !menu.open : null,
      formats: menu ? menu.querySelectorAll('button').length : 0,
      outsideExport: [...toolbar.querySelectorAll('button')].filter(
        (button) => !button.closest('.audit-export-menu') && /^Export/.test(button.textContent ?? '')
      ).length,
      whenText: when?.textContent?.trim() ?? '',
      timeText: time?.textContent?.trim() ?? '',
      timeTitle: time?.getAttribute('title') ?? null,
      dateTime: time?.getAttribute('datetime') ?? null
    };
  });
  console.log(`browser-gate: audit at 1440: ${JSON.stringify(audit1440)}`);
  if (!audit1440) throw new Error('the Table drew no filter row at 1440');
  if (audit1440.toolbarHeight > 70) {
    throw new Error(
      `the 1440 filter row is ${audit1440.toolbarHeight} px tall; the measured two-row card was ` +
        '119 px and one band is one row'
    );
  }
  if (audit1440.menus !== 1) {
    throw new Error(`the 1440 toolbar drew ${audit1440.menus} Export controls, not one`);
  }
  if (audit1440.outsideExport !== 0) {
    throw new Error(
      `the 1440 toolbar drew ${audit1440.outsideExport} export button(s) outside the one control`
    );
  }
  if (audit1440.menuClosed !== true || audit1440.formats !== 2) {
    throw new Error(
      `the 1440 Export menu is closed=${audit1440.menuClosed} with ${audit1440.formats} formats`
    );
  }
  if (!/^\d{2}:\d{2}:\d{2}$/.test(audit1440.timeText)) {
    throw new Error(`the When cell's time reads ${JSON.stringify(audit1440.timeText)}`);
  }
  if (!/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} \(/.test(audit1440.timeTitle ?? '')) {
    throw new Error(
      `the When cell's time carries no exact moment: ${JSON.stringify(audit1440.timeTitle)}`
    );
  }
  if (!audit1440.dateTime || Number.isNaN(Date.parse(audit1440.dateTime))) {
    throw new Error(`the When cell's time carries ${JSON.stringify(audit1440.dateTime)}`);
  }
  // **ONE CLOCK, ONE SPACING (audit-43 F8).** The measured cell read `08:46:18· 8s ago`: the dot
  // was glued to the seconds and the only gap was a margin. The text itself must say the age beside
  // the clock with one space, so a copy, a screen reader or a phone reads the same join.
  if (!(audit1440.whenText ?? '').startsWith(`${audit1440.timeText} · `)) {
    throw new Error(`the When cell glued its age to the clock: ${JSON.stringify(audit1440.whenText)}`);
  }
  if (!/ · .+ (ago|just now)$/.test(audit1440.whenText ?? '')) {
    throw new Error(`the When cell lost the age: ${JSON.stringify(audit1440.whenText)}`);
  }
  if (/\d·/.test(audit1440.whenText ?? '')) {
    throw new Error(`the When cell has no space before the separator: ${JSON.stringify(audit1440.whenText)}`);
  }
  step(
    `at 1440 the filters are one ${audit1440.toolbarHeight} px row with one closed Export menu ` +
      `(${audit1440.formats} formats, none outside it); When is ${audit1440.timeText} with ` +
      `"${audit1440.whenText.replace(audit1440.timeText, '').trim()}" beside it, the exact ` +
      `instant in its title`
  );

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
      documentScrollWidth: document.documentElement.scrollWidth,
      started: (() => {
        const cell = row?.querySelectorAll('td.runs-fact')[2];
        const time = cell?.querySelector('time');
        const box = time?.getBoundingClientRect();
        const age = cell?.querySelector('.hint');
        const ageBox = age?.getBoundingClientRect();
        const value = cell?.querySelector('.runs-started');
        return {
          text: time?.textContent?.trim() ?? null,
          title: time?.getAttribute('title') ?? null,
          datetime: time?.getAttribute('datetime') ?? null,
          visible: time !== null && time !== undefined && (box?.width ?? 0) > 0,
          cellText: cell?.textContent?.trim() ?? null,
          valueText: value?.textContent?.trim() ?? null,
          valueChildren: value?.children.length ?? 0,
          ageHeight: Math.round(ageBox?.height ?? 0)
        };
      })()
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
  // **A RUN IS PLACED IN TIME ON THE PAGE THAT INSPECTS IT.** The history said `31m 11s ago` with
  // no title; the Table dates the same class of run with a clock + age and the instant in a title.
  if (
    !runs1440.started.visible ||
    !/^\d{2}:\d{2}:\d{2}$/.test(runs1440.started.text ?? '') ||
    !/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} \(/.test(runs1440.started.title ?? '') ||
    !/\.\d{3}Z$/.test(runs1440.started.datetime ?? '')
  ) {
    throw new Error(`the 1440 Runs Started reads ${JSON.stringify(runs1440.started)}`);
  }
  // **THE 1440 CELL KEEPS THE AGE ON THE CLOCK'S LINE (audit-43 F8).** The measured cell was
  // 140 px wide and the 107 px age wrapped, stranding `ago`; the value is one nowrap wrapper and
  // the join is one literal space on each side of the dot.
  if (
    runs1440.started.valueChildren !== 2 ||
    !/^\d{2}:\d{2}:\d{2} · .+ ago$/.test(runs1440.started.valueText ?? '') ||
    !(runs1440.started.valueText ?? '').startsWith(`${runs1440.started.text} · `) ||
    /\d·/.test(runs1440.started.valueText ?? '') ||
    runs1440.started.ageHeight > 24
  ) {
    throw new Error(`the 1440 Runs Started wraps or glues: ${JSON.stringify(runs1440.started)}`);
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
        ),
        started: (() => {
          const cell = row.querySelectorAll('td.runs-fact')[2];
          const label = cell?.querySelector('.runs-fact-label');
          const time = cell?.querySelector('time');
          const age = cell?.querySelector('.hint');
          const value = cell?.querySelector('.runs-started');
          const labelBox = label?.getBoundingClientRect();
          const timeBox = time?.getBoundingClientRect();
          const ageBox = age?.getBoundingClientRect();
          return {
            clock: time?.textContent?.trim() ?? null,
            cellText: cell?.textContent?.trim() ?? null,
            valueText: value?.textContent?.trim() ?? null,
            valueChildren: value?.children.length ?? 0,
            labelRight: Math.round(labelBox?.right ?? 0),
            timeLeft: Math.round(timeBox?.left ?? 0),
            ageLeft: Math.round(ageBox?.left ?? 0),
            timeTop: Math.round(timeBox?.top ?? 0),
            ageTop: Math.round(ageBox?.top ?? 0),
            ageHeight: Math.round(ageBox?.height ?? 0)
          };
        })()
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
    // **THE AGE IS IN THE VALUE CELL, NOT THE LABEL'S COLUMN (audit-43 F8).** The measured 390
    // card put the hint at x=48, under `Started` at x=160, beginning the line with a dangling `·`
    // because it was the `<td>` grid's third child. The wrapper gives the grid a label and one
    // value; the age must sit right of the label and on the clock's own line.
    if (!/^\d{2}:\d{2}:\d{2}$/.test(card.started.clock ?? '')) {
      throw new Error(`the 390 card's Started reads ${JSON.stringify(card.started.clock)}`);
    }
    if (
      card.started.valueChildren !== 2 ||
      !/^\d{2}:\d{2}:\d{2} · .+ ago$/.test(card.started.valueText ?? '') ||
      !(card.started.valueText ?? '').startsWith(`${card.started.clock} · `) ||
      /\d·/.test(card.started.valueText ?? '') ||
      card.started.ageLeft <= card.started.labelRight ||
      Math.abs(card.started.ageTop - card.started.timeTop) > 6 ||
      card.started.ageHeight > 24
    ) {
      throw new Error(`the 390 card's Started is not one value cell: ${JSON.stringify(card.started)}`);
    }
    if (card.started.ageLeft <= card.started.timeLeft) {
      throw new Error(`the 390 card's age is not beside its clock: ${JSON.stringify(card.started)}`);
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
  // **THE CATALOGUE'S SENTENCES ARE READABLE (the twenty-eighth list, item 2).** Measured before
  // this leg: 12 of 46 descriptions were truncated at 1440 and 44 of 46 at 390, because the row cut
  // them to one line with an ellipsis and offered no way to read the rest — 5,936 px of text lost at
  // 1440 and 22,898 px at 390. The row now clamps to two lines, so the gate counts how many are
  // still cut SHORTER than their text and fails if the one-line shape returns.
  const catalogueClamp = async (width) => {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 900 });
    await page.waitForTimeout(400);
    return page.evaluate(() => {
      const folded = [...document.querySelectorAll('.tool-description')];
      const cutToOne = folded.filter(
        (element) => element.scrollHeight > element.clientHeight + 2 && element.clientHeight <= 22
      );
      const clampedToTwo = folded.filter((element) => element.clientHeight > 22);
      return {
        width: window.innerWidth,
        descriptions: folded.length,
        cutToOne: cutToOne.length,
        clampedToTwo: clampedToTwo.length,
        tallest: folded.reduce((max, element) => Math.max(max, element.clientHeight), 0)
      };
    });
  };
  const clamp1280 = await catalogueClamp(1280);
  const clamp390 = await catalogueClamp(390);
  if (clamp1280.cutToOne !== 0 || clamp390.cutToOne !== 0) {
    throw new Error(
      `a description is still cut to one line: ${JSON.stringify({ clamp1280, clamp390 })}`
    );
  }
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.waitForTimeout(300);
  // The gate's own catalogue has seven short descriptions, so this leg proves the SHAPE (nothing is
  // cut to one line) rather than the count; the count is measured on the operator's 46-tool page in
  // the live check, where the long sentences actually live.
  step(
    `the catalogue's descriptions are never cut to one line: ${clamp1280.descriptions} rows at ` +
      `1280, ${clamp390.descriptions} at 390, tallest ${clamp1280.tallest}/${clamp390.tallest}px`
  );

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
  // once; the measured `build 0.3.42 · online` here was a third copy. The status line is labelled
  // now, and the update card is where the update story is written.
  await page.getByRole('button', { name: 'Machine', exact: true }).click();
  await page.getByRole('heading', { name: 'Machine', exact: true }).waitFor({ timeout: 10_000 });
  // **THE PROCESS TABLE'S MOST REPEATED CONTROL IS NOT "KILL" (the twenty-eighth list, item 3).**
  // Measured on the operator's machine: 235 rows, **222 `Kill` buttons** at 33 x 28 px, so the
  // Machine page's commonest control was the destructive one and it was under the phone's 44 px
  // target. A row offers its `⋯` menu now, and Kill lives inside it.
  // The list is a device CALL, not a render: wait for the table before measuring it, or the leg
  // reads an empty page and calls that a missing menu (measured: it passed three times in a row and
  // then failed once under load).
  await page
    .locator('.system-table tbody tr td.system-pid')
    .first()
    .waitFor({ timeout: 10_000 });
  const processKills = await page.evaluate(() => {
    const rows = [...document.querySelectorAll('tbody tr')].filter((row) =>
      row.querySelector('.system-pid')
    );
    const killCells = [...document.querySelectorAll('td.system-kill-cell')];
    // A cell's Kill button lives INSIDE a closed `<details>`: jsdom and Chrome both keep it in the
    // DOM, so the measurement is "is this button a bare one the row draws", not "does it exist".
    const bareKill = killCells.filter((cell) => {
      const button = cell.querySelector('button');
      return button !== null && button.closest('details.row-menu') === null;
    }).length;
    const menus = killCells.filter((cell) => cell.querySelector('details.row-menu')).length;
    const protectedCells = killCells.filter(
      (cell) => (cell.textContent ?? '').trim() === 'Protected'
    ).length;
    return { rows: rows.length, bareKill, menus, protectedCells };
  });
  if (processKills.bareKill !== 0) {
    throw new Error(
      `the process table draws ${processKills.bareKill} bare Kill buttons; the action is a row menu`
    );
  }
  if (processKills.menus === 0) {
    throw new Error('the process table draws no row menu for its killable processes');
  }
  step(
    `the process table offers ${processKills.menus} row menus and ${processKills.protectedCells} ` +
      `protected rows, with ${processKills.bareKill} bare Kill buttons`
  );

  const machinePageText = await page.locator('.system-page').innerText();
  // THE OLD SENTENCE, NOT ANY "BUILD". `build 0.3.42 · online` was the repeated status; the repair
  // task row below names the build as the one that did not report a fact (audit-42 F7), which is a
  // different claim and is asserted on its own row.
  if (/build \d+\.\d+\.\d+/.test(machinePageText)) {
    throw new Error('the Machine tab repeated the build the header already shows');
  }
  const statusLine = (await page.locator('.system-status').innerText()).trim();
  if (statusLine !== 'Status · online') {
    throw new Error(`the Machine tab's status line read "${statusLine}"`);
  }
  // **AN ABSENT REPAIR TASK IS NOT AN ABSENT TASK (audit-42 F7).** The fixture's online machine
  // carries no `repair_task` fact — the measured 84T on 0.3.291 shape — so the row must say the
  // build did not report it, name the running version, and never say `not registered` for an
  // answer the console did not read.
  const repairTaskRow = await page.evaluate(() => {
    const row = [...document.querySelectorAll('.facts.host > div')].find(
      (entry) => entry.querySelector('dt')?.textContent?.trim() === 'Repair task'
    );
    if (!row) return null;
    const value = row.querySelector('dd');
    return { text: value?.textContent?.trim() ?? '', muted: value?.classList.contains('muted') ?? false };
  });
  if (!repairTaskRow) {
    throw new Error('the Machine tab drew no Repair task row for an online machine without the fact');
  }
  if (repairTaskRow.text !== 'not reported by this build (v0.3.42)') {
    throw new Error(`the Machine tab's Repair task row read ${JSON.stringify(repairTaskRow.text)}`);
  }
  if (!repairTaskRow.muted) {
    throw new Error('the Machine tab did not mark the absent Repair task row as absent');
  }
  step(`the Repair task row says "${repairTaskRow.text}" for the fixture's online build`);
  // **THE MACHINE PAGE NAMES WHAT THE MACHINE IS (audit-44 F8).** The frame's OS, architecture and
  // core count are one line above Memory; the measured page started at Memory and never said.
  const platformRow = await page.evaluate(() => {
    const row = [...document.querySelectorAll('.facts.host > div')].find(
      (entry) => entry.querySelector('dt')?.textContent?.trim() === 'Platform'
    );
    return row?.querySelector('dd')?.textContent?.trim() ?? null;
  });
  if (platformRow !== 'Windows 11 · x86_64 · 12 cores') {
    throw new Error(`the Machine tab's platform line read ${JSON.stringify(platformRow)}`);
  }
  step(`the Machine tab names the platform: "${platformRow}"`);
  // **THE DEVICE'S OWN UPDATE STORY IS THE PANEL'S GRID, NOT A GREY PARAGRAPH (audit-40 F3).** The
  // card states one fact per labelled row — the panel's `Last target` / `Last outcome` / `Last
  // result` / `Last check` vocabulary — and the running row carries the release comparison. The
  // fixture's applied answer is a refusal to 0.3.43, so the applied version is not what runs and
  // the grid leads with `Running`; the target is the refused one.
  const updateRows = await page.evaluate(() => {
    const grid = document.querySelector('.system-update dl');
    if (!grid) return null;
    return Array.from(grid.querySelectorAll(':scope > div')).map((row) => ({
      label: row.querySelector('dt')?.textContent?.trim() ?? '',
      value: row.querySelector('dd')?.textContent?.trim() ?? ''
    }));
  });
  if (!updateRows) {
    throw new Error('the Machine tab did not draw the update card');
  }
  const expectedRows = ['Running', 'Last target', 'Last outcome', 'Last result', 'Last check', 'Channel'];
  const labels = updateRows.map((row) => row.label);
  if (labels.join('|') !== expectedRows.join('|')) {
    throw new Error(`the update card's rows read ${JSON.stringify(labels)}`);
  }
  const rowValue = (label) => updateRows.find((row) => row.label === label)?.value ?? '';
  if (rowValue('Running') !== '0.3.42 · behind the release 0.3.280') {
    throw new Error(`the update card's Running row read "${rowValue('Running')}"`);
  }
  if (rowValue('Last target') !== '0.3.43') {
    throw new Error(`the update card's Last target row read "${rowValue('Last target')}"`);
  }
  if (rowValue('Last outcome') !== 'refused') {
    throw new Error(`the update card's Last outcome row read "${rowValue('Last outcome')}"`);
  }
  if (!rowValue('Last result') || !rowValue('Last check')) {
    throw new Error('the update card did not date its result and its check');
  }
  if (rowValue('Channel') !== 'stable') {
    throw new Error(`the update card's Channel row read "${rowValue('Channel')}"`);
  }
  // **ONE VERSION, AND THE RELEASE IT IS PLACED AGAINST.** The verdict row names the running build
  // once and the release once; no other row repeats the running version.
  const cardText = updateRows.map((row) => row.value).join(' · ');
  if ((cardText.match(/0\.3\.42/g) ?? []).length !== 1) {
    throw new Error(`the update card repeats the running version: "${cardText}"`);
  }
  if ((cardText.match(/0\.3\.280/g) ?? []).length !== 1) {
    throw new Error(`the update card repeats the release version: "${cardText}"`);
  }
  // **ONE MACHINE HAS ONE CURRENCY (the thirty-fourth list).** Measured on the operator's page: one
  // machine read `behind 0.3.292` in Needs attention and `current (the release)` on its own tab, in
  // one visit — Home measured against the fleet's newest build and the tab against the installer.
  // The card computes Home's target now; this leg checks the Running row's sentence names a target at
  // least as new as the fleet's newest, and that it states a comparison at all.
  const sameCurrency = await page.evaluate(() => {
    const row = [...document.querySelectorAll('.system-update dl > div')].find(
      (entry) => entry.querySelector('dt')?.textContent?.trim() === 'Running'
    );
    return {
      text: row?.querySelector('dd')?.textContent?.trim() ?? '',
      fleetNewest: [...document.querySelectorAll('.fleet-table tbody .fleet-build')]
        .map((node) => (node.textContent ?? '').trim().replace(/^v/, ''))
        .filter(Boolean)
    };
  });
  if (!/behind|ahead|current|release unknown/.test(sameCurrency.text)) {
    throw new Error(`the update card's Running row states no comparison: "${sameCurrency.text}"`);
  }
  const parseOne = (value) => value.split('.').map(Number);
  const compareTwo = (a, b) => {
    for (let i = 0; i < Math.max(a.length, b.length); i += 1) {
      const diff = (a[i] ?? 0) - (b[i] ?? 0);
      if (diff !== 0) return diff;
    }
    return 0;
  };
  const targets = (sameCurrency.text.match(/\d+\.\d+\.\d+/g) ?? []).map(parseOne);
  const fleet = sameCurrency.fleetNewest.map(parseOne);
  const newest = fleet.sort(compareTwo).pop();
  if (targets.length >= 2 && newest) {
    const target = targets[targets.length - 1];
    if (compareTwo(target, newest) < 0) {
      throw new Error(
        `the update card measures against ${target.join('.')}, older than the fleet's newest ` +
          `${newest.join('.')}: "${sameCurrency.text}"`
      );
    }
  }
  step(
    `the update card compares against the same currency Home does: "${sameCurrency.text}"`
  );

  step(
    `the Machine tab draws the update card as labelled rows: ${updateRows
      .map((row) => `${row.label} ${row.value}`)
      .join(' · ')}`
  );
  // **THE START LINE STATES WHEN, NOT WHICH BUILD.** The topbar chip states `Running · v0.3.42`
  // and the card's first row carries the release comparison; the same number a third time beside
  // them was the measured repetition (audit-43 F7). The exact instant stays in the title.
  const machineStart = (await page.locator('.started-line').innerText()).trim();
  if (machineStart !== 'started 1h ago') {
    throw new Error(`the Machine tab's start line read "${machineStart}"`);
  }
  const machineStartTitle = (await page.locator('.started-line').getAttribute('title')) ?? '';
  if (!machineStartTitle.includes(`(${new Date(NOW - 60 * 60_000).toISOString()})`)) {
    throw new Error(`the Machine start title "${machineStartTitle}" does not carry the exact instant`);
  }
  step(`the Machine tab states "${machineStart}" with its exact instant in the title`);
  // **THE RETURN SAYS HOW LONG THE MACHINE WAS GONE (audit-44 F12).** The channel pair is the
  // console's own proof: the close and the reopen 41 minutes later. It sits beside the start, so the
  // page that reads `started 1h ago` also says the machine was away before it.
  const machineRecovery = (await page.locator('.recovery-line').innerText()).trim();
  if (machineRecovery !== 'back after 41 m') {
    throw new Error(`the Machine tab's recovery line read "${machineRecovery}"`);
  }
  const recoveryTitle = (await page.locator('.recovery-line').getAttribute('title')) ?? '';
  if (!recoveryTitle.includes(`(${new Date(NOW - 2 * 60 * 60_000).toISOString()})`)) {
    throw new Error(`the recovery title "${recoveryTitle}" does not carry the exact return`);
  }
  if ((await page.locator('.recovery-line').getAttribute('data-recovery-source')) !== 'console') {
    throw new Error('the recovery line does not name the console as its record');
  }
  step(`the Machine tab says how long the machine was away before this start: ${machineRecovery}`);
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
  step('the Machine tab states reachability as Status · online, and its start line states only when it started');

  // **THE CARD AT BOTH WIDTHS, AND THE PHONE KEEPS IT IN THE VIEWPORT.** The panel's grid is one
  // label/value row per fact; at 390 the rows wrap rather than pushing the page sideways. These are
  // the two pictures the F3 brief asks for.
  await page.setViewportSize({ width: 1440, height: 960 });
  await page.waitForTimeout(300);
  const card1440 = await page.evaluate(() => {
    const card = document.querySelector('.system-update');
    const first = card?.querySelector('dl > div');
    const box = (element) =>
      element
        ? {
            y: Math.round(element.getBoundingClientRect().y),
            width: Math.round(element.getBoundingClientRect().width),
            height: Math.round(element.getBoundingClientRect().height)
          }
        : null;
    return {
      tag: card?.tagName ?? null,
      rows: card?.querySelectorAll('dl > div').length ?? 0,
      card: box(card),
      firstRow: box(first),
      labelFont: first ? getComputedStyle(first.querySelector('dt')).fontSize : null
    };
  });
  console.log(`browser-gate: update card at 1440: ${JSON.stringify(card1440)}`);
  await page.screenshot({ path: path.join(OUT, 'machine-1440.png') });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(300);
  const machine390 = await page.evaluate(() => ({
    viewport: document.documentElement.clientWidth,
    scrollWidth: document.documentElement.scrollWidth,
    rows: document.querySelectorAll('.system-update dl > div').length,
    firstLabel: document.querySelector('.system-update dl > div dt')?.textContent?.trim() ?? ''
  }));
  if (machine390.scrollWidth > machine390.viewport) {
    throw new Error(
      `the Machine tab at 390 scrolls sideways: ${machine390.viewport}/${machine390.scrollWidth}`
    );
  }
  if (machine390.rows !== updateRows.length) {
    throw new Error(
      `the update card drew ${machine390.rows} rows at 390; the fixture has ${updateRows.length}`
    );
  }
  if (machine390.firstLabel !== 'Running') {
    throw new Error(`the update card's first row at 390 was ${JSON.stringify(machine390.firstLabel)}`);
  }

  // **AT 390 THE STRIP SHOWS THE TAB THE ROUTE CHOSE, AND THE CONNECT SHEET FITS (audit-41 F2/F3).**
  // The seven labels are 447 px inside a 358 px strip, and `#device/machine` used to draw no
  // selected tab at all: `Machine` sat at 389..461 with `scrollLeft 0`. The same 390 sheet put 440
  // px of content in a 389 px body — the `Tokens` tab cut to `Toke` and the endpoint sentence cut
  // mid-word. Both are measured as a person meets them: the strip after the viewport resized, the
  // strip on a fresh `#device/machine` load, and the sheet opened from that page's own door.
  const stripFacts = () =>
    page.evaluate(() => {
      const strip = document.querySelector('.device-head .device-tabs');
      const active = strip?.querySelector('[aria-current="page"]');
      const band = strip?.getBoundingClientRect();
      const tab = active?.getBoundingClientRect();
      return {
        viewport: document.documentElement.clientWidth,
        pageWidth: document.documentElement.scrollWidth,
        band: band ? { left: Math.round(band.left), right: Math.round(band.right) } : null,
        clientWidth: strip?.clientWidth ?? 0,
        scrollWidth: strip?.scrollWidth ?? 0,
        scrollLeft: Math.round(strip?.scrollLeft ?? 0),
        active: active?.textContent?.trim() ?? null,
        tab: tab ? { left: Math.round(tab.left), right: Math.round(tab.right) } : null
      };
    });
  const stripResized = await stripFacts();
  // The audit's own page: a fresh load at 390 with `#device/machine` in the fragment. The selection
  // is in localStorage, so the route and the machine survive the reload.
  await page.goto(base + '/#device/machine', { waitUntil: 'domcontentloaded', timeout: 30_000 });
  await page
    .getByRole('heading', { name: 'Machine', exact: true, level: 1 })
    .waitFor({ state: 'visible', timeout: 10_000 });
  await page.locator('.device-head .device-tabs').waitFor({ state: 'visible', timeout: 10_000 });
  await page.locator('.system-update').waitFor({ state: 'visible', timeout: 10_000 });
  await page.waitForTimeout(400);
  const stripLoaded = await stripFacts();
  await page.locator('.device-connect button').click();
  const connectPhone = page.getByRole('dialog', { name: 'Connect a client' });
  await connectPhone.waitFor({ state: 'visible', timeout: 10_000 });
  await page.waitForTimeout(300);
  const sheetFacts = await page.evaluate(() => {
    const box = (element) => {
      const rect = element?.getBoundingClientRect();
      return rect
        ? {
            left: Math.round(rect.left),
            right: Math.round(rect.right),
            width: Math.round(rect.width),
            height: Math.round(rect.height)
          }
        : null;
    };
    const sheet = document.querySelector('.sheet');
    const body = document.querySelector('.sheet-body');
    const strip = document.querySelector('.connect-sheet .connect-tabs');
    const tabs = [...document.querySelectorAll('.connect-sheet .connect-tabs button')];
    const panel = document.querySelector('.connect-sheet [role="tabpanel"]');
    const door = [...document.querySelectorAll('.connect-sheet p.hint')].find((paragraph) =>
      (paragraph.textContent ?? '').includes('takes a device token')
    );
    return {
      viewport: document.documentElement.clientWidth,
      sheet: box(sheet),
      body: body
        ? { ...box(body), clientWidth: body.clientWidth, scrollWidth: body.scrollWidth }
        : null,
      tabs: tabs.map((tab) => ({ label: tab.textContent?.trim() ?? '', ...box(tab) })),
      tabStrip: box(strip),
      panel: box(panel),
      door: door
        ? { ...box(door), clientWidth: door.clientWidth, scrollWidth: door.scrollWidth }
        : null
    };
  });
  await page.keyboard.press('Escape');
  await connectPhone.waitFor({ state: 'hidden', timeout: 5_000 });
  await page.waitForTimeout(200);

  const phoneProblems = [];
  for (const [where, facts] of [
    ['after the resize', stripResized],
    ['on a fresh #device/machine load', stripLoaded]
  ]) {
    if (facts.active !== 'Machine') {
      phoneProblems.push(`${where} the strip's active tab read ${JSON.stringify(facts.active)}`);
    }
    if (!facts.band || !facts.tab) {
      phoneProblems.push(`${where} the strip or its active tab has no box`);
    } else if (facts.tab.left < facts.band.left - 1 || facts.tab.right > facts.band.right + 1) {
      phoneProblems.push(
        `${where} Machine sits at ${facts.tab.left}..${facts.tab.right}, outside the strip ` +
          `${facts.band.left}..${facts.band.right} (scrollLeft ${facts.scrollLeft})`
      );
    }
    if (facts.pageWidth > facts.viewport) {
      phoneProblems.push(`${where} the Device page scrolls sideways (${facts.viewport}/${facts.pageWidth})`);
    }
  }
  if (stripLoaded.scrollWidth <= stripLoaded.clientWidth) {
    phoneProblems.push(
      `the strip does not scroll at 390 (${stripLoaded.clientWidth}/${stripLoaded.scrollWidth}); ` +
        'the measurement would not prove the active tab was brought back'
    );
  }
  if (!sheetFacts.sheet || !sheetFacts.body) {
    phoneProblems.push('the 390 Connect sheet or its body has no box');
  } else {
    if (Math.abs(sheetFacts.sheet.width - sheetFacts.viewport) > 1) {
      phoneProblems.push(
        `the 390 sheet is ${sheetFacts.sheet.width} px wide in a ${sheetFacts.viewport} px viewport`
      );
    }
    if (sheetFacts.body.scrollWidth > sheetFacts.body.clientWidth + 1) {
      phoneProblems.push(
        `the sheet body scrolls sideways (` +
          `${sheetFacts.body.clientWidth}/${sheetFacts.body.scrollWidth})`
      );
    }
    const inner = { left: sheetFacts.body.left, right: sheetFacts.body.right };
    const insideBody = (element) =>
      element && element.left >= inner.left - 1 && element.right <= inner.right + 1;
    if (!insideBody(sheetFacts.tabStrip)) {
      phoneProblems.push(
        `the sheet's tab strip is outside the body: ${JSON.stringify(sheetFacts.tabStrip)}`
      );
    }
    if (sheetFacts.tabs.map((tab) => tab.label).join(' · ') !== 'Endpoint · Clients · Tokens') {
      phoneProblems.push(
        `the sheet's tabs read ${JSON.stringify(sheetFacts.tabs.map((tab) => tab.label))}`
      );
    }
    for (const tab of sheetFacts.tabs) {
      if (!insideBody(tab)) {
        phoneProblems.push(
          `the ${tab.label} tab is outside the body (${tab.left}..${tab.right} of ` +
            `${inner.left}..${inner.right})`
        );
      }
    }
    if (!insideBody(sheetFacts.panel)) {
      phoneProblems.push(`the Endpoint panel is outside the body: ${JSON.stringify(sheetFacts.panel)}`);
    }
    if (!sheetFacts.door) {
      phoneProblems.push('the endpoint sentence is missing from the 390 sheet');
    } else {
      if (!insideBody(sheetFacts.door)) {
        phoneProblems.push(
          `the endpoint sentence is outside the body (${sheetFacts.door.left}..${sheetFacts.door.right})`
        );
      }
      if (sheetFacts.door.scrollWidth > sheetFacts.door.clientWidth + 1) {
        phoneProblems.push(
          `the endpoint sentence is ${sheetFacts.door.scrollWidth} px in a ` +
            `${sheetFacts.door.clientWidth} px column`
        );
      }
      if (sheetFacts.door.height < 24) {
        phoneProblems.push(
          `the endpoint sentence is one ${sheetFacts.door.height} px line in a 358 px column`
        );
      }
    }
  }
  if (phoneProblems.length > 0) {
    throw new Error(`the 390 Device strip / Connect sheet: ${phoneProblems.join('; ')}`);
  }
  console.log(
    `browser-gate: at 390 #device/machine's strip is ${stripLoaded.clientWidth}/` +
      `${stripLoaded.scrollWidth}, scrollLeft ${stripLoaded.scrollLeft}, active ` +
      `${JSON.stringify(stripLoaded.active)} at ${stripLoaded.tab.left}..${stripLoaded.tab.right} ` +
      `in ${stripLoaded.band.left}..${stripLoaded.band.right}; the Connect sheet is ` +
      `${sheetFacts.sheet.width} px with a ${sheetFacts.body.clientWidth}/` +
      `${sheetFacts.body.scrollWidth} body and a ${sheetFacts.door.clientWidth}/` +
      `${sheetFacts.door.scrollWidth} endpoint sentence`
  );
  await page.screenshot({ path: path.join(OUT, 'machine-390.png') });
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.waitForTimeout(200);
  step(
    `the update card keeps its ${machine390.rows} labelled rows at 390 with no sideways scroll ` +
      `(${machine390.viewport}/${machine390.scrollWidth})`
  );

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
  for (const fact of ['session 2, which is disconnected', 'Sign in at the machine']) {
    if (!helperSessionLine.includes(fact)) {
      throw new Error(
        `the Screen helper line read "${helperSessionLine}", missing ${JSON.stringify(fact)}`
      );
    }
  }
  // **ONE REFUSAL, ONCE (the thirtieth list, item 1).** Measured at 1440 with the helper
  // unregistered: the Screen tab carried 591 characters for one disabled button and told the same
  // refusal three times — the lead's `…locked and disconnected sessions refuse.`, the reason's
  // `Capture is off: …`, and a paragraph above the button ending `… not a black frame.` This leg
  // counts the sentences that name a refusal and fails if a third one comes back.
  const screenSentences = await page.evaluate(() => {
    const screen = document.querySelector('main');
    const notes = [...document.querySelectorAll('.screen-view-note, .screen-capture-reason, .hint')]
      .filter((element) => element.getClientRects().length > 0)
      .map((element) => (element.textContent ?? '').replace(/\s+/g, ' ').trim())
      .filter((text) => text.length > 0);
    const refusals = notes.filter((text) =>
      /locked|disconnected|refus|Capture is off|cannot be captured/i.test(text)
    );
    return { notes: notes.slice(0, 6), refusals, chars: (screen?.innerText ?? '').length };
  });
  if (screenSentences.refusals.length > 1) {
    throw new Error(
      `the Screen tab states the refusal ${screenSentences.refusals.length} times: ${JSON.stringify(
        screenSentences.refusals
      )}`
    );
  }
  step(
    `the Screen tab states its refusal once (${screenSentences.refusals.length} of ${screenSentences.notes.length} notes, page ${screenSentences.chars} chars)`
  );

  const captureButton = page.getByRole('button', { name: 'Capture screen', exact: true });
  if (!(await captureButton.isDisabled())) {
    throw new Error('Capture screen is enabled while the helper\'s session is disconnected');
  }
  if (!((await captureButton.getAttribute('title')) ?? '').includes('session 2')) {
    throw new Error('the disabled Capture screen does not name the helper\'s session');
  }
  step(`the Screen page states "${helperSessionLine}" and disables Capture screen`);
  await page.screenshot({ path: path.join(OUT, '05b-screen-disconnected.png') });

  // **A HELPER THE FLEET SAYS IS NOT REGISTERED IS NOT AN INVITATION.** The session fact above
  // disables the press because the helper is registered in a dead session; this leg removes the
  // registration itself and proves the button is off for that stronger fact too. The fleet polls
  // every five seconds, so the page reaches the new answer without a reload.
  const registeredHelper = DEVICE.host.helper;
  DEVICE.host.helper = { registered: false, pid: null, session: null };
  try {
    await page.waitForFunction(
      () => {
        const button = document.querySelector('.screen-capture');
        return (
          document.querySelector('#screen-helper-missing') !== null &&
          button instanceof HTMLButtonElement &&
          button.disabled
        );
      },
      undefined,
      { timeout: 15_000 }
    );
    const helperMissing = await page.evaluate(() => {
      const button = document.querySelector('.screen-capture');
      const note = document.querySelector('#screen-helper-missing');
      return {
        disabled: button instanceof HTMLButtonElement ? button.disabled : false,
        title: button?.getAttribute('title') ?? '',
        note: note?.textContent?.trim() ?? ''
      };
    });
    if (!helperMissing.note.includes('has not registered')) {
      throw new Error(
        `the Screen helper-missing line read ${JSON.stringify(helperMissing.note)}`
      );
    }
    if (!helperMissing.title.includes('has not registered')) {
      throw new Error('the disabled Capture screen does not name the unregistered helper');
    }
    await page.screenshot({ path: path.join(OUT, '05c-screen-helper-missing.png') });
    step('the Screen page disables Capture screen when the fleet says the helper has not registered');
  } finally {
    DEVICE.host.helper = registeredHelper;
  }

  // **THE FILES FILTER STARTS WITH THE LIST ON A PHONE.** The measured 390 page wrapped the filter
  // onto its own line but `margin-left: auto` pushed it to x=101 while New folder and Upload began
  // at x=33. It is a full-width line aligned with the other controls now. The listing comes from
  // the gate's `files.list` stub, so the filter (drawn only with entries) is on the page.
  await page
    .locator('nav.device-tabs')
    .getByRole('button', { name: 'Files', exact: true })
    .click();
  await page.getByRole('heading', { name: 'Files', exact: true, level: 1 }).waitFor({
    timeout: 10_000
  });
  await page.locator('.files-filter input').waitFor({ timeout: 10_000 });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(300);
  const files390 = await page.evaluate(() => {
    const tools = document.querySelector('.files-tools');
    const filter = document.querySelector('.files-filter');
    const newFolder = document.querySelector('.files-new');
    const box = (element) => {
      const at = element?.getBoundingClientRect();
      return at ? { left: Math.round(at.left), width: Math.round(at.width) } : null;
    };
    return {
      viewport: document.documentElement.clientWidth,
      scrollWidth: document.documentElement.scrollWidth,
      tools: box(tools),
      filter: box(filter),
      newFolder: box(newFolder)
    };
  });
  console.log(`browser-gate: Files at 390: ${JSON.stringify(files390)}`);
  if (files390.scrollWidth > files390.viewport) {
    throw new Error(
      `the Files page at 390 scrolls sideways: ${files390.viewport}/${files390.scrollWidth}`
    );
  }
  if (!files390.filter || !files390.newFolder || !files390.tools) {
    throw new Error('the Files toolbar, filter or New folder has no box at 390');
  }
  if (files390.filter.left !== files390.newFolder.left) {
    throw new Error(
      `the Files filter starts at x=${files390.filter.left}, not with the other controls at ` +
        `x=${files390.newFolder.left}`
    );
  }
  if (files390.filter.width < files390.tools.width - 2) {
    throw new Error(
      `the Files filter is ${files390.filter.width} px wide in a ${files390.tools.width} px toolbar`
    );
  }
  await page.screenshot({ path: path.join(OUT, '05d-files-390.png') });
  step(
    `at 390 the Files filter starts at x=${files390.filter.left} with the other controls and spans ` +
      `the ${files390.tools.width} px toolbar`
  );

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
  // flags on the gate machine; the sentence under them must state that count, not a blank. The
  // form is one press behind `New token` once the registry has credentials (backlog item 3), so the
  // leg opens it before reading the count — and the list is checked BEFORE that press.
  await connect.getByRole('button', { name: 'New token' }).click();
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
  // **THE DATES ARE THE RECORD'S NOW (the twenty-sixth list, item 3).** A summary row states only
  // what differs between credentials (how many machines, when it was last used); the id, the tool
  // count and the three dates are one press away, in the record that button opens. The gate opens
  // the first token's record and reads those cells there, so the ISO shape and the exact moment in
  // the `title` are still measured, on the surface that now carries them.
  const dated = tokenRows.first();
  const datedFace = await dated.innerText();
  if (!datedFace.includes('used ') && !datedFace.includes('never used')) {
    throw new Error(`the first token's row states no use: ${JSON.stringify(datedFace)}`);
  }
  await dated.getByRole('button', { name: /^[^]*Show details/ }).or(
    dated.locator('.token-disclosure')
  ).first().click();
  const datedRecord = page.locator('.token-record').first();
  await datedRecord.waitFor({ timeout: 10_000 });
  const recordField = (label) => datedRecord.locator('dt', { hasText: label }).locator('..').locator('dd');
  const createdCell = datedRecord.locator('dt:text-is("Created") + dd time');
  const usedCell = datedRecord.locator('dt:text-is("Last used") + dd time');
  const expiresCell = datedRecord.locator('dt:text-is("Expires") + dd time');
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
  await neverToken.locator('.token-disclosure').click();
  const neverRecord = page.locator('.token-record').first();
  const neverExpiry = (await neverRecord.locator('dt:text-is("Expires") + dd').innerText()).trim();
  // At the sheet's 600 px container width the cell's own `Expires` label is inline, so the value is
  // the cell's suffix; a digit anywhere would mean a date was drawn for a token that has none.
  if (neverExpiry !== 'never' || /\d/.test(neverExpiry)) {
    throw new Error(`a token with no expiry read ${JSON.stringify(neverExpiry)}, not never`);
  }
  if ((await neverToken.locator('.token-expires time').count()) !== 0) {
    throw new Error('a token with no expiry drew a date');
  }
  // The row says `never used` in its own words, and the record says `never` for the date.
  const neverUsed = await neverToken.locator('.token-facts').innerText();
  if (!neverUsed.includes('never used') || neverUsed.includes('used 2')) {
    throw new Error(`a token that was never used read ${JSON.stringify(neverUsed)}, not never used`);
  }
  const neverRecordUsed = (await neverRecord.locator('dt:text-is("Last used") + dd').innerText()).trim();
  if (neverRecordUsed !== 'never') {
    throw new Error(`a token that was never used drew ${JSON.stringify(neverRecordUsed)} for its date`);
  }
  step(
    'the token sheet dates created/used/expires in ISO with the exact moment in the title, and never for a token with no expiry'
  );
  await page.screenshot({ path: path.join(OUT, '06-connect-tokens.png') });

  // ITEM 5, MEASURED AT 1280. The sheet is 600 px wide on desktop, so the token row is measured
  // against the sheet, not the viewport: the old sixth column put Revoke past its right edge.
  await page.waitForTimeout(400);
  const sheet1280 = await page.locator('.sheet').boundingBox();
  // **ONE LINE, NOT 156 PX (the twenty-sixth list, item 3).** Measured before this change on the
  // operator's page: seven rows of 156-164 px each repeating `39 tools · destructive allowed ·
  // Expires never`. A summary row now states the two facts that differ, so the gate records its
  // height beside the sheet's — a regression that puts the table cells back shows up as a number.
  const rowGeometry = await page.locator('.token-table tbody tr.token-row').evaluateAll((rows) =>
    rows.map((row) => Math.round(row.getBoundingClientRect().height))
  );
  step(`token rows at 1280 measure ${JSON.stringify(rowGeometry)} px tall (was 156-164 before item 3)`);
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
  // **THE PHONE KEEPS THE BUILD.** The topbar used to hide its whole meta span below 640 px, so an
  // offline Device page at 390 named its version nowhere (the 14R's `v0.3.275` existed only on
  // Home). The version has its own element now; the status word is what the phone drops.
  const topbarBuild390 = await page.evaluate(() => {
    const version = document.querySelector('.machine-menu-version');
    const box = version?.getBoundingClientRect();
    return {
      text: version?.textContent?.trim() ?? null,
      visible: version !== null && version.getClientRects().length > 0,
      right: Math.round(box?.right ?? 0),
      viewport: document.documentElement.clientWidth
    };
  });
  if (!topbarBuild390.visible || !/^· v\d/.test(topbarBuild390.text ?? '')) {
    throw new Error(`the 390 topbar's build reads ${JSON.stringify(topbarBuild390)}`);
  }
  if (topbarBuild390.right > topbarBuild390.viewport) {
    throw new Error(`the 390 topbar's build is outside the viewport: ${topbarBuild390.right}`);
  }
  step(
    `at 390 the topbar keeps the build (${topbarBuild390.text}) beside the machine, as 1440 does`
  );
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
  // The same sentence at 390: the status, version and start stack inside one card, and the silence's
  // age is still stated. The fleet list is Home with no machine chosen, so clear the choice through
  // the header's own menu first. While it is open, the menu proves it is a switcher: name, state and
  // build only, never Home's board (disk, the day's calls, the last absence).
  await page.locator('details.machine-menu > summary').click();
  const switcherRows = await page.evaluate(() =>
    [...document.querySelectorAll('.machine-pop > button')]
      .filter((button) => button.querySelector('.machine-pop-name'))
      .map((button) => button.textContent?.trim() ?? '')
  );
  if (switcherRows.length === 0) {
    throw new Error('the machine menu drew no machine row');
  }
  for (const row of switcherRows) {
    if (/GiB|client calls?|\bfailed\b|back |away since/.test(row)) {
      throw new Error(`the machine menu drew a board fact: "${row}"`);
    }
    if (!/(online|offline) · v\d/.test(row)) {
      throw new Error(`the machine menu row lost its state and build: "${row}"`);
    }
  }
  step(`the machine menu is a switcher (${switcherRows.length} rows: name, state, build; no counters)`);
  await page.getByRole('button', { name: 'All machines' }).click();
  await page.locator('nav.rail button[title="Home"]').click();
  await page.getByRole('heading', { name: 'Home', exact: true }).waitFor({ timeout: 10_000 });
  await page.getByRole('button', { name: /^Open DESKTOP-14RJCR8/ }).waitFor({ timeout: 10_000 });
  // The attention rows and the installer strip are two reads of one page: `/fleet` can answer
  // before `/installer`, and measuring the block then sees only the offline machine. The strip is
  // asserted below, so wait for the read that makes the measured 5 rows the settled page's.
  await page.locator('.installer').waitFor({ timeout: 10_000 });
  const offlineState390 = (
    await page
      .locator('tr.fleet-row', {
        has: page.getByRole('button', { name: /^Open DESKTOP-14RJCR8/ })
      })
      .locator('.fleet-state')
      .textContent()
  )?.trim();
  if (offlineState390 !== 'offline · last seen 1h 49m ago') {
    throw new Error(`the offline row read "${offlineState390}" at 390`);
  }
  step(`at 390 the offline row says "${offlineState390}"`);
  // **AT 390 THE TABLE IS ONE CARD PER MACHINE.** The name owns the first line, status and version
  // share the next, and the start gets the line under them. No sideways scroll, and every fact the
  // desktop table states is still on the card.
  const fleet390 = await page.evaluate(() => {
    const table = document.querySelector('table.fleet-table');
    const rows = [...document.querySelectorAll('tr.fleet-row')].map((row) => {
      const name = row.querySelector('.fleet-name');
      const status = row.querySelector('.fleet-status');
      const version = row.querySelector('.fleet-version');
      const started = row.querySelector('.fleet-started');
      const nameBox = name?.getBoundingClientRect();
      return {
        name: name?.textContent?.trim() ?? '',
        status: status?.textContent?.trim() ?? '',
        version: version?.textContent?.trim() ?? '',
        started: started?.textContent?.trim() ?? '',
        width: Math.round(row.getBoundingClientRect().width),
        height: Math.round(row.getBoundingClientRect().height),
        nameWidth: nameBox ? Math.round(nameBox.width) : 0,
        nameBottom: nameBox ? Math.round(nameBox.bottom) : null,
        statusTop: status ? Math.round(status.getBoundingClientRect().top) : null,
        versionTop: version ? Math.round(version.getBoundingClientRect().top) : null,
        startedTop: started ? Math.round(started.getBoundingClientRect().top) : null
      };
    });
    const attention = document.querySelector('.attention');
    const tableBox = table?.getBoundingClientRect();
    const installer = document.querySelector('.installer');
    return {
      viewport: window.innerWidth,
      scrollWidth: document.documentElement.scrollWidth,
      rows,
      attentionRows: attention ? attention.querySelectorAll('.attention-row').length : 0,
      attentionNames: attention
        ? [...attention.querySelectorAll('.attention-machine')].map((name) =>
            name.textContent?.trim() ?? ''
          )
        : [],
      attentionDoors: attention
        ? [...attention.querySelectorAll('.attention-row')].map(
            (row) => row.querySelectorAll('button').length
          )
        : [],
      attentionTop: attention ? Math.round(attention.getBoundingClientRect().top) : null,
      tableTop: tableBox ? Math.round(tableBox.top) : null,
      tableBottom: tableBox ? Math.round(tableBox.bottom) : null,
      installerTop: installer ? Math.round(installer.getBoundingClientRect().top) : null
    };
  });
  if (fleet390.rows.length !== 4) {
    throw new Error(`the fleet drew ${fleet390.rows.length} rows at 390`);
  }
  for (const row of fleet390.rows) {
    if (!row.status.includes('online') && !row.status.includes('offline')) {
      throw new Error(`the ${row.name} row lost its status at 390`);
    }
    if (!/^v0\.3\.42$/.test(row.version)) {
      throw new Error(`the ${row.name} row's version read "${row.version}" at 390`);
    }
    if (row.started === '') {
      throw new Error(`the ${row.name} row lost its start at 390`);
    }
    if (row.nameWidth < 120) {
      throw new Error(`the ${row.name} name is only ${row.nameWidth} px wide at 390`);
    }
    if (
      row.statusTop === null ||
      row.versionTop === null ||
      Math.abs(row.statusTop - row.versionTop) > 4
    ) {
      throw new Error(`the ${row.name} row's status and version do not share a line at 390`);
    }
    if (row.startedTop === null || row.nameBottom === null || row.startedTop < row.nameBottom) {
      throw new Error(`the ${row.name} row's start is not below its name at 390`);
    }
    if (row.width > fleet390.viewport) {
      throw new Error(`the ${row.name} card is ${row.width} px wide at 390`);
    }
  }
  // **THE PHONE SHOWS MACHINES, NOT PROBLEMS.** Four machines and five problems are four rows at
  // 390; one door each, so the same machine's action is not offered twice (audit-40 F6).
  if (fleet390.attentionRows !== 4) {
    throw new Error(`the attention block drew ${fleet390.attentionRows} rows at 390, not 4`);
  }
  if (new Set(fleet390.attentionNames).size !== fleet390.attentionNames.length) {
    throw new Error(
      `the attention block drew one machine twice at 390: ${JSON.stringify(fleet390.attentionNames)}`
    );
  }
  if (fleet390.attentionDoors.some((doors) => doors !== 1)) {
    throw new Error(
      `an attention row drew ${JSON.stringify(fleet390.attentionDoors)} doors at 390, not one each`
    );
  }
  if (fleet390.attentionTop === null || fleet390.attentionTop > (fleet390.tableTop ?? 0)) {
    throw new Error('Needs attention is not above the fleet at 390');
  }
  if (
    fleet390.installerTop === null ||
    fleet390.tableBottom === null ||
    fleet390.installerTop < fleet390.tableBottom
  ) {
    throw new Error('the installer is not below the fleet at 390');
  }
  if (fleet390.scrollWidth > fleet390.viewport) {
    throw new Error(
      `the fleet page scrolls sideways at 390: ${fleet390.scrollWidth} > ${fleet390.viewport}`
    );
  }
  const heights390 = fleet390.rows.map((row) => `${row.name} ${row.height}px`).join(', ');
  step(
    `at 390 the fleet is four stacked cards (${heights390}) with a ${fleet390.attentionRows}-row ` +
      'attention block; no sideways scroll'
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
  // The form is one press behind `New token` (backlog item 3).
  await tokensDoor.getByRole('button', { name: 'New token' }).click();
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
  // **THE RECORD NAMES THE MACHINES.** A count alone sent the operator minting another token to
  // answer "does this one cover 84T3LIN?"; the count is the row's fact (backlog item 3) and the
  // machine names are one press behind it, so the gate opens the row and reads them there.
  const fleetScopeRow = tokensDoor.getByRole('row', { name: /zss-harness-20261106-fleet/ });
  const fleetFace = await fleetScopeRow.innerText();
  if (!/\d+ machines? · /.test(fleetFace)) {
    throw new Error(`the fleet token's row stated no reach count: ${JSON.stringify(fleetFace)}`);
  }
  await fleetScopeRow.locator('.token-disclosure').click();
  const fleetScopeText = await page.locator('.token-record').first().innerText();
  if (!fleetScopeText.includes(DEVICE.name)) {
    throw new Error(`the fleet token's record did not name the machines: ${JSON.stringify(fleetScopeText)}`);
  }
  // **THE SCOPE CELL NAMES THE DOOR, NOT ONLY THE MACHINES.** A token whose scope names a machine
  // is still refused at that machine's URL when it is a fleet token; the cell must say which door
  // the credential opens, in the same words the 401 uses.
  if (!fleetScopeText.includes('fleet · /mcp/all')) {
    throw new Error(`the fleet token's scope did not name its door: ${JSON.stringify(fleetScopeText)}`);
  }
  const deviceScopeRow = tokensDoor.getByRole('row', { name: /gate-laptop/ });
  await deviceScopeRow.locator('.token-disclosure').click();
  const deviceScopeText = await page.locator('.token-record').first().innerText();
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
      rows: [...sheet.querySelectorAll('.token-table tbody tr.token-row')].map((row) => {
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
  // **THE TABLE OPENS ON THE SAME AMOUNT OF RECORD AS THE TIMELINE (the thirty-first list).**
  // Measured on the operator's page: the Table built 1,001 rows at once — 17,074 elements and a
  // 41,467 px page in 478 ms — while the Timeline beside it drew 60 behind a door. These two views
  // are one record; this leg counts the rows the Table draws and fails if the wall comes back.
  const tableFold = await page.evaluate(() => {
    const rows = [...document.querySelectorAll('tr.audit-row')];
    const doors = [...document.querySelectorAll('.audit-more')].map((button) =>
      (button.textContent ?? '').replace(/\s+/g, ' ').trim()
    );
    return { rows: rows.length, doors: doors.slice(0, 3), elements: document.querySelectorAll('*').length };
  });
  if (tableFold.rows > 120) {
    throw new Error(`the Table draws ${tableFold.rows} rows at once; a day opens on its newest 60`);
  }
  if (tableFold.doors.length === 0) {
    throw new Error('the Table draws no door for the rest of its day');
  }
  step(
    `the Table opens on ${tableFold.rows} rows with ${tableFold.doors.length} door(s): ${tableFold.doors.join(' / ')}`
  );

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
    const rail = document.querySelector('.rail')?.getBoundingClientRect() ?? null;
    const toolbar = document.querySelector('.audit-toolbar')?.getBoundingClientRect() ?? null;
    const fold = document.querySelector('details.audit-filter-fold');
    const visible = (element) => {
      if (element.getClientRects().length === 0) return false;
      if (typeof element.checkVisibility !== 'function') return true;
      return element.checkVisibility({ checkVisibilityCSS: true, contentVisibilityAuto: true });
    };
    const toolbarControls = toolbar
      ? [...document.querySelector('.audit-toolbar').querySelectorAll('summary')].filter(visible)
      : [];
    return {
      viewport: document.documentElement.clientWidth,
      documentScrollWidth: document.documentElement.scrollWidth,
      rowTop: Math.round(rowBox?.top ?? 0),
      rowBottom: Math.round(rowBox?.bottom ?? 0),
      railTop: Math.round(rail?.top ?? 0),
      toolbarHeight: Math.round(toolbar?.height ?? 0),
      toolbarControls: toolbarControls.map((entry) => entry.textContent?.trim() ?? ''),
      foldOpen: fold ? fold.open : null,
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
    `at 390 one ${audit390.toolbarHeight} px filter row (${audit390.toolbarControls.join(', ')}) ` +
      `and the first card y=${audit390.rowTop}–${audit390.rowBottom} above the rail at ` +
      `${audit390.railTop}; the device cell draws "${audit390.device.short}" with the full name ` +
      `in its title (row ${audit390.device.rowHeight} px, cell ` +
      `${audit390.device.cellWidth}×${audit390.device.cellHeight}, was 67×91)`
  );
  // **THE CONTROLS START ABOVE THE FOLD AND THE FIRST ROW IS NOT CUT.** The measured phone put the
  // first row at y=746 with the rail at 781: one line of the card showed and the rest sat under the
  // bar. The one filter row (`Filters` + `Export`) is 43 px, and the first card must end above the
  // rail, not merely start above it.
  if (audit390.toolbarHeight > 60) {
    throw new Error(`the 390 filter row is ${audit390.toolbarHeight} px tall, not one row`);
  }
  if (
    audit390.toolbarControls.length !== 2 ||
    !audit390.toolbarControls.some((text) => text.startsWith('Filters')) ||
    !audit390.toolbarControls.some((text) => text.startsWith('Export'))
  ) {
    throw new Error(
      `the 390 filter row drew ${JSON.stringify(audit390.toolbarControls)}, not Filters and Export`
    );
  }
  if (audit390.foldOpen !== false) {
    throw new Error('the 390 filter fields are not folded by default');
  }
  if (audit390.rowTop >= audit390.railTop) {
    throw new Error(
      `the 390 first audit row starts at y=${audit390.rowTop}, below the rail at ` +
        `${audit390.railTop}`
    );
  }
  if (audit390.rowBottom > audit390.railTop) {
    throw new Error(
      `the 390 first audit row ends at y=${audit390.rowBottom}, under the rail at ` +
        `${audit390.railTop}`
    );
  }
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
  // **THE PHONE FOLDS THE FIELDS, NOT THE FILTERS.** `Filters` is one press away and the count is
  // on its summary; the range control is inside it (audit-41 F4).
  await page.locator('.audit-filter-fold > summary').click();
  await page.getByLabel('Time').waitFor({ state: 'visible', timeout: 5_000 });
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
  const rangeMatch = /^Showing (.+), (\d+) lines?$/.exec(rangeSentence390);
  if (!rangeMatch) {
    throw new Error(`the range sentence read ${JSON.stringify(rangeSentence390)}`);
  }
  // **THE SENTENCE NAMES THE RANGE THE REQUEST ASKED FOR.** At 00:30 "Last hour" spans two local
  // days, and the sentence says `11-06 23:30 – 11-07 00:30`; checking a single-date shape would
  // call that correct sentence a failure until 01:00.
  const namedRange = rangeText(state.lastRange.from, state.lastRange.to);
  if (rangeMatch[1] !== namedRange) {
    throw new Error(
      `the range sentence named ${JSON.stringify(rangeMatch[1])}, ` +
        `the answer's range was ${JSON.stringify(namedRange)}`
    );
  }
  const rangeRows = await page.locator('.audit-row').count();
  if (Number(rangeMatch[2]) !== state.lastRange.count || rangeRows !== state.lastRange.count) {
    throw new Error(
      `the sentence and rows counted ${rangeMatch[2]}/${rangeRows}, the answer held ` +
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

  // **THE COUNT NAMES THE FILTER, NOT ONLY THE WINDOW.** The measured 390 page drew 0 rows and said
  // `29 lines` above `No calls match`: the count was the range's, not the filter's. With `Failed`
  // chosen and no failure in this range, the sentence still counts the range's lines and names the
  // filter's zero, and the empty state says the filter found nothing.
  const rangeCount = state.lastRange.count;
  await page.getByLabel('Status').selectOption('failed');
  await page.waitForFunction(
    (count) =>
      (document.querySelector('.audit-cap')?.textContent ?? '').includes(
        `0 of the ${count} lines match the filters`
      ),
    rangeCount,
    { timeout: 10_000 }
  );
  const filteredCap = (await page.locator('.audit-cap').textContent())?.trim() ?? '';
  if (
    !new RegExp(
      `^Showing .+, ${rangeCount} lines?; 0 of the ${rangeCount} lines? match the filters$`
    ).test(filteredCap)
  ) {
    throw new Error(`the filtered range sentence read ${JSON.stringify(filteredCap)}`);
  }
  if ((await page.locator('.audit-row').count()) !== 0) {
    throw new Error('the Failed filter drew rows in a range with no failed line');
  }
  const filteredEmpty = (await page.locator('.audit-empty').innerText()).trim();
  if (!/^No calls match /.test(filteredEmpty)) {
    throw new Error(`the filtered empty state read ${JSON.stringify(filteredEmpty)}`);
  }
  await page.screenshot({ path: path.join(OUT, '07e-filtered-range-390.png') });
  step(
    `at 390 the filtered range says ${JSON.stringify(filteredCap)} over 0 rows, and the empty ` +
      'state names the filter'
  );
  await page.getByLabel('Status').selectOption('all');

  // **THE RANGE THAT WAS DRAWN IS THE RANGE THAT LEAVES.** Export is beside Time and asks the
  // console's own route for the same two bounds; it must not save a different window or filter.
  const exportRequestsBefore = state.exportRequests;
  const exportAnswered = page.waitForResponse(
    (response) => new URL(response.url()).pathname === '/audit/export',
    { timeout: 10_000 }
  );
  await page.locator('.audit-export-menu > summary').click();
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
  await page.locator('.audit-export-menu > summary').click();
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
  // The row's name is the door now; it lands on the machine's own Overview (audit-43 F1).
  await page.getByRole('button', { name: /^Open DESKTOP-14RJCR8/ }).click();
  await page.getByRole('heading', { name: 'Overview', exact: true }).waitFor({ timeout: 10_000 });
  const offlineHash = await page.evaluate(() => window.location.hash);
  if (offlineHash !== '#device/overview') {
    throw new Error(`the offline row's door landed on ${JSON.stringify(offlineHash)}`);
  }
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

  // **THE OFFLINE ADVICE NAMES THE DOOR THAT WORKS FROM HERE (audit 37 F6).** The sentence used to
  // send the operator to a loopback panel the console cannot open and did not address. It now links
  // the console's own Device → Machine view and names the machine's panel as local text, not a link.
  const advice = page.locator('.notice');
  const machineDoor = advice.getByRole('link', { name: 'Device → Machine' });
  if ((await machineDoor.count()) !== 1) {
    throw new Error('the offline hub offers no Device → Machine link');
  }
  const doorHref = await machineDoor.getAttribute('href');
  if (doorHref !== '#device/machine') {
    throw new Error(`the offline door links ${JSON.stringify(doorHref)}`);
  }
  const doorBox = await machineDoor.boundingBox();
  if (!doorBox || doorBox.x < 0 || doorBox.x + doorBox.width > 390) {
    throw new Error(`the Device → Machine link is outside the 390 viewport: ${JSON.stringify(doorBox)}`);
  }
  const adviceText = await advice.innerText();
  if (
    !adviceText.includes('http://127.0.0.1:18090/') ||
    !adviceText.includes('opens only at the machine')
  ) {
    throw new Error(
      `the offline advice does not name the local panel as local: ${JSON.stringify(adviceText)}`
    );
  }
  if ((await advice.locator('a[href*="127.0.0.1"], a[href*="18090"]').count()) !== 0) {
    throw new Error('the offline advice draws the machine-local panel as a link');
  }

  // **A PRESS IS NOT THE POLL (audit-38 F3).** The measured Retry refetched `/fleet/<id>/tools`
  // and left a byte-identical 4,208-character page: no spinner, no outcome, no timestamp, so the
  // operator could not tell their press from the five-second poll. Hold the 503 so the working
  // button can be seen, then require the same offline result to be said again with the moment the
  // press settled. The held answer also proves the press made one read of its own.
  await page.waitForTimeout(500); // the hub's mount read has landed; the press is the next read
  const retryToolsBefore = state.offlineToolsRequests;
  const retryBefore = (await advice.innerText()).trim();
  state.offlineToolsDelayMs = 1200;
  await advice.getByRole('button', { name: 'Retry', exact: true }).click();
  const retryPending = advice.getByRole('button', { name: 'Retrying…', exact: true });
  await retryPending.waitFor({ state: 'visible', timeout: 2_000 });
  if (!(await retryPending.isDisabled())) {
    throw new Error('the offline Retry stayed enabled while its read was in flight');
  }
  const retryPendingText = (await advice.innerText()).trim();
  await page.screenshot({ path: path.join(OUT, '07c-offline-retry-pending-390.png') });
  const retryOutcome = advice.locator('.retry-outcome');
  await retryOutcome.waitFor({ state: 'visible', timeout: 10_000 });
  const retryOutcomeText = (await retryOutcome.innerText()).trim();
  if (!/^Still offline · last checked \d{2}:\d{2}\.$/.test(retryOutcomeText)) {
    throw new Error(`the retry outcome read ${JSON.stringify(retryOutcomeText)}`);
  }
  if (await advice.getByRole('button', { name: 'Retry', exact: true }).isDisabled()) {
    throw new Error('the offline Retry stayed disabled after its read settled');
  }
  if (state.offlineToolsRequests !== retryToolsBefore + 1) {
    throw new Error(
      `the offline Retry made ${state.offlineToolsRequests - retryToolsBefore} tools reads, not one`
    );
  }
  const retryAfter = (await advice.innerText()).trim();
  await page.screenshot({ path: path.join(OUT, '07d-offline-retry-390.png') });
  state.offlineToolsDelayMs = 0;
  console.log(
    `browser-gate: offline Retry ${JSON.stringify({ before: retryBefore, pending: retryPendingText, after: retryAfter })}`
  );
  step(`the offline Retry is disabled as "Retrying…", then says "${retryOutcomeText}"`);
  await machineDoor.click();
  await page.getByRole('heading', { name: 'Machine', exact: true }).waitFor({ timeout: 10_000 });
  // **A THUMB IS NOT A CURSOR (the twenty-ninth list, items 1 and 3).** Measured on the operator's
  // phone: every one of Home's 25 controls in `main` was under 44 px — `Tokens` and `Add a machine`
  // at 30 px, the fleet row's name door at 26 px, and the audit table's per-record opener at
  // 24 x 28 px. The phone now has one rule that says a control is at least 44 px tall, and this
  // leg counts the ones that are not. It runs with a coarse pointer, because that is when the rule
  // is meant to apply — a desktop narrowed to 390 px is not a phone.
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(400);
  const phoneTouch = await page.evaluate(() => {
    const targets = [
      ...document.querySelectorAll('main button, main a[href], main summary, main input')
    ].filter((element) => {
      const box = element.getBoundingClientRect();
      return box.width > 0 && box.height > 0;
    });
    const under = targets.filter((element) => element.getBoundingClientRect().height < 43);
    return {
      coarse: window.matchMedia('(pointer: coarse)').matches,
      targets: targets.length,
      under44: under.length,
      shortest: targets
        .map((element) => ({
          text: (element.getAttribute('aria-label') || element.textContent || '')
            .replace(/\s+/g, ' ')
            .trim()
            .slice(0, 20),
          height: Math.round(element.getBoundingClientRect().height)
        }))
        .sort((a, b) => a.height - b.height)
        .slice(0, 4)
    };
  });
  if (phoneTouch.under44 !== 0) {
    throw new Error(`the phone's controls are under 44 px: ${JSON.stringify(phoneTouch)}`);
  }
  step(
    `the phone's controls measure >= 44 px (${phoneTouch.targets} targets, ${phoneTouch.under44} under, coarse pointer ${phoneTouch.coarse})`
  );

  // **THE CARD IS THE DOOR ON THE PHONE (the twenty-ninth list, item 2).** Measured before this:
  // a fleet row was 358 x 103 px and the name link inside it 156 x 26 — 11% of the card — and
  // tapping the card's body at its centre hit `.fleet-status` and navigated nowhere. The link now
  // covers the card, and this leg measures both the coverage and the tap.
  // The previous legs left the page on a machine's tab; the fleet table is Home's.
  await page.locator('nav.rail button[title="Home"]').click();
  await page.getByRole('heading', { name: 'Home', exact: true }).waitFor({ timeout: 10_000 });
  await page.locator('.fleet-table tbody tr').first().waitFor({ timeout: 10_000 });
  // **THE FIRST CARD MUST BE ON SCREEN FOR A TAP TO BE MEASURABLE.** `elementFromPoint` answers
  // `null` for a point outside the viewport, which read as "the tap missed" on a page whose table
  // simply started below the fold (the legend row), so the card is scrolled into view first.
  await page.locator('.fleet-table tbody tr').first().scrollIntoViewIfNeeded();
  await page.waitForTimeout(400);
  const cardDoor = await page.evaluate(() => {
    const rows = [...document.querySelectorAll('.fleet-table tbody tr')];
    const measured = rows.map((row) => {
      const card = row.getBoundingClientRect();
      const door = row.querySelector('.fleet-open')?.getBoundingClientRect();
      const centre = document.elementFromPoint(
        Math.round(card.x + card.width / 2),
        Math.round(card.y + card.height / 2)
      );
      return {
        coverage: door ? Math.round(((door.width * door.height) / (card.width * card.height)) * 100) : 0,
        centreIsTheDoor: door ? centre !== null && (centre.closest('.fleet-open') !== null) : false
      };
    });
    const more = rows[0]?.querySelector('.row-menu > summary');
    return {
      rows: measured.length,
      worst: Math.min(...measured.map((entry) => entry.coverage)),
      centreOnDoor: measured.filter((entry) => entry.centreIsTheDoor).length,
      moreHeight: more ? Math.round(more.getBoundingClientRect().height) : 0
    };
  });
  if (cardDoor.worst < 95) {
    throw new Error(
      `the phone's fleet card is not the door (worst coverage ${cardDoor.worst}%): ${JSON.stringify(cardDoor)}`
    );
  }
  if (cardDoor.centreOnDoor !== cardDoor.rows) {
    throw new Error(
      `tapping a card's body lands outside its door on ${cardDoor.rows - cardDoor.centreOnDoor} of ${cardDoor.rows}`
    );
  }
  step(
    `the phone's fleet card is one door: every card ${cardDoor.worst}%+ covered, the centre of all ` +
      `${cardDoor.rows} lands on the door, the ... menu still ${cardDoor.moreHeight} px of its own`
  );

  step('at 390 the offline advice links Device → Machine and names the machine-local panel as local');
  await page.locator('nav.rail button[title="Home"]').click();
  await page.getByRole('heading', { name: 'Home', exact: true }).waitFor({ timeout: 10_000 });

  // **THE MACHINE MENU DOES NOT OFFER WHAT IS ALREADY SHOWING (the twenty-seventh list, item 1).**
  // The measured menu read `DESKTOP-9VH92BJ online · v0.3.292` in its summary and then drew the same
  // words as its second row: five rows, four choices. The summary names the machine in force, so a
  // row is a machine you could switch TO.
  await page.locator('.machine-menu').click();
  await page.waitForTimeout(400);
  const machineMenu = await page.evaluate(() => {
    const menu = document.querySelector('.machine-menu');
    const summary = menu.querySelector('summary');
    const summaryLabel = summary.textContent.replace(/\s+/g, ' ').trim();
    const current = menu.querySelector('summary .machine-menu-label')?.textContent?.trim() ?? null;
    const rows = [...menu.querySelectorAll('.machine-pop button')].map((b) => ({
      text: b.textContent.replace(/\s+/g, ' ').trim(),
      name: b.querySelector('.machine-pop-name')?.textContent?.trim() ?? null
    }));
    return { summaryLabel, current, rows };
  });
  if (!machineMenu.current) throw new Error('the machine menu names no current machine');
  const echoes = machineMenu.rows.filter((row) => row.name === machineMenu.current);
  if (echoes.length !== 0) {
    throw new Error(
      `the machine menu re-offered the machine already in the summary (${machineMenu.current}): ` +
        JSON.stringify(echoes)
    );
  }
  if (machineMenu.rows[0]?.text !== 'All machines') {
    throw new Error(`the machine menu's first row is ${JSON.stringify(machineMenu.rows[0])}`);
  }
  step(
    `the machine menu names ${machineMenu.current} once and lists ${machineMenu.rows.length} choices ` +
      `(${machineMenu.rows.map((row) => row.name ?? row.text).join(', ')})`
  );
  await page.keyboard.press('Escape');
  await page.waitForTimeout(200);

  // **THE OFFLINE TOKEN FORM DOES NOT CLAIM THE CONSOLE'S COUNT.** `/fleet/<id>/tools` is 503 for
  // this machine, so the count comes from `/tools`. The form must name that catalogue, cover the
  // machine with the row's own silence, and draw the static-fallback note on the default preset —
  // the measured bug hid the note behind Choose tools… and printed 46 as the machine's.
  await page.locator('.account > summary').click();
  await page.locator('.account').getByRole('button', { name: 'Tokens', exact: true }).click();
  const offlineTokens = page.getByRole('dialog', { name: 'Tokens' });
  await offlineTokens.waitFor({ timeout: 10_000 });
  // The credential list is the section's answer; the mint form is one press behind `New token`
  // (backlog item 3).
  await offlineTokens.getByRole('button', { name: 'New token' }).click();
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

  // **THE RECORD'S ROWS LIVE IN ACTIVITY, NOT ON THE MACHINE'S PAGE.** The end leg opens the
  // machine whose 1000-line window holds the measured 974-row day: 283 client calls, 580 console
  // reads and 111 rows no caller was recorded for, with 9 channel boundaries and 17 repeated keys
  // withheld. Overview keeps one day line and one door; the 974 rows are Activity's, drawn once
  // when the door is taken. Nothing on the machine's page folds or counts that window any more.
  await page.locator('details.machine-menu > summary').click();
  await page.getByRole('button', { name: 'All machines' }).click();
  await page.getByRole('button', { name: /^Open DESKTOP-7QK3MN2/ }).click();
  await page.getByRole('heading', { name: 'Overview', exact: true }).waitFor({ timeout: 10_000 });
  const bigHash = await page.evaluate(() => window.location.hash);
  if (bigHash !== '#device/overview') {
    throw new Error(`the measured row's door landed on ${JSON.stringify(bigHash)}`);
  }
  const bigToday = (await page.locator('.home-today').textContent())?.trim() ?? '';
  if (!bigToday.startsWith(`Today ${BIG_CLIENT} client calls`)) {
    throw new Error(`the measured hub's day line read ${JSON.stringify(bigToday)}`);
  }
  // The window is full of today's lines, so its oldest is not the day's first call: no span.
  if (bigToday.includes('spanning')) {
    throw new Error(`a cut window stated a day span: ${JSON.stringify(bigToday)}`);
  }
  const bigOffenders = await page.evaluate(() => ({
    rows: document.querySelectorAll('.timeline-row').length,
    capped: document.querySelectorAll('.timeline-capped').length,
    folds: document.querySelectorAll('.home-more').length,
    chips: document.querySelectorAll(
      '.timeline-console-reads, .timeline-unattributed'
    ).length,
    headings: document.querySelectorAll('.timeline-day-label').length
  }));
  if (
    bigOffenders.rows !== 0 ||
    bigOffenders.capped !== 0 ||
    bigOffenders.folds !== 0 ||
    bigOffenders.chips !== 0 ||
    bigOffenders.headings !== 0
  ) {
    throw new Error(
      `the measured hub still draws the record: ${JSON.stringify(bigOffenders)}`
    );
  }
  const bigAll = page.locator('.home-day').getByRole('button', { name: /All activity/ });
  if ((await bigAll.count()) !== 1) {
    throw new Error('the measured hub has no single All activity door');
  }
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(400);
  if ((await page.locator('.timeline-row').count()) !== 0) {
    throw new Error('the measured hub drew Activity rows at 390');
  }
  await page.screenshot({ path: path.join(OUT, '07c-measured-hub-390.png') });
  await page.setViewportSize({ width: 1440, height: 960 });
  await page.waitForTimeout(400);
  await page.screenshot({ path: path.join(OUT, '07d-measured-hub-1440.png') });
  // THE ONE DOOR OPENS THE RECORD. The same 1000-line window draws its 974 rows in Activity, so
  // the machine's page did not drop them, it moved them to the page that owns them.
  await bigAll.click();
  await page.getByRole('heading', { name: 'Activity', exact: true, level: 1 }).waitFor({
    timeout: 10_000
  });
  // The Activity page opens on the fleet's window; the machine's chip is the scope that reads its
  // own 1000-line window, which is where the measured 974 rows are drawn. The view may have been
  // left on the Table by an earlier leg, so the timeline is chosen first.
  const timelineTab = page.getByRole('tab', { name: 'Timeline', exact: true });
  if (
    (await timelineTab.count()) > 0 &&
    (await timelineTab.getAttribute('aria-selected')) !== 'true'
  ) {
    await timelineTab.click();
  }
  const bigChip = page.getByRole('button', { name: 'DESKTOP-7QK3MN2', exact: true });
  await bigChip.waitFor({ state: 'visible', timeout: 10_000 });
  await bigChip.click();
  await page.waitForFunction(
    (expected) => document.querySelectorAll('.timeline-row').length === expected,
    Math.min(BIG_DRAWABLE, 60),
    { timeout: 30_000 }
  );
  // The measured 974-row day is 60 rows and one door now; the door opens the rest in place.
  const bigDoor = page.locator('.timeline-more');
  if ((await bigDoor.count()) !== 1) {
    throw new Error(`the measured day drew ${await bigDoor.count()} doors for 974 rows`);
  }
  await bigDoor.click();
  await page.waitForFunction(
    (expected) => document.querySelectorAll('.timeline-row').length === expected,
    BIG_DRAWABLE,
    { timeout: 30_000 }
  );
  const activityRows = await page.locator('.timeline-row').count();
  if (activityRows !== BIG_DRAWABLE) {
    throw new Error(`Activity drew ${activityRows} rows, not the measured ${BIG_DRAWABLE}`);
  }
  const lastBigRow = (await page.locator('.timeline-row').last().textContent())?.trim() ?? '';
  if (!lastBigRow.includes('Unattributed 110')) {
    throw new Error(`the record's last row was ${JSON.stringify(lastBigRow)}`);
  }
  step(
    `the measured hub is one day line (${BIG_CLIENT} client calls, no span on a cut window) ` +
      `with no rows, chips or fold; its one door opens Activity with all ${BIG_DRAWABLE} rows`
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
  await page.getByRole('heading', { name: 'Home', exact: true }).waitFor({ timeout: 10_000 });
  await page.getByText(INSTALLER_SENTENCE).waitFor({ timeout: 10_000 });
  if ((await page.getByRole('link', { name: 'Download Windows installer' }).count()) !== 0) {
    throw new Error('the unavailable installer still drew a download link');
  }
  step('with no installer staged the fleet states the sentence and draws no link');

  // **THE SIGN-IN PANEL HAS AN ADDRESS.** A cold boot at `#signin` draws the panel even with the
  // gate's session in force — no sign-out gesture first — and Back returns to the console still
  // signed in (audit-39 F6). The signed-in panel is headed by its state, not by a second invitation
  // to sign in (audit-40 F8).
  await page.goto(base + '/#signin', { waitUntil: 'domcontentloaded' });
  await page.getByRole('heading', { name: 'You are signed in' }).waitFor({ timeout: 10_000 });
  if (new URL(page.url()).hash !== '#signin') {
    throw new Error(`#signin was rewritten to ${new URL(page.url()).hash}`);
  }
  await page.getByText('This tab is already signed in.').waitFor({ timeout: 10_000 });
  await page.getByRole('button', { name: 'Back to the console' }).click();
  await page.getByRole('heading', { name: 'Home', exact: true }).waitFor({ timeout: 10_000 });
  await page.waitForFunction(() => window.location.hash === '#home', null, { timeout: 10_000 });
  step('the sign-in panel answers at #signin, and Back returns to the console still signed in');

  // **ONE FAILED READ IS STATED ONCE, ON ITS PAGE (audit-44 F3).** A fresh boot answers `/fleet`
  // with 500: Home owns the one statement and the one Retry, and the shell banner does not repeat
  // it. The banner used to be a fixed layer at the window's top; it now lives in the shell's row
  // between the header and the page, which the refused enrollment below measures.
  state.failFleet = true;
  await page.evaluate(() => localStorage.removeItem('summrise.selected.device'));
  await page.goto(base + '/', { waitUntil: 'domcontentloaded' });
  await page.getByRole('heading', { name: 'Home', exact: true }).waitFor({ timeout: 10_000 });
  const statement = page.getByText('The console could not read the machine list.', {
    exact: true
  });
  await statement.waitFor({ timeout: 10_000 });
  if ((await page.getByRole('alert').count()) !== 0) {
    throw new Error('a failed fleet read also drew the shell banner');
  }
  if ((await statement.count()) !== 1) {
    throw new Error('the failed fleet read did not state its failure exactly once');
  }
  if ((await page.getByRole('button', { name: 'Retry' }).count()) !== 1) {
    throw new Error('the failed fleet read did not own exactly one Retry');
  }
  const failureTopbar = await page.locator('.topbar').boundingBox();
  const failureNotice = await page.locator('.notice').first().boundingBox();
  if (
    !failureTopbar ||
    !failureNotice ||
    failureNotice.y < failureTopbar.y + failureTopbar.height
  ) {
    throw new Error('the failed read notice is not below the topbar');
  }
  step('a failed fleet read states one sentence on its page, with one Retry and no shell banner');

  // The banner's own placement: a refused enrollment (an action no page below owns) draws it in
  // the `alerts` row. The topbar is 52 px at this width; the banner's box must start below it, and
  // keep the measured 736 px width so it reads as the same object the audit photographed.
  state.failEnroll = true;
  await page
    .getByRole('main')
    .getByRole('button', { name: 'Add a machine' })
    .click();
  const banner = page.locator('.alert-banner').first();
  await banner.waitFor({ timeout: 10_000 });
  await banner.getByText('The console could not create an enrollment code.').waitFor({
    timeout: 10_000
  });
  const topbar = await page.locator('.topbar').boundingBox();
  const stack = await page.locator('.alert-stack').boundingBox();
  const bannerBox = await banner.boundingBox();
  if (!topbar || !stack || !bannerBox) {
    throw new Error('the topbar or the failure banner has no box');
  }
  const headerBottom = topbar.y + topbar.height;
  if (stack.y < headerBottom || bannerBox.y < headerBottom) {
    throw new Error(
      `the banner's row starts at y ${stack.y} and the banner at y ${bannerBox.y}, ` +
        `over a topbar that ends at ${headerBottom}`
    );
  }
  if (Math.abs(bannerBox.width - 736) > 1) {
    throw new Error(`the failure banner is ${bannerBox.width} px wide, not the measured 736`);
  }
  if ((await page.getByRole('alert').count()) !== 1) {
    throw new Error('the action failure drew more than one banner');
  }
  if ((await banner.getByRole('button', { name: 'Dismiss' }).count()) !== 1) {
    throw new Error('the action failure banner lost its Dismiss control');
  }
  step(
    `the action failure banner sits below the topbar (row y ${Math.round(stack.y)}, ` +
      `banner y ${Math.round(bannerBox.y)} > ${Math.round(headerBottom)}), 736 px wide`
  );

  // **A FAILURE PLANTED BY THIS SCRIPT IS NOT A CONSOLE ERROR.** The installer leg above answers
  // `/installer` with a 500 on purpose, and the browser reports every 500 as a console error; the
  // audit's job is to catch failures the PRODUCT causes, so the deliberate one is removed by the
  // sentence that describes it. Nothing else is filtered: a real 500 still fails the run.
  const planted = errors.filter((line) => /status of 500 \(Internal Server Error\)/.test(String(line)));
  if (planted.length > 0) {
    console.log(
      `browser-gate: ${planted.length} planted 500(s) excused — the installer leg breaks that read on purpose`
    );
  }
  const realErrors = errors.filter((line) => !planted.includes(line));
  if (realErrors.length > 0) throw new Error('console errors: ' + realErrors.join(' ;; '));
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
