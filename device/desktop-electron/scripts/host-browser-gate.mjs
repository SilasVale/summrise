#!/usr/bin/env node
// Owned host/offline DOM fixture. The bridges below are test doubles; no Electron,
// device, native compositor, URL policy, physical desktop or DSH is exercised.
// Requires Console's locked Playwright and Chromium, explicitly installed by CI's console-web job.
// This browser fixture is separate from the self-contained mandatory Desktop node tests; missing tools fail.
import assert from 'node:assert/strict';
import { accessSync, closeSync, constants, existsSync, fsyncSync, mkdirSync, mkdtempSync, openSync, realpathSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { withAuthorPaintScope } from '../../../scripts/author-paint-scope.mjs';

const require = createRequire(new URL('../../console/web/package.json', import.meta.url));
const { chromium } = require('playwright');
const source = new URL('../src/', import.meta.url);
const option = name => {
  const index = process.argv.indexOf(name);
  return index < 0 ? undefined : process.argv[index + 1];
};
const only = option('--case') || 'all';
const caseNames = ['keyboard', 'layout', 'states', 'actions', 'poll', 'lifecycle', 'missing', 'offline'];
assert.ok(only === 'all' || caseNames.includes(only), `unknown case: ${only}`);
const requestedOut = option('--artifacts');
let out;
if (requestedOut) {
  const requested = path.resolve(requestedOut);
  const parent = realpathSync(path.dirname(requested));
  out = path.join(parent, path.basename(requested));
  assert.equal(existsSync(out), false, `artifact output must be fresh: ${out}`);
  mkdirSync(out);
} else {
  out = mkdtempSync(path.join(realpathSync(os.tmpdir()), 'summrise-host-browser-gate-'));
}
assert.equal(realpathSync(out), out, 'artifact output resolves to the fresh directory');
const outputParent = openSync(path.dirname(out), 'r');
try { fsyncSync(outputParent); } finally { closeSync(outputParent); }
function durableWrite(name, content) {
  const file = openSync(path.join(out, name), 'w', 0o600);
  try { writeFileSync(file, content); fsyncSync(file); } finally { closeSync(file); }
  const directory = openSync(out, 'r');
  try { fsyncSync(directory); } finally { closeSync(directory); }
}
durableWrite('node.pid', `${process.pid}\n`);
const results = [];
const evidence = { fixture: 'mock bridges / real host and offline DOM; no native compositor proof', output: out, results };
const save = () => durableWrite('results.json', `${JSON.stringify(evidence, null, 2)}\n`);

function installFixture({ missing = false, targets = ['http://127.0.0.1:19277/'] } = {}) {
  const clone = value => JSON.parse(JSON.stringify(value));
  const records = {
    browser: { url: '', title: '', visible: false, canBack: false, canFwd: false, load: null },
    harness: { url: '', title: '', visible: false, load: null },
  };
  const calls = [];
  const listeners = {};
  const timers = [];
  const held = {};
  const waiters = {};
  const log = (pane, method, args = []) => calls.push({ pane, method, args: clone(args) });
  const read = pane => {
    log(pane, 'state');
    if (held[pane]) return new Promise(resolve => (waiters[pane] ||= []).push(resolve));
    return Promise.resolve(clone(records[pane]));
  };
  const action = (pane, method, update) => (...args) => {
    log(pane, method, args);
    const answer = update?.(...args);
    return Promise.resolve(answer || { ok: true, url: records[pane].url });
  };
  const subscription = (pane, kind) => callback => {
    listeners[`${pane}:${kind}`] = callback;
    return () => { delete listeners[`${pane}:${kind}`]; };
  };
  // Drive the production polling callback deterministically; leave its guard and all promises real.
  window.setInterval = (callback, ms) => { timers.push({ callback, ms }); return timers.length; };
  window.__fixture = {
    calls,
    records,
    clear: () => { calls.length = 0; },
    set: (pane, record) => { Object.assign(records[pane], clone(record)); },
    paint: pane => listeners[`${pane}:gone`]?.(),
    nav: record => listeners['browser:nav']?.(record),
    tick: () => timers.forEach(timer => timer.callback()),
    hold: pane => { held[pane] = true; },
    release: pane => {
      held[pane] = false;
      (waiters[pane] || []).splice(0).forEach(resolve => resolve(clone(records[pane])));
    },
    pending: pane => (waiters[pane] || []).length,
  };
  if (missing) return;
  window.summriseBrowser = {
    navigate: action('browser', 'navigate', url => {
      records.browser.requested = url;
      return { ok: true, url };
    }),
    back: action('browser', 'back'),
    fwd: action('browser', 'fwd'),
    reload: action('browser', 'reload'),
    zoom: action('browser', 'zoom'),
    place: action('browser', 'place', bounds => { records.browser.visible = bounds !== null; }),
    state: () => read('browser'),
    recover: action('browser', 'recover'),
    onNav: subscription('browser', 'nav'),
    onGone: subscription('browser', 'gone'),
  };
  window.summriseHarness = {
    targets: () => { log('harness', 'targets'); return Promise.resolve({ ok: true, targets }); },
    open: action('harness', 'open', () => { records.harness.url = targets[0] || 'about:blank'; }),
    place: action('harness', 'place', bounds => { records.harness.visible = bounds !== null; }),
    go: action('harness', 'go', url => { records.harness.url = url; }),
    state: () => read('harness'),
    reload: action('harness', 'reload'),
    recover: action('harness', 'recover'),
    onGone: subscription('harness', 'gone'),
  };
}

let server;
let browser;
async function pageFixture(options = {}, viewport = { width: 1280, height: 900 }) {
  const context = await browser.newContext({ viewport, reducedMotion: 'reduce' });
  const external = [];
  await context.route(/^https?:/, route => { external.push(route.request().url()); return route.abort(); });
  await context.addInitScript(installFixture, options);
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  const allowed = new Set(['host.html', 'host.css', 'host.js'].map(name => new URL(name, source).href));
  const unknown = [];
  page.on('request', request => {
    const url = new URL(request.url());
    url.search = ''; url.hash = '';
    if (url.protocol === 'file:' && !allowed.has(url.href)) unknown.push(url.href);
  });
  await page.goto(new URL(`host.html${options.tab === 'harness' ? '?tab=harness' : ''}`, source).href);
  await page.waitForFunction(() => !!window.__fixture);
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  return { page, errors, context: { async close() {
    try {
      assert.deepEqual(external, [], 'host DOM makes no external requests');
      assert.deepEqual(unknown, [], 'every local host resource is explicitly owned');
      assert.deepEqual(errors, [], 'host scripts have no uncaught exceptions');
    } finally { await context.close(); }
  } } };
}
async function focusedControl(page, nativeSlotSelector = '') {
  // Read after real keyboard focus/scroll painting; this helper never scrolls the target itself.
  const { value: result, evidence: authorPaintScopeEvidence } = await withAuthorPaintScope(page, authorPaintScope =>
    page.evaluate(async ({ nativeSlotSelector, authorPaintScope }) => {
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const colorCanvas = new OffscreenCanvas(1, 1).getContext('2d', { willReadFrequently: true });
    const rgba = (value) => {
      // A rejected assignment retains fillStyle. Two different sentinels distinguish rejection
      // from a valid colour that happens to equal either sentinel.
      colorCanvas.fillStyle = '#010203';
      colorCanvas.fillStyle = value;
      const first = colorCanvas.fillStyle;
      colorCanvas.fillStyle = '#fefdfc';
      colorCanvas.fillStyle = value;
      const normalized = colorCanvas.fillStyle;
      if (first !== normalized) throw new Error(`contrast: invalid paint color ${value}`);
      // Read RGB opaquely so unpremultiplying an 8-bit alpha channel cannot distort the colour.
      let alpha = 1, opaque = normalized;
      const commaAlpha = /^rgba\(.+,\s*([\d.eE+-]+)(%?)\)$/.exec(normalized);
      const slashAlpha = /\/\s*([\d.eE+-]+)(%?)\s*\)$/.exec(normalized);
      if (commaAlpha || slashAlpha) {
        const match = commaAlpha || slashAlpha;
        alpha = Number(match[1]) / (match[2] === '%' ? 100 : 1);
        opaque = commaAlpha ? normalized.replace(/,[^,]+\)$/, ', 1)') : normalized.replace(/\/[^/]+\)$/, '/ 1)');
      }
      if (!Number.isFinite(alpha) || alpha < 0 || alpha > 1) throw new Error(`contrast: invalid paint color alpha ${value}`);
      colorCanvas.clearRect(0, 0, 1, 1);
      colorCanvas.fillStyle = opaque;
      colorCanvas.fillRect(0, 0, 1, 1);
      const [r, g, b, a] = colorCanvas.getImageData(0, 0, 1, 1).data;
      if (a !== 255) throw new Error(`contrast: invalid paint color serialization ${value}`);
      return { r, g, b, a: alpha };
    };
    const clear = { r: 0, g: 0, b: 0, a: 0 };
    const over = (front, back) => {
      const a = front.a + back.a * (1 - front.a);
      const channel = (key) => a === 0 ? 0 : (front[key] * front.a + back[key] * back.a * (1 - front.a)) / a;
      return { r: channel('r'), g: channel('g'), b: channel('b'), a };
    };
    // Only a globally proved absence of authored fill/all declarations can establish this UA mapping.
    let forcedFillProvenance = null;
    const proveDefaultTextFillProvenance = () => {
      if (forcedFillProvenance === true) return;
      const sheets = new Set(), roots = new Set(), animations = new Set();
      const reject = (reason) => { throw new Error(`contrast: unsupported paint text-fill provenance ${reason}`); };
      const inspectStyle = (declaration) => {
        if (!declaration || typeof declaration.getPropertyValue !== 'function') reject('unreadable declaration');
        if (declaration.getPropertyValue('-webkit-text-fill-color').trim() || declaration.getPropertyValue('all').trim()) reject('authored fill/all declaration');
      };
      const inspectRules = (rules) => {
        if (!rules) reject('unreadable rule list');
        for (const rule of rules) {
          if ('style' in rule && rule.style) inspectStyle(rule.style);
          if (rule.type === CSSRule.IMPORT_RULE) {
            if (!rule.styleSheet) reject('unresolved import');
            inspectSheet(rule.styleSheet);
          }
          if ('cssRules' in rule) inspectRules(rule.cssRules);
        }
      };
      const inspectSheet = (sheet) => {
        if (!sheet) reject('unresolved stylesheet');
        if (sheets.has(sheet)) return;
        sheets.add(sheet);
        try { inspectRules(sheet.cssRules); }
        catch (error) {
          if (error instanceof Error && error.message.startsWith('contrast: unsupported paint')) throw error;
          reject('inaccessible stylesheet');
        }
      };
      const inspectAnimations = (root) => {
        if (typeof root.getAnimations !== 'function') reject('unreadable animation scope');
        let list;
        try { list = root.getAnimations(); }
        catch { reject('inaccessible animations'); }
        if (!Array.isArray(list)) reject('unknown animation list');
        for (const animation of list) {
          if (animations.has(animation)) continue;
          animations.add(animation);
          let frames;
          try {
            if (animation.effect === null) continue;
            if (!animation.effect || typeof animation.effect.getKeyframes !== 'function') reject('unknown animation effect');
            frames = animation.effect.getKeyframes();
          } catch (error) {
            if (error instanceof Error && error.message.startsWith('contrast: unsupported paint')) throw error;
            reject('unreadable animation keyframes');
          }
          if (!Array.isArray(frames)) reject('unknown animation keyframes');
          for (const frame of frames) {
            if (!frame || typeof frame !== 'object' || Array.isArray(frame)) reject('unreadable animation frame');
            if (Object.keys(frame).some((key) => ['webkittextfillcolor', 'all'].includes(key.replace(/-/g, '').toLowerCase()))) reject('authored animation fill/all declaration');
          }
        }
      };
      const inspectRoot = (root) => {
        if (roots.has(root)) return;
        roots.add(root);
        inspectAnimations(root);
        if (root.styleSheets) for (const sheet of root.styleSheets) inspectSheet(sheet);
        if (root.adoptedStyleSheets) for (const sheet of root.adoptedStyleSheets) inspectSheet(sheet);
        for (const node of root.querySelectorAll('*')) {
          if (node.hasAttribute('style')) inspectStyle(node.style);
          if (node.matches('style, link[rel~="stylesheet"]')) {
            if (!node.sheet) reject('unresolved owner stylesheet');
            inspectSheet(node.sheet);
          }
          if (node.shadowRoot) inspectRoot(node.shadowRoot);
          else if (node instanceof HTMLElement && node.localName.includes('-')) reject('uninspectable custom-element scope');
        }
      };
      inspectRoot(document);
      forcedFillProvenance = true; // Cache only within this synchronous sampler invocation.
    };
    const provenForcedCurrentColor = (node, cs) => {
      if (!matchMedia('(forced-colors: active)').matches || cs.forcedColorAdjust !== 'auto' || typeof node.computedStyleMap !== 'function') return false;
      let typedColor, typedFill;
      try {
        const typed = node.computedStyleMap();
        typedColor = typed.get('color')?.toString();
        typedFill = typed.get('-webkit-text-fill-color')?.toString();
      } catch { throw new Error('contrast: unsupported paint unavailable typed text-fill provenance'); }
      if (!typedColor || typedColor !== typedFill) return false;
      const scope = typeof authorPaintScope === 'undefined' ? null : authorPaintScope;
      if (!scope || scope.version !== 1 || scope.method !== 'cdp-pierced-dom' || !Number.isSafeInteger(scope.sequence) || scope.sequence <= 0 || scope.documentRoot !== document.documentElement) throw new Error('contrast: unsupported paint text-fill provenance author scope missing or stale live document');
      proveDefaultTextFillProvenance();
      return true;
    };
    // Bounded shadow non-effect proof, not a colour/selector waiver. Unknown or intersecting paint fails.
    const paints = new WeakMap();
    const paint = (node) => {
      if (paints.has(node)) return paints.get(node);
      const cs = getComputedStyle(node);
      // These paints cannot be inferred from solid ancestral colours. A diagnostic is a failure,
      // never an omitted node or an invented opaque background.
      for (const [property, ordinary] of [['backgroundImage', 'none'], ['filter', 'none'], ['backdropFilter', 'none'],
        ['mixBlendMode', 'normal'], ['backgroundBlendMode', 'normal'], ['maskImage', 'none'],
        ['textShadow', 'none'], ['boxShadow', 'none']]) {
        const value = cs[property];
        if (value && value !== ordinary) throw new Error(`contrast: unsupported paint ${property}=${value} on ${node.tagName}.${node.className}`);
      }
      if (cs.backgroundClip.includes('text')) throw new Error('contrast: unsupported paint backgroundClip=text');
      if (cs.webkitTextFillColor && cs.webkitTextFillColor !== cs.color && !provenForcedCurrentColor(node, cs)) throw new Error(`contrast: unsupported paint webkitTextFillColor=${cs.webkitTextFillColor}`);
      const opacity = Number(cs.opacity);
      if (!Number.isFinite(opacity) || opacity < 0 || opacity > 1) throw new Error(`contrast: unsupported paint opacity=${cs.opacity}`);
      const value = { bg: rgba(cs.backgroundColor), opacity };
      paints.set(node, value);
      return value;
    };
    const compositePixels = (node, foreground, outside = false) => {
      const html = document.documentElement, body = document.body;
      // The root background (or propagated body background) paints the canvas, outside the
      // element opacity groups. Do not paint it a second time as a child-layer background.
      const htmlPaint = paint(html), bodyPaint = paint(body);
      const propagated = htmlPaint.bg.a === 0;
      const canvas = over(propagated ? bodyPaint.bg : htmlPaint.bg, { r: 255, g: 255, b: 255, a: 1 });
      let front = foreground, back = clear;
      for (let current = node; current; current = current.parentElement) {
        const own = paint(current);
        const bg = (outside && current === node) || current === html || (propagated && current === body) ? clear : own.bg;
        front = over(front, bg);
        back = over(back, bg);
        // CSS opacity is applied to the composited group, including its own background, before
        // that group is placed over its parent's background.
        front = { ...front, a: front.a * own.opacity };
        back = { ...back, a: back.a * own.opacity };
      }
      const finish = (pixel) => {
        const result = over(pixel, canvas);
        return { r: Math.round(result.r), g: Math.round(result.g), b: Math.round(result.b), a: 1 };
      };
      return { foreground: finish(front), background: finish(back) };
    };
    const lum = (c) => [c.r, c.g, c.b].map((v) => v / 255).map((v) => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4).reduce((sum, v, i) => sum + v * [.2126, .7152, .0722][i], 0);
    const contrast = (a, b) => (Math.max(lum(a), lum(b)) + .05) / (Math.min(lum(a), lum(b)) + .05);
    const element = document.activeElement, cs = getComputedStyle(element);
    if (!element.matches('a[href],button,input,select,textarea,summary')) throw new Error(`keyboard: active element is not an interactive target: ${element.tagName}`);
    const r = element.getBoundingClientRect();
    const rect = { x: r.x, y: r.y, right: r.right, bottom: r.bottom, width: r.width, height: r.height };
    const clip = { x: 0, y: 0, right: innerWidth, bottom: innerHeight };
    const unexpectedScroll = [];
    for (let parent = element.parentElement; parent; parent = parent.parentElement) {
      const style = getComputedStyle(parent), box = parent.getBoundingClientRect();
      if (['auto', 'scroll', 'hidden', 'clip'].includes(style.overflowX)) { clip.x = Math.max(clip.x, box.left + parent.clientLeft); clip.right = Math.min(clip.right, box.left + parent.clientLeft + parent.clientWidth); }
      if (['auto', 'scroll', 'hidden', 'clip'].includes(style.overflowY)) { clip.y = Math.max(clip.y, box.top + parent.clientTop); clip.bottom = Math.min(clip.bottom, box.top + parent.clientTop + parent.clientHeight); }
      if ((['hidden', 'clip'].includes(style.overflowY) && parent.scrollTop !== 0) || (['hidden', 'clip'].includes(style.overflowX) && parent.scrollLeft !== 0)) unexpectedScroll.push(parent.className || parent.tagName);
    }
    const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
    let slot = null;
    if (nativeSlotSelector) {
      const target = document.querySelector(nativeSlotSelector);
      if (!target) throw new Error(`keyboard: missing native slot ${nativeSlotSelector}`);
      const s = target.getBoundingClientRect(); slot = { x: s.x, y: s.y, right: s.right, bottom: s.bottom };
    }
    const intersects = (a, b) => Math.min(a.right, b.right) - Math.max(a.x, b.x) > 1 && Math.min(a.bottom, b.bottom) - Math.max(a.y, b.y) > 1;
    const width = parseFloat(cs.outlineWidth), offset = parseFloat(cs.outlineOffset);
    if (!Number.isFinite(offset) || offset < 0) throw new Error(`contrast: unsupported paint outlineOffset=${cs.outlineOffset}`);
    const outline = [
      { x: r.x - offset - width, right: r.right + offset + width, y: r.y - offset - width, bottom: r.y - offset, horizontal: true },
      { x: r.x - offset - width, right: r.right + offset + width, y: r.bottom + offset, bottom: r.bottom + offset + width, horizontal: true },
      { x: r.x - offset - width, right: r.x - offset, y: r.y - offset, bottom: r.bottom + offset, horizontal: false },
      { x: r.right + offset, right: r.right + offset + width, y: r.y - offset, bottom: r.bottom + offset, horizontal: false },
    ];
    // At least one full-thickness outline stroke must survive the actual clipping chain.
    const exposedOutline = outline.some(edge => {
      const x = Math.max(edge.x, clip.x), right = Math.min(edge.right, clip.right), y = Math.max(edge.y, clip.y), bottom = Math.min(edge.bottom, clip.bottom);
      if (edge.horizontal ? bottom - y < width - .01 || right <= x : right - x < width - .01 || bottom <= y) return false;
      return !slot || !intersects({ x, y, right, bottom }, slot);
    });
    const pixels = compositePixels(element, rgba(cs.outlineColor), true);
    const focusRatio = contrast(pixels.foreground, pixels.background);
    if (!Number.isFinite(focusRatio)) throw new Error('contrast: invalid measured focus ratio');
    return { id: element.id, name: element.getAttribute('aria-label') || element.textContent.trim().slice(0, 60), rect, clip, slot,
      hidden: cs.visibility === 'hidden' || !!element.closest('[hidden],[aria-hidden="true"]'), hit: element === hit || element.contains(hit),
      unexpectedScroll, slotIntersection: slot ? intersects(rect, slot) : false, focusVisible: element.matches(':focus-visible'),
      outline: { width, style: cs.outlineStyle, offset, exposed: exposedOutline, contrast: focusRatio },
      minimumHeight: (nativeSlotSelector ? innerWidth <= 640 : innerWidth < 640) ? 40 : 32 };
  }, { nativeSlotSelector, authorPaintScope }));
  const { rect, clip, outline } = result;
  assert.equal(result.hidden, false, `keyboard target stays visible: ${JSON.stringify(result)}`);
  assert.ok(rect.width > 0 && rect.height >= result.minimumHeight, `keyboard control keeps its target size: ${JSON.stringify(result)}`);
  assert.ok(rect.x >= clip.x - 1 && rect.right <= clip.right + 1 && rect.y >= clip.y - 1 && rect.bottom <= clip.bottom + 1, `real Tab stop is fully inside every clipping ancestor: ${JSON.stringify(result)}`);
  assert.deepEqual(result.unexpectedScroll, [], `real Tab cannot scroll a fixed hidden ancestor: ${JSON.stringify(result)}`);
  assert.ok(result.hit, `real Tab target receives pointer hits at its visible position: ${JSON.stringify(result)}`);
  assert.equal(result.slotIntersection, false, `native slot cannot cover the real Tab target: ${JSON.stringify(result)}`);
  assert.ok(result.focusVisible && outline.width >= 2 && outline.style === 'solid' && outline.exposed, `real Tab stop has an exposed 2px focus-visible outline: ${JSON.stringify(result)}`);
  assert.ok(outline.contrast >= 3, `real Tab stop outline contrasts at least 3:1: ${JSON.stringify(result)}`);
  return { ...result, authorPaintScopeEvidence };
}

