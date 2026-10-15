// oracle-translate.mjs — THE PRODUCER `fixtures/translate-corpus.json` NEVER HAD IN THE TREE.
//
// **THE CORPUS'S RUST TESTS NAME THIS FILE**: "produced by the SHIPPING TypeScript
// (`gateway/wasm/oracle-translate.mjs`)" — and it did not exist. **617 cases, the largest differential in this
// repository, could not be re-recorded**: every one was a snapshot whose producer was gone, which is this
// repository's own phrase ("a corpus is a snapshot, not a comparison") arriving at the one instrument with no
// replacement. **A corpus nobody can re-record cannot tell you that the shipping side MOVED.**
//
// **SO THIS REPLAYS RATHER THAN REGENERATES.** For the functions whose TypeScript home still exists, it feeds each
// RECORDED input back through the LIVE code and compares against the RECORDED answer:
//
//   node oracle-translate.mjs --check    # exit 1 if any live answer has moved (the gate)
//   node oracle-translate.mjs            # re-record those answers (a no-op when nothing moved)
//
// **AND IT ADDS CASES WITHOUT EVER DROPPING ONE.** The functions whose shipping implementation was deleted at the
// `/v1` cutover are not touched at all: for those the snapshot IS the record, and "re-recording" them would mean
// re-implementing the very thing they pin.
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import {
  clientSessionId,
  fnvHex,
  opencodeSessionHeader,
  passthroughHeaders,
  syntheticSessionId,
} from "../src/routing-logic.ts";

const FIXTURE = fileURLToPath(new URL("fixtures/translate-corpus.json", import.meta.url));
const headers = (pairs) => new Headers(pairs ?? {});

/** The five functions this oracle owns, each with the adapter its recorded `input` shape needs. */
const LIVE = {
  fnv_hex: (input) => fnvHex(input),
  passthrough_headers: (input) =>
    Object.fromEntries(
      passthroughHeaders(input.bearerKey ?? null, {
        apiKeyHeader: input.apiKeyHeader ?? false,
        extra: input.extra ?? {},
      }).entries(),
    ),
  client_session_id: (input) => clientSessionId(headers(input)),
  synthetic_session_id: (input) => syntheticSessionId(input),
  opencode_session_header: (input) => opencodeSessionHeader(headers(input.headers), input.uid),
};

/** Cases this oracle ADDS, in the recorded shape. Appended only when the name is not already there. */
const ADDITIONS = [
  ["fnv_hex", "an astral character (a surrogate pair is two units)", "\u{1f600}", fnvHex("\u{1f600}")],
  ["passthrough_headers", "an EMPTY key sends no auth header", { bearerKey: "", apiKeyHeader: null, extra: {} },
    LIVE.passthrough_headers({ bearerKey: "", apiKeyHeader: null, extra: {} })],
  ["passthrough_headers", "an EMPTY extra value is skipped", { bearerKey: "sk-1", apiKeyHeader: null, extra: { "x-empty": "", "x-set": "v" } },
    LIVE.passthrough_headers({ bearerKey: "sk-1", apiKeyHeader: null, extra: { "x-empty": "", "x-set": "v" } })],
  ["opencode_session_header", "a uid whose ascii is not its utf-16", { headers: {}, uid: "用户一" },
    LIVE.opencode_session_header({ headers: {}, uid: "用户一" })],
];

const doc = JSON.parse(readFileSync(FIXTURE, "utf8"));
const owned = new Set(Object.keys(LIVE));

let checked = 0;
const moved = [];
for (const c of doc.cases) {
  if (!owned.has(c.fn)) continue;
  const got = LIVE[c.fn](c.input);
  checked += 1;
  if (JSON.stringify(got) !== JSON.stringify(c.expected.value)) {
    moved.push(`  ${c.fn} / ${c.name}\n    recorded: ${JSON.stringify(c.expected.value)}\n    live    : ${JSON.stringify(got)}`);
  }
}

console.error(`oracle-translate: replayed ${checked} recorded case(s) over ${owned.size} function(s)`);
if (moved.length) {
  console.error(`oracle-translate: ${moved.length} answer(s) have MOVED:\n${moved.join("\n")}`);
} else {
  console.error("oracle-translate: every one still answers what the fixture records");
}

const have = new Set(doc.cases.map((c) => `${c.fn} :: ${c.name}`));
const fresh = ADDITIONS.filter(([fn, name]) => !have.has(`${fn} :: ${name}`)).map(([fn, name, input, value]) => ({
  fn,
  name,
  input,
  expected: { value },
}));
console.error(`oracle-translate: ${fresh.length} case(s) would be ADDED`);

if (process.argv.includes("--check")) {
  process.exit(moved.length ? 1 : 0);
}
if (fresh.length) {
  doc.cases = [...doc.cases, ...fresh];
  writeFileSync(FIXTURE, JSON.stringify(doc, null, 2) + "\n");
  console.error(`oracle-translate: wrote ${doc.cases.length} cases`);
}
