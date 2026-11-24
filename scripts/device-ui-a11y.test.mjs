import test from 'node:test';
import assert from 'node:assert/strict';
import * as audit from './device-ui-a11y.mjs';
import { spawn, spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { existsSync, mkdtempSync, readFileSync } from 'node:fs';
import { setTimeout as delay } from 'node:timers/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// Actual Chromium and a data URL exercise serialized capture, without a server or product fixture.
// The owned output root is supplied by the caller; temporary profiles are retained for inspection.
async function captureInChrome(markup, { loopback = false } = {}) {
  const chrome = ['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser']
    .find(command => spawnSync(command, ['--version'], { encoding: 'utf8', timeout: 5000 }).status === 0);
  assert.ok(chrome, 'Chromium is required for serialized DOM capture regression');
  const profile = mkdtempSync(join(process.env.DEVICE_UI_A11Y_TEST_OUT || tmpdir(), 'a11y-capture-'));
  const html = `<!doctype html><html><head><meta charset="utf-8"><style>body{margin:0;color:#000;background:#fff;font:13px sans-serif} .secondary,footer{font-size:12px} .secondary *{font-size:inherit}</style></head><body>${markup}</body></html>`;
  const browser = spawn(chrome, ['--headless=new', '--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage', '--disable-background-networking', '--disable-component-update', '--no-first-run', '--no-default-browser-check', '--remote-debugging-port=0', `--user-data-dir=${profile}`, 'about:blank'], { stdio: 'ignore' });
  const exited = new Promise(resolve => browser.once('exit', resolve));
  let socket, server;
  try {
    const portFile = join(profile, 'DevToolsActivePort');
    // CHROME'S FIRST LAUNCH IS SLOW ON A SHARED RUNNER. The port file appears once the browser
    // has built its caches, and on a loaded machine that takes far longer than the half second
    // the old budget allowed; the test then reported a browser that had never started. The
    // budget is thirty seconds now, and the wait still ends at once if the browser is gone.
    for (let attempt = 0; attempt < 600 && !existsSync(portFile) && browser.exitCode === null; attempt++) await delay(50);
    assert.ok(existsSync(portFile), 'Chromium must publish its owned debugging port');
    const port = Number(readFileSync(portFile, 'utf8').split('\n')[0]);
    // THE PORT FILE IS WRITTEN BEFORE THE HTTP ENDPOINT ACCEPTS, AND THE FIRST LIST CAN COME
    // BACK WITHOUT A PAGE TARGET. Both are the browser still coming up rather than a broken
    // browser, so the list is asked for again for a while before it is treated as a failure.
    let targets = [];
    for (let attempt = 0; attempt < 20; attempt++) {
      try {
        const listed = await (await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(5000) })).json();
        targets = Array.isArray(listed) ? listed : [];
        if (targets.some(t => t.type === 'page')) break;
      } catch (error) {
        if (attempt === 19) throw error;
      }
      await delay(250);
    }
    const target = targets.find(t => t.type === 'page');
    assert.ok(target, 'Chromium must expose its owned page target');
    socket = new WebSocket(target.webSocketDebuggerUrl);
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('Chromium websocket did not open')), 5000);
      socket.onopen = () => { clearTimeout(timer); resolve(); };
      socket.onerror = () => { clearTimeout(timer); reject(new Error('Chromium websocket failed')); };
    });
    let next = 0;
    const pending = new Map();
    socket.onmessage = event => {
      const message = JSON.parse(event.data);
      const request = pending.get(message.id);
      if (!request) return;
      pending.delete(message.id); clearTimeout(request.timer);
      if (message.error) request.reject(new Error('Chromium rejected capture protocol command'));
      else request.resolve(message.result);
    };
    const send = (method, params = {}) => new Promise((resolve, reject) => {
      const id = ++next;
      const timer = setTimeout(() => { pending.delete(id); reject(new Error(`Chromium timed out during ${method}`)); }, 5000);
      pending.set(id, { resolve, reject, timer });
      socket.send(JSON.stringify({ id, method, params }));
    });
    let documentUrl = `data:text/html;charset=utf-8,${encodeURIComponent(html)}`;
    if (loopback) {
      server = createServer((request, response) => {
        response.setHeader('content-type', 'text/html; charset=utf-8'); response.end(html);
      });
      await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
      documentUrl = 'http://127.0.0.1:' + server.address().port + '/';
    }
    await send('Page.enable');
    await send('Page.navigate', { url: documentUrl });
    let ready = false;
    for (let attempt = 0; attempt < 100; attempt++) {
      const response = await send('Runtime.evaluate', { expression: `location.href === ${JSON.stringify(documentUrl)} && document.readyState === 'complete'`, returnByValue: true });
      if (response.result.value === true) { ready = true; break; }
      await delay(20);
    }
    assert.ok(ready, 'Owned capture document must finish loading');
    const response = await send('Runtime.evaluate', { expression: `(${audit.captureDeviceUiSnapshot.toString()})()`, returnByValue: true });
    assert.equal(response.exceptionDetails, undefined, 'Serialized capture must evaluate without module closure dependencies');
    return response.result.value;
  } finally {
    socket?.close(); browser.kill();
    await Promise.race([exited, delay(3000, undefined, { ref: false })]);
    if (server) {
      server.closeAllConnections();
      await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
    }
  }
}


