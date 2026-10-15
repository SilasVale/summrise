//! THE CATALOGUE FILE LAYER — `gateway/src/store/file-config.ts`, ported.
//!
//! **WHAT IT IS.** `gateway/config/models.ts` is a committed document that declares custom providers, models on
//! built-in channels, and display overrides for built-ins. **THE FILE WINS PER ID/PREFIX OVER THE CONSOLE (KV)** —
//! that is what "configured from the file" means — and the console marks file-declared entries because an edit
//! made there would be reverted by the next deploy.
//!
//! **WHY THE WORKER READS A JSON TWIN.** The TypeScript imports the module at build time. Rust cannot read a `.ts`
//! file with `include_str!` (unquoted keys, a `satisfies` clause) — so `gateway/config/models.json` is generated
//! from it by `gateway/scripts/sync-catalogue-json.mjs`, and `test/catalogue-twins.test.mjs` FAILS when the twin
//! drifts. The TypeScript stays the operator's edit surface and the compiler-checked half; the twin is the
//! worker's copy of the same bytes.
//!
//! **AND THIS MODULE IS WHY THE THREE READ ROUTES COULD NOT BE PORTED BEFORE IT.** `store/models.ts`,
//! `store/providers.ts` and the admin reads all merge this layer; a route that read KV alone would have served a
//! catalogue missing every file-declared entry — with no oracle to notice, because the shipped document is empty
//! and an empty merge is the identity function.

use serde_json::{Map, Value};
use worker::Env;

/// The document, parsed ONCE per isolate. A malformed twin is a deploy-time error in the TypeScript and a
/// `None` here — the reads it feeds then behave exactly as they did before the layer existed, which is the
/// source's own "a catalogue that cannot be parsed must degrade to nothing extra, never to a 500".
static PARSED: std::sync::OnceLock<Option<CatalogueLayers>> = std::sync::OnceLock::new();

/// The three layers the document may declare, in the source's own `CatalogueLayers` shape.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CatalogueLayers {
    pub providers: Vec<Value>,
    pub models: Vec<Value>,
    pub overrides: Vec<Value>,
}

/// `loadCatalogueFile(deps)` — the committed document, validated ONCE (the source caches it the same way, and
/// for the same reason: "a malformed file fails the deploy" rather than every request).
pub fn load_catalogue_file() -> CatalogueLayers {
    PARSED
        .get_or_init(|| parse_catalogue_file(&committed_document()?, &known_prefixes()).ok())
        .clone()
        .unwrap_or_default()
}

/// The twin's bytes, compiled in. `include_str!` is the point: the worker carries the document, and a deployment
/// cannot be missing a file at runtime.
fn committed_document() -> Option<Value> {
    serde_json::from_str(include_str!("../../config/models.json")).ok()
}

/// `KNOWN_PREFIXES` — the console's route table, imported from the model store so the two spellings of "which
/// channels exist" are one.
fn known_prefixes() -> Vec<&'static str> {
    crate::model_store::known_prefixes()
}

