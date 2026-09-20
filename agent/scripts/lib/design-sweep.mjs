// design-sweep — the shared core every UI's design sweep is built from.
//
// WHY IT EXISTS (round 59). Rounds 50, 56 and 58 wrote three sweep tools — the panel's, the
// console's and (ad hoc) the extension's — and by the third one the duplication was plain: the same
// in-page checks (headings, landmarks, overflow/clipping/slivers, accessible names, reflow), the
// same judge, the same report shape, three times over. Three copies of a *measurement* is worse than
// three copies of ordinary code: when one copy learns something the others stay wrong.
//
// The history of those lessons is exactly why this file exists:
//   * an ellipsis is NOT clipped text (a detector that cries wolf stops being read);
//   * a login gate legitimately has no `nav`, while a page must have exactly one `main`;
//   * a gradient is judged by its WORST stop (the probe's own rule, reused here);
//   * a sweep that reads nothing is not a sweep that found nothing.
//
// Each UI supplies only what is genuinely its own: the URL to serve, the page list, the API
// fixtures, and whether it has a nav at all.

/** The in-page checks, as source text for the emitted browser script.
 *
 *  A FUNCTION OF THE ROOT SELECTOR, not a constant. These checks are evaluated IN THE PAGE by
 *  `page.evaluate(string)`, so an identifier from the Node side (the obvious `const ROOT_SEL`) is
 *  simply undefined there — measured: the extension sweep died on `ROOT_SEL is not defined` while
 *  the Node script defined it perfectly well. Inlining the selector at emit time makes that
 *  impossible to get wrong. */
export function pageChecks(rootSelector) {
  const text = PAGE_CHECKS_TEMPLATE.replaceAll("ROOT_SEL", JSON.stringify(rootSelector));
  // EVERY PROBE GOES OUT AS A JSON STRING, not as a template literal (round 57). The emitted script does not run
  // this text — it WRITES it into a file, and a probe left as a template literal there loses one more level of
  // escaping on the way to the page: `/^color\(/` in the emitted file reached the browser as `/^color(/` and threw
  // "Unterminated group", and the same mechanism turned `\s` into `s` for thirty-seven rounds. JSON.stringify has no
  // levels to lose, which is why the contrast probe has been shipped this way since round 88.
  return text.replace(/const (\w+) = `([\s\S]*?)`;/g, (_, name, body) => `const ${name} = ${JSON.stringify(body)};`);
}

