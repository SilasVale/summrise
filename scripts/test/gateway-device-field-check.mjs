// gateway-device-field-check.mjs — THE LAST LINK: THE FIELDS THE GATEWAY FORWARDS MUST BE FIELDS THE DEVICE SENDS.
//
// WHY IT EXISTS (round 101 of the standing goal). The wire-field family found a real gap on its first run twice — the
// panel reading a field no fixture carried (`first_seq`), the console reading `h.id` where the fixture said `prefix:` and
// `last_boot_kind` where it said `verdict:`. Both were the SAME rule one layer in: a name that only one end knows.
//
// This is that rule at the layer where the two languages meet. `gateway/src/plugins/mcp.ts` reads a device's answer (the
// `last_boot_kind`/`last_boot` pair it forwards for the console's crash row, the `update_available`/`pinned_to` group it
// forwards for the version badge) and it does so by HAND — no generated contract reaches across the language boundary
// here. A field the DEVICE stopped sending, or spelled differently, would make the gateway forward `undefined` forever,
// and every suite on both sides would stay green because each is internally consistent.
//
// THE RULE: a snake_case field the gateway reads from a device answer must appear in the Rust sources
// (`agent/src/**/*.rs`) or in the shared fixtures (`agent/tests/fixtures/*.json`).
//
// WHAT IT DOES NOT CHECK: which endpoint carries it, its type, or the console's use of it (the console has its own gate).
// Only that the NAME exists where the device would have to spell it.
//
// Run: node scripts/test/gateway-device-field-check.mjs
import { decomment } from "./lib/decomment.mjs";
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

/** COMMENTS ARE NOT PRODUCERS (round 135). Every field gate matched the RAW text of the files it read, so a field named only
 *  in a COMMENT satisfied it — measured: a harness field plus a hook read plus `// only_a_comment_field …` in a Rust source
 *  passed `wire-field-check` with rc=0. A deleted producer can be kept alive by a comment, which is the opposite of what
 *  these gates are for.
 *
 *  The strip is deliberately conservative because `//` also opens a URL: whole-line comments and block comments always go,
 *  and a trailing `// …` goes only when it is not preceded by a colon. */
const deviceSide = [
  ...files(join(ROOT, "agent/src"), (n) => n.endsWith(".rs")),
  ...files(join(ROOT, "agent/tests/fixtures"), (n) => n.endsWith(".json")),
]
  .map((f) => decomment(readFileSync(f, "utf8")))
  .join("\n");

/** THE MODULES WHOSE ANSWERS COME FROM A DEVICE, named because the alternative was measured and is wrong (round 123).
 *
 *  The obvious widening — walk every TypeScript file under the gateway's sources — reported 35 fields, and reading them settled it: `tool_use_id`, `media_type`, `max_tokens`, `prompt_tokens`, `cache_read_input_tokens` are the vocabulary of the UPSTREAM LLM PROVIDERS
 *  (Anthropic, OpenAI, the relays), not of the agent. Three vocabularies share one field syntax, which is the same lesson
 *  `one-derivation-check`'s first run taught about four vocabularies sharing a word: a broad pattern does not become a rule
 *  by matching more.
 *
 *  So the list stays, and this is the reason it is short: these are the files that read the AGENT's own answers, and the
 *  rule — every field must be spelled by the agent's Rust or a shared fixture — is about that end of the wire only. */
const consumers = [
  "gateway/src/plugins/mcp.ts",
  "gateway/src/device-fetch.ts",
  "gateway/src/plugins/devices.ts",
];

let reads = 0;
const missing = [];
for (const rel of consumers) {
  const text = decomment(readFileSync(join(ROOT, rel), "utf8"));
  const seen = new Set();
  // ANY receiver, for the reason the panel's and the console's versions of this gate record: each of these files names
  // its answer differently (`j`, `u`, `probe`, `raw`, …), and the narrow first list matched five fields where the tree has
  // more. A snake_case property is the signal — the gateway's own code is camelCase throughout.
  for (const m of text.matchAll(/\b\w{1,8}\??\.([a-z][a-z0-9]*(?:_[a-z0-9]+)+)\b/g)) {
    const field = m[1];
    if (seen.has(field)) continue;
    seen.add(field);
    reads++;
    if (!new RegExp(`\\b${field}\\b`).test(deviceSide)) {
      missing.push(`${rel}: reads "${field}" from a device answer, which the agent's sources and fixtures never spell`);
    }
  }
}

// LOW, AND THE REASON IS THE SAME ONE THE CONSOLE'S GATE RECORDS: these files map a device answer to camelCase early
// (`DeviceProbeState`), so only a handful of snake_case reads reach them. Five is what the tree has; four leaves room for
// a deletion without letting a moved tree pass.
if (reads < 4) {
  console.error(`FAIL read only ${reads} device field(s) in the gateway — the tree moved, so this proves nothing`);
  process.exit(1);
}
if (missing.length) {
  console.error(
    `gateway-device-field: ${missing.length} forwarded field(s) the device does not spell:\n  ` +
      missing.join("\n  ") +
      `\n\nThe gateway forwards a device's answer field by field, by hand — so a name the device stopped sending, or\n` +
      `spells differently, becomes \`undefined\` on the console with every suite on both sides still green. That is the\n` +
      `shape of the two defects the same rule caught one layer in (rounds 82 and 100).`,
  );
  process.exit(1);
}
console.log(
  `gateway-device-field: ${reads} device field(s) read across ${consumers.length} gateway module(s), every one spelled by ` +
    `the agent or a fixture`,
);
