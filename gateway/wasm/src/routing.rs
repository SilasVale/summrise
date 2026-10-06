//! THE ROUTING DECISION, MOVED FROM `gateway/src/upstream.ts`.
//!
//! WHY THIS ONE. It answers WHERE A REQUEST GOES — which upstream host, which wire path, whether the model
//! prefix is stripped, and whether the body is translated on the way — and every arm of the handler depends
//! on its answer. It is also where the two recorded security fixes live, and both are pinned by the corpus:
//!
//!   * **audit round F4**: on the US-egress path the prefix is MODEL-DERIVED ARBITRARY TEXT, and unencoded it
//!     "could inject `&path=…` into the egress URL and re-point the proxy request". Both parameters are
//!     percent-encoded, and the corpus carries a prefix that would escape without it;
//!   * **`hasOwnProperty`**, once more: an unknown prefix falls through to `defaultRoute` rather than finding
//!     something on `Object.prototype`.
//!
//! AND TWO CHANNELS ARE ALWAYS DIRECT — `amd` and `r4` ignore the US exit even when it is on, because the
//! egress relay's TARGETS map has no entry for them and "an unknown target silently falls back to zen, which
//! would answer with the wrong model under the user's key" (the trap both builders record).

/// `usProxyBase(env)` — the US exit's base URL. **THE DEFAULT IS A PRODUCTION HOSTNAME**, which is why this
/// file is declared in `agent/tests/production_host.rs` beside `upstream.ts` and `channels.ts`.
pub fn us_proxy_base(env: Option<&serde_json::Value>) -> String {
    env.and_then(|e| e.get("US_PROXY_BASE"))
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .unwrap_or("https://v.saisi.online")
        .to_string()
}

/// What a route IS: the four fields every builder answers with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteInfo {
    /// `"passthrough"` or `"translate"` — whether the body is reshaped on the way.
    pub is_translate: bool,
    pub kind: &'static str,
    /// Whether the advertised prefix comes off the model name before it is sent.
    pub strip_prefix: bool,
    pub upstream: String,
}

/// `ROUTE_TABLE`'s keys — **"does the BUILT-IN route table own this prefix?"** (`isBuiltInPrefix`).
///
/// It is a list rather than a lookup over `pick_route`'s arms because the ORDER is the contract: a built-in
/// prefix is decided by `pick_route` even when a custom provider claims the same prefix, and only a prefix the
/// table does not own may reach `provider_for_prefix`.
pub fn is_built_in_prefix(prefix: &str) -> bool {
    matches!(
        prefix,
        "or" | "ds" | "qw" | "og" | "nv" | "gmi" | "cm" | "amd" | "r4"
    )
}

/// `SUPPORTED_PROVIDER_APIS` — the dialects a custom provider may declare, and the path each one APPENDS to
/// its `baseURL`.
///
/// **THE JOIN IS DSH'S, DELIBERATELY** (`upstream.ts`'s own note): `baseURL` is a PREFIX and the dialect's path
/// is appended — what the OpenAI SDK does with `baseURL` — which is why DSH's settings can name the gateway
/// itself as a provider. One dialect today; a second is a row here plus its translator.
pub const SUPPORTED_PROVIDER_APIS: [(&str, &str); 1] = [("openai-completions", "/chat/completions")];

/// `JSON.stringify(provider?.api)` as a template literal renders it — `undefined` for an absent field, the
/// quoted string for a string, `null` for an explicit null. It rides inside the unroutable sentence, so it is
/// part of the bytes a client receives.
fn js_json_stringify(v: Option<&serde_json::Value>) -> String {
    match v {
        None => "undefined".to_string(),
        Some(value) => serde_json::to_string(value).unwrap_or_else(|_| "undefined".to_string()),
    }
}

