import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';

const sourceUrl = new URL('../device/device/assets/panel-terminal.js', import.meta.url);
const source = readFileSync(sourceUrl, 'utf8');

// Each response has a finite chain of async continuations. Drain that chain without
// a real clock, socket, xterm implementation, shell, or serial device.
async function settle() {
  for (let turn = 0; turn < 20; turn += 1) await Promise.resolve();
}

// Execute the production IIFE unchanged. These doubles expose only its DOM/xterm,
// HTTP and timer boundaries; requests, cursors, bytes and session ownership are real
// production behavior. The page auth value and serial port are synthetic fixtures.
function panel() {
  const elements = new Map();
  const requests = [];
  const timers = new Map();
  let nextTimer = 0;
  let terminal;

  function element(id) {
    if (!elements.has(id)) {
      elements.set(id, {
        value: id === 'term-port' ? 'COM_TEST' : '57600',
        textContent: '',
        listeners: {},
        addEventListener(event, callback) { this.listeners[event] = callback; },
      });
    }
    return elements.get(id);
  }

  class Terminal {
    constructor() { this.writes = []; terminal = this; }
    loadAddon() {}
    open() {}
    focus() {}
    reset() { this.writes = []; }
    onData(callback) { this.input = callback; }
    onResize(callback) { this.resize = callback; }
    write(data) { this.writes.push(typeof data === 'string' ? data : Buffer.from(data)); }
  }

  vm.runInNewContext(source, {
    Terminal,
    FitAddon: { FitAddon: class { fit() {} } },
    document: {
      documentElement: {},
      getElementById: element,
      querySelector: () => ({ content: 'test-page-auth' }),
    },
    getComputedStyle: () => ({ getPropertyValue: () => '' }),
    window: { addEventListener() {} },
    fetch(url, options) {
      return new Promise((resolve, reject) => requests.push({ url, options, resolve, reject }));
    },
    atob: data => Buffer.from(data, 'base64').toString('binary'),
    setTimeout(callback, delay) {
      const id = ++nextTimer;
      timers.set(id, { callback, delay });
      return id;
    },
    clearTimeout(id) { timers.delete(id); },
  }, { filename: sourceUrl.pathname });

  function take(url) {
    const index = requests.findIndex(request => request.url === url);
    assert.notEqual(index, -1, `expected ${url}; pending: ${requests.map(request => request.url)}`);
    return requests.splice(index, 1)[0];
  }

  async function respond(request, answer, status = 200) {
    request.resolve({ ok: status >= 200 && status < 300, status, json: async () => answer });
    await settle();
  }

  async function reject(request) {
    request.reject(new TypeError('controlled transport failure'));
    await settle();
  }

  async function tick(delay) {
    const due = [...timers].filter(([, timer]) => timer.delay === delay);
    for (const [id, timer] of due) {
      timers.delete(id);
      timer.callback();
    }
    await settle();
  }

  return {
    element, take, respond, reject, tick, requests, terminal,
    click: id => element(id).listeners.click(),
    status: () => element('term-status').textContent,
  };
}

async function emptyPanel() {
  const app = panel();
  await app.respond(app.take('/api/terminal'), { sessions: [] });
  return app;
}

async function newPty(app, id) {
  const created = app.click('term-pty');
  const request = app.take('/api/terminal/create');
  assert.equal(request.options.headers['x-summrise-panel'], 'test-page-auth');
  assert.deepEqual(JSON.parse(request.options.body), { kind: 'pty', rows: 30, cols: 100 });
  await app.respond(request, { id, kind: 'pty' });
  await created;
  return app.take(`/api/terminal/${id}/read?cursor=0&wait_ms=15000`);
}

