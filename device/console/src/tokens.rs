//! Permanent API tokens: the console-issued credential an AI client keeps in a fixed MCP
//! configuration.
//!
//! A token is `srm_` plus 32 bytes of OS randomness, in lowercase hex. It is shown **once**, in the
//! create response, and only its SHA-256 is persisted, so the file (or a copy of it) never holds a
//! usable credential. A client presents the token as its bearer on `/mcp/:id`; the console finds the
//! record by hash, refuses a token that names another device, and signs a sixty-second grant with
//! the token's tools and destructive consent. The device protocol and its offline verification are
//! unchanged, because the device only ever sees a grant.
//!
//! A token is a bearer with **no expiry**: revocation is the withdrawal, and it removes the record.
//! The scope is fixed at creation — one device, a non-empty set of registry tools, and the operator's
//! destructive consent — so a token can never name a tool or a device the operator did not.
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

/// The prefix every permanent token carries, so a bearer is recognized without a second field.
pub(crate) const TOKEN_PREFIX: &str = "srm_";

/// How many hex characters follow the prefix: 32 bytes.
pub(crate) const TOKEN_HEX_CHARS: usize = 64;

/// The full length of a token: the prefix plus its 32 bytes.
pub(crate) const TOKEN_LEN: usize = TOKEN_PREFIX.len() + TOKEN_HEX_CHARS;

/// How long the grant minted for one token request lives.
///
/// Sixty seconds is [`crate::grants::MIN_GRANT_LIFETIME_S`], the shortest a grant may live: the point
/// of a permanent token is that the *token* is long-lived, not the grant it is exchanged for.
pub(crate) const MINTED_GRANT_LIFETIME_S: u64 = 60;

/// The store's file name under the data directory.
const TOKENS_FILE: &str = "tokens.json";

/// One permanent token as it is stored: the public facts plus the hash of the secret.
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
}

/// A request to mint one token, after the operator has filled a form.
pub(crate) struct Draft {
    pub(crate) name: String,
    pub(crate) device_id: String,
    pub(crate) tools: Vec<String>,
    pub(crate) allow_destructive: bool,
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
            hash: hash_token(&plaintext),
        };
        let meta = token.meta();

        let mut next = self.tokens.clone();
        next.insert(token.id.clone(), token);
        self.save(&next).map_err(|_| TokenError::Storage)?;
        self.tokens = next;

        Ok((meta, plaintext))
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
    for token in list {
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
}
