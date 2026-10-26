//! Permanent API tokens: the console-issued credential an AI client keeps in a fixed MCP
//! configuration.
//!
//! A token is `srm_` plus 32 bytes of OS randomness, in lowercase hex. It is shown **once**, in the
//! create or rotate response, and only its SHA-256 is persisted, so the file (or a copy of it) never
//! holds a usable credential. A client presents the token as its bearer on `/mcp/:id`; the console finds the
//! record by hash, refuses a token that names another device, and signs a sixty-second grant with
//! the token's tools and destructive consent. The device protocol and its offline verification are
//! unchanged, because the device only ever sees a grant.
//!
//! A token is a bearer whose lifetime the operator chooses at creation: absent by default, in
//! which case revocation is the only withdrawal and it removes the record. A token given a lifetime
//! stops authenticating calls at that instant, as a revoked one does. Rotation is the other way
//! out: one atomic replacement whose new id and secret keep the old scope and lifetime policy, so
//! the previous bearer stops working at the same instant the next one starts.
//! The scope is fixed at creation — one device, a non-empty set of registry tools, and the operator's
//! destructive consent — so a token can never name a tool or a device the operator did not.
//!
//! The record also keeps when it last authenticated a call. The in-memory fact moves on every
//! successful exchange, the file at most once a minute ([`LAST_USED_SAVE_MS`]), so a reload may lag
//! by under a minute. The operator's list shows it: revoking an unused token is an informed act.
//!
//! The file is `<data>/tokens.json`, written whole through a temporary file and a rename, mode 0600,
//! like the device registry. A file this build cannot trust is quarantined and the console starts
//! with no tokens: the file is a lookup table for a credential the operator can mint again, and a
//! damaged one must deny API clients, not take the control plane down.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::auth::{constant_time_eq, random_32};
use crate::config::{self, StartupError};
use crate::hex;
use crate::store;

/// At most this many tokens may exist at once.
pub(crate) const MAX_TOKENS: usize = 64;

/// The longest a token's name may be, in characters, after trimming.
pub(crate) const MAX_NAME_CHARS: usize = 80;

/// The shortest lifetime a token may be given at creation, in seconds: one hour.
///
/// A shorter window is a unit error (`ms` sent as seconds), not a credential, so it is refused
/// rather than minted with a surprise expiry.
pub(crate) const MIN_TOKEN_EXPIRY_S: u64 = 3_600;

/// The longest lifetime a token may be given at creation, in seconds: 365 days.
pub(crate) const MAX_TOKEN_EXPIRY_S: u64 = 31_536_000;

/// The prefix every token carries, so a bearer is recognized without a second field.
pub(crate) const TOKEN_PREFIX: &str = "srm_";

/// How many hex characters follow the prefix: 32 bytes.
pub(crate) const TOKEN_HEX_CHARS: usize = 64;

/// The full length of a token: the prefix plus its 32 bytes.
pub(crate) const TOKEN_LEN: usize = TOKEN_PREFIX.len() + TOKEN_HEX_CHARS;

/// How long the grant minted for one token request lives.
///
/// Sixty seconds is [`crate::grants::MIN_GRANT_LIFETIME_S`], the shortest a grant may live: the point
/// of a long-lived token is that the *token* is long-lived, not the grant it is exchanged for.
pub(crate) const MINTED_GRANT_LIFETIME_S: u64 = 60;

/// How often a token's last use is allowed to rewrite the file.
///
/// Every successful exchange updates the in-memory fact; the file is written only when the value it
/// already holds is older than this, so a busy client does not rewrite the credential file once per
/// request. A reload can therefore lag by under a minute, which is the documented behaviour.
pub(crate) const LAST_USED_SAVE_MS: u64 = 60_000;

/// The store's file name under the data directory.
const TOKENS_FILE: &str = "tokens.json";

/// One token as it is stored: the public facts plus the hash of the secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    /// The record's id, from the OS CSPRNG. Revocation names it; it is not a credential.
    pub id: String,
    /// The operator's label for the token.
    pub name: String,
    /// The one device the token may reach.
    pub device_id: String,
    /// The registry tools the minted grant may name.
    pub tools: Vec<String>,
    /// The operator's signed consent to destructive tools under this token.
    pub allow_destructive: bool,
    /// When the token was created, in Unix milliseconds.
    pub created_at_ms: u64,
    /// When the token stops being a credential, in Unix milliseconds; absent means it never does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
    /// When the token last authenticated an MCP call, in Unix milliseconds; absent until it does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_ms: Option<u64>,
    /// The value in the file right now: the memory fact may be newer while the save window is open.
    /// It is not a second fact to serialize — the file's own `last_used_ms` is what this mirrors.
    #[serde(skip)]
    saved_last_used_ms: Option<u64>,
    /// SHA-256 of the token, 64 lowercase hex. The plaintext is never stored.
    pub hash: String,
}

impl Token {
    /// The public facts, without the hash: what a list may carry.
    fn meta(&self) -> TokenMeta {
        TokenMeta {
            id: self.id.clone(),
            name: self.name.clone(),
            device_id: self.device_id.clone(),
            tools: self.tools.clone(),
            allow_destructive: self.allow_destructive,
            created_at_ms: self.created_at_ms,
            expires_at_ms: self.expires_at_ms,
            last_used_ms: self.last_used_ms,
        }
    }
}

/// One token as the operator's list sees it: never the hash, never the secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TokenMeta {
    pub id: String,
    pub name: String,
    pub device_id: String,
    pub tools: Vec<String>,
    pub allow_destructive: bool,
    pub created_at_ms: u64,
    /// When the token stops being a credential, in Unix milliseconds; absent means it never does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
    /// When the token last authenticated an MCP call, in Unix milliseconds; absent until it does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_ms: Option<u64>,
}

/// A request to mint one token, after the operator has filled a form.
pub(crate) struct Draft {
    pub(crate) name: String,
    pub(crate) device_id: String,
    pub(crate) tools: Vec<String>,
    pub(crate) allow_destructive: bool,
    /// The requested lifetime in seconds; absent means the token never expires.
    pub(crate) expires_in_s: Option<u64>,
}

/// Why a token could not be minted, or a revocation could not be written.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TokenError {
    /// The name is empty after trimming.
    NameEmpty,
    /// The name is longer than [`MAX_NAME_CHARS`] after trimming.
    NameTooLong,
    /// No tools were named; a token that permits nothing is a mistake, not a token.
    EmptyTools,
    /// At least one tool is not in the device's capability registry.
    UnknownTool,
    /// The token names a device this console does not have.
    UnknownDevice,
    /// [`MAX_TOKENS`] tokens already exist.
    TooManyTokens,
    /// The requested lifetime is shorter than [`MIN_TOKEN_EXPIRY_S`].
    ExpiryTooShort,
    /// The requested lifetime is longer than [`MAX_TOKEN_EXPIRY_S`].
    ExpiryTooLong,
    /// The file could not be written; memory still serves the previous list.
    Storage,
}