test('a delayed ended read cannot write into or end a replacement session', async () => {
  const app = await emptyPanel();
  const oldRead = await newPty(app, 'old');
  const closing = app.click('term-close');
  const close = app.take('/api/terminal/old/close');
  assert.equal(close.options.method, 'POST');
  assert.equal(close.options.headers['x-summrise-panel'], 'test-page-auth');
  await app.respond(close, { closed: true });
  await closing;
  const currentRead = await newPty(app, 'current');
  await app.respond(oldRead, {
    text: 'old output', data_base64: 'b2xkIG91dHB1dA==', cursor: 99, alive: false, exit_code: 7,
  });
  assert.equal(app.status(), 'pty current · live');
  assert.deepEqual(app.terminal.writes, [], 'retired bytes must not reach the replacement canvas');
  await app.respond(currentRead, {
    text: 'new', data_base64: 'bmV3', cursor: 3, alive: true, exit_code: null,
  });
  await app.tick(50);
  app.take('/api/terminal/current/read?cursor=3&wait_ms=15000');
});

test('a required close is shared across newer intents and Close cancels replacement', async () => {
  const app = await emptyPanel();
  const oldRead = await newPty(app, 'old');
  const closing = app.click('term-close');
  const oldClose = app.take('/api/terminal/old/close');
  const creating = app.click('term-pty');
  await settle();
  assert.equal(app.requests.length, 0, 'replacement must await the same required close acknowledgement');
  const lastClose = app.click('term-close');
  await app.respond(oldClose, { closed: true });
  await Promise.all([closing, creating, lastClose]);
  await app.respond(oldRead, { text: 'late', data_base64: 'bGF0ZQ==', cursor: 99, alive: false, exit_code: 0 });
  await app.tick(40); await app.tick(50);
  assert.equal(app.requests.length, 0, 'latest Close cancels queued New and old polling');
  assert.equal(app.status(), 'session closed');
  assert.deepEqual(app.terminal.writes, []);
});

test('input queued before close cannot be sent to a replacement session', async () => {
  const app = await emptyPanel();
  await newPty(app, 'old');
  app.terminal.input('old-session-command\r');
  const closing = app.click('term-close');
  await app.respond(app.take('/api/terminal/old/close'), { closed: true });
  await closing;
  await newPty(app, 'current');
  await app.tick(40);
  assert.equal(app.requests.length, 0, 'the retired input timer cannot issue a write');
  app.terminal.input('current-command\r');
  await app.tick(40);
  const input = app.take('/api/terminal/current/write');
  assert.deepEqual(JSON.parse(input.options.body), { data: 'current-command\r' });
});

test('New pty reclaims a naturally ended session before starting its replacement', async () => {
  const app = await emptyPanel();
  const read = await newPty(app, 'old');
  app.terminal.input('before-end\r');
  await app.respond(read, { text: '', data_base64: '', cursor: 0, alive: false, exit_code: null });
  assert.equal(app.status(), 'session ended', 'a nullable code must not invent an exit fact');
  await app.tick(40);
  app.terminal.input('after-end\r');
  app.terminal.resize({ rows: 31, cols: 101 });
  await app.tick(40);
  await app.tick(50);
  assert.equal(app.requests.length, 0, 'EOF retires polling, queued input and resize while retaining the cleanup owner');

  const creating = app.click('term-pty');
  const close = app.take('/api/terminal/old/close');
  assert.equal(close.options.method, 'POST');
  assert.equal(close.options.headers['x-summrise-panel'], 'test-page-auth');
  assert.equal(app.requests.length, 0, 'replacement creation must wait for the old close response');
  await app.respond(close, { closed: true });
  const create = app.take('/api/terminal/create');
  assert.equal(create.options.headers['x-summrise-panel'], 'test-page-auth');
  assert.deepEqual(JSON.parse(create.options.body), { kind: 'pty', rows: 30, cols: 100 });
  await app.respond(create, { id: 'current', kind: 'pty' });
  await creating;
  const currentRead = app.take('/api/terminal/current/read?cursor=0&wait_ms=15000');
  assert.equal(app.status(), 'pty current · live');
  app.terminal.input('current-command\r');
  await app.tick(40);
  const input = app.take('/api/terminal/current/write');
  assert.deepEqual(JSON.parse(input.options.body), { data: 'current-command\r' });
  await app.respond(input, {});
  await app.respond(currentRead, { text: 'new', data_base64: 'bmV3', cursor: 3, alive: true, exit_code: null });
  await app.tick(50);
  app.take('/api/terminal/current/read?cursor=3&wait_ms=15000');
  assert.equal(app.status(), 'pty current · live', 'cleanup of the ended id cannot end its replacement');
});

