// Acceptance for the device's actual Rust pages. No server, framework, or installed Node packages.
// The ignored Rust test owns the loopback server and its temporary registry. Exit 2 is a missing
// browser/runtime, never a successful skip. Reports contain geometry/status only, never tokens or
// request/terminal payloads. Controlled failures are explicitly labelled in each interaction.
import { spawn, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { randomUUID } from 'node:crypto';

const USAGE = 'usage: node scripts/device-ui-browser.mjs --base-url <loopback-url> --out <owned-dir>';
const REQUIRED_CASES = [
  'panel-normal', 'panel-first', 'panel-empty', 'panel-unreadable', 'panel-bounded-long',
  'activity-working', 'activity-refused', 'activity-closed-failure', 'activity-aborted',
  'activity-orphan', 'activity-no-goal', 'activity-no-session-empty',
  'activity-no-session-recorded', 'activity-unreadable', 'activity-multi-long'
];
const delay = ms => new Promise(r => setTimeout(r, ms));
const requireThat = (value, detail) => { if (!value) throw new Error(detail); };
class MissingBrowser extends Error {}

export function parseCli(args) {
  if (args.length === 1 && args[0] === '--help') return { help: true };
  const values = {};
  for (let i = 0; i < args.length; i += 2) {
    requireThat(['--base-url', '--out'].includes(args[i]) && args[i + 1] && !values[args[i]], USAGE);
    values[args[i]] = args[i + 1];
  }
  requireThat(values['--base-url'] && values['--out'], USAGE);
  let base;
  try { base = new URL(values['--base-url']); } catch { throw new Error('base URL must be loopback HTTP'); }
  requireThat(base.protocol === 'http:' && ['127.0.0.1', '[::1]', 'localhost'].includes(base.hostname)
    && !base.username && !base.password && base.pathname === '/' && !base.search && !base.hash,
  'base URL must be a loopback HTTP origin');
  return { baseUrl: base.origin, out: resolve(values['--out']) };
}

function localUrl(value, baseUrl) {
  requireThat(typeof value === 'string' && value.startsWith('/') && !value.startsWith('//')
    && !value.includes('\\') && !/[\r\n\0]/.test(value), 'manifest URL must be relative to the loopback origin');
  const url = new URL(value, baseUrl);
  requireThat(url.origin === baseUrl && !url.username && !url.password, 'manifest URL must be relative');
  return url.href;
}

export function validateManifest(manifest, baseUrl) {
  requireThat(manifest && Array.isArray(manifest.cases) && manifest.cases.length <= 100, 'invalid fixture matrix');
  const names = new Set();
  for (const item of manifest.cases) {
    requireThat(item && /^[a-z][a-z0-9-]{0,79}$/.test(item.name) && !names.has(item.name), 'invalid matrix case name');
    requireThat(['panel', 'activity'].includes(item.kind), 'invalid matrix case kind');
    requireThat(item.expectedH1Count === undefined || Number.isInteger(item.expectedH1Count)
      && item.expectedH1Count >= 0 && item.expectedH1Count <= 1, 'invalid matrix heading expectation');
    localUrl(item.url, baseUrl);
    requireThat(new URL(item.url, baseUrl).pathname === '/__ui-fixtures/' + item.name, 'matrix must use actual Rust fixture route');
    names.add(item.name);
  }
  requireThat(REQUIRED_CASES.every(name => names.has(name)), 'incomplete required fixture matrix');
  requireThat(manifest.live && typeof manifest.live === 'object', 'missing live page manifest');
  for (const name of ['panel', 'activity', 'root']) localUrl(manifest.live[name], baseUrl);
  return manifest; // Preserve opaque metadata added by the Rust owner.
}

function findChrome() {
  const candidates = process.env.CHROME ? [process.env.CHROME]
    : ['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser'];
  const chrome = candidates.find(name => spawnSync(name, ['--version'], { stdio: 'ignore', timeout: 5000 }).status === 0);
  if (!chrome) throw new MissingBrowser('no Chromium found (set CHROME to an installed browser)');
  if (typeof WebSocket !== 'function' || typeof fetch !== 'function') throw new MissingBrowser('Node with built-in WebSocket and fetch is required');
  return chrome;
}

// Bounded CDP commands reject on protocol errors, socket loss, and timeout. No error object from
// Chrome is printed: those objects can contain expression source, URLs, or private page values.
function commandDiagnostic(error) {
  // A NODE FS ERROR IS ITSELF THE ANSWER. The errno says whether the profile was busy
  // (ENOTEMPTY/EBUSY), unreadable (EACCES) or already gone (ENOENT); dropping it left the
  // CI failure unreadable ("kind":"command" and nothing else).
  const errno = typeof error.code === 'string' && error.code ? error.code : null;
  return { kind: error.kind || (errno ? 'cleanup' : 'command'), ...(error.method ? { method: error.method } : {}),
    ...(Number.isInteger(error.protocolCode) ? { code: error.protocolCode } : {}),
    ...(error.protocolClass ? { classification: error.protocolClass } : {}), ...(error.protocolReason ? { reason: error.protocolReason } : {}),
    ...(errno && !error.protocolReason ? { reason: errno + (error.syscall ? ' during ' + error.syscall : '') } : {}),
    ...(error.fetch ? { fetch: error.fetch } : {}) };
}
function protocolClass(message) {
  if (/interception|request.*id|invalid.*state|already|not.*paused/i.test(message)) return 'interception-state';
  if (/method/i.test(message)) return 'method';
  return 'protocol';
}

function protocolReason(message) {
  if (/invalid interception.?id|invalid.*request.*id/i.test(message)) return 'invalid-interception-id';
  if (/only.*response.*stage|not.*response.*stage|headers.*received/i.test(message)) return 'wrong-response-stage';
  if (/already|handled/i.test(message)) return 'already-handled';
  if (/body.*taken|body.*stream/i.test(message)) return 'body-taken';
  return 'other';
}
function fetchErrorReason(reason) {
  return ['Failed','Aborted','TimedOut','AccessDenied','ConnectionClosed','ConnectionReset','ConnectionRefused','ConnectionAborted','ConnectionFailed','NameNotResolved','InternetDisconnected','AddressUnreachable','BlockedByClient','BlockedByResponse'].includes(reason) ? reason : reason === undefined ? 'absent' : 'other';
}

export class Cdp {
  constructor(socket) {
    this.socket = socket; this.next = 0; this.pending = new Map(); this.listeners = new Map(); this.paused = new Map();
    this.fetchTasks = new Set(); this.releaseTasks = new Set(); this.commandFailures = []; this.eventErrors = []; this.pauseMetadata = new Map(); this.pageEpoch = 0; this.interaction = 'startup'; this.sequence = 0;
    socket.addEventListener('message', event => {
      this.sequence += 1;
      let message;
      try { message = JSON.parse(String(event.data)); } catch { return; }
      if (message.id) {
        const entry = this.pending.get(message.id);
        if (!entry) return;
        this.pending.delete(message.id); clearTimeout(entry.timer);
        if (message.error) entry.reject(Object.assign(new Error('CDP protocol error: ' + entry.method),
          { kind: 'protocol', method: entry.method, protocolCode: message.error.code, protocolClass: protocolClass(message.error.message || ''), protocolReason: protocolReason(message.error.message || ''), ...(entry.fetch ? { fetch: { ...entry.fetch, cancellation: entry.lease?.cancelSequence ? entry.lease.cancelSequence < entry.releaseSequence ? 'before-release' : 'during-release' : 'not-observed' } } : {}) }));
        else entry.resolve(message.result || {});
      } else {
        for (const listener of this.listeners.get(message.method) || []) {
          const task = Promise.resolve().then(() => listener(message.params || {})).catch(error => {
            this.eventFailure = true;
            if (this.eventErrors.length < 32) this.eventErrors.push({ event: message.method, ...commandDiagnostic(error) });
          });
          if (message.method === 'Fetch.requestPaused') {
            this.fetchTasks.add(task); task.then(() => this.fetchTasks.delete(task));
          }
        }
      }
    });
    socket.addEventListener('close', () => this.rejectPending());
    socket.addEventListener('error', () => this.rejectPending('transport'));
  }
  rejectPending(kind = 'closed') {
    for (const p of this.pending.values()) { clearTimeout(p.timer); p.reject(Object.assign(new Error('CDP connection closed'), { kind, method: p.method })); }
    this.pending.clear();
  }
  on(name, listener) {
    const list = this.listeners.get(name) || new Set(); list.add(listener); this.listeners.set(name, list);
    return () => list.delete(listener);
  }
  send(method, params = {}, origin = 'direct') {
    if (method === 'Browser.close' && this.browserControl) return this.browserControl.send(method, params);
    const release = /^Fetch\.(?:continueRequest|continueResponse|failRequest|fulfillRequest)$/.test(method);
    // Claim this pause synchronously. Waiting for Chrome's ACK leaves it visible to cleanup, and
    // that ACK can also arrive after the SAME request's response-stage pause has been recorded.
    const lease = this.fetchLeases?.get(params.requestId);
    const releaseSequence = release ? ++this.sequence : 0;
    const firstClaim = !!lease && !lease.claimed;
    const metadata = this.pauseMetadata.get(params.requestId);
    const fetch = metadata ? { ...metadata, origin, ownership: this.paused.has(params.requestId) ? 'paused' : 'absent', firstClaim } : undefined;
    if (release && lease && firstClaim) { lease.claimed = true; lease.releaseSequence = releaseSequence; }
    if (release) { this.paused.delete(params.requestId); this.pauseMetadata.delete(params.requestId); }
    const command = new Promise((resolveCommand, reject) => {
      const id = ++this.next;
      const timer = setTimeout(() => { this.pending.delete(id); reject(Object.assign(new Error('CDP command timeout: ' + method), { kind: 'timeout', method })); }, 15000);
      this.pending.set(id, { resolve: resolveCommand, reject, timer, method, fetch, lease, releaseSequence, firstClaim });
      try { this.socket.send(JSON.stringify({ id, method, params })); }
      catch { clearTimeout(timer); this.pending.delete(id); reject(Object.assign(new Error('CDP send failed'), { kind: 'transport', method })); }
    });
    if (release) {
      this.releaseTasks.add(command);
      command.then(() => this.releaseTasks.delete(command), error => {
        this.releaseTasks.delete(command);
        if (this.commandFailures.length < 32) this.commandFailures.push(commandDiagnostic(error));
      });
    }
    return command;
  }
  async drainFetch() {
    const end = Date.now() + 15000;
    while (this.fetchTasks.size || this.releaseTasks.size) {
      if (Date.now() >= end) throw Object.assign(new Error('Fetch handlers did not drain'), { kind: 'timeout', method: 'Fetch.drain' });
      const work = [...this.fetchTasks, ...this.releaseTasks];
      let timer;
      let answers;
      try {
        answers = await Promise.race([Promise.allSettled(work), new Promise((_, reject) => {
          timer = setTimeout(() => reject(Object.assign(new Error('Fetch handlers did not drain'), { kind: 'timeout', method: 'Fetch.drain' })), Math.max(1, end - Date.now()));
        })]);
      } finally { clearTimeout(timer); }
      const failed = answers.find(answer => answer.status === 'rejected');
      if (failed) throw failed.reason;
    }
  }
  async evaluate(expression) {
    const answer = await this.send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true, userGesture: true });
    requireThat(!answer.exceptionDetails && answer.result?.type !== 'error', 'browser evaluation failed');
    return answer.result?.value;
  }
  async until(expression, detail, timeout = 10000) {
    const end = Date.now() + timeout;
    while (Date.now() < end) {
      if (await this.evaluate(expression)) return;
      await delay(60);
    }
    throw new Error(detail);
  }
  close() { this.socket.close(); this.rejectPending(); }
}

