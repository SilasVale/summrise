#!/usr/bin/env node
// Render the REAL panel bundle with a stubbed device API, and audit it.
//
// WHY THIS EXISTS. Every earlier visual check in this repo was a hand-built HTML
// gallery: I wrote the markup, linked the stylesheet, and measured the elements I
// had just created. That verifies the CSS I was thinking about and nothing else —
// and it is how five chrome contrast defects lived through several rounds of
// "auditing": `#session-count`, `.side-time`, `.side-count`, `.tab.active` and
// `.view-switch-btn.active` were wrong the whole time and no feature-by-feature
// gallery could see them, because I only ever measured what I was working on.
//
// This harness instead loads `resources/panel/panel.js` and `panel.css` — the
// exact bytes the agent embeds via include_str! — into a page that serves at a
// real `/panel/` origin, with `window.fetch` stubbed to return a fixed device
// state. The app boots through its own production path (including computeBoot's
// same-origin branch) and renders its own component tree. Then it:
//
//   1. measures EVERY visible text node, alpha-compositing both background alpha
//      and the ancestor `opacity` chain (element opacity is in NEITHER
//      getComputedStyle(color) NOR backgroundColor — a probe that ignores it
//      reports a dimmed element at its full colour);
//   2. asserts each governance element is PRESENT, so a clean contrast sweep over
//      a page that failed to render cannot pass;
//   3. checks nothing in the top bar overflows.
//
// Pages are addressed over http://vale.test/panel/ and satisfied by Playwright
// route interception, so NO listener is opened anywhere — the run is safe on a box
// where binding a port is not allowed.
//
// Usage (needs the Playwright runtime; see VALE_BROWSER_HELPER):
//   node scripts/panel-render-audit.mjs --out /tmp/panel-audit
//
// Regenerate panel.js/panel.css first: (cd agent/resources/panel-react && npm run build)


import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createHash } from "node:crypto";
// THE PROBE IS IMPORTED, NOT REFERENCED (round 265). This file's own emit message has said "the PROBE moved out of this
// file so its math can be UNIT TESTED" — and the audit half kept calling a bare `PROBE` that no longer exists, so the
// run that a browser makes possible died on `PROBE is not defined` before measuring one row. Nothing caught it: CI runs
// the emit path (no browser), the skip-path gate pins exit 2, and the only environment that reaches line 911 is a
// device — where the failure looked like the audit being broken rather than a missing import.
// ALIASED ON PURPOSE: this file counts its own `failures`, and shadowing that name with the helper would be a
// redeclaration error rather than a bug anybody could read.
import { PROBE_SOURCE, failures as contrastFailures, unmeasurable } from "./lib/contrast-probe.mjs";
// AND THE SAME WAIVER POLICY THE SWEEP JUDGES WITH (round 265). The first run of this audit that ever completed
// reported two failures the design sweep waives on purpose — `span.approval-grant` at 1.19 light / 1.25 dark: the
// grant pill's outline, whose signal is the TEXT inside it and whose band the sweep measured and wrote down. Two
// instruments reading one measurement and reaching two verdicts is a policy kept in the wrong place; the list lives
// in `lib/design-sweep.mjs` now and both read it, band semantics included (a row matching a waived selector at a
// DIFFERENT ratio is still a failure — a waiver is for the ratio it was measured at).
import { DECORATIVE_WAIVERS } from "./lib/design-sweep.mjs";
import { bundleSweep, piecesModule } from "./lib/sweep-bundle.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const PANEL = join(ROOT, "agent", "resources", "panel");
const OUT = (() => {
  const i = process.argv.indexOf("--out");
  return i >= 0 ? process.argv[i + 1] : "/tmp/panel-render-audit";
})();

const SID = "term-audit-0";

