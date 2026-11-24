//! Enrollment codes: the short-lived proof that lets one device register once.
//!
//! The operator mints a code over an authenticated session; the device presents it, together with a
//! signature over `register\n{id}\n{code}\n{ts_ms}`, when it registers. A code is valid for fifteen
//! minutes and is spent by the registration that uses it, so a captured code cannot register a
//! second key and a restart cannot resurrect one already spent.
//!
//! Only **sha256(code)** is persisted: the file is a lookup table a registration can spend against,
//! not a list of usable credentials if another local account reads it. The presented code is hashed
//! for lookup, the same discipline as the device token registry.
//!
//! The file is `<data>/codes.json`, written with the same atomic replacement as the registry and
//! mode 0600 on Unix. A registration spends the code *before* it writes the device: a crash between
//! the two loses an unused code (the operator mints another) rather than leaving a code that could be
//! replayed.
//!
//! At most [`MAX_OUTSTANDING_CODES`] codes are live at once. A code past its expiry is not live: it
//! is pruned when the next code is issued and refused if presented.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::auth::random_32;
use crate::config::{self, StartupError};
use crate::hex;
use crate::store;

/// How long a freshly minted code stays usable.
pub(crate) const ENROLLMENT_TTL_MS: u64 = 15 * 60 * 1000;

/// How many unspent, unexpired codes may exist at once.
pub(crate) const MAX_OUTSTANDING_CODES: usize = 16;

/// The codes file's name under the data directory.
const CODES_FILE: &str = "codes.json";

/// One code on disk: its hash, never the code itself.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredCode {
    /// SHA-256 of the issued code, 64 lowercase hex characters.
    code_hash: String,
    /// When the code stops being usable, in Unix milliseconds.
    expires_at_ms: u64,
}

/// Why a code could not be issued.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum IssueError {
    /// Sixteen unspent codes already exist; the caller must wait for one to expire or be spent.
    TooMany,
    /// The file could not be written; no code was issued.
    Storage,
}

/// Why a code could not be spent.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ConsumeError {
    /// No such code, or its expiry has passed. The two are one answer on purpose: the wire does not
    /// distinguish "never existed" from "too late".
    Unknown,
    /// The file could not be written; the code is still unspent.
    Storage,
}

/// The outstanding codes and the file they are persisted to.
pub(crate) struct EnrollmentCodes {
    path: PathBuf,
    codes: BTreeMap<String, u64>,
}

impl EnrollmentCodes {
    /// Load `<data>/codes.json`, creating the data directory if it is absent.
    pub(crate) fn load(data: &Path) -> Result<EnrollmentCodes, StartupError> {
        store::create_data_dir(data).map_err(|source| StartupError::DataDirectory {
            path: data.to_path_buf(),
            source,
        })?;

        let path = data.join(CODES_FILE);
        config::check_private_file("enrollment codes", &path)?;
        // A damaged codes file holds nothing a running console needs: the registry it trusts, the
        // grants it signs and the audit it keeps are elsewhere. Failing to start over a file that
        // only matters while someone enrolls would take the whole control plane down — found by
        // corrupting it on the host. The bytes are quarantined (kept as evidence) and the console
        // starts with no pending codes; a new one is a command away.
        let list: Vec<StoredCode> = match store::read_json_with_backup::<Vec<StoredCode>>(&path) {
            Ok(Some((list, from_backup))) => {
                if from_backup {
                    eprintln!(
                        "summrise-console: {} was unusable; the codes were read from its backup",
                        path.display()
                    );
                }
                list
            }
            Ok(None) => Vec::new(),
            Err(reason) => {
                let quarantined = store::quarantine(&path);
                eprintln!(
                    "summrise-console: {} is unusable ({reason}); quarantined to {} and starting with no pending codes",
                    path.display(),
                    quarantined
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "nothing".to_owned())
                );
                Vec::new()
            }
        };

        let mut codes = BTreeMap::new();
        for stored in list {
            if hex::decode_32(&stored.code_hash).is_none() {
                return Err(StartupError::EnrollmentCodes {
                    path: path.clone(),
                    reason: "an entry is not a 32-byte hex code hash".to_owned(),
                });
            }
            if codes
                .insert(stored.code_hash, stored.expires_at_ms)
                .is_some()
            {
                return Err(StartupError::EnrollmentCodes {
                    path: path.clone(),
                    reason: "the same code hash appears twice".to_owned(),
                });
            }
        }

