//! THE CONSOLE'S MODEL STORE, IN RUST — `gateway/src/store/models.ts`'s decisions, and the TypeScript this module
//! EMITS.
//!
//! WHY THIS MODULE EXISTS (2026-10-14). `store/models.ts` was the largest store host left in the console's tail
//! and it is NOT one decision: the advertised-id merge, the `/v1/models` extras, the disabled set's sort, the
//! two custom-model upserts, the facet-override "emptied means deleted" rule, the reasoning-effort chain and the
//! CATALOGUE PROJECTION are each a product condition, and the file only becomes a host when every one of them has
//! an owner. This is that owner.
//!
//! **TWO OF ITS RULES ALREADY LIVED ONE FILE OVER.** `advertised_ids` and `extra_model_entries` are `store.rs`'s
//! (the worker's `/v1/models` reads them through `v1.rs`), and they are RE-EXPORTED here rather than moved: a
//! move would churn the call sites for no behaviour, and the emission needs the rules wherever they live. The
//! manifest names THIS module as the producer — it is the emitter — and this header is where a reader learns that
//! two of the functions it renders are defined next door.
//!
//! **AND THE PROJECTION IS THE ONE THAT HAS COST THIS REPOSITORY TWICE.** `ROUTE_INFO[].models` holds BARE names
//! (`"mimo-v2.5"`) while the disabled set holds FULL advertised ids (`"og/mimo-v2.5"`); comparing them directly
//! matches NOTHING, which is the mismatch that made the console's Models page set the wrong channel (round 58)
//! and left its default card permanently empty (round 65). The rule is written down ONCE here, in the function
//! every card goes through.

use serde_json::Value;
use worker::Env;

/// `models:custom` / `models:overrides` / `models:disabled` — the three KV keys this store owns. Named because
/// the EMISSION publishes them and the worker's own admin routes will read them.
pub const CUSTOM_MODELS_KEY: &str = "models:custom";
pub const MODEL_OVERRIDES_KEY: &str = "models:overrides";
pub const DISABLED_MODELS_KEY: &str = "models:disabled";

/// The two rules that live in `store.rs` and are rendered from here.
pub use crate::store::{advertised_ids, extra_model_entries};

/// `[...ids].sort()` — **JAVASCRIPT'S DEFAULT SORT COMPARES UTF-16 CODE UNITS**, which is not Rust's byte order
/// for a non-ASCII id. A model id is ASCII today; the comparison is written the engine's way anyway, because a
/// sort that agrees only on today's data is a sort that changes behaviour on tomorrow's.
pub fn js_sorted(mut ids: Vec<String>) -> Vec<String> {
    ids.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    ids
}

/// `KNOWN_PREFIXES` — `ROUTE_INFO.map((r) => r.prefix).filter((p) => p && p !== "none")`, and **IT IS NOT THE
/// CORE'S [`summrise_core::catalogue::RESERVED_PREFIXES`]** — which is what the first version of this function
/// assumed, and the fixture caught it: the reserved list has TEN entries (the two card-less channels `ds/` and
/// `amd/` included) while the route table has SEVEN cards, so the refusal below printed two prefixes the console
/// would have refused. **THE ORDER IS BYTES A CLIENT RECEIVES** (the message PRINTS the list).
///
/// `the_known_prefixes_are_the_console_route_table` reads `gateway/src/channels.ts` and compares, so the two
/// spellings cannot drift silently.
pub const KNOWN_PREFIXES: [&str; 7] = ["og/", "or/", "nv/", "gmi/", "qw/", "cm/", "r4/"];

/// The refusal's own list — `KNOWN_PREFIXES.join(", ")`.
pub fn known_prefixes() -> Vec<&'static str> {
    KNOWN_PREFIXES.to_vec()
}

/// The four ROUTING facets a built-in's record may never carry: its `wire`, its egress, and the two
/// response-shape switches are pinned in the channel registry, and a stored copy would be a second,
/// contradictory answer to "where does this request go".
pub const PINNED_FACETS: [&str; 4] = ["wire", "usEgress", "search", "responsesOnly"];

/// The reasoning levels the router speaks — `parseModelSpec` refuses anything else BY NAME rather than dropping
/// it ("a typo that silently means 'no default' is a behaviour change nobody can see"), and the catalogue file's
/// override validator reuses the same list.
pub const REASONING_EFFORTS: [&str; 4] = ["low", "medium", "high", "max"];