test('serialized DOM capture inherits secondary context while critical roles stay key', async () => {
  const s = await captureInChrome(`<main><div data-text-role="meta" class="secondary">release <span>version</span><span role="status">live</span><div role="alert"><span>alert</span></div><button><span>action</span></button><details open><summary><span>evidence</span></summary></details><div class="receipt"><span>receipt</span></div><div class="effects"><span>effect</span></div></div><p data-text-role="meta" class="secondary"><span>explanation</span></p><footer><span>footer <kbd>f</kbd></span><div><span>nested footer</span></div></footer><small><span>small context</span></small><p>main state</p></main>`);
  assert.equal(s.captureError, undefined);
  const category = suffix => s.texts.find(t => t.selector.endsWith(suffix))?.category;
  const root = 'main:nth-of-type(1)';
  assert.equal(category(`${root}>div:nth-of-type(1)`), 'meta', 'release container text is secondary');
  assert.equal(category(`${root}>div:nth-of-type(1)>span:nth-of-type(1)`), 'meta', 'version inherits explicit secondary context');
  assert.equal(category(`${root}>p:nth-of-type(1)>span:nth-of-type(1)`), 'meta', 'explanatory context is secondary');
  assert.equal(category('footer:nth-of-type(1)>span:nth-of-type(1)>kbd:nth-of-type(1)'), 'meta', 'keyboard hint inherits footer semantics');
  assert.equal(category('footer:nth-of-type(1)>div:nth-of-type(1)>span:nth-of-type(1)'), 'meta', 'footer semantics traverse intermediate containers');
  assert.equal(category('small:nth-of-type(1)>span:nth-of-type(1)'), 'meta', 'small descendants inherit metadata semantics');
  for (const suffix of ['span:nth-of-type(2)', 'div:nth-of-type(1)>span:nth-of-type(1)', 'button:nth-of-type(1)>span:nth-of-type(1)', 'details:nth-of-type(1)>summary:nth-of-type(1)>span:nth-of-type(1)', 'div:nth-of-type(2)>span:nth-of-type(1)', 'div:nth-of-type(3)>span:nth-of-type(1)']) {
    assert.equal(category(`${root}>div:nth-of-type(1)>${suffix}`), 'key', `critical descendant ${suffix} overrides secondary context`);
  }
  assert.equal(category(`${root}>p:nth-of-type(2)`), 'key', 'ordinary body text keeps the stricter policy');
  const serialized = JSON.stringify(s);
  for (const payload of ['explanation', 'nested footer', 'main state']) assert.equal(serialized.includes(payload), false, 'capture must remain sanitized');
  const checked = check(s);
  assert.equal(checked.failures.filter(f => f.check === 'key-text-size').length, 6, 'all six critical descendants still fail at 12px');
});

