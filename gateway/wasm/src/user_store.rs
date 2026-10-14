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
use crate::device_store::{
    kv_delete, kv_json_cached, kv_list_prefix, kv_put, kv_text, kv_text_cached, now_ms,
};
use crate::key_lock::{with_key_lock, KeyedLocks};

/// `ADMIN_ID` — the seeded account, and the id whose login goes through the ADMIN password rather than the
/// record's own hash. `ADMIN_USERNAME` is the same string: **user ID = username**, which is what makes the KV
/// keys readable (`user:admin`).
pub const ADMIN_ID: &str = "admin";

/// `ADMIN_USERNAME` — the name the seeded account logs in with, and the same value as [`ADMIN_ID`] by design.
pub const ADMIN_USERNAME: &str = "admin";

/// `generateGatewayToken()`'s `randomHex(24)` — 24 bytes, 48 hex characters.
pub const GATEWAY_TOKEN_BYTES: usize = 24;

/// `createUser`'s per-user salt — the source's `randomHex(16)`.
pub const PASSWORD_SALT_BYTES: usize = 16;

/// `createInvite`'s code — `randomHex(5).toUpperCase()`, so ten hex characters. **NO RUST CALLER YET**: the
/// admin route that mints invites has not been ported, and the EMISSION is what reads this today.
pub const INVITE_CODE_BYTES: usize = 5;

/// `createInvite`'s TTL — the source's own reason: *"7-day TTL: an invite shared in chat must not stay valid
/// forever."* Like [`INVITE_CODE_BYTES`], its reader today is the emission.
pub const INVITE_TTL_SECS: u64 = 7 * 24 * 60 * 60;

/// **`user:<id>` AND `token:<token>` — THE TWO KEY FAMILIES THIS STORE OWNS.** `access.rs` writes them too (an
/// Access-provisioned account is a user), so they live HERE and are imported there: a key family spelled in two
/// modules is the class this repository has paid for three times.
pub const USER_KEY_PREFIX: &str = "user:";
pub const TOKEN_KEY_PREFIX: &str = "token:";

/// `u-admin` — the v1 schema's random id, and the marker `token:<t>` held before the migration to "username as
/// KV key".
pub const V1_ADMIN_ID: &str = "u-admin";

/// `auth:admin_password` — where the admin's `salt:hash` lives. KV IS AUTHORITATIVE.
pub const ADMIN_PASSWORD_KEY: &str = "auth:admin_password";

/// `_admin_seeded` — the marker a completed seed writes, and the early return for every later boot.
pub const ADMIN_SEEDED_KEY: &str = "_admin_seeded";

/// `CLIENT_KEY` — the legacy gateway token a deployer may have set as a Worker secret. The seed uses it instead
/// of minting, so an existing deployment's scripts keep working.
pub const CLIENT_KEY: &str = "CLIENT_KEY";

/// The v1 -> v2 schema's three addresses: the random-id admin record, its key blob, and the old username index.
pub const V1_ADMIN_USER_KEY: &str = "user:u-admin";
pub const V1_ADMIN_UKEYS_KEY: &str = "ukeys:u-admin";
pub const V1_USERNAME_KEY: &str = "username:admin";

/// **`legacy` IS THE SALT EVERY MIGRATED HASH USES**, and it is why a bare hash was a lockout: the source's own
/// comment records that `getAdminPassword` returning an un-salted hash made `verifyAdminPassword` fail forever
/// ("admin locked out of the console"). The record format is `salt:hash`, and this is the salt the WORKER SECRET
/// is migrated under.
pub const LEGACY_SALT: &str = "legacy";

/// `setAdminPassword`'s fresh salt — the source's `randomHex(8)`.
pub const ADMIN_PASSWORD_SALT_BYTES: usize = 8;

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
    BYOK_CHANNELS
        .iter()
        .map(|(_, user_key, ..)| *user_key)
        .collect()
}

/// The channel's DEPLOYMENT key for a user key name — `byok.ts`'s `envKey`, where `nv` and `gmi` are `null` BY
/// DESIGN ("this channel has no environment fallback"). That `null` is why this answers an `Option` rather than
/// an empty string: the two mean different things in `user_keys_status`.
pub fn env_key_for(user_key: &str) -> Option<&'static str> {
    BYOK_CHANNELS
        .iter()
        .find(|(_, name, ..)| *name == user_key)
        .and_then(|(_, _, env_key, ..)| *env_key)
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

/// `${salt}:${hash}` — the one stored format, used by the migration, the seed's writes and every verification.
pub fn admin_password_record(salt: &str, hash: &str) -> String {
    format!("{salt}:{hash}")
}

/// The two halves of a stored password, **SPLIT ON THE FIRST COLON EXACTLY AS `String.prototype.split(":")`
/// DESTRUCTURING TAKES THE FIRST TWO PARTS**: a hash containing a colon keeps the rest in the second half, and a
/// record with no colon at all answers `(whole, "")` — which is what the source's `stored.split(":")` gives when
/// the migration never ran.
pub fn admin_password_parts(stored: &str) -> (String, String) {
    match stored.split_once(':') {
        Some((salt, hash)) => (salt.to_string(), hash.to_string()),
        None => (stored.to_string(), String::new()),
    }
}