async function readableText(page, selector, expected, nativeSlotSelector = '') {
  const initial = await page.evaluate(({ selector, expected, nativeSlotSelector }) => {
    const element = document.querySelector(selector);
    if (!element) throw new Error(`long text: missing ${selector}`);
    if (element.textContent !== expected || !expected.trim()) throw new Error(`long text: ${selector} does not retain the exact required content`);
    const fragments = [];
    const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      if (!node.textContent.trim()) continue;
      for (let parent = node.parentElement; parent; parent = parent.parentElement) {
        const cs = getComputedStyle(parent), r = parent.getBoundingClientRect();
        if (cs.display === 'none' || cs.visibility !== 'visible' || cs.contentVisibility === 'hidden' || Number(cs.opacity) === 0 || parent.matches('[hidden],[aria-hidden="true"]')) throw new Error(`long text: hidden content in ${selector}`);
        if (cs.webkitLineClamp && !['none', '0'].includes(cs.webkitLineClamp)) throw new Error(`long text: line clamp in ${selector}`);
        if (cs.textOverflow === 'ellipsis') throw new Error(`long text: ellipsis in ${selector}`);
        if (['hidden', 'clip'].includes(cs.overflowY) && parent.scrollHeight > parent.clientHeight + 1) throw new Error(`long text: non-scroll vertical clipping in ${selector}`);
        if (parent === element && (!(r.width > 0) || !(r.height > 0))) throw new Error(`long text: no visible text area in ${selector}`);
      }
      const range = document.createRange(); range.selectNodeContents(node);
      for (const r of range.getClientRects()) if (r.width > 0 && r.height > 0) fragments.push({ x: r.x, y: r.y, right: r.right, bottom: r.bottom });
    }
    if (!fragments.length) throw new Error(`long text: no rendered glyph lines in ${selector}`);
    let slot = null;
    if (nativeSlotSelector) {
      const node = document.querySelector(nativeSlotSelector);
      if (!node) throw new Error(`long text: missing ${nativeSlotSelector}`);
      const r = node.getBoundingClientRect(); slot = { x: r.x, y: r.y, width: r.width, height: r.height };
    }
    return { lineCount: fragments.length, slot };
  }, { selector, expected, nativeSlotSelector });
  if (nativeSlotSelector) assert.ok(initial.slot.width > 1 && initial.slot.height > 1, 'required text is read beside a positive native slot');
  const lines = [];
  for (let lineIndex = 0; lineIndex < initial.lineCount; lineIndex++) {
    const proof = await page.evaluate(async ({ selector, expected, lineIndex, lineCount, nativeSlotSelector }) => {
      // host.js may replace an unchanged textContent during its normal paint callback. Never retain
      // a text node or Range over a rendering boundary; rebuild from the live exact field each read.
      const readLive = () => {
        const element = document.querySelector(selector);
        if (!element || !element.isConnected || element.textContent !== expected) throw new Error(`long text: required live content changed while reading ${selector}`);
        const fragments = [];
        const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
        for (let node = walker.nextNode(); node; node = walker.nextNode()) {
          if (!node.textContent.trim()) continue;
          if (!node.isConnected || !element.contains(node)) throw new Error(`long text: disconnected live content in ${selector}`);
          const range = document.createRange(); range.selectNodeContents(node);
          for (const r of range.getClientRects()) if (r.width > 0 && r.height > 0) fragments.push({ node, rect: { x: r.x, y: r.y, right: r.right, bottom: r.bottom, width: r.width, height: r.height } });
        }
        if (fragments.length !== lineCount) throw new Error(`long text: live line count changed in ${selector}: ${fragments.length}, expected ${lineCount}`);
        const fragment = fragments[lineIndex];
        if (!fragment) throw new Error(`long text: missing live rendered line ${lineIndex} in ${selector}`);
        for (let parent = fragment.node.parentElement; parent; parent = parent.parentElement) {
          const cs = getComputedStyle(parent), r = parent.getBoundingClientRect();
          if (cs.display === 'none' || cs.visibility !== 'visible' || cs.contentVisibility === 'hidden' || Number(cs.opacity) === 0 || parent.matches('[hidden],[aria-hidden="true"]')) throw new Error(`long text: hidden live line ${lineIndex} in ${selector}`);
          if (cs.webkitLineClamp && !['none', '0'].includes(cs.webkitLineClamp)) throw new Error(`long text: clamped live line ${lineIndex} in ${selector}`);
          if (cs.textOverflow === 'ellipsis') throw new Error(`long text: ellipsized live line ${lineIndex} in ${selector}`);
          if (['hidden', 'clip'].includes(cs.overflowY) && parent.scrollHeight > parent.clientHeight + 1) throw new Error(`long text: non-scroll vertical clipping in ${selector}`);
          if (parent === element && (!(r.width > 0) || !(r.height > 0))) throw new Error(`long text: no live text area in ${selector}`);
        }
        return { element, ...fragment };
      };
      let live = readLive();
      const scrolled = [];
      // Host metadata may scroll only its recovery ASIDE. The offline page may scroll its document.
      // Never make a hidden ancestor scroll to rescue clipped text.
      for (let parent = live.node.parentElement; parent; parent = parent.parentElement) {
        if (!nativeSlotSelector && parent === document.scrollingElement) continue;
        const cs = getComputedStyle(parent);
        if (!['auto', 'scroll'].includes(cs.overflowY) || !nativeSlotSelector || !parent.matches('.recovery-panel')) continue;
        const box = parent.getBoundingClientRect(), r = readLive().rect;
        const top = box.top + parent.clientTop, bottom = top + parent.clientHeight;
        const delta = r.y < top ? r.y - top : r.bottom > bottom ? r.bottom - bottom : 0;
        if (delta) { parent.scrollTop += delta; scrolled.push(parent.className); }
      }
      if (!nativeSlotSelector) {
        const r = readLive().rect, delta = r.y < 0 ? r.y : r.bottom > innerHeight ? r.bottom - innerHeight : 0;
        if (delta) { document.scrollingElement.scrollTop += delta; scrolled.push('document'); }
      }
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      live = readLive();
      const { element, rect } = live, clip = { x: 0, y: 0, right: innerWidth, bottom: innerHeight }, unexpectedScroll = [];
      for (let parent = live.node.parentElement; parent; parent = parent.parentElement) {
        const cs = getComputedStyle(parent), box = parent.getBoundingClientRect();
        if (['auto', 'scroll', 'hidden', 'clip'].includes(cs.overflowX)) { clip.x = Math.max(clip.x, box.left + parent.clientLeft); clip.right = Math.min(clip.right, box.left + parent.clientLeft + parent.clientWidth); }
        if (['auto', 'scroll', 'hidden', 'clip'].includes(cs.overflowY)) { clip.y = Math.max(clip.y, box.top + parent.clientTop); clip.bottom = Math.min(clip.bottom, box.top + parent.clientTop + parent.clientHeight); }
        if ((['hidden', 'clip'].includes(cs.overflowY) && parent.scrollTop !== 0) || (['hidden', 'clip'].includes(cs.overflowX) && parent.scrollLeft !== 0)) unexpectedScroll.push(parent.className || parent.tagName);
      }
      const hit = document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2);
      let slotIntersection = false, slot = null;
      if (nativeSlotSelector) {
        const native = document.querySelector(nativeSlotSelector);
        if (!native) throw new Error(`long text: missing ${nativeSlotSelector}`);
        const r = native.getBoundingClientRect();
        slot = { x: r.x, y: r.y, width: r.width, height: r.height };
        slotIntersection = Math.min(rect.right, r.right) - Math.max(rect.x, r.x) > 1 && Math.min(rect.bottom, r.bottom) - Math.max(rect.y, r.y) > 1;
      }
      return { lineIndex, lineCount, rect, clip, scrolled, unexpectedScroll, hit: element === hit || element.contains(hit), slotIntersection, slot };
    }, { selector, expected, lineIndex, lineCount: initial.lineCount, nativeSlotSelector });
    const { rect, clip } = proof;
    assert.ok(rect.x >= clip.x - 1 && rect.right <= clip.right + 1 && rect.y >= clip.y - 1 && rect.bottom <= clip.bottom + 1, `complete required text line is reachable: ${selector}: ${JSON.stringify(proof)}`);
    assert.deepEqual(proof.unexpectedScroll, [], `required text cannot scroll a fixed hidden ancestor: ${selector}`);
    assert.ok(proof.hit, `required text line stays exposed to the reader: ${selector}: ${JSON.stringify(proof)}`);
    assert.equal(proof.slotIntersection, false, `native slot cannot cover required text: ${selector}`);
    if (nativeSlotSelector) {
      assert.ok(proof.slot.width > 1 && proof.slot.height > 1, 'native slot stays positive while every text line is read');
      assert.deepEqual(proof.slot, initial.slot, 'native slot keeps fixed coordinates while every text line is read');
      const pane = /^#(browser|harness)-slot$/.exec(nativeSlotSelector)?.[1];
      assert.ok(pane, 'required host text uses a known native pane');
      const placed = (await calls(page, pane, 'place')).at(-1)?.args[0];
      assert.deepEqual(placed, { x: Math.round(initial.slot.x), y: Math.round(initial.slot.y), width: Math.round(initial.slot.width), height: Math.round(initial.slot.height) }, 'bridge placement stays synchronized while every text line is read');
      proof.placed = placed;
    }
    lines.push(proof);
  }
  if (nativeSlotSelector) {
    const slot = await page.locator(nativeSlotSelector).evaluate(element => {
      const r = element.getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height };
    });
    assert.deepEqual(slot, initial.slot, 'reading every metadata line preserves fixed native slot coordinates');
  }
  return { selector, characters: expected.length, lineCount: initial.lineCount, lines };
}

