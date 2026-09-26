#!/usr/bin/env node
// console-design-sweep — the gateway console's design, measured like the panel's.
//
// The console is a second UI behind Cloudflare Access, which is why nothing rendered it until round
// 55. Measuring it needs no login: build it to a temp dir, ship that to the device, and serve it
// from disk through Playwright routes with the same /api fixtures its own render smokes use.
//
//   cd gateway/ui && npx vite build --outDir /tmp/console-build --emptyOutDir
//   tar czf /tmp/console.tgz -C /tmp/console-build .   (ship it; extract on the device to
//                                                       C:\ProgramData\Summrise\pwout\console)
//   node agent/scripts/console-design-sweep.mjs --emit > /tmp/console-sweep.js   (run on the device)
//   node agent/scripts/console-design-sweep.mjs --judge <report.json>
//
// The checks and the judge are the shared core's; this file supplies the URL, the page list, the API
// fixtures, the login pass (which exists only when /api/me answers 401) and the three widths.
//
// AXES COVERED, so nobody re-measures what is already known (round 85):
//   contrast (resting) · contrast (HOVERED — the 24 :hover rules in the console's sheet) · geometry,
//   clipping and slivers at 1440/900/720/640/320 · WCAG reflow at the width the criterion names · accessible names · keyboard focus rings.
//   Hover measured clean on 2026-09-16: 6 pages, 128 interactive elements (13-38 each), 0 under AA —
//   unlike the panel's, where the first hover run found a dark-theme button at 1.94 (round 84).
// NOT covered: the Electron-only shell, and any state the fixtures cannot produce (the login pass is
// the only 401 path here).
//
// MEASURED BY HAND, NOT YET BY THIS TOOL (round 88): every class on screen, checked against the
// browser's own parsed selectors. 221 classes are styled; three were not —
//   ov-firstrun, ov-firstrun-line   class names in Overview.tsx that NO stylesheet ever defined
//                                   (they arrived with the first-run card and were never styled);
//                                   pruned from the markup, where they had never done anything.
//   stat-off                        the Overview's default tone, and DELIBERATELY unstyled:
//                                   .stat-card::before already paints the faint bar that "off" means.
// The pass was written into this file and then REMOVED: its emitted form split class names on the
// letter "s" when it reached the device, so its report listed "btn btn-" and "rail-clu" as unstyled.
// The browser gave the right answer when asked by hand; a check whose output I could not trust does
// not ship. Whoever picks this up should report the STYLED count beside the unstyled list, so an
// empty read can never look like a clean page.
import { readFileSync, readdirSync } from "node:fs";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { failures, unmeasurable, PROBE_SOURCE } from "./lib/contrast-probe.mjs";
import {markCoverageNotes, marksProbe, surfaceProbe, namesProbe, reflowProbe, judgeReport, reportSummary, UNSTYLED_SOURCE, focusPass, ackPass, ackNotes, pressPass, idlePass, motionPass, TARGETS_SOURCE, THEME_SOURCE, diag, pressDelta, discoverPressTargets } from "./lib/design-sweep.mjs";
import { bundleSweep, piecesModule } from "./lib/sweep-bundle.mjs";
import { join } from "node:path";

const mode = process.argv[2];
// WHICH AXES TO RUN. The panel sweep has had this since round 31 and the console had none: every run measured
// everything, which took ~95 s and did not fit in one tool call — so an axis could only be checked by paying for all
// of them. `--passes=press` is what makes a single axis measurable; the default is every axis, unchanged.
// BAKED INTO THE EMITTED SCRIPT below, because that is where the gates are: a helper defined out here does not
// exist on the device (the first run of this failed with "wants is not defined").
const PASSES = (process.argv.find((a) => a.startsWith('--passes=')) || '').slice('--passes='.length).split(',').filter(Boolean);