test('Close releases the owned session after natural EOF', async () => {
  const app = await emptyPanel();
  const read = await newPty(app, 'ended');
  app.terminal.input('queued-before-end\r');
  await app.respond(read, { text: 'last', data_base64: 'bGFzdA==', cursor: 4, alive: false, exit_code: 0 });
  assert.equal(app.status(), 'session ended (exit 0)');
  assert.equal(Buffer.concat(app.terminal.writes).toString('utf8'), 'last', 'the ended response still delivers its final bytes');
  app.terminal.input('after-end\r');
  app.terminal.resize({ rows: 31, cols: 101 });
  await app.tick(40);
  await app.tick(50);
  assert.equal(app.requests.length, 0, 'EOF stops polling, queued input and resize before explicit cleanup');

  const closing = app.click('term-close');
  const close = app.take('/api/terminal/ended/close');
  assert.equal(close.options.method, 'POST');
  assert.equal(close.options.headers['x-summrise-panel'], 'test-page-auth');
  await app.respond(close, { closed: true });
  await closing;
  assert.equal(app.status(), 'session closed');
  await app.tick(40);
  await app.tick(50);
  assert.equal(app.requests.length, 0, 'a released ended session cannot resume input, resize or polling');
});

for (const shape of ['object', 'array']) {
  test(`existing ${shape} session list preserves UTF-8 bytes, cursor, input, resize, auth and end`, async () => {
    const app = panel();
    const sessions = [{ id: 'existing', kind: 'pty' }];
    await app.respond(app.take('/api/terminal'), shape === 'object' ? { sessions } : sessions);
    assert.equal(app.status(), 'pty existing · live');
    const first = app.take('/api/terminal/existing/read?cursor=0&wait_ms=15000');
    assert.equal(first.options.headers['x-summrise-panel'], 'test-page-auth');
    await app.respond(first, {
      text: '�', data_base64: '5Lit5g==', cursor: 4, alive: true, exit_code: null,
    });
    await app.tick(50);
    await app.respond(app.take('/api/terminal/existing/read?cursor=4&wait_ms=15000'), {
      text: '��', data_base64: 'loc=', cursor: 6, alive: true, exit_code: null,
    });
    assert.equal(Buffer.concat(app.terminal.writes).toString('utf8'), '中文');
    app.terminal.input('echo marker\r');
    await app.tick(40);
    const input = app.take('/api/terminal/existing/write');
    assert.equal(input.options.method, 'POST');
    assert.equal(input.options.headers['x-summrise-panel'], 'test-page-auth');
    assert.deepEqual(JSON.parse(input.options.body), { data: 'echo marker\r' });
    await app.respond(input, {});
    app.terminal.resize({ rows: 31, cols: 101 });
    const resize = app.take('/api/terminal/existing/resize');
    assert.equal(resize.options.headers['x-summrise-panel'], 'test-page-auth');
    assert.deepEqual(JSON.parse(resize.options.body), { rows: 31, cols: 101 });
    await app.respond(resize, {});
    await app.tick(50);
    await app.respond(app.take('/api/terminal/existing/read?cursor=6&wait_ms=15000'), {
      text: '', data_base64: '', cursor: 6, alive: false, exit_code: 0,
    });
    assert.equal(app.status(), 'session ended (exit 0)');
    await app.tick(50);
    assert.equal(app.requests.length, 0, 'a naturally ended session stops polling');
  });
}

