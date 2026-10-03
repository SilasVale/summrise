// oracle-translate.mjs — the ORACLE for the NON-STREAMING half, the sibling of `oracle.mjs`.
//
// `oracle.mjs` drives the shipping `AnthropicStreamEncoder` and writes `fixtures/stream-corpus.json`.
// The four PURE functions beside it — `toOpenAIRequest`, `toAnthropicResponse`, `sse`, `toSSE` — had
// only HAND-WRITTEN Rust tests, which is a different and weaker thing: a hand-written test asserts what
// its author believed the TypeScript does, while an oracle asserts what the TypeScript DID. This file
// closes that gap the same way the streaming half was closed.
//
//   cd gateway/wasm && node oracle-translate.mjs > fixtures/translate-corpus.json
//
// IT IS COMMITTED SO THE FIXTURE IS REGENERABLE, and it is a `.mjs` for the reason AGENTS.md's
// carve-outs name: the oracle IS the shipping TypeScript, and only a JavaScript runtime can run it.
//
// THE COMPARISON IS THE JSON TEXT, NOT THE PARSED VALUE, and that is deliberate: `Object.keys` order is
// what the response BYTES are, the Rust side carries `preserve_order`, and comparing parsed values would
// quietly accept a reordered document. A case whose TypeScript THROWS is recorded as `threw`, because
// "both refuse is an equivalence" and a port that quietly accepted it would answer a document where the
// JavaScript answers an exception.
import { toOpenAIRequest, toAnthropicResponse, sse, toSSE } from "../src/anthropic-translate.ts";
// The plugin is 1,696 lines and reads keys, channels, KV and upstreams — but these two are pure, and
// they are its safety net. Node imports the module fine (measured), so the oracle drives the REAL
// functions rather than a snapshot of them.
import {
  scrubKeys,
  redactSecrets,
  sseResponse,
  keyMissingError,
  providerKeyMissingError,
  detectRoute,
  toolsRegionOf,
  needsBodyParse,
  isSearchOnlyRequest,
  isForcedWebSearch,
  extractByokKeys,
  bearerKeyFor,
  isKeyMissing,
  checkRateLimit,
} from "../src/plugins/translate.ts";
import {
  stripBracket,
  fnvHex,
  clientSessionId,
  syntheticSessionId,
  opencodeSessionHeader,
  passthroughHeaders,
} from "../src/upstream.ts";
import {
  allowedOrigins,
  isLoopbackOrigin,
  isLoopbackHost,
  isAllowedOrigin,
  corsHeadersFor,
} from "../src/http.ts";
import {
  safeEq,
  parseCookie,
  csrfCookieViolation,
  b64urlDecodeStr,
  sessionCookieHeader,
  clearSessionCookieHeader,
} from "../src/auth.ts";
// The scan/rewrite family has its own module — the same one the port took its rules from.
import {
  scanTopLevelModel,
  rawWithModel,
  rawWithTopLevelField,
  rawWithDeepSeekProvider,
  rawWithOxAlphaReasoningDefault,
  countBase64Payloads,
  estimateTextTokens,
  estimateTokens,
} from "../src/body-scan.ts";

// ── the corpus ────────────────────────────────────────────────────────────────────────────────────
// Every case is a SHAPE THE DEPLOYED WORKER CAN SEE, plus the two degenerate ends (an empty request and
// an empty upstream), plus one case that must NOT bite: unknown fields on the request are carried
// nowhere, and a document that already looks Anthropic-shaped is not double-translated.
const requestCases = [
  {
    name: "plain text, one user turn",
    input: { model: "m", max_tokens: 16, messages: [{ role: "user", content: "hi" }] },
  },
  {
    name: "system as a string",
    input: { system: "be brief", messages: [{ role: "user", content: "hi" }] },
  },
  {
    name: "system as a block array, mixed types",
    input: {
      system: [
        { type: "text", text: "one" },
        { type: "image", source: {} },
        { type: "text", text: "two" },
      ],
      messages: [{ role: "user", content: [{ type: "text", text: "hi" }] }],
    },
  },
  {
    name: "tools and a tool_choice",
    input: {
      messages: [{ role: "user", content: "go" }],
      tools: [{ name: "t", description: "d", input_schema: { type: "object", properties: {} } }],
      tool_choice: { type: "auto" },
    },
  },
  {
    name: "an assistant turn with a tool_use block",
    input: {
      messages: [
        { role: "user", content: "go" },
        { role: "assistant", content: [{ type: "tool_use", id: "tu1", name: "t", input: { a: 1 } }] },
        { role: "user", content: [{ type: "tool_result", tool_use_id: "tu1", content: "done" }] },
      ],
    },
  },
  { name: "empty request", input: {} },
  {
    name: "unknown fields must not be carried",
    input: { messages: [{ role: "user", content: "hi" }], nonsense: { deep: [1, 2, 3] }, stream: false },
  },
];

const responseCases = [
  {
    name: "a plain choice",
    input: { choices: [{ message: { role: "assistant", content: "hello" }, finish_reason: "stop" }], usage: { prompt_tokens: 3, completion_tokens: 1 } },
  },
  {
    name: "reasoning_content",
    input: { choices: [{ message: { content: "answer", reasoning_content: "thinking" }, finish_reason: "stop" }] },
  },
  {
    name: "reasoning plus a reasoning_details list",
    input: { choices: [{ message: { content: "a", reasoning: "r", reasoning_details: [{ type: "text", text: "d" }] }, finish_reason: "stop" }] },
  },
  {
    name: "tool calls",
    input: {
      choices: [
        {
          message: {
            content: null,
            tool_calls: [{ id: "c1", type: "function", function: { name: "t", arguments: '{"a":1}' } }],
          },
          finish_reason: "tool_calls",
        },
      ],
    },
  },
  { name: "no choices at all", input: {} },
  { name: "an empty choices array", input: { choices: [] } },
  { name: "a choice with no message", input: { choices: [{ finish_reason: "stop" }] } },
];

const sseCases = [
  { name: "a simple event", event: "ping", data: { type: "ping" } },
  { name: "an event with unicode and a newline in a string", event: "delta", data: { text: "a\nb — 中文" } },
  { name: "an event with nested arrays", event: "x", data: { a: [1, { b: 2 }] } },
];