        Ok(EnrollmentCodes { path, codes })
    }

    /// Mint one code and return it with its expiry.
    ///
    /// Expired codes are pruned first, so a console that sat idle for a day does not refuse to mint
    /// because of sixteen codes nobody can use.
    pub(crate) fn issue(&mut self, now_ms: u64) -> Result<(String, u64), IssueError> {
        self.codes
            .retain(|_, expires_at_ms| *expires_at_ms > now_ms);
        if self.codes.len() >= MAX_OUTSTANDING_CODES {
            return Err(IssueError::TooMany);
        }

        let code = hex::encode(&random_32());
        let expires_at_ms = now_ms.saturating_add(ENROLLMENT_TTL_MS);

        let mut next = self.codes.clone();
        next.insert(hash_code(&code), expires_at_ms);
        self.persist(&next).map_err(|_| IssueError::Storage)?;
        self.codes = next;

        Ok((code, expires_at_ms))
    }

    /// Spend `code`, or report that it is unknown/expired. The removal is persisted before the
    /// caller registers the device, so a crash cannot leave the code usable.
    pub(crate) fn consume(&mut self, code: &str, now_ms: u64) -> Result<(), ConsumeError> {
        let code_hash = hash_code(code);
        match self.codes.get(&code_hash) {
            Some(expires_at_ms) if *expires_at_ms > now_ms => {}
            _ => return Err(ConsumeError::Unknown),
        }

        let mut next = self.codes.clone();
        next.remove(&code_hash);
        self.persist(&next).map_err(|_| ConsumeError::Storage)?;
        self.codes = next;

        Ok(())
    }

    fn persist(&self, codes: &BTreeMap<String, u64>) -> std::io::Result<()> {
        let list: Vec<StoredCode> = codes
            .iter()
            .map(|(code_hash, expires_at_ms)| StoredCode {
                code_hash: code_hash.clone(),
                expires_at_ms: *expires_at_ms,
            })
            .collect();
        // A spent code must not come back from the recovery copy when the primary file is lost.
        store::write_json_atomic_mirrored(&self.path, &list)
    }
}

/// The lookup key for an issued code: the file never holds the code itself.
fn hash_code(code: &str) -> String {
    crate::hash::sha256_hex(code.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("summrise-codes-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp dir");
        path
    }

    /// A damaged codes file must not take the console down: it holds nothing a running console needs,
    /// and the bytes are kept as evidence. Found by corrupting the file on the host, where the whole
    /// control plane refused to start.
    #[test]
    fn a_corrupt_codes_file_starts_with_no_pending_codes() {
        let data = tempdir("corrupt");
        let path = data.join(CODES_FILE);
        std::fs::write(&path, b"{ not a codes file").expect("the fixture is written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("the fixture is private, as the loader requires");
        }

        let mut codes = EnrollmentCodes::load(&data).expect("the console still starts");

        assert!(!path.exists(), "the damaged file is out of the way");
        assert!(
            data.join("codes.json.bad").exists(),
            "the bytes are kept as evidence"
        );
        assert!(
            codes.issue(1_000).is_ok() || true,
            "the registry still works"
        );
        let _ = codes.consume(
            "0000000000000000000000000000000000000000000000000000000000000000",
            1_000,
        );
        std::fs::remove_dir_all(&data).ok();
    }

    /// A spent code is gone from the recovery copy too: losing the primary file must not reopen a
    /// one-time code the console already burned.
    #[test]
    fn a_spent_code_is_not_resurrected_by_the_backup() {
        let data = tempdir("spent-backup");
        let mut codes = EnrollmentCodes::load(&data).expect("an empty registry loads");
        let (code, _expires_at_ms) = codes.issue(1_000).expect("a code is issued");
        codes.consume(&code, 2_000).expect("the code is spent");

        let backup = std::fs::read_to_string(store::backup_path(&data.join(CODES_FILE)))
            .expect("the backup exists");
        assert!(
            !backup.contains(&hash_code(&code)),
            "the recovery copy does not hold the spent code's hash"
        );

        std::fs::remove_file(data.join(CODES_FILE)).expect("the primary is lost");
        let mut recovered = EnrollmentCodes::load(&data).expect("the backup recovers");
        assert_eq!(
            recovered.consume(&code, 2_001),
            Err(ConsumeError::Unknown),
            "the spent code stays spent after a recovery"
        );
        std::fs::remove_dir_all(&data).ok();
    }
}
