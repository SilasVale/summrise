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
} from "../src/plugins/translate.ts";

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