/// The record the FRESH seed writes. **FIELD ORDER IS THE STORED BYTES**:
/// `id, username, role, enabled, createdAt, token` — and `role` is `"admin"`, which is the only difference from
/// an Access-provisioned account.
pub fn admin_record(username: &str, token: &str, now_ms: i64) -> Value {
    json!({
        "id": username,
        "username": username,
        "role": "admin",
        "enabled": true,
        "createdAt": now_ms,
        "token": token,
    })
}

/// The empty-token backfill's test: **A RECORD THAT EXISTS WITH A FALSY `token`**.
///
/// WHY IT EXISTS (the source's own record): deployments seeded BEFORE the mint fix hold `user:admin` with
/// `token: ""` and `_admin_seeded = "1"`, so the early return locks them out FOREVER — bootstrap and
/// reset-password 403 on `!admin?.token` while register/login 500. Healing is unreachable from a healthy
/// deployment (no valid session could ever have existed with an empty token), so no hijack is possible.
pub fn backfill_needed(existing: Option<&Value>) -> bool {
    match existing {
        Some(record) => {
            !crate::device_registry::js_truthy(record.get("token").unwrap_or(&Value::Null))
        }
        None => false,
    }
}

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

/// The process-once flag — the source's module-level `let seeded`, and the reason `seedAdmin` may be called on
/// every request the console makes without paying for the reads.
static SEEDED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `__resetSeedForTests()` — the test hook, which the source documents as "never called in production".
pub fn reset_seed_for_tests() {
    SEEDED.store(false, std::sync::atomic::Ordering::SeqCst);
}

/// `seedAdmin(env)` — **THE WHOLE SEQUENCE, AND ITS ORDER IS THE PRODUCT**.
///
/// Three migrations live in it, each one a lockout that was fixed by hand once:
///
/// 1. **v1 -> v2, the schema**: a random-id admin (`user:u-admin`, `ukeys:u-admin`, `username:admin`) becomes
///    "username as KV key" (`user:admin`), and a token still mapped to `u-admin` is remapped. It runs inside a
///    `try` because **a failed migration must not block startup** — the old deployment keeps working.
/// 2. **the empty-token backfill**: a deployment seeded BEFORE the mint fix holds `user:admin` with `token: ""`
///    and `_admin_seeded = "1"`, so the early returns below would lock it out forever. [`backfill_needed`]
///    decides; a record with a non-empty token is untouched.
/// 3. **round-394, the fresh seed**: mint ONLY when no `user:admin` record exists, because a v1 upgrade (which
///    has no marker) used to fall through and CLOBBER the just-migrated record — a new random token that orphaned
///    the remapped `token:V1TOK -> admin` mapping (which then still authenticated), and `ukeys:admin` reset to
///    `{}`, wiping the migrated keys. **The migration IS the seed for a v1 upgrade.**
///
/// The `seeded` flag is set BEFORE the work and after the KV check, exactly as the source's `if (!env.KEYS ||
/// seeded) return; seeded = true;` — a second concurrent call in the same isolate must not run it twice.
pub async fn seed_admin(env: &Env) {
    if env.kv("KEYS").is_err() {
        return;
    }
    if SEEDED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }

    // 1. THE v1 -> v2 MIGRATION.
    if let Some(old_raw) = kv_text(env, V1_ADMIN_USER_KEY).await {
        if let Ok(mut admin) = serde_json::from_str::<Value>(&old_raw) {
            if let Some(object) = admin.as_object_mut() {
                object.insert("id".into(), json!(ADMIN_ID));
            }
            let user_key = format!("{USER_KEY_PREFIX}{ADMIN_ID}");
            kv_put(env, &user_key, &admin.to_string(), None).await;
            crate::store::cache_put(&user_key, admin.clone(), now_ms());
            kv_delete(env, V1_ADMIN_USER_KEY).await;
            let token = admin
                .get("token")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !token.is_empty() {
                let token_key = format!("{TOKEN_KEY_PREFIX}{token}");
                if kv_text(env, &token_key).await.as_deref() == Some(V1_ADMIN_ID) {
                    kv_put(env, &token_key, ADMIN_ID, None).await;
                    crate::store::cache_put(&token_key, json!(ADMIN_ID), now_ms());
                }
            }
            if let Some(uks) = kv_text(env, V1_ADMIN_UKEYS_KEY).await {
                let ukeys_key = format!("ukeys:{ADMIN_ID}");
                kv_put(env, &ukeys_key, &uks, None).await;
                // **THE CACHED VALUE IS THE PARSED OBJECT WHILE KV HOLDS THE RAW STRING** — the source's
                // `cset(key, parsed || {})`, whose `|| {}` is what a malformed blob caches as.
                let parsed = serde_json::from_str::<Value>(&uks).unwrap_or_else(|_| json!({}));
                crate::store::cache_put(&ukeys_key, parsed, now_ms());
                kv_delete(env, V1_ADMIN_UKEYS_KEY).await;
            }
            kv_delete(env, V1_USERNAME_KEY).await;
        }
    }

    // 2. THE EMPTY-TOKEN BACKFILL.
    let legacy_token = kv_text(env, CLIENT_KEY)
        .await
        .filter(|v| !v.is_empty())
        .or_else(|| {
            env.var(CLIENT_KEY)
                .ok()
                .map(|v| v.to_string())
                .filter(|v| !v.is_empty())
        })
        .unwrap_or_default();
    let user_key = format!("{USER_KEY_PREFIX}{ADMIN_ID}");
    let existing = kv_text(env, &user_key)
        .await
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());
    if backfill_needed(existing.as_ref()) {
        let mut record = existing.unwrap_or_else(|| json!({}));
        let admin_token = if legacy_token.is_empty() {
            generate_gateway_token()
        } else {
            legacy_token.clone()
        };
        if let Some(object) = record.as_object_mut() {
            object.insert("token".into(), json!(admin_token));
        }
        kv_put(env, &user_key, &record.to_string(), None).await;
        crate::store::cache_put(&user_key, record, now_ms());
        let token_key = format!("{TOKEN_KEY_PREFIX}{admin_token}");
        kv_put(env, &token_key, ADMIN_ID, None).await;
        crate::store::cache_put(&token_key, json!(ADMIN_ID), now_ms());
        kv_put(env, ADMIN_SEEDED_KEY, "1", None).await;
    }

    // 3. THE FRESH SEED — and the two early returns that make an upgrade a no-op.
    if kv_text(env, ADMIN_SEEDED_KEY).await.is_some() {
        return;
    }
    if kv_text(env, &user_key).await.is_some() {
        return;
    }
    let admin_token = if legacy_token.is_empty() {
        generate_gateway_token()
    } else {
        legacy_token
    };
    let admin = admin_record(ADMIN_USERNAME, &admin_token, now_ms());
    kv_put(env, &user_key, &admin.to_string(), None).await;
    crate::store::cache_put(&user_key, admin, now_ms());
    let token_key = format!("{TOKEN_KEY_PREFIX}{admin_token}");
    kv_put(env, &token_key, ADMIN_ID, None).await;
    crate::store::cache_put(&token_key, json!(ADMIN_ID), now_ms());

    // The key blob: a Worker secret per name, whichever the deployer set.
    let mut ukeys = serde_json::Map::new();
    for name in user_key_names() {
        let from_kv = kv_text(env, name).await.unwrap_or_default();
        let value = if from_kv.is_empty() {
            env.var(name)
                .ok()
                .map(|v| v.to_string())
                .unwrap_or_default()
        } else {
            from_kv
        };
        if !value.is_empty() {
            ukeys.insert(name.to_string(), json!(value));
        }
    }
    let ukeys_key = format!("ukeys:{ADMIN_ID}");
    let ukeys = Value::Object(ukeys);
    kv_put(env, &ukeys_key, &ukeys.to_string(), None).await;
    crate::store::cache_put(&ukeys_key, ukeys, now_ms());
    kv_put(env, ADMIN_SEEDED_KEY, "1", None).await;
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