async function calls(page, pane, method) {
  return page.evaluate(({ pane, method }) => window.__fixture.calls.filter(call => call.pane === pane && call.method === method), { pane, method });
}
async function publish(page, pane, record) {
  await page.evaluate(({ pane, record }) => { window.__fixture.set(pane, record); window.__fixture.paint(pane); }, { pane, record });
  const state = record.load?.status || (record.visible ? 'nopage' : 'hidden');
  await page.waitForFunction(({ pane, state }) => document.getElementById(`${pane}-status`).dataset.state === state, { pane, state });
}
async function boundsCheck(page, pane) {
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const measurements = await page.evaluate(pane => {
    const rect = element => {
      const r = element.getBoundingClientRect();
      return { x: r.x, y: r.y, right: r.right, bottom: r.bottom, width: r.width, height: r.height };
    };
    const effectivePaintRect = element => {
      const raw = rect(element), clip = { x: 0, y: 0, right: innerWidth, bottom: innerHeight }, ancestors = [];
      for (let node = element; node; node = node.parentElement) {
        const cs = getComputedStyle(node), box = node.getBoundingClientRect();
        // This sampler supports only the actual untransformed rectangular overflow model. Unknown
        // clipping is a diagnostic failure, never permission to pretend that a control is invisible.
        for (const [property, ordinary] of [['transform', 'none'], ['translate', 'none'], ['rotate', 'none'], ['scale', 'none'], ['perspective', 'none'], ['clipPath', 'none'], ['maskImage', 'none'], ['clip', 'auto'], ['contain', 'none'], ['contentVisibility', 'visible']]) {
          if (cs[property] && cs[property] !== ordinary) throw new Error(`layout: unsupported paint clipping ${property}=${cs[property]} on ${node.id || node.tagName}`);
        }
        if (!['1', 'normal'].includes(cs.zoom)) throw new Error(`layout: unsupported paint clipping zoom=${cs.zoom}`);
        for (const axis of ['overflowX', 'overflowY']) if (!['visible', 'auto', 'scroll', 'hidden', 'clip'].includes(cs[axis])) throw new Error(`layout: unsupported paint clipping ${axis}=${cs[axis]}`);
        if ((cs.overflowX === 'clip' || cs.overflowY === 'clip') && cs.overflowClipMargin !== '0px') throw new Error(`layout: unsupported overflowClipMargin=${cs.overflowClipMargin}`);
        if (node !== element) {
          if (['auto', 'scroll', 'hidden', 'clip'].includes(cs.overflowX)) { clip.x = Math.max(clip.x, box.left + node.clientLeft); clip.right = Math.min(clip.right, box.left + node.clientLeft + node.clientWidth); }
          if (['auto', 'scroll', 'hidden', 'clip'].includes(cs.overflowY)) { clip.y = Math.max(clip.y, box.top + node.clientTop); clip.bottom = Math.min(clip.bottom, box.top + node.clientTop + node.clientHeight); }
          if (cs.overflowX !== 'visible' || cs.overflowY !== 'visible') ancestors.push({ name: node.id || node.className || node.tagName, overflowX: cs.overflowX, overflowY: cs.overflowY, scrollTop: node.scrollTop, scrollLeft: node.scrollLeft, clientWidth: node.clientWidth, clientHeight: node.clientHeight });
        }
      }
      const paint = { x: Math.max(raw.x, clip.x), y: Math.max(raw.y, clip.y), right: Math.min(raw.right, clip.right), bottom: Math.min(raw.bottom, clip.bottom) };
      return { clip, paint: paint.right > paint.x && paint.bottom > paint.y ? paint : null, ancestors };
    };
    const box = document.getElementById(`${pane}-pane`);
    const recovery = box.querySelector('.recovery-panel');
    const controls = [...document.querySelectorAll('.host-sidebar button'), ...box.querySelectorAll('button, input, select')].map(element => ({ id: element.id, disabled: element.disabled, ...rect(element), ...effectivePaintRect(element) }));
    return {
      slot: rect(document.getElementById(`${pane}-slot`)), status: rect(document.getElementById(`${pane}-status`)), controls,
      stage: rect(box.querySelector('.viewport-column')), recovery: { ...rect(recovery), tag: recovery.tagName, label: recovery.getAttribute('aria-label'), overflow: getComputedStyle(recovery).overflowY },
      sidebar: rect(document.querySelector('.host-sidebar')), header: rect(document.querySelector('.utility-header')), main: document.querySelectorAll('main').length,
      heading: { count: [...document.querySelectorAll('h1')].filter(element => element.getClientRects().length).length, size: getComputedStyle(box.querySelector('h1')).fontSize, line: getComputedStyle(box.querySelector('h1')).lineHeight },
      width: innerWidth, height: innerHeight, scrollWidth: document.documentElement.scrollWidth,
      forcedColors: matchMedia('(forced-colors: active)').matches,
      theme: ['body', '.host-sidebar', '.utility-header', `#${pane}-pane .viewport-column`, `#${pane}-slot`, '[role="tab"][aria-selected="true"]'].map(selector => {
        const element = document.querySelector(selector), style = getComputedStyle(element);
        return { selector, background: style.backgroundColor, color: style.color, indicator: style.borderLeftColor };
      }),
    };
  }, pane);
  const expected = pane === 'browser'
    ? ['browser-back', 'browser-fwd', 'browser-reload', 'browser-url', 'browser-go', 'browser-zoom', 'browser-retry', 'browser-toggle', 'browser-recover']
    : ['harness-open', 'harness-target', 'harness-go', 'harness-reload', 'harness-retry', 'harness-toggle', 'harness-recover'];
  assert.deepEqual(measurements.controls.map(control => control.id).sort(), ['tab-browser', 'tab-harness', ...expected].sort(), `${pane}: every navigation, page and recovery control is present`);
  for (const [name, id] of [['Browser', 'tab-browser'], ['Harness', 'tab-harness']]) assert.equal(await page.getByRole('tab', { name, exact: true }).getAttribute('id'), id, `${name}: tool stays labelled at every rail size`);
  if (!measurements.forcedColors) {
    assert.equal(measurements.theme[0].background, 'rgb(243, 240, 232)');
    assert.equal(measurements.theme[0].color, 'rgb(29, 28, 26)');
    for (const plane of measurements.theme.slice(1, -1)) assert.equal(plane.background, 'rgb(251, 249, 244)', `${plane.selector}: warm paper plane`);
    assert.equal(measurements.theme.at(-1).background, 'rgb(246, 229, 216)', 'selected tool uses the warm selected plane');
    assert.equal(measurements.theme.at(-1).indicator, 'rgb(180, 69, 31)', 'selected tool retains a meaningful brand indicator');
  }
  assert.equal(measurements.main, 1);
  assert.equal(measurements.heading.count, 1, `${pane}: one visible page heading`);
  assert.equal(measurements.heading.size, '28px');
  assert.equal(measurements.heading.line, '36px');
  assert.equal(measurements.recovery.tag, 'ASIDE', `${pane}: recovery is independent of the native work plane`);
  assert.ok(measurements.recovery.label?.trim(), `${pane}: recovery has an accessible label`);
  assert.ok(['auto', 'scroll'].includes(measurements.recovery.overflow), `${pane}: recovery can scroll independently`);
  assert.ok(measurements.scrollWidth <= measurements.width, `${pane}: no horizontal page overflow: ${JSON.stringify(measurements)}`);
  const intersects = (a, b) => Math.min(a.right, b.right) - Math.max(a.x, b.x) > 1 && Math.min(a.bottom, b.bottom) - Math.max(a.y, b.y) > 1;
  assert.equal(intersects(measurements.stage, measurements.recovery), false, `${pane}: work plane and recovery do not overlap`);
  if (measurements.width >= 1280) {
    assert.equal(measurements.sidebar.width, 232, 'A3 desktop rail is 232px');
    assert.equal(measurements.header.height, 64, 'A3 desktop utility header is 64px');
  } else if (measurements.width === 960) {
    assert.equal(measurements.sidebar.width, 72, 'tablet keeps the compact labelled 72px tool rail');
    assert.equal(measurements.header.height, 64);
  }
  if (measurements.width >= 960) {
    assert.ok(measurements.stage.right <= measurements.recovery.x + 1, `${pane}: recovery sits beside the work plane`);
    assert.ok(measurements.stage.width > measurements.recovery.width, `${pane}: native work plane is primary`);
  } else {
    assert.ok(measurements.stage.bottom <= measurements.recovery.y + 1, `${pane}: recovery reflows below the work plane`);
  }
  assert.ok(measurements.slot.width > 1 && measurements.slot.height > 1, `${pane}: active native slot has usable area`);
  assert.ok(measurements.slot.x >= 0 && measurements.slot.y >= 0 && measurements.slot.right <= measurements.width + 1 && measurements.slot.bottom <= measurements.height + 1, `${pane}: native slot stays in the viewport`);
  for (let i = 0; i < measurements.controls.length; i++) for (const b of measurements.controls.slice(i + 1)) {
    const a = measurements.controls[i];
    assert.equal(!!a.paint && !!b.paint && intersects(a.paint, b.paint), false, `${a.id}/${b.id}: painted control regions do not overlap: ${JSON.stringify({ pane, viewport: { width: measurements.width, height: measurements.height }, a, b })}`);
  }
  const reachable = [];
  for (const control of measurements.controls) {
    // Scrolling a toolbar or recovery panel is intentional. Requiring every control above the
    // native slot would reject the new composition; require its full box reachable without cover.
    await page.locator(`#${control.id}`).scrollIntoViewIfNeeded();
    const visible = await page.locator(`#${control.id}`).evaluate((element, pane) => {
      const r = element.getBoundingClientRect();
      const clip = { x: 0, y: 0, right: innerWidth, bottom: innerHeight };
      const unexpectedScroll = [];
      for (let parent = element.parentElement; parent; parent = parent.parentElement) {
        const style = getComputedStyle(parent), box = parent.getBoundingClientRect();
        if (['auto', 'scroll', 'hidden', 'clip'].includes(style.overflowX)) { clip.x = Math.max(clip.x, box.left + parent.clientLeft); clip.right = Math.min(clip.right, box.left + parent.clientLeft + parent.clientWidth); }
        if (['auto', 'scroll', 'hidden', 'clip'].includes(style.overflowY)) { clip.y = Math.max(clip.y, box.top + parent.clientTop); clip.bottom = Math.min(clip.bottom, box.top + parent.clientTop + parent.clientHeight); }
        if (['hidden', 'clip'].includes(style.overflowY) && parent.scrollTop !== 0) unexpectedScroll.push(parent.className || parent.tagName);
      }
      const slot = document.getElementById(`${pane}-slot`).getBoundingClientRect();
      const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
      return { id: element.id, x: r.x, y: r.y, right: r.right, bottom: r.bottom, width: r.width, height: r.height, clip, unexpectedScroll,
        hit: element === hit || element.contains(hit), slot: { x: slot.x, y: slot.y, right: slot.right, bottom: slot.bottom } };
    }, pane);
    const minimum = measurements.width <= 640 ? 40 : 32;
    assert.ok(visible.height >= minimum, `${control.id}: ${minimum}px control at this viewport`);
    assert.ok(visible.width > 0 && visible.x >= visible.clip.x - 1 && visible.right <= visible.clip.right + 1 && visible.y >= visible.clip.y - 1 && visible.bottom <= visible.clip.bottom + 1, `${control.id}: full control is reachable through its actual scroll area: ${JSON.stringify(visible)}`);
    assert.deepEqual(visible.unexpectedScroll, [], `${control.id}: fixed work plane cannot mask clipping by scrolling a hidden ancestor`);
    assert.ok(visible.hit, `${control.id}: reachable control receives pointer hits`);
    assert.equal(intersects(visible, visible.slot), false, `${control.id}: native slot cannot cover this control`);
    reachable.push(visible);
  }
  // Exercise the real tab order as well as geometric scroll reachability. Disabled controls remain
  // visible for state explanation and are deliberately omitted from keyboard activation.
  await page.keyboard.press('Tab');
  await page.locator(`#tab-${pane}`).focus();
  const reached = new Set(), keyboardProof = [];
  const enabled = measurements.controls.filter(control => !control.disabled && control.id !== `tab-${pane === 'browser' ? 'harness' : 'browser'}`);
  for (let index = 0; index < enabled.length; index++) {
    const focused = await focusedControl(page, `#${pane}-slot`);
    assert.deepEqual(focused.slot, { x: measurements.slot.x, y: measurements.slot.y, right: measurements.slot.right, bottom: measurements.slot.bottom }, `${focused.id}: native slot keeps fixed coordinates at every actual Tab stop`);
    const placed = (await calls(page, pane, 'place')).at(-1)?.args[0];
    assert.deepEqual(placed, { x: Math.round(measurements.slot.x), y: Math.round(measurements.slot.y), width: Math.round(measurements.slot.width), height: Math.round(measurements.slot.height) }, `${focused.id}: bridge placement remains synchronized at every actual Tab stop`);
    reached.add(focused.id); keyboardProof.push({ ...focused, placed });
    await page.keyboard.press('Tab');
  }
  for (const control of enabled) assert.ok(reached.has(control.id), `${control.id}: real Tab order reaches every enabled active-pane control`);
  await page.locator(`#tab-${pane}`).focus();
  const slot = await page.locator(`#${pane}-slot`).evaluate(element => {
    const r = element.getBoundingClientRect();
    return { x: r.x, y: r.y, width: r.width, height: r.height };
  });
  const last = (await calls(page, pane, 'place')).at(-1)?.args[0];
  assert.deepEqual(last, { x: Math.round(slot.x), y: Math.round(slot.y), width: Math.round(slot.width), height: Math.round(slot.height) }, `${pane}: last bridge bounds follow actual DOM reflow`);
  assert.equal(slot.x, measurements.slot.x, `${pane}: reaching support controls does not move the native slot`);
  assert.equal(slot.y, measurements.slot.y, `${pane}: native slot keeps fixed viewport coordinates`);
  return { ...measurements, reachable, keyboard: [...reached], keyboardProof };
}

