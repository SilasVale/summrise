//! `zen-go-proxy`'s portable half — the Anthropic↔OpenAI translation, the SSE encoder, and the
//! shared policy family.
//!
//! ## What is here, and what the shape of the file says
//!
//! The 403 lines of `proxies/zen-go-proxy/src/index.js` split into three kinds of thing, and the two
//! portable kinds are in this file:
//!
//!  * **the I/O** — the upstream fetch, the streaming relay, the BYOK key, the Durable Object that
//!    carries placement. Untouched, and they stay in JavaScript until the entry point is ported.
//!  * **the policy** — `isLoopbackOrigin`, `isLoopbackHost`, `requestHost`, `corsHeaders`,
//!    `redactSecrets`, `jsonError`. **These are the SAME FUNCTIONS AS `zen-us-proxy`'s, byte for
//!    byte in the JavaScript**, and this crate depends on that crate rather than re-typing them: the
//!    two satellite workers are autonomous by ADR 0003, and a rule that exists twice in one language
//!    is the shape that lets them drift — which they already have, in the `[::1]` case the sibling
//!    crate's header documents.
//!  * **the translation** — `toOpenAIRequest`, `toAnthropicResponse`, `sse`, `toSSE`. This is the
//!    part that is actually specific to zen-go, and it is where the interesting traps are.
//!
//! ## THE TWO RULES THAT DECIDE WHETHER A PORT IS CORRECT
//!
//! * **`JSON.parse` OF A MODEL'S TOOL ARGUMENTS, WITH A `{}` DEFAULT AND A SWALLOWED ERROR.** A model
//!   emits whatever it emits; a malformed `arguments` string must not take the request down, and the
//!   answer is an empty object, not an error and not a re-serialised string.
//! * **AN OPTIONAL FIELD IS COPIED ONLY WHEN THE PROPERTY IS PRESENT** — and that is `!== undefined`,
//!   not truthiness. `temperature: 0` and `max_tokens: 0` are real values a caller sends, and a port
//!   that writes `if (req.temperature)` drops both silently. The same distinction separates
//!   `usage.prompt_tokens || 0` (truthiness is right there) from `req.temperature !== undefined`
//!   (truthiness would be wrong).
//!
//! ## `serde_json::Value` AND NOT A STRUCT, and why
//!
//! The translation is a SHAPE TRANSFORM, not a validation: the request comes from a client and the
//! response from a model, and a struct with named fields would reject the parts this function passes
//! through untouched while a `Map` loses the parts it does not name. So the input is a
//! `serde_json::Value` and the output is a `Value`, with the SAME key order the JavaScript's object
//! literals produce — `serde_json` preserves insertion order only with the `preserve_order` feature,
//! and **this crate asks for it**, because the response is signed by nothing but compared by bytes.

use serde_json::{json, Map, Value};

pub use summrise_zen_us::cors_headers;
pub use summrise_zen_us::{is_loopback_host, is_loopback_origin, redact_secrets, request_host};

