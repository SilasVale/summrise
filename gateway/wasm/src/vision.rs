//! GATEWAY-SIDE VISION PRE-PROCESSING, MOVED FROM `gateway/src/plugins/translate-vision.ts`.
//!
//! WHY THIS ONE. The gateway's own models are text-only, so an incoming Anthropic image block is replaced by a
//! DESCRIPTION of the picture, produced by a vision model (`VISION_MODEL`, default `og/mimo-v2.5`) — and the
//! request then goes upstream carrying text. **IT IS THE LAST KNOWN DIFFERENCE IN THE DIVERGENCE SWEEP**:
//! measured 2026-10-06, a request with an image made the shipping route dial TWICE (the describe, then the real
//! call) and this worker ONCE, which means every image sent to this worker after a cutover would reach a
//! text-only model as raw base64.
//!
//! THE THREE THINGS THAT ARE EASY TO GET WRONG, each of them a comment in the source:
//!
//!   * **A FAILED DESCRIBE MUST FAIL THE REQUEST** (round-119). The old code injected
//!     `(图片描述失败…)` as if it were a description and the client believed the image had been seen. The
//!     markers are checked, and a match throws.
//!   * **`declaredVision` SHORT-CIRCUITS THE WHOLE PASS** — a custom provider's model record may declare
//!     `input: [text, image]` (DSH's shape). Describing a picture the target model can see itself would replace
//!     it with a lossy summary AND cost an upstream call.
//!   * **THE DESCRIBE CALL USES THE KV SETTING FOR ITS US EXIT, NOT THE REQUEST PATH'S FALLBACK** — the source
//!     reads `getGlobalSetting(env, "US_PROXY")` alone (no `US_PROXY` env fallback), and with the proxy on, a
//!     describe that went direct would turn every image into a failure marker for US users (round-119 again).
//!
//! THE CACHE IS WHAT MAKES IT AFFORDABLE: the client re-sends the same base64 every turn, and the key is
//! user-scoped (`img-desc:<uid>:<sha256(model:data)[0..16]>`) because a content-only key let one user's
//! description be served to another who sent the same bytes (round-45). Seven days, and a failed or empty
//! description is never cached.

use serde_json::{json, Map, Value};
use worker::*;

/// `env.VISION_MODEL || "og/mimo-v2.5"`.
pub const DEFAULT_VISION_MODEL: &str = "og/mimo-v2.5";

/// The describe prompt, verbatim — it is part of the request the upstream sees.
pub const DESCRIBE_PROMPT: &str = "请用中文详细描述这张图片的内容，包括所有可见文字（OCR）、界面元素、布局。若是截图或表格，请逐行说明关键内容。只输出描述，不要额外说明。";

/// `expirationTtl: 7 * 24 * 60 * 60`.
pub const CACHE_TTL_SECONDS: u64 = 604_800;

/// `max_tokens: 1500` on the describe request.
pub const DESCRIBE_MAX_TOKENS: i64 = 1500;

/// **THE THREE MARKERS `preprocessImages` TREATS AS HARD ERRORS** — `/图片描述失败|图片描述为空|图片数据为空/`.
pub fn describe_failed(desc: &str) -> bool {
    desc.contains("图片描述失败") || desc.contains("图片描述为空") || desc.contains("图片数据为空")
}

/// The sentence a failed describe throws.
///
/// `desc.replace(/^\((图片描述失败|图片描述为空|图片数据为空)[：:]?/, "")` — the marker and its optional colon
/// come off the front, and what remains is what the operator reads.
pub fn describe_failure_message(desc: &str) -> String {
    let stripped = strip_marker_prefix(desc);
    format!("vision preprocessing failed: {stripped}")
}

/// The prefix strip above, as its own function so a test can drive it without the sentence around it.
fn strip_marker_prefix(desc: &str) -> String {
    let Some(rest) = desc.strip_prefix('(') else {
        return desc.to_string();
    };
    for marker in ["图片描述失败", "图片描述为空", "图片数据为空"] {
        if let Some(after) = rest.strip_prefix(marker) {
            return after
                .strip_prefix('：')
                .or_else(|| after.strip_prefix(':'))
                .unwrap_or(after)
                .to_string();
        }
    }
    desc.to_string()
}

