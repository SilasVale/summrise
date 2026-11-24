/**
 * Device-page measurements, deliberately separate from business-state assertions.
 * The driver serializes captureDeviceUiSnapshot.toString(): keep every browser helper inside it.
 * No text, input values, URLs with queries, attributes containing payloads, or environment is returned.
 * A passing result is a bounded DOM audit, not whole-page visual or assistive-technology acceptance.
 */
export function captureDeviceUiSnapshot(options = {}) {
  try {
    const selector = el => {
      const parts = [];
      for (let node = el; node && node.nodeType === 1; node = node.parentElement) {
        const tag = node.localName;
        const siblings = node.parentElement ? [...node.parentElement.children].filter(s => s.localName === tag) : [node];
        parts.unshift(`${tag}:nth-of-type(${siblings.indexOf(node) + 1})`);
      }
      return parts.join('>');
    };
    const rect = el => {
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height };
    };
    const visuallyHidden = el => {
      for (let node = el; node; node = node.parentElement) {
        const cs = getComputedStyle(node);
        if (cs.display === 'none' || cs.visibility === 'hidden' || cs.visibility === 'collapse' || Number(cs.opacity) === 0) return true;
        if (node.hidden || cs.contentVisibility === 'hidden') return true;
        if (cs.clipPath === 'inset(50%)' || /^rect\(0px,? 0px,? 0px,? 0px\)$/.test(cs.clip)) return true;
        if (node.tagName === 'DETAILS' && !node.open) {
          const summary = [...node.children].find(c => c.tagName === 'SUMMARY');
          if (el !== node && (!summary || (el !== summary && !summary.contains(el)))) return true;
        }
      }
      return false;
    };
    const visible = el => !visuallyHidden(el) && el.getClientRects().length > 0 && rect(el).width > 0 && rect(el).height > 0;
    const layers = el => {
      const result = [];
      for (let node = el; node; node = node.parentElement) {
        const cs = getComputedStyle(node);
        // Preserve image-presence evidence, never a CSS URL that could contain credentials.
        result.unshift({ backgroundColor: cs.backgroundColor, backgroundImage: cs.backgroundImage === 'none' ? 'none' : 'present', opacity: Number(cs.opacity) });
      }
      return result;
    };
    const nonblank = node => !!node && !!node.textContent?.trim();
    const contentName = el => {
      if (el.matches('[aria-hidden="true"],script,style') || getComputedStyle(el).display === 'none') return false;
      if (el.getAttribute('aria-label')?.trim() || (el.matches('img') && el.getAttribute('alt')?.trim())) return true;
      return [...el.childNodes].some(node => node.nodeType === 3 ? !!node.textContent.trim() : node.nodeType === 1 && contentName(node));
    };
    const labelled = el => {
      const refs = (el.getAttribute('aria-labelledby') || '').trim().split(/\s+/).filter(Boolean).map(id => document.getElementById(id));
      const labels = [...(el.labels || [])];
      return {
        named: !!el.getAttribute('aria-label')?.trim() || refs.some(nonblank) || labels.some(contentName)
          || (el.matches('button,summary,a,[role="button"],[role="link"]') && contentName(el))
          || (el.matches('input[type="button"],input[type="submit"],input[type="reset"]') && !!el.value?.trim()),
        visibleLabel: labels.some(label => nonblank(label) && visible(label)) || refs.some(label => nonblank(label) && visible(label)),
      };
    };
    const all = [...document.querySelectorAll('*')];
    const visibleElements = all.filter(visible);
    // The skip link is intentionally clipped until focused; inspect its DOM order without exposing href.
    const skipLink = document.querySelector('a.device-skip');
    const skipHref = skipLink?.getAttribute('href') || '';
    const skipTarget = skipHref.startsWith('#') ? document.getElementById(skipHref.slice(1)) : null;
    const keyboardCandidates = all.filter(el => !el.disabled && !el.closest('[inert]') && el.tabIndex >= 0
      && (visible(el) || el === skipLink));
    const controlElements = visibleElements.filter(el => el.matches('button,input:not([type="hidden"]),select,textarea,summary,a[href],[role="button"],[role="link"]') || (el.hasAttribute('tabindex') && el.tabIndex >= 0));
    const controls = controlElements.map(el => {
      let target = el;
      if (el.matches('input[type="checkbox"],input[type="radio"]') && el.labels?.length) target = [...el.labels].find(visible) || el;
      const name = labelled(el);
      const sizeRequired = el.matches('button,input,select,textarea,summary,.button,.btn,.local-nav a,[role="button"]');
      return {
        selector: selector(el), kind: el.localName, height: rect(target).height, sizeRequired,
        named: name.named, visibleLabel: name.visibleLabel,
        keyboard: el.disabled || (!el.closest('[inert]') && el.tabIndex === 0),
        ariaHidden: !!el.closest('[aria-hidden="true"]'), disabled: !!el.disabled,
      };
    });
    const texts = [];
    for (const el of visibleElements) {
      if (el.matches('script,style,noscript,option,svg,svg *,canvas') || el.closest('.xterm')) continue;
      if (![...el.childNodes].some(n => n.nodeType === 3 && n.textContent.trim())) continue;
      const cs = getComputedStyle(el);
      const important = el.closest('summary,button,[role="button"],[role="status"],[role="alert"],.chip,.receipt,.effects,.enroll-status,.timeline-error,.step-outcome');
      // Secondary context inherits through wrappers; critical actions/status/evidence still win.
      let metadata = !!el.closest('dt,time,small,footer,[data-text-role="meta"],aside.local-sidebar > p.local-scope');
      for (let node = el; node && !important; node = node.parentElement) {
        if (/(?:^|[\s-])(?:meta|hint|eyebrow|label|time|by|holder|caller|dim|note|badge|kind|statusbar)(?:[\s-]|$)/.test(node.getAttribute('class') || '')) metadata = true;
      }
      texts.push({ selector: selector(el), fontSize: parseFloat(cs.fontSize), category: important || !metadata ? 'key' : 'meta', color: cs.color, layers: layers(el) });
    }
    const panes = [];
    const boxes = [];
    const width = document.documentElement.clientWidth;
    const intentionalContainer = el => el.closest('pre,.xterm');
    const addBox = (el, r, includeSelf = false) => {
      const intentionalRoot = intentionalContainer(el), rootRect = intentionalRoot ? rect(intentionalRoot) : r;
      const edges = { limitLeft: 0, limitRight: width, limitTop: null, limitBottom: null };
      let xResolved = false, yResolved = false, xScroll = false;
      for (let node = includeSelf ? el : el.parentElement; node; node = node.parentElement) {
        const cs = getComputedStyle(node), bounds = rect(node);
        if (!xResolved && ['auto', 'scroll', 'hidden', 'clip'].includes(cs.overflowX)) {
          xResolved = true; xScroll = ['auto', 'scroll'].includes(cs.overflowX);
          if (!xScroll) { edges.limitLeft = bounds.left; edges.limitRight = bounds.right; }
        }
        if (!yResolved && ['auto', 'scroll', 'hidden', 'clip'].includes(cs.overflowY)) {
          yResolved = true;
          if (['hidden', 'clip'].includes(cs.overflowY)) { edges.limitTop = bounds.top; edges.limitBottom = bounds.bottom; }
        }
      }
      // A scroll ancestor makes horizontal contents reachable; its pane is measured separately.
      // Vertical clipping remains measurable even within a horizontally scrolling pane.
      if (xScroll) { edges.limitLeft = r.left; edges.limitRight = r.right; }
      boxes.push({ selector: selector(el), left: r.left, right: r.right, top: r.top, bottom: r.bottom, ...edges, intentional: !!intentionalRoot && el !== intentionalRoot, containerLeft: rootRect.left, containerRight: rootRect.right });
    };
    for (const el of visibleElements) {
      if (el.matches('html,body,svg,svg *,canvas')) continue;
      const cs = getComputedStyle(el), r = rect(el);
      const intentionalRoot = intentionalContainer(el);
      const rootRect = intentionalRoot ? rect(intentionalRoot) : null;
      const containerLeft = rootRect?.left ?? r.left, containerRight = rootRect?.right ?? r.right;
      if (cs.overflowX === 'auto' || cs.overflowX === 'scroll') panes.push({ selector: selector(el), clientWidth: el.clientWidth, scrollWidth: el.scrollWidth, intentional: !!intentionalRoot, left: r.left, right: r.right, containerLeft, containerRight });
      addBox(el, r);
      if (!el.closest('.xterm')) for (const node of el.childNodes) {
        if (node.nodeType !== 3 || !node.textContent.trim()) continue;
        const range = document.createRange();
        range.selectNodeContents(node);
        for (const textRect of range.getClientRects()) addBox(el, textRect, true);
      }
    }
    // Ignore only this menu's native closed-content suppression. CSS/HTML/ARIA hiding,
    // inertness and any other closed ancestor remain independent availability failures.
    // This reads computed styles without changing open, style, hidden or any other DOM state.
    const navigationAvailability = (el, disclosure) => {
      const available = { displayed: true, visibility: true, unhidden: true, uninert: true,
        ariaExposed: true, painted: true, unclipped: true, contentVisible: true,
        noOtherClosedDisclosure: true };
      for (let node = el; node; node = node.parentElement) {
        const cs = getComputedStyle(node);
        available.displayed &&= cs.display !== 'none';
        available.visibility &&= !['hidden', 'collapse'].includes(cs.visibility);
        available.unhidden &&= !node.hidden;
        available.uninert &&= !node.hasAttribute('inert');
        available.ariaExposed &&= node.getAttribute('aria-hidden') !== 'true';
        available.painted &&= Number(cs.opacity) > 0;
        available.unclipped &&= cs.clipPath === 'none' && cs.clip === 'auto';
        available.contentVisible &&= cs.contentVisibility !== 'hidden';
        available.noOtherClosedDisclosure &&= node.tagName !== 'DETAILS' || node.open || node === disclosure;
      }
      return available;
    };
    const navigationLink = (link, disclosure) => {
      let path = 'other';
      try { const url = new URL(link.href); if (url.origin === location.origin && ['/panel', '/activity'].includes(url.pathname) && !url.search && !url.hash) path = url.pathname; } catch { /* unknown link stays other */ }
      return { path, current: link.getAttribute('aria-current') === 'page', visible: visible(link),
        named: labelled(link).named, keyboard: link.tabIndex === 0 && !link.closest('[inert],[aria-hidden="true"]'),
        availability: navigationAvailability(link, disclosure) };
    };
    const nav = [...document.querySelectorAll('aside.local-sidebar > nav.local-nav,details.local-mobile-menu > nav.local-nav')].filter(visible).map(el => ({
      selector: selector(el), kind: el.parentElement.matches('details.local-mobile-menu') ? 'mobile' : 'desktop',
      named: el.getAttribute('aria-label') === 'Local navigation', links: [...el.querySelectorAll('a[href]')].map(link => navigationLink(link, el.parentElement)),
    }));
    // Owned closed links retain structural/current-route/availability facts, separately from
    // visible navigation. Every phone fixture is also keyboard-opened and audited AFTER its
    // default-closed screenshot; the live interaction retains both actual route crossings.
    const navigationMenus = [...document.querySelectorAll('details.local-mobile-menu')].filter(visible).map(menu => {
      const summary = [...menu.children].find(el => el.tagName === 'SUMMARY');
      const ownedNavs = [...menu.children].filter(el => el.matches('nav.local-nav'));
      const ownedNav = ownedNavs.length === 1 ? ownedNavs[0] : null;
      return { selector: selector(menu), open: menu.open,
        ownsNav: !!ownedNav && menu.firstElementChild === summary,
        summaryVisible: !!summary && visible(summary), summaryNamed: !!summary && labelled(summary).named,
        summaryKeyboard: !!summary && summary.tabIndex === 0 && !summary.closest('[inert],[aria-hidden="true"]'),
        summaryHeight: summary ? rect(summary).height : 0,
        ownedNavigation: ownedNav ? { named: ownedNav.getAttribute('aria-label') === 'Local navigation',
          availability: navigationAvailability(ownedNav, menu),
          links: [...ownedNav.querySelectorAll('a[href]')].map(link => navigationLink(link, menu)) } : null };
    });
    const durations = value => value.split(',').map(v => parseFloat(v) * (v.trim().endsWith('ms') ? 0.001 : 1));
    const motions = visibleElements.map(el => {
      const cs = getComputedStyle(el);
      return { selector: selector(el), transitionDurations: durations(cs.transitionDuration), animationDurations: cs.animationName === 'none' ? [0] : durations(cs.animationDuration), scrollBehavior: cs.scrollBehavior };
    });
    const active = document.activeElement;
    let focus = null;
    if (active && active !== document.body && active !== document.documentElement) {
      const cs = getComputedStyle(active);
      // Outlines paint against the surrounding surface, outside the active element's own fill.
      focus = { selector: selector(active), visible: visible(active), focusVisible: active.matches(':focus-visible'), outlineStyle: cs.outlineStyle, outlineWidth: parseFloat(cs.outlineWidth), outlineOffset: parseFloat(cs.outlineOffset), color: cs.outlineColor, layers: layers(active.parentElement || active).concat({ backgroundColor: 'rgba(0, 0, 0, 0)', backgroundImage: 'none', opacity: Number(cs.opacity) }) };
    }
    return {
      schemaVersion: 1,
      viewport: { width: innerWidth, height: innerHeight, scale: visualViewport?.scale || 1 },
      document: { scrollWidth: document.documentElement.scrollWidth, clientWidth: width, bodyScrollWidth: document.body?.scrollWidth || 0 },
      media: { forcedColors: matchMedia('(forced-colors: active)').matches, reducedMotion: matchMedia('(prefers-reduced-motion: reduce)').matches },
      headings: visibleElements.filter(el => /^H[1-6]$/.test(el.tagName)).map(el => ({ selector: selector(el), level: Number(el.tagName.slice(1)) })),
      landmarks: visibleElements.filter(el => el.matches('main,[role="main"]')).map(el => ({ selector: selector(el), kind: 'main' })),
      nav, navigationMenus,
      skip: { exists: !!skipLink, first: keyboardCandidates[0] === skipLink, targetExists: !!skipTarget, targetMain: !!skipTarget?.matches('main,[role="main"]'), targetFocusable: !!skipTarget && (skipTarget.tabIndex >= 0 || skipTarget.hasAttribute('tabindex')) },
      controls, texts, panes, boxes, motions, focus,
    };
  } catch {
    return { schemaVersion: 1, captureError: 'dom-capture-failed' };
  }
}