const PAGE_CHECKS_TEMPLATE = `
const SURFACE = \`(() => {
  const desc = (el) => el.tagName.toLowerCase() + (typeof el.className === 'string' && el.className.trim() ? '.' + el.className.trim().split(/\\\\s+/).slice(0,2).join('.') : '') + (el.id ? '#' + el.id : '');
  const heads = [...document.querySelectorAll('h1,h2,h3,h4')];
  const lv = heads.map((e) => Number(e.tagName.slice(1)));
  let skipped = 0;
  for (let i = 1; i < lv.length; i++) if (lv[i] - lv[i - 1] > 1) skipped++;
  const over = [], clipped = [], slivers = [];
  for (const el of document.querySelectorAll(ROOT_SEL + ' *')) {
    const st = getComputedStyle(el);
    if (st.display === 'none' || st.visibility === 'hidden') continue;
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) continue;
    const own = [...el.childNodes].some((n) => n.nodeType === 3 && (n.textContent || '').trim().length > 0);
    const scrolls = st.overflowX === 'auto' || st.overflowX === 'scroll';
    const ellipsises = st.textOverflow === 'ellipsis';
    if (el.scrollWidth > el.clientWidth + 1 && !scrolls && !ellipsises) over.push(desc(el) + ' ' + el.clientWidth + '<' + el.scrollWidth);
    if (own && el.scrollWidth > el.clientWidth + 1 && !ellipsises) clipped.push(desc(el));
    const text = (el.textContent || '').trim();
    if (own && text.length > 24 && r.width < 60) slivers.push(desc(el) + ' w=' + Math.round(r.width));
  }
  const loudResult = (() => {
    // FOUR SYNTAXES, because the browser does not hand back the one this was written for (round 57). Besides
    // rgb(r, g, b) and rgba(r, g, b, a) it returns rgb(r g b / a) and — for any colour the sheet declares with a
    // modern function — color(srgb 0.09 0.09 0.11 / 0.88), whose components are 0-1 floats. The old parser read those
    // as raw 0-255 numbers, produced garbage, and (before the fail-closed guard above) counted them.
    // (No backticks in this comment: it is inside PAGE_CHECKS_TEMPLATE — 41st time.)
    const parse = (c) => {
      const m = /(?:rgba?|color)\\(([^)]+)\\)/.exec(c);
      if (!m) return null;
      const parts = m[1].split(/[\\s,/]+/).filter(Boolean);
      // ONLY THE NUMBERS: color(srgb 0.95 0.95 0.96 / 0.88) puts the COLOUR SPACE NAME first, and taking p[0] as r
      // made it NaN — which the fail-closed guard above then counted (six of them, measured round 59, every one of
      // them this one syntax). Filtering to finite numbers reads all four syntaxes with one rule.
      // (No backticks in this comment: it lives inside PAGE_CHECKS_TEMPLATE — 42nd time.)
      const p = parts.map(Number).filter(Number.isFinite);
      const srgb = /^color\\(/.test(c);
      const scale = srgb && p.length >= 3 && p[0] <= 1 && p[1] <= 1 && p[2] <= 1 ? 255 : 1;
      return { r: p[0] * scale, g: p[1] * scale, b: p[2] * scale, a: p.length > 3 ? p[3] : 1 };
    };
    const isLoud = ${loudnessOf.toString()};
    const loud = [];
    let unreadable = 0;
    const unreadableSamples = [];
    for (const el of document.querySelectorAll(ROOT_SEL + ' *')) {
      const st = getComputedStyle(el);
      if (st.display === 'none' || st.visibility === 'hidden' || Number(st.opacity) < 0.5) continue;
      const c = parse(st.backgroundColor);
      if (!c || c.a < 0.5) continue;
      const { sat, l, loud: shouts } = isLoud(c);
      if (!Number.isFinite(sat) || !Number.isFinite(l)) { unreadable++; if (unreadableSamples.length < 3) unreadableSamples.push(st.backgroundColor); continue; }
      if (!shouts) continue;
      const r = el.getBoundingClientRect();
      if (r.width < 14 || r.height < 12 || r.width * r.height < 400) continue;
      loud.push(desc(el) + ' ' + Math.round(r.width * r.height) + 'px2 ' + st.backgroundColor.replace(/\\s/g, ''));
    }
    return { list: [...new Set(loud)].slice(0, 6), unreadable, unreadableSamples };
  })();
  return {
    loud: loudResult.list,
    loudUnreadable: loudResult.unreadable,
    loudUnreadableSamples: loudResult.unreadableSamples,
    h1Count: heads.filter((e) => e.tagName === 'H1').length,
    firstIsH1: heads.length > 0 && heads[0].tagName === 'H1',
    skipped, mains: document.querySelectorAll('main').length, navs: document.querySelectorAll('nav').length,
    over: [...new Set(over)].slice(0, 8), clipped: [...new Set(clipped)].slice(0, 8), slivers: [...new Set(slivers)].slice(0, 8),
    // ── HOW MANY THINGS ON THIS PAGE ARE SHOUTING ─────────────────────────────────────────────────────────
    // "One focal point per surface" is the last clause of the spine and the only one with no continuous check:
    // it was measured by hand on four surfaces (panel Terminal 1, panel Settings 0, console Overview 0, landing 1
    // — the download CTA) and then not measured again. LOUD is an element whose FILL is genuinely saturated
    // (not white, black or grey) and big enough to be a surface rather than a dot. ONE is a page with something
    // to say; ZERO is a page that is all context, which is right for a form or a dashboard; TWO means nothing on
    // it is the focal point, because two things are asking to be looked at first.
    // ── THE MARK LANGUAGE, AS THE BROWSER ACTUALLY PAINTS IT ────────────────────────────────────────────────
    // The silhouettes are asserted against the SHEET by unit tests, and round 25 showed what that cannot see: a
    // rule later in the cascade overrode '.plug-dot[error]''s diamond and left a stray halo around it. The sheet
    // was right and the page was wrong. This reads the COMPUTED style of every state mark on the page, groups by
    // family, and reports any family whose states share a shape.
    //
    // A FAMILY is a mark's class without its state qualifier ('.cmd-dot[data-state="fail"]' → '.cmd-dot'), and a
    // STATE is whatever the element carries: 'data-state', 'data-live', or the second class. The signature is the
    // geometry that survives colour blindness — radius, rotation, and whether it is a fill, a ring or a haloed
    // fill — because colour is the SECOND channel and this check exists for the user who cannot read it.
    marks: (() => {
      const families = new Map();
      for (const el of document.querySelectorAll(ROOT_SEL + ' *')) {
        const st = getComputedStyle(el);
        if (st.display === 'none' || st.visibility === 'hidden') continue;
        const r = el.getBoundingClientRect();
        if (r.width < 4 || r.height < 4 || r.width > 40 || r.height > 40) continue;
        const cls = typeof el.className === 'string' ? el.className.trim().split(/\\s+/) : [];
        const state = el.getAttribute('data-state') || el.getAttribute('data-live');
        // the family is the class the STATE rules hang off: with a data-attribute it is the first class, with a
        // modifier class it is everything except the last one
        const base = state ? cls[0] : cls.length > 1 ? cls.slice(0, -1).join('.') : null;
        const which = state || (cls.length > 1 ? cls[cls.length - 1] : null);
        if (!base || !which) continue;
        if (!/\\.(dot|dotcol|mark|led|chip|signal|state)$|(dot|led|mark)$/.test(base)) continue;
        const bg = st.backgroundColor;
        // ZERO ALPHA IS NOT A FILL, IN WHATEVER SPELLING THE BROWSER RETURNS (round 76). This test excluded exactly
        // two strings — "transparent" and "rgba(0, 0, 0, 0)" — which are the two forms the SHEETS write. A COMPUTED
        // style returns a third: "color(srgb 0 0 0 / 0)", and a transparent ring was therefore read as a fill inside
        // its own ring. Six CI findings against the console's device LED, whose off state is a ring and correct.
        // Reading the components instead of matching strings covers all the syntaxes, and it cannot mistake a black
        // CHANNEL for an alpha: three components means opaque, whatever they are.
        const noFill = (c) => {
          if (/^transparent$/i.test(c)) return true;
          const inner = /\(([^)]*)\)/.exec(c);
          if (!inner) return false;
          const parts = inner[1].split(/[\s,/]+/).filter(Boolean);
          return parts.length > 3 && Number(parts[3]) === 0;
        };
        const filled = !!bg && !noFill(bg);
        const shadow = st.boxShadow;
        // DEFINED HERE, AND MISSING FOR NINE ROUNDS (round 55). The kind expression below has used 'inset' since
        // round 46 and nothing ever declared it — so this probe threw ReferenceError the moment it ran, and the
        // sweep's marks axis would have died in CI on the next push. It survived because round 46 verified the RULE
        // with a reimplementation on the device instead of running THIS probe, and the judge's self-test feeds the
        // judge a synthetic report rather than the probe's output. A reimplementation is not a test of the original.
        // (No backticks: this text is inside PAGE_CHECKS_TEMPLATE, and one would end the template — 40th time.)
        const inset = /inset/.test(shadow);
        // A FILL AND A RING AT ONCE IS ITS OWN KIND (round 46). Until now 'inset' won outright, so a mark that set a
        // background and inherited an inset shadow computed as 'ring' — distinct from a solid, and therefore passing.
        // That is how four broken plugin dots survived every sweep: the panel's own shape check had the same hole
        // (round 45), the console's found it by mutation (round 44), and this probe reported them as clean rings.
        const kind = inset && filled ? 'ring+fill' : inset ? 'ring' : filled && shadow !== 'none' ? 'halo' : filled ? 'solid' : 'empty';
        const sig = [st.borderTopLeftRadius, st.transform === 'none' ? 'flat' : 'rotated', kind].join('/');
        const key = base;
        if (!families.has(key)) families.set(key, new Map());
        families.get(key).set(which, sig);
      }
      const collisions = [];
      for (const [fam, states] of families) {
        if (states.size < 2) continue;
        const bySig = new Map();
        for (const [state, sig] of states) {
          if (bySig.has(sig)) collisions.push(fam + ': ' + bySig.get(sig) + ' and ' + state + ' paint identically (' + sig + ')');
          else bySig.set(sig, state);
        }
      }
      const ringFill = [];
      for (const [fam, states] of families) {
        for (const [state, sig] of states) if (sig.indexOf('ring+fill') >= 0) ringFill.push(fam + '[' + state + ']');
      }
      return { families: [...families].map(([f, m]) => f + '[' + [...m.keys()].join(',') + ']'), collisions: collisions.slice(0, 6), ringFill: ringFill.slice(0, 6) };
    })(),
  };
})()\`;

const NAMES = \`(() => {
  const SEL = 'button, a[href], input, select, textarea, [role="button"], [role="tab"], [role="switch"], [role="checkbox"], [role="link"]';
  const name = (el) => {
    const by = el.getAttribute('aria-labelledby');
    if (by) { const t = by.split(/\\\\s+/).map((id) => (document.getElementById(id) || {}).textContent || '').join(' ').trim(); if (t) return t; }
    const label = el.getAttribute('aria-label'); if (label && label.trim()) return label.trim();
    if (el.tagName === 'INPUT' || el.tagName === 'SELECT' || el.tagName === 'TEXTAREA') {
      if (el.id) { const l = document.querySelector('label[for="' + CSS.escape(el.id) + '"]'); if (l && l.textContent.trim()) return l.textContent.trim(); }
      const wrap = el.closest('label'); if (wrap && wrap.textContent.trim()) return wrap.textContent.trim();
      if (el.tagName === 'INPUT' && (el.type === 'submit' || el.type === 'button') && el.value) return el.value;
    }
    const text = (el.textContent || '').trim(); if (text) return text;
    // AN IMAGE WITH ALT TEXT NAMES ITS LINK (measured: the console's rail brand read as "title-only"
    // until this branch existed — a false positive in the detector, not a defect in the page).
    const img = el.querySelector('img[alt]'); if (img && img.alt.trim()) return img.alt.trim();
    const title = el.getAttribute('title'); if (title && title.trim()) return 'title-only: ' + title.trim();
    return '';
  };
  const unnamed = [], titleOnly = [];
  let checked = 0;
  for (const el of document.querySelectorAll(SEL)) {
    const st = getComputedStyle(el);
    if (st.display === 'none' || st.visibility === 'hidden') continue;
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) continue;
    if (el.getAttribute('aria-hidden') === 'true') continue;
    checked++;
    const n = name(el);
    const d = el.tagName.toLowerCase() + (typeof el.className === 'string' && el.className.trim() ? '.' + el.className.trim().split(/\\\\s+/)[0] : '') + (el.id ? '#' + el.id : '');
    if (!n) unnamed.push(d);
    else if (n.startsWith('title-only:')) titleOnly.push(d + ' -> ' + n.slice(11));
  }
  return { checked, unnamed: [...new Set(unnamed)], titleOnly: [...new Set(titleOnly)] };
})()\`;

const REFLOW = \`(() => ({
  docScrollWidth: document.documentElement.scrollWidth,
  viewport: window.innerWidth,
  docScrollsSideways: document.documentElement.scrollWidth > window.innerWidth + 1,
  sideScrollers: [...new Set([...document.querySelectorAll(ROOT_SEL + ' *')]
    .filter((el) => {
      const st = getComputedStyle(el);
      if (st.display === 'none' || st.visibility === 'hidden') return false;
      const r = el.getBoundingClientRect();
      return r.width >= 40 && r.height >= 20 && el.scrollWidth > el.clientWidth + 2 && (st.overflowX === 'auto' || st.overflowX === 'scroll');
    })
    .map((el) => el.tagName.toLowerCase() + (typeof el.className === 'string' && el.className.trim() ? '.' + el.className.trim().split(/\\\\s+/)[0] : '') + ' ' + el.clientWidth + '<' + el.scrollWidth))].slice(0, 8),
}))()\`;
`;

/** Every class on screen that no parsed rule styles — asked of the BROWSER (CSSOM), not of the
 *  stylesheet read as text.
 *
 *  WHY IT LIVES HERE AND NOT IN AN ADAPTER: round 88 wrote this inline in the console sweep's emitted
 *  template, and on the device the pattern `/\\s+/` arrived as `/s+/` — so class names were split on
 *  the letter "s" and the report listed "btn btn-" and "rail-clu" as unstyled, with 38 classes found
 *  where the browser sees 221. A string that is embedded in another string needs escaping that a
 *  template literal does not give it; `JSON.stringify` does, which is why the contrast probe has been
 *  shipped this way all along.
 *
 *  `styledClasses` is returned BESIDE the list on purpose: a read that found no stylesheets proves
 *  nothing, and a caller must be able to tell that apart from a page with nothing to report. */