/// `providerRoute(provider)` — a custom provider's route, or the reason it must NOT be dialled.
///
/// **AN UNROUTABLE RECORD IS AN ERROR ROUTE, NEVER THE DEFAULT CHANNEL** — the source's own words, and the
/// failure they name is exactly what this crate did: a request for a custom provider's model fell through to
/// the default channel and answered `502 config_error — "CMD_API_KEY not configured"` (measured 2026-10-06 on
/// the built worker) where the shipping route dialled the provider's own `baseURL`.
///
/// Returns the route and, for the error arm, the sentence the client gets:
/// `custom provider <prefix|?> cannot be routed: <api> is not a protocol this gateway serves (see
/// store/providers.ts)`.
pub fn provider_route(provider: &serde_json::Value) -> (RouteInfo, Option<String>) {
    let base = provider
        .get("baseURL")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let api = provider.get("api").and_then(|v| v.as_str()).unwrap_or("");
    let join = SUPPORTED_PROVIDER_APIS
        .iter()
        .find(|(name, _)| *name == api)
        .map(|(_, path)| *path);
    match join {
        Some(path) if !base.is_empty() => (
            RouteInfo {
                is_translate: true,
                kind: "custom",
                strip_prefix: true,
                upstream: format!("{base}{path}"),
            },
            None,
        ),
        _ => {
            // `provider?.prefix || "?"` — the truthiness of the RAW value, then its text.
            let raw = provider.get("prefix").unwrap_or(&serde_json::Value::Null);
            let name = if crate::translate::truthy_js(raw) {
                crate::stream::js_text(raw)
            } else {
                "?".to_string()
            };
            (
                RouteInfo {
                    is_translate: false,
                    kind: "custom",
                    strip_prefix: true,
                    upstream: String::new(),
                },
                Some(format!(
                    "custom provider {name} cannot be routed: {} is not a protocol this gateway serves (see store/providers.ts)",
                    js_json_stringify(provider.get("api"))
                )),
            )
        }
    }
}

/// `VERIFY_PATH` — the path an Anthropic-format upstream serves.
const VERIFY_PATH: &str = "/v1/messages";
const CHAT_PATH: &str = "/v1/chat/completions";

/// The US-egress form of a direct URL, or the direct URL itself.
///
/// **THE ENCODING IS audit round F4.** The prefix is model-derived arbitrary text, and the path is the
/// upstream's; both ride as query parameters of the relay's `/api/zen`, so an unencoded `&` in either would
/// add a parameter of the caller's choosing. `encodeURIComponent` — and this port's encoder — leave the
/// characters that cannot break a query string alone and escape the rest.
fn via(
    env: Option<&serde_json::Value>,
    us_proxy: Option<&str>,
    prefix: &str,
    direct: &str,
    path: &str,
) -> String {
    match us_proxy {
        Some(_) => format!(
            "{}/api/zen?target={}&path={}",
            us_proxy_base(env),
            encode_uri_component(prefix),
            encode_uri_component(path)
        ),
        None => direct.to_string(),
    }
}

