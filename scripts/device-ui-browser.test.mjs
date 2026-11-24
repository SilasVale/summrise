import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { Cdp, openBrowser, validateManifest } from './device-ui-browser.mjs';

const driverUrl = new URL('./device-ui-browser.mjs', import.meta.url);
const driverSource = readFileSync(driverUrl, 'utf8');
const cliOutput = mkdtempSync(join(tmpdir(), 'device-ui-browser-cli-tests-'));

// Test boundary: expose existing private helpers in memory by adding exports only.
// Their bodies and the public CDP/browser implementation remain production code.
// No alternate driver, baseline source, or product page is supplied by these tests.
const privateSource = driverSource
  .replace(/(?<!export )async function (click|executeInteraction|key|keyboardStart|tabTo|installNetworkBoundary|resetInteraction|navigate)\(/g, 'export async function $1(')
  .replace('function inspectEnrollmentContract(', 'export function inspectEnrollmentContract(')
  .replace('const requireThat =', 'export const requireThat =');
const actions = await import('data:text/javascript;base64,' + Buffer.from(privateSource).toString('base64'));

// Execute the exact production disclosure acceptance block, including its focused
// remote-object checks, connected/focus wait, exception checks and finally release.
const disclosureStart = driverSource.indexOf("        const selector = 'details > summary';");
const disclosureEnd = driverSource.indexOf('\n      });', disclosureStart);
assert.ok(disclosureStart >= 0 && disclosureEnd > disclosureStart, 'production keyboard action must be extractable');
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
const exerciseDisclosure = new AsyncFunction('cdp', 'tabTo', 'key', 'requireThat', driverSource.slice(disclosureStart, disclosureEnd));

function runCli(args, env = {}) {
  return spawnSync(process.execPath, [fileURLToPath(driverUrl), ...args], {
    encoding: 'utf8', timeout: 15000,
    env: { ...process.env, ...env, NODE_OPTIONS: '' },
  });
}

// Infrastructure fixtures only: native Chrome documents, CDP and loopback HTTP.
// They prove driver mechanics, not Rust product rendering, UI acceptance or hardware.
async function withBrowser(action) {
  const browser = await openBrowser();
  try {
    assert.ok(browser.cdp instanceof Cdp, 'use the public production transport');
    return await action(browser.cdp);
  } finally {
    await browser.stop();
  }
}

async function documentFixture(cdp, html) {
  // Opaque data URLs cannot follow their own fragment link in Chrome. about:blank
  // preserves native skip-link activation for these owned infrastructure documents.
  await cdp.send('Page.navigate', { url: 'about:blank' });
  const tree = await cdp.send('Page.getFrameTree');
  await cdp.send('Page.setDocumentContent', { frameId: tree.frameTree.frame.id, html });
}

function withDeadline(promise, detail) {
  let timer;
  const deadline = new Promise((resolve, reject) => {
    timer = setTimeout(() => reject(new Error(detail)), 5000);
  });
  return Promise.race([promise, deadline]).finally(() => clearTimeout(timer));
}

async function withNetworkFixture(action) {
  const server = createServer((request, response) => {
    if (request.url === '/boot') {
      response.setHeader('content-type', 'text/html');
      response.end('<!doctype html><title>new document</title><script>window.bootSeen=false;fetch("/api/terminal/boot/read").then(response=>response.json()).then(value=>bootSeen=value.boundary===true)</script>');
      return;
    }
    const document = request.url === '/';
    response.setHeader('content-type', document ? 'text/html' : 'application/json');
    response.end(document
      ? '<!doctype html><title>Fetch ownership infrastructure only</title>'
      : JSON.stringify({ text: 'fixture', cursor: 1, alive: true }));
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const base = 'http://127.0.0.1:' + server.address().port;
  try {
    await withBrowser(async cdp => {
      const network = await actions.installNetworkBoundary(cdp, base);
      network.visualFixture = false;
      await cdp.send('Page.navigate', { url: base + '/' });
      await cdp.until("document.readyState==='complete' && document.title==='Fetch ownership infrastructure only'", 'loopback document did not load');
      await action(cdp, network, base);
    });
  } finally {
    server.closeAllConnections();
    await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
  }
}

async function withSecondOrigin(action) {
  let httpHits = 0;
  let wsHits = 0;
  const server = createServer((request, response) => {
    httpHits += 1;
    response.setHeader('access-control-allow-origin', '*');
    response.end('owned second origin');
  });
  server.on('upgrade', (request, socket) => {
    wsHits += 1;
    socket.end('HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n');
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const base = 'http://127.0.0.1:' + server.address().port;
  try {
    await action(base, () => ({ httpHits, wsHits }));
  } finally {
    server.closeAllConnections();
    await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
  }
}

async function foreignRequest(cdp, url, websocket = false) {
  const expression = websocket
    ? `window.foreignDone=false;{const socket=new WebSocket(${JSON.stringify(url.replace('http:', 'ws:'))});socket.onerror=()=>foreignDone=true;socket.onopen=()=>{foreignDone=true;socket.close();}}`
    : `window.foreignDone=false;void fetch(${JSON.stringify(url)}).catch(()=>null).finally(()=>foreignDone=true)`;
  await cdp.evaluate(expression);
  await cdp.until('foreignDone===true', 'the foreign request was neither refused nor completed');
}

test('CLI help works without an installed browser', () => {
  const result = runCli(['--help'], { CHROME: join(cliOutput, 'no-browser') });
  assert.equal(result.status, 0);
  assert.match(result.stdout, /--base-url.*--out/);
});

test('CLI rejects an external origin before creating output or opening a browser', () => {
  const out = join(cliOutput, 'external-origin-must-not-create');
  const result = runCli(['--base-url', 'https://example.com', '--out', out]);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /loopback/);
  assert.equal(existsSync(out), false);
});

test('missing required CLI arguments fail with usage', () => {
  const result = runCli([]);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /usage:/);
});

test('an explicit unavailable browser exits 2 and cannot become a green skip', () => {
  const out = join(cliOutput, 'missing-browser-output');
  const result = runCli(['--base-url', 'http://127.0.0.1:1', '--out', out], {
    CHROME: join(cliOutput, 'no-such-browser'),
  });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /Chromium/);
  assert.equal(existsSync(out), false);
});

test('manifest validation preserves opaque metadata and rejects escaped origins or incomplete matrices', () => {
  const cases = [
    'panel-normal', 'panel-first', 'panel-empty', 'panel-unreadable', 'panel-bounded-long',
    'activity-working', 'activity-refused', 'activity-closed-failure', 'activity-aborted',
    'activity-orphan', 'activity-no-goal', 'activity-no-session-empty',
    'activity-no-session-recorded', 'activity-unreadable', 'activity-multi-long',
  ].map(name => ({ name, url: '/__ui-fixtures/' + name, kind: name.startsWith('panel') ? 'panel' : 'activity' }));
  const manifest = { cases, live: { panel: '/panel', root: '/', activity: '/activity' }, future: { opaque: true } };
  assert.equal(validateManifest(manifest, 'http://127.0.0.1:1').future.opaque, true);
  assert.throws(() => validateManifest({ ...manifest, cases: cases.slice(1) }, 'http://127.0.0.1:1'), /matrix/);
  assert.throws(() => validateManifest({ ...manifest, cases: [{ ...cases[0], url: '//example.com/' }, ...cases.slice(1)] }, 'http://127.0.0.1:1'), /relative/);
  assert.throws(() => validateManifest({ ...manifest, live: { ...manifest.live, panel: '/\\example.com/' } }, 'http://127.0.0.1:1'), /relative/);
});

test('real CDP rejects protocol and evaluation errors privately and captures a native document', async () => {
  await withBrowser(async cdp => {
    await assert.rejects(cdp.send('NotARealDomain.noSuchMethod'), /CDP protocol error/);
    const result = await cdp.send('Page.navigate', { url: 'data:text/html,<title>driver-transport</title><p>infrastructure only</p>' });
    assert.equal(result.errorText, undefined);
    await cdp.until("document.title==='driver-transport' && document.readyState==='complete'", 'data document did not load');
    assert.equal(await cdp.evaluate('6*7'), 42);
    await assert.rejects(cdp.evaluate("(()=>{throw new Error('private-payload-must-not-escape')})()"), /^Error: browser evaluation failed$/);
    const image = await cdp.send('Page.captureScreenshot', { format: 'png' });
    assert.equal(Buffer.from(image.data, 'base64').subarray(1, 4).toString(), 'PNG');
  });
});

test('a native mouse click opens the targeted summary disclosure once', async () => {
  await withBrowser(async cdp => {
    await documentFixture(cdp, '<details id="all"><summary>All activity</summary><p>infrastructure body</p></details>');
    await actions.click(cdp, '#all > summary');
    assert.equal(await cdp.evaluate("document.querySelector('#all').open"), true);
  });
});

test('a nested summary click opens ancestors without pre-toggling its own disclosure', async () => {
  await withBrowser(async cdp => {
    await documentFixture(cdp, '<details id="all"><summary>All activity</summary><details data-run="fixture"><summary>Run</summary><div class="activity-run-body"></div></details></details>');
    await actions.click(cdp, 'details[data-run] > summary');
    assert.deepEqual(await cdp.evaluate("[document.querySelector('#all').open,document.querySelector('[data-run]').open]"), [true, true]);
  });
});

test('the form contract checks a synthetic boot token privately without inventing script nonces', async () => {
  await withBrowser(async cdp => {
    const token = 'a'.repeat(64);
    const html = `<meta name="summrise-panel" content="${token}"><form id="enroll-form" method="post" action="http://127.0.0.1/panel/enroll"><input name="panel_token" value="${token}"><label for="enroll-console">Console</label><input id="enroll-console" name="console_url"><label for="enroll-code">Code</label><input id="enroll-code" name="code"><input name="force"><input name="allow_plaintext"></form><script>void 0</script>`;
    await documentFixture(cdp, html);
    const observed = await cdp.evaluate(`(${actions.inspectEnrollmentContract.toString()})()`);
    assert.equal(observed.tokenMatchesMeta, true);
    assert.equal(observed.fieldsPresent, true);
    assert.equal(observed.inlineScriptCount, 1);
    assert.equal(observed.noncedInlineScriptCount, 0);
    assert.equal(JSON.stringify(observed).includes(token), false, 'diagnostics cannot contain the token');
    await cdp.evaluate("document.querySelector('[name=panel_token]').value='wrong'");
    assert.equal((await cdp.evaluate(`(${actions.inspectEnrollmentContract.toString()})()`)).tokenMatchesMeta, false);
  });
});

test('a failed interaction resets native media, scale, viewport, focus and request handlers', async () => {
  await withBrowser(async cdp => {
    const network = { handler: null, responseHandler: null, responseObservers: new Set() };
    await documentFixture(cdp, '<input id="focus">');
    const row = await actions.executeInteraction(cdp, network, async () => {
      network.handler = () => true;
      network.responseHandler = () => true;
      network.responseObservers.add(() => {});
      await cdp.send('Emulation.setEmulatedMedia', { features: [{ name: 'forced-colors', value: 'active' }, { name: 'prefers-reduced-motion', value: 'reduce' }] });
      await cdp.send('Emulation.setDeviceMetricsOverride', { width: 390, height: 900, deviceScaleFactor: 1, mobile: false });
      await cdp.send('Emulation.setPageScaleFactor', { pageScaleFactor: 2 });
      await cdp.evaluate("document.querySelector('#focus').focus()");
      throw new Error('controlled infrastructure assertion');
    });
    assert.equal(row.status, 'fail');
    assert.equal(row.detail, 'controlled infrastructure assertion');
    assert.notEqual(row.cleanupFailed, true, 'the controlled assertion must not mask cleanup failure');
    assert.equal(network.handler, null);
    assert.equal(network.responseHandler, null);
    assert.equal(network.responseObservers.size, 0);
    assert.deepEqual(await cdp.evaluate("[innerWidth,visualViewport.scale,matchMedia('(forced-colors: active)').matches,matchMedia('(prefers-reduced-motion: reduce)').matches,document.activeElement.tagName]"), [1280, 1, false, false, 'BODY']);
  });
});

for (const hiddenFirst of [false, true]) {
  test('native keyboard disclosure follows the Tab-focused summary ' + (hiddenFirst ? 'past a hidden earlier summary' : 'after skip-link focus'), async () => {
    await withBrowser(async cdp => {
      const earlier = hiddenFirst
        ? '<details hidden id="earlier"><summary>Earlier hidden</summary></details>'
        : '<details id="earlier"><summary>Earlier outside main</summary></details>';
      await documentFixture(cdp, '<a class="device-skip" href="#main">Skip</a>' + earlier + '<main id="main" tabindex="-1"><details id="target"><summary>Actual keyboard target</summary><p>Body</p></details></main>');
      await actions.keyboardStart(cdp);
      await actions.key(cdp, 'Tab', 'Tab', 9);
      assert.equal(await cdp.evaluate("document.activeElement.matches('a.device-skip')"), true);
      await actions.key(cdp, 'Enter', 'Enter', 13);
      assert.equal(await cdp.evaluate('location.hash'), '#main');
      await cdp.evaluate("window.enterEvents=[];document.addEventListener('keypress',event=>{if(event.key==='Enter')enterEvents.push({trusted:event.isTrusted})})");
      await exerciseDisclosure(cdp, actions.tabTo, actions.key, actions.requireThat);
      assert.deepEqual(await cdp.evaluate("[document.querySelector('#earlier').open,document.querySelector('#target').open,document.activeElement===document.querySelector('#target > summary')]"), [false, true, true]);
      assert.deepEqual(await cdp.evaluate('enterEvents'), [{ trusted: true }], 'Enter produces exactly one trusted native keypress');
    });
  });
}

test('native Enter honors keydown preventDefault without a synthetic char fallback', async () => {
  await withBrowser(async cdp => {
    await documentFixture(cdp, '<details><summary>Blocked summary</summary><p>body</p></details>');
    await cdp.evaluate("window.keypresses=0;document.querySelector('summary').addEventListener('keydown',event=>event.preventDefault());document.addEventListener('keypress',()=>keypresses++)");
    await actions.keyboardStart(cdp);
    await actions.tabTo(cdp, 'summary');
    await actions.key(cdp, 'Enter', 'Enter', 13);
    assert.deepEqual(await cdp.evaluate("[document.querySelector('details').open,keypresses]"), [false, 0]);
  });
});

test('the actual embedded xterm receives exactly one carriage return from native Enter', async () => {
  await withBrowser(async cdp => {
    await documentFixture(cdp, '<div id="terminal" style="width:800px;height:400px"></div>');
    await cdp.evaluate(readFileSync(new URL('../device/device/assets/xterm.js', import.meta.url), 'utf8'));
    await cdp.evaluate("window.term=new Terminal({cols:80,rows:24});term.open(document.querySelector('#terminal'));window.input=[];term.onData(data=>input.push(data));term.focus()");
    await actions.key(cdp, 'Enter', 'Enter', 13);
    assert.deepEqual(await cdp.evaluate('input'), ['\r']);
  });
});

test('focus moving before keyboard capture fails closed without toggling a different parent', async () => {
  await withBrowser(async cdp => {
    await documentFixture(cdp, '<details><summary>Summary</summary><p>body</p></details><button>Other focus</button>');
    await actions.keyboardStart(cdp);
    const moveFocus = async (transport, selector) => {
      await actions.tabTo(transport, selector);
      await transport.evaluate("document.querySelector('button').focus()");
    };
    await assert.rejects(exerciseDisclosure(cdp, moveFocus, actions.key, actions.requireThat), /focus changed before activation/);
    assert.equal(await cdp.evaluate("document.querySelector('details').open"), false);
  });
});

for (const stage of ['request', 'response']) {
  test(`native Fetch ${stage} release acknowledgement and concurrent reset have one owner`, async () => {
    await withNetworkFixture(async (cdp, network) => {
      let signalPause;
      const paused = new Promise(resolve => { signalPause = resolve; });
      const hold = async event => {
        if (new URL(event.request.url).pathname !== '/api/terminal/owned/read') return false;
        signalPause(event);
        return true;
      };
      if (stage === 'request') network.handler = hold;
      else network.responseHandler = hold;
      await cdp.evaluate("window.completed=false;void fetch('/api/terminal/owned/read').then(response=>response.json()).then(()=>completed=true)");
      const held = await withDeadline(paused, 'native Fetch pause was not observed');
      const release = cdp.send(stage === 'request' ? 'Fetch.continueRequest' : 'Fetch.continueResponse', { requestId: held.requestId });
      await actions.resetInteraction(cdp, network);
      await release;
      await cdp.until('window.completed===true', 'owned Fetch request did not complete');
      assert.equal(cdp.eventFailure, undefined, 'cleanup cannot hide infrastructure event failure');
    });
  });
}

test('an in-flight native Fetch handler drains before reset releases its pause', async () => {
  await withNetworkFixture(async (cdp, network) => {
    let signalEntered;
    let resume;
    const entered = new Promise(resolve => { signalEntered = resolve; });
    const gate = new Promise(resolve => { resume = resolve; });
    network.handler = async event => {
      if (new URL(event.request.url).pathname !== '/held-handler') return false;
      signalEntered();
      await gate;
      return false;
    };
    await cdp.evaluate("window.completed=false;void fetch('/held-handler').then(response=>response.json()).then(()=>completed=true)");
    await withDeadline(entered, 'native Fetch handler did not start');
    const reset = actions.resetInteraction(cdp, network);
    resume();
    await reset;
    await cdp.until('completed===true', 'handler request did not complete');
    assert.equal(cdp.eventFailure, undefined);
    assert.equal(cdp.commandFailures.length, 0);
  });
});

test('navigation cancelling a held native Fetch response leaves no orphan cleanup owner', async () => {
  await withNetworkFixture(async (cdp, network, base) => {
    let signalPause;
    const paused = new Promise(resolve => { signalPause = resolve; });
    network.responseHandler = async event => {
      if (new URL(event.request.url).pathname !== '/api/terminal/cancel/read') return false;
      signalPause(event);
      return true;
    };
    await cdp.evaluate("void fetch('/api/terminal/cancel/read').catch(()=>{})");
    await withDeadline(paused, 'native Fetch response was not paused');
    await cdp.send('Page.navigate', { url: base + '/?after-cancel' });
    await cdp.until("document.readyState==='complete' && location.search==='?after-cancel'", 'cancellation navigation did not finish');
    await actions.resetInteraction(cdp, network);
    assert.equal(cdp.eventFailure, undefined);
    assert.equal(cdp.commandFailures.length, 0);
  });
});

test('a response observer waiting during a browser abort cannot continue a retired interception', async () => {
  await withNetworkFixture(async (cdp, network) => {
    let signalEntered;
    let resume;
    const entered = new Promise(resolve => { signalEntered = resolve; });
    const gate = new Promise(resolve => { resume = resolve; });
    network.responseObservers.add(async event => {
      if (new URL(event.request.url).pathname !== '/api/terminal/abort/read') return;
      signalEntered();
      await gate;
    });
    await cdp.evaluate("window.aborted=false;window.abortController=new AbortController();void fetch('/api/terminal/abort/read',{signal:abortController.signal}).catch(()=>aborted=true)");
    await withDeadline(entered, 'native response observer did not start');
    await cdp.evaluate('abortController.abort()');
    await cdp.until('aborted===true', 'the real browser abort did not complete');
    resume();
    await cdp.drainFetch();
    assert.equal(cdp.eventFailure, undefined, 'a canceled response cannot produce another protocol continuation');
    assert.equal(cdp.commandFailures.length, 0);
  });
});

test('a visual Activity cursor await cannot fulfill a browser-canceled request lease', async () => {
  await withNetworkFixture(async (cdp, network) => {
    network.visualFixture = true;
    let signalEntered;
    let resume;
    const entered = new Promise(resolve => { signalEntered = resolve; });
    const gate = new Promise(resolve => { resume = resolve; });
    const evaluate = cdp.evaluate.bind(cdp);
    // Hold only the production cursor-evaluation boundary, then execute its real
    // expression unchanged. Cancellation comes from the actual browser request.
    cdp.evaluate = async expression => {
      if (expression.startsWith("Number(document.querySelector('#feed')")) {
        signalEntered();
        await gate;
      }
      return evaluate(expression);
    };
    try {
      await cdp.evaluate("window.aborted=false;window.abortController=new AbortController();void fetch('/api/activity',{signal:abortController.signal}).catch(()=>aborted=true)");
      await withDeadline(entered, 'the native visual cursor evaluation did not start');
      await cdp.evaluate('abortController.abort()');
      await cdp.until('aborted===true', 'the real Activity request abort did not complete');
      resume();
      await cdp.drainFetch();
      assert.equal(cdp.eventFailure, undefined, 'a cursor await cannot fulfill a canceled native interception');
      assert.equal(cdp.commandFailures.length, 0);
    } finally {
      resume();
      cdp.evaluate = evaluate;
    }
  });
});

test('a genuine invalid native Fetch release keeps cleanup failed with bounded diagnostics', async () => {
  await withNetworkFixture(async (cdp, network, base) => {
    const invalidId = 'intentionally-invalid-owned-test-request';
    cdp.paused.set(invalidId, false);
    const row = await actions.executeInteraction(cdp, network, async () => {});
    assert.equal(row.status, 'fail');
    assert.equal(row.cleanupFailed, true);
    assert.ok(row.cleanupFailures?.some(item => item.step === 'release-request' && item.error?.kind === 'protocol'), 'genuine native release failure must retain its classification');
    assert.ok(cdp.commandFailures.some(error => error.kind === 'protocol' && error.code === -32602), 'genuine release failure must remain in the global infrastructure failure channel');
    const diagnostic = JSON.stringify(row.cleanupFailures);
    assert.equal(diagnostic.includes(invalidId), false, 'bounded diagnostics cannot expose request IDs');
    assert.equal(diagnostic.includes(base), false, 'bounded diagnostics cannot expose URLs');
  });
});

test('a failed polite close still exits owned Chrome and removes its profile while staying fatal', async () => {
  // Expose only this invocation's owned handles in memory. The production shutdown
  // body is unchanged; only Browser.close rejects with the controlled error below.
  const exposedSource = driverSource.replace('return { cdp, stop };', 'return { cdp, stop, browser, profile };');
  const exposed = await import('data:text/javascript;base64,' + Buffer.from(exposedSource).toString('base64'));
  const owned = await exposed.openBrowser();
  const send = owned.cdp.send.bind(owned.cdp);
  const failure = Object.assign(new Error('controlled polite close rejection'), {
    kind: 'protocol', method: 'Browser.close', protocolCode: -32000,
  });
  owned.cdp.send = (method, params) => method === 'Browser.close' ? Promise.reject(failure) : send(method, params);
  try {
    await assert.rejects(owned.stop(), error => error === failure, 'the original polite-close error stays fatal');
    assert.ok(owned.browser.exitCode !== null || owned.browser.signalCode !== null, 'the exact owned browser must have exited');
    assert.equal(existsSync(owned.profile), false, 'the verified owned profile must have been removed');
  } finally {
    owned.cdp.send = send;
    if (owned.browser.exitCode === null && owned.browser.signalCode === null) await owned.stop();
  }
});

test('owned browser control acknowledges close while its page connection disappears', async () => {
  const exposedSource = driverSource.replace('return { cdp, stop };', 'return { cdp, stop, browser, profile };');
  const exposed = await import('data:text/javascript;base64,' + Buffer.from(exposedSource).toString('base64'));
  const owned = await exposed.openBrowser();
  const send = owned.cdp.send.bind(owned.cdp);
  let acknowledged = false;
  owned.cdp.send = (method, params) => {
    const command = send(method, params);
    if (method !== 'Browser.close') return command;
    owned.cdp.socket.close();
    return command.then(answer => { acknowledged = true; return answer; });
  };
  try {
    await owned.stop();
    assert.equal(acknowledged, true, 'the exact owned browser control channel must acknowledge its real close command');
    assert.equal(owned.browser.exitCode, 0, 'the exact owned Chrome must exit normally');
    assert.equal(owned.browser.signalCode, null, 'a fallback signal is not a natural close');
    assert.equal(existsSync(owned.profile), false, 'the verified owned profile must be gone');
  } finally {
    owned.cdp.send = send;
    if (owned.browser.exitCode === null && owned.browser.signalCode === null) await owned.stop();
  }
});

test('an ordinary page disconnect while owned Chrome lives stays fatal after fallback cleanup', async () => {
  const exposedSource = driverSource.replace('return { cdp, stop };', 'return { cdp, stop, browser, profile };');
  const exposed = await import('data:text/javascript;base64,' + Buffer.from(exposedSource).toString('base64'));
  const owned = await exposed.openBrowser();
  owned.cdp.socket.close();
  await new Promise(resolve => owned.cdp.socket.addEventListener('close', resolve, { once: true }));
  await assert.rejects(owned.stop(), /CDP connection closed/);
  assert.ok(owned.browser.exitCode !== null || owned.browser.signalCode !== null, 'fallback must exit the exact owned Chrome');
  assert.equal(existsSync(owned.profile), false, 'fatal disconnect still removes the verified owned profile');
});

test('a close acknowledgement cannot hide the exact owned launcher exiting by signal', async () => {
  const exposedSource = driverSource.replace('return { cdp, stop };', 'return { cdp, stop, browser, profile };');
  const exposed = await import('data:text/javascript;base64,' + Buffer.from(exposedSource).toString('base64'));
  const owned = await exposed.openBrowser();
  const send = owned.cdp.send.bind(owned.cdp);
  // Negative CDP boundary: supply a false close ACK while actually terminating only
  // this invocation's owned launcher. The positive case requires the real control ACK.
  owned.cdp.send = (method, params) => {
    if (method !== 'Browser.close') return send(method, params);
    owned.browser.kill('SIGKILL');
    return Promise.resolve({});
  };
  try {
    await assert.rejects(owned.stop(), /owned Chromium did not exit normally/);
    assert.equal(owned.browser.signalCode, 'SIGKILL');
    assert.equal(existsSync(owned.profile), false, 'fatal abnormal exit still removes the verified owned profile');
  } finally {
    owned.cdp.send = send;
    if (owned.browser.exitCode === null && owned.browser.signalCode === null) await owned.stop();
  }
});

test('signal fallback after a false close acknowledgement always stays fatal', async () => {
  const exposedSource = driverSource.replace('return { cdp, stop };', 'return { cdp, stop, browser, profile };');
  const exposed = await import('data:text/javascript;base64,' + Buffer.from(exposedSource).toString('base64'));
  const owned = await exposed.openBrowser();
  const send = owned.cdp.send.bind(owned.cdp);
  owned.cdp.send = (method, params) => method === 'Browser.close' ? Promise.resolve({}) : send(method, params);
  try {
    let stopRejection;
    await assert.rejects(owned.stop().catch(error => { stopRejection = error; throw error; }), /signal fallback/);
    const profilePresent = existsSync(owned.profile);
    // Build bounded rejection metadata only after preserving the original observation.
    const diagnostic = profilePresent ? {
      kind: stopRejection?.kind || 'command', method: stopRejection?.method || null,
      protocolCode: Number.isInteger(stopRejection?.protocolCode) ? stopRejection.protocolCode : null,
      cleanupFailures: Array.isArray(stopRejection?.cleanupFailures)
        ? stopRejection.cleanupFailures.slice(0, 4).map(item => ({ step: item.step,
          kind: item.error?.kind || 'command', method: item.error?.method || null,
          code: Number.isInteger(item.error?.code) ? item.error.code : null,
          classification: item.error?.classification || null, reason: item.error?.reason || null })) : [],
      ownedExitCode: owned.browser.exitCode, ownedSignal: owned.browser.signalCode,
    } : null;
    assert.equal(profilePresent, false, 'fallback failure still removes the verified owned profile'
      + (diagnostic ? ': ' + JSON.stringify(diagnostic) : ''));
  } finally {
    owned.cdp.send = send;
    if (owned.browser.exitCode === null && owned.browser.signalCode === null) await owned.stop();
  }
});

test('already exited owned Chrome cannot bypass this stop acknowledgement', async () => {
  const exposedSource = driverSource.replace('return { cdp, stop };', 'return { cdp, stop, browser, profile };');
  const exposed = await import('data:text/javascript;base64,' + Buffer.from(exposedSource).toString('base64'));
  const owned = await exposed.openBrowser();
  const exited = new Promise(resolve => owned.browser.once('exit', resolve));
  await owned.cdp.browserControl.send('Browser.close');
  await withDeadline(exited, 'the exact owned browser did not exit after the external close');
  await assert.rejects(owned.stop(), /close acknowledgement/);
  assert.equal(existsSync(owned.profile), false, 'missing this-stop acknowledgement still removes the verified owned profile');
});

test('acknowledged navigation retirement releases the old body before blank commit and intercepted boot', async t => {
  await withNetworkFixture(async (cdp, network, base) => {
    let signalPause;
    const paused = new Promise(resolve => { signalPause = resolve; });
    network.responseHandler = async event => {
      if (!new URL(event.request.url).pathname.endsWith('/old/read')) return false;
      await cdp.send('Fetch.getResponseBody', { requestId: event.requestId });
      signalPause(event);
      return true;
    };
    await cdp.evaluate("window.oldDone=false;void fetch('/api/terminal/old/read').then(response=>response.json()).then(()=>oldDone=true)");
    const held = await withDeadline(paused, 'the old native response was not held');
    network.handler = async event => {
      if (new URL(event.request.url).pathname !== '/api/terminal/boot/read') return false;
      await cdp.send('Fetch.fulfillRequest', {
        requestId: event.requestId,
        responseCode: 200,
        responseHeaders: [{ name: 'content-type', value: 'application/json' }],
        body: Buffer.from(JSON.stringify({ boundary: true })).toString('base64'),
      });
      return true;
    };
    const send = cdp.send.bind(cdp);
    const order = [];
    let released = false;
    cdp.send = async (method, params, origin) => {
      if (method === 'Network.setBlockedURLs' || method === 'Network.emulateNetworkConditions') {
        const label = method === 'Network.setBlockedURLs'
          ? (params.urls.length ? 'url-deny' : 'url-clear')
          : (params.offline ? 'offline' : 'online');
        order.push('send-' + label);
        const answer = await send(method, params, origin);
        order.push('ack-' + label);
        return answer;
      }
      if (method === 'Fetch.disable') {
        order.push('send-disable');
        const answer = await send(method, params, origin);
        order.push('ack-disable');
        await cdp.until('oldDone===true', 'the disable ACK did not release the old HTTP body');
        released = true;
        return answer;
      }
      if (method === 'Fetch.enable') {
        assert.deepEqual(params.patterns, [
          { urlPattern: '*', requestStage: 'Request' },
          { urlPattern: base + '/api/terminal/*/read*', requestStage: 'Response' },
          { urlPattern: base + '/api/terminal/create', requestStage: 'Response' },
        ], 'the rearmed Fetch domain must preserve the original interception patterns');
        assert.equal(await cdp.evaluate("location.href==='about:blank'"), true);
        order.push('send-enable');
        const answer = await send(method, params, origin);
        order.push('ack-enable');
        return answer;
      }
      if (method === 'Page.navigate') order.push(params.url === 'about:blank' ? 'send-blank' : 'send-target');
      return send(method, params, origin);
    };
    try {
      await actions.navigate(cdp, base + '/boot');
      await cdp.until('bootSeen===true', 'the target boot request was not intercepted');
      assert.equal(released, true);
      assert.deepEqual(order, [
        'send-url-deny', 'ack-url-deny', 'send-offline', 'ack-offline',
        'send-disable', 'ack-disable', 'send-blank', 'send-enable', 'ack-enable',
        'send-url-clear', 'ack-url-clear', 'send-online', 'ack-online', 'send-target',
      ]);
      assert.equal(await cdp.evaluate('navigator.onLine'), true);
      assert.equal(cdp.paused.has(held.requestId), false);
      assert.equal(network.leases.has(held.requestId), false);
      assert.equal(cdp.eventFailure, undefined);
      assert.equal(cdp.commandFailures.length, 0);
      t.diagnostic(JSON.stringify({ window: 'navigation', order, oldBodyReleased: released, bootIntercepted: true, online: true }));
    } finally {
      cdp.send = send;
      if (cdp.paused.has(held.requestId)) await cdp.send('Fetch.continueResponse', { requestId: held.requestId });
    }
  });
});

test('navigation drains an existing response observer and holds queued pauses before disabling', async () => {
  await withNetworkFixture(async (cdp, network, base) => {
    let signalEntered;
    let resume;
    const entered = new Promise(resolve => { signalEntered = resolve; });
    const gate = new Promise(resolve => { resume = resolve; });
    network.responseObservers.add(async event => {
      if (!new URL(event.request.url).pathname.endsWith('/owned/read')) return;
      signalEntered();
      await gate;
    });
    await cdp.evaluate("void fetch('/api/terminal/owned/read')");
    await withDeadline(entered, 'the native response observer did not start');
    const send = cdp.send.bind(cdp);
    let disabled = false;
    cdp.send = (method, params, origin) => {
      if (method === 'Fetch.disable') disabled = true;
      return send(method, params, origin);
    };
    const navigation = actions.navigate(cdp, base + '/');
    let signalQueued;
    const queued = new Promise(resolve => { signalQueued = resolve; });
    const remove = cdp.on('Fetch.requestPaused', event => {
      if (new URL(event.request.url).pathname.endsWith('/queued/read')) signalQueued(event);
    });
    try {
      await cdp.evaluate("void fetch('/api/terminal/queued/read').catch(()=>null)");
      const held = await withDeadline(queued, 'the queued native request was not paused');
      assert.equal(network.phase, 'quiescing');
      assert.equal(cdp.paused.has(held.requestId), true);
      assert.equal(disabled, false, 'the active observer must finish before disable is sent');
      resume();
      await navigation;
      assert.equal(disabled, true);
      assert.equal(cdp.paused.has(held.requestId), false);
      assert.equal(network.leases.has(held.requestId), false);
      assert.equal(cdp.eventFailure, undefined);
      assert.equal(cdp.commandFailures.length, 0);
    } finally {
      remove();
      resume();
      cdp.send = send;
      // Join any already-failed navigation after preserving the primary assertion.
      await Promise.allSettled([navigation]);
    }
  });
});

test('navigation refuses a local pause without an owned lease', async () => {
  await withNetworkFixture(async (cdp, network, base) => {
    cdp.paused.set('native-unowned-pause', true);
    try {
      await assert.rejects(actions.navigate(cdp, base + '/'), /unowned Fetch pause/);
    } finally {
      cdp.paused.delete('native-unowned-pause');
    }
  });
});

test('a genuine duplicate release stays fatal and sticky across later navigation', async t => {
  await withNetworkFixture(async (cdp, network, base) => {
    let signalPause;
    const paused = new Promise(resolve => { signalPause = resolve; });
    network.responseHandler = async event => { signalPause(event); return true; };
    await cdp.evaluate("void fetch('/api/terminal/duplicate/read')");
    const held = await withDeadline(paused, 'the duplicate-release response was not paused');
    await cdp.send('Fetch.continueResponse', { requestId: held.requestId });
    const failureCount = cdp.commandFailures.length;
    let nativeError;
    await assert.rejects(cdp.send('Fetch.continueResponse', { requestId: held.requestId }), error => {
      nativeError = { kind: error.kind, method: error.method, protocolCode: error.protocolCode,
        classification: error.protocolClass || null, reason: error.protocolReason || null };
      return error.kind === 'protocol' && error.method === 'Fetch.continueResponse'
        && Number.isInteger(error.protocolCode) && error.protocolCode < 0;
    });
    const appended = cdp.commandFailures.slice(failureCount);
    assert.equal(appended.length, 1, 'the rejected duplicate must append its own diagnostic');
    const recorded = appended[0];
    assert.equal(recorded.kind, nativeError.kind);
    assert.equal(recorded.method, nativeError.method);
    assert.equal(recorded.code, nativeError.protocolCode);
    const snapshot = structuredClone(recorded);
    await actions.navigate(cdp, base + '/');
    assert.ok(cdp.commandFailures.includes(recorded), 'navigation must retain the exact recorded failure object');
    assert.deepEqual(recorded, snapshot, 'navigation cannot mutate the recorded failure');
    t.diagnostic(JSON.stringify({ nativeDuplicateError: nativeError,
      recordedFailure: { kind: recorded.kind, method: recorded.method, code: recorded.code,
        classification: recorded.classification || null, reason: recorded.reason || null },
      sameRecordedFailureRetained: true }));
  });
});

for (const failed of ['Fetch.disable', 'Fetch.enable']) {
  test(`navigation ${failed} failure stays fatal before target boot`, async () => {
    await withNetworkFixture(async (cdp, network, base) => {
      const send = cdp.send.bind(cdp);
      const original = new Error('injected boundary command failure');
      cdp.send = (method, params, origin) => method === failed ? Promise.reject(original) : send(method, params, origin);
      try {
        await assert.rejects(actions.navigate(cdp, base + '/boot'), error => error === original);
        assert.notEqual(await cdp.evaluate('location.pathname'), '/boot');
      } finally {
        cdp.send = send;
      }
    });
  });
}

test('blank navigation protocol refusal is fatal before re-enabling or target boot', async () => {
  await withNetworkFixture(async (cdp, network, base) => {
    const send = cdp.send.bind(cdp);
    let enabled = false;
    cdp.send = (method, params, origin) => {
      if (method === 'Fetch.enable') enabled = true;
      return send(method, method === 'Page.navigate' && params.url === 'about:blank' ? { url: 'invalid boundary url' } : params, origin);
    };
    try {
      await assert.rejects(actions.navigate(cdp, base + '/boot'), error => error.kind === 'protocol' && error.method === 'Page.navigate');
      assert.equal(enabled, false);
      assert.notEqual(await cdp.evaluate('location.pathname'), '/boot');
    } finally {
      cdp.send = send;
    }
  });
});

test('a quiesced foreign request is blocked and counted before it can reach the second owned origin', async t => {
  await withSecondOrigin(async (other, hits) => {
    await withNetworkFixture(async (cdp, network, base) => {
      let signalEntered;
      let resume;
      const entered = new Promise(resolve => { signalEntered = resolve; });
      const gate = new Promise(resolve => { resume = resolve; });
      network.responseObservers.add(async event => {
        if (!new URL(event.request.url).pathname.endsWith('/owned/read')) return;
        signalEntered();
        await gate;
      });
      await cdp.evaluate("void fetch('/api/terminal/owned/read')");
      await withDeadline(entered, 'the policy fixture observer did not start');
      const navigation = actions.navigate(cdp, base + '/');
      let signalForeign;
      const foreign = new Promise(resolve => { signalForeign = resolve; });
      const remove = cdp.on('Fetch.requestPaused', event => {
        if (new URL(event.request.url).origin === other) signalForeign(event);
      });
      try {
        await cdp.evaluate(`window.foreignDone=false;void fetch(${JSON.stringify(other + '/queued')}).catch(()=>null).finally(()=>foreignDone=true)`);
        await withDeadline(foreign, 'the quiesced foreign request was not observed');
        assert.equal(network.blocked, 1, 'the quiesced policy violation must remain accounted');
        await cdp.until('foreignDone===true', 'the foreign request was not refused');
        assert.deepEqual(hits(), { httpHits: 0, wsHits: 0 });
        resume();
        await navigation;
        assert.deepEqual(hits(), { httpHits: 0, wsHits: 0 });
        assert.equal(network.blocked, 1);
        assert.equal(cdp.eventFailure, undefined);
        assert.equal(cdp.commandFailures.length, 0);
        t.diagnostic(JSON.stringify({ window: 'quiescing', protocol: 'http', ...hits(), blocked: network.blocked }));
      } finally {
        remove();
        resume();
        await Promise.allSettled([navigation]);
      }
    });
  });
});

for (const websocket of [false, true]) {
  test(`a new foreign ${websocket ? 'WebSocket' : 'HTTP'} request stays blocked while Fetch is disabled`, async t => {
    await withSecondOrigin(async (other, hits) => {
      await withNetworkFixture(async (cdp, network, base) => {
        const send = cdp.send.bind(cdp);
        let probed = false;
        cdp.send = async (method, params, origin) => {
          const answer = await send(method, params, origin);
          if (method === 'Fetch.disable') {
            probed = true;
            await foreignRequest(cdp, other + '/disabled', websocket);
            assert.deepEqual(hits(), { httpHits: 0, wsHits: 0 }, 'disabled-domain policy must prevent second-origin contact');
            t.diagnostic(JSON.stringify({ window: 'fetch-disabled', protocol: websocket ? 'websocket' : 'http', ...hits() }));
          }
          return answer;
        };
        try {
          await actions.navigate(cdp, base + '/');
          assert.equal(probed, true);
          assert.deepEqual(hits(), { httpHits: 0, wsHits: 0 });
          assert.equal(cdp.eventFailure, undefined);
          assert.equal(cdp.commandFailures.length, 0);
        } finally {
          cdp.send = send;
        }
      });
    });
  });
}

for (const failed of ['url-deny', 'offline-deny', 'url-clear', 'online-clear']) {
  test(`a native network guard ${failed} protocol refusal stays fatal before target boot`, async t => {
    await withNetworkFixture(async (cdp, network, base) => {
      const send = cdp.send.bind(cdp);
      let injected = false;
      let observed;
      cdp.send = (method, params, origin) => {
        const match = failed === 'url-deny'
          ? method === 'Network.setBlockedURLs' && params.urls.length
          : failed === 'url-clear'
            ? method === 'Network.setBlockedURLs' && !params.urls.length
            : failed === 'offline-deny'
              ? method === 'Network.emulateNetworkConditions' && params.offline
              : method === 'Network.emulateNetworkConditions' && !params.offline;
        if (match) {
          injected = true;
          return send(method, method === 'Network.setBlockedURLs'
            ? { urls: 'native invalid vector' }
            : { ...params, offline: 'native invalid boolean' }, origin);
        }
        return send(method, params, origin);
      };
      try {
        await assert.rejects(actions.navigate(cdp, base + '/boot'), error => {
          observed = error;
          return error.kind === 'protocol' && error.protocolCode === -32602 && error.method.startsWith('Network.');
        });
        assert.equal(injected, true);
        assert.notEqual(await cdp.evaluate('location.pathname'), '/boot');
        t.diagnostic(JSON.stringify({ fault: failed, kind: observed.kind, code: observed.protocolCode, method: observed.method, targetBooted: false, phase: network.phase }));
      } finally {
        cdp.send = send;
      }
    });
  });
}
