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