const toSseCases = [
  {
    name: "a full message",
    input: { id: "msg_1", model: "m", role: "assistant", content: [{ type: "text", text: "hi" }], usage: { input_tokens: 1, output_tokens: 2 } },
  },
  { name: "no content", input: { id: "msg_2", model: "m" } },
];

// ── the body scan and rewrite ─────────────────────────────────────────────────────────────────────
// THE OBSERVABLE RESULT, NOT THE OFFSETS: the JavaScript indexes UTF-16 code units and the port walks
// bytes, so the two agree on the rewritten TEXT (the boundaries land on ASCII delimiters) and can differ
// on the raw index NUMBERS when non-ASCII text precedes the field. Comparing the numbers would make the
// corpus a test of the encoding rather than of the scan.
const BIG = 2 * 1024 * 1024;
const bigBodyWithModelLate = () => {
  const pad = "x".repeat(BIG + 1024);
  return `{"system":"${pad}","model":"late/model"}`;
};
const bigBodyWithFieldLate = () => {
  const pad = "x".repeat(BIG + 1024);
  return `{"system":"${pad}","reasoning":{"effort":"low"}}`;
};
const scanCases = [
  { name: "a lone model", text: '{"model":"a/b"}' },
  { name: "model AFTER other fields (the comma arm)", text: '{"system":"s","tools":[],"model":"a/b"}' },
  { name: "model inside a nested object is NOT top level", text: '{"x":{"model":"nope"}}' },
  { name: "model inside an array is not top level", text: '{"x":[{"model":"nope"}]}' },
  { name: "an escaped quote in a value before model", text: '{"system":"a\\"b","model":"a/b"}' },
  { name: "an escaped backslash before model", text: '{"system":"a\\\\","model":"a/b"}' },
  { name: "model is not a string", text: '{"model":5}' },
  { name: "no model at all", text: '{"system":"s"}' },
  { name: "whitespace before the value", text: '{"model" :  "a/b" }' },
  { name: "an empty body", text: "" },
  { name: "model BEYOND the 2 MiB ceiling", text: bigBodyWithModelLate() },
];
const rawModelCases = [
  { name: "replace it", text: '{"model":"old/m","x":1}', model: "new/m" },
  { name: "absent leaves the body ALONE", text: '{"x":1}', model: "new/m" },
  { name: "an empty body", text: "", model: "new/m" },
  { name: "replace one that is not a string", text: '{"model":5}', model: "new/m" },
];
const topFieldCases = [
  { name: "replace an existing field", text: '{"provider":{"order":["x"]},"model":"m"}' },
  { name: "append an absent field", text: '{"model":"m"}' },
  { name: "append into an empty object (no comma)", text: "{}" },
  { name: "append into a body with trailing whitespace", text: '{"model":"m"  }' },
  { name: "no closing brace at all", text: '{"model":"m"' },
  { name: "the field is beyond the ceiling: UNCHANGED", text: bigBodyWithFieldLate() },
];
const bsProviderCases = [
  { name: "absent", text: '{"model":"m"}' },
  { name: "present", text: '{"provider":{"order":["other"]},"model":"m"}' },
];
const oxCases = [
  { name: "absent gets the default", text: '{"model":"m"}' },
  { name: "present is respected", text: '{"reasoning":{"effort":"low"},"model":"m"}' },
  { name: "present but null still counts as present", text: '{"reasoning":null,"model":"m"}' },
];

// ── the token estimates ───────────────────────────────────────────────────────────────────────────
// THE LENGTHS ARE UTF-16 CODE UNITS in the JavaScript, so the CJK cases below are not decoration: a port
// counting BYTES would come out ~3x long on exactly the bodies the recorded incident is about.
const b64 = (n) => "A".repeat(n);
const tokensCases = [
  { name: "nothing", text: "{}" },
  { name: "no data key", text: '{"messages":[{"content":"hi"}]}' },
  { name: "511 chars is NOT a payload", text: `{"data":"${b64(511)}"}` },
  { name: "512 chars IS", text: `{"data":"${b64(512)}"}` },
  { name: "two payloads", text: `{"data":"${b64(600)}","x":1,"data":"${b64(700)}"}` },
  {
    name: "a payload BEFORE the last user message is not counted",
    text: `{"messages":[{"role":"user","content":[{"data":"${b64(600)}"}]},{"role":"assistant","content":"x"},{"role":"user","content":"y"}]}`,
  },
  {
    name: "a payload IN the last user message is counted",
    text: `{"messages":[{"role":"user","content":"x"},{"role":"user","content":[{"data":"${b64(600)}"}]}]}`,
  },
  { name: "a non-base64 char ends the run", text: `{"data":"${b64(600)}!!!"}` },
  { name: "an empty body", text: "" },
];
const textTokenCases = [
  { name: "ascii", s: "a".repeat(400), textLen: 400 },
  { name: "CJK (the code-unit case)", s: "中".repeat(100), textLen: 100 },
  { name: "mixed", s: "中文abc".repeat(50), textLen: 350 },
  { name: "an empty string", s: "", textLen: 0 },
  { name: "a base64 run inside the sample is stripped first", s: `{"data":"${b64(600)}"}`, textLen: 620 },
  { name: "textLen larger than the sample", s: "a".repeat(100), textLen: 4000 },
];
const estimateCases = [
  { name: "an empty object", value: {} },
  { name: "ascii text", value: { messages: [{ content: "hello world" }] } },
  { name: "CJK text", value: { messages: [{ content: "中文内容" }] } },
  { name: "one image", value: { messages: [{ content: [{ type: "image", source: { data: b64(600) } }] }] } },
  { name: "a number input", value: 42 },
  { name: "a null input", value: null },
  // THE REAL CALLER PASSES A STRING — the parameter is typed `any`, so the object cases above estimate
  // `"[object Object]"`, which is what the JavaScript does and NOT what the route does.
  { name: "a STRING body, ascii", value: '{"messages":[{"content":"hello world"}]}' },
  { name: "a STRING body, CJK", value: '{"messages":[{"content":"中文内容测试"}]}' },
  {
    name: "a STRING body with one image in the last user message",
    value: `{"messages":[{"role":"user","content":[{"type":"image","source":{"data":"${b64(600)}"}}]}]}`,
  },
];