function rgba(value) {
  if (typeof value !== 'string') return null;
  if (value === 'transparent') return [0, 0, 0, 0];
  const hex = /^#([\da-f]{6}|[\da-f]{8})$/i.exec(value);
  if (hex) return [0, 2, 4].map(i => parseInt(hex[1].slice(i, i + 2), 16) / 255).concat(hex[1].length === 8 ? parseInt(hex[1].slice(6), 16) / 255 : 1);
  const match = /^(rgba?|color)\((.*)\)$/.exec(value.trim());
  if (!match) return null;
  const srgb = match[1] === 'color';
  let body = match[2].trim();
  if (srgb) { if (!body.startsWith('srgb ')) return null; body = body.slice(5); }
  const tokens = body.replace(/[,/]/g, ' ').split(/\s+/);
  if (tokens.length !== 3 && tokens.length !== 4) return null;
  const channels = tokens.map((token, index) => {
    if (!/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)%?$/.test(token)) return NaN;
    const number = parseFloat(token);
    return token.endsWith('%') ? number / 100 : number / (index === 3 || srgb ? 1 : 255);
  });
  if (channels.length === 3) channels.push(1);
  return channels.every(c => Number.isFinite(c) && c >= 0 && c <= 1) ? channels : null;
}
function over(foreground, background) {
  const alpha = foreground[3] + background[3] * (1 - foreground[3]);
  if (!alpha) return [0, 0, 0, 0];
  return [0, 1, 2].map(i => (foreground[i] * foreground[3] + background[i] * background[3] * (1 - foreground[3])) / alpha).concat(alpha);
}
function pixel(layers, foreground = [0, 0, 0, 0]) {
  let result = foreground;
  for (const layer of [...layers].reverse()) {
    const background = rgba(layer.backgroundColor);
    if (!background || layer.backgroundImage !== 'none') return null;
    result = over(result, background);
    result[3] *= layer.opacity;
  }
  return over(result, [1, 1, 1, 1]);
}
function contrast(color, layers) {
  const foreground = rgba(color), background = pixel(layers);
  if (!foreground || !background) return null;
  const painted = pixel(layers, foreground);
  if (!painted) return null;
  const luminance = rgb => rgb.slice(0, 3).map(c => c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4).reduce((sum, c, i) => sum + c * [0.2126, 0.7152, 0.0722][i], 0);
  const a = luminance(painted), b = luminance(background);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

/** Node-side policy and math. Invalid/missing measurements fail closed; diagnostics never echo payloads. */
export function checkDeviceUiSnapshot(snapshot, options = {}) {
  const failures = [], metrics = {};
  const fail = (check, detail) => failures.push({ check, detail });
  const result = () => ({ failures, metrics });
  const object = v => v !== null && typeof v === 'object' && !Array.isArray(v);
  const finite = v => typeof v === 'number' && Number.isFinite(v);
  const positive = v => finite(v) && v > 0;
  const bool = v => typeof v === 'boolean';
  const str = v => typeof v === 'string';
  const array = (v, predicate) => Array.isArray(v) && v.every(x => object(x) && predicate(x));
  const availability = v => object(v) && ['displayed','visibility','unhidden','uninert','ariaExposed','painted','unclipped','contentVisible','noOtherClosedDisclosure'].every(k => bool(v[k]));
  const routeLink = l => str(l.path) && ['current','named','keyboard','visible'].every(k => bool(l[k]));
  const ownedNavigation = n => n === null || object(n) && bool(n.named) && availability(n.availability)
    && array(n.links, l => routeLink(l) && availability(l.availability));
  const diagnostic = v => typeof v === 'string' && /^[a-z][a-z0-9-]*:nth-of-type\([1-9]\d*\)(>[a-z][a-z0-9-]*:nth-of-type\([1-9]\d*\))*$/.test(v) ? v : 'element';
  const paint = v => Array.isArray(v) && v.length > 0 && v.every(l => object(l) && str(l.backgroundColor) && str(l.backgroundImage) && finite(l.opacity) && l.opacity >= 0 && l.opacity <= 1);
  if (!object(options) || (options.expectedH1Count !== undefined && (!Number.isInteger(options.expectedH1Count) || options.expectedH1Count < 0))
    || (options.viewportWidth !== undefined && !positive(options.viewportWidth)) || (options.zoom !== undefined && !positive(options.zoom))
    || ['forcedColors', 'reducedMotion', 'checkFocus'].some(k => options[k] !== undefined && !bool(options[k]))
    || (options.caseName !== undefined && !str(options.caseName))
    || (options.expectedRoute !== undefined && !['/panel', '/activity'].includes(options.expectedRoute))) {
    fail('options', 'Invalid audit options'); return result();
  }
  if (!object(snapshot) || snapshot.schemaVersion !== 1 || snapshot.captureError !== undefined
    || !object(snapshot.viewport) || !positive(snapshot.viewport.width) || !positive(snapshot.viewport.height) || !positive(snapshot.viewport.scale)
    || !object(snapshot.document) || !['scrollWidth', 'clientWidth', 'bodyScrollWidth'].every(k => finite(snapshot.document[k]) && snapshot.document[k] >= 0) || !positive(snapshot.document.clientWidth)
    || !object(snapshot.media) || !bool(snapshot.media.forcedColors) || !bool(snapshot.media.reducedMotion)
    || !array(snapshot.headings, h => str(h.selector) && Number.isInteger(h.level) && h.level >= 1 && h.level <= 6)
    || !array(snapshot.landmarks, l => str(l.selector) && l.kind === 'main')
    || !array(snapshot.nav, n => str(n.selector) && ['desktop','mobile'].includes(n.kind) && bool(n.named) && array(n.links, routeLink))
    || !array(snapshot.navigationMenus, m => str(m.selector) && ['open','ownsNav','summaryVisible','summaryNamed','summaryKeyboard'].every(k => bool(m[k])) && finite(m.summaryHeight) && m.summaryHeight >= 0 && ownedNavigation(m.ownedNavigation))
    || !object(snapshot.skip) || !['exists', 'first', 'targetExists', 'targetMain', 'targetFocusable'].every(k => bool(snapshot.skip[k]))
    || !array(snapshot.controls, c => str(c.selector) && str(c.kind) && finite(c.height) && c.height >= 0 && ['named', 'visibleLabel', 'keyboard', 'ariaHidden', 'disabled'].every(k => bool(c[k])) && (c.sizeRequired === undefined || bool(c.sizeRequired)))
    || !array(snapshot.texts, t => str(t.selector) && positive(t.fontSize) && ['key', 'meta'].includes(t.category) && str(t.color) && paint(t.layers)) || snapshot.texts.length === 0
    || !array(snapshot.panes, p => str(p.selector) && finite(p.clientWidth) && p.clientWidth >= 0 && finite(p.scrollWidth) && p.scrollWidth >= 0 && bool(p.intentional) && ['left', 'right', 'containerLeft', 'containerRight'].every(k => finite(p[k])))
    || !array(snapshot.boxes, b => str(b.selector) && ['left', 'right', 'top', 'bottom', 'limitLeft', 'limitRight', 'containerLeft', 'containerRight'].every(k => finite(b[k])) && ['limitTop', 'limitBottom'].every(k => b[k] === null || finite(b[k])) && bool(b.intentional))
    || !array(snapshot.motions, m => str(m.selector) && [m.transitionDurations, m.animationDurations].every(a => Array.isArray(a) && a.length > 0 && a.every(v => finite(v) && v >= 0)) && str(m.scrollBehavior))
    || !(snapshot.focus === null || (object(snapshot.focus) && str(snapshot.focus.selector) && bool(snapshot.focus.visible) && bool(snapshot.focus.focusVisible) && str(snapshot.focus.outlineStyle) && finite(snapshot.focus.outlineWidth) && finite(snapshot.focus.outlineOffset) && str(snapshot.focus.color) && paint(snapshot.focus.layers)))) {
    fail('snapshot', 'Missing, malformed, or unsupported DOM measurements'); return result();
  }
  const s = snapshot;
  metrics.viewportWidth = s.viewport.width;
  metrics.h1Count = s.headings.filter(h => h.level === 1).length;
  metrics.mainCount = s.landmarks.length;
  metrics.controlCount = s.controls.length;
  metrics.textCount = s.texts.length;
  const contained = p => p.containerLeft >= -1 && p.containerRight <= s.document.clientWidth + 1;
  const boundedPane = p => contained(p) && p.left >= -1 && p.right <= s.document.clientWidth + 1;
  metrics.intentionalScrollCount = s.panes.filter(p => p.intentional && boundedPane(p) && p.scrollWidth > p.clientWidth + 1).length;
  metrics.forcedColors = s.media.forcedColors;
  metrics.reducedMotion = s.media.reducedMotion;
  metrics.scale = s.viewport.scale;
  if (options.viewportWidth !== undefined && Math.abs(s.viewport.width - options.viewportWidth) > 1) fail('viewport', 'CSS viewport width differs from requested width');
  if (options.zoom !== undefined && Math.abs(s.viewport.scale - options.zoom) > 0.01) fail('zoom', 'Visual viewport scale differs from requested scale');
  if (options.forcedColors !== undefined && s.media.forcedColors !== options.forcedColors) fail('forced-colors', 'Forced-colors emulation does not match request');
  if (options.reducedMotion !== undefined && s.media.reducedMotion !== options.reducedMotion) fail('reduced-motion', 'Reduced-motion emulation does not match request');
  if (options.expectedH1Count === undefined ? metrics.h1Count > 1 : metrics.h1Count !== options.expectedH1Count) fail('headings', `Visible h1 count ${metrics.h1Count} violates page expectation`);
  if (s.landmarks.length !== 1) fail('main-landmark', `Expected one visible main landmark; measured ${s.landmarks.length}`);
  if (!Object.values(s.skip).every(Boolean)) fail('skip-link', 'Skip link must precede controls and resolve to a focusable main landmark');
  const routesValid = links => links.length === 2
    && ['/panel', '/activity'].every(path => links.filter(l => l.path === path).length === 1)
    && links.every(l => l.named && l.keyboard) && links.filter(l => l.current).length === 1
    && (options.expectedRoute === undefined || links.some(l => l.path === options.expectedRoute && l.current));
  const independentlyAvailable = v => Object.values(v).every(Boolean);
  const menusValid = s.navigationMenus.length <= 1 && s.navigationMenus.every(m =>
    m.ownsNav && m.summaryVisible && m.summaryNamed && m.summaryKeyboard && m.summaryHeight >= 32
    && m.ownedNavigation && m.ownedNavigation.named && independentlyAvailable(m.ownedNavigation.availability)
    && routesValid(m.ownedNavigation.links)
    && m.ownedNavigation.links.every(l => independentlyAvailable(l.availability) && l.visible === m.open));
  const visibleNavigation = s.nav.length === 1 && s.nav[0].named
    && s.nav[0].kind === (s.navigationMenus.length ? 'mobile' : 'desktop')
    && routesValid(s.nav[0].links) && s.nav[0].links.every(l => l.visible);
  const closedNativeMenu = s.nav.length === 0 && s.navigationMenus.length === 1 && !s.navigationMenus[0].open;
  const consistentMenu = s.navigationMenus.length === 0 || s.navigationMenus[0].open === (s.nav.length > 0);
  metrics.navigationMode = visibleNavigation ? 'visible' : closedNativeMenu && menusValid ? 'closed-native-menu' : 'unavailable';
  if (!menusValid || !consistentMenu || !(visibleNavigation || closedNativeMenu)) fail('navigation', 'Device navigation must expose both routes with the correct current page, or a named keyboard-operable native menu awaiting open-link verification');
  if (s.document.scrollWidth > s.document.clientWidth + 1 || s.document.bodyScrollWidth > s.document.clientWidth + 1) fail('document-overflow', `Document width ${Math.max(s.document.scrollWidth, s.document.bodyScrollWidth)} exceeds viewport ${s.document.clientWidth}`);
  for (const p of s.panes) if (p.scrollWidth > p.clientWidth + 1 && !(p.intentional && boundedPane(p))) fail('pane-overflow', `${diagnostic(p.selector)} has ${Math.round(p.scrollWidth - p.clientWidth)}px horizontal overflow`);
  for (const b of s.boxes) if (!(b.intentional && contained(b)) && (b.left < b.limitLeft - 1 || b.right > b.limitRight + 1 || (b.limitTop !== null && b.top < b.limitTop - 1) || (b.limitBottom !== null && b.bottom > b.limitBottom + 1))) fail('clipping', `${diagnostic(b.selector)} extends outside its reachable bounds`);
  for (const c of s.controls) {
    const name = diagnostic(c.selector);
    if (!c.named) fail('control-name', `${name} has no accessible-name source`);
    if (['input', 'select', 'textarea'].includes(c.kind) && !c.visibleLabel) fail('input-label', `${name} has no visible associated label`);
    if (c.sizeRequired !== false && c.height < 32) fail('control-size', `${name} click target is ${c.height.toFixed(2)}px high; expected at least 32px`);
    if (!c.disabled && !c.keyboard) fail('keyboard', `${name} is excluded from natural keyboard order or uses positive tabindex`);
    if (!c.disabled && c.ariaHidden) fail('hidden-focus', `${name} is interactive inside aria-hidden content`);
  }
  const ratios = [];
  for (const t of s.texts) {
    if (t.fontSize < (t.category === 'meta' ? 12 : 13)) fail(`${t.category}-text-size`, `${diagnostic(t.selector)} is ${t.fontSize}px; expected at least ${t.category === 'meta' ? 12 : 13}px`);
    const ratio = contrast(t.color, t.layers);
    if (ratio === null) fail('text-contrast-unmeasured', `${diagnostic(t.selector)} has unsupported paint; contrast requires visual review`);
    else { ratios.push(ratio); if (ratio < 4.5) fail('text-contrast', `${diagnostic(t.selector)} contrast ${ratio.toFixed(3)} is below 4.5`); }
  }
  metrics.minTextContrast = ratios.length ? Math.min(...ratios) : null;
  metrics.measuredTextContrastCount = ratios.length;
  if (options.checkFocus) {
    const f = s.focus;
    if (!f || !f.visible || !f.focusVisible || f.outlineStyle !== 'solid' || f.outlineWidth < 2 || f.outlineOffset < 2) fail('focus', 'Keyboard focus must be visible with a solid outline at least 2px wide and 2px offset');
    if (f) {
      const ratio = contrast(f.color, f.layers);
      metrics.focusContrast = ratio;
      if (ratio === null) fail('focus-contrast-unmeasured', `${diagnostic(f.selector)} focus paint needs visual review`);
      else if (ratio < 3) fail('focus-contrast', `${diagnostic(f.selector)} focus contrast ${ratio.toFixed(3)} is below 3`);
    }
  }
  if (s.media.reducedMotion) for (const m of s.motions) if (m.transitionDurations.some(v => v > 0) || m.animationDurations.some(v => v > 0) || m.scrollBehavior === 'smooth') fail('motion', `${diagnostic(m.selector)} retains motion under reduced-motion`);
  return result();
}
