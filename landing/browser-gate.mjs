#!/usr/bin/env node
// Local landing fixtures. All release responses below are synthetic, never publication evidence.
// --unit requires Console's provisioned jsdom; the browser gate requires its locked Playwright and Chromium.
// CI's console-web job provisions those dependencies. The landing-only job does not; missing tools fail.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { withAuthorPaintScope } from '../scripts/author-paint-scope.mjs';

const requireConsole = createRequire(new URL("../device/console/web/package.json", import.meta.url));
const publicDir = new URL("./public/", import.meta.url);
const html = readFileSync(new URL("index.html", publicDir), "utf8");
const languageScript = readFileSync(new URL("assets/lang-zh.js", publicDir), "utf8");
const siteScript = readFileSync(new URL("assets/site.js", publicDir), "utf8");
const releasesURL = "https://api.github.com/repos/SilasVale/summrise/releases?per_page=10";
const releasePage = "https://github.com/SilasVale/summrise/releases";
const fixturePage = `${releasePage}/tag/v2.0.20`;
const fixtureAsset = (name) => ({ name, browser_download_url: `${releasePage}/download/v2.0.20/${name}` });
const releaseFixture = (assets = [fixtureAsset("SummriseSetup-2.0.20-canary.exe"), fixtureAsset("release-2.0.20-canary.json"), fixtureAsset("SHA256SUMS")]) => [{
  draft: false, tag_name: "v2.0.20", html_url: fixturePage, assets,
}];
const tick = () => new Promise((resolve) => setImmediate(resolve));

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

