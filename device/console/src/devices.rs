//! The device registry.
//!
//! A device's id **is** the hex of its Ed25519 public key — self-certifying, as in the mesh. That
//! proves *what* the key is, not *who may register it*, so registration is bound by two more things:
//! a one-time enrollment code minted by the operator, and a signature by the key over
//! `register\n{id}\n{code}\n{ts_ms}`. The proof is checked in `http`; this module stores the result.
//!
//! Registration also issues the token the device presents on its channel. Only the token's SHA-256
//! is persisted, so the registry file (and a copy of it) never contains a usable credential.
//!
//! The file is `<data>/devices.json`, written whole through a temporary file and a rename so a crash
//! leaves either the previous registry or the new one, never a half-written list. A C1-era file with
//! no `token_hash` still loads; such a device simply cannot open a channel until it registers again.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};

use crate::auth::constant_time_eq;
use crate::config::{self, StartupError};
use crate::hex;
use crate::store;

/// The registry's file name under the data directory.
const REGISTRY_FILE: &str = "devices.json";

/// One registered device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// The device's id: the lowercase hex of `public_key`.
    pub id: String,
    /// The 32-byte Ed25519 public key, in hex.
    pub public_key: String,
    /// The name the device registered, if it gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// When the device first registered, in Unix milliseconds.
    pub registered_at_ms: u64,
    /// When the console last heard from the device, in Unix milliseconds.
    pub last_seen_ms: u64,
}

/// One device as it is stored: the public facts plus the hash of its channel token.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredDevice {
    #[serde(flatten)]
    device: Device,
    /// SHA-256 of the device token, in hex. Absent for a device that registered before tokens
    /// existed (C1); such a device has to register again to use the channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    token_hash: Option<String>,
}

/// Why a registration was refused.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RegisterError {
    /// `public_key` is not 32 bytes of valid Ed25519 key material.
    InvalidPublicKey,
    /// `id` is not the hex of `public_key`.
    IdMismatch,
    /// The stored registry disagrees with itself about this id.
    Conflict,
    /// The registry could not be written; the in-memory list is unchanged.
    Storage,
}

/// The in-memory registry and the file it is persisted to.
pub(crate) struct DeviceRegistry {
    path: PathBuf,
    devices: BTreeMap<String, StoredDevice>,
}

impl DeviceRegistry {
    /// Load `<data>/devices.json`, creating the data directory if it is absent.
    pub(crate) fn load(data: &Path) -> Result<DeviceRegistry, StartupError> {
        store::create_data_dir(data).map_err(|source| StartupError::DataDirectory {
            path: data.to_path_buf(),
            source,
        })?;

        let path = data.join(REGISTRY_FILE);
        config::check_private_file("device registry", &path)?;
        let read = store::read_json_with_backup::<Vec<StoredDevice>>(&path).map_err(|reason| {
            StartupError::Registry {
                path: path.clone(),
                reason,
            }
        })?;
        let list: Vec<StoredDevice> = match read {
            Some((list, from_backup)) => {
                if from_backup {
                    eprintln!(
                        "summrise-console: {} was unusable; the registry was read from its backup",
                        path.display()
                    );
                }
                list
            }
            None => Vec::new(),
        };

        let mut devices = BTreeMap::new();
        for stored in list {
            validate(&stored).map_err(|reason| StartupError::Registry {
                path: path.clone(),
                reason,
            })?;
            if devices.insert(stored.device.id.clone(), stored).is_some() {
                return Err(StartupError::Registry {
                    path: path.clone(),
                    reason: "the same id appears twice".to_owned(),
                });
            }
        }

        Ok(DeviceRegistry { path, devices })
    }

