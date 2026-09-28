// THE CASCADE GATE — A RULE THAT CAN NEVER WIN, FOUND IN A RENDERED DOM.
//
// WHY THIS FILE EXISTS, AND WHY THE FOUR INSTRUMENTS THAT ALREADY READ THIS SHEET COULD NOT FIND IT.
// `deadStyles.test.mjs` asks whether a class name is ever EMITTED, `stylesheet-hygiene` and the two design
// contracts read declarations, and `feedback-check`'s shadow scan reads `:active`/`:hover` pairs. All five read
// the sheet as TEXT, and every one of them is blind to this class of defect for the same reason: **the rule IS
// used by name.** A rule can be dead while its class is alive on the page — it just never wins on any element
// that has it, because a DENSER rule covers every one of them for every property it sets.
//
// The instance that produced this file was `.auth-lang` in the console's sheet:
//
//     .auth-aside .auth-lang { position: absolute; top: 24px; right: 24px; }   /* (0,2,0) — the live one */
//     .auth-lang            { position: absolute; top: 18px; right: 18px; }   /* (0,1,0) — never applies */
//
// The base rule was not a harmless duplicate: it carried DIFFERENT NUMBERS (18/18 against 24/24). It was left
// behind when the brand block moved out of the card, and nothing could see it, because the element it names is
// on the page and the class it names is in the markup. `.auth-lang` was deleted; this is what keeps the next
// one from arriving.
//
// THE CRITERION — the whole of it, and it needs a rendered DOM rather than the sheet:
//
//     [...document.querySelectorAll(base)].filter(e => !e.matches(denser)).length === 0
//
// Generalized from one pair to the whole sheet: a base rule `.foo` CAN NEVER WIN when it matches at least one
// element and, for EVERY element it matches and EVERY property it declares, some denser rule that also matches
// that element declares that property. (Denser = higher specificity, or equal specificity and later in the
// sheet — the cascade's own order. A property the base marks `!important` is only beaten by an `!important`.)
//
// THE CONTROL IS THE POINT, and this file asserts it rather than describing it. A gate written as "there is a
// denser rule with an overlapping property" finds 115 rules in this repository's seven sheets and 114 of them
// are ordinary CSS design — `.btn` is the base, `.modal-card .btn` is a variant, and buttons outside the modal
// still use the base. A gate that flags 115 things is a gate nobody reads. So the same sheet must yield a
// NEGATIVE the gate can be checked against:
//
//     .auth-lang   matched 1, denser ".auth-aside .auth-lang"  → survives 0  → DEAD (the real finding)
//     .auth-tab    matched 3, denser ".auth-tab.active"        → survives 2  → alive (THE CONTROL)
//
// `.auth-tab` differs from `.auth-lang` in exactly one way that matters — a tab that is not `.active` is left
// to the base rule — and the control asserting that is what separates this gate from the 115-finding one. The
// DOM is the REAL login route (see below), so it contains both; a hand-made fixture would only prove that the
// gate agrees with whoever wrote the fixture.
//
// MUTATION: a second `.auth-lang`-shaped pair — a class used only inside a container that a denser rule already
//           styles completely. Appended to `src/styles/globals.css`, which is all this criterion reads on the
//           sheet side (the mutated class is already on the rendered page, so no rebuild is needed to plant it):
//
//               .auth-aside .auth-pitch {
//                 position: relative; max-width: 30em; font-size: var(--fs-md);
//                 line-height: 1.7; color: var(--text-secondary);
//               }
//
//           That is precisely the `.auth-lang` accident as a refactor would produce it: copy the rule under a
//           container selector, leave the old one behind. `.auth-pitch` matches one element (the paragraph in
//           `<aside class="auth-aside">`) and the new rule covers all five of its properties on it.
// RESULT:   exit 1 — `✖ no rule in the console's sheet can never win`, `ℹ pass 2`, `ℹ fail 1`:
//
//             "the console has 1 rule(s) that can never win, on the login route:
//                .auth-pitch — matched 1 element(s), and a denser rule covers position, max-width,
//                font-size, line-height, color on every one of them
//              Delete the base rule (a rule no element is left to cannot apply), or make the denser one
//              narrower so the elements it does not cover keep the base."
//
//           The CONTROL passed in the same run (`✔ the control is not flagged`), which is the point: one run
//           flagged the planted rule and left `.auth-tab` alone. Reverted, the same three tests pass with
//           `0 that can never win`.
//
// BOUNDARY — what this gate does NOT test, measured rather than assumed, because a boundary read as coverage
// is worse than no gate:
//
//   * IT TESTS ONE ROUTE. The DOM is the login page (mounted by the built bundle with `/api/me` answering 401),
//     so only rules whose class appears THERE are testable. Measured on this sheet after `.auth-lang` was
//     deleted: 181 unique standalone class rules, 196 rule occurrences (the extra 15 are re-declarations inside
//     media blocks), of which **178 occurrences match ZERO elements on this route** and only 18 are testable at
//     all. "Matched no element on this page" is a DIFFERENT question with a different answer (296 of 332 rules
//     here, nearly all pseudo-elements, other routes, theme variants, or another surface's classes sharing the
//     sheet) and it is NOT what this gate reports.
//   * IT SKIPS STATE AND PSEUDO-ELEMENT SELECTORS as CANDIDATES for the denser rule: `:hover`, `:focus`,
//     `:active`, `:disabled` and `::before` apply to an element only sometimes, so a base rule they cover is
//     not a rule that can never win — at rest it is the only one that applies.
//   * IT DOES NOT EVALUATE `@media` CONDITIONS. Rules inside the sheet's media blocks ARE scanned (a dead rule
//     at a breakpoint is still dead), but two rules that can never be active at the same viewport are still
//     compared. On this sheet that costs nothing — the one finding is top-level — but it is where a false
//     positive would come from.
//   * SPECIFICITY IS COUNTED (ids, classes/attributes/pseudo-classes, elements), not computed by a browser
//     engine. It is the same arithmetic `scripts/test/feedback-check.mjs` uses for its shadow scan.
//
// WHERE IT RUNS. It needs a rendered DOM and no browser: `querySelectorAll` and `matches` are selector matching
// over a DOM tree, which jsdom implements, so this IS a CI job — the `ui` job, which runs `npm run build`
// before `npm test`. Run it by hand the same way:
//
//     cd gateway/ui && npm run build && npm test        # or: node --test test/deadCascade.test.mjs
//
// It is therefore NOT on the device-browser path (`live-panel-probe.mjs`) and needs no deploy: the DOM comes
// from the bundle this tree just built, and `scripts/test/console-assets-check.mjs` is what keeps that bundle
// equal to a rebuild of `src/`. What it does NOT get from jsdom is LAYOUT — no geometry is measured here, and
// a rule's position on the page is not evidence this gate can produce.
// `after` IS IMPORTED, NOT TAKEN OFF `test`. The module-level hooks are the DOCUMENTED API — Node 22's own
// reference lists `before`/`after`/`beforeEach`/`afterEach` as module exports, and this repository's 107
// `node:test` suites contain no prior art for either form. `test.after` works on Node 24 (this box) and is
// *not* documented on Node 22, which is what the `ui` CI job runs; a test file that throws on import fails as a
// whole, so the documented form is the one to write.
import test, { after } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { JSDOM } from "jsdom";

