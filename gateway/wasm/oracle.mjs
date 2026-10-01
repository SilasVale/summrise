// The ORACLE for the streaming half: the SHIPPING TypeScript class is driven over a corpus, and what
// it produced is written out as the fixture the Rust test replays.
//
//   cd gateway/wasm && node oracle.mjs > fixtures/stream-corpus.json
//
// IT IS COMMITTED SO THE FIXTURE IS REGENERABLE, and it is a `.mjs` for the reason AGENTS.md's carve-outs name:
// the oracle IS the shipping TypeScript, and only a JavaScript runtime can run it. When the TypeScript class
// changes, this regenerates the expectations and `cargo test` says whether the port still agrees.
//
// The direction matters: the expectations come from the code the deployed worker runs, not from the
// port. A case whose TypeScript THROWS is recorded as `threw: true`, and the Rust side must raise too
// — "both refuse is an equivalence", and a port that quietly accepted it would answer a stream where
// the JavaScript answers an exception.
import { AnthropicStreamEncoder } from "../src/anthropic-translate.ts";

const chunk = (o) => o;
const delta = (d, finish = null, extra = {}) => ({
  id: extra.id ?? "chatcmpl-1",
  object: "chat.completion.chunk",
  choices: [{ index: 0, delta: d, finish_reason: finish }],
  ...extra,
});

// ── the corpus ────────────────────────────────────────────────────────────────────────────────────
const cases = [];
const add = (name, chunks, tail = "", clientModel = "og/m", upstreamModel = "m") =>
  cases.push({ name, clientModel, upstreamModel, chunks, tail });

// 1. a plain text stream, and the empty one
add("plain text, stop", [
  delta({ role: "assistant", content: "" }),
  delta({ content: "Hello" }),
  delta({ content: ", world" }),
  delta({}, "stop"),
]);
add("nothing at all", []);

// 2. the usage-only final chunk (round-116): choices is empty and the tokens must still arrive
add("usage-only final chunk", [
  delta({ content: "hi" }),
  delta({}, "stop"),
  chunk({ id: "chatcmpl-1", object: "chat.completion.chunk", choices: [], usage: { prompt_tokens: 11, completion_tokens: 3 } }),
]);
add("usage with cache details", [
  delta({ content: "hi" }),
  chunk({
    choices: [],
    usage: { prompt_tokens: 11, completion_tokens: 3, prompt_tokens_details: { cached_tokens: 7 } },
  }),
]);
add("usage with the zen cache name", [
  delta({ content: "hi" }),
  chunk({ choices: [], usage: { prompt_cache_hit_tokens: 5 } }),
]);

// 3. a mid-stream error frame (round-123/124) — terminal, and no message_delta after it
add("mid-stream error frame", [
  delta({ content: "partial" }),
  chunk({ error: { message: "upstream exploded", type: "server_error" } }),
  delta({ content: "never translated" }),
]);
add("error frame with no message", [chunk({ error: {} })]);

// 4. thinking → text (a type switch closes and opens), and both reasoning spellings
add("reasoning_content then text", [
  delta({ reasoning_content: "let me think" }),
  delta({ reasoning_content: " harder" }),
  delta({ content: "answer" }),
  delta({}, "stop"),
]);
add("reasoning as an array", [delta({ reasoning: ["a", "b"] }), delta({ content: "x" })]);
add("reasoning as a string", [delta({ reasoning: "plain" })]);