// A device state that exercises every governance surface at once: a stated goal,
// the gate armed, a command waiting for a decision, a grant already in force, and
// a trail carrying intent + the branches not taken.
const EVENTS = [
  { seq: 1, ts: 1789000000, kind: "status", status: "opened" },
  { seq: 2, ts: 1789000001, kind: "goal", text: "provision the ONU at 0/1 on VLAN 100, then save the config" },
  { seq: 3, ts: 1789000002, kind: "approval", status: "armed" },
  {
    seq: 4, ts: 1789000010, kind: "command/start", command: "display ont info 0 1",
    intent: "check whether the ONU is actually online before changing its config",
    considered: ["reset the ONU", "check the OLT uplink first"],
  },
  { seq: 5, ts: 1789000011, kind: "output", text: "ONT 0/1 online, VLAN 1" },
  { seq: 6, ts: 1789000012, kind: "command/end", exit_code: 0, duration_ms: 900 },
  { seq: 7, ts: 1789000020, kind: "approval", status: "approved", text: "vlan 100" },
  { seq: 8, ts: 1789000021, kind: "approval", status: "granted", text: "vlan" },
  { seq: 9, ts: 1789000030, kind: "command/start", command: "vlan 100", intent: "apply the change" },
  { seq: 10, ts: 1789000031, kind: "command/end", exit_code: 1, duration_ms: 200 },
  // A COMMAND THAT NEVER ENDED, AND WHY IT LIVES HERE (round 27). Round 26 added this to the OPERATION feed and
  // claimed cmd-dot's `running` state was then rendered; the mark-coverage note said otherwise, in the same CI log
  // ("cmd-dot ... rendered 3 (fail, muted, ok)"), because the operation feed is the History page's Runs scope and
  // the command CARDS are built from THIS trail (useCommandEvents: "A trailing start with no end: still running ...
  // surface it as a LIVE card"). One of the six cmd-dot states had never been painted anywhere; this is the stub
  // that paints it, and the note is what says whether it worked.
  // THE TWO ENDINGS THAT WERE NEVER PAINTED (round 31 of the standing goal). The mark-coverage note has reported
  // `cmd-dot` as "rendered 4 (fail, muted, ok, running)" for rounds — the interrupter and the backgrounded command are
  // the two states of the same six-value vocabulary that no surface produced, and `bg` is precisely the state round 29
  // had to stop the trajectory from renaming to `warn`. The reason string travels on `command/end` and is what
  // `stateFromEnd` switches on, so two pairs of events are all it takes to give both a surface.
  { seq: 13, ts: 1789000050, kind: "command/start", command: "ping -t 192.168.1.1", intent: "watch the link while the config is saved" },
  { seq: 14, ts: 1789000051, kind: "command/end", reason: "backgrounded", duration_ms: 400 },
  { seq: 15, ts: 1789000060, kind: "command/start", command: "scp firmware.bin stc@192.168.1.1:/tmp/", intent: "stage the image" },
  { seq: 16, ts: 1789000061, kind: "command/end", reason: "interrupted", duration_ms: 1500 },
  { seq: 11, ts: 1789000040, kind: "output", text: "waiting for the uplink to settle" },
  { seq: 12, ts: 1789000041, kind: "command/start", command: "reboot", intent: "pick up the new firmware slot" },
];

const SESSION = {
  id: SID, kind: "pty", label: "d1", status: "live", bytes: 512,
  // `idle_ms` IS THE PER-SESSION FACT THE MARK LANGUAGE READS (lib/liveness.ts): the agent's own
  // `last_output.elapsed()`, and a session inside the window is WORKING — the halo. THE FIXTURE HAD NO SUCH FIELD
  // AT ALL, so every sweep this harness ever produced photographed a panel where no session could be working:
  // the marquee silhouette of the whole language was invisible to the gate that checks silhouette coverage.
  // 900 ms here, and the quiet seeds below, so the strip carries one halo and two rings.
  idle_ms: 900,
  held_by_human: false, approval_required: true, approval_grants: ["display"],
  goal: "provision the ONU at 0/1 on VLAN 100, then save the config",
  pending_approval: {
    id: "ap-1",
    command: "vlan 100 / port vlan 100 0/1 1",
    expires_in_ms: 47000,
  },
};

/** Elements the governance surface must render. A contrast sweep that measured a
 *  blank page would otherwise be a clean, meaningless PASS. */
const REQUIRED = [
  ["goal bar", "#goal-bar"],
  ["goal text", ".goal-text"],
  ["approval prompt", ".approval-prompt"],
  ["pending command", ".approval-cmd"],
  ["countdown", ".approval-left"],
  ["approve button", ".approval-approve"],
  ["refuse button", ".approval-refuse"],
  ["remember button", ".approval-remember"],
  ["session tab", ".tab-name"],
  ["view switch", ".view-switch-btn.active"],
  ["status session count", "#session-count"],
  ["side time", ".side-time"],
  ["side count", ".side-count"],
];

/** The harness fixture's own data, as a module (the three values that used to be interpolated into the stub). */
function stubPieces() {
  // ONE PIECES GENERATOR, IN THE ASSEMBLER (round 272) — these three are plain data, so there is no quoting to think
  // about here at all; that is the point of the generator owning it.
  return piecesModule({ sid: SID, session: SESSION, events: EVENTS });
}

