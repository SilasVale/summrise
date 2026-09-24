// CARRY AND DETECT ARE TWO LISTS, AND NOTHING HELD THEM TOGETHER.
//
// `useSessions.ts` writes that sentence about itself:
//
//   AND THE LIST IS WHERE THE BUG WAS. It omitted `idleMs` and `commandRunning` while `wireFields` did
//   too, so a refresh neither carried them nor noticed them — a row kept its discovery values for life
//   and three consumers read them as live (`sessionActive`, the rail's `anyCommandRunning`,
//   `idleSessions`' offer-to-close). CARRY AND DETECT ARE TWO LISTS; fixing the bug needed both.
//
// The list must stay explicit: `pendingApproval` is DERIVED at map time, so comparing the mapped objects
// would report a change on every poll (the comment records that attempt and why it broke). So the two lists
// cannot be merged — but they CAN be compared, and until now nothing did.
//
// This gate reads both and fails when DETECT stops covering CARRY.
//
// Run: node scripts/test/session-carry-detect-check.mjs
import { decomment } from "./lib/decomment.mjs";
import { readFileSync } from "node:fs";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";
const FILE = "agent/resources/panel-react/src/hooks/useSessions.ts";
const src = decomment(readFileSync(`${ROOT}/${FILE}`, "utf8"));

/** The keys an object-returning arrow declares, ignoring spreads (those come from another list). */
function keysOf(block) {
  const out = new Set();
  for (const line of block.split("\n")) {
    const m = line.match(/^\s{2}([A-Za-z_][A-Za-z0-9_]*)\s*:/);
    if (m) out.add(m[1]);
  }
  return out;
}

function blockAfter(marker) {
  const i = src.indexOf(marker);
  if (i < 0) return null;
  const open = src.indexOf("{", i);
  let depth = 0;
  for (let j = open; j < src.length; j++) {
    if (src[j] === "{") depth++;
    else if (src[j] === "}") {
      depth--;
      if (depth === 0) return src.slice(open + 1, j);
    }
  }
  return null;
}

const liveBlock = blockAfter("const liveFields = (s: any) => ({");
const wireBlock = blockAfter("const wireFields = (s: any) => ({");
// `wireFieldsChanged` is an EXPRESSION-bodied arrow — `(a, b): boolean => expr;` — so there is no brace to
// walk. (The first version of this file used the brace walker for all three and reported every field as
// missing, because it had walked into an unrelated block further down.)
function expressionAfter(marker) {
  const i = src.indexOf(marker);
  if (i < 0) return null;
  const arrow = src.indexOf("=>", i);
  const end = src.indexOf(";", arrow);
  if (arrow < 0 || end < 0) return null;
  return src.slice(arrow + 2, end);
}
const changedBlock = expressionAfter("const wireFieldsChanged = (");
if (!liveBlock || !wireBlock || !changedBlock) {
  console.error(`FAIL ${FILE}: could not read one of the three lists — this gate cannot measure.`);
  process.exit(2);
}

// CARRY = what liveFields produces plus what wireFields adds. A spread is the link between them.
const live = keysOf(liveBlock);
const wire = keysOf(wireBlock);
if (!/\.\.\.liveFields\(s\)/.test(wireBlock)) {
  console.error(
    `FAIL ${FILE}: \`wireFields\` no longer spreads \`liveFields\` — the CARRY side is now two lists of ` +
      `its own, and this comparison cannot see half of it.`,
  );
  process.exit(1);
}
const carry = new Set([...live, ...wire]);

// DETECT = the fields `wireFieldsChanged` compares. It reads them off the mapped objects, so the left
// operand names the field; `?.id` narrows to an identity without changing which field it is.
const detect = new Set();
for (const m of changedBlock.matchAll(/existing\.([A-Za-z_][A-Za-z0-9_]*)/g)) detect.add(m[1]);

const missing = [...carry].filter((k) => !detect.has(k)).sort();
const extra = [...detect].filter((k) => !carry.has(k)).sort();

if (missing.length || extra.length) {
  console.error(`FAIL ${FILE}: CARRY and DETECT disagree about the session row.`);
  for (const k of missing) {
    console.error(
      `  ${k}: wireFields carries it, wireFieldsChanged never compares it — a refresh would neither ` +
        `carry nor notice a change, which is the bug the comment above that list records.`,
    );
  }
  for (const k of extra) {
    console.error(`  ${k}: wireFieldsChanged compares it, but no list carries it.`);
  }
  console.error(
    `\nAdd it to \`wireFieldsChanged\` (and keep \`pendingApproval\` narrow — its deadline is derived).`,
  );
  process.exit(1);
}

console.log(
  `session-carry-detect: ${carry.size} carried field(s), all ${detect.size} compared by wireFieldsChanged ` +
    `(${live.size} from liveFields + ${wire.size} from wireFields).`,
);
