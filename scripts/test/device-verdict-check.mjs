// device-verdict-check.mjs — THE UPDATE VERDICT CROSSES THREE FILES, AND ALL THREE MUST STILL BE IN THE CHAIN.
//
// WHY THIS EXISTS (round 79 of the standing goal). Round 29 fixed a defect this objective exists to remove: the console
// decided for itself whether a device was behind, by comparing the version somebody last reported against the version
// the CDN advertises — a second computation of a fact the DEVICE already answers, and it got the answer BACKWARDS for two
// devices at once (the one that was behind, and the one deliberately pinned). The fix routes the device's own verdict
// through the wire: `/api/update` on the device → the worker's probe → `/api/plugins/status` → the client's type → the
// devices view, which prefers it and keeps the old comparison only as the fallback for agents too old to answer.
//
// THAT CHAIN IS FOUR LINKS IN THREE FILES, and a single deleted field anywhere in it silently returns the console to
// computing the fact itself — with every suite still green, because nothing else asserts that the console READS it. This
// gate is that assertion, and its failure message is the regression's name.
//
// WHAT IT CHECKS: the worker still forwards the verdict, the client's type still declares it, and the view still reads
// it. NOT the fallback's shape (that is the view's business and its own tests) and not the device side (the Rust suite
// and the wire fixtures own that end).
//
// Run: node scripts/test/device-verdict-check.mjs
import { readFileSync } from "node:fs";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";
const read = (p) => readFileSync(`${ROOT}/${p}`, "utf8");

const LINKS = [
  {
    file: "gateway/src/plugins/mcp.ts",
    what: "the worker FORWARDS the device's verdict into the console's payload",
    // The worker assembles the object field by field (spread-guarded, so an absent value is ABSENT rather than empty);
    // the first version of this pattern looked for a shape the code never had and reported a link that is intact.
    test: (t) => /state\.update\s*=\s*\{[\s\S]{0,300}update_available:/.test(t),
  },
  {
    file: "gateway/ui/src/api/client.ts",
    what: "the console's client TYPE declares the verdict",
    test: (t) => /update\?:\s*\{[\s\S]{0,200}update_available/.test(t),
  },
  {
    file: "gateway/ui/src/views/DevicesPanel.tsx",
    what: "the devices view READS the verdict",
    // THE STRUCTURE, NOT THE WORDS. The first version asked whether `update_available` and `verdict` appear anywhere in
    // the file — and a mutation that made the view ignore the device's verdict (`const verdict = undefined`) still
    // passed, because both words survive in the fallback and the variable. This asks for the READ: the verdict assigned
    // FROM the probed status, which is the link the mutation broke.
    test: (t) => /\bverdict\s*=\s*st\?\.\s*update\b/.test(t) && /verdict\s*\?\s*verdict\.update_available/.test(t),
  },
];

let fail = 0;
for (const l of LINKS) {
  let text;
  try {
    text = read(l.file);
  } catch (e) {
    console.error(`FAIL ${l.file} is missing (${e.message}) — the verdict's chain is broken at this link`);
    fail++;
    continue;
  }
  if (l.test(text)) {
    console.log(`ok   ${l.file}: ${l.what}`);
  } else {
    console.error(
      `FAIL ${l.file} no longer ${l.what}. The console would be computing the update verdict ITSELF again — the defect ` +
        `round 29 fixed, where the comparison got the answer backwards for the device that was behind AND the one that ` +
        `was deliberately pinned. Restore the link, or change the design deliberately and update this gate's reason.`,
    );
    fail++;
  }
}

if (fail) {
  console.error(`\ndevice-verdict: ${fail} broken link(s) in the verdict's chain`);
  process.exit(1);
}
console.log("\ndevice-verdict: the device's own update verdict still crosses all three files");