/// The in-memory token registry and the file it is persisted to.
pub(crate) struct Tokens {
    path: PathBuf,
    tokens: BTreeMap<String, Token>,
}

impl Tokens {
    /// Load `<data>/tokens.json`, creating the data directory if it is absent.
    pub(crate) fn load(data: &Path) -> Result<Tokens, StartupError> {
        store::create_data_dir(data).map_err(|source| StartupError::DataDirectory {
            path: data.to_path_buf(),
            source,
        })?;

        let path = data.join(TOKENS_FILE);
        config::check_private_file("API tokens", &path)?;
        // A token file never uses the registry's backup path: a backup is the previous complete
        // list, so the save after a revocation would keep the withdrawn hash alive on disk, and a
        // lost `tokens.json` would then resurrect the token. Remove one left by an older build.
        let _ = std::fs::remove_file(store::backup_path(&path));
        let list: Vec<Token> = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(list) => list,
                Err(reason) => {
                    quarantine_and_start_empty(&path, &format!("not a tokens file: {reason}"));
                    Vec::new()
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                quarantine_and_start_empty(&path, &error.to_string());
                Vec::new()
            }
        };

        let (tokens, dropped) = match index(list) {
            Ok(indexed) => indexed,
            Err(reason) => {
                quarantine_and_start_empty(&path, &reason);
                (BTreeMap::new(), 0)
            }
        };
        if dropped > 0 {
            // A tool leaving the registry does not make the file untrustworthy: the records that
            // name it are dropped and the operator's fixed configs keep working where they can.
            eprintln!(
                "summrise-console: {}: {dropped} API token record(s) named tools outside this \
                 build's registry and were ignored",
                path.display()
            );
        }

        Ok(Tokens { path, tokens })
    }

    /// Every token's public facts, newest first. Never the hash and never the secret.
    pub(crate) fn list(&self) -> Vec<TokenMeta> {
        let mut list: Vec<TokenMeta> = self.tokens.values().map(Token::meta).collect();
        // Newest first; equal timestamps order by id so a list is stable rather than file order.
        list.sort_by(|a, b| {
            b.created_at_ms
                .cmp(&a.created_at_ms)
                .then_with(|| a.id.cmp(&b.id))
        });
        list
    }

    /// Mint one token and return its public facts with the plaintext, which is the only time it is
    /// ever visible.
    ///
    /// `device_known` is the registry's answer about `draft.device_id`; this store does not keep a
    /// second copy of the device list, so the caller passes the fact in.
    pub(crate) fn create(
        &mut self,
        draft: Draft,
        device_known: bool,
        now_ms: u64,
    ) -> Result<(TokenMeta, String), TokenError> {
        let name = draft.name.trim();
        if name.is_empty() {
            return Err(TokenError::NameEmpty);
        }
        if name.chars().count() > MAX_NAME_CHARS {
            return Err(TokenError::NameTooLong);
        }
        if draft.tools.is_empty() {
            return Err(TokenError::EmptyTools);
        }
        if draft
            .tools
            .iter()
            .any(|tool| summrise_core::registry::tool(tool).is_none())
        {
            return Err(TokenError::UnknownTool);
        }
        if !device_known {
            return Err(TokenError::UnknownDevice);
        }
        if self.tokens.len() >= MAX_TOKENS {
            return Err(TokenError::TooManyTokens);
        }
        if let Some(seconds) = draft.expires_in_s {
            if seconds < MIN_TOKEN_EXPIRY_S {
                return Err(TokenError::ExpiryTooShort);
            }
            if seconds > MAX_TOKEN_EXPIRY_S {
                return Err(TokenError::ExpiryTooLong);
            }
        }
        // An absent lifetime stays absent: permanence is the default, not a sentinel value.
        let expires_at_ms = draft
            .expires_in_s
            .map(|seconds| now_ms.saturating_add(seconds.saturating_mul(1_000)));

        // The plaintext exists in this scope, in the return value and in nothing else: the record
        // keeps only its hash, and the file is written from the record.
        let plaintext = format!("{TOKEN_PREFIX}{}", hex::encode(&random_32()));
        let token = Token {
            id: hex::encode(&random_32()),
            name: name.to_owned(),
            device_id: draft.device_id,
            tools: draft.tools,
            allow_destructive: draft.allow_destructive,
            created_at_ms: now_ms,
            expires_at_ms,
            last_used_ms: None,
            saved_last_used_ms: None,
            hash: hash_token(&plaintext),
        };
        let meta = token.meta();

        let mut next = self.tokens.clone();
        next.insert(token.id.clone(), token);
        self.save(&next).map_err(|_| TokenError::Storage)?;
        self.tokens = next;

        Ok((meta, plaintext))
    }

    /// Replace one token's identity in one save, keeping its scope and lifetime policy.
    ///
    /// The next map is built first — the old record removed, the replacement inserted with the old
    /// name, device, tools and destructive consent, a new id, secret and creation time, and a new
    /// expiry of the same duration as the old one (or none, if the old was permanent) — then
    /// written once. Memory adopts it only after the write succeeds, so a failed save is [`Storage`]
    /// with the old token still valid in memory and in the file: there is no moment with both
    /// secrets or neither.
    ///
    /// The count does not change, so [`MAX_TOKENS`] is not checked: a rotation at the cap is the
    /// operator replacing a token, not minting an extra one.
    ///
    /// [`Storage`]: TokenError::Storage
    pub(crate) fn rotate(
        &mut self,
        id: &str,
        now_ms: u64,
    ) -> Result<Option<(TokenMeta, String)>, TokenError> {
        let Some(old) = self.tokens.get(id) else {
            return Ok(None);
        };

        // The old record's lifetime is the policy — expiry minus creation — so the replacement gets
        // a fresh period of the same length, never the old absolute date.
        let lifetime = old
            .expires_at_ms
            .map(|expires_at_ms| expires_at_ms.saturating_sub(old.created_at_ms));

        // The plaintext exists in this scope, in the return value and in nothing else: the record
        // keeps only its hash, and the file is written from the record.
        let plaintext = format!("{TOKEN_PREFIX}{}", hex::encode(&random_32()));
        let token = Token {
            id: hex::encode(&random_32()),
            name: old.name.clone(),
            device_id: old.device_id.clone(),
            tools: old.tools.clone(),
            allow_destructive: old.allow_destructive,
            created_at_ms: now_ms,
            expires_at_ms: lifetime.map(|duration| now_ms.saturating_add(duration)),
            last_used_ms: None,
            saved_last_used_ms: None,
            hash: hash_token(&plaintext),
        };
        let meta = token.meta();

        let mut next = self.tokens.clone();
        next.remove(id);
        next.insert(token.id.clone(), token);
        self.save(&next).map_err(|_| TokenError::Storage)?;
        self.tokens = next;

        Ok(Some((meta, plaintext)))
    }

    /// Remove a token, returning the record so the audit can name its device.
    pub(crate) fn revoke(&mut self, id: &str) -> Result<Option<Token>, TokenError> {
        if !self.tokens.contains_key(id) {
            return Ok(None);
        }
        let mut next = self.tokens.clone();
        let removed = next.remove(id);
        self.save(&next).map_err(|_| TokenError::Storage)?;
        self.tokens = next;
        Ok(removed)
    }

    /// Remove every token scoped to `device_id`, returning the records. Device removal calls this so
    /// a retired machine's tokens do not outlive it and consume the cap with no page to revoke them.
    pub(crate) fn revoke_for_device(&mut self, device_id: &str) -> Result<Vec<Token>, TokenError> {
        let removed: Vec<Token> = self
            .tokens
            .values()
            .filter(|token| token.device_id == device_id)
            .cloned()
            .collect();
        if removed.is_empty() {
            return Ok(removed);
        }
        let mut next = self.tokens.clone();
        for token in &removed {
            next.remove(&token.id);
        }
        self.save(&next).map_err(|_| TokenError::Storage)?;
        self.tokens = next;
        Ok(removed)
    }

    /// The token whose hash is `hash`, if any.
    pub(crate) fn find_by_hash(&self, hash: &str) -> Option<Token> {
        // The comparison walks the whole hash, as every credential comparison in this service does.
        self.tokens
            .values()
            .find(|token| constant_time_eq(token.hash.as_bytes(), hash.as_bytes()))
            .cloned()
    }

    /// Record that the token whose hash is `hash` authenticated a call at `now_ms`.
    ///
    /// Every use moves the in-memory fact; the file is written only when the value it holds is absent
    /// or at least [`LAST_USED_SAVE_MS`] old. A failed write is not a failed call: the memory fact
    /// stays, the previous persisted value is kept, and the next due use tries again. Returns whether
    /// a record matched at all.
    pub(crate) fn mark_used(&mut self, hash: &str, now_ms: u64) -> bool {
        let Some(token) = self
            .tokens
            .values_mut()
            .find(|token| constant_time_eq(token.hash.as_bytes(), hash.as_bytes()))
        else {
            return false;
        };
        let previous = token.saved_last_used_ms;
        token.last_used_ms = Some(now_ms);
        let due =
            previous.is_none_or(|previous| now_ms.saturating_sub(previous) >= LAST_USED_SAVE_MS);
        if !due {
            return true;
        }

        // The write is the whole list, as every mutation of this store is. On success the file holds
        // exactly the current memory values, so those are the values now persisted.
        let next = self.tokens.clone();
        if self.save(&next).is_ok() {
            self.tokens = next;
            for token in self.tokens.values_mut() {
                token.saved_last_used_ms = token.last_used_ms;
            }
        }
        true
    }

    /// Replace the file with `tokens`, atomically.
    fn save(&self, tokens: &BTreeMap<String, Token>) -> std::io::Result<()> {
        let list: Vec<&Token> = tokens.values().collect();
        store::write_json_atomic_without_backup(&self.path, &list)
    }
}

