//! summrise-model-drift — what the model CHANNELS advertise vs what their upstreams offer.
//!
//! Ported out of `scripts/model-drift.mjs` (137 lines, deleted in the same landing). The functions,
//! the sentences and the exit codes are that file's, transliterated; the port's own record of WHY is
//! below, because a tool whose output a person reads is only as good as the decisions behind it.
//!
//! # WHY
//!
//! The gateway advertises a model catalogue at `/v1/models` from a HARDCODED registry
//! (`gateway/src/channels.ts`'s `MODEL_REGISTRY`). Adding or retiring a model means editing that
//! source and redeploying — so when an upstream adds a model, or silently RETIRES one, nothing tells
//! anyone. The dangerous direction is the second: the gateway goes on promising a model the upstream
//! no longer serves, and the first caller to pick it gets an error.
//!
//! # WHAT IT IS NOT
//!
//! It is NOT a way to add models. A registry record carries SIX facets (advertised id, upstream wire
//! slug, US-egress policy, web-search capability, health card, vision) and an upstream's model list
//! tells you only the first. Auto-advertising everything an upstream offers would promise models for
//! which this gateway has no policy at all — which is why `offered_not_advertised` is an OPPORTUNITY
//! LIST FOR A HUMAN, never an automatic addition.
//!
//! # HOW IT COMPARES, AND WHY THE OBVIOUS WAY IS WRONG
//!
//! Stripping the channel prefix and diffing the names produces FALSE POSITIVES: the router
//! normalises further (Claude Code appends a `[context-window]` marker and `stripBracket` removes it
//! before routing) and some advertised ids are deliberate aliases no upstream lists. So a name that
//! does not match is reported as CHECK, not as a defect — **nothing here may claim an upstream
//! retired a model on the strength of a string comparison**, and the report ends by saying so.
//!
//! # THE ONE DISTINCTION THAT MUST SURVIVE THE PORT
//!
//! `gmi` / `qw` / `amd` / `ds` answer **401 to an unauthenticated /models** (verified), so they are
//! NOT compared rather than reported as empty — *"could not check" and "offers nothing" are different
//! facts, and this repo has been bitten by collapsing them before.* The same rule is why an
//! unreadable CATALOGUE prints `Nothing was compared.` and exits 1 instead of printing a report that
//! looks like everything matches.

use std::collections::BTreeSet;

use serde_json::{json, Map, Value};

/// One upstream compared by PREFIX, and the URL an unauthenticated GET can read.
pub struct Channel {
    /// The catalogue's own spelling of the channel, e.g. `or`.
    pub prefix: &'static str,
    /// The upstream's `/models` endpoint.
    pub url: &'static str,
}

/// The four upstreams this tool compares.
///
/// `gmi` / `qw` / `amd` / `ds` answer **401 to an unauthenticated /models** (verified), so they are
/// NOT compared rather than reported as empty — *"could not check" and "offers nothing" are
/// different facts, and this repo has been bitten by collapsing them before.*
pub const CHANNELS: [Channel; 4] = [
    Channel {
        prefix: "or",
        url: "https://openrouter.ai/api/v1/models",
    },
    Channel {
        prefix: "nv",
        url: "https://integrate.api.nvidia.com/v1/models",
    },
    Channel {
        prefix: "cm",
        url: "https://api.commandcode.ai/provider/v1/models",
    },
    Channel {
        prefix: "og",
        url: "https://opencode.ai/zen/go/v1/models",
    },
];

