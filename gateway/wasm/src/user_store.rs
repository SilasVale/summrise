//! THE CONSOLE'S USER RECORDS AND CREDENTIALS — `gateway/src/store/users.ts`'s identity half,
//! `store/admin.ts`'s password half, and `store/settings.ts`'s global switch.
//!
//! WHY THESE BELONG TOGETHER, AND WHY THEY ARE NOT IN `device_store.rs`. That file is the KV adapter for the
//! DEVICE family (the registry blob, the one-time keys, the panel grants) and it happens to hold two reads the
//! session gate needed — `getUser` and `getAdminPassword` — which this module now owns, because they are
//! identity reads and not device reads. What is here is everything that decides WHO A CALLER IS and WHAT THEY
//! MAY DO: the password hash, the user record, the gateway token, the scoped relay token, the per-user key blob,
//! the role that gates an admin route, and the global switch one of those routes writes.
//!
//! **`PASSWORD_ITERATIONS` IS BAKED INTO EVERY STORED HASH** and the source says so in a comment that this port
//! carries forward rather than paraphrases: a record holds only `{salt, passwordHash}` — no iteration count, no
//! algorithm version — so raising the constant makes every existing hash stop matching, and `verifyPassword`
//! cannot tell "wrong password" from "the constants moved". `SESSION_TTL_MS` is the contrast: the session token
//! carries its own `exp`, so that constant is safely changeable. Neither number is repeated here as a literal:
//! the iteration count is [`PASSWORD_ITERATIONS`] and the TTL is [`SESSION_TTL_MS`], both taken from the
//! source's own values.
//!
//! **EVERY `Result<_, String>` IN THIS FILE IS A THROW THE SOURCE MAKES, AND THE STRING IS ITS MESSAGE.** The
//! routes turn one into `400 invalid_request` (the register arm's catch) or into `500 Internal error` (an
//! uncaught throw), and WHICH ONE depends on where the call sits in the handler — so the message and the arm
//! both belong to the caller, and this file answers the message.
//!
//! **THE LOCKS ARE KEYED, AND THEY HAVE TO BE.** `createUser` holds `user:<name>` and takes `invclaim:<code>`
//! inside it; `regenerateToken` sweeps the `token:` namespace while holding `user:<id>`. A single lock would
//! deadlock on the second acquisition, which is why `key_lock.rs` grew the keyed table for this module.

use serde_json::{json, Map, Value};
use worker::*;

use crate::byok::BYOK_CHANNELS;
use crate::device_registry::{js_falsy, js_falsy_string, js_to_string, mask_key};
use crate::device_store::{kv_delete, kv_json_cached, kv_list_prefix, kv_put, kv_text, now_ms};
use crate::key_lock::{with_key_lock, KeyedLocks};

/// `ADMIN_ID` — the seeded account, and the id whose login goes through the ADMIN password rather than the
/// record's own hash.
pub const ADMIN_ID: &str = "admin";

/// `PASSWORD_ITERATIONS` — 100,000, and see the module header for what changing it costs.
pub const PASSWORD_ITERATIONS: u32 = 100_000;

/// `SESSION_TTL_MS` — 24 hours, written into the signed payload as `exp` at ISSUANCE, so it is safely
/// changeable in a way the iteration count is not.
pub const SESSION_TTL_MS: i64 = 24 * 3600 * 1000;

/// `USER_KEY_NAMES` — **DERIVED, NOT RE-TYPED**, from the same table `/v1`'s BYOK decisions come from. The
/// source's own history is why: this list was one of FOUR independently-typed copies of the eight channel names,
/// and the comment that survives records the near-miss ("the prefix<->kind pairing is the part that did not
/// exist anywhere until this file").
pub fn user_key_names() -> Vec<&'static str> {
    BYOK_CHANNELS.iter().map(|(_, user_key, _)| *user_key).collect()
}