    /// Register a key and its channel token, or record that a known key was seen again.
    ///
    /// The write happens before the in-memory list is replaced, so a failed write leaves the
    /// registry serving exactly what is still on disk.
    pub(crate) fn register(
        &mut self,
        id: &str,
        public_key: &str,
        name: Option<String>,
        token: &[u8; 32],
        now_ms: u64,
    ) -> Result<Device, RegisterError> {
        let key = parse_public_key(public_key).ok_or(RegisterError::InvalidPublicKey)?;
        let presented_id = hex::decode_32(id).ok_or(RegisterError::IdMismatch)?;
        if presented_id != key {
            return Err(RegisterError::IdMismatch);
        }

        let canonical = hex::encode(&key);
        let device = match self.devices.get(&canonical) {
            Some(existing) => {
                if existing.device.public_key != canonical {
                    return Err(RegisterError::Conflict);
                }
                Device {
                    id: canonical.clone(),
                    public_key: canonical,
                    name: name.or_else(|| existing.device.name.clone()),
                    registered_at_ms: existing.device.registered_at_ms,
                    last_seen_ms: now_ms,
                }
            }
            None => Device {
                id: canonical.clone(),
                public_key: canonical,
                name,
                registered_at_ms: now_ms,
                last_seen_ms: now_ms,
            },
        };

        let stored = StoredDevice {
            device: device.clone(),
            token_hash: Some(hash_token(token)),
        };
        let mut next = self.devices.clone();
        next.insert(device.id.clone(), stored);
        self.persist(&next).map_err(|_| RegisterError::Storage)?;
        self.devices = next;

        Ok(device)
    }

    /// Forget a device: its registration and the token hash that admits it.
    ///
    /// `false` means no such device. The file is written before memory changes, so a failed write
    /// leaves the registry admitting exactly what it admitted before.
    pub(crate) fn remove(&mut self, id: &str) -> Result<bool, RegisterError> {
        if !self.devices.contains_key(id) {
            return Ok(false);
        }
        let mut next = self.devices.clone();
        next.remove(id);
        self.persist(&next).map_err(|_| RegisterError::Storage)?;
        self.devices = next;
        Ok(true)
    }

    /// Give a device the name the operator knows it by, or clear it.
    ///
    /// The name is the console's label, not the device's claim: a machine that announces
    /// `DESKTOP-14RJCR8` can be recorded as `studio`. `None` clears it, which lets the announced name
    /// show through again. The write happens before the in-memory list changes, so a failed write
    /// leaves the registry serving what is still on disk.
    pub(crate) fn rename(
        &mut self,
        id: &str,
        name: Option<String>,
    ) -> Result<Option<Device>, RegisterError> {
        let Some(existing) = self.devices.get(id).cloned() else {
            return Ok(None);
        };
        let mut next = self.devices.clone();
        let updated = Device {
            name,
            ..existing.device.clone()
        };
        next.insert(
            id.to_owned(),
            StoredDevice {
                device: updated.clone(),
                token_hash: existing.token_hash.clone(),
            },
        );
        self.persist(&next).map_err(|_| RegisterError::Storage)?;
        self.devices = next;
        Ok(Some(updated))
    }

    /// Every device, ordered by id so the wire list and the file are stable.
    pub(crate) fn list(&self) -> Vec<Device> {
        self.devices
            .values()
            .map(|stored| stored.device.clone())
            .collect()
    }

    /// Record that the device opened a channel, so the operator's list can say when it was last
    /// heard. In memory only: the file keeps the registration, and a console restart shows the
    /// registration time again rather than inventing a heartbeat it did not persist.
    /// One device's record, by id.
    pub(crate) fn get(&self, id: &str) -> Option<&Device> {
        self.devices.get(id).map(|stored| &stored.device)
    }

    pub(crate) fn touch(&mut self, id: &str, now_ms: u64) {
        if let Some(stored) = self.devices.get_mut(id) {
            stored.device.last_seen_ms = now_ms;
        }
    }

    /// How many devices are registered.
    pub(crate) fn len(&self) -> usize {
        self.devices.len()
    }

    /// Whether a device is registered under this id.
    pub(crate) fn contains(&self, id: &str) -> bool {
        self.devices.contains_key(id)
    }

    /// Whether `token` is the channel token issued when this device registered.
    ///
    /// The comparison is over the token's hash and walks the whole value: a caller learns nothing
    /// from how many leading bytes it guessed.
    pub(crate) fn verify_token(&self, id: &str, token: &[u8; 32]) -> bool {
        let Some(stored) = self.devices.get(id) else {
            return false;
        };
        let Some(expected) = stored.token_hash.as_deref() else {
            return false;
        };
        constant_time_eq(hash_token(token).as_bytes(), expected.as_bytes())
    }
}

