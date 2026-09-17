// contrast-probe.mjs — the ONE copy of the contrast math, for the browser AND for tests.
//
// WHY THIS EXISTS. Four rounds of contrast work were done with an ad-hoc snippet
// retyped into a browser console each time, and that snippet had TWO defects that
// only surfaced by luck:
//
//   * it skipped any element with a background-image ANYWHERE among its ancestors,
//     so one gradient wrapper in the panel skipped EVERY text node and the sweep
//     reported `checked=0, underAA=0` — a check that read nothing and looked like a
//     pass (round 33's lesson, committed again by the tool built to apply it);
//   * it read `rgba(255,255,255,0.07)` as WHITE, so twenty findings were chips
//     measured against a surface that is not there.
//
// `agent/scripts/panel-render-audit.mjs` already had the correct implementation —
// compositing background alpha, the ancestor OPACITY chain, and the foreground's
// own alpha. The defect was that it was a STRING inside one audit script, so
// nothing could test it and every new sweep re-derived it. This module is that
// implementation, in one place, with the math as REAL FUNCTIONS.
//
// THE TESTED CODE IS THE CODE THAT RUNS. `PROBE_SOURCE` embeds these functions
// with `Function.prototype.toString()`, so the unit test in
// `scripts/test/contrast-probe-check.mjs` exercises the exact text the browser
// evaluates — not a copy that can drift from it. A test on one copy can only ever
// compare copies.

/** Composite a stack of `{r,g,b,a}` layers, innermost LAST, over `base`.
 *  Order matters and is the thing both defects got wrong: the list runs from the
 *  element upward, so it is applied from the end backwards. */
export function compositeStack(stack, base = { r: 255, g: 255, b: 255 }) {
  let out = { ...base };
  for (let i = stack.length - 1; i >= 0; i--) {
    const c = stack[i];
    out = {
      r: c.r * c.a + out.r * (1 - c.a),
      g: c.g * c.a + out.g * (1 - c.a),
      b: c.b * c.a + out.b * (1 - c.a),
    };
  }
  return out;
}

/** WCAG relative-luminance contrast ratio, rounded to 2dp. */
export function contrastRatio(fg, bg) {
  const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); };
  const lum = (c) => 0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b);
  const [hi, lo] = [lum(fg), lum(bg)].sort((p, q) => q - p);
  return +((hi + 0.05) / (lo + 0.05)).toFixed(2);
}

/** The WCAG AA bar for TEXT at this size/weight: 3.0 counts as "large" at >=24px,
 *  or >=18.66px when bold; everything else needs 4.5. Getting this wrong is the
 *  difference between a sweep that reports real failures and one that cries wolf. */
export function aaThreshold(fontSize, fontWeight) {
  const large = fontSize >= 24 || (fontSize >= 18.66 && Number(fontWeight) >= 700);
  return large ? 3 : 4.5;
}

/** `#rgb`/`#rrggbb`/`rgb()`/`rgba()` -> {r,g,b,a}; null when unparseable. */
export function parseColour(c) {
  const s = String(c || "").trim();
  // HEX FIRST, AND EXPLICITLY. The numeric scrape below reads '#f4f4f5' as the numbers 4, 4 and 5 —
  // a real colour, silently wrong — and '#71717a' as nothing at all, because the 'a' ends the run.
  // It went unnoticed because the RENDERED sweep only ever sees 'rgb()'/'rgba()': getComputedStyle
  // never returns hex, so the bug lived in the one path nothing called until a STATIC check read the
  // token sheet (round 43). Its own suite had no hex case, which is why 13 green checks proved
  // nothing about it.
  const hex = /^#([0-9a-f]{3,8})$/i.exec(s);
  if (hex) {
    const digits = hex[1];
    const expand = (d) => parseInt(d.length === 1 ? d + d : d, 16);
    if (digits.length === 3 || digits.length === 4) {
      return {
        r: expand(digits[0]),
        g: expand(digits[1]),
        b: expand(digits[2]),
        a: digits.length === 4 ? expand(digits[3]) / 255 : 1,
      };
    }
    if (digits.length === 6 || digits.length === 8) {
      return {
        r: parseInt(digits.slice(0, 2), 16),
        g: parseInt(digits.slice(2, 4), 16),
        b: parseInt(digits.slice(4, 6), 16),
        a: digits.length === 8 ? parseInt(digits.slice(6, 8), 16) / 255 : 1,
      };
    }
    return null; // 5 or 7 digits is not a colour
  }
  // rgb() / rgba() (and anything else numeric) keeps the original scrape.
  const m = s.match(/[\d.]+/g);
  if (!m || m.length < 3) return null;
  const [r, g, b] = m.map(Number);
  return { r, g, b, a: m.length > 3 ? Number(m[3]) : 1 };
}