export const UNSTYLED_SOURCE = `(() => {
  const styled = new Set();
  const collect = (rules) => {
    for (let i = 0; i < rules.length; i++) {
      const r = rules[i];
      if (r.selectorText) {
        const m = r.selectorText.match(/\\.([A-Za-z_][\\w-]*)/g);
        if (m) for (const s of m) styled.add(s.slice(1));
      }
      if (r.cssRules && r.cssRules.length) collect(r.cssRules);
    }
  };
  // AN UNREADABLE SHEET IS NOT AN ABSENT ONE. A sheet the page cannot hand over (cross-origin, or a Rules
  // object the browser refuses) contributes no class names, so every class it styles looks UNSTYLED — a
  // false finding produced by a check that could not read its own basis. Counted and returned instead of
  // swallowed, so the judge can say the basis was incomplete rather than reporting a clean sheet.
  let sheetsUnreadable = 0;
  for (let i = 0; i < document.styleSheets.length; i++) {
    try { collect(document.styleSheets[i].cssRules); } catch (e) { sheetsUnreadable++; }
  }
  const unstyled = new Map();
  for (const el of document.querySelectorAll('#root *')) {
    const cls = typeof el.className === 'string' ? el.className : '';
    for (const c of cls.split(/\\s+/).filter(Boolean)) if (!styled.has(c)) unstyled.set(c, el.tagName.toLowerCase());
  }
  return { styledClasses: styled.size, sheetsUnreadable, classes: [...unstyled.keys()].sort(), tags: Object.fromEntries(unstyled) };
})()`;

/** The judge: one implementation of "is this report a defect", whatever UI produced it.
 *
 *  `opts.ignore` is a list of `{ match: RegExp, reason: string }` for findings this HARNESS cannot
 *  judge — never for findings that are inconvenient. Each suppression is printed with its reason, so
 *  a reader sees what was set aside and why rather than a clean line that hides it.
 */
/** THE FOCUS PROBE, in one place. It was written in the panel adapter, copied into the console's, and
 *  the copy kept the panel's two defects for two rounds after the panel's were fixed (rounds 133-135) —
 *  a strong check whose twin was stale. It is a shared source now, so the next fix lands once. The
 *  caller supplies the presses count and the navigation; this does the loop and the verdict.
 *
 *  Escaping to the body is COUNTED, not passed: a page with nothing focusable would otherwise report a
 *  clean sheet indistinguishable from a page with good rings. `judgeReport` fails a row that landed on
 *  nothing. */
export const FOCUS_SOURCE = `(() => {
  const el = document.activeElement;
  if (!el || el === document.body) return 'escaped';
  const st = getComputedStyle(el);
  const visible = (parseFloat(st.outlineWidth) > 0 && st.outlineStyle !== 'none') || (st.boxShadow && st.boxShadow !== 'none');
  return visible ? 'ok' : 'no-ring';
})()`;

/** THE SWEEP REPORTS ITSELF TO THE AGENT'S DIAGNOSTIC RING (round 196).
 *
 *  `terminal_diag_read` has returned an empty list for every call this session has ever made, and round 182
 *  established why: the ring works, is capped at 200, has roundtrip and multibyte tests — and NOTHING WRITES
 *  TO IT. Its documented design is "POST a diagnostic line from the calling client", so the client is the
 *  writer, and no client ever did.
 *
 *  What that costs was measured in round 181: a sweep call timed out, the report stayed stale for half an
 *  hour, and there was no way to tell a run that was still working from one that had been killed. Two lines —
 *  start and finish — retire that ambiguity, and the ring starts earning the place it already occupies.
 *
 *  Self-contained on purpose: the adapters inline this source into the emitted script, so it may not
 *  reference anything from this module. Failures are swallowed because a sweep must never die of bookkeeping:
 *  a device whose agent is down still needs its design measured. */
export const DIAG_SOURCE = `async function diag(line) {
  try {
    // NO REGEX AND NO BACKSLASHES, DELIBERATELY. The first version of this helper was written inside a
    // template literal and emitted as /tokens*:s*.../ — every backslash eaten by one of the three escaping
    // layers this file has — so it never matched, and the guard below returned silently. It took a direct
    // endpoint probe to find, because the helper is designed to swallow its own failures. Reading the token
    // by line prefix and the path with forward slashes (Node accepts them on Windows) removes both hazards
    // rather than counting backslashes correctly, which is the mistake this session has now made 29 times.
    const cfg = require("fs").readFileSync("D:/Vale/etc/config.yaml", "utf8");
    let token = "";
    for (const l of cfg.split(String.fromCharCode(10))) {
      const t = l.trim();
      if (t.indexOf("device_token") === 0) { token = t.slice(t.indexOf(":") + 1).trim().replace(/["']/g, ""); break; }
    }
    if (!token) return;
    await fetch("http://127.0.0.1:18080/api/tools/terminal_diag_write", {
      method: "POST",
      headers: { authorization: "Bearer " + token, "content-type": "application/json" },
      // FLAT, not wrapped in an arguments object. The agent's dispatch reads required fields at the TOP
      // LEVEL: a wrapped body answers 200 with invalid_params "missing required field: line", which round
      // 196 found by probing the endpoint after this helper's catch had swallowed it twice.
      body: JSON.stringify({ line: "sweep " + line }),
    });
  } catch (e) { /* a sweep must not die of bookkeeping */ }
}`;

/** WHICH VERDICTS TRUST A COMPUTED VALUE, AND WHY EACH ONE IS STILL HONEST (round 187).
 *
 *  Round 186 found the focus check reporting eighteen missing rings that the browser was painting — a
 *  computed style is not a painted pixel, and the check had been reading one and calling it the other. That
 *  is a CLASS, not an incident, so every other verdict in this library was audited against it:
 *
 *    * TARGET SIZE (2.5.8) is safe BY CONSTRUCTION: getBoundingClientRect returns the POST-transform box, so
 *      a control scaled down to 12px measures 12px. It was already paint-aware and did not need changing.
 *    * TYPE FLOOR could be fooled — getComputedStyle().fontSize is pre-transform, so a scale(0.5) would paint
 *      9px while reporting 18px — and the audit found 3 transformed elements in the panel with ZERO text
 *      leaves among them. No text in any of the three UIs is scaled, so the floor measures what it claims.
 *    * CONTRAST composites the element's own colours and opacity against its detected surface, so an
 *      ANCESTOR's opacity, a mix-blend-mode, a backdrop-filter or a filter would all invalidate the ratio.
 *      The audit found 25 text leaves in the console and not one with any of those in its ancestor chain.
 *    * UNSTYLED compares rendered class names against parsed selectors: no paint involved, nothing to fool.
 *
 *  Re-run the audit rather than believing this note if a UI ever gains a scale, a fade wrapper or a blend:
 *  the method is one page scan that walks every text leaf's ancestor chain and reports the four properties.
 */
/** THE FOCUS PASS, in one place, inlined into every adapter's emitted script by `.toString()` — the same
 *  trick PROBE_SOURCE uses. The loop is the part that drifted when it was copied: the panel's counted
 *  nothing and treated focus escaping to the body as a pass, and the console's copy kept both defects for
 *  two rounds after the panel's were fixed (rounds 133-135). It runs ON THE DEVICE because a Tab press
 *  must be a real one — a synthetic KeyboardEvent does not move focus. */
/**
 * THE PRESS, AS THE BROWSER RENDERS IT — one implementation, shared by every adapter (round 55).
 *
 * WHY IT EXISTS. `feedback-check.mjs` proves an `:active` RULE EXISTS in a sheet; it cannot see whether the press is
 * VISIBLE. Round 51 measured the panel's presses by hand and found the ACTIVE TAB dead — `.tab.active` and
 * `.tab:active` are both (0,2,0) and the state rule came later — and rounds 52-53 turned that class into a
 * sheet-level check. This is the other half: the same measurement, in the sweep, so a press that stops rendering is
 * caught on the page rather than in a stylesheet.
 *
 * WITHOUT CLICKING ANYTHING. `mouse.down()` then a read, then the pointer is MOVED OFF the element before
 * `mouse.up()`: releasing over the same control fires a click, and a pass that presses every button on every page
 * would navigate, close sessions and toggle the theme while measuring. The elements that answer a press are the same
 * ones that act on a click, which is exactly why the release has to happen somewhere else.
 *
 * A TARGET THAT IS NOT ON THE PAGE IS A NOTE, NOT A FINDING — the panel and the desktop render different controls —
 * but the caller records how many were measured, because a press pass that measured nothing is not a clean pass.
 */
