// device-facts.test.mjs — A VIEW RENDERS THE DEVICE FACT; IT DOES NOT READ THE WIRE FIELD.
//
// WHY (round 38 of the standing goal). The panel has had this ratchet since round 34 (`sessionFacts.test.ts`), after
// two audit rounds found the same class of defect one fact at a time. The console had the same shape and no guard:
//
//   * round 35 — both device surfaces derived "is the agent answering" as `!!status?.agent_up`, which collapses
//     three states into two, so a device nobody had checked was painted RED with the word "offline";
//   * round 38 — and both ALSO counted it, each with its own `filter(...).length`, so "N devices online" was
//     computed twice from one fact and the count had the same blind spot.
//
// `agent_up` is now owned by `src/lib/deviceState.ts`: `agentSignal` for a row, `deviceIsUp` for a boolean, and
// `deviceTally` for the counts. A view that reads the field directly is starting a second derivation, and a second
// derivation is how two surfaces come to disagree about one device.
//
// `tunnel_up` and `last_boot_kind` are NOT in the list, deliberately: one gates an ACTION (can the panel be opened
// over a dead tunnel) and the other decides whether a row exists at all — both render decisions from a value, not
// claims about the device's state. Only `agent_up` has a model function that every surface is supposed to share.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SRC = path.join(HERE, "..", "src");

test("no view reads `agent_up` — the fact lives in lib/deviceState", () => {
  const offenders = [];
  let scanned = 0;

  for (const dir of ["views", "components"]) {
    for (const file of readdirSync(path.join(SRC, dir)).filter((f) => f.endsWith(".tsx"))) {
      scanned++;
      // COMMENTS ARE STRIPPED FIRST — the fourth time this session (rounds 20, 23, 34, here): a comment that
      // explains why a field is NOT read is not a read, and a check that cannot tell them apart deletes its reasons.
      const src = readFileSync(path.join(SRC, dir, file), "utf8")
        .replace(/\/\*[\s\S]*?\*\//g, "")
        .replace(/^\s*\/\/.*$/gm, "");
      const m = /\bagent_up\b/.exec(src);
      if (m) {
        const line = src.slice(0, m.index).split("\n").length;
        offenders.push(`${dir}/${file}:${line} reads \`agent_up\` — use agentSignal() / deviceIsUp() / deviceTally()`);
      }
    }
  }

  assert.deepEqual(
    offenders,
    [],
    `${offenders.length} view(s) read the wire field directly. A second derivation is how two surfaces come to\n` +
      `disagree about one device:\n  ${offenders.join("\n  ")}`,
  );

  // A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN.
  assert.ok(scanned > 5, `only ${scanned} view(s) scanned — this test is reading the wrong directory`);
});