// ── the session and header decisions ──────────────────────────────────────────────────────────────
// `zen/go` 400s without `x-opencode-session`, so these bytes are load-bearing. The FNV cases are the ones
// a port gets wrong quietly: `Math.imul` keeps the low 32 bits of a signed multiply, and the fold is two
// halves rather than one 64-bit hash.
const bracketCases = [
  { name: "a trailing bracket", s: "model[1m]" },
  { name: "not at the end", s: "a[b]c" },
  { name: "two groups, only the last goes", s: "[a][b]" },
  { name: "an empty group", s: "model[]" },
  { name: "an unclosed bracket", s: "a[" },
  { name: "a closing bracket alone", s: "a]" },
  { name: "nothing to strip", s: "model" },
  { name: "an empty string", s: "" },
  { name: "a bracket group with a space inside", s: "m[ 1m ]" },
];
const fnvCases = [
  { name: "an empty string", s: "" },
  { name: "ascii", s: "summrise-og-session-v1:u1" },
  { name: "CJK (UTF-16 units, not bytes)", s: "用户一" },
  { name: "a long uid", s: "u".repeat(500) },
  { name: "punctuation", s: "a-b_c.d:e/f" },
];
const clientIdCases = [
  { name: "the first name wins", headers: { "x-opencode-session": "a", "x-client-request-id": "b" } },
  { name: "the second when the first is blank", headers: { "x-opencode-session": "   ", "x-client-request-id": "b" } },
  { name: "session_id", headers: { session_id: "c" } },
  { name: "x-session-id", headers: { "x-session-id": "d" } },
  { name: "CASE-INSENSITIVE lookup", headers: { "X-Opencode-Session": "e" } },
  { name: "trimmed", headers: { "x-opencode-session": "  f  " } },
  { name: "none at all", headers: {} },
  { name: "an unrelated header", headers: { authorization: "Bearer x" } },
];
const sessionHeaderCases = [
  { name: "the client id", headers: { "x-opencode-session": "abc" }, uid: "u1" },
  { name: "the synthetic fallback", headers: {}, uid: "u1" },
  { name: "a blank client id falls back", headers: { "x-opencode-session": " " }, uid: "u1" },
];
const passthroughCases = [
  { name: "a bearer key", bearerKey: "sk-1", apiKeyHeader: null, extra: {} },
  { name: "the api key header form", bearerKey: "sk-1", apiKeyHeader: "x-api-key", extra: {} },
  { name: "no key at all", bearerKey: null, apiKeyHeader: null, extra: {} },
  { name: "an empty key is no key", bearerKey: "", apiKeyHeader: null, extra: {} },
  { name: "extra headers", bearerKey: "sk-1", apiKeyHeader: null, extra: { "x-a": "1", "x-b": "2" } },
  { name: "an empty extra value adds nothing", bearerKey: "sk-1", apiKeyHeader: null, extra: { "x-a": "" } },
  { name: "an empty apiKeyHeader means Bearer", bearerKey: "sk-1", apiKeyHeader: "", extra: {} },
];

// ── the CORS gate ─────────────────────────────────────────────────────────────────────────────────
// A SECURITY SURFACE, and the case that matters is the one audit P2 was: a loopback ORIGIN arriving at a
// NON-loopback request host must get no grant. Every other case here is a spelling of "which origin is
// this", which is where a port drifts.
const originListCases = [
  { name: "no configuration", configured: null },
  { name: "an empty configuration", configured: "" },
  { name: "a blank configuration", configured: "   " },
  { name: "one origin", configured: "https://a.example" },
  { name: "several, with spaces", configured: " https://a.example , https://b.example " },
  { name: "a trailing comma keeps the rest", configured: "https://a.example," },
  { name: "only commas is the default", configured: ",,," },
];
const loopbackOriginCases = [
  { name: "http localhost", origin: "http://localhost" },
  { name: "http localhost with a port", origin: "http://localhost:8787" },
  { name: "https 127.0.0.1", origin: "https://127.0.0.1" },
  { name: "UPPERCASE host is lowercased by URL", origin: "http://LOCALHOST" },
  { name: "ftp is not a loopback origin", origin: "ftp://localhost" },
  { name: "a real origin", origin: "https://example.com" },
  { name: "localhost as a LABEL is not localhost", origin: "http://localhost.evil.com" },
  { name: "not a url at all", origin: "not a url" },
  { name: "an empty string", origin: "" },
  { name: "a relative url", origin: "/api/health" },
  { name: "IPv6 loopback is NOT matched", origin: "http://[::1]:8787" },
  { name: "127.0.0.2 is not loopback", origin: "http://127.0.0.2" },
];
const loopbackHostCases = [
  { name: "localhost", host: "localhost" },
  { name: "127.0.0.1", host: "127.0.0.1" },
  { name: "LOCALHOST is not (case-sensitive)", host: "LOCALHOST" },
  { name: "127.0.0.2", host: "127.0.0.2" },
  { name: "an empty host", host: "" },
  { name: "a deployed host", host: "api.saisi.online" },
];
const allowedOriginCases = [
  { name: "an allowlisted origin", origin: "https://ai.saisi.online", requestHost: "api.saisi.online" },
  { name: "a loopback origin at a loopback host", origin: "http://localhost:8787", requestHost: "localhost" },
  { name: "a loopback origin at a DEPLOYED host gets NOTHING (audit P2)", origin: "http://localhost:8787", requestHost: "api.saisi.online" },
  { name: "a loopback origin with no host at all", origin: "http://localhost:8787", requestHost: null },
  { name: "a foreign origin", origin: "https://evil.example", requestHost: "api.saisi.online" },
  { name: "an empty origin", origin: "", requestHost: "api.saisi.online" },
  { name: "the configured override", origin: "https://test.example", requestHost: "test.example", configured: "https://test.example" },
  { name: "the override REPLACES the default", origin: "https://ai.saisi.online", requestHost: "api.saisi.online", configured: "https://test.example" },
];
const corsHeaderCases = [
  { name: "allowed reflects and varies", origin: "https://ai.saisi.online", requestHost: "api.saisi.online" },
  { name: "not allowed has no ACAO", origin: "https://evil.example", requestHost: "api.saisi.online" },
  { name: "a loopback pair reflects", origin: "http://localhost:8787", requestHost: "localhost" },
  { name: "a loopback origin at a deployed host does not", origin: "http://localhost:8787", requestHost: "api.saisi.online" },
  { name: "an empty origin", origin: "", requestHost: "api.saisi.online" },
];