/// `parseModelSpec(body)` — the console's model validator, ported with its own error strings because they ARE
/// the response bytes a client renders.
///
/// **EVERY REFUSAL HERE IS NAMED RATHER THAN GENERIC, AND THAT IS THE POINT OF THE FUNCTION**: this is the only
/// place an operator hears about a mistake (the source says so of the id, the prefix, the name and the effort
/// alike), so "invalid model" would be a port that kept the check and lost the instruction.
///
/// The rules, in the source's order: the id (trimmed, and it must LOOK like `prefix/name`), the prefix against
/// the route table, `ownedBy` (defaulting to the bare prefix), `wire` (only on `og/`, where the wire spelling is
/// not the model id), the three boolean switches (`=== true`, so a `"true"` STRING is not one), the display name
/// (80 UTF-16 units, no control characters), the two capacities (`Number.isInteger`, 1..=10_000_000 — zero and
/// negatives are the shapes an operator reaches for to mean "unset", which omitting it already means) and the
/// reasoning default (an unknown level is REFUSED rather than dropped: a typo that silently means "no default"
/// is a behaviour change nobody can see).
pub fn parse_model_spec(body: &Value, known_prefixes: &[&str]) -> Result<Value, String> {
    let raw_id = summrise_core::text::js_or_empty(body.get("id"));
    let id = summrise_core::text::js_trim(&raw_id).to_string();
    if id.is_empty() {
        return Err("id is required".into());
    }
    if !looks_like_a_model_id(&id) {
        return Err(format!(
            "id must look like \"prefix/name\" — got {}",
            serde_json::to_string(&Value::String(id.clone())).unwrap_or_default()
        ));
    }
    let slash = id.find('/').map(|i| i + 1).unwrap_or(0);
    let prefix = &id[..slash];
    if !known_prefixes.contains(&prefix) {
        return Err(format!(
            "unknown channel prefix {}; known: {}",
            serde_json::to_string(&Value::String(prefix.to_string())).unwrap_or_default(),
            known_prefixes.join(", ")
        ));
    }
    let owned_by = {
        let raw_owned = summrise_core::text::js_or_empty(body.get("ownedBy"));
        let raw = summrise_core::text::js_trim(&raw_owned);
        if raw.is_empty() {
            prefix.replace('/', "")
        } else {
            raw.to_string()
        }
    };
    let mut spec = serde_json::Map::new();
    spec.insert("id".into(), Value::String(id.clone()));
    spec.insert("owned_by_placeholder".into(), Value::Null);
    spec.remove("owned_by_placeholder");
    spec.insert("ownedBy".into(), Value::String(owned_by));

    let raw_wire = summrise_core::text::js_or_empty(body.get("wire"));
    let wire = summrise_core::text::js_trim(&raw_wire);
    if !wire.is_empty() && prefix == "og/" {
        spec.insert("wire".into(), Value::String(wire.to_string()));
    }
    for (from, to) in [
        ("usEgress", "usEgress"),
        ("search", "search"),
        ("responsesOnly", "responsesOnly"),
    ] {
        if body
            .get(from)
            .map(|v| v == &Value::Bool(true))
            .unwrap_or(false)
        {
            spec.insert(to.into(), Value::Bool(true));
        }
    }
    let raw_name = summrise_core::text::js_or_empty(body.get("name"));
    let name = summrise_core::text::js_trim(&raw_name);
    if !name.is_empty() {
        // The control-character class is the point of the check, not an accident: a newline or a NUL in a
        // display name breaks the listing it is printed in.
        if summrise_core::text::js_length(name) > 80
            || name.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}')
        {
            return Err("name must be at most 80 characters, with no control characters".into());
        }
        spec.insert("name".into(), Value::String(name.to_string()));
    }
    for field in ["contextWindow", "maxTokens"] {
        let raw = body.get(field).unwrap_or(&Value::Null);
        if raw.is_null() || raw.as_str() == Some("") {
            continue;
        }
        let n = summrise_core::text::js_number(raw);
        if !summrise_core::text::js_is_integer(n) || n <= 0.0 || n > 10_000_000.0 {
            return Err(format!(
                "{field} must be a whole number of tokens between 1 and 10000000"
            ));
        }
        // **AN INTEGER, NOT A FLOAT.** `json!(n)` on an `f64` renders `128000.0` where JavaScript renders
        // `128000` — and this value is echoed back to the console AND stored in KV, so the port would have
        // written a record whose bytes differ from the shipping one. `js_is_integer` has already refused a
        // fractional value, and the ceiling is below `i64`, so the cast is exact.
        spec.insert(
            field.to_string(),
            Value::Number(serde_json::Number::from(n as i64)),
        );
    }
    let raw_effort = summrise_core::text::js_or_empty(body.get("reasoningEffort"));
    let effort = summrise_core::text::js_trim(&raw_effort);
    if !effort.is_empty() {
        if !REASONING_EFFORTS.contains(&effort) {
            return Err(format!(
                "reasoningEffort must be one of low, medium, high, max — got {}",
                serde_json::to_string(&Value::String(effort.to_string())).unwrap_or_default()
            ));
        }
        spec.insert("reasoningEffort".into(), Value::String(effort.to_string()));
    }
    Ok(Value::Object(spec))
}

/// `/^[a-z0-9]+\/\S+$/` — **`\S` IS THE JAVASCRIPT SET**, so `U+00A0` and `U+FEFF` are whitespace here too (see
/// [`summrise_core::text::is_js_space`]): an id whose tail carries a no-break space is refused, which is what the
/// console has always done and what a hand-rolled `!c.is_whitespace()` would NOT do.
pub(crate) fn looks_like_a_model_id(id: &str) -> bool {
    let Some(slash) = id.find('/') else {
        return false;
    };
    let (head, tail) = (&id[..slash], &id[slash + 1..]);
    !head.is_empty()
        && head
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && !tail.is_empty()
        && !tail.chars().any(summrise_core::text::is_js_space)
}

/// `putFacetOverride(env, override)` — the read, the emptied-or-upsert rule ([`put_override`]) and the write.
pub async fn put_facet_override(env: &Env, o: &Value) -> Vec<Value> {
    let cur = read_list(env, MODEL_OVERRIDES_KEY).await;
    let next = put_override(&cur, o);
    write_list(env, MODEL_OVERRIDES_KEY, &next).await;
    next
}

/// `putCustomModel(env, spec)` — the same shape for the console's own records.
pub async fn put_custom_model(env: &Env, spec: &Value) -> Vec<Value> {
    let cur = read_list(env, CUSTOM_MODELS_KEY).await;
    let next = upsert_by_id(&cur, spec);
    write_list(env, CUSTOM_MODELS_KEY, &next).await;
    next
}

/// The ids of a list of records, in order — `next.map((m) => m.id)`.
pub fn ids_of(list: &[Value]) -> Vec<Value> {
    list.iter()
        .map(|m| m.get("id").cloned().unwrap_or(Value::Null))
        .collect()
}

/// `readList(env, key)` — **THE CONSOLE'S OWN READ, AND ITS TWO SILENT ARMS**: a cache hit wins (and a
/// non-array hit answers `[]` rather than the value), and a malformed or unreadable blob answers `[]` too —
/// "a catalogue that cannot be parsed must degrade to nothing extra, never to a 500 that takes the whole gateway
/// down". The cache is then refreshed with what was read, INCLUDING the empty answer.
pub async fn read_list(env: &Env, key: &str) -> Vec<Value> {
    let now = crate::device_store::now_ms();
    if let Some(hit) = crate::store::cached_get(key, now) {
        return hit.as_array().cloned().unwrap_or_default();
    }
    let out = match crate::device_store::kv_text(env, key).await {
        Some(raw) => serde_json::from_str::<Value>(&raw)
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default(),
        None => Vec::new(),
    };
    crate::store::cache_put(key, Value::Array(out.clone()), now);
    out
}

