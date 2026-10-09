//! THE DEVICE REGISTRY'S KV ADAPTER — every read and write the devices family makes, in one place, so the
//! decisions in `device_registry.rs` never see a binding.
//!
//! WHY AN ADAPTER AND NOT CALLS FROM THE HANDLERS. The exception §2 carves out is *a platform call that decides
//! nothing*, and `workers-rs`'s KV binding decides nothing: `get`, `put`, `delete`, `list`. What IS a decision —
//! which key, what TTL, what the cached copy means, whether a read-modify-write needs the lock — is here, once,
//! where the corpus can reach it and where the write-through cache cannot be forgotten in a fourth handler.
//!
//! THE KEYS, each matching the source module that owns it:
//!
//! ```text
//!     devices:v1              -> JSON array of device records (store/devices.ts)
//!     plugins:v1              -> token -> { device, createdAt, expiresAt } (store/plugins.ts)
//!     regkey:<code>           -> "1", 1 h TTL — an UNSPENT one-time install key
//!     reggrant:<code>         -> "1", 15 min TTL — the same install's register handoff
//!     regclaim:<code>         -> "1", 60 s — the tunnel-token single-flight lock
//!     regclaim2:<code>        -> "1", 60 s — /api/register's own lock
//!     panelgrant:<code>       -> JSON { device, mintedAt }, 120 s TTL
//!     cf:api_token            -> the account-level Cloudflare API token
//!     auth:admin_password     -> the console's admin hash (`salt:hash`), read by the session gate
//!     user:<uid>              -> a console user record, read by the session gate
//!     sess-revoked:<cookie>   -> "1" when a session was logged out
//! ```
//!
//! **THE CACHE IS THE CRATE'S, NOT A SECOND ONE.** `store.rs` already carries `cget`/`cset`/`cdel` as decisions
//! (`ttl_for`, the 512 bound, the insertion-order eviction), and `devices:`, `plugins:`, `cf:`, `auth:` and
//! `user:` are all on its SHORT-TTL list — the source put them there for a measured reason ("a device registered
//! via /api/register on a COLD isolate stayed invisible on hot isolates for up to 24h — /proxy and /mcp 404'd on
//! the new device"). Every write below refreshes it (write-through), which is what makes an admin change take
//! effect on the hot isolate at once.
//!
//! **THE TWO LOCKS ARE MODULE STATE, ONE PER READ-MODIFY-WRITE BLOB**, mirroring `withKeyLock(DEVICES_KEY, …)`
//! and `withKeyLock(PLUGIN_KEY, …)`. They serialize the section ACROSS awaits within one isolate, which is the
//! guarantee the source's comments record losing (a clobbered device, a resurrected plugin link).

use serde_json::{Map, Value};
use worker::*;

use crate::device_registry::{self, devices_from_raw, plugin_links_from_raw, RenameOutcome};
use crate::ip_rate_limit::Counters;
use crate::key_lock::KeyLock;

/// `Date.now()` — the crate's one clock read. (The platform half of the time source: `worker::Date::now()` is
/// `new Date()`, the same value the JavaScript reads.)
pub fn now_ms() -> i64 {
    Date::now().as_millis() as i64
}

/// The per-isolate counter table `createIpRateLimiter` keeps in module scope.
pub static PUBLIC_RATE_COUNTERS: std::sync::Mutex<Counters> = std::sync::Mutex::new(Vec::new());

/// `withKeyLock(DEVICES_KEY, …)`.
static DEVICES_LOCK: KeyLock = KeyLock::new();
/// `withKeyLock(PLUGIN_KEY, …)`.
static PLUGIN_LOCK: KeyLock = KeyLock::new();
/// `__notRevoked` — the session gate's 60 s negative cache, bounded at 512 like the source's `Map`.
pub static NOT_REVOKED: std::sync::Mutex<Vec<(String, bool, i64)>> =
    std::sync::Mutex::new(Vec::new());

const DEVICES_KEY: &str = "devices:v1";
const PLUGIN_KEY: &str = "plugins:v1";
const REGKEY_PREFIX: &str = "regkey:";
const REGGRANT_PREFIX: &str = "reggrant:";

/* ─────────────────────────── the KV primitives ─────────────────────────── */

