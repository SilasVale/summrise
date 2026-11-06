#!/usr/bin/env node
// Screenshot the built React console without a live console.
//
//   node scripts/screenshot.mjs [tag]
//
// The page in `dist/index.html` is a single file, but it still asks its same-origin origin for JSON.
// This script starts a loopback HTTP server that serves that one file, then lets Playwright answer
// every console route with a fixture. No console, no credentials, no network.
//
// Output is `<tag>/` under `/tmp/console-react-shot` (override the parent with CONSOLE_SHOT_ROOT):
//
//   <page>.png          1280x900, light
//   <page>-dark.png     1280x900, dark
//   <page>-390.png      390x844, light (only devices and terminal)
//   index.html          a gallery
//
// Pages that need a device grant (Files, Terminal, Browser, Screen, Memory, Monitors, Runs, System,
// Events) are captured in their device-picker state. The script does not mint a grant and does not
// fake a terminal or an MCP answer; `main` is left to say what it could and could not show.
//
// The browser is the Playwright runtime already on the machine. Resolution follows the legacy
// `scripts/console-smoke.mjs`: createRequire, then a list of candidates. A dependency is not added to
// package.json.

import { createHash } from 'node:crypto';
import { mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import http from 'node:http';
import { createRequire } from 'node:module';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const WEB = path.resolve(HERE, '..');
const DIST_INDEX = path.join(WEB, 'dist', 'index.html');

const OUT_ROOT = path.resolve(process.env.CONSOLE_SHOT_ROOT || '/tmp/console-react-shot');
const RAW_TAG = process.argv[2] ?? 'latest';
const TAG_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;
if (!TAG_PATTERN.test(RAW_TAG)) {
  console.error(
    `screenshot: invalid tag ${JSON.stringify(RAW_TAG)} (use letters, digits, dot, dash or underscore)`
  );
  process.exit(2);
}
const OUT = path.resolve(OUT_ROOT, RAW_TAG);
if (!OUT.startsWith(OUT_ROOT + path.sep)) {
  console.error(`screenshot: refusing output outside ${OUT_ROOT}: ${OUT}`);
  process.exit(2);
}

// ---------------------------------------------------------------------------------------------
// Playwright resolution. `playwright-core` is a tool here, not an application dependency, so it is
// looked up the same way the older console smoke test did and never installed by this script.
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
    'screenshot: no Playwright found. Set PLAYWRIGHT_CORE_DIR to the package directory, or NODE_PATH.'
  );
  console.error(`screenshot: tried:\n  ${PLAYWRIGHT_CANDIDATES.join('\n  ')}`);
  process.exit(2);
}

// ---------------------------------------------------------------------------------------------
// Fixtures. Three devices, two online and one offline; a mixed build; host facts; audit lines;
// token metadata; the tool catalogue; and the routes the dialogs would read. `NOW` is fixed for the
// run so two passes cannot disagree about what "four minutes ago" means.
// ---------------------------------------------------------------------------------------------

const NOW = Date.now();
const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const GIB = 1024 ** 3;
const TIB = 1024 ** 4;

/** A stable 64-hex id for a label: the console's ids and token record ids are that shape. */
function hexId(label) {
  return createHash('sha256').update(`summrise-console-screenshot:${label}`).digest('hex');
}

const DEVICE = {
  workshop: hexId('device/workshop'),
  attic: hexId('device/attic'),
  studio: hexId('device/studio'),
  legacy: hexId('device/legacy'),
  stale: hexId('device/stale')
};
const TOKEN = {
  workshop: hexId('token/workshop'),
  attic: hexId('token/attic'),
  fleet: hexId('token/fleet')
};