/// The records as a map by id and how many were dropped for naming unknown tools, or why the file
/// cannot be trusted.
///
/// The checks are the file's own invariants: an id and hash the console could have written, a device
/// and tools this build knows, and no duplicate id or hash (two records with one hash would make
/// revocation name an arbitrary one of them). A tool that left the registry is not a corrupt file:
/// that record cannot be used, so it is dropped, not the whole list.
fn index(list: Vec<Token>) -> Result<(BTreeMap<String, Token>, usize), String> {
    let mut tokens = BTreeMap::new();
    let mut hashes = BTreeSet::new();
    let mut dropped = 0;
    for mut token in list {
        if !is_lower_hex_64(&token.id) {
            return Err("an entry's id is not 32 bytes of lowercase hex".to_owned());
        }
        if !is_lower_hex_64(&token.hash) {
            return Err("an entry's hash is not 32 bytes of lowercase hex".to_owned());
        }
        if !is_lower_hex_64(&token.device_id) {
            return Err(
                "an entry names a device id that is not 32 bytes of lowercase hex".to_owned(),
            );
        }
        if token.name.trim().is_empty() || token.name.chars().count() > MAX_NAME_CHARS {
            return Err("an entry has an empty or overlong name".to_owned());
        }
        if token.tools.is_empty()
            || token
                .tools
                .iter()
                .any(|tool| summrise_core::registry::tool(tool).is_none())
        {
            dropped += 1;
            continue;
        }
        if !hashes.insert(token.hash.clone()) {
            return Err("the same hash appears twice".to_owned());
        }
        // The loaded value is what the file holds; memory starts out agreeing with it.
        token.saved_last_used_ms = token.last_used_ms;
        if tokens.insert(token.id.clone(), token).is_some() {
            return Err("the same id appears twice".to_owned());
        }
    }
    if tokens.len() > MAX_TOKENS {
        return Err(format!(
            "the file holds {} records, more than the {MAX_TOKENS} this console can mint",
            tokens.len()
        ));
    }
    Ok((tokens, dropped))
}

/// Quarantine `path` and say so once; the console then starts with no API tokens.
fn quarantine_and_start_empty(path: &Path, reason: &str) {
    let quarantined = store::quarantine(path);
    eprintln!(
        "summrise-console: {} is unusable ({reason}); quarantined to {} and starting with no API \
         tokens",
        path.display(),
        quarantined
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "nothing".to_owned())
    );
}