const UI = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SHEET = path.join(UI, "src", "styles", "globals.css");

/** A rule applies only SOMETIMES when its selector says so; those cannot make a base rule unwinnable. */
const DYNAMIC = /::|:(hover|focus|focus-visible|focus-within|active|disabled|enabled|checked|indeterminate|target|visited|link|any-link|placeholder-shown|user-invalid|user-valid|open|popover-open|autofill|default|required|optional|valid|invalid|in-range|out-of-range|read-only|read-write)\b/;

/** (ids, classes/attributes/pseudo-classes, elements) — `feedback-check.mjs`'s arithmetic, kept identical. */
function specificity(sel) {
  const ids = (sel.match(/#[\w-]+/g) || []).length;
  const cls = (sel.match(/\.[\w-]+|\[[^\]]+\]|:(?!:)[\w-]+(\([^)]*\))?/g) || []).length;
  const els = (sel.replace(/[#.][\w-]+|\[[^\]]+\]|::?[\w-]+(\([^)]*\))?/g, " ").match(/[a-zA-Z][\w-]*/g) || []).length;
  return [ids, cls, els];
}
const beats = (a, b) => (a[0] !== b[0] ? a[0] > b[0] : a[1] !== b[1] ? a[1] > b[1] : a[2] > b[2]);
const ties = (a, b) => a[0] === b[0] && a[1] === b[1] && a[2] === b[2];

/**
 * THE LOGIN ROUTE, RENDERED — the repo's own harness rather than a second one: `render-smoke.mjs` mounts the
 * built bundle in jsdom with `/api/me` answering 401, which is the state that renders `Auth` (its `RENDER OK`).
 * WHAT THIS ADDS is only the DOM object itself, because a smoke that prints is not a DOM a test can query.
 */
function renderLoginRoute() {
  const html = readFileSync(path.join(UI, "..", "public", "index.html"), "utf8");
  const asset = /assets\/(index-[^"]*\.js)/.exec(html)?.[1];
  assert.ok(asset, "gateway/public/index.html names no built bundle — run `npm run build` in gateway/ui first");
  const jsPath = path.join(UI, "..", "public", "assets", asset);
  assert.ok(
    existsSync(jsPath),
    `gateway/public/assets/${asset} is missing — the DOM this gate measures comes from the built console, so run \`npm run build\` in gateway/ui first`,
  );
  const js = readFileSync(jsPath, "utf8");

  const dom = new JSDOM(html, { url: "https://ai.saisi.online/", runScripts: "outside-only", pretendToBeVisual: true });
  const { window } = dom;
  window.fetch = async () =>
    new Response(JSON.stringify({ type: "error", error: { message: "unauthorized" } }), { status: 401 });
  // jsdom evaluates the bundle as a classic script, so `import.meta` needs a harness-only shim (render-smoke.mjs).
  window.__ims = (s) => s;
  window.eval(
    js
      .replaceAll("import.meta.resolve", "window.__ims")
      .replaceAll("import.meta.url", JSON.stringify("https://ai.saisi.online/")),
  );
  // AND THE MOUNT IS ASYNCHRONOUS — `render-smoke.mjs` waits 400 ms for React to paint, and the first version of
  // this gate did not. It read an EMPTY document, so every class rule matched zero elements and the contract
  // below passed VACUOUSLY; the floor test is what refused it. A gate that cannot tell "no findings" from "no
  // subject" is the defect this whole file is about, arriving from the other side.
  return new Promise((resolve) => setTimeout(() => resolve(window), 500));
}

/** The sheet as RULES, through a CSSOM rather than a regex over the text (multi-line selectors and comments
 *  are two things this repository's text scanners have already misread once). Media blocks are descended. */
function readSheet() {
  const css = readFileSync(SHEET, "utf8");
  const dom = new JSDOM(`<!doctype html><html><head><style>${css}</style></head><body></body></html>`);
  const rules = [];
  const walk = (list) => {
    for (const r of list) {
      if (r.cssRules && r.media) { walk(r.cssRules); continue; } // an @media block is not a rule; its contents are
      if (r.selectorText) rules.push({ sel: r.selectorText, style: r.style, order: rules.length });
    }
  };
  walk(dom.window.document.styleSheets[0].cssRules);
  return rules;
}

function declarations(style) {
  const out = new Map();
  for (let i = 0; i < style.length; i++) {
    const prop = style[i];
    out.set(prop, { value: style.getPropertyValue(prop), important: style.getPropertyPriority(prop) === "important" });
  }
  return out;
}

/** THE CRITERION. Returns every standalone class rule that matches ≥1 element and can never win on any of them. */
function rulesThatCanNeverWin(document, rules) {
  const parsed = rules.map((r) => ({ ...r, spec: specificity(r.sel), decls: declarations(r.style) }));
  const bases = parsed.filter((r) => /^\.[A-Za-z0-9_-]+$/.test(r.sel));
  const enumerated = new Set();
  const testable = [];
  const zeroMatch = [];
  const dead = [];

  for (const b of bases) {
    enumerated.add(b.sel);
    let matched;
    try {
      matched = [...document.querySelectorAll(b.sel)];
    } catch {
      continue; // a selector jsdom cannot parse is not evidence of anything
    }
    if (!matched.length) { zeroMatch.push(b.sel); continue; }
    const declared = declarations(b.style);
    if (!declared.size) continue;
    testable.push(b.sel);

    // Per element: is EVERY property this rule sets already set, to a winning effect, by a denser rule?
    const uncovered = [];
    for (const el of matched) {
      for (const [prop, own] of declared) {
        const beaten = parsed.some((r) => {
          if (r.sel === b.sel && r.order === b.order) return false;
          if (DYNAMIC.test(r.sel)) return false;
          if (!(beats(r.spec, b.spec) || (ties(r.spec, b.spec) && r.order > b.order))) return false;
          const theirs = r.decls.get(prop);
          if (theirs === undefined) return false;
          if (own.important && !theirs.important) return false;
          try { return el.matches(r.sel); } catch { return false; }
        });
        if (!beaten) { uncovered.push({ prop, el }); break; }
      }
      if (uncovered.length) break;
    }
    if (!uncovered.length) dead.push({ sel: b.sel, matched: matched.length, props: [...declared.keys()] });
  }
  return { enumerated: [...enumerated], testable, zeroMatch, dead, ruleCount: rules.length };
}

const window = await renderLoginRoute();
const document = window.document;
const report = rulesThatCanNeverWin(document, readSheet());
// jsdom's `pretendToBeVisual` keeps a requestAnimationFrame loop running; without this the child process outlives
// the assertions by ~30s waiting for the event loop to drain.
after(() => window.close());

// A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN (this repository's oldest rule). Three floors, because three
// things have to hold at once for the contract below to mean anything: the sheet was read, the ROUTE was
// mounted, and the criterion had something to test.
test("the cascade scan read the console's sheet and the login route", () => {
  assert.ok(report.ruleCount > 300, `read ${report.ruleCount} rule(s) from globals.css — the sheet did not parse`);
  assert.ok(
    report.enumerated.length > 150,
    `found ${report.enumerated.length} standalone class rule(s); this sheet declares ~181, so the scan is reading the wrong thing`,
  );
  assert.ok(
    document.querySelector(".auth-card"),
    "the login route did not render, so every class rule matched zero elements and the contract below would pass vacuously",
  );
  assert.ok(document.querySelectorAll(".auth-tab").length >= 3, "the login route rendered without its tab track");
  assert.ok(
    report.testable.length >= 10,
    `only ${report.testable.length} rule(s) were testable — expected the login route to exercise more of the sheet`,
  );
});

// THE CONTRACT.
test("no rule in the console's sheet can never win", () => {
  const named = report.dead
    .map((d) => `  ${d.sel} — matched ${d.matched} element(s), and a denser rule covers ${d.props.join(", ")} on every one of them`)
    .join("\n");
  assert.deepEqual(
    report.dead,
    [],
    `the console has ${report.dead.length} rule(s) that can never win, on the login route:\n${named}\n` +
      `Delete the base rule (a rule no element is left to cannot apply), or make the denser one narrower so the ` +
      `elements it does not cover keep the base.`,
  );
});

// THE CONTROL — the half that makes the contract above mean something. A gate that flags every base rule with a
// denser sibling flags 115 rules in this repository and is a gate nobody reads; `.auth-tab` is the shape it must
// NOT flag, and asserting it here is what proves the criterion still draws that line.
test("the control is not flagged: a base rule with a denser sibling that leaves some elements to it", () => {
  const tabs = [...document.querySelectorAll(".auth-tab")];
  const active = [...document.querySelectorAll(".auth-tab.active")];
  assert.ok(tabs.length >= 3, `the control needs the tab track; found ${tabs.length} .auth-tab element(s)`);
  assert.ok(active.length >= 1, "the control needs one .auth-tab.active, or the denser rule is not on the page");
  assert.ok(
    tabs.length - active.length >= 1,
    "the control is vacuous: every .auth-tab is .active, so .auth-tab.active covers the base rule completely",
  );
  assert.ok(
    report.testable.includes(".auth-tab"),
    ".auth-tab was not testable, so this control proves nothing about what the gate does with a live base rule",
  );
  assert.ok(
    !report.dead.some((d) => d.sel === ".auth-tab"),
    "THE GATE OVER-FIRES: .auth-tab is ordinary CSS — .auth-tab.active is a VARIANT and the other tabs keep the base rule",
  );
  console.log(
    `deadCascade: ${report.enumerated.length} standalone class rule(s), ${report.ruleCount} rule occurrence(s); ` +
      `${report.zeroMatch.length} occurrence(s) match no element ON THIS ROUTE (a different question, not reported as dead); ` +
      `${report.testable.length} testable; ${report.dead.length} that can never win. ` +
      `Control .auth-tab: matched ${tabs.length}, .auth-tab.active covers ${active.length}, ${tabs.length - active.length} survive — NOT flagged.`,
  );
});