/// `writeList(env, key, value)` — the KV write, and the cache written through with the SAME value.
pub async fn write_list(env: &Env, key: &str, value: &[Value]) {
    let text = Value::Array(value.to_vec()).to_string();
    crate::device_store::kv_put(env, key, &text, None).await;
    crate::store::cache_put(
        key,
        Value::Array(value.to_vec()),
        crate::device_store::now_ms(),
    );
}

/// `setModelDisabled(env, id, disabled)` — the set, the sort, the write.
///
/// **THE SORT IS `[...set].sort()` AND IT IS OBSERVABLE** — the disabled list is a response field
/// (`disabledModels`) the console renders in order, and [`js_sorted`] is the engine's own comparison.
pub async fn set_model_disabled(env: &Env, id: &str, disabled: bool) -> Vec<String> {
    let mut ids: Vec<String> = read_list(env, DISABLED_MODELS_KEY)
        .await
        .iter()
        .map(summrise_core::text::js_string)
        .collect();
    if disabled {
        if !ids.iter().any(|x| x == id) {
            ids.push(id.to_string());
        }
    } else {
        ids.retain(|x| x != id);
    }
    let next = js_sorted(ids);
    write_list(
        env,
        DISABLED_MODELS_KEY,
        &next
            .iter()
            .map(|s| Value::String(s.clone()))
            .collect::<Vec<_>>(),
    )
    .await;
    next
}

/// `deleteCustomModel(env, id)` — **`removed` IS THE ANSWER**, and it is what the route turns into a 404: the
/// list is only written when something left, so a delete of an id this deployment does not own is not a write.
pub async fn delete_custom_model(env: &Env, id: &str) -> bool {
    let cur = read_list(env, CUSTOM_MODELS_KEY).await;
    let (next, removed) = remove_by_id(&cur, id);
    if !removed {
        return false;
    }
    write_list(env, CUSTOM_MODELS_KEY, &next).await;
    true
}

/// `isBuiltIn(id)` — membership in the model registry/// `isBuiltIn(id)` — membership in the model registry, with the TABLE passed in: the registry's data lives in
/// `registry.rs` (the worker) and in `channels.ts` (the console), and what this owns is the RULE — an id is
/// built-in exactly when the registry lists it, which is what tells the console whether to offer "disable" or
/// "delete".
pub fn is_built_in(registry_ids: &[&str], id: &str) -> bool {
    registry_ids.contains(&id)
}

/// `putCustomModel`'s upsert: the record with this id is dropped and the new one APPENDED — which is the
/// source's `[...cur.filter((m) => m.id !== spec.id), spec]`, and it means an update MOVES the record to the end
/// of the list rather than replacing it in place.
pub fn upsert_by_id(list: &[Value], spec: &Value) -> Vec<Value> {
    let id = spec.get("id").and_then(Value::as_str).unwrap_or_default();
    let mut out: Vec<Value> = list
        .iter()
        .filter(|m| m.get("id").and_then(Value::as_str) != Some(id))
        .cloned()
        .collect();
    out.push(spec.clone());
    out
}

/// `deleteCustomModel`: the list without that id, and whether anything left — the source's `next.length ===
/// cur.length` answer.
pub fn remove_by_id(list: &[Value], id: &str) -> (Vec<Value>, bool) {
    let out: Vec<Value> = list
        .iter()
        .filter(|m| m.get("id").and_then(Value::as_str) != Some(id))
        .cloned()
        .collect();
    let removed = out.len() != list.len();
    (out, removed)
}

/// `putFacetOverride`'s emptying test: four fields, all falsy, in the source's own order.
///
/// **AN OVERRIDE IS A DECORATION, SO CLEARING ALL OF IT MUST LEAVE THE MODEL EXACTLY AS THE REGISTRY DESCRIBES.**
/// The alternative — keeping an empty record — is a stale row in the store that looks like a decision somebody
/// made, which is what the source's comment refuses.
pub fn override_emptied(o: &Value) -> bool {
    ["name", "contextWindow", "maxTokens", "reasoningEffort"]
        .iter()
        .all(|k| !crate::device_registry::js_truthy(o.get(*k).unwrap_or(&Value::Null)))
}

/// `putFacetOverride`: the emptied rule decides between a REMOVAL and the upsert every other store uses.
pub fn put_override(list: &[Value], o: &Value) -> Vec<Value> {
    if override_emptied(o) {
        remove_by_id(
            list,
            o.get("id").and_then(Value::as_str).unwrap_or_default(),
        )
        .0
    } else {
        upsert_by_id(list, o)
    }
}

/// `declaredReasoningEffort`'s FIRST source: a console-added record for this id. **NOT THE WHOLE FUNCTION** — the
/// source short-circuits between three stores, and passing all three results in would make the host read KV
/// three times where the source reads once. So the ORDER is the host's call sites and the three TESTS are here.
pub fn declared_from_custom(custom: &[Value], id: &str) -> Option<String> {
    custom
        .iter()
        .find(|m| m.get("id").and_then(Value::as_str) == Some(id))
        .and_then(|m| m.get("reasoningEffort"))
        .and_then(truthy_text)
}

/// The SECOND source: a model inside a CUSTOM PROVIDER's record, found by the id's part after the first `/`.
///
/// `String(m?.id ?? "")` IS THE COMPARISON, and `?? ""` is why it is [`summrise_core::text::js_or_empty`] rather
/// than a truthiness helper: a model whose `id` is absent compares as the EMPTY string, so an id of `"acme/"`
/// (bare part `""`) matches it. A `0` id would compare as `"0"` and not match — both are the source's answers.
pub fn declared_from_provider(provider: Option<&Value>, id: &str) -> Option<String> {
    let slash = id.find('/')?;
    let bare = &id[slash + 1..];
    let models = provider?.get("models")?.as_array()?;
    models
        .iter()
        .find(|m| summrise_core::text::js_or_empty(m.get("id")) == bare)
        .and_then(|m| m.get("reasoningEffort"))
        .and_then(truthy_text)
}

