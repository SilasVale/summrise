// ── THE MUTATION THAT MUST FAIL THIS GATE (moved here from the ledger table, landing 4b) ──
// Read this when you change this file: the mutation is how you find out whether the gate can still
// fail at all. A gate that cannot be broken is worse than no gate.
//
// MUTATION: append a byte to a mirrored instrument (`agent/scripts/live-panel-probe.mjs`) without re-syncing `gateway/public/code/files/instruments/`
// RESULT:   exit 1: "gateway/public/code/files/instruments is out of date, so the Source Viewer serves instrument code that nobody runs - including the probe AGENTS.md tells a reader to point at a device. Re-sync with `bash gateway/scripts/sync-code-viewer.sh` and commit the mirror." BOTH DIRECTIONS RUN BY EXIT CODE: clean exit 0 (pass 922), mutated exit 1, restored exit 0
//
// MUTATION: add a NEW instrument to `agent/scripts/` and never re-sync the mirror (e.g. `touch agent/scripts/newprobe.mjs`)
// RESULT:   exit 1: "agent/scripts holds instruments that the Source Viewer does not publish, so a reader cannot reach code this repository tells them to run. Re-sync with `bash gateway/scripts/sync-code-viewer.sh` and commit the mirror." THE FIRST VERSION OF THIS TEST CHECKED ONLY mirrored -> source, so a new unmirrored instrument passed while the viewer was quietly incomplete; the gateway mirror's test had checked its own `missing` direction since it was written
//
// MUTATION: edit `agent/scripts/e2e/e2e.js` and forget to re-copy it to `index/public/summrise-agent/e2e.js`
// RESULT:   exit 1: "index/public/summrise-agent/e2e.js is not the suite in agent/scripts/e2e/e2e.js - and that file is what the device FETCHES to run a section against a real panel, so a stale copy measures a product that is not there." THE PAIR WAS COPIED BY HAND IN ROUNDS 103, 113 AND 114 WITH NO ASSERTION; it is the same shape as the panel build output, which IS gated (panel-sheet-freshness-check)

// instruments-mirror.test.mjs — THE SOURCE VIEWER'S SECOND MIRROR MUST MATCH ITS SOURCE.
//
// WHY THIS EXISTS (round 134). Round 128 learned that `gateway/src` has a TRACKED MIRROR under
// `gateway/public/code/files/summrise-gate/`, kept in sync by `gateway/scripts/sync-code-viewer.sh` and guarded by
// `code-viewer-mirror.test.mjs` — the gate that caught an un-synced comment edit. Round 133 asked the obvious
// follow-up and found the OTHER mirror: nine files under `gateway/public/code/files/instruments/`, **guarded by
// nothing at all**. `grep -rln "files/instruments" gateway/test/` returned no file.
//
// WHAT IS AT STAKE, IN THE SAME WORDS THE OTHER GATE USES: a stale mirror means the Source Viewer serves code that
// nobody runs — and this mirror holds the instruments AGENTS.md tells a reader to run AGAINST THE DEVICE, including
// `live-panel-probe.mjs`, whose entire hand-over story is that the device fetches it by URL.
//
// NO REDACTION LOGIC IS NEEDED HERE, and that is measured rather than assumed: the sync script's three redaction
// rules are keyed to `src/...` paths (`auth.ts`, `store/devices.ts`, `plugins/devices.ts`), and every mirrored
// instrument — including the two that carry a production host, `console-design-sweep.mjs` (1) and
// `landing-design-sweep.mjs` (3) — is byte-identical to its source in `agent/scripts/`.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const GATEWAY = join(dirname(fileURLToPath(import.meta.url)), "..");
const MIRROR = join(GATEWAY, "public/code/files/instruments");
const SOURCE = join(GATEWAY, "..", "agent/scripts");

function walk(dir, base = dir) {
  return readdirSync(dir).flatMap((name) => {
    const full = join(dir, name);
    return statSync(full).isDirectory() ? walk(full, base) : [full.slice(base.length + 1)];
  });
}

