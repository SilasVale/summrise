//! Saved terminal connections: named presets an operator reuses per device.
//!
//! They are operator convenience, not device state: the console stores them, and the device never sees
//! one until a session is created from it. That is why they live here rather than on the device — a
//! device that changes owners must not inherit the previous owner's host names.
//!
//! **No credential is stored.** An SSH password is typed at use time; a key never leaves the machine
//! it is on. The preset names the shape of the connection (kind, host, port, user, serial framing,
//! and the *path* of a key the device already has) and nothing a stolen console file would let
//! somebody log in with.
//!
//! The file is `<data>/connections.json`, written whole through the store's atomic write, so a crash
//! leaves the previous list or the new one.

use std::path::{Path, PathBuf};

use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::config::StartupError;
use crate::store;

/// The file name under the data directory.
const FILE: &str = "connections.json";

/// How many presets one device may carry. A list longer than this is a list nobody reads.
const MAX_PER_DEVICE: usize = 32;

/// One saved connection, as the wire sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    /// A short random id, stable for the life of the preset.
    pub id: String,
    pub name: String,
    /// `pty`, `ssh` or `serial`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// The OS path of a serial port (`COM3`, `/dev/ttyUSB0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baud: Option<u32>,
    /// `kind: "ssh"`: an absolute path to an OpenSSH key **on the device**. Configuration, not a
    /// credential — the key never leaves the machine it is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
}

/// What the page sends when it saves a preset.
#[derive(Clone, Debug, Deserialize)]
pub struct Draft {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub port_path: Option<String>,
    #[serde(default)]
    pub baud: Option<u32>,
    #[serde(default)]
    pub key_path: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Stored {
    device_id: String,
    connection: Connection,
}

/// The saved connections, every device's in one file.
pub struct Connections {
    path: PathBuf,
    entries: Vec<Stored>,
}

impl Connections {
    /// Load the file, or start empty when there is none.
    pub fn load(data: &Path) -> Result<Connections, StartupError> {
        let path = data.join(FILE);
        let read = store::read_json_with_backup::<Vec<Stored>>(&path).map_err(|reason| {
            StartupError::Registry {
                path: path.clone(),
                reason,
            }
        })?;
        let entries = read.map(|(list, _)| list).unwrap_or_default();
        Ok(Connections { path, entries })
    }

    /// This device's presets, in the order they were saved.
    pub fn list(&self, device: &str) -> Vec<Connection> {
        self.entries
            .iter()
            .filter(|stored| stored.device_id == device)
            .map(|stored| stored.connection.clone())
            .collect()
    }

    /// Save one preset and return it.
    pub fn add(&mut self, device: &str, draft: Draft) -> Result<Connection, &'static str> {
        let connection = validated(draft)?;
        if self.list(device).len() >= MAX_PER_DEVICE {
            return Err("too many saved connections for this device");
        }
        self.entries.push(Stored {
            device_id: device.to_owned(),
            connection: connection.clone(),
        });
        self.persist()
            .map_err(|_| "the connections file could not be written")?;
        Ok(connection)
    }

    /// Remove one preset; `false` when this device has no such id.
    pub fn remove(&mut self, device: &str, id: &str) -> Result<bool, &'static str> {
        let before = self.entries.len();
        self.entries
            .retain(|stored| !(stored.device_id == device && stored.connection.id == id));
        if self.entries.len() == before {
            return Ok(false);
        }
        self.persist()
            .map_err(|_| "the connections file could not be written")?;
        Ok(true)
    }

    fn persist(&self) -> std::io::Result<()> {
        store::write_json_atomic(&self.path, &self.entries)
    }
}

/// Check the shape the kind implies, and give the preset an id.
///
/// The same rule the device applies, applied a step earlier: a preset that cannot become a session is
/// refused when it is saved, not when the operator is waiting for a prompt.
fn validated(draft: Draft) -> Result<Connection, &'static str> {
    if draft.kind != "ssh"
        && draft
            .key_path
            .as_deref()
            .is_some_and(|path| !path.trim().is_empty())
    {
        return Err("only an ssh preset takes a key file");
    }
    let name = draft.name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err("the name must be 1 to 64 printable characters");
    }
    let bounded = |value: &str, limit: usize| !value.is_empty() && value.chars().count() <= limit;
    let connection = match draft.kind.as_str() {
        "pty" => Connection {
            id: mint(),
            name: name.to_owned(),
            kind: "pty".to_owned(),
            host: None,
            port: None,
            user: None,
            port_path: None,
            baud: None,
            key_path: None,
        },
        "ssh" => {
            let host = draft
                .host
                .as_deref()
                .map(str::trim)
                .filter(|host| bounded(host, 255));
            let user = draft
                .user
                .as_deref()
                .map(str::trim)
                .filter(|user| bounded(user, 128));
            let Some(host) = host else {
                return Err("an ssh preset needs a host");
            };
            let Some(user) = user else {
                return Err("an ssh preset needs a user");
            };
            let Some(port) = draft.port.filter(|port| *port > 0) else {
                return Err("an ssh preset needs a port");
            };
            let key_path = draft
                .key_path
                .as_deref()
                .map(str::trim)
                .filter(|path| bounded(path, 260))
                .map(str::to_owned);
            Connection {
                id: mint(),
                name: name.to_owned(),
                kind: "ssh".to_owned(),
                host: Some(host.to_owned()),
                port: Some(port),
                user: Some(user.to_owned()),
                port_path: None,
                baud: None,
                key_path,
            }
        }
        "serial" => {
            let path = draft
                .port_path
                .as_deref()
                .map(str::trim)
                .filter(|path| bounded(path, 260));
            let Some(path) = path else {
                return Err("a serial preset needs a port path");
            };
            let baud = draft.baud.filter(|baud| (1..=4_000_000).contains(baud));
            Connection {
                id: mint(),
                name: name.to_owned(),
                kind: "serial".to_owned(),
                host: None,
                port: None,
                user: None,
                port_path: Some(path.to_owned()),
                baud,
                key_path: None,
            }
        }
        _ => return Err("the kind must be pty, ssh or serial"),
    };
    Ok(connection)
}