/// `parseCatalogueFile(rawDoc, deps)` — **THE RISKY HALF, AND ITS REFUSALS ARE THE SOURCE'S OWN WORDS.**
///
/// A document that expresses something the API would refuse is a document that silently does nothing, so the
/// FILE goes through the same validators the console's POST does ("one validator, two writers"): the provider
/// entries through [`summrise_core::provider_spec::parse_provider_spec`], and the inline `apiKey` refused
/// outright — the file's threat model is a git history that lives forever.
pub fn parse_catalogue_file(
    raw: &Value,
    known_prefixes: &[&str],
) -> Result<CatalogueLayers, String> {
    let Some(doc) = raw.as_object() else {
        return Err(
            "config/models.ts must export an object with providers/models/overrides".into(),
        );
    };
    let mut out = CatalogueLayers::default();
    for (i, entry) in array_of(doc, "providers").iter().enumerate() {
        // The URL parse is the HOST's half of the parser's seam, exactly as in the admin route.
        let base_text = summrise_core::provider_spec::provider_base_text(entry);
        let base = crate::store::parsed_base(&base_text);
        let spec = summrise_core::provider_spec::parse_provider_spec(entry, base.as_ref(), None)
            .map_err(|e| format!("config/models.ts providers[{i}]: {}", e.error))?;
        if crate::device_registry::js_truthy(spec.get("apiKey").unwrap_or(&Value::Null)) {
            return Err(format!(
                "config/models.ts providers[{i}]: an inline apiKey cannot be committed — use apiKeyEnv and set \
                 the secret with wrangler"
            ));
        }
        out.providers.push(spec);
    }
    for (i, entry) in array_of(doc, "models").iter().enumerate() {
        let id = summrise_core::text::js_trim(&summrise_core::text::js_or_empty(entry.get("id")))
            .to_string();
        if id.is_empty() {
            return Err(format!("config/models.ts models[{i}]: id is required"));
        }
        // A built-in belongs in `overrides`, not here: `models[]` creates a record this deployment owns, and
        // creating one for a built-in is the shadowing the console refuses with a 200-but-override — the file
        // must not have a third answer.
        let slash = id.find('/').map(|i| i + 1).unwrap_or(0);
        if !known_prefixes.contains(&&id[..slash])
            && !crate::model_store::looks_like_a_model_id(&id)
        {
            return Err(format!(
                "config/models.ts models[{i}].id must look like \"prefix/name\" — got {}",
                serde_json::to_string(&Value::String(id)).unwrap_or_default()
            ));
        }
        out.models.push(entry.clone());
    }
    for (i, entry) in array_of(doc, "overrides").iter().enumerate() {
        let id = summrise_core::text::js_trim(&summrise_core::text::js_or_empty(entry.get("id")))
            .to_string();
        if id.is_empty() {
            return Err(format!("config/models.ts overrides[{i}]: id is required"));
        }
        let pinned: Vec<&str> = crate::model_store::PINNED_FACETS
            .iter()
            .copied()
            .filter(|f| entry.get(*f).is_some())
            .collect();
        if !pinned.is_empty() {
            return Err(format!(
                "config/models.ts overrides[{i}]: {} is a ROUTING facet pinned in the channel registry — a file \
                 may set name, contextWindow, maxTokens and reasoningEffort only",
                pinned.join(", ")
            ));
        }
        let effort = summrise_core::text::js_or_empty(entry.get("reasoningEffort"));
        if !effort.is_empty() && !crate::model_store::REASONING_EFFORTS.contains(&effort.as_str()) {
            return Err(format!(
                "config/models.ts overrides[{i}].reasoningEffort must be one of {}",
                crate::model_store::REASONING_EFFORTS.join(", ")
            ));
        }
        let mut o = Map::new();
        o.insert("id".into(), Value::String(id));
        for field in ["name", "contextWindow", "maxTokens", "reasoningEffort"] {
            let raw = entry.get(field).unwrap_or(&Value::Null);
            if !crate::device_registry::js_truthy(raw) {
                continue;
            }
            let value = if field == "name" || field == "reasoningEffort" {
                Value::String(crate::stream::js_text(raw))
            } else {
                Value::Number(serde_json::Number::from(
                    summrise_core::text::js_number(raw) as i64,
                ))
            };
            o.insert(field.to_string(), value);
        }
        out.overrides.push(Value::Object(o));
    }
    Ok(out)
}

fn array_of<'a>(doc: &'a Map<String, Value>, key: &str) -> &'a Vec<Value> {
    static EMPTY: Vec<Value> = Vec::new();
    doc.get(key).and_then(Value::as_array).unwrap_or(&EMPTY)
}

/// `mergeLayers(file, kv, keyOf)` — **THE PRECEDENCE RULE, AND IT IS THE PART THAT CAN SILENTLY DISCARD AN
/// OPERATOR'S WORK**: the file's entries win for their own ids, and every KV entry the file does not declare
/// survives, in KV's order.
pub fn merge_layers(file: &[Value], kv: &[Value], key_of: impl Fn(&Value) -> String) -> Vec<Value> {
    let declared: Vec<String> = file.iter().map(&key_of).collect();
    let mut out: Vec<Value> = kv
        .iter()
        .filter(|k| !declared.contains(&key_of(k)))
        .cloned()
        .collect();
    out.extend(file.iter().cloned());
    out
}

/// `fileDeclaredKeys(layers)` — the ids/prefixes the FILE owns, which is what the console marks as not-locally-
/// editable. **A PROVIDER'S KEY IS ITS BARE PREFIX** (`"acme/"` → `"acme"`), which is how the merge compares them
/// too.
pub fn file_declared_keys(layers: &CatalogueLayers) -> (Vec<String>, Vec<String>, Vec<String>) {
    let providers = layers
        .providers
        .iter()
        .map(|p| summrise_core::provider_spec::bare_prefix(p.get("prefix").unwrap_or(&Value::Null)))
        .collect();
    let ids = |list: &[Value]| -> Vec<String> {
        list.iter()
            .map(|m| summrise_core::text::js_string(m.get("id").unwrap_or(&Value::Null)))
            .collect()
    };
    (providers, ids(&layers.models), ids(&layers.overrides))
}