/**
 * The in-page probe. Evaluated with `page.evaluate(PROBE_SOURCE)`; returns one
 * row per visible text-bearing element.
 *
 * `skipSelector` defaults to `.xterm` — the terminal paints from its own palette,
 * not the app's tokens, so measuring it mixes two colour systems.
 * `assertRendered` makes a page that produced ZERO rows FAIL rather than pass: a
 * sweep that read nothing is not a sweep that found nothing.
 */
/** The colour stops of a computed `background-image`, as colours — or [] when it cannot be read.
 *
 *  A GRADIENT IS MEASURABLE, CONSERVATIVELY. The probe used to return `cr: null` for every text
 *  node over one (honest, and it left 58% of the desktop density's text unjudged: 140 of 242 rows).
 *  Reading its stops instead gives a BOUND: if the WORST stop still clears AA, the text clears it on
 *  every stop. That is the safe direction — a gradient cannot hide a failure behind an average.
 *
 *  `color(srgb …)`, conic gradients and image URLs return [] and stay unmeasurable: a parser for
 *  every colour function is how a probe starts lying again. */
export function gradientStops(image) {
  const text = String(image || "");
  if (!text || text === "none") return [];
  const out = [];
  for (const m of text.matchAll(/(rgba?\([^)]*\)|transparent)/g)) {
    if (m[1] === "transparent") {
      out.push({ r: 0, g: 0, b: 0, a: 0 });
      continue;
    }
    const c = parseColour(m[1]);
    if (c) out.push(c);
  }
  return out;
}

/** The LOWEST contrast `fg` has against any stop of a gradient, each composited over `base`.
 *  Returns null when there is nothing to measure. */
export function worstOverGradient(fg, stops, base) {
  let worst = null;
  for (const stop of stops) {
    const bg = compositeStack([{ ...stop, a: stop.a ?? 1 }], base);
    const ratio = contrastRatio(fg, bg);
    if (worst === null || ratio < worst) worst = ratio;
  }
  return worst;
}