async function renderedAccess(page) {
  const { value: result, evidence: authorPaintScopeEvidence } = await withAuthorPaintScope(page, authorPaintScope =>
    page.evaluate(({ authorPaintScope } = {}) => {
    const colorCanvas = new OffscreenCanvas(1, 1).getContext('2d', { willReadFrequently: true });
    const rgba = (value) => {
      // A rejected assignment retains fillStyle. Two different sentinels distinguish rejection
      // from a valid colour that happens to equal either sentinel.
      colorCanvas.fillStyle = '#010203';
      colorCanvas.fillStyle = value;
      const first = colorCanvas.fillStyle;
      colorCanvas.fillStyle = '#fefdfc';
      colorCanvas.fillStyle = value;
      const normalized = colorCanvas.fillStyle;
      if (first !== normalized) throw new Error(`contrast: invalid paint color ${value}`);
      // Read RGB opaquely so unpremultiplying an 8-bit alpha channel cannot distort the colour.
      let alpha = 1, opaque = normalized;
      const commaAlpha = /^rgba\(.+,\s*([\d.eE+-]+)(%?)\)$/.exec(normalized);
      const slashAlpha = /\/\s*([\d.eE+-]+)(%?)\s*\)$/.exec(normalized);
      if (commaAlpha || slashAlpha) {
        const match = commaAlpha || slashAlpha;
        alpha = Number(match[1]) / (match[2] === '%' ? 100 : 1);
        opaque = commaAlpha ? normalized.replace(/,[^,]+\)$/, ', 1)') : normalized.replace(/\/[^/]+\)$/, '/ 1)');
      }
      if (!Number.isFinite(alpha) || alpha < 0 || alpha > 1) throw new Error(`contrast: invalid paint color alpha ${value}`);
      colorCanvas.clearRect(0, 0, 1, 1);
      colorCanvas.fillStyle = opaque;
      colorCanvas.fillRect(0, 0, 1, 1);
      const [r, g, b, a] = colorCanvas.getImageData(0, 0, 1, 1).data;
      if (a !== 255) throw new Error(`contrast: invalid paint color serialization ${value}`);
      return { r, g, b, a: alpha };
    };
    const clear = { r: 0, g: 0, b: 0, a: 0 };
    const over = (front, back) => {
      const a = front.a + back.a * (1 - front.a);
      const channel = (key) => a === 0 ? 0 : (front[key] * front.a + back[key] * back.a * (1 - front.a)) / a;
      return { r: channel('r'), g: channel('g'), b: channel('b'), a };
    };
    // Only a globally proved absence of authored fill/all declarations can establish this UA mapping.
    let forcedFillProvenance = null;
    const proveDefaultTextFillProvenance = () => {
      if (forcedFillProvenance === true) return;
      const sheets = new Set(), roots = new Set(), animations = new Set();
      const reject = (reason) => { throw new Error(`contrast: unsupported paint text-fill provenance ${reason}`); };
      const inspectStyle = (declaration) => {
        if (!declaration || typeof declaration.getPropertyValue !== 'function') reject('unreadable declaration');
        if (declaration.getPropertyValue('-webkit-text-fill-color').trim() || declaration.getPropertyValue('all').trim()) reject('authored fill/all declaration');
      };
      const inspectRules = (rules) => {
        if (!rules) reject('unreadable rule list');
        for (const rule of rules) {
          if ('style' in rule && rule.style) inspectStyle(rule.style);
          if (rule.type === CSSRule.IMPORT_RULE) {
            if (!rule.styleSheet) reject('unresolved import');
            inspectSheet(rule.styleSheet);
          }
          if ('cssRules' in rule) inspectRules(rule.cssRules);
        }
      };
      const inspectSheet = (sheet) => {
        if (!sheet) reject('unresolved stylesheet');
        if (sheets.has(sheet)) return;
        sheets.add(sheet);
        try { inspectRules(sheet.cssRules); }
        catch (error) {
          if (error instanceof Error && error.message.startsWith('contrast: unsupported paint')) throw error;
          reject('inaccessible stylesheet');
        }
      };
      const inspectAnimations = (root) => {
        if (typeof root.getAnimations !== 'function') reject('unreadable animation scope');
        let list;
        try { list = root.getAnimations(); }
        catch { reject('inaccessible animations'); }
        if (!Array.isArray(list)) reject('unknown animation list');
        for (const animation of list) {
          if (animations.has(animation)) continue;
          animations.add(animation);
          let frames;
          try {
            if (animation.effect === null) continue;
            if (!animation.effect || typeof animation.effect.getKeyframes !== 'function') reject('unknown animation effect');
            frames = animation.effect.getKeyframes();
          } catch (error) {
            if (error instanceof Error && error.message.startsWith('contrast: unsupported paint')) throw error;
            reject('unreadable animation keyframes');
          }
          if (!Array.isArray(frames)) reject('unknown animation keyframes');
          for (const frame of frames) {
            if (!frame || typeof frame !== 'object' || Array.isArray(frame)) reject('unreadable animation frame');
            if (Object.keys(frame).some((key) => ['webkittextfillcolor', 'all'].includes(key.replace(/-/g, '').toLowerCase()))) reject('authored animation fill/all declaration');
          }
        }
      };
      const inspectRoot = (root) => {
        if (roots.has(root)) return;
        roots.add(root);
        inspectAnimations(root);
        if (root.styleSheets) for (const sheet of root.styleSheets) inspectSheet(sheet);
        if (root.adoptedStyleSheets) for (const sheet of root.adoptedStyleSheets) inspectSheet(sheet);
        for (const node of root.querySelectorAll('*')) {
          if (node.hasAttribute('style')) inspectStyle(node.style);
          if (node.matches('style, link[rel~="stylesheet"]')) {
            if (!node.sheet) reject('unresolved owner stylesheet');
            inspectSheet(node.sheet);
          }
          if (node.shadowRoot) inspectRoot(node.shadowRoot);
          else if (node instanceof HTMLElement && node.localName.includes('-')) reject('uninspectable custom-element scope');
        }
      };
      inspectRoot(document);
      forcedFillProvenance = true; // Cache only within this synchronous sampler invocation.
    };
    const provenForcedCurrentColor = (node, cs) => {
      if (!matchMedia('(forced-colors: active)').matches || cs.forcedColorAdjust !== 'auto' || typeof node.computedStyleMap !== 'function') return false;
      let typedColor, typedFill;
      try {
        const typed = node.computedStyleMap();
        typedColor = typed.get('color')?.toString();
        typedFill = typed.get('-webkit-text-fill-color')?.toString();
      } catch { throw new Error('contrast: unsupported paint unavailable typed text-fill provenance'); }
      if (!typedColor || typedColor !== typedFill) return false;
      const scope = typeof authorPaintScope === 'undefined' ? null : authorPaintScope;
      if (!scope || scope.version !== 1 || scope.method !== 'cdp-pierced-dom' || !Number.isSafeInteger(scope.sequence) || scope.sequence <= 0 || scope.documentRoot !== document.documentElement) throw new Error('contrast: unsupported paint text-fill provenance author scope missing or stale live document');
      proveDefaultTextFillProvenance();
      return true;
    };
    // Bounded shadow non-effect proof, not a colour/selector waiver. Unknown or intersecting paint fails.
    const paints = new WeakMap();
    const paint = (node) => {
      if (paints.has(node)) return paints.get(node);
      const cs = getComputedStyle(node);
      // These paints cannot be inferred from solid ancestral colours. A diagnostic is a failure,
      // never an omitted node or an invented opaque background.
      for (const [property, ordinary] of [['backgroundImage', 'none'], ['filter', 'none'], ['backdropFilter', 'none'],
        ['mixBlendMode', 'normal'], ['backgroundBlendMode', 'normal'], ['maskImage', 'none'],
        ['textShadow', 'none'], ['boxShadow', 'none']]) {
        const value = cs[property];
        if (value && value !== ordinary) throw new Error(`contrast: unsupported paint ${property}=${value} on ${node.tagName}.${node.className}`);
      }
      if (cs.backgroundClip.includes('text')) throw new Error('contrast: unsupported paint backgroundClip=text');
      if (cs.webkitTextFillColor && cs.webkitTextFillColor !== cs.color && !provenForcedCurrentColor(node, cs)) throw new Error(`contrast: unsupported paint webkitTextFillColor=${cs.webkitTextFillColor}`);
      const opacity = Number(cs.opacity);
      if (!Number.isFinite(opacity) || opacity < 0 || opacity > 1) throw new Error(`contrast: unsupported paint opacity=${cs.opacity}`);
      const value = { bg: rgba(cs.backgroundColor), opacity };
      paints.set(node, value);
      return value;
    };
    const compositePixels = (node, foreground, outside = false) => {
      const html = document.documentElement, body = document.body;
      // The root background (or propagated body background) paints the canvas, outside the
      // element opacity groups. Do not paint it a second time as a child-layer background.
      const htmlPaint = paint(html), bodyPaint = paint(body);
      const propagated = htmlPaint.bg.a === 0;
      const canvas = over(propagated ? bodyPaint.bg : htmlPaint.bg, { r: 255, g: 255, b: 255, a: 1 });
      let front = foreground, back = clear;
      for (let current = node; current; current = current.parentElement) {
        const own = paint(current);
        const bg = (outside && current === node) || current === html || (propagated && current === body) ? clear : own.bg;
        front = over(front, bg);
        back = over(back, bg);
        // CSS opacity is applied to the composited group, including its own background, before
        // that group is placed over its parent's background.
        front = { ...front, a: front.a * own.opacity };
        back = { ...back, a: back.a * own.opacity };
      }
      const finish = (pixel) => {
        const result = over(pixel, canvas);
        return { r: Math.round(result.r), g: Math.round(result.g), b: Math.round(result.b), a: 1 };
      };
      return { foreground: finish(front), background: finish(back) };
    };
    const lum = (c) => [c.r, c.g, c.b].map((v) => v / 255).map((v) => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4).reduce((sum, v, i) => sum + v * [.2126, .7152, .0722][i], 0);
    const contrast = (a, b) => (Math.max(lum(a), lum(b)) + .05) / (Math.min(lum(a), lum(b)) + .05);
    const failures = [], clipped = [], small = [], motion = [];
    let textPairs = 0, controlPairs = 0, minimum = 21;
    const visible = [...document.querySelectorAll('body *')].filter(element => {
      const r = element.getBoundingClientRect(), style = getComputedStyle(element);
      return r.width > 1 && r.height > 1 && style.visibility !== 'hidden' && !element.closest('[aria-hidden="true"]');
    });
    for (const element of visible) {
      const cs = getComputedStyle(element), name = element.id || element.tagName.toLowerCase();
      paint(element);
      const box = element.getBoundingClientRect();
      if (box.x < -1 || box.right > innerWidth + 1) clipped.push(`${name}: outside horizontal viewport`);
      if (element.clientWidth > 0 && element.scrollWidth > element.clientWidth + 1 && ['hidden', 'clip'].includes(cs.overflowX) && !element.matches('input,select')) clipped.push(`${name}: text clipped horizontally`);
      const durations = `${cs.animationDuration},${cs.transitionDuration}`.split(',').map(parseFloat);
      if (matchMedia('(prefers-reduced-motion: reduce)').matches && durations.some(duration => duration > 0)) motion.push(name);
      let text = [...element.childNodes].filter(node => node.nodeType === Node.TEXT_NODE).map(node => node.textContent.trim()).join('');
      let foreground = rgba(cs.color);
      if (element.matches('input')) {
        text = element.value || element.placeholder;
        if (!element.value && element.placeholder) {
          const placeholder = getComputedStyle(element, '::placeholder'), opacity = Number(placeholder.opacity);
          if (!Number.isFinite(opacity) || opacity < 0 || opacity > 1) throw new Error(`contrast: unsupported placeholder opacity=${placeholder.opacity}`);
          foreground = rgba(placeholder.color); foreground.a *= opacity;
        }
      } else if (element.matches('select')) text = element.selectedOptions[0]?.textContent.trim();
      if (text && !element.matches(':disabled')) {
        const pixels = compositePixels(element, foreground), ratio = contrast(pixels.foreground, pixels.background);
        if (!Number.isFinite(ratio)) throw new Error('contrast: invalid measured text ratio');
        const size = parseFloat(cs.fontSize), bold = Number(cs.fontWeight) >= 700;
        textPairs++; minimum = Math.min(minimum, ratio);
        if (size < 12) small.push(`${name}:${size}px`);
        if (ratio < (size >= 24 || (bold && size >= 18.66) ? 3 : 4.5)) failures.push(`${name}: text ${ratio.toFixed(2)}:1`);
      }
      if (element.matches('button:not([role="tab"]), input, select') && !element.disabled) {
        const fill = rgba(cs.backgroundColor), fillPixels = compositePixels(element, fill, true);
        let ratio = contrast(fillPixels.foreground, fillPixels.background);
        if (parseFloat(cs.borderTopWidth) > 0 && !['none', 'hidden'].includes(cs.borderTopStyle)) {
          if (cs.backgroundClip !== 'border-box') throw new Error(`contrast: unsupported control backgroundClip=${cs.backgroundClip}`);
          const pixels = compositePixels(element, over(rgba(cs.borderTopColor), fill), true);
          ratio = Math.max(ratio, contrast(pixels.foreground, pixels.background));
        }
        if (!Number.isFinite(ratio)) throw new Error('contrast: invalid measured control ratio');
        controlPairs++;
        if (ratio < 3) failures.push(`${name}: control boundary ${ratio.toFixed(2)}:1`);
      }
    }
    const active = document.activeElement, focusStyle = getComputedStyle(active);
    let focusContrast = null;
    if (!['none', 'hidden'].includes(focusStyle.outlineStyle) && parseFloat(focusStyle.outlineWidth) >= 2) {
      if (parseFloat(focusStyle.outlineOffset) < 0) throw new Error(`contrast: unsupported paint negative outlineOffset=${focusStyle.outlineOffset}`);
      const pixels = compositePixels(active, rgba(focusStyle.outlineColor), true);
      focusContrast = contrast(pixels.foreground, pixels.background);
      if (!Number.isFinite(focusContrast)) throw new Error('contrast: invalid measured focus ratio');
    }
    return { textPairs, controlPairs, minimum, failures, clipped, small, motion, focusContrast,
      reducedMotion: matchMedia('(prefers-reduced-motion: reduce)').matches, forcedColors: matchMedia('(forced-colors: active)').matches };
  }, { authorPaintScope }));
  for (const key of ['failures', 'clipped', 'small', 'motion']) assert.deepEqual(result[key], [], `${key}: ${JSON.stringify(result)}`);
  assert.ok(result.textPairs >= 10, `real rendered text pairs: ${JSON.stringify(result)}`);
  assert.ok(result.controlPairs >= 1, 'at least one actual enabled control boundary was measured');
  assert.ok(result.focusContrast >= 3, `visible focus contrasts at least 3:1 with its surrounding plane: ${JSON.stringify(result)}`);
  assert.equal(result.reducedMotion, true);
  return { ...result, authorPaintScopeEvidence };
}