/// `encodeURIComponent`, as a scanner.
///
/// **THE UNRESERVED SET IS THE SPEC'S**: `A-Z a-z 0-9 - _ . ! ~ * ' ( )`. Everything else becomes
/// `%XX` in UPPERCASE hex, and a non-ASCII character is escaped BYTE BY BYTE from its UTF-8 encoding —
/// which is what the JavaScript does, and the corpus carries a non-ASCII prefix.
fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let c = b as char;
        if c.is_ascii_alphanumeric()
            || matches!(c, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')')
        {
            out.push(c);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `defaultRoute` — no prefix, or an unknown one, goes to the DEFAULT channel.
///
/// Since the 2026-09-10 retirement that is Command Code, and the model name passes through VERBATIM
/// (`stripPrefix: false`), because Command Code's catalogue covers the `claude-*`/`gpt-5.6-*`/`gemini-*`/
/// `deepseek/*` spellings an unprefixed name is likely to use.
fn default_route(
    env: Option<&serde_json::Value>,
    us_proxy: Option<&str>,
    prefix: &str,
) -> RouteInfo {
    RouteInfo {
        is_translate: true,
        kind: "commandgoat",
        strip_prefix: false,
        upstream: via(
            env,
            us_proxy,
            prefix,
            "https://api.commandcode.ai/provider/v1/chat/completions",
            CHAT_PATH,
        ),
    }
}

/// **`pickRoute(prefix, env, usProxy, requestPath)`** — the nine built-in prefixes and the default.
///
/// The table is a match rather than a map, and the ORDER is the source's own listing. Every arm is pinned by
/// the corpus, including the two that ignore the US exit and the two that pick by `requestPath`.
pub fn pick_route(
    prefix: &str,
    env: Option<&serde_json::Value>,
    us_proxy: Option<&str>,
    request_path: &str,
) -> RouteInfo {
    let path = if request_path.is_empty() {
        VERIFY_PATH
    } else {
        request_path
    };
    match prefix {
        "or" => RouteInfo {
            is_translate: false,
            kind: "openrouter",
            strip_prefix: true,
            upstream: via(
                env,
                us_proxy,
                prefix,
                &format!("https://openrouter.ai/api{path}"),
                path,
            ),
        },
        "ds" => RouteInfo {
            is_translate: false,
            kind: "deepseek",
            strip_prefix: true,
            upstream: via(
                env,
                us_proxy,
                prefix,
                "https://api.deepseek.com/anthropic/v1/messages",
                "/anthropic/v1/messages",
            ),
        },
        "qw" => {
            if path == CHAT_PATH {
                RouteInfo {
                    is_translate: false,
                    kind: "qwen",
                    strip_prefix: true,
                    upstream: via(
                        env,
                        us_proxy,
                        prefix,
                        "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1/chat/completions",
                        "/compatible-mode/v1/chat/completions",
                    ),
                }
            } else {
                RouteInfo {
                    is_translate: false,
                    kind: "qwen",
                    strip_prefix: true,
                    upstream: via(
                        env,
                        us_proxy,
                        prefix,
                        "https://token-plan.ap-southeast-1.maas.aliyuncs.com/apps/anthropic/v1/messages",
                        "/apps/anthropic/v1/messages",
                    ),
                }
            }
        }
        "og" => RouteInfo {
            is_translate: true,
            kind: "opencode",
            strip_prefix: true,
            upstream: via(
                env,
                us_proxy,
                prefix,
                "https://opencode.ai/zen/go/v1/chat/completions",
                CHAT_PATH,
            ),
        },
        "nv" => RouteInfo {
            is_translate: false,
            kind: "nvidia",
            strip_prefix: true,
            upstream: via(
                env,
                us_proxy,
                prefix,
                "https://integrate.api.nvidia.com/v1/chat/completions",
                CHAT_PATH,
            ),
        },
        "gmi" => RouteInfo {
            is_translate: false,
            kind: "gmi",
            strip_prefix: true,
            upstream: via(
                env,
                us_proxy,
                prefix,
                "https://api.gmi-serving.com/v1/chat/completions",
                CHAT_PATH,
            ),
        },
        "cm" => RouteInfo {
            is_translate: true,
            kind: "commandgoat",
            strip_prefix: true,
            upstream: via(
                env,
                us_proxy,
                prefix,
                "https://api.commandcode.ai/provider/v1/chat/completions",
                CHAT_PATH,
            ),
        },
        // **amd AND r4 ARE ALWAYS DIRECT.** They ignore the US exit even when it is on: the egress relay's
        // TARGETS map has no entry for either, and an unknown target silently falls back to zen — which would
        // answer with the wrong model under the user's own key.
        "amd" => RouteInfo {
            is_translate: false,
            kind: "amd",
            strip_prefix: true,
            upstream: if path == CHAT_PATH {
                "https://developer.amd.com.cn/radeon/api/v1/chat/completions".to_string()
            } else {
                "https://developer.amd.com.cn/radeon/api/v1/messages".to_string()
            },
        },
        "r4" => RouteInfo {
            is_translate: false,
            kind: "r4",
            strip_prefix: true,
            upstream: if path == CHAT_PATH {
                "https://api.r4.codes/v1/chat/completions".to_string()
            } else {
                "https://api.r4.codes/v1/messages".to_string()
            },
        },
        _ => default_route(env, us_proxy, prefix),
    }
}

/// **THE MODEL -> THE ROUTE CHAIN, WHICH IS WHERE THE KIND COMES FROM.**
///
/// The handler derives everything from the model string, in this order, and each step is a decision the
/// corpora pin separately:
///
/// ```text
///     retiredModelHint(model)      -> a 400, and NOT a redirect
///     prefix = model.split("/")[0] || ""
///     usProxy = usEgress(model) || the US_PROXY setting
///     route = pickRoute(prefix, env, usProxy, requestPath)
///     upstreamModel = wireModelName(prefix, stripBracket(stripPrefix ? model.slice(prefix.length+1) : model))
/// ```
///
/// **THE RETIRED GATE REFUSES RATHER THAN REDIRECTS**, which is worth stating because the map's name
/// (`RETIRED_MODELS`) and its shape (advertised id -> replacement) both suggest otherwise. The source returns
/// `jsonError(400, "Model … was retired on 2026-09-10 … Use … instead.")` and the client chooses.
pub const RETIRED_MODELS: [(&str, &str); 13] = [
    ("ds/deepseek-v4-flash", "cm/deepseek/deepseek-v4.1-flash"),
    ("og/deepseek-v4-flash", "og/deepseek-v4.1-flash"),
    (
        "cm/deepseek/deepseek-v4-flash",
        "cm/deepseek/deepseek-v4.1-flash",
    ),
    (
        "cm/deepseek/deepseek-v4-flash-vision-exp",
        "cm/deepseek/deepseek-v4.1-flash",
    ),
    (
        "or/deepseek/deepseek-v4-flash-0731",
        "cm/deepseek/deepseek-v4.1-flash",
    ),
    ("amd/deepseek-v4-flash", "cm/deepseek/deepseek-v4.1-flash"),
    (
        "amd/deepseek-v4-flash-vision-exp",
        "cm/deepseek/deepseek-v4.1-flash",
    ),
    ("deepseek-v4-flash", "cm/deepseek/deepseek-v4.1-flash"),
    (
        "deepseek-v4-flash-vision-exp",
        "cm/deepseek/deepseek-v4.1-flash",
    ),
    ("deepseek-v4-flash-0731", "cm/deepseek/deepseek-v4.1-flash"),
    (
        "deepseek/deepseek-v4-flash",
        "cm/deepseek/deepseek-v4.1-flash",
    ),
    (
        "deepseek/deepseek-v4-flash-vision-exp",
        "cm/deepseek/deepseek-v4.1-flash",
    ),
    (
        "deepseek/deepseek-v4-flash-0731",
        "cm/deepseek/deepseek-v4.1-flash",
    ),
];

/// `retiredModelHint(model)` — the replacement to NAME, or `None`.
pub fn retired_model_hint(model: &str) -> Option<&'static str> {
    RETIRED_MODELS
        .iter()
        .find(|(id, _)| *id == model)
        .map(|(_, to)| *to)
}

