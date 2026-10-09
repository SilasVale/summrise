//! THE DEVICE REGISTRY'S DECISIONS — MOVED FROM `gateway/src/plugins/devices.ts`,
//! `gateway/src/store/devices.ts`, `store/regkeys.ts` AND `store/grants.ts`.
//!
//! WHY THESE ARE THE FIRST HALF OF THIS PORT AND THE ONLY HALF THAT CAN BE PROVED WITHOUT A BINDING. Every
//! function here decides something a caller can act on — whether a name is a device name, whether a hostname may
//! be dialled, what a console row shows, whether a code is even worth a KV read, what a rename refuses — and not
//! one of them touches KV, `fetch`, or the clock. The I/O is in `device_store.rs` and the handlers are in
//! `devices.rs`; this file answers questions over values.
//!
//! **THE FOUR REGEXES ARE HAND-ROLLED, AND THAT IS A CHOICE THE CRATE ALREADY MADE.** The source spells four
//! shapes — `^[A-Za-z0-9_-]{1,32}$` (a device name), `^([a-z0-9-]+\.)+[a-z0-9-]+$/i` (a hostname),
//! `^[0-9a-f]{64}$/i` (a device token) and `^[0-9a-f]{32}$/` (a panel grant) — and each is a whole-string match
//! over a fixed character class. A regex engine would be a new dependency for four one-line predicates; the
//! transliterations below are exact for those classes, and their edge cases are pinned by tests.
//!
//! **WHERE THE SOURCE'S OWN QUIRKS ARE REPRODUCED, THEY SAY SO.** Two matter and both are load-bearing:
//!
//!   * `String(body?.name || "").trim()` is a TRUTHINESS test, not a presence test — `0`, `false`, `null` and
//!     `""` all become `""` before the trim (`js_falsy_string`), while `5` becomes `"5"` and `{}` becomes
//!     `"[object Object]"` (both then fail the name rule with the same message the source gives).
//!   * a device's JSON is built in the source's KEY ORDER, because the body is compared as bytes: the console
//!     row (`name, hostname, token, mcp, registeredAt?, lastSeenAt?, lastVersion?`), the MCP snippet
//!     (`JSON.stringify(snippet, null, 2)`), and the record written back to KV.
//!
//! **AND ONE THING IS BUILT RATHER THAN SPELLED**: the hostname rule's message names the deployment's default
//! device host, and `agent/tests/production_host.rs` is a ratchet over which files may name one. That constant
//! is already declared in `gateway/wasm/src/device.rs` for the same reason, so the message is `d1` + that
//! constant — byte for byte the string `devices.ts` carries, with the hostname written down in ONE file.

use serde_json::{Map, Value};

/// `validateDevice`'s name rule, verbatim.
pub const NAME_RULE: &str = "Device name must be 1-32 chars: letters/digits/_ -";
/// `validateDevice`'s token rule, verbatim.
pub const TOKEN_RULE: &str = "Token must be at least 8 chars";

/// `validateDevice`'s hostname rule, verbatim — see the module header for why it is assembled.
pub fn hostname_rule() -> String {
    format!(
        "hostname must be a domain like d1{}",
        crate::device::DEFAULT_DEVICE_HOST_SUFFIX
    )
}

/* ─────────────────────────── the shapes ─────────────────────────── */