/// The THIRD source: the facet override that DECORATES a built-in — last, because it may set a default the
/// registry never had and cannot contradict it about anything else.
pub fn declared_from_override(overrides: &[Value], id: &str) -> Option<String> {
    overrides
        .iter()
        .find(|o| o.get("id").and_then(Value::as_str) == Some(id))
        .and_then(|o| o.get("reasoningEffort"))
        .and_then(truthy_text)
}

/// A truthy string field, or `None` — the `if (x.reasoningEffort) return x.reasoningEffort` test.
fn truthy_text(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_string)
}

/// `catalogue(env)`'s ROUTE PROJECTION — the static cards with their per-channel lists FILTERED, plus one
/// synthetic card per prefix that only custom models or a custom provider uses.
///
/// THE ARGUMENTS ARE THE READS, and each one is a host duty: `route_info` is the console's display table
/// (`channels.ts`'s `ROUTE_INFO`, prose and all), `advertised` is the id list, `custom` is the console's own
/// model records, `provided` is the ADVERTISED CUSTOM-PROVIDER models (flattened to `{wire, provider}` by the
/// host, which is also where `advertised_provider_models` runs), and `provider_cards` is one entry per provider
/// whose `host` the HOST parsed (`new URL(baseURL).host`, with the raw string on a throw — a hand-edited record
/// must still show a card).
///
/// **AND THE BARE-vs-PREFIXED RULE IS THE WHOLE FUNCTION:** `"none"` keeps a name bare and every other prefix
/// joins, because `ROUTE_INFO` holds bare names while the advertised set holds full ids.
pub fn catalogue_routes(
    route_info: &Value,
    advertised: &[String],
    custom: &Value,
    provided: &Value,
    provider_cards: &Value,
) -> Vec<Value> {
    let empty = Vec::new();
    let custom_ids: Vec<&str> = custom
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|c| c.get("id").and_then(Value::as_str))
        .collect();
    let full_id = |prefix: &str, bare: &str| {
        if prefix == "none" {
            bare.to_string()
        } else {
            format!("{prefix}{bare}")
        }
    };
    let mut routes: Vec<Value> = Vec::new();
    for r in route_info.as_array().unwrap_or(&empty) {
        let prefix = r.get("prefix").and_then(Value::as_str).unwrap_or("");
        let models = r
            .get("models")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let filtered: Vec<Value> = models
            .into_iter()
            .filter(|bare| {
                let bare = bare.as_str().unwrap_or_default();
                let id = full_id(prefix, bare);
                // Gone from the advertised set for ANY reason — disabled, or replaced by a custom record of the
                // same id — means gone from this channel's list too.
                advertised.iter().any(|a| a == &id) && !custom_ids.iter().any(|c| *c == id)
            })
            .collect();
        let mut card = r.clone();
        if let Some(object) = card.as_object_mut() {
            object.insert("models".to_string(), Value::Array(filtered));
        }
        routes.push(card);
    }
    // A custom model on a prefix with no static route still needs somewhere to appear, or the gateway would
    // advertise an id under no channel at all. `cut > 0` is the source's test: a LEADING slash is not a prefix.
    for c in custom.as_array().unwrap_or(&empty) {
        let id = c.get("id").and_then(Value::as_str).unwrap_or_default();
        let cut = id.find('/').map(|i| i as i64).unwrap_or(-1);
        let (prefix, bare) = if cut > 0 {
            (
                id[..cut as usize + 1].to_string(),
                id[cut as usize + 1..].to_string(),
            )
        } else {
            ("none".to_string(), id.to_string())
        };
        match routes
            .iter_mut()
            .find(|r| r.get("prefix").and_then(Value::as_str) == Some(prefix.as_str()))
        {
            Some(existing) => push_bare(existing, &bare),
            None => routes.push(serde_json::json!({
                "prefix": prefix,
                "backend": if prefix == "none" { "Command Code (default)" } else { prefix.as_str() },
                "desc": "Added from the console.",
                "models": [bare],
            })),
        }
    }
    // CUSTOM PROVIDERS get a card too: the console's "model routing" section is built from this list, and a
    // provider whose models are advertised by /v1/models but appear under no channel is the exact drift
    // ROUTE_INFO's header records (an advertised id the console cannot explain).
    for p in provider_cards.as_array().unwrap_or(&empty) {
        let bare = p.get("prefix").and_then(Value::as_str).unwrap_or("");
        let prefix = format!("{bare}/");
        let models: Vec<Value> = provided
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .filter(|m| m.get("provider").and_then(Value::as_str) == Some(bare))
            .filter_map(|m| m.get("wire").cloned())
            .collect();
        let api = p.get("api").and_then(Value::as_str).unwrap_or("");
        let join = crate::routing::SUPPORTED_PROVIDER_APIS
            .iter()
            .find(|(name, _)| *name == api)
            .map(|(_, path)| *path)
            .unwrap_or("");
        let label = p
            .get("label")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or(bare);
        let base_url = p.get("baseURL").and_then(Value::as_str).unwrap_or("");
        let host = p.get("host").and_then(Value::as_str).unwrap_or("");
        let card = serde_json::json!({
            "prefix": prefix,
            "backend": label,
            "desc": format!(
                "Custom provider — {host} via {}; requests go to {base_url}{join}",
                if api.is_empty() { "?" } else { api }
            ),
            "models": models,
        });
        match routes
            .iter_mut()
            .find(|r| r.get("prefix").and_then(Value::as_str) == Some(prefix.as_str()))
        {
            Some(existing) => {
                for m in card
                    .get("models")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    push_bare(existing, m.as_str().unwrap_or_default());
                }
            }
            None => routes.push(card),
        }
    }
    routes
}