/* ─────────────────── the merged reads every consumer goes through ─────────────────── */

/// `customProviders(env)` — the KV list with the FILE's providers merged OVER it, keyed by bare prefix.
pub async fn custom_providers(env: &Env) -> Vec<Value> {
    let file = load_catalogue_file();
    let kv = crate::store::custom_provider_list(env).await;
    merge_layers(&file.providers, &kv, |p| {
        summrise_core::provider_spec::bare_prefix(p.get("prefix").unwrap_or(&Value::Null))
    })
}

/// `customModels(env)` — the same shape, keyed by id.
pub async fn custom_models(env: &Env) -> Vec<Value> {
    let file = load_catalogue_file();
    let kv = crate::model_store::read_list(env, crate::model_store::CUSTOM_MODELS_KEY).await;
    merge_layers(&file.models, &kv, |m| {
        summrise_core::text::js_string(m.get("id").unwrap_or(&Value::Null))
    })
}

/// `facetOverrides(env)` — and the overrides have their own reader because the console's write path reads them
/// before an upsert.
pub async fn facet_overrides(env: &Env) -> Vec<Value> {
    let file = load_catalogue_file();
    let kv = crate::model_store::read_list(env, crate::model_store::MODEL_OVERRIDES_KEY).await;
    merge_layers(&file.overrides, &kv, |o| {
        summrise_core::text::js_string(o.get("id").unwrap_or(&Value::Null))
    })
}