/// `^[A-Za-z0-9_-]{1,32}$`.
pub fn valid_device_name(s: &str) -> bool {
    let n = s.encode_utf16().count();
    (1..=32).contains(&n)
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// `^([a-z0-9-]+\.)+[a-z0-9-]+$/i` — at least two labels, each non-empty, each alphanumeric-or-dash.
///
/// THE `/i` IS ASCII-ONLY IN JAVASCRIPT TOO, which is why `is_ascii_alphanumeric` is the exact test rather than
/// `is_alphanumeric`: the Unicode-aware version would accept `d1.agent.tést`, which the source refuses.
pub fn valid_hostname(s: &str) -> bool {
    let labels: Vec<&str> = s.split('.').collect();
    labels.len() >= 2
        && labels
            .iter()
            .all(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
}

/// `^[0-9a-f]{n}$/i` — the device token's shape (`n = 64`) and the panel grant's (`n = 32`).
pub fn is_hex_of_len(s: &str, n: usize) -> bool {
    s.len() == n && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// The three fields `validateDevice` returns, in the source's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceFields {
    pub name: String,
    pub hostname: String,
    pub token: String,
}

/// `String(v || "")` — the truthiness arm the source's `body?.name || ""` performs before every trim.
///
/// `undefined`, `null`, `false`, `0` and `""` are all falsy and all become `""`; every other value is
/// `String(v)`, so `true` → `"true"` and `{}` → `"[object Object]"` (both then fail the name rule).
pub fn js_falsy_string(v: Option<&Value>) -> String {
    js_truthy_string(v, "")
}

/// `String(v || fallback)` — the general form, which `verifySessionToken`'s `String(data.role || "user")`
/// and `validateDevice`'s three fields are both instances of.
pub fn js_truthy_string(v: Option<&Value>, fallback: &str) -> String {
    match v {
        Some(value) if !js_falsy(value) => js_to_string(value),
        _ => fallback.to_string(),
    }
}

/// `String(v)` with `undefined` for a missing key — the difference `String(data.uid)` turns on, and the
/// difference `js_truthy_string` deliberately does not have.
pub fn js_to_string_of(v: Option<&Value>) -> String {
    match v {
        Some(value) => js_to_string(value),
        None => "undefined".to_string(),
    }
}

/// JavaScript truthiness for a JSON value: `undefined`, `null`, `false`, `0`, `NaN` and `""` are falsy.
pub fn js_falsy(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !*b,
        Value::Number(n) => n.as_f64() == Some(0.0) || n.as_f64().is_none_or(f64::is_nan),
        Value::String(s) => s.is_empty(),
        Value::Array(_) | Value::Object(_) => false,
    }
}

/// `String(v)` for the JSON values a request body can carry.
pub fn js_to_string(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => js_number_string(n),
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(|x| match x {
                Value::Null => String::new(),
                other => js_to_string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// `String(number)`: an integer prints without a fraction, which `serde_json`'s own `to_string` does not do for
/// a float that arrived as `1.0`.
fn js_number_string(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    let f = n.as_f64().unwrap_or(f64::NAN);
    if f.fract() == 0.0 && f.abs() < 1e21 {
        return format!("{}", f as i64);
    }
    format!("{f}")
}

/// `validateDevice(body)` — the name, the hostname, the token, IN THAT ORDER (the order is which message a
/// caller with two bad fields gets).
pub fn validate_device(body: &Value) -> Result<DeviceFields, String> {
    let get = |k: &str| body.get(k);
    let name = js_falsy_string(get("name")).trim().to_string();
    let hostname = js_falsy_string(get("hostname")).trim().to_string();
    let token = js_falsy_string(get("token")).trim().to_string();
    if !valid_device_name(&name) {
        return Err(NAME_RULE.to_string());
    }
    if !valid_hostname(&hostname) {
        return Err(hostname_rule());
    }
    if token.chars().count() < 8 {
        return Err(TOKEN_RULE.to_string());
    }
    Ok(DeviceFields {
        name,
        hostname,
        token,
    })
}

/// `maskKey(v)` from `store/users.ts` — the console's only view of a device token.
///
/// Character-wise rather than byte-wise: the source's `slice` counts UTF-16 code units, and a token is hex, so
/// the two agree on every value this route can produce. (`v[0]` on an empty string is `undefined`, which is why
/// the `!v` arm comes first there and here.)
pub fn mask_key(v: &str) -> String {
    if v.is_empty() {
        return "not configured".to_string();
    }
    let units: Vec<char> = v.chars().collect();
    if units.len() <= 6 {
        return format!(
            "{}…{}",
            units[0],
            units[units.len().saturating_sub(2)..]
                .iter()
                .collect::<String>()
        );
    }
    format!(
        "{}…{}",
        units[..3].iter().collect::<String>(),
        units[units.len() - 4..].iter().collect::<String>()
    )
}

/// `mcpConfig(d)` — the Claude Code snippet, and the ONLY place a raw device token is returned.
///
/// `JSON.stringify(snippet, null, 2)` is `serde_json::to_string_pretty`: two spaces per level, `": "` between a
/// key and its value, and the keys in insertion order (this crate enables `preserve_order` for exactly this
/// class of comparison).
pub fn mcp_config(hostname: &Value, token: &Value) -> (String, String) {
    let url = format!("https://{}/mcp", js_to_string(hostname));
    let snippet = serde_json::json!({
        "mcpServers": {
            "summrise-agent": {
                "type": "http",
                "url": url,
                "headers": { "Authorization": format!("Bearer {}", js_to_string(token)) },
            }
        }
    });
    let json = serde_json::to_string_pretty(&snippet).unwrap_or_default();
    (url, json)
}

/// One row of `GET /api/devices` — `{ name, hostname, token, mcp, registeredAt?, lastSeenAt?, lastVersion? }`.
///
/// **A MISSING FIELD IS OMITTED, NOT NULL.** The source writes `registeredAt: d.registeredAt` and
/// `JSON.stringify` drops an `undefined` value, so a record with no `lastSeenAt` has no `lastSeenAt` key —
/// while a record whose field is `null` HAS the key and it is null. `Option` cannot tell those apart here, which
/// is why the map is built by presence.
pub fn device_row(d: &Value) -> Value {
    let mut row = Map::new();
    let has = |k: &str| d.get(k);
    if let Some(v) = has("name") {
        row.insert("name".to_string(), v.clone());
    }
    if let Some(v) = has("hostname") {
        row.insert("hostname".to_string(), v.clone());
    }
    row.insert(
        "token".to_string(),
        Value::String(mask_key(&js_falsy_string(has("token")))),
    );
    let (url, json) = mcp_config(
        has("hostname").unwrap_or(&Value::Null),
        has("token").unwrap_or(&Value::Null),
    );
    let mut mcp = Map::new();
    mcp.insert("url".to_string(), Value::String(url));
    mcp.insert("json".to_string(), Value::String(json));
    row.insert("mcp".to_string(), Value::Object(mcp));
    for k in ["registeredAt", "lastSeenAt", "lastVersion"] {
        if let Some(v) = has(k) {
            row.insert(k.to_string(), v.clone());
        }
    }
    Value::Object(row)
}

/// `GET /api/devices/install-cmd`'s document: both keys are always present, `null` when the upstream had
/// nothing usable — which is what tells the page to fall back to its built-in version.
pub fn install_doc(version: Option<&str>, download: Option<&str>) -> Value {
    let mut m = Map::new();
    m.insert("ok".to_string(), Value::Bool(true));
    m.insert(
        "version".to_string(),
        version.map_or(Value::Null, |v| Value::String(v.to_string())),
    );
    m.insert(
        "download".to_string(),
        download.map_or(Value::Null, |v| Value::String(v.to_string())),
    );
    Value::Object(m)
}

/// `listRegKeys`'s row filter and shape: `{ code, expiresAt }`, keeping only keys whose expiry is in the future.
///
/// **THE FILTER IS THE POINT** — the source's own comment records why: "KV list() keeps returning the NAMES of
/// expired-but-not-yet-reaped keys (value gone, entry visible until compaction) — the devices page showed a pile
/// of dead 'unused keys'". `expiresAt` is `(k.expiration || 0) * 1000`, so a key with NO expiry reported is
/// dropped (0 is not > now).
pub fn reg_key_rows(keys: &[(String, Option<u64>)], now_ms: i64, prefix: &str) -> Vec<Value> {
    keys.iter()
        .map(|(name, expiration)| {
            let expires_at = expiration.unwrap_or(0) as i64 * 1000;
            let code = name.strip_prefix(prefix).unwrap_or(name).to_string();
            (code, expires_at)
        })
        .filter(|(_, expires_at)| *expires_at > now_ms)
        .map(|(code, expires_at)| {
            let mut m = Map::new();
            m.insert("code".to_string(), Value::String(code));
            m.insert("expiresAt".to_string(), Value::Number(expires_at.into()));
            Value::Object(m)
        })
        .collect()
}

/// The two refusals `handleDeviceRename` makes on the SHAPE of its body, in the source's order — before any KV
/// read, so a malformed rename costs nothing.
pub fn rename_shape_error(new_name: &str, hostname: &str) -> Option<String> {
    if !valid_device_name(new_name) {
        return Some(NAME_RULE.to_string());
    }
    if !hostname.is_empty() && !valid_hostname(hostname) {
        return Some(hostname_rule());
    }
    None
}

/* ─────────────────── the decisions over a stored list ─────────────────── */

/// `readDevicesRaw`: a missing value, a non-array, or invalid JSON all answer `[]` — and a NON-ARRAY is `[]`
/// rather than an error, which is what keeps one corrupt write from taking the console's device page down.
pub fn devices_from_raw(raw: Option<&str>) -> Vec<Value> {
    let Some(text) = raw.filter(|t| !t.is_empty()) else {
        return Vec::new();
    };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Array(items)) => items,
        _ => Vec::new(),
    }
}

/// `{...base, k: v}` — a key that already exists KEEPS ITS POSITION, a new one is appended. That is JavaScript's
/// own spread order and it is what the records written back to KV are compared as bytes against.
pub fn js_spread(base: &Map<String, Value>, overrides: Vec<(String, Value)>) -> Map<String, Value> {
    let mut out = base.clone();
    for (k, v) in overrides {
        out.insert(k, v);
    }
    out
}

fn device_at(devices: &[Value], name: &str) -> Option<usize> {
    devices
        .iter()
        .position(|d| d.get("name").and_then(Value::as_str) == Some(name))
}

/// `getDevice(env, name)`: the FIRST record with that name (`find`, not the last).
pub fn find_device(devices: &[Value], name: &str) -> Option<Value> {
    device_at(devices, name).map(|i| devices[i].clone())
}

/// `upsertDevice`'s body: replace in place, or append. **AND THE round-106 REPAIR, WHICH IS A DECISION**: an
/// admin edit that carries no `proxySecret` must not wipe the stored one ("`/panel/` token injection broke
/// permanently until re-registration"), so a falsy incoming secret takes the stored one — APPENDED, which is
/// where `{...device, proxySecret}` would put it.
pub fn upsert_into(devices: &mut Vec<Value>, device: Value) -> Value {
    let incoming_secret = device.get("proxySecret").map(|v| !js_falsy(v));
    match device_at(
        devices,
        device.get("name").and_then(Value::as_str).unwrap_or(""),
    ) {
        Some(i) => {
            let mut merged = device;
            let stored_secret = devices[i].get("proxySecret").cloned();
            if incoming_secret != Some(true) {
                if let Some(secret) = stored_secret.filter(|v| !js_falsy(v)) {
                    if let Some(obj) = merged.as_object() {
                        merged = Value::Object(js_spread(
                            obj,
                            vec![("proxySecret".to_string(), secret)],
                        ));
                    }
                }
            }
            devices[i] = merged.clone();
            merged
        }
        None => {
            devices.push(device.clone());
            device
        }
    }
}

/// `insertDevice`: append only when the name is free — the existence check runs INSIDE the lock, which is what
/// closed the round-122 takeover (`getDevice` then `upsertDevice` was check-then-act).
pub fn insert_into(devices: &mut Vec<Value>, device: Value) -> Option<Value> {
    let name = device.get("name").and_then(Value::as_str).unwrap_or("");
    if device_at(devices, name).is_some() {
        return None;
    }
    devices.push(device.clone());
    Some(device)
}

/// `deleteDevice`: filter by name, and answer whether anything went.
pub fn delete_from(devices: &mut Vec<Value>, name: &str) -> bool {
    let before = devices.len();
    devices.retain(|d| d.get("name").and_then(Value::as_str) != Some(name));
    devices.len() != before
}

/// `renameDevice`'s three outcomes.
#[derive(Clone, Debug, PartialEq)]
pub enum RenameOutcome {
    Renamed(Value),
    NotFound,
    NameTaken,
}

/// `renameDevice(env, oldName, newName, hostname?)`. The record is REPLACED IN PLACE with `name` (and
/// `hostname` when one was given) overridden on a spread of the OLD record, so the token, the proxy secret, the
/// registration date and every metadata field survive — which is the whole reason the route exists ("the old
/// flow forced delete + re-add, which rotated the token and invalidated the device's own config.yaml").
#[allow(clippy::ptr_arg)] // the source's `findIndex` + in-place replace needs the list, not a slice
pub fn rename_in(
    devices: &mut Vec<Value>,
    old_name: &str,
    new_name: &str,
    hostname: Option<&str>,
) -> RenameOutcome {
    let Some(i) = device_at(devices, old_name) else {
        return RenameOutcome::NotFound;
    };
    if devices
        .iter()
        .enumerate()
        .any(|(j, d)| j != i && d.get("name").and_then(Value::as_str) == Some(new_name))
    {
        return RenameOutcome::NameTaken;
    }
    let mut overrides = vec![("name".to_string(), Value::String(new_name.to_string()))];
    if let Some(h) = hostname {
        overrides.push(("hostname".to_string(), Value::String(h.to_string())));
    }
    let updated = match devices[i].as_object() {
        Some(base) => Value::Object(js_spread(base, overrides)),
        None => return RenameOutcome::NotFound,
    };
    devices[i] = updated.clone();
    RenameOutcome::Renamed(updated)
}

/* ─────────────────── plugin links (store/plugins.ts) ─────────────────── */

/// `readFreshPluginLinks`: `null` when the stored blob is CORRUPT — and the callers must abort rather than write
/// (`{}` would delete every link), which is why this is an `Option` and not an empty map.
pub fn plugin_links_from_raw(raw: Option<&str>) -> Option<Map<String, Value>> {
    match raw {
        // An EMPTY string is falsy in the source (`raw ? parse : {}`) — absent, not corrupt.
        None | Some("") => Some(Map::new()),
        Some(text) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(m)) => Some(m),
            _ => None,
        },
    }
}