// ── the auth decisions ────────────────────────────────────────────────────────────────────────────
// ALL SECURITY SHAPES, and none of them throws when it drifts. The CSRF cases carry the arm that looks
// wrong and is deliberate: an ABSENT Sec-Fetch-Site is ALLOWED, because browsers always send it and a
// non-browser client has no ambient cookie to ride.
const safeEqCases = [
  { name: "equal", a: "abc", b: "abc" },
  { name: "one character differs", a: "abc", b: "abd" },
  { name: "the first character differs", a: "abc", b: "zbc" },
  { name: "different lengths", a: "abc", b: "abcd" },
  { name: "both empty", a: "", b: "" },
  { name: "one empty", a: "", b: "a" },
  { name: "unicode", a: "中文", b: "中文" },
];
const cookieCases = [
  { name: "one pair", s: "a=1" },
  { name: "several", s: "a=1; b=2" },
  { name: "a pair with no equals is SKIPPED", s: "a=1; junk; b=2" },
  { name: "a value containing equals", s: "a=1=2" },
  { name: "spaces are trimmed on BOTH sides", s: "  a  =  1  " },
  { name: "a duplicate name keeps the LAST", s: "a=1; a=2" },
  { name: "an empty value", s: "a=" },
  { name: "an empty name", s: "=b" },
  { name: "an empty string", s: "" },
  { name: "trailing semicolon", s: "a=1;" },
];
const csrfCases = [
  { name: "a GET is not gated", method: "GET", cookie: "ag_session=x", secFetchSite: "cross-site" },
  { name: "a POST with NO cookie", method: "POST", cookie: "", secFetchSite: "cross-site" },
  { name: "a POST with an unrelated cookie", method: "POST", cookie: "other=1", secFetchSite: "cross-site" },
  { name: "same-origin passes", method: "POST", cookie: "ag_session=x", secFetchSite: "same-origin" },
  { name: "none passes", method: "POST", cookie: "ag_session=x", secFetchSite: "none" },
  { name: "cross-site is refused", method: "POST", cookie: "ag_session=x", secFetchSite: "cross-site" },
  { name: "same-site is refused", method: "POST", cookie: "ag_session=x", secFetchSite: "same-site" },
  { name: "an ABSENT header is ALLOWED (the non-browser arm)", method: "POST", cookie: "ag_session=x", secFetchSite: null },
  { name: "the site value is case-insensitive", method: "POST", cookie: "ag_session=x", secFetchSite: "SAME-ORIGIN" },
  { name: "the method is case-insensitive", method: "post", cookie: "ag_session=x", secFetchSite: "cross-site" },
  { name: "a DEVICE PAIR cookie counts", method: "POST", cookie: "summrise_pt_d1=y", secFetchSite: "cross-site" },
  { name: "a device cookie that is not a pair does not", method: "POST", cookie: "summrise_other=y", secFetchSite: "cross-site" },
  { name: "DELETE is gated", method: "DELETE", cookie: "ag_session=x", secFetchSite: "cross-site" },
];
const b64Cases = [
  { name: "plain", s: "aGVsbG8=" },
  { name: "unpadded", s: "aGVsbG8" },
  { name: "url alphabet", s: "a-_b" },
  { name: "whitespace is ignored", s: "aGVs bG8=" },
  { name: "one byte", s: "YQ==" },
  { name: "two bytes", s: "YWI=" },
  { name: "an empty string", s: "" },
  { name: "an invalid character", s: "aGVs*bG8=" },
  { name: "padding in the middle", s: "aG=sbG8=" },
  { name: "a bad length", s: "a" },
  { name: "bytes that are not utf-8", s: "/w==" },
];
const authSessionHeaderCases = [
  { name: "not secure", token: "t", maxAgeSec: 3600, secure: false },
  { name: "secure", token: "t", maxAgeSec: 3600, secure: true },
  { name: "zero max age", token: "t", maxAgeSec: 0, secure: false },
];
const authClearHeaderCases = [{ name: "not secure", secure: false }, { name: "secure", secure: true }];

// ── the rate-limit guard ──────────────────────────────────────────────────────────────────────────
// THE CLOCK IS PINNED, because `Math.floor(Date.now() / 60000)` is the minute bucket and
// `Math.floor(Date.now() / 86400000)` is the day bucket — with a moving clock only one bucket is ever
// exercised. Each case uses its OWN TOKEN, because the shipping worker's counters are module-level maps
// that persist across calls, and the limits are reached by COUNTING (`preload`), not by a magic value.
//
// The day limit needs its counts SPREAD ACROSS MINUTES: the minute limit is checked first, so 4000
// requests in one minute are refused at 48. Spreading them (48 per minute, advancing the clock) reaches
// the day counter without tripping the minute one — and it is what the deployed worker sees, since a real
// day contains 1440 minute buckets.
const FIXED_NOW = 1_700_000_000_000;
const rateLimitCases = [
  { name: "a GET is not counted", hasKeys: true, method: "GET", path: "/v1/messages" },
  { name: "a path outside /v1 is not counted", hasKeys: true, method: "POST", path: "/api/health" },
  { name: "no KEYS binding means no guard", hasKeys: false, method: "POST", path: "/v1/messages" },
  // PRELOADED TO THE LIMIT, and that is what makes the exclusion OBSERVABLE: with an empty counter both
  // implementations allow a single call, so the case passed while the gate was broken (measured — the
  // mutation "count_tokens is no longer excluded" did not bite until this preload was added).
  { name: "count_tokens is EXCLUDED even at the limit", hasKeys: true, method: "POST", path: "/v1/messages/count_tokens", preload: 48 },
  { name: "messages is counted", hasKeys: true, method: "POST", path: "/v1/messages", preload: 1 },
  { name: "chat/completions is counted", hasKeys: true, method: "POST", path: "/v1/chat/completions", preload: 1 },
  { name: "responses is counted", hasKeys: true, method: "POST", path: "/v1/responses", preload: 1 },
  { name: "47 is still allowed", hasKeys: true, method: "POST", path: "/v1/messages", preload: 47 },
  { name: "48 is refused", hasKeys: true, method: "POST", path: "/v1/messages", preload: 48 },
  { name: "4000 spread across minutes is refused on the DAY", hasKeys: true, method: "POST", path: "/v1/messages", spread: 4000 },
];