export async function openBrowser(chrome = findChrome()) {
  const profile = mkdtempSync(join(realpathSync(tmpdir()), 'device-ui-browser-'));
  // THE OWNED CHROME GETS ITS OWN PROCESS GROUP. Chrome's helpers - the zygote, the
  // renderers, the GPU process - outlive the launcher when the launcher is signalled, and an
  // orphan that still holds the profile keeps that directory busy, so the removal below fails
  // with a real ENOTEMPTY/EBUSY and the evidence says the cleanup failed for a reason nobody
  // can act on. A group of its own is what makes "end this invocation's Chrome" mean the whole
  // tree, and only this invocation's tree: the group is created by this spawn.
  const browser = spawn(chrome, ['--headless=new', '--disable-gpu', '--no-sandbox', '--disable-dev-shm-usage',
    '--disable-background-networking', '--disable-component-update', '--no-first-run', '--no-default-browser-check',
    '--no-proxy-server', '--remote-debugging-address=127.0.0.1', '--remote-debugging-port=0',
    `--user-data-dir=${profile}`, 'about:blank'], { stdio: 'ignore', detached: true });
  // End every process of this invocation's group, and the launcher itself. The group id is the
  // launcher's pid because the detached spawn made it a group leader; a group that is already
  // gone answers ESRCH, which is the ordinary "nothing left to end" case.
  const signalOwnedTree = signal => {
    let signalled = false;
    if (typeof browser.pid === 'number') {
      try { process.kill(-browser.pid, signal); signalled = true; } catch { signalled = false; }
    }
    try { browser.kill(signal); signalled = true; } catch { /* the launcher is already gone */ }
    return signalled;
  };
  // A node that dies must not leave its owned Chrome running: the group is ended here as well.
  const endOwnedTreeOnExit = () => signalOwnedTree('SIGKILL');
  // Whether any process of this invocation's group is still alive. A group that is gone answers
  // ESRCH, which is the ordinary "nothing left" case.
  const ownedTreeAlive = () => {
    if (typeof browser.pid !== 'number') return false;
    try { process.kill(-browser.pid, 0); return true; } catch { return false; }
  };
  process.once('exit', endOwnedTreeOnExit);
  let spawnFailed = false;
  browser.on('error', () => { spawnFailed = true; });
  let cdp;
  let browserControl;
  const stop = async () => {
    let politeError;
    let closeAcknowledged = false;
    const alive = () => browser.exitCode === null && browser.signalCode === null && !spawnFailed;
    if (alive()) {
      const exited = new Promise(r => browser.once('exit', r));
      // Let Chrome stop its profile-writing children before removing the owned directory. Closing
      // the page socket and immediately SIGTERM-ing the launcher can race those final writes.
      try {
        if (cdp && cdp.socket.readyState === WebSocket.OPEN) { await cdp.send('Browser.close'); closeAcknowledged = true; }
        else if (cdp) throw Object.assign(new Error('CDP connection closed before shutdown'), { kind: 'closed', method: 'Browser.close' });
        else signalOwnedTree('SIGTERM');
      } catch (error) { politeError = error; }
      await Promise.race([exited, delay(3000)]);
      if (alive()) { politeError ||= Object.assign(new Error('owned Chromium required signal fallback'), { kind: 'owned-exit', method: 'Browser.close' }); signalOwnedTree('SIGTERM'); await Promise.race([exited, delay(2000)]); }
      if (alive()) { signalOwnedTree('SIGKILL'); await Promise.race([exited, delay(2000)]); }
      if (alive()) {
        const failure = new Error('owned Chromium did not exit before cleanup');
        if (politeError) { politeError.cleanupFailures = [{ step: 'owned-exit', error: commandDiagnostic(failure) }]; throw politeError; }
        throw failure;
      }
    }
    if (cdp && !politeError && !closeAcknowledged) politeError = Object.assign(new Error('owned Chromium close acknowledgement missing'), { kind: 'owned-exit', method: 'Browser.close' });
    if (cdp && !politeError && (browser.exitCode !== 0 || browser.signalCode !== null)) {
      politeError = Object.assign(new Error('owned Chromium did not exit normally'), { kind: 'owned-exit', method: 'Browser.close' });
    }
    try {
      cdp?.close(); browserControl?.close();
      // THE LAUNCHER IS GONE, BUT ITS HELPERS MAY NOT BE. An orphan that survived the launcher
      // (the launcher was killed, or it exited before its children did) still writes into the
      // profile, and a file that appears between the directory read and the removal leaves the
      // directory not empty. End the whole owned group and wait for it to be gone, so the removal
      // is not racing a writer that is still writing.
      signalOwnedTree('SIGKILL');
      for (let attempt = 0; attempt < 40 && ownedTreeAlive(); attempt += 1) await delay(50);
      // Delete only the unique profile created by THIS invocation, after verifying its absolute path.
      const actual = realpathSync(profile);
      requireThat(dirname(actual) === realpathSync(tmpdir()) && basename(actual).startsWith('device-ui-browser-')
        && actual === profile, 'refusing unexpected browser profile cleanup');
      // The removal retries for a bounded time: the kernel closes a dying process's handles a
      // moment after the signal, and a file written in that moment would otherwise be read as a
      // directory that is not empty.
      rmSync(actual, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 });
    } catch (error) {
      if (!politeError) throw error;
      politeError.cleanupFailures = [{ step: 'owned-profile', error: commandDiagnostic(error) }];
    }
    process.removeListener('exit', endOwnedTreeOnExit);
    if (politeError) throw politeError;
  };
  try {
    let port;
    let browserPath;
    // CHROME'S FIRST LAUNCH ON A LOADED RUNNER CAN TAKE TWENTY SECONDS OR MORE. The old
    // fifteen-second budget ended the wait while the browser was still building its caches,
    // and the browser was then reported as one that had never started. The wait is thirty
    // seconds now, and it still ends at once when the browser is gone.
    for (let i = 0; i < 300; i += 1) {
      if (spawnFailed || browser.exitCode !== null) throw new MissingBrowser('Chromium did not start');
      try { const lines = readFileSync(join(profile, 'DevToolsActivePort'), 'utf8').split('\n'); port = Number(lines[0]); browserPath = lines[1]; } catch {}
      if (port) break;
      await delay(100);
    }
    if (!port) throw new MissingBrowser('Chromium did not open its debugging port');
    requireThat(/^\/devtools\/browser\/[a-z0-9-]+$/i.test(browserPath || ''), 'invalid owned browser control endpoint');
    // THE PORT FILE IS WRITTEN BEFORE THE HTTP ENDPOINT ACCEPTS, AND THE FIRST LIST CAN COME
    // BACK WITHOUT A PAGE TARGET. Both are the browser still coming up rather than a broken
    // browser, so the list is asked for again for a while before it is treated as a failure.
    let pages = [];
    for (let i = 0; i < 20; i += 1) {
      try {
        const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(5000) });
        const listed = await response.json();
        pages = Array.isArray(listed) ? listed : [];
        if (pages.some(item => item.type === 'page')) break;
      } catch (error) {
        if (i === 19) throw error;
      }
      await delay(250);
    }
    const page = pages.find(item => item.type === 'page');
    requireThat(page?.webSocketDebuggerUrl, 'Chromium has no page target');
    const socket = new WebSocket(page.webSocketDebuggerUrl);
    await new Promise((r, reject) => {
      const timer = setTimeout(() => reject(new Error('CDP open timeout')), 5000);
      socket.addEventListener('open', () => { clearTimeout(timer); r(); }, { once: true });
      socket.addEventListener('error', () => { clearTimeout(timer); reject(new Error('CDP open failed')); }, { once: true });
    });
    cdp = new Cdp(socket);
    // Browser.close belongs to the browser connection: closing its page target can destroy the
    // page's transport before that transport delivers the close acknowledgement.
    const controlSocket = new WebSocket(`ws://127.0.0.1:${port}${browserPath}`);
    await new Promise((resolveControl, reject) => {
      const timer = setTimeout(() => { controlSocket.close(); reject(new Error('owned browser control open timeout')); }, 5000);
      controlSocket.addEventListener('open', () => { clearTimeout(timer); resolveControl(); }, { once: true });
      controlSocket.addEventListener('error', () => { clearTimeout(timer); reject(new Error('owned browser control open failed')); }, { once: true });
    });
    browserControl = new Cdp(controlSocket); cdp.browserControl = browserControl;
    await cdp.send('Page.enable'); await cdp.send('Page.setLifecycleEventsEnabled', { enabled: true });
    await cdp.send('Runtime.enable'); await cdp.send('Network.enable');
    return { cdp, stop };
  } catch (error) { await stop(); throw error; }
}

async function navigate(cdp, url) {
  if (cdp.prepareNavigation) await cdp.prepareNavigation();
  await navigateDocument(cdp, url);
}

async function navigateDocument(cdp, url) {
  const loaded = new Set();
  const remove = cdp.on('Page.lifecycleEvent', event => { if (event.name === 'load') loaded.add(event.loaderId); });
  try {
    const result = await cdp.send('Page.navigate', { url });
    requireThat(!result.errorText, 'browser navigation refused');
    if (result.loaderId) {
      const end = Date.now() + 10000;
      while (!loaded.has(result.loaderId) && Date.now() < end) await delay(50);
      requireThat(loaded.has(result.loaderId), 'new document did not finish loading');
    }
    await cdp.until(`location.href === ${JSON.stringify(url)} && document.readyState === 'complete'`, 'page did not finish loading');
    await cdp.evaluate('document.fonts.ready.then(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))))');
  } finally { remove(); }
}
async function viewport(cdp, width) {
  await cdp.send('Emulation.setDeviceMetricsOverride', { width, height: 900, deviceScaleFactor: 1, mobile: false });
}
async function key(cdp, keyName, code = keyName, virtualKey = 0) {
  const text = keyName === 'Enter' ? { text: '\r', unmodifiedText: '\r' } : {};
  await cdp.send('Input.dispatchKeyEvent', { type: 'keyDown', key: keyName, code, windowsVirtualKeyCode: virtualKey, nativeVirtualKeyCode: virtualKey, ...text });
  await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key: keyName, code, windowsVirtualKeyCode: virtualKey, nativeVirtualKeyCode: virtualKey });
}
async function tabTo(cdp, selector) {
  for (let i = 0; i < 120; i += 1) {
    await key(cdp, 'Tab', 'Tab', 9);
    if (await cdp.evaluate(`document.activeElement?.matches(${JSON.stringify(selector)})`)) return;
  }
  throw new Error('required control unreachable in keyboard order');
}
async function keyboardStart(cdp) {
  await cdp.evaluate(`(() => {document.body.setAttribute('tabindex','-1'); document.body.focus(); document.body.removeAttribute('tabindex');window.scrollTo(0,0)})()`);
}
async function click(cdp, selector) {
  const point = await cdp.evaluate(`(() => {
    const e = document.querySelector(${JSON.stringify(selector)}); if (!e) return null;
    const ownDisclosure = e.tagName === 'SUMMARY' ? e.parentElement : null;
    for (let p=e.parentElement;p;p=p.parentElement) if(p.tagName==='DETAILS' && p!==ownDisclosure) p.open=true;
    e.scrollIntoView({block:'center'}); const r=e.getBoundingClientRect();
    return r.width && r.height ? {x:r.x+r.width/2,y:r.y+r.height/2}:null;
  })()`);
  requireThat(point, 'required interactive control missing or hidden');
  await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', ...point, button: 'left', clickCount: 1 });
  await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...point, button: 'left', clickCount: 1 });
}
async function screenshot(cdp, file) {
  const layout = await cdp.send('Page.getLayoutMetrics');
  const size = layout.cssContentSize || layout.contentSize;
  requireThat(size?.width > 0 && size?.height > 0 && size.height < 100000, 'invalid full-page screenshot bounds');
  const image = await cdp.send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true,
    clip: { x: 0, y: 0, width: size.width, height: size.height, scale: 1 } });
  writeFileSync(file, Buffer.from(image.data, 'base64'));
}