export const PROBE_SOURCE = `(() => {
  const compositeStack = ${compositeStack.toString()};
  const contrastRatio = ${contrastRatio.toString()};
  const aaThreshold = ${aaThreshold.toString()};
  const parseColour = ${parseColour.toString()};
  const gradientStops = ${gradientStops.toString()};
  const worstOverGradient = ${worstOverGradient.toString()};
  const SKIP = ${JSON.stringify(".xterm")};

  const effBg = (el) => { const st = []; let p = el; let gradient = false; let stops = [];
    while (p) { const cs = getComputedStyle(p);
      if (cs.backgroundImage && cs.backgroundImage !== 'none') {
        stops = gradientStops(cs.backgroundImage);
        gradient = true;
        // KEEP WALKING: the gradient sits ON something, and that something is the base every stop
        // composites over. Stopping here is what forced the old probe to answer null.
        p = p.parentElement;
        continue;
      }
      const c = parseColour(cs.backgroundColor);
      if (c && c.a > 0) { st.push(c); if (c.a === 1) break; } p = p.parentElement; }
    return { colour: compositeStack(st), gradient, stops }; };

  const chainOpacity = (el) => { let o = 1, p = el;
    while (p && p !== document.documentElement) { o *= parseFloat(getComputedStyle(p).opacity || '1'); p = p.parentElement; }
    return o; };

  const effFg = (el, bg) => { const c = parseColour(getComputedStyle(el).color); if (!c) return null;
    const a = (c.a ?? 1) * chainOpacity(el);
    return { r: c.r * a + bg.r * (1 - a), g: c.g * a + bg.g * (1 - a), b: c.b * a + bg.b * (1 - a) }; };

  const rows = []; const seen = new Set();
  for (const el of document.querySelectorAll('body *')) {
    if (SKIP && el.closest(SKIP)) continue;
    if (el.tagName === 'SCRIPT' || el.tagName === 'STYLE') continue;
    if (![...el.childNodes].some(n => n.nodeType === 3 && n.textContent.trim())) continue;
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) continue;
    const st = getComputedStyle(el);
    if (st.visibility === 'hidden' || st.display === 'none' || parseFloat(st.opacity) === 0) continue;
    const cls = typeof el.className === 'string' ? el.className : '';
    const key = cls + '|' + (el.textContent || '').slice(0, 16);
    if (seen.has(key)) continue; seen.add(key);
    // WCAG 1.4.3 EXEMPTS INACTIVE CONTROLS. A disabled button at opacity 0.45
    // composites to a real 2.1:1 reading — a truthful measurement of a control
    // nobody can use yet, and not a violation. Flagged rather than filtered so the
    // number stays visible; a sweep should not silently drop inconvenient rows.
    const inactive = !!(el.disabled || el.closest('[disabled]') || el.closest('[aria-disabled="true"]'));
    const bg = effBg(el);
    const fg = effFg(el, bg.colour); if (!fg) continue;
    const size = parseFloat(st.fontSize);
    // A GRADIENT IS MEASURED AS A BOUND, not skipped: the worst of its stops, each composited
    // over the background beyond it. 'approx' says how the number was obtained (and a gradient
    // whose stops could not be parsed keeps 'cr: null', counted by unmeasurable() so a caller can
    // print the number instead of letting a skip read as a pass).
    const gradientCr = bg.gradient && bg.stops.length
      ? worstOverGradient(fg, bg.stops, bg.colour)
      : null;
    rows.push({
      sel: el.tagName.toLowerCase() + (cls ? '.' + cls.trim().split(/\\s+/).join('.') : ''),
      text: (el.textContent || '').trim().slice(0, 24),
      size, weight: st.fontWeight,
      need: aaThreshold(size, st.fontWeight),
      inactive,
      gradient: bg.gradient,
      approx: gradientCr !== null,
      cr: bg.gradient ? gradientCr : contrastRatio(fg, bg.colour),
    });
  }
  // ── GRAPHICS: BORDERS, MARKS AND DOTS (round 125) ────────────────────────────────────────────────
  // WCAG 1.4.11 asks 3:1 of a non-text element that carries meaning. The text loop above cannot see one,
  // because it requires the element to own a text node — which is why the two graphic defects this suite
  // has found were both found by hand.
  //
  // THE ROW CARRIES ITS OWN EVIDENCE: paint and surface are the two colours the ratio came from, so a
  // number that looks wrong explains itself instead of needing a separate investigation. Rounds 123 and
  // 124 each cost a whole round to a row that said cr: 7.03 with nothing to say WHY.
  //
  // The painter is resolved in this order: a drawn border; the element's own background; a zero-offset
  // BOX-SHADOW with a spread (the dot idiom — the shadow IS the mark); ::before/::after (the other dot
  // idiom); SVG fill, then stroke.
  const painterOf = (el, st) => {
    const widths = [st.borderTopWidth, st.borderRightWidth, st.borderBottomWidth, st.borderLeftWidth].map((w) => parseFloat(w) || 0);
    const bcols = [st.borderTopColor, st.borderRightColor, st.borderBottomColor, st.borderLeftColor].map(parseColour);
    // THE COLOUR MUST COME FROM A SIDE THAT HAS WIDTH. A zero-width border still REPORTS a colour — the
    // inherited text colour — and taking the first coloured side picked it: the boot triangle read
    // rgb(82,82,91) instead of its own #92400e, and the row's cr was wrong for two rounds. The row's own
    // paint field is what exposed it, in one look, which is why it is there.
    for (let i = 0; i < 4; i++) {
      if (widths[i] >= 1 && bcols[i] && (bcols[i].a ?? 1) > 0.05) return { colour: bcols[i], from: 'border' };
    }
    // A RING COMES BEFORE THE FILL. A zero-offset box-shadow WITH A SPREAD is drawn around the element,
    // so it is the OUTERMOST visible edge — and once it is drawn, the fill's ratio against the surface
    // stops being what a viewer sees. Round 128 added exactly such a ring to the active tab's lane dot
    // (the dot was 1.11 against the accent and invisible) and this pass went on reading the fill, so the
    // row stayed a false positive. Preferring the ring makes it report 4.99, the ratio that decides
    // whether the mark is visible at all. The spread test is what separates a RING from a plain drop
    // shadow: a shadow with no spread does not draw an edge around the element.
    const shadow = (st.boxShadow || '').split(',')[0] || '';
    if (shadow && shadow !== 'none') {
      // THE COLOUR IS NOT A WHITESPACE TOKEN. getComputedStyle serialises a box-shadow with the colour
      // FIRST — "rgba(255, 255, 255, 1) 0px 0px 0px 1px" — so splitting on spaces tears it into
      // "rgba(255," / "255," / "255," / "1)", none of which parses, and the branch could never fire. It
      // was inherited from the original shadow code and only mattered once a ring existed to look for
      // (round 128's active-tab dot): the row stayed a false positive at 1.11 while the probe insisted
      // it had checked. Pull the colour out with a pattern, and take the lengths from the px tokens.
      const colourText = /rgba?\([^)]*\)|#[0-9a-f]{3,8}/i.exec(shadow);
      const col = colourText ? parseColour(colourText[0]) : null;
      const px = shadow.trim().split(/\\s+/).filter((x) => /^-?[\\d.]+px$/.test(x)).map(parseFloat);
      if (col && (col.a ?? 1) > 0.05 && px.length >= 2 && px[0] === 0 && px[1] === 0 && (px[3] ?? 0) > 0) {
        return { colour: col, from: 'ring' };
      }
    }
    const own = parseColour(st.backgroundColor);
    if (own && (own.a ?? 1) > 0.05) return { colour: own, from: 'background' };
    for (const pseudo of ['::before', '::after']) {
      const ps = getComputedStyle(el, pseudo);
      if (!ps || ps.content === 'none' || ps.display === 'none') continue;
      const pb = parseColour(ps.backgroundColor);
      if (pb && (pb.a ?? 1) > 0.05) return { colour: pb, from: pseudo };
      const pw = ['borderTopWidth', 'borderLeftWidth'].map((k) => parseFloat(ps[k]) || 0);
      if (pw.some((w) => w >= 1)) {
        const pc = [ps.borderTopColor, ps.borderLeftColor].map(parseColour).filter((x) => x && (x.a ?? 1) > 0.05)[0];
        if (pc) return { colour: pc, from: pseudo };
      }
    }
    if (el instanceof SVGElement) {
      const f = parseColour(st.fill);
      if (f && (f.a ?? 1) > 0.05) return { colour: f, from: 'fill' };
      const sk = parseColour(st.stroke);
      if (sk && (sk.a ?? 1) > 0.05) return { colour: sk, from: 'stroke' };
    }
    return null;
  };

  const rgbStr = (c) => c ? 'rgb(' + Math.round(c.r) + ', ' + Math.round(c.g) + ', ' + Math.round(c.b) + ')' : null;

  for (const el of document.querySelectorAll('body *')) {
    if (SKIP && el.closest(SKIP)) continue;
    if (el.tagName === 'SCRIPT' || el.tagName === 'STYLE') continue;
    if ([...el.childNodes].some(n => n.nodeType === 3 && n.textContent.trim())) continue;
    // NOT EVERY SMALL PAINTED THING IS A MARK, and two false-positive classes were measured before this
    // line was written (round 126's first full audit: 47 "failures", mostly noise):
    //   * SVG. An icon's <path> inherits fill: black and its real colour comes from the <svg> above it,
    //     so every decorative glyph reported cr ~1. SVG is excluded, and that is a stated LIMIT: a
    //     sparkline's stroke is not measured by this instrument.
    //   * CONTROLS. A .btn-mini is 26px tall and passed the mark test, then compared its soft background
    //     with the card behind it. A control is judged by its TEXT, in the loop above.
    if (el instanceof SVGElement || el.closest('svg')) continue;
    if (/^(BUTTON|A|INPUT|SELECT|TEXTAREA|LABEL)$/.test(el.tagName)) continue;
    const r = el.getBoundingClientRect();
    if (r.width < 3 || r.height < 3) continue;
    if (Math.min(r.width, r.height) > 24) continue;
    const st = getComputedStyle(el);
    if (st.visibility === 'hidden' || st.display === 'none' || parseFloat(st.opacity) === 0) continue;
    const cls = typeof el.className === 'string' ? el.className : '';
    const key = 'g|' + cls + '|' + Math.round(r.width) + 'x' + Math.round(r.height);
    if (seen.has(key)) continue; seen.add(key);
    const inactive = !!(el.disabled || el.closest('[disabled]') || el.closest('[aria-disabled="true"]'));
    // THE SURFACE IS WHAT THE MARK SITS ON — the PARENT's resolved background, never the element's own.
    // Calling effBg(el) compared a dot's background with itself and reported cr: 1 for every dot in the
    // UI (76 rows on the first full audit). For a border-drawn mark the parent is still right: the
    // element's own background, if any, is transparent in that idiom.
    const bg = el.parentElement ? effBg(el.parentElement) : effBg(el);
    const surface = bg.colour;
    const painter = painterOf(el, st);
    if (!painter) continue;
    const paint = painter.colour;
    const a = paint.a ?? 1;
    const composited = { r: paint.r * a + surface.r * (1 - a), g: paint.g * a + surface.g * (1 - a), b: paint.b * a + surface.b * (1 - a) };
    rows.push({
      sel: el.tagName.toLowerCase() + (cls ? '.' + cls.trim().split(/\\s+/).join('.') : ''),
      text: '',
      kind: 'graphic',
      paint: rgbStr(composited) + ' (' + painter.from + ')',
      surface: rgbStr(surface),
      size: Math.min(r.width, r.height),
      weight: '400',
      need: 3.0,
      inactive,
      gradient: bg.gradient,
      approx: false,
      // WHERE THE MARK SITS IN THE DOM, as a short ancestor chain. Round 143 proved the two 1.16 tab-dot
      // rows were an artifact by reproducing the state three times and NOT finding them, and then wrote down
      // what was missing: "WHETHER the element was inside .active when it was captured. That field is the
      // next step." It took until round 201 to add it, because every attempt died in this file's escaping
      // layers — so this version uses NO REGEX and NO BACKSLASHES, only trim and split on a literal space.
      context: (function () { var c = []; var p = el.parentElement; for (var i = 0; i < 3 && p; i++) { if (typeof p.className === 'string' && p.className.trim()) c.push(p.className.trim().split(' ').filter(function (x) { return x; }).join('+')); p = p.parentElement; } return c.join(' < '); })(),
      cr: contrastRatio(composited, surface),
    });
  }

  return rows;
})()`;

