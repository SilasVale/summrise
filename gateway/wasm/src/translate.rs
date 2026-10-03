//! The gateway's Anthropic ↔ OpenAI translation — the largest pure-logic file in the worker.
//!
//! ## WHY THIS IS A SEPARATE MODULE AND NOT MORE OF `lib.rs`
//!
//! `gateway/src/anthropic-translate.ts` is 798 lines, of which this module is the non-streaming half:
//! the two translators, the two message converters, and the SSE encoder. **The other 540 lines are
//! `streamOgToAnthropic`**, which converts an OpenAI SSE stream incrementally and needs a
//! `ReadableStream`; it is the next piece, and it is not smuggled in here.
//!
//! ## THE THREE THINGS THIS DOES THAT `zen-go-proxy`'s TRANSLATION DOES NOT
//!
//! The sibling file is the closest thing in this repository to the same code — same protocol, same
//! shape, same author — and **the differences are the value of a differential over a copy**:
//!
//! * **IMAGES.** `convertUserMessage` carries text AND image parts together as an OpenAI content
//!   ARRAY, so a vision model receives the image; the satellite's version joins the text into a string
//!   and has no image arm at all. **`flush()` is what keeps them adjacent**, and a port that appended
//!   the image after the text would produce a different document.
//! * **THE STREAM FLAG IS THE CALLER'S.** The gateway writes `stream: !!req.stream`; the satellite
//!   hardcodes `false`. So `stream` is a **`!!` on a possibly-absent value** here, which is neither of
//!   the two optional-field rules next to it.
//! * **TOOL PARAMETERS ARE NORMALIZED.** `raw.type ? raw : {type: "object", properties: raw.properties || {}}`
//!   — a schema without a `type` is REBUILT rather than passed through, because a backend rejects it.
//!   The satellite has no such normalisation and would forward the bare object.
//!
//! ## AND `sse` + `toSSE` ARE A **THIRD** COPY
//!
//! `sse` is byte-identical in three places now: here, in `zen-go-proxy/src/index.js` and in
//! `zen-us-proxy/src/index.js`. **`toSSE` is NOT identical in any of them** — the gateway's
//! `content_block_start` empties the block (`{...block, text: "", thinking: "", input: {}}`, round-96's
//! fix for a double-emit) and emits a `signature_delta` after a `thinking_delta`; the satellites send
//! the block as it is and no signature.
//!
//! **So the one function that IS the same is the one to share, and the one that differs is the one
//! that must stay.** `sse` below is the shared shape and lives here; the two satellites keep theirs
//! until a shared crate is worth the coupling. **Merging them would be a behaviour change dressed as
//! a refactor**, and this repository has a standing rule about that shape.
//!
//! ## `serde_json::Value` AGAIN, AND `preserve_order` AGAIN
//!
//! Same reason as the sibling crate: this is a SHAPE TRANSFORM, the key order is the wire order, and
//! `serde_json`'s default `Map` sorts keys alphabetically.

use serde_json::{json, Map, Value};

/// `sse(name, data)` — one frame, and the byte-identical third copy described in the module header.
pub fn sse(name: &str, data: &Value) -> String {
    format!(
        "event: {name}\ndata: {}\n\n",
        serde_json::to_string(data).expect("a Value is serialisable")
    )
}