test('serialized DOM capture rejects clipped closed native navigation', async t => {
  // Real native closed disclosure and actual serialized capture/policy. This is extractor
  // infrastructure evidence, not a replacement for the Rust renderer's 45-case acceptance.
  const observations = [];
  for (const [name, navStyle, linkStyle, ancestorStyle] of [
    ['unclipped', '', '', ''],
    ['nav-inset-100', 'clip-path:inset(100%)', '', ''],
    ['link-circle-zero', '', 'clip-path:circle(0px)', ''],
    ['ancestor-legacy-clip', '', '', 'position:absolute;clip:rect(0px,1px,1px,0px)'],
  ]) {
    const s = await captureInChrome(`<a class="device-skip" href="#main">Skip</a><div style="${ancestorStyle}"><details class="local-mobile-menu"><summary style="min-height:40px">Menu</summary><nav class="local-nav" aria-label="Local navigation" style="${navStyle}"><a href="/panel" style="${linkStyle}">This machine</a><a href="/activity" aria-current="page">Activity</a></nav></details></div><main id="main" tabindex="-1"><p>Capture regression</p></main>`, { loopback: true });
    assert.equal(s.captureError, undefined, 'actual extractor must finish');
    const menu = s.navigationMenus[0];
    assert.equal(menu?.open, false, 'capture must not open the native disclosure');
    assert.deepEqual(s.nav, [], 'native-closed links cannot become visible navigation evidence');
    assert.deepEqual(menu.ownedNavigation.links.map(link => [link.path, link.current, link.visible]),
      [['/panel', false, false], ['/activity', true, false]], 'real origin/routes/current facts must reach the checker');
    const unclipped = menu.ownedNavigation.availability.unclipped
      && menu.ownedNavigation.links.every(link => link.availability.unclipped);
    const refused = has(check(s, { expectedRoute: '/activity' }), 'navigation');
    observations.push({ name, unclipped, refused });
    t.diagnostic(JSON.stringify({ name, unclipped, refused, nativeClosed: !menu.open }));
    // Collect all actual scenarios before asserting, so RED preserves each bad extraction.
  }
  assert.deepEqual(observations, [
    { name: 'unclipped', unclipped: true, refused: false },
    { name: 'nav-inset-100', unclipped: false, refused: true },
    { name: 'link-circle-zero', unclipped: false, refused: true },
    { name: 'ancestor-legacy-clip', unclipped: false, refused: true },
  ]);
});

test('closed native mobile navigation is a disclosure contract, never visible links', () => {
  const s = closedMenuSnapshot();
  const result = check(s, { expectedRoute: '/activity' });
  assert.equal(has(result, 'navigation'), false, 'a named native disclosure can expose navigation after opening');
  assert.equal(result.metrics.navigationMode, 'closed-native-menu');
  for (const key of ['ownsNav', 'summaryVisible', 'summaryNamed', 'summaryKeyboard']) {
    const bad = structuredClone(s); bad.navigationMenus[0][key] = false;
    assert.equal(has(check(bad), 'navigation'), true, key + ' must be verified');
  }
  const openWithoutLinks = structuredClone(s); openWithoutLinks.navigationMenus[0].open = true;
  assert.equal(has(check(openWithoutLinks), 'navigation'), true, 'opened disclosure must expose actual visible links');
  s.navigationMenus[0].summaryHeight = 31.99;
  assert.equal(has(check(s), 'navigation'), true, 'the disclosure keeps the 32px target minimum');
});

function closedMenuSnapshot() {
  const s = snapshot(); s.nav = [];
  s.navigationMenus = [{ selector: 'details:nth-of-type(1)', open: false, ownsNav: true,
    summaryVisible: true, summaryNamed: true, summaryKeyboard: true, summaryHeight: 40,
    ownedNavigation: { named: true, availability: availableNavigation(), links: [
      { path: '/panel', current: false, named: true, keyboard: true, visible: false, availability: availableNavigation() },
      { path: '/activity', current: true, named: true, keyboard: true, visible: false, availability: availableNavigation() },
    ] } }];
  return s;
}
function availableNavigation() {
  return { displayed: true, visibility: true, unhidden: true, uninert: true, ariaExposed: true,
    painted: true, unclipped: true, contentVisible: true, noOtherClosedDisclosure: true };
}