/// Eight random bytes as hex: short enough to sit in a URL, wide enough not to collide by accident.
fn mint() -> String {
    let mut bytes = [0u8; 8];
    OsRng.fill_bytes(&mut bytes);
    let mut text = String::with_capacity(16);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("summrise-connections-{}", mint()));
        std::fs::create_dir_all(&dir).expect("the temp directory");
        dir
    }

    fn ssh_draft() -> Draft {
        Draft {
            name: "the db host".to_owned(),
            kind: "ssh".to_owned(),
            host: Some("db.internal".to_owned()),
            port: Some(22),
            user: Some("operator".to_owned()),
            port_path: None,
            baud: None,
            key_path: None,
        }
    }

    /// A key's path is configuration and round-trips; the key itself never appears. Only ssh may
    /// name one, and the store is where that rule lives for every caller.
    #[test]
    fn a_keys_path_round_trips_and_only_ssh_may_carry_one() {
        let dir = temp();
        let key = "C:\\Users\\operator\\.ssh\\id_ed25519";
        {
            let mut connections = Connections::load(&dir).expect("loads empty");
            let mut draft = ssh_draft();
            draft.key_path = Some(key.to_owned());
            let saved = connections
                .add("device-a", draft)
                .expect("ssh with a key saves");
            assert_eq!(saved.key_path.as_deref(), Some(key));

            let pty = Draft {
                name: "shell".to_owned(),
                kind: "pty".to_owned(),
                host: None,
                port: None,
                user: None,
                port_path: None,
                baud: None,
                key_path: Some(key.to_owned()),
            };
            assert!(connections.add("device-a", pty).is_err());
        }
        let connections = Connections::load(&dir).expect("reloads");
        assert_eq!(
            connections.list("device-a")[0].key_path.as_deref(),
            Some(key)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_preset_round_trips_through_the_file() {
        let dir = temp();
        {
            let mut connections = Connections::load(&dir).expect("loads empty");
            connections.add("device-a", ssh_draft()).expect("ssh saves");
            connections
                .add(
                    "device-a",
                    Draft {
                        name: "console cable".to_owned(),
                        kind: "serial".to_owned(),
                        host: None,
                        port: None,
                        user: None,
                        port_path: Some("COM3".to_owned()),
                        baud: Some(115200),
                        key_path: None,
                    },
                )
                .expect("serial saves");
        }
        let connections = Connections::load(&dir).expect("reloads");
        let list = connections.list("device-a");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].host.as_deref(), Some("db.internal"));
        assert_eq!(list[1].port_path.as_deref(), Some("COM3"));
        assert!(connections.list("device-b").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_preset_is_scoped_to_its_device() {
        let dir = temp();
        let mut connections = Connections::load(&dir).expect("loads");
        let saved = connections.add("device-a", ssh_draft()).expect("saves");
        assert!(connections.list("device-b").is_empty());
        assert!(!connections.remove("device-b", &saved.id).expect("removes"));
        assert!(connections.remove("device-a", &saved.id).expect("removes"));
        assert!(connections.list("device-a").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_preset_that_cannot_become_a_session_is_refused() {
        let dir = temp();
        let mut connections = Connections::load(&dir).expect("loads");
        assert!(connections
            .add(
                "device-a",
                Draft {
                    name: "no host".to_owned(),
                    kind: "ssh".to_owned(),
                    host: None,
                    port: Some(22),
                    user: Some("operator".to_owned()),
                    port_path: None,
                    baud: None,
                    key_path: None,
                }
            )
            .is_err());
        assert!(connections
            .add(
                "device-a",
                Draft {
                    name: "no kind".to_owned(),
                    kind: "telnet".to_owned(),
                    host: None,
                    port: None,
                    user: None,
                    port_path: None,
                    baud: None,
                    key_path: None,
                }
            )
            .is_err());
        assert!(connections.list("device-a").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_credential_has_nowhere_to_go() {
        // The draft type is the promise: there is no field for a password or a key, so a page cannot
        // send one and this file cannot store one.
        let connection = validated(ssh_draft()).expect("valid");
        let serialised = serde_json::to_string(&connection).expect("serialises");
        assert!(!serialised.contains("password"), "{serialised}");
        assert!(!serialised.contains("key"), "{serialised}");
    }
}
