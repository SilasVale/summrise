// console-wire-field-check.mjs — THE SAME RULE AS `wire-field-check`, ON THE OTHER FRONT END.
//
// WHY IT EXISTS (round 100 of the standing goal). Two of this session's real defects were a console field name that no
// producer sends:
//
//   * the Devices page looked a device's health up as `h.id` while the fixture wrote `prefix:`, so every lookup missed and
//     `prov-dot.missing` could not render at all;
//   * its crash row waited on `st.last_boot_kind` while the fixture wrote `verdict:`, so the row never appeared and
//     `sig-dot.err` had no surface in 136 sweeps.
//
// Both took rounds to find by hand. `wire-field-check` already applies the rule to the PANEL; the console is the front end
// where both of those happened, and it had no equivalent.
//
// THE RULE: a snake_case field the console reads from a device answer must appear in something that PRODUCES it — the
// gateway's own sources (`gateway/src/**`) or the fixtures the console's render smokes serve (`gateway/ui/*-render-smoke.mjs`
// and the console sweep's table). A field that appears nowhere the other end speaks is one of those two defects.
//
// WHAT IT DOES NOT CHECK: the panel (its own gate), the Rust device side (the gateway forwards what it receives), or
// types — only that the NAME is spoken somewhere.
//
// Run: node scripts/test/console-wire-field-check.mjs
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";

function files(dir, test) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...files(p, test));
    else if (test(name)) out.push(p);
  }
  return out;
}

/** The console's producers: what the gateway sends, and what its fixtures serve. */
const producers = [
  ...files(join(ROOT, "gateway/src"), (n) => n.endsWith(".ts")),
  ...files(join(ROOT, "gateway/ui"), (n) => n.endsWith("-render-smoke.mjs")),
  join(ROOT, "agent/scripts/console-design-sweep.mjs"),
]
  .map((f) => readFileSync(f, "utf8"))
  .join("\n");

/** The console's readers, excluding its own tests (they assert, they do not parse the wire). */
const readers = files(join(ROOT, "gateway/ui/src"), (n) => /\.tsx?$/.test(n) && !n.includes(".test."));

let reads = 0;
const missing = [];
for (const f of readers) {
  const rel = relative(ROOT, f);
  const seen = new Set();
  for (const m of readFileSync(f, "utf8").matchAll(/\b\w{1,10}\??\.([a-z][a-z0-9]*(?:_[a-z0-9]+)+)\b/g)) {
    const field = m[1];
    if (seen.has(field)) continue;
    seen.add(field);
    reads++;
    if (!new RegExp(`\\b${field}\\b`).test(producers)) {
      missing.push(`${rel}: reads "${field}", which neither the gateway nor any console fixture carries`);
    }
  }
}

// A LOWER FLOOR THAN THE PANEL'S, and the reason is a fact about the console: it maps a device answer to camelCase
// earlier (`deviceState.ts`), so far fewer snake_case reads reach its views. Seven is what the tree has; five leaves room
// for a deletion without letting a moved tree pass.
if (reads < 5) {
  console.error(`FAIL read only ${reads} wire field(s) from the console — the tree moved, so this proves nothing`);
  process.exit(1);
}
if (missing.length) {
  console.error(
    `console-wire-field: ${missing.length} field(s) the console reads and nothing produces:\n  ` +
      missing.join("\n  ") +
      `\n\nTwo of this session's defects were exactly this — a page looking a device's health up by \`h.id\` against a\n` +
      `fixture saying \`prefix:\`, and a crash row waiting on \`last_boot_kind\` against \`verdict:\`. Add the field to a\n` +
      `producer or a fixture, or take the read out.`,
  );
  process.exit(1);
}
console.log(
  `console-wire-field: ${reads} field(s) read across ${readers.length} console module(s), every one spoken by the gateway or a fixture`,
);