test('closed native menus reject empty owned routes and the wrong current route', () => {
  const empty = closedMenuSnapshot(); empty.navigationMenus[0].ownedNavigation.links = [];
  assert.equal(has(check(empty, { expectedRoute: '/activity' }), 'navigation'), true, 'a valid summary cannot substitute for actual owned links');
  assert.equal(has(check(closedMenuSnapshot(), { expectedRoute: '/panel' }), 'navigation'), true, 'closed fixture must check its own expected route');
  const duplicate = closedMenuSnapshot(); duplicate.navigationMenus[0].ownedNavigation.links[0].path = '/activity';
  assert.equal(has(check(duplicate), 'navigation'), true, 'both distinct local routes are required while closed');
  const twoCurrent = closedMenuSnapshot(); twoCurrent.navigationMenus[0].ownedNavigation.links[0].current = true;
  assert.equal(has(check(twoCurrent), 'navigation'), true, 'closed links retain exactly one current route');
});

test('closed native menus reject independent concealment and unusable owned links', () => {
  for (const owner of ['nav', 'link']) for (const key of Object.keys(availableNavigation())) {
    const s = closedMenuSnapshot(), nav = s.navigationMenus[0].ownedNavigation;
    (owner === 'nav' ? nav : nav.links[0]).availability[key] = false;
    assert.equal(has(check(s), 'navigation'), true, owner + ' independently unavailable: ' + key);
  }
  for (const key of ['named', 'keyboard']) {
    const s = closedMenuSnapshot(); s.navigationMenus[0].ownedNavigation.links[0][key] = false;
    assert.equal(has(check(s), 'navigation'), true, 'closed link ' + key + ' is required');
  }
  const malformed = closedMenuSnapshot(); delete malformed.navigationMenus[0].ownedNavigation.links[0].availability;
  assert.equal(has(check(malformed), 'snapshot'), true, 'missing capture facts must fail closed');
});

test('navigation checks exact active route and rejects hidden or nonkeyboard links', () => {
  assert.equal(has(check(snapshot(), { expectedRoute: '/activity' }), 'navigation'), false);
  assert.equal(has(check(snapshot(), { expectedRoute: '/panel' }), 'navigation'), true, 'one current route is insufficient if it is the wrong page');
  for (const key of ['named', 'keyboard', 'visible']) {
    const s = snapshot(); s.nav[0].links[0][key] = false;
    assert.equal(has(check(s), 'navigation'), true, key + ' must remain required');
  }
  const noNavigation = snapshot(); noNavigation.nav = [];
  assert.equal(has(check(noNavigation), 'navigation'), true, 'arbitrary missing or hidden navigation is not a disclosure');
  const malformed = snapshot(); malformed.navigationMenus = [{ open: false }];
  assert.equal(has(check(malformed), 'snapshot'), true, 'disclosure facts fail closed');
});

test('metadata font lower bound remains enforced after semantic classification', () => {
  const s = snapshot(); s.texts[0].category = 'meta'; s.texts[0].fontSize = 11.99;
  assert.equal(has(check(s), 'meta-text-size'), true);
});