/// The 400 the retired gate sends, with the source's own sentence.
pub fn retired_model_error(model: &str, hint: &str) -> crate::responses::Built {
    crate::responses::json_error(
        400,
        &format!(
            "Model {model} was retired on 2026-09-10 — the DeepSeek V4 Flash line is superseded by V4.1 Flash. Use {hint} instead."
        ),
        "invalid_request_error",
    )
}

/// `stripBracket(s)` — `s.replace(/\[[^\]]*\]$/, "")`.
///
/// **THE REGEX IS `\[[^\]]*\]$`, AND THE `[^\]]*` IS THE WHOLE POINT.** A bracket at the end comes off only
/// when the LAST `[` before it has no `]` in between — so `x[a]b]` is returned UNCHANGED, because the class
/// cannot cross the inner `]`. My first version used `rfind('[')` and would have stripped it, which is a
/// divergence no shape-based test would see; the corpus carries the case.
pub fn strip_bracket(s: &str) -> String {
    if !s.ends_with(']') {
        return s.to_string();
    }
    let inner = &s[..s.len() - 1];
    match inner.rfind('[') {
        Some(open) if !inner[open + 1..].contains(']') => s[..open].to_string(),
        _ => s.to_string(),
    }
}

/// `OG_FORCE_US_PROXY` — the registry's own `usEgress` records, derived rather than copied.
pub fn og_force_us_proxy(model_id: &str) -> bool {
    crate::registry::MODEL_REGISTRY
        .iter()
        .any(|m| m.id == model_id && m.us_egress == Some(true))
}

/// What the handler resolved for one model string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModel {
    pub prefix: String,
    pub route: RouteInfo,
    /// The name the upstream is asked for — `wireModelName` over the stripped, bracket-free id.
    pub upstream_model: String,
    /// The custom-provider record this route came from, when it came from one. The request path needs it for
    /// the credential (`store::provider_key`) and for the per-model facets (`store::provider_model_vision`),
    /// which is why the source carries `provider` on its `RouteInfo` too.
    pub provider: Option<serde_json::Value>,
    /// `providerRoute`'s ERROR arm: the sentence to answer with. `None` means the route is dialable.
    pub unroutable: Option<String>,
}

/// The chain, with the US-proxy SETTING as an argument (the KV read is the caller's), and the custom-provider
/// records as data for the same reason.
///
/// **THE ORDER IS THE SOURCE'S, AND IT IS THE WHOLE POINT OF THIS FUNCTION**: a built-in prefix is decided by
/// `pick_route` FIRST, then a custom provider, then `pick_route` again as the default channel — so a custom
/// record can never shadow a built-in, and a prefix nobody owns still reaches the default rather than 404ing.
pub fn resolve_model(
    model: &str,
    us_proxy_setting: bool,
    request_path: &str,
    env: Option<&serde_json::Value>,
    providers: &[serde_json::Value],
) -> ResolvedModel {
    // `model.split("/")[0] || ""` — JS's `|| ""` only matters for an EMPTY first segment, which is what a
    // leading slash gives.
    let prefix = model.split('/').next().unwrap_or("").to_string();
    let us_proxy = if og_force_us_proxy(model) || us_proxy_setting {
        Some("1")
    } else {
        None
    };
    let provider = if is_built_in_prefix(&prefix) {
        None
    } else {
        crate::store::provider_for_prefix(providers, &prefix)
    };
    let (route, unroutable) = match &provider {
        Some(p) => provider_route(p),
        None => (pick_route(&prefix, env, us_proxy, request_path), None),
    };
    let stripped = if route.strip_prefix {
        // `effectiveModel.slice(prefix.length + 1)` — the prefix AND its slash.
        model
            .char_indices()
            .nth(prefix.chars().count() + 1)
            .map(|(i, _)| &model[i..])
            .unwrap_or("")
    } else {
        model
    };
    let upstream_model = crate::registry::wire_model_name(&prefix, &strip_bracket(stripped));
    ResolvedModel {
        prefix,
        route,
        upstream_model,
        provider,
        unroutable,
    }
}