function buildHarness() {
  // A URL MODE (round 230). With VALE_PANEL_BUNDLE_URL set, the harness REFERENCES the panel instead of inlining it: the
  // emitter then runs where the repo is NOT (on a device), and the page it builds loads the very bundle that device serves —
  // the same principle the live probe follows, and the reason a device run no longer needs the 660 KB artifact carried to it.
  // Read-only, no writes: the emitter's only other job is to write the harness out.
  const bundleUrl = (process.env.VALE_PANEL_BUNDLE_URL || "").replace(/\/+$/, "");
  const css = bundleUrl ? "" : readFileSync(join(PANEL, "panel.css"), "utf8");
  // WHAT THIS HARNESS CANNOT REACH, measured rather than assumed (round 45): the evidence drawer —
  // and with it the browser-action badges and the screenshot timestamp — renders ONLY in the
  // Electron shell, because BrowserPage mounts its pane behind `window.valeEmbedded` and a plain
  // browser gets an explanation page instead. A fixture for it was tried and removed: it could not
  // render, and a dead fixture is a lie about coverage. Those three badge inks were therefore fixed
  // on the STATIC pair sweep's measurement (1.99 -> 5.73+ on the light chrome surface) and are
  // confirmed there and in the token contract; the rendered confirmation needs the desktop app.
  const js = bundleUrl ? "" : readFileSync(join(PANEL, "panel.js"), "utf8");
  // No closing script tag may appear in ANY inline script, and there are two of them: the bundle
  // here and the stub below. Checking only the bundle is how a comment in the stub silently cut the
  // whole fixture in half — the page still loaded, still rendered a panel, and simply had no
  // sessions, which reads like a product bug rather than a broken harness.
  if (!bundleUrl && /<\/script/i.test(js)) throw new Error("panel.js contains a closing script tag — inline embedding is unsafe");

  // THE STUB IS A REAL MODULE NOW (round 270), ASSEMBLED FOR A BROWSER. It was a 640-line template literal with three
  // interpolations (the session id, the session object, the event list), and two of its three guards existed only
  // because of that: a search for a backtick ("a backtick in this template ends the literal and the file stops
  // parsing" — round 154 wrote two of them into a comment ABOUT the other trap) and a compile of the emitted text.
  // lib/sweep/panel-stub.cjs is ordinary code, the fixture data arrives as the pieces module, and `target: "browser"`
  // is what makes the assembler leave `require` out of the preamble entirely — this payload runs in a PAGE, where
  // capturing a native require would throw before the fixture had stubbed anything.
  const { code: stub } = bundleSweep({
    entry: "panel-stub.cjs",
    target: "browser",
    modules: {
      "panel-stub.cjs": readFileSync(join(ROOT, "agent", "scripts", "lib", "sweep", "panel-stub.cjs"), "utf8"),
      "pieces.cjs": stubPieces(),
    },
  });

  // THE ONE GUARD THAT STAYS, BECAUSE IT IS ABOUT THE PAGE RATHER THAN ABOUT ESCAPING: this payload is inlined into an
  // HTML <script> tag, and a closing script tag anywhere inside it — even in a comment — cuts the fixture in half. The
  // page still loads, still renders a panel, and simply has no sessions, which reads like a product bug rather than a
  // broken harness (round 156's shape).
  if (/<\/script/i.test(stub)) throw new Error("the stub contains a closing script tag — it would cut the fixture short");
  // NO BACKTICK SEARCH AND NO COMPILE HERE ANY MORE (round 270). Both existed because the stub was a template literal:
  // a backtick ended it, and an escape collapsed by a three-layer edit could become a real newline that made the
  // emitted stub a syntax error while the emitter reported success. The stub is a module and the assembler compiles
  // what it returns, so the first hazard is gone and the second is the assembler's contract — the same one every
  // emitter in this repository now shares.
  // A STALE HARNESS MUST SAY SO. The panel sweep measures a DELIVERED copy of this file, and rounds 189
  // found one that predated round 172's tab-strip fix: its inlined CSS collapsed the strip to 17px, the
  // sweep reported real-looking overflow findings, and a waiver hid them. Nothing in a report said which
  // generation had been measured. This stamp is that missing fact — the built stylesheet's size and hash,
  // carried into every report so a reader can see the harness is older than the build it should match.
  // THE STAMP CAN COME FROM THE CALLER (round 237). When the sheet is not in this tree — a device running URL mode, where the
  // panel is served rather than checked out — the only way to keep one identity for it is for whoever CAN read it to hand the
  // value over. One environment variable, the same on both emitters, so the harness and the sweep that judges it cannot
  // disagree about which stylesheet was measured.
  const stamp = process.env.VALE_HARNESS_STAMP || (() => {
    try {
      const css = readFileSync(join(ROOT, "agent", "resources", "panel", "panel.css"));
      return css.length + "-" + createHash("sha256").update(css).digest("hex").slice(0, 12);
    } catch (e) { return "unknown"; }
  })();
  return `<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="vale-harness-build" content="${stamp}">
<meta name="viewport" content="width=device-width, initial-scale=1"><title>Vale Agent</title>
<style>${css}</style>${bundleUrl ? `<link rel="stylesheet" href="${bundleUrl}/panel.css">` : ""}</head><body><div id="root"></div>
<script>${stub}</script>${bundleUrl ? `<script type="module" src="${bundleUrl}/panel.js"></script>` : `<script type="module">${js}</script>`}</body></html>`;
}