/// Whether `value` is exactly 64 lowercase hex characters: the only shape this build writes.
fn is_lower_hex_64(value: &str) -> bool {
    value.len() == TOKEN_HEX_CHARS
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// The lookup key for a presented token: SHA-256 of the whole string, prefix included.
pub(crate) fn hash_token(token: &str) -> String {
    crate::hash::sha256_hex(token.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("summrise-tokens-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp dir");
        path
    }

    fn device_id() -> String {
        "ab".repeat(32)
    }

    fn other_device() -> String {
        "cd".repeat(32)
    }

    fn draft(name: &str, device_id: &str, tools: &[&str]) -> Draft {
        Draft {
            name: name.to_owned(),
            device_id: device_id.to_owned(),
            tools: tools.iter().map(|tool| (*tool).to_owned()).collect(),
            allow_destructive: false,
            expires_in_s: None,
        }
    }

    fn is_token(text: &str) -> bool {
        let Some(body) = text.strip_prefix(TOKEN_PREFIX) else {
            return false;
        };
        body.len() == TOKEN_HEX_CHARS
            && body
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    }

    /// The secret is shown once and stored as its hash: the file is not a credential, and the list
    /// carries neither the hash nor the secret.
    #[test]
    fn a_minted_token_is_srm_plus_sixty_four_hex_and_only_its_hash_reaches_the_file() {
        let data = tempdir("mint");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");

        let (meta, plaintext) = tokens
            .create(
                draft(
                    "  laptop-dsh  ",
                    &device_id(),
                    &["files.read", "memory.search"],
                ),
                true,
                1_000,
            )
            .expect("a valid token");

        assert!(
            is_token(&plaintext),
            "the token is 'srm_' plus 64 lowercase hex, got {plaintext}"
        );
        assert_eq!(plaintext.len(), TOKEN_LEN);
        assert_eq!(meta.name, "laptop-dsh", "the name is stored trimmed");
        assert_eq!(meta.device_id, device_id());
        assert_eq!(
            meta.tools,
            vec!["files.read".to_owned(), "memory.search".to_owned()]
        );
        assert!(!meta.allow_destructive);
        assert_eq!(meta.created_at_ms, 1_000);

        let hash = hash_token(&plaintext);
        let stored = tokens
            .find_by_hash(&hash)
            .expect("the hash finds the token");
        assert_eq!(stored.id, meta.id);
        assert_eq!(stored.hash, hash);
        assert_eq!(stored.tools, meta.tools);

        let text = std::fs::read_to_string(data.join(TOKENS_FILE)).expect("the file is readable");
        assert!(text.contains(&hash), "the file keeps the hash");
        assert!(
            !text.contains(&plaintext),
            "the file must never hold the token"
        );

        let listed = tokens.list();
        assert_eq!(listed, vec![meta.clone()]);
        let json = serde_json::to_value(&listed[0]).expect("a meta serializes");
        assert!(
            json.get("hash").is_none(),
            "the list carries no hash: {json}"
        );
        assert!(
            json.get("token").is_none(),
            "the list carries no secret: {json}"
        );

        let reloaded = Tokens::load(&data).expect("the store reloads");
        assert_eq!(
            reloaded.find_by_hash(&hash).map(|token| token.id),
            Some(meta.id),
            "a restart still finds the token"
        );
        assert!(
            tokens.find_by_hash(&"00".repeat(32)).is_none(),
            "another hash finds nothing"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(data.join(TOKENS_FILE))
                .expect("the file exists")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "the token file is private");
        }
        std::fs::remove_dir_all(&data).ok();
    }

    /// Revocation is a fact on disk: a restart must not admit a token the operator withdrew.
    #[test]
    fn revoking_a_token_removes_it_from_memory_and_from_the_file() {
        let data = tempdir("revoke");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");
        let (meta, plaintext) = tokens
            .create(draft("ci", &device_id(), &["files.read"]), true, 1_000)
            .expect("a valid token");
        let hash = hash_token(&plaintext);

        assert!(
            tokens
                .revoke(&meta.id)
                .expect("the write succeeds")
                .is_some(),
            "the token was there"
        );
        assert!(tokens.find_by_hash(&hash).is_none());
        assert!(tokens.list().is_empty());
        assert!(
            tokens
                .revoke(&meta.id)
                .expect("a second revocation is a no-op")
                .is_none(),
            "revoking a gone token reports that it was gone"
        );

        let reloaded = Tokens::load(&data).expect("the store reloads");
        assert!(reloaded.find_by_hash(&hash).is_none());
        std::fs::remove_dir_all(&data).ok();
    }

    /// Rotation keeps the scope and replaces the identity: a new id and secret, a fresh creation
    /// time and no last use, with the old hash gone from the one record left behind.
    #[test]
    fn rotating_a_token_keeps_its_scope_and_replaces_its_identity() {
        let data = tempdir("rotate");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");
        let mut scope = draft("laptop-dsh", &device_id(), &["files.read", "memory.search"]);
        scope.allow_destructive = true;
        let (old_meta, old_plaintext) = tokens.create(scope, true, 1_000).expect("a valid token");
        assert!(
            tokens.mark_used(&hash_token(&old_plaintext), 1_500),
            "the old record has a use to lose"
        );

        let (meta, plaintext) = tokens
            .rotate(&old_meta.id, 2_000)
            .expect("the rotation writes")
            .expect("the id was there");

        assert!(
            is_token(&plaintext),
            "the replacement is 'srm_' plus 64 lowercase hex, got {plaintext}"
        );
        assert_ne!(plaintext, old_plaintext, "a rotation mints a new secret");
        assert_ne!(meta.id, old_meta.id, "a rotation mints a new id");
        assert_eq!(meta.name, old_meta.name, "the name is kept");
        assert_eq!(meta.device_id, old_meta.device_id, "the device is kept");
        assert_eq!(meta.tools, old_meta.tools, "the tools are kept");
        assert!(meta.allow_destructive, "the destructive consent is kept");
        assert_eq!(meta.created_at_ms, 2_000, "the new record is new");
        assert_eq!(
            meta.last_used_ms, None,
            "the new record has never been used"
        );

        let old_hash = hash_token(&old_plaintext);
        let new_hash = hash_token(&plaintext);
        assert!(
            tokens.find_by_hash(&old_hash).is_none(),
            "the old hash is gone from memory"
        );
        assert_eq!(
            tokens.find_by_hash(&new_hash).map(|token| token.id),
            Some(meta.id.clone()),
            "the new hash finds the replacement"
        );
        assert_eq!(tokens.list(), vec![meta.clone()], "one record, the new one");

        let text = std::fs::read_to_string(data.join(TOKENS_FILE)).expect("the file is readable");
        assert!(!text.contains(&old_hash), "the old hash is gone from disk");
        assert!(text.contains(&new_hash), "the new hash is on disk");
        assert!(
            !text.contains(&plaintext),
            "the file must never hold the token"
        );

        let reloaded = Tokens::load(&data).expect("the store reloads");
        assert!(
            reloaded.find_by_hash(&old_hash).is_none(),
            "a reload does not resurrect the old hash"
        );
        assert_eq!(
            reloaded.find_by_hash(&new_hash).map(|token| token.id),
            Some(meta.id.clone()),
            "a reload finds the replacement by its hash"
        );
        assert_eq!(
            reloaded.list(),
            vec![meta],
            "a reload holds exactly the new record"
        );
        std::fs::remove_dir_all(&data).ok();
    }

    /// Rotation replaces a record rather than adding one, so it is allowed at the cap: the operator
    /// does not have to revoke a token to make room for its replacement.
    #[test]
    fn rotating_at_the_cap_keeps_the_count() {
        let data = tempdir("rotate-at-cap");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");
        let mut ids = Vec::new();
        for index in 0..MAX_TOKENS {
            let (meta, _) = tokens
                .create(
                    draft(&format!("client-{index}"), &device_id(), &["files.read"]),
                    true,
                    1_000 + index as u64,
                )
                .expect("a token under the cap");
            ids.push(meta.id);
        }

        let (rotated, _) = tokens
            .rotate(&ids[0], 2_000)
            .expect("a rotation at the cap writes")
            .expect("the id was there");

        assert_ne!(rotated.id, ids[0], "the record was replaced");
        assert_eq!(
            tokens.list().len(),
            MAX_TOKENS,
            "rotation replaces one record instead of adding one"
        );
        assert_eq!(
            Tokens::load(&data).expect("the store reloads").list().len(),
            MAX_TOKENS,
            "the file still holds exactly the cap"
        );
        std::fs::remove_dir_all(&data).ok();
    }

    /// An unknown id is a plain `None`, and nothing is written: a rotation of something that is not
    /// there is not a way to rewrite the list.
    #[test]
    fn rotating_an_unknown_id_writes_nothing() {
        let data = tempdir("rotate-unknown");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");
        let (meta, _) = tokens
            .create(draft("laptop", &device_id(), &["files.read"]), true, 1_000)
            .expect("a valid token");
        let before = std::fs::read(data.join(TOKENS_FILE)).expect("the file is readable");
        let list_before = tokens.list();

        let rotated = tokens
            .rotate(&"ff".repeat(32), 2_000)
            .expect("an unknown id is not an error");

        assert!(rotated.is_none(), "there is no such token to replace");
        assert_eq!(tokens.list(), list_before, "the list is unchanged");
        assert_eq!(tokens.list()[0].id, meta.id, "the token is still there");
        assert_eq!(
            std::fs::read(data.join(TOKENS_FILE)).expect("the file is readable"),
            before,
            "nothing was written"
        );
        std::fs::remove_dir_all(&data).ok();
    }

    /// A failed save is not a half rotation: `Storage` is returned, memory keeps the old record and
    /// the file is untouched, so the old secret is still the one that works.
    #[cfg(unix)]
    #[test]
    fn a_failed_rotation_write_leaves_the_old_token_valid() {
        use std::os::unix::fs::PermissionsExt;

        let data = tempdir("rotate-write-failure");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");
        let (meta, plaintext) = tokens
            .create(draft("laptop", &device_id(), &["files.read"]), true, 1_000)
            .expect("a valid token");
        let hash = hash_token(&plaintext);
        let before = std::fs::read(data.join(TOKENS_FILE)).expect("the file is readable");
        let list_before = tokens.list();

        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o500))
            .expect("the directory is made read-only");
        let rotated = tokens.rotate(&meta.id, 2_000);
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700))
            .expect("the directory is writable again");

        assert_eq!(rotated.err(), Some(TokenError::Storage));
        assert_eq!(
            tokens.find_by_hash(&hash).map(|token| token.id),
            Some(meta.id.clone()),
            "the old token still authenticates from memory"
        );
        assert_eq!(tokens.list(), list_before, "the list is unchanged");
        assert_eq!(
            std::fs::read(data.join(TOKENS_FILE)).expect("the file is readable"),
            before,
            "the file still holds exactly the old record"
        );
        assert_eq!(
            Tokens::load(&data)
                .expect("the file still loads")
                .find_by_hash(&hash)
                .map(|token| token.id),
            Some(meta.id),
            "a reload still finds the old token"
        );
        std::fs::remove_dir_all(&data).ok();
    }

    /// A cap, not an unbounded credential list.
    #[test]
    fn the_registry_refuses_the_sixty_fifth_token() {
        let data = tempdir("cap");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");
        for index in 0..MAX_TOKENS {
            tokens
                .create(
                    draft(&format!("client-{index}"), &device_id(), &["files.read"]),
                    true,
                    1_000 + index as u64,
                )
                .expect("a token under the cap");
        }

        let refused = tokens.create(
            draft("one-too-many", &device_id(), &["files.read"]),
            true,
            2_000,
        );

        assert_eq!(refused.err(), Some(TokenError::TooManyTokens));
        assert_eq!(tokens.list().len(), MAX_TOKENS);
        assert_eq!(
            Tokens::load(&data).expect("the store reloads").list().len(),
            MAX_TOKENS,
            "the refusal was not written"
        );
        std::fs::remove_dir_all(&data).ok();
    }

    /// Every refused create is typed, and none of them writes anything.
    #[test]
    fn names_tools_and_the_device_are_validated_without_writing() {
        let data = tempdir("validate");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");

        assert_eq!(
            tokens
                .create(draft("   ", &device_id(), &["files.read"]), true, 0)
                .err(),
            Some(TokenError::NameEmpty)
        );
        let long = "n".repeat(MAX_NAME_CHARS + 1);
        assert_eq!(
            tokens
                .create(draft(&long, &device_id(), &["files.read"]), true, 0)
                .err(),
            Some(TokenError::NameTooLong)
        );
        assert_eq!(
            tokens.create(draft("x", &device_id(), &[]), true, 0).err(),
            Some(TokenError::EmptyTools)
        );
        assert_eq!(
            tokens
                .create(draft("x", &device_id(), &["no.such.tool"]), true, 0)
                .err(),
            Some(TokenError::UnknownTool)
        );
        assert_eq!(
            tokens
                .create(draft("x", &device_id(), &["files.read"]), false, 0)
                .err(),
            Some(TokenError::UnknownDevice)
        );

        assert!(
            tokens.list().is_empty(),
            "a refused create leaves the store empty"
        );
        assert!(
            !data.join(TOKENS_FILE).exists(),
            "and writes no file at all"
        );

        let exact = "n".repeat(MAX_NAME_CHARS);
        tokens
            .create(draft(&exact, &device_id(), &["files.read"]), true, 0)
            .expect("eighty characters is the limit");
        assert_eq!(
            tokens
                .create(draft("y", &other_device(), &["files.read"]), true, 0)
                .ok()
                .map(|(meta, _)| meta.device_id),
            Some(other_device()),
            "a second known device is a different scope, not a refusal"
        );
        assert_eq!(tokens.list().len(), 2);
        std::fs::remove_dir_all(&data).ok();
    }

    /// A damaged file denies API clients but does not take the console down; the bytes are kept.
    #[test]
    fn a_damaged_tokens_file_starts_with_no_tokens_and_keeps_its_bytes() {
        let data = tempdir("corrupt");
        let path = data.join(TOKENS_FILE);
        std::fs::write(&path, b"{ not a tokens file").expect("the fixture is written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("the fixture is private");
        }

        let tokens = Tokens::load(&data).expect("the console still starts");

        assert!(tokens.list().is_empty());
        assert!(!path.exists(), "the damaged file is out of the way");
        assert!(
            data.join("tokens.json.bad").exists(),
            "the bytes are kept as evidence"
        );
        std::fs::remove_dir_all(&data).ok();
    }

    /// A record whose hash is not one the console could have written makes the whole file
    /// untrustworthy: a credential file with a broken hash is not partly served.
    #[test]
    fn a_record_with_a_bad_hash_quarantines_the_whole_file() {
        let data = tempdir("untrusted-hash");
        let path = data.join(TOKENS_FILE);
        let bad = serde_json::json!([{
            "id": "ab".repeat(32),
            "name": "laptop",
            "device_id": "ab".repeat(32),
            "tools": ["files.read"],
            "allow_destructive": false,
            "created_at_ms": 1_000,
            "hash": "zz",
        }]);
        std::fs::write(&path, bad.to_string()).expect("the fixture is written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("the fixture is private");
        }

        let tokens = Tokens::load(&data).expect("the console still starts");

        assert!(tokens.list().is_empty(), "no record is served");
        assert!(!path.exists(), "the file is out of the way");
        assert!(
            data.join("tokens.json.bad").exists(),
            "the bytes are kept as evidence"
        );
        std::fs::remove_dir_all(&data).ok();
    }

    /// A tool that left the registry drops that record, not every token: fixed configurations for
    /// other tools keep working after a registry change.
    #[test]
    fn a_record_with_a_tool_outside_the_registry_is_dropped_alone() {
        let data = tempdir("registry-drift");
        let path = data.join(TOKENS_FILE);
        let valid = serde_json::json!([{
            "id": "aa".repeat(32),
            "name": "keep",
            "device_id": "ab".repeat(32),
            "tools": ["files.read"],
            "allow_destructive": false,
            "created_at_ms": 2_000,
            "hash": "11".repeat(32),
        }, {
            "id": "bb".repeat(32),
            "name": "drop",
            "device_id": "ab".repeat(32),
            "tools": ["no.such.tool"],
            "allow_destructive": false,
            "created_at_ms": 1_000,
            "hash": "22".repeat(32),
        }]);
        std::fs::write(&path, valid.to_string()).expect("the fixture is written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("the fixture is private");
        }

        let tokens = Tokens::load(&data).expect("the console still starts");

        assert_eq!(tokens.list().len(), 1, "only the valid record is served");
        assert_eq!(tokens.list()[0].name, "keep");
        assert!(path.exists(), "a registry change is not file corruption");
        assert!(
            !data.join("tokens.json.bad").exists(),
            "nothing is quarantined"
        );
        std::fs::remove_dir_all(&data).ok();
    }

    /// A revocation is durable: the store keeps no backup of the pre-revocation list, so losing
    /// `tokens.json` cannot resurrect a withdrawn token.
    #[test]
    fn a_revocation_leaves_no_backup_to_restore() {
        let data = tempdir("revoke-durable");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");
        let (meta, plaintext) = tokens
            .create(draft("laptop", &device_id(), &["files.read"]), true, 0)
            .expect("a valid token");
        tokens.revoke(&meta.id).expect("the revocation writes");

        assert!(
            !data.join("tokens.json.bak").exists(),
            "a credential file keeps no previous list"
        );
        std::fs::remove_file(data.join(TOKENS_FILE)).expect("the operator loses the file");
        let reloaded = Tokens::load(&data).expect("the console still starts");
        assert!(reloaded.list().is_empty(), "no token is resurrected");
        assert!(reloaded.find_by_hash(&hash_token(&plaintext)).is_none());
        std::fs::remove_dir_all(&data).ok();
    }

    /// The cap is a property of the file too: a hand-edited list larger than the console can mint
    /// is quarantined, not served.
    #[test]
    fn a_file_over_the_cap_is_quarantined() {
        let data = tempdir("over-cap");
        let path = data.join(TOKENS_FILE);
        let list: Vec<serde_json::Value> = (0..MAX_TOKENS + 1)
            .map(|index| {
                serde_json::json!({
                    "id": format!("{index:064x}"),
                    "name": format!("token-{index}"),
                    "device_id": device_id(),
                    "tools": ["files.read"],
                    "allow_destructive": false,
                    "created_at_ms": 1_000,
                    "hash": format!("{:064x}", index + 10_000),
                })
            })
            .collect();
        std::fs::write(&path, serde_json::json!(list).to_string()).expect("the fixture is written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("the fixture is private");
        }

        let tokens = Tokens::load(&data).expect("the console still starts");

        assert!(tokens.list().is_empty());
        assert!(data.join("tokens.json.bad").exists());
        std::fs::remove_dir_all(&data).ok();
    }

    /// Removing a device removes its tokens, not another device's.
    #[test]
    fn revoking_for_a_device_removes_only_that_devices_tokens() {
        let data = tempdir("revoke-device");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");
        let (_a1, _) = tokens
            .create(draft("one", &device_id(), &["files.read"]), true, 0)
            .expect("a token");
        let (_a2, _) = tokens
            .create(draft("two", &device_id(), &["files.read"]), true, 0)
            .expect("a token");
        let (_b, _) = tokens
            .create(draft("other", &other_device(), &["files.read"]), true, 0)
            .expect("a token");

        let removed = tokens
            .revoke_for_device(&device_id())
            .expect("the revocation writes");

        assert_eq!(removed.len(), 2);
        assert_eq!(tokens.list().len(), 1);
        assert_eq!(tokens.list()[0].name, "other");
        let reloaded = Tokens::load(&data).expect("the store reloads");
        assert_eq!(reloaded.list().len(), 1);
        assert_eq!(reloaded.list()[0].name, "other");
        std::fs::remove_dir_all(&data).ok();
    }

    /// A leftover temporary file is not the store; the complete file is what loads.
    #[test]
    fn a_leftover_temporary_file_does_not_replace_the_store() {
        let data = tempdir("leftover");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");
        let (meta, plaintext) = tokens
            .create(draft("laptop", &device_id(), &["files.read"]), true, 1_000)
            .expect("a valid token");
        std::fs::write(data.join("tokens.json.tmp"), b"{ half a write")
            .expect("the leftover is written");

        let reloaded = Tokens::load(&data).expect("the complete file is readable");

        assert_eq!(
            reloaded
                .find_by_hash(&hash_token(&plaintext))
                .map(|token| token.id),
            Some(meta.id),
            "the complete file is what loads, not the leftover"
        );
        assert!(
            data.join("tokens.json.tmp").exists(),
            "the leftover is not the store and is not silently promoted"
        );
        std::fs::remove_dir_all(&data).ok();
    }

    /// A token that has never authenticated a call says so by saying nothing.
    #[test]
    fn a_new_token_has_no_last_use() {
        let data = tempdir("last-used-none");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");
        let (meta, _) = tokens
            .create(draft("laptop", &device_id(), &["files.read"]), true, 1_000)
            .expect("a valid token");

        assert_eq!(meta.last_used_ms, None, "a new token has never been used");
        let json = serde_json::to_value(&tokens.list()[0]).expect("a meta serializes");
        assert!(
            json.get("last_used_ms").is_none(),
            "an unused token carries no last_used_ms: {json}"
        );
        std::fs::remove_dir_all(&data).ok();
    }

    /// Every use updates memory; the file is rewritten only once the persisted value is a minute
    /// old, so a busy client does not rewrite the credential file on every request.
    #[test]
    fn a_use_is_remembered_every_time_and_written_once_a_minute() {
        let data = tempdir("last-used-throttle");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");
        let (meta, plaintext) = tokens
            .create(draft("laptop", &device_id(), &["files.read"]), true, 1_000)
            .expect("a valid token");
        let hash = hash_token(&plaintext);

        assert!(tokens.mark_used(&hash, 10_000), "a known token is marked");
        assert_eq!(
            tokens.list()[0].last_used_ms,
            Some(10_000),
            "the first use is remembered"
        );
        assert_eq!(
            Tokens::load(&data).expect("the file loads").list()[0].last_used_ms,
            Some(10_000),
            "the first use is written: there was no previous value"
        );

        // Inside the minute the memory fact moves and the file still holds the first write.
        assert!(tokens.mark_used(&hash, 40_000));
        assert_eq!(tokens.list()[0].last_used_ms, Some(40_000));
        assert_eq!(
            Tokens::load(&data).expect("the file loads").list()[0].last_used_ms,
            Some(10_000),
            "a use inside the window does not rewrite the file"
        );

        // The window is measured from what was persisted, not from the last in-memory use.
        assert!(tokens.mark_used(&hash, 70_000));
        assert_eq!(tokens.list()[0].last_used_ms, Some(70_000));
        let reloaded = Tokens::load(&data).expect("the file loads");
        assert_eq!(
            reloaded.list()[0].last_used_ms,
            Some(70_000),
            "the use after the window writes the newest value"
        );
        assert_eq!(reloaded.list()[0].id, meta.id, "the record is the same one");
        std::fs::remove_dir_all(&data).ok();
    }

    /// A use of a hash this store does not hold changes no record and writes nothing.
    #[test]
    fn marking_an_unknown_hash_returns_false_and_touches_nothing() {
        let data = tempdir("last-used-unknown");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");
        let (_meta, plaintext) = tokens
            .create(draft("laptop", &device_id(), &["files.read"]), true, 1_000)
            .expect("a valid token");
        let before = std::fs::read(data.join(TOKENS_FILE)).expect("the file is readable");
        let list_before = tokens.list();

        let marked = tokens.mark_used(&"00".repeat(32), 50_000);

        assert!(!marked, "an unknown hash marks nothing");
        assert_eq!(tokens.list(), list_before, "no record changed");
        assert_eq!(
            std::fs::read(data.join(TOKENS_FILE)).expect("the file is readable"),
            before,
            "nothing was written"
        );
        assert!(
            tokens
                .find_by_hash(&hash_token(&plaintext))
                .expect("the token is there")
                .last_used_ms
                .is_none(),
            "the known token was not marked by someone else's hash"
        );
        std::fs::remove_dir_all(&data).ok();
    }

    /// A file this build wrote before the field existed loads: an absent value is `None`, and the
    /// first use after the upgrade writes it.
    #[test]
    fn an_old_file_without_last_used_loads_with_none() {
        let data = tempdir("last-used-old-file");
        let path = data.join(TOKENS_FILE);
        let hash = "11".repeat(32);
        let old = serde_json::json!([{
            "id": "aa".repeat(32),
            "name": "laptop",
            "device_id": device_id(),
            "tools": ["files.read"],
            "allow_destructive": false,
            "created_at_ms": 1_000,
            "hash": hash,
        }]);
        std::fs::write(&path, old.to_string()).expect("the fixture is written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("the fixture is private");
        }

        let mut tokens = Tokens::load(&data).expect("an old file still loads");

        assert_eq!(tokens.list().len(), 1);
        assert_eq!(
            tokens.list()[0].last_used_ms,
            None,
            "the absent field reads as never used"
        );
        assert!(tokens.mark_used(&hash, 20_000));
        assert_eq!(
            Tokens::load(&data).expect("the file reloads").list()[0].last_used_ms,
            Some(20_000),
            "the first post-upgrade use is persisted, because there was nothing to throttle"
        );
        std::fs::remove_dir_all(&data).ok();
    }

    /// A failed write is not a failed call: memory keeps the fact, the file keeps the old value, and
    /// the next due use tries again.
    #[cfg(unix)]
    #[test]
    fn a_failed_last_use_write_keeps_the_memory_fact() {
        use std::os::unix::fs::PermissionsExt;

        let data = tempdir("last-used-write-failure");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");
        let (_meta, plaintext) = tokens
            .create(draft("laptop", &device_id(), &["files.read"]), true, 1_000)
            .expect("a valid token");
        let hash = hash_token(&plaintext);

        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o500))
            .expect("the directory is made read-only");
        let marked = tokens.mark_used(&hash, 30_000);
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700))
            .expect("the directory is writable again");

        assert!(marked, "a save failure does not fail the use");
        assert_eq!(
            tokens.list()[0].last_used_ms,
            Some(30_000),
            "the in-memory fact stays"
        );
        assert_eq!(
            Tokens::load(&data).expect("the file loads").list()[0].last_used_ms,
            None,
            "the file was not written"
        );
        std::fs::remove_dir_all(&data).ok();
    }

    /// A write that fails leaves memory exactly as the file still is.
    #[cfg(unix)]
    #[test]
    fn a_failed_write_leaves_the_previous_list_serving() {
        use std::os::unix::fs::PermissionsExt;

        let data = tempdir("write-failure");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");
        let (meta, _) = tokens
            .create(draft("first", &device_id(), &["files.read"]), true, 1_000)
            .expect("a valid token");

        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o500))
            .expect("the directory is made read-only");
        let refused = tokens.create(draft("second", &device_id(), &["files.read"]), true, 2_000);
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700))
            .expect("the directory is writable again");

        assert_eq!(refused.err(), Some(TokenError::Storage));
        assert_eq!(tokens.list(), vec![meta.clone()], "memory is unchanged");
        assert_eq!(
            Tokens::load(&data).expect("the file still loads").list(),
            vec![meta],
            "and the file still holds the first token"
        );
        std::fs::remove_dir_all(&data).ok();
    }

    /// The window is inclusive at both ends, one second outside is a typed refusal, and the
    /// computed time saturates instead of wrapping.
    #[test]
    fn the_expiry_window_is_one_hour_to_a_year_and_its_edges_are_typed() {
        let data = tempdir("expiry-bounds");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");
        let now = 1_000;

        let mut shortest = draft("shortest", &device_id(), &["files.read"]);
        shortest.expires_in_s = Some(MIN_TOKEN_EXPIRY_S);
        let (shortest_meta, _) = tokens
            .create(shortest, true, now)
            .expect("one hour is allowed");
        assert_eq!(
            shortest_meta.expires_at_ms,
            Some(now + MIN_TOKEN_EXPIRY_S * 1_000),
            "the lifetime is stored as an absolute time"
        );

        let mut longest = draft("longest", &device_id(), &["files.read"]);
        longest.expires_in_s = Some(MAX_TOKEN_EXPIRY_S);
        let (longest_meta, _) = tokens
            .create(longest, true, now)
            .expect("a year is allowed");
        assert_eq!(
            longest_meta.expires_at_ms,
            Some(now + MAX_TOKEN_EXPIRY_S * 1_000),
            "the longest lifetime is inclusive"
        );

        let mut too_short = draft("too-short", &device_id(), &["files.read"]);
        too_short.expires_in_s = Some(MIN_TOKEN_EXPIRY_S - 1);
        assert_eq!(
            tokens.create(too_short, true, now).err(),
            Some(TokenError::ExpiryTooShort),
            "one second under an hour is a unit error"
        );

        let mut too_long = draft("too-long", &device_id(), &["files.read"]);
        too_long.expires_in_s = Some(MAX_TOKEN_EXPIRY_S + 1);
        assert_eq!(
            tokens.create(too_long, true, now).err(),
            Some(TokenError::ExpiryTooLong),
            "one second over a year is a unit error"
        );

        assert_eq!(
            tokens.list().len(),
            2,
            "a refused window mints nothing: the two accepted tokens are the store"
        );

        let mut saturated = draft("saturated", &device_id(), &["files.read"]);
        saturated.expires_in_s = Some(MIN_TOKEN_EXPIRY_S);
        let (saturated_meta, _) = tokens
            .create(saturated, true, u64::MAX)
            .expect("a clock at the end of time still mints");
        assert_eq!(
            saturated_meta.expires_at_ms,
            Some(u64::MAX),
            "now + lifetime saturates instead of wrapping"
        );
        std::fs::remove_dir_all(&data).ok();
    }

    /// No lifetime is the default and stays permanent: the record, the list and the file all say
    /// nothing rather than invent an expiry.
    #[test]
    fn a_create_without_an_expiry_is_permanent() {
        let data = tempdir("expiry-none");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");

        let (meta, _) = tokens
            .create(draft("laptop", &device_id(), &["files.read"]), true, 1_000)
            .expect("a valid token");

        assert_eq!(meta.expires_at_ms, None, "an absent lifetime is permanent");
        let listed = serde_json::to_value(&tokens.list()[0]).expect("a meta serializes");
        assert!(
            listed.get("expires_at_ms").is_none(),
            "the list omits the absent fact: {listed}"
        );
        let text = std::fs::read_to_string(data.join(TOKENS_FILE)).expect("the file is readable");
        assert!(
            !text.contains("expires_at_ms"),
            "the file omits the absent fact: {text}"
        );

        let reloaded = Tokens::load(&data).expect("the store reloads");
        assert_eq!(
            reloaded.list()[0].expires_at_ms,
            None,
            "a permanent token is still permanent after a restart"
        );
        std::fs::remove_dir_all(&data).ok();
    }

    /// A file this build wrote before the field existed loads as permanent: no migration turns the
    /// operator's existing tokens into expiring ones.
    #[test]
    fn an_old_file_without_an_expiry_loads_permanent() {
        let data = tempdir("expiry-old-file");
        let path = data.join(TOKENS_FILE);
        let old = serde_json::json!([{
            "id": "aa".repeat(32),
            "name": "laptop",
            "device_id": device_id(),
            "tools": ["files.read"],
            "allow_destructive": false,
            "created_at_ms": 1_000,
            "hash": "11".repeat(32),
        }]);
        std::fs::write(&path, old.to_string()).expect("the fixture is written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("the fixture is private");
        }

        let tokens = Tokens::load(&data).expect("an old file still loads");

        assert_eq!(tokens.list().len(), 1, "the record is served");
        assert_eq!(
            tokens.list()[0].expires_at_ms,
            None,
            "the absent field reads as no expiry"
        );
        assert!(
            path.exists(),
            "an old file is not quarantined for lacking the field"
        );
        std::fs::remove_dir_all(&data).ok();
    }

    /// Rotation carries the lifetime policy, not the old absolute date: a thirty-day-old token
    /// created with ninety days rotates to now + ninety days, and a permanent token stays permanent.
    #[test]
    fn rotation_keeps_the_lifetime_policy() {
        let data = tempdir("rotate-expiry");
        let mut tokens = Tokens::load(&data).expect("an empty store loads");
        const DAY_MS: u64 = 86_400_000;
        const DAY_S: u64 = 86_400;

        let mut thirty = draft("thirty", &device_id(), &["files.read"]);
        thirty.expires_in_s = Some(30 * DAY_S);
        let (thirty_meta, _) = tokens.create(thirty, true, 1_000).expect("a valid token");
        assert_eq!(thirty_meta.expires_at_ms, Some(1_000 + 30 * DAY_MS));

        let mut ninety = draft("ninety", &device_id(), &["files.read"]);
        ninety.expires_in_s = Some(90 * DAY_S);
        let (ninety_meta, _) = tokens.create(ninety, true, 1_000).expect("a valid token");
        assert_eq!(ninety_meta.expires_at_ms, Some(1_000 + 90 * DAY_MS));

        let (permanent_meta, _) = tokens
            .create(
                draft("permanent", &device_id(), &["files.read"]),
                true,
                1_000,
            )
            .expect("a valid token");
        assert_eq!(permanent_meta.expires_at_ms, None);

        // Thirty days later the old expiry is irrelevant: the policy is the duration it was given.
        let rotated_at = 1_000 + 30 * DAY_MS;
        let (rotated_thirty, _) = tokens
            .rotate(&thirty_meta.id, rotated_at)
            .expect("the write succeeds")
            .expect("the id exists");
        assert_eq!(rotated_thirty.created_at_ms, rotated_at);
        assert_eq!(
            rotated_thirty.expires_at_ms,
            Some(rotated_at + 30 * DAY_MS),
            "a thirty-day policy stays thirty days from the rotation"
        );

        let (rotated_ninety, _) = tokens
            .rotate(&ninety_meta.id, rotated_at)
            .expect("the write succeeds")
            .expect("the id exists");
        assert_eq!(
            rotated_ninety.expires_at_ms,
            Some(rotated_at + 90 * DAY_MS),
            "a ninety-day policy stays ninety days, not the old absolute date"
        );

        let (rotated_permanent, _) = tokens
            .rotate(&permanent_meta.id, rotated_at)
            .expect("the write succeeds")
            .expect("the id exists");
        assert_eq!(
            rotated_permanent.expires_at_ms, None,
            "a permanent token stays permanent"
        );

        let reloaded = Tokens::load(&data).expect("the store reloads");
        assert_eq!(reloaded.list().len(), 3);
        let disk = std::fs::read_to_string(data.join(TOKENS_FILE)).expect("the file is readable");
        let records: Vec<serde_json::Value> =
            serde_json::from_str(&disk).expect("the file is a JSON list");
        let ninety_record = records
            .iter()
            .find(|record| record["id"] == serde_json::json!(rotated_ninety.id))
            .expect("the rotated ninety-day record is on disk");
        assert_eq!(
            ninety_record["expires_at_ms"],
            serde_json::json!(rotated_at + 90 * DAY_MS),
            "the policy reaches the file: {ninety_record}"
        );
        std::fs::remove_dir_all(&data).ok();
    }
}