// These are raw checker inputs, never a DOM substitute or browser/Windows evidence.
const layer = (backgroundColor = 'rgb(255, 255, 255)', opacity = 1) => ({ backgroundColor, opacity, backgroundImage: 'none' });
const text = (color = 'rgb(0, 0, 0)', layers = [layer()]) => ({ selector: 'body:nth-of-type(1)>p:nth-of-type(1)', fontSize: 13, category: 'key', color, layers });
function snapshot() {
  return {
    schemaVersion: 1, viewport: { width: 390, height: 900, scale: 1 },
    document: { scrollWidth: 390, clientWidth: 390, bodyScrollWidth: 390 },
    media: { forcedColors: false, reducedMotion: false },
    headings: [], landmarks: [{ selector: 'main:nth-of-type(1)', kind: 'main' }],
    nav: [{ selector: 'nav:nth-of-type(1)', kind: 'desktop', named: true, links: [{ path: '/activity', current: true, named: true, keyboard: true, visible: true }, { path: '/panel', current: false, named: true, keyboard: true, visible: true }] }],
    navigationMenus: [],
    skip: { exists: true, first: true, targetExists: true, targetMain: true, targetFocusable: true },
    controls: [], texts: [text()], panes: [], boxes: [], motions: [], focus: null,
  };
}
const check = (input, options) => {
  assert.equal(typeof audit.checkDeviceUiSnapshot, 'function', 'checker export must exist');
  return audit.checkDeviceUiSnapshot(input, options);
};
const has = (result, name) => result.failures.some(f => f.check === name);

test('fails closed on missing, malformed, truncated and nonfinite snapshot facts', () => {
  for (const bad of [null, {}, [], { ...snapshot(), texts: undefined }, { ...snapshot(), controls: [null] }, { ...snapshot(), viewport: { width: NaN, height: 900, scale: 1 } }, { ...snapshot(), captureError: 'failed' }]) {
    assert.equal(has(check(bad), 'snapshot'), true);
  }
});

test('allows no-goal states by default but enforces explicit heading expectations', () => {
  assert.deepEqual(check(snapshot()).failures, []);
  assert.equal(has(check(snapshot(), { expectedH1Count: 1 }), 'headings'), true);
  const s = snapshot(); s.headings = [{ selector: 'h1:nth-of-type(1)', level: 1 }, { selector: 'h1:nth-of-type(2)', level: 1 }];
  assert.equal(has(check(s), 'headings'), true);
});

test('computes WCAG sRGB luminance instead of averaging channels', () => {
  const s = snapshot(); s.texts = [text('rgb(255, 0, 0)')];
  const result = check(s);
  assert.equal(has(result, 'text-contrast'), true);
  assert.ok(Math.abs(result.metrics.minTextContrast - 3.99847677) < 0.000001);
});

test('composites translucent foregrounds and nested background/group opacity', () => {
  const s = snapshot(); s.texts = [text('rgba(0, 0, 0, 0.5)')];
  assert.ok(Math.abs(check(s).metrics.minTextContrast - 3.97665302) < 0.000001);
  s.texts = [text('rgb(0 0 0)', [layer('rgb(255 255 255)'), layer('rgba(255, 255, 255, 0.5)', 0.5)])];
  assert.ok(Math.abs(check(s).metrics.minTextContrast - 3.97665302) < 0.000001);
  s.texts = [text('color(srgb 0 0 0 / 100%)', [layer('color(srgb 1 1 1)')])];
  assert.equal(check(s).metrics.minTextContrast, 21);
});

test('uses unrounded 4.5 contrast threshold and distinct 3 focus threshold', () => {
  const s = snapshot(); s.texts = [text('rgb(118 118 118)')];
  assert.equal(has(check(s), 'text-contrast'), false);
  s.texts = [text('rgb(119 119 119)')];
  assert.equal(has(check(s), 'text-contrast'), true);
  s.texts = [text()];
  s.focus = { selector: 'button:nth-of-type(1)', visible: true, focusVisible: true, outlineStyle: 'solid', outlineWidth: 2, outlineOffset: 2, color: 'rgb(148 148 148)', layers: [layer()] };
  assert.equal(has(check(s, { checkFocus: true }), 'focus-contrast'), false);
  s.focus.color = 'rgb(149 149 149)';
  assert.equal(has(check(s, { checkFocus: true }), 'focus-contrast'), true);
});