/**
 * IDLE REPAINT, MEASURED AS DOM MUTATIONS (round 64).
 *
 * The objective lists idle repaint among the things a claim is verified by, and nothing measured it: `useNow` carries
 * the contract for one clock (round 62), and that is a unit test about one hook, not a measurement of the panel.
 *
 * WHY DOM MUTATIONS ARE THE RIGHT PROXY: React writes to the DOM only when the rendered output DIFFERS. So a panel
 * that re-renders on a timer while nothing has changed produces no mutations, and a panel that writes something is
 * writing something that changed. Under the harness's STATIC fixtures nothing ever changes, which makes the bar exact:
 * an idle panel should mutate NOTHING, and every mutation is either a clock or a re-render that recomputed a value
 * from unchanged inputs.
 *
 * NO REGEXES IN THIS FUNCTION, deliberately: it is inlined into the emitted script through `toString()`, and a single
 * backslash inside that template literal is eaten before the page sees it (rounds 55-58, four times).
 */
export async function idlePass(page, ms = 6000) {
  await page.evaluate(() => {
    const el = document.getElementById("root") || document.body;
    const state = { mutations: 0, byTarget: {}, samples: [] };
    window.__valeIdle = state;
    // A TEXT NODE HAS NO IDENTITY, SO THE REPORT NAMES ITS PARENT (round 68). The first CI run of this pass reported
    // "6 DOM mutation(s) ... (#text x6)" — one per second, which is a live duration ticking and NOT a repaint, but
    // the report could not say WHICH text, so the finding was undiagnosable by construction. A characterData
    // mutation is about the parent element as far as a reader is concerned.
    const name = (node) => {
      const el = node.nodeType === 3 ? node.parentElement || node : node;
      const tag = el.tagName ? el.tagName.toLowerCase() : "#node";
      const cls = el.className && typeof el.className === "string" ? el.className.trim().split(" ")[0] : "";
      return tag + (el.id ? "#" + el.id : cls ? "." + cls : "");
    };
    const observer = new MutationObserver((records) => {
      for (const r of records) {
        state.mutations++;
        const key = r.attributeName === "data-vale-idle-probe" ? "__probe" : name(r.target);
        state.byTarget[key] = (state.byTarget[key] || 0) + 1;
        if (state.samples.length < 8) state.samples.push(key + " " + r.type + (r.attributeName ? ":" + r.attributeName : ""));
      }
    });
    observer.observe(el, { subtree: true, childList: true, characterData: true, attributes: true });
    // THE INSTRUMENT PROVES IT IS ALIVE, because ZERO IS OTHERWISE UNFALSIFIABLE. An observer attached to the wrong
    // node, or a filter that matches nothing, reports a perfect idle panel forever — and "a scan that read nothing
    // is not a clean scan" is the trap this suite keeps catching. So the window opens with ONE deliberate mutation of
    // the panel's own root; it is counted like any other and subtracted by the judge, and a run that does not see it
    // is reported as a blind instrument rather than as a still panel.
    el.setAttribute("data-vale-idle-probe", String(Date.now()));
    window.__valeIdleStop = () => { observer.disconnect(); return { ...state, selfTest: state.byTarget.__probe !== undefined }; };
  });
  await page.waitForTimeout(ms);
  return page.evaluate(() => window.__valeIdleStop());
}

export async function pressPass(page, targets, label = {}) {
  const rows = [];
  const styleOf = (sel) => page.evaluate((s) => {
    for (const el of document.querySelectorAll(s)) {
      const r = el.getBoundingClientRect();
      const st = getComputedStyle(el);
      if (r.width < 6 || r.height < 6 || st.display === "none" || st.visibility === "hidden") continue;
      const cls = typeof el.className === "string" && el.className ? "." + el.className.trim().split(/\s+/).join(".") : "";
      return {
        where: el.tagName.toLowerCase() + cls + (el.id ? "#" + el.id : ""),
        transform: st.transform, opacity: st.opacity, background: st.backgroundColor, filter: st.filter,
      };
    }
    return null;
  }, sel);
  for (const sel of targets) {
    const box = await page.evaluate((s) => {
      for (const el of document.querySelectorAll(s)) {
        const r = el.getBoundingClientRect();
        const st = getComputedStyle(el);
        if (r.width < 6 || r.height < 6 || st.display === "none" || st.visibility === "hidden") continue;
        return { x: r.left + r.width / 2, y: r.top + r.height / 2, w: Math.round(r.width), h: Math.round(r.height) };
      }
      return null;
    }, sel);
    if (!box) { rows.push({ sel, note: "not rendered on this page" }); continue; }
    const before = await styleOf(sel);
    await page.mouse.move(box.x, box.y);
    await page.waitForTimeout(80);
    await page.mouse.down();
    await page.waitForTimeout(120);
    const pressed = await styleOf(sel);
    // OFF THE ELEMENT FIRST — see the note above: releasing here would click it.
    await page.mouse.move(box.x, Math.max(0, box.y - 80));
    await page.mouse.up();
    await page.waitForTimeout(60);
    const props = before && pressed ? ["transform", "opacity", "background", "filter"].filter((k) => before[k] !== pressed[k]) : [];
    rows.push({ sel, where: pressed ? pressed.where : before.where, size: box.w + "x" + box.h, changed: props.length > 0, props, before, pressed, ...label });
  }
  return rows;
}

export async function focusPass(page, presses, label = {}) {
  await page.evaluate(() => document.body.focus());
  let landed = 0;
  let escaped = 0;
  let missing = 0;
  // HOW MANY THE PIXELS RESCUED FROM A WRONG VERDICT. Reported rather than hidden: a number that keeps
  // climbing means the computed-style check is drifting further from what the browser paints.
  let paintConfirmed = 0;
  // HOW OFTEN THE CHECK COULD NOT LOOK. Separate from missing on purpose: see the catch below.
  let unconfirmed = 0;
  const unconfirmedOn = [];
  // AND WHICH ONES. A count alone leaves the next reader to re-derive the finding — round 136 got
  // "missing: 11" from the extension and could not tell a real defect from a broken probe. The offenders
  // name themselves instead, in the same tag.class form the rest of the suite uses.
  const missingOn = [];
  // THE FIRST OFFENDER'S COMPUTED STYLES, so a surprising count explains itself. Round 136 got
  // "missing: 11" from the extension and could not tell a real defect from a misreading; the extension's
  // sheet DOES carry a :focus-visible ring and an input:focus box-shadow, so the next reader needs the
  // numbers, not another guess.
  let evidence = null;
  for (let i = 0; i < presses; i++) {
    await page.keyboard.press("Tab");
    const verdict = await page.evaluate(() => {
      const el = document.activeElement;
      const name = (e) => {
        const cls = typeof e.className === "string" && e.className ? "." + e.className.trim().split(/\s+/).join(".") : "";
        return e.tagName.toLowerCase() + cls + (e.id ? "#" + e.id : "");
      };
      if (!el || el === document.body) return { verdict: "escaped", where: "body" };
      const st = getComputedStyle(el);
      const visible =
        (parseFloat(st.outlineWidth) > 0 && st.outlineStyle !== "none") ||
        (st.boxShadow && st.boxShadow !== "none");
      return {
        verdict: visible ? "ok" : "no-ring",
        where: name(el),
        outline: st.outlineStyle + " " + st.outlineWidth + " " + st.outlineColor,
        boxShadow: String(st.boxShadow).slice(0, 60),
        focusVisible: el.matches(":focus-visible"),
        ringToken: getComputedStyle(document.documentElement).getPropertyValue("--focus-ring").trim() || "(undefined)",
      };
    });
    if (verdict.verdict === "ok") landed++;
    else if (verdict.verdict === "escaped") escaped++;
    else {
      // A COMPUTED STYLE IS NOT A PAINTED RING, AND THIS IS WHERE THAT WAS PROVEN. Six rounds chased a
      // cascade that did not exist: a keyboard-focused console button reports "solid 0px" and boxShadow
      // "none" while the browser draws a 2px accent ring around it — confirmed by screenshot in round 186,
      // after a stale bundle, a cached sheet, a pointer leak, a missing !important and a layered-!important
      // hypothesis had each been tested and killed. getComputedStyle does not report what the UA paints for
      // :focus-visible on every control.
      //
      // SO THE STYLE CHECK IS A CANDIDATE, NOT A VERDICT, and the pixels are the authority. Capture the
      // element's neighbourhood while it is keyboard-focused, then blur and re-focus it programmatically —
      // which drops :focus-visible and therefore the ring — and compare. Different bytes mean something IS
      // painted and this was a false positive; identical bytes mean the control really has no focus
      // indication, which is the WCAG 2.4.7 failure this check exists to find. Only candidates pay for the
      // two screenshots, so a clean page costs nothing.
      let painted = false;
      let paintFailed = null;
      try {
        const box = await page.evaluate(() => {
          const e = document.activeElement;
          if (!e) return null;
          const r = e.getBoundingClientRect();
          return { x: Math.max(0, Math.floor(r.x - 6)), y: Math.max(0, Math.floor(r.y - 6)), width: Math.ceil(r.width + 12), height: Math.ceil(r.height + 12) };
        });
        if (box && box.width > 0 && box.height > 0) {
          const withRing = await page.screenshot({ clip: box });
          await page.evaluate(() => { const e = document.activeElement; if (e && e.blur) { e.blur(); e.focus(); } });
          const withoutRing = await page.screenshot({ clip: box });
          painted = !withRing.equals(withoutRing);
        }
        paintFailed = null;
      } catch (e) {
        // A CHECK THAT COULD NOT LOOK MUST NOT REPORT A FINDING, and must not report a pass either. The
        // first version set painted = false here, which turned a failed screenshot into "this control has no
        // focus indication" — the same false-finding shape that cost six rounds before round 186. Null means
        // UNCONFIRMED: counted separately, and treated by the judge as a failure of the CHECK, because a
        // measurement that did not happen is not evidence of anything (rounds 133-134's rule).
        paintFailed = String(e && e.message ? e.message : e).slice(0, 80);
      }
      if (painted === true) {
        landed++;
        paintConfirmed++;
      } else if (painted === null) {
        unconfirmed++;
        if (unconfirmedOn.length < 5) unconfirmedOn.push(verdict.where);
      } else {
        missing++;
        if (missingOn.length < 8) missingOn.push(verdict.where);
        if (!evidence) evidence = verdict;
      }
    }
  }
  return {
    ...label,
    pressed: presses,
    landed,
    escaped,
    missing,
    ...(paintConfirmed ? { paintConfirmed } : {}),
    ...(unconfirmed ? { unconfirmed, unconfirmedOn, ...(paintFailed ? { paintFailed } : {}) } : {}),
    ...(missingOn.length ? { missingOn } : {}),
    ...(evidence ? { why: { where: evidence.where, outline: evidence.outline, boxShadow: evidence.boxShadow, focusVisible: evidence.focusVisible, ringToken: evidence.ringToken } } : {}),
  };
}

