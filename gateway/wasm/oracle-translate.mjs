// oracle-translate.mjs — THE PRODUCER `fixtures/translate-corpus.json` NEVER HAD IN THE TREE.
//
// **THE CORPUS'S RUST TESTS NAME THIS FILE**: "produced by the SHIPPING TypeScript
// (`gateway/wasm/oracle-translate.mjs`)" — and it did not exist. **617 cases, the largest differential in this
// repository, could not be re-recorded**: every one was a snapshot whose producer was gone, which is this
// repository's own phrase ("a corpus is a snapshot, not a comparison") arriving at the one instrument with no
// replacement. **A corpus nobody can re-record cannot tell you that the shipping side MOVED.**
//
// **SO THIS REPLAYS RATHER THAN REGENERATES.** For the functions whose TypeScript home still exists — **33 of the
// corpus's 65 today, `node oracle-translate.mjs --check` prints the count it replayed and the count it owns** — it
// feeds each RECORDED input back through the LIVE code and compares against the RECORDED answer:
//
//   node oracle-translate.mjs --check    # exit 1 if any live answer has moved (the gate)
//   node oracle-translate.mjs            # re-record those answers (a no-op when nothing moved)
//
// **FOUR THINGS ABOUT THE RECORDING THAT A READER WOULD GET WRONG, EACH ONE FOUND BY RUNNING IT** (they are why
// the replay exists rather than a harness somebody wrote once):
//
//   1. **The recording is a PROJECTION.** `model_spec` records `{id, ownedBy}` — the fields the WORKER carries —
//      while the console's live spec has since grown `wire`, `search`, `probe`, `reasoningMax`, `usEgress`. The
//      live answer is shrunk onto the recorded keys before comparing, so a field the recording does not have
//      cannot fail the gate, and one it does have must still hold.
//   2. **`undefined` is `{"null": true}`** — the corpus's own sentinel for "no such model / no such wire", which
//      its Rust tests decode. JSON has no `undefined`; the recording named it.
//   3. **A `Set` is not a list in JSON**: `allowed_origins` returns a `ReadonlySet`, which serializes to `{}`.
//   4. **The Rust struct's field is not the TypeScript's**: the worker's `RouteInfo` carries `is_translate`, the
//      console's carries a `type` STRING. Reading the wrong one made all 45 routing cases look like passthrough.
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
  stripBracket,
  syntheticSessionId,
  wireModelName,
} from "../src/routing-logic.ts";
import {
  modelSpec,
  reasoningMaxParsedFor,
  reasoningMaxRawFor,
  wireSpec,
} from "../src/channels-logic.ts";
import {
  clearSessionCookieHeader,
  csrfCookieViolation,
  parseCookie,
  safeEq,
  sessionCookieHeader,
} from "../src/auth-logic.ts";
import { issueSessionSecret, sessionSecret } from "../src/session-logic.ts";
import {
  isChannelDownFailure,
  ogTimeoutMs,
  passthroughTimeoutMs,
  retryPolicyFor,
  upstreamTimeoutMs,
} from "../src/reliability-logic.ts";
import {
  allowedOrigins,
  corsHeadersFor,
  isAllowedOrigin,
  isLoopbackHost,
  isLoopbackOrigin,
} from "../src/cors-logic.ts";
import { deviceHostError } from "../src/device-dial-logic.ts";
import { hostAllowError } from "../src/device-fetch.ts";
// `pickRoute` and `resolveRoute` are the CONSOLE's own routing, in `upstream.ts` — the file the worker's
// `routing.rs` was ported from and which remains the console's live implementation.
import { pickRoute } from "../src/upstream.ts";
import { encodeBase64Utf8, probeEnvKeyName } from "../src/tooling-logic.ts";

const FIXTURE = fileURLToPath(new URL("fixtures/translate-corpus.json", import.meta.url));
const headers = (pairs) => new Headers(pairs ?? {});

/**
 * **EVERY FUNCTION THIS ORACLE OWNS, WITH THE ADAPTER ITS RECORDED `input` SHAPE NEEDS.**
 *
 * The adapter is not documentation — **THE REPLAY VERIFIES IT**: a wrong argument order, a field read the wrong way
 * or a default guessed instead of taken from the case turns the comparison red on the recorded answer. That is why
 * this table can be extended function by function without a separate proof for each row.
 */