async function installNetworkBoundary(cdp, baseUrl) {
  const state = { blocked: 0, errors: 0, handler: null, responseHandler: null, responseObservers: new Set(), leases: new Map(), births: new Map(), cancelled: new Set(), retiredCount: 0, visualFixture: true, phase: 'active', generation: 0, boundaries: 0 };
  const patterns = [
    { urlPattern: '*', requestStage: 'Request' },
    { urlPattern: baseUrl + '/api/terminal/*/read*', requestStage: 'Response' },
    { urlPattern: baseUrl + '/api/terminal/create', requestStage: 'Response' }
  ];
  const requireOwnedPauses = () => {
    for (const requestId of cdp.paused.keys()) requireThat(state.leases.get(requestId)?.generation === state.generation, 'unowned Fetch pause at navigation boundary');
  };
  const retireDisabledLeases = () => {
    requireOwnedPauses();
    for (const [requestId, lease] of state.leases) {
      requireThat(lease.generation === state.generation, 'unexpected Fetch lease generation');
      lease.retired = true;
      cdp.paused.delete(requestId); cdp.pauseMetadata.delete(requestId); state.leases.delete(requestId);
    }
  };
  cdp.fetchLeases = state.leases;
  cdp.on('Network.requestWillBeSent', event => {
    state.births.set(event.requestId, { epoch: cdp.pageEpoch, interaction: cdp.interaction });
    if (state.births.size > 512) state.births.delete(state.births.keys().next().value);
  });
  cdp.on('Runtime.exceptionThrown', () => { state.errors += 1; });
  cdp.on('Page.frameNavigated', event => { if (!event.frame.parentId) cdp.pageEpoch += 1; });
  cdp.on('Network.loadingFailed', event => {
    if (!event.canceled) return;
    state.cancelled.add(event.requestId);
    if (state.cancelled.size > 256) state.cancelled.delete(state.cancelled.values().next().value);
    for (const [requestId, lease] of state.leases) {
      if (lease.networkId !== event.requestId) continue;
      lease.retired = true; lease.cancelSequence = ++cdp.sequence; state.retiredCount += 1;
      if (state.leases.get(requestId) === lease) { cdp.paused.delete(requestId); cdp.pauseMetadata.delete(requestId); state.leases.delete(requestId); }
    }
  });
  cdp.on('Fetch.requestPaused', async event => {
    const responseStage = event.responseStatusCode !== undefined || event.responseErrorReason !== undefined;
    const lease = { networkId: event.networkId, responseStage, retired: false, claimed: false, generation: state.generation };
    if (event.networkId && state.cancelled.has(event.networkId)) { state.retiredCount += 1; return; }
    state.leases.set(event.requestId, lease);
    const current = () => !lease.retired && state.leases.get(event.requestId) === lease && cdp.paused.has(event.requestId);
    cdp.paused.set(event.requestId, event.responseStatusCode !== undefined || event.responseErrorReason !== undefined);
    const url = new URL(event.request.url);
    cdp.pauseMetadata.set(event.requestId, { stage: responseStage ? 'response' : 'request', kind: /\/api\/terminal\/[^/]+\/read$/.test(url.pathname) ? 'terminal-read' : url.pathname === '/api/terminal/create' ? 'terminal-create' : url.pathname === '/api/activity' ? 'activity' : url.pathname.startsWith('/api/runs/') ? 'run-detail' : 'other', epoch: cdp.pageEpoch, interaction: cdp.interaction, birth: state.births.get(event.networkId) || { epoch: 'unknown', interaction: 'unknown' }, statusPresent: event.responseStatusCode !== undefined, statusClass: event.responseStatusCode ? Math.trunc(event.responseStatusCode / 100) + 'xx' : 'none', errorReason: fetchErrorReason(event.responseErrorReason), networkIdPresent: !!event.networkId });
    // Enforce the original request-origin policy even while ordinary pauses are queued.
    if (!responseStage && !['data:', 'blob:', 'about:'].includes(url.protocol) && url.origin !== baseUrl) {
      state.blocked += 1;
      await cdp.send('Fetch.failRequest', { requestId: event.requestId, errorReason: 'BlockedByClient' }); return;
    }
    // During quiescence new pauses belong to the retiring domain and remain held until
    // Fetch.disable acknowledges their release. No command races that domain shutdown.
    if (state.phase !== 'active') {
      if (state.phase === 'disabled') retireDisabledLeases();
      return;
    }
    try {
    if (event.responseErrorReason !== undefined) {
      await cdp.send('Fetch.failRequest', { requestId: event.requestId, errorReason: 'Aborted' }); return;
    }
    if (event.responseStatusCode !== undefined) {
      for (const observer of state.responseObservers) { await observer(event); if (!current()) return; }
      if (state.responseHandler && await state.responseHandler(event)) return;
      if (!current()) return;
      await cdp.send('Fetch.continueResponse', { requestId: event.requestId }, 'auto-response'); return;
    }
    if (state.visualFixture && url.pathname === '/api/activity') {
      // SSR visual fixtures have a deliberately fixed boot/cursor; polling the live registry would
      // overwrite the fixture's state. Keep this transport fixture explicit in every matrix record.
      const next = await cdp.evaluate(`Number(document.querySelector('#feed')?.dataset.next?.split('.').at(-1)) || 0`);
      if (!current()) return;
      await fulfill(cdp, event, { rows: [], next, more: false, counts: null }); return;
    }
    if (state.visualFixture && url.pathname === '/api/terminal') {
      await fulfill(cdp, event, { sessions: [] }); return;
    }
    if (state.handler && await state.handler(event)) return;
    if (!current()) return;
    await cdp.send('Fetch.continueRequest', { requestId: event.requestId });
    } finally { if (!cdp.paused.has(event.requestId) && state.leases.get(event.requestId) === lease) state.leases.delete(event.requestId); }
  });
  await cdp.send('Fetch.enable', { patterns });
  cdp.prepareNavigation = async () => {
    requireThat(state.phase === 'active', 'Fetch navigation boundary is not active');
    state.phase = 'quiescing';
    await cdp.drainFetch();
    requireOwnedPauses();
    // Fetch is temporarily off only behind an acknowledged network-level guard. Drain
    // policy refusals again before disabling so they cannot race that domain transition.
    await cdp.send('Network.setBlockedURLs', { urls: ['http://*', 'https://*', 'ws://*', 'wss://*'] });
    // Chrome's URL block list does not cover new WebSocket handshakes. Offline
    // is scoped to this owned page and remains acknowledged until Fetch is active again.
    await cdp.send('Network.emulateNetworkConditions', { offline: true, latency: 0, downloadThroughput: -1, uploadThroughput: -1 });
    await cdp.drainFetch();
    requireOwnedPauses();
    await cdp.send('Fetch.disable');
    state.phase = 'disabled';
    retireDisabledLeases();
    // The old document exits only after its Fetch domain relinquishes ownership. Keep
    // interception off on this owned blank document, then restore the SAME patterns before
    // any target HTML or boot script can issue requests.
    await navigateDocument(cdp, 'about:blank');
    state.phase = 'arming';
    await cdp.send('Fetch.enable', { patterns });
    requireThat(!state.leases.size && !cdp.paused.size, 'unexpected Fetch pause before target document');
    await cdp.send('Network.setBlockedURLs', { urls: [] });
    await cdp.send('Network.emulateNetworkConditions', { offline: false, latency: 0, downloadThroughput: -1, uploadThroughput: -1 });
    state.generation += 1; state.boundaries += 1; state.phase = 'active';
  };
  return state;
}
async function fulfill(cdp, event, body, status = 200, headers = []) {
  await cdp.send('Fetch.fulfillRequest', { requestId: event.requestId, responseCode: status,
    responseHeaders: [{ name: 'content-type', value: 'application/json' }, ...headers],
    body: Buffer.from(typeof body === 'string' ? body : JSON.stringify(body)).toString('base64') });
}

async function snapshot(cdp, helper, options) {
  const raw = await cdp.evaluate(`(${helper.captureDeviceUiSnapshot.toString()})(${JSON.stringify(options)})`);
  const result = helper.checkDeviceUiSnapshot(raw, options);
  requireThat(Array.isArray(result?.failures) && result.metrics, 'accessibility helper contract failed');
  return result;
}

function requireAudit(checked, detail) {
  if (checked.failures.length) throw Object.assign(new Error(detail), { audit: checked });
}

// This is the existing per-boot form nonce contract. Device pages currently emit a bare inline
// copy script; requiring every script.nonce invented a different CSP contract. Never return values.
function inspectEnrollmentContract() {
  const form = document.querySelector('#enroll-form');
  const meta = document.querySelector('meta[name="summrise-panel"]');
  const fields = ['panel_token', 'console_url', 'code', 'force', 'allow_plaintext'];
  const scripts = [...document.scripts].filter(script => !script.src);
  return {
    formPresent: !!form, metaPresent: !!meta,
    fieldsPresent: !!form && fields.every(name => !!form.elements.namedItem(name)),
    methodMatches: form?.method.toLowerCase() === 'post',
    actionMatches: !!form && new URL(form.action).pathname === '/panel/enroll',
    tokenMatchesMeta: !!form?.elements.namedItem('panel_token') && !!meta && form.elements.namedItem('panel_token').value === meta.content,
    tokenShape: !!meta && /^[a-f0-9]{64}$/i.test(meta.content),
    portablyLabeled: !!document.querySelector('label[for="enroll-console"]') && !!document.querySelector('label[for="enroll-code"]'),
    inlineScriptCount: scripts.length, noncedInlineScriptCount: scripts.filter(script => !!script.nonce).length
  };
}

