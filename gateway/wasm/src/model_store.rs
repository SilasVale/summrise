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

/// **A ROW'S LIFECYCLE — THE FIELDS THAT LET AN ADVERTISED ROW BE DEMOTED RATHER THAN DELETED.**
///
/// A store row could only be `models:disabled`d, which is an operator's decision about a row they looked at. **A
/// ROW THAT ANSWERS 400 UPSTREAM IS NOT A DECISION, IT IS AN OBSERVATION**, and the catalogue lifecycle plan puts
/// the observation on the row: `status` is what a scheduled check concluded, `verifiedAt`/`verifiedBy` say when and
/// by what, `ephemeral` marks a free tier that can vanish without notice, and `assumed` records the facets adoption
/// could not derive (`usEgress`, a region policy) so the console can show what was guessed.
///
/// **THE WIRE NAMES ARE camelCase AND THAT IS FIXED BY THE PLAN** (`status`, `ephemeral`, `verifiedAt`,
/// `verifiedBy`, `assumed`) because the console reads them; the Rust field names are snake_case and the mapping is
/// in `lifecycle_of`.
///
/// **ABSENT MEANS ACTIVE, AND AN UNKNOWN STATUS DOES TOO.** A row written before this field existed is a row that is
/// still advertised, so `Active` is the only honest default; and a status string this build does not know (a newer
/// console wrote it) must not silently hide a row — **hiding is what `Deprecated` means, and it is the one value
/// that can make a model disappear**, so it is only ever set by a value that says so explicitly.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lifecycle {
    pub status: Status,
    /// A limited-time free tier: advertised, but the console says so.
    pub ephemeral: bool,
    pub verified_at: Option<i64>,
    pub verified_by: Option<String>,
    /// Facets adoption could not derive and a human has not yet declared.
    pub assumed: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Active,
    Alpha,
    Beta,
    Deprecated,
}

impl Status {
    /// The wire spelling — the same strings the console switches on.
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Active => "active",
            Status::Alpha => "alpha",
            Status::Beta => "beta",
            Status::Deprecated => "deprecated",
        }
    }

    fn from_wire(value: &str) -> Status {
        match value {
            "alpha" => Status::Alpha,
            "beta" => Status::Beta,
            "deprecated" => Status::Deprecated,
            _ => Status::Active,
        }
    }
}

/// **THE ROW'S LIFECYCLE, READ FROM THE ROW** — one place, so the filter, the admin state and the console's copy
/// cannot disagree about what a field means. A missing `status` is `Active` (see the type's note).
pub fn lifecycle_of(row: &Value) -> Lifecycle {
    let status = row["status"].as_str().unwrap_or("");
    Lifecycle {
        status: Status::from_wire(status),
        ephemeral: row["ephemeral"].as_bool().unwrap_or(false),
        verified_at: row["verifiedAt"].as_i64(),
        verified_by: row["verifiedBy"].as_str().map(str::to_string),
        assumed: row["assumed"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// **THE ROW WITH ITS LIFECYCLE WRITTEN ON — the pure half of `set_lifecycle`, and the half worth testing.**
///
/// Every field is written, including the ones the caller left unset: the write is the WHOLE lifecycle, so a row that
/// stops being `ephemeral` stops saying so, and a stale `verifiedAt` cannot survive a new observation. **The one
/// exception is `assumed`**, which is a LIST: an empty one is written as an empty list, because "nothing was
/// assumed" is a state the console shows.
///
/// The wire names are camelCase (see `Lifecycle`); a row that is not an object is returned untouched, because these
/// records come from a KV value a person may have written and losing one to a type error would be worse than
/// carrying it.
pub fn with_lifecycle(row: &Value, lifecycle: &Lifecycle) -> Value {
    let Some(object) = row.as_object() else {
        return row.clone();
    };
    let mut out = object.clone();
    out.insert(
        "status".into(),
        Value::String(lifecycle.status.as_str().to_string()),
    );
    out.insert("ephemeral".into(), Value::Bool(lifecycle.ephemeral));
    match lifecycle.verified_at {
        Some(at) => out.insert("verifiedAt".into(), Value::Number(at.into())),
        None => out.remove("verifiedAt"),
    };
    match lifecycle.verified_by.as_deref() {
        Some(by) => out.insert("verifiedBy".into(), Value::String(by.to_string())),
        None => out.remove("verifiedBy"),
    };
    out.insert(
        "assumed".into(),
        Value::Array(
            lifecycle
                .assumed
                .iter()
                .map(|a| Value::String(a.clone()))
                .collect(),
        ),
    );
    Value::Object(out)
}

/// **WRITE THE LIFECYCLE ONTO THE STORE ROW WITH THIS ID, AND RETURN THE LIST.** One row moves; every other row —
/// and its order — is untouched, because the listing's order is part of its bytes.
///
/// The KV half is deliberately thin: `read_list` + `with_lifecycle` + `write_list`. **Its proof belongs to the admin
/// corpus**, which is where every other KV write in this worker is recorded (writes and their TTLs), and which
/// `admin-oracle.mjs` can still record while the console's admin plugin exists.
pub async fn set_lifecycle(env: &Env, id: &str, lifecycle: &Lifecycle) -> Vec<Value> {
    let rows = read_list(env, CUSTOM_MODELS_KEY).await;
    let updated: Vec<Value> = rows
        .iter()
        .map(|row| {
            if row.get("id").and_then(|v| v.as_str()) == Some(id) {
                with_lifecycle(row, lifecycle)
            } else {
                row.clone()
            }
        })
        .collect();
    write_list(env, CUSTOM_MODELS_KEY, &updated).await;
    updated
}

/// **THE IDS A LISTING MAY ADVERTISE, FROM THE ROWS THEMSELVES** — and the ONLY thing this drops is a row whose
/// lifecycle says `deprecated`.
///
/// **WHY IT IS HERE RATHER THAN IN `advertised_ids`**: that function takes ids, and an id cannot carry a status.
/// The rows are read one layer up (`catalogue_file::custom_models`, the KV-plus-file merge), so the filter runs
/// before the ids are taken — **a filter downstream of the information it needs is a filter that cannot see it**,
/// which is what the plan's "only store rows are filtered by status" means in practice.
///
/// **CURATED ROWS NEVER COME THROUGH HERE**: the registry's ids go to `advertised_ids` directly and are filtered by
/// `models:disabled` alone, so a curated row keeps its place whatever its status says. Order is preserved because
/// the listing's order is part of its bytes.
pub fn advertised_row_ids(rows: &[Value]) -> Vec<String> {
    rows.iter()
        .filter(|row| !is_deprecated(row))
        .map(|row| summrise_core::text::js_string(row.get("id").unwrap_or(&Value::Null)))
        .collect()
}

/// **THE ONE PREDICATE THAT HIDES A ROW**, so that "deprecated" is spelled once and the store filter, the admin
/// state and any later reader ask the same question.
pub fn is_deprecated(row: &Value) -> bool {
    lifecycle_of(row).status == Status::Deprecated
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

// **THE EMISSION IS GONE (2026-10-17), WITH THE MODULE THAT CONSUMED IT.** `models-logic.ts` was the model
// store's TypeScript twin — the three KV key names, the advertised-id merge, the `/v1/models` extras, the sort,
// the two upserts, the facet-override rule, the reasoning chain and the catalogue projection — and its only
// consumer was `gateway/src/store/models.ts`, deleted with the cluster it belonged to. **THE RULES STAY**: they
// are this module's, the worker calls them in-process, and the admin corpus replays them.