/// JavaScript truthiness, spelled out — `truthy_js` in the sibling crate, and it is duplicated here
/// for the same reason `sse` is: **the two crates are autonomous satellites and the gateway, and a
/// shared crate for four lines is a coupling neither asked for.** It is the same eleven lines.
pub(crate) fn truthy_js(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// What `Array.prototype.join` does with one element — and both converters push `b.text`
/// UNCONDITIONALLY, so a `text` block with no `text` still COUNTS and joins to the empty string.
fn js_join_part(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// `typeof m.content === "string" ? m.content : m.content || []` — a STRING is used as it is
/// (INCLUDING THE EMPTY STRING), a TRUTHY non-string is itself, and a FALSY one is `[]`.
///
/// Returns `None` for the empty-array case, so the caller has nothing to walk.
fn effective_content(content: Option<&Value>) -> Option<&Value> {
    match content {
        Some(Value::String(_)) => content,
        Some(other) if truthy_js(other) => Some(other),
        _ => None,
    }
}

/// `for (const b of content)` as the THROW it is on a truthy non-iterable — the door throws rather
/// than differing quietly, and workers-rs turns a panic into the same 500 the `TypeError` produced.
fn non_iterable_content(v: &Value) -> ! {
    panic!(
        "gateway: message content {v} is neither a string nor an array, so it cannot be iterated"
    );
}

/// `convertUserMessage(m, messages)` — a user turn, and THE IMAGE PATH IS THE POINT.
///
/// **THE ORDER IS `flush()`, `flush()`, `flush()`**: a `tool_result` closes the pending text and
/// pushes its own `tool` message; an `image` closes the pending text and pushes an `image_url` part.
/// So a message of `[text, image, text]` becomes THREE parts in the array and the two text runs are
/// SEPARATE entries, joined per run — not one string with the image in the middle, which is what a
/// port that collected first and built after would produce.
///
/// **`if (data)` GUARDS THE IMAGE**: an image block with no `source.data` contributes NOTHING, not an
/// `image_url` with an empty data URL. And `b.source?.media_type || "image/png"` is a truthiness
/// default, so an empty or missing media type becomes `image/png` while `"0"` survives.
fn convert_user_message(turn: &Value, messages: &mut Vec<Value>) {
    let mut parts: Vec<Value> = Vec::new();
    let mut text_buf: Vec<String> = Vec::new();
    // `flush` IS A CLOSURE OVER `parts` AND `textBuf` IN THE JAVASCRIPT, so it can be called from
    // three places; here it is a function taking both, and the borrow is the same shape.
    fn flush(parts: &mut Vec<Value>, text_buf: &mut Vec<String>) {
        if !text_buf.is_empty() {
            parts.push(json!({ "type": "text", "text": text_buf.join("\n") }));
            text_buf.clear();
        }
    }

    match effective_content(turn.get("content")) {
        // **A STRING CONTENT IS PUSHED ONLY IF NON-EMPTY** — `if (content) messages.push(...)` — and
        // the FUNCTION ENDS there, so a string content never reaches the part-array path. **THE MATCH
        // IS THE FUNCTION'S LAST STATEMENT**, so an arm that has run is an arm that is done: there is
        // no `return` here and clippy was right to refuse the one the first version wrote.
        Some(Value::String(s)) => {
            if !s.is_empty() {
                messages.push(json!({ "role": "user", "content": s.clone() }));
            }
        }
        Some(Value::Array(blocks)) => {
            for b in blocks {
                if b.is_null() {
                    panic!(
                        "gateway: a content block is null, which the JavaScript reads .type off"
                    );
                }
                let kind = b.get("type").and_then(Value::as_str).unwrap_or_default();
                if kind == "tool_result" {
                    flush(&mut parts, &mut text_buf);
                    // `(b.content || []).map(c => c.text || c.thinking || "").join("\n")` — a chain of
                    // TRUTHINESS defaults, so a block with neither answers the empty string and a
                    // block whose `text` is `0` falls through to its `thinking`.
                    let tool_text = match b.get("content") {
                        Some(Value::String(s)) => s.clone(),
                        _ => {
                            let inner: Vec<String> = b
                                .get("content")
                                .and_then(Value::as_array)
                                .unwrap_or(&Vec::new())
                                .iter()
                                .map(|c| {
                                    if truthy_js(c.get("text").unwrap_or(&Value::Null)) {
                                        c.get("text").cloned().expect("just checked")
                                    } else if truthy_js(c.get("thinking").unwrap_or(&Value::Null)) {
                                        c.get("thinking").cloned().expect("just checked")
                                    } else {
                                        Value::String(String::new())
                                    }
                                })
                                .map(|v| match v {
                                    Value::String(s) => s,
                                    other => other.to_string(),
                                })
                                .collect();
                            inner.join("\n")
                        }
                    };
                    // **`tool_call_id: b.tool_use_id` IS OPTIONAL-BY-ABSENCE** — `JSON.stringify` drops
                    // a key whose value is `undefined`, so a tool result with no id has no
                    // `tool_call_id` at all. The satellite's port learned this the hard way; here it is
                    // written right the first time, with the reason attached.
                    let mut msg = Map::new();
                    msg.insert("role".into(), Value::String("tool".into()));
                    if let Some(id) = b.get("tool_use_id") {
                        msg.insert("tool_call_id".into(), id.clone());
                    }
                    msg.insert("content".into(), Value::String(tool_text));
                    messages.push(Value::Object(msg));
                } else if kind == "text" {
                    // UNCONDITIONAL: the part still counts when `text` is absent, and joins to "".
                    text_buf.push(js_join_part(b.get("text")));
                } else if kind == "image" {
                    flush(&mut parts, &mut text_buf);
                    let media_type = match b.get("source").and_then(|s| s.get("media_type")) {
                        Some(v) if truthy_js(v) => v.clone(),
                        _ => Value::String("image/png".into()),
                    };
                    let data = b
                        .get("source")
                        .and_then(|s| s.get("data"))
                        .filter(|d| truthy_js(d))
                        .cloned()
                        .unwrap_or(Value::String(String::new()));
                    // **`if (data)` IS A TRUTHINESS GUARD, NOT A PRESENCE CHECK** — so a block with no
                    // `source` at all contributes NOTHING, and this is the difference between a
                    // vision request and one the model receives with an empty data URL.
                    if truthy_js(&data) {
                        let url = format!(
                            "data:{};base64,{}",
                            match &media_type {
                                Value::String(s) => s.clone(),
                                other => other.to_string(),
                            },
                            match &data {
                                Value::String(s) => s.clone(),
                                other => other.to_string(),
                            }
                        );
                        parts.push(json!({
                            "type": "image_url",
                            "image_url": { "url": url },
                        }));
                    }
                }
            }
            flush(&mut parts, &mut text_buf);
            // **AN EMPTY PARTS ARRAY PRODUCES NO MESSAGE** — `if (parts.length)`.
            if !parts.is_empty() {
                messages.push(json!({ "role": "user", "content": parts }));
            }
        }
        Some(other) => non_iterable_content(other),
        // **AN EMPTY CONTENT AND A FALSY ONE BOTH PRODUCE NOTHING** — `effective_content`
        // answered `None` for both, and this arm is the whole of that case.
        None => {}
    }
}

/// `convertAssistantMessage(m, messages)` — thinking, text and tool calls, and it ALWAYS pushes.
///
/// It starts as `{role: "assistant", content: null}` and only gains the other three when there are
/// any, so a turn with no content is a message with a `null` content — a value the upstream expects.
fn convert_assistant_message(turn: &Value, messages: &mut Vec<Value>) {
    let mut msg = Map::new();
    msg.insert("role".into(), Value::String("assistant".into()));
    msg.insert("content".into(), Value::Null);
    let mut text_parts: Vec<String> = Vec::new();
    let mut think_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    match effective_content(turn.get("content")) {
        Some(Value::String(s)) => text_parts.push(s.clone()),
        Some(Value::Array(blocks)) => {
            for b in blocks {
                if b.is_null() {
                    panic!(
                        "gateway: a content block is null, which the JavaScript reads .type off"
                    );
                }
                // ALL THREE ARE PUSHED UNCONDITIONALLY and count even when the field is absent.
                match b.get("type").and_then(Value::as_str).unwrap_or_default() {
                    "thinking" => think_parts.push(js_join_part(b.get("thinking"))),
                    "text" => text_parts.push(js_join_part(b.get("text"))),
                    "tool_use" => {
                        // **`JSON.stringify(b.input || {})` RE-SERIALISES**, so the arguments travel as
                        // a STRING, and `input || {}` is a truthiness default — a falsy input becomes
                        // `{}` rather than that value. `id` and `name` are optional-by-absence.
                        let input = b
                            .get("input")
                            .filter(|v| truthy_js(v))
                            .cloned()
                            .unwrap_or_else(|| json!({}));
                        let mut call = Map::new();
                        if let Some(id) = b.get("id") {
                            call.insert("id".into(), id.clone());
                        }
                        call.insert("type".into(), Value::String("function".into()));
                        let mut func = Map::new();
                        if let Some(name) = b.get("name") {
                            func.insert("name".into(), name.clone());
                        }
                        func.insert(
                            "arguments".into(),
                            Value::String(serde_json::to_string(&input).expect("serialisable")),
                        );
                        call.insert("function".into(), Value::Object(func));
                        tool_calls.push(Value::Object(call));
                    }
                    _ => {}
                }
            }
        }
        Some(other) => non_iterable_content(other),
        None => {}
    }
    if !text_parts.is_empty() {
        msg.insert("content".into(), Value::String(text_parts.join("\n")));
    }
    if !think_parts.is_empty() {
        msg.insert(
            "reasoning_content".into(),
            Value::String(think_parts.join("\n")),
        );
    }
    if !tool_calls.is_empty() {
        msg.insert("tool_calls".into(), Value::Array(tool_calls));
    }
    messages.push(Value::Object(msg));
}

/// `toOpenAIRequest(req, model)` — the Anthropic request, as an OpenAI chat completion.
///
/// **AND `stream: !!req.stream` IS THE CALLER'S FLAG, DOUBLED.** A missing `stream` is `false`, a
/// truthy one is `true`, and a falsy-but-present one (`0`, `""`, `null`) is also `false` — so this is
/// neither of the two optional-field rules beside it, and it is the line the sibling worker does not
/// have.
pub fn to_openai_request(req: &Value, model: &str) -> Value {
    let mut messages: Vec<Value> = Vec::new();

    // ── the system prompt ───────────────────────────────────────────────────────────────────────
    match req.get("system").filter(|v| truthy_js(v)) {
        Some(Value::Array(blocks)) => {
            let text: Vec<String> = blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .map(|b| js_join_part(b.get("text")))
                .collect();
            // **`.join("\n")` ON AN EMPTY LIST IS `""`, AND `if (text)` IS A TRUTHINESS TEST** — so a
            // system array of no text blocks produces no message.
            let joined = text.join("\n");
            if !joined.is_empty() {
                messages.push(json!({ "role": "system", "content": joined }));
            }
        }
        Some(other) => {
            let text = match other {
                Value::String(s) => s.clone(),
                v => v.to_string(),
            };
            if !text.is_empty() {
                messages.push(json!({ "role": "system", "content": text }));
            }
        }
        None => {}
    }

    // ── the conversation ─────────────────────────────────────────────────────────────────────────
    // **`for (const m of req.messages || [])` ITERATES ANY ITERABLE**, so a string is its characters
    // and a truthy number throws. Both facts came from the sibling's differential, written here
    // rather than learned again.
    let turns: Vec<Value> = match req.get("messages") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.clone(),
        Some(Value::String(s)) => s.chars().map(|c| Value::String(c.to_string())).collect(),
        Some(other) => non_iterable_content(other),
    };
    for turn in &turns {
        match turn.get("role").and_then(Value::as_str).unwrap_or_default() {
            "user" => convert_user_message(turn, &mut messages),
            "assistant" => convert_assistant_message(turn, &mut messages),
            _ => {
                // **ANY OTHER ROLE IS COPIED, AND A NON-STRING CONTENT IS `JSON.stringify`'d** — and an
                // ABSENT content is an ABSENT KEY, since `JSON.stringify(undefined)` is `undefined`.
                let mut msg = Map::new();
                if let Some(r) = turn.get("role") {
                    msg.insert("role".into(), r.clone());
                }
                match turn.get("content") {
                    Some(Value::String(s)) => {
                        msg.insert("content".into(), Value::String(s.clone()));
                    }
                    Some(other) => {
                        msg.insert(
                            "content".into(),
                            Value::String(serde_json::to_string(other).expect("serialisable")),
                        );
                    }
                    None => {}
                }
                messages.push(Value::Object(msg));
            }
        }
    }

    // ── the envelope ──────────────────────────────────────────────────────────────────────────────
    let mut out = Map::new();
    out.insert("model".into(), Value::String(model.to_string()));
    out.insert("messages".into(), Value::Array(messages));
    // **`!!req.stream`**: absent, `null`, `0` and `""` are all `false`; a truthy value is `true`. The
    // key is ALWAYS present here, whatever the caller's flag was.
    out.insert(
        "stream".into(),
        Value::Bool(truthy_js(req.get("stream").unwrap_or(&Value::Null))),
    );
    // Three optional fields, three rules: `max_tokens` truthiness, `temperature`/`top_p` PRESENCE (so
    // a JSON `null` is forwarded, because a JSON document has no `undefined`).
    if truthy_js(req.get("max_tokens").unwrap_or(&Value::Null)) {
        out.insert(
            "max_tokens".into(),
            req.get("max_tokens").cloned().expect("just checked"),
        );
    }
    if let Some(t) = req.get("temperature") {
        out.insert("temperature".into(), t.clone());
    }
    if let Some(t) = req.get("top_p") {
        out.insert("top_p".into(), t.clone());
    }

    // ── the tools, WITH THE PARAMETERS NORMALISATION THIS WORKER HAS AND THE SATELLITE DOES NOT ───
    // **`req.tools?.length` IS TRUTHY ON A LENGTH**, so an empty array takes no branch and the
    // `tool_choice` nested inside it goes with it.
    if let Some(tools) = req.get("tools").and_then(Value::as_array) {
        if !tools.is_empty() {
            out.insert(
                "tools".into(),
                Value::Array(
                    tools
                        .iter()
                        .map(|t| {
                            // **`t.input_schema && typeof t.input_schema === "object"`** — so a schema
                            // that is a STRING, a NUMBER, an array or `null` becomes `{}`, and only a
                            // real object survives. Then `raw.type ? raw : {type, properties}`: a
                            // schema WITHOUT a `type` is REBUILT, because a backend rejects one. The
                            // satellite has no such step and forwards the bare object.
                            let raw = match t.get("input_schema") {
                                Some(Value::Object(o)) => Value::Object(o.clone()),
                                _ => json!({}),
                            };
                            let parameters = if truthy_js(raw.get("type").unwrap_or(&Value::Null)) {
                                raw.clone()
                            } else {
                                json!({
                                    "type": "object",
                                    "properties": match raw.get("properties") {
                                        Some(v) if truthy_js(v) => v.clone(),
                                        _ => json!({}),
                                    },
                                })
                            };
                            let mut func = Map::new();
                            if let Some(name) = t.get("name") {
                                func.insert("name".into(), name.clone());
                            }
                            func.insert(
                                "description".into(),
                                match t.get("description") {
                                    Some(v) if truthy_js(v) => v.clone(),
                                    _ => Value::String(String::new()),
                                },
                            );
                            func.insert("parameters".into(), parameters);
                            json!({ "type": "function", "function": Value::Object(func) })
                        })
                        .collect(),
                ),
            );
            // **`if (req.tool_choice)` IS TRUTHINESS**, so `null` and `""` insert nothing.
            if let Some(tc) = req.get("tool_choice").filter(|v| truthy_js(v)) {
                let kind = tc.get("type").and_then(Value::as_str).unwrap_or_default();
                let mapped = match kind {
                    "tool" => {
                        let mut func = Map::new();
                        if let Some(name) = tc.get("name") {
                            func.insert("name".into(), name.clone());
                        }
                        json!({ "type": "function", "function": Value::Object(func) })
                    }
                    "any" => Value::String("required".into()),
                    _ => Value::String("auto".into()),
                };
                out.insert("tool_choice".into(), mapped);
            }
        }
    }
    Value::Object(out)
}

/// `reasonTextOf(msg)` — the reasoning text, and there are THREE shapes.
///
/// **AND THE THIRD ONE JOINS WITH `""`, NOT `"\n"`** — `msg.reasoning_details.map(...).join("")` —
/// which is a different separator from every other join in this file, and a port that harmonised them
/// would change the text a thinking block carries.
///
/// A detail entry is a string, or an object with a `text`; a detail with neither joins as the empty
/// string. And the ORDER is `reasoning` first, then `reasoning_content` — so a message carrying both
/// answers with `reasoning`.
fn reason_text_of(msg: &Value) -> String {
    if let Some(s) = msg.get("reasoning").and_then(Value::as_str) {
        return s.to_string();
    }
    if let Some(s) = msg.get("reasoning_content").and_then(Value::as_str) {
        return s.to_string();
    }
    if let Some(details) = msg.get("reasoning_details").and_then(Value::as_array) {
        return details
            .iter()
            .map(|r| match r {
                Value::String(s) => s.clone(),
                other => match other.get("text") {
                    Some(v) if truthy_js(v) => match v {
                        Value::String(s) => s.clone(),
                        o => o.to_string(),
                    },
                    _ => String::new(),
                },
            })
            .collect::<Vec<String>>()
            .join("");
    }
    String::new()
}

/// `STREAM_STOP_MAP` — four keys, and an unknown or missing reason lands on `end_turn`.
const STREAM_STOP_MAP: [(&str, &str); 4] = [
    ("stop", "end_turn"),
    ("tool_calls", "tool_use"),
    ("function_call", "tool_use"),
    ("length", "max_tokens"),
];

/// `toAnthropicResponse(up, model)` — the OpenAI completion, as an Anthropic message.
///
/// **AND `cache_read_input_tokens` READS TWO PLACES.** `prompt_cache_hit_tokens ||
/// prompt_tokens_details?.cached_tokens || 0` — the satellite reads only the first, so a completion
/// that reports cache hits the OpenAI way answers `0` there and the real number here. **This is a
/// behaviour difference between two workers, and it is a FIX rather than a divergence**: the gateway
/// reads both so a cache hit surfaces to the client instead of always showing 0.
pub fn to_anthropic_response(up: &Value, model: &str) -> Value {
    let choice = up
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first());
    let msg = choice
        .and_then(|c| c.get("message"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut blocks: Vec<Value> = Vec::new();

    // THINKING, THEN TEXT, THEN TOOL CALLS — the wire order, whatever order the upstream wrote.
    let reason_text = reason_text_of(&msg);
    if !reason_text.is_empty() {
        blocks.push(json!({
            "type": "thinking",
            "thinking": reason_text,
            // **AN EMPTY SIGNATURE, NOT AN ABSENT KEY** — the Anthropic schema has the field.
            "signature": "",
        }));
    }
    if truthy_js(msg.get("content").unwrap_or(&Value::Null)) {
        blocks.push(json!({
            "type": "text",
            "text": msg.get("content").cloned().expect("just checked"),
        }));
    }
    for tc in msg
        .get("tool_calls")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
    {
        // **A MALFORMED `arguments` STRING BECOMES `{}` AND IS NOT AN ERROR** — a model emits whatever
        // it emits, and the upstream must not be taken down by a tool call it cannot run.
        let arguments = tc
            .get("function")
            .and_then(|f| f.get("arguments"))
            .and_then(Value::as_str)
            .unwrap_or("{}");
        let input: Value = serde_json::from_str(arguments).unwrap_or_else(|_| json!({}));
        // `id` is OPTIONAL-BY-ABSENCE and `name` is a TRUTHINESS DEFAULT to "unknown".
        let mut block = Map::new();
        block.insert("type".into(), Value::String("tool_use".into()));
        if let Some(id) = tc.get("id") {
            block.insert("id".into(), id.clone());
        }
        block.insert(
            "name".into(),
            match tc.get("function").and_then(|f| f.get("name")) {
                Some(v) if truthy_js(v) => v.clone(),
                _ => Value::String("unknown".into()),
            },
        );
        block.insert("input".into(), input);
        blocks.push(Value::Object(block));
    }

    let stop_reason = match choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(Value::as_str)
    {
        Some(key) => STREAM_STOP_MAP
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| *v)
            .unwrap_or("end_turn"),
        None => "end_turn",
    };

    // **`id` IS OPTIONAL-BY-ABSENCE**, so the object is BUILT rather than written as a literal — the
    // same five places, and the same reason, as the sibling crate.
    let mut out = Map::new();
    if let Some(id) = up.get("id") {
        out.insert("id".into(), id.clone());
    }
    out.insert("type".into(), Value::String("message".into()));
    out.insert("role".into(), Value::String("assistant".into()));
    out.insert("model".into(), Value::String(model.to_string()));
    out.insert("content".into(), Value::Array(blocks));
    out.insert("stop_reason".into(), Value::String(stop_reason.into()));
    out.insert("stop_sequence".into(), Value::Null);
    out.insert(
        "usage".into(),
        json!({
            "input_tokens": usage_int(up, "prompt_tokens"),
            "output_tokens": usage_int(up, "completion_tokens"),
            "cache_creation_input_tokens": 0,
            "cache_read_input_tokens": usage_cache_read(up),
        }),
    );
    Value::Object(out)
}

/// `up.usage?.prompt_tokens || 0` — a truthiness default, and **the number is passed through rather
/// than rounded**, because a provider that reports a float is reporting it today.
fn usage_int(up: &Value, key: &str) -> Value {
    match up.get("usage").and_then(|u| u.get(key)) {
        Some(Value::Number(n)) if n.as_f64().is_some_and(|f| f != 0.0) => Value::Number(n.clone()),
        _ => Value::from(0),
    }
}

/// `up.usage?.prompt_cache_hit_tokens || up.usage?.prompt_tokens_details?.cached_tokens || 0`.
///
/// **TWO SOURCES AND THE FIRST TRUTHY ONE WINS**, so a completion reporting both answers with the
/// first — and a completion reporting neither, or a zero in the first, answers `0`.
fn usage_cache_read(up: &Value) -> Value {
    let first = up
        .get("usage")
        .and_then(|u| u.get("prompt_cache_hit_tokens"));
    if truthy_js(first.unwrap_or(&Value::Null)) {
        return first.cloned().expect("just checked");
    }
    let second = up
        .get("usage")
        .and_then(|u| u.get("prompt_tokens_details"))
        .and_then(|d| d.get("cached_tokens"));
    if truthy_js(second.unwrap_or(&Value::Null)) {
        return second.cloned().expect("just checked");
    }
    Value::from(0)
}

/// `toSSE(res)` — the whole non-streaming Anthropic message, as a sequence of SSE frames.
///
/// **THREE THINGS HERE ARE NOT IN THE SATELLITES' VERSION, AND ALL THREE ARE FIXES**:
///
///  * **`content_block_start` CARRIES AN EMPTIED BLOCK** — `{...block, text: "", thinking: "", input: {}}`
///    (round-96's fix for a double-emit: the block's content was ALSO sent as the first delta). So the
///    block's `type` survives and its payload does not.
///  * **A THINKING BLOCK EMITS TWO DELTAS** — `thinking_delta` and then `signature_delta`, the second
///    carrying `block.signature || ""` so an unsigned block still sends the field.
///  * **A `server_tool_use` BLOCK HAS ITS OWN ARM** and its `partial_json` is
///    `JSON.stringify(block.input || {})` — a truthiness default, so a `server_tool_use` with no input
///    sends `{}` rather than `undefined` (which would throw inside `JSON.stringify`).
///
/// **AND AN UNKNOWN BLOCK TYPE GETS A START AND A STOP AND NO DELTA**, exactly as in the satellites.
/// `{ stop_reason: res.stop_reason, stop_sequence: null }` — with `undefined` DROPPED, as
/// `JSON.stringify` drops it.
fn delta_of(res: &Value) -> Value {
    let mut delta = serde_json::Map::new();
    if let Some(reason) = res.get("stop_reason") {
        delta.insert("stop_reason".to_string(), reason.clone());
    }
    delta.insert("stop_sequence".to_string(), Value::Null);
    Value::Object(delta)
}

pub fn to_sse(res: &Value) -> String {
    // **THE TYPESCRIPT ITERATES `res.content` AND CRASHES WHEN IT IS ABSENT.** Measured by the oracle
    // (`gateway/wasm/oracle-translate.mjs`), on the case named "no content":
    //
    //     Cannot read properties of undefined (reading 'forEach')
    //
    // and the corpus's rule is the one this repository states for every port — BOTH REFUSING IS AN
    // EQUIVALENCE, and a port that quietly accepted it would answer a document the JavaScript never
    // produces. So this refuses in the only way Rust has, which under workerd is the same 500 the
    // throw is. It is a degenerate input (the route builds `res` from an Anthropic-shaped document, so
    // it always has `content`), which is exactly why the hand-written tests never found it.
    if res.get("content").is_none() {
        panic!("toSSE: content is required (the TypeScript throws here)");
    }
    let mut start_message = res.clone();
    if let Some(obj) = start_message.as_object_mut() {
        // THE SPREAD IS SHALLOW AND THREE KEYS ARE REPLACED — the id, model, role, type and usage
        // survive, and the content is dropped wholesale.
        obj.insert("content".into(), Value::Array(Vec::new()));
        obj.insert("stop_reason".into(), Value::Null);
        obj.insert("stop_sequence".into(), Value::Null);
    }

    let mut out = sse(
        "message_start",
        &json!({ "type": "message_start", "message": start_message }),
    );
    let blocks = res
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (i, block) in blocks.iter().enumerate() {
        // **THE START BLOCK IS THE BLOCK WITH ITS PAYLOAD EMPTIED** — `type` and any other key
        // survive, and `text`, `thinking` and `input` are overwritten, so a block that had no `input`
        // GAINS one.
        let mut start_block = block.clone();
        if let Some(obj) = start_block.as_object_mut() {
            obj.insert("text".into(), Value::String(String::new()));
            obj.insert("thinking".into(), Value::String(String::new()));
            obj.insert("input".into(), json!({}));
        }
        out += &sse(
            "content_block_start",
            &json!({ "type": "content_block_start", "index": i, "content_block": start_block }),
        );
        let kind = block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let delta = |d: Value| {
            sse(
                "content_block_delta",
                &json!({ "type": "content_block_delta", "index": i, "delta": d }),
            )
        };
        match kind {
            "thinking" => {
                out += &delta(json!({
                    "type": "thinking_delta",
                    "thinking": block.get("thinking").cloned().unwrap_or(Value::Null),
                }));
                // **THE SIGNATURE DELTA IS A SECOND FRAME, NOT PART OF THE FIRST** — and its value is
                // `block.signature || ""`, a truthiness default.
                out += &delta(json!({
                    "type": "signature_delta",
                    "signature": match block.get("signature") {
                        Some(v) if truthy_js(v) => v.clone(),
                        _ => Value::String(String::new()),
                    },
                }));
            }
            "text" => {
                out += &delta(json!({
                    "type": "text_delta",
                    "text": block.get("text").cloned().unwrap_or(Value::Null),
                }))
            }
            "tool_use" => {
                // **`partial_json` IS OPTIONAL-BY-ABSENCE, AND THIS IS THE SIXTH PLACE THAT RULE
                // SHOWS UP.** `JSON.stringify(block.input)` is `undefined` for a block with no
                // `input`, and `JSON.stringify` DROPS a key whose value is `undefined`, so the delta
                // object carries NO `partial_json` at all. **The first version serialised
                // `Value::Null` and sent `"partial_json":null`**, which is a different frame: a client
                // that parses the field gets a parse error on `null` where it should have seen its own
                // "no arguments" shape.
                //
                // **AND THE SIBLING ARM IS ONE CHARACTER OF LOGIC AWAY** — `block.input || {}` sends
                // `{}` where this sends nothing, and the test below pins both.
                let mut d = Map::new();
                d.insert("type".into(), Value::String("input_json_delta".into()));
                if let Some(input) = block.get("input") {
                    d.insert(
                        "partial_json".into(),
                        Value::String(serde_json::to_string(input).expect("serialisable")),
                    );
                }
                out += &delta(Value::Object(d))
            }
            // **THE SERVER-TOOL ARM DIFFERS FROM THE TOOL ARM IN ONE CHARACTER'S WORTH OF LOGIC**:
            // `block.input || {}` against a bare `block.input`, so a `server_tool_use` with no input
            // sends `{}` instead of serialising `null`.
            "server_tool_use" => {
                out += &delta(json!({
                    "type": "input_json_delta",
                    "partial_json": serde_json::to_string(&match block.get("input") {
                        Some(v) if truthy_js(v) => v.clone(),
                        _ => json!({}),
                    })
                    .expect("serialisable"),
                }))
            }
            // **AN UNKNOWN BLOCK TYPE GETS A START AND A STOP AND NO DELTA** — the chain has no
            // `else`, and a client that expects one delta per block will notice.
            _ => {}
        }
        out += &sse(
            "content_block_stop",
            &json!({ "type": "content_block_stop", "index": i }),
        );
    }
    out += &sse(
        "message_delta",
        &json!({
            "type": "message_delta",
            // **A KEY WHOSE VALUE IS `undefined` IS DROPPED BY `JSON.stringify`, AND THAT IS A BYTE.**
            // The TypeScript writes `delta: { stop_reason: res.stop_reason, stop_sequence: null }`, so a
            // response with NO `stop_reason` produces `{"stop_sequence":null}` — the corpus measured it,
            // and `unwrap_or(Value::Null)` kept the key and changed the bytes. An EXPLICIT `null` is a
            // value and keeps its key, which is why this asks whether the key is there rather than
            // whether it is null.
            "delta": delta_of(res),
            // `res.usage?.output_tokens || 0` — OPTIONAL CHAINING, so a response with no `usage`
            // answers 0 here (and would throw in the satellites' version, which uses a bare `.`).
            "usage": { "output_tokens": match res.get("usage").and_then(|u| u.get("output_tokens")) {
                Some(v) if truthy_js(v) => v.clone(),
                _ => Value::from(0),
            } },
        }),
    );
    out += &sse("message_stop", &json!({ "type": "message_stop" }));
    out
}

#[cfg(test)]
// THE NAMES SHOUT WHERE THE BEHAVIOUR IS THE OPPOSITE OF WHAT A READER EXPECTS: a zero that is kept,
// a null that is forwarded, an image that is dropped, a block that is emptied twice. The capitals
// are the note, the same as the other three crates' tests.
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn an_IMAGE_with_no_data_contributes_NOTHING_and_one_with_data_keeps_its_place() {
        // **A VISION REQUEST IS A PRODUCT FEATURE AND THE GUARD IS `if (data)`** — a block with no
        // `source`, or an empty `source.data`, adds no part at all. A port that emitted
        // `data:image/png;base64,` would send a broken image and get a model error instead of text.
        let no_data = json!({ "messages": [{ "role": "user", "content": [
            { "type": "text", "text": "what is this" },
            { "type": "image", "source": { "media_type": "image/jpeg" } },
        ] }] });
        let out = to_openai_request(&no_data, "m");
        // **THE PARTS ARRAY HAS ONE ENTRY LEFT IN IT** — the text run — so the assertion is on the
        // ARRAY and not on its first element. The first version compared `[0]` with the whole array and
        // reported a difference that was an indexing mistake in the test.
        let content = &out["messages"][0]["content"];
        assert_eq!(
            content,
            &json!([{ "type": "text", "text": "what is this" }]),
            "the image is dropped and the text survives: {out}"
        );

        // AND WITH DATA IT IS A `data:` URL, AND THE MEDIA TYPE'S TRUTHINESS DEFAULT IS `image/png`.
        let with_data = json!({ "messages": [{ "role": "user", "content": [
            { "type": "image", "source": { "media_type": "", "data": "QUJD" } },
        ] }] });
        let out2 = to_openai_request(&with_data, "m");
        assert_eq!(
            out2["messages"][0]["content"][0]["image_url"]["url"],
            json!("data:image/png;base64,QUJD"),
            "{out2}"
        );
    }

    #[test]
    fn a_FLUSH_between_text_runs_MAKES_THREE_PARTS_not_one_string_with_an_image_inside() {
        // **THE ORDER IS THE `flush()` CALLS**, and this is the part the sibling has no equivalent of.
        // `[text, image, text]` is THREE array entries: the two text runs are separate parts, each
        // joined per run, with the image between them. A port that collected the text first and built
        // the array after would produce TWO parts and put the image last.
        let req = json!({ "messages": [{ "role": "user", "content": [
            { "type": "text", "text": "before" },
            { "type": "image", "source": { "media_type": "image/png", "data": "QQ==" } },
            { "type": "text", "text": "after" },
        ] }] });
        let out = to_openai_request(&req, "m");
        let parts = out["messages"][0]["content"].as_array().expect("an array");
        assert_eq!(parts.len(), 3, "three parts: {out}");
        assert_eq!(parts[0], json!({ "type": "text", "text": "before" }));
        assert_eq!(parts[1]["type"], json!("image_url"));
        assert_eq!(parts[2], json!({ "type": "text", "text": "after" }));
    }

    #[test]
    fn the_STREAM_flag_is_THE_CALLERS_and_it_is_DOUBLED() {
        // **A THIRD RULE, BESIDE THE TWO OPTIONAL-FIELD ONES.** `!!req.stream`: absent is false, a
        // truthy value is true, and so is a TRUTHY STRING — `"false"` is `true` here, which is the
        // difference between `!!` and a boolean check.
        // THE FLAG LIST IS `[(&str, Value, bool)]` AND A MISSING FLAG IS A `None` STRING — because a
        // heterogeneous array of `Option<Value>` and `Value` is not a thing Rust will type, and the
        // first version tried.
        for (label, flag, want) in [
            ("absent", None, false),
            ("true", Some(json!(true)), true),
            ("1", Some(json!(1)), true),
            ("\"false\"", Some(json!("false")), true),
            ("\"\"", Some(json!("")), false),
            ("0", Some(json!(0)), false),
            ("null", Some(json!(null)), false),
            ("{}", Some(json!({})), true),
        ] {
            let mut req = json!({ "messages": [] });
            if let Some(f) = flag {
                req["stream"] = f;
            }
            let out = to_openai_request(&req, "m");
            assert_eq!(out["stream"], json!(want), "for a {label} flag: {out}");
            // AND THE KEY IS ALWAYS THERE, whatever the caller's flag was.
            assert!(out.as_object().expect("an object").contains_key("stream"));
        }
    }

    #[test]
    fn a_TOOL_SCHEMA_without_a_type_is_REBUILT_and_a_non_object_schema_becomes_an_empty_one() {
        // **THIS WORKER HAS A STEP THE SATELLITE DOES NOT**, and it is a FIX: a backend rejects a
        // function tool whose parameters have no `type`, so a bare `{properties}` is rebuilt as
        // `{type:"object", properties}` and anything that is not an object becomes `{}` and is then
        // rebuilt the same way.
        let cases: &[(&str, Value)] = &[
            (
                r#"{"name":"t","input_schema":{"type":"object","properties":{"a":{"type":"string"}}}}"#,
                json!({ "type": "object", "properties": { "a": { "type": "string" } } }),
            ),
            // NO `type` → REBUILT, keeping the properties.
            (
                r#"{"name":"t","input_schema":{"properties":{"a":1}}}"#,
                json!({ "type": "object", "properties": { "a": 1 } }),
            ),
            // ABSENT → `{}` → REBUILT to an empty object.
            (
                r#"{"name":"t"}"#,
                json!({ "type": "object", "properties": {} }),
            ),
            // A STRING SCHEMA IS NOT AN OBJECT → `{}` → rebuilt.
            (
                r#"{"name":"t","input_schema":"nope"}"#,
                json!({ "type": "object", "properties": {} }),
            ),
            // `properties: 0` is FALSY, so the rebuild's own default answers `{}`.
            (
                r#"{"name":"t","input_schema":{"properties":0}}"#,
                json!({ "type": "object", "properties": {} }),
            ),
        ];
        for (tool, want) in cases {
            // **THE CASES ARE A SINGLE TOOL, NOT A REQUEST** — the first version parsed them as a whole
            // request, so `tools` was absent, `out["tools"]` was null, and every case reported a
            // difference that was the TEST building the wrong document. A test that constructs its
            // input wrongly fails in the same direction as a port that reads it wrongly, which is why
            // the message names the input.
            let tool: Value = serde_json::from_str(tool).expect("JSON");
            let out = to_openai_request(&json!({ "messages": [], "tools": [tool.clone()] }), "m");
            assert_eq!(
                out["tools"][0]["function"]["parameters"], *want,
                "for a tool of {tool}"
            );
        }
    }

    #[test]
    fn the_THREE_reasoning_shapes_are_tried_IN_ORDER_and_the_third_joins_with_nothing() {
        // **`reasoning` BEFORE `reasoning_content`**, so a message carrying both answers with the
        // first — and the segment list joins with `""` where every other join in this file is `"\n"`.
        for (msg, want) in [
            (json!({ "reasoning": "a", "reasoning_content": "b" }), "a"),
            (json!({ "reasoning_content": "b" }), "b"),
            (json!({ "reasoning": 5, "reasoning_content": "b" }), "b"),
            (
                json!({ "reasoning_details": ["x", { "text": "y" }, { "text": 0 }, {}] }),
                "xy",
            ),
            (json!({ "reasoning_details": "not an array" }), ""),
            (json!({}), ""),
            (json!({ "reasoning": "" }), ""),
        ] {
            let out = to_anthropic_response(
                &json!({ "choices": [{ "message": msg.clone(), "finish_reason": "stop" }] }),
                "m",
            );
            if want.is_empty() {
                // **AN EMPTY REASONING PRODUCES NO THINKING BLOCK** — `if (reasonText)`.
                assert_eq!(
                    out["content"].as_array().expect("an array").len(),
                    0,
                    "for {msg}"
                );
            } else {
                assert_eq!(out["content"][0]["thinking"], json!(want), "for {msg}");
            }
        }
    }

    #[test]
    fn cache_read_INPUT_tokens_READS_TWO_PLACES_and_the_first_TRUTHY_one_wins() {
        // **THE GATEWAY FIXED WHAT THE SATELLITE STILL GETS WRONG**: zen reads only
        // `prompt_cache_hit_tokens`, so an OpenAI-shaped completion answers 0 there and the real
        // number here. A zero in the FIRST place falls through to the second — it is a truthiness
        // chain, not a presence check.
        // **THE EXPECTED VALUES ARE `Value`s, NOT INTEGERS**, and that is not tidiness: one case
        // answers the STRING `"8"`, so a list inferred as `(Value, i64)` cannot hold it. The first
        // version wrote bare integers and the compiler named the type.
        for (usage, want) in [
            (json!({ "prompt_cache_hit_tokens": 7 }), json!(7)),
            (
                json!({ "prompt_tokens_details": { "cached_tokens": 9 } }),
                json!(9),
            ),
            // BOTH → the first wins.
            (
                json!({ "prompt_cache_hit_tokens": 7, "prompt_tokens_details": { "cached_tokens": 9 } }),
                json!(7),
            ),
            // A ZERO IN THE FIRST FALLS THROUGH, because the chain is `||`.
            (
                json!({ "prompt_cache_hit_tokens": 0, "prompt_tokens_details": { "cached_tokens": 9 } }),
                json!(9),
            ),
            (json!({}), json!(0)),
            // **A STRING IS FORWARDED AS A STRING** — the chain is `||`, so "8" is truthy and becomes
            // the value, and the Anthropic schema wanting a number is not this function's business.
            (json!({ "prompt_cache_hit_tokens": "8" }), json!("8")),
        ] {
            let out = to_anthropic_response(
                &json!({ "choices": [{ "message": { "content": "x" } }], "usage": usage.clone() }),
                "m",
            );
            assert_eq!(
                out["usage"]["cache_read_input_tokens"],
                json!(want),
                "for {usage}"
            );
        }
    }

    #[test]
    fn the_SSE_start_block_is_EMPTIED_and_a_thinking_block_emits_TWO_deltas() {
        // **ROUND-96'S DOUBLE-EMIT FIX, TWICE OVER.** The start block keeps the block's `type` and
        // loses its payload — and GAINS `input` even if it had none — while the payload arrives in the
        // deltas. And a thinking block sends `thinking_delta` and then `signature_delta` as SEPARATE
        // frames, which is one frame more than the satellites' version produces.
        let res = to_anthropic_response(
            &json!({ "id": "x", "choices": [{ "message": { "reasoning_content": "th" }, "finish_reason": "stop" }] }),
            "m",
        );
        let out = to_sse(&res);
        let first = out.split("event: ").nth(1).expect("a first frame");
        let data: Value =
            serde_json::from_str(first.split("data: ").nth(1).expect("a data").trim_end())
                .expect("JSON");
        assert_eq!(
            data["message"]["content"]
                .as_array()
                .expect("an array")
                .len(),
            0
        );
        // A THINKING BLOCK: start + TWO deltas + stop, plus message_start/delta/stop.
        assert_eq!(
            out.matches("event: content_block_delta").count(),
            2,
            "{out}"
        );
        assert!(out.contains("thinking_delta"), "{out}");
        assert!(
            out.contains("signature_delta"),
            "and the signature is its own frame: {out}"
        );
        // AND THE START BLOCK'S PAYLOAD IS EMPTY.
        // `.nth(1)` ANSWERS AN ITERATOR, NOT A STRING — so the second split needs `.next()` to reach
        // the text. The first version indexed the `Split` and the compiler named the type.
        // **A FRAME IS FOUND BY ITS OWN BOUNDARIES, NOT BY CHAINING SPLITS.** `split(..).nth(1)`
        // answers an ITERATOR, so the second split needs `.next()` — and the first version indexed a
        // `Split` and then split the wrong piece, which surfaced as `EOF while parsing a value` two
        // lines from the cause. The offsets are the honest way to read a frame.
        let at = out
            .find("event: content_block_start")
            .expect("a start frame");
        let data_at = out[at..].find("data: ").expect("a data line") + at + "data: ".len();
        let end_at = out[data_at..].find("\n\n").expect("the frame's blank line") + data_at;
        let start: Value =
            serde_json::from_str(&out[data_at..end_at]).expect("the frame body is JSON");
        let block = &start["content_block"];
        assert_eq!(
            block["type"],
            json!("thinking"),
            "the type survives: {start}"
        );
        assert_eq!(
            block["thinking"],
            json!(""),
            "the payload does not: {start}"
        );
        assert_eq!(
            block["signature"],
            json!(""),
            "and the signature is emptied too: {start}"
        );
    }

    #[test]
    fn a_SERVER_TOOL_USE_with_NO_input_sends_an_EMPTY_OBJECT_while_a_TOOL_USE_sends_NO_KEY() {
        // **ONE CHARACTER OF LOGIC APART AND A DIFFERENT FRAME ON THE WIRE.** `partial_json` is
        // OPTIONAL-BY-ABSENCE for a `tool_use` — `JSON.stringify(undefined)` is `undefined`, and
        // `JSON.stringify` drops a key whose value is `undefined`, so a tool call with no arguments
        // carries NO `partial_json` — while the `server_tool_use` arm writes `block.input || {}` and
        // therefore always carries one.
        //
        // **AND THE FIRST VERSION OF THIS TEST ASSERTED THE OPPOSITE**, because it was written against
        // the port's first version, which serialised `Value::Null` and sent `"partial_json":null`. A
        // client that parses the field gets a parse error on `null` where it should have seen its own
        // "no arguments" shape — so the test was pinning the bug, which is the one thing a test must
        // never do.
        let tool_use = json!({
            "id": "x", "content": [{ "type": "tool_use", "id": "t" }],
            "stop_reason": "end_turn", "usage": { "output_tokens": 1 },
        });
        let out = to_sse(&tool_use);
        assert!(
            !out.contains("partial_json"),
            "a tool_use with no input sends NO partial_json key: {out}"
        );
        let server_tool_use = json!({
            "id": "x", "content": [{ "type": "server_tool_use", "id": "s" }],
            "stop_reason": "end_turn", "usage": { "output_tokens": 1 },
        });
        let out2 = to_sse(&server_tool_use);
        assert!(
            out2.contains(r#""partial_json":"{}""#),
            "a server_tool_use with no input sends an empty object: {out2}"
        );
    }

    #[test]
    fn a_STRING_content_is_pushed_ONLY_IF_NON_EMPTY_and_a_STRING_MESSAGES_ITERATES_AS_CHARS() {
        // `if (content) messages.push({role:"user", content})` — an empty string content produces NO
        // message and RETURNS, so it never reaches the part-array path.
        let empty = json!({ "messages": [{ "role": "user", "content": "" }] });
        assert_eq!(
            to_openai_request(&empty, "m")["messages"]
                .as_array()
                .expect("an array")
                .len(),
            0
        );
        // AND THE STRING-ITERATION RULE, which the sibling's differential found and this port writes
        // right the first time.
        let chars = json!({ "messages": "ab" });
        let msgs = to_openai_request(&chars, "m")["messages"]
            .as_array()
            .expect("an array")
            .clone();
        assert_eq!(msgs.len(), 2, "one message per character: {msgs:?}");
    }
}

#[cfg(test)]
mod oracle_corpus {
    //! `fixtures/translate-corpus.json` was produced by the SHIPPING TypeScript
    //! (`gateway/wasm/oracle-translate.mjs`), and this replays it.
    //!
    //! THE COMPARISON IS THE JSON TEXT, NOT THE PARSED VALUE: `Object.keys` order is what the response
    //! bytes are, this crate carries `preserve_order`, and comparing parsed values would quietly accept a
    //! reordered document.
    //!
    //! A CASE THE TYPESCRIPT THROWS ON IS A CASE THIS MUST REFUSE, and "both refuse" is asserted with
    //! `catch_unwind` — the only refusal Rust has. That arm is not decoration: the FIRST run of this
    //! corpus found `to_sse` answering a document where the TypeScript crashes, which no hand-written
    //! test had asked about.
    use super::*;

    fn corpus() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/translate-corpus.json"
        );
        let text = std::fs::read_to_string(path).expect("the corpus is committed");
        serde_json::from_str(&text).expect("the corpus parses")
    }

    #[test]
    fn every_case_matches_the_shipping_typescript() {
        let doc = corpus();
        let cases = doc["cases"].as_array().expect("cases");
        assert!(
            cases.len() >= 15,
            "a corpus that shrank is a corpus that stopped covering"
        );
        let mut checked = 0;
        for case in cases {
            let name = case["name"].as_str().unwrap_or("?");
            let func = case["fn"].as_str().unwrap_or("?");
            let input = &case["input"];
            let model = case["model"].as_str().unwrap_or("m");
            let expected = &case["expected"];
            let throws = expected.get("threw").is_some();
            // THE TWO CORPUS TESTS ARE DISJOINT: `redact.rs` owns its own functions, and this one SKIPS
            // them rather than refusing them. (The first version panicked on an unknown name, which the
            // `catch_unwind` below then read as "this refused where the TypeScript answered" — a test
            // reporting its own overlap as a port defect.)
            if !matches!(
                func,
                "to_openai_request" | "to_anthropic_response" | "sse" | "to_sse"
            ) {
                continue;
            }

            // The call, captured as a refusal rather than a value when it panics.
            let outcome: Result<String, ()> = std::panic::catch_unwind(|| match func {
                "to_openai_request" => {
                    serde_json::to_string(&to_openai_request(input, model)).unwrap()
                }
                "to_anthropic_response" => {
                    serde_json::to_string(&to_anthropic_response(input, model)).unwrap()
                }
                "sse" => sse(case["event"].as_str().unwrap_or(""), input),
                "to_sse" => to_sse(input),
                other => panic!("unknown function in the corpus: {other}"),
            })
            .map_err(|_| ());

            match (throws, outcome) {
                (true, Err(())) => {
                    checked += 1;
                }
                (true, Ok(value)) => panic!(
                    "{func} / {name}: the TypeScript THROWS and this answered {value:.120} — both \
                     refusing is the equivalence the corpus asks for"
                ),
                (false, Err(())) => {
                    panic!("{func} / {name}: this refused where the TypeScript answered")
                }
                (false, Ok(value)) => {
                    let want = expected
                        .get("json")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                        .or_else(|| {
                            expected
                                .get("text")
                                .and_then(|v| v.as_str())
                                .map(str::to_string)
                        })
                        .unwrap_or_else(|| {
                            panic!("{func} / {name}: the corpus has no expectation")
                        });
                    assert_eq!(value, want, "{func} / {name}: the bytes differ");
                    checked += 1;
                }
            }
        }
        // 19 of the corpus's cases are this module's; the rest belong to `redact.rs`, which has its own
        // test. The floor is what keeps this from passing vacuously if the corpus is regenerated wrong.
        assert_eq!(checked, 19, "the translate half of the corpus changed size");
    }
}