/// `[图片内容描述]\n${desc}` — the block an image becomes.
pub fn vision_block(desc: &str) -> String {
    format!("[图片内容描述]\n{desc}")
}

/// `isVisionCapable(model, upstreamModel, env)` — the `VISION_CAPABLE_MODELS` allowlist, matched against the
/// ADVERTISED id and the WIRE id alike.
pub fn is_vision_capable(model: &str, upstream_model: &str, env: Option<&Value>) -> bool {
    let raw = env
        .and_then(|e| e.get("VISION_CAPABLE_MODELS"))
        .map(crate::stream::js_text)
        .unwrap_or_default();
    // `String(x || "")`: an absent var and an empty one are the same, and a non-string is its text.
    let list: Vec<&str> = raw
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    list.contains(&model) || list.contains(&upstream_model)
}

/// `img-desc:${uid || "anon"}:${h}` — and `""` when the data is short enough that caching is not worth it
/// (`data.length > 16`).
pub fn image_cache_key(uid: &str, digest_hex: &str, data_len: usize) -> String {
    if data_len > 16 {
        let uid = if uid.is_empty() { "anon" } else { uid };
        format!("img-desc:{uid}:{digest_hex}")
    } else {
        String::new()
    }
}

/// The `{key, shape}` table — `BYOK_CHANNELS`' nine rows PLUS `custom`.
///
/// **`custom` IS WRITTEN OUT RATHER THAN DERIVED, AND THE SOURCE SAYS WHY**: it is a route KIND with no BYOK
/// channel behind it — its models are served through the OpenAI-compatible dialect and its key comes from its
/// own record, so the `key` column is unused. A naive "derive the whole table" would drop the entry and every
/// describe against a custom provider would answer "视觉模型后端不支持".
pub fn vision_backend(kind: &str) -> Option<(&'static str, &'static str)> {
    // **THE SHAPE IS THIS MODULE'S FACT; THE KEY NAME IS NOT.** The first port wrote the nine key names out
    // again in a `TABLE` here, and `byok.rs`'s own header says what that costs: its table is "the ONE canonical
    // copy of a mapping that was previously written out FIVE times". `gateway/test/byok.test.mjs` caught the
    // second copy across the language boundary — its assertion is "the nine must be derived, and absent from the
    // consumer" — so the key column is DERIVED from `REQUIRED_KEY_BY_KIND` and only the dialect stays here.
    const SHAPES: [(&str, &str); 9] = [
        ("opencode", "openai"),
        ("deepseek", "anthropic"),
        ("qwen", "anthropic"),
        ("openrouter", "anthropic"),
        ("nvidia", "openai"),
        ("gmi", "openai"),
        ("commandgoat", "openai"),
        ("amd", "anthropic"),
        ("r4", "anthropic"),
    ];
    if kind == "custom" {
        // The TENTH: a route KIND with no BYOK channel behind it, so its key column is unused (see the doc above).
        return Some(("", "openai"));
    }
    // **THE ENV-KEY COLUMN, NOT THE USER-BLOB FIELD.** `byok.rs` carries both: `BYOK_CHANNELS` is
    // (kind, ENV KEY NAME, env fallback) and `REQUIRED_KEY_BY_KIND` is (kind, the field `extractByokKeys` spells
    // in the user's blob) — `OPENCODE_GO_API_KEY` against `opencodeGo`. The vision pass reads the ENV KEY, and the
    // Rust test that pins this table said so on the first attempt ("left: opencodeGo").
    let key = crate::byok::BYOK_CHANNELS
        .iter()
        .find(|(k, _, _)| *k == kind)
        .map(|(_, env_key, _)| *env_key)?;
    SHAPES
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, shape)| (key, *shape))
}