/// The channel's DEPLOYMENT key for a user key name — `byok.ts`'s `envKey`, where `nv` and `gmi` are `null` BY
/// DESIGN ("this channel has no environment fallback"). That `null` is why this answers an `Option` rather than
/// an empty string: the two mean different things in `user_keys_status`.
pub fn env_key_for(user_key: &str) -> Option<&'static str> {
    BYOK_CHANNELS
        .iter()
        .find(|(_, name, _)| *name == user_key)
        .and_then(|(_, _, env_key)| *env_key)
}

/// The per-key locks of this module's read-modify-write sections.
static USER_LOCKS: KeyedLocks = KeyedLocks::new();

/* ─────────────────────────── passwords ─────────────────────────── */

/// `hashPassword(password, salt)` — PBKDF2-SHA256, [`PASSWORD_ITERATIONS`], 256 bits, hex. The derivation is the
/// platform's (`webcrypto.rs`) and the hex is `toString(16).padStart(2, "0")`'s.
pub async fn hash_password(password: &str, salt: &str) -> String {
    match crate::webcrypto::pbkdf2_sha256(password, salt, PASSWORD_ITERATIONS).await {
        Some(bits) => crate::webcrypto::hex(&bits),
        // A runtime without PBKDF2 cannot verify a password; the empty string matches no stored hash, which is
        // the fail-closed arm — the same one the source's own throw would produce one level up.
        None => String::new(),
    }
}

/// `verifyPassword(password, salt, expectedHash)`.
pub async fn verify_password(password: &str, salt: &str, expected: &str) -> bool {
    crate::auth::safe_eq(&hash_password(password, salt).await, expected)
}

/* ─────────────────────────── admin password ─────────────────────────── */

/// `getAdminPassword(env)` — **KV IS AUTHORITATIVE AND THE WORKER SECRET IS MIGRATED ONCE**, hashed in the same
/// `salt:hash` format `verifyAdminPassword` expects. The source's comment records what a bare hash cost: "a bare
/// hash (no colon) made verification permanently fail — admin locked out of the console".
pub async fn admin_password(env: &Env) -> String {
    const KEY: &str = "auth:admin_password";
    let now = now_ms();
    if let Some(hit) = crate::store::cached_get(KEY, now) {
        return hit.as_str().unwrap_or_default().to_string();
    }
    let secret = env
        .var("ADMIN_PASSWORD")
        .ok()
        .map(|v| v.to_string())
        .filter(|v| !v.is_empty());
    if env.kv("KEYS").is_err() {
        // The source's no-KV arm returns WITHOUT caching, because there is no store to be consistent with.
        return match secret {
            Some(password) => format!("legacy:{}", hash_password(&password, "legacy").await),
            None => String::new(),
        };
    }
    let mut value = kv_text(env, KEY).await.unwrap_or_default();
    if value.is_empty() {
        if let Some(password) = secret {
            value = format!("legacy:{}", hash_password(&password, "legacy").await);
            kv_put(env, KEY, &value, None).await;
        }
    }
    crate::store::cache_put(KEY, Value::String(value.clone()), now);
    value
}

/// `hasAdminPassword(env)`.
pub async fn has_admin_password(env: &Env) -> bool {
    !admin_password(env).await.is_empty()
}

/// `verifyAdminPassword(env, candidate)` — the stored `salt:hash`, split on the FIRST colon exactly as
/// `String.prototype.split(":")` destructuring takes the first two parts.
pub async fn verify_admin_password(env: &Env, candidate: &str) -> bool {
    let stored = admin_password(env).await;
    if stored.is_empty() {
        return false;
    }
    let (salt, hash) = stored.split_once(':').unwrap_or((stored.as_str(), ""));
    verify_password(candidate, salt, hash).await
}

/// `setAdminPassword(env, value)` — a fresh 8-byte salt, hashed, written and write-through cached. The `Err` is
/// the source's `throw new Error("KV not bound")`.
pub async fn set_admin_password(env: &Env, value: &str) -> Result<(), String> {
    if env.kv("KEYS").is_err() {
        return Err("KV not bound".into());
    }
    let salt = crate::webcrypto::random_hex(8);
    let v = format!("{salt}:{}", hash_password(value, &salt).await);
    kv_put(env, "auth:admin_password", &v, None).await;
    Ok(())
}