/// `if (!existing.models.includes(bare)) existing.models.push(bare)` — on a card this function built or cloned.
fn push_bare(card: &mut Value, bare: &str) {
    let Some(object) = card.as_object_mut() else {
        return;
    };
    let models = object
        .entry("models".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(list) = models.as_array_mut() else {
        return;
    };
    if !list.iter().any(|m| m.as_str() == Some(bare)) {
        list.push(Value::String(bare.to_string()));
    }
}

/// `ROUTE_INFO` — the console's channel table, **AS THE WORKER'S COPY OF THE SAME BYTES**.
///
/// `channels.ts` stays the source (the prefix, the backend NAME, the description and the bare model names
/// `routeModelsFor` derives from the registry); `scripts/sync-route-info-json.mjs` mirrors it here, and
/// `test/model-config-file.test.mjs` fails when the twin drifts. The prose is the operator's editing surface, and
/// moving it into Rust to emit it back would have been a bigger decision than this route is worth.
static ROUTE_INFO: std::sync::OnceLock<Vec<Value>> = std::sync::OnceLock::new();

/// The cards, parsed once per isolate. A malformed twin answers an EMPTY table rather than a 500 — the same
/// "degrade to nothing extra" rule the catalogue file follows.
pub fn route_info() -> Vec<Value> {
    ROUTE_INFO
        .get_or_init(|| {
            serde_json::from_str::<Value>(include_str!("../route-info.json"))
                .ok()
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default()
        })
        .clone()
}

/// The emitted module's header, and then the module./// The emitted module's header, and then the module.
const TS_MODULE: &str = r#"/**
 * models-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/model_store.rs` (`render_typescript`), which is now the single source for the
 * rules below. The freshness test in that module is a byte comparison against this file, so a hand edit fails
 * there with the text and one command to fix it:
 *
 *     SUMMRISE_REFRESH_MODELS=1 cargo test --manifest-path gateway/wasm/Cargo.toml model_store::emission
 *
 * `gateway/src/store/models.ts` re-exports this module and keeps the HOST: the three KV reads and writes, the
 * per-isolate cache, the file-layer merge and the writes' write-through. **THE RULES ARE HERE** — the three key
 * names, the advertised-id merge, the /v1/models extras, the disabled set's sort, the two upserts, the
 * facet-override rule, the reasoning-effort chain and the CATALOGUE PROJECTION.
 *
 * **AND THE PROJECTION IS THE ONE THAT HAS COST THIS REPOSITORY TWICE**: `ROUTE_INFO` holds BARE names while the
 * advertised set holds FULL ids, and comparing them directly matches NOTHING — the mismatch that made the
 * console's Models page set the wrong channel (round 58) and left its default card permanently empty (round 65).
 *
 * `barePrefix` and `SUPPORTED_PROVIDER_APIS` are IMPORTED from the provider emission rather than restated: one
 * rule, one owner, and the provider store already publishes both.
 */

import { SUPPORTED_PROVIDER_APIS, barePrefix } from "./providers-logic.ts";

/** `models:custom` — the console's own model records. */
export const CUSTOM_MODELS_KEY = "models:custom";

/** `models:overrides` — display-only decoration for a built-in. */
export const MODEL_OVERRIDES_KEY = "models:overrides";

/** `models:disabled` — built-in ids the console switched off. */
export const DISABLED_MODELS_KEY = "models:disabled";

/** `[...ids].sort()` — **JAVASCRIPT'S DEFAULT SORT COMPARES UTF-16 CODE UNITS**, and a model id is ASCII today.
 *  The comparison is written the engine's way anyway: a sort that agrees only on today's data is a sort that
 *  changes behaviour on tomorrow's. */
export function jsSorted(ids: string[]): string[] {
  return [...ids].sort((a, b) => {
    const au = a.split("").map((c) => c.charCodeAt(0));
    const bu = b.split("").map((c) => c.charCodeAt(0));
    for (let i = 0; i < Math.min(au.length, bu.length); i++) {
      if (au[i] !== bu[i]) return au[i]! - bu[i]!;
    }
    return au.length - bu.length;
  });
}

/** The advertised ids: built-ins minus the disabled ones, then the console-added, then the providers'. **THE
 *  ORDER IS THE LISTING'S**, which is why this is a function rather than three spreads at a call site. */
export function advertisedIds(
  registryIds: string[],
  disabled: string[],
  customIds: string[],
  providerIds: string[],
): string[] {
  return [...registryIds.filter((id) => !disabled.includes(id)), ...customIds, ...providerIds];
}

/** The NON-BUILT-IN `/v1/models` entries — console-added models and custom provider models — with the facets a
 *  provider model may declare. ONE merge point, so the list a client reads and the catalogue the console renders
 *  cannot drift apart. */
export function extraModelEntries(
  custom: any[],
  provided: any[],
): {
  id: string;
  owned_by: string;
  name?: string;
  context_window?: number;
  max_tokens?: number;
}[] {
  const out: {
    id: string;
    owned_by: string;
    name?: string;
    context_window?: number;
    max_tokens?: number;
  }[] = [];
  for (const m of custom) {
    const e: any = { id: m.id, owned_by: m.ownedBy };
    if (m.name) e.name = m.name;
    if (m.contextWindow) e.context_window = m.contextWindow;
    if (m.maxTokens) e.max_tokens = m.maxTokens;
    out.push(e);
  }
  for (const entry of provided) {
    const label = entry.provider?.label || barePrefix(entry.provider?.prefix);
    const e: any = { id: entry.id, owned_by: label };
    if (entry.model?.name) e.name = entry.model.name;
    if (entry.model?.contextWindow) e.context_window = entry.model.contextWindow;
    if (entry.model?.maxTokens) e.max_tokens = entry.model.maxTokens;
    out.push(e);
  }
  return out;
}

/** `isBuiltIn(id)` — membership in the registry, with the TABLE passed in, because the registry's data is the
 *  console's own (`channels.ts`) and the rule is this module's. */
export function isBuiltIn(registryIds: string[], id: string): boolean {
  return registryIds.includes(id);
}

/** `putCustomModel`'s upsert: the record with this id is dropped and the new one APPENDED — so an update MOVES
 *  it to the end of the list rather than replacing it in place. */
export function upsertById<T extends { id: string }>(list: T[], spec: T): T[] {
  return [...list.filter((m) => m.id !== spec.id), spec];
}

/** `deleteCustomModel`: the list without that id, and whether anything left. */
export function removeById<T extends { id: string }>(
  list: T[],
  id: string,
): { next: T[]; removed: boolean } {
  const next = list.filter((m) => m.id !== id);
  return { next, removed: next.length !== list.length };
}

/** **AN OVERRIDE IS A DECORATION, SO CLEARING ALL OF IT MUST LEAVE THE MODEL EXACTLY AS THE REGISTRY DESCRIBES.**
 *  The alternative — keeping an empty record — is a stale row that looks like a decision somebody made. */
export function overrideEmptied(o: {
  name?: unknown;
  contextWindow?: unknown;
  maxTokens?: unknown;
  reasoningEffort?: unknown;
}): boolean {
  return !o.name && !o.contextWindow && !o.maxTokens && !o.reasoningEffort;
}

/** `putFacetOverride`: the emptied rule decides between a REMOVAL and the same upsert every other store uses. */
export function putOverride<T extends { id: string }>(list: T[], o: T): T[] {
  return overrideEmptied(o as any) ? removeById(list, o.id).next : upsertById(list, o);
}

/** `declaredReasoningEffort`'s FIRST source: a console-added record for this id. The ORDER between the three
 *  sources is the caller's — the source short-circuits, and passing all three results in would read KV three
 *  times where it reads once. */
export function declaredFromCustom(custom: any[], id: string): string | null {
  return custom.find((m) => m?.id === id)?.reasoningEffort || null;
}

/** The SECOND source: a model inside a CUSTOM PROVIDER's record, found by the id's part after the first `/`.
 *  **`String(m?.id ?? "")` IS THE COMPARISON**: an absent id compares as the EMPTY string, so an id of `"acme/"`
 *  matches it, while a `0` id compares as `"0"` and does not. */
export function declaredFromProvider(provider: any, id: string): string | null {
  const slash = id.indexOf("/");
  if (slash < 0) return null;
  const bare = id.slice(slash + 1);
  const models = Array.isArray(provider?.models) ? provider.models : [];
  const model = models.find((m: any) => String(m?.id ?? "") === bare);
  return model?.reasoningEffort || null;
}

/** The THIRD source: the facet override that DECORATES a built-in — last, because it may set a default the
 *  registry never had and cannot contradict it about anything else. */
export function declaredFromOverride(overrides: any[], id: string): string | null {
  return overrides.find((o) => o?.id === id)?.reasoningEffort || null;
}

/** The catalogue's ROUTE PROJECTION — the static cards with their per-channel lists filtered, plus one synthetic
 *  card per prefix that only custom models or a custom provider uses.
 *
 *  `routeInfo` is the console's own display table, `provided` is the advertised provider models FLATTENED to
 *  `{wire, provider}` (the provider's bare prefix), and `providerCards` carries the `host` the CALLER parsed —
 *  `new URL(baseURL).host`, or the raw string when that throws, because a hand-edited record must still show a
 *  card rather than dropping out of the console. */
export function catalogueRoutes(
  routeInfo: any[],
  advertised: string[],
  custom: any[],
  provided: { wire: string; provider: string }[],
  providerCards: { prefix: string; label: string; baseURL: string; api: string; host: string }[],
): any[] {
  const fullId = (prefix: string, bare: string) => (prefix === "none" ? bare : prefix + bare);
  const routes: any[] = routeInfo.map((r) => ({
    ...r,
    models: (r.models as string[]).filter((bare) => {
      const id = fullId(r.prefix, bare);
      // Gone from the advertised set for ANY reason — disabled, or replaced by a custom record of the same id —
      // means gone from this channel's list too.
      return advertised.includes(id) && !custom.some((c) => c.id === id);
    }),
  }));
  for (const c of custom) {
    const cut = c.id.indexOf("/");
    const prefix = cut > 0 ? c.id.slice(0, cut + 1) : "none";
    const bare = cut > 0 ? c.id.slice(cut + 1) : c.id;
    const existing = routes.find((r) => r.prefix === prefix);
    if (existing) {
      if (!existing.models.includes(bare)) existing.models.push(bare);
    } else {
      routes.push({
        prefix,
        backend: prefix === "none" ? "Command Code (default)" : prefix,
        desc: "Added from the console.",
        models: [bare],
      });
    }
  }
  for (const p of providerCards) {
    const prefix = `${p.prefix}/`;
    const models = provided.filter((m) => m.provider === p.prefix).map((m) => m.wire);
    const join = Object.prototype.hasOwnProperty.call(SUPPORTED_PROVIDER_APIS, p.api)
      ? SUPPORTED_PROVIDER_APIS[p.api]
      : "";
    const card = {
      prefix,
      backend: p.label || p.prefix,
      desc: `Custom provider — ${p.host} via ${p.api || "?"}; requests go to ${p.baseURL}${join}`,
      models,
    };
    const existing = routes.find((r) => r.prefix === card.prefix);
    if (!existing) routes.push(card);
    else for (const m of models) if (!existing.models.includes(m)) existing.models.push(m);
  }
  return routes;
}
"#;

/// **THE PRODUCER.** The emitted module is exactly this string.
pub fn render_typescript() -> String {
    TS_MODULE.to_string()
}

/// The committed emission, and the path the refresh command writes.
pub fn typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/store/models-logic.ts")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// **THE UPSERTS AND THE EMPTIED RULE, EACH WITH THE ARM A CARELESS PORT COLLAPSES.**
    ///
    /// MUTATION: make `put_override` always upsert (drop the emptied arm).
    /// RESULT:   the emptied assertion fails — the store would keep an empty record that looks like a decision.
    #[test]
    fn the_record_rules_are_the_sources() {
        let a = json!({"id": "x", "name": "X"});
        let b = json!({"id": "y", "name": "Y"});
        // An update MOVES the record to the end.
        assert_eq!(
            upsert_by_id(&[a.clone(), b.clone()], &a),
            vec![b.clone(), a.clone()]
        );
        assert_eq!(
            upsert_by_id(std::slice::from_ref(&a), &json!({"id": "z"})).len(),
            2
        );
        let (next, removed) = remove_by_id(&[a.clone(), b.clone()], "x");
        assert!(removed);
        assert_eq!(next, vec![b.clone()]);
        assert!(!remove_by_id(std::slice::from_ref(&a), "nope").1);
        // A record with no id is kept by both (`undefined !== id`).
        assert_eq!(upsert_by_id(&[json!({})], &a).len(), 2);
        // The emptied rule: all four falsy.
        assert!(override_emptied(&json!({"id": "x"})));
        assert!(override_emptied(
            &json!({"id": "x", "name": "", "contextWindow": 0})
        ));
        assert!(!override_emptied(&json!({"id": "x", "name": "N"})));
        assert!(!override_emptied(&json!({"id": "x", "contextWindow": 5})));
        assert!(!override_emptied(&json!({"id": "x", "maxTokens": 5})));
        assert!(!override_emptied(
            &json!({"id": "x", "reasoningEffort": "low"})
        ));
        assert_eq!(
            put_override(std::slice::from_ref(&a), &json!({"id": "x"})),
            Vec::<Value>::new()
        );
        assert_eq!(
            put_override(std::slice::from_ref(&a), &json!({"id": "x", "name": "N"})).len(),
            1
        );
        assert!(is_built_in(&["a", "b"], "a"));
        assert!(!is_built_in(&["a", "b"], "c"));
        // The sort is the engine's: UTF-16 code units, not bytes.
        assert_eq!(
            js_sorted(vec!["b".into(), "A".into(), "a".into()]),
            vec!["A".to_string(), "a".to_string(), "b".to_string()]
        );
    }

    /// **THE THREE REASONING SOURCES, INCLUDING THE `?? ""` ARM NOBODY WOULD GUESS.** An id of `"acme/"`
    /// (nothing after the slash) matches a provider model whose `id` is ABSENT — because `String(m?.id ?? "")`
    /// is the empty string — and a model whose id is the NUMBER `0` does not, because it compares as `"0"`.
    #[test]
    fn the_reasoning_chain_is_the_sources() {
        let custom = vec![json!({"id": "a/b", "reasoningEffort": "max"})];
        assert_eq!(
            declared_from_custom(&custom, "a/b"),
            Some("max".to_string())
        );
        assert_eq!(declared_from_custom(&custom, "a/c"), None);
        assert_eq!(
            declared_from_custom(&[json!({"id": "a/b", "reasoningEffort": ""})], "a/b"),
            None
        );
        let provider = json!({"models": [{"id": "chat", "reasoningEffort": "high"}, {}]});
        assert_eq!(
            declared_from_provider(Some(&provider), "acme/chat"),
            Some("high".to_string())
        );
        assert_eq!(
            declared_from_provider(Some(&provider), "acme/"),
            None,
            "an empty record declares nothing"
        );
        assert_eq!(declared_from_provider(None, "acme/chat"), None);
        assert_eq!(
            declared_from_provider(Some(&provider), "chat"),
            None,
            "no slash, no provider"
        );
        // An absent id compares as "" — so a model with no id IS found for the bare id "".
        assert_eq!(
            declared_from_provider(
                Some(&json!({"models": [{"reasoningEffort": "low"}]})),
                "acme/"
            ),
            Some("low".to_string())
        );
        assert_eq!(
            declared_from_provider(
                Some(&json!({"models": [{"id": 0, "reasoningEffort": "low"}]})),
                "acme/"
            ),
            None,
            "a numeric id compares as \"0\", not as the empty string"
        );
        let overrides = vec![json!({"id": "og/x", "reasoningEffort": "medium"})];
        assert_eq!(
            declared_from_override(&overrides, "og/x"),
            Some("medium".to_string())
        );
        assert_eq!(declared_from_override(&overrides, "og/y"), None);
    }

    /// **THE PROJECTION, AND THE BARE-vs-PREFIXED TRAP IT EXISTS FOR.**
    ///
    /// MUTATION: compare `bare` against `advertised` directly instead of building the full id.
    /// RESULT:   every static card's model list empties — the round-58/65 defect, where `"og/mimo-v2.5"` never
    ///           matched the bare `"mimo-v2.5"` the table holds.
    #[test]
    fn the_catalogue_projection_joins_the_prefix() {
        let route_info = json!([
            {"prefix": "og/", "backend": "OpenCode Go", "desc": "d", "models": ["mimo", "gone"]},
            {"prefix": "none", "backend": "default", "desc": "d", "models": ["deepseek/x"]},
        ]);
        let advertised = vec![
            "og/mimo".to_string(),
            // An UNPREFIXED model is advertised BARE — that is what `"none"` means in `full_id`, and writing
            // `none/deepseek/x` here is what the first version of this test did.
            "deepseek/x".to_string(),
            "acme/chat".to_string(),
        ];
        let routes = catalogue_routes(&route_info, &advertised, &json!([]), &json!([]), &json!([]));
        assert_eq!(
            routes[0]["models"],
            json!(["mimo"]),
            "the full id is prefix + bare"
        );
        assert_eq!(
            routes[1]["models"],
            json!(["deepseek/x"]),
            "\"none\" keeps the name bare"
        );
        // A custom model on a route-less prefix gets its own card.
        let custom = json!([{"id": "my/thing"}]);
        let routes = catalogue_routes(&route_info, &advertised, &custom, &json!([]), &json!([]));
        let card = routes
            .iter()
            .find(|r| r["prefix"] == json!("my/"))
            .expect("a synthetic card");
        assert_eq!(card["backend"], json!("my/"));
        assert_eq!(card["desc"], json!("Added from the console."));
        assert_eq!(card["models"], json!(["thing"]));
        // ...and a custom model with NO slash lands on "none", WITHOUT one being invented.
        let routes = catalogue_routes(
            &json!([]),
            &advertised,
            &json!([{"id": "bare"}]),
            &json!([]),
            &json!([]),
        );
        assert_eq!(routes[0]["prefix"], json!("none"));
        assert_eq!(routes[0]["backend"], json!("Command Code (default)"));
        assert_eq!(routes[0]["models"], json!(["bare"]));
        // **A CUSTOM RECORD REPLACING A BUILT-IN'S ID STAYS ON THE CARD, AND THAT IS THE SOURCE'S NET
        // BEHAVIOUR RATHER THAN ITS COMMENT'S.** The filter drops it ("gone for any reason" — the sentence
        // above `fullId`), and then the CUSTOM loop below finds the same card by prefix and pushes the wire name
        // back on. So the filter's exclusion is what a DISABLED id gets, and a replacement returns through its
        // own arm. The first version of this test asserted `[]` — the comment's reading — and the port was
        // right: **the difference between the comment and the code is exactly what a port must not "fix".**
        let advertised2 = vec!["og/mimo".to_string()];
        let routes = catalogue_routes(
            &route_info,
            &advertised2,
            &json!([{"id": "og/mimo"}]),
            &json!([]),
            &json!([]),
        );
        assert_eq!(
            routes[0]["models"],
            json!(["mimo"]),
            "the replacement returns via the custom loop"
        );
    }

    /// **THE PROVIDER CARDS, INCLUDING THE `join` THAT COMES FROM THE DIALECT TABLE** — the `desc` is a
    /// response body a console renders, so its bytes are pinned here.
    #[test]
    fn the_catalogue_appends_provider_cards() {
        let provided = json!([
            {"wire": "chat", "provider": "acme"},
            {"wire": "other", "provider": "other"}
        ]);
        let cards = json!([{
            "prefix": "acme", "label": "Acme", "baseURL": "https://acme.test/v1",
            "api": "openai-completions", "host": "acme.test"
        }]);
        let routes = catalogue_routes(&json!([]), &[], &json!([]), &provided, &cards);
        assert_eq!(routes[0]["prefix"], json!("acme/"));
        assert_eq!(routes[0]["backend"], json!("Acme"));
        assert_eq!(routes[0]["models"], json!(["chat"]));
        assert_eq!(
            routes[0]["desc"],
            json!("Custom provider — acme.test via openai-completions; requests go to https://acme.test/v1/chat/completions")
        );
        // A record with no label falls back to its bare prefix, and an unknown dialect joins nothing.
        let cards = json!([{"prefix": "acme", "baseURL": "https://acme.test", "api": "weird", "host": "acme.test"}]);
        let routes = catalogue_routes(&json!([]), &[], &json!([]), &provided, &cards);
        assert_eq!(routes[0]["backend"], json!("acme"));
        assert_eq!(
            routes[0]["desc"],
            json!("Custom provider — acme.test via weird; requests go to https://acme.test")
        );
        // A provider whose prefix already has a static card MERGES into it rather than duplicating.
        let route_info =
            json!([{"prefix": "acme/", "backend": "static", "desc": "d", "models": []}]);
        let routes = catalogue_routes(&route_info, &[], &json!([]), &provided, &cards);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0]["backend"], json!("static"));
        assert_eq!(routes[0]["models"], json!(["chat"]));
    }
}

#[cfg(test)]
#[cfg(test)]
mod emission {
    use super::*;

    /// **THE FRESHNESS CHECK** — the committed file IS `render_typescript()`'s bytes.
    #[test]
    fn the_typescript_is_what_this_module_emits() {
        let path = typescript_path();
        let rendered = render_typescript();
        if std::env::var("SUMMRISE_REFRESH_MODELS").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write models-logic.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_MODELS=1 cargo test model_store::emission",
                path.display()
            )
        });
        assert_eq!(
            committed, rendered,
            "gateway/src/store/models-logic.ts is not what model_store.rs emits — \
             run SUMMRISE_REFRESH_MODELS=1 cargo test model_store::emission"
        );
    }

    /// **THE EMISSION CARRIES THE KEY NAMES AND EVERY RULE**, because a KV key spelled in one language and read
    /// in the other is the class this repository has paid for three times.
    ///
    /// MUTATION: drop the `fullId` join from the emitted `catalogueRoutes`.
    /// RESULT:   this test fails naming the rule — and the console's Models page would show empty channels,
    ///           which is the round-58/65 defect.
    #[test]
    fn the_emission_carries_the_rules() {
        let ts = render_typescript();
        for item in [
            format!("export const CUSTOM_MODELS_KEY = \"{CUSTOM_MODELS_KEY}\";"),
            format!("export const MODEL_OVERRIDES_KEY = \"{MODEL_OVERRIDES_KEY}\";"),
            format!("export const DISABLED_MODELS_KEY = \"{DISABLED_MODELS_KEY}\";"),
            "export function advertisedIds(".to_string(),
            "export function isBuiltIn(".to_string(),
            "export function extraModelEntries(".to_string(),
            "export function jsSorted(".to_string(),
            "export function upsertById<".to_string(),
            "export function removeById<".to_string(),
            "export function overrideEmptied(".to_string(),
            "export function putOverride<".to_string(),
            "export function declaredFromCustom(".to_string(),
            "export function declaredFromProvider(".to_string(),
            "export function declaredFromOverride(".to_string(),
            "export function catalogueRoutes(".to_string(),
            "const fullId = (prefix: string, bare: string) => (prefix === \"none\" ? bare : prefix + bare);".to_string(),
            "return advertised.includes(id) && !custom.some((c) => c.id === id);".to_string(),
            "String(m?.id ?? \"\") === bare".to_string(),
        ] {
            assert!(ts.contains(&item), "the emission lost: {item}");
        }
        // The two shared helpers are IMPORTED, not restated.
        assert!(ts.contains(
            "import { SUPPORTED_PROVIDER_APIS, barePrefix } from \"./providers-logic.ts\";"
        ));
    }
}