// 5. tool calls: id/name first, args later, and the delayed-start case (round-116)
add("tool call, id and name first", [
  delta({ tool_calls: [{ index: 0, id: "call_1", type: "function", function: { name: "lookup", arguments: "" } }] }),
  delta({ tool_calls: [{ index: 0, function: { arguments: '{"a"' } }] }),
  delta({ tool_calls: [{ index: 0, function: { arguments: ":1}" } }] }),
  delta({}, "tool_calls"),
]);
add("tool call, args BEFORE the id (the delayed start)", [
  delta({ tool_calls: [{ index: 0, function: { arguments: '{"a":1}' } }] }),
  delta({ tool_calls: [{ index: 0, id: "call_9", function: { name: "late" } }] }),
  delta({}, "tool_calls"),
]);
add("tool call whose id never arrives (start emitted at finish)", [
  delta({ tool_calls: [{ index: 0, function: { arguments: "{}" } }] }),
  delta({}, "tool_calls"),
]);
add("interleaved tools 0,1,0,1 (round-19/96/97)", [
  delta({ tool_calls: [{ index: 0, id: "a", function: { name: "fa", arguments: "1" } }] }),
  delta({ tool_calls: [{ index: 1, id: "b", function: { name: "fb", arguments: "2" } }] }),
  delta({ tool_calls: [{ index: 0, function: { arguments: "3" } }] }),
  delta({ tool_calls: [{ index: 1, function: { arguments: "4" } }] }),
  delta({}, "tool_calls"),
]);
add("a text block between two tool blocks", [
  delta({ content: "before" }),
  delta({ tool_calls: [{ index: 0, id: "a", function: { name: "fa", arguments: "1" } }] }),
  delta({ content: "middle" }),
  delta({ tool_calls: [{ index: 0, function: { arguments: "2" } }] }),
  delta({}, "tool_calls"),
]);
// ELEVEN PARALLEL TOOLS, because the JS object's key order is observable here: finish() emits the
// delayed starts and the closing stops in canonical-index order, and a lexicographic sort would put
// "10" before "2".
add(
  "eleven parallel tools, starts all delayed",
  Array.from({ length: 11 }, (_, i) => delta({ tool_calls: [{ index: i, function: { arguments: `{"i":${i}}` } }] })).concat([
    delta({}, "tool_calls"),
  ]),
);

// 6. the finish_reason map
for (const reason of ["stop", "tool_calls", "function_call", "length", "something_else", ""]) {
  add(`finish_reason ${JSON.stringify(reason)}`, [delta({ content: "x" }), delta({}, reason)]);
}

// 7. the tail buffer: a partial frame the reader had not split yet
add("tail carries a complete frame", [delta({ content: "x" })], 'data: {"choices":[{"delta":{"content":"tail"}}]}\n');
add("tail carries [DONE]", [delta({ content: "x" })], "data: [DONE]\n");
add("tail carries malformed JSON", [delta({ content: "x" })], "data: {oops\n");
add("tail carries no data line", [delta({ content: "x" })], "event: ping\n");

// 8. shapes the encoder has to survive
add("content as a number", [delta({ content: 42 })]);
add("content as an object", [delta({ content: { a: 1 } })]);
add("id is not a string", [delta({ content: "x" }, null, { id: 7 })]);
add("no id on any chunk", [{ choices: [{ index: 0, delta: { content: "x" } }] }]);
add("choices is not an array", [chunk({ choices: 5 })]);
add("a chunk that is not an object", [5, "x", null]);
add("tool_calls as a string (iterable, char by char)", [delta({ tool_calls: "ab" })]);
add("an empty tool_calls array", [delta({ tool_calls: [] })]);
add("finish() twice is the same as once", [delta({ content: "x" }, "stop")]);
add("a delta that is not an object", [{ choices: [{ index: 0, delta: 7 }] }]);
add("finish_reason as a number", [delta({}, 3)]);
add("error frame after the encoder already finished", [delta({ content: "x" }, "stop"), chunk({ error: { message: "late" } })]);

// ── play every case through the SHIPPING class ────────────────────────────────────────────────────
const out = [];
for (const c of cases) {
  const enc = new AnthropicStreamEncoder(c.clientModel, c.upstreamModel);
  let events = "";
  let threw = null;
  try {
    for (const ch of c.chunks) {
      enc.push(ch);
      events += enc.take();
    }
    const fin = enc.finish(c.tail);
    if (fin) events += fin;
    // A second finish() must be a no-op, and the flags are part of the contract.
    const again = enc.finish(c.tail);
    if (again) events += again;
  } catch (e) {
    threw = String(e && e.message ? e.message : e);
  }
  out.push({
    name: c.name,
    clientModel: c.clientModel,
    upstreamModel: c.upstreamModel,
    chunks: c.chunks,
    tail: c.tail,
    expected: threw ? { threw: true } : { events, started: enc.started, finished: enc.finished },
  });
}

console.log(JSON.stringify({ note: "Generated by gateway/wasm/oracle.mjs from the SHIPPING TypeScript class. Do not edit by hand.", cases: out }, null, 1));