/// `toOpenAIRequest(req, model)` — the Anthropic request, as an OpenAI chat completion.
///
/// **THE ORDER OF `messages` IS THE CONTRACT**, and three rules decide it:
///
///  * a `system` that is an ARRAY contributes only its `text` blocks, joined with `\n`; a string
///    contributes itself; **an empty system contributes NOTHING** (no empty message), because a
///    provider rejects an empty content string;
///  * a `user` turn's `tool_result` blocks become their OWN `tool` messages — pushed DURING the walk
///    over the content blocks, which is why a `tool` message can appear BETWEEN the text the user
///    sent and the text they sent after it; **and a user turn that contributed ONLY tool results
///    contributes no `user` message at all** (`textParts.length` is the test, not "did we see a user
///    turn");
///  * an `assistant` turn ALWAYS produces a message, even with no content: it starts as
///    `{role, content: null}` and only gains `content`, `reasoning_content` and `tool_calls` when
///    there are any. **A `null` content is a value the upstream is expected to receive**, and dropping
///    the key would change the wire format.
pub fn to_openai_request(req: &Value, model: &str) -> Value {
    let mut messages: Vec<Value> = Vec::new();

    // ── the system prompt ───────────────────────────────────────────────────────────────────────
    match req.get("system") {
        Some(Value::Array(blocks)) => {
            // ONLY `text` BLOCKS, joined with a newline, and the array is filtered BEFORE it is
            // joined — an empty array joins to `""`, which is falsy, so no message is emitted.
            let text: Vec<String> = blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .map(|b| {
                    b.get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                })
                .collect();
            if !text.is_empty() {
                messages.push(json!({ "role": "system", "content": text.join("\n") }));
            }
        }
        Some(Value::String(s)) if !s.is_empty() => {
            messages.push(json!({ "role": "system", "content": s }));
        }
        // A STRING SYSTEM THAT IS EMPTY IS DROPPED — the array arm above already handled arrays, so
        // what is left here is the empty string and `null`. `if (text)` is a truthiness test, so an
        // empty string produces no message.
        Some(Value::String(_)) | None => {}
        Some(other) => {
            // The JavaScript's `if (text)` admits a truthy non-string, so `system: 5` produces
            // `{role:"system", content:5}`. Reproduced deliberately and flagged in the tests below —
            // it is a defect-shaped behaviour, and a port that "fixed" it would be a change, not a
            // translation.
            if truthy_js(other) {
                messages.push(json!({ "role": "system", "content": other.clone() }));
            }
        }
    }

    // ── the conversation ─────────────────────────────────────────────────────────────────────────
    // `for (const m of req.messages || [])` ITERATES WHATEVER IS THERE, and the first version of this
    // port used `as_array()` — so a STRING `messages` produced an empty conversation where the
    // JavaScript produced ONE MESSAGE PER CHARACTER. With a twelve-character string that is twelve
    // `{role: undefined, content: undefined}` messages, which `JSON.stringify` renders as twelve `{}`.
    // The differential caught it because the case was in the corpus, and the answer is only correct by
    // accident: the JavaScript's answer is garbage and this port must be the same garbage, because
    // **"more forgiving" is a silent behaviour change in the direction nobody tests** — an empty
    // conversation is one a provider ACCEPTS.
    let empty: Vec<Value> = Vec::new();
    let turns: Vec<Value> = match req.get("messages") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.clone(),
        // A STRING IS AN ITERABLE OF ITS CHARACTERS, and each character is a `m` with no role and no
        // content — so the fallback arm below produces the twelve empty messages described above.
        Some(Value::String(s)) => s.chars().map(|c| Value::String(c.to_string())).collect(),
        // `for (const b of 5)` is a TypeError: anything else is a THROW, and the refusal is the same
        // one a non-iterable `content` gets, for the same reason.
        Some(other) => non_iterable_content(other),
    };
    for turn in turns {
        let role = turn.get("role").and_then(Value::as_str).unwrap_or_default();
        let content = turn.get("content");
        match role {
            "user" => {
                let mut text_parts: Vec<String> = Vec::new();
                match effective_content(content) {
                    None => {}
                    Some(Value::String(s)) => text_parts.push(s.clone()),
                    Some(Value::Array(blocks)) => {
                        for b in blocks {
                            if b.is_null() {
                                panic!("zen-go: a content block is null, which the JavaScript reads .type off");
                            }
                            let kind = b.get("type").and_then(Value::as_str).unwrap_or_default();
                            if kind == "tool_result" {
                                // **A TOOL RESULT BECOMES ITS OWN MESSAGE**, and it is pushed DURING
                                // the walk, so it can land between two pieces of the user's text.
                                let tool_text = match b.get("content") {
                                    Some(Value::String(s)) => s.clone(),
                                    _ => {
                                        let inner: Vec<String> = b
                                            .get("content")
                                            .and_then(Value::as_array)
                                            .unwrap_or(&empty)
                                            .iter()
                                            .map(|c| {
                                                c.get("text")
                                                    .and_then(Value::as_str)
                                                    .or_else(|| {
                                                        c.get("thinking").and_then(Value::as_str)
                                                    })
                                                    .unwrap_or_default()
                                                    .to_string()
                                            })
                                            .collect();
                                        inner.join("\n")
                                    }
                                };
                                // **AN ABSENT `tool_use_id` IS AN ABSENT KEY**, because the JavaScript's
                                // `tool_call_id: b.tool_use_id` is `undefined` when the field is missing
                                // and `JSON.stringify` drops a key whose value is `undefined`. The first
                                // version inserted `null`, which the differential rendered as
                                // `"tool_call_id":null` against a key that is not there at all.
                                let mut tool_msg = Map::new();
                                tool_msg.insert("role".into(), Value::String("tool".into()));
                                if let Some(id) = b.get("tool_use_id") {
                                    tool_msg.insert("tool_call_id".into(), id.clone());
                                }
                                tool_msg.insert("content".into(), Value::String(tool_text));
                                messages.push(Value::Object(tool_msg));
                            } else if kind == "text" {
                                // **PUSHED UNCONDITIONALLY**, so a `text` block with no `text` still
                                // counts as a part and joins to the empty string.
                                text_parts.push(js_join_part(b.get("text")));
                            }
                        }
                    }
                    // **A CONTENT THAT IS NEITHER A STRING NOR AN ARRAY IS A THROW, NOT A SKIP**, and
                    // this arm comes LAST so an array is matched by the arm above it. The differential
                    // found it by CRASHING on a corpus case rather than by reporting a difference,
                    // which is the best way an instrument can find a shape.
                    Some(other) => non_iterable_content(other),
                }
                // **A USER TURN THAT PRODUCED NO TEXT PRODUCES NO MESSAGE** — the test is the text
                // parts, not the turn. A turn that was only tool results must not become an empty
                // `user` message, and the differential's corpus is full of them.
                if !text_parts.is_empty() {
                    messages.push(json!({ "role": "user", "content": text_parts.join("\n") }));
                }
            }
            "assistant" => {
                // **ALWAYS PUSHED, even with no content** — and it STARTS as `content: null`.
                let mut msg = Map::new();
                msg.insert("role".into(), Value::String("assistant".into()));
                msg.insert("content".into(), Value::Null);
                let mut text_parts: Vec<String> = Vec::new();
                let mut think_parts: Vec<String> = Vec::new();
                let mut tool_calls: Vec<Value> = Vec::new();
                match effective_content(content) {
                    None => {}
                    Some(Value::String(s)) => text_parts.push(s.clone()),
                    Some(Value::Array(blocks)) => {
                        for b in blocks {
                            // **A `null` BLOCK IS A THROW** — the JavaScript reads `b.type` off it.
                            if b.is_null() {
                                panic!("zen-go: a content block is null, which the JavaScript reads .type off");
                            }
                            let kind = b.get("type").and_then(Value::as_str).unwrap_or_default();
                            match kind {
                                // ALL THREE ARE PUSHED UNCONDITIONALLY, and the part still counts when
                                // the field is absent — `join` renders it as "".
                                "thinking" => think_parts.push(js_join_part(b.get("thinking"))),
                                "text" => text_parts.push(js_join_part(b.get("text"))),
                                "tool_use" => {
                                    // **THE ARGUMENTS ARE RE-SERIALISED**, so `{"a":1,"b":2}` comes
                                    // back as a STRING and not as an object. `input || {}` is a
                                    // truthiness default, so a falsy `input` (`0`, `""`, `false`)
                                    // becomes `{}` rather than that value.
                                    let input = b
                                        .get("input")
                                        .filter(|v| truthy_js(v))
                                        .cloned()
                                        .unwrap_or_else(|| json!({}));
                                    // THE SAME RULE FOR `id` AND `name`: absent means the key is not
                                    // there. `{"type":"function","function":{"arguments":"{}"}}` is what a
                                    // tool_use block with no id and no name serialises to, and it is a
                                    // different document from one carrying two nulls.
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
                                        Value::String(
                                            serde_json::to_string(&input)
                                                .expect("a Value is serialisable"),
                                        ),
                                    );
                                    call.insert("function".into(), Value::Object(func));
                                    tool_calls.push(Value::Object(call));
                                }
                                _ => {}
                            }
                        }
                    }
                    // LAST ARM, so an array is matched above — see the user's arm for the reasoning.
                    Some(other) => non_iterable_content(other),
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
            _ => {
                // **ANY OTHER ROLE IS COPIED, AND A NON-STRING CONTENT IS `JSON.stringify`'d** — so
                // an array of content blocks becomes a JSON string in a field a provider will not
                // parse. That is the deployed behaviour and it is reproduced rather than repaired.
                // **AN ABSENT FIELD IS AN ABSENT KEY, NOT AN EMPTY STRING.** `JSON.stringify` drops a
                // property whose value is `undefined`, so a character of a string `messages` — which
                // has neither a role nor a content — becomes `{}` and not
                // `{"role":"","content":"undefined"}`. The first version of this arm produced the
                // latter, and the differential showed it as twelve `{}` against twelve filled objects
                // on a twelve-character string: the empty object is the deployed answer.
                let mut msg = Map::new();
                if let Some(r) = turn.get("role") {
                    msg.insert("role".into(), r.clone());
                }
                match content {
                    Some(Value::String(s)) => {
                        msg.insert("content".into(), Value::String(s.clone()));
                    }
                    Some(other) => {
                        msg.insert(
                            "content".into(),
                            Value::String(
                                serde_json::to_string(other).expect("a Value is serialisable"),
                            ),
                        );
                    }
                    // `JSON.stringify(undefined)` is the VALUE `undefined`, so the key is dropped.
                    None => {}
                }
                messages.push(Value::Object(msg));
            }
        }
    }

    // ── the envelope ──────────────────────────────────────────────────────────────────────────────
    // `stream: false` is CONSTANT and comes BEFORE the optional fields, so the key order is
    // `{model, messages, stream, …}` and the differential compares it.
    let mut out = Map::new();
    out.insert("model".into(), Value::String(model.to_string()));
    out.insert("messages".into(), Value::Array(messages));
    out.insert("stream".into(), Value::Bool(false));
    // **`if (req.max_tokens)` IS A TRUTHINESS TEST** — so `max_tokens: 0` is DROPPED, while
    // `temperature: 0` is KEPT because that line tests `!== undefined`. Two adjacent optional fields
    // with two different rules, and a port that harmonises them changes the request.
    if truthy_js(req.get("max_tokens").unwrap_or(&Value::Null)) {
        out.insert(
            "max_tokens".into(),
            req.get("max_tokens").cloned().expect("just checked"),
        );
    }
    // **`!== undefined` IS A PRESENCE TEST, AND JSON's `null` IS NOT `undefined`.** A client that sends
    // `temperature: null` gets it forwarded, and the first version of this port dropped it by testing
    // `is_null()` — which is a DIFFERENT language's idea of absent. The property is absent, or it is
    // there; there is no third state in a JSON document, and that is exactly why the JavaScript's
    // `undefined` has no JSON spelling and its `!== undefined` behaves as "the key exists".
    if let Some(t) = req.get("temperature") {
        out.insert("temperature".into(), t.clone());
    }
    if let Some(t) = req.get("top_p") {
        out.insert("top_p".into(), t.clone());
    }

    // ── the tools ────────────────────────────────────────────────────────────────────────────────
    // **`req.tools?.length` IS A TRUTHINESS TEST ON A LENGTH**, so `tools: []` takes no branch and
    // the `tool_choice` that might have come with it is DROPPED. An empty array of tools plus a
    // tool_choice is a legal-looking request that loses its tool_choice on the way upstream.
    if let Some(tools) = req.get("tools").and_then(Value::as_array) {
        if !tools.is_empty() {
            out.insert(
                "tools".into(),
                Value::Array(
                    tools
                        .iter()
                        .map(|t| {
                            // **A `null` TOOL IS A THROW**, like a null content block: the JavaScript
                            // reads `t.name` off it. The differential found this one by crashing on a
                            // corpus case, which is the third time an instrument's crash has been the
                            // finding — a harness that reports a difference tells you WHAT, and one that
                            // crashes tells you there is something.
                            if t.is_null() {
                                panic!(
                                    "zen-go: a tool is null, which the JavaScript reads .name off"
                                );
                            }
                            // The function object is BUILT, not written as a literal, because three of
                            // its four fields are optional-by-absence: `name` is dropped when the tool
                            // has none, while `description` and `parameters` are TRUTHINESS defaults and
                            // are therefore always present. **Those are two different rules three lines
                            // apart**, and writing the literal made them look like one.
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
                            func.insert(
                                "parameters".into(),
                                match t.get("input_schema") {
                                    Some(v) if truthy_js(v) => v.clone(),
                                    _ => json!({}),
                                },
                            );
                            json!({ "type": "function", "function": Value::Object(func) })
                        })
                        .collect(),
                ),
            );
            // **`if (req.tool_choice)` IS A TRUTHINESS TEST**, so `tool_choice: null` and
            // `tool_choice: ""` insert NOTHING — and the first version used `if let Some(..)`, which
            // saw a `null` as present and emitted a `tool_choice` the request did not carry.
            if let Some(tc) = req.get("tool_choice").filter(|v| truthy_js(v)) {
                let kind = tc.get("type").and_then(Value::as_str).unwrap_or_default();
                let mapped = match kind {
                    "tool" => {
                        // AND AN ABSENT `name` IS AN ABSENT KEY: `{type:"tool"}` with no name
                        // serialises to `{"type":"function","function":{}}` and not to a null name.
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

/// `const content = typeof m.content === "string" ? m.content : m.content || []` — the line that
/// decides what a turn's content IS before anything iterates it, and it has three answers:
///
///  * **A STRING IS USED AS IT IS, INCLUDING THE EMPTY STRING.** `content: ""` is therefore a user turn
///    with one empty text part, which `textParts.length` accepts, so it produces
///    `{role:"user",content:""}` — a message with empty content, which a provider may reject.
///  * **A TRUTHY NON-STRING IS ITSELF**, so a number is a number and the loop over it throws.
///  * **A FALSY NON-STRING IS `[]`** — `null`, `0`, `false` and `""` all become an empty array, which is
///    why `content: null` does NOT throw, and why the first version of this port (which panicked on it)
///    was wrong about a case the differential carried.
///
/// `None` is the empty-array case, so the caller has nothing to walk.
fn effective_content(content: Option<&Value>) -> Option<&Value> {
    match content {
        Some(Value::String(_)) => content,
        Some(other) if truthy_js(other) => Some(other),
        _ => None,
    }
}

/// The JavaScript's `for (const b of content)`, as the THROW it is on anything that is not a string
/// and not an array. A `Value` that is a truthy number, bool or object is not iterable, so the loop is
/// a `TypeError` there and the request 500s — and workers-rs turns a panic into the same 500.
fn non_iterable_content(v: &Value) -> ! {
    panic!(
        "zen-go: message content {v} is neither a string nor an array, so it cannot be iterated"
    );
}

/// What `Array.prototype.join` does with one element, and the JavaScript pushes `b.text`
/// UNCONDITIONALLY for a `text` block — so a block with no `text` pushes `undefined`, the part still
/// COUNTS, and `join` renders it as the EMPTY STRING.
///
/// **AND `join` RENDERS BOTH `undefined` AND `null` AS EMPTY** — that is the rule, and it is the
/// reason `[{"type":"text"}]` produces `content: ""` and not `content: null`. The first version of this
/// port skipped the block, so the part did not count and the message kept its `content: null`; the
/// differential rendered the difference as `""` against `null` on a block that is not malformed at all.
fn js_join_part(v: Option<&Value>) -> String {
    match v {
        // `join` renders undefined AND null as the empty string, which is not the same as `String(null)`.
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// JavaScript truthiness for the values that can appear here, spelled out rather than delegated.
///
/// The cases that matter are the ones a Rust reader would get wrong: `0` and `""` are FALSY in
/// JavaScript and `is_null()` is not the same test, `[]` and `{}` are TRUTHY (so a falsy `input` is
/// never an empty object — an empty object is truthy and passes through), and `NaN` does not exist
/// in a `Value`. **`true` is truthy and so is every non-empty string, including `"0"` and `"false"`,
/// which is why `description: "0"` survives while `description: 0` does not.**
fn truthy_js(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `toAnthropicResponse(up, model)` — the OpenAI completion, as an Anthropic message.
///
/// **THE STOP REASON MAP HAS FOUR KEYS AND A DEFAULT OF `end_turn`**, and the default is not a
/// fallback for a missing `finish_reason` alone: an UNKNOWN finish reason also lands on `end_turn`,
/// which is what a new provider-level reason will do.
///
/// The `|| 0` defaults in `usage` are truthiness defaults, so a reported `0` and a missing field are
/// indistinguishable in the output — and `prompt_cache_hit_tokens` becomes `cache_read_input_tokens`
/// while `cache_creation_input_tokens` is a hard `0` because this provider never reports it.
pub fn to_anthropic_response(up: &Value, model: &str) -> Value {
    let choice = up
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first());
    let msg = choice
        .and_then(|c| c.get("message"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let empty: Vec<Value> = Vec::new();
    let mut blocks: Vec<Value> = Vec::new();

    // THINKING FIRST, THEN TEXT, THEN TOOL CALLS — the order is the wire order, and a completion with
    // all three emits them in this order regardless of the order the upstream wrote them in.
    if truthy_js(msg.get("reasoning_content").unwrap_or(&Value::Null)) {
        blocks.push(json!({
            "type": "thinking",
            "thinking": msg.get("reasoning_content").cloned().expect("just checked"),
            // **THE SIGNATURE IS AN EMPTY STRING, NOT ABSENT** — the Anthropic schema wants the key,
            // and a client that reads `block.signature` would get `undefined` without it.
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
        .unwrap_or(&empty)
    {
        // **A MALFORMED `arguments` STRING BECOMES `{}` AND IS NOT AN ERROR** — a model emits
        // whatever it emits, and the upstream must not be taken down by a tool call it will not be
        // able to run. `JSON.parse("")` throws in JavaScript, so the empty string is the common case
        // and the `{}` default is load-bearing.
        let arguments = tc
            .get("function")
            .and_then(|f| f.get("arguments"))
            .and_then(Value::as_str)
            .unwrap_or("{}");
        let input: Value = serde_json::from_str(arguments).unwrap_or_else(|_| json!({}));
        // **AND THE `id` HERE IS OPTIONAL-BY-ABSENCE TOO** — the same rule as the message `id`, the
        // tool `name` and the tool_choice `name`, and the last of the five. A tool call with no id
        // serialises to a block with no `id`, and a client that reads `block.id` gets `undefined`
        // rather than a `null` it would have to learn to treat as absent.
        let mut block = Map::new();
        block.insert("type".into(), Value::String("tool_use".into()));
        // **THE `id` IS OPTIONAL-BY-ABSENCE TOO** — the same rule as the message `id`, the tool `name`
        // and the tool_choice `name`, and the last of the five. A tool call with no id serialises to a
        // block with NO `id` key, and a client that reads `block.id` gets `undefined` rather than a
        // `null` it would have to learn to treat as absent.
        if let Some(id) = tc.get("id") {
            block.insert("id".into(), id.clone());
        }
        block.insert(
            "name".into(),
            // **`|| "unknown"` IS A TRUTHINESS DEFAULT**, so a missing, null OR EMPTY name all become
            // the literal string "unknown".
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
        Some("stop") => "end_turn",
        Some("tool_calls") | Some("function_call") => "tool_use",
        Some("length") => "max_tokens",
        _ => "end_turn",
    };

    // **THE OBJECT IS BUILT RATHER THAN WRITTEN AS A LITERAL**, because `id` is OPTIONAL-BY-ABSENCE:
    // `id: up.id` is `undefined` for a completion with no id and `JSON.stringify` drops the key, so
    // the answer is `{type, role, model, content, …}` with no `id` at all — not with `"id": null`.
    // Every other field here is a constant or a defaulted value and is therefore always present.
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
            // THIS PROVIDER NEVER REPORTS CACHE CREATION, so the field is a constant 0 and not a
            // mapped value. A client that reads it gets 0, which is the truth about this provider.
            "cache_creation_input_tokens": 0,
            "cache_read_input_tokens": usage_int(up, "prompt_cache_hit_tokens"),
        }),
    );
    Value::Object(out)
}

/// `up.usage?.prompt_tokens || 0` — a truthiness default, so a reported 0 and a missing field are
/// the same output. **Note this is NOT the `!== undefined` rule the request's optional fields use**,
/// and the two are three lines apart in the JavaScript.
fn usage_int(up: &Value, key: &str) -> Value {
    let v = up.get("usage").and_then(|u| u.get(key));
    match v {
        // **THE NUMBER IS PASSED THROUGH AND NOT ROUNDED, AND THAT IS THE WHOLE POINT OF RETURNING A
        // `Value`.** The first version returned a `u64`, so an upstream that reported
        // `prompt_tokens: 1.5` came out as `0` — not a rounding, a LOSS. The Anthropic schema says
        // integers, and the JavaScript does not check: a float is truthy, so it is forwarded, and the
        // differential's case is a provider that reports one.
        Some(Value::Number(n)) if n.as_f64().is_some_and(|f| f != 0.0) => Value::Number(n.clone()),
        _ => Value::from(0),
    }
}

/// `sse(name, data)` — one SSE frame: `event: …\ndata: …\n\n`.
///
/// **THE BLANK LINE IS PART OF THE FRAME**, and the data is `JSON.stringify`'d, so a frame whose
/// payload contains a newline is still one frame: `JSON.stringify` escapes it, and a hand-rolled
/// encoder that did not would split one event into two.
pub fn sse(name: &str, data: &Value) -> String {
    format!(
        "event: {name}\ndata: {}\n\n",
        serde_json::to_string(data).expect("a Value is serialisable")
    )
}

/// `toSSE(res)` — the whole non-streaming Anthropic message, as a sequence of SSE frames.
///
/// **THE FRAME COUNT IS DERIVABLE, and the `message_start` frame IS NOT THE MESSAGE.** It is the
/// message with `content: []` and both stop fields nulled, because the content arrives in the frames
/// that follow and a client that read the content from `message_start` would read it twice. The
/// usage in `message_delta` is only `output_tokens` — the input tokens were in `message_start`.
///
/// **`index` IS THE BLOCK'S POSITION**, and it counts from zero across the whole content array, so a
/// client can pair `content_block_start` with its `content_block_stop` by index alone.
pub fn to_sse(res: &Value) -> String {
    // **THE SPREAD IS A SHALLOW COPY**, so `message_start`'s message keeps `id`, `model`, `role`,
    // `type`, `usage` and the constant fields, and only those three are replaced. A block inside
    // `content` would be shared by reference in JavaScript; here it is dropped wholesale.
    let mut start_message = res.clone();
    start_message
        .as_object_mut()
        .expect("the response is an object")
        .insert("content".into(), Value::Array(Vec::new()));
    if let Some(obj) = start_message.as_object_mut() {
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
        out += &sse(
            "content_block_start",
            &json!({ "type": "content_block_start", "index": i, "content_block": block.clone() }),
        );
        let kind = block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match kind {
            "thinking" => {
                out += &sse(
                    "content_block_delta",
                    &json!({ "type": "content_block_delta", "index": i, "delta": { "type": "thinking_delta", "thinking": block.get("thinking").cloned().unwrap_or(Value::Null) } }),
                )
            }
            "text" => {
                out += &sse(
                    "content_block_delta",
                    &json!({ "type": "content_block_delta", "index": i, "delta": { "type": "text_delta", "text": block.get("text").cloned().unwrap_or(Value::Null) } }),
                )
            }
            "tool_use" => {
                out += &sse(
                    "content_block_delta",
                    &json!({ "type": "content_block_delta", "index": i, "delta": { "type": "input_json_delta", "partial_json": serde_json::to_string(block.get("input").unwrap_or(&json!({}))).expect("serialisable") } }),
                )
            }
            // **AN UNKNOWN BLOCK TYPE PRODUCES A START AND A STOP AND NO DELTA** — the `if/else if`
            // chain has no `else`, and a client that expects exactly one delta per block will notice.
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
            "delta": { "stop_reason": res.get("stop_reason").cloned().unwrap_or(Value::Null), "stop_sequence": Value::Null },
            // ONLY `output_tokens`, and it is read with a `.` and not a `?.` in the JavaScript — so a
            // response with no `usage` THROWS there. Here it is `0` instead, which is the one place
            // this port is more forgiving than the code it ports, and the differential says so.
            "usage": { "output_tokens": res.get("usage").and_then(|u| u.get("output_tokens")).cloned().unwrap_or(json!(0)) },
        }),
    );
    out += &sse("message_stop", &json!({ "type": "message_stop" }));
    out
}

#[cfg(test)]
// THE NAMES SHOUT WHERE THE BEHAVIOUR IS THE OPPOSITE OF WHAT A READER EXPECTS — a zero that is
// dropped, a null that is kept, a `{}` that is truthy. The capitals are the note.
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_TEMPERATURE_is_KEPT_and_a_zero_MAX_TOKENS_is_DROPPed() {
        // **TWO ADJACENT OPTIONAL FIELDS, TWO DIFFERENT RULES.** `max_tokens` is copied only when
        // truthy, so a caller's `0` vanishes; `temperature` is copied when the property is present,
        // so a caller's `0` survives. A port that harmonised them would change one of them.
        let req = json!({ "messages": [], "max_tokens": 0, "temperature": 0 });
        let out = to_openai_request(&req, "m");
        assert!(
            out.get("max_tokens").is_none(),
            "max_tokens: 0 is dropped: {out}"
        );
        assert_eq!(
            out.get("temperature"),
            Some(&json!(0)),
            "temperature: 0 is kept"
        );
    }

    #[test]
    fn a_user_turn_that_was_ONLY_tool_results_produces_NO_user_message() {
        let req = json!({
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "t1", "content": "42" },
                    { "type": "tool_result", "tool_use_id": "t2", "content": [{ "text": "a" }, { "thinking": "b" }] },
                ],
            }],
        });
        let out = to_openai_request(&req, "m");
        let msgs = out["messages"].as_array().expect("an array");
        // TWO `tool` MESSAGES AND NO `user` MESSAGE — the test for whether a user message exists is
        // whether the turn contributed TEXT, not whether a user turn was seen.
        assert_eq!(msgs.len(), 2, "{out}");
        assert_eq!(msgs[0]["role"], json!("tool"));
        assert_eq!(msgs[0]["content"], json!("42"));
        // A tool result's array content joins `text` then `thinking` per block, with `\n`.
        assert_eq!(msgs[1]["content"], json!("a\nb"), "{out}");
    }

    #[test]
    fn a_tool_message_can_land_BETWEEN_two_pieces_of_the_users_text() {
        // The `tool` message is pushed DURING the walk over the content blocks, so its position is
        // determined by where the tool_result sits — not appended afterwards.
        let req = json!({
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "before" },
                    { "type": "tool_result", "tool_use_id": "t1", "content": "R" },
                    { "type": "text", "text": "after" },
                ],
            }],
        });
        let out = to_openai_request(&req, "m");
        let roles: Vec<&str> = out["messages"]
            .as_array()
            .expect("an array")
            .iter()
            .map(|m| m["role"].as_str().unwrap_or("?"))
            .collect();
        assert_eq!(
            roles,
            vec!["tool", "user"],
            "the tool message comes first: {out}"
        );
    }

    #[test]
    fn an_ASSISTANT_turn_with_no_content_is_still_pushed_with_a_NULL_content() {
        let req = json!({ "messages": [{ "role": "assistant", "content": [] }] });
        let out = to_openai_request(&req, "m");
        let msgs = out["messages"].as_array().expect("an array");
        assert_eq!(
            msgs.len(),
            1,
            "an assistant turn always produces a message: {out}"
        );
        // **`content: null` IS A VALUE THE UPSTREAM EXPECTS**, and it is present because the message
        // starts as `{role, content: null}` — the key is not merely absent.
        assert!(msgs[0]
            .as_object()
            .expect("an object")
            .contains_key("content"));
        assert_eq!(msgs[0]["content"], Value::Null);
    }

    #[test]
    fn an_empty_ARRAY_of_tools_drops_the_tool_choice_with_it() {
        // **`req.tools?.length` IS TRUTHY ON A LENGTH**, so `tools: []` takes no branch — and the
        // `tool_choice` that is nested inside that branch goes with it. A caller sending an empty
        // tools array and a tool_choice loses the tool_choice on the way upstream.
        let req = json!({
            "messages": [],
            "tools": [],
            "tool_choice": { "type": "any" },
        });
        let out = to_openai_request(&req, "m");
        assert!(out.get("tools").is_none(), "{out}");
        assert!(
            out.get("tool_choice").is_none(),
            "and the tool_choice with it: {out}"
        );
        // WITH ONE TOOL the same tool_choice maps to the string "required".
        let req2 = json!({
            "messages": [],
            "tools": [{ "name": "t" }],
            "tool_choice": { "type": "any" },
        });
        assert_eq!(
            to_openai_request(&req2, "m")["tool_choice"],
            json!("required")
        );
        // AN UNRECOGNISED CHOICE TYPE MAPS TO "auto", which is the `else` of the chain.
        let req3 = json!({
            "messages": [],
            "tools": [{ "name": "t" }],
            "tool_choice": { "type": "nonsense" },
        });
        assert_eq!(to_openai_request(&req3, "m")["tool_choice"], json!("auto"));
    }

    #[test]
    fn MALFORMED_tool_arguments_become_an_empty_object_and_not_an_error() {
        // A MODEL EMITS WHATEVER IT EMITS. `JSON.parse("")` throws in JavaScript and the `{}` default
        // is what keeps the request alive; `JSON.parse("{oops")` throws for the same reason. Neither
        // may take the request down, and neither may re-serialise the broken string.
        for arguments in ["", "{oops", "null", "[1,2]", "\"a string\"", "123", "true"] {
            let up = json!({
                "id": "x", "choices": [{ "message": { "tool_calls": [
                    { "id": "t1", "function": { "name": "f", "arguments": arguments } }
                ] } }],
            });
            let out = to_anthropic_response(&up, "m");
            let block = &out["content"][0];
            assert_eq!(block["type"], json!("tool_use"), "for {arguments:?}: {out}");
            // `null`, `[1,2]`, `"a string"`, `123` and `true` PARSE, so they come back as themselves —
            // the fallback is only for what does not parse.
            let parsed: Result<Value, _> = serde_json::from_str(arguments);
            let want = parsed.unwrap_or_else(|_| json!({}));
            assert_eq!(block["input"], want, "for {arguments:?}: {out}");
        }
    }

    #[test]
    fn an_empty_tool_NAME_becomes_the_literal_unknown_and_so_does_a_missing_one() {
        // `tc.function?.name || "unknown"` is a TRUTHINESS default, so `""`, `null` and an absent name
        // are all "unknown" — while the string `"0"` is truthy and survives.
        for (name, want) in [
            (json!("real"), json!("real")),
            (json!(""), json!("unknown")),
            (Value::Null, json!("unknown")),
            (json!("0"), json!("0")),
            (json!("false"), json!("false")),
        ] {
            let up = json!({
                "choices": [{ "message": { "tool_calls": [
                    { "id": "t", "function": { "name": name.clone(), "arguments": "{}" } }
                ] } }],
            });
            let out = to_anthropic_response(&up, "m");
            assert_eq!(out["content"][0]["name"], want, "for {name}");
        }
    }

    #[test]
    fn an_UNKNOWN_finish_reason_lands_on_end_turn_like_a_missing_one() {
        for (finish, want) in [
            (json!("stop"), json!("end_turn")),
            (json!("tool_calls"), json!("tool_use")),
            (json!("function_call"), json!("tool_use")),
            (json!("length"), json!("max_tokens")),
            (json!("content_filter"), json!("end_turn")),
            (Value::Null, json!("end_turn")),
        ] {
            let up = json!({ "choices": [{ "finish_reason": finish.clone(), "message": { "content": "x" } }] });
            let out = to_anthropic_response(&up, "m");
            assert_eq!(out["stop_reason"], want, "for {finish}");
        }
    }

    #[test]
    fn the_THINKING_block_carries_an_EMPTY_SIGNATURE_and_comes_FIRST() {
        // The signature is a CONSTANT empty string and not an absent key, because the Anthropic
        // schema has the field and a client that reads `block.signature` would get `undefined`.
        let up = json!({
            "choices": [{ "message": {
                "reasoning_content": "thought",
                "content": "answer",
                "tool_calls": [{ "id": "t", "function": { "name": "f", "arguments": "{}" } }],
            } }],
        });
        let out = to_anthropic_response(&up, "m");
        let kinds: Vec<&str> = out["content"]
            .as_array()
            .expect("an array")
            .iter()
            .map(|b| b["type"].as_str().unwrap_or("?"))
            .collect();
        assert_eq!(
            kinds,
            vec!["thinking", "text", "tool_use"],
            "the order is the wire order: {out}"
        );
        assert_eq!(
            out["content"][0]["signature"],
            json!(""),
            "and the signature is an empty string"
        );
    }

    #[test]
    fn the_SSE_message_start_frame_carries_the_message_with_NO_content() {
        let res = to_anthropic_response(
            &json!({ "id": "x", "choices": [{ "message": { "content": "hello" } }] }),
            "m",
        );
        let sse_out = to_sse(&res);
        // **THE MESSAGE APPEARS TWICE AND THE CLIENT MUST NOT READ IT TWICE**: once in
        // `message_start` with `content: []` and both stop fields null, and once as the frames.
        let first = sse_out.split("event: ").nth(1).expect("a first frame");
        let data: Value = serde_json::from_str(
            first
                .split("data: ")
                .nth(1)
                .expect("a data line")
                .trim_end(),
        )
        .expect("JSON");
        assert_eq!(
            data["message"]["content"]
                .as_array()
                .expect("an array")
                .len(),
            0
        );
        assert_eq!(data["message"]["stop_reason"], Value::Null);
        assert_eq!(data["message"]["stop_sequence"], Value::Null);
        // AND THE ID AND MODEL SURVIVE THE SPREAD, which is the point of a shallow copy.
        assert_eq!(data["message"]["id"], json!("x"));
        assert_eq!(data["message"]["model"], json!("m"));
        // ONE FRAME PER BLOCK, PLUS message_start, message_delta AND message_stop.
        let frames = sse_out.matches("event: ").count();
        assert_eq!(
            frames,
            // message_start, then THREE frames for the one text block (start, delta, stop), then
            // message_delta and message_stop. The arithmetic is spelled out so the number is derived
            // rather than pasted — and clippy's "this operation has no effect" is right about the
            // multiplication and wrong about the intent, so the count is written in words.
            1 + 3 + 2,
            "one text block is three frames plus the two ends: {sse_out}"
        );
    }

    #[test]
    fn an_UNKNOWN_block_type_produces_a_start_and_a_stop_and_NO_delta() {
        // The chain is `if / else if / else if` with no `else`, so a block this code does not know
        // yields TWO frames and no delta — and a client that expects exactly one delta per block
        // will notice.
        let res = json!({
            "id": "x", "content": [{ "type": "redacted_thinking" }],
            "stop_reason": "end_turn", "usage": { "output_tokens": 5 },
        });
        let out = to_sse(&res);
        let deltas = out.matches("event: content_block_delta").count();
        assert_eq!(deltas, 0, "no delta for an unknown block: {out}");
        assert_eq!(out.matches("event: content_block_start").count(), 1);
        assert_eq!(out.matches("event: content_block_stop").count(), 1);
    }

    #[test]
    #[should_panic(expected = "content block")]
    fn a_NULL_content_block_TAKES_the_request_down_exactly_as_the_JavaScript_does() {
        // **THE DOOR THROWS INSTEAD OF DIFFERING QUIETLY**, and the first version of this port SKIPPED
        // the null and answered normally — a "more forgiving" proxy, which is a silent behaviour change
        // and the exact shape this migration's own rules forbid. The JavaScript reads `b.type` off
        // every element, so a `null` is a `TypeError` and the request 500s; workers-rs turns a panic
        // into the same 500, so the two agree ON THE WIRE even though the mechanism differs.
        //
        // This is the repo's own pattern one layer down: `panel-logic`'s `require_present` throws for
        // `r.events.find` on a non-array for the same reason and carries the same note.
        let req = json!({ "messages": [{ "role": "user", "content": [null] }] });
        let _ = to_openai_request(&req, "m");
    }

    #[test]
    fn an_ABSENT_field_is_an_ABSENT_key_and_never_a_null() {
        // **THE RULE THIS WHOLE FILE TURNED ON, and it appeared FIVE TIMES**: the message `id`, the
        // message `role` and `content` on the fallback arm, a tool's `name`, a tool_use block's `id`,
        // a tool_call's `tool_call_id` and a tool_choice's `name`. `JSON.stringify` DROPS a property
        // whose value is `undefined`, and the first version of this port wrote `unwrap_or(Value::Null)`
        // at every one of them — so a request without a tool name carried `"name":null` upstream, and a
        // completion without an id carried `"id":null` back to a client that expects the key or
        // nothing. **`null` is a VALUE THE CALLER SENT; an absent field is the absence of one**, and
        // the two are not interchangeable on the wire.
        let req = json!({ "messages": [], "tools": [{}] });
        let out = to_openai_request(&req, "m");
        let func = &out["tools"][0]["function"];
        assert!(
            !func.as_object().expect("an object").contains_key("name"),
            "{out}"
        );
        // AND A `tool_choice` WITH A TYPE BUT NO NAME HAS NO `name` EITHER.
        let req2 = json!({ "messages": [], "tools": [{}], "tool_choice": { "type": "tool" } });
        let out2 = to_openai_request(&req2, "m");
        assert!(
            !out2["tool_choice"]["function"]
                .as_object()
                .expect("an object")
                .contains_key("name"),
            "{out2}"
        );
        // AND A COMPLETION WITH NO `id` COMES BACK WITH NO `id`.
        let out3 = to_anthropic_response(
            &json!({ "choices": [{ "message": { "content": "x" } }] }),
            "m",
        );
        assert!(
            !out3.as_object().expect("an object").contains_key("id"),
            "{out3}"
        );
    }

    #[test]
    fn a_text_block_with_NO_text_STILL_COUNTS_and_joins_to_the_empty_string() {
        // **`textParts.push(b.text)` IS UNCONDITIONAL**, so a `text` block with no `text` pushes
        // `undefined`, the part counts, and `Array.prototype.join` renders `undefined` AND `null` as
        // the empty string. The first version skipped the block, so the part did not count and the
        // assistant message kept its `content: null` — on input that is not malformed at all.
        let req = json!({ "messages": [{ "role": "assistant", "content": [{ "type": "text" }] }] });
        let out = to_openai_request(&req, "m");
        assert_eq!(out["messages"][0]["content"], json!(""), "{out}");
        // AND A `null` CONTENT IS NOT A THROW: `m.content || []` makes every falsy content an empty
        // array, so the first version — which panicked on `content: null` — refused input the
        // JavaScript walks without complaint.
        let req2 = json!({ "messages": [{ "role": "assistant", "content": null }] });
        let out2 = to_openai_request(&req2, "m");
        assert_eq!(
            out2["messages"][0]["content"],
            Value::Null,
            "and the message keeps its null: {out2}"
        );
    }

    #[test]
    fn a_FRACTIONAL_token_count_is_PASSED_THROUGH_and_not_rounded() {
        // `up.usage?.prompt_tokens || 0` forwards a float, because a float is truthy. The first
        // version's helper returned a `u64`, so an upstream reporting `1.5` came out as `0` — a loss,
        // not a rounding. Returning a `Value` is what makes the rule expressible at all.
        let up = json!({ "id": "c", "choices": [{ "message": { "content": "x" } }], "usage": { "prompt_tokens": 1.5 } });
        let out = to_anthropic_response(&up, "m");
        assert_eq!(out["usage"]["input_tokens"], json!(1.5), "{out}");
        // A ZERO is falsy and becomes 0, and a missing field is 0 — the two are indistinguishable,
        // which is the truthiness default doing its job.
        let zero = json!({ "choices": [{ "message": {} }], "usage": { "prompt_tokens": 0 } });
        assert_eq!(
            to_anthropic_response(&zero, "m")["usage"]["input_tokens"],
            json!(0)
        );
    }

    #[test]
    fn a_STRING_messages_ITERATES_AS_ITS_CHARACTERS_because_the_JavaScript_does() {
        // `for (const m of req.messages || [])` iterates ANY iterable, so a twelve-character string is
        // twelve turns, each with no role and no content, and `JSON.stringify` renders each as `{}`.
        // The first version used `as_array()` and produced an EMPTY conversation — and an empty
        // conversation is one a provider ACCEPTS, which is the dangerous direction to differ in.
        let out = to_openai_request(&json!({ "messages": "abc" }), "m");
        let msgs = out["messages"].as_array().expect("an array");
        assert_eq!(msgs.len(), 3, "one message per character: {out}");
        for m in msgs {
            assert_eq!(
                m.as_object().expect("an object").len(),
                0,
                "and it is an empty object: {m}"
            );
        }
    }

    #[test]
    fn the_optional_fields_use_TWO_different_presence_rules_and_that_is_the_point() {
        // `max_tokens` truthiness, `temperature`/`top_p` presence. The request is the same and the
        // three fields answer differently, which is the assertion.
        let req = json!({ "messages": [], "max_tokens": 5, "temperature": null, "top_p": 0.5 });
        let out = to_openai_request(&req, "m");
        assert_eq!(out.get("max_tokens"), Some(&json!(5)));
        // **`temperature: null` IS FORWARDED**, and this test asserted the opposite until the
        // differential did. `!== undefined` is a PRESENCE test, and a JSON document has no
        // `undefined` — so `null` is a value the caller sent, not an absence. Testing `is_null()` is
        // importing another language's idea of missing into a document that does not have one.
        assert_eq!(
            out.get("temperature"),
            Some(&Value::Null),
            "null is forwarded: {out}"
        );
        assert_eq!(out.get("top_p"), Some(&json!(0.5)));
        // AND THE KEY ORDER PUTS THEM AFTER `stream`, which is where the JavaScript's object literal
        // puts them and which `preserve_order` is what makes possible.
        let keys: Vec<&str> = out
            .as_object()
            .expect("an object")
            .keys()
            .map(|k| k.as_str())
            .collect();
        assert_eq!(
            keys,
            vec![
                "model",
                "messages",
                "stream",
                "max_tokens",
                "temperature",
                "top_p"
            ],
            "{keys:?}"
        );
    }
}