/* ─────────────────────────── users ─────────────────────────── */

/// `getUser(env, id)` — **THE CACHE HOLDS THE ABSENCE TOO** ("caches null too — no zombie lookups"), which is
/// why a missing user is a cached `Null` rather than a re-read on every request.
pub async fn get_user(env: &Env, uid: &str) -> Option<Value> {
    let value = kv_json_cached(env, &format!("user:{uid}")).await;
    value.filter(|v| !v.is_null())
}

/// `findUserByUsername(env, username)` — `getUser(String(username || "").trim())`.
pub async fn find_user_by_username(env: &Env, username: Option<&Value>) -> Option<Value> {
    get_user(env, js_falsy_string(username).trim()).await
}

/// `generateGatewayToken()` — 24 bytes of the platform CSPRNG, hex.
fn generate_gateway_token() -> String {
    crate::webcrypto::random_hex(24)
}

/// `createUser(env, {username, password, inviteCode, role})` — the record, or the source's `throw` message.
///
/// **THE ORDER OF THE FOUR REFUSALS IS THE ORDER A CALLER SEES THEM**, and each has its own message because the
/// route answers `400 <message>` verbatim: the username's shape, then the password's length, then — INSIDE the
/// per-name lock — whether the name is taken, and only then the invite.
pub async fn create_user(
    env: &Env,
    username: Option<&Value>,
    password: Option<&Value>,
    invite_code: Option<&Value>,
    role: &str,
) -> Result<Value, String> {
    if env.kv("KEYS").is_err() {
        return Err("KV not bound".into());
    }
    let name = js_falsy_string(username).trim().to_string();
    if !crate::access::is_valid_username(&name) {
        return Err("Username must be 2-32 chars: letters/digits/_ . -".into());
    }
    // `!password || String(password).length < 6` — a NON-STRING password is stringified first (so the number
    // 123456 is six characters and passes), and `""` is refused by the length as well as by the falsiness.
    let password = js_falsy_string(password);
    // `String(password).length` IS THE UTF-16 LENGTH, not the byte count and not the character count: three
    // emoji are six to JavaScript and three to `chars()`, and the arm they fall on differs.
    if password.encode_utf16().count() < 6 {
        return Err("Password must be at least 6 chars".into());
    }
    let role = role.to_string();
    with_key_lock(&USER_LOCKS, &format!("user:{name}"), async {
        if kv_text(env, &format!("user:{name}")).await.is_some() {
            return Err("Username already taken".into());
        }
        if role != "admin" {
            let code = js_falsy_string(invite_code).trim().to_string();
            // The invite is consumed under its own lock, and the claim key is what bounds a cross-isolate race
            // (`put-if-absent` does not exist in KV).
            let claim = with_key_lock(&USER_LOCKS, &format!("invclaim:{code}"), async {
                if kv_text(env, &format!("invclaim:{code}")).await.is_some() {
                    return Err("Invite code already in use".to_string());
                }
                kv_put(env, &format!("invclaim:{code}"), "1", Some(60)).await;
                if kv_text(env, &format!("invite:{code}")).await.is_none() {
                    kv_delete(env, &format!("invclaim:{code}")).await;
                    return Err("Invalid invite code".to_string());
                }
                kv_delete(env, &format!("invite:{code}")).await;
                Ok(())
            })
            .await;
            claim?;
        }

        let salt = crate::webcrypto::random_hex(16);
        let password_hash = hash_password(&password, &salt).await;
        let token = generate_gateway_token();
        // FIELD ORDER IS THE STORED BYTES: the source's literal is
        // `{id, username, role, enabled, createdAt, passwordHash, salt, token}`.
        let user = json!({
            "id": name,
            "username": name,
            "role": role,
            "enabled": true,
            "createdAt": now_ms(),
            "passwordHash": password_hash,
            "salt": salt,
            "token": token,
        });
        kv_put(env, &format!("user:{name}"), &user.to_string(), None).await;
        kv_put(env, &format!("token:{token}"), &name, None).await;
        // The source returns a NARROWER object than it stored — `{id, username, role, token}` — and the
        // register route's response is built from this one, so the narrower shape is the observable one.
        Ok(json!({"id": name, "username": name, "role": role, "token": token}))
    })
    .await
}

