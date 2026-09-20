#!/usr/bin/env node
// press-anchor-check.mjs — a press must be measured against the HOVER, not against rest.
//
// WHY THIS EXISTS (round 95). `pressPass` is the only instrument that can see whether a press reaches the screen:
// `feedback-check.mjs` proves an `:active` RULE exists, and the judge fails a row whose before and during snapshots
// are identical. But the pass took its "before" snapshot with the pointer PARKED AWAY from the control, so a sheet
// that answered `:hover` and had no press rule at all still produced a difference — the hover made it — and the row
// read `changed: true`.
//
// MEASURED ON THE DEVICE, on the landing's theme toggle, which has a `:hover` rule and no `:active` rule:
//
//     light .theme-toggle 32x32  hoverChanges=[background,color]  pressAddsBeyondHover=[]  PRESS-ADDS-NOTHING
//     dark  .theme-toggle 32x32  hoverChanges=[background,color]  pressAddsBeyondHover=[]  PRESS-ADDS-NOTHING
//
// ...while `.btn-primary` (transform) and `a` (opacity) both add something beyond their hover on the same page. So
// the toggle was the one input on that surface that answered a hover and ignored a press, and BOTH gates missed it:
// the rendered pass for the reason above, and the sheet check because the landing's stylesheet is inline in
// `index/src/page.js` and `feedback-check.mjs` only read the panel's built sheet and the console's source sheet.
//
// WHAT THIS PINS, in two halves — the same split `svgRootPaints` uses in the contrast probe, because the DOM loop
// around the rule cannot run without a browser and the rule itself can:
//
//   1. THE RULE, as a pure function: `pressDelta(hovered, pressed)`.
//   2. THE WIRING, in the EMITTED artifact: the hovered snapshot is read before `mouse.down()`, and the verdict
//      comes from the hovered baseline. A rule the device does not run is not a rule.
import { pressDelta } from "../../agent/scripts/lib/design-sweep.mjs";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";

let n = 0;
const t = (desc, fn) => { fn(); n += 1; };

// ── 1. THE RULE ──────────────────────────────────────────────────────────────────────────────────────────────
const RESTING = { transform: "none", opacity: "1", background: "rgb(255,255,255)", filter: "none" };

t("a control that only answers a HOVER adds nothing to a press", () => {
  // The landing's theme toggle, exactly as the device measured it: the hover moves the background, and the press
  // changes nothing. This is the shape that used to read as `changed: true`.
  const hovered = { ...RESTING, background: "rgb(240,240,241)" };
  const pressed = { ...hovered };
  assert.deepEqual(pressDelta(hovered, pressed), [], "a hover-only control must NOT report a press that renders");
  // ...and the OLD anchor on the same three snapshots, for the record: resting against pressed differs, which is
  // exactly what the false pass was made of.
  const oldAnchor = ["transform", "opacity", "background", "filter"].filter((k) => RESTING[k] !== pressed[k]);
  assert.deepEqual(oldAnchor, ["background"], "the resting anchor saw the hover and called it a press");
});

t("a control that answers the press itself reports the property it moved", () => {
  const hovered = { ...RESTING, background: "rgb(240,240,241)" };
  assert.deepEqual(pressDelta(hovered, { ...hovered, transform: "matrix(1, 0, 0, 1, 0, 1)" }), ["transform"]);
  assert.deepEqual(pressDelta(hovered, { ...hovered, opacity: "0.72" }), ["opacity"]);
  // BOTH, when a control does both — the row then says so instead of reporting the first one.
  assert.deepEqual(pressDelta(hovered, { ...hovered, transform: "matrix(1, 0, 0, 1, 0, 1)", opacity: "0.9" }), ["transform", "opacity"]);
});

t("a LAYOUT change is not a press, and a missing snapshot is not a pass", () => {
  // The objective's budget: a press is transform/opacity. A width that moves on press re-lays-out the page every
  // frame, and counting it would let exactly the press `feedback-check.mjs` bans pass here.
  const hovered = { ...RESTING };
  assert.deepEqual(pressDelta(hovered, { ...hovered, width: "12px", height: "14px" }), [], "layout properties are not a press response");
  // A snapshot the probe could not read is UNKNOWN, never "it moved".
  assert.deepEqual(pressDelta(null, { ...hovered, transform: "x" }), []);
  assert.deepEqual(pressDelta(hovered, null), []);
});

