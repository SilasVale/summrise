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
  return PAGE_CHECKS_TEMPLATE.replaceAll("ROOT_SEL", JSON.stringify(rootSelector));
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
  return {
    h1Count: heads.filter((e) => e.tagName === 'H1').length,
    firstIsH1: heads.length > 0 && heads[0].tagName === 'H1',
    skipped, mains: document.querySelectorAll('main').length, navs: document.querySelectorAll('nav').length,
    over: [...new Set(over)].slice(0, 8), clipped: [...new Set(clipped)].slice(0, 8), slivers: [...new Set(slivers)].slice(0, 8),
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
  for (let i = 0; i < document.styleSheets.length; i++) {
    try { collect(document.styleSheets[i].cssRules); } catch (e) {}
  }
  const unstyled = new Map();
  for (const el of document.querySelectorAll('#root *')) {
    const cls = typeof el.className === 'string' ? el.className : '';
    for (const c of cls.split(/\\s+/).filter(Boolean)) if (!styled.has(c)) unstyled.set(c, el.tagName.toLowerCase());
  }
  return { styledClasses: styled.size, classes: [...unstyled.keys()].sort(), tags: Object.fromEntries(unstyled) };
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
export async function focusPass(page, presses, label = {}) {
  await page.evaluate(() => document.body.focus());
  let landed = 0;
  let escaped = 0;
  let missing = 0;
  // HOW MANY THE PIXELS RESCUED FROM A WRONG VERDICT. Reported rather than hidden: a number that keeps
  // climbing means the computed-style check is drifting further from what the browser paints.
  let paintConfirmed = 0;
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
      } catch (e) { painted = false; }
      if (painted) {
        landed++;
        paintConfirmed++;
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
export const THEME_SOURCE = `(() => {
  const body = getComputedStyle(document.body).backgroundColor;
  let stored = '';
  try { stored = localStorage.getItem('vale-theme') || ''; } catch (e) { stored = '(unavailable)'; }
  // The app's own attribute when it has one, so this does not depend on the storage key never changing.
  const attr = document.documentElement.getAttribute('data-theme') || '';
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
  const suppressed = [];
  for (const s of report.surfaces) {
    const where = s.width ? `${s.page}@${s.width}px` : s.page;
    if (s.h1Count !== 1 || !s.firstIsH1) findings.push(`${where}: h1 count ${s.h1Count}, first-is-h1 ${s.firstIsH1}`);
    if (s.skipped) findings.push(`${where}: ${s.skipped} skipped heading level(s)`);
    if (s.mains !== 1) findings.push(`${where}: ${s.mains} main landmark(s), expected exactly 1`);
    if (s.navs > 1) findings.push(`${where}: ${s.navs} nav landmarks — a page has one navigation`);
    else if (s.navs !== 1 && !(opts.navless || []).includes(s.page)) {
      findings.push(`${where}: ${s.navs} nav landmark(s), expected exactly 1`);
    }
    for (const [kind, list] of [["overflow", s.over], ["clipping", s.clipped], ["sliver", s.slivers]]) {
      if (list && list.length) findings.push(`${where}: ${kind} — ${list.join("; ")}`);
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