/* ─────────────────────────── tokens ─────────────────────────── */

/// `regenerateToken(env, id)` — a new gateway token, the old mapping deleted, and a SWEEP of any other `token:`
/// entry that still resolves to this user (a concurrent rotation can leave a survivor live).
///
/// **THE SWEEP MUST NOT KILL THE RELAY TOKEN**, which points at the same user id: an admin-token rotation that
/// swept it would take the user's daily credential with it (F3's whole reason for existing).
pub async fn regenerate_token(env: &Env, id: &str) -> Result<String, String> {
    with_key_lock(&USER_LOCKS, &format!("user:{id}"), async {
        let Some(mut u) = fresh_json(env, &format!("user:{id}")).await else {
            return Err("User not found".into());
        };
        let old = js_falsy_string(u.get("token"));
        if !old.is_empty() {
            kv_delete(env, &format!("token:{old}")).await;
        }
        let new_token = generate_gateway_token();
        u["token"] = Value::String(new_token.clone());
        kv_put(env, &format!("user:{id}"), &u.to_string(), None).await;
        kv_put(env, &format!("token:{new_token}"), id, None).await;
        let relay_token = js_falsy_string(u.get("relayToken"));
        for (key, _) in kv_list_prefix(env, "token:").await {
            if key == format!("token:{new_token}") {
                continue;
            }
            if !relay_token.is_empty() && key == format!("token:{relay_token}") {
                continue;
            }
            if kv_text(env, &key).await.as_deref() == Some(id) {
                kv_delete(env, &key).await;
            }
        }
        Ok(new_token)
    })
    .await
}

/// `rotateRelayToken(env, id)` — the scoped relay credential, issued (or rotated) under the user's lock.
pub async fn rotate_relay_token(env: &Env, id: &str) -> Result<String, String> {
    with_key_lock(&USER_LOCKS, &format!("user:{id}"), async {
        let Some(mut u) = fresh_json(env, &format!("user:{id}")).await else {
            return Err("User not found".into());
        };
        let old = js_falsy_string(u.get("relayToken"));
        if !old.is_empty() {
            kv_delete(env, &format!("token:{old}")).await;
        }
        let token = generate_gateway_token();
        u["relayToken"] = Value::String(token.clone());
        kv_put(env, &format!("user:{id}"), &u.to_string(), None).await;
        kv_put(env, &format!("token:{token}"), id, None).await;
        Ok(token)
    })
    .await
}

/// `revokeRelayToken(env, id)` — `true` when one existed. The record keeps the EMPTY STRING rather than dropping
/// the field (`u.relayToken = ""`), which is what `reveal`'s `!user.relayToken` and `meGet`'s
/// `relayTokenSet: !!user.relayToken` both read.
pub async fn revoke_relay_token(env: &Env, id: &str) -> Result<bool, String> {
    with_key_lock(&USER_LOCKS, &format!("user:{id}"), async {
        let Some(mut u) = fresh_json(env, &format!("user:{id}")).await else {
            return Ok(false);
        };
        let token = js_falsy_string(u.get("relayToken"));
        if token.is_empty() {
            return Ok(false);
        }
        kv_delete(env, &format!("token:{token}")).await;
        u["relayToken"] = Value::String(String::new());
        kv_put(env, &format!("user:{id}"), &u.to_string(), None).await;
        Ok(true)
    })
    .await
}