const LIVE = {
  // ── the header/session family (`routing-logic.ts`, an emission of `session.rs`) ────────────────
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
  strip_bracket: (input) => stripBracket(input),
  wire_model_name: (input) => wireModelName(input.prefix, input.stripped),
  // The routing decision's own convention, copied from the RUST test that reads the same 45 cases.
  pick_route: (input) => {
    const env = {};
    if (input.usProxy != null) env.US_PROXY = input.usProxy;
    if (input.base != null) env.US_PROXY_BASE = input.base;
    const got = pickRoute(input.prefix ?? "", env, input.usProxy ?? null, input.requestPath ?? "");
    // **THE RUST STRUCT'S FIELD IS NOT THE TYPESCRIPT'S**: the worker's `RouteInfo` carries `is_translate`, while
    // the console's carries a `type` STRING. Reading the wrong one made every case look like a passthrough — the
    // replay said so on the first run, which is the whole reason it replays instead of trusting an adapter.
    return { type: got.type, kind: got.kind, stripPrefix: got.stripPrefix, upstream: got.upstream };
  },

  // ── the channel registry (`channels-logic.ts`, an emission of `registry.rs`) ───────────────────
  model_spec: (input) => modelSpec(input),
  wire_spec: (input) => wireSpec(input),
  reasoning_max_raw_for: (input) => reasoningMaxRawFor(input),
  reasoning_max_parsed_for: (input) => reasoningMaxParsedFor(input),

  // ── auth/session (`auth-logic.ts`) ─────────────────────────────────────────────────────────────
  safe_eq: (input) => safeEq(input.a, input.b),
  parse_cookie: (input) => parseCookie(input),
  session_cookie_header: (input) => sessionCookieHeader(input.token, input.maxAgeSec, input.secure),
  clear_session_cookie_header: (input) => clearSessionCookieHeader(input.secure),
  issue_session_secret: (input) => issueSessionSecret({ SESSION_SECRET: input.secret }),
  session_secret: (input) => sessionSecret({ SESSION_SECRET: input.secret }, input.password),
  // The live function takes a REQUEST (`request.method`, `request.headers.get`) — the recording's shape gives the
  // three things it reads, so the adapter builds the smallest request that has exactly those.
  csrf_cookie_violation: (input) =>
    csrfCookieViolation(
      new Request("https://oracle.test/", {
        method: input.method,
        headers: {
          ...(input.cookie == null ? {} : { cookie: input.cookie }),
          ...(input.secFetchSite == null ? {} : { "sec-fetch-site": input.secFetchSite }),
        },
      }),
    ),

  // ── reliability (`reliability-logic.ts`) ───────────────────────────────────────────────────────
  upstream_timeout_ms: (input) => upstreamTimeoutMs(input.env),
  og_timeout_ms: (input) => ogTimeoutMs(input.env),
  passthrough_timeout_ms: (input) => passthroughTimeoutMs(input.env, input.kind),
  retry_policy_for: (input) => retryPolicyFor(input.kind, input.upstreamModel, input.timeoutMs),

  // ── origins and device hosts ───────────────────────────────────────────────────────────────────
  // **A `Set` IS NOT A LIST IN JSON** — it serializes to `{}`, which is what the first run compared against seven
  // recorded arrays. The live function returns a ReadonlySet; the recording is an array.
  allowed_origins: (input) => [...allowedOrigins(input.configured)],
  is_allowed_origin: (input) => isAllowedOrigin(input.origin, input.requestHost, input.configured),
  cors_headers_for: (input) => corsHeadersFor(input.origin, input.requestHost, input.configured),
  is_loopback_origin: (input) => isLoopbackOrigin(input.origin),
  is_loopback_host: (input) => isLoopbackHost(input.host),
  device_host_error: (input) => deviceHostError(input.hostname),
  host_allow_error: (input) => hostAllowError(input.hostname, { DEVICE_HOST_SUFFIX: input.suffix }),
  is_channel_down_failure: (input) => isChannelDownFailure(input.detail),

  // ── tooling (`tooling-logic.ts`) ───────────────────────────────────────────────────────────────
  encode_base64_utf8: (input) => encodeBase64Utf8(input),
  probe_env_key_name: (input) => probeEnvKeyName(input),
};

