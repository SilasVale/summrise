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

/** The judge: one implementation of "is this report a defect", whatever UI produced it. */
export function judgeReport(report, opts = {}) {
  const findings = [];
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
    findings.push(`${f.page}: ${f.missing} Tab stop(s) with no visible focus ring`);
  }
  for (const r of report.reflow || []) {
    if (r.docScrollsSideways) findings.push(`reflow @${r.width}px: the document scrolls sideways (${r.docScrollWidth} > ${r.viewport})`);
    // A toolbar-style scroller is WCAG 1.4.10's own exception; reported, not failed.
    if (r.sideScrollers.length) console.log(`note: scrollers at ${r.width}px (allowed for toolbars) — ${r.sideScrollers.join("; ")}`);
  }
  return findings;
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