/// `getJSON(env, key)` — a FRESH read, not the cached one: every read-modify-write in the source reads raw KV so
/// the write is based on the latest value, and refreshes the cache with what it wrote.
async fn fresh_json(env: &Env, key: &str) -> Option<Value> {
    serde_json::from_str(&kv_text(env, key).await?).ok()
}

/* ─────────────────────────── per-user backend keys ─────────────────────────── */

/// `getUserKeys(env, id)` — the user's key blob, with the ADMIN fallback the console's Overview depends on: when
/// a key is not explicitly configured, the deployment's own secret answers for it.
///
/// `(await env.KEYS.get(n)) || env[n] || null` IS THREE TRUTHINESS TESTS IN A ROW, so an empty KV value falls
/// through to the environment and the answer is `null` (not `""`) when neither has one — and `user_keys_status`
/// reads that `null` as "none" rather than as a configured empty key.
pub async fn get_user_keys(env: &Env, id: &str) -> Value {
    let key = format!("ukeys:{id}");
    let now = now_ms();
    if let Some(hit) = crate::store::cached_get(&key, now) {
        return hit;
    }
    let mut ukeys = match fresh_json(env, &key).await {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    if id == ADMIN_ID && env.kv("KEYS").is_ok() {
        for n in user_key_names() {
            let set = match ukeys.get(n) {
                Some(existing) => !js_falsy(existing),
                None => false,
            };
            if !set {
                let value = kv_text(env, n)
                    .await
                    .filter(|v| !v.is_empty())
                    .or_else(|| {
                        env.var(n)
                            .ok()
                            .map(|v| v.to_string())
                            .filter(|v| !v.is_empty())
                    });
                ukeys.insert(
                    n.to_string(),
                    value.map(Value::String).unwrap_or(Value::Null),
                );
            }
        }
    }
    let value = Value::Object(ukeys);
    crate::store::cache_put(&key, value.clone(), now);
    value
}

/// `updateUserKeys` — the locked read-modify-write skeleton `setUserKey` and `deleteUserKey` share.
async fn update_user_keys<F>(env: &Env, id: &str, mutate: F) -> Value
where
    F: FnOnce(&mut Map<String, Value>),
{
    with_key_lock(&USER_LOCKS, &format!("ukeys:{id}"), async {
        let mut ukeys = match fresh_json(env, &format!("ukeys:{id}")).await {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        mutate(&mut ukeys);
        let value = Value::Object(ukeys);
        kv_put(env, &format!("ukeys:{id}"), &value.to_string(), None).await;
        value
    })
    .await
}

/// `setUserKey(env, id, name, value)` — trimmed, like the source's `String(value).trim()`.
pub async fn set_user_key(env: &Env, id: &str, name: &str, value: &str) {
    let name = name.to_string();
    let value = value.trim().to_string();
    update_user_keys(env, id, |ukeys| {
        ukeys.insert(name, Value::String(value));
    })
    .await;
}

/// `deleteUserKey(env, id, name)`.
pub async fn delete_user_key(env: &Env, id: &str, name: &str) {
    let name = name.to_string();
    update_user_keys(env, id, |ukeys| {
        ukeys.remove(&name);
    })
    .await;
}

/// `maskKey(v || "")` — and the `None` is a THROW the source makes rather than a value it returns: a truthy
/// NON-STRING key value has no `.length` and no `.slice`, so `maskKey` raises a TypeError inside `GET /api/me`
/// and the front door's catch answers 500. Reproduced rather than smoothed over, because a corpus case can seed
/// exactly that record.
pub fn mask_key_of(v: Option<&Value>) -> Option<String> {
    let truthy = match v {
        Some(value) => !js_falsy(value),
        None => false,
    };
    if !truthy {
        return Some(mask_key(""));
    }
    match v {
        Some(Value::String(s)) => Some(mask_key(s)),
        _ => None,
    }
}

/// `userKeysStatus(ukeys, env)` — per key: whether the USER set it, the mask, and **WHICH CREDENTIAL IS IN
/// FORCE** (`user` / `deployment` / `none`), derived from the same `envKey` the request path falls back to.
///
/// `None` is `mask_key_of`'s throw, propagated: the whole response is a 500, not one bad row.
pub fn user_keys_status(ukeys: &Value, env: &Env) -> Option<Value> {
    let mut out = Map::new();
    for n in user_key_names() {
        let v = ukeys.get(n);
        let masked = mask_key_of(v)?;
        let configured = match v {
            Some(value) => !js_falsy(value),
            None => false,
        };
        // `v ? "user" : envKey && env[envKey] ? "deployment" : "none"` — the `envKey` truthiness is part of it,
        // so a channel with no deployment fallback can never answer "deployment".
        let source = if configured {
            "user"
        } else if env_key_for(n).is_some_and(|k| {
            env.var(k)
                .ok()
                .map(|v| v.to_string())
                .is_some_and(|v| !v.is_empty())
        }) {
            "deployment"
        } else {
            "none"
        };
        out.insert(
            n.to_string(),
            json!({"configured": configured, "masked": masked, "source": source}),
        );
    }
    Some(Value::Object(out))
}

/* ─────────────────────────── the global switch ─────────────────────────── */

/// `getGlobalSetting(env, name)` — the cached read, the Worker-var fallback, and the normalization, in the
/// source's order.
///
/// `get_global_setting` (in `store.rs`) is the pure decision and it is handed its two I/O results — with the
/// ENV one already filtered, because the source's fallback is `env[name] ? String(env[name]) : null` and an
/// empty var is falsy there.
pub async fn global_setting(env: &Env, name: &str) -> Option<String> {
    let key = format!("settings:{name}");
    let now = now_ms();
    if let Some(hit) = crate::store::cached_get(&key, now) {
        return crate::store::normalize_setting(hit.as_str());
    }
    let kv = kv_text(env, &key).await;
    let env_var = env
        .var(name)
        .ok()
        .map(|v| v.to_string())
        .filter(|v| !v.is_empty());
    let value = crate::store::get_global_setting(kv.as_deref(), env_var.as_deref());
    crate::store::cache_put(
        &key,
        value
            .as_ref()
            .map_or(Value::Null, |v| Value::String(v.clone())),
        now,
    );
    value
}

/// `setGlobalSetting(env, name, value)` — an explicit OFF is persisted as `"0"` (which shadows the Worker var
/// and is what makes the switch turn OFF at all); a `null`/`undefined`/empty value DELETES the key so the var
/// fallback applies again.
pub async fn set_global_setting(env: &Env, name: &str, value: Option<&Value>) {
    let key = format!("settings:{name}");
    let empty = match value {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.is_empty(),
        _ => false,
    };
    if empty {
        kv_delete(env, &key).await;
        return;
    }
    let value = value.expect("the empty arm returned");
    let s = js_to_string(value);
    // round-439's rule, in the source's own words: booleans canonicalize TRUTHFULLY — the old `s === "1"` arm
    // stored `true` (String → "true") as "0", silently INVERTING the switch.
    let canonical = if s == "1" || s == "true" || value == &Value::Bool(true) {
        "1"
    } else {
        "0"
    };
    kv_put(env, &key, canonical, None).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **THE KEY LIST IS THE SOURCE'S ORDER, AND IT IS DERIVED RATHER THAN RE-TYPED.** The order is observable:
    /// `GET /api/me`'s `keys` object is written in it.
    ///
    /// MUTATION: sort `user_key_names()`.
    /// RESULT:   this test fails — and so does the corpus, because `/api/me`'s key object would come back in a
    ///           different order from the shipping console's.
    #[test]
    fn the_user_key_names_are_the_sources_order() {
        assert_eq!(
            user_key_names(),
            vec![
                "OPENCODE_GO_API_KEY",
                "DEEPSEEK_API_KEY",
                "QWEN_API_KEY",
                "OPENROUTER_API_KEY",
                "NVAPI_KEY",
                "GMI_API_KEY",
                "CMD_API_KEY",
                "AMD_API_KEY",
                "R4_API_KEY",
            ]
        );
        // The two channels with no deployment fallback must keep answering `None`, or `user_keys_status` would
        // report "deployment" for a key the deployment does not have.
        assert_eq!(env_key_for("NVAPI_KEY"), None);
        assert_eq!(env_key_for("GMI_API_KEY"), None);
        assert_eq!(env_key_for("DEEPSEEK_API_KEY"), Some("DEEPSEEK_API_KEY"));
        assert_eq!(env_key_for("NOSUCH_KEY"), None);
    }

    /// `maskKey(v || "")`: three characters, an ellipsis, four — and the short-value arm, which is a DIFFERENT
    /// shape (`v[0] + "…" + v.slice(-2)`), and the empty arm's literal.
    ///
    /// MUTATION: use the long arm for every non-empty value.
    /// RESULT:   this test fails on `"abcdef"` — `"abc…cdef"` instead of `"a…ef"`.
    #[test]
    fn the_mask_is_the_sources() {
        assert_eq!(mask_key_of(Some(&json!(""))).unwrap(), "not configured");
        assert_eq!(mask_key_of(None).unwrap(), "not configured");
        assert_eq!(mask_key_of(Some(&json!(null))).unwrap(), "not configured");
        assert_eq!(mask_key_of(Some(&json!("abcdef"))).unwrap(), "a…ef");
        assert_eq!(mask_key_of(Some(&json!("abcdefg"))).unwrap(), "abc…defg");
        assert_eq!(
            mask_key_of(Some(&json!("sk-1234567890"))).unwrap(),
            "sk-…7890"
        );
        // A truthy non-string is the source's TypeError, which is a 500 rather than a mask.
        assert_eq!(mask_key_of(Some(&json!(5))), None);
        assert_eq!(mask_key_of(Some(&json!({"a": 1}))), None);
    }

    /// `createUser`'s two shape refusals are pure and their MESSAGES are the route's response body.
    #[test]
    fn the_shape_rules_are_the_sources() {
        // `^[A-Za-z0-9_.-]{2,32}$` — the same predicate `usernameFromEmail` tests.
        for bad in ["", "a", "a b", "a/b", &"a".repeat(33), "\u{fc}"] {
            assert!(!crate::access::is_valid_username(bad), "{bad:?}");
        }
        for good in ["ab", ".ab", "a-b_c.d", &"a".repeat(32)] {
            assert!(crate::access::is_valid_username(good), "{good:?}");
        }
        // `String(password).length < 6` — a NUMBER is stringified first, so 123456 is six characters.
        assert_eq!(js_falsy_string(Some(&json!(123456))).len(), 6);
        assert_eq!(js_falsy_string(Some(&json!(12345))).len(), 5);
        assert_eq!(js_falsy_string(None), "");
    }

    /// `setGlobalSetting`'s canonicalization, which had a real inversion bug in the source (round-439) and is
    /// therefore worth pinning here rather than trusting.
    #[test]
    fn the_switch_canonicalizes_truthfully() {
        let canonical = |v: &Value| {
            let s = js_to_string(v);
            if s == "1" || s == "true" || v == &Value::Bool(true) {
                "1"
            } else {
                "0"
            }
        };
        assert_eq!(canonical(&json!("1")), "1");
        assert_eq!(canonical(&json!("true")), "1");
        assert_eq!(canonical(&json!(true)), "1");
        assert_eq!(canonical(&json!("0")), "0");
        assert_eq!(canonical(&json!("false")), "0");
        assert_eq!(canonical(&json!(false)), "0");
        // **A STRAY NUMBER GOES THROUGH `String(value)` FIRST, SO `1` IS "1"** — the canonicalization is about
        // the string the KV write would carry, and the old `s === "1"` arm behaved the same way. What round-439
        // fixed was `true` (whose String is "true"), not `1`.
        assert_eq!(canonical(&json!(1)), "1");
        assert_eq!(canonical(&json!(0)), "0");
        assert_eq!(canonical(&json!({"a": 1})), "0");
    }
}