/// `crypto.subtle.digest("SHA-256", …)`, hex, first 16 bytes — **THE SOURCE'S OWN API, THROUGH THE SAME WEB
/// CRYPTO**.
///
/// `worker::crypto::DigestStream` is the typed wrapper and it CANNOT do this: its binding exposes `digest()` but
/// not the writer a `WritableStream` needs (`worker-sys-0.8.7/src/types/crypto.rs` has `new` and `digest` only).
/// WebCrypto's one-shot `digest` needs no writer, and it exists in the worker runtime AND in the Node harness the
/// differentials run in — which is why the cache key can be byte-identical rather than merely self-consistent.
pub async fn sha256_hex16(input: &str) -> Option<String> {
    use worker::wasm_bindgen::JsCast;
    use worker::js_sys;
    use worker::wasm_bindgen::JsValue;
    let crypto = js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("crypto")).ok()?;
    let subtle = js_sys::Reflect::get(&crypto, &JsValue::from_str("subtle")).ok()?;
    let digest = js_sys::Reflect::get(&subtle, &JsValue::from_str("digest")).ok()?;
    let digest: js_sys::Function = digest.dyn_into().ok()?;
    let bytes = js_sys::Uint8Array::from(input.as_bytes());
    let promise = digest
        .call2(&subtle, &JsValue::from_str("SHA-256"), &bytes)
        .ok()?;
    let promise: js_sys::Promise = promise.dyn_into().ok()?;
    let buffer = worker::wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .ok()?;
    let array = js_sys::Uint8Array::new(&buffer);
    let mut out = String::new();
    for i in 0..16.min(array.length()) {
        out.push_str(&format!("{:02x}", array.get_index(i)));
    }
    Some(out)
}

/// What `preprocessImages` needs from the caller: the request path's already-resolved pieces.
pub struct VisionInputs<'a> {
    /// The ADVERTISED model id, as the client spelled it.
    pub model: &'a str,
    /// The WIRE id the route resolved.
    pub upstream_model: &'a str,
    /// `user?.id || ""`.
    pub uid: &'a str,
    /// `route.kind === "custom" && providerModelVision(route.provider, upstreamModel)`.
    pub declared_vision: bool,
    /// The user's BYOK record, already extracted (`extract_byok_keys`).
    pub byok: &'a Map<String, Value>,
    /// The KV `settings:US_PROXY` value ALONE — no env fallback, which is what the source reads here.
    pub us_proxy_setting: bool,
    /// The custom-provider records, for `resolveRoute` on the vision model's own prefix.
    pub providers: &'a [Value],
    /// The env as JSON, for `VISION_MODEL`/`VISION_CAPABLE_MODELS` and the route chain.
    pub env: &'a Value,
    /// `upstreamTimeoutMs(env)` — the describe call's budget, read by the caller which already built the env.
    pub timeout_ms: f64,
}

/// `preprocessImages(messages, env, ukeys, model, upstreamModel, uid, declaredVision)`.
///
/// Returns the rewritten messages and whether anything changed. **A FAILED DESCRIBE RETURNS `Err`** — the
/// source throws, and the caller lets it become the request's failure rather than fabricating a description.
pub async fn preprocess_images(
    env: &Env,
    messages: &Value,
    inputs: &VisionInputs<'_>,
) -> Result<(Value, bool)> {
    let Some(list) = messages.as_array() else {
        return Ok((messages.clone(), false));
    };
    if inputs.declared_vision
        || is_vision_capable(inputs.model, inputs.upstream_model, Some(inputs.env))
    {
        return Ok((messages.clone(), false));
    }
    let vision_model = inputs
        .env
        .get("VISION_MODEL")
        .map(crate::stream::js_text)
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_VISION_MODEL.to_string());

    let mut changed = false;
    let mut out: Vec<Value> = Vec::with_capacity(list.len());
    for message in list {
        // `m.role !== "user" || typeof m.content !== "object" || !Array.isArray(m.content)` — note that a
        // STRING content is `typeof "string"`, so it takes this branch too.
        let is_user = message.get("role").and_then(|r| r.as_str()) == Some("user");
        let content = message.get("content").and_then(|c| c.as_array());
        let (Some(content), true) = (content, is_user) else {
            out.push(message.clone());
            continue;
        };
        if !content
            .iter()
            .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("image"))
        {
            out.push(message.clone());
            continue;
        }
        let mut new_content: Vec<Value> = Vec::with_capacity(content.len());
        for block in content {
            if block.get("type").and_then(|t| t.as_str()) != Some("image") {
                new_content.push(block.clone());
                continue;
            }
            let source = block.get("source").cloned().unwrap_or(Value::Null);
            let desc = describe_image(env, &source, &vision_model, inputs).await;
            if describe_failed(&desc) {
                // **THE REQUEST FAILS.** The client sees the error and retries or removes the image; it does not
                // get a fabricated description it will believe.
                return Err(Error::RustError(describe_failure_message(&desc)));
            }
            new_content.push(json!({ "type": "text", "text": vision_block(&desc) }));
            changed = true;
        }
        let mut copy = message.clone();
        if let Some(obj) = copy.as_object_mut() {
            obj.insert("content".to_string(), Value::Array(new_content));
        }
        out.push(copy);
    }
    Ok((Value::Array(out), changed))
}