async function unitGate() {
  const { JSDOM } = requireConsole("jsdom");
  const cases = [];
  const test = (name, run) => cases.push({ name, run });
  function open({ query = "", saved, blockedStorage = false, scripts = true } = {}) {
    const dom = new JSDOM(html, { url: `https://landing.fixture/${query}`, runScripts: "outside-only" });
    const { window } = dom;
    if (saved) window.localStorage.setItem("summrise.lang", saved);
    if (blockedStorage) Object.defineProperty(window, "localStorage", { get() { throw new Error("fixture: storage blocked"); } });
    let answer, reject;
    window.fetch = (url) => {
      assert.equal(url, releasesURL, "only the published-release lookup is permitted");
      return new Promise((resolve, fail) => { answer = resolve; reject = fail; });
    };
    if (scripts) { window.eval(languageScript); window.eval(siteScript); }
    return {
      window, document: window.document,
      async reply(body, status = 200) { answer({ ok: status < 400, status, json: () => Promise.resolve(body) }); await tick(); },
      async fail() { reject(new Error("fixture: network unavailable")); await tick(); },
      close: () => window.close(),
    };
  }
  function status(page, expected) {
    const node = page.document.getElementById("release-status");
    assert.ok(node, "release lookup has a visible status");
    assert.equal(node.getAttribute("role"), "status");
    assert.equal(node.dataset.state, expected);
    assert.ok(node.textContent.trim().length > 15, "the release state explains the next step");
    return node;
  }
  test("a language query works when storage is blocked", async () => {
    const p = open({ query: "?lang=zh", blockedStorage: true });
    try { assert.equal(p.document.documentElement.lang, "zh-CN"); assert.match(p.document.querySelector("h1").textContent, /AI/); } finally { p.close(); }
  });
  test("language defaults, saved choice, query precedence and toggle persistence", async () => {
    for (const [options, expected] of [[{}, "en"], [{ saved: "zh" }, "zh-CN"], [{ saved: "zh", query: "?lang=en" }, "en"]]) {
      const p = open(options);
      try {
        assert.equal(p.document.documentElement.lang, expected);
        p.document.getElementById("lang-toggle").click();
        assert.equal(p.window.localStorage.getItem("summrise.lang"), expected === "en" ? "zh" : "en");
      } finally { p.close(); }
    }
  });
  test("pending release lookup keeps all release fallbacks usable", async () => {
    const p = open();
    try { status(p, "pending"); for (const id of ["dl-installer", "art-installer", "art-doc", "art-sums", "dl-notes"]) assert.equal(p.document.getElementById(id).href, releasePage); } finally { p.close(); }
  });
  test("a failed release lookup explains the error without claiming a version", async () => {
    const p = open();
    try { await p.fail(); status(p, "error"); assert.equal(p.document.getElementById("release-tag").textContent, "see the releases page"); assert.equal(p.document.getElementById("dl-installer").href, releasePage); } finally { p.close(); }
  });
  test("an empty or draft-only release list is distinct from a lookup error", async () => {
    for (const releases of [[], [{ draft: true }]]) {
      const p = open();
      try { await p.reply(releases); status(p, "empty"); assert.equal(p.document.getElementById("art-doc").href, releasePage); } finally { p.close(); }
    }
  });
  test("a complete fixture release supplies the actual asset and notes destinations", async () => {
    const p = open();
    try {
      await p.reply(releaseFixture()); status(p, "success");
      assert.equal(p.document.getElementById("release-tag").textContent, "v2.0.20");
      assert.equal(p.document.getElementById("art-installer").href, releaseFixture()[0].assets[0].browser_download_url);
      assert.equal(p.document.getElementById("art-doc").href, releaseFixture()[0].assets[1].browser_download_url);
      assert.equal(p.document.getElementById("art-sums").href, releaseFixture()[0].assets[2].browser_download_url);
      assert.equal(p.document.getElementById("dl-notes").href, fixturePage);
    } finally { p.close(); }
  });
  test("a partial fixture release identifies fallback links in both languages", async () => {
    const p = open();
    try {
      await p.reply(releaseFixture([fixtureAsset("SummriseSetup-2.0.20-canary.exe")]));
      status(p, "partial"); assert.equal(p.document.getElementById("art-doc").href, fixturePage);
      assert.equal(p.document.getElementById("art-doc").textContent.trim(), "View release");
      p.document.getElementById("lang-toggle").click();
      status(p, "partial"); assert.equal(p.document.getElementById("art-doc").textContent.trim(), "查看发布页");
      assert.equal(p.document.getElementById("release-tag").textContent, "v2.0.20");
    } finally { p.close(); }
  });
  test("a partial fixture without an installer retains an explicitly labeled release fallback", async () => {
    const p = open();
    try {
      await p.reply(releaseFixture([fixtureAsset("release-2.0.20-canary.json"), fixtureAsset("SHA256SUMS")]));
      status(p, "partial");
      assert.equal(p.document.getElementById("dl-installer").href, fixturePage);
      assert.equal(p.document.getElementById("dl-installer-label").textContent, "View Windows releases");
      assert.equal(p.document.getElementById("art-installer").textContent, "View release");
      assert.equal(p.document.getElementById("art-doc").href, releaseFixture()[0].assets[1].browser_download_url);
    } finally { p.close(); }
  });
  test("asset roles reject sidecars, wrong versions and suffix lookalikes regardless of upload order", async () => {
    const bad = [
      "SummriseSetup-2.0.20-canary.sidecar.json", "SummriseSetup-2.0.20-canary.exe.sidecar.json",
      "SummriseSetup-2.0.19-canary.exe", "SummriseSetup-2x0x20-canary.exe", "SummriseSetup-2.0.20-canary.exe.bak",
      "release-2.0.20-canary.sidecar.json", "release-2.0.19-canary.json", "release-2x0x20-canary.json",
      "release-2.0.20-canary.json.sidecar.json", "release-2.0.20-canary.inventory.json", "inventory.json",
      "SHA256SUMS.asc", "SHA256SUMS.json", "SHA256SUMS-2.0.20",
    ];
    const good = ["SummriseSetup-2.0.20-canary.exe", "release-2.0.20-canary.json", "SHA256SUMS"];
    for (const names of [[...bad, ...good], [...bad].reverse().concat([...good].reverse()), [good[2], ...bad.slice(7), good[1], ...bad.slice(0, 7), good[0]]]) {
      const p = open();
      try {
        await p.reply(releaseFixture(names.map(fixtureAsset))); status(p, "success");
        assert.equal(p.document.getElementById("art-installer").href, `${releasePage}/download/v2.0.20/SummriseSetup-2.0.20-canary.exe`);
        assert.equal(p.document.getElementById("dl-installer").href, `${releasePage}/download/v2.0.20/SummriseSetup-2.0.20-canary.exe`);
        assert.equal(p.document.getElementById("art-doc").href, `${releasePage}/download/v2.0.20/release-2.0.20-canary.json`);
        assert.equal(p.document.getElementById("art-sums").href, `${releasePage}/download/v2.0.20/SHA256SUMS`);
      } finally { p.close(); }
    }
  });
  test("wrong-version-only assets retain partial release fallbacks in both languages", async () => {
    const p = open();
    try {
      await p.reply(releaseFixture(["SummriseSetup-2.0.19-canary.exe", "release-2.0.19-canary.json", "SHA256SUMS"].map(fixtureAsset)));
      for (const translated of [false, true]) {
        if (translated) p.document.getElementById("lang-toggle").click();
        status(p, "partial");
        for (const id of ["dl-installer", "art-installer", "art-doc"]) assert.equal(p.document.getElementById(id).href, fixturePage);
        assert.equal(p.document.getElementById("art-doc").textContent.trim(), translated ? "查看发布页" : "View release");
        assert.equal(p.document.getElementById("art-sums").href, `${releasePage}/download/v2.0.20/SHA256SUMS`);
      }
    } finally { p.close(); }
  });
  test("a checksum lookalike cannot make an otherwise complete release successful", async () => {
    const p = open();
    try {
      await p.reply(releaseFixture(["SummriseSetup-2.0.20-canary.exe", "release-2.0.20-canary.json", "SHA256SUMS.asc", "SHA256SUMS.json"].map(fixtureAsset)));
      status(p, "partial"); assert.equal(p.document.getElementById("art-sums").href, fixturePage);
    } finally { p.close(); }
  });
  test("documented stable and legacy version-only filenames match the selected optional-v tag", async () => {
    for (const [tag, installer, doc] of [
      ["2.0.20", "SummriseSetup-2.0.20-stable.exe", "release-2.0.20-stable.json"],
      ["v0.3.12", "SummriseSetup-0.3.12.exe", "release-0.3.12.json"],
      ["v2.0.20", "SummriseSetup-2.0.20-canary.exe", "release-2.0.20.json"],
    ]) {
      const p = open();
      try {
        const releases = releaseFixture([installer, doc, "SHA256SUMS"].map(fixtureAsset)); releases[0].tag_name = tag;
        await p.reply(releases); status(p, "success");
        assert.equal(p.document.getElementById("art-installer").href, `${releasePage}/download/v2.0.20/${installer}`);
        assert.equal(p.document.getElementById("art-doc").href, `${releasePage}/download/v2.0.20/${doc}`);
      } finally { p.close(); }
    }
  });
  test("an explicitly different channel document remains a partial fallback", async () => {
    const p = open();
    try {
      await p.reply(releaseFixture(["SummriseSetup-2.0.20-canary.exe", "release-2.0.20-stable.json", "SHA256SUMS"].map(fixtureAsset)));
      status(p, "partial"); assert.equal(p.document.getElementById("art-doc").href, fixturePage);
      assert.equal(p.document.getElementById("art-installer").href, `${releasePage}/download/v2.0.20/SummriseSetup-2.0.20-canary.exe`);
    } finally { p.close(); }
  });
  test("ambiguous same-version channel assets do not depend on array order", async () => {
    const names = ["SummriseSetup-2.0.20-canary.exe", "SummriseSetup-2.0.20-stable.exe", "release-2.0.20-canary.json", "release-2.0.20-stable.json", "SHA256SUMS"];
    for (const order of [names, [...names].reverse()]) {
      const p = open();
      try {
        await p.reply(releaseFixture(order.map(fixtureAsset))); status(p, "partial");
        for (const id of ["dl-installer", "art-installer", "art-doc"]) assert.equal(p.document.getElementById(id).href, fixturePage);
      } finally { p.close(); }
    }
  });
  test("first published release selection still includes prereleases and skips drafts", async () => {
    const p = open();
    try {
      const releases = releaseFixture(); releases[0].prerelease = true;
      await p.reply([{ draft: true, tag_name: "v9.0.0" }, ...releases, { ...releases[0], tag_name: "v1.0.0", html_url: `${releasePage}/tag/v1.0.0` }]);
      status(p, "success"); assert.equal(p.document.getElementById("release-tag").textContent, "v2.0.20");
      assert.equal(p.document.getElementById("dl-notes").href, fixturePage);
    } finally { p.close(); }
  });
  test("unversioned release tags preserve notes and honest partial asset fallbacks", async () => {
    const p = open();
    try {
      const releases = releaseFixture(); releases[0].tag_name = "current";
      await p.reply(releases); status(p, "partial");
      for (const id of ["art-installer", "art-doc", "dl-notes"]) assert.equal(p.document.getElementById(id).href, fixturePage);
    } finally { p.close(); }
  });
  test("HTTP and malformed release responses remain honest lookup errors", async () => {
    for (const [body, code] of [[{}, 503], [{ message: "fixture: rate limited" }, 200]]) {
      const p = open();
      try { await p.reply(body, code); status(p, "error"); assert.equal(p.document.getElementById("art-installer").href, releasePage); } finally { p.close(); }
    }
  });
  test("a missing Console screenshot exposes a translated alternative", async () => {
    const p = open({ query: "?lang=zh" });
    try {
      const image = p.document.querySelector('img[src="assets/console-home.png"]');
      image.dispatchEvent(new p.window.Event("error"));
      assert.equal(image.hidden, true);
      assert.equal(p.document.getElementById("shot-fallback").hidden, false);
      assert.match(p.document.getElementById("shot-fallback").textContent, /控制台/);
    } finally { p.close(); }
  });
  test("without JavaScript the English page retains anchors, docs and release links", async () => {
    const p = open({ scripts: false });
    try {
      assert.equal(p.document.documentElement.lang, "en");
      assert.equal(p.document.getElementById("lang-toggle").hidden, true, "hide a control that requires JavaScript");
      for (const anchor of ["capabilities", "surfaces", "download"]) assert.ok(p.document.getElementById(anchor));
      assert.equal(p.document.getElementById("dl-installer").href, releasePage);
      assert.ok(p.document.querySelector('a[href="https://github.com/SilasVale/summrise/tree/main/docs"]'));
    } finally { p.close(); }
  });
  let failures = 0;
  for (const { name, run } of cases) {
    try { await run(); console.log(`PASS DOM fixture: ${name}`); }
    catch (error) { failures++; console.error(`FAIL DOM fixture: ${name}\n${error.stack}`); }
  }
  console.log(`landing DOM fixtures: ${cases.length - failures}/${cases.length} pass; ${failures} failure(s); synthetic release data, no network`);
  process.exitCode = failures ? 1 : 0;
}