/// The router's own name normalisation, mirrored: Claude Code appends a `[context-window]` marker
/// and `stripBracket` removes it before routing.
///
/// The JavaScript is `String(name).replace(/\[[^\]]*\]$/, "").trim()`, and both details matter:
/// the pattern is ANCHORED AT THE END (a bracket in the middle is part of the name, and a group
/// cannot span a `]`), and the REPLACE RUNS BEFORE THE TRIM — so `"name[1m]  "` keeps its marker,
/// because the trailing spaces mean `$` does not match.
pub fn normalise(name: &str) -> String {
    let mut end = name.len();
    if name.ends_with(']') {
        let close = name.len() - 1;
        // `[^\]]*` cannot contain a `]`, so the group opens after the LAST `]` before the final one
        // (or anywhere before it, when there is none) — and the regex takes the leftmost such `[`.
        let floor = name[..close].rfind(']').map_or(0, |k| k + 1);
        if let Some(open) = name[floor..close].find('[') {
            end = floor + open;
        }
    }
    name[..end].trim().to_string()
}

/// Advertised ids for one channel, normalised, prefix stripped.
///
/// The catalogue order is preserved (the JavaScript mapped, it did not sort), and an id that is
/// exactly `prefix/` survives as the empty name: the filter ran on the PREFIXED id, and an empty
/// name is what the catalogue said.
pub fn advertised_for(ids: &[String], prefix: &str) -> Vec<String> {
    let with_slash = format!("{prefix}/");
    ids.iter()
        .filter(|id| id.starts_with(&with_slash))
        .map(|id| normalise(&id[with_slash.len()..]))
        .collect()
}

/// The comparison, in both directions.
///
/// `advertised_not_offered` is the one that matters (a promise the upstream may not keep);
/// `offered_not_advertised` is an opportunity list. Neither is a verdict — see the module header.
pub struct Diff {
    pub advertised_not_offered: Vec<String>,
    pub offered_not_advertised: Vec<String>,
}

/// The comparison, pure so it can be tested without a network.
///
/// Both sides are normalised HERE as well (the advertised side already was): the JavaScript mapped
/// `normalise` over each array inside its Sets, so a caller handing in raw names gets the same
/// answer. Sets, so duplicates collapse; sorted, so the report's order is the names' and not the
/// upstream's. `BTreeSet` orders by bytes and JavaScript's `.sort()` by UTF-16 code units — the same
/// order for every ASCII model id, which is every id either side has ever listed.
pub fn diff_channel(advertised: &[String], offered: &[String]) -> Diff {
    let o: BTreeSet<String> = offered.iter().map(|n| normalise(n)).collect();
    let a: BTreeSet<String> = advertised.iter().map(|n| normalise(n)).collect();
    Diff {
        advertised_not_offered: a.difference(&o).cloned().collect(),
        offered_not_advertised: o.difference(&a).cloned().collect(),
    }
}

/// The model list out of one upstream reply.
///
/// The JavaScript was `const list = body && (body.data || body.models); if (!Array.isArray(list))
/// throw …; list.map(m => typeof m === "string" ? m : m && m.id).filter(Boolean)`. So: `data` wins
/// by JS TRUTHINESS (an empty array is truthy, a `null` is not), a truthy `data` that is not an
/// array is REFUSED rather than falling back to `models`, and an entry is its own string or its
/// `id`, with a falsy result dropped.
///
/// ONE NARROWING, and it is deliberate: the JavaScript would have coerced ANY truthy `id` through
/// `String(...)`, which is how an object id becomes `[object Object]`. This accepts a non-empty
/// string or a number; nothing lists a model as `[object Object]`.
pub fn model_ids(body: &Value) -> Result<Vec<String>, String> {
    let list = body
        .get("data")
        .filter(|v| truthy(v))
        .or_else(|| body.get("models"))
        .filter(|v| v.is_array());
    let Some(list) = list else {
        return Err("no model list in the reply".to_string());
    };
    Ok(list
        .as_array()
        .expect("filtered to an array above")
        .iter()
        .filter_map(entry_id)
        .collect())
}

