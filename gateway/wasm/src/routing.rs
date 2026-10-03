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