async function browserGate() {
  const { chromium } = requireConsole("playwright");
  const { mkdir } = await import("node:fs/promises");
  const shots = process.env.LANDING_SHOTS;
  if (shots) await mkdir(shots, { recursive: true });
  const browser = await chromium.launch({
    headless: true,
    ...((process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH || process.env.CHROME) ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH || process.env.CHROME } : {}),
    args: ["--no-sandbox", "--disable-dev-shm-usage", "--disable-background-networking", "--no-proxy-server"],
  });
  let passed = 0;
  let opened = 0;
  const origin = "https://summrise.fixture";
  async function open({ width = 1280, lang = "en", query = `?lang=${lang}`, saved, blockedStorage = false, mode = "success", javaScriptEnabled = true, missingScreenshot = false, forcedColors = "none", reducedMotion = "no-preference" } = {}) {
    const context = await browser.newContext({ viewport: { width, height: 900 }, javaScriptEnabled, serviceWorkers: "block", forcedColors, reducedMotion });
    const external = [];
    const unknown = [];
    const errors = [];
    let pendingRoute;
    const page = await context.newPage();
    page.setDefaultTimeout(5000);
    page.on("pageerror", (error) => errors.push(error.message));
    await context.addInitScript(({ saved, blockedStorage }) => {
      if (saved) localStorage.setItem("summrise.lang", saved);
      if (blockedStorage) Object.defineProperty(window, "localStorage", { get() { throw new Error("fixture: storage blocked"); } });
    }, { saved, blockedStorage });
    await context.route("**/*", async (route) => {
      const url = new URL(route.request().url());
      if (url.href === releasesURL) {
        if (mode === "pending") { pendingRoute = route; return; }
        if (mode === "network-error") { await route.abort("failed"); return; }
        const body = mode === "empty" ? [] : mode === "draft" ? [{ draft: true }]
          : mode === "malformed" ? { message: "fixture: unexpected response" }
          : mode === "partial" ? releaseFixture([fixtureAsset("release-2.0.20-canary.json"), fixtureAsset("SHA256SUMS")]) : releaseFixture();
        await route.fulfill({ status: mode === "http-error" ? 503 : 200, contentType: "application/json", body: JSON.stringify(body) });
        return;
      }
      if (url.origin !== origin) { external.push(url.href); await route.abort("blockedbyclient"); return; }
      const pathname = url.pathname === "/" ? "index.html" : url.pathname.slice(1);
      const types = { "index.html": "text/html", "assets/site.css": "text/css", "assets/site.js": "application/javascript", "assets/lang-zh.js": "application/javascript", "assets/favicon.svg": "image/svg+xml", "assets/console-home.png": "image/png" };
      if (!types[pathname]) { unknown.push(url.href); await route.fulfill({ status: 404, body: "Local fixture: unknown request" }); return; }
      if (missingScreenshot && pathname === "assets/console-home.png") { await route.fulfill({ status: 404, body: "Local fixture: asset unavailable" }); return; }
      await route.fulfill({ status: 200, contentType: types[pathname], body: readFileSync(fileURLToPath(new URL(pathname, publicDir))) });
    });
    await page.goto(`${origin}/${query}`, { waitUntil: "load" });
    opened++;
    return {
      page,
      async reply(body) { assert.ok(pendingRoute, "release lookup was intercepted"); await pendingRoute.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(body) }); },
      async close() {
        try { assert.deepEqual(external, [], "fixture never follows external download or contact links"); assert.deepEqual(unknown, [], "every same-origin request has an explicit fixture"); assert.deepEqual(errors, [], "page scripts have no uncaught exceptions"); }
        finally { await context.close(); }
      },
    };
  }
  async function withPage(options, run) {
    const fixture = await open(options);
    try { await run(fixture.page, fixture); }
    finally { await fixture.close(); }
  }
  async function state(page, expected) {
    await page.waitForFunction((expected) => document.getElementById("release-status")?.dataset.state === expected, expected);
    assert.equal(await page.locator("#release-status").getAttribute("role"), "status");
    assert.equal(await page.locator("#release-status").isVisible(), true);
  }
  async function layout(page) {
    const { value: result, evidence: authorPaintScopeEvidence } = await withAuthorPaintScope(page, authorPaintScope =>
      page.evaluate(({ authorPaintScope } = {}) => {
      const width = innerWidth;
      const visible = (node) => node.getClientRects().length && getComputedStyle(node).visibility !== "hidden" && !node.closest(".skip");
      const elements = [...document.querySelectorAll("body *")].filter(visible);
      const outside = [];
      const clipped = [];
      for (const node of elements) {
        const rect = node.getBoundingClientRect(), style = getComputedStyle(node);
        const name = node.tagName.toLowerCase() + (node.id ? `#${node.id}` : "");
        if (rect.width > 0 && (rect.left < -1 || rect.right > width + 1)) outside.push(`${name}:${Math.round(rect.left)}..${Math.round(rect.right)}`);
        if (node.clientWidth > 0 && node.scrollWidth > node.clientWidth + 1 && ["hidden", "clip", "auto", "scroll"].includes(style.overflowX)) clipped.push(`${name} +${node.scrollWidth - node.clientWidth}`);
      }
      const small = elements.filter((node) => node.matches(".meta,.kicker,.eyebrow") && parseFloat(getComputedStyle(node).fontSize) < 12).map((node) => node.className);
      const zoom = parseFloat(getComputedStyle(document.documentElement).zoom) || 1;
      const minimumControlHeight = matchMedia("(max-width: 639px)").matches ? 40 : 32;
      const controls = elements.filter((node) => node.matches("a,button") && node.getBoundingClientRect().height / zoom < minimumControlHeight - .5).map((node) => node.id || node.textContent.trim());
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
      const failures = [];
      let measured = 0, minimum = 21;
      // Validate every visible owned paint, including non-text nested panels.
      for (const node of elements) paint(node);
      for (const node of elements) {
        if (![...node.childNodes].some((text) => text.nodeType === Node.TEXT_NODE && text.textContent.trim())) continue;
        const cs = getComputedStyle(node), pixels = compositePixels(node, rgba(cs.color));
        const ratio = contrast(pixels.foreground, pixels.background);
        if (!Number.isFinite(ratio)) throw new Error('contrast: invalid measured text ratio');
        minimum = Math.min(minimum, ratio); measured++;
        const size = parseFloat(cs.fontSize), bold = Number(cs.fontWeight) >= 700;
        if (ratio < (size >= 24 || (bold && size >= 18.66) ? 3 : 4.5)) failures.push(`${node.tagName}:${ratio.toFixed(2)} ${node.textContent.trim().slice(0, 35)}`);
      }
      const controlContrast = [];
      let controlPairs = 0;
      for (const node of elements.filter((element) => element.matches('button,.btn'))) {
        const cs = getComputedStyle(node), fill = rgba(cs.backgroundColor);
        const fillPixels = compositePixels(node, fill, true);
        let ratio = contrast(fillPixels.foreground, fillPixels.background);
        if (parseFloat(cs.borderTopWidth) > 0 && !['none', 'hidden'].includes(cs.borderTopStyle)) {
          if (cs.backgroundClip !== 'border-box') throw new Error(`contrast: unsupported control backgroundClip=${cs.backgroundClip}`);
          const pixels = compositePixels(node, over(rgba(cs.borderTopColor), fill), true);
          ratio = Math.max(ratio, contrast(pixels.foreground, pixels.background));
        }
        if (!Number.isFinite(ratio)) throw new Error('contrast: invalid measured control ratio');
        controlPairs++;
        if (ratio < 3) controlContrast.push(`${node.id || node.className}: ${ratio.toFixed(2)}:1 control boundary`);
      }
      const rect = (selector) => {
        const node = document.querySelector(selector);
        if (!node) return null;
        const box = node.getBoundingClientRect();
        return { x: box.x, y: box.y, right: box.right, bottom: box.bottom, width: box.width, height: box.height };
      };
      const planes = ["body", ".masthead", ".hero", ".product-section", ".capabilities-section", ".control-section", ".setup-section", ".download-section", ".site-footer"].map((selector) => {
        const node = document.querySelector(selector);
        return node ? { selector, ...rect(selector), background: getComputedStyle(node).backgroundColor, color: getComputedStyle(node).color, image: getComputedStyle(node).backgroundImage } : { selector, missing: true };
      });
      const measures = [...document.querySelectorAll(".measure")].map((node) => {
        const box = node.getBoundingClientRect();
        return { x: box.x, right: box.right, width: box.width };
      });
      const sections = [...document.querySelectorAll("main > section")].map((section) => ({ id: section.id || "hero", label: section.getAttribute("aria-labelledby"), heading: document.getElementById(section.getAttribute("aria-labelledby"))?.tagName }));
      return { width, scrollWidth: document.documentElement.scrollWidth, outside, clipped, small, controls, controlContrast, controlPairs, failures, measured, minimum,
        h1: document.querySelectorAll("h1").length, main: document.querySelectorAll("main").length,
        overflow: [getComputedStyle(document.body).overflowX, getComputedStyle(document.documentElement).overflowX],
        planes, measures, sections, forcedColors: matchMedia("(forced-colors: active)").matches,
        effectiveWidth: width / (parseFloat(getComputedStyle(document.documentElement).zoom) || 1),
        hierarchy: { guide: rect(".product-guide"), stage: rect(".shot"), intro: rect(".control-intro"), proof: rect(".control-proof"), download: rect(".download-intro"), resources: rect(".artifacts"),
          facts: document.querySelectorAll(".product-facts > div").length, workflow: document.querySelectorAll(".product-guide ol > li").length,
          benefits: document.querySelectorAll(".capability-list > article").length, setup: document.querySelectorAll(".setup-steps > li").length,
          primary: document.querySelectorAll('.hero-actions > .btn-accent[href="#download"]').length },
        font: getComputedStyle(document.body).fontFamily };
    }, { authorPaintScope }));
    assert.equal(result.h1, 1);
    assert.equal(result.main, 1);
    assert.ok(result.scrollWidth <= result.width + 1, JSON.stringify(result));
    for (const key of ["outside", "clipped", "small", "controls", "controlContrast", "failures"]) assert.deepEqual(result[key], [], `${key}: ${JSON.stringify(result)}`);
    assert.ok(result.controlPairs >= 2, "actual primary action and control boundaries were measured");
    assert.ok(result.overflow.every((value) => value !== "hidden" && value !== "clip"), "global clipping cannot mask overflow");
    assert.ok(result.measures.length >= 7, "navigation, story and footer keep independent readable content areas");
    for (const measure of result.measures) assert.ok(measure.width > 0 && measure.x > 0 && measure.right < result.width, `content stays inside its full-width plane: ${JSON.stringify(measure)}`);
    assert.deepEqual(result.sections.map(({ id, heading }) => [id, heading]), [["hero", "H1"], ["download", "H2"], ["surfaces", "H2"], ["capabilities", "H2"], ["trust", "H2"], ["setup", "H2"]], "labelled sections with the download immediately after the hero");
    for (const plane of result.planes) {
      assert.equal(plane.missing, undefined, `${plane.selector}: required story plane exists`);
      assert.ok(Math.abs(plane.width - result.width) <= 1, `${plane.selector}: full-width story plane`);
      assert.equal(plane.image, "none", `${plane.selector}: readable plane has no background image or gradient`);
    }
    if (!result.forcedColors) {
      const expected = { body: "rgb(247, 244, 238)", ".masthead": "rgb(255, 255, 255)", ".hero": "rgb(247, 244, 238)", ".product-section": "rgb(255, 255, 255)", ".capabilities-section": "rgba(0, 0, 0, 0)", ".control-section": "rgb(255, 240, 230)", ".setup-section": "rgb(255, 255, 255)", ".download-section": "rgba(0, 0, 0, 0)", ".site-footer": "rgb(255, 255, 255)" };
      for (const plane of result.planes) assert.equal(plane.background, expected[plane.selector], `${plane.selector}: warm brand plane`);
      assert.equal(result.planes[0].color, "rgb(23, 22, 26)", "warm ink on the light canvas");
    }
    for (const role of ["guide", "stage", "intro", "proof", "download", "resources"]) assert.ok(result.hierarchy[role]?.width > 0 && result.hierarchy[role].height > 0, `${role}: nonempty product area`);
    for (const [support, primary] of [["guide", "stage"], ["intro", "proof"], ["download", "resources"]]) {
      const a = result.hierarchy[support], b = result.hierarchy[primary];
      const beside = a.right <= b.x + 1, below = a.bottom <= b.y + 1;
      assert.ok(beside || below, `${support}/${primary}: distinct nonoverlapping work areas`);
      if (result.effectiveWidth >= 1000) {
        assert.ok(beside, `${support}/${primary}: independent columns on a desktop`);
        assert.ok(b.width > a.width, `${primary}: primary work area is wider than supporting context`);
      }
      if (result.effectiveWidth <= 560) assert.ok(below, `${primary}: reflows below context on phone`);
    }
    for (const key of ["facts", "workflow", "benefits", "setup"]) assert.equal(result.hierarchy[key], 3, `${key}: complete product explanation`);
    assert.equal(result.hierarchy.primary, 1, "one primary hero action");
    assert.match(result.font, /sans-serif/);
    assert.ok(result.measured > 60, `actual rendered contrast pairs: ${result.measured}`);
    return { ...result, authorPaintScopeEvidence };
  }
  async function test(name, run) { await run(); passed++; console.log(`PASS browser fixture: ${name}`); }
  try {
    for (const lang of ["en", "zh"]) for (const width of [390, 1280, 1440]) {
      await test(`${lang} @${width}: real layout, visible text contrast and resource destinations`, () => withPage({ lang, width }, async (page) => {
        await state(page, "success");
        await page.locator("#console-shot").scrollIntoViewIfNeeded();
        await page.waitForFunction(() => document.getElementById("console-shot").naturalWidth > 0);
        const result = await layout(page);
        assert.equal(await page.locator("html").getAttribute("lang"), lang === "zh" ? "zh-CN" : "en");
        assert.equal(await page.locator("#art-installer").getAttribute("href"), releaseFixture()[0].assets[0].browser_download_url);
        assert.equal(await page.locator("#art-doc").getAttribute("href"), releaseFixture()[0].assets[1].browser_download_url);
        assert.equal(await page.locator("#art-sums").getAttribute("href"), releaseFixture()[0].assets[2].browser_download_url);
        assert.equal(await page.locator("#dl-notes").getAttribute("href"), fixturePage);
        console.log(`  ${result.measured} rendered text pairs, minimum ${result.minimum.toFixed(2)}:1`);
        if (shots) await page.screenshot({ path: `${shots}/landing-${lang}-${width}.png`, fullPage: true });
      }));
    }
    for (const lang of ["en", "zh"]) for (const width of [390, 1280]) {
      await test(`${lang} @${width}: every real Tab stop stays reachable with composited focus`, () => withPage({ lang, width, reducedMotion: "reduce" }, async (page) => {
        const total = await page.locator('a[href],button:not([hidden])').count();
        const expected = await page.locator('a[href],button:not([hidden])').evaluateAll(nodes => nodes.map(node => node.id || node.getAttribute("href")));
        const reached = [];
        for (let index = 0; index < total; index++) {
          await page.keyboard.press("Tab");
          await focusedControl(page);
          reached.push(await page.evaluate(() => document.activeElement.id || document.activeElement.getAttribute("href")));
        }
        assert.deepEqual(reached, expected, "every visible link and language control is reached in actual DOM order");
      }));
    }
    for (const lang of ["en", "zh"]) for (const mode of ["pending", "network-error", "http-error", "empty", "draft", "malformed", "partial"]) {
      await test(`${lang}: ${mode} release response remains truthful`, () => withPage({ lang, width: 390, mode }, async (page, fixture) => {
        const expected = mode === "pending" ? "pending" : ["network-error", "http-error", "malformed"].includes(mode) ? "error" : mode === "partial" ? "partial" : "empty";
        await state(page, expected); await layout(page);
        assert.equal(await page.locator("#dl-installer").getAttribute("href"), mode === "partial" ? fixturePage : releasePage);
        if (mode === "partial") {
          assert.equal(await page.locator("#art-installer").innerText(), lang === "zh" ? "查看发布页" : "View release");
          assert.equal(await page.locator("#art-doc").getAttribute("href"), releaseFixture()[0].assets[1].browser_download_url);
        }
        if (mode === "pending") {
          await page.locator("#lang-toggle").focus(); await page.keyboard.press("Space");
          await fixture.reply(releaseFixture()); await state(page, "success");
          assert.equal(await page.locator("html").getAttribute("lang"), lang === "en" ? "zh-CN" : "en");
          assert.equal(await page.locator("#release-tag").innerText(), "v2.0.20");
        }
      }));
    }
    for (const [options, expected] of [[{ query: "" }, "en"], [{ query: "", saved: "zh" }, "zh-CN"], [{ query: "?lang=en", saved: "zh" }, "en"], [{ query: "?lang=zh", blockedStorage: true }, "zh-CN"]]) {
      await test(`language defaults, persistence or query precedence: ${JSON.stringify(options)}`, () => withPage(options, async (page) => {
        assert.equal(await page.locator("html").getAttribute("lang"), expected);
        await page.locator("#lang-toggle").focus(); await page.keyboard.press("Space");
        assert.equal(await page.locator("html").getAttribute("lang"), expected === "en" ? "zh-CN" : "en");
        if (!options.blockedStorage) assert.equal(await page.evaluate(() => localStorage.getItem("summrise.lang")), expected === "en" ? "zh" : "en");
      }));
    }
    await test("keyboard reaches main, language, download resources and footer without external navigation", () => withPage({}, async (page) => {
      await page.keyboard.press("Tab");
      assert.equal(await page.evaluate(() => document.activeElement.className), "skip");
      await page.keyboard.press("Enter");
      assert.equal(await page.evaluate(() => document.activeElement.id), "main");
      await page.locator(".skip").focus();
      const count = await page.locator('a[href],button:not([hidden])').count();
      const reached = new Set();
      for (let i = 0; i < count - 1; i++) {
        await page.keyboard.press("Tab");
        const focused = await page.evaluate(() => {
          const node = document.activeElement, style = getComputedStyle(node);
          return { id: node.id, footer: !!node.closest(".footer-links"), outline: parseFloat(style.outlineWidth), visible: style.outlineStyle !== "none" };
        });
        if (focused.id) reached.add(focused.id);
        if (focused.footer) reached.add("footer");
        assert.ok(focused.outline >= 2 && focused.visible, `visible 2px keyboard focus: ${JSON.stringify(focused)}`);
      }
      for (const id of ["lang-toggle", "dl-installer", "art-installer", "art-doc", "art-sums", "footer"]) assert.ok(reached.has(id), `keyboard reaches ${id}`);
      const navDownload = page.locator('.nav-links > a:first-child.btn-accent[href="#download"]');
      await navDownload.focus(); await page.keyboard.press("Enter");
      assert.equal(new URL(page.url()).hash, "#download");
    }));
    for (const lang of ["en", "zh"]) {
      await test(`${lang}: 200% CSS zoom and long-copy reflow fixture`, () => withPage({ lang }, async (page) => {
        await page.evaluate((lang) => {
          document.documentElement.style.zoom = "2";
          document.querySelector(".hero-lede").append(lang === "zh" ? " 长文案布局测试：检查内容是否完整显示。".repeat(4) : " Long-copy layout fixture checks that the full text remains visible.".repeat(4));
        }, lang);
        await layout(page);
        if (shots) await page.screenshot({ path: `${shots}/landing-${lang}-zoom200.png`, fullPage: true });
      }));
      await test(`${lang}: forced colors retain visible controls and keyboard focus`, () => withPage({ lang, width: 390, forcedColors: "active" }, async (page) => {
        await layout(page); await page.locator("#lang-toggle").focus();
        assert.equal(await page.evaluate(() => matchMedia("(forced-colors: active)").matches), true);
        assert.equal(await page.locator("#lang-toggle").evaluate((node) => getComputedStyle(node).outlineWidth), "2px");
        if (shots) await page.screenshot({ path: `${shots}/landing-${lang}-forced-colors.png`, fullPage: true });
      }));
      await test(`${lang}: reduced motion and missing screenshot alternative`, () => withPage({ lang, width: 390, reducedMotion: "reduce", missingScreenshot: true }, async (page) => {
        await page.locator(".shot").scrollIntoViewIfNeeded();
        await page.waitForFunction(() => !document.getElementById("shot-fallback").hidden);
        assert.equal(await page.locator("#console-shot").isVisible(), false);
        assert.equal(await page.locator("html").evaluate((node) => getComputedStyle(node).scrollBehavior), "auto");
        assert.match(await page.locator("#shot-fallback").innerText(), lang === "zh" ? /控制台/ : /Console/);
        await layout(page);
      }));
    }
    await test("no-JS English keeps mobile anchors and release fallbacks usable", () => withPage({ javaScriptEnabled: false, query: "?lang=zh", width: 390 }, async (page) => {
      assert.equal(await page.locator("html").getAttribute("lang"), "en");
      assert.equal(await page.locator("#lang-toggle").isVisible(), false);
      assert.equal(await page.locator("#dl-installer").getAttribute("href"), releasePage);
      for (const id of ["capabilities", "surfaces", "download"]) assert.equal(await page.locator(`#${id}`).count(), 1);
      assert.equal(await page.locator(".nav-links").isVisible(), true);
      await layout(page);
    }));
    console.log(`landing browser fixtures: ${passed} pass across ${opened} isolated contexts; release data is synthetic, no publication/download/contact requests`);
  } catch (error) {
    console.error(`landing browser fixtures: ${passed} passed before first failure across ${opened} contexts\n${error.stack}`);
    process.exitCode = 1;
  } finally { await browser.close(); }
}

if (process.argv.includes("--unit")) await unitGate();
else await browserGate();