/// JavaScript truthiness for a JSON value: `null`, `false`, `0` and `""` are falsy — and so is a
/// MISSING key, which is why the caller filters before it falls back.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// One entry of a model list: the entry itself when it is a string, otherwise its `id`.
fn entry_id(entry: &Value) -> Option<String> {
    let candidate = match entry {
        Value::String(s) => s.clone(),
        other => match other.get("id") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => return None,
        },
    };
    // `.filter(Boolean)`: an empty id is not an id.
    (!candidate.is_empty()).then_some(candidate)
}

/// What one channel's comparison found, or why it could not be made.
pub enum ChannelReport {
    Checked {
        advertised: usize,
        offered: usize,
        diff: Diff,
    },
    /// The upstream could not be read. `advertised` is still counted — the catalogue side was read
    /// — but the report line says NOTHING about this upstream's catalogue.
    Unreadable { error: String, advertised: usize },
}

/// The whole report: the catalogue's size, then one entry per channel IN `CHANNELS` ORDER.
pub struct Report {
    pub gateway: String,
    pub advertised_total: usize,
    pub channels: Vec<(&'static str, ChannelReport)>,
}

impl Report {
    /// Whether any channel advertises a name its upstream does not list — the ONLY thing `--strict`
    /// fails on. An opportunity is not a check, and an unreadable upstream is not either: it says
    /// nothing about its catalogue.
    pub fn any_check(&self) -> bool {
        self.channels.iter().any(|(_, channel)| match channel {
            ChannelReport::Checked { diff, .. } => !diff.advertised_not_offered.is_empty(),
            ChannelReport::Unreadable { .. } => false,
        })
    }

    /// The report a person reads. These sentences ARE the tool.
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "model-drift: {} advertises {} models\n",
            self.gateway, self.advertised_total
        ));
        for (prefix, channel) in &self.channels {
            match channel {
                ChannelReport::Unreadable { error, .. } => {
                    out.push_str(&format!(
                        "  {prefix}/  could NOT be checked ({error}) — says nothing about its catalogue\n"
                    ));
                }
                ChannelReport::Checked {
                    advertised,
                    offered,
                    diff,
                } => {
                    out.push_str(&format!(
                        "  {prefix}/  advertised {advertised}, upstream offers {offered}\n"
                    ));
                    if !diff.advertised_not_offered.is_empty() {
                        out.push_str(
                            "      CHECK — advertised but no upstream entry of that name (an alias, or a retired model):\n",
                        );
                        for name in &diff.advertised_not_offered {
                            out.push_str(&format!("        {prefix}/{name}\n"));
                        }
                    }
                    if !diff.offered_not_advertised.is_empty() {
                        out.push_str(&format!(
                            "      opportunity — upstream offers, we do not advertise: {} (adding one needs the registry's other five facets)\n",
                            diff.offered_not_advertised.len()
                        ));
                    }
                }
            }
        }
        out.push_str(
            "  NOTE: a CHECK is not a verdict. Nothing here may claim an upstream retired a model on a name comparison.\n",
        );
        out
    }

    /// The same report for a script: the JavaScript's field names, in the JavaScript's order.
    pub fn to_json(&self) -> Value {
        let mut channels = Map::new();
        for (prefix, channel) in &self.channels {
            let mut c = Map::new();
            match channel {
                ChannelReport::Unreadable { error, advertised } => {
                    c.insert("error".to_string(), Value::String(error.clone()));
                    c.insert("advertised".to_string(), json!(advertised));
                }
                ChannelReport::Checked {
                    advertised,
                    offered,
                    diff,
                } => {
                    c.insert("advertised".to_string(), json!(advertised));
                    c.insert("offered".to_string(), json!(offered));
                    c.insert(
                        "advertisedNotOffered".to_string(),
                        json!(diff.advertised_not_offered),
                    );
                    c.insert(
                        "offeredNotAdvertised".to_string(),
                        json!(diff.offered_not_advertised),
                    );
                }
            }
            channels.insert((*prefix).to_string(), Value::Object(c));
        }
        let mut report = Map::new();
        report.insert("gateway".to_string(), Value::String(self.gateway.clone()));
        report.insert("advertisedTotal".to_string(), json!(self.advertised_total));
        report.insert("channels".to_string(), Value::Object(channels));
        Value::Object(report)
    }
}