/// `getPluginByToken`'s expiry rule: **A MISSING `expiresAt` IS EXPIRED** — links made before the TTL feature
/// shipped never expired, which was permanent `browser_*` control.
pub fn plugin_link_expired(link: &Value, now_ms: i64) -> bool {
    match link.get("expiresAt").and_then(Value::as_f64) {
        Some(exp) => exp < now_ms as f64,
        None => true,
    }
}

/// `migratePluginLinks` — re-point every link that names `old_name`; answers whether anything moved (which is
/// what decides whether a write happens at all).
pub fn migrate_links(links: &mut Map<String, Value>, old_name: &str, new_name: &str) -> bool {
    let mut migrated = false;
    for link in links.values_mut() {
        if link.get("device").and_then(Value::as_str) == Some(old_name) {
            if let Some(obj) = link.as_object_mut() {
                obj.insert("device".to_string(), Value::String(new_name.to_string()));
                migrated = true;
            }
        }
    }
    migrated
}

/// `delete obj[key]` FOR EVERY MATCHING KEY, IN A MAP THAT KEEPS ITS ORDER.
///
/// **`Map::remove` IS NOT `delete`.** With `preserve_order` a `serde_json::Map` is an `IndexMap`, and its
/// `remove` is `swap_remove`: the LAST entry is moved into the hole, so deleting the first of three keys
/// reorders the other two. Measured by the corpus on the delete path (the shipping TypeScript wrote
/// `deadplugin, legacyplugin` and this worker wrote `legacyplugin, deadplugin`) — the same records, in a
/// different order, written to KV. Rebuilding is the only removal that keeps JavaScript's order.
pub fn map_without(links: &Map<String, Value>, doomed: &[String]) -> Map<String, Value> {
    let mut out = Map::new();
    for (key, value) in links {
        if !doomed.contains(key) {
            out.insert(key.clone(), value.clone());
        }
    }
    out
}