test('serial creation sends explicit port and baud and displays a controlled refusal', async () => {
  const app = await emptyPanel();
  const created = app.click('term-serial');
  const request = app.take('/api/terminal/create');
  assert.deepEqual(JSON.parse(request.options.body), {
    kind: 'serial', rows: 30, cols: 100, port: 'COM_TEST', baud: 57600,
  });
  assert.equal(request.options.headers['x-summrise-panel'], 'test-page-auth');
  await app.respond(request, { error: 'serial fixture refusal' });
  await created;
  assert.equal(app.status(), '{"error":"serial fixture refusal"}');
  assert.equal(app.requests.length, 0, 'a refusal starts no read loop');
});


// Observe calls the real IIFE makes at its xterm boundary without changing their effects.
function terminalCalls(terminal) {
  const counts = { open: 0, focus: 0, reset: 0 };
  for (const name of Object.keys(counts)) {
    const original = terminal[name].bind(terminal);
    terminal[name] = (...args) => { counts[name] += 1; return original(...args); };
  }
  return () => ({ ...counts });
}

test('a late bootstrap list cannot redirect a live session or its queued input', async () => {
  const app = panel();
  const bootstrap = app.take('/api/terminal');
  const calls = terminalCalls(app.terminal);
  const first = await newPty(app, 'current');
  await app.respond(first, { text: 'B', data_base64: 'Qg==', cursor: 1, alive: true, exit_code: null });
  await app.tick(50);
  const next = app.take('/api/terminal/current/read?cursor=1&wait_ms=15000');
  const activeCalls = calls();
  app.terminal.input('queued-current\r');
  await app.respond(bootstrap, { sessions: [{ id: 'listed', kind: 'pty' }] });
  await app.tick(40);
  const input = app.take('/api/terminal/current/write');
  assert.equal(input.options.method, 'POST');
  assert.equal(input.options.headers['x-summrise-panel'], 'test-page-auth');
  assert.deepEqual(JSON.parse(input.options.body), { data: 'queued-current\r' });
  assert.equal(app.requests.length, 0, 'the superseded list cannot start a listed-session read or write');
  assert.equal(app.status(), 'pty current · live');
  assert.equal(Buffer.concat(app.terminal.writes).toString('utf8'), 'B');
  assert.deepEqual(calls(), activeCalls, 'the superseded list cannot reopen, reset or refocus xterm');
  await app.respond(input, {});
  await app.respond(next, { text: 'C', data_base64: 'Qw==', cursor: 2, alive: true, exit_code: null });
  await app.tick(50);
  app.take('/api/terminal/current/read?cursor=2&wait_ms=15000');
  assert.equal(Buffer.concat(app.terminal.writes).toString('utf8'), 'BC');
});

test('New pty invalidates bootstrap attachment before its create acknowledgement', async () => {
  const app = panel();
  const bootstrap = app.take('/api/terminal');
  const calls = terminalCalls(app.terminal);
  const creating = app.click('term-pty');
  const create = app.take('/api/terminal/create');
  const pendingCalls = calls();
  await app.respond(bootstrap, { sessions: [{ id: 'listed', kind: 'pty' }] });
  assert.equal(app.requests.length, 0, 'an earlier list cannot start a read while a newer create is pending');
  assert.deepEqual(calls(), pendingCalls, 'the earlier list cannot claim the canvas or focus');
  await app.respond(create, { id: 'current', kind: 'pty' });
  await creating;
  app.take('/api/terminal/current/read?cursor=0&wait_ms=15000');
  assert.equal(app.status(), 'pty current · live');
});

