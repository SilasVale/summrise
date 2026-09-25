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
