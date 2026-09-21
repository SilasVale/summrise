// wire-field-check.mjs — EVERY FIELD THE PANEL READS MUST EXIST SOMEWHERE THE DEVICE OR ITS FIXTURES SPEAK.
//
// WHY THIS EXISTS (round 82 of the standing goal). The class of defect this session kept paying for is a field name that
// only one end knows: `prov-dot` rendered nothing because the page looked its health up by `h.id` while the fixture wrote
// `prefix:`; the console's crash row never appeared because the fixture said `verdict:` where the page reads
// `last_boot_kind`; and the panel's `boot-mark info` took seven rounds, one of whose suspects was exactly this. Every
// time, the page was reading a field, the fixture was carrying a different one, and nothing compared the two.
//
// WHAT IT CHECKS, narrowly and by name: the snake_case field reads in the panel's WIRE-PARSING hooks must appear in at
// least one place the other end actually speaks — the device harness's stubs, or the shared fixtures under
// `agent/tests/fixtures/`. A field that appears in neither is either a typo, a field the device no longer sends, or a
// fixture that has fallen behind; all three are the defect above.
//
// WHAT IT DOES NOT CHECK: types, values, which endpoint a field arrives on (that is the harness's own business, and
// round 73 records what it cost), or the console's parsers, which have their own fixtures.
//
// Run: node scripts/test/wire-field-check.mjs
import { decomment } from "./lib/decomment.mjs";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";

/** The hooks that parse a device answer. Named, so a new one is added deliberately rather than swept in. */
const PARSERS = [
  "agent/resources/panel-react/src/hooks/useAgentVitals.ts",
  "agent/resources/panel-react/src/hooks/useCommandEvents.ts",
  "agent/resources/panel-react/src/hooks/usePlugins.ts",
  "agent/resources/panel-react/src/hooks/useSessions.ts",
  "agent/resources/panel-react/src/hooks/useMonitors.ts",
];

/** Field reads that are NOT device fields, declared with the reason (the pattern every check in this repo uses). */
const NOT_DEVICE_FIELDS = new Set([
  // locally-built or computed values whose names look like wire fields
  "session_id", "ev",
]);

/** COMMENTS ARE NOT PRODUCERS (round 135). Every field gate matched the RAW text of the files it read, so a field named only
 *  in a COMMENT satisfied it — measured: a harness field plus a hook read plus `// only_a_comment_field …` in a Rust source
 *  passed `wire-field-check` with rc=0. A deleted producer can be kept alive by a comment, which is the opposite of what
 *  these gates are for.
 *
 *  The strip is deliberately conservative because `//` also opens a URL: whole-line comments and block comments always go,
 *  and a trailing `// …` goes only when it is not preceded by a colon. */
const harness = readFileSync(join(ROOT, "agent/scripts/panel-render-audit.mjs"), "utf8");
const fixtures = readdirSync(join(ROOT, "agent/tests/fixtures"))
  .filter((f) => f.endsWith(".json"))
  .map((f) => readFileSync(join(ROOT, "agent/tests/fixtures", f), "utf8"))
  .join("\n");
const otherEnd = harness + "\n" + fixtures;

/** THE PRODUCERS, which are what actually matters (round 108). `otherEnd` above is the TEST side — a stub and fixtures —
 *  and a field that exists ONLY there is a field the device may not send at all, which is precisely the class that cost
 *  this session `prov-dot` (a page looking health up by `h.id` while the fixture said `prefix:`) and the console's crash
 *  row (`last_boot_kind` against `verdict:`). A wire field has to be spelled by something that SENDS it: the agent's Rust
 *  or the gateway's TypeScript. */
function producerText() {
  const out = [];
  const walk = (dir, test) => {
    for (const name of readdirSync(dir)) {
      const p = join(dir, name);
      if (statSync(p).isDirectory()) walk(p, test);
      else if (test(name)) out.push(decomment(readFileSync(p, "utf8")));
    }
  };
  walk(join(ROOT, "agent/src"), (n) => n.endsWith(".rs"));
  walk(join(ROOT, "gateway/src"), (n) => n.endsWith(".ts"));
  return out.join("\n");
}
const producers = producerText();

let reads = 0;
const missing = [];
for (const rel of PARSERS) {
  const text = decomment(readFileSync(join(ROOT, rel), "utf8"));
  const seen = new Set();
  // ANY receiver, because the hooks name their answers differently (`j`, `d`, `st`, `row`, `next`, …) — the narrow
  // first version matched ten fields and its own floor said so. A snake_case property is the signal: the panel is
  // camelCase throughout, so an underscore name is a WIRE field wherever it is read from.
  for (const m of text.matchAll(/\b\w{1,8}\??\.([a-z][a-z0-9]*(?:_[a-z0-9]+)+)\b/g)) {
    const field = m[1];
    if (seen.has(field) || NOT_DEVICE_FIELDS.has(field)) continue;
    seen.add(field);
    reads++;
    if (!new RegExp(`\\b${field}\\b`).test(otherEnd)) {
      missing.push(`${rel}: reads "${field}", which neither the device harness nor any fixture carries`);
    } else if (!new RegExp(`\\b${field}\\b`).test(producers)) {
      // IN A FIXTURE BUT IN NO PRODUCER: the page can render it in the harness and nowhere else.
      missing.push(`${rel}: reads "${field}", which only a fixture carries — no Rust or gateway source sends it`);
    }
  }
}

if (reads < 20) {
  console.error(`FAIL read only ${reads} wire field(s) — the parsers moved, so this proves nothing`);
  process.exit(1);
}
if (missing.length) {
  console.error(
    `wire-field: ${missing.length} field(s) the panel reads and no fixture speaks:\n  ` +
      missing.join("\n  ") +
      `\n\nThree of these cost rounds in this session: a page reading \`h.id\` against a fixture saying \`prefix:\`, a\n` +
      `crash row waiting on \`last_boot_kind\` against a fixture saying \`verdict:\`, and \`prov-dot\` rendering nothing at\n` +
      `all for three rounds. Add the field to the harness stub or a fixture, or declare it in NOT_DEVICE_FIELDS with a\n` +
      `reason.`,
  );
  process.exit(1);
}
console.log(
  `wire-field: ${reads} field(s) read by ${PARSERS.length} panel parser(s) — every one spoken by the harness or a fixture ` +
    `AND by a producer (the agent's Rust or the gateway), so none of them renders only in a stub`,
);
