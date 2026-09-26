#!/usr/bin/env node
// ── THE MUTATION THAT MUST FAIL THIS GATE (moved here from the ledger table, landing 4b) ──
// Read this when you change this file: the mutation is how you find out whether the gate can still
// fail at all. A gate that cannot be broken is worse than no gate.
//
// MUTATION: read the press baseline BEFORE the hover (put `const hovered = await styleOf(sel);` back above `page.mouse.move`), or add `width` to `pressDelta`'s key list
// RESULT:   exit 1 both ways: "the baseline is read BEFORE the hover (move@…, hovered@…) — that is the resting anchor this check exists for", and "layout properties are not a press response". It pins the rule as a pure function AND the wiring in all three EMITTED artifacts, because a probe measured against rest calls every hover a press — that is how the landing's theme toggle passed for as long as it existed. The first version of the wiring assertion checked only "hovered before down" and PASSED the mutation (round 95). **AND IT REACHES EVERY CONTROL A PAGE RENDERS (round 15)**: it scrolls an element into view only when it is not

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
      env: { ...process.env, SUMMRISE_LANDING_OUT: join(out, "landing") },
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

      // AND IT MAY NOT ACCUSE FROM A BLIND SPOT (round 15). The pass takes the element's rect as it finds it: for
      // `.device-logs-toggle` that was y=1582 in an 860px viewport, so the pointer moved to a coordinate outside the
      // page, nothing hovered, nothing pressed, and the row read "press adds nothing" — a finding against a button
      // that answers. Four rules, pinned here because each is a way this instrument lied:
      assert.ok(src.includes('el.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "instant" })'), `${name}: the element is not scrolled into view — a control below the fold will be "pressed" at a coordinate outside the page`);
      // The condition is wrapped across lines in the source, so the assertion names its parts rather than one
      // formatted string: a check that goes stale on a reflow is a check that gets deleted instead of fixed.
      assert.ok(
        src.includes("const movedPage =") && src.includes("before.bottom > innerHeight"),
        `${name}: every element is scrolled unconditionally — an instrument may move the page to REACH a control, it may not rearrange the page it is measuring`,
      );
      assert.ok(src.includes("Math.min(r.right, innerWidth)"), `${name}: the rect is not clamped to the viewport, so a half-visible control is refused or pressed off-page`);
      assert.ok(src.includes("document.elementFromPoint"), `${name}: the hit test is gone — a covered element and a still element would read the same`);
      assert.ok(src.includes("the pointer never reached this control"), `${name}: a press the pointer never delivered is reported as an answer (or as its absence) instead of as a note`);
      // (The judge half — `r.reached !== false` gating the dead-control verdict — lives in `judgeReport`, which is
      // NOT part of the emitted sweep, so it is planted at the judge: `panel-design-sweep.bash`.)
      // THE PASS PRESSES WHAT IT WAS ASKED TO PRESS: rows are annotated, never dropped, because a pass that presses
      // nothing proves nothing (CI's floor said so twice while this was being got wrong).
      // AND "ARRIVED" IS DECIDED AT THE POINT PRESSED (round 265). This pinned
      // `!(box.movedPage && hovered && hovered.hit === false)` — which counted the hit test ONLY when the element had
      // to be scrolled first. The connect tabs inside a closed `<details>` were already "in the viewport" by their
      // rect while the section behind them took every hit, so a control that presses perfectly was reported twelve
      // times as one that ignores a press. `box.reaches` is `elementFromPoint` AT THE CLAMPED CENTRE — the coordinate
      // the press uses — so it is the honest test, and `checkVisibility` keeps the passes from offering a control the
      // browser does not render in the first place.
      assert.ok(src.includes("const reached = box.reaches !== false"), `${name}: the row no longer carries whether the pointer arrived`);
      // SCOPED TO THE DISCOVERY, because the first version of this assertion matched the guard's TEXT ANYWHERE and
      // the same three lines also live in the two press probes — so deleting the discovery's copy still passed. A
      // check that a string exists is not a check that the RULE is where it has to be (round 265's own mutation
      // caught it: the guard was removed from `discoverPressTargets` and the gate stayed green).
      const discovery = src.indexOf("function discoverPressTargets");
      assert.ok(discovery >= 0, `${name}: the DOM discovery is not embedded`);
      assert.ok(
        src.slice(discovery, discovery + 4000).includes("checkVisibilityCSS: true"),
        `${name}: the DOM discovery offers controls the browser does not render — a closed <details> keeps layout boxes for its content, which is how the connect tabs were pressed through the section drawn over them`,
      );
      // AND IT ASKS THE DOM FOR THE CONTROLS, with the count that lets the judge size its floor to the page.
      assert.ok(src.includes("function discoverPressTargets"), `${name}: the DOM discovery is not embedded — a control on a page no list names is never pressed (that is how the log toggle was missed)`);
      assert.ok(src.includes("found,"), `${name}: the row does not carry what the page HAD, so the judge cannot tell a one-control page from a vacuous pass`);
      assert.ok(src.includes("label.skip"), `${name}: the discovery has no skip list, so its cap is spent on chrome another pass already presses (that is how the log toggle stayed unpressed)`);
      // AND A PASS THAT MOVES THE POINTER PUTS IT BACK. Leaving it where the last press ended meant the next
      // surface's probes ran with whatever sat under that position still hovered — a session row kept its actions
      // revealed and the target probe measured a state nobody had asked for.
      assert.ok(src.includes("A pass that moves the pointer owns putting it back"), `${name}: the pass leaves the pointer where it stopped, so the next surface measures whatever that position hovers`);
      // THE PANEL IS WHERE A HOVER-REVEALED TARGET EXISTS, so the reveal pass is pinned there and only there: the
      // console's stylesheets have no `:hover` rule that reveals a child (`display`/`visibility`/`opacity` of a
      // descendant — checked when this was written) and the landing's action buttons are always in flow. If either
      // grows one, this assertion is where the next reader should widen the pass rather than guess.
      // AND THE EMPTY FLEET RENDERS THE OVERVIEW, WHERE THE OFF TONE LIVES (round 25). The console declares
      // `stat-off` unstyled-by-design — a class with no matching rule, painted by the base card — and the unstyled
      // pass reported it UNSEEN ("1 of 1 declared unstyled-by-design class(es) were not seen in this run (0 unstyled
      // name(s) over 6 page(s))"). The tone goes off when there is nothing to report (no device online, no channels,
      // no keys); the empty-fleet fixture existed and visited only #/devices and #/keys, so the one state that needs
      // that declaration had never been rendered by anything. A state with no surface cannot be measured.
      if (name.startsWith("console")) {
        assert.ok(
          src.includes("'overview-empty', '#/'"),
          `${name}: the empty-fleet fixture does not render the Overview — the stat-off declaration then waives a state no surface shows`,
        );
      }
      if (!name.startsWith("console") && !name.startsWith("landing")) {
        assert.ok(src.includes("function revealPass"), `${name}: the revealed state is not measured deliberately — a hover-revealed target is then measured only when the pointer happens to rest on its row`);
        // AND THE ACKNOWLEDGEMENT'S LATENCY IS A PASS OF ITS OWN (round 19): the emitted script must carry it, or
        // the "fires on the event, not on the network" clause is asserted nowhere but in a comment.
        assert.ok(src.includes("function ackPass"), `${name}: the acknowledgement latency is not measured — the panel's promise that feedback fires on the EVENT is then only a claim in a doc comment`);
        assert.ok(src.includes("msToAck"), `${name}: the acknowledgement pass does not report how long it took`);
        // AND IT REACHES ITS CONTROL THE SAME WAY THE PRESS PASS DOES (round 19): the first run clicked two buttons
        // at y=1200-1430 in an 860px viewport, measured nothing, and reported a finding. The same three rules —
        // minimal scroll, clamp to the visible part, hit-test the point — are asserted here for the same reason.
        assert.ok(src.includes('el.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "instant" })'), `${name}: the acknowledgement pass does not scroll its control into view — a click outside the page reads as "no acknowledgement"`);
        assert.ok(src.includes("box.reaches === false"), `${name}: the acknowledgement pass presses controls the pointer cannot reach`);
        // AND ITS BASELINE IS THE HOVER, not rest: read with the pointer away, a control's own hover rule looks
        // like an acknowledgement and every control passes for free.
        // DECLARATION-AGNOSTIC, and that is not fussiness: this assertion matched the literal `const before` and went
        // red in CI the moment the baseline became `let` so the retry could re-read it. A gate that pins a keyword
        // instead of an ORDER reports the edit rather than the rule.
        assert.ok(
          src.indexOf("before = await read(sel)") > src.indexOf("await page.mouse.move(box.x, box.y)\n    await page.waitForTimeout(260)"),
          `${name}: the acknowledgement baseline is read before the hover — that is the resting anchor this rule exists for`,
        );
        assert.ok(src.includes("Math.min(r.right, innerWidth)"), `${name}: the acknowledgement pass does not clamp the rect to the viewport`);
      }
      // (The emitter's own guard — every borrowed helper must be DEFINED in the text it prints — is enforced where it
      // can bite: each `--emit` branch calls `assertEmbedded`, the pre-commit hook runs all of them, and
      // `panel-design-sweep.bash` asserts the three emitters still call it.)
    });
  }
} finally {
  rmSync(out, { recursive: true, force: true });
}

console.log(`press-anchor: all ${n} checks passed`);