// ── the BYOK decisions ────────────────────────────────────────────────────────────────────────────
// The three irregular kind names are the reason this table is centralised at all, so every one of the nine
// kinds is a case, and `nv`/`gmi` carry `envKey: null` — an env value of that name must NOT be used.
const ukeyRecords = [
  { name: "every field set", input: { DEEPSEEK_API_KEY: "d", OPENCODE_GO_API_KEY: "o", OPENROUTER_API_KEY: "r", QWEN_API_KEY: "q", NVAPI_KEY: "n", GMI_API_KEY: "g", CMD_API_KEY: "c", AMD_API_KEY: "a", R4_API_KEY: "4" } },
  { name: "an empty record", input: {} },
  { name: "falsy values become null", input: { DEEPSEEK_API_KEY: "", OPENCODE_GO_API_KEY: 0, OPENROUTER_API_KEY: false, QWEN_API_KEY: null } },
  { name: "a truthy non-string is CARRIED as it is", input: { DEEPSEEK_API_KEY: 5, OPENCODE_GO_API_KEY: { a: 1 }, OPENROUTER_API_KEY: ["x"] } },
  { name: "an unknown field is not carried", input: { SOMETHING_ELSE: "x", DEEPSEEK_API_KEY: "d" } },
];

const bearerCases = [
  { name: "the user key", ukeys: { DEEPSEEK_API_KEY: "u" }, env: {}, kind: "deepseek" },
  { name: "the env fallback", ukeys: {}, env: { DEEPSEEK_API_KEY: "e" }, kind: "deepseek" },
  { name: "neither", ukeys: {}, env: {}, kind: "deepseek" },
  { name: "the user key wins", ukeys: { DEEPSEEK_API_KEY: "u" }, env: { DEEPSEEK_API_KEY: "e" }, kind: "deepseek" },
  { name: "nv has NO env fallback", ukeys: {}, env: { NVAPI_KEY: "e" }, kind: "nvidia" },
  { name: "gmi has NO env fallback", ukeys: {}, env: { GMI_API_KEY: "e" }, kind: "gmi" },
  { name: "nv still takes the user key", ukeys: { NVAPI_KEY: "u" }, env: {}, kind: "nvidia" },
  { name: "the irregular opencode -> opencodeGo", ukeys: { OPENCODE_GO_API_KEY: "u" }, env: {}, kind: "opencode" },
  { name: "the irregular commandgoat -> cmd", ukeys: { CMD_API_KEY: "u" }, env: {}, kind: "commandgoat" },
  { name: "an unknown kind", ukeys: { DEEPSEEK_API_KEY: "u" }, env: {}, kind: "nope" },
  { name: "a falsy user key falls through to env", ukeys: { DEEPSEEK_API_KEY: "" }, env: { DEEPSEEK_API_KEY: "e" }, kind: "deepseek" },
  { name: "a falsy env value is not a key", ukeys: {}, env: { DEEPSEEK_API_KEY: "" }, kind: "deepseek" },
];

const keyMissingCases = [
  { name: "present", ukeys: { DEEPSEEK_API_KEY: "u" }, kind: "deepseek" },
  { name: "absent", ukeys: {}, kind: "deepseek" },
  { name: "absent but env has it", ukeys: {}, env: { DEEPSEEK_API_KEY: "e" }, kind: "deepseek" },
  { name: "nv absent, env set — still missing", ukeys: {}, env: { NVAPI_KEY: "e" }, kind: "nvidia" },
  { name: "an UNKNOWN kind is not missing", ukeys: {}, kind: "nope" },
  { name: "an empty kind", ukeys: {}, kind: "" },
];

// ── the request-shape decisions ───────────────────────────────────────────────────────────────────
// The cases are the INCIDENTS the source records, because that is what a corpus is for: a schema property
// NAMED `messages`, a `web_search` literal sitting AFTER the tools array, a nested array inside a tool,
// and the whitespace `\s` really accepts.
const routeCases = [
  { method: "POST", path: "/v1/messages/count_tokens" },
  { method: "POST", path: "/v1/messages" },
  { method: "POST", path: "/v1/chat/completions" },
  { method: "POST", path: "/v1/responses" },
  { method: "GET", path: "/v1/messages" },
  { method: "POST", path: "/api/v1/messages" },
  { method: "POST", path: "/v1/messagesx" },
  { method: "POST", path: "/" },
];

const toolsRegionCases = [
  { name: "no tools at all", text: '{"messages":[]}' },
  { name: "a simple array", text: '{"tools":[{"name":"a"}],"messages":[]}' },
  {
    name: "a schema property NAMED messages (round 42)",
    text: '{"tools":[{"name":"a","input_schema":{"properties":{"messages":{"type":"array"}}}}],"messages":[]}',
  },
  {
    name: "a nested array inside a tool",
    text: '{"tools":[{"name":"a","x":[[1,2],{"y":[3]}]}],"model":"m"}',
  },
  {
    name: "a web_search literal AFTER the array must stay out",
    text: '{"tools":[{"name":"a"}],"messages":[{"content":[{"type":"server_tool_use","name":"web_search"}]}]}',
  },
  { name: "an empty array", text: '{"tools":[],"messages":[]}' },
  { name: "the array is the whole body", text: '{"tools":[{"name":"a"}]}' },
  { name: "unbalanced, never closed", text: '{"tools":[{"name":"a"}' },
];

const bodyParseCases = [
  { name: "nothing special", rawText: '{"messages":[{"content":"hi"}]}', toolsRegion: "" },
  { name: "an image type", rawText: '{"type":"image"}', toolsRegion: "" },
  { name: "an image type with spaces", rawText: '{"type" :  "image"}', toolsRegion: "" },
  { name: "an image type with a newline", rawText: '{"type"\n:\t"image"}', toolsRegion: "" },
  {
    name: "a NON-BREAKING SPACE between them, which JS \s accepts",
    rawText: '{"type"\u00a0:"image"}',
    toolsRegion: "",
  },
  { name: "web_search in the region", rawText: "{}", toolsRegion: '{"name":"web_search"}' },
  { name: "web_search OUTSIDE the region", rawText: '{"messages":[{"name":"web_search"}]}', toolsRegion: "" },
  { name: "type image but not the value", rawText: '{"type":"image/png"}', toolsRegion: "" },
];