/// `findUserByToken(env, token)` — **THE CONSOLE API TOKEN'S OWNER, AND THE ROLE THAT COMES WITH IT.**
///
/// `token:<token>` is an INDEX into the user namespace (`store/users.ts` writes it beside the record), so this
/// is one cached KV read plus the record read the caller would have made anyway. The absence is CACHED — the
/// source says why in the same breath as `getUser`'s ("caches null too — no zombie lookups"), and
/// `kv_text_cached` writes the `Null` that makes a missing token free on the second ask.
///
/// **THE RELAY TOKEN IS THE SECOND ARM, AND IT IS A ROLE DOWNGRADE RATHER THAN A SECOND USER** (ADR-0007 step
/// 1): a caller presenting a user's scoped relay token resolves to that user with role `"relay"`, so `/v1`'s
/// translate and models routes dual-accept it while `/mcp` (admin-only, below) and the adminKey recovery gates
/// reject it. The record is CLONED for that arm — the source returns a copy "so the cached record's role is
/// never mutated", and mutating it in place would corrupt the isolate's cache for every later caller.
///
/// `safeEq` on both comparisons, and the ORDER is the source's: the admin token is tested first, so the
/// impossible double-match resolves to the real role rather than to `"relay"`.
pub async fn find_user_by_token(env: &Env, token: &str) -> Option<Value> {
    // `if (!token) return null` — the empty token never reaches KV, which is what makes `token:` itself
    // unreadable as a credential.
    if token.is_empty() {
        return None;
    }
    let name = kv_text_cached(env, &format!("token:{token}")).await?;
    // `if (!name) return null` — an index with no owner answers null rather than an error.
    if name.is_empty() {
        return None;
    }
    let user = get_user(env, &name).await?;
    let token_field = user.get("token");
    if token_field.is_some_and(|v| !js_falsy(v) && crate::auth::safe_eq(token, &js_to_string(v))) {
        return Some(user);
    }
    let relay_field = user.get("relayToken");
    if relay_field.is_some_and(|v| !js_falsy(v) && crate::auth::safe_eq(token, &js_to_string(v))) {
        let mut downgraded = user.clone();
        downgraded["role"] = json!("relay");
        return Some(downgraded);
    }
    Some(user)
}