/// **`museResponsesExit(env)` — the `/v1/responses` arm's US exit, and it has FOUR cases.**
///
/// ```text
///     "vercel"   -> the relay's own /api/zen with target=og and the path encoded
///     "zen-us"   -> the zen-us host
///     http(s):// -> used VERBATIM (an operator pointing it anywhere)
///     anything else, including unset -> the oracle host
/// ```
///
/// **THE LAST CASE IS THE DEFAULT AND IT IS A HOST, NOT AN ERROR.** The og/muse-spark Contributor tier is
/// responses-only upstream AND Meta region-blocks it for CN, so this arm is FORCED through a US exit whatever
/// the setting says — which is why there is no "direct" branch here at all.
///
/// This function names three production hosts, and it lives in THIS file because `routing.rs` is already
/// declared in `agent/tests/production_host.rs` for the `US_PROXY_BASE` default. A new file would have needed
/// its own declaration, and the gate's own note says the list "may only shrink".
pub fn muse_responses_exit(env: Option<&serde_json::Value>) -> String {
    match env
        .and_then(|e| e.get("MUSE_RESPONSES_EXIT"))
        .and_then(|v| v.as_str())
    {
        Some("vercel") => format!(
            "{}/api/zen?target=og&path={}",
            us_proxy_base(env),
            encode_uri_component("/v1/responses")
        ),
        Some("zen-us") => "https://zen-us.saisi.online/v1/responses".to_string(),
        Some(v) if v.starts_with("http://") || v.starts_with("https://") => v.to_string(),
        _ => "https://oracle.saisi.online/v1/responses".to_string(),
    }
}

/// The og zen host's own `/v1/responses` — the exit's alternative.
pub fn og_responses_direct() -> String {
    "https://opencode.ai/zen/go/v1/responses".to_string()
}

#[cfg(test)]
mod tests {
    //! **PINNED BY HAND, WITH THE REASON.** `pickRoute` is exported and pure, so its differential is a
    //! straight oracle replay — and the oracle attempt this round was reverted rather than half-landed after
    //! a name collision in that file cost more than the round had left. The pins below are the facts a port
    //! of this function can get wrong, taken from the source and its recorded incidents.
    use super::*;

