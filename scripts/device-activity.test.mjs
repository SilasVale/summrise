import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';

const sourceUrl = new URL('../device/device/src/activity.rs', import.meta.url);
const rust = readFileSync(sourceUrl, 'utf8');
const script = rust.match(/pub\(crate\) const VIEW_SCRIPT: &str = r##"([\s\S]*?)"##;/)?.[1];
assert.ok(script, 'the current Rust page must supply its production VIEW_SCRIPT');

// Drain only finite promise continuations. Timers and HTTP remain manually driven;
// these tests never use a real clock, browser, service or network.
async function settle() {
  for (let turn = 0; turn < 20; turn += 1) await Promise.resolve();
}

// Minimal DOM boundary for the existing server-rendered feed/run. There is no
// alternate renderer or business logic here: VIEW_SCRIPT creates the output nodes,
// chooses requests, owns the cursor, follows the tail and retires late responses.
class Element {
  constructor(tag = 'div') {
    this.tagName = tag.toUpperCase();
    this.children = [];
    this.attributes = {};
    this.listeners = {};
    this.isConnected = true;
    this.open = false;
    this.text = '';
  }

  get textContent() { return this.text + this.children.map(child => child.textContent).join(''); }
  set textContent(value) { this.text = String(value); this.children = []; }
  append(node) { this.children.push(node); }
  getAttribute(name) { return this.attributes[name] ?? null; }
  setAttribute(name, value) { this.attributes[name] = String(value); }
  addEventListener(name, listener) { this.listeners[name] = listener; }
  querySelectorAll() { return []; }
  querySelector() { return null; }
}

function activity({ running = false } = {}) {
  const requests = [];
  const intervals = new Map();
  const scrolls = [];
  const events = {};
  let nextInterval = 0;
  const status = new Element('span');
  status.textContent = 'live';
  const total = new Element('span');
  total.textContent = '1';
  const transcript = new Element('details');
  const run = new Element('details');
  run.setAttribute('data-run', 'run-observed');
  const body = new Element();
  run.querySelector = selector => selector === '.activity-run-body' ? body : null;
  const duration = new Element('span');
  duration.textContent = 'pending duration';
  const runningRow = new Element('article');
  runningRow.setAttribute('data-id', '1');
  runningRow.setAttribute('data-status', 'running');
  runningRow.setAttribute('data-started', '1000');
  runningRow.querySelector = selector => selector === '.activity-duration' ? duration : null;
  const feed = new Element();
  feed.attributes = { 'data-boot': '123', 'data-next': '123.1', 'data-state': 'rows' };
  feed.querySelectorAll = selector => {
    if (selector === 'details[data-run]') return [run];
    if (running && (selector === '.activity-row[data-id]' || selector === '[data-status="running"][data-started]')) return [runningRow];
    return [];
  };
  feed.closest = () => transcript;
  const meta = new Element('meta');
  meta.setAttribute('content', 'test-page-auth');
  const document = {
    body: { scrollHeight: 2000 },
    getElementById: id => id === 'feed' ? feed : id === 'activity-state' ? status : null,
    querySelector: selector => {
      if (selector === 'meta[name="summrise-panel"]') return meta;
      if (selector === '[data-count-total]') return total;
      return null;
    },
    querySelectorAll: () => [],
    createElement: tag => new Element(tag),
    addEventListener: (name, listener) => { events[name] = listener; },
  };
  const window = {
    reloads: 0,
    location: {
      href: 'http://127.0.0.1/activity?session=one',
      reload() { window.reloads += 1; },
    },
    addEventListener: (name, listener) => { events[name] = listener; },
    setInterval(callback) {
      const id = ++nextInterval;
      intervals.set(id, callback);
      return id;
    },
    clearInterval(id) { intervals.delete(id); },
    scrollTo: (x, y) => scrolls.push([x, y]),
  };
  class Clock extends Date {
    static now() { return 2000; }
  }

  vm.runInNewContext(script, {
    document, window,
    fetch(url, options) {
      return new Promise((resolve, reject) => requests.push({ url, options, resolve, reject }));
    },
    Date: Clock,
  }, { filename: sourceUrl.pathname });

  function take(url) {
    const index = requests.findIndex(request => request.url === url);
    assert.notEqual(index, -1, `expected ${url}; pending: ${requests.map(request => request.url)}`);
    return requests.splice(index, 1)[0];
  }

  async function respond(request, answer, status = 200) {
    request.resolve({ status, ok: status >= 200 && status < 300, json: async () => answer });
    await settle();
  }

  async function poll(answer = { rows: [], next: 2, more: false, counts: null }) {
    const request = requests.find(request => request.url.startsWith('/api/activity?'));
    assert.ok(request, 'the production page must request a poll');
    await respond(take(request.url), answer);
  }

  async function tickIntervals() {
    for (const callback of [...intervals.values()]) callback();
    await settle();
  }

  function toggleRun(open) {
    run.open = open;
    run.listeners.toggle();
  }

  return {
    requests, intervals, scrolls, events, status, total, transcript, run, body,
    document, window, duration, feed, take, respond, poll, tickIntervals, toggleRun,
  };
}

test('a late poll for an old query cannot change the current view or keep polling', async () => {
  const app = activity();
  const request = app.take('/api/activity?after=123.1&limit=200');
  app.window.location.href = 'http://127.0.0.1/activity?session=two';
  app.status.textContent = 'awaiting new view';
  await app.respond(request, { rows: [], next: 2, counts: { total: 42, failed: 0, groups: {} } });
  assert.equal(app.status.textContent, 'awaiting new view');
  assert.equal(app.total.textContent, '1', 'retired counts cannot replace the selected page counts');
  assert.equal(app.scrolls.length, 0);
  await app.tickIntervals();
  assert.equal(app.requests.length, 0, 'the old view cannot request another poll');
});

test('pagehide retires polling and cached restoration reloads the page only once', async () => {
  const app = activity();
  const request = app.take('/api/activity?after=123.1&limit=200');
  app.events.pagehide();
  app.status.textContent = 'left';
  await app.respond(request, { rows: [], next: 3 });
  assert.equal(app.status.textContent, 'left');
  assert.equal(app.intervals.size, 0, 'leaving clears polling and duration timers');
  assert.equal(app.window.reloads, 0, 'a late response cannot reload an inactive view');
  app.events.pageshow({ persisted: false });
  assert.equal(app.window.reloads, 0);
  app.events.pageshow({ persisted: true });
  app.events.pageshow({ persisted: true });
  assert.equal(app.window.reloads, 1, 'cached restoration obtains a fresh server ledger');
});

test('a failed poll reports stale and the next controlled poll can recover', async () => {
  const app = activity();
  const request = app.take('/api/activity?after=123.1&limit=200');
  request.reject(new Error('offline fixture'));
  await settle();
  assert.match(app.status.textContent, /stale.*retrying/i);
  await app.tickIntervals();
  await app.poll();
  assert.equal(app.status.textContent, 'live');
  assert.equal(app.window.reloads, 0);
});

for (const hash of ['#main', '#all-activity']) {
  test(`${hash} keeps polling, cursor advancement and running durations in the same view`, async () => {
    const app = activity({ running: true });
    app.status.textContent = 'pending fragment';
    app.window.location.href += hash;
    await app.poll();
    assert.equal(app.status.textContent, 'live');
    await app.tickIntervals();
    app.take('/api/activity?after=123.2&limit=200');
    assert.equal(app.duration.textContent, '1.0s');
  });
}

test('opening an SSR run fetches and renders the observed command, result and output once', async () => {
  const app = activity();
  await app.poll();
  app.toggleRun(true);
  const request = app.take('/api/runs/run-observed');
  assert.equal(request.options.headers['x-summrise-panel'], 'test-page-auth');
  assert.equal(request.options.cache, 'no-store');
  await app.respond(request, {
    run: {
      command: 'observed-command', args: ['observed-argument'], cwd: '/recorded/path',
      exit_code: 7, duration_ms: 140, stdout_bytes: 9, stderr_bytes: 3,
      stdout_preview: 'observed output',
    },
  });
  assert.equal(app.body.children[0].tagName, 'DL');
  assert.deepEqual(app.body.children[0].children.map(node => node.textContent), [
    'command', 'observed-command observed-argument', 'cwd', '/recorded/path',
    'result', 'exit 7 · 140ms', 'output', 'stdout 9 B · stderr 3 B',
  ]);
  assert.equal(app.body.children.at(-1).textContent, 'observed output');
  app.toggleRun(false);
  app.toggleRun(true);
  assert.equal(app.requests.length, 0, 'reopening an already loaded run preserves its body');
});

test('a run request failure has visible feedback', async () => {
  const app = activity();
  await app.poll();
  app.toggleRun(true);
  await app.respond(app.take('/api/runs/run-observed'), null, 503);
  assert.match(app.body.textContent, /run run-observed unavailable \(status 503\)/);
});

test('a detached run body rejects a late successful response', async () => {
  const app = activity();
  await app.poll();
  app.toggleRun(true);
  const request = app.take('/api/runs/run-observed');
  app.body.isConnected = false;
  await app.respond(request, { run: { command: 'late' } });
  assert.equal(app.body.children.length, 0);
});

test('pagehide rejects a late run response even while its body remains connected', async () => {
  const app = activity();
  await app.poll();
  app.toggleRun(true);
  const request = app.take('/api/runs/run-observed');
  app.events.pagehide();
  await app.respond(request, { run: { command: 'late' } });
  assert.equal(app.body.children.length, 0);
  assert.equal(app.window.reloads, 0);
});

test('only an open transcript follows and f toggles following against fresh tail geometry', async () => {
  const app = activity();
  await app.poll();
  assert.equal(app.scrolls.length, 0, 'a closed transcript cannot pull the reader to its tail');
  app.transcript.open = true;
  await app.tickIntervals();
  await app.poll();
  assert.deepEqual(app.scrolls.at(-1), [0, 2000]);
  app.events.keydown({ key: 'f', target: { tagName: 'BODY' } });
  const before = app.scrolls.length;
  await app.tickIntervals();
  await app.poll();
  assert.equal(app.scrolls.length, before, 'f disables tail following');
  app.events.keydown({ key: 'f', target: { tagName: 'BODY' } });
  app.document.body.scrollHeight = 3000;
  await app.tickIntervals();
  await app.poll();
  assert.deepEqual(app.scrolls.at(-1), [0, 3000]);
});

test('closing a pre-opened run disclosure performs no request', async () => {
  const app = activity();
  await app.poll();
  app.run.open = true;
  app.toggleRun(false);
  assert.equal(app.requests.length, 0);
});