/** THE MOTION MEASUREMENT, both states in order. A single reduced-motion number is vacuous — it looks
 *  the same whether the page honours the preference or has no motion at all (round 134). `render`
 *  re-renders the page and is supplied by the adapter. */
export async function motionPass(page, render, label = {}) {
  const count = async () => {
    const list = await page.evaluate(() => {
      const animating = [];
      for (const el of document.querySelectorAll("*")) {
        const st = getComputedStyle(el);
        const dur = parseFloat(st.transitionDuration) > 0 ? st.transitionDuration : null;
        const anim =
          st.animationName && st.animationName !== "none"
            ? st.animationName + " x" + st.animationIterationCount
            : null;
        if (!dur && !anim) continue;
        const key =
          typeof el.className === "string" && el.className
            ? "." + el.className.trim().split(/\s+/).join(".")
            : el.tagName.toLowerCase();
        animating.push(key + (dur ? " trans=" + dur : "") + (anim ? " anim=" + anim : ""));
      }
      return [...new Set(animating)];
    });
    return list;
  };
  await page.emulateMedia({ reducedMotion: null });
  await render();
  const normal = await count();
  await page.emulateMedia({ reducedMotion: "reduce" });
  await render();
  const reduced = await count();
  await page.emulateMedia({ reducedMotion: null });
  return { ...label, normal: normal.length, reduced: reduced.length, stillAnimating: reduced.slice(0, 6) };
}

/** WCAG 2.5.8 TARGET SIZE (MINIMUM), the FULL criterion — which is not "24x24 or fail".
 *
 *  The rule is: a target must be at least 24x24 CSS px, OR have enough SPACING that a 24px circle centred on
 *  it does not overlap another target's circle. Most compact UIs satisfy it through the second clause, and a
 *  check that ignored that would report a dozen false findings and be turned off within a week. So this
 *  measures the distance to the nearest other target and applies the criterion as written.
 *
 *  Nothing measured this before round 162. The sweep's own probe uses 24px for a different question (whether
 *  a non-text element is a MARK rather than a block), which is how the number was already in the codebase
 *  without the criterion being checked. */
/** WHAT THE PAGE ACTUALLY RENDERED, as opposed to what the navigation asked for.
 *
 *  Round 175 shipped a two-theme fixture whose REPORT said `theme: light` for every render, because the three
 *  blocks recorded a hardcoded field while only the URLs had been changed — so `Terminal-fail-dark` reported
 *  light, and two same-named surfaces looked like one. The renders were right and the report lied about them,
 *  which is worse than not measuring: a dark regression would have been filed under light and compared
 *  against the wrong numbers.
 *
 *  This reads the theme off the PAGE — the stored preference the app itself uses, plus the body background,
 *  which is what the eye sees. The adapters record what this returns and FAIL when it disagrees with what
 *  they navigated to, so the family of bug that cost round 175 cannot come back quietly.
 *
 *  Two signals rather than one deliberately: a stubbed localStorage could agree while the painted background
 *  does not, and `bodyBackground` is the half that a reader would actually notice. */
/** IS A SURFACE LOUD — a saturated fill that competes for the page's focus?
 *
 *  ONE RULE, TWO AXES (round 78). This computation lived only inside the emitted probe, so nothing could ask the
 *  same question of a TOKEN. The rendered axis found the dark info chip (`#1a3a5c`, saturation 0.559) on the one page
 *  that renders that badge while the light one is a pale tint the rule skips by design (lightness above 0.9) — and a
 *  sheet-level check can find the same thing everywhere, on both UIs, without a browser.
 *
 *  THE BANDS ARE MEASURED, NOT CHOSEN: saturation 0.35 with a lightness between 0.2 and 0.9 is what an operator reads
 *  as "something shouting". HSL saturation is d / (1 - |2l - 1|), which is the form the probe has always used — and
 *  the one the probe's own gate checks. (No backticks: this function is inlined into an emitted template.) */
export function loudnessOf(colour) {
  const [R, G, B] = [colour.r / 255, colour.g / 255, colour.b / 255];
  const mx = Math.max(R, G, B), mn = Math.min(R, G, B);
  const l = (mx + mn) / 2, d = mx - mn;
  const sat = d === 0 ? 0 : d / (1 - Math.abs(2 * l - 1));
  return { sat, l, loud: Number.isFinite(sat) && Number.isFinite(l) && sat >= 0.35 && l >= 0.2 && l <= 0.9 };
}

export const THEME_SOURCE = `(() => {
  const body = getComputedStyle(document.body).backgroundColor;
  let stored = '';
  try { stored = localStorage.getItem('vale-theme') || ''; } catch (e) { stored = '(unavailable)'; }
  // THE APP WRITES data-theme ON BODY, AND THIS READ html UNTIL ROUND 74. So the attr field came back "(none)" on every
  // surface this sweep has ever measured: the theme-lie axis had nothing to compare an intention against, and the
  // rail walk's new labels fell back to the loop's own value — which is how six contrast findings were filed against
  // the light theme while the pages were rendered dark. Body first, then the document element as a fallback for
  // surfaces that put it elsewhere. (No backticks: this source is embedded in an emitted template literal.)
  const attr = document.body.getAttribute('data-theme') || document.documentElement.getAttribute('data-theme') || '';
  return { stored, attr, bodyBackground: body };
})()`;

export const TARGETS_SOURCE = `(() => {
  const SEL = 'button, a[href], input:not([type="hidden"]), select, textarea, [role="button"], [role="tab"], [role="switch"], [role="checkbox"]';
  const els = [...document.querySelectorAll(SEL)].filter((el) => {
    const r = el.getBoundingClientRect();
    const st = getComputedStyle(el);
    return r.width > 0 && r.height > 0 && st.visibility !== 'hidden' && st.display !== 'none' && !el.disabled;
  });
  const name = (el) => {
    const cls = typeof el.className === 'string' && el.className ? '.' + el.className.trim().split(/\\s+/).slice(0, 2).join('.') : '';
    return el.tagName.toLowerCase() + cls;
  };
  const box = (el) => el.getBoundingClientRect();
  const centre = (r) => ({ x: r.left + r.width / 2, y: r.top + r.height / 2 });
  const dist = (a, b) => Math.hypot(a.x - b.x, a.y - b.y);
  const small = [];
  for (const el of els) {
    const r = box(el);
    if (r.width >= 24 && r.height >= 24) continue;
    // The spacing clause: a 24px circle centred here must not overlap another target's circle.
    const c = centre(r);
    let nearest = Infinity;
    for (const other of els) {
      if (other === el) continue;
      const d = dist(c, centre(box(other)));
      if (d < nearest) nearest = d;
    }
    small.push({
      sel: name(el),
      text: (el.getAttribute('aria-label') || el.textContent || '').trim().slice(0, 24),
      w: Math.round(r.width),
      h: Math.round(r.height),
      nearest: Number.isFinite(nearest) ? Math.round(nearest * 10) / 10 : null,
      // 24px circles overlap when their centres are closer than 24px.
      passesBySpacing: nearest >= 24,
    });
  }
  const bySel = new Map();
  for (const o of small) {
    const prev = bySel.get(o.sel);
    if (!prev || o.w * o.h < prev.w * prev.h) bySel.set(o.sel, o);
  }
  return { checked: els.length, undersized: small.length, distinct: [...bySel.values()] };
})()`;