const searchOnlyCases = [
  { name: "the one web_search tool", input: { tools: [{ type: "web_search_20250305" }] } },
  { name: "two tools", input: { tools: [{ type: "web_search_20250305" }, { type: "x" }] } },
  { name: "a tool_choice is present", input: { tools: [{ type: "web_search_20250305" }], tool_choice: { type: "auto" } } },
  { name: "the wrong type", input: { tools: [{ type: "other" }] } },
  { name: "no tools key", input: { messages: [] } },
  { name: "an empty array", input: { tools: [] } },
  { name: "tools is not an array", input: { tools: "x" } },
  { name: "null body", input: null },
  { name: "an empty object", input: {} },
];

const forcedSearchCases = [
  { name: "type tool naming it", input: { type: "tool", name: "web_search" } },
  { name: "type tool naming something else", input: { type: "tool", name: "other" } },
  { name: "type any listing it", input: { type: "any", tools: [{ name: "web_search" }] } },
  { name: "type any listing others", input: { type: "any", tools: [{ name: "a" }, { name: "b" }] } },
  { name: "type any with an empty list", input: { type: "any", tools: [] } },
  { name: "type auto (a DECLARATION is not intent)", input: { type: "auto" } },
  { name: "null", input: null },
  { name: "an empty object", input: {} },
];

// ── the response builders ─────────────────────────────────────────────────────────────────────────
// The shapes a client reads when something is WRONG. A `Response` body is a STREAM, so these are read
// with `await res.text()` and pushed below the synchronous corpus rather than inside it.
const sseCases2 = [
  { name: "a body", input: "event: x\ndata: {}\n\n" },
  { name: "an empty body", input: "" },
  { name: "null body", input: null },
];

const keyMissingKinds = [
  "deepseek",
  "opencode",
  "openrouter",
  "qwen",
  "nvidia",
  "gmi",
  "amd",
  "commandgoat",
  "r4",
  "an unknown kind",
  "",
  // **THE PROTOTYPE CHAIN, RECORDED RATHER THAN MATCHED.** `KEY_MISSING_MESSAGES[kind]` is a PROPERTY
  // lookup, so a kind of `constructor` finds `Object.prototype.constructor` — a FUNCTION — and the
  // JavaScript answers a 502 whose message is that function's SOURCE. A Rust table has no prototype and
  // answers null. Reproducing an accident of the prototype chain would be porting a bug, so the case is
  // in the corpus with `divergence` set and the Rust test asserts the difference is exactly this.
  { name: "the prototype chain", input: "constructor", divergence: "prototype" },
  { name: "toString", input: "toString", divergence: "prototype" },
];

const providerCases = [
  { name: "prefix and apiKeyEnv", input: { prefix: "myco", apiKeyEnv: "MYCO_KEY" } },
  { name: "prefix only", input: { prefix: "myco" } },
  { name: "apiKeyEnv only", input: { apiKeyEnv: "MYCO_KEY" } },
  { name: "an empty prefix takes the fallback", input: { prefix: "", apiKeyEnv: "MYCO_KEY" } },
  { name: "an empty apiKeyEnv takes the fallback", input: { prefix: "myco", apiKeyEnv: "" } },
  { name: "no provider at all", input: undefined },
  { name: "an empty record", input: {} },
];

// ── the redaction pair ────────────────────────────────────────────────────────────────────────────
// The regex cases are the ones a transliteration gets wrong: a word boundary, a GREEDY class, the
// prefix alternation's own order, and the `String(msg || "")` coercion for non-strings.
const scrubCases = [
  { name: "a bare key", input: "sk-abcdefgh" },
  { name: "a key inside a sentence", input: "failed with key sk-abcdefgh in the header" },
  { name: "the greedy class eats the whole thing", input: "sk-or-v1-abcdefgh" },
  { name: "no word boundary, no match", input: "xsk-abcdefgh" },
  { name: "a boundary from a dash", input: "-sk-abcdefgh" },
  { name: "seven characters is too short", input: "sk-abcdefg" },
  { name: "xoxb", input: "token xoxb-12345678 end" },
  { name: "or-", input: "or-abcdefgh" },
  { name: "two keys in one string", input: "sk-aaaaaaaa and rc-bbbbbbbb" },
  { name: "already scrubbed", input: "***" },
  { name: "unicode around a key", input: "密钥 sk-abcdefgh 结束" },
  { name: "a number input", input: 42 },
  { name: "null input", input: null },
  { name: "an array input", input: ["sk-abcdefgh", "x"] },
  { name: "an object input", input: { a: 1 } },
  { name: "an empty string", input: "" },
];

const redactCases = [
  { name: "one secret", text: "the key is longsecret123 ok", secrets: ["longsecret123"] },
  { name: "too short to matter", text: "the key is short ok", secrets: ["short"] },
  { name: "duplicates collapse", text: "longsecret123 and longsecret123", secrets: ["longsecret123", "longsecret123"] },
  {
    name: "nested: the long one goes first",
    text: "a longsecret123 and longsecret",
    secrets: ["longsecret123", "longsecret"],
  },
  {
    name: "nested, declared the other way round",
    text: "a longsecret123 and longsecret",
    secrets: ["longsecret", "longsecret123"],
  },
  { name: "no secrets at all", text: "nothing to hide", secrets: [] },
  { name: "a non-string entry", text: "12345678 here", secrets: [12345678] },
  { name: "falsy entries are skipped", text: "x longsecret123", secrets: [null, "", "longsecret123", undefined] },
  { name: "every occurrence", text: "longsecret123 longsecret123 longsecret123", secrets: ["longsecret123"] },
];

function attempt(fn) {
  try {
    return { value: fn() };
  } catch (e) {
    return { threw: String(e && e.message ? e.message : e) };
  }
}

const cases = [];
for (const c of requestCases) {
  const r = attempt(() => toOpenAIRequest(c.input, "og/model-x"));
  cases.push({
    fn: "to_openai_request",
    name: c.name,
    input: c.input,
    model: "og/model-x",
    expected: r.threw ? { threw: true, message: r.threw } : { json: JSON.stringify(r.value) },
  });
}
for (const c of responseCases) {
  const r = attempt(() => toAnthropicResponse(c.input, "og/model-x"));
  cases.push({
    fn: "to_anthropic_response",
    name: c.name,
    input: c.input,
    model: "og/model-x",
    expected: r.threw ? { threw: true, message: r.threw } : { json: JSON.stringify(r.value) },
  });
}
for (const c of sseCases) {
  const r = attempt(() => sse(c.event, c.data));
  cases.push({
    fn: "sse",
    name: c.name,
    event: c.event,
    input: c.data,
    expected: r.threw ? { threw: true, message: r.threw } : { text: r.value },
  });
}
for (const c of toSseCases) {
  const r = attempt(() => toSSE(c.input));
  cases.push({
    fn: "to_sse",
    name: c.name,
    input: c.input,
    expected: r.threw ? { threw: true, message: r.threw } : { text: r.value },
  });
}