/// One KV string. `Ok(None)` from the binding and a failed call both answer `None`, which is the source's
/// `env.KEYS.get(key)` returning null and the `catch` arms around it.
pub(crate) async fn kv_text(env: &Env, key: &str) -> Option<String> {
    let kv = env.kv("KEYS").ok()?;
    kv.get(key).text().await.ok().flatten()
}

/// `cget(key)` then `env.KEYS.get(key)`, with the write-through cache — the shape every `store/*` read has.
pub(crate) async fn kv_text_cached(env: &Env, key: &str) -> Option<String> {
    let now = now_ms();
    if let Some(hit) = crate::store::cached_get(key, now) {
        return hit.as_str().map(str::to_string);
    }
    let value = kv_text(env, key).await;
    crate::store::cache_put(
        key,
        value
            .as_ref()
            .map_or(Value::Null, |v| Value::String(v.clone())),
        now,
    );
    value
}

/// `env.KEYS.put(key, value[, {expirationTtl}])` + `cset` — the write-through half, in one place.
pub(crate) async fn kv_put(env: &Env, key: &str, value: &str, ttl_secs: Option<u64>) {
    let Ok(kv) = env.kv("KEYS") else {
        return;
    };
    let builder = match kv.put(key, value) {
        Ok(builder) => builder,
        Err(_) => return,
    };
    let builder = match ttl_secs {
        Some(ttl) => builder.expiration_ttl(ttl),
        None => builder,
    };
    if builder.execute().await.is_ok() {
        crate::store::cache_put(key, Value::String(value.to_string()), now_ms());
    }
}

/// `env.KEYS.delete(key)` + `cdel`.
pub(crate) async fn kv_delete(env: &Env, key: &str) {
    let Ok(kv) = env.kv("KEYS") else {
        return;
    };
    if kv.delete(key).await.is_ok() {
        crate::store::cache_del(key);
    }
}