async function offlineLayout(page) {
  const result = await page.evaluate(() => {
    const rect = selector => {
      const element = document.querySelector(selector), r = element.getBoundingClientRect();
      return { x: r.x, y: r.y, right: r.right, bottom: r.bottom, width: r.width, height: r.height, tag: element.tagName, background: getComputedStyle(element).backgroundColor };
    };
    return { width: innerWidth, scroll: document.documentElement.scrollWidth, sidebar: rect('.offline-sidebar'), header: rect('.utility-header'), primary: rect('.requested-device'), support: rect('.recovery-help'),
      canvas: getComputedStyle(document.body).backgroundColor, ink: getComputedStyle(document.body).color,
      h1: document.querySelectorAll('h1').length, main: document.querySelectorAll('main').length, heading: getComputedStyle(document.querySelector('h1')).fontSize,
      retryInPrimary: !!document.getElementById('retry').closest('.requested-device'), forcedColors: matchMedia('(forced-colors: active)').matches,
      overflow: [getComputedStyle(document.body).overflowX, getComputedStyle(document.documentElement).overflowX] };
  });
  assert.equal(result.h1, 1); assert.equal(result.main, 1); assert.equal(result.heading, '28px');
  assert.ok(result.scroll <= result.width, 'offline recovery fits the viewport');
  assert.ok(result.overflow.every(value => !['hidden', 'clip'].includes(value)), 'offline recovery cannot conceal horizontal overflow');
  assert.equal(result.retryInPrimary, true, 'Retry belongs to the requested-device recovery action');
  assert.equal(result.support.tag, 'ASIDE', 'automatic recovery guidance is independent support');
  assert.ok(result.primary.width > 0 && result.support.width > 0 && result.primary.height > 0 && result.support.height > 0, 'both recovery areas contain real content');
  if (result.width >= 1280) {
    assert.equal(result.sidebar.width, 232); assert.equal(result.header.height, 64);
    assert.ok(result.primary.right <= result.support.x + 1 && result.primary.width > result.support.width, 'requested-device action is the primary desktop area beside support');
  } else if (result.width <= 640) assert.ok(result.primary.bottom <= result.support.y + 1, 'phone recovery support follows the primary action');
  if (!result.forcedColors) {
    assert.equal(result.canvas, 'rgb(243, 240, 232)'); assert.equal(result.ink, 'rgb(29, 28, 26)');
    for (const key of ['sidebar', 'header', 'primary']) assert.equal(result[key].background, 'rgb(251, 249, 244)', `offline ${key}: warm paper plane`);
  }
  return result;
}