for (const c of scrubCases) {
  const r = attempt(() => scrubKeys(c.input));
  cases.push({
    fn: "scrub_keys",
    name: c.name,
    input: c.input,
    expected: r.threw ? { threw: true, message: r.threw } : { text: r.value },
  });
}
for (const c of redactCases) {
  const r = attempt(() => redactSecrets(c.text, c.secrets));
  cases.push({
    fn: "redact_secrets",
    name: c.name,
    input: { text: c.text, secrets: c.secrets },
    expected: r.threw ? { threw: true, message: r.threw } : { text: r.value },
  });
}

for (const c of safeEqCases) {
  cases.push({
    fn: "safe_eq",
    name: c.name,
    input: { a: c.a, b: c.b },
    expected: { value: safeEq(c.a, c.b) },
  });
}
for (const c of cookieCases) {
  cases.push({ fn: "parse_cookie", name: c.name, input: c.s, expected: { value: parseCookie(c.s) } });
}
for (const c of csrfCases) {
  const headers = { get: (n) => (n === "cookie" ? c.cookie : n === "sec-fetch-site" ? c.secFetchSite : null) };
  cases.push({
    fn: "csrf_cookie_violation",
    name: c.name,
    input: { method: c.method, cookie: c.cookie, secFetchSite: c.secFetchSite },
    expected: { value: csrfCookieViolation({ method: c.method, headers }) },
  });
}
for (const c of b64Cases) {
  let expected;
  try {
    expected = { value: b64urlDecodeStr(c.s) };
  } catch (e) {
    expected = { threw: true, message: String(e && e.message ? e.message : e) };
  }
  cases.push({ fn: "b64_url_decode", name: c.name, input: c.s, expected });
}
for (const c of authSessionHeaderCases) {
  cases.push({
    fn: "session_cookie_header",
    name: c.name,
    input: { token: c.token, maxAgeSec: c.maxAgeSec, secure: c.secure },
    expected: { value: sessionCookieHeader(c.token, c.maxAgeSec, c.secure) },
  });
}
for (const c of authClearHeaderCases) {
  cases.push({
    fn: "clear_session_cookie_header",
    name: c.name,
    input: { secure: c.secure },
    expected: { value: clearSessionCookieHeader(c.secure) },
  });
}

for (const c of originListCases) {
  cases.push({
    fn: "allowed_origins",
    name: c.name,
    input: { configured: c.configured },
    expected: { value: [...allowedOrigins(c.configured === null ? undefined : { CONSOLE_ORIGINS: c.configured })] },
  });
}
for (const c of loopbackOriginCases) {
  cases.push({
    fn: "is_loopback_origin",
    name: c.name,
    input: { origin: c.origin },
    expected: { value: isLoopbackOrigin(c.origin) },
  });
}
for (const c of loopbackHostCases) {
  cases.push({
    fn: "is_loopback_host",
    name: c.name,
    input: { host: c.host },
    expected: { value: isLoopbackHost(c.host) },
  });
}
for (const c of allowedOriginCases) {
  const env = c.configured ? { CONSOLE_ORIGINS: c.configured } : undefined;
  cases.push({
    fn: "is_allowed_origin",
    name: c.name,
    input: { origin: c.origin, requestHost: c.requestHost, configured: c.configured ?? null },
    expected: { value: isAllowedOrigin(c.origin, c.requestHost ?? undefined, env) },
  });
}
for (const c of corsHeaderCases) {
  const env = c.configured ? { CONSOLE_ORIGINS: c.configured } : undefined;
  const req = { headers: { get: (n) => (n === "origin" ? c.origin : null) }, url: `https://${c.requestHost || "x"}/api/health` };
  cases.push({
    fn: "cors_headers_for",
    name: c.name,
    input: { origin: c.origin, requestHost: c.requestHost, configured: c.configured ?? null },
    expected: { value: corsHeadersFor(req, env) },
  });
}

const headersToObject = (h) => {
  const out = {};
  for (const [k, v] of h.entries()) out[k] = v;
  return out;
};
for (const c of bracketCases) {
  cases.push({ fn: "strip_bracket", name: c.name, input: c.s, expected: { value: stripBracket(c.s) } });
}
for (const c of fnvCases) {
  cases.push({ fn: "fnv_hex", name: c.name, input: c.s, expected: { value: fnvHex(c.s) } });
  cases.push({
    fn: "synthetic_session_id",
    name: c.name,
    input: c.s,
    expected: { value: syntheticSessionId(c.s) },
  });
}
for (const c of clientIdCases) {
  cases.push({
    fn: "client_session_id",
    name: c.name,
    input: c.headers,
    expected: { value: clientSessionId(c.headers) },
  });
}
for (const c of sessionHeaderCases) {
  cases.push({
    fn: "opencode_session_header",
    name: c.name,
    input: { headers: c.headers, uid: c.uid },
    expected: { value: opencodeSessionHeader(c.headers, c.uid) },
  });
}
for (const c of passthroughCases) {
  cases.push({
    fn: "passthrough_headers",
    name: c.name,
    input: {
      bearerKey: c.bearerKey,
      apiKeyHeader: c.apiKeyHeader,
      extra: c.extra,
    },
    expected: {
      value: headersToObject(
        passthroughHeaders(c.bearerKey, {
          apiKeyHeader: c.apiKeyHeader === null ? false : c.apiKeyHeader,
          extra: c.extra,
        }),
      ),
    },
  });
}

for (const c of tokensCases) {
  const got = countBase64Payloads(c.text);
  cases.push({
    fn: "count_base64_payloads",
    name: c.name,
    input: c.text,
    expected: { value: { images: got.images, removedChars: got.removedChars } },
  });
}
for (const c of textTokenCases) {
  cases.push({
    fn: "estimate_text_tokens",
    name: c.name,
    input: { s: c.s, textLen: c.textLen },
    expected: { value: estimateTextTokens(c.s, c.textLen) },
  });
}
for (const c of estimateCases) {
  cases.push({
    fn: "estimate_tokens",
    name: c.name,
    input: { value: c.value },
    expected: { value: estimateTokens(c.value) },
  });
}