/// `env.KEYS.list({prefix})` — the first page, like the source's single call.
pub(crate) async fn kv_list_prefix(env: &Env, prefix: &str) -> Vec<(String, Option<u64>)> {
    let Ok(kv) = env.kv("KEYS") else {
        return Vec::new();
    };
    match kv.list().prefix(prefix.to_string()).execute().await {
        Ok(response) => response
            .keys
            .into_iter()
            .map(|k| (k.name, k.expiration))
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// `env.KEYS.get(key)` for a JSON-valued record: the parse, with a corrupt value answering `None` — the arm
/// `getJSON` takes, and the reason a broken write does not take the console down.
pub(crate) async fn kv_json_cached(env: &Env, key: &str) -> Option<Value> {
    let raw = kv_text_cached(env, key).await?;
    serde_json::from_str(&raw).ok()
}

/* ─────────────────────────── devices:v1 ─────────────────────────── */

/// `readDevicesRaw` — a FRESH KV read, used inside the lock.
async fn read_devices_fresh(env: &Env) -> Vec<Value> {
    devices_from_raw(kv_text(env, DEVICES_KEY).await.as_deref())
}

/// `listDevices` — through the cache, which is the read every caller outside a lock makes.
pub async fn list_devices(env: &Env) -> Vec<Value> {
    let now = now_ms();
    if let Some(hit) = crate::store::cached_get(DEVICES_KEY, now) {
        return serde_json::from_value(hit).unwrap_or_default();
    }
    let devices = read_devices_fresh(env).await;
    crate::store::cache_put(DEVICES_KEY, Value::Array(devices.clone()), now);
    devices
}

/// `getDevice(env, name)`.
pub async fn get_device(env: &Env, name: &str) -> Option<Value> {
    device_registry::find_device(&list_devices(env).await, name)
}

/// `saveDevices` — the put plus the cache refresh, which is what `cset(DEVICES_KEY, devices)` is for.
async fn save_devices(env: &Env, devices: &[Value]) {
    let text =
        serde_json::to_string(&Value::Array(devices.to_vec())).unwrap_or_else(|_| "[]".into());
    kv_put(env, DEVICES_KEY, &text, None).await;
}

/// `upsertDevice` — lock, FRESH read, merge, save.
pub async fn upsert_device(env: &Env, device: Value) -> Value {
    let _guard = DEVICES_LOCK.lock().await;
    let mut devices = read_devices_fresh(env).await;
    let saved = device_registry::upsert_into(&mut devices, device);
    save_devices(env, &devices).await;
    saved
}

/// `insertDevice` — lock, fresh read, insert-if-absent. `None` means the name was taken.
pub async fn insert_device(env: &Env, device: Value) -> Option<Value> {
    let _guard = DEVICES_LOCK.lock().await;
    let mut devices = read_devices_fresh(env).await;
    let inserted = device_registry::insert_into(&mut devices, device)?;
    save_devices(env, &devices).await;
    Some(inserted)
}

/// `deleteDevice`.
pub async fn delete_device(env: &Env, name: &str) -> bool {
    let _guard = DEVICES_LOCK.lock().await;
    let mut devices = read_devices_fresh(env).await;
    let removed = device_registry::delete_from(&mut devices, name);
    save_devices(env, &devices).await;
    removed
}

/// `renameDevice`.
pub async fn rename_device(
    env: &Env,
    old_name: &str,
    new_name: &str,
    hostname: Option<&str>,
) -> RenameOutcome {
    let _guard = DEVICES_LOCK.lock().await;
    let mut devices = read_devices_fresh(env).await;
    let outcome = device_registry::rename_in(&mut devices, old_name, new_name, hostname);
    if matches!(outcome, RenameOutcome::Renamed(_)) {
        save_devices(env, &devices).await;
    }
    outcome
}

/* ─────────────────────────── registration keys ─────────────────────────── */

/// `randomHex(n)` — the platform CSPRNG, hex, lowercased exactly as `createRegKey` does.
fn random_hex_lower(bytes: usize) -> String {
    crate::webcrypto::random_hex(bytes).to_lowercase()
}

/// `createRegKey` — 8 bytes of randomness, 1 h TTL.
pub async fn create_reg_key(env: &Env) -> String {
    let code = random_hex_lower(8);
    kv_put(env, &format!("{REGKEY_PREFIX}{code}"), "1", Some(60 * 60)).await;
    code
}

/// `hasRegKey` — the code is lowercased AGAIN here (the endpoint already lowered it, and so does the store:
/// "same code, two spellings" would otherwise be two keys).
pub async fn has_reg_key(env: &Env, code: &str) -> bool {
    if code.is_empty() {
        return false;
    }
    kv_text(env, &format!("{REGKEY_PREFIX}{}", code.to_lowercase()))
        .await
        .is_some_and(|v| !v.is_empty())
}

/// `hasRegGrant`.
pub async fn has_reg_grant(env: &Env, code: &str) -> bool {
    if code.is_empty() {
        return false;
    }
    kv_text(env, &format!("{REGGRANT_PREFIX}{}", code.to_lowercase()))
        .await
        .is_some_and(|v| !v.is_empty())
}

/// `deleteRegKey`.
pub async fn delete_reg_key(env: &Env, code: &str) {
    if code.is_empty() {
        return;
    }
    kv_delete(env, &format!("{REGKEY_PREFIX}{}", code.to_lowercase())).await;
}

/// `deleteRegGrant`.
pub async fn delete_reg_grant(env: &Env, code: &str) {
    if code.is_empty() {
        return;
    }
    kv_delete(env, &format!("{REGGRANT_PREFIX}{}", code.to_lowercase())).await;
}

/// `consumeRegKey` — spend the key by deleting it and issuing the 15-minute grant the same install completes
/// `/api/register` with.
pub async fn consume_reg_key(env: &Env, code: &str) {
    let k = code.to_lowercase();
    if k.is_empty() {
        return;
    }
    kv_delete(env, &format!("{REGKEY_PREFIX}{k}")).await;
    kv_put(env, &format!("{REGGRANT_PREFIX}{k}"), "1", Some(15 * 60)).await;
}

/// `listRegKeys` — the live keys, with their remaining TTL.
pub async fn list_reg_keys(env: &Env) -> Vec<Value> {
    let keys = kv_list_prefix(env, REGKEY_PREFIX).await;
    device_registry::reg_key_rows(&keys, now_ms(), REGKEY_PREFIX)
}

/* ─────────────────────────── the claim locks ─────────────────────────── */

/// `env.KEYS.get("regclaim:<k>")` — the single-flight lock's read.
pub async fn reg_claim(env: &Env, prefix: &str, code: &str) -> bool {
    kv_text(env, &format!("{prefix}{code}"))
        .await
        .is_some_and(|v| !v.is_empty())
}

/// `env.KEYS.put("regclaim:<k>", "1", {expirationTtl: 60})` — the 60 s window the source's comment measures:
/// "the lock key makes the claim atomic-enough (KV put-if-absent is not available; a 30s TTL lock bounds the
/// race)" — and the code's own number is 60.
pub async fn take_reg_claim(env: &Env, prefix: &str, code: &str) {
    kv_put(env, &format!("{prefix}{code}"), "1", Some(60)).await;
}

/// The `finally` arm: `env.KEYS.delete(claimKey).catch(() => {})`.
pub async fn release_reg_claim(env: &Env, prefix: &str, code: &str) {
    kv_delete(env, &format!("{prefix}{code}")).await;
}

/* ─────────────────────────── panel grants ─────────────────────────── */

/// One panel grant, as `store/grants.ts` shapes it.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelGrant {
    pub device: String,
    pub minted_at: i64,
}

/// `createPanelGrant` — 16 bytes of randomness (32 hex), 120 s TTL, `{device, mintedAt}` as the value.
pub async fn create_panel_grant(env: &Env, device: &str) -> String {
    let code = random_hex_lower(16);
    let value = serde_json::json!({ "device": device, "mintedAt": now_ms() });
    kv_put(
        env,
        &format!("panelgrant:{code}"),
        &serde_json::to_string(&value).unwrap_or_default(),
        Some(120),
    )
    .await;
    code
}

/// `getPanelGrant` — **THE SHAPE CHECK COMES FIRST**, so a garbage probe never costs a KV read; a malformed
/// stored value behaves like a missing grant.
pub async fn get_panel_grant(env: &Env, code: &str) -> Option<PanelGrant> {
    let k = code.to_lowercase();
    if !device_registry::is_hex_of_len(&k, 32) {
        return None;
    }
    let raw = kv_text(env, &format!("panelgrant:{k}")).await?;
    let parsed: Value = serde_json::from_str(&raw).ok()?;
    let device = parsed.get("device").and_then(Value::as_str)?;
    if device.is_empty() {
        return None;
    }
    Some(PanelGrant {
        device: device.to_string(),
        minted_at: parsed.get("mintedAt").and_then(Value::as_i64).unwrap_or(0),
    })
}

/// `deletePanelGrant`.
pub async fn delete_panel_grant(env: &Env, code: &str) {
    let k = code.to_lowercase();
    if k.is_empty() {
        return;
    }
    kv_delete(env, &format!("panelgrant:{k}")).await;
}

/* ─────────────────────────── plugin links ─────────────────────────── */

/// `listPluginLinks` — the cached read.
async fn list_plugin_links(env: &Env) -> Map<String, Value> {
    let now = now_ms();
    if let Some(hit) = crate::store::cached_get(PLUGIN_KEY, now) {
        if let Ok(Value::Object(map)) = serde_json::from_value::<Value>(hit) {
            return map;
        }
    }
    let map = plugin_links_from_raw(kv_text(env, PLUGIN_KEY).await.as_deref()).unwrap_or_default();
    crate::store::cache_put(PLUGIN_KEY, Value::Object(map.clone()), now);
    map
}

/// `readFreshPluginLinks` — a FRESH read inside the caller's lock, and `None` when the blob is corrupt so the
/// caller aborts rather than writing `{}` over every link.
async fn read_plugin_links_fresh(env: &Env) -> Option<Map<String, Value>> {
    plugin_links_from_raw(kv_text(env, PLUGIN_KEY).await.as_deref())
}

/// `savePluginLinks` — put plus write-through.
async fn save_plugin_links(env: &Env, map: &Map<String, Value>) {
    let text = serde_json::to_string(&Value::Object(map.clone())).unwrap_or_else(|_| "{}".into());
    kv_put(env, PLUGIN_KEY, &text, None).await;
}

/// `getPluginByToken` — the cached lookup, plus the one-time expiry sweep INSIDE THE LOCK against a fresh read
/// (the source's "Extension audit L3": the unlocked sweep could resurrect links another isolate revoked).
pub async fn get_plugin_by_token(env: &Env, token: &str) -> Option<Value> {
    let map = list_plugin_links(env).await;
    let link = map.get(token)?;
    if !device_registry::plugin_link_expired(link, now_ms()) {
        return Some(link.clone());
    }
    let _guard = PLUGIN_LOCK.lock().await;
    let mut fresh = read_plugin_links_fresh(env).await?;
    if let Some(candidate) = fresh.get(token) {
        if device_registry::plugin_link_expired(candidate, now_ms()) {
            // `delete map[token]`, order-preserving — see `map_without`.
            fresh = device_registry::map_without(&fresh, &[token.to_string()]);
            save_plugin_links(env, &fresh).await;
        }
    }
    None
}

/// `migratePluginLinks(env, oldName, newName)` — the rename's second write.
pub async fn migrate_plugin_links(env: &Env, old_name: &str, new_name: &str) -> bool {
    let _guard = PLUGIN_LOCK.lock().await;
    let Some(mut fresh) = read_plugin_links_fresh(env).await else {
        return false;
    };
    if device_registry::migrate_links(&mut fresh, old_name, new_name) {
        save_plugin_links(env, &fresh).await;
        return true;
    }
    false
}

/// `removePluginLinksForDevice(env, device)` — the delete path's revocation.
pub async fn remove_plugin_links_for_device(env: &Env, device: &str) -> usize {
    let _guard = PLUGIN_LOCK.lock().await;
    let Some(mut fresh) = read_plugin_links_fresh(env).await else {
        return 0;
    };
    let removed = device_registry::remove_links_for_device(&mut fresh, device);
    if removed > 0 {
        save_plugin_links(env, &fresh).await;
    }
    removed
}

/* ─────────────────────────── the rest of the gate's reads ─────────────────────────── */

/// `getCfToken` — the account-level Cloudflare API token `/api/install/tunnel-token` hands back.
pub async fn cf_token(env: &Env) -> String {
    kv_text_cached(env, "cf:api_token")
        .await
        .unwrap_or_default()
}

/// `sess-revoked:<cookie>` — the logout blacklist read, with the source's 60 s negative cache and its 512 bound.
pub async fn session_revoked(env: &Env, cookie: &str) -> bool {
    let now = now_ms();
    // **THE CACHE IS CONSULTED, RELEASED, AND ONLY THEN IS KV READ** — a `MutexGuard` held across the await
    // below would block every other request on this isolate for the duration of a KV round trip (the runtime is
    // single-threaded, so `std::sync::Mutex` there is not a wait, it is a deadlock). Clippy's
    // `await_holding_lock` is what caught the first version of this function.
    if let Ok(cache) = NOT_REVOKED.lock() {
        if let Some(hit) = cache.iter().find(|(c, _, _)| c == cookie) {
            if hit.2 > now {
                return hit.1;
            }
        }
    }
    let revoked = kv_text(env, &format!("sess-revoked:{cookie}"))
        .await
        .as_deref()
        == Some("1");
    if let Ok(mut cache) = NOT_REVOKED.lock() {
        if cache.len() >= 512 {
            cache.clear();
        }
        match cache.iter_mut().find(|(c, _, _)| c == cookie) {
            Some(entry) => *entry = (cookie.to_string(), revoked, now + 60_000),
            None => cache.push((cookie.to_string(), revoked, now + 60_000)),
        }
    }
    revoked
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **`js_spread` IS `{...base, k: v}`, AND THE ONE THING IT MUST GET RIGHT IS POSITION.** A key that already
    /// exists keeps its place (so a renamed record's JSON is the same shape as the one it replaced), and a new
    /// key is appended — which is where the round-106 repaired `proxySecret` lands.
    #[test]
    fn the_spread_keeps_positions() {
        let base: Map<String, Value> = serde_json::from_str(
            "{\"name\":\"d1\",\"hostname\":\"h\",\"token\":\"t\",\"registeredAt\":7}",
        )
        .unwrap();
        let merged = crate::device_registry::js_spread(
            &base,
            vec![
                ("name".to_string(), Value::String("d9".into())),
                ("proxySecret".to_string(), Value::String("s".into())),
            ],
        );
        assert_eq!(
            serde_json::to_string(&Value::Object(merged)).unwrap(),
            "{\"name\":\"d9\",\"hostname\":\"h\",\"token\":\"t\",\"registeredAt\":7,\"proxySecret\":\"s\"}"
        );
    }
}