impl DeviceRegistry {
    fn persist(&self, devices: &BTreeMap<String, StoredDevice>) -> std::io::Result<()> {
        let list: Vec<&StoredDevice> = devices.values().collect();
        // The backup is a recovery copy of the **current** registry, never a history: a removed
        // device must not reappear when the primary is lost and the backup is read.
        store::write_json_atomic_mirrored(&self.path, &list)
    }
}

/// The verifying half of a device key, or `None` if the hex is not a valid Ed25519 point.
pub(crate) fn verifying_key(public_key: &str) -> Option<VerifyingKey> {
    let raw = hex::decode_32(public_key)?;
    VerifyingKey::from_bytes(&raw).ok()
}

fn parse_public_key(public_key: &str) -> Option<[u8; 32]> {
    Some(verifying_key(public_key)?.to_bytes())
}

fn hash_token(token: &[u8; 32]) -> String {
    crate::hash::sha256_hex(token)
}

fn validate(stored: &StoredDevice) -> Result<(), String> {
    let device = &stored.device;
    let key = parse_public_key(&device.public_key)
        .ok_or_else(|| format!("device '{}' has an invalid public key", device.id))?;
    if hex::decode_32(&device.id) != Some(key) {
        return Err(format!("device '{}' is not its public key", device.id));
    }
    if let Some(token_hash) = &stored.token_hash {
        if hex::decode_32(token_hash).is_none() {
            return Err(format!("device '{}' has an invalid token hash", device.id));
        }
    }
    Ok(())
}

#[cfg(test)]
mod remove_tests {
    use super::*;

    /// Revocation is a fact on disk: a console that restarts must not admit a device the operator
    /// removed, and removing one that is not there is a no-op, not an error.
    #[test]
    fn a_removed_device_is_gone_from_memory_and_from_the_file() {
        let dir = std::env::temp_dir().join(format!("summrise-remove-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let mut registry = DeviceRegistry::load(&dir).expect("an empty registry loads");
        let key = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
        let id = hex::encode(&key.verifying_key().to_bytes());
        registry
            .register(&id, &id, None, &[3u8; 32], 1_000)
            .expect("the device registers");

        assert!(registry.remove(&id).expect("the write succeeds"));
        assert!(DeviceRegistry::load(&dir)
            .expect("the registry reloads")
            .list()
            .is_empty());
        assert!(!registry.remove(&id).expect("a second removal is a no-op"));

        // The recovery copy is the current registry, not the pre-removal one: losing the primary
        // must not resurrect the revoked device.
        let backup = store::backup_path(&dir.join(REGISTRY_FILE));
        assert!(
            !std::fs::read_to_string(&backup)
                .unwrap_or_default()
                .contains(&id),
            "the backup does not name the removed device"
        );
        std::fs::remove_file(dir.join(REGISTRY_FILE)).expect("the primary is lost");
        assert!(
            DeviceRegistry::load(&dir)
                .expect("the backup recovers an empty registry")
                .list()
                .is_empty(),
            "a lost primary recovered from the backup does not admit the removed device"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod rename_tests {
    use super::*;

    /// The operator's label is the console's fact: it persists, it can be cleared, and clearing it
    /// lets the announced name show through again.
    #[test]
    fn a_device_can_be_named_and_unnamed() {
        let dir = std::env::temp_dir().join(format!("summrise-rename-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let mut registry = DeviceRegistry::load(&dir).expect("an empty registry loads");
        let key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let public = hex::encode(&key.verifying_key().to_bytes());
        let id = public.clone();
        registry
            .register(&id, &public, None, &[9u8; 32], 1_000)
            .expect("the device registers");

        let renamed = registry
            .rename(&id, Some("studio".to_owned()))
            .expect("the write succeeds")
            .expect("the device exists");
        assert_eq!(renamed.name.as_deref(), Some("studio"));
        assert_eq!(
            DeviceRegistry::load(&dir)
                .expect("the registry reloads")
                .list()[0]
                .name
                .as_deref(),
            Some("studio"),
            "the label is on disk, not only in memory"
        );

        let cleared = registry
            .rename(&id, None)
            .expect("the write succeeds")
            .expect("the device exists");
        assert_eq!(cleared.name, None);
        assert_eq!(
            registry
                .rename("00".repeat(32).as_str(), Some("x".to_owned()))
                .expect("no write"),
            None,
            "an unknown device is not created by a rename"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