/// `generateGatewayToken()` — [`GATEWAY_TOKEN_BYTES`] bytes of the platform CSPRNG, hex.
fn generate_gateway_token() -> String {
    crate::webcrypto::random_hex(GATEWAY_TOKEN_BYTES)
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

        let salt = crate::webcrypto::random_hex(PASSWORD_SALT_BYTES);
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

/// `setUserEnabled(env, id, enabled)` — **THE LOCK IS THE DECISION**, and the source's comment records what its
/// absence cost (round-122): an unlocked read-modify-write let an enable/disable land with a STALE token value,
/// and a concurrent `regenerateToken`'s delete of `token:T1` then made the record's own `T1` dead — `x-api-key`
/// stops working while the console still shows `T1`. Three writers share this key: create, regenerate, enable.
///
/// The read is `fresh_json` (RAW KV, not the cache) so the write is based on the latest value, and the cache is
/// refreshed with the new object afterwards — the source's own two comments.
pub async fn set_user_enabled(env: &Env, id: &str, enabled: bool) -> Result<Value, String> {
    with_key_lock(&USER_LOCKS, &format!("user:{id}"), async {
        let Some(mut u) = fresh_json(env, &format!("user:{id}")).await else {
            return Err("User not found".into());
        };
        u["enabled"] = Value::Bool(enabled);
        kv_put(env, &format!("user:{id}"), &u.to_string(), None).await;
        Ok(u)
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
                let value = kv_text(env, n).await.filter(|v| !v.is_empty()).or_else(|| {
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

/// The SECOND emitted module: `gateway/src/store/admin-logic.ts`.
///
/// **WHY IT IS A SECOND FILE RATHER THAN MORE OF THE FIRST.** `users-logic.ts` answers "what is a user"; this one
/// answers "what is the ADMIN, and how does a deployment come to have one" — the v1 schema's addresses, the
/// password record's format, and the SEED, whose order is a product decision (round-394: a v1 upgrade that fell
/// through to the fresh seed had its just-migrated record clobbered). Two subjects, two files, one producer.
///
/// **AND IT TAKES THE PLATFORM AS AN ARGUMENT.** Every other emission is either a pure function or a leaf that
/// imports another emission; the seed is a SEQUENCE of KV reads and writes, so it is rendered against a `deps`
/// object the host fills in. That keeps the ORDER — which is the whole decision — in Rust while the four calls
/// stay the host's.
const TS_MODULE_ADMIN: &str = r#"/**
 * admin-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/user_store.rs` (`render_admin_typescript`), which is the single source for the
 * rules below. The freshness test in that module is a byte comparison against this file:
 *
 *     SUMMRISE_REFRESH_ADMIN=1 cargo test --manifest-path gateway/wasm/Cargo.toml user_store::emission
 *
 * `gateway/src/store/admin.ts` re-exports this module and keeps the HOST: the `seeded` process-once flag, the
 * `__resetSeedForTests` hook, the Worker's own `crypto.subtle` PBKDF2, and the KV calls the seed is handed.
 *
 * **THREE MIGRATIONS LIVE IN THE SEED, AND EACH ONE WAS ONCE A LOCKOUT** — the v1 -> v2 schema, the empty-token
 * backfill, and round-394's "mint only when no record exists". Their ORDER is why this file exists.
 */

import { ADMIN_ID, ADMIN_USERNAME, USER_KEY_NAMES } from "./users-logic.ts";

/** `auth:admin_password` — where the admin's `salt:hash` lives. **KV IS AUTHORITATIVE.** */
export const ADMIN_PASSWORD_KEY = "auth:admin_password";

/** `_admin_seeded` — the marker a completed seed writes, and the early return for every later boot. */
export const ADMIN_SEEDED_KEY = "_admin_seeded";

/** `CLIENT_KEY` — the legacy gateway token a deployer may have set as a Worker secret. The seed uses it instead
 *  of minting, so an existing deployment's scripts keep working. */
export const CLIENT_KEY = "CLIENT_KEY";

/** The v1 -> v2 schema's three addresses: the random-id admin record, its key blob, and the old username index. */
export const V1_ADMIN_USER_KEY = "user:u-admin";
export const V1_ADMIN_UKEYS_KEY = "ukeys:u-admin";
export const V1_USERNAME_KEY = "username:admin";

/** `u-admin` — the id `token:<t>` held before the migration to "username as KV key". */
export const V1_ADMIN_ID = "u-admin";

/** **`legacy` IS THE SALT EVERY MIGRATED HASH USES**, and it is why a bare hash was a lockout: a stored hash
 *  without its salt made verification fail forever. */
export const LEGACY_SALT = "legacy";

/** `setAdminPassword`'s fresh salt — the source's `randomHex(8)`. */
export const ADMIN_PASSWORD_SALT_BYTES = 8;

/** `${salt}:${hash}` — the one stored format, used by the migration, the seed's writes and every verification. */
export function adminPasswordRecord(salt: string, hash: string): string {
  return `${salt}:${hash}`;
}

/** The two halves of a stored password, **SPLIT ON THE FIRST COLON EXACTLY AS `String.prototype.split(":")`
 *  DESTRUCTURING TAKES THE FIRST TWO PARTS** — a hash containing a colon keeps the rest in the second half, and
 *  a record with no colon at all answers `(whole, "")`. */
export function adminPasswordParts(stored: string): { salt: string; hash: string } {
  const [salt, hash] = stored.split(":");
  return { salt: salt ?? "", hash: hash ?? "" };
}

/** The record the FRESH seed writes. **FIELD ORDER IS THE STORED BYTES**:
 *  `id, username, role, enabled, createdAt, token` — and `role` is `"admin"`, the only difference from an
 *  Access-provisioned account. */
export function adminRecord(name: string, token: string, nowMs: number): unknown {
  return { id: name, username: name, role: "admin", enabled: true, createdAt: nowMs, token };
}

/** The empty-token backfill's test: **A RECORD THAT EXISTS WITH A FALSY `token`**. A deployment seeded before
 *  the mint fix holds `token: ""` with `_admin_seeded` set, so the early returns would lock it out forever. */
export function backfillNeeded(existing: unknown): boolean {
  return !!existing && !(existing as { token?: unknown }).token;
}

/** The platform the seed runs on. Four KV calls, the cache, the mint, the clock, and the Worker's secrets. */
export interface AdminSeedDeps {
  get(key: string): Promise<string | null>;
  put(key: string, value: string): Promise<void>;
  delete(key: string): Promise<void>;
  /** `cset` — the per-isolate cache. It is handed the PARSED object for a key blob, which is what the source
   *  cached (`cset(key, parsed || {})`). */
  cache(key: string, value: unknown): void;
  /** `generateGatewayToken()`. */
  mint(): string;
  /** `Date.now()`. */
  now(): number;
  /** `env[name]` — the Worker secret a channel key may come from. */
  secret(name: string): unknown;
}

/** **`seedAdmin`'s SEQUENCE, AND ITS ORDER IS THE PRODUCT.** */
export async function seedAdminSequence(deps: AdminSeedDeps): Promise<void> {
  // 1. THE v1 -> v2 MIGRATION. Inside a `try` because **a failed migration must not block startup**.
  const oldRaw = await deps.get(V1_ADMIN_USER_KEY);
  if (oldRaw) {
    try {
      const admin = JSON.parse(oldRaw);
      admin.id = ADMIN_ID;
      await deps.put(`user:${ADMIN_ID}`, JSON.stringify(admin));
      deps.cache(`user:${ADMIN_ID}`, admin);
      await deps.delete(V1_ADMIN_USER_KEY);
      // The token mapping is remapped only when it still points at the v1 id.
      if (admin.token && (await deps.get(`token:${admin.token}`)) === V1_ADMIN_ID) {
        await deps.put(`token:${admin.token}`, ADMIN_ID);
        deps.cache(`token:${admin.token}`, ADMIN_ID);
      }
      const uks = await deps.get(V1_ADMIN_UKEYS_KEY);
      if (uks) {
        await deps.put(`ukeys:${ADMIN_ID}`, uks);
        let parsed = null;
        try {
          parsed = JSON.parse(uks);
        } catch {
          /* malformed JSON */
        }
        deps.cache(`ukeys:${ADMIN_ID}`, parsed || {});
        await deps.delete(V1_ADMIN_UKEYS_KEY);
      }
      await deps.delete(V1_USERNAME_KEY);
    } catch {
      /* a failed migration must not block startup */
    }
  }

  // `String(... || "")` IS the source's `env.CLIENT_KEY || ""` with the type the deps object carries: a secret
  // that is absent, null or falsy contributes nothing, and a non-string one is rendered as itself.
  const legacyToken = (await deps.get(CLIENT_KEY)) || String(deps.secret(CLIENT_KEY) || "");

  // 2. THE EMPTY-TOKEN BACKFILL — heal exactly like fresh: mint and remap. A record with a NON-EMPTY token is
  // left completely untouched.
  try {
    const existingRaw = await deps.get(`user:${ADMIN_ID}`);
    if (existingRaw) {
      const existing = JSON.parse(existingRaw);
      if (backfillNeeded(existing)) {
        const adminToken = legacyToken || deps.mint();
        existing.token = adminToken;
        await deps.put(`user:${ADMIN_ID}`, JSON.stringify(existing));
        deps.cache(`user:${ADMIN_ID}`, existing);
        await deps.put(`token:${adminToken}`, ADMIN_ID);
        deps.cache(`token:${adminToken}`, ADMIN_ID);
        await deps.put(ADMIN_SEEDED_KEY, "1");
      }
    }
  } catch {
    /* a failed backfill must not block startup */
  }

  // 3. THE FRESH SEED, and the two early returns that make an upgrade a no-op. **round-394: mint ONLY when no
  // `user:admin` record exists** — the migration IS the seed for a v1 upgrade.
  if (await deps.get(ADMIN_SEEDED_KEY)) return;
  if (await deps.get(`user:${ADMIN_ID}`)) return;
  const adminToken = legacyToken || deps.mint();
  const admin = adminRecord(ADMIN_USERNAME, adminToken, deps.now());
  await deps.put(`user:${ADMIN_ID}`, JSON.stringify(admin));
  deps.cache(`user:${ADMIN_ID}`, admin);
  await deps.put(`token:${adminToken}`, ADMIN_ID);
  deps.cache(`token:${adminToken}`, ADMIN_ID);

  // The key blob: a Worker secret per name, whichever the deployer set.
  const ukeys: Record<string, string> = {};
  for (const n of USER_KEY_NAMES) {
    const v = (await deps.get(n)) || String(deps.secret(n) || "");
    if (v) ukeys[n] = v;
  }
  await deps.put(`ukeys:${ADMIN_ID}`, JSON.stringify(ukeys));
  deps.cache(`ukeys:${ADMIN_ID}`, ukeys);
  await deps.put(ADMIN_SEEDED_KEY, "1");
}

/** The platform `getAdminPassword` runs on. */
export interface AdminPasswordDeps {
  /** `cget(key)` — the per-isolate cache, which answers `undefined` for a miss. */
  cached(key: string): unknown;
  hasKv: boolean;
  /** `env.ADMIN_PASSWORD` — the Worker secret a fresh deployment may carry. */
  secretValue: unknown;
  get(key: string): Promise<string | null>;
  put(key: string, value: string): Promise<void>;
  cache(key: string, value: string): void;
  /** `hashPassword(password, LEGACY_SALT)`. */
  hash(password: string): Promise<string>;
}

/** `getAdminPassword(env)` — **KV IS AUTHORITATIVE AND THE WORKER SECRET IS MIGRATED ONCE**, hashed in the same
 *  `salt:hash` format verification expects. **A CACHED VALUE WINS EVEN WHEN IT IS THE EMPTY STRING**, and the
 *  no-KV arm returns WITHOUT caching, because there is no store to be consistent with. */
export async function resolveAdminPassword(deps: AdminPasswordDeps): Promise<string> {
  const hit = deps.cached(ADMIN_PASSWORD_KEY);
  if (hit !== undefined) return String(hit);
  const secret = deps.secretValue ? String(deps.secretValue) : "";
  const legacy = async () => adminPasswordRecord(LEGACY_SALT, await deps.hash(secret));
  if (!deps.hasKv) return secret ? await legacy() : "";
  let v = (await deps.get(ADMIN_PASSWORD_KEY)) || "";
  if (!v && secret) {
    // one-time migration from the Worker secret — stored HASHED, because a bare hash (no colon) made
    // verification permanently fail: admin locked out of the console.
    v = await legacy();
    await deps.put(ADMIN_PASSWORD_KEY, v);
  }
  deps.cache(ADMIN_PASSWORD_KEY, v);
  return v;
}
"#;

/// **THE PRODUCER FOR `admin-logic.ts`.**
pub fn render_admin_typescript() -> String {
    TS_MODULE_ADMIN.to_string()
}

/// The committed admin emission, and the path the refresh command writes.
pub fn admin_typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/store/admin-logic.ts")
}

/// The emitted module's header, and then the module.
const TS_MODULE: &str = r#"/**
 * users-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/user_store.rs` (`render_typescript`), which is now the single source for the
 * decisions below. The freshness test in that module is a byte comparison against this file, so a hand edit
 * fails there with the text and one command to fix it:
 *
 *     SUMMRISE_REFRESH_USERS=1 cargo test --manifest-path gateway/wasm/Cargo.toml user_store::emission
 *
 * `gateway/src/store/users.ts` re-exports this module and keeps the HOST: the KV reads and writes, the keyed
 * locks, the record reads, the invites write and the RouteDO calls. **THE DECISIONS ARE HERE** — who the
 * seeded admin is, which key names a user may hold, how long a token is, how many bytes a salt and an invite
 * code take, and the mask and three-state "which credential is in force" answers the console renders.
 */

import { BYOK_CHANNELS } from "./byok.ts";

/** The seeded account's name. `ADMIN_ID` is the same string: **user ID = username**, which is what makes the
 *  KV keys readable (`user:admin`). */
export const ADMIN_USERNAME = "admin";

/** The seeded account's id, and the id whose login goes through the ADMIN password rather than the record's own
 *  hash. */
export const ADMIN_ID = "admin";

/** Every env-var name a USER may hold a key for, DERIVED from the emitted BYOK table since round 193 — this was
 *  the third of four independently-typed copies of the same names. */
export const USER_KEY_NAMES: string[] = BYOK_CHANNELS.map((c) => c.userKey);

/** `generateGatewayToken()`'s `randomHex(24)` — bytes, so 48 hex characters. */
export const GATEWAY_TOKEN_BYTES = 24;

/** `createUser`'s per-user salt — the source's `randomHex(16)`. */
export const PASSWORD_SALT_BYTES = 16;

/** `createInvite`'s code — `randomHex(5).toUpperCase()`, so ten hex characters. */
export const INVITE_CODE_BYTES = 5;

/** `createInvite`'s TTL: *"7-day TTL: an invite shared in chat must not stay valid forever."* */
export const INVITE_TTL_SECS = 7 * 24 * 60 * 60;

/** `maskKey(v)` — the console's only view of a device token or a user's key. **CODE-UNIT-WISE, because that is
 *  what `v.length` and `v.slice` count**: an astral character is two units, so `"abcd😀"` takes the SHORT arm
 *  on both sides. (`v[0]` on an empty string is `undefined`, which is why the `!v` arm comes first.) */
export function maskKey(v: string): string {
  if (!v) return "not configured";
  if (v.length <= 6) return v[0] + "…" + v.slice(-2);
  return v.slice(0, 3) + "…" + v.slice(-4);
}

/** Per-key configured/masked status for the console (shared by the auth plugin's /api/me and the admin plugin's
 *  user list — was copy-pasted in both with subtly different types).
 *
 *  AND WHICH CREDENTIAL IS IN FORCE: `configured` answers "has this user stored this key"; it does NOT answer
 *  "will a request on this channel be served, and on whose credential" — and since the request path falls back
 *  to the channel's Worker secret (`bearerKeyFor`), those are different questions.
 *
 *      user        this user's own key is set — it wins, the deployment is not charged
 *      deployment  no user key, but the channel declares an envKey and the worker has it
 *      none        neither — including nv/gmi, whose envKey is null BY DESIGN, so a stray secret of the same
 *                  name is not a fallback here
 *
 *  Pure: `env` is passed in, never read from a global, so the three states are directly testable and the
 *  console cannot invent a fourth. */
export function userKeysStatus(
  ukeys: Record<string, any>,
  env: Record<string, any> = {},
): Record<string, { configured: boolean; masked: string; source: "user" | "deployment" | "none" }> {
  const out: Record<
    string,
    { configured: boolean; masked: string; source: "user" | "deployment" | "none" }
  > = {};
  for (const n of USER_KEY_NAMES) {
    const v = ukeys?.[n];
    const channel = BYOK_CHANNELS.find((c) => c.userKey === n);
    const envKey = channel?.envKey ?? null;
    const source: "user" | "deployment" | "none" = v
      ? "user"
      : envKey && env[envKey]
        ? "deployment"
        : "none";
    out[n] = { configured: !!v, masked: maskKey(v || ""), source };
  }
  return out;
}
"#;

/// **THE PRODUCER.** The emitted module is exactly this string.
pub fn render_typescript() -> String {
    TS_MODULE.to_string()
}

/// The committed emission, and the path the refresh command writes.
pub fn typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/store/users-logic.ts")
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
        // **THE ARM BOUNDARY IS COUNTED IN UTF-16 CODE UNITS, WHICH IS WHERE THIS PORT WAS WRONG.** `"abcd😀"`
        // is FIVE characters and SIX units: the source answers `a…😀` (the short arm), and a port that
        // collected `chars()` answered the long one — a different shape for the same key.
        //
        // MUTATION: collect `chars()` instead of `encode_utf16()`.
        // RESULT:   this assertion fails with `abcd…abcd😀` — and every ASCII row above still passes, which is
        //           exactly why the divergence survived until the emission made the two sides comparable.
        assert_eq!(mask_key_of(Some(&json!("abcd😀"))).unwrap(), "a…😀");
        assert_eq!(mask_key_of(Some(&json!("abcdefgh😀"))).unwrap(), "abc…gh😀");
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

    /// **THE SEED'S THREE MIGRATIONS, AS THE PURE RULES THEY REST ON.**
    ///
    /// MUTATION: make `backfill_needed` answer `false` for a record whose `token` is the empty string.
    /// RESULT:   this test fails — and a deployment seeded before the mint fix stays locked out forever, which is
    ///           the defect the backfill exists for (`bootstrap` and `reset-password` 403 on `!admin?.token`, and
    ///           `register`/`login` 500 with "Admin password not configured").
    #[test]
    fn the_admin_record_rules_are_the_sources() {
        // `${salt}:${hash}`, and the SPLIT IS THE FIRST COLON — a hash containing one keeps the rest.
        assert_eq!(admin_password_record("legacy", "ab"), "legacy:ab");
        assert_eq!(
            admin_password_parts("legacy:ab:cd"),
            ("legacy".to_string(), "ab:cd".to_string())
        );
        // A record with NO colon answers (whole, "") — the source's `stored.split(":")` destructuring.
        assert_eq!(
            admin_password_parts("nocolon"),
            ("nocolon".to_string(), String::new())
        );
        assert_eq!(admin_password_parts(""), (String::new(), String::new()));
        // The fresh record's shape, field for field.
        assert_eq!(
            admin_record("admin", "tok", 7),
            json!({"id": "admin", "username": "admin", "role": "admin", "enabled": true, "createdAt": 7, "token": "tok"})
        );
        // The backfill's test: a record that EXISTS with a FALSY token — and only that.
        assert!(backfill_needed(Some(&json!({"id": "admin", "token": ""}))));
        assert!(backfill_needed(Some(&json!({"id": "admin"}))));
        assert!(backfill_needed(Some(
            &json!({"id": "admin", "token": null})
        )));
        assert!(!backfill_needed(Some(
            &json!({"id": "admin", "token": "t"})
        )));
        assert!(
            !backfill_needed(None),
            "no record is the FRESH seed's arm, not the backfill's"
        );
        // The v1 addresses are the schema's own strings, and the token marker is the v1 id.
        assert_eq!(V1_ADMIN_USER_KEY, "user:u-admin");
        assert_eq!(V1_ADMIN_UKEYS_KEY, "ukeys:u-admin");
        assert_eq!(V1_USERNAME_KEY, "username:admin");
        assert_eq!(V1_ADMIN_ID, "u-admin");
        assert_eq!(LEGACY_SALT, "legacy");
        assert_eq!(ADMIN_PASSWORD_SALT_BYTES, 8);
        // The key families, from their owner.
        assert_eq!(USER_KEY_PREFIX, "user:");
        assert_eq!(TOKEN_KEY_PREFIX, "token:");
    }
}

#[cfg(test)]
mod emission {
    use super::*;

    /// **THE FRESHNESS CHECK — AND IT IS THE REASON THE EMISSION CANNOT DRIFT.** The same shape `mcp_tools.rs`,
    /// `settings_logic.rs` and `provider_spec.rs` use: the committed file IS `render_typescript()`'s bytes.
    #[test]
    fn the_typescript_is_what_this_module_emits() {
        let path = typescript_path();
        let rendered = render_typescript();
        if std::env::var("SUMMRISE_REFRESH_USERS").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write users-logic.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_USERS=1 cargo test user_store::emission",
                path.display()
            )
        });
        assert_eq!(
            committed, rendered,
            "gateway/src/store/users-logic.ts is not what user_store.rs emits — \
             run SUMMRISE_REFRESH_USERS=1 cargo test user_store::emission"
        );
    }

    /// **THE EMISSION MUST CARRY THE RULES, NOT A SUMMARY OF THEM.** A floor on the TEXT: it fails when a rule
    /// is dropped from the renderer rather than when a byte moves.
    ///
    /// MUTATION: drop the `envKey && env[envKey]` arm from the emitted `userKeysStatus`.
    /// RESULT:   this test fails naming the arm — and the console would report `none` for a channel whose
    ///           deployment key IS configured, which is the 502 the three states exist to explain.
    #[test]
    fn the_emission_carries_every_rule() {
        let ts = render_typescript();
        for rule in [
            "export const ADMIN_USERNAME = \"admin\";",
            "export const ADMIN_ID = \"admin\";",
            "BYOK_CHANNELS.map((c) => c.userKey)",
            "export const GATEWAY_TOKEN_BYTES = 24;",
            "export const PASSWORD_SALT_BYTES = 16;",
            "export const INVITE_CODE_BYTES = 5;",
            "export const INVITE_TTL_SECS = 7 * 24 * 60 * 60;",
            "if (!v) return \"not configured\";",
            "if (v.length <= 6) return v[0] + \"…\" + v.slice(-2);",
            "return v.slice(0, 3) + \"…\" + v.slice(-4);",
            "envKey && env[envKey]",
            "masked: maskKey(v || \"\")",
        ] {
            assert!(ts.contains(rule), "the emission lost: {rule}");
        }
    }

    /// **THE ADMIN EMISSION'S FRESHNESS CHECK** — a second module, one producer.
    #[test]
    fn the_admin_typescript_is_what_this_module_emits() {
        let path = admin_typescript_path();
        let rendered = render_admin_typescript();
        if std::env::var("SUMMRISE_REFRESH_ADMIN").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write admin-logic.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_ADMIN=1 cargo test user_store::emission",
                path.display()
            )
        });
        assert_eq!(
            committed, rendered,
            "gateway/src/store/admin-logic.ts is not what user_store.rs emits — \
             run SUMMRISE_REFRESH_ADMIN=1 cargo test user_store::emission"
        );
    }

    /// **THE SEED'S THREE MIGRATIONS ARE IN THE TEXT, IN ORDER, WITH THE GUARDS THAT MAKE EACH ONE SAFE.**
    ///
    /// MUTATION: move the `user:${ADMIN_ID}` early return above the empty-token backfill.
    /// RESULT:   this test fails on the order assertion — and a deployment seeded with an empty token would stay
    ///           locked out forever, which is the defect the backfill was written for.
    #[test]
    fn the_admin_emission_carries_the_seed_in_order() {
        let ts = render_admin_typescript();
        for item in [
            format!("export const ADMIN_PASSWORD_KEY = \"{ADMIN_PASSWORD_KEY}\";"),
            format!("export const ADMIN_SEEDED_KEY = \"{ADMIN_SEEDED_KEY}\";"),
            format!("export const CLIENT_KEY = \"{CLIENT_KEY}\";"),
            format!("export const V1_ADMIN_USER_KEY = \"{V1_ADMIN_USER_KEY}\";"),
            format!("export const V1_ADMIN_UKEYS_KEY = \"{V1_ADMIN_UKEYS_KEY}\";"),
            format!("export const V1_USERNAME_KEY = \"{V1_USERNAME_KEY}\";"),
            format!("export const V1_ADMIN_ID = \"{V1_ADMIN_ID}\";"),
            format!("export const LEGACY_SALT = \"{LEGACY_SALT}\";"),
            format!("export const ADMIN_PASSWORD_SALT_BYTES = {ADMIN_PASSWORD_SALT_BYTES};"),
            "export function adminPasswordRecord(".to_string(),
            "export function adminPasswordParts(".to_string(),
            "export function adminRecord(".to_string(),
            "export function backfillNeeded(".to_string(),
            "export async function seedAdminSequence(".to_string(),
            "export async function resolveAdminPassword(".to_string(),
            "import { ADMIN_ID, ADMIN_USERNAME, USER_KEY_NAMES } from \"./users-logic.ts\";"
                .to_string(),
        ] {
            assert!(ts.contains(&item), "the emission lost: {item}");
        }
        // THE ORDER: migration, then backfill, then the marker return, then the record return, then the seed.
        let at = |needle: &str| {
            ts.find(needle)
                .unwrap_or_else(|| panic!("missing {needle}"))
        };
        let migration = at("const oldRaw = await deps.get(V1_ADMIN_USER_KEY);");
        let backfill = at("2. THE EMPTY-TOKEN BACKFILL");
        let marker = at("if (await deps.get(ADMIN_SEEDED_KEY)) return;");
        let existing = at("if (await deps.get(`user:${ADMIN_ID}`)) return;");
        let seed = at("const admin = adminRecord(ADMIN_USERNAME, adminToken, deps.now());");
        assert!(
            migration < backfill && backfill < marker && marker < existing && existing < seed,
            "the seed's order is the product: migration -> backfill -> marker -> record -> fresh seed"
        );
        // And the legacy fallback is the SHARED name, not a second spelling of it.
        assert!(
            ts.contains(r#"(await deps.get(CLIENT_KEY)) || String(deps.secret(CLIENT_KEY) || "")"#)
        );
    }
}