// Mirrors `src/useToolCatalogue.ts`, which mirrors the device's registry. These are the names a grant
// form offers; /tools is the console's served copy, with each tool's destructive flag.
const TOOL_NAMES = [
  'files.read',
  'files.write',
  'files.mkdir',
  'files.list',
  'memory.append',
  'memory.update',
  'memory.delete',
  'memory.search',
  'memory.list',
  'memory.export',
  'terminal.create',
  'terminal.write',
  'terminal.read',
  'terminal.resize',
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

// What a plugin exposes once it is running: two friendly tools, and the screenshot of a hub with
// something a built-in cannot do.
const PLUGIN_TOOL_NAMES = ['hello.greet', 'hello.echo'];
const DESTRUCTIVE_TOOLS = new Set([
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

/// The live catalogue for one device. `legacy` is an older build: its list has no `plugins.*`, which
/// is exactly how the page detects the "newer Summrise" state.
function liveToolsFor(deviceId) {
  const names =
    deviceId === DEVICE.legacy
      ? TOOL_NAMES.filter((name) => !name.startsWith('plugins.'))
      : TOOL_NAMES;
  const tools = names.map((name) => ({
    name,
    description: `${name} on this machine`,
    inputSchema: { type: 'object', properties: {}, additionalProperties: false },
    source: 'builtin',
    capability: name.split('.')[0],
    destructive: DESTRUCTIVE_TOOLS.has(name)
  }));
  if (deviceId === DEVICE.workshop) {
    for (const name of PLUGIN_TOOL_NAMES) {
      tools.push({
        name,
        description: `${name} from the Hello plugin`,
        inputSchema: {
          type: 'object',
          properties:
            name === 'hello.greet'
              ? { name: { type: 'string', description: 'Who to greet' } }
              : { text: { type: 'string', description: 'Text to echo' } },
          required: name === 'hello.echo' ? ['text'] : []
        },
        source: 'plugin:hello',
        capability: 'hello',
        destructive: false
      });
    }
  }
  return tools;
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

const DEVICES = [
  {
    id: DEVICE.workshop,
    name: 'workshop',
    online: true,
    version: '0.3.42',
    last_seen_ms: NOW - 12_000,
    last_frame_ms: NOW - 12_000,
    registered_at_ms: NOW - 41 * DAY,
    last_attempt: { attempts: 2, reason: 'connection reset by peer' },
    host: {
      os: { name: 'Windows 11 Pro', version: '10.0.26100', arch: 'x86_64' },
      machine: 'DESKTOP-WORKSHOP',
      uptime_s: 3 * 86_400 + 4 * 3_600 + 18 * 60,
      cpu: { cores: 16 },
      memory: { total_bytes: 64 * GIB, available_bytes: 27 * GIB + 412 * 1024 * 1024 },
      disks: [
        { name: 'C:', total_bytes: 1024 * GIB, free_bytes: 312 * GIB },
        { name: 'D:', total_bytes: 2 * TIB, free_bytes: Math.round(1.34 * TIB) }
      ],
      sessions: [
        { id: 1, user: null, state: 'connected' },
        { id: 2, user: 'operator', state: 'active' }
      ],
      helper: { registered: true, pid: 4812, session: 2 },
      update: {
        current: { version: '0.3.42', channel: 'stable' },
        previous: { version: '0.3.41', channel: 'stable' },
        pending: null,
        last_swap: { outcome: 'applied', version: '0.3.42', at_ms: NOW - 2 * DAY - 3 * HOUR },
        repairs: [
          {
            at_ms: NOW - 9 * DAY,
            from: 'baseline',
            sha256: hexId('repair/workshop')
          }
        ]
      }
    }
  },
  {
    id: DEVICE.attic,
    name: 'attic',
    online: true,
    version: '0.3.42',
    last_seen_ms: NOW - 38_000,
    last_frame_ms: NOW - 38_000,
    registered_at_ms: NOW - 12 * DAY,
    last_attempt: null,
    host: {
      os: { name: 'Ubuntu', version: '24.04.1 LTS', arch: 'x86_64' },
      machine: 'attic',
      uptime_s: 11 * 86_400 + 6 * 3_600,
      cpu: { cores: 8 },
      memory: { total_bytes: 16 * GIB, available_bytes: 9 * GIB + 700 * 1024 * 1024 },
      disks: [{ name: '/', total_bytes: 512 * GIB, free_bytes: 241 * GIB }],
      sessions: [],
      helper: { registered: false, pid: null, session: null },
      update: {
        current: { version: '0.3.42', channel: 'stable' },
        previous: null,
        pending: { version: '0.3.43', channel: 'canary' },
        last_swap: { outcome: 'applied', version: '0.3.42', at_ms: NOW - 6 * HOUR },
        repairs: []
      }
    }
  },
  {
    id: DEVICE.studio,
    name: 'studio',
    online: false,
    version: '0.3.30',
    last_seen_ms: NOW - 3 * HOUR - 21 * MINUTE,
    last_frame_ms: NOW - 3 * HOUR - 21 * MINUTE,
    registered_at_ms: NOW - 80 * DAY,
    last_attempt: { attempts: 5, reason: 'timed out waiting for the channel hello' },
    host: {
      os: { name: 'macOS', version: '15.1', arch: 'arm64' },
      machine: 'studio.local',
      uptime_s: 2 * 86_400 + 5 * 3_600,
      cpu: { cores: 10 },
      memory: { total_bytes: 32 * GIB, available_bytes: 6 * GIB },
      disks: [{ name: 'Macintosh HD', total_bytes: 1 * TIB, free_bytes: 194 * GIB }],
      sessions: [{ id: 1, user: 'operator', state: 'disconnected' }],
      helper: { registered: false, pid: null, session: null },
      update: {
        current: { version: '0.3.30', channel: 'stable' },
        previous: { version: '0.3.29', channel: 'stable' },
        pending: null,
        last_swap: {
          outcome: 'rolled_back',
          version: '0.3.30',
          at_ms: NOW - 18 * DAY,
          reason: 'the new build did not answer the probe'
        },
        repairs: []
      }
    }
  },
  {
    id: DEVICE.stale,
    name: 'stale',
    online: true,
    version: '0.3.42',
    last_seen_ms: NOW - 8_000,
    last_frame_ms: NOW - 8_000,
    registered_at_ms: NOW - 60 * DAY,
    host: {
      os: { name: 'Windows 11 Pro', version: '10.0.26100', arch: 'x86_64' },
      machine: 'DESKTOP-STALE',
      uptime_s: 2 * 86_400,
      cpu: { cores: 12 },
      memory: { total_bytes: 32 * GIB, available_bytes: 11 * GIB },
      disks: [{ name: 'C:', total_bytes: 512 * GIB, free_bytes: 120 * GIB }]
    }
  },
  {
    id: DEVICE.legacy,
    name: 'legacy',
    online: true,
    version: '0.3.18',
    last_seen_ms: NOW - 26_000,
    last_frame_ms: NOW - 26_000,
    registered_at_ms: NOW - 120 * DAY,
    host: {
      os: { name: 'Windows 11 Pro', version: '10.0.22631', arch: 'x86_64' },
      machine: 'DESKTOP-LEGACY',
      uptime_s: 9 * 86_400,
      cpu: { cores: 8 },
      memory: { total_bytes: 32 * GIB, available_bytes: 18 * GIB },
      disks: [{ name: 'C:', total_bytes: 512 * GIB, free_bytes: 90 * GIB }]
    }
  }
];

const TOKENS = [
  {
    id: TOKEN.workshop,
    name: 'workshop-dsh',
    device_id: DEVICE.workshop,
    tools: [
      'files.list',
      'files.read',
      'terminal.create',
      'terminal.write',
      'terminal.read',
      'terminal.screen',
      'browser.list',
      'browser.screenshot',
      'system.process_list',
      'state.get'
    ],
    allow_destructive: false,
    created_at_ms: NOW - 12 * DAY,
    last_used_ms: NOW - 4 * MINUTE,
    expires_at_ms: null
  },
  {
    id: TOKEN.attic,
    name: 'attic-ops',
    device_id: DEVICE.attic,
    tools: ['runs.exec', 'runs.list', 'system.process_list', 'system.net_test', 'host.facts'],
    allow_destructive: true,
    created_at_ms: NOW - 5 * DAY,
    last_used_ms: null,
    expires_at_ms: NOW + 26 * DAY
  },
  {
    id: TOKEN.fleet,
    name: 'dsh-fleet',
    device_id: '',
    device_ids: [DEVICE.workshop, DEVICE.attic, DEVICE.studio],
    tools: [
      'files.list',
      'files.read',
      'memory.search',
      'memory.list',
      'monitor.list',
      'monitor.status',
      'runs.list',
      'system.process_list',
      'host.facts'
    ],
    allow_destructive: false,
    created_at_ms: NOW - 2 * DAY,
    last_used_ms: NOW - 26 * MINUTE,
    expires_at_ms: null
  }
];

/** The console writes a sentence for every stored line; the stub must too, or the page renders codes. */
const LINE_SENTENCES = {
  'runs.exec': 'Ran a command',
  'terminal.write': 'Typed into a terminal',
  'terminal.list': 'Listed the terminal sessions',
  'files.read': 'Read a file',
  'files.list': 'Listed a folder',
  'memory.search': 'Searched its memory',
  'state.get': "Read the machine's state",
  'browser.screenshot': 'Read the browser\'s page',
  'system.net_test': 'Tested a connection',
  'tools/list': "Listed the machine's tools"
};
const KIND_SENTENCES = {
  grant: 'Issued a grant',
  token: 'Issued a client token',
  token_revoke: 'Revoked a client token',
  register: 'A machine registered',
  enroll: 'Created an enrollment code',
  denied: 'Refused a request'
};

/** One audit line, newest first in the list below. `kind` is the console's closed set. */
function auditLine(
  agoMs,
  kind,
  { device, status = 200, tool = null, detail = null, token = null, caller = null } = {}
) {
  const line = { ts_ms: NOW - agoMs, kind, status };
  if (device) line.device_id = device;
  if (tool) line.tool = tool;
  if (detail) line.detail = detail;
  if (token) line.token_id = token;
  // WHO MADE THE CALL. The fixture's proxied rows are a client's; the console's own terminal reads
  // say so explicitly, and the timeline marks them with its quiet word.
  if (kind === 'mcp' || caller) line.caller = caller ?? 'client';
  line.sentence =
    (tool && LINE_SENTENCES[tool]) ?? KIND_SENTENCES[kind] ?? (tool ? `Used \`${tool}\`` : 'Listed the tools');
  return line;
}

const AUDIT = [
  auditLine(50_000, 'mcp', {
    device: DEVICE.workshop,
    tool: 'runs.exec',
    detail: 'run r-1 exited 0 in 42 ms',
    token: TOKEN.workshop
  }),
  auditLine(2 * MINUTE, 'mcp', { device: DEVICE.workshop, tool: 'terminal.write' }),
  // The console's own read: the terminal page's ten-second `terminal.list`. It is in the record and
  // in the timeline, marked "console", and the Now band does not count it as the AI's work.
  auditLine(4 * MINUTE, 'mcp', { device: DEVICE.workshop, tool: 'terminal.list', caller: 'console' }),
  auditLine(5 * MINUTE, 'mcp', { device: DEVICE.attic, tool: 'files.list', token: TOKEN.fleet }),
  auditLine(8 * MINUTE, 'mcp', { device: DEVICE.attic, tool: 'runs.exec', token: TOKEN.attic }),
  auditLine(13 * MINUTE, 'mcp', {
    device: DEVICE.workshop,
    tool: 'memory.search',
    detail: 'jsonrpc error -32002',
    token: TOKEN.fleet
  }),
  auditLine(21 * MINUTE, 'mcp', { device: DEVICE.studio, tool: 'state.get' }),
  auditLine(34 * MINUTE, 'grant', { device: DEVICE.workshop, detail: 'allow_destructive=true' }),
  auditLine(47 * MINUTE, 'token', { device: DEVICE.attic, detail: 'allow_destructive=true' }),
  auditLine(1 * HOUR + 4 * MINUTE, 'mcp', {
    device: DEVICE.workshop,
    tool: 'browser.screenshot',
    token: TOKEN.workshop
  }),
  auditLine(1 * HOUR + 18 * MINUTE, 'mcp', {
    device: DEVICE.attic,
    tool: 'system.net_test',
    token: TOKEN.attic
  }),
  auditLine(1 * HOUR + 42 * MINUTE, 'denied', {
    device: DEVICE.studio,
    status: 403,
    detail: 'grant_expired'
  }),
  auditLine(2 * HOUR + 3 * MINUTE, 'register', { device: DEVICE.attic }),
  auditLine(3 * HOUR + 12 * MINUTE, 'mcp', {
    device: DEVICE.workshop,
    tool: 'runs.exec',
    detail: 'jsonrpc error -32001',
    token: TOKEN.workshop
  }),
  auditLine(4 * HOUR + 8 * MINUTE, 'token_revoke', { device: DEVICE.studio, detail: 'revoked' }),
  auditLine(5 * HOUR + 31 * MINUTE, 'enroll'),
  auditLine(6 * HOUR + 2 * MINUTE, 'denied', {
    device: DEVICE.workshop,
    status: 403,
    detail: 'grant_expired'
  }),
  auditLine(7 * HOUR + 19 * MINUTE, 'grant', { device: DEVICE.attic, detail: 'allow_destructive=false' }),
  auditLine(9 * HOUR + 44 * MINUTE, 'register', { device: DEVICE.workshop }),
  auditLine(12 * HOUR + 6 * MINUTE, 'mcp', {
    device: DEVICE.attic,
    tool: 'monitor.probe',
    token: TOKEN.fleet
  }),
  auditLine(18 * HOUR + 25 * MINUTE, 'token', { device: DEVICE.workshop, detail: 'allow_destructive=false' })
];

/**
 * P2-12's long day: 1000 audit lines across one calendar day, newest first. The page asks the
 * console for its cap, so this is what a busy machine's "Today" is. Each sentence carries the row
 * number, which lets the harness wait for the LAST row by name and not only by count.
 */
const BIG_AUDIT = (() => {
  const dayStart = new Date(NOW);
  dayStart.setHours(0, 0, 0, 0);
  // Spread the day's lines from a second ago back towards midnight. The step is at least 1 ms so no
  // two rows can share the audit's (time, tool, status) identity.
  const step = Math.max(1, Math.floor((NOW - dayStart.getTime() - MINUTE) / 999));
  const tools = ['files.read', 'files.list', 'runs.exec', 'terminal.write', 'memory.search', 'system.net_test'];
  return Array.from({ length: 1000 }, (_, index) => {
    const tool = tools[index % tools.length];
    const line = auditLine(1_000 + index * step, 'mcp', {
      device: index % 3 === 0 ? DEVICE.attic : DEVICE.workshop,
      tool,
      ...(index % 53 === 0 ? { detail: 'jsonrpc error -32002' } : {})
    });
    line.sentence = `Call ${index + 1} · ${line.sentence}`;
    return line;
  });
})();

/** The audit the stub serves. The activity-1000 scenario swaps the long day in and puts it back. */
let stubAudit = AUDIT;

function denialLine(agoMs, detail, status) {
  return { ts_ms: NOW - agoMs, kind: 'denied', status, detail };
}

const DENIALS = [
  denialLine(35_000, 'bad_operator_token', 401),
  denialLine(2 * MINUTE, 'missing_session', 401),
  denialLine(6 * MINUTE, 'unknown_device', 404),
  denialLine(14 * MINUTE, 'bad_device_token', 401),
  denialLine(28 * MINUTE, 'expired_session', 401),
  denialLine(46 * MINUTE, 'suppressed 17', 429),
  denialLine(1 * HOUR + 7 * MINUTE, 'bad_registration_proof', 401)
];

const CONNECTIONS = {
  [DEVICE.workshop]: [
    { id: 'conn-workshop-shell', name: 'Workshop PowerShell', kind: 'pty' },
    {
      id: 'conn-workshop-ssh',
      name: 'Workshop SSH',
      kind: 'ssh',
      host: 'workshop.lan',
      port: 22,
      user: 'operator'
    }
  ],
  [DEVICE.attic]: [
    {
      id: 'conn-attic-serial',
      name: 'Attic console',
      kind: 'serial',
      port_path: '/dev/ttyUSB0',
      baud: 115200
    }
  ],
  [DEVICE.studio]: []
};

const SESSION = { authenticated: true, email: 'operator@summrise.test', via: 'token' };
const HEALTH = { version: '0.3.218', devices: DEVICES.length };
const MESH = { configured: true, broker: 'https://mesh.summrise.test' };

// ---------------------------------------------------------------------------------------------
// The loopback server. Only `/` and the page's icon link are real responses; every console route is
// answered by Playwright before the request reaches this server.
// ---------------------------------------------------------------------------------------------

// A valid 1x1 PNG for the page's `<link rel="icon">`. Without it Chromium logs a missing resource,
// which is noise unrelated to the console.
const ICON_PNG = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
  'base64'
);

const indexHtml = await readFile(DIST_INDEX).catch((error) => {
  console.error(`screenshot: cannot read ${DIST_INDEX}: ${error.message}`);
  console.error('screenshot: build the page first: cd device/console/web && npm run build');
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
      response.end(ICON_PNG);
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
  if (!address || typeof address === 'string') {
    throw new Error('the loopback server did not report a port');
  }
  return { server, origin: `http://127.0.0.1:${address.port}` };
}

async function stopServer(server) {
  if (!server) return;
  await new Promise((resolve) => {
    server.close(() => resolve());
    if (typeof server.closeAllConnections === 'function') server.closeAllConnections();
  });
}

// ---------------------------------------------------------------------------------------------
// Stub routes. A null answer means "no stub": the route handler turns that into a 404, records a
// failed API request, and the run fails. `/mcp` answers exactly ONE tool, `terminal.list`, with an
// empty list: the Terminal page now reads the device's sessions on mount, and "no sessions" is the
// truthful state of a harness that does not fake a shell. Any other tool is still a loud failure.
// ---------------------------------------------------------------------------------------------

const API_PATHS = new Set([
  '/session',
  '/auth',
  '/logout',
  '/fleet',
  '/audit',
  '/denials',
  '/health',
  '/tokens',
  '/tools',
  '/mesh',
  '/enroll',
  '/grants',
  '/mcp',
  '/mcp/all'
]);

function isApiPath(pathname) {
  return (
    API_PATHS.has(pathname) ||
    pathname === '/events' ||
    pathname.startsWith('/fleet/') ||
    pathname.startsWith('/tokens/')
  );
}

function json(body, status = 200) {
  return { status, body };
}

function noContent() {
  return { status: 204, body: null };
}

function requestBody(request) {
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
  if (method === 'GET' && pathname === '/audit') {
    // `/audit` honours `limit` the way the console does: missing is 20, above 1000 is clamped. The
    // activity-1000 scenario can therefore show its 1000 rows only because the page asked for 1000;
    // a window that slid back to 200 would fail the wait, not quietly photograph a slice.
    const raw = Number(url.searchParams.get('limit') ?? 20);
    const limit = Number.isFinite(raw) ? Math.max(0, Math.min(Math.trunc(raw), 1000)) : 20;
    // `?device=` is applied during the read, like the console's own scan: a selected machine's
    // window is its lines, not a slice of the fleet's that another machine may have filled.
    const device = url.searchParams.get('device');
    const window = device === null ? stubAudit : stubAudit.filter((line) => line.device_id === device);
    return json(window.slice(0, limit));
  }
  // The page follows the audit as it happens; the stub answers with one snapshot, in the order the
  // console sends it (oldest first), so the Now band and the timeline are photographed live rather
  // than stuck on "Reconnecting".
  if (method === 'GET' && pathname === '/events') {
    const lines = stubAudit.slice(0, 8).reverse();
    return { status: 200, sse: `event: snapshot\ndata: ${JSON.stringify({ lines })}\n\n` };
  }
  if (method === 'GET' && pathname === '/denials') return json(DENIALS);
  if (method === 'GET' && pathname === '/tools') return json(TOOL_NAMES.map((name) => ({ name })));
  if (method === 'GET' && pathname === '/mesh') return json(MESH);
  if (method === 'POST' && pathname === '/auth') {
    return json({ token: 'srm_screenshot_stub', expires_at_ms: NOW + HOUR });
  }
  if (method === 'POST' && pathname === '/logout') return noContent();
  if (method === 'POST' && pathname === '/enroll') {
    return json({ code: '7F3K-9Q2M', expires_at_ms: NOW + 10 * MINUTE });
  }
  if (method === 'POST' && pathname === '/mcp') {
    const body = requestBody(request);
    const name = body?.request?.params?.name;
    // THE OFFLINE FIXTURE ANSWERS LIKE AN OFFLINE DEVICE. `studio` is the machine the fleet marks
    // down; a proxy call to it is the console's 503 (`device reconnecting`), not a successful empty
    // answer — otherwise the photographs would show a working page on a machine that is not there.
    if (body?.device_id === DEVICE.studio) {
      return json({ error: 'device reconnecting', retry_after_ms: 1000 }, 503);
    }
    // The workspace photographs need the two calls a pty open makes; the ANSWER is empty output, not
    // a fake shell. Every other tool is still a loud failure.
    if (name === 'terminal.list') return json({ result: { content: [{ type: 'text', text: '[]' }] } });
    if (name === 'system.process_list') {
      return json({ result: { content: [{ type: 'text', text: '[]' }] } });
    }
    // The browser view is photographed without a runtime: the empty sentence and the install/retry
    // affordances are the state an operator hits on a clean machine.
    if (name === 'browser.list') {
      return json({ result: { content: [{ type: 'text', text: '[]' }] } });
    }
    if (name === 'hello.echo') {
      const text = String(body?.request?.params?.arguments?.text ?? '');
      if (text === 'error') {
        return json({
          result: {
            isError: true,
            content: [{ type: 'text', text: 'the plugin refused: that text is not allowed' }]
          }
        });
      }
      return json({
        result: { content: [{ type: 'text', text: JSON.stringify({ echo: text }) }] }
      });
    }
    if (name === 'plugins.list') {
      const deviceId = body?.device_id;
      const plugins = deviceId === DEVICE.workshop ? PLUGINS : [];
      return json({ result: { content: [{ type: 'text', text: JSON.stringify(plugins) }] } });
    }
    if (name === 'plugins.inspect') {
      const path = String(body?.request?.params?.arguments?.path ?? '');
      if (path.toLowerCase().includes('hello')) {
        return json({
          result: {
            content: [
              {
                type: 'text',
                text: JSON.stringify({
                  id: 'com.example.hello',
                  namespace: 'hello',
                  name: 'Hello',
                  version: '0.1.0',
                  description: 'Two friendly tools, and the reference for writing a plugin.'
                })
              }
            ]
          }
        });
      }
      return json({
        result: {
          isError: true,
          content: [
            {
              type: 'text',
              text: JSON.stringify({
                code: 'NO_MANIFEST',
                message: 'No plugin.json in that folder.',
                action: 'Open the guide'
              })
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
                  started_at_ms: NOW - 5 * 60_000,
                  stdout_bytes: 6,
                  stderr_bytes: 0
                }
              ])
            }
          ]
        }
      });
    }
    if (name === 'runs.get') {
      console.error('runs.get called', JSON.stringify(requestBody(request)));
      return json({
        result: {
          content: [
            {
              type: 'text',
              text: JSON.stringify({
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
              })
            }
          ]
        }
      });
    }
    if (name === 'terminal.create') {
      return json({ result: { content: [{ type: 'text', text: JSON.stringify({ id: 'shot-pty-1' }) }] } });
    }
    if (name === 'terminal.resize') {
      return json({ result: { content: [{ type: 'text', text: JSON.stringify({ rows: 24, cols: 80 }) }] } });
    }
    if (name === 'terminal.read') {
      return json({ result: { content: [{ type: 'text', text: JSON.stringify({ data: '', cursor: 0 }) }] } });
    }
    if (name === 'terminal.close') {
      return json({ result: { content: [{ type: 'text', text: '{}' }] } });
    }
    return null;
  }
  if (method === 'POST' && pathname === '/grants') {
    const body = requestBody(request);
    return json({
      grant: {
        device_id: body.device_id,
        tools: body.tools ?? [],
        expires_in_s: body.expires_in_s,
        allow_destructive: body.allow_destructive
      },
      signature: 'screenshot-stub-signature'
    });
  }

  if (method === 'GET' && pathname === '/fleet') return json(DEVICES);

  const liveTools = /^\/fleet\/([^/]+)\/tools$/.exec(pathname);
  if (liveTools && method === 'GET') {
    const id = decodeURIComponent(liveTools[1]);
    if (id === DEVICE.stale) {
      return json({ error: 'tools_unavailable', message: "the console could not read this machine's tools" }, 500);
    }
    return json({ tools: liveToolsFor(id) });
  }

  const fleet = /^\/fleet\/([^/]+)(?:\/(name|connections|mesh))?$/.exec(pathname);
  if (fleet) {
    const deviceId = decodeURIComponent(fleet[1]);
    const leaf = fleet[2];
    if (method === 'DELETE' && !leaf) return noContent();
    if (method === 'POST' && leaf === 'name') {
      const body = requestBody(request);
      return json({ id: deviceId, name: body.name ?? null });
    }
    if (leaf === 'connections' && method === 'GET') {
      return json(CONNECTIONS[deviceId] ?? []);
    }
    if (leaf === 'connections' && method === 'POST') {
      return json({ id: 'conn-screenshot-stub', ...requestBody(request) }, 201);
    }
    if (method === 'GET' && leaf === 'mesh') {
      return json({
        configured: true,
        reachable: deviceId !== DEVICE.studio,
        candidates: deviceId === DEVICE.studio ? 0 : 2
      });
    }
    return null;
  }

  const connection = /^\/fleet\/([^/]+)\/connections\/([^/]+)$/.exec(pathname);
  if (connection && method === 'DELETE') return noContent();

  if (pathname === '/tokens' && method === 'GET') return json(TOKENS);
  if (pathname === '/tokens' && method === 'POST') {
    const body = requestBody(request);
    return json(
      {
        ...TOKENS[0],
        id: TOKEN.fleet,
        name: body.name ?? 'screenshot-stub',
        token: `srm_${'0'.repeat(64)}`
      },
      201
    );
  }
  if (/^\/tokens\/[^/]+\/rotate$/.test(pathname) && method === 'POST') {
    return json({ ...TOKENS[0], id: TOKEN.fleet, token: `srm_${'0'.repeat(64)}` }, 201);
  }
  if (/^\/tokens\/[^/]+$/.test(pathname) && method === 'DELETE') return noContent();

  return null;
}

function pathAndSearch(url) {
  return url.pathname + url.search;
}

async function fulfill(route, reply) {
  const headers = { 'cache-control': 'no-store' };
  if (reply.status === 204) {
    await route.fulfill({ status: 204, headers });
    return;
  }
  if (reply.sse !== undefined) {
    await route.fulfill({
      status: reply.status,
      headers: { ...headers, 'content-type': 'text/event-stream' },
      body: reply.sse
    });
    return;
  }
  await route.fulfill({
    status: reply.status,
    headers: { ...headers, 'content-type': 'application/json; charset=utf-8' },
    body: JSON.stringify(reply.body)
  });
}

// ---------------------------------------------------------------------------------------------
// Browser-side helpers.
// ---------------------------------------------------------------------------------------------

const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const PAGES = [
  {
    id: 'home',
    label: 'Home',
    heading: 'Machines',
    state: 'the fleet zero state: each machine\'s build, reachability and the way in'
  },
  {
    id: 'tools',
    label: 'Device',
    tab: 'Tools',
    heading: 'Tools',
    state:
      'the machine\'s one answer: the exec form first, then the live tool catalogue and the plugin list'
  },
  {
    id: 'activity',
    label: 'Activity',
    heading: 'Activity',
    state: 'the timeline: machine chips and one search over the record'
  },
  {
    id: 'terminal',
    label: 'Device',
    tab: 'Terminal',
    heading: 'Terminal',
    state: 'the device workspace: terminal, the most direct way to take over'
  },
  {
    id: 'screen',
    label: 'Device',
    tab: 'Screen',
    heading: 'Screen',
    state: 'the device workspace: the screen tab'
  },
  {
    id: 'browser',
    label: 'Device',
    tab: 'Screen',
    view: 'Browser',
    heading: 'Screen',
    state: 'the device workspace: the browser view, reachable as its own fragment'
  },
  {
    id: 'files',
    label: 'Device',
    tab: 'Files',
    heading: 'Files',
    state: 'the device workspace: the files tab'
  },
  {
    id: 'runs',
    label: 'Device',
    tab: 'Runs',
    heading: 'Runs',
    state:
      'the device workspace: the run history — the exec form lives under Tools → Run a command'
  },
  {
    id: 'machine',
    label: 'Device',
    tab: 'Machine',
    heading: 'Machine',
    state: 'the device workspace: the machine tab'
  }
];

const PASSES = [];
for (const width of [1280, 800, 390, 360]) {
  for (const theme of ['light', 'dark']) {
    const suffix =
      width === 1280 ? (theme === 'light' ? '' : '-dark') : `-${width}${theme === 'dark' ? '-dark' : ''}`;
    PASSES.push({
      key: `${width}-${theme}`,
      width,
      viewport: { width, height: width < 640 ? 844 : 900 },
      colorScheme: theme,
      suffix,
      pages: PAGES
    });
  }
}

function newState(pass) {
  return {
    pass: pass.key,
    label: 'boot',
    pending: new Set(),
    pageErrors: [],
    apiFailures: [],
    consoleErrors: []
  };
}

async function installPage(page, state, origin) {
  page.on('pageerror', (error) => {
    state.pageErrors.push({ page: state.label, message: error.stack || error.message });
  });
  page.on('console', (message) => {
    if (message.type() === 'error') {
      state.consoleErrors.push({ page: state.label, message: message.text() });
    }
  });
  page.on('request', (request) => {
    try {
      if (isApiPath(new URL(request.url()).pathname)) state.pending.add(request);
    } catch {
      /* not an HTTP request */
    }
  });
  const done = (request) => state.pending.delete(request);
  page.on('requestfinished', done);
  page.on('requestfailed', (request) => {
    done(request);
    try {
      if (isApiPath(new URL(request.url()).pathname)) {
        state.apiFailures.push({
          page: state.label,
          method: request.method(),
          url: pathAndSearch(new URL(request.url())),
          status: null,
          detail: request.failure()?.errorText ?? 'request failed'
        });
      }
    } catch {
      /* a request we cannot name is not an API failure we can report */
    }
  });

  await page.route('**/*', async (route) => {
    const request = route.request();
    let url;
    try {
      url = new URL(request.url());
    } catch {
      await route.abort('blockedbyclient').catch(() => {});
      return;
    }

    // The page is served by the loopback server; its icon is too. Everything else is the console's
    // JSON surface and must be answered here.
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

    try {
      if (url.pathname === '/mcp') {
        const body = requestBody(request);
        const name = body?.request?.params?.name;
        const text = body?.request?.params?.arguments?.text;
        if (name === 'hello.echo' && text === 'slow') await delay(1200);
      }
      const reply = replyFor(request, url);
      if (reply) {
        await fulfill(route, reply);
        return;
      }
      if (isApiPath(url.pathname)) {
        state.apiFailures.push({
          page: state.label,
          method: request.method(),
          url: pathAndSearch(url),
          status: 404,
          detail: 'no stub for this API route'
        });
        await fulfill(route, json({ error: 'screenshot_stub_missing', path: url.pathname }, 404));
        return;
      }
      await route.continue();
    } catch (error) {
      state.apiFailures.push({
        page: state.label,
        method: request.method(),
        url: pathAndSearch(url),
        status: null,
        detail: `route handler failed: ${error instanceof Error ? error.message : String(error)}`
      });
      await route
        .fulfill({
          status: 500,
          headers: { 'content-type': 'application/json; charset=utf-8' },
          body: JSON.stringify({ error: 'screenshot_route_failed' })
        })
        .catch(() => {});
    }
  });
}

async function settle(page, state, quietMs = 150) {
  const deadline = Date.now() + 5_000;
  while (state.pending.size > 0 && Date.now() < deadline) {
    await delay(25);
  }
  await delay(quietMs);
}

async function assertPage(page, meta) {
  // THE PAGE NAME IS DRAWN ONCE, IN THE CONTENT. The top bar used to repeat it as a crumb; removing
  // that duplicate means this gate now checks the rail's `aria-current` and the page's own heading
  // (below) instead of two copies of the same string agreeing.
  try {
    await page.waitForFunction(
      ({ label }) => {
        const active = document.querySelector('nav.rail .rail-btn[aria-current="page"]');
        return active !== null && active.getAttribute('title') === label;
      },
      { label: meta.label },
      { timeout: 5_000 }
    );
  } catch {
    const active = await page
      .locator('nav.rail .rail-btn[aria-current="page"]')
      .getAttribute('title')
      .catch(() => null);
    throw new Error(
      `navigation did not land on ${meta.label} (aria-current=${active ?? 'none'})`
    );
  }

  try {
    await page
      .locator('main')
      .getByRole('heading', { name: meta.heading, exact: true, level: 1 })
      .first()
      .waitFor({ state: 'visible', timeout: 5_000 });
  } catch {
    throw new Error(`the ${meta.label} page rendered without its "${meta.heading}" heading`);
  }
}

async function goTo(page, meta) {
  // `title` carries the destination label: Rail.tsx sets `title={PAGE_LABELS[entry]}`.
  const button = page.locator(`nav.rail button[title=${JSON.stringify(meta.label)}]`);
  await button.waitFor({ state: 'visible', timeout: 5_000 });
  await button.click();
  if (meta.tab) {
    const tab = page.locator('nav.device-tabs').getByRole('button', { name: meta.tab, exact: true });
    await tab.waitFor({ state: 'visible', timeout: 5_000 });
    await tab.click();
  }
  if (meta.view) {
    const view = page.getByRole('button', { name: meta.view, exact: true });
    await view.waitFor({ state: 'visible', timeout: 5_000 });
    await view.click();
  }
  await assertPage(page, meta);
}

async function boot(page, state, origin) {
  const response = await page.goto(origin + '/', {
    waitUntil: 'domcontentloaded',
    timeout: 30_000
  });
  if (!response || !response.ok()) {
    throw new Error(`GET / answered ${response ? response.status() : 'nothing'}`);
  }
  if (await page.locator('input#token').isVisible().catch(() => false)) {
    throw new Error('the page showed the sign-in form; /session was not accepted as authenticated');
  }
  await page.locator('nav.rail').waitFor({ state: 'visible', timeout: 10_000 });
  try {
    // The fleet is the default; wait for its one h1 and for the fleet read to land.
    await page.getByRole('heading', { name: 'Machines', exact: true, level: 1 }).waitFor({
      state: 'visible',
      timeout: 10_000
    });
    await page.waitForFunction(
      () => document.querySelectorAll('.fleet-row').length > 0 || document.querySelector('.empty-state'),
      null,
      { timeout: 10_000 }
    );
  } catch {
    const text = await page.locator('main').innerText().catch(() => '(no main)');
    throw new Error(`the fleet did not load on Home: ${JSON.stringify(text.replace(/\s+/g, ' ').slice(0, 300))}`);
  }
  await settle(page, state);
}

async function chooseMachine(page, state, name) {
  await page.locator('.machine-menu > summary').click();
  const menu = page.locator('.machine-pop');
  await menu.getByRole('button', { name: new RegExp(`^${name}`) }).click();
  await settle(page, state, 350);
}

async function capturePage(page, state, meta, pass) {
  await settle(page, state);
  if (process.env.DEBUG_LAYOUT) {
    const overflow = await page.evaluate(() => {
      const vw = document.documentElement.clientWidth;
      const bad = [...document.querySelectorAll('*')]
        .map((el) => {
          const r = el.getBoundingClientRect();
          return {
            tag: el.tagName,
            cls: typeof el.className === 'string' ? el.className : el.className?.baseVal ?? '',
            left: Math.round(r.left),
            right: Math.round(r.right),
            width: Math.round(r.width)
          };
        })
        .filter((x) => x.right > vw + 1 || x.left < -1 || x.width > vw + 1)
        .slice(0, 25);
      return { vw, scrollWidth: document.documentElement.scrollWidth, bad };
    });
    console.log('DEBUG ' + JSON.stringify({ pass: pass.key, page: meta.id, ...overflow }));
  }
  await page.evaluate(() => {
    for (const node of document.querySelectorAll('main, .body, .shell')) {
      if (node instanceof HTMLElement) node.scrollTop = 0;
    }
    window.scrollTo(0, 0);
  });
  await page.mouse.move(8, 8);
  const file = `${meta.id}${pass.suffix}.png`;
  await page.screenshot({ path: path.join(OUT, file), animations: 'disabled', fullPage: false });
  captures.push({ pass: pass.key, page: meta.id, file, note: meta.state });
  console.log(`  ${pass.key.padEnd(6)} ${file}`);

  if (meta.id === 'tools') {
    // ONE TAB, THREE SECTIONS. The gallery's `tools` capture is the top of the page (the exec form);
    // these are the states the page offers below it: one tool's sheet, the no-match search, the
    // plugin half on the same page, and the two plugin states a machine can be in.
    await page.locator('.tool-row').first().click();
    const dialog = page.getByRole('dialog');
    await dialog.waitFor({ state: 'visible', timeout: 5_000 });
    await settle(page, state, 250);
    const toolFile = `tools-tool${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, toolFile), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'tools-tool',
      file: toolFile,
      note: 'one tool inside the Tools tab: detail, annotations and schema — the sheet is a catalogue'
    });
    console.log(`  ${pass.key.padEnd(6)} ${toolFile}`);
    await page.locator('.sheet-close').click();
    await dialog.waitFor({ state: 'hidden', timeout: 5_000 }).catch(() => undefined);

    // Empty search: the sentence and the one action that undoes it.
    const search = page.getByLabel('Search tools');
    await search.fill('zzz-no-such-tool');
    await page.getByText('No tool matches "zzz-no-such-tool".').waitFor({ state: 'visible', timeout: 5_000 });
    const emptySearch = `tools-empty-search${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, emptySearch), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'tools-empty-search',
      file: emptySearch,
      note: 'the no-match state: the query is quoted and Clear restores the catalogue'
    });
    console.log(`  ${pass.key.padEnd(6)} ${emptySearch}`);
    await page.getByRole('button', { name: 'Clear the search' }).click();

    // The plugin list is the second section of the same page: scroll to it so the capture shows the
    // section rather than the catalogue that sits above it.
    await page.locator('section[aria-labelledby="tools-plugins"]').scrollIntoViewIfNeeded();
    await page.locator('.plugin-name').filter({ hasText: 'Hello' }).first().waitFor({ state: 'visible', timeout: 5_000 });
    await settle(page, state, 250);
    const pluginsFile = `tools-plugins${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, pluginsFile), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'tools-plugins',
      file: pluginsFile,
      note: 'the plugin section of the Tools tab: the rows that add to the catalogue'
    });
    console.log(`  ${pass.key.padEnd(6)} ${pluginsFile}`);

    // Two more machines, two more plugin states: the empty teaching state (attic) and an older
    // build with no plugins.* (legacy).
    await chooseMachine(page, state, 'attic');
    await page.getByText('No plugins yet. Add one when a built-in cannot do the job.').waitFor({ state: 'visible', timeout: 5_000 });
    await page.locator('section[aria-labelledby="tools-plugins"]').scrollIntoViewIfNeeded();
    const emptyFile = `tools-plugins-empty${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, emptyFile), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'tools-plugins-empty',
      file: emptyFile,
      note: 'the empty plugin section: no manifest example, one Add plugin action'
    });
    console.log(`  ${pass.key.padEnd(6)} ${emptyFile}`);

    await chooseMachine(page, state, 'legacy');
    await page.getByText('This machine needs a newer Summrise to host plugins.').waitFor({ state: 'visible', timeout: 5_000 });
    await page.locator('section[aria-labelledby="tools-plugins"]').scrollIntoViewIfNeeded();
    const oldFile = `tools-plugins-old${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, oldFile), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'tools-plugins-old',
      file: oldFile,
      note: 'an older build: the sentence and Open Machine, never an Add button that would fail'
    });
    console.log(`  ${pass.key.padEnd(6)} ${oldFile}`);

    await chooseMachine(page, state, 'workshop');
    await page.locator('.plugin-name').filter({ hasText: 'Hello' }).first().waitFor({ state: 'visible', timeout: 5_000 });

    // THE EXEC FORM MOVED HERE. It was the top of the Runs page; the capture follows it so the
    // section that makes a run is reviewed on the page that owns it, while the Runs capture is the
    // history it left behind.
    await page.locator('section[aria-labelledby="tools-run"]').scrollIntoViewIfNeeded();
    await page
      .getByRole('heading', { name: 'Run a command on this machine', exact: true })
      .waitFor({ state: 'visible', timeout: 5_000 });
    await page.getByRole('button', { name: 'Run', exact: true }).waitFor({ state: 'visible', timeout: 5_000 });
    await settle(page, state, 250);
    const runFormFile = `tools-run${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, runFormFile), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'tools-run',
      file: runFormFile,
      note: 'the exec form under Tools: command, working directory, timeout and the shell toggle'
    });
    console.log(`  ${pass.key.padEnd(6)} ${runFormFile}`);
  }

  if (meta.id === 'home') {
    // Home is two states: the fleet (zero machines chosen) and the hub. Open the first machine once,
    // then photograph the hub; the rest of the pass continues with that machine selected.
    await page.locator('.fleet-open').first().click();
    await page.getByRole('heading', { name: 'Home', exact: true, level: 1 }).waitFor({ state: 'visible', timeout: 5_000 });
    await settle(page, state, 350);
    const hubFile = `home-hub${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, hubFile), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'home-hub',
      file: hubFile,
      note: 'the hub: one status line, the endpoint with Copy, one accent, counts, the last three calls'
    });
    console.log(`  ${pass.key.padEnd(6)} ${hubFile}`);

    // Connect is a sheet: the endpoint first, then the token job in the same sheet.
    await page.getByRole('button', { name: 'Connect a client' }).click();
    const connect = page.getByRole('dialog', { name: 'Connect a client' });
    await connect.waitFor({ state: 'visible', timeout: 5_000 });
    await settle(page, state, 250);
    const endpointFile = `connect-endpoint${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, endpointFile), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'connect-endpoint',
      file: endpointFile,
      note: 'Connect: the endpoint, Copy, and one sentence on the bearer'
    });
    console.log(`  ${pass.key.padEnd(6)} ${endpointFile}`);

    await connect.getByRole('tab', { name: 'Tokens' }).click();
    await connect.getByRole('heading', { name: 'Create a token' }).waitFor({ state: 'visible', timeout: 5_000 });
    await settle(page, state, 250);
    const tokensFile = `connect-tokens${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, tokensFile), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'connect-tokens',
      file: tokensFile,
      note: 'Connect: one credential list with its scope column, and the mint form above it'
    });
    console.log(`  ${pass.key.padEnd(6)} ${tokensFile}`);
    await page.locator('.sheet-close').click();
    await connect.waitFor({ state: 'hidden', timeout: 5_000 }).catch(() => undefined);

    await chooseMachine(page, state, 'studio');
    // The offline device's fixture carries `last_attempt` with a reason, so the hub's sentence names
    // the count and the reason now; the wait follows the new sentence and its titled reason.
    await page
      .getByText(/studio is not connected after 5 attempts/)
      .waitFor({ state: 'visible', timeout: 5_000 });
    await page
      .getByTitle('timed out waiting for the channel hello')
      .waitFor({ state: 'visible', timeout: 5_000 });
    const offlineFile = `home-offline${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, offlineFile), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'home-offline',
      file: offlineFile,
      note: 'Home with an offline machine: the door dimmed, Copy disabled, one banner'
    });
    console.log(`  ${pass.key.padEnd(6)} ${offlineFile}`);

    await chooseMachine(page, state, 'stale');
    await page
      .getByText('The machine is running. The console could not read its tool list, so these counts may be stale.')
      .waitFor({ state: 'visible', timeout: 5_000 });
    const staleFile = `home-stale${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, staleFile), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'home-stale',
      file: staleFile,
      note: 'Home with a catalogue it could not read: the counts are labelled, not invented'
    });
    console.log(`  ${pass.key.padEnd(6)} ${staleFile}`);

    await chooseMachine(page, state, 'workshop');
    // The hub's door card is what says "a machine is chosen and its page is up"; the old anchor was
    // The anchor must be something the hub actually renders on every capture: `.status-line` and
    // then `.home-hero` both stopped existing, and each time every home capture timed out. The Now
    // band is the page's first real subject now, and it is always there (empty or not).
    await page.locator('main .live-now, main .page-head').first().waitFor({ state: 'visible', timeout: 5_000 });
  }

  if (meta.id === 'activity') {
    // The timeline row carries the evidence panel. Photograph it open before switching to the table,
    // so the stacked panel and its Result are reviewed at the phone widths too, not just the line.
    const firstLine = page.locator('.timeline-row button.timeline-line').first();
    if ((await firstLine.count()) > 0) {
      await firstLine.click();
      await page.locator('.evidence').first().waitFor({ state: 'visible', timeout: 5_000 });
      await page.locator('.evidence').first().scrollIntoViewIfNeeded();
      await settle(page, state, 150);
      const detailFile = `activity-detail${pass.suffix}.png`;
      await page.screenshot({ path: path.join(OUT, detailFile), animations: 'disabled', fullPage: false });
      captures.push({
        pass: pass.key,
        page: 'activity-detail',
        file: detailFile,
        note: 'one timeline row open: the evidence panel above its Result, stacked on a phone'
      });
      console.log(`  ${pass.key.padEnd(6)} ${detailFile}`);
      await firstLine.click();
      // Opening the row scrolled the page; the table capture that follows must start at the top.
      await page.evaluate(() => {
        for (const node of document.querySelectorAll('main, .body, .shell')) {
          if (node instanceof HTMLElement) node.scrollTop = 0;
        }
        window.scrollTo(0, 0);
      });
    }
    // The page opens on the timeline; the table, its disclosure and "Open in Runs" are one tab
    // away. The harness photographs the table because that is where the id and the run link live.
    await page.getByRole('tab', { name: 'Table' }).click();
    // The record is disclosed by its own button now, not a click anywhere on the row: a keyboard
    // reaches the same control. Photograph it open so the name-to-id step and the copy button are
    // reviewed, not just the summary table.
    await page
      .locator('.audit-row')
      .first()
      .getByRole('button', { name: /Show the record for/ })
      .click();
    await settle(page, state, 250);
    const detailFile = `audit-detail${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, detailFile), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'audit-detail',
      file: detailFile,
      note: 'one line open: full device id and its copy button'
    });
    console.log(`  ${pass.key.padEnd(6)} ${detailFile}`);

    // The run detail's handle, followed: Activity → Runs on the same device. The stub returns one
    // record; the screenshot is the page the operator lands on, not a second Activity claim.
    await page.getByRole('button', { name: 'Open in Runs' }).click();
    await page.getByRole('heading', { name: 'Runs' }).waitFor({ state: 'visible', timeout: 5_000 });
    await settle(page, state, 350);
    console.error('runs page text:', (await page.locator('.runs-page').innerText()).slice(0, 400));
    const runFile = `activity-run${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, runFile), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'activity-run',
      file: runFile,
      note: 'the run id opened Runs on the same device; the record is the stub’s one run'
    });
    console.log(`  ${pass.key.padEnd(6)} ${runFile}`);
  }
}

async function captureWorkspace(page, state, pass) {
  const meta = PAGES.find((entry) => entry.id === 'terminal');
  try {
    await goTo(page, meta);
    // A previous capture (Activity → Runs) may have selected a device already, in which case the
    // workspace is on screen and the picker's Use button does not exist. Both paths lead here.
    const workspace = page.locator('main .terminal-workspace');
    if ((await workspace.count()) === 0) {
      const use = page.locator('main .picker button', { hasText: 'Use' }).first();
      if ((await use.count()) === 0) return;
      await use.click();
    }
    // The page opens with the session card and no session (P0-3), so the workspace exists only after
    // one is opened; that capture moved below, after the pty is up.

    // THE CANVAS IS THE PALETTE'S, so photograph it. Open the first pty; the answer above is empty
    // output, which is enough to mount xterm and show the white (light) / #131418 (dark) canvas.
    await page.getByRole('button', { name: '+ New session' }).click();
    const dialog = page.getByRole('dialog', { name: 'New terminal session' });
    await dialog.getByRole('button', { name: 'Open' }).click();
    await page.locator('.term-host .xterm').first().waitFor({ state: 'visible', timeout: 5_000 });
    await settle(page, state, 400);
    const sessionFile = `terminal-session${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, sessionFile), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'terminal-session',
      file: sessionFile,
      note: 'one pty open; the empty output is a canvas, not a fake shell'
    });
    console.log(`  ${pass.key.padEnd(6)} ${sessionFile}`);

    // The session card leads the page and the terminal is beneath it: photograph them together, now
    // that a session exists.
    await page.locator('main .terminal-workspace').first().waitFor({ state: 'visible', timeout: 5_000 });
    await settle(page, state, 250);
    const workspaceFile = `terminal-workspace${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, workspaceFile), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'terminal-workspace',
      file: workspaceFile,
      note: 'the session card above the terminal: machine, session, age and the last keystroke'
    });
    console.log(`  ${pass.key.padEnd(6)} ${workspaceFile}`);

    // The one menu the session keeps: photograph it open, so its placement and its one-column
    // action list are part of the review instead of a claim in a commit message.
    await page.getByLabel('Session tools').click();
    await settle(page, state, 250);
    const toolsFile = `terminal-tools${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, toolsFile), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'terminal-tools',
      file: toolsFile,
      note: 'the session menu open: Find is the only toolbar button left beside it'
    });
    console.log(`  ${pass.key.padEnd(6)} ${toolsFile}`);

    // The Files root with the upload control: the one browser-shaped control on the page is now the
    // console's button. The device is still selected, so Files lists without a second pick.
    await goTo(page, PAGES.find((entry) => entry.id === 'files'));
    await page.getByText('notes.txt').waitFor({ state: 'visible', timeout: 5_000 });
    await settle(page, state, 300);
    const filesFile = `files-root${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, filesFile), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'files-root',
      file: filesFile,
      note: 'the real Files root with the upload control; the list is the stub fixture'
    });
    console.log(`  ${pass.key.padEnd(6)} ${filesFile}`);

    // AN OFFLINE MACHINE IS THE ONE EMPTY PAGE WITH A REASON — and the state a person is most
    // likely to mistake for a broken console. The sentence and the disabled control are the page,
    // so they are photographed rather than described.
    await chooseMachine(page, state, 'studio');
    await goTo(page, meta);
    await page.locator('.offline-notice').waitFor({ state: 'visible', timeout: 5_000 });
    await settle(page, state, 300);
    const offlineFile = `terminal-offline${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, offlineFile), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'terminal-offline',
      file: offlineFile,
      note: 'a machine that is down: the reason above the work, and New session disabled'
    });
    console.log(`  ${pass.key.padEnd(6)} ${offlineFile}`);

    // The same machine, one tab over: the notice belongs to every device page, not only the one it
    // was written for. Files answers 503, so the page shows the refusal it would really show.
    await goTo(page, PAGES.find((entry) => entry.id === 'files'));
    // The terminal page stays mounted (hidden) across destinations and carries its own notice, so
    // the locator names the tab-scoped sentence instead of counting notices.
    await page
      .locator('.offline-notice')
      .filter({ hasText: 'Nothing on this page can reach it' })
      .waitFor({ state: 'visible', timeout: 5_000 });
    await settle(page, state, 300);
    const filesOfflineFile = `files-offline${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, filesOfflineFile), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'files-offline',
      file: filesOfflineFile,
      note: 'Files on a machine that is down: the same notice, and the refusal the console really sends'
    });
    console.log(`  ${pass.key.padEnd(6)} ${filesOfflineFile}`);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    captureFailures.push({ pass: pass.key, page: 'terminal-workspace', message });
    console.error(`screenshot: could not capture the workspace (${pass.key}): ${message}`);
  }
}

/**
 * P2-12: a 1000-row day, measured the way the operator meets it.
 *
 * A fresh context opens directly on #activity; the stub serves the 1000-line fixture; the clock is
 * the page's own performance.now(), relative to navigation start. The wait is for the last row by
 * name, not just for a count, and two animation frames pass before the reading — a node in the DOM
 * is not yet a painted row. The printed number is navigation -> audit -> React -> paint.
 */
async function captureActivity1000(browser, origin, width) {
  const passKey = `${width}-light`;
  const state = newState({ key: passKey });
  state.label = 'activity-1000';
  const context = await browser.newContext({
    viewport: { width, height: width < 640 ? 844 : 900 },
    colorScheme: 'light',
    deviceScaleFactor: 1
  });
  const page = await context.newPage();
  await installPage(page, state, origin);
  stubAudit = BIG_AUDIT;
  try {
    await page.goto(origin + '/#activity', { waitUntil: 'domcontentloaded', timeout: 30_000 });
    await page.waitForFunction(
      () => {
        const rows = document.querySelectorAll('.timeline-row');
        const last = rows[rows.length - 1];
        return (
          rows.length === 1000 &&
          last !== undefined &&
          (last.textContent ?? '').includes('Call 1000')
        );
      },
      null,
      { timeout: 30_000 }
    );
    await page.evaluate(
      () =>
        new Promise((resolve) => {
          requestAnimationFrame(() => requestAnimationFrame(() => resolve(null)));
        })
    );
    // performance.now() is relative to this document's navigation start, so the reading includes
    // the fetch and the whole first render rather than only the last React commit.
    const ms = Math.round(await page.evaluate(() => performance.now()));
    const rows = await page.locator('.timeline-row').count();
    // The stretch labels are a count a person can walk the day by: item 5 dropped the one-call
    // ones, so the harness prints how many the fixture still draws on a 1000-row day.
    const stretches = await page.locator('.timeline-stretch').count();
    const file = `activity-1000-${width}.png`;
    await page.screenshot({ path: path.join(OUT, file), animations: 'disabled', fullPage: false });
    captures.push({
      pass: passKey,
      page: 'activity-1000',
      file,
      note: `1000 rows of one day at ${width}px: ${ms} ms from navigation to the last row painted`
    });
    console.log(`  ${passKey.padEnd(6)} ${file}`);
    console.log(
      `activity-1000${width === 1280 ? '' : ` at ${width}`}: ${ms} ms, ${rows} rows, ` +
        `${stretches} stretch labels`
    );
    // A day long enough to scroll past its label: the one place the sticky rule can be seen. The
    // measured offset stays 0 while the day passes under the label.
    const sticky = await page.evaluate(() => {
      const main = document.querySelector('main');
      const label = document.querySelector('.timeline-day-label');
      if (!main || !label) return null;
      main.scrollTop = 600;
      const offset = Math.round(
        label.getBoundingClientRect().top - main.getBoundingClientRect().top
      );
      return {
        scrollTop: Math.round(main.scrollTop),
        offset,
        background: getComputedStyle(label).backgroundColor
      };
    });
    if (sticky) {
      console.log(`activity-sticky${width === 1280 ? '' : ` at ${width}`}: ${JSON.stringify(sticky)}`);
      const stickyFile = `activity-sticky-${width}.png`;
      await page.screenshot({ path: path.join(OUT, stickyFile), animations: 'disabled', fullPage: false });
      captures.push({
        pass: passKey,
        page: 'activity-sticky',
        file: stickyFile,
        note: `the day label holding the top at ${width}px, ${sticky.scrollTop} px into the day`
      });
    }
    await page.evaluate(() => {
      const main = document.querySelector('main');
      if (main) main.scrollTop = 0;
    });
    return { width, ms, rows };
  } finally {
    stubAudit = AUDIT;
    for (const issue of state.pageErrors) pageErrors.push({ pass: passKey, ...issue });
    for (const issue of state.apiFailures) apiFailures.push({ pass: passKey, ...issue });
    for (const issue of state.consoleErrors) consoleErrors.push({ pass: passKey, ...issue });
    await context.close().catch(() => {});
  }
}

// ---------------------------------------------------------------------------------------------
// Gallery.
// ---------------------------------------------------------------------------------------------

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (character) => {
    switch (character) {
      case '&':
        return '&amp;';
      case '<':
        return '&lt;';
      case '>':
        return '&gt;';
      case '"':
        return '&quot;';
      default:
        return '&#39;';
    }
  });
}

function buildGallery() {
  const byKey = new Map(captures.map((capture) => [`${capture.pass}:${capture.page}`, capture.file]));
  const image = (file, alt) =>
    file
      ? `<a href="${escapeHtml(file)}"><img src="${escapeHtml(file)}" alt="${escapeHtml(alt)}" loading="lazy"></a>`
      : '<p class="missing">not captured</p>';

  const widths = [360, 390, 800, 1280];
  const label = (meta) =>
    meta.tab
      ? `${meta.label} · ${meta.tab}${meta.view ? ` · ${meta.view}` : ''}`
      : meta.label;
  const shot = (width, theme, id) => byKey.get(`${width}-${theme}:${id}`);

  const rows = PAGES.map((meta) => {
    const pairs = widths
      .map(
        (width) => `
        <div class="pair">
          <figure><figcaption>light · ${width}px</figcaption>${image(shot(width, 'light', meta.id), `${label(meta)}, light, ${width}`)}</figure>
          <figure><figcaption>dark · ${width}px</figcaption>${image(shot(width, 'dark', meta.id), `${label(meta)}, dark, ${width}`)}</figure>
        </div>`
      )
      .join('');
    return `
      <section class="page">
        <h2>${escapeHtml(label(meta))}</h2>
        <p class="state">${escapeHtml(meta.state)}</p>
        ${pairs}
      </section>`;
  }).join('');

  const workspaceKeys = [
    ['terminal-workspace', 'the sessions-first empty state'],
    ['terminal-session', 'one pty on the canvas'],
    ['terminal-tools', 'the session menu'],
    ['files-root', 'the real Files root'],
    ['audit-detail', 'one Activity record open'],
    ['activity-detail', 'one timeline record open'],
    ['activity-run', 'the run opened from Activity']
  ];
  const workspaceRows = `
    <section class="page">
      <h2>Workspace</h2>
      <p class="state">A selected device, light and dark at 1280: the states the jsdom suite cannot see.</p>
      ${workspaceKeys
        .map(
          ([id, note]) => `
        <div class="pair">
          <figure><figcaption>${escapeHtml(note)} · light</figcaption>${image(byKey.get(`1280-light:${id}`), `${note}, light`)}</figure>
          <figure><figcaption>${escapeHtml(note)} · dark</figcaption>${image(byKey.get(`1280-dark:${id}`), `${note}, dark`)}</figure>
        </div>`
        )
        .join('')}
    </section>`;

  const known = new Set([
    ...PAGES.map((meta) => meta.id),
    ...workspaceKeys.map(([id]) => id),
    'activity-1000',
    'activity-sticky'
  ]);
  const extras = [...new Set(captures.map((capture) => capture.page))].filter((id) => !known.has(id));

  // P2-12's measurement, kept beside the ordinary Activity page so the cost of a full day has a
  // picture and a number in the same place.
  const measured = captures.filter(
    (capture) => capture.page === 'activity-1000' || capture.page === 'activity-sticky'
  );
  const measuredRows = measured.length
    ? `
    <section class="page">
      <h2>Long timeline: 1000 rows</h2>
      <p class="state">One fresh navigation per width: navigation → last row painted, then 600 px into the same day for the sticky label.</p>
      <div class="pair">
        ${measured
          .map(
            (capture) =>
              `<figure><figcaption>${escapeHtml(capture.note)}</figcaption>${image(capture.file, capture.note)}</figure>`
          )
          .join('')}
      </div>
    </section>`
    : '';
  const stateRows = extras
    .map(
      (id) => `
      <section class="page">
        <h2>${escapeHtml(id)}</h2>
        <div class="pair">
          <figure><figcaption>light · 1280</figcaption>${image(byKey.get(`1280-light:${id}`), `${id}, light`)}</figure>
          <figure><figcaption>dark · 1280</figcaption>${image(byKey.get(`1280-dark:${id}`), `${id}, dark`)}</figure>
        </div>
        <div class="pair">
          <figure><figcaption>360 · light</figcaption>${image(byKey.get(`360-light:${id}`), `${id}, 360`)}</figure>
          <figure><figcaption>800 · dark</figcaption>${image(byKey.get(`800-dark:${id}`), `${id}, 800 dark`)}</figure>
        </div>
      </section>`
    )
    .join('');

  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>React console screenshots — ${escapeHtml(RAW_TAG)}</title>
<style>
  :root { color-scheme: light; }
  * { box-sizing: border-box; }
  body { margin: 0; padding: 24px; background: #f6f6f7; color: #18181b; font: 14px/1.55 system-ui, sans-serif; }
  header { max-width: 1180px; margin: 0 auto 28px; }
  h1 { margin: 0 0 4px; font-size: 22px; }
  .meta { margin: 0; color: #5d5d66; }
  .page { max-width: 1180px; margin: 0 auto 36px; padding-top: 18px; border-top: 1px solid #dedee3; }
  .page h2 { margin: 0; font-size: 17px; }
  .state { margin: 2px 0 12px; color: #5d5d66; font-size: 13px; }
  .pair { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }
  figure { margin: 0; }
  figcaption { margin-bottom: 6px; color: #5d5d66; font-size: 12px; }
  img { display: block; width: 100%; height: auto; border: 1px solid #d8d8de; border-radius: 10px; background: #fff; }
  .mobile { display: inline-block; width: 390px; max-width: 100%; margin: 0 18px 18px 0; vertical-align: top; }
  .missing { margin: 0; padding: 40px 16px; border: 1px dashed #c9c9d1; border-radius: 10px; color: #8a8a94; text-align: center; }
  @media (max-width: 800px) { .pair { grid-template-columns: 1fr; } }
</style>
</head>
<body>
<header>
  <h1>React console screenshots — ${escapeHtml(RAW_TAG)}</h1>
  <p class="meta">
    Rendered ${escapeHtml(new Date(NOW).toISOString())} from <code>dist/index.html</code> with stubbed console JSON.
    Every destination at 360, 390, 800 and 1280 px, light and dark, plus the states the plan names.
  </p>
</header>
${rows}
${workspaceRows}
${measuredRows}
${stateRows}
</body>
</html>
`;
}