const cases = {
  async keyboard() {
    const { page, context, errors } = await pageFixture();
    try {
      assert.equal(await page.locator('[role=tablist]').getAttribute('aria-orientation'), 'vertical');
      assert.deepEqual(await page.locator('[role=tab]').evaluateAll(tabs => tabs.map(tab => tab.tabIndex)), [0, -1], 'only the active Browser tab is in the tab order');
      await page.locator('#tab-browser').focus();
      await page.evaluate(() => window.__fixture.clear());
      await page.keyboard.press('ArrowRight');
      await page.waitForFunction(() => document.activeElement.id === 'tab-harness' && document.getElementById('tab-harness').getAttribute('aria-selected') === 'true');
      assert.deepEqual(await page.locator('[role=tab]').evaluateAll(tabs => tabs.map(tab => tab.tabIndex)), [-1, 0]);
      const switched = await page.evaluate(() => window.__fixture.calls.filter(call => call.method === 'place'));
      assert.deepEqual(switched[0], { pane: 'browser', method: 'place', args: [null] }, 'outgoing view hides before incoming placement');
      await page.keyboard.press('Home');
      await page.waitForFunction(() => document.activeElement.id === 'tab-browser' && !document.getElementById('browser-pane').hidden);
      await page.keyboard.press('End');
      await page.waitForFunction(() => document.activeElement.id === 'tab-harness' && !document.getElementById('harness-pane').hidden);
      await page.keyboard.press('ArrowRight');
      await page.waitForFunction(() => document.activeElement.id === 'tab-browser');
      await page.keyboard.press('ArrowLeft');
      await page.waitForFunction(() => document.activeElement.id === 'tab-harness');
      await page.keyboard.press('ArrowUp');
      await page.waitForFunction(() => document.activeElement.id === 'tab-browser' && document.getElementById('tab-browser').getAttribute('aria-selected') === 'true');
      await page.keyboard.press('ArrowDown');
      await page.waitForFunction(() => document.activeElement.id === 'tab-harness' && document.getElementById('tab-harness').getAttribute('aria-selected') === 'true');
      await page.keyboard.press('Tab');
      assert.ok(await page.locator('#harness-pane').evaluate(pane => pane.contains(document.activeElement)), 'Tab reaches controls in the active pane');
      assert.deepEqual(errors, []);
      await page.screenshot({ path: path.join(out, 'keyboard.png') });
    } finally { await context.close(); }
  },
  async layout() {
    const { page, context, errors } = await pageFixture();
    try {
      const layouts = [];
      for (const viewport of [{ width: 390, height: 844 }, { width: 960, height: 900 }, { width: 1280, height: 900 }, { width: 1440, height: 900 }, { width: 640, height: 450 }]) {
        await page.setViewportSize(viewport);
        for (const pane of ['browser', 'harness']) {
          await page.locator(`#tab-${pane}`).click();
          await page.waitForFunction(pane => !document.getElementById(`${pane}-pane`).hidden, pane);
          const url = pane === 'browser' ? `https://example.invalid/${'long-path-segment/'.repeat(18)}` : 'http://127.0.0.1:19277/';
          const title = 'Long fixture machine and page title '.repeat(8), desc = 'Fixture recovery explanation '.repeat(8);
          await publish(page, pane, { visible: true, url, canBack: true, canFwd: true, title, load: { status: 'failed', code: -102, desc, url } });
          const longText = [];
          for (const [suffix, expected] of [['title', title], ['url', url], ['detail', pane === 'harness' ? `DSH not answering (${desc})` : desc]]) {
            longText.push(await readableText(page, `#${pane}-status-${suffix}`, expected, `#${pane}-slot`));
          }
          layouts.push({ pane, viewport, zoom: viewport.width === 640 ? '200% reflow equivalent of 1280x900 (640x450 CSS viewport)' : '100%', longText, measurements: await boundsCheck(page, pane), accessibility: await renderedAccess(page) });
          await page.screenshot({ path: path.join(out, `${pane}-${viewport.width}.png`) });
        }
      }
      const styles = await page.evaluate(() => ({
        body: { color: getComputedStyle(document.body).color, background: getComputedStyle(document.body).backgroundColor, font: getComputedStyle(document.body).fontFamily },
        state: getComputedStyle(document.querySelector('.state')).fontSize,
        metadata: getComputedStyle(document.querySelector('.url')).fontSize,
        planes: ['.host-sidebar', '.utility-header', '.viewport-column', '.slot'].map(selector => ({ selector, color: getComputedStyle(document.querySelector(selector)).backgroundColor })),
      }));
      assert.equal(styles.body.background, 'rgb(243, 240, 232)');
      assert.equal(styles.body.color, 'rgb(29, 28, 26)');
      assert.match(styles.body.font, /sans-serif/);
      for (const plane of styles.planes) assert.equal(plane.color, 'rgb(251, 249, 244)', `${plane.selector}: warm paper work plane`);
      assert.equal(styles.state, '13px');
      assert.equal(styles.metadata, '12px');
      await page.emulateMedia({ forcedColors: 'active', reducedMotion: 'reduce' });
      await page.locator('#tab-browser').click();
      await page.locator('#tab-browser').focus();
      await page.keyboard.press('ArrowRight');
      const focus = await page.locator('#tab-harness').evaluate(element => ({ width: getComputedStyle(element).outlineWidth, style: getComputedStyle(element).outlineStyle }));
      assert.equal(focus.width, '2px');
      assert.equal(focus.style, 'solid');
      await boundsCheck(page, 'harness');
      await page.screenshot({ path: path.join(out, 'host-forced-colors.png') });
      const forcedPrimary = [];
      for (const pane of ['harness', 'browser']) {
        if (pane === 'browser') await page.keyboard.press('Home');
        await boundsCheck(page, pane);
        const selector = pane === 'browser' ? '#browser-go' : '#harness-open';
        const primary = await page.locator(selector).evaluate(element => {
          const style = getComputedStyle(element);
          const rect = element.getBoundingClientRect();
          const focus = getComputedStyle(document.activeElement);
          return {
            label: element.textContent.trim(), foreground: style.color,
            background: style.backgroundColor, border: style.borderColor,
            forcedColorAdjust: style.forcedColorAdjust,
            forcedColors: matchMedia('(forced-colors: active)').matches,
            rect: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
            focusedElement: document.activeElement.id,
            focusOutline: { width: focus.outlineWidth, style: focus.outlineStyle },
          };
        });
        forcedPrimary.push({ pane, ...primary, accessibility: await renderedAccess(page) });
        await page.screenshot({ path: path.join(out, `${pane}-forced-colors.png`) });
      }
      evidence.layout = { layouts, styles, focus, forcedPrimary };
      assert.deepEqual(errors, []);
    } finally { await context.close(); }
  },
  async states() {
    const { page, context } = await pageFixture();
    try {
      const observed = [];
      evidence.states = observed;
      for (const pane of ['browser', 'harness']) {
        await page.locator(`#tab-${pane}`).click();
        for (const [state, visible, load] of [
          ['hidden', false, null], ['nopage', true, null],
          ['loading', true, { status: 'loading', url: 'https://example.invalid/requested' }],
          ['ready', true, { status: 'ready', url: 'https://example.invalid/ready' }],
          ['failed', true, { status: 'failed', code: -102, desc: 'ERR_CONNECTION_REFUSED', url: 'https://example.invalid/failed' }],
          ['gone', false, { status: 'gone', desc: 'renderer stopped', url: 'https://example.invalid/requested' }],
        ]) {
          // Visibility is controlled by the host's Hide/Show intent. Establish it through the
          // real control so a queued slot ResizeObserver cannot contradict an invented native state.
          if (await page.locator(`#${pane}-toggle`).textContent() !== (visible ? 'Hide' : 'Show')) {
            await page.locator(`#${pane}-toggle`).click();
          }
          await page.waitForFunction(({ pane, visible }) => window.__fixture.records[pane].visible === visible, { pane, visible });
          await publish(page, pane, { visible, load, title: 'Fixture page' });
          await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
          observed.push({ pane, state, expectedVisible: visible, text: await page.locator(`#${pane}-status`).innerText(), trace: await page.evaluate(pane => ({ record: window.__fixture.records[pane], calls: window.__fixture.calls.slice(-8) }), pane) });
          assert.equal(await page.locator(`#${pane}-retry`).isEnabled(), state === 'failed', `${pane}/${state}: Retry is failure-only`);
          assert.equal(await page.locator(`#${pane}-toggle`).textContent(), visible ? 'Hide' : 'Show', `${pane}/${state}: toggle reflects requested visibility`);
          assert.equal(await page.locator(`#${pane}-status-url`).textContent(), load?.url || (pane === 'harness' ? 'http://127.0.0.1:19277/' : '(none)'));
        }
      }
      evidence.states = observed;
      await page.screenshot({ path: path.join(out, 'harness-gone.png') });
    } finally { await context.close(); }
    const { page: empty, context: emptyContext } = await pageFixture({ tab: 'harness', targets: [] });
    try {
      assert.equal(await empty.locator('#harness-target option').count(), 1);
      assert.equal(await empty.locator('#harness-target').inputValue(), '');
      assert.equal(await empty.locator('#harness-go').isEnabled(), false);
      await empty.screenshot({ path: path.join(out, 'harness-empty.png') });
    } finally { await emptyContext.close(); }
  },
  async actions() {
    const { page, context } = await pageFixture();
    try {
      await publish(page, 'browser', { visible: true, url: 'https://example.invalid/old', canBack: true, canFwd: true, load: { status: 'ready', url: 'https://example.invalid/old' } });
      await page.evaluate(() => window.__fixture.clear());
      await page.locator('#browser-url').fill('  https://example.invalid/requested?q=fixture  ');
      await page.locator('#browser-url').press('Enter');
      assert.deepEqual((await calls(page, 'browser', 'navigate')).map(call => call.args), [['https://example.invalid/requested?q=fixture']]);
      await page.locator('#tab-browser').focus();
      evidence.requestedUrlBeforePoll = await page.evaluate(() => ({ input: document.getElementById('browser-url').value, active: document.activeElement.id, committed: window.__fixture.records.browser.url, calls: window.__fixture.calls.slice() }));
      // The fixture owns the interval clock. Blur alone does not repaint the deliberately preserved draft.
      // Deliver the stale state reply through the production polling callback before inspecting its result.
      await page.evaluate(() => window.__fixture.tick());
      evidence.requestedUrlAfterPoll = await page.locator('#browser-url').inputValue();
      assert.equal(await page.locator('#browser-url').inputValue(), 'https://example.invalid/requested?q=fixture', 'requested URL survives a stale committed-state reply');
      for (const [id, method] of [['browser-back', 'back'], ['browser-fwd', 'fwd'], ['browser-reload', 'reload'], ['browser-recover', 'recover']]) {
        await page.locator(`#${id}`).click();
        assert.deepEqual((await calls(page, 'browser', method)).at(-1)?.args, [], `${method} has no payload`);
      }
      await page.locator('#browser-zoom').selectOption('1.1');
      assert.deepEqual((await calls(page, 'browser', 'zoom')).at(-1)?.args, [1.1]);
      await publish(page, 'browser', { visible: true, load: { status: 'failed', code: -105, desc: 'ERR_NAME_NOT_RESOLVED', url: 'https://example.invalid/retry-target' } });
      await page.locator('#browser-retry').click();
      assert.deepEqual((await calls(page, 'browser', 'navigate')).at(-1)?.args, ['https://example.invalid/retry-target']);
      await page.locator('#tab-harness').click();
      await page.locator('#harness-open').click();
      await page.locator('#harness-go').click();
      assert.deepEqual((await calls(page, 'harness', 'go')).at(-1)?.args, ['http://127.0.0.1:19277/']);
      await page.locator('#harness-reload').click();
      await page.locator('#harness-recover').click();
      await publish(page, 'harness', { visible: true, load: { status: 'failed', code: -102, desc: 'ERR_CONNECTION_REFUSED', url: 'http://127.0.0.1:19277/' } });
      await page.locator('#harness-retry').click();
      assert.ok((await calls(page, 'harness', 'open')).length >= 3);
      for (const method of ['open', 'reload', 'recover']) assert.ok((await calls(page, 'harness', method)).every(call => call.args.length === 0), `${method} stays payload-free`);
      evidence.actions = await page.evaluate(() => window.__fixture.calls);
    } finally { await context.close(); }
  },
  async poll() {
    const { page, context } = await pageFixture();
    try {
      await page.evaluate(() => { window.__fixture.clear(); window.__fixture.hold('browser'); window.__fixture.tick(); window.__fixture.tick(); window.__fixture.tick(); });
      assert.equal((await calls(page, 'browser', 'state')).length, 1, 'slow visible polling has only one state call in flight');
      assert.equal(await page.evaluate(() => window.__fixture.pending('browser')), 1);
      await page.evaluate(() => { window.__fixture.set('browser', { visible: true, load: { status: 'loading', url: 'https://example.invalid/slow' } }); window.__fixture.release('browser'); });
      await page.waitForFunction(() => document.getElementById('browser-status').dataset.state === 'loading');
      await page.evaluate(() => { Object.defineProperty(document, 'hidden', { configurable: true, value: true }); window.__fixture.clear(); window.__fixture.tick(); });
      assert.equal((await calls(page, 'browser', 'state')).length, 0, 'hidden document does not poll');
      await page.evaluate(() => { Object.defineProperty(document, 'hidden', { configurable: true, value: false }); document.dispatchEvent(new Event('visibilitychange')); });
      await page.waitForFunction(() => window.__fixture.calls.some(call => call.pane === 'browser' && call.method === 'state'));
      assert.equal((await calls(page, 'harness', 'open')).length, 0, 'polling Browser does not start Harness');
    } finally { await context.close(); }
  },
  async lifecycle() {
    const { page, context } = await pageFixture();
    try {
      assert.equal((await calls(page, 'harness', 'open')).length, 0, 'Harness stays lazy at Browser boot');
      await page.locator('#browser-toggle').click();
      assert.equal((await calls(page, 'browser', 'place')).at(-1)?.args[0], null, 'Hide sends bounds=null');
      await page.setViewportSize({ width: 390, height: 844 });
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      assert.equal((await calls(page, 'browser', 'place')).at(-1)?.args[0], null, 'resize preserves explicit Hide');
      await page.locator('#tab-harness').click();
      await page.locator('#harness-toggle').click();
      assert.equal((await calls(page, 'harness', 'place')).at(-1)?.args[0], null);
      await page.locator('#tab-browser').click();
      await page.locator('#tab-harness').click();
      assert.notEqual((await calls(page, 'harness', 'place')).at(-1)?.args[0], null, 'switch back re-shows Harness');
      await page.evaluate(() => { window.__fixture.clear(); window.dispatchEvent(new Event('pagehide')); });
      assert.deepEqual((await calls(page, 'browser', 'place')).at(-1)?.args, [null]);
      assert.deepEqual((await calls(page, 'harness', 'place')).at(-1)?.args, [null]);
    } finally { await context.close(); }
  },
  async missing() {
    const { page, context } = await pageFixture({ missing: true }, { width: 390, height: 844 });
    try {
      assert.equal(await page.locator('#bridge-missing').isVisible(), true);
      assert.match(await page.locator('#bridge-missing').textContent(), /summriseBrowser and summriseHarness/);
      assert.equal(await page.locator('button:enabled, input:enabled, select:enabled').count(), 0);
      assert.equal(await page.evaluate(() => window.__fixture.calls.length), 0);
      await page.screenshot({ path: path.join(out, 'bridge-missing.png') });
    } finally { await context.close(); }
  },
  async offline() {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, reducedMotion: 'reduce' });
    const external = [], errors = [], unknown = [];
    await context.route(/^https?:/, route => { external.push(route.request().url()); return route.abort(); });
    const page = await context.newPage();
    page.on('pageerror', error => errors.push(error.message));
    page.on('request', request => {
      const url = new URL(request.url()); url.search = ''; url.hash = '';
      if (url.protocol === 'file:' && url.href !== new URL('offline.html', source).href) unknown.push(url.href);
    });
    const layouts = [];
    try {
      const address = '<img src=x onerror="window.fixtureInjected=true"> & ' + 'very-long-machine-name-'.repeat(18);
      for (const viewport of [{ width: 390, height: 844 }, { width: 1280, height: 900 }, { width: 1440, height: 900 }, { width: 640, height: 450 }]) for (const requested of ['panel', 'activity']) {
        await page.setViewportSize(viewport);
        const url = new URL('offline.html', source);
        url.searchParams.set('address', address);
        url.searchParams.set('page', requested);
        await page.goto(url.href);
        assert.equal(await page.locator('#retry').evaluate(element => document.activeElement === element), true, 'offline Retry receives initial keyboard focus');
        assert.ok((await page.locator('#address').textContent()).includes(address), 'query address is rendered as text');
        assert.equal(await page.locator('#address img').count(), 0);
        assert.equal(await page.evaluate(() => window.fixtureInjected), undefined, 'address text never executes markup');
        assert.ok((await page.locator('#note').textContent()).includes(requested), 'offline page keeps the requested return page');
        const policy = await page.locator('meta[http-equiv="Content-Security-Policy"]').getAttribute('content');
        assert.equal(policy, "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'", 'offline page retains its closed resource policy');
        const metrics = await page.evaluate(() => ({ width: innerWidth, scroll: document.documentElement.scrollWidth, text: getComputedStyle(document.getElementById('address')).overflowWrap, buttonHeight: document.getElementById('retry').getBoundingClientRect().height, focusWidth: getComputedStyle(document.getElementById('retry')).outlineWidth }));
        assert.ok(metrics.scroll <= metrics.width, `long escaped address wraps at ${viewport.width}px: ${JSON.stringify(metrics)}`);
        assert.ok(metrics.buttonHeight >= (viewport.width <= 640 ? 40 : 32));
        const longText = await readableText(page, '#address', address);
        await page.keyboard.press('Tab'); await page.keyboard.press('Shift+Tab');
        await page.locator('#retry').focus();
        const focusProof = await focusedControl(page);
        assert.equal(focusProof.id, 'retry', 'native Retry acquires focus after keyboard-mode input');
        layouts.push({ requested, viewport, metrics, longText, focusProof, layout: await offlineLayout(page), accessibility: await renderedAccess(page) });
        let reloads = 0;
        const navigation = frame => { if (frame === page.mainFrame()) reloads++; };
        page.on('framenavigated', navigation);
        await Promise.all([page.waitForEvent('load'), page.keyboard.press('Enter')]);
        page.off('framenavigated', navigation);
        assert.equal(reloads, 1, 'one keyboard activation reloads exactly once');
        await page.screenshot({ path: path.join(out, `offline-${requested}-${viewport.width}.png`) });
      }
      await page.emulateMedia({ forcedColors: 'active', reducedMotion: 'reduce' });
      await page.locator('#retry').focus();
      await page.screenshot({ path: path.join(out, 'offline-forced-colors.png') });
      assert.equal(await page.locator('#retry').evaluate(element => getComputedStyle(element).outlineWidth), '2px');
      const forcedLongText = await readableText(page, '#address', address);
      await page.keyboard.press('Tab'); await page.keyboard.press('Shift+Tab');
      await page.locator('#retry').focus();
      evidence.offline = { layouts, forced: { layout: await offlineLayout(page), longText: forcedLongText, focusProof: await focusedControl(page), accessibility: await renderedAccess(page) } };
      assert.deepEqual(external, [], 'offline recovery makes no external requests');
      assert.deepEqual(unknown, [], 'offline recovery loads only its own document');
      assert.deepEqual(errors, [], 'offline script has no uncaught exceptions');
    } finally { await context.close(); }
  },
};