// HOW TO RENDER THIS CONSOLE FROM HERE WITHOUT DELIVERING A DIRECTORY (round 17 of the standing goal). The sweep
// below measures a DELIVERED build — `C:\ProgramData\Summrise\pwout\console`, several files, one transfer each. For a
// one-off look at a page there is a cheaper path, and it was used to verify the `.sig-dot` silhouettes on the real
// console (ok = circle, err = a ROTATED diamond at 2px radius; inks 6.10 and 4.65 against their own cards):
//
//   1. `gateway/public/` holds the whole build. Emit ONE file: a JSON map of path -> contents for `index.html`,
//      `style.css`, `favicon.svg`, `icons.svg` and everything in `assets/` (about 390 KB, one transfer).
//   2. On the device, route `http://summrise.test/**` by looking the pathname up in that map, with `/` and any
//      extension-less path falling back to `/index.html` (it is an SPA) and a 404 for a missing FILE.
//   3. Route `/api/**` ON THE SAME ORIGIN. The built console calls `/api/me`, not the deployed host — the sweep's
//      own handler keys on `https://ai.saisi.online`, which is right for ITS delivery and wrong for this one.
//
//   TWO TRAPS, both of which cost an attempt here:
//     * INLINING THE BUNDLE INTO THE HTML DOES NOT WORK. The bundle contains `</script>` and `<!--` sequences that
//       end the script element early however carefully the closing tags are escaped — the page renders the source
//       as text. Serve it as a FILE and let the browser fetch it as a module.
//     * KEEP `type="module"`. The entry tag is `<script type="module" crossorigin src=...>`; dropping the attribute
//       makes the ESM bundle a classic script and nothing mounts (`#root` stays empty with no error).
//
// COMPUTED AT EMIT TIME, NOT BAKED (round 223). Round 191 wrote the entry digest as a LITERAL, so
// rebuilding the UI changed the file and made the check unsatisfiable — a fix for exactly this was made
// for the panel harness in round 210 and NOT applied here, which is why the console sweep failed on the
// first rebuild after that. The value is computed in the module (where the repository is) and only its
// result is inlined into the emitted script.
// ONE ROOT, BOTH ENDS (round 77). The stamp is baked from the entry the RUN will serve — the same
// `SUMMRISE_SWEEP_ROOT` the sweep reads at run time — and until now it was read from a hard-coded
// `gateway/public`, the copy the RELEASE flow writes. So a CI run that built the console from its own
// checkout and served that while the stamp came from the last published build reported every run as
// "stale" — a true statement about two artefacts and a useless one about a commit. The default is
// unchanged, so a device run compares the delivered copy against the delivered copy it was emitted for.
// WHERE THE EMITTED SWEEP LOOKS BY DEFAULT — the device's paths, now the EMITTER's values rather than text inside a
// template literal (round 268 removed the level of escaping that made them four-backslash strings).
const DEFAULT_ROOT = "C:\\ProgramData\\Summrise\\pwout\\console";
const DEFAULT_REPORT_PATH = "C:\\ProgramData\\Summrise\\pwout\\console-sweep.json";
const HERE = fileURLToPath(new URL(".", import.meta.url));

const ENTRY_STAMP = (() => {
  const root = process.env.SUMMRISE_SWEEP_ROOT || new URL("../../gateway/public", import.meta.url).pathname;
  try {
    const b = readFileSync(new URL("index.html", new URL(root.endsWith("/") ? root : root + "/", "file://")));
    return { bytes: b.length, sha: createHash("sha256").update(b).digest("hex").slice(0, 12) };
  } catch (e) { return { bytes: -1, sha: "(unreadable)" }; }
})();
/** The console's state sheets, concatenated: its `dist` is a pruned build artifact, so the SOURCE sheets are what the
 *  mark-coverage note reads (the same choice `console-marks-check.mjs` makes, for the same reason). */
function consoleSheets() {
  const dir = new URL("../../gateway/ui/src/styles/", import.meta.url).pathname;
  return readdirSync(dir)
    .filter((f) => f.endsWith(".css"))
    .map((f) => readFileSync(dir + f, "utf8"))
    .join("\n");
}

function browserScript() {
  // THE PAYLOAD IS A REAL MODULE NOW (round 268) — the third of five. It was this file's own template literal: 484
  // lines, 74% of the file, sixteen interpolations, and a parse guard copied from the panel because a backtick in a
  // COMMENT here had already cost two `node --check` cycles. lib/sweep/console-run.cjs is ordinary code, the
  // run-varying values arrive as the generated pieces module, and the assembler resolves the payload's requires and
  // compiles what it returns — so the guard this emitter hand-copied is now the contract every emitter shares.
  const { code } = bundleSweep({
    entry: "console-run.cjs",
    modules: {
      "console-run.cjs": readFileSync(join(HERE, "lib", "sweep", "console-run.cjs"), "utf8"),
      "pieces.cjs": piecesSource(),
    },
  });
  return code;
}

/** The run-varying pieces, as a module (the same shape as the landing's and the panel's). */
function piecesSource() {
  // ONE PIECES GENERATOR, IN THE ASSEMBLER (round 272). This function used to spell out the quoting rule itself —
  // functions by `.toString()`, everything else by JSON — in four different emitters. What is left is the facts.
  return piecesModule({
    config: {
      root: DEFAULT_ROOT,
      selector: "#root",
      reportPath: DEFAULT_REPORT_PATH,
      expectedEntry: ENTRY_STAMP,
      passes: PASSES,
    },
    probe: PROBE_SOURCE,
    unstyled: UNSTYLED_SOURCE,
    targets: TARGETS_SOURCE,
    theme: THEME_SOURCE,
    checks: { SURFACE: surfaceProbe, NAMES: namesProbe, REFLOW: reflowProbe },
    marks: marksProbe,
    diag,
    passes: { focusPass, pressDelta, discoverPressTargets, pressPass, ackPass, ackNotes, idlePass, motionPass },
  });
}