test("code viewer: the instruments mirror matches agent/scripts byte for byte", () => {
  const mirrored = walk(MIRROR);
  assert.ok(mirrored.length > 0, `no files under ${MIRROR} — the mirror was DELETED rather than moved`);

  const missingSource = mirrored.filter((rel) => {
    try { statSync(join(SOURCE, rel)); return false; } catch { return true; }
  });
  assert.deepEqual(
    missingSource, [],
    `the instruments mirror holds files with no source under agent/scripts — a viewer serving code this worker does ` +
    `not run. Re-sync with \`bash gateway/scripts/sync-code-viewer.sh\` and commit the mirror.`,
  );

  // AND THE OTHER DIRECTION, WHICH THIS TEST WAS MISSING FOR ONE ROUND (round 137). `sync-code-viewer.sh` copies
  // `agent/scripts/*.mjs` plus `lib/*.mjs` by GLOB, so a NEW instrument that is never re-synced would be absent from the
  // Source Viewer while every file that IS there still matched -- this test passed and the viewer was quietly incomplete.
  // The gateway mirror's test has checked its own `missing` direction since it was written; this one did not, and the
  // asymmetry is the same one round 133 was about: a guard that looks only at what exists cannot see what is gone.
  const expected = ["*.mjs", "lib/*.mjs"].flatMap((pattern) => {
    const dir = join(SOURCE, dirname(pattern));
    const suffix = pattern.slice(pattern.lastIndexOf("*") + 1);
    return readdirSync(dir)
      .filter((n) => n.endsWith(suffix))
      .map((n) => (dirname(pattern) === "." ? n : join(dirname(pattern), n)));
  });
  const notMirrored = expected.filter((rel) => !mirrored.includes(rel));
  assert.deepEqual(
    notMirrored, [],
    `agent/scripts holds instruments that the Source Viewer does not publish, so a reader cannot reach code this ` +
    `repository tells them to run. Re-sync with \`bash gateway/scripts/sync-code-viewer.sh\` and commit the mirror.`,
  );

  // AND THE PUBLISHED COPY OF THE E2E SUITE, WHICH IS THE SAME SHAPE AND HAD NO GUARD EITHER (round 138).
  // `index/public/summrise-agent/e2e.js` is the copy the DEVICE fetches — AGENTS.md's cadence hands a section to a real
  // device through it — and it was copied BY HAND in rounds 103, 113 and 114. Three manual copies, no assertion. The
  // panel's build output has exactly this shape and IS gated (`panel-sheet-freshness-check.mjs`); this pair was not.
  const e2ePublished = join(GATEWAY, "..", "index/public/summrise-agent/e2e.js");
  const e2eSource = join(SOURCE, "e2e/e2e.js");
  assert.strictEqual(
    readFileSync(e2ePublished, "utf8"),
    readFileSync(e2eSource, "utf8"),
    `index/public/summrise-agent/e2e.js is not the suite in agent/scripts/e2e/e2e.js — and that file is what the ` +
    `device FETCHES to run a section against a real panel, so a stale copy measures a product that is not there. ` +
    `Re-copy it (\`cp agent/scripts/e2e/e2e.js index/public/summrise-agent/e2e.js\`) and commit, the way rounds ` +
    `103, 113 and 114 each did by hand.`,
  );

  const differing = mirrored.filter(
    (rel) => readFileSync(join(MIRROR, rel), "utf8") !== readFileSync(join(SOURCE, rel), "utf8"),
  );
  assert.deepEqual(
    differing, [],
    `gateway/public/code/files/instruments is out of date, so the Source Viewer serves instrument code that nobody ` +
    `runs — including the probe AGENTS.md tells a reader to point at a device. Re-sync with ` +
    `\`bash gateway/scripts/sync-code-viewer.sh\` and commit the mirror.`,
  );
});