/// `removePluginLinksForDevice` — delete every link for one device; answers how many went.
pub fn remove_links_for_device(links: &mut Map<String, Value>, device: &str) -> usize {
    let doomed: Vec<String> = links
        .iter()
        .filter(|(_, l)| l.get("device").and_then(Value::as_str) == Some(device))
        .map(|(token, _)| token.clone())
        .collect();
    if !doomed.is_empty() {
        *links = map_without(links, &doomed);
    }
    doomed.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// **THE FOUR SHAPES, INCLUDING THE ONES A "LOOKS RIGHT" TRANSLITERATION GETS WRONG.**
    ///
    /// MUTATION: make `valid_hostname` accept a single label (drop the `>= 2`).
    /// RESULT:   `the_name_and_hostname_rules` fails on `"localhost"`, which the source's
    ///           `^([a-z0-9-]+\.)+[a-z0-9-]+$` refuses.
    #[test]
    fn the_name_and_hostname_rules() {
        assert!(valid_device_name("d1"));
        assert!(valid_device_name("d-1_x"));
        assert!(valid_device_name(&"a".repeat(32)));
        assert!(!valid_device_name(""));
        assert!(!valid_device_name(&"a".repeat(33)));
        assert!(!valid_device_name("d 1"));
        assert!(!valid_device_name("d1."));
        assert!(!valid_device_name("d1/../../etc"));
        assert!(valid_hostname("d1.agent.test"));
        assert!(valid_hostname("D1.Agent.Test"));
        assert!(valid_hostname("a-b.c-d"));
        assert!(!valid_hostname("localhost"));
        assert!(!valid_hostname("d1..test"));
        assert!(!valid_hostname(".d1.test"));
        assert!(!valid_hostname("d1.test."));
        assert!(!valid_hostname("d1 agent.test"));
        assert!(
            !valid_hostname("d1.agent.tést"),
            "the /i flag is ASCII-only"
        );
        assert!(is_hex_of_len(&"a".repeat(64), 64));
        assert!(is_hex_of_len(&"A".repeat(64), 64));
        assert!(!is_hex_of_len(&"a".repeat(63), 64));
        assert!(!is_hex_of_len(&"g".repeat(64), 64));
        assert!(is_hex_of_len(&"0".repeat(32), 32));
    }

    /// The truthiness arm, which is a decision rather than a detail: `{name: 0}` and `{name: false}` must give
    /// the NAME message, and `{name: {}}` must give it too (as `"[object Object]"`), not a token message.
    #[test]
    fn validate_device_takes_the_sources_order_and_truthiness() {
        assert_eq!(
            validate_device(
                &json!({"name": "d1", "hostname": "d1.agent.test", "token": "0123456789abcdef"})
            ),
            Ok(DeviceFields {
                name: "d1".into(),
                hostname: "d1.agent.test".into(),
                token: "0123456789abcdef".into()
            })
        );
        // Trims first, then tests.
        assert_eq!(
            validate_device(
                &json!({"name": " d1 ", "hostname": " d1.agent.test ", "token": " 01234567 "})
            )
            .unwrap()
            .name,
            "d1"
        );
        // THE ORDER: a body with everything wrong reports the NAME.
        assert_eq!(
            validate_device(&json!({"name": "", "hostname": "", "token": ""})),
            Err(NAME_RULE.to_string())
        );
        // ...and a good name with a bad hostname reports the hostname, not the token.
        assert_eq!(
            validate_device(&json!({"name": "d1", "hostname": "localhost", "token": ""})),
            Err(hostname_rule())
        );
        // `0`, `false` and a missing body are all the empty string → the NAME message.
        for body in [json!({"name": 0}), json!({"name": false}), json!({})] {
            assert_eq!(validate_device(&body), Err(NAME_RULE.to_string()));
        }
        // A token of 7 code units fails; 8 passes.
        assert_eq!(
            validate_device(
                &json!({"name": "d1", "hostname": "d1.agent.test", "token": "1234567"})
            ),
            Err(TOKEN_RULE.to_string())
        );
        assert!(validate_device(
            &json!({"name": "d1", "hostname": "d1.agent.test", "token": "12345678"})
        )
        .is_ok());
        // The message names the DEFAULT device host even when a deployment overrides the suffix — the source's
        // literal does, and this is built from the same constant rather than transcribed.
        assert!(hostname_rule().ends_with(crate::device::DEFAULT_DEVICE_HOST_SUFFIX));
    }

    /// `maskKey` — all three arms, including the `length <= 6` one whose `v[0] + "…" + v.slice(-2)` overlaps.
    #[test]
    fn mask_key_is_the_sources() {
        assert_eq!(mask_key(""), "not configured");
        assert_eq!(mask_key("abcdef"), "a…ef");
        assert_eq!(mask_key("abc"), "a…bc");
        assert_eq!(mask_key("a"), "a…a");
        assert_eq!(mask_key("0123456789abcdef"), "012…cdef");
    }

    /// The MCP snippet is a byte contract: two-space indent, insertion order, and an `undefined` hostname is the
    /// string `"undefined"` (the source's template literal does not care).
    #[test]
    fn the_mcp_snippet_is_byte_for_byte_the_sources() {
        let (url, json) = mcp_config(&json!("d1.agent.test"), &json!("tok-123"));
        assert_eq!(url, "https://d1.agent.test/mcp");
        assert_eq!(
            json,
            "{\n  \"mcpServers\": {\n    \"summrise-agent\": {\n      \"type\": \"http\",\n      \"url\": \"https://d1.agent.test/mcp\",\n      \"headers\": {\n        \"Authorization\": \"Bearer tok-123\"\n      }\n    }\n  }\n}"
        );
        let (url, _) = mcp_config(&Value::Null, &Value::Null);
        assert_eq!(url, "https://null/mcp");
        let (url, _) = mcp_config(&Value::Bool(true), &Value::Null);
        assert_eq!(url, "https://true/mcp");
    }

    /// **THE ROW'S OMISSION RULE IS THE WHOLE TEST.** A record with no `lastSeenAt` must have no such key, and a
    /// record whose `lastSeenAt` is `null` must carry it as null.
    #[test]
    fn the_device_row_omits_absent_fields_and_keeps_the_order() {
        let row = device_row(&json!({
            "name": "d1",
            "hostname": "d1.agent.test",
            "token": "0123456789abcdef",
            "proxySecret": "must-not-be-here"
        }));
        let text = serde_json::to_string(&row).unwrap();
        assert!(
            text.starts_with(
                "{\"name\":\"d1\",\"hostname\":\"d1.agent.test\",\"token\":\"012…cdef\",\"mcp\":{"
            ),
            "{text}"
        );
        assert!(!text.contains("lastSeenAt"));
        assert!(
            !text.contains("must-not-be-here"),
            "the proxy secret is never in a row"
        );

        let row = device_row(&json!({"name": "d1", "registeredAt": 5, "lastSeenAt": null}));
        let text = serde_json::to_string(&row).unwrap();
        assert!(text.contains("\"registeredAt\":5"));
        assert!(text.contains("\"lastSeenAt\":null"));
    }

    /// The install document always carries both keys — that is how the page picks its fallback.
    #[test]
    fn the_install_doc_always_has_both_keys() {
        assert_eq!(
            serde_json::to_string(&install_doc(None, None)).unwrap(),
            "{\"ok\":true,\"version\":null,\"download\":null}"
        );
        assert_eq!(
            serde_json::to_string(&install_doc(Some("1.2.3"), None)).unwrap(),
            "{\"ok\":true,\"version\":\"1.2.3\",\"download\":null}"
        );
        assert_eq!(
            serde_json::to_string(&install_doc(Some("1.2.3"), Some("https://x/y.tgz"))).unwrap(),
            "{\"ok\":true,\"version\":\"1.2.3\",\"download\":\"https://x/y.tgz\"}"
        );
    }

    /// The registration-key list, expiry filter included — a key whose `expiration` was not reported is dead.
    #[test]
    fn reg_keys_drop_the_expired_and_keep_the_live() {
        let keys = vec![
            ("regkey:live".to_string(), Some(2000)),
            ("regkey:dead".to_string(), Some(500)),
            ("regkey:none".to_string(), None),
        ];
        let rows = reg_key_rows(&keys, 1_000_000, "regkey:");
        assert_eq!(rows.len(), 1);
        assert_eq!(
            serde_json::to_string(&rows[0]).unwrap(),
            "{\"code\":\"live\",\"expiresAt\":2000000}"
        );
    }

    /// **THE RENAME'S TWO OUTCOMES AND WHAT IT MUST PRESERVE.** The token and the proxy secret are the point of
    /// the route; the `name_taken` check must not fire on the device's OWN name (index `i` is excluded) and the
    /// hostname is only overridden when one was given.
    #[test]
    fn rename_preserves_the_record_and_refuses_a_taken_name() {
        let mut devices = vec![
            json!({"name": "d1", "hostname": "d1.agent.test", "token": "tok-a", "proxySecret": "s", "registeredAt": 7}),
            json!({"name": "d2", "hostname": "d2.agent.test", "token": "tok-b"}),
        ];
        match rename_in(&mut devices, "d1", "d9", None) {
            RenameOutcome::Renamed(d) => {
                assert_eq!(d.get("token").unwrap(), "tok-a");
                assert_eq!(d.get("proxySecret").unwrap(), "s");
                assert_eq!(d.get("registeredAt").unwrap(), 7);
                assert_eq!(d.get("hostname").unwrap(), "d1.agent.test");
                // THE ORDER IS UNCHANGED: name, hostname, token, proxySecret, registeredAt.
                assert_eq!(
                    serde_json::to_string(&d).unwrap(),
                    "{\"name\":\"d9\",\"hostname\":\"d1.agent.test\",\"token\":\"tok-a\",\"proxySecret\":\"s\",\"registeredAt\":7}"
                );
            }
            other => panic!("expected a rename, got {other:?}"),
        }
        match rename_in(&mut devices, "d9", "d2", None) {
            RenameOutcome::NameTaken => {}
            other => panic!("expected name_taken, got {other:?}"),
        }
        // Renaming to its OWN name is not a conflict.
        assert!(matches!(
            rename_in(&mut devices, "d9", "d9", Some("new.agent.test")),
            RenameOutcome::Renamed(_)
        ));
        assert_eq!(devices[0].get("hostname").unwrap(), "new.agent.test");
        assert_eq!(
            rename_in(&mut devices, "nope", "x", None),
            RenameOutcome::NotFound
        );
    }

    /// `upsertDevice`'s round-106 repair, and the position the repaired key takes.
    #[test]
    fn upsert_keeps_a_stored_proxy_secret() {
        let mut devices = vec![json!({"name": "d1", "token": "tok", "proxySecret": "stored"})];
        let saved = upsert_into(
            &mut devices,
            json!({"name": "d1", "token": "tok", "registeredAt": 9}),
        );
        assert_eq!(saved.get("proxySecret").unwrap(), "stored");
        assert_eq!(
            serde_json::to_string(&saved).unwrap(),
            "{\"name\":\"d1\",\"token\":\"tok\",\"registeredAt\":9,\"proxySecret\":\"stored\"}"
        );
        // An INCOMING secret wins, and keeps its own position.
        let mut devices = vec![json!({"name": "d1", "proxySecret": "stored"})];
        let saved = upsert_into(&mut devices, json!({"name": "d1", "proxySecret": "fresh"}));
        assert_eq!(saved.get("proxySecret").unwrap(), "fresh");
        // A NEW device is appended.
        let mut devices = vec![json!({"name": "d1"})];
        upsert_into(&mut devices, json!({"name": "d2"}));
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[1].get("name").unwrap(), "d2");
        assert!(insert_into(&mut devices, json!({"name": "d2"})).is_none());
        assert!(insert_into(&mut devices, json!({"name": "d3"})).is_some());
        assert!(delete_from(&mut devices, "d3"));
        assert!(!delete_from(&mut devices, "d3"));
    }

    /// `readDevicesRaw`'s three "not a list" arms, and the plugin map's one "corrupt" arm — which must be `None`
    /// so a caller aborts instead of writing `{}` over every link.
    #[test]
    fn the_raw_reads_treat_corruption_the_way_the_source_does() {
        assert!(devices_from_raw(None).is_empty());
        assert!(devices_from_raw(Some("")).is_empty());
        assert!(devices_from_raw(Some("{")).is_empty());
        assert!(devices_from_raw(Some("{}")).is_empty());
        assert_eq!(devices_from_raw(Some("[{\"name\":\"d1\"}]")).len(), 1);
        assert_eq!(plugin_links_from_raw(None).unwrap().len(), 0);
        assert!(plugin_links_from_raw(Some("[]")).is_none());
        assert!(plugin_links_from_raw(Some("{")).is_none());
        assert_eq!(
            plugin_links_from_raw(Some("{\"t\":{\"device\":\"d1\"}}"))
                .unwrap()
                .len(),
            1
        );
    }

    /// **THE ORDER OF WHAT IS LEFT IS PART OF THE PORT.** `Map::remove` would swap the last entry into the
    /// hole; `delete obj[key]` leaves the rest in place, and the blob written back to KV is compared as bytes.
    ///
    /// MUTATION: replace `map_without`'s rebuild with `links.remove(token)`.
    /// RESULT:   `the_link_map_keeps_its_order` fails — three seeded links come back as
    ///           `legacyplugin, deadplugin` where the source wrote `deadplugin, legacyplugin`.
    #[test]
    fn the_link_map_keeps_its_order() {
        let mut links = Map::new();
        for key in ["liveplugin", "deadplugin", "legacyplugin"] {
            links.insert(key.to_string(), serde_json::json!({"device": "d1"}));
        }
        let mut links = {
            let mut m = Map::new();
            for (k, v) in &links {
                m.insert(k.clone(), v.clone());
            }
            m
        };
        let removed = map_without(&links, &["liveplugin".to_string()]);
        assert_eq!(
            removed.keys().cloned().collect::<Vec<_>>(),
            vec!["deadplugin".to_string(), "legacyplugin".to_string()]
        );
        // ...and the in-place form the store uses agrees.
        links = removed;
        assert_eq!(remove_links_for_device(&mut links, "d1"), 2);
        assert!(links.is_empty());
    }

    /// The plugin-link migrations: the missing-expiry rule, the re-point, and the per-device revoke.
    #[test]
    fn plugin_links_migrate_and_revoke() {
        assert!(plugin_link_expired(&json!({"device": "d1"}), 1000));
        assert!(plugin_link_expired(
            &json!({"device": "d1", "expiresAt": 999}),
            1000
        ));
        assert!(!plugin_link_expired(
            &json!({"device": "d1", "expiresAt": 1001}),
            1000
        ));
        let mut links = plugin_links_from_raw(Some(
            "{\"t1\":{\"device\":\"d1\"},\"t2\":{\"device\":\"d2\"},\"t3\":{\"device\":\"d1\"}}",
        ))
        .unwrap();
        assert!(migrate_links(&mut links, "d1", "d9"));
        assert!(!migrate_links(&mut links, "d1", "d9"));
        assert_eq!(links["t1"]["device"], "d9");
        assert_eq!(links["t2"]["device"], "d2");
        assert_eq!(remove_links_for_device(&mut links, "d9"), 2);
        assert_eq!(links.len(), 1);
        assert_eq!(remove_links_for_device(&mut links, "d9"), 0);
    }

    /// `rename_shape_error` — shape only, in the source's order, and an EMPTY hostname is "leave it alone" rather
    /// than "invalid".
    #[test]
    fn the_rename_shape_check_matches_the_source() {
        assert_eq!(rename_shape_error("d9", ""), None);
        assert_eq!(rename_shape_error("d9", "d9.agent.test"), None);
        assert_eq!(rename_shape_error("", ""), Some(NAME_RULE.to_string()));
        assert_eq!(rename_shape_error("d 9", ""), Some(NAME_RULE.to_string()));
        assert_eq!(rename_shape_error("d9", "localhost"), Some(hostname_rule()));
    }
}