async function interactionDiagnostics(cdp) {
  return cdp.evaluate(`(() => {
    const active=document.activeElement, term=document.querySelector('#term-status');
    const text=term?.textContent || '';
    return {
      viewport:{width:innerWidth,height:innerHeight,scrollY,scrollHeight:document.documentElement.scrollHeight,scale:visualViewport?.scale || 1,forcedColors:matchMedia('(forced-colors: active)').matches,reducedMotion:matchMedia('(prefers-reduced-motion: reduce)').matches},
      focus:{tag:active?.tagName || '',xtermInput:!!active?.matches('.xterm-helper-textarea')},
      disclosure:{allOpen:!!document.querySelector('#all-activity')?.open,runCount:document.querySelectorAll('#feed details[data-run]').length,openRunCount:document.querySelectorAll('#feed details[data-run][open]').length,loadedRunCount:document.querySelectorAll('#feed .activity-run-body dl').length,unavailableRun:!![...document.querySelectorAll('#feed .activity-run-body')].find(body=>body.textContent.includes('unavailable'))},
      terminal:{inputPresent:!!document.querySelector('.xterm-helper-textarea'),state:text.includes('live')?'live':text.includes('closed')?'closed':text.includes('ended')?'ended':text?'other':'absent'}
    };
  })()`);
}

async function resetInteraction(cdp, network) {
  network.handler = null; network.responseHandler = null; network.responseObservers.clear();
  const failures = [];
  const attempt = async (step, action) => { try { await action(); } catch (error) { failures.push({ step, error: commandDiagnostic(error) }); } };
  await attempt('drain-fetch', () => cdp.drainFetch());
  for (const [requestId, responseStage] of [...cdp.paused]) {
    // A handler can have claimed a snapshot entry while an earlier release was awaiting its ACK.
    if (!cdp.paused.has(requestId) || cdp.paused.get(requestId) !== responseStage) continue;
    await attempt(responseStage ? 'release-response' : 'release-request', () => cdp.send(responseStage ? 'Fetch.continueResponse' : 'Fetch.failRequest',
      responseStage ? { requestId } : { requestId, errorReason: 'Aborted' }, 'reset-response'));
  }
  await attempt('drain-fetch', () => cdp.drainFetch());
  await attempt('media', () => cdp.send('Emulation.setEmulatedMedia', { features: [] }));
  await attempt('scale', () => cdp.send('Emulation.setPageScaleFactor', { pageScaleFactor: 1 }));
  await attempt('viewport', () => viewport(cdp, 1280));
  await attempt('focus', () => keyboardStart(cdp));
  if (failures.length) throw Object.assign(new Error('interaction state could not be fully reset'), { cleanupFailures: failures });
}

async function executeInteraction(cdp, network, action) {
  const row = { status: 'running' };
  try { const observations = await action(); row.status = 'pass'; if (observations) row.observations = observations; }
  catch (error) { row.status = 'fail'; row.detail = error.message; if (error.audit) row.audit = error.audit; if (error.observations) row.observations = error.observations; }
  finally {
    try { row.diagnostics = await interactionDiagnostics(cdp); } catch { row.diagnostics = { unavailable: true }; }
    try { await resetInteraction(cdp, network); }
    catch (error) { row.status = 'fail'; row.cleanupFailed = true; row.cleanupFailures = error.cleanupFailures || [{ step: 'reset', error: commandDiagnostic(error) }]; }
  }
  return row;
}

export async function runAcceptance({ baseUrl, out }, chrome = findChrome()) {
  const response = await fetch(baseUrl + '/__ui-fixtures/manifest', { redirect: 'error', signal: AbortSignal.timeout(10000) });
  requireThat(response.ok, 'Rust fixture manifest unavailable');
  const manifest = validateManifest(await response.json(), baseUrl);
  const helper = await import('./device-ui-a11y.mjs');
  requireThat(typeof helper.captureDeviceUiSnapshot === 'function' && typeof helper.checkDeviceUiSnapshot === 'function', 'accessibility helper unavailable');
  mkdirSync(out, { recursive: true });
  const runDir = mkdtempSync(join(realpathSync(out), 'device-ui-'));
  const report = { schemaVersion: 1, status: 'running', widths: [390, 1280, 1440],
    cases: [], interactions: [], limitations: ['Local isolated Rust fixture only; no external console, real devices, Windows, or serial hardware acceptance.'] };
  const browser = await openBrowser(chrome);
  const { cdp } = browser;
  let network;
  const record = async (name, controlledFixture, action) => {
    cdp.interaction = name;
    const row = { name, controlledFixture, ...await executeInteraction(cdp, network, action) };
    report.interactions.push(row);
    console.log(JSON.stringify({ interaction: name, controlledFixture, status: row.status, ...(row.detail ? { detail: row.detail } : {}) }));
  };
  try {
    network = await installNetworkBoundary(cdp, baseUrl);
    for (const width of report.widths) {
      for (const item of manifest.cases) {
        cdp.interaction = 'visual-' + item.name;
        const entry = { name: item.name, kind: item.kind, width, visualTransportFixture: true, status: 'running' };
        report.cases.push(entry);
        try {
          await viewport(cdp, width); await navigate(cdp, localUrl(item.url, baseUrl));
          requireThat(await cdp.evaluate(`[...document.querySelectorAll('details.local-mobile-menu')].every(menu => !menu.open)`), 'visual fixture mobile menu must retain its default closed state');
          const checked = await snapshot(cdp, helper, { caseName: item.name, viewportWidth: width, expectedH1Count: item.expectedH1Count,
            expectedRoute: item.kind === 'panel' ? '/panel' : '/activity' });
          entry.metrics = checked.metrics; entry.failures = checked.failures;
          const file = `${item.name}-${width}.png`;
          await screenshot(cdp, join(runDir, file)); entry.screenshot = file;
          if (checked.metrics.navigationMode === 'closed-native-menu') {
            // Preserve the default-closed image above, then qualify THIS fixture's actual
            // opened content. The live route exercise alone cannot qualify all fixture states.
            await keyboardStart(cdp);
            await key(cdp, 'Tab', 'Tab', 9);
            requireThat(await cdp.evaluate(`document.activeElement?.matches('a.device-skip')`), 'fixture skip link is not first in keyboard order');
            await key(cdp, 'Tab', 'Tab', 9);
            requireThat(await cdp.evaluate(`document.activeElement?.matches('details.local-mobile-menu > summary')`), 'fixture mobile summary is not next in natural keyboard order');
            await key(cdp, 'Enter', 'Enter', 13);
            await cdp.until(`document.querySelector('details.local-mobile-menu')?.open && document.activeElement?.matches('details.local-mobile-menu > summary')`, 'fixture native mobile menu did not open with keyboard focus retained');
            entry.navigationOpenAudit = await snapshot(cdp, helper, { viewportWidth: width, expectedH1Count: item.expectedH1Count,
              expectedRoute: item.kind === 'panel' ? '/panel' : '/activity', checkFocus: true });
            entry.failures.push(...entry.navigationOpenAudit.failures);
            requireThat(entry.navigationOpenAudit.metrics.navigationMode === 'visible', 'fixture opened navigation did not expose its actual links');
            entry.navigationOpenScreenshot = `${item.name}-${width}-menu-open.png`;
            await screenshot(cdp, join(runDir, entry.navigationOpenScreenshot));
          }
          entry.status = entry.failures.length ? 'fail' : 'pass';
        } catch (error) { entry.status = 'fail'; (entry.failures ||= []).push({ check: 'browser', detail: error.message }); }
        writeFileSync(join(runDir, `${item.name}-${width}.json`), JSON.stringify(entry, null, 2) + '\n');
        console.log(JSON.stringify({ case: item.name, width, status: entry.status, failures: entry.failures?.length || 0 }));
      }
    }
    // Representative accessibility modes are real browser emulation, measured by the same helper.
    for (const kind of ['panel', 'activity']) {
      const item = manifest.cases.find(c => c.kind === kind && c.name === (kind === 'panel' ? 'panel-normal' : 'activity-working'));
      const expectedRoute = kind === 'panel' ? '/panel' : '/activity';
      await record(kind + '-keyboard-skip-disclosure', false, async () => {
        await viewport(cdp, 390); await navigate(cdp, localUrl(item.url, baseUrl));
        // Reset focus without synthesizing a focus style; Tab itself must reveal the first link.
        await keyboardStart(cdp);
        await key(cdp, 'Tab', 'Tab', 9);
        requireThat(await cdp.evaluate(`document.activeElement?.matches('a.device-skip')`), 'skip link is not first in keyboard order');
        const checked = await snapshot(cdp, helper, { viewportWidth: 390, checkFocus: true, expectedH1Count: item.expectedH1Count, expectedRoute });
        requireAudit(checked, 'keyboard focus accessibility check failed');
        await screenshot(cdp, join(runDir, kind + '-keyboard-focus-390.png'));
        await key(cdp, 'Enter', 'Enter', 13);
        requireThat(await cdp.evaluate(`location.hash === ${JSON.stringify(kind === 'panel' ? '#content' : '#main')}`), 'skip link target failed');
        const selector = 'details > summary';
        requireThat(await cdp.evaluate(`!!document.querySelector(${JSON.stringify(selector)})`), 'keyboard disclosure missing');
        await tabTo(cdp, selector);
        // Tab can reach a later summary after the skip target. Keep the exact focused node rather
        // than measuring querySelector's first summary, which may be earlier or hidden.
        const focused = await cdp.send('Runtime.evaluate', { expression: 'document.activeElement', returnByValue: false });
        const objectId = focused.result?.objectId;
        requireThat(objectId, 'keyboard disclosure focus reference missing');
        try {
          const initial = await cdp.send('Runtime.callFunctionOn', { objectId, functionDeclaration: 'function(){return {summary:this.matches("details > summary"),focused:document.activeElement===this,open:this.parentElement?.open};}', returnByValue: true });
          requireThat(!initial.exceptionDetails && initial.result?.value?.summary && initial.result.value.focused && typeof initial.result.value.open === 'boolean', 'keyboard disclosure focus changed before activation');
          const before = initial.result.value.open;
          await key(cdp, 'Enter', 'Enter', 13);
          const changed = await cdp.send('Runtime.callFunctionOn', { objectId,
            functionDeclaration: 'function(before){return new Promise(resolve=>{const end=performance.now()+10000;const check=()=>{if(this.isConnected && document.activeElement===this && this.parentElement.open!==before) return resolve(true);if(performance.now()>=end) return resolve(false);requestAnimationFrame(check);};check();});}',
            arguments: [{ value: before }], awaitPromise: true, returnByValue: true });
          requireThat(!changed.exceptionDetails && changed.result?.value === true, 'keyboard disclosure did not toggle');
        } finally { await cdp.send('Runtime.releaseObject', { objectId }); }
      });
      await record(kind + '-forced-colors-reduced-motion-zoom', false, async () => {
        await cdp.send('Emulation.setEmulatedMedia', { features: [{ name: 'forced-colors', value: 'active' }, { name: 'prefers-reduced-motion', value: 'reduce' }] });
        await viewport(cdp, 390); await navigate(cdp, localUrl(item.url, baseUrl));
        await keyboardStart(cdp);
        await key(cdp, 'Tab', 'Tab', 9);
        let checked = await snapshot(cdp, helper, { viewportWidth: 390, forcedColors: true, reducedMotion: true, checkFocus: true, expectedRoute });
        requireAudit(checked, 'forced colors or reduced motion check failed');
        await screenshot(cdp, join(runDir, kind + '-forced-colors-390.png'));
        await cdp.send('Emulation.setEmulatedMedia', { features: [] });
        // A 1280 CSS-pixel window at 200% browser zoom has a 640 CSS-pixel layout viewport.
        // Narrowing the layout viewport tests reflow without mislabelling pinch scale as page zoom.
        await viewport(cdp, 640); await navigate(cdp, localUrl(item.url, baseUrl));
        checked = await snapshot(cdp, helper, { viewportWidth: 640, expectedRoute });
        requireAudit(checked, '200-percent equivalent layout reflow failed');
        await screenshot(cdp, join(runDir, kind + '-zoom-layout-640.png'));
        await viewport(cdp, 1280);
        await cdp.send('Emulation.setPageScaleFactor', { pageScaleFactor: 2 });
        try {
          checked = await snapshot(cdp, helper, { viewportWidth: 1280, zoom: 2, expectedRoute });
          requireAudit(checked, '200-percent visual zoom check failed');
          await screenshot(cdp, join(runDir, kind + '-zoom-visual-200.png'));
        } finally { await cdp.send('Emulation.setPageScaleFactor', { pageScaleFactor: 1 }); }
      });
    }
    network.visualFixture = false;
    await liveInteractions({ cdp, baseUrl, manifest, record, network, runDir, helper });
    if (network.blocked) report.interactions.push({ name: 'same-origin-network-boundary', status: 'fail', controlledFixture: false, count: network.blocked });
    if (network.errors || cdp.eventFailure || cdp.commandFailures.length) report.interactions.push({ name: 'browser-script-errors', status: 'fail', controlledFixture: false, count: network.errors, infrastructure: { events: cdp.eventErrors, commands: cdp.commandFailures } });
    report.status = report.cases.every(c => c.status === 'pass') && report.interactions.every(c => c.status === 'pass') ? 'pass' : 'fail';
  } finally {
    await browser.stop();
    if (report.status === 'running') report.status = 'fail';
    writeFileSync(join(runDir, 'results.json'), JSON.stringify(report, null, 2) + '\n');
  }
  console.log(JSON.stringify({ status: report.status, cases: report.cases.length, interactions: report.interactions.length, evidence: runDir }));
  return report.status === 'pass' ? 0 : 1;
}