const OVERFLOW = `(() => {
  const bad = [];
  for (const el of document.querySelectorAll('#root *')) {
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) continue;
    if (el.scrollWidth > el.clientWidth + 1) {
      const cls = typeof el.className === 'string' ? el.className.trim().split(/\\s+/)[0] : '';
      bad.push(el.tagName.toLowerCase() + (cls ? '.' + cls : '') + ' over=' + (el.scrollWidth - el.clientWidth));
    }
  }
  return [...new Set(bad)].slice(0, 12);
})()`;

async function main() {
  mkdirSync(OUT, { recursive: true });
  const html = buildHarness();
  const harnessPath = join(OUT, "panel-harness.html");
  writeFileSync(harnessPath, html);

  // WHERE THIS RUNS. The audit needs a Playwright runtime, and on the Linux dev
  // box there is none that can launch (the system chromium is missing
  // libatk-1.0.so.0 and there is no sudo). The runtime that DOES work is the
  // device's bundled one, reached through the agent's `browser_run_script` tool —
  // which is how every round of this work actually audited the panel. So:
  //
  //   * with VALE_BROWSER_HELPER set (on a device, or any box with the bundle)
  //     this script runs the whole audit itself;
  //   * without it, it still generates the harness and prints the probe, so the
  //     same measurement can be driven from wherever a runtime exists.
  //
  // Emitting rather than failing is the point: a check that can only run in one
  // environment is a check that quietly stops running.
  const helper = process.env.VALE_BROWSER_HELPER;
  if (!helper) {
    console.log("No VALE_BROWSER_HELPER — harness written, running in EMIT mode.");
    console.log("  harness: " + harnessPath);
    console.log("  drive it by loading that file in any Playwright page, routing");
    console.log("  http://vale.test/** to its body, then evaluating the PROBE from");
    console.log("  lib/contrast-probe.mjs and the OVERFLOW snippet in this file.");
    console.log("  (The PROBE moved out of this file so its math can be UNIT TESTED —");
    console.log("   scripts/test/contrast-probe-check.mjs exercises the exact text the");
    console.log("   browser evaluates.)");
    // A SKIP IS NOT A PASS, AND THE EXIT CODE SAYS WHICH.
    //
    // This used to exit 0, and the message above used to admit it: "a caller
    // watching only the exit code reads this skip as a pass". Writing the defect
    // down and leaving it is how it survived — the sentence read as a caveat
    // rather than as a bug.
    //
    // The convention, now explicit and pinned by scripts/test/panel-audit-skip-check.mjs:
    //   0  the audit RAN and found nothing
    //   1  the audit RAN and found failures
    //   2  the audit DID NOT RUN (no Playwright runtime in this environment)
    //
    // 2 is deliberately NOT 0. Emitting the harness instead of failing is still
    // right — a check that can only run in one environment quietly stops running —
    // but "I could not look" must be distinguishable from "I looked and it is
    // clean", and the exit code is the only channel a caller is guaranteed to read.
    console.log("EXIT 2: THE AUDIT DID NOT RUN — this is a SKIP, not a pass.");
    process.exit(2);
  }
  // A DYNAMIC import() NEEDS A URL ON WINDOWS (round 242). `VALE_BROWSER_HELPER` is an absolute path like
  // `D:\Vale\components\playwright\helper.mjs`, and `await import()` of that string dies with
  // "Only URLs with a scheme in: file, data, and node are supported ... Received protocol 'c:'" — which is what a runner sets
  // and what a bare terminal does NOT, so the same emitter worked from a PTY and failed under `browser_run_script`. The
  // emitted PROBE never hit this because its `require(process.env.VALE_BROWSER_HELPER)` is CommonJS, which is happy with a
  // Windows path: one environment value, two module systems, one of them strict about it.
  const { acquireBrowser } = await import(pathToFileURL(helper).href);
  const { page, close } = await acquireBrowser();

  // Served at a REAL origin and path, satisfied by interception — the panel sees
  // location.pathname === "/panel/" and boots through its own production branch.
  // No listener is opened anywhere.
  await page.route("http://vale.test/**", (route) =>
    route.fulfill({ status: 200, contentType: "text/html; charset=utf-8", body: html }),
  );

  const pageErrors = [];
  page.on("pageerror", (e) => pageErrors.push(String(e.message).slice(0, 200)));

  let failures = 0;
  let measured = 0;
  let low = 0;

  for (const theme of ["light", "dark"]) {
    for (const mode of ["pending", "idle"]) {
      await page.setViewportSize({ width: 1280, height: 720 });
      await page.goto(`http://vale.test/panel/?theme=${theme}&mode=${mode}`, { waitUntil: "load" });
      await page.waitForTimeout(1800);

      // THE SHARED JUDGEMENT, NOT A LOCAL `cr < 4.5` (round 265). The audit's own filter predated the probe's
      // `need` / `inactive` fields: it asked 4.5 of every row including GRAPHICS (which need 3, so it would have
      // reported false failures) and counted an UNREADABLE row as passing (every comparison against null is false),
      // which is the "an absence is not evidence" rule this suite applies everywhere else.
      const rows = await page.evaluate(PROBE_SOURCE);
      const all = contrastFailures(rows);
      const blind = unmeasurable(rows);
      measured += rows.length;
      // A WAIVED ROW IS PRINTED, NOT COUNTED — and a waived SELECTOR at a ratio outside its measured band stays a
      // failure, which is the half that keeps the exemption from quietly widening (the sweep learned that in round
      // 95: an entry written for one number set aside every ratio that element could ever produce).
      const waivedHere = [];
      const under = [];
      for (const r of all) {
        const why = DECORATIVE_WAIVERS.find((d) => d.match.test(String(r.sel)));
        if (why && (why.values || []).some(([lo, hi]) => typeof r.cr === "number" && r.cr >= lo && r.cr <= hi)) {
          waivedHere.push(`${r.sel} ${r.cr} — ${why.reason}`);
          continue;
        }
        under.push(r);
      }
      low += under.length;
      console.log(`\n--- ${theme} / ${mode}: ${rows.length} nodes, ${under.length} failing${blind.length ? `, ${blind.length} UNMEASURABLE` : ""}${waivedHere.length ? `, ${waivedHere.length} waived` : ""} ---`);
      for (const w of waivedHere) console.log(`  WAIVED ${w}`);
      if (blind.length) {
        failures++;
        console.log(`  UNMEASURABLE ${blind.length} row(s) — a reading nobody could take is not a pass: ${blind.slice(0, 3).map((r) => r.sel).join(", ")}`);
      }
      for (const r of under) {
        failures++;
        console.log(`  LOW ${String(r.cr).padStart(5)}  ${String(r.size).padStart(4)}px  ${r.sel.slice(0, 46)}  "${r.text}"`);
      }

      // Presence: a clean sweep over a page that did not render proves nothing.
      if (mode === "pending") {
        const found = await page.evaluate(
          (sels) => Object.fromEntries(sels.map(([k, s]) => [k, !!document.querySelector(s)])),
          REQUIRED,
        );
        for (const [k] of REQUIRED) {
          if (!found[k]) {
            failures++;
            console.log(`  MISSING  ${k}`);
          }
        }
        const over = await page.evaluate(OVERFLOW);
        for (const o of over) {
          failures++;
          console.log(`  OVERFLOW ${o}`);
        }
        if (pageErrors.length) {
          failures++;
          console.log(`  PAGE ERRORS ${JSON.stringify(pageErrors.slice(0, 3))}`);
        }
        await page.screenshot({ path: join(OUT, `panel-${theme}.png`), fullPage: false });
      }
    }
  }

  // NODES, NOT "TEXT NODES": the probe reads graphic rows too (a mark, a ring, a chip's fill), and they are judged
  // at 3:1 rather than 4.5 — so "under AA" was the wrong noun for a count that includes them (round 265).
  console.log(`\n== ${measured} nodes measured, ${low} failing, ${failures} failure(s) ==`);
  console.log(`screenshots: ${OUT}/panel-light.png, ${OUT}/panel-dark.png`);
  await close();
  process.exit(failures ? 1 : 0);
}

main().catch((e) => {
  console.error("FATAL " + e.message);
  process.exit(1);
});