// ── 2. THE WIRING, IN THE ARTIFACT THE DEVICE RUNS ───────────────────────────────────────────────────────────
// THE EMIT IS THE ARTIFACT. A module that reads the hovered snapshot while the emitted script still reads the resting
// one is the class of mistake this repository has paid for repeatedly (the probe's escaping layers), so this reads
// the emitted text rather than the module.
const ROOT = fileURLToPath(new URL("../..", import.meta.url));
const SWEEPS = ["panel-design-sweep", "console-design-sweep", "landing-design-sweep"];
const out = mkdtempSync(join(tmpdir(), "press-anchor-"));
try {
  for (const name of SWEEPS) {
    const src = execFileSync("node", [join(ROOT, "agent", "scripts", name + ".mjs"), "--emit"], {
      cwd: ROOT, stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, VALE_LANDING_OUT: join(out, "landing") },
    }).toString();
    t(`${name}: the emitted artifact parses`, () => {
      assert.ok(src.length > 5000, `${name}: the emit produced ${src.length} bytes, so it proves nothing`);
      assert.doesNotThrow(() => new Function(src), `${name}: the emitted script must parse`);
    });
    t(`${name}: the press is measured against the HOVERED state`, () => {
      // THE ORDER IS THE MEASUREMENT: hover the control, let the hover transition SETTLE, read the baseline, then
      // press. Asserting only "hovered before down" does not catch the defect — the first version of this check did
      // exactly that and passed a mutated probe that read the baseline with the pointer still parked away, which IS
      // the bug. (Found by running the mutation, which is the only way this kind of assertion is ever found.)
      const move = src.indexOf("await page.mouse.move(box.x, box.y)");
      const hover = src.indexOf("const hovered = await styleOf(sel)");
      const down = src.indexOf("await page.mouse.down()");
      const call = src.indexOf("const props = pressDelta(hovered, pressed)");
      assert.ok(move > 0, `${name}: no hover move — the baseline cannot be the hover`);
      assert.ok(down > 0, `${name}: no mouse.down() — there is no press to measure`);
      assert.ok(move < hover, `${name}: the baseline is read BEFORE the hover (move@${move}, hovered@${hover}) — that is the resting anchor this check exists for`);
      assert.ok(hover < down, `${name}: the baseline is read AFTER the press (hovered@${hover}, down@${down})`);
      assert.ok(call > down, `${name}: the verdict is not pressDelta(hovered, pressed) — the baseline is not the hover`);
      assert.ok(src.includes("function pressDelta"), `${name}: pressDelta is not embedded, so the browser runs a different rule`);

      // AND A PRESS THE POINTER NEVER DELIVERED IS NOT A PRESS THE CONTROL IGNORED (round 103). The pass took the
      // element's rect as it found it: for `.device-logs-toggle` that was y=1582 in an 860px viewport, so the mouse
      // moved to a coordinate outside the page, nothing was hovered, nothing was pressed, and the row read
      // "press adds nothing" — a finding against a button that answers. An instrument that reports a defect which
      // is really its own blind spot is the one failure mode this file exists to prevent, so the three rules are
      // pinned here: scroll it into view first, refuse to measure one that will not fit, and say so when the point
      // resolves to something else.
      assert.ok(src.includes('el.scrollIntoView({ block: "center", behavior: "instant" })'), `${name}: the element is not scrolled into view — a control below the fold will be "pressed" at a coordinate outside the page and reported as still`);
      assert.ok(src.includes("box.offscreen"), `${name}: an element that cannot be brought into view is measured anyway, which is how a blind spot becomes a finding`);
      assert.ok(src.includes("document.elementFromPoint"), `${name}: the hit test is gone — a covered element and a still element would read the same`);
      assert.ok(src.includes("the pointer never reached this element"), `${name}: a press the pointer never delivered is reported as an answer (or as its absence) instead of as a note`);
    });
  }
} finally {
  rmSync(out, { recursive: true, force: true });
}

console.log(`press-anchor: all ${n} checks passed`);