function judge(file) {
  const report = JSON.parse(readFileSync(file, "utf8"));
  const findings = judgeReport(report, {
    navless: ["login"],
    implicitStates: {
      "stat-off": "the Overview's default tone: the base .stat-card::before already paints the faint bar that off means",
    },
  });
  for (const t of report.themeChecks || []) {
    const seen = t.stored || t.attr;
    if (seen && seen !== t.intended) {
      findings.push(`theme: ${t.page} was navigated as "${t.intended}" and rendered "${seen}" — the report would be describing a page it did not render`);
    }
  }
  for (const t of report.targets || []) {
    for (const u of t.distinct || []) {
      if (!u.passesBySpacing) {
        findings.push(`target size (${t.page}): ${u.sel} is ${u.w}x${u.h} with its nearest neighbour ${u.nearest}px away — 2.5.8 wants 24x24 or 24px of spacing ("${u.text}")`);
      }
    }
    // ── THE SECOND HALF OF THE SPACING CLAUSE IS MEASURED HERE AND ENFORCED ON THE PANEL (round 19) ─────────────
    // The criterion lives in THREE judges — panel, console, landing — and round 17 strengthened ONE of them:
    // `passesBySpacing` is centre-to-centre, which is wrong against a LARGE neighbour, where the circle has to clear
    // that neighbour's BOX. The probe has computed `passesByFullRule` and `insideSel` for every sweep since round 17,
    // so this sweep's report already carries the answer and only its judge never looked. Counted before it is
    // enforced, for the reason rounds 16-17 paid for twice: a criterion nobody has counted arrives as a surprise.
    const wouldFail = (t.distinct || []).filter((u) => u.passesBySpacing && u.passesByFullRule === false);
    if (wouldFail.length) {
      const worst = wouldFail.slice().sort((a, b) => (a.gapToBox ?? 99) - (b.gapToBox ?? 99))[0];
      console.log(
        `note: target size (${t.page}): ${wouldFail.length} of ${(t.distinct || []).length} undersized target(s) pass on CENTRE distance alone — ` +
          `worst ${worst.sel} is ${worst.w}x${worst.h} with its centre ${worst.gapToBox}px from ${worst.nearSel} (${worst.nearW}x${worst.nearH}). ` +
          `The PANEL enforces this half; this sweep does not yet.`,
      );
    }
  }
  for (const r of failures(report.rows).slice(0, 10)) {
    findings.unshift(`${r.cr} ${r.page}${r.width ? "@" + r.width + "px" : ""} ${r.sel} "${String(r.text).slice(0, 24)}"`);
  }
  // THE STATES THIS SHEET DECLARES AND THIS RUN NEVER PAINTED (round 50 of the standing goal). The panel has had this
  // queue since round 32 — it is how `cmd-dot` was found rendering four of its six states, one of them with no rule at
  // all — and until now it lived inside the panel's sweep, so the console's unrendered states were SILENT. Same
  // implementation, this surface's sheets: the console's styles are SOURCE files (its dist is a pruned build artifact),
  // which is the same choice `console-marks-check.mjs` makes for the same reason.
  for (const line of markCoverageNotes(consoleSheets(), report)) console.log(line);
  console.log(reportSummary("console", report));
  if (unmeasurable(report.rows).length) console.log(`note: ${unmeasurable(report.rows).length} node(s) unmeasurable`);
  if (!findings.length) {
    console.log("console design sweep OK: nothing above found a defect");
    return 0;
  }
  console.error(`\n${findings.length} finding(s):\n  ` + findings.join("\n  "));
  return 1;
}

if (mode === "--emit") {
  // THE ASSEMBLER COMPILED IT (bundleSweep's last act) and the helpers are in scope by construction, so the
  // hand-copied parse guard and the substring assertion both go.
  process.stdout.write(browserScript());
} else if (mode === "--judge") {
  const file = process.argv[3];
  if (!file) {
    console.error("usage: console-design-sweep.mjs --judge <report.json>");
    process.exit(2);
  }
  process.exit(judge(file));
} else {
  console.error(readFileSync(fileURLToPath(import.meta.url), "utf8").split("\n").slice(1, 18).join("\n"));
  console.error("\nusage: console-design-sweep.mjs --emit | --judge <report.json>");
  process.exit(2);
}