test('Close without an attached session invalidates a pending bootstrap list', async () => {
  const app = panel();
  const bootstrap = app.take('/api/terminal');
  const calls = terminalCalls(app.terminal);
  await app.click('term-close');
  const closedCalls = calls();
  const closedStatus = app.status();
  await app.respond(bootstrap, { sessions: [{ id: 'listed', kind: 'pty' }] });
  await app.tick(40);
  await app.tick(50);
  assert.equal(app.requests.length, 0, 'Close intent cannot be undone by a late list read');
  assert.equal(app.status(), closedStatus);
  assert.deepEqual(calls(), closedCalls, 'a canceled bootstrap cannot open, reset or focus xterm');
});

test('coalesced create intents release the stale allocation before the latest session', async () => {
  const app = await emptyPanel();
  const calls = terminalCalls(app.terminal);
  const older = app.click('term-pty');
  const oldCreate = app.take('/api/terminal/create');
  const newer = app.click('term-pty');
  await settle();
  assert.equal(app.requests.length, 0, 'only one create may be unresolved');
  const before = calls();
  await app.respond(oldCreate, { id: 'retired', kind: 'pty' });
  const release = app.take('/api/terminal/retired/close');
  assert.equal(release.options.headers['x-summrise-panel'], 'test-page-auth');
  assert.equal(app.requests.length, 0, 'the newer intent waits for stale allocation cleanup');
  assert.deepEqual(calls(), before, 'retired allocation cannot claim the view');
  await app.respond(release, { closed: true });
  const create = app.take('/api/terminal/create');
  await app.respond(create, { id: 'current', kind: 'pty' });
  await Promise.all([older, newer]);
  const first = app.take('/api/terminal/current/read?cursor=0&wait_ms=15000');
  await app.respond(first, { text: 'B', data_base64: 'Qg==', cursor: 1, alive: true, exit_code: null });
  await app.tick(50);
  app.take('/api/terminal/current/read?cursor=1&wait_ms=15000');
  app.terminal.input('current-only\r'); await app.tick(40);
  const input = app.take('/api/terminal/current/write');
  assert.deepEqual(JSON.parse(input.options.body), { data: 'current-only\r' });
  assert.equal(app.status(), 'pty current · live');
  assert.equal(Buffer.concat(app.terminal.writes).toString('utf8'), 'B');
});

test('Close retires an in-flight create and releases its late allocation without opening it', async () => {
  const app = await emptyPanel();
  const calls = terminalCalls(app.terminal);
  const creating = app.click('term-pty');
  const create = app.take('/api/terminal/create');
  const closing = app.click('term-close');
  const closedCalls = calls();
  const closedStatus = app.status();
  await app.respond(create, { id: 'retired', kind: 'pty' });
  const release = app.take('/api/terminal/retired/close');
  assert.equal(release.options.method, 'POST');
  assert.equal(release.options.headers['x-summrise-panel'], 'test-page-auth');
  assert.equal(app.requests.length, 0, 'a late allocation cannot start reading after Close');
  assert.equal(app.status(), closedStatus);
  assert.deepEqual(calls(), closedCalls);
  await app.respond(release, { closed: true });
  await Promise.all([creating, closing]);
  await app.tick(40);
  await app.tick(50);
  assert.equal(app.requests.length, 0);
});

test('newer New intents join required cleanup and only the latest creates', async () => {
  const app = await emptyPanel();
  await newPty(app, 'old');
  const older = app.click('term-pty');
  const oldClose = app.take('/api/terminal/old/close');
  const newer = app.click('term-pty');
  await settle();
  assert.equal(app.requests.length, 0, 'no duplicate close or create before the shared acknowledgement');
  await app.respond(oldClose, { closed: true });
  const create = app.take('/api/terminal/create');
  await app.respond(create, { id: 'current', kind: 'pty' });
  await Promise.all([older, newer]);
  app.take('/api/terminal/current/read?cursor=0&wait_ms=15000');
  assert.equal(app.requests.length, 0, 'only the latest intent allocates');
  app.terminal.input('current-only\r'); await app.tick(40);
  assert.deepEqual(JSON.parse(app.take('/api/terminal/current/write').options.body), { data: 'current-only\r' });
  assert.equal(app.status(), 'pty current · live');
});