/// **SAY SO.** An unreadable catalogue is not an empty one, and a report that printed "everything
/// matches" here would be the exact failure this repo keeps finding.
pub fn cannot_read(gateway: &str, message: &str) -> String {
    format!("model-drift: cannot read {gateway}/v1/models — {message}. Nothing was compared.")
}

/// 0 for a report that was produced — drift is normal — and 1 only for `--strict` with a CHECK.
pub fn exit_code(strict: bool, any_check: bool) -> u8 {
    u8::from(strict && any_check)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn the_four_compared_channels_are_the_ones_an_unauthenticated_get_can_read() {
        // gmi / qw / amd / ds answer 401 to an unauthenticated /models, so they are NOT in this list
        // rather than in it with an empty answer.
        assert_eq!(
            CHANNELS.iter().map(|c| c.prefix).collect::<Vec<_>>(),
            ["or", "nv", "cm", "og"]
        );
        for channel in CHANNELS {
            assert!(
                channel.url.starts_with("https://") && channel.url.ends_with("/models"),
                "{} is compared against {}",
                channel.prefix,
                channel.url
            );
        }
    }

    // ── normalise: the router's own normalisation, mirrored ──────────────────────────────────────

    #[test]
    fn normalise_strips_a_trailing_context_window_marker() {
        assert_eq!(normalise("gpt-5.6-luna:floor[1m]"), "gpt-5.6-luna:floor");
        assert_eq!(
            normalise("claude-sonnet-4[context-window]"),
            "claude-sonnet-4"
        );
        assert_eq!(normalise("x[]"), "x");
        assert_eq!(normalise("plain"), "plain");
    }

    #[test]
    fn normalise_only_strips_a_bracket_group_that_ends_the_string() {
        // The pattern is `\[[^\]]*\]$`: a bracket in the middle is part of the name.
        assert_eq!(normalise("a[1m]b"), "a[1m]b");
        // The group cannot span a `]`, so only the LAST one goes.
        assert_eq!(normalise("a[1m][2m]"), "a[1m]");
        // A `]` closing an earlier group stops the scan: there is no match here at all.
        assert_eq!(normalise("a[b]c]"), "a[b]c]");
    }

    #[test]
    fn normalise_replaces_before_it_trims() {
        assert_eq!(normalise("  spaced  "), "spaced");
        // The marker is at the end of the UNTRIMMED string only if nothing follows it — trailing
        // whitespace means `$` does not match, and the trim happens after. Order matters.
        assert_eq!(normalise("name[1m]  "), "name[1m]");
    }

    // ── advertisedFor: the catalogue side, filtered to one channel ───────────────────────────────

    #[test]
    fn advertised_for_keeps_only_its_own_prefix_and_strips_it() {
        let advertised = ids(&["or/a", "or/b[1m]", "nv/c", "orb/d", "or", "or/"]);
        assert_eq!(
            advertised_for(&advertised, "or"),
            // `or/` strips to the empty name and IS kept: the filter ran on the prefixed id, and an
            // empty name is what the catalogue said. `orb/d` never matched — the slash is required.
            ids(&["a", "b", ""])
        );
    }

    #[test]
    fn advertised_for_preserves_the_catalogue_order() {
        assert_eq!(
            advertised_for(&ids(&["or/z", "or/a"]), "or"),
            ids(&["z", "a"])
        );
    }

    // ── diffChannel: the comparison, pure so it can be tested without a network ──────────────────

    #[test]
    fn diff_channel_sorts_both_directions() {
        let d = diff_channel(&ids(&["m", "z", "a"]), &ids(&["c", "b", "a"]));
        assert_eq!(d.advertised_not_offered, ids(&["m", "z"]));
        assert_eq!(d.offered_not_advertised, ids(&["b", "c"]));
    }

    #[test]
    fn diff_channel_on_a_channel_that_offers_nothing() {
        // An upstream that ANSWERED with an empty list is not an upstream that could not be checked:
        // every advertised name is a CHECK, and there is no opportunity list to print.
        let d = diff_channel(&ids(&["b", "a"]), &[]);
        assert_eq!(d.advertised_not_offered, ids(&["a", "b"]));
        assert!(d.offered_not_advertised.is_empty());
    }

    #[test]
    fn diff_channel_deduplicates_after_normalising_both_sides() {
        let d = diff_channel(&ids(&["a[1m]", "a"]), &ids(&["b", "b[context-window]"]));
        // ONE `a` and ONE `b`: the marker made each pair the same name, and the Sets collapsed it.
        assert_eq!(d.advertised_not_offered, ids(&["a"]));
        assert_eq!(d.offered_not_advertised, ids(&["b"]));
    }

    // ── the reply's model list ───────────────────────────────────────────────────────────────────

    #[test]
    fn model_ids_reads_data_first_then_models() {
        assert_eq!(model_ids(&json!({"data": [{"id": "x"}]})), Ok(ids(&["x"])));
        assert_eq!(model_ids(&json!({"models": ["y"]})), Ok(ids(&["y"])));
        // `body.data || body.models` is JS truthiness, and an EMPTY ARRAY IS TRUTHY: `data` wins even
        // when it holds nothing, so this reply is an empty catalogue rather than a fallback.
        assert_eq!(model_ids(&json!({"data": [], "models": ["y"]})), Ok(vec![]));
        // ... and a FALSY `data` hands the question to `models`.
        assert_eq!(
            model_ids(&json!({"data": null, "models": ["y"]})),
            Ok(ids(&["y"]))
        );
    }

    #[test]
    fn model_ids_drops_what_is_not_a_usable_id() {
        assert_eq!(
            model_ids(
                &json!({"data": ["a", "", {"id": "b"}, {"id": ""}, {"nope": 1}, 7, null, ["c"]]})
            ),
            Ok(ids(&["a", "b"]))
        );
    }

    #[test]
    fn model_ids_refuses_a_reply_that_is_not_a_model_list() {
        assert_eq!(
            model_ids(&json!({"error": "nope"})),
            Err("no model list in the reply".to_string())
        );
        // The JavaScript checked `Array.isArray`, so a truthy `data` that is not an array is refused
        // rather than quietly falling back to `models`.
        assert_eq!(
            model_ids(&json!({"data": {"id": "x"}, "models": ["y"]})),
            Err("no model list in the reply".to_string())
        );
        assert_eq!(
            model_ids(&json!(null)),
            Err("no model list in the reply".to_string())
        );
    }

    // ── the report: these sentences ARE the tool ─────────────────────────────────────────────────

    fn mixed_report() -> Report {
        Report {
            gateway: "https://api.saisi.online".to_string(),
            advertised_total: 3,
            channels: vec![
                (
                    "or",
                    ChannelReport::Checked {
                        advertised: 2,
                        offered: 1,
                        diff: Diff {
                            advertised_not_offered: ids(&["b"]),
                            offered_not_advertised: ids(&["c"]),
                        },
                    },
                ),
                (
                    "nv",
                    ChannelReport::Unreadable {
                        error: "HTTP 401".to_string(),
                        advertised: 1,
                    },
                ),
                (
                    "cm",
                    ChannelReport::Checked {
                        advertised: 0,
                        offered: 4,
                        diff: Diff {
                            advertised_not_offered: vec![],
                            offered_not_advertised: ids(&["m1", "m2"]),
                        },
                    },
                ),
            ],
        }
    }

    #[test]
    fn the_text_report_is_the_sentences_the_tool_exists_to_print() {
        assert_eq!(
            mixed_report().render_text(),
            [
                "model-drift: https://api.saisi.online advertises 3 models",
                "  or/  advertised 2, upstream offers 1",
                "      CHECK — advertised but no upstream entry of that name (an alias, or a retired model):",
                "        or/b",
                "      opportunity — upstream offers, we do not advertise: 1 (adding one needs the registry's other five facets)",
                "  nv/  could NOT be checked (HTTP 401) — says nothing about its catalogue",
                "  cm/  advertised 0, upstream offers 4",
                "      opportunity — upstream offers, we do not advertise: 2 (adding one needs the registry's other five facets)",
                "  NOTE: a CHECK is not a verdict. Nothing here may claim an upstream retired a model on a name comparison.",
                "",
            ]
            .join("\n")
        );
    }

    #[test]
    fn a_channel_with_no_drift_prints_only_what_is_true_about_it() {
        let report = Report {
            gateway: "https://api.saisi.online".to_string(),
            advertised_total: 1,
            channels: vec![(
                "og",
                ChannelReport::Checked {
                    advertised: 1,
                    offered: 1,
                    diff: Diff {
                        advertised_not_offered: vec![],
                        offered_not_advertised: vec![],
                    },
                },
            )],
        };
        let text = report.render_text();
        assert!(text.contains("  og/  advertised 1, upstream offers 1"));
        assert!(
            !text.contains("CHECK —"),
            "no CHECK header for a channel with nothing to check"
        );
        assert!(
            !text.contains("opportunity —"),
            "no opportunity line when there is none"
        );
    }

    #[test]
    fn any_check_is_true_only_for_an_advertised_name_no_upstream_lists() {
        assert!(mixed_report().any_check());
        // An OPPORTUNITY alone is not a check: the exit code is about a promise this gateway may be
        // making that an upstream cannot keep, not about a model we have not adopted.
        let opportunities_only = Report {
            gateway: "g".to_string(),
            advertised_total: 0,
            channels: vec![(
                "or",
                ChannelReport::Checked {
                    advertised: 0,
                    offered: 2,
                    diff: Diff {
                        advertised_not_offered: vec![],
                        offered_not_advertised: ids(&["a", "b"]),
                    },
                },
            )],
        };
        assert!(!opportunities_only.any_check());
        // ... and neither is an unreadable upstream, which says NOTHING about its catalogue.
        let unreadable_only = Report {
            gateway: "g".to_string(),
            advertised_total: 0,
            channels: vec![(
                "nv",
                ChannelReport::Unreadable {
                    error: "HTTP 401".to_string(),
                    advertised: 0,
                },
            )],
        };
        assert!(!unreadable_only.any_check());
    }

    #[test]
    fn the_json_report_keeps_the_field_names_and_order_the_javascript_wrote() {
        assert_eq!(
            mixed_report().to_json().to_string(),
            r#"{"gateway":"https://api.saisi.online","advertisedTotal":3,"channels":{"or":{"advertised":2,"offered":1,"advertisedNotOffered":["b"],"offeredNotAdvertised":["c"]},"nv":{"error":"HTTP 401","advertised":1},"cm":{"advertised":0,"offered":4,"advertisedNotOffered":[],"offeredNotAdvertised":["m1","m2"]}}}"#
        );
    }

    #[test]
    fn cannot_read_says_nothing_was_compared() {
        assert_eq!(
            cannot_read("https://api.saisi.online", "HTTP 500"),
            "model-drift: cannot read https://api.saisi.online/v1/models — HTTP 500. Nothing was compared."
        );
    }

    #[test]
    fn exit_code_is_one_only_when_strict_meets_a_check() {
        // Drift alone is NORMAL: the report is the product, and an operator running it by hand must
        // not read a failure out of it.
        assert_eq!(exit_code(false, true), 0);
        assert_eq!(exit_code(true, false), 0);
        assert_eq!(exit_code(true, true), 1);
    }
}