async function responseJson(cdp, event) {
  const result = await cdp.send('Fetch.getResponseBody', { requestId: event.requestId }, 'response-body');
  return JSON.parse(result.base64Encoded ? Buffer.from(result.body, 'base64').toString() : result.body);
}
async function newPty(cdp, network) {
  let id = null;
  const observer = async event => {
    if (new URL(event.request.url).pathname !== '/api/terminal/create') return;
    const answer = await responseJson(cdp, event);
    if (answer.kind === 'pty' && typeof answer.id === 'string') id = answer.id;
  };
  network.responseObservers.add(observer);
  try {
    await click(cdp, '#term-pty');
    const end = Date.now() + 10000;
    while (!id && Date.now() < end) await delay(50);
    requireThat(id, 'actual PTY create response missing');
    await cdp.until(`document.querySelector('#term-status')?.textContent.includes(${JSON.stringify(id)}) && document.querySelector('#term-status')?.textContent.includes('live') && !!document.querySelector('.xterm-helper-textarea')`, 'created PTY id did not become active');
    return id;
  } finally { network.responseObservers.delete(observer); }
}
async function terminalInput(cdp, text) {
  await click(cdp, '#term-host');
  // xterm's hidden textarea is its public keyboard input surface; focus it explicitly and verify
  // before dispatching real CDP text/Enter. Clicking the canvas midpoint alone can hit padding.
  await cdp.evaluate(`document.querySelector('.xterm-helper-textarea')?.focus()`);
  requireThat(await cdp.evaluate(`document.activeElement?.matches('.xterm-helper-textarea')`), 'terminal keyboard input did not receive focus');
  await cdp.send('Input.insertText', { text }); await key(cdp, 'Enter', 'Enter', 13);
}

async function mobileNavigation({ cdp, baseUrl, helper, runDir }) {
  const menuSelector = 'details.local-mobile-menu';
  const summarySelector = menuSelector + ' > summary';
  const navSelector = menuSelector + ' > nav.local-nav';
  const openWithKeyboard = async expectedRoute => {
    const closed = await snapshot(cdp, helper, { viewportWidth: 390, expectedRoute });
    requireAudit(closed, 'closed mobile navigation accessibility check failed');
    requireThat(closed.metrics.navigationMode === 'closed-native-menu', 'new mobile document did not start with its native menu closed');
    await keyboardStart(cdp);
    await key(cdp, 'Tab', 'Tab', 9);
    requireThat(await cdp.evaluate(`document.activeElement?.matches('a.device-skip')`), 'mobile skip link is not first in keyboard order');
    await key(cdp, 'Tab', 'Tab', 9);
    requireThat(await cdp.evaluate(`document.activeElement?.matches(${JSON.stringify(summarySelector)})`), 'named mobile menu summary is not next in natural keyboard order');
    await key(cdp, 'Enter', 'Enter', 13);
    await cdp.until(`document.querySelector(${JSON.stringify(menuSelector)})?.open && document.activeElement === document.querySelector(${JSON.stringify(summarySelector)})`, 'native mobile menu did not open while retaining keyboard focus');
    const opened = await snapshot(cdp, helper, { viewportWidth: 390, expectedRoute, checkFocus: true });
    requireAudit(opened, 'opened mobile navigation accessibility check failed');
    requireThat(opened.metrics.navigationMode === 'visible', 'opened mobile menu did not expose its actual route links');
  };
  await viewport(cdp, 390);
  for (const [start, destination] of [['/panel', '/activity'], ['/activity', '/panel']]) {
    await navigate(cdp, localUrl(start, baseUrl));
    await openWithKeyboard(start);
    await screenshot(cdp, join(runDir, 'mobile-navigation-' + start.slice(1) + '-390.png'));
    for (const path of ['/panel', '/activity']) {
      await key(cdp, 'Tab', 'Tab', 9);
      requireThat(await cdp.evaluate(`document.activeElement?.matches(${JSON.stringify(navSelector + ' > a[href="' + path + '"]')})`), 'mobile route links are not in their natural keyboard order');
    }
    if (destination === '/panel') {
      // Return to the preceding real link with Shift+Tab, without assigning focus.
      for (const type of ['keyDown', 'keyUp']) await cdp.send('Input.dispatchKeyEvent', {
        type, key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9, nativeVirtualKeyCode: 9, modifiers: 8 });
    }
    requireThat(await cdp.evaluate(`document.activeElement?.matches(${JSON.stringify(navSelector + ' > a[href="' + destination + '"]')})`), 'mobile destination link did not retain keyboard focus');
    requireAudit(await snapshot(cdp, helper, { viewportWidth: 390, expectedRoute: start, checkFocus: true }), 'mobile route keyboard focus accessibility check failed');
    await key(cdp, 'Enter', 'Enter', 13);
    await cdp.until(`location.pathname === ${JSON.stringify(destination)} && !location.search && !location.hash && document.readyState === 'complete'`, 'mobile keyboard route navigation failed');
    // Verify the new document's current route after opening its own default-closed menu.
    await openWithKeyboard(destination);
  }
  await viewport(cdp, 1280);
  return { nativeMenuKeyboardOpened: true, naturalRouteOrder: true, keyboardRouteCrossings: 2,
    destinationCurrentRoutesVerified: true, defaultClosedDocumentsVerified: true };
}