for (const c of scanCases) {
  const scan = scanTopLevelModel(c.text);
  cases.push({
    fn: "scan_top_level_model",
    name: c.name,
    input: c.text,
    expected: { value: { model: scan.model, found: scan.valueStart >= 0 } },
  });
}
for (const c of rawModelCases) {
  cases.push({
    fn: "raw_with_model",
    name: c.name,
    input: { raw: c.text, model: c.model },
    expected: { value: rawWithModel(c.text, c.model) },
  });
}
for (const c of topFieldCases) {
  cases.push({
    fn: "raw_with_top_level_field",
    name: c.name,
    input: c.text,
    expected: { value: rawWithTopLevelField(c.text, "reasoning", { effort: "max" }) },
  });
}
for (const c of bsProviderCases) {
  cases.push({
    fn: "raw_with_deepseek_provider",
    name: c.name,
    input: c.text,
    expected: { value: rawWithDeepSeekProvider(c.text) },
  });
}
for (const c of oxCases) {
  cases.push({
    fn: "raw_with_ox_alpha_reasoning_default",
    name: c.name,
    input: c.text,
    expected: { value: rawWithOxAlphaReasoningDefault(c.text) },
  });
}

// A FRESH TOKEN PER CASE, and the clock pinned. `Date.now` is restored at the end.
const realNow = Date.now;
Date.now = () => FIXED_NOW;
for (const c of rateLimitCases) {
  const token = `tok-${c.name}`;
  const path = c.path;
  if (c.preload) {
    for (let i = 0; i < c.preload; i += 1) checkRateLimit({ KEYS: {} }, "POST", path, token);
  }
  if (c.spread) {
    let done = 0;
    let minute = 0;
    while (done < c.spread) {
      Date.now = () => FIXED_NOW + minute * 60_000;
      for (let i = 0; i < 48 && done < c.spread; i += 1, done += 1) {
        checkRateLimit({ KEYS: {} }, "POST", path, token);
      }
      minute += 1;
    }
    Date.now = () => FIXED_NOW;
  }
  const res = checkRateLimit(c.hasKeys ? { KEYS: {} } : {}, c.method, path, token);
  cases.push({
    fn: "rate_limit",
    name: c.name,
    // THE PRIMING SHAPE IS RECORDED, not just the final call: the Rust side has to reach the same state,
    // and "the counters were already at 48" is not reproducible from an outcome.
    input: {
      hasKeys: c.hasKeys,
      method: c.method,
      path,
      token,
      now: FIXED_NOW,
      preload: c.preload || 0,
      spread: c.spread || 0,
    },
    expected: res === null ? { null: true } : { status: res.status, body: await res.text() },
  });
}
Date.now = realNow;

for (const c of ukeyRecords) {
  cases.push({
    fn: "extract_byok_keys",
    name: c.name,
    input: c.input,
    expected: { value: extractByokKeys(c.input) },
  });
}
for (const c of bearerCases) {
  const byok = extractByokKeys(c.ukeys);
  cases.push({
    fn: "bearer_key_for",
    name: c.name,
    input: { ukeys: c.ukeys, env: c.env, kind: c.kind },
    expected: { value: bearerKeyFor(c.env, byok, c.kind) },
  });
}
for (const c of keyMissingCases) {
  const byok = extractByokKeys(c.ukeys);
  const input = { ukeys: c.ukeys, kind: c.kind };
  if (c.env !== undefined) input.env = c.env;
  cases.push({
    fn: "is_key_missing",
    name: c.name,
    input,
    // `env` OMITTED is the historical BYOK-only semantics, so the case omits the key rather than passing
    // an empty object — the two are different calls.
    expected: { value: c.env === undefined ? isKeyMissing(c.kind, byok) : isKeyMissing(c.kind, byok, c.env) },
  });
}

for (const c of routeCases) {
  cases.push({
    fn: "detect_route",
    name: `${c.method} ${c.path}`,
    input: c,
    expected: { value: detectRoute(c.method, c.path) },
  });
}
for (const c of toolsRegionCases) {
  cases.push({
    fn: "tools_region_of",
    name: c.name,
    input: c.text,
    expected: { value: toolsRegionOf(c.text) },
  });
}
for (const c of bodyParseCases) {
  cases.push({
    fn: "needs_body_parse",
    name: c.name,
    input: { rawText: c.rawText, toolsRegion: c.toolsRegion },
    expected: { value: needsBodyParse(c.rawText, c.toolsRegion) },
  });
}
for (const c of searchOnlyCases) {
  cases.push({
    fn: "is_search_only_request",
    name: c.name,
    input: c.input,
    expected: { value: isSearchOnlyRequest(c.input) },
  });
}
for (const c of forcedSearchCases) {
  cases.push({
    fn: "is_forced_web_search",
    name: c.name,
    input: c.input,
    expected: { value: isForcedWebSearch(c.input) },
  });
}

// The builders, read asynchronously because a Response body is a stream.
for (const c of sseCases2) {
  const res = sseResponse(c.input);
  cases.push({
    fn: "sse_response",
    name: c.name,
    input: c.input,
    expected: { status: res.status, headers: Object.fromEntries(res.headers), body: await res.text() },
  });
}
for (const kind of keyMissingKinds) {
  const value = typeof kind === "string" ? kind : kind.input;
  const name = typeof kind === "string" ? kind : kind.name;
  const res = keyMissingError(value);
  if (res === null) {
    cases.push({ fn: "key_missing_error", name, input: value, expected: { null: true } });
    continue;
  }
  cases.push({
    fn: "key_missing_error",
    name,
    input: value,
    expected: { status: res.status, headers: Object.fromEntries(res.headers), body: await res.text() },
    ...(typeof kind === "object" && kind.divergence ? { divergence: kind.divergence } : {}),
  });
}
for (const c of providerCases) {
  const res = providerKeyMissingError(c.input);
  cases.push({
    fn: "provider_key_missing_error",
    name: c.name,
    input: c.input === undefined ? null : c.input,
    expected: { status: res.status, headers: Object.fromEntries(res.headers), body: await res.text() },
  });
}

console.log(
  JSON.stringify(
    {
      note: "Generated by gateway/wasm/oracle-translate.mjs from the SHIPPING TypeScript. Do not edit by hand.",
      cases,
    },
    null,
    1,
  ),
);