// Release acknowledgements are server facts. Failures retain exact-ID ownership until an
// explicit action retries it; no test reaches into production cleanup/coalescing internals.
const failedReleases = [
  ['audit refusal', 500, { error: 'audit_unwritable', detail: "the device's event log could not be written, so the operation was not run" }],
  ['unconfirmed object', 200, {}],
  ['negative confirmation', 200, { closed: false }],
  ['generic refusal', 400, { error: 'terminal_close_refused', detail: 'session owned is unknown or has been closed' }],
  ['invalid id', 400, { error: 'invalid_session', detail: 'invalid session' }],
  ['authorization refusal', 403, { error: 'panel_only', detail: 'not admitted' }],
  ['capacity refusal', 409, { error: 'too_many_txns', detail: 'capacity reached' }],
];
for (const [label, status, answer] of failedReleases) {
  test(`${label} cannot confirm release or allow New; explicit retry keeps the exact id`, async () => {
    const app = await emptyPanel();
    const read = await newPty(app, 'owned');
    app.terminal.input('retired-input\r');
    const creating = app.click('term-pty');
    const release = app.take('/api/terminal/owned/close');
    assert.equal(release.options.method, 'POST');
    assert.equal(release.options.headers['x-summrise-panel'], 'test-page-auth');
    await app.respond(release, answer, status);
    assert.equal(app.requests.length, 0, 'an unconfirmed release must block replacement allocation');
    await creating;
    assert.match(app.status(), /failed|unable|retry/i);
    assert.doesNotMatch(app.status(), /session closed|\blive\b/);
    await app.respond(read, { text: 'late', data_base64: 'bGF0ZQ==', cursor: 99, alive: false, exit_code: 0 });
    app.terminal.input('after-failure\r'); app.terminal.resize({ rows: 31, cols: 101 });
    await app.tick(40); await app.tick(50); await app.tick(300);
    assert.equal(app.requests.length, 0, 'no old IO or automatic cleanup retry');
    assert.deepEqual(app.terminal.writes, []);
    const retrying = app.click('term-pty');
    const retry = app.take('/api/terminal/owned/close');
    assert.equal(retry.options.headers['x-summrise-panel'], 'test-page-auth');
    assert.equal(app.requests.length, 0);
    await app.respond(retry, { closed: true });
    const create = app.take('/api/terminal/create');
    await app.respond(create, { id: 'replacement', kind: 'pty' }); await retrying;
    app.take('/api/terminal/replacement/read?cursor=0&wait_ms=15000');
    app.terminal.input('current-only\r'); await app.tick(40);
    assert.deepEqual(JSON.parse(app.take('/api/terminal/replacement/write').options.body), { data: 'current-only\r' });
  });
}

test('a transport-failed Close retains ownership and retries only on an explicit action', async () => {
  const app = await emptyPanel(); await newPty(app, 'owned');
  const closing = app.click('term-close');
  await app.reject(app.take('/api/terminal/owned/close')); await closing;
  assert.match(app.status(), /failed|unable|retry/i);
  assert.doesNotMatch(app.status(), /session closed/);
  await app.tick(40); await app.tick(50); await app.tick(300);
  assert.equal(app.requests.length, 0);
  const retrying = app.click('term-close');
  await app.respond(app.take('/api/terminal/owned/close'), { closed: true }); await retrying;
  assert.equal(app.status(), 'session closed');
  await app.click('term-close');
  assert.equal(app.requests.length, 0, 'confirmed release does not repeat');
});

test('only typed unknown-session absence can finish an already missing owned release', async () => {
  const app = await emptyPanel(); await newPty(app, 'owned');
  const creating = app.click('term-pty');
  await app.respond(app.take('/api/terminal/owned/close'), {
    error: 'unknown_session', detail: 'session owned is unknown or has been closed',
  }, 400);
  const create = app.take('/api/terminal/create');
  await app.respond(create, { id: 'replacement', kind: 'pty' }); await creating;
  app.take('/api/terminal/replacement/read?cursor=0&wait_ms=15000');
  assert.equal(app.status(), 'pty replacement · live');
});

