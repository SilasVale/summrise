//! THE GLOBAL SETTING'S DECISIONS, AND THE TYPESCRIPT THIS MODULE EMITS.
//!
//! ── WHY AN EMISSION AND NOT A ROUTE ───────────────────
//!
//! `gateway/src/store/settings.ts` carries three decisions: `normalizeSetting` (private),
//! `globalSettingEnabled` (exported) and the canonicalization inside `setGlobalSetting`. They are reached by
//! `decide`-style calls nowhere, because **`npm test` CANNOT REACH THE DECISION ROUTE AT ALL** — `helpers.mjs`'s
//! `WASM_GATE` stub answers a fixed model list for the `/v1` alias tests and knows nothing about `/__decision`.
//! Deleting a twin would therefore delete the only implementation the tests can reach.
//!
//! **SO THE RUST EMITS THE TYPESCRIPT, EXACTLY AS `mcp_tools.rs` ALREADY DOES** — `render_typescript()` writes
//! `gateway/src/mcp-tools.ts`, which the manifest classifies GENERATED, and the freshness test is a byte comparison.
//! There is no third copy: **the emission IS the Rust's output.**
//!
//! MUTATION: change `normalizeSetting`'s OFF set to `"0"` alone (drop `"false"`), then re-emit.
//! RESULT:   `store.cache.test.mjs` fails on the `"false"` case — the setting would read as ON.
//!
//! MUTATION: make `canonicalSetting` answer `"0"` for the boolean `true`.
//! RESULT:   the round-439 defect returns: a caller passing a real boolean INVERTS the switch.

use serde_json::Value;

/// `normalizeSetting(v)` — null = off, everything else passes through.
pub fn normalize_setting(v: Option<&str>) -> Option<String> {
    match v {
        Some(s) if s == "0" || s == "false" => None,
        other => other.map(str::to_string),
    }
}

/// `globalSettingEnabled(v)` — **AND THIS IS NOT `normalizeSetting(v) !== null`**: the EMPTY STRING is off here
/// (`normalizeSetting("")` answers `""`, which is not `"0"` or `"false"`).
pub fn global_setting_enabled(v: Option<&str>) -> bool {
    !matches!(v, None | Some("") | Some("0") | Some("false"))
}

/// `setGlobalSetting`'s value decision: the string to persist, or `None` for "delete the key".
///
/// **THE `None` ARM IS round-94's** — "no value set" (delete → the Worker var) and "explicitly off" (persist `"0"`,
/// which SHADOWS the var) are different answers, and collapsing them made a setting with an env fallback
/// impossible to turn OFF.
///
/// **AND THE BOOLEAN `true` IS round-439's** — the older `s === "1"` arm stored `String(true)` (`"true"`) as `"0"`,
/// *"silently INVERTING the switch for any caller passing a real boolean."*
pub fn canonical_setting(value: &Value) -> Option<String> {
    if value.is_null() || value.as_str().is_some_and(|s| s.is_empty()) {
        return None;
    }
    let s = crate::stream::js_text(value);
    Some(
        if s == "1" || s == "true" || value == &Value::Bool(true) {
            "1"
        } else {
            "0"
        }
        .to_string(),
    )
}

/// The emitted module's header.
const TS_HEADER: &str = "\
/**
 * settings-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/settings_logic.rs` (`render_typescript`). The freshness test in that module is a
 * byte comparison against this file, so a hand edit fails there with the text and one command to fix it:
 *
 *     SUMMRISE_REFRESH_SETTINGS=1 cargo test --manifest-path gateway/wasm/Cargo.toml settings_logic
 *
 * The decisions live in Rust; this is their emission, which is why the manifest classifies it GENERATED rather
 * than LOGIC — and why `npm test` can still run it.
 */

/** null = off, everything else passes through. */
export function normalizeSetting(v: string | null | undefined): string | null {
  if (v !== null && v !== undefined && (v === \"0\" || v === \"false\")) return null;
  return (v ?? null) as string | null;
}

/** Off is null, undefined, the empty string, \"0\" and \"false\". **NOT `normalizeSetting(v) !== null`.** */
export function globalSettingEnabled(v: string | null | undefined): boolean {
  return !(v === null || v === undefined || v === \"\" || v === \"0\" || v === \"false\");
}

/** The string to persist, or null for \"delete the key\" (an explicit OFF shadows the Worker var). */
export function canonicalSetting(value: unknown): string | null {
  if (value === null || value === undefined || value === \"\") return null;
  const s = String(value);
  return s === \"1\" || s === \"true\" || value === true ? \"1\" : \"0\";
}
";

/// **THE PRODUCER.** The emitted module is exactly this string.
pub fn render_typescript() -> String {
    TS_HEADER.to_string()
}

/// The committed emission, and the path the refresh command writes.
pub fn typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/store/settings-logic.ts")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// **THE FRESHNESS CHECK — AND IT IS THE REASON THE EMISSION CANNOT DRIFT.** The same shape `mcp_tools.rs` uses.
    #[test]
    fn the_typescript_is_what_this_module_emits() {
        let path = typescript_path();
        let rendered = render_typescript();
        if std::env::var("SUMMRISE_REFRESH_SETTINGS").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write settings-logic.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_SETTINGS=1 cargo test settings_logic",
                path.display()
            )
        });
        assert_eq!(
            committed, rendered,
            "gateway/src/store/settings-logic.ts is not what settings_logic.rs emits — \
             run SUMMRISE_REFRESH_SETTINGS=1 cargo test settings_logic"
        );
    }

    /// **THE TWO ARE NOT THE SAME FUNCTION, AND THE EMPTY STRING IS WHERE THEY PART.**
    ///
    /// MUTATION: implement `global_setting_enabled` as `normalize_setting(v).is_some()`.
    /// RESULT:   the `Some("")` assertion fails — the empty string would read as ON.
    #[test]
    fn the_empty_string_is_off_for_enabled_and_on_for_normalize() {
        assert_eq!(normalize_setting(Some("")), Some(String::new()));
        assert!(!global_setting_enabled(Some("")));
        assert_eq!(normalize_setting(Some("0")), None);
        assert!(!global_setting_enabled(Some("0")));
        assert_eq!(normalize_setting(Some("false")), None);
        assert!(!global_setting_enabled(Some("false")));
        assert!(!global_setting_enabled(None));
        assert_eq!(normalize_setting(Some("1")), Some("1".to_string()));
        assert!(global_setting_enabled(Some("1")));
        // A var that spells ON its own way stays what the var says.
        assert!(global_setting_enabled(Some("true")));
        assert_eq!(normalize_setting(Some("true")), Some("true".to_string()));
    }

    /// **THE EMPTY ARM IS "DELETE" AND THE BOOLEAN `true` IS ON** — round-94 and round-439.
    ///
    /// MUTATION: canonicalize with `s == "1"` alone.
    /// RESULT:   the boolean-`true` assertion fails: it would persist `"0"` and INVERT the switch.
    #[test]
    fn the_canonical_value_or_delete() {
        assert_eq!(canonical_setting(&Value::Null), None);
        assert_eq!(canonical_setting(&json!("")), None);
        assert_eq!(canonical_setting(&json!("1")), Some("1".to_string()));
        assert_eq!(canonical_setting(&json!("true")), Some("1".to_string()));
        assert_eq!(canonical_setting(&json!(true)), Some("1".to_string()));
        assert_eq!(canonical_setting(&json!(false)), Some("0".to_string()));
        assert_eq!(canonical_setting(&json!("0")), Some("0".to_string()));
        assert_eq!(canonical_setting(&json!("yes")), Some("0".to_string()));
    }
}
