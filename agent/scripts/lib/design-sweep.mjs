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
      missing++;
      if (missingOn.length < 8) missingOn.push(verdict.where);
      if (!evidence) evidence = verdict;
    }
  }
  return {
    ...label,
    pressed: presses,
    landed,
    escaped,
    missing,
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
    if (typeof u.styledClasses === "number" && u.styledClasses < 100) {
      findings.push(`unstyled check on ${u.page || "?"}: only ${u.styledClasses} styled classes found — the collector read almost nothing, so its silence means nothing`);
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