/// **THE FILE LAYER'S TWO PURE RULES, EMITTED — `file-config-logic.ts` (2026-10-14).**
///
/// `store/file-config.ts` is the catalogue FILE layer: the validator, the merge that gives the FILE precedence,
/// and the key sets the console marks as not-locally-editable. `catalogue_file.rs` is its Rust twin, and the
/// admin family's 82-case corpus already proves the layer end to end (`GET /api/admin/catalogue`,
/// `/api/admin/models`, `/api/admin/public`). What this renders is the half that is PURE and shared word for
/// word; the validator's 76 lines and the module-load cache stay the host's for now, because their SHAPE differs
/// by necessity (the console reads `config/models.ts`, the worker reads its JSON twin).
///
/// **WHY THESE TWO FIRST**: they are the rules whose two spellings could disagree without any test noticing — the
/// merge's precedence and the provider-key normalisation (`"acme/"` → `"acme"`) — and each is small enough to
/// compare by reading.
const TS_MODULE: &str = r#"/**
 * file-config-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/catalogue_file.rs` (`render_typescript`). The freshness test there is a byte
 * comparison:
 *
 *     SUMMRISE_REFRESH_FILE_CONFIG=1 cargo test --manifest-path gateway/wasm/Cargo.toml catalogue_file::emission
 *
 * `gateway/src/store/file-config.ts` re-exports this module and keeps the host: the document validator's four
 * refusal arms, the `config/models.ts` import, and the module-load cache.
 */

/** The layers a validated document declares.
 *
 *  **GENERIC IN THE ENTRY TYPES, AND THAT IS THE HOST'S SHAPE, NOT A CONVENIENCE**: the console's layers are
 *  `ProviderSpec[]` / `ModelSpec[]` / `FacetOverride[]` — three types this generated leaf may not import (it
 *  would be an edge back into its own host). The defaults make a bare `CatalogueLayers` accept those concrete
 *  types, and the two functions below only ever read `prefix` and `id`. */
export interface CatalogueLayers<P = any, M = any, O = any> {
  providers: P[];
  models: M[];
  overrides: O[];
}

/**
 * **THE FILE WINS, AND THE ORDER OF THE RESULT IS PART OF THE ANSWER**: the KV entries that survive come FIRST
 * and the file's follow, so a list rendered from this shows the console's own entries and then the committed
 * ones. The comparison is by [`FileDeclaredKeys`]-style key — a bare prefix for providers, an id for models and
 * overrides — because `"acme/"` and `"acme"` are the same provider.
 */
export function mergeLayers<T>(file: T[], kv: T[], keyOf: (t: T) => string): T[] {
  const declared = new Set(file.map(keyOf));
  return [...kv.filter((k) => !declared.has(keyOf(k))), ...file];
}

/** Ids/prefixes THIS FILE declares — what the console must mark as not-locally-editable.
 *
 *  **A PROVIDER'S KEY IS ITS BARE PREFIX**: `"acme/"` becomes `"acme"`, which is how the merge compares them. */
export function fileDeclaredKeys(layers: CatalogueLayers): {
  providers: Set<string>;
  models: Set<string>;
  overrides: Set<string>;
} {
  return {
    providers: new Set(layers.providers.map((p) => String(p.prefix ?? "").replace(/\/$/, ""))),
    models: new Set(layers.models.map((m) => m.id)),
    overrides: new Set(layers.overrides.map((o) => o.id)),
  };
}
"#;

/// **THE PRODUCER.** The emitted module is exactly this string.
pub fn render_typescript() -> String {
    TS_MODULE.to_string()
}

/// The committed emission, and the path the refresh command writes.
pub fn typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/store/file-config-logic.ts")
}

#[cfg(test)]
mod emission {
    use super::*;

    /// **THE FRESHNESS CHECK** — the committed file IS `render_typescript()`'s bytes.
    #[test]
    fn the_typescript_is_what_this_module_emits() {
        let path = typescript_path();
        let rendered = render_typescript();
        if std::env::var("SUMMRISE_REFRESH_FILE_CONFIG").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write file-config-logic.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_FILE_CONFIG=1 cargo test catalogue_file::emission",
                path.display()
            )
        });
        assert_eq!(
            committed, rendered,
            "gateway/src/store/file-config-logic.ts is not what catalogue_file.rs emits — \
             run SUMMRISE_REFRESH_FILE_CONFIG=1 cargo test catalogue_file::emission"
        );
    }

    /// **THE TWO RULES THE EMISSION EXISTS FOR ARE IN IT, SPELLED THE WAY THE HOST NEEDS THEM.** The merge's
    /// precedence (the dichotomous `filter` before the spread) and the bare-prefix normalisation are the two
    /// lines a "cleanup" would rewrite into something that reads the same and behaves differently.
    #[test]
    fn the_emission_carries_the_rules() {
        let ts = render_typescript();
        for item in [
            "export function mergeLayers<T>(file: T[], kv: T[], keyOf: (t: T) => string): T[] {",
            "const declared = new Set(file.map(keyOf));",
            "return [...kv.filter((k) => !declared.has(keyOf(k))), ...file];",
            "export function fileDeclaredKeys(layers: CatalogueLayers): {",
            "providers: new Set(layers.providers.map((p) => String(p.prefix ?? \"\").replace(/\\/$/, \"\"))),",
            "models: new Set(layers.models.map((m) => m.id)),",
            "overrides: new Set(layers.overrides.map((o) => o.id)),",
        ] {
            assert!(ts.contains(item), "the emission lost: {item}");
        }
    }

    /// **AND THE RUST'S OWN MERGE AGREES WITH IT, CASE FOR CASE** — the two implementations are one rule in two
    /// languages, so the check is a table rather than a reading: a file entry shadows the KV entry with the same
    /// key, the survivors keep their order, and the file's entries follow.
    #[test]
    fn the_rust_merge_is_the_emitted_rule() {
        let kv = vec![
            serde_json::json!({"prefix": "acme/"}),
            serde_json::json!({"prefix": "kept/"}),
        ];
        let file = vec![serde_json::json!({"prefix": "acme/"})];
        let merged = merge_layers(&file, &kv, |v| {
            summrise_core::provider_spec::bare_prefix(v.get("prefix").unwrap_or(&Value::Null))
        });
        let order: Vec<String> = merged
            .iter()
            .map(|v| {
                summrise_core::provider_spec::bare_prefix(v.get("prefix").unwrap_or(&Value::Null))
            })
            .collect();
        assert_eq!(
            order,
            vec!["kept".to_string(), "acme".to_string()],
            "the survivor comes first and the file's entry follows — `\"acme/\"` and `\"acme\"` are one provider"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// **THE PRECEDENCE, WITH BOTH SIDES SUPPLIED** — the property that can lose an operator's work.
    ///
    /// MUTATION: swap the two halves of `merge_layers` (KV wins).
    /// RESULT:   this test fails — and a console edit to a file-declared id would silently revert on the next
    ///           deploy, which is the failure the console's "declared in config/models.ts" marking exists for.
    #[test]
    fn the_file_wins_and_the_rest_of_kv_survives() {
        let file = vec![json!({"id": "og/from-file", "name": "File"})];
        let kv = vec![
            json!({"id": "og/from-file", "name": "Console"}),
            json!({"id": "og/only-console", "name": "Console"}),
        ];
        let merged = merge_layers(&file, &kv, |m| {
            summrise_core::text::js_string(m.get("id").unwrap_or(&Value::Null))
        });
        let seen: Vec<String> = merged
            .iter()
            .map(|m| {
                format!(
                    "{}={}",
                    summrise_core::text::js_string(m.get("id").unwrap_or(&Value::Null)),
                    summrise_core::text::js_string(m.get("name").unwrap_or(&Value::Null))
                )
            })
            .collect();
        assert_eq!(seen, vec!["og/only-console=Console", "og/from-file=File"]);
        // ...and an empty file changes NOTHING, which is what makes the layer safe on a deployment that
        // declares nothing.
        assert_eq!(
            merge_layers(&[], &kv, |m| summrise_core::text::js_string(
                m.get("id").unwrap_or(&Value::Null)
            )),
            kv
        );
    }

    /// **THE SHIPPED DOCUMENT PARSES, AND IT IS THE SAME THREE ARRAYS THE TYPESCRIPT DECLARES.** The twin's
    /// FRESHNESS is `gateway/test/catalogue-twins.test.mjs`'s job (it can run the generator); this asserts the
    /// worker's half — that the compiled-in bytes are a valid, empty document.
    #[test]
    fn the_committed_document_is_valid_and_empty() {
        let raw = committed_document().expect("config/models.json must be valid JSON");
        let layers =
            parse_catalogue_file(&raw, &known_prefixes()).expect("the shipped document must parse");
        assert_eq!(
            layers,
            CatalogueLayers::default(),
            "the shipped document declares nothing"
        );
        let (p, m, o) = file_declared_keys(&layers);
        assert!(p.is_empty() && m.is_empty() && o.is_empty());
    }

    /// **THE VALIDATOR'S REFUSALS, EACH WITH THE SOURCE'S WORDS** — a document that expresses something the API
    /// would refuse must not be accepted here, or the file becomes a second, quieter writer.
    #[test]
    fn the_document_validator_refuses_what_the_api_refuses() {
        let good_provider = json!({
            "prefix": "acme/", "baseURL": "https://acme.test/v1", "api": "openai-completions",
            "apiKeyEnv": "ACME_API_KEY", "models": [{"id": "chat"}]
        });
        let err = |doc: Value| {
            parse_catalogue_file(&doc, &known_prefixes()).expect_err("must be refused")
        };
        // Not an object.
        assert!(err(json!([])).contains("must export an object"));
        // An INLINE credential: the file's threat model is a git history that lives forever. **THE PROVIDER
        // MUST NOT ALSO NAME A BINDING**, because the parser refuses "both sources" first — which is what this
        // case did on its first run, and the assertion below then failed for the wrong refusal.
        let mut with_key = good_provider.clone();
        with_key.as_object_mut().unwrap().remove("apiKeyEnv");
        with_key["apiKey"] = json!("sk-inline");
        assert!(
            err(json!({"providers": [with_key]})).contains("an inline apiKey cannot be committed")
        );
        // A built-in prefix is refused by the PROVIDER parser (its own 409), not by this module.
        let mut builtin = good_provider.clone();
        builtin["prefix"] = json!("og/");
        assert!(err(json!({"providers": [builtin]})).contains("reserved by a built-in channel"));
        // A model needs an id, and its id must look like one.
        assert!(err(json!({"models": [{}]})).contains("id is required"));
        assert!(err(json!({"models": [{"id": "a b"}]})).contains("must look like"));
        // An override may not carry a routing facet, and its effort must be one the router speaks.
        assert!(err(json!({"overrides": [{"id": "og/x", "wire": "w"}]}))
            .contains("pinned in the channel"));
        // **AND `null` IS "PRESENT" TOO** — the source's test is `o[f] !== undefined`, not truthiness, so a
        // document that spells a routing facet as null is refused rather than silently cleaned. (This test's
        // own "valid document" used to carry `"wire": null` and was refused, which is how the arm was checked.)
        assert!(err(json!({"overrides": [{"id": "og/x", "wire": null}]}))
            .contains("pinned in the channel"));
        assert!(
            err(json!({"overrides": [{"id": "og/x", "reasoningEffort": "extreme"}]}))
                .contains("must be one of low, medium, high, max")
        );
        // ...and a good document comes back in the source's shape: the override keeps only its display facets.
        let ok = parse_catalogue_file(
            &json!({"providers": [good_provider], "models": [{"id": "qw/thing"}],
                    "overrides": [{"id": "og/x", "name": "X", "contextWindow": 100}]}),
            &known_prefixes(),
        )
        .expect("a valid document");
        assert_eq!(ok.providers.len(), 1);
        assert_eq!(ok.models.len(), 1);
        assert_eq!(
            ok.overrides,
            vec![json!({"id": "og/x", "name": "X", "contextWindow": 100})]
        );
        assert_eq!(
            file_declared_keys(&ok),
            (
                vec!["acme".to_string()],
                vec!["qw/thing".to_string()],
                vec!["og/x".to_string()]
            )
        );
    }
}