/// `describeImage(env, ukeys, source, visionModel, uid)` — the cache, the route, the call, the markers.
pub async fn describe_image(
    env: &Env,
    source: &Value,
    vision_model: &str,
    inputs: &VisionInputs<'_>,
) -> String {
    let media_type = source
        .get("media_type")
        .and_then(|m| m.as_str())
        .filter(|m| !m.is_empty())
        .unwrap_or("image/png");
    let data = source
        .get("data")
        .and_then(|d| d.as_str())
        .unwrap_or("")
        .to_string();
    if data.is_empty() {
        return "(图片数据为空)".to_string();
    }
    // The cache key: user-scoped, content-derived, SHA-256, first 16 bytes.
    let cache_key = match sha256_hex16(&format!("{vision_model}:{data}")).await {
        Some(hex) => image_cache_key(inputs.uid, &hex, data.len()),
        None => String::new(),
    };
    if !cache_key.is_empty() {
        if let Ok(kv) = env.kv("KEYS") {
            if let Ok(Some(hit)) = kv.get(&cache_key).text().await {
                if !hit.is_empty() {
                    return hit;
                }
            }
        }
    }

    let prefix = vision_model.split('/').next().unwrap_or("").to_string();
    // `resolveRoute(env, prefix, usProxy)` with the default request path — and the US exit comes from the KV
    // SETTING alone here (`getGlobalSetting(env, "US_PROXY")`), not the request path's env fallback.
    let route = crate::routing::resolve_model(
        vision_model,
        inputs.us_proxy_setting,
        "/v1/messages",
        Some(inputs.env),
        inputs.providers,
    );
    if let Some(reason) = &route.unroutable {
        return format!("(图片描述失败：{reason})");
    }
    let upstream_model = crate::session::strip_bracket(&if route.route.strip_prefix {
        vision_model
            .char_indices()
            .nth(prefix.chars().count() + 1)
            .map(|(i, _)| &vision_model[i..])
            .unwrap_or("")
            .to_string()
    } else {
        vision_model.to_string()
    });
    let kind = route.route.kind;
    let session = if kind == "opencode" {
        crate::session::opencode_session_header(&serde_json::Map::new(), inputs.uid)
    } else {
        Vec::new()
    };

    let content = json!([
        { "type": "image", "source": { "type": "base64", "media_type": media_type, "data": data } },
        { "type": "text", "text": DESCRIBE_PROMPT },
    ]);
    let mini = json!({
        "model": vision_model,
        "max_tokens": DESCRIBE_MAX_TOKENS,
        "messages": [{ "role": "user", "content": content }],
    });

    let Some((key_field, shape)) = vision_backend(kind) else {
        return "(图片描述失败：视觉模型后端不支持)".to_string();
    };
    // **THE ENV HALF IS THE CHANNEL'S `envKey`, NOT ITS `userKey`** — for nv and gmi `userKey` names a binding
    // the channel does not deploy while `envKey` is null (no deployment fallback, by design). Using the wrong
    // column spent the deployment's key for a user who owns none, while the same user's text request on the
    // same channel answered 502 "not configured".
    // **THE SAME RESOLVER THE REQUEST PATH USES**, so "user key, then the channel's `envKey`" cannot drift
    // between the two — including the `envKey: null` rule that makes nv and gmi pure BYOK. `custom` has no BYOK
    // channel behind it and takes its credential from its own record.
    let bearer = if kind == "custom" {
        crate::store::provider_key(inputs.env, route.provider.as_ref())
    } else {
        crate::byok::bearer_key_for(inputs.env, inputs.byok, kind)
            .as_str()
            .unwrap_or("")
            .to_string()
    };
    if bearer.is_empty() {
        return if kind == "custom" {
            let raw = route
                .provider
                .as_ref()
                .and_then(|p| p.get("prefix"))
                .unwrap_or(&Value::Null);
            let name = if crate::translate::truthy_js(raw) {
                crate::stream::js_text(raw)
            } else {
                "custom provider".to_string()
            };
            format!("(图片描述失败：{name} 未配置 key)")
        } else {
            format!("(图片描述失败：{key_field} 未配置)")
        };
    }

    let url = route.route.upstream.clone();
    let (headers, body) = if shape == "anthropic" {
        // Anthropic-format upstream: the text lives in `content[]` blocks.
        (
            crate::session::passthrough_headers(Some(&bearer), None, &[]),
            serde_json::to_string(&{
                let mut m = mini.clone();
                if let Some(obj) = m.as_object_mut() {
                    obj.insert("model".to_string(), json!(upstream_model));
                }
                m
            })
            .unwrap_or_default(),
        )
    } else {
        // OpenAI-format upstream: `toOpenAIRequest` forwards image parts as `image_url`, and only og/ gets the
        // zen per-conversation session header.
        let translated = crate::translate::to_openai_request(&mini, &upstream_model);
        let mut headers = vec![
            ("Authorization".to_string(), format!("Bearer {bearer}")),
            ("Content-Type".to_string(), "application/json".to_string()),
        ];
        headers.extend(session.iter().cloned());
        (
            headers,
            serde_json::to_string(&translated).unwrap_or_default(),
        )
    };

    let response = fetch_describe(&url, &headers, &body, inputs.timeout_ms).await;
    let (status, text) = match response {
        Ok(pair) => pair,
        Err(marker) => return marker,
    };
    if !(200..=299).contains(&status) {
        return format!("(图片描述失败：{status})");
    }
    let Ok(json) = serde_json::from_str::<Value>(&text) else {
        return "(图片描述失败：响应解析失败)".to_string();
    };
    let desc = if shape == "anthropic" {
        json.get("content")
            .and_then(|c| c.as_array())
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_string()
            })
            .unwrap_or_default()
    } else {
        json.get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .map(crate::stream::js_text)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    let desc = if desc.is_empty() {
        "(图片描述为空)".to_string()
    } else {
        desc
    };
    // The cache write: skipped for a failure or an empty description, and best-effort (`/* KV write failed */`).
    if !cache_key.is_empty() && !describe_failed(&desc) && !desc.is_empty() {
        if let Ok(kv) = env.kv("KEYS") {
            // `expirationTtl: 7 * 24 * 60 * 60` — the builder's own method, and `execute` is the write.
            if let Ok(builder) = kv.put(&cache_key, desc.clone()) {
                let _ = builder.expiration_ttl(CACHE_TTL_SECONDS).execute().await;
            }
        }
    }
    desc
}