/** Cases this oracle ADDS, in the recorded shape. Appended only when the name is not already there. */
const ADDITIONS = [
  [
    "fnv_hex",
    "an astral character (a surrogate pair is two units)",
    "\u{1f600}",
    fnvHex("\u{1f600}"),
  ],
  [
    "passthrough_headers",
    "an EMPTY key sends no auth header",
    { bearerKey: "", apiKeyHeader: null, extra: {} },
    LIVE.passthrough_headers({ bearerKey: "", apiKeyHeader: null, extra: {} }),
  ],
  [
    "passthrough_headers",
    "an EMPTY extra value is skipped",
    { bearerKey: "sk-1", apiKeyHeader: null, extra: { "x-empty": "", "x-set": "v" } },
    LIVE.passthrough_headers({
      bearerKey: "sk-1",
      apiKeyHeader: null,
      extra: { "x-empty": "", "x-set": "v" },
    }),
  ],
  [
    "opencode_session_header",
    "a uid whose ascii is not its utf-16",
    { headers: {}, uid: "用户一" },
    LIVE.opencode_session_header({ headers: {}, uid: "用户一" }),
  ],
];

const doc = JSON.parse(readFileSync(FIXTURE, "utf8"));
const owned = new Set(Object.keys(LIVE));

/**
 * **THE RECORDING IS A PROJECTION, AND THE ORACLE MUST COMPARE PROJECTIONS.**
 *
 * `model_spec` proved it: the recorded value is `{id, ownedBy}` — the two fields the WORKER carries — while the
 * console's live spec has grown `wire`, `search`, `probe`, `reasoningMax`, `usEgress` and more since. Comparing the
 * whole live object against the recording reported eleven "MOVED" answers that had not moved at all. **What the
 * corpus records is the contract the Rust side must reproduce; extra fields on the console's side are the
 * console's own business.**
 *
 * So the live answer is SHRUNK onto the recorded shape — recursively, key by key — and only then compared. A key
 * the recording does not have cannot fail the gate, and a key it does have must still hold the recorded value.
 */
/**
 * **`undefined` IS RECORDED AS `{"null": true}`** — the corpus's own sentinel, which its Rust tests decode (a
 * missing model, an unknown wire and an empty wire name are all "no such spec"). JSON has no `undefined`, so the
 * recording named it; the oracle must speak the same language in both directions.
 */
function encode(value) {
  if (value === undefined) return { null: true };
  if (Array.isArray(value)) return value.map(encode);
  if (value !== null && typeof value === "object" && !(value instanceof Set)) {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, encode(v)]));
  }
  return value;
}

function shrink(live, recorded) {
  if (Array.isArray(recorded)) {
    if (!Array.isArray(live)) return live;
    return live.map((v, i) => (i < recorded.length ? shrink(v, recorded[i]) : v));
  }
  if (recorded !== null && typeof recorded === "object") {
    if (live === null || typeof live !== "object") return live;
    return Object.fromEntries(Object.entries(recorded).map(([k, v]) => [k, shrink(live[k], v)]));
  }
  return live;
}

let checked = 0;
const moved = [];
for (const c of doc.cases) {
  if (!owned.has(c.fn)) continue;
  const got = encode(LIVE[c.fn](c.input));
  checked += 1;
  if (JSON.stringify(shrink(got, c.expected.value)) !== JSON.stringify(c.expected.value)) {
    moved.push(
      `  ${c.fn} / ${c.name}\n    recorded: ${JSON.stringify(c.expected.value)}\n    live    : ${JSON.stringify(got)}`,
    );
  }
}

console.error(
  `oracle-translate: replayed ${checked} recorded case(s) over ${owned.size} function(s)`,
);
if (moved.length) {
  console.error(`oracle-translate: ${moved.length} answer(s) have MOVED:\n${moved.join("\n")}`);
} else {
  console.error("oracle-translate: every one still answers what the fixture records");
}

const have = new Set(doc.cases.map((c) => `${c.fn} :: ${c.name}`));
const fresh = ADDITIONS.filter(([fn, name]) => !have.has(`${fn} :: ${name}`)).map(
  ([fn, name, input, value]) => ({
    fn,
    name,
    input,
    expected: { value },
  }),
);
console.error(`oracle-translate: ${fresh.length} case(s) would be ADDED`);

if (process.argv.includes("--check")) {
  process.exit(moved.length ? 1 : 0);
}
if (fresh.length) {
  doc.cases = [...doc.cases, ...fresh];
  writeFileSync(FIXTURE, JSON.stringify(doc, null, 2) + "\n");
  console.error(`oracle-translate: wrote ${doc.cases.length} cases`);
}
