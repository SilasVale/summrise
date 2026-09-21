// session-row-check.mjs — A DEVICE FIELD IS READ IN ONE PLACE, OR THE SPINE IS BROKEN AGAIN.
//
// WHY THIS EXISTS (round 98 of the standing goal). Rounds 96 and 97 removed the same duplication twice, in the same file:
// the mapping from a device row to a session row appeared TWICE byte-identical, and the part of it that is the DEVICE's to
// say appeared THREE times (the full row, the revive of a tombstone, and the sync of a hold that changed without an
// event). Both were the spine's defect in its most literal form — one fact written down more than once, free to drift the
// moment one copy gains a field. A field added to the wire would have reached a NEW row and neither of the refreshed
// ones.
//
// THE RULE THAT KEEPS IT FIXED is absolute and mechanical: in `useSessions.ts` — the one module that turns device rows
// into session rows — every snake_case field READ from a wire row must sit inside `wireFields`, the single definition.
// A snake_case read anywhere else in that file is a second definition, whoever wrote it and for whatever good reason.
//
// WHAT IT DOES NOT CHECK: the panel's other hooks (their fields are the wire-field gate's business), the camelCase names
// the row uses afterwards, or whether `wireFields` is used everywhere it should be — that is what the panel's own tests
// and the three call sites are for.
//
// Run: node scripts/test/session-row-check.mjs
import { readFileSync } from "node:fs";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";
const FILE = "agent/resources/panel-react/src/hooks/useSessions.ts";
const src = readFileSync(`${ROOT}/${FILE}`, "utf8");
const lines = src.split("\n");

// THE ALLOWED REGION IS THE MAPPING SECTION — everything above the hook: the small `map*` helpers, `wireFields` and
// `mapRow`. Two attempts narrowed it further and each was corrected by what it found: `wireFields` alone flagged
// `mapRow`'s own identity reads, and `wireFields` + `mapRow` flagged `mapGrants`/`mapPending`, which are the helpers the
// definition calls. That IS the shape of one definition — a section, not a line — and the rule reads: outside this
// section, no device field is read.
const start = 0;
const hook = lines.findIndex((l) => l.startsWith("export function useSessions"));
if (hook < 0) {
  console.error(`FAIL ${FILE} has no useSessions — this check cannot tell the mapping section from the hook`);
  process.exit(1);
}
if (!lines.some((l) => l.includes("const wireFields = (s: any) =>"))) {
  console.error(
    `FAIL ${FILE} has no wireFields definition — the single place a device field may be read is gone, so this check ` +
      `would be scanning for nothing. Rounds 96-97 extracted it; restore it or explain the new design in this gate.`,
  );
  process.exit(1);
}
const end = hook - 1;

const READ = /\bs\??\.[a-z][a-z0-9]*(?:_[a-z0-9]+)+\b/g;
let inside = 0;
let outside = 0;
const offenders = [];
lines.forEach((line, i) => {
  const reads = line.match(READ);
  if (!reads) return;
  if (i >= start && i <= end) {
    inside += reads.length;
    return;
  }
  outside += reads.length;
  offenders.push(`${FILE}:${i + 1} reads ${reads.join(", ")}`);
});

if (inside < 6) {
  console.error(`FAIL read only ${inside} device field(s) inside the row definitions — they moved, so this proves nothing`);
  process.exit(1);
}
if (outside) {
  console.error(
    `session-row: ${outside} device field read(s) outside the one definition:\n  ` +
      offenders.join("\n  ") +
      `\n\nEvery device field is read in \`wireFields\` ONLY (rounds 96-97 extracted it after the same duplication ` +
      `appeared twice: a byte-identical row mapper, and the device-owned half of it written three times). A read ` +
      `anywhere else is a second definition, and a field added to the wire reaches one copy and not the others.`,
  );
  process.exit(1);
}
console.log(`session-row: ${inside} device field(s) read, all inside the single pair of row definitions (wireFields + mapRow)`);