/// `fetchDescribeOrError`: a network throw becomes the historical `(图片描述失败：<message>)` marker string
/// rather than a propagated error — the caller's `preprocessImages` then treats it as a hard failure.
async fn fetch_describe(
    url: &str,
    headers: &[(String, String)],
    body: &str,
    timeout_ms: f64,
) -> std::result::Result<(u16, String), String> {
    let mut init = RequestInit::new();
    init.with_method(Method::Post);
    let h = Headers::new();
    for (k, v) in headers {
        if h.set(k, v).is_err() {
            return Err(format!("(图片描述失败：header {k})"));
        }
    }
    init.with_headers(h);
    init.with_body(Some(body.into()));
    let request = match Request::new_with_init(url, &init) {
        Ok(r) => r,
        Err(e) => return Err(format!("(图片描述失败：{})", js_error_message(&e))),
    };
    let fetcher = Fetch::Request(request);
    let fetch = fetcher.send();
    let delay = Delay::from(std::time::Duration::from_millis(timeout_ms.max(0.0) as u64));
    futures_util::pin_mut!(fetch);
    futures_util::pin_mut!(delay);
    match futures_util::future::select(fetch, delay).await {
        futures_util::future::Either::Left((Ok(mut res), _)) => {
            let status = res.status_code();
            match res.text().await {
                Ok(text) => Ok((status, text)),
                Err(e) => Err(format!("(图片描述失败：{})", js_error_message(&e))),
            }
        }
        futures_util::future::Either::Left((Err(e), _)) => {
            Err(format!("(图片描述失败：{})", js_error_message(&e)))
        }
        futures_util::future::Either::Right((_, _)) => {
            Err(format!("(图片描述失败：timeout after {timeout_ms}ms)"))
        }
    }
}

