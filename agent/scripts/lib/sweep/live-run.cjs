// live-run.cjs — the LIVE panel probe's device-side program, as a real module (round 269).
//
// The last of the emitters that build a device script inside a template literal: 68 lines here, three interpolations,
// and — since round 167 — a parse guard copied from the panel after a backtick in a comment would have produced a
// broken script "in the middle of 16 KB". The program is ordinary code now, the three run-varying values (the probe,
// the marks source, the config paths to look for a token in) arrive as the generated pieces module beside it, and the
// assembler resolves the payload's own requires and compiles what it returns.
const P = require("./pieces.cjs");

const fs = require("fs");
const PROBE = P.probe;
const MARKS = P.marks;
const SELECTOR = P.selector;
const CONFIGS = P.configPaths;
(async () => {
  let token = null, where = null;
  for (const c of CONFIGS) {
    if (!fs.existsSync(c)) continue;
    const m = /token\s*:\s*["']?([A-Za-z0-9_\-]{8,})/.exec(fs.readFileSync(c, 'utf8'));
    if (m) { token = m[1]; where = c; break; }
  }
  if (!token) {
    console.log(JSON.stringify({ error: 'no token found in any known config', tried: CONFIGS }));
    process.exit(1);
  }
  const { acquireBrowser } = require(process.env.SUMMRISE_BROWSER_HELPER);
  const { page, close } = await acquireBrowser();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e.message).slice(0, 120)));
  const out = { configAt: where, densities: {} };
  for (const [density, path_, vp] of [['panel', '/panel/', { width: 1280, height: 860 }], ['desktop', '/desktop/', { width: 1440, height: 900 }]]) {
    await page.setViewportSize(vp);
    await page.goto('http://127.0.0.1:18080' + path_ + '?token=' + token, { waitUntil: 'load' });
    await page.waitForTimeout(3500);
    const state = await page.evaluate(() => ({
      connForm: !!document.getElementById('conn-form'),
      rootNodes: document.querySelectorAll('#root *').length,
      tabs: document.querySelectorAll('.tab, .dtab').length,
      railDots: [...document.querySelectorAll('.rail-dot, .dtab-dot')].map((d) => d.getAttribute('data-state')).slice(0, 8),
    }));
    const marks = await page.evaluate(MARKS, SELECTOR);
    const rows = await page.evaluate(PROBE);
    const text = rows.filter((r) => r.kind !== 'graphic');
    const graphics = rows.filter((r) => r.kind === 'graphic');
    out.densities[density] = {
      state,
      rows: rows.length,
      text: text.length,
      graphics: graphics.length,
      textFailing: text.filter((r) => !r.inactive && r.cr < r.need).map((r) => r.sel + ' ' + r.cr + '<' + r.need),
      graphicFailing: graphics.filter((r) => !r.inactive && r.cr < r.need).map((r) => r.sel + ' ' + r.cr + '<' + r.need),
      unmeasurable: rows.filter((r) => r.cr === null || r.cr === undefined).length,
      // WHAT THE LIVE PANEL PAINTS, family by family, and any two states of one family that paint IDENTICALLY. The
      // harness reports the same shape; the value here is that these are the states the DEVICE renders, which is not
      // the same set — the sweeps' mark-coverage note counts six families the harness never paints.
      marks,
    };
  }
  out.errors = errors.slice(0, 4);
  // A READING BECOMES A GUARD (round 30 of the standing goal). Everything this probe collects is a list that must be
  // EMPTY on a healthy panel: failing text, failing graphics, and the two mark-axis verdicts the sweeps also make —
  // two states of one family painting identically, and a mark that is both a fill and a ring. Printing them and
  // exiting 0 would make the probe a report nobody has to answer; the exit code is what the runbook step can act on.
  // Note what a non-zero exit MEANS here: the live panel's state varies with what the device is doing (two sessions
  // today, fourteen another day), so it says "look at this", not "the build is broken".
  const bad = [];
  for (const [density, d] of Object.entries(out.densities)) {
    for (const key of ['textFailing', 'graphicFailing']) {
      for (const f of d[key]) bad.push(density + ' ' + key + ': ' + f);
    }
    if (d.marks && d.marks.collisions) for (const c of d.marks.collisions) bad.push(density + ' mark collision: ' + c);
    if (d.marks && d.marks.ringFill) for (const c of d.marks.ringFill) bad.push(density + ' ring+fill: ' + c);
  }
  out.verdict = bad.length ? { ok: false, problems: bad } : { ok: true };
  console.log(JSON.stringify(out, null, 1));
  await close();
  process.exit(bad.length ? 1 : 0);
})().catch((e) => { console.error('FATAL', e.message); process.exit(1); });