test('a failed stale allocation release stops queued New until explicit retry', async () => {
  const app = await emptyPanel();
  const calls = terminalCalls(app.terminal);
  const older = app.click('term-pty'); const oldCreate = app.take('/api/terminal/create');
  const newer = app.click('term-pty');
  await settle(); assert.equal(app.requests.length, 0, 'one allocation request at a time');
  const before = calls();
  await app.respond(oldCreate, { id: 'retired', kind: 'pty' });
  const release = app.take('/api/terminal/retired/close');
  await app.respond(release, { error: 'audit_unwritable', detail: 'operation was not run' }, 500);
  await Promise.all([older, newer]);
  assert.deepEqual(calls(), before, 'unselected allocation never opens or resets the view');
  assert.match(app.status(), /failed|unable|retry/i);
  await app.tick(40); await app.tick(300);
  assert.equal(app.requests.length, 0, 'queued New is not an automatic retry after cleanup fails');
  const retrying = app.click('term-pty');
  await app.respond(app.take('/api/terminal/retired/close'), { closed: true });
  const create = app.take('/api/terminal/create');
  await app.respond(create, { id: 'current', kind: 'pty' }); await retrying;
  app.take('/api/terminal/current/read?cursor=0&wait_ms=15000');
  assert.equal(app.status(), 'pty current · live');
});

test('Close cancels a create burst but retains its late allocation after release rejection', async () => {
  const app = await emptyPanel();
  const actions = [app.click('term-pty')]; const create = app.take('/api/terminal/create');
  for (let index = 0; index < 40; index += 1) actions.push(app.click('term-pty'));
  actions.push(app.click('term-close'));
  await settle(); assert.equal(app.requests.length, 0, 'forty intents cannot issue forty allocations');
  await app.respond(create, { id: 'retired', kind: 'pty' });
  await app.reject(app.take('/api/terminal/retired/close'));
  await Promise.all(actions);
  assert.match(app.status(), /failed|unable|retry/i);
  await app.tick(40); await app.tick(300);
  assert.equal(app.requests.length, 0);
  const retrying = app.click('term-close');
  await app.respond(app.take('/api/terminal/retired/close'), { closed: true }); await retrying;
  assert.equal(app.status(), 'session closed');
  assert.equal(app.requests.length, 0, 'Close did not resurrect any queued create');
});

test('a create burst coalesces to the latest serial fields captured at action start', async () => {
  const app = await emptyPanel();
  const actions = [app.click('term-pty')]; const first = app.take('/api/terminal/create');
  for (let index = 0; index < 40; index += 1) actions.push(app.click('term-pty'));
  app.element('term-port').value = 'COM_LATEST'; app.element('term-baud').value = '38400';
  actions.push(app.click('term-serial'));
  app.element('term-port').value = 'COM_CHANGED_AFTER_CLICK'; app.element('term-baud').value = '9600';
  await settle(); assert.equal(app.requests.length, 0, 'create is single flight independently of backend cap');
  await app.respond(first, { id: 'retired', kind: 'pty' });
  const close = app.take('/api/terminal/retired/close');
  assert.equal(app.requests.length, 0);
  await app.respond(close, { closed: true });
  const latest = app.take('/api/terminal/create');
  assert.deepEqual(JSON.parse(latest.options.body), { kind: 'serial', rows: 30, cols: 100, port: 'COM_LATEST', baud: 38400 });
  await app.respond(latest, { id: 'serial-current', kind: 'serial' }); await Promise.all(actions);
  app.take('/api/terminal/serial-current/read?cursor=0&wait_ms=15000');
  assert.equal(app.requests.length, 0);
  assert.equal(app.status(), 'serial serial-current · live');
});