async function liveInteractions({ cdp, baseUrl, manifest, record, network, runDir, helper }) {
  const live = manifest.live;
  await viewport(cdp, 1280);
  await record('root-panel-and-navigation', false, async () => {
    for (const url of [live.root, live.panel]) {
      await navigate(cdp, localUrl(url, baseUrl));
      requireThat(await cdp.evaluate(`!!document.querySelector('#enroll-form') && !!document.querySelector('#term-pty') && !!document.querySelector('form[action="/api/panel/update"] input[name="panel_token"]')`), 'root/panel actions missing');
    }
    await click(cdp, 'aside.local-sidebar > nav.local-nav > a[href="/activity"]');
    await cdp.until(`location.pathname === '/activity' && document.readyState === 'complete'`, 'Activity navigation failed');
    requireAudit(await snapshot(cdp, helper, { viewportWidth: 1280, expectedRoute: '/activity' }), 'desktop Activity navigation accessibility check failed');
    await click(cdp, 'aside.local-sidebar > nav.local-nav > a[href="/panel"]');
    await cdp.until(`location.pathname === '/panel' && document.readyState === 'complete'`, 'Panel navigation failed');
    requireAudit(await snapshot(cdp, helper, { viewportWidth: 1280, expectedRoute: '/panel' }), 'desktop Panel navigation accessibility check failed');
    return { mobileNavigation: await mobileNavigation({ cdp, baseUrl, helper, runDir }) };
  });
  for (const denied of [false, true]) {
    await record('clipboard-' + (denied ? 'denied' : 'accepted'), true, async () => {
      await navigate(cdp, localUrl(live.panel, baseUrl));
      await cdp.evaluate(`(() => {
        Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText: async text => { if (${denied}) throw new DOMException('denied','NotAllowedError'); window.__uiCopiedLength=text.length; }}});
        document.execCommand = () => false;
      })()`);
      for (const selector of ['[data-copy="local-config"]', '[data-copy="diagnostics"]']) {
        await click(cdp, selector);
        await cdp.until(`document.querySelector(${JSON.stringify(selector)}).textContent.includes(${JSON.stringify(denied ? 'Copy refused' : 'Copied')})`, 'clipboard feedback missing');
        requireThat(await cdp.evaluate(denied
          ? `(() => {const b=document.querySelector(${JSON.stringify(selector)}),s=document.getElementById(b.dataset.copy);return !s.hidden && getSelection().toString().length>0})()`
          : 'window.__uiCopiedLength > 0'), 'clipboard result or manual fallback missing');
      }
    });
  }
  await record('enrollment-fields-nonce-and-real-local-refusal', false, async () => {
    await navigate(cdp, localUrl(live.panel, baseUrl));
    const contract = await cdp.evaluate(`(${inspectEnrollmentContract.toString()})()`);
    requireThat(['formPresent','metaPresent','fieldsPresent','methodMatches','actionMatches','tokenMatchesMeta','tokenShape','portablyLabeled'].every(key => contract[key]), 'enrollment form fields/per-boot nonce contract changed');
    let submitted = false;
    network.handler = async event => {
      if (new URL(event.request.url).pathname !== '/panel/enroll') return false;
      const fields = new URLSearchParams(event.request.postData || '');
      submitted = event.request.method === 'POST' && !!fields.get('panel_token') && fields.has('console_url') && fields.has('code');
      return false;
    };
    try {
      await cdp.evaluate(`document.querySelector('#enroll-console').value='not-a-console-url'; document.querySelector('#enroll-code').value='controlled-browser-fixture'`);
      await click(cdp, '#enroll-submit');
      await cdp.until(`document.querySelector('#enroll-status')?.textContent.toLowerCase().includes('failed')`, 'enrollment refusal banner missing');
      requireThat(submitted, 'enrollment did not submit original token and required fields');
    } finally { network.handler = null; }
    return { enrollment: contract };
  });
  await record('actual-update-request-with-original-token-and-no-source-refusal', false, async () => {
    await navigate(cdp, localUrl(live.panel, baseUrl));
    let saw = false;
    const expected = await cdp.evaluate(`document.querySelector('meta[name="summrise-panel"]').content`);
    network.handler = async event => {
      if (new URL(event.request.url).pathname === '/api/panel/update') {
        saw = event.request.method === 'POST' && new URLSearchParams(event.request.postData || '').get('panel_token') === expected;
      }
      return false;
    };
    try {
      await click(cdp, '#update-now');
      await cdp.until(`document.body.textContent.includes('no update source is configured on this device')`, 'actual update refusal missing');
      requireThat(saw, 'actual update POST did not preserve the original token');
    } finally { network.handler = null; }
  });
  await record('update-post-keeps-token-and-controlled-refusal', true, async () => {
    await navigate(cdp, localUrl(live.panel, baseUrl));
    let saw = false;
    network.handler = async event => {
      if (new URL(event.request.url).pathname !== '/api/panel/update') return false;
      const form = new URLSearchParams(event.request.postData || '');
      saw = event.request.method === 'POST' && !!form.get('panel_token');
      await fulfill(cdp, event, { error: 'controlled update refusal' }, 403); return true;
    };
    try {
      await click(cdp, 'form[action="/api/panel/update"] button[type="submit"]');
      const end = Date.now() + 5000; while (!saw && Date.now() < end) await delay(50);
      requireThat(saw, 'update POST token missing');
      await cdp.until(`document.body.textContent.includes('controlled update refusal')`, 'update refusal response missing');
    } finally { network.handler = null; }
  });
  await record('tokenless-mutations-refused', false, async () => {
    await navigate(cdp, localUrl(live.panel, baseUrl));
    const statuses = await cdp.evaluate(`Promise.all([
      fetch('/api/terminal/create',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({kind:'pty'})}),
      fetch('/api/panel/update',{method:'POST',headers:{'content-type':'application/x-www-form-urlencoded'},body:''}),
      fetch('/panel/enroll',{method:'POST',headers:{'content-type':'application/x-www-form-urlencoded'},body:'console_url=invalid&code=fixture'})
    ].map(p=>p.then(r=>r.status)))`);
    requireThat(statuses.every(status => status === 401 || status === 403), 'tokenless action was not refused');
  });
  // Read seeded live session/receipt history before New pty closes an auto-attached session.
  await activityInteractions({ cdp, baseUrl, live, record, network, runDir });
  await record('real-local-pty-marker-input-read-close', false, async () => {
    await navigate(cdp, localUrl(live.panel, baseUrl));
    const id = await newPty(cdp, network);
    const marker = 'DEVICE_UI_' + randomUUID().replaceAll('-', '');
    const half = marker.length >> 1;
    let received = ''; let sawRead = false; let readCount = 0; let writeSeen = false;
    const observer = async event => {
      if (new URL(event.request.url).pathname !== '/api/terminal/' + id + '/read') return;
      const answer = await responseJson(cdp, event); readCount += 1;
      received += answer.data_base64 ? Buffer.from(answer.data_base64, 'base64').toString() : answer.text || '';
      received = received.slice(-65536); sawRead ||= received.includes(marker);
    };
    network.responseObservers.add(observer);
    network.handler = async event => {
      if (new URL(event.request.url).pathname === '/api/terminal/' + id + '/write') writeSeen = true;
      return false;
    };
    try {
      await terminalInput(cdp, `printf '%s%s\\n' '${marker.slice(0,half)}' '${marker.slice(half)}'`);
      const end = Date.now() + 10000; while (!sawRead && Date.now() < end) await delay(60);
      if (!writeSeen || !sawRead) throw Object.assign(new Error('PTY marker was not written and read from actual terminal output'), { observations: { terminal: { createdIdMatched: true, writeSeen, readCount, markerRead: sawRead } } });
      await screenshot(cdp, join(runDir, 'live-pty-1280.png'));
      await click(cdp, '#term-close');
      await cdp.until(`document.querySelector('#term-status')?.textContent.includes('closed') && !document.querySelector('#term-status')?.textContent.includes('live')`, 'PTY close status missing');
      return { terminal: { createdIdMatched: true, writeSeen, readCount, markerRead: sawRead } };
    } finally { network.responseObservers.delete(observer); network.handler = null; received = ''; }
  });
  await record('real-terminal-ended-read-retired-after-close-and-new-session', true, async () => {
    await navigate(cdp, localUrl(live.panel, baseUrl));
    const oldId = await newPty(cdp, network);
    let held = null; let retiredCursor = null;
    network.responseHandler = async event => {
      if (held || new URL(event.request.url).pathname !== '/api/terminal/' + oldId + '/read') return false;
      const result = await cdp.send('Fetch.getResponseBody', { requestId: event.requestId }, 'response-body');
      const answer = JSON.parse(result.base64Encoded ? Buffer.from(result.body, 'base64').toString() : result.body);
      if (answer.alive !== false) return false;
      held = event; retiredCursor = answer.cursor; return true;
    };
    let removeRequest = () => {};
    try {
      await terminalInput(cdp, 'exit');
      const end = Date.now() + 10000; while (!held && Date.now() < end) await delay(50);
      requireThat(held && Number.isFinite(retiredCursor), 'real ended terminal read was not captured');
      const retiredPath = new URL(held.request.url).pathname;
      await click(cdp, '#term-close');
      const newId = await newPty(cdp, network);
      requireThat(newId !== oldId, 'replacement PTY did not create a distinct session');
      const status = await cdp.evaluate(`document.querySelector('#term-status').textContent`);
      const cursors = [];
      removeRequest = cdp.on('Network.requestWillBeSent', event => {
        const url = new URL(event.request.url);
        if (url.pathname.match(/^\/api\/terminal\/[^/]+\/read$/) && url.pathname !== retiredPath) cursors.push(Number(url.searchParams.get('cursor')));
      });
      // Release the unchanged real end response only after Close and New pty both completed.
      await cdp.send('Fetch.continueResponse', { requestId: held.requestId }, 'held-response'); held = null;
      await terminalInput(cdp, "printf 'new-session-after-retired-read\\n'");
      const next = Date.now() + 5000; while (!cursors.length && Date.now() < next) await delay(50);
      requireThat(cursors.length > 0 && cursors.every(cursor => cursor !== retiredCursor), 'retired terminal cursor replaced the new cursor');
      requireThat(await cdp.evaluate(`document.querySelector('#term-status').textContent === ${JSON.stringify(status)}`), 'retired end response replaced the active session status');

      // The original delayed-EOF check above keeps its controlled transport. These next
      // checks let unchanged real EOF reach the UI before any New or Close action.
      network.responseHandler = null; removeRequest(); removeRequest = () => {};
      const registryExpression = id => `(async () => {
        const response = await fetch('/api/terminal', {cache:'no-store',headers:{'x-summrise-panel':document.querySelector('meta[name="summrise-panel"]').content}});
        if (!response.ok) throw new Error('terminal registry read refused');
        const body = await response.json();
        if (!Array.isArray(body.sessions)) throw new Error('terminal registry rows missing');
        return body.sessions.some(row => row.id === ${JSON.stringify(id)});
      })()`;
      const registryContains = id => cdp.evaluate(registryExpression(id));
      const deliveredNaturalEnd = async id => {
        let servedEof = false;
        const observer = async event => {
          if (new URL(event.request.url).pathname !== '/api/terminal/' + id + '/read') return;
          const answer = await responseJson(cdp, event);
          if (answer.alive === false && typeof answer.text === 'string' && Number.isFinite(answer.cursor)) servedEof = true;
        };
        network.responseObservers.add(observer);
        try {
          await terminalInput(cdp, 'exit');
          await cdp.until(`document.querySelector('#term-status')?.textContent.includes('session ended') && !document.querySelector('#term-status')?.textContent.includes('live')`, 'natural terminal end was not visible before cleanup');
          requireThat(servedEof, 'visible end was not backed by the owned real EOF response');
        } finally { network.responseObservers.delete(observer); }
      };

      await deliveredNaturalEnd(newId);
      requireThat(await registryContains(newId), 'naturally ended owned terminal was not retained before New');
      let endedClosePosted = false; let endedCloseAcknowledged = false; let removedBeforeCreate = false;
      const endedClosePath = '/api/terminal/' + newId + '/close';
      const removeCloseResponse = cdp.on('Network.responseReceived', event => {
        if (new URL(event.response.url).pathname === endedClosePath && event.response.status === 200) endedCloseAcknowledged = true;
      });
      network.handler = async event => {
        const path = new URL(event.request.url).pathname;
        if (path === endedClosePath && event.request.method === 'POST') endedClosePosted = true;
        if (path === '/api/terminal/create') {
          requireThat(endedClosePosted && endedCloseAcknowledged, 'New did not finish closing the exact ended terminal before create');
          requireThat(!await registryContains(newId), 'New reached create before removing the ended terminal registry row');
          removedBeforeCreate = true;
        }
        return false;
      };
      let replacementId;
      try { replacementId = await newPty(cdp, network); }
      finally { network.handler = null; removeCloseResponse(); }
      requireThat(replacementId !== newId && removedBeforeCreate, 'natural EOF replacement did not recycle the ended terminal first');
      requireThat(!await registryContains(newId) && await registryContains(replacementId), 'New did not replace exactly the owned ended registry row');

      await deliveredNaturalEnd(replacementId);
      requireThat(await registryContains(replacementId), 'naturally ended owned terminal was not retained before Close');
      let explicitClosePosted = false;
      network.handler = async event => {
        if (new URL(event.request.url).pathname === '/api/terminal/' + replacementId + '/close' && event.request.method === 'POST') explicitClosePosted = true;
        return false;
      };
      try {
        await click(cdp, '#term-close');
        await cdp.until(`document.querySelector('#term-status')?.textContent.includes('closed') && !document.querySelector('#term-status')?.textContent.includes('live')`, 'ended terminal Close status missing');
        await cdp.until(`(${registryExpression(replacementId)}).then(present => !present)`, 'Close did not remove the exact naturally ended terminal registry row');
        requireThat(explicitClosePosted, 'Close did not post the exact naturally ended terminal');
      } finally { network.handler = null; }

      // Hold only this new document's actual initial list, while New creates its own view.
      const bootstrapOwned = []; const bootstrapDisposers = [];
      let heldBootstrap = null; let bootstrapRequestId = null; let bootstrapNetworkId = null;
      let bootstrapFinished = false; let bootstrapReleased = false; let attachA = null; let attachB = null;
      let bReceived = ''; let bCursor = null; let bMarkerWrite = false; let aWrite = false; let aReadAfterRelease = false;
      const bReadCursors = [];
      const beforeMarker = 'DEVICE_UI_BEFORE_' + randomUUID().replaceAll('-', '');
      const afterMarker = 'DEVICE_UI_AFTER_' + randomUUID().replaceAll('-', '');
      const halves = marker => [marker.slice(0, marker.length >> 1), marker.slice(marker.length >> 1)];
      const afterHalves = halves(afterMarker);
      const printMarker = (marker, quiet = false) => {
        const parts = halves(marker);
        return (quiet ? 'PS1=; ' : '') + "printf '%s%s\\n' '" + parts[0] + "' '" + parts[1] + "'";
      };
      const waitForFact = async (test, detail) => {
        const end = Date.now() + 10000;
        while (!test() && Date.now() < end) await delay(50);
        requireThat(test(), detail);
      };
      const activeOwnedIndex = ids => `${JSON.stringify(ids.map(id => 'pty ' + id + ' · live'))}.indexOf(document.querySelector('#term-status')?.textContent)`;
      const textWitness = `(async () => {
        const rows = document.querySelector('#term-host .xterm-rows');
        const canvas = document.querySelector('#term-host .xterm-text-layer') || document.querySelector('#term-host canvas');
        const data = rows ? new TextEncoder().encode(rows.textContent) : canvas?.getContext('2d')?.getImageData(0,0,canvas.width,canvas.height).data;
        if (!data?.length) return null;
        const hash = new Uint8Array(await crypto.subtle.digest('SHA-256', data));
        return (rows ? 'dom:' : 'canvas:' + canvas.width + ':' + canvas.height + ':') + Array.from(hash, byte => byte.toString(16).padStart(2,'0')).join('');
      })()`;
      const renderSettled = () => cdp.evaluate('document.fonts.ready.then(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))))');
      const observeB = async event => {
        if (!attachB || new URL(event.request.url).pathname !== '/api/terminal/' + attachB + '/read') return;
        const answer = await responseJson(cdp, event);
        bReceived = (bReceived + (answer.data_base64 ? Buffer.from(answer.data_base64, 'base64').toString('utf8') : answer.text || '')).slice(-65536);
        if (Number.isFinite(answer.cursor)) bCursor = answer.cursor;
      };
      const removeBootstrapObservers = () => {
        network.handler = null; network.responseHandler = null; network.responseObservers.delete(observeB);
        for (const dispose of bootstrapDisposers.splice(0)) dispose();
      };
      try {
        attachA = await newPty(cdp, network); bootstrapOwned.push(attachA);
        bootstrapDisposers.push(cdp.on('Network.requestWillBeSent', event => {
          if (!bootstrapNetworkId && event.request.method === 'GET' && new URL(event.request.url).pathname === '/api/terminal') bootstrapNetworkId = event.requestId;
        }));
        bootstrapDisposers.push(cdp.on('Network.loadingFinished', event => {
          if (event.requestId === bootstrapNetworkId) bootstrapFinished = true;
        }));
        network.handler = async event => {
          const url = new URL(event.request.url);
          if (!bootstrapRequestId && event.request.method === 'GET' && url.pathname === '/api/terminal') {
            bootstrapRequestId = event.requestId;
            await cdp.send('Fetch.continueRequest', { requestId: event.requestId, interceptResponse: true }, 'bootstrap-list-request');
            return true;
          }
          if (url.pathname === '/api/terminal/' + attachA + '/write') aWrite = true;
          if (bootstrapReleased && url.pathname === '/api/terminal/' + attachA + '/read') aReadAfterRelease = true;
          if (attachB && url.pathname === '/api/terminal/' + attachB + '/write' && event.request.method === 'POST') {
            const data = JSON.parse(event.request.postData || '{}').data;
            if (typeof data === 'string' && afterHalves.every(part => data.includes(part))) bMarkerWrite = true;
          }
          if (bootstrapReleased && attachB && url.pathname === '/api/terminal/' + attachB + '/read') bReadCursors.push(Number(url.searchParams.get('cursor')));
          return false;
        };
        network.responseHandler = async event => {
          if (event.requestId !== bootstrapRequestId) return false;
          requireThat(event.responseStatusCode === 200, 'actual initial terminal list was refused');
          const answer = await responseJson(cdp, event);
          requireThat(Array.isArray(answer.sessions) && answer.sessions[0]?.id === attachA, 'initial real list did not select the exact owned terminal');
          heldBootstrap = event; return true;
        };
        await navigate(cdp, localUrl(live.panel, baseUrl));
        await waitForFact(() => !!heldBootstrap && !!bootstrapNetworkId, 'owned initial real terminal list response was not held');
        attachB = await newPty(cdp, network); bootstrapOwned.push(attachB);
        requireThat(attachB !== attachA && !bootstrapReleased && !!heldBootstrap, 'New did not become live before the initial list was released');
        network.responseObservers.add(observeB);
        await terminalInput(cdp, printMarker(beforeMarker, true));
        await waitForFact(() => bReceived.includes(beforeMarker) && Number.isFinite(bCursor), 'replacement primer was not read from the actual owned terminal');
        await renderSettled();
        const witnessBefore = await cdp.evaluate(textWitness);
        requireThat(witnessBefore, 'actual terminal text renderer could not be observed');
        const cursorBefore = bCursor;
        const statusBefore = await cdp.evaluate(`document.querySelector('#term-status').textContent`);
        requireThat(await cdp.evaluate(`(${activeOwnedIndex([attachB])}) === 0`), 'replacement was not active before the late list');
        bootstrapReleased = true;
        await cdp.send('Fetch.continueResponse', { requestId: heldBootstrap.requestId }, 'bootstrap-list-response'); heldBootstrap = null;
        network.responseHandler = null;
        await waitForFact(() => bootstrapFinished, 'real initial list response did not finish after release');
        await renderSettled();
        requireThat(await cdp.evaluate(textWitness) === witnessBefore, 'late initial list reset the active terminal renderer');
        await terminalInput(cdp, printMarker(afterMarker));
        await waitForFact(() => bMarkerWrite && bReceived.includes(afterMarker) && bReadCursors.length > 0, 'post-list marker did not write and read through the replacement terminal');
        requireThat(bReadCursors.every(cursor => Number.isFinite(cursor) && cursor >= cursorBefore) && bCursor > cursorBefore, 'late initial list reset or replaced the active read cursor');
        requireThat(!aWrite && !aReadAfterRelease, 'late initial list redirected active terminal IO to the older terminal');
        requireThat(await cdp.evaluate(`document.querySelector('#term-status').textContent === ${JSON.stringify(statusBefore)}`), 'late initial list replaced the active terminal status');
        removeBootstrapObservers();
        await click(cdp, '#term-close');
        await cdp.until(`(${registryExpression(attachB)}).then(present => !present)`, 'UI Close did not remove the owned bootstrap replacement');
        requireThat(await registryContains(attachA), 'held-list original disappeared before owned cleanup');
        await navigate(cdp, localUrl(live.panel, baseUrl));
        await cdp.until(`(${activeOwnedIndex([attachA])}) === 0`, 'fresh panel did not attach the exact owned original for cleanup');
        await click(cdp, '#term-close');
        await cdp.until(`(${registryExpression(attachA)}).then(present => !present)`, 'UI Close did not remove the owned bootstrap original');
      } finally {
        if (heldBootstrap) await cdp.send('Fetch.continueResponse', { requestId: heldBootstrap.requestId }, 'bootstrap-list-cleanup');
        removeBootstrapObservers();
        // On failure, only these captured own IDs may be closed, after the assertions.
        for (let count = 0; count < bootstrapOwned.length; count += 1) {
          const remaining = [];
          for (const id of bootstrapOwned) if (await registryContains(id)) remaining.push(id);
          if (!remaining.length) break;
          let index = await cdp.evaluate(activeOwnedIndex(remaining));
          if (index < 0) {
            await navigate(cdp, localUrl(live.panel, baseUrl));
            await cdp.until(`(${activeOwnedIndex(remaining)}) >= 0`, 'owned bootstrap cleanup target did not attach');
            index = await cdp.evaluate(activeOwnedIndex(remaining));
          }
          requireThat(index >= 0, 'refusing to close an unowned bootstrap view');
          await click(cdp, '#term-close');
          await cdp.until(`(${registryExpression(remaining[index])}).then(present => !present)`, 'owned bootstrap UI cleanup did not remove its row');
        }
        for (const id of bootstrapOwned) requireThat(!await registryContains(id), 'owned bootstrap session remained after UI cleanup');
        bReceived = '';
      }
      // Refuse one exact owned close before server dispatch. The registry must retain A;
      // only an explicit retry may release it and permit replacement B.
      const releaseOwned = [];
      let releaseA = null; let releaseB = null; let refusedRelease = false;
      let retryAcknowledged = false; let retryRemovedBeforeCreate = false; let unexpectedCreate = false;
      let failurePhase = true; let replacementMarker = false; let replacementWrite = false;
      let markerBytes = '';
      const releaseMarker = 'DEVICE_UI_RELEASE_' + randomUUID().replaceAll('-', '');
      const releaseHalves = [releaseMarker.slice(0, releaseMarker.length >> 1), releaseMarker.slice(releaseMarker.length >> 1)];
      const releaseObserver = async event => {
        const path = new URL(event.request.url).pathname;
        if (releaseA && path === '/api/terminal/' + releaseA + '/close') {
          const answer = await responseJson(cdp, event);
          requireThat(event.responseStatusCode === 200 && answer.closed === true, 'owned release retry was not confirmed');
          retryAcknowledged = true;
        }
        if (releaseB && path === '/api/terminal/' + releaseB + '/read') {
          const answer = await responseJson(cdp, event);
          markerBytes = (markerBytes + (answer.data_base64 ? Buffer.from(answer.data_base64, 'base64').toString('utf8') : answer.text || '')).slice(-65536);
          replacementMarker ||= markerBytes.includes(releaseMarker);
        }
      };
      try {
        releaseA = await newPty(cdp, network); releaseOwned.push(releaseA);
        network.responseObservers.add(releaseObserver);
        network.handler = async event => {
          const path = new URL(event.request.url).pathname;
          if (path === '/api/terminal/' + releaseA + '/close') {
            if (!refusedRelease) {
              refusedRelease = true;
              await fulfill(cdp, event, { error: 'audit_unwritable', detail: "the device's event log could not be written, so the operation was not run" }, 500);
              return true;
            }
            await cdp.send('Fetch.continueRequest', { requestId: event.requestId, interceptResponse: true }, 'release-retry-request');
            return true;
          }
          if (path === '/api/terminal/create') {
            unexpectedCreate ||= failurePhase;
            requireThat(!failurePhase && retryAcknowledged, 'replacement was requested before confirmed release retry');
            requireThat(!await registryContains(releaseA), 'replacement was requested while failed-release row remained');
            retryRemovedBeforeCreate = true;
          }
          if (releaseB && path === '/api/terminal/' + releaseB + '/write') {
            const data = JSON.parse(event.request.postData || '{}').data;
            replacementWrite ||= typeof data === 'string' && releaseHalves.every(part => data.includes(part));
          }
          return false;
        };
        await click(cdp, '#term-pty');
        await cdp.until(`/Close failed.*retry/i.test(document.querySelector('#term-status')?.textContent || '')`, 'release failure was not visible');
        requireThat(refusedRelease && !unexpectedCreate && await registryContains(releaseA), 'failed release did not retain the exact owned row and block New');
        requireThat(await cdp.evaluate(`!document.querySelector('#term-status').textContent.includes('session closed') && !document.querySelector('#term-status').textContent.includes('live')`), 'failed release falsely reported a closed or live session');
        failurePhase = false;
        releaseB = await newPty(cdp, network); releaseOwned.push(releaseB);
        requireThat(releaseB !== releaseA && retryAcknowledged && retryRemovedBeforeCreate && !unexpectedCreate, 'explicit retry did not clean the old owner before replacement');
        await terminalInput(cdp, "printf '%s%s\\n' '" + releaseHalves[0] + "' '" + releaseHalves[1] + "'");
        const markerEnd = Date.now() + 10000;
        while ((!replacementMarker || !replacementWrite) && Date.now() < markerEnd) await delay(50);
        requireThat(replacementMarker && replacementWrite, 'post-release replacement marker did not write and read through the owned PTY');
        await click(cdp, '#term-close');
        await cdp.until(`(${registryExpression(releaseB)}).then(present => !present)`, 'replacement release fixture row remained after UI Close');
      } finally {
        network.handler = null; network.responseObservers.delete(releaseObserver);
        // Only this subcase's captured IDs can require cleanup; UI retains failed-release ownership.
        if (releaseOwned.length) {
          await click(cdp, '#term-close');
          for (const id of releaseOwned) await cdp.until(`(${registryExpression(id)}).then(present => !present)`, 'owned failed-release fixture row remained after UI cleanup');
        }
        markerBytes = '';
      }
      return { release: { controlledCloseRefusedBeforeDispatch: refusedRelease, failedOwnedRowRetained: true,
        failureVisibleWithoutFalseClosed: true, replacementBlockedOnFailure: !unexpectedCreate,
        explicitRetryAcknowledged: retryAcknowledged, failedRowRemovedBeforeCreate: retryRemovedBeforeCreate,
        replacementMarkerWriteRead: replacementMarker && replacementWrite, ownedRowsRemovedByUIClose: true },
        terminal: { heldEofCursorIsolated: true, heldEofStatusIsolated: true,
        actualEofDeliveredBeforeNew: true, endedRowPresentBeforeNew: true, endedCloseAcknowledgedBeforeCreate: true,
        endedRowRemovedBeforeCreate: true, distinctReplacementLive: true, actualEofDeliveredBeforeClose: true,
        endedRowPresentBeforeClose: true, exactEndedRowRemovedByClose: true },
        bootstrap: { actualInitialOwnedListHeld: true, replacementLiveBeforeListRelease: true, actualLateListDelivered: true,
          replacementRendererRetained: true, replacementStatusRetained: true, replacementCursorRetained: true,
          actualReplacementMarkerWriteRead: true, noOlderTerminalIO: true, replacementRemovedByUIClose: true,
          originalAutoAttachedForCleanup: true, originalRemovedByUIClose: true } };
    } finally {
      if (held) await cdp.send('Fetch.continueResponse', { requestId: held.requestId }, 'held-cleanup');
      network.responseHandler = null; removeRequest();
      await click(cdp, '#term-close');
    }
  });
  await record('invalid-serial-controlled-refusal', true, async () => {
    await navigate(cdp, localUrl(live.panel, baseUrl));
    let saw = false;
    network.handler = async event => {
      if (new URL(event.request.url).pathname !== '/api/terminal/create') return false;
      const data = JSON.parse(event.request.postData || '{}');
      saw = data.kind === 'serial' && data.port === '/__ui-fixtures/invalid-serial-port' && data.baud === 57600;
      await fulfill(cdp, event, { error: 'controlled invalid serial port' }, 400); return true;
    };
    try {
      await cdp.evaluate(`document.querySelector('#term-port').value='/__ui-fixtures/invalid-serial-port';document.querySelector('#term-baud').value='57600'`);
      await click(cdp, '#term-serial');
      await cdp.until(`document.querySelector('#term-status')?.textContent.includes('controlled invalid serial port')`, 'serial refusal status missing');
      requireThat(saw, 'serial fields were not sent unchanged');
    } finally { network.handler = null; }
  });
}