test('rejects unknown color formats, alpha ranges, and image backgrounds instead of claiming contrast', () => {
  for (const color of ['lab(50 0 0)', 'rgba(0,0,0,2)', 'rgb(999 0 0)', 'secret-value']) {
    const s = snapshot(); s.texts = [text(color)];
    assert.equal(has(check(s), 'text-contrast-unmeasured'), true);
    assert.equal(JSON.stringify(check(s)).includes(color), false);
  }
  const s = snapshot(); s.texts[0].layers[0].backgroundImage = 'linear-gradient(white, black)';
  assert.equal(has(check(s), 'text-contrast-unmeasured'), true);
});

test('checks font and control boundaries and exempts bounded intentional scroll only', () => {
  const s = snapshot(); s.texts[0].fontSize = 12.99;
  assert.equal(has(check(s), 'key-text-size'), true);
  s.texts[0].category = 'meta'; s.texts[0].fontSize = 12;
  assert.equal(has(check(s), 'meta-text-size'), false);
  s.controls = [{ selector: 'button:nth-of-type(1)', kind: 'button', height: 31.9, named: true, visibleLabel: true, keyboard: true, ariaHidden: false, disabled: false }];
  assert.equal(has(check(s), 'control-size'), true);
  s.controls[0].height = 32;
  assert.equal(has(check(s), 'control-size'), false);
  s.panes = [{ selector: 'pre:nth-of-type(1)', clientWidth: 200, scrollWidth: 500, intentional: true, left: 16, right: 216, containerLeft: 16, containerRight: 374 }];
  assert.equal(has(check(s), 'pane-overflow'), false);
  s.panes[0].containerRight = 500;
  assert.equal(has(check(s), 'pane-overflow'), true);
  s.panes[0].containerRight = 374; s.panes[0].intentional = false;
  assert.equal(has(check(s), 'pane-overflow'), true);
});

test('checks labels, landmarks, skip, nav and positive tabindex without leaking supplied diagnostics', () => {
  const s = snapshot(); s.controls = [{ selector: 'secret-value', kind: 'input', height: 32, named: false, visibleLabel: false, keyboard: false, ariaHidden: true, disabled: false }];
  s.landmarks = []; s.skip.first = false; s.nav[0].links[1].current = true;
  const result = check(s);
  for (const name of ['control-name', 'input-label', 'keyboard', 'hidden-focus', 'main-landmark', 'skip-link', 'navigation']) assert.equal(has(result, name), true, name);
  assert.equal(JSON.stringify(result).includes('secret-value'), false);
});

test('detects text clipping from raw rectangles while allowing reachable vertical page content', () => {
  const s = snapshot();
  const box = { selector: 'p:nth-of-type(1)', left: 16, right: 374, limitLeft: 0, limitRight: 390, top: 2000, bottom: 2020, limitTop: null, limitBottom: null, intentional: false, containerLeft: 16, containerRight: 374 };
  s.boxes = [box];
  assert.equal(has(check(s), 'clipping'), false);
  box.right = 400;
  assert.equal(has(check(s), 'clipping'), true);
  box.right = 374; box.limitBottom = 2010;
  assert.equal(has(check(s), 'clipping'), true);
});

test('requires real emulation facts and visible solid keyboard focus when requested', () => {
  const s = snapshot();
  for (const [options, failure] of [[{ viewportWidth: 1280 }, 'viewport'], [{ forcedColors: true }, 'forced-colors'], [{ reducedMotion: true }, 'reduced-motion'], [{ zoom: 2 }, 'zoom'], [{ checkFocus: true }, 'focus']]) assert.equal(has(check(s, options), failure), true);
  s.media.reducedMotion = true;
  s.motions = [{ selector: 'button:nth-of-type(1)', transitionDurations: [0.12], animationDurations: [0], scrollBehavior: 'auto' }];
  assert.equal(has(check(s, { reducedMotion: true }), 'motion'), true);
});

test('rejects invalid options instead of weakening checks', () => {
  for (const options of [null, { expectedH1Count: -1 }, { viewportWidth: Infinity }, { checkFocus: 'yes' }, { zoom: 0 }]) assert.equal(has(check(snapshot(), options), 'options'), true);
});