// ---------------------------------------------------------------------------------------------
// Run.
// ---------------------------------------------------------------------------------------------

const captures = [];
const pageErrors = [];
const apiFailures = [];
const consoleErrors = [];
const captureFailures = [];

let browser = null;
let server = null;

try {
  await rm(OUT, { recursive: true, force: true });
  await mkdir(OUT, { recursive: true });
  console.log(`screenshot: tag ${RAW_TAG} -> ${OUT}`);

  const started = await startServer();
  server = started.server;
  const origin = started.origin;
  console.log(`screenshot: serving dist/index.html at ${origin}/`);

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

  for (const pass of PASSES) {
    const context = await browser.newContext({
      viewport: pass.viewport,
      colorScheme: pass.colorScheme,
      deviceScaleFactor: 1
    });
    const page = await context.newPage();
    const state = newState(pass);
    await installPage(page, state, origin);

    try {
      await boot(page, state, origin);
      const [first, ...rest] = pass.pages;

      state.label = first.id;
      try {
        // Enter every page through the rail: the harness does not assume which page the app opens
        // on (it now opens on Devices).
        await goTo(page, first);
        await capturePage(page, state, first, pass);
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        captureFailures.push({ pass: pass.key, page: first.id, message });
        console.error(`screenshot: could not capture ${first.id} (${pass.key}): ${message}`);
      }

      for (const meta of rest) {
        state.label = meta.id;
        try {
          await goTo(page, meta);
          await capturePage(page, state, meta, pass);
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          captureFailures.push({ pass: pass.key, page: meta.id, message });
          console.error(`screenshot: could not capture ${meta.id} (${pass.key}): ${message}`);
        }
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      captureFailures.push({ pass: pass.key, page: 'boot', message });
      console.error(`screenshot: could not capture the ${pass.key} pass: ${message}`);
    } finally {
      for (const issue of state.pageErrors) pageErrors.push({ pass: pass.key, ...issue });
      for (const issue of state.apiFailures) apiFailures.push({ pass: pass.key, ...issue });
      for (const issue of state.consoleErrors) consoleErrors.push({ pass: pass.key, ...issue });
      if (pass.width >= 800) await captureWorkspace(page, state, pass);
      await context.close().catch(() => {});
    }
  }

  // The measurement is its own fresh navigation, at the two widths the item names. It runs after the
  // passes so the two screenshots cannot disturb a page state the ordinary gallery depends on.
  for (const width of [1280, 390]) {
    try {
      await captureActivity1000(browser, origin, width);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      captureFailures.push({ pass: `${width}-light`, page: 'activity-1000', message });
      console.error(`screenshot: could not measure activity-1000 at ${width}: ${message}`);
    }
  }
} finally {
  await browser?.close().catch(() => {});
  await stopServer(server);
}

const files = await readdir(OUT).catch(() => []);
const pngs = files.filter((name) => name.endsWith('.png'));
const gallery = buildGallery();
await writeFile(path.join(OUT, 'index.html'), gallery, 'utf8');

console.log('');
console.log('screenshot: page states:');
for (const meta of PAGES) console.log(`  ${meta.id.padEnd(8)} ${meta.state}`);
if (consoleErrors.length > 0) {
  console.warn(`screenshot: ${consoleErrors.length} console error(s) (not fatal):`);
  for (const issue of consoleErrors) {
    console.warn(`  [${issue.pass}/${issue.page}] ${issue.message}`);
  }
}
if (pageErrors.length > 0) {
  console.error(`screenshot: ${pageErrors.length} page error(s):`);
  for (const issue of pageErrors) console.error(`  [${issue.pass}/${issue.page}] ${issue.message}`);
}
if (apiFailures.length > 0) {
  console.error(`screenshot: ${apiFailures.length} failed API request(s):`);
  for (const issue of apiFailures) {
    console.error(
      `  [${issue.pass}/${issue.page}] ${issue.method} ${issue.url} -> ${issue.status ?? 'failed'} (${issue.detail})`
    );
  }
}
if (captureFailures.length > 0) {
  console.error(`screenshot: ${captureFailures.length} page(s) could not be captured:`);
  for (const issue of captureFailures) {
    console.error(`  [${issue.pass}/${issue.page}] ${issue.message}`);
  }
}

const failed = pageErrors.length + apiFailures.length + captureFailures.length > 0;
console.log(
  `screenshot: ${pngs.length} PNG${pngs.length === 1 ? '' : 's'} + index.html in ${OUT}`
);
if (failed) {
  console.error(
    `screenshot: FAILED — ${pageErrors.length} page error(s), ` +
      `${apiFailures.length} failed API request(s), ` +
      `${captureFailures.length} uncaptured page(s)`
  );
  process.exitCode = 1;
} else {
  console.log('screenshot: ok');
}