async function activityInteractions({ cdp, baseUrl, live, record, network, runDir }) {
  await record('session-goal-current-history-and-deep-links', false, async () => {
    requireThat(Array.isArray(live.sessionIds) && live.sessionIds.length >= 2, 'live session metadata missing');
    for (const id of live.sessionIds.slice(0, 2)) {
      await navigate(cdp, new URL('/activity?session=' + encodeURIComponent(id), baseUrl).href);
      requireThat(await cdp.evaluate(`!!document.querySelector('#ledger') && document.querySelectorAll('h1').length===1 && !!document.querySelector('a[aria-current="page"][href*="session="]')`), 'selected session goal/current work missing');
    }
    const receipt = live.receipt;
    requireThat(typeof receipt === 'string', 'receipt deep-link metadata missing');
    await navigate(cdp, localUrl(receipt, baseUrl));
    requireThat(await cdp.evaluate(`!!document.querySelector('#ledger') && !!document.querySelector('details[open]')`), 'receipt deep link did not open work');
    for (const query of ['?session=browser-fixture-unknown', '?txn=browser-fixture-unknown']) {
      await navigate(cdp, baseUrl + '/activity' + query);
      requireThat(await cdp.evaluate(`/not found|not in|no longer|unknown|missing/i.test(document.querySelector('main').textContent)`), 'unknown deep link lacks missing-state explanation');
    }
  });
  await record('activity-filter-disclosure-and-run-detail', false, async () => {
    await navigate(cdp, localUrl(live.activity, baseUrl));
    await click(cdp, '#all-activity > summary');
    const group = await cdp.evaluate(`(() => {const f=document.querySelector('#activity-filter');return [...f.options].find(o=>o.value!=='all')?.value})()`);
    requireThat(group, 'activity filter choices missing');
    await cdp.evaluate(`(() => {const f=document.querySelector('#activity-filter');f.value=${JSON.stringify(group)};f.dispatchEvent(new Event('change',{bubbles:true}))})()`);
    requireThat(await cdp.evaluate(`(() => {const rows=[...document.querySelectorAll('#feed .activity-row[data-group]')];return rows.length>0 && rows.every(r=>r.hidden === (r.dataset.group!==${JSON.stringify(group)}))})()`), 'tool filter did not preserve matching rows');
    await cdp.evaluate(`(() => {const f=document.querySelector('#activity-filter');f.value='all';f.dispatchEvent(new Event('change',{bubbles:true}))})()`);
    await click(cdp, 'details[data-run] > summary');
    await cdp.until(`document.querySelector('details[data-run][open] .activity-run-body dl') !== null`, 'actual run detail did not load');
    await screenshot(cdp, join(runDir, 'activity-run-detail-1280.png'));
  });
  await record('run-detail-controlled-failure-and-late-view-retirement', true, async () => {
    let held = null;
    network.handler = async event => {
      if (!new URL(event.request.url).pathname.startsWith('/api/runs/')) return false;
      await fulfill(cdp, event, { error: 'controlled run unavailable' }, 503); return true;
    };
    try {
      await navigate(cdp, localUrl(live.activity, baseUrl));
      await click(cdp, '#all-activity > summary'); await click(cdp, 'details[data-run] > summary');
      await cdp.until(`document.querySelector('details[data-run][open] .activity-run-body')?.textContent.includes('unavailable')`, 'controlled run failure feedback missing');
      network.handler = async event => {
        if (!new URL(event.request.url).pathname.startsWith('/api/runs/')) return false;
        held = event; return true;
      };
      await navigate(cdp, localUrl(live.activity, baseUrl));
      await click(cdp, '#all-activity > summary'); await click(cdp, 'details[data-run] > summary');
      const end = Date.now() + 5000; while (!held && Date.now() < end) await delay(50);
      requireThat(held, 'delayed run response was not captured');
      await cdp.evaluate(`document.documentElement.dataset.retiredSentinel='run-retained';history.pushState({},'', '/activity?session=browser-fixture-run-new-view');window.dispatchEvent(new PageTransitionEvent('pagehide'))`);
      await fulfill(cdp, held, { error: 'controlled retired run denial' }, 403); held = null;
      await delay(150);
      requireThat(await cdp.evaluate(`location.search==='?session=browser-fixture-run-new-view' && document.documentElement.dataset.retiredSentinel==='run-retained'`), 'late run denial replaced the new view');
    } finally {
      if (held) await cdp.send('Fetch.failRequest', { requestId: held.requestId, errorReason: 'Aborted' });
      network.handler = null;
    }
  });
  await record('activity-stale-and-recovery', true, async () => {
    let failed = false;
    network.handler = async event => {
      if (new URL(event.request.url).pathname !== '/api/activity' || failed) return false;
      failed = true; await fulfill(cdp, event, { error: 'controlled poll failure' }, 503); return true;
    };
    try {
      await navigate(cdp, localUrl(live.activity, baseUrl));
      await cdp.until(`document.querySelector('#activity-state')?.textContent.includes('stale')`, 'failed poll did not show stale');
      await cdp.until(`document.querySelector('#activity-state')?.textContent==='live'`, 'poll recovery did not restore live');
      requireThat(failed, 'controlled poll failure was not exercised');
    } finally { network.handler = null; }
  });
  await record('activity-follow-toggle-and-late-view-retirement', true, async () => {
    let held = null;
    network.handler = async event => {
      if (new URL(event.request.url).pathname !== '/api/activity' || held) return false;
      held = event; return true;
    };
    try {
      await navigate(cdp, localUrl(live.activity, baseUrl));
      const end = Date.now() + 5000; while (!held && Date.now() < end) await delay(50);
      requireThat(held, 'controlled delayed poll was not captured');
      await click(cdp, '#all-activity > summary');
      await cdp.evaluate('document.activeElement?.blur();window.scrollTo(0,0)');
      await key(cdp, 'f', 'KeyF', 70);
      const next = await cdp.evaluate(`Number(document.querySelector('#feed').dataset.next.split('.').at(-1)) || 0`);
      await fulfill(cdp, held, { rows: [], next, more: false, counts: null }); held = null;
      await delay(150);
      requireThat(await cdp.evaluate('scrollY < 100'), 'disabled follow unexpectedly moved to the tail');
      await key(cdp, 'f', 'KeyF', 70);
      const followEnd = Date.now() + 5000; while (!held && Date.now() < followEnd) await delay(50);
      requireThat(held, 'enabled follow poll was not captured');
      await fulfill(cdp, held, { rows: [], next, more: false, counts: null }); held = null;
      await cdp.until('scrollY > 100', 're-enabled follow did not reach the tail');
      // Pause another poll, retire the document through its real pagehide lifecycle, then release a
      // late denial. The old document may not reload and replace the newly selected session.
      const waitEnd = Date.now() + 5000; while (!held && Date.now() < waitEnd) await delay(50);
      requireThat(held, 'late response was not captured');
      await cdp.evaluate(`document.documentElement.dataset.retiredSentinel='retained';history.pushState({},'', '/activity?session=browser-fixture-new-view');window.dispatchEvent(new PageTransitionEvent('pagehide'))`);
      await fulfill(cdp, held, { error: 'controlled retired denial' }, 403); held = null;
      await delay(150);
      requireThat(await cdp.evaluate(`location.search==='?session=browser-fixture-new-view' && document.readyState==='complete' && document.documentElement.dataset.retiredSentinel==='retained'`), 'late response replaced the new view');
    } finally {
      if (held) await cdp.send('Fetch.failRequest', { requestId: held.requestId, errorReason: 'Aborted' });
      network.handler = null;
    }
  });
}

export async function main(args = process.argv.slice(2)) {
  try {
    const options = parseCli(args);
    if (options.help) { console.log(USAGE); return 0; }
    const chrome = findChrome();
    return await runAcceptance(options, chrome);
  } catch (error) {
    // All thrown messages are our bounded diagnostics, never raw CDP/fetch/private server payloads.
    const known = error instanceof MissingBrowser || error.message === USAGE
      || /^(base URL|manifest |invalid |incomplete |missing live|Rust fixture|accessibility helper|Chromium|Node with)/.test(error.message);
    console.error(known ? error.message : 'device UI browser acceptance failed; no full acceptance result');
    return error instanceof MissingBrowser ? 2 : 1;
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) process.exitCode = await main();