export function judgeReport(report, opts = {}) {
  const findings = [];
  const notes = report.notes || (report.notes = []);
  const suppressed = [];
  for (const s of report.surfaces) {
    const where = s.width ? `${s.page}@${s.width}px` : s.page;
    if (s.h1Count !== 1 || !s.firstIsH1) findings.push(`${where}: h1 count ${s.h1Count}, first-is-h1 ${s.firstIsH1}`);
    if (s.skipped) findings.push(`${where}: ${s.skipped} skipped heading level(s)`);
    if (s.mains !== 1) findings.push(`${where}: ${s.mains} main landmark(s), expected exactly 1`);
    if (s.navs > 1) findings.push(`${where}: ${s.navs} nav landmarks — a page has one navigation`);
    // PREFIX, NOT EXACT. `navless: ["login"]` was an exact match on a page name, so when the console's login
    // page gained a dark render (`login-dark`, round 229) the page that has no navigation BY DESIGN was
    // reported as missing one. A navless entry names a FAMILY of renders — the same page in another theme, at
    // another width — and matching the name alone is the one-of-N shape this session keeps finding.
    else if (s.navs !== 1 && !(opts.navless || []).some((n) => String(s.page).startsWith(n))) {
      findings.push(`${where}: ${s.navs} nav landmark(s), expected exactly 1`);
    }
    for (const [kind, list] of [["overflow", s.over], ["clipping", s.clipped], ["sliver", s.slivers]]) {
      if (list && list.length) findings.push(`${where}: ${kind} — ${list.join("; ")}`);
    }
    // THE MARK LANGUAGE AS PAINTED. A family whose two states render identically is colour-only wherever a cascade
    // override or a missing rule made it so — the sheet can be right while the page is wrong, which is exactly how
    // `.plug-dot[error]` kept a stray halo through a unit test that passed (round 25).
    // A MARK THAT IS BOTH A FILL AND A RING IS NEITHER, and it is not a collision — no other state shares it, so the
    // distinctness check above would pass it. The vocabulary is solid / ring / halo / empty; this is what a rule
    // produces by accident when it sets a background and inherits an inset shadow, which is exactly the four
    // .plug-dot arms of round 45. Rendered, not read: the probe reports the computed kind per state.
    if (s.marks && (s.marks.ringFill || []).length) {
      findings.push(`${where}: ${s.marks.ringFill.length} mark(s) are a FILL inside a RING — the vocabulary is solid / ring / halo / empty, and a mark that is two of them is neither: ${s.marks.ringFill.join("; ")}`);
    }
    if (s.marks && (s.marks.collisions || []).length) {
      findings.push(`${where}: states of one mark paint identically — ${s.marks.collisions.join("; ")}`);
    }
    // ONE FOCAL POINT, AT MOST. Two loud surfaces means neither is the thing the page is about; the ceiling is
    // one, and zero is allowed because a form or a dashboard is all context and should not shout. A page that
    // needs an exception gets one here, by name, with the reason — the same shape as `navless`.
    // A COLOUR THE PROBE COULD NOT READ IS A FAILURE, not a skip: every comparison against NaN is false, so an
    // unreadable background used to pass every guard and be counted as loud (rounds 18-56, found in 55). Reporting
    // the count means the next occurrence is loud in the report instead of silently inflating it.
    // REPORTED, NOT FAILED — for now. Six backgrounds across 24 surfaces (measured round 57) are in a syntax this
    // parser still cannot read; every other colour is read and the axis agrees with the hand measurements. A NOTE is
    // the honest severity: a failure would block every run on a residue nobody has looked at yet, and silence is what
    // let this axis report nonsense for thirty-seven rounds. The count rides in the summary so it cannot be ignored.
    if (s.loudUnreadable) {
      const samples = (s.loudUnreadableSamples || []).slice(0, 2).join("; ");
      notes.push(`${where}: ${s.loudUnreadable} background(s) in a colour syntax this probe cannot read — skipped, not counted${samples ? " (" + samples + ")" : ""}`);
    }
    // THE EXCEPTION IS ABOUT ELEMENTS, NOT PAGE NAMES (round 68). It used to except a whole page by name prefix,
    // which hid its siblings — CI rendered `Terminal-16-sessions`, `Desktop-16-sessions`, `Desktop-Terminal-fail-dark`
    // and `Desktop-empty`, all two-loud with the SAME rail button and the new-session action, and none of them was
    // covered. Naming the ELEMENTS cannot widen: a page that starts shouting about something else still fails, which
    // is what round 42's own note warned a page-name exception could never do.
    const NAV_LOUD = [
      { match: /rail-btn|desktop-rail-btn/, why: "the rail button is WHICH PAGE YOU ARE ON (navigation state)" },
      { match: /^div\.tab|^button\.dtab/, why: "the active session tab is WHICH SESSION (navigation state)" },
      { match: /btn-new/, why: "the primary action on the page, which state-colour-check protects deliberately" },
      // THE APPROVAL GATE, which `state-colour-check` lists as a PURPOSE in its own right (round 69). It appears
      // only while a command is waiting for the operator, and on that page it is not competing with the primary
      // action — it IS the primary action. CI's next run named it in a loud finding, which is how the omission
      // became visible: the exception had listed the three navigation and action elements and not this one.
      { match: /approval-approve/, why: "the approval gate's Approve — the one control that outranks the page's action while a command is held" },
    ];
    const navLoud = (s.loud || []).every((entry) => NAV_LOUD.some((n) => n.match.test(String(entry))));
    if ((s.loud || []).length > 1 && !navLoud) {
      findings.push(`${where}: ${s.loud.length} loud elements — a page has ONE focal point at most — ${s.loud.join("; ")}`);
    }
  }
  for (const n of report.names || []) {
    const where = `${n.page}${n.width ? "@" + n.width + "px" : ""}`;
    if (n.unnamed.length) findings.push(`${where}: ${n.unnamed.length} control(s) with NO accessible name — ${n.unnamed.join(", ")}`);
    if (n.titleOnly.length) findings.push(`${where}: ${n.titleOnly.length} control(s) named only by title — ${n.titleOnly.join(", ")}`);
  }
  // HOW OFTEN THE COMPUTED-STYLE VERDICT WAS OVERRULED BY THE PIXELS. Not a finding — the pixels are the
  // authority and they said the ring is painted — but it is the number to WATCH: it was 18 out of 18 in
  // round 186, where the style check called every console ring missing while the browser drew all of them,
  // and six rounds went by before anyone looked at a screenshot. A count that keeps climbing means the
  // style check is drifting further from what is painted, and the next drift may not be benign.
  // WHICH HARNESS GENERATION WAS MEASURED. Round 189 lost an afternoon to a delivered harness that
  // predated a CSS fix: its inlined stylesheet collapsed the tab strip to 17px, the sweep reported overflow
  // that looked like a live defect, and a waiver hid it. The stamp travels in the report and is printed
  // here, so a reader can see at a glance that the fixture is older than the build it should match.
  if (report.harnessBuild) console.log(`note: harness build ${report.harnessBuild}`);
  // AND WHEN IT IS NOT THE BUILD THE SWEEP EXPECTED, THE WHOLE REPORT IS SUSPECT. Round 189 measured a
  // delivered harness two generations old: its inlined CSS collapsed the tab strip to 17px and the sweep
  // reported overflow that looked exactly like a live regression. A finding is the right weight — nothing
  // below can be trusted until the fixture is regenerated.
  // A DELIVERED COPY OLDER THAN THE BUILD MEASURES SOMETHING NOBODY CAN NAME. Same weight as a stale panel
  // harness, and the same evidence-free failure mode: a UI whose CSS has moved on reports findings that look
  // live. The check travels with every console and extension report.
  // AND WHEN IT IS CURRENT, SAY SO. The panel prints its harness generation on every run, so a reader always
  // knows which artifact was measured; the console and the extension were silent about it unless something
  // was wrong. Provenance that only appears in a failure is provenance nobody can check.
  if (report.entryCheck && !report.entryCheck.stale && !report.entryCheck.error) {
    console.log(`note: delivered entry ${report.entryCheck.bytes} bytes / sha ${report.entryCheck.sha} — matches the build`);
  }
  if (report.entryCheck && report.entryCheck.stale) {
    const e = report.entryCheck;
    findings.push(
      e.error
        ? `the delivered entry could not be read (${e.error}) — expected ${e.expected.bytes} bytes, sha ${e.expected.sha}`
        : `the delivered entry is ${e.bytes} bytes / sha ${e.sha} but this sweep was emitted against ${e.expected.bytes} / ${e.expected.sha} — every measurement below is of a stale build`,
    );
  }
  if (report.harnessStale) {
    findings.push(
      `the harness is build ${report.harnessBuild} but this sweep was emitted against ${report.expectedHarnessBuild} — ` +
        `every measurement below is of a stale fixture`,
    );
  }
  const paintTotal = (report.focus || []).reduce((a, f) => a + (f.paintConfirmed || 0), 0);
  const pressedTotal = (report.focus || []).reduce((a, f) => a + (f.pressed || 0), 0);
  if (paintTotal) {
    console.log(
      `note: the pixels overruled the computed-style focus verdict ${paintTotal} time(s) of ${pressedTotal} press(es) — ` +
        `the rings are painted, the style check cannot see them`,
    );
  }
  for (const f of report.focus || []) {
    // A CHECK THAT COULD NOT LOOK IS NOT A PASS. The pixel confirmation can fail (an offscreen element, a
    // clip the browser refuses), and the first version turned that into "no focus indication" — a false
    // finding. It is now its own verdict, and it FAILS the run, because an unperformed measurement proves
    // nothing either way.
    if (f.unconfirmed) {
      findings.push(
        `${f.page ? f.page + ': ' : ''}${f.unconfirmed} focus candidate(s) could not be confirmed against the pixels` +
          `${(f.unconfirmedOn || []).length ? ' — ' + f.unconfirmedOn.join(', ') : ''} — the check could not look, so it cannot say`,
      );
    }
    if (f.missing) findings.push(`${f.page ? f.page + ': ' : ''}${f.missing} Tab stop(s) with no visible focus ring`);
    // A RUN THAT LANDED NOWHERE IS NOT A PASSING RUN. Focus escaping to the body used to count as "ok",
    // so a page with nothing focusable reported a clean sheet — the same "a skip reads as a pass" defect
    // the colour sweep bans. A row that says it pressed keys and landed on nothing is a finding.
    if (f.pressed > 0 && f.landed === 0) {
      findings.push(
        `${f.density || ''}${f.theme ? '/' + f.theme : ''}: focus landed on nothing in ${f.pressed} Tab press(es) ` +
          `(${f.escaped} escaped to the body) — that is a report of no focusable targets, not of good focus rings`,
      );
    }
  }
  // A PRESS THAT RENDERS NOTHING IS A CONTROL THAT DOES NOT ANSWER (round 55). The same measurement that found the
  // panel's dead active tab by hand, in the sweep: `pressPass` presses each target with the pointer and compares the
  // computed style before and during. `feedback-check.mjs` proves the RULE exists; only this can see whether it
  // reaches the screen.
  for (const row of report.press || []) {
    const where = `${row.density || "?"}/${row.theme || "?"}`;
    const dead = (row.rows || []).filter((r) => r.changed === false);
    for (const d of dead) findings.push(`${where}: ${d.sel} (${d.where}) renders NOTHING when pressed — before and during are identical (${d.size})`);
    // A PASS THAT PRESSED NOTHING IS NOT A CLEAN PASS. Targets are per surface, and a page that renders none of them
    // would otherwise report zero dead presses forever.
    if ((row.measured || 0) < 2) {
      findings.push(`${where}: the press pass measured ${row.measured || 0} control(s) — a press pass that pressed nothing proves nothing`);
    }
  }
  // IDLE REPAINT (round 64). The objective lists it among the things a claim is verified by, and the measurement is
  // DOM mutations on a settled page under STATIC fixtures: React writes to the DOM only when the output differs, so
  // nothing changing means nothing written. The observer proves it is alive by seeing one deliberate mutation of the
  // panel's own root, and that is required — a blind observer reports a perfectly still panel forever.
  for (const row of report.idle || []) {
    const where = `${row.density || "?"}/${row.theme || "?"}/${row.page || "?"}`;
    const probe = (row.byTarget || {}).__probe || 0;
    const real = (row.mutations || 0) - probe;
    if (probe < 1) {
      findings.push(`${where}: the idle observer did not see its own probe mutation — a blind instrument reports a still panel forever, so this measurement proves nothing`);
    }
    if (real > 0) {
      const all = Object.entries(row.byTarget || {}).filter(([k]) => k !== "__probe");
      // A CLOCK IS NOT A REPAINT (round 69). The first CI run of the pass reported "6 mutations in 6s (#text x6)" —
      // one per second — and naming the PARENT turned it into `span.approval-left x6`: the approval countdown
      // counting down. That is a value that is SUPPOSED to change, and calling it a repaint of unchanged output
      // would be a false finding, which is worse than none. The exemptions are an explicit table with a reason
      // each, the same shape `state-colour-check` uses for the rules it deliberately does not judge: a table cannot
      // quietly grow the way a regex can, and the elements exempted are exactly the ones that render a live
      // duration.
      const CLOCKS = [
        { match: /^span\.approval-left$/, why: "the approval countdown — seconds until the gate closes" },
        { match: /^span\.cmd-duration$/, why: "a running command's elapsed time (the panel's one clock)" },
        { match: /^span\.traj-/, why: "the same elapsed time inside the trajectory view" },
        { match: /^\.details-duration$/, why: "the same elapsed time in the details column" },
      ];
      const clock = (name) => CLOCKS.find((c) => c.match.test(name));
      const repaints = all.filter(([name]) => !clock(name));
      const clocks = all.filter(([name]) => clock(name));
      if (clocks.length) {
        notes.push(`${where}: ${clocks.map(([k, v]) => `${k} x${v}`).join(", ")} changed while idle — a live duration, exempt by name with its reason in CLOCKS`);
      }
      if (repaints.length) {
        const targets = repaints.map(([k, v]) => `${k} x${v}`).join(", ");
        findings.push(`${where}: ${repaints.reduce((n, [, v]) => n + v, 0)} DOM mutation(s) in ${row.seconds}s while idle — nothing changed under static fixtures, so this is a repaint of unchanged output (${targets})`);
      }
    }
  }

  // REDUCED MOTION IS A CONTRACT, NOT A COURTESY. `motion` entries come from a render with the
  // preference EMULATED: anything still carrying a transition or an infinite animation under it is a
  // finding. Measured round 77 — the panel density honoured the preference and the desktop density
  // did not (19 elements with motion, 19 after), and the fix took four rounds of cause-finding:
  // cascade order, then selector scope, then specificity, then an id, because xterm.js injects its
  // stylesheet at runtime and no equal-specificity rule of ours can win.
  for (const m of report.motion || []) {
    // BOTH SHAPES: `animating` is the older single-number row, `stillAnimating` the newer one.
    const still = m.stillAnimating || m.animating || [];
    if (still.length) {
      findings.push(`reduced motion (${m.density || "?"}): ${still.length} element(s) still animate — ${still.slice(0, 3).join("; ")}`);
    }
    // A CHECK THAT FOUND NOTHING TO SUPPRESS PROVES NOTHING. Round 134 added the second measurement: if
    // nothing animates WITHOUT the preference either, then "nothing animates under reduce" says nothing
    // about the rule — the page has no motion to honour, or the probe matched nothing at all.
    if (typeof m.normal === "number" && m.normal === 0) {
      findings.push(
        `reduced motion (${m.density || "?"}): 0 elements animate WITHOUT the preference, so this result ` +
          `proves nothing about prefers-reduced-motion — the check found nothing to suppress`,
      );
    }
  }
  // HOVER IS A STATE, and until round 84 neither instrument looked at it: the static pair sweep reads
  // base rules, and the rendered passes measure the RESTING DOM. The panel carries 73 :hover rules —
  // exactly where a designer reaches for a lighter accent. Measured: 31 interactive elements, 24
  // hoverable in the harness, 0 under AA in either theme.
  // A CLASS THE PAGE RENDERS THAT NO RULE STYLES — the mirror of dead CSS, and the failure a prune
  // causes. Round 88 found three by hand; this is the same question, asked by the browser (CSSOM)
  // on every sweep. `opts.implicitStates` names classes that are unstyled ON PURPOSE because a base
  // rule already produces their appearance, each with the reason printed rather than hidden.
  for (const u of report.unstyled || []) {
    const all = u.classes || [];
    const waived = all.filter((c) => (opts.implicitStates || {})[c]);
    for (const c of waived) console.log(`note: ${c} is unstyled by design — ${opts.implicitStates[c]}`);
    const live = all.filter((c) => !(opts.implicitStates || {})[c]);
    if (live.length) {
      // THE WORDING MATTERS, and it took a round to get it right: a class with no matching rule is NOT
      // an unstyled element. A base class styles it (`.view` on `view terminal`), or an ATTRIBUTE does
      // (`tab-dot serial` is painted by a [data-kind] rule), or it is a deliberate test marker. What is
      // true — and what is worth failing on — is narrower: this name is on screen and nothing matches
      // it, so either it is an inert extra to prune or it needs a reason to stay.
      findings.push(`class name(s) with no matching rule on ${u.page || "?"}: ${live.join(", ")} — on screen, matched by nothing`);
    }
    // A READ THAT FOUND NO STYLESHEETS PROVES NOTHING (round 88's collector reported 38 styled
    // classes where the browser sees 221, and its empty findings looked like a clean page).
    // THE THRESHOLD IS THE MEASURED FAILURE, not a guess: the console's pages have 221 styled classes
    // and round 88's broken collector reported 38 — so a floor of 20 would not have caught it. 100 is
    // below every real page in either UI and above every broken read seen so far.
    // THE FLOOR IS PER-UI, because a page can be legitimately small. The extension's options page is
    // three controls styled by element and id selectors, and its sheet defines FOUR classes — a floor of
    // 100 would report "the collector read almost nothing" forever, which is the false alarm this
    // parameter removes. The panel and console keep the strict default.
    // A SHEET THE COLLECTOR COULD NOT READ IS A HOLE IN ITS BASIS, and every class that sheet styles looks
    // unstyled. The floor above catches a collector that read almost nothing; this catches one that read
    // almost everything — the failure the floor cannot see.
    if (u.sheetsUnreadable > 0) {
      findings.push(
        `unstyled check on ${u.page || "?"}: ${u.sheetsUnreadable} stylesheet(s) could not be read, so their classes look unstyled — the basis is incomplete`,
      );
    }
    if (typeof u.styledClasses === "number" && u.styledClasses < (opts.unstyledFloor ?? 100)) {
      findings.push(`unstyled check on ${u.page || "?"}: only ${u.styledClasses} styled classes found (floor ${opts.unstyledFloor ?? 100}) — the collector read almost nothing, so its silence means nothing`);
    }
  }
  // THE TYPE FLOOR, IN THE RENDERED PAGE. designScale.test.ts pins the SCALE — names, order, and a 10px floor
  // — but a token being 10px and the rendered text being 10px are different claims, and only the second one
  // is what a reader experiences. Round 164 looked at the rows every sweep already collects: the panel's
  // smallest rendered size is 10 and the console's is 10.8, against 2035 and 590 text rows. Nothing was
  // wrong; this is what keeps it that way, because a one-off look is not a guard.
  for (const r of report.rows || []) {
    if (r.kind === 'graphic') continue;
    if (typeof r.size === 'number' && r.size > 0 && r.size < 10) {
      findings.push(`type floor: ${r.sel} renders at ${r.size}px on ${r.page || '?'} — the scale's floor is 10px ("${String(r.text || '').slice(0, 24)}")`);
    }
  }
  // A SCAN THAT COULD NOT READ MOST OF THE PAGE IS NOT A CLEAN SCAN. Rows the probe cannot measure are
  // excluded from judgement — correctly, since guessing at them is how a probe starts lying — but until
  // round 165 they appeared ONLY as a number in the summary line, so a report that had stopped measuring
  // anything would still exit 0 with "nothing above found a defect". That is the same vacuity this suite has
  // found in its own checks five times over, and the same rule it already applies to the unstyled scan: a
  // floor, stated with the number that failed it.
  //
  // Both live reports are at 0.0% (the panel's 496 gradient-surfaced rows are measured against their stops,
  // not skipped), so the floor is not a nuisance today — it is what would catch the day it is not.
  {
    const all = (report.rows || []).length;
    const blind = (report.rows || []).filter((r) => r.cr === null || r.cr === undefined || Number.isNaN(r.cr)).length;
    const floor = opts.unmeasurableFloor ?? 0.1;
    if (all > 0 && blind / all > floor) {
      findings.push(
        `only ${all - blind} of ${all} rows could be measured (${((blind / all) * 100).toFixed(1)}% unmeasurable, floor ${(floor * 100).toFixed(0)}%) — the absences below prove nothing`,
      );
    }
  }
  for (const h of report.hover || []) {
    if (h.underAA && h.underAA.length) {
      findings.push(`hover (${h.density || "?"}/${h.theme || "?"}): ${h.underAA.length} element(s) below AA while hovered — ${h.underAA.slice(0, 3).join("; ")}`);
    }
  }
  // THE HARNESS DELIVERED THE PUSH, OR THE MEASUREMENT IS OF ANOTHER PANEL. Every panel surface this suite has
  // ever measured was taken with the SSE fixture either serving a frame (connected) or refusing every call (the
  // failure surfaces, which are SUPPOSED to read as reconnecting). Nothing asserted which — so a fixture change that
  // shut the stream would silently turn every surface into a reconnecting panel and every finding into a statement
  // about a screen nobody sees. The harness publishes {opened, fail}; this is the clause that reads it.
  // AND THE RENDERED TEXT IS THE SECOND WITNESS, independent of the flag above. "Sessions unavailable" is the
  // panel's own sentence for a push that never arrived; on a surface whose harness did NOT report the failure
  // fixture, seeing it means the measurement describes a screen the operator never sees. Two signals, one fact —
  // the flag says what the fixture did, this says what the panel concluded from it, and a fixture that lies about
  // itself would have to lie in both places to get past.
  const harnessFailed = new Map((report.sse || []).map((r) => [r.page, r.fail === true]));
  for (const row of report.rows || []) {
    if (!/Sessions unavailable/i.test(String(row.text || ""))) continue;
    if (harnessFailed.get(row.page) === true) continue;      // the failure fixture is meant to say exactly this
    findings.push(`${row.page || "?"}: the panel renders "Sessions unavailable" and the harness did not report the failure fixture — this surface was measured with the push missing`);
  }
  for (const row of report.sse || []) {
    if (row.fail === true) continue;                       // a failure surface is meant to be disconnected
    if (row.opened !== true) {
      findings.push(`${row.page || "?"}: the harness never opened the SSE stream, so this surface was measured in the RECONNECTING state — the fixture failed, not the panel`);
    }
  }
  for (const r of report.reflow || []) {
    if (r.docScrollsSideways) {
      findings.push({ text: `reflow @${r.width}px: the document scrolls sideways (${r.docScrollWidth} > ${r.viewport})`, entry: r });
    }
    // A toolbar-style scroller is WCAG 1.4.10's own exception; reported, not failed.
    if (r.sideScrollers.length) console.log(`note: scrollers at ${r.width}px (allowed for toolbars) — ${r.sideScrollers.join("; ")}`);
  }
  const kept = [];
  for (const f of findings) {
    const text = typeof f === "string" ? f : f.text;
    const entry = typeof f === "string" ? null : f.entry;
    // An exemption may look at the REPORT ENTRY as well as the finding's text. The panel's tab-strip
    // artifact is only an artifact when the offending scrollers are tab children — a rule matching
    // the text alone would also hide a genuine 320px reflow defect, and the sweep's own gate caught
    // exactly that when the first version of this exemption was written.
    const rule = (opts.ignore || []).find((i) => (i.test ? i.test(text, entry) : i.match.test(text)));
    if (rule) suppressed.push({ finding: text, reason: rule.reason });
    else kept.push(text);
  }
  for (const s of suppressed) console.log(`note: set aside (${s.reason}) — ${s.finding}`);
  return kept;
}

/** How a sweep reports its result, so three tools read the same way. */
export function reportSummary(label, report) {
  const surfaces = report.surfaces.length;
  const blind = (report.rows || []).filter((r) => r.cr === null).length;
  return (
    `${label}: ${(report.rows || []).length} text nodes · ${surfaces} surface(s) · ` +
    `${(report.names || []).length} name checks` +
    (blind ? ` · ${blind} unmeasurable` : "")
  );
}