try {
  const executable = process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH || chromium.executablePath();
  accessSync(executable, constants.X_OK);
  evidence.executable = realpathSync(executable);
  // Own ephemeral loopback transport only. Never attach to an existing browser/CDP endpoint.
  server = await chromium.launchServer({ executablePath: executable, headless: true, host: '127.0.0.1', port: 0 });
  durableWrite('chromium.pid', `${server.process().pid}\n`);
  evidence.chromiumPid = server.process().pid;
  const transport = new URL(server.wsEndpoint());
  evidence.transport = transport.host;
  assert.equal(transport.hostname, '127.0.0.1');
  assert.ok(![7738, 18081].includes(Number(transport.port)), 'owned transport avoids protected ports');
  browser = await chromium.connect(server.wsEndpoint());
  evidence.browserVersion = browser.version();
  for (const [name, run] of Object.entries(cases)) {
    if (only !== 'all' && name !== only) continue;
    const started = Date.now();
    try {
      await run();
      results.push({ name, outcome: 'pass', durationMs: Date.now() - started });
      console.log(`PASS ${name}`);
    } catch (error) {
      results.push({ name, outcome: 'fail', durationMs: Date.now() - started, error: error.stack });
      throw error;
    } finally { save(); }
  }
} catch (error) {
  evidence.error = error.stack;
  save();
  console.error(error.stack);
  process.exitCode = 1;
} finally {
  evidence.cleanup = [];
  try {
    await browser?.close();
    evidence.cleanup.push({ target: 'browser', outcome: 'closed' });
  } catch (error) {
    evidence.cleanup.push({ target: 'browser', outcome: 'failed', error: error.stack });
    process.exitCode = 1;
  } finally {
    try {
      await server?.close();
      evidence.cleanup.push({ target: 'owned-server', outcome: 'closed' });
    } catch (error) {
      evidence.cleanup.push({ target: 'owned-server', outcome: 'failed', error: error.stack });
      process.exitCode = 1;
    } finally {
      save();
    }
  }
}
