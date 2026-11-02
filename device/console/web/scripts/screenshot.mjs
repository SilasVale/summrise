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
  studio: hexId('device/studio')
};
const TOKEN = {
  workshop: hexId('token/workshop'),
  attic: hexId('token/attic'),
  fleet: hexId('token/fleet')
};

// Mirrors `src/useToolNames.ts`, which mirrors the device's registry. These are the names a grant
// form offers; /tools is the console's served copy.
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

/// The live catalogue for one device. `studio` is an older build: its list has no `plugins.*`, which
/// is exactly how the page detects the "newer Summrise" state.
function liveToolsFor(deviceId) {
  const names =
    deviceId === DEVICE.studio
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

/** One audit line, newest first in the list below. `kind` is the console's closed set. */
function auditLine(agoMs, kind, { device, status = 200, tool = null, detail = null, token = null } = {}) {
  const line = { ts_ms: NOW - agoMs, kind, status };
  if (device) line.device_id = device;
  if (tool) line.tool = tool;
  if (detail) line.detail = detail;
  if (token) line.token_id = token;
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
  if (method === 'GET' && pathname === '/audit') return json(AUDIT);
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
    // The workspace photographs need the two calls a pty open makes; the ANSWER is empty output, not
    // a fake shell. Every other tool is still a loud failure.
    if (name === 'terminal.list') return json({ result: { content: [{ type: 'text', text: '[]' }] } });
    if (name === 'system.process_list') {
      return json({ result: { content: [{ type: 'text', text: '[]' }] } });
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
    return json({ tools: liveToolsFor(decodeURIComponent(liveTools[1])) });
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
    id: 'capabilities',
    label: 'Capabilities',
    heading: 'Capabilities',
    state: 'the live tool catalogue, grouped by capability'
  },
  {
    id: 'plugins',
    label: 'Plugins',
    heading: 'Plugins',
    state: 'the plugins the selected machine hosts'
  },
  { id: 'activity', label: 'Activity', heading: 'Activity', state: 'the audit table' },
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
    state: 'the device workspace: the runs tab'
  },
  {
    id: 'machine',
    label: 'Device',
    tab: 'Machine',
    heading: 'Machine',
    state: 'the device workspace: the machine tab'
  }
];

const PASSES = [
  {
    key: 'light',
    viewport: { width: 1280, height: 900 },
    colorScheme: 'light',
    suffix: '',
    pages: PAGES
  },
  {
    key: 'dark',
    viewport: { width: 1280, height: 900 },
    colorScheme: 'dark',
    suffix: '-dark',
    pages: PAGES
  },
  {
    key: 'mobile',
    viewport: { width: 390, height: 844 },
    colorScheme: 'light',
    suffix: '-390',
    pages: PAGES.filter((page) => ['home', 'activity', 'terminal', 'capabilities', 'plugins'].includes(page.id))
  }
];

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
  }

  if (meta.id === 'audit') {
    // The record is the row, but the full id lives in the disclosure. Photograph it open so the
    // name-to-id step and the copy button are reviewed, not just the summary table.
    await page.getByRole('button', { name: 'Details' }).first().click();
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
    await workspace.waitFor({ state: 'visible', timeout: 5_000 }).catch(async (error) => {
      await page.screenshot({ path: path.join(OUT, `terminal-workspace${pass.suffix}-failed.png`) });
      console.error('workspace debug:', (await page.locator('main').innerText()).slice(0, 1200));
      throw error;
    });
    await settle(page, state, 300);
    const file = `terminal-workspace${pass.suffix}.png`;
    await page.screenshot({ path: path.join(OUT, file), animations: 'disabled', fullPage: false });
    captures.push({
      pass: pass.key,
      page: 'terminal-workspace',
      file,
      note: 'selected the first device; the session grant opens the terminal without a form'
    });
    console.log(`  ${pass.key.padEnd(6)} ${file}`);

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
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    captureFailures.push({ pass: pass.key, page: 'terminal-workspace', message });
    console.error(`screenshot: could not capture the workspace (${pass.key}): ${message}`);
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

  const rows = PAGES.map((meta) => {
    const light = byKey.get(`light:${meta.id}`);
    const dark = byKey.get(`dark:${meta.id}`);
    return `
      <section class="page">
        <h2>${escapeHtml(meta.label)}</h2>
        <p class="state">${escapeHtml(meta.state)}</p>
        <div class="pair">
          <figure><figcaption>light · 1280×900</figcaption>${image(light, `${meta.label}, light`)}</figure>
          <figure><figcaption>dark · 1280×900</figcaption>${image(dark, `${meta.label}, dark`)}</figure>
        </div>
      </section>`;
  }).join('');

  const workspaceRows = `
    <section class="page">
      <h2>Workspace</h2>
      <p class="state">A selected device: the sessions-first empty state, and one pty's canvas (no output; no fake shell).</p>
      <div class="pair">
        <figure><figcaption>empty · light</figcaption>${image(byKey.get('light:terminal-workspace'), 'Terminal workspace empty, light')}</figure>
        <figure><figcaption>empty · dark</figcaption>${image(byKey.get('dark:terminal-workspace'), 'Terminal workspace empty, dark')}</figure>
      </div>
      <div class="pair">
        <figure><figcaption>pty open · light</figcaption>${image(byKey.get('light:terminal-session'), 'Terminal canvas, light')}</figure>
        <figure><figcaption>pty open · dark</figcaption>${image(byKey.get('dark:terminal-session'), 'Terminal canvas, dark')}</figure>
      </div>
      <div class="pair">
        <figure><figcaption>session menu · light</figcaption>${image(byKey.get('light:terminal-tools'), 'Session menu, light')}</figure>
        <figure><figcaption>session menu · dark</figcaption>${image(byKey.get('dark:terminal-tools'), 'Session menu, dark')}</figure>
      </div>
      <div class="pair">
        <figure><figcaption>files root · light</figcaption>${image(byKey.get('light:files-root'), 'Files root, light')}</figure>
        <figure><figcaption>files root · dark</figcaption>${image(byKey.get('dark:files-root'), 'Files root, dark')}</figure>
      </div>
      <div class="pair">
        <figure><figcaption>activity line open · light</figcaption>${image(byKey.get('light:audit-detail'), 'Activity detail, light')}</figure>
        <figure><figcaption>activity line open · dark</figcaption>${image(byKey.get('dark:audit-detail'), 'Activity detail, dark')}</figure>
      </div>
      <div class="pair">
        <figure><figcaption>run opened from Activity · light</figcaption>${image(byKey.get('light:activity-run'), 'Run opened from Activity, light')}</figure>
        <figure><figcaption>run opened from Activity · dark</figcaption>${image(byKey.get('dark:activity-run'), 'Run opened from Activity, dark')}</figure>
      </div>
    </section>`;

  const mobileRows = PASSES.find((pass) => pass.key === 'mobile')
    .pages.map((meta) => {
      const file = byKey.get(`mobile:${meta.id}`);
      return `
        <figure class="mobile">
          <figcaption>${escapeHtml(meta.label)} · light · 390×844</figcaption>
          ${image(file, `${meta.label}, light, 390 by 844`)}
        </figure>`;
    })
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
    Full-size pages are 1280×900, light and dark; Devices and Terminal also have a 390×844 light pass.
  </p>
</header>
${workspaceRows}
${rows}
<section class="page">
  <h2>Small screens</h2>
  <p class="state">Light only, 390×844: the rail becomes a bottom strip and the columns collapse.</p>
  ${mobileRows}
</section>
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
      if (pass.key !== 'mobile') await captureWorkspace(page, state, pass);
      await context.close().catch(() => {});
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