/// `e.message`, not `String(e)` — the same rule the request path's detail follows.
fn js_error_message(e: &worker::Error) -> String {
    let text = e.to_string();
    text.strip_prefix("Error: ").unwrap_or(&text).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **THE PURE HALF, PINNED — AND THE MARKERS ARE THE POINT.** `describe_failed` is what stands between a
    /// failed describe and a client that believes its image was seen (round-119).
    ///
    /// MUTATION: make `describe_failed` always answer `false`.
    /// RESULT:   the marker cases below fail, and `preprocess_images` would inject `(图片描述失败：…)` as a
    ///           description — the fabricated-description bug the round-119 fix removed.
    #[test]
    fn the_vision_markers_and_blocks_are_the_sources() {
        assert!(describe_failed("(图片描述失败：500)"));
        assert!(describe_failed("(图片描述为空)"));
        assert!(describe_failed("(图片数据为空)"));
        assert!(!describe_failed("[图片内容描述]\n一只猫"));
        // **THE THROWN SENTENCE DROPS THE MARKER AND ITS COLON — AND KEEPS THE CLOSING PAREN.** The source's
        // regex is `/^\((图片描述失败|图片描述为空|图片数据为空)[：:]?/`: anchored at the START, so the `)` at
        // the end is not its business. A port that "cleaned that up" would answer a different sentence from the
        // one operators have been reading since round 119.
        assert_eq!(
            describe_failure_message("(图片描述失败：upstream 500)"),
            "vision preprocessing failed: upstream 500)"
        );
        assert_eq!(
            describe_failure_message("(图片描述为空)"),
            "vision preprocessing failed: )"
        );
        // A description that is NOT a marker is passed through unchanged.
        assert_eq!(
            describe_failure_message("一只猫"),
            "vision preprocessing failed: 一只猫"
        );
        assert_eq!(vision_block("一只猫"), "[图片内容描述]\n一只猫");
    }

    /// The allowlist matches the ADVERTISED id and the WIRE id, trims, and drops empty entries.
    #[test]
    fn the_vision_allowlist_is_the_sources() {
        let env = json!({ "VISION_CAPABLE_MODELS": " og/mimo-v2.5 , r4/deepseek-v4.1-flash ,, " });
        assert!(is_vision_capable("og/mimo-v2.5", "mimo-v2.5", Some(&env)));
        assert!(is_vision_capable("x/y", "r4/deepseek-v4.1-flash", Some(&env)));
        assert!(!is_vision_capable("og/deepseek-v4.1-flash", "deepseek-flash", Some(&env)));
        // An absent or empty var is an empty list, not a wildcard.
        assert!(!is_vision_capable("a", "b", None));
        assert!(!is_vision_capable("a", "b", Some(&json!({ "VISION_CAPABLE_MODELS": "" }))));
    }

    /// The cache key: `data.length > 16` decides whether there is one at all, and an empty uid is `anon`.
    #[test]
    fn the_image_cache_key_is_the_sources() {
        assert_eq!(image_cache_key("u1", "abcd", 17), "img-desc:u1:abcd");
        assert_eq!(image_cache_key("", "abcd", 17), "img-desc:anon:abcd");
        // 16 characters is NOT longer than 16 — no cache key.
        assert_eq!(image_cache_key("u1", "abcd", 16), "");
    }

    /// The backend table: nine channels plus `custom`, and the two shapes.
    #[test]
    fn the_vision_backends_are_the_sources() {
        assert_eq!(vision_backend("opencode"), Some(("OPENCODE_GO_API_KEY", "openai")));
        assert_eq!(vision_backend("deepseek"), Some(("DEEPSEEK_API_KEY", "anthropic")));
        assert_eq!(vision_backend("nvidia"), Some(("NVAPI_KEY", "openai")));
        assert_eq!(vision_backend("amd"), Some(("AMD_API_KEY", "anthropic")));
        // **`custom` IS NOT ONE OF THE NINE AND MUST NOT BE DROPPED.**
        assert_eq!(vision_backend("custom"), Some(("", "openai")));
        assert_eq!(vision_backend("nope"), None);
    }
}