/** Rows that fail their own AA bar. Takes the probe's rows; pure.
 *  Rows the probe could not measure (`gradient: true`, `cr: null`) are EXCLUDED —
 *  they are not passes and they are not failures, and `unmeasurable()` counts them
 *  so a caller can print the number instead of letting a skip read as a pass.
 *
 *  USE THESE, DO NOT RE-IMPLEMENT THE COMPARISON. Round 119 recorded an "open contrast defect" on a
 *  ghost button after filtering rows by hand with `r.cr < r.need` — which drops the `inactive` waiver and
 *  flagged a DISABLED control ("Notifications unavailable", opacity 0.45) that WCAG exempts and this
 *  file already excludes. The real sweep never reported it: `failures()` below does the filtering, the
 *  judge kept 41/41 green, and the false item existed only in my ad-hoc reader. An ad-hoc reader that
 *  forgets one field invents defects, and then they have to be hunted down. */
export function failures(rows) {
  return rows.filter((r) => r.cr !== null && !r.inactive && r.cr < (r.need ?? aaThreshold(r.size, r.weight)));
}

// ── WHAT THIS INSTRUMENT CANNOT SEE: GRAPHIC CONTRAST (rounds 123-124, recorded after two attempts) ──
//
// WCAG 1.4.11 asks 3:1 of a non-text element that carries meaning — a status dot, a chip's border, a
// warning triangle — and this probe is BLIND to all of them, because its loop requires the element to own
// a text node. That is why the two graphic defects this suite has found (the flapping chip's border in
// round 109, the boot triangle in round 121) were both discovered BY HAND.
//
// TWO ATTEMPTS, BOTH DELETED AFTER MEASURING THEM:
//   * round 123 looked for the painter in one place and reported 31 false failures reading cr: 1, for
//     dots painted by box-shadow, ::before/::after and SVG fill/stroke;
//   * round 126 ran it across all six pages, both densities and themes (74 marks) and it FOUND A REAL
//     DEFECT: .monitor-mark, .waiting-mark, .rail-dot[data-state=waiting] and .cmd-dot[data-state=warn]
//     painted themselves with --state-warn, the fill token, which measures 2.63 on the dark card and
//     2.92 in the panel density against the 3:1 a graphic needs. FIXED to --warn-ink; re-measured on the
//     rendered page at 6.45 / 9.13 / 8.23. The flapping variant and the boot triangle had been fixed
//     earlier (rounds 109, 121) because those two were found BY HAND; the rest were invisible until this
//     instrument existed. Failures across the audit: 18 -> 10.
//   * RINGS ARE READ NOW, and the reason they were not is worth keeping (round 129). getComputedStyle
//     serialises a box-shadow with the COLOUR FIRST — "rgb(255, 255, 255) 0px 0px 0px 1px" — so the old
//     code, which split the value on whitespace and parsed each token, tore the colour into
//     "rgba(255," / "255," / "255," / "1)" and never found one. Every box-shadow branch had been dead
//     since it was written; it only mattered once there was a ring to look for. The colour now comes out
//     with a pattern and the lengths from the px tokens, and a ring (zero offset, non-zero spread) is
//     preferred over the fill because it is the OUTERMOST visible edge.
//     VERIFIED on the page: the active tab's dot reports rgb(255, 255, 255) 0px 0px 0px 1px, an inactive
//     one reports none — which also confirms round 128's ring is really applied.
//   * THE AUDIT'S OWN ARTIFACT: its tab-dot.ssh row still reads 1.11, because the loop CLICKS EVERY TAB
//     in turn and the row is captured while a different one is active. The dot is only on the accent when
//     its own tab is active. Judge that state by activating that tab, not by reading the sweep's row.
//   * STILL FAILING, RECORDED RATHER THAN CHASED, with the evidence the rows carry:
//       span.ag-dot / span.side-dot / span.tab-dot   rgb(191,58,10) on dark   2.53-2.96
//         FIXED in round 127: --accent-ink, which the sheet's own note defines as "the accent for CHROME
//         — icons, dots, borders", and which measures 3.78 dark / 4.26 light. Re-measured on the page:
//         3.23-4.30 across the dots, all over 3:1. Failures across the audit: 10 -> 6.
//       span.tab-dot.ssh                             blue on the ACTIVE tab's orange   1.11
//         a state collision: the dot's colour and the surface it lands on are both state-bearing.
//       span.dtab-dot                                rgb(161,161,170), 2.54           <- --chrome-ink-faint,
//         which is DELIBERATELY faint by name. Decorative until someone says otherwise, which is a
//         judgement this instrument cannot make.
//       span.approval-grant (x4)                     a 1px border, 1.19-1.27          <- NEEDS A RULE,
//         not a fix: "any visible border is meaningful" is false, and a chip outline is decoration. The
//         pass needs a meaningfulness test before its count can be read as a defect list.
//   * round 124 resolved all three mechanisms, and its VALIDATION against a known element failed: on the
//     boot triangle the probe reads **7.03** where the tested maths, applied to the surface stack read
//     off the page (--warn-ink #92400e on the chip's opaque #f4f4f5), gives **6.45**. So its SURFACE
//     resolution is wrong for a mark sitting inside an opaque chip — it reads through to the chrome
//     behind it. That is the concrete thing to fix, and the comparison above is the test to fix it against.
//
// THE RULE THAT CAUGHT IT, worth more than either attempt: validate a new instrument against ONE element
// whose value is known — and derive that reference from colours READ OFF THE PAGE, not from an assumed
// surface. Round 121's hand figure was computed against a guessed background; it happened to be right for
// the light theme and I would not have known if it had not been.
//
// WHAT A CORRECT VERSION STILL NEEDS: painter resolution (all three mechanisms, as round 124 had it) AND
// a surface walk that stops at the first OPAQUE ancestor background rather than reading past it. Then
// re-run this exact comparison before reading any other row.
/** Rows inside an INACTIVE control. WCAG 1.4.3 exempts them, so `failures()`
 *  excludes them — but they are reported, not hidden. */
export function graphics(rows) {
  return rows.filter((r) => r.kind === 'graphic');
}

export function inactive(rows) {
  return rows.filter((r) => r.inactive && r.cr !== null);
}

/** Rows the probe declined to measure, with the reason. */
export function unmeasurable(rows) {
  return rows.filter((r) => r.cr === null);
}