    fn env_with(pairs: &[(&str, &str)]) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        for (k, v) in pairs {
            m.insert((*k).to_string(), serde_json::json!(v));
        }
        serde_json::Value::Object(m)
    }

    /// **THE ORACLE REPLAY**, and the reason the hand pins above are kept beside it: the corpus pins the
    /// BYTES (what the shipping `pickRoute` answers, case by case) while the pins say WHY each one matters.
    /// `fixtures/translate-corpus.json`'s `pick_route` cases are produced by `oracle-translate.mjs` calling
    /// the exported function directly — no capture, no stub.
    #[test]
    fn every_routing_case_matches_the_shipping_typescript() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/translate-corpus.json"
        );
        let text = std::fs::read_to_string(path).expect("the corpus is committed");
        let doc: serde_json::Value = serde_json::from_str(&text).expect("the corpus parses");
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            if case["fn"].as_str() != Some("pick_route") {
                continue;
            }
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let env = {
                let mut m = serde_json::Map::new();
                if let Some(p) = input["usProxy"].as_str() {
                    m.insert("US_PROXY".into(), serde_json::json!(p));
                }
                if let Some(b) = input["base"].as_str() {
                    m.insert("US_PROXY_BASE".into(), serde_json::json!(b));
                }
                serde_json::Value::Object(m)
            };
            let got = pick_route(
                input["prefix"].as_str().unwrap_or(""),
                Some(&env),
                input["usProxy"].as_str(),
                input["requestPath"].as_str().unwrap_or(""),
            );
            let want = &case["expected"]["value"];
            let as_json = serde_json::json!({
                "type": if got.is_translate { "translate" } else { "passthrough" },
                "kind": got.kind,
                "stripPrefix": got.strip_prefix,
                "upstream": got.upstream,
            });
            assert_eq!(&as_json, want, "{name}");
            checked += 1;
        }
        assert!(
            checked >= 40,
            "the routing corpus shrank to {checked} cases"
        );
    }

    /// **THE CUSTOM PROVIDERS, REPLAYED — ELEVEN CASES FROM THE SHIPPING CHAIN.**
    ///
    /// MUTATION: delete the `provider_for_prefix` branch in `resolve_model` (leave `pick_route` alone) and run
    ///           this test.
    /// RESULT:   the eight routable cases fail with the DEFAULT channel's route, which is exactly what the
    ///           built worker did before the port — measured live as
    ///           `502 config_error — "CMD_API_KEY not configured"` where the shipping route dialled
    ///           `https://acme.test/v1/chat/completions` with `Bearer sk-acme-inline`.
    ///
    /// **THE ORDER IS THE SUBJECT, NOT THE LOOKUP**: one case gives a built-in prefix a custom record claiming
    /// it (the built-in must win), and one gives a prefix nobody owns (the DEFAULT channel, not a 404).
    #[test]
    fn the_custom_providers_match_the_shipping_chain() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/provider-corpus.json"
        );
        let text = std::fs::read_to_string(path).expect("the provider corpus is committed");
        let doc: serde_json::Value = serde_json::from_str(&text).expect("it parses");
        let cases = doc["cases"].as_array().expect("cases");
        let mut routable = 0;
        let mut unroutable = 0;
        for case in cases {
            let name = case["name"].as_str().unwrap_or("?");
            let providers: Vec<serde_json::Value> =
                case["providers"].as_array().cloned().unwrap_or_default();
            let env = case["env"].clone();
            let model = case["model"].as_str().unwrap_or("");
            let got = resolve_model(model, false, "/v1/messages", Some(&env), &providers);
            let want = &case["expected"];
            let want_type = want["type"].as_str().unwrap_or("");
            assert_eq!(
                got.route.is_translate,
                want_type == "translate",
                "{name}: is_translate"
            );
            assert_eq!(got.route.kind, want["kind"].as_str().unwrap_or(""), "{name}: kind");
            assert_eq!(
                got.route.strip_prefix,
                want["stripPrefix"].as_bool().unwrap_or(false),
                "{name}: stripPrefix"
            );
            assert_eq!(
                got.route.upstream,
                want["upstream"].as_str().unwrap_or(""),
                "{name}: upstream"
            );
            // `route.type === "error"` is `unroutable`, and the sentence is the client's bytes.
            assert_eq!(
                got.unroutable.as_deref(),
                want["reason"].as_str(),
                "{name}: the unroutable sentence"
            );
            if got.unroutable.is_some() {
                unroutable += 1;
            } else {
                routable += 1;
            }
            // The credential and the vision facet come off the record the prefix matched — the same lookup the
            // request path does, so this asserts the lookup itself and not a second copy of it.
            let provider = crate::store::provider_for_prefix(&providers, &got.prefix);
            assert_eq!(
                crate::store::provider_key(&env, provider.as_ref()),
                want["providerKey"].as_str().unwrap_or(""),
                "{name}: providerKey"
            );
            for (wire, want_vision) in want["visionOf"].as_object().expect("visionOf") {
                assert_eq!(
                    crate::store::provider_model_vision(provider.as_ref(), wire),
                    want_vision.as_bool().unwrap_or(false),
                    "{name}: vision of {wire}"
                );
            }
        }
        assert_eq!(cases.len(), 11, "the provider corpus changed size");
        // **BOTH ARMS ARE COVERED**, or the corpus would pass for a port that routed everything (or nothing).
        assert_eq!(routable, 8, "the routable half");
        assert_eq!(unroutable, 3, "the error-route half");
    }

    fn corpus() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/translate-corpus.json"
        );
        let text = std::fs::read_to_string(path).expect("the corpus is committed");
        serde_json::from_str(&text).expect("the corpus parses")
    }

    /// **THE MODEL -> ROUTE CHAIN, REPLAYED FROM THE SHIPPING TYPESCRIPT.** Sixty-nine cases calling the
    /// exported pieces directly: every prefix on both request paths with and without the exit, a bracket
    /// suffix, a leading slash, a bare id, and the thirteen retired ids.
    #[test]
    fn the_model_resolution_chain_matches_the_shipping_typescript() {
        let doc = corpus();
        let mut checked = 0;
        let mut retired = 0;
        for case in doc["cases"].as_array().expect("cases") {
            if case["fn"].as_str() != Some("resolve_model") {
                continue;
            }
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let model = input["model"].as_str().unwrap_or("");
            let expected = &case["expected"];
            if let Some(hint) = expected["retiredHint"].as_str() {
                assert_eq!(retired_model_hint(model), Some(hint), "{name}");
                let err = retired_model_error(model, hint);
                assert_eq!(err.status, 400, "{name}");
                assert!(
                    err.body.contains(hint),
                    "{name}: the message names the replacement"
                );
                assert!(err.body.contains("retired on 2026-09-10"), "{name}");
                retired += 1;
                continue;
            }
            let got = resolve_model(
                model,
                input["usProxySetting"].as_bool().unwrap_or(false),
                input["requestPath"].as_str().unwrap_or(""),
                None,
                // The route corpus drives the BUILT-IN table; a custom provider's records are its own
                // corpus (`provider-corpus.json`) and its own test below.
                &[],
            );
            assert_eq!(
                got.prefix,
                expected["prefix"].as_str().unwrap_or(""),
                "{name}: prefix"
            );
            assert_eq!(
                got.upstream_model,
                expected["upstreamModel"].as_str().unwrap_or(""),
                "{name}: upstream model"
            );
            let want_route = &expected["route"];
            assert_eq!(
                got.route.kind,
                want_route["kind"].as_str().unwrap_or(""),
                "{name}: kind"
            );
            assert_eq!(
                got.route.strip_prefix,
                want_route["stripPrefix"].as_bool().unwrap_or(false),
                "{name}: stripPrefix"
            );
            assert_eq!(
                got.route.upstream,
                want_route["upstream"].as_str().unwrap_or(""),
                "{name}: upstream"
            );
            checked += 1;
        }
        assert!(checked >= 56, "the chain corpus shrank to {checked} cases");
        assert_eq!(retired, 13, "the retired map changed size");
    }

    #[test]
    fn the_responses_exit_has_four_cases_and_the_default_is_a_host() {
        // **THE DEFAULT IS A HOSTNAME, WHICH IS WHY THIS PIN LIVES IN THIS FILE.** `routing.rs` is declared in
        // `agent/tests/production_host.rs` for the `US_PROXY_BASE` default; a FIXTURE that captured this URL
        // would have needed its own declaration, so the oracle points the exit at a test host and the real
        // default is pinned here instead.
        let env = |v: &str| serde_json::json!({ "MUSE_RESPONSES_EXIT": v });
        assert_eq!(
            muse_responses_exit(Some(&env("vercel"))),
            "https://v.saisi.online/api/zen?target=og&path=%2Fv1%2Fresponses"
        );
        assert_eq!(
            muse_responses_exit(Some(&env("zen-us"))),
            "https://zen-us.saisi.online/v1/responses"
        );
        // An http(s) value is used VERBATIM — an operator pointing it anywhere.
        assert_eq!(
            muse_responses_exit(Some(&env("https://exit.example"))),
            "https://exit.example"
        );
        assert_eq!(
            muse_responses_exit(Some(&env("http://plain.example"))),
            "http://plain.example"
        );
        // Anything else — including unset, and including a NON-URL string — is the default host.
        assert_eq!(
            muse_responses_exit(None),
            "https://oracle.saisi.online/v1/responses"
        );
        assert_eq!(
            muse_responses_exit(Some(&env("something-else"))),
            "https://oracle.saisi.online/v1/responses"
        );
        assert_eq!(
            og_responses_direct(),
            "https://opencode.ai/zen/go/v1/responses"
        );
    }

    #[test]
    fn the_nine_prefixes_answer_their_own_kind_and_wire_path() {
        let cases = [
            (
                "or",
                "openrouter",
                false,
                "https://openrouter.ai/api/v1/messages",
            ),
            (
                "ds",
                "deepseek",
                false,
                "https://api.deepseek.com/anthropic/v1/messages",
            ),
            (
                "qw",
                "qwen",
                false,
                "https://token-plan.ap-southeast-1.maas.aliyuncs.com/apps/anthropic/v1/messages",
            ),
            (
                "og",
                "opencode",
                true,
                "https://opencode.ai/zen/go/v1/chat/completions",
            ),
            (
                "nv",
                "nvidia",
                false,
                "https://integrate.api.nvidia.com/v1/chat/completions",
            ),
            (
                "gmi",
                "gmi",
                false,
                "https://api.gmi-serving.com/v1/chat/completions",
            ),
            (
                "cm",
                "commandgoat",
                true,
                "https://api.commandcode.ai/provider/v1/chat/completions",
            ),
            (
                "amd",
                "amd",
                false,
                "https://developer.amd.com.cn/radeon/api/v1/messages",
            ),
            ("r4", "r4", false, "https://api.r4.codes/v1/messages"),
        ];
        for (prefix, kind, translate, upstream) in cases {
            let r = pick_route(prefix, None, None, "/v1/messages");
            assert_eq!(r.kind, kind, "{prefix}");
            assert_eq!(r.is_translate, translate, "{prefix}");
            assert!(r.strip_prefix, "{prefix}: every built-in strips its prefix");
            assert_eq!(r.upstream, upstream, "{prefix}");
        }
    }

    #[test]
    fn the_default_keeps_the_model_name_verbatim() {
        // An unprefixed name has a real chance of resolving at Command Code, whose catalogue covers the
        // claude-*/gpt-5.6-*/gemini-*/deepseek/* spellings — so `stripPrefix` is FALSE here, alone among the
        // arms.
        for prefix in ["zz", "", "constructor", "toString"] {
            let r = pick_route(prefix, None, None, "/v1/messages");
            assert_eq!(r.kind, "commandgoat", "{prefix}");
            assert!(r.is_translate, "{prefix}");
            assert!(
                !r.strip_prefix,
                "{prefix}: the model name passes through verbatim"
            );
            assert!(
                r.upstream.ends_with("/provider/v1/chat/completions"),
                "{prefix}"
            );
        }
    }

    #[test]
    fn the_us_exit_encodes_both_parameters() {
        // **audit round F4**: the prefix is model-derived ARBITRARY text, and unencoded it "could inject
        // `&path=…` into the egress URL and re-point the proxy request". The case below is the injection
        // itself, and the assertion is that it survives as ONE opaque value.
        let env = env_with(&[]);
        let r = pick_route("a&path=/evil", Some(&env), Some("1"), "/v1/messages");
        assert!(
            r.upstream
                .starts_with("https://v.saisi.online/api/zen?target=a%26path%3D%2Fevil&path="),
            "{}",
            r.upstream
        );
        assert!(
            !r.upstream.contains("&path=/evil"),
            "the injection must not survive: {}",
            r.upstream
        );
        // A non-ASCII prefix is escaped BYTE BY BYTE from its UTF-8 encoding, which is what
        // `encodeURIComponent` does.
        let r = pick_route("中文", Some(&env), Some("1"), "/v1/messages");
        assert!(
            r.upstream.contains("target=%E4%B8%AD%E6%96%87"),
            "{}",
            r.upstream
        );
        // The path rides the same way.
        let r = pick_route("or", Some(&env), Some("1"), "/v1/a b");
        assert!(r.upstream.ends_with("path=%2Fv1%2Fa%20b"), "{}", r.upstream);
    }

    #[test]
    fn the_us_proxy_base_is_overridable_and_defaults_to_the_relay() {
        assert_eq!(us_proxy_base(None), "https://v.saisi.online");
        assert_eq!(
            us_proxy_base(Some(&env_with(&[]))),
            "https://v.saisi.online"
        );
        assert_eq!(
            us_proxy_base(Some(&env_with(&[(
                "US_PROXY_BASE",
                "https://exit.example"
            )]))),
            "https://exit.example"
        );
        // `|| ` is truthiness: an EMPTY base falls back to the default rather than producing "/api/zen…".
        assert_eq!(
            us_proxy_base(Some(&env_with(&[("US_PROXY_BASE", "")]))),
            "https://v.saisi.online"
        );
    }

    #[test]
    fn amd_and_r4_stay_direct_even_with_the_exit_on() {
        // The egress relay's TARGETS map has no entry for either, and an unknown target silently falls back
        // to zen — which would answer with the wrong model under the user's own key. Both builders record
        // that trap, and this is the assertion that keeps it closed.
        let env = env_with(&[]);
        for (prefix, host) in [
            ("amd", "https://developer.amd.com.cn/radeon/api"),
            ("r4", "https://api.r4.codes"),
        ] {
            for path in ["/v1/messages", "/v1/chat/completions"] {
                let r = pick_route(prefix, Some(&env), Some("1"), path);
                assert!(
                    r.upstream.starts_with(host),
                    "{prefix} {path}: {}",
                    r.upstream
                );
                assert!(
                    !r.upstream.contains("/api/zen"),
                    "{prefix} {path} took the exit"
                );
            }
        }
    }

    #[test]
    fn the_two_path_picking_channels_switch_on_the_request_path() {
        let env = env_with(&[]);
        // qw/ and amd/ and r4/ serve BOTH formats natively, so the path decides — and qw/ rides the US exit
        // while the other two do not.
        let chat = pick_route("qw", Some(&env), None, "/v1/chat/completions");
        assert!(
            chat.upstream
                .ends_with("/compatible-mode/v1/chat/completions"),
            "{}",
            chat.upstream
        );
        let messages = pick_route("qw", Some(&env), None, "/v1/messages");
        assert!(
            messages.upstream.ends_with("/apps/anthropic/v1/messages"),
            "{}",
            messages.upstream
        );
        // An EMPTY requestPath means `/v1/messages` (`requestPath || VERIFY_PATH`).
        let empty = pick_route("or", Some(&env), None, "");
        assert!(
            empty.upstream.ends_with("/v1/messages"),
            "{}",
            empty.upstream
        );
    }
}
