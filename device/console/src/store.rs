//! One atomic file replacement, shared by the two files the console persists.
//!
//! The console's JSON files are written whole through a sibling temporary file, synced, and renamed
//! into place. A reader therefore sees either the previous file or the complete new one,
//! never a half-written list. The rename replaces the destination on Unix and on Windows (Rust's
//! `fs::rename` uses `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` there).
//!
//! Both files are **0600 on Unix**: the registry holds token hashes and the codes file holds the
//! hashes a registration spends, and neither is for another local account's eyes. Windows has no
//! POSIX mode; there the ACL of the data directory is what keeps them private, and this module
//! deliberately does not pretend a mode it cannot set.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Serialize `value` and atomically replace `path` with it, keeping the previous file as `.bak`.
pub(crate) fn write_json_atomic<T>(path: &Path, value: &T) -> std::io::Result<()>
where
    T: Serialize + ?Sized,
{
    serialize_and_replace(path, value, true)
}

/// Serialize `value` and replace both `path` and its `.bak` with it.
///
/// A revocation or a spent one-time code must not survive in the recovery copy. The backup is
/// written **first**: if the primary write then fails, memory keeps the old state while the backup
/// already holds the new one — a recovery copy that is never older than the state the operator asked
/// for. (A recovery copy that lags would resurrect exactly what was withdrawn.)
pub(crate) fn write_json_atomic_mirrored<T>(path: &Path, value: &T) -> std::io::Result<()>
where
    T: Serialize + ?Sized,
{
    let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    write_atomic(&backup_path(path), &bytes)?;
    write_atomic(path, &bytes)
}

/// Serialize `value` and atomically replace `path` **without** a `.bak` copy.
///
/// Credential files use this. A backup of the token registry is the previous complete list, so the
/// save after a revocation would keep the withdrawn hash alive on disk; the token store never reads
/// a backup, and this makes sure there is not one to restore by hand either.
pub(crate) fn write_json_atomic_without_backup<T>(path: &Path, value: &T) -> std::io::Result<()>
where
    T: Serialize + ?Sized,
{
    serialize_and_replace(path, value, false)
}

fn serialize_and_replace<T>(path: &Path, value: &T, backup: bool) -> std::io::Result<()>
where
    T: Serialize + ?Sized,
{
    let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    // The registry is the console's memory of every device it trusts: one lost file would mean
    // re-enrolling every machine by hand. Keep the previous contents beside it before replacing.
    if backup && path.exists() {
        let _ = fs::copy(path, backup_path(path));
    }
    write_atomic(path, &bytes)
}

/// `<path>.bak`, the previous complete contents of `path`.
pub(crate) fn backup_path(path: &Path) -> std::path::PathBuf {
    path.with_extension("json.bak")
}

/// The one-time login code a console started with `--login-code` leaves for the running process.
pub(crate) const LOGIN_CODE_FILE: &str = "login.code";

/// A login code and the instant it stops working.
#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LoginCode {
    pub code: String,
    pub expires_at_ms: u64,
}

/// Write a login code for a running console to find, replacing any earlier one.
///
/// The code is a short-lived alternative to typing the operator token. It lives in the data
/// directory because the process that runs the console and the process that prints the link are two
/// different invocations; the file is the only thing they share.
pub(crate) fn write_login_code(data: &Path, code: &LoginCode) -> std::io::Result<PathBuf> {
    create_data_dir(data)?;
    let path = data.join(LOGIN_CODE_FILE);
    // No recovery copy: this file is a 256-bit one-time password with minutes to live, and a `.bak`
    // would keep a superseded or spent code on disk for no recovery anyone wants.
    write_json_atomic_without_backup(&path, code)?;
    Ok(path)
}

/// Spend a login code: `Ok(())` when it matched and is still valid, and the file is removed either
/// way, so a code works once and a stale one cannot be replayed.
pub(crate) fn spend_login_code(data: &Path, presented: &str, now_ms: u64) -> Result<(), String> {
    let path = data.join(LOGIN_CODE_FILE);
    let bytes = fs::read(&path).map_err(|_| "no login code is waiting".to_owned())?;
    let stored: LoginCode = serde_json::from_slice(&bytes)
        .map_err(|_| "the login code file is unreadable".to_owned())?;
    if stored.expires_at_ms <= now_ms {
        let _ = fs::remove_file(&path);
        return Err("the login code has expired".to_owned());
    }
    if !constant_time_eq_str(&stored.code, presented) {
        // A mismatch does not burn the link: a mistyped URL is not an attack, and the code is 256 bits
        // of randomness, not a password to guess.
        return Err("the login code does not match".to_owned());
    }
    let _ = fs::remove_file(&path);
    // Older builds wrote the code with a backup; removing it here cleans those up as the file is
    // spent, so no copy of a used code outlives the code itself.
    let _ = fs::remove_file(backup_path(&path));
    Ok(())
}

/// Compare two strings byte by byte without an early exit, so a caller learns nothing from timing.
fn constant_time_eq_str(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    let mut difference = (a.len() ^ b.len()) as u8;
    for index in 0..a.len().max(b.len()) {
        difference |= a.get(index).copied().unwrap_or(0) ^ b.get(index).copied().unwrap_or(0);
    }
    difference == 0
}

/// Move a file this build cannot read aside, keeping the bytes as evidence.
///
/// `None` means the rename failed; the caller still starts with an empty view, because a damaged
/// file must not be able to stop a process that does not need it.
pub(crate) fn quarantine(path: &Path) -> Option<std::path::PathBuf> {
    let mut name = path.as_os_str().to_owned();
    name.push(".bad");
    let quarantined = std::path::PathBuf::from(name);
    let _ = fs::remove_file(&quarantined);
    fs::rename(path, &quarantined).ok().map(|()| quarantined)
}

/// Read a JSON file, falling back to its `.bak` when the file is missing or unreadable.
///
/// `Ok(None)` means neither version exists (a first start). The fallback is not silent: the caller
/// is told which file answered, so a start that used the backup can say so.
pub(crate) fn read_json_with_backup<T: DeserializeOwned>(
    path: &Path,
) -> Result<Option<(T, bool)>, String> {
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<T>(&bytes) {
            Ok(value) => Ok(Some((value, false))),
            Err(error) => {
                let backup = backup_path(path);
                match fs::read(&backup) {
                    Ok(bytes) => serde_json::from_slice(&bytes)
                        .map(|value| Some((value, true)))
                        .map_err(|error| {
                            format!(
                                "{} is unreadable ({error}) and its backup {} is too",
                                path.display(),
                                backup.display()
                            )
                        }),
                    Err(_) => Err(format!("{} is unreadable: {error}", path.display())),
                }
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let backup = backup_path(path);
            match fs::read(&backup) {
                Ok(bytes) => serde_json::from_slice(&bytes)
                    .map(|value| Some((value, true)))
                    .map_err(|error| format!("{} is unreadable: {error}", backup.display())),
                Err(_) => Ok(None),
            }
        }
        Err(error) => Err(format!("{} is unreadable: {error}", path.display())),
    }
}

/// Create the data directory if it is absent.
///
/// On Unix it is created 0700: it holds the registry, the enrollment codes and the two indexes.
/// An existing directory keeps the mode its operator gave it; the files inside it are still 0600.
pub(crate) fn create_data_dir(data: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;

        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(data)
    }
    #[cfg(not(unix))]
    {
        // Windows relies on the ACLs of the install tree.
        fs::create_dir_all(data)
    }
}

/// Write `bytes` through `<path>.json.tmp`, sync it, and rename it over `path`.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension("json.tmp");

    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            // `mode` only applies when the temporary file is created; a leftover from an interrupted
            // write could have a looser mode, so the final file is pinned explicitly before the
            // rename.
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(bytes)?;
        file.sync_all()?;
    }

    fs::rename(&temporary, path)
}

#[cfg(test)]
mod login_code_tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("summrise-login-{}-{}", std::process::id(), tag));
        let _ = fs::remove_dir_all(&path);
        path
    }

    /// A login code is a one-shot secret: it works once, it expires, and a wrong one never works.
    #[test]
    fn a_login_code_is_spent_once_and_expires() {
        let data = tempdir("code");
        let code = LoginCode {
            code: "a".repeat(64),
            expires_at_ms: 2_000,
        };
        write_login_code(&data, &code).expect("the code is written");

        assert!(
            spend_login_code(&data, "b".repeat(64).as_str(), 1_000).is_err(),
            "a wrong code is refused"
        );
        assert_eq!(
            spend_login_code(&data, &"a".repeat(64), 1_000),
            Ok(()),
            "the right code signs in"
        );
        assert!(
            spend_login_code(&data, &"a".repeat(64), 1_000).is_err(),
            "and cannot be replayed"
        );

        write_login_code(&data, &code).expect("the code is written again");
        assert!(
            spend_login_code(&data, &"a".repeat(64), 3_000).is_err(),
            "an expired code is refused"
        );
        std::fs::remove_dir_all(&data).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
    struct Item {
        name: String,
    }

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("summrise-store-{}-{}", std::process::id(), tag));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp dir");
        path
    }

    /// The registry's memory survives a lost file: the previous complete contents are kept beside it
    /// and a reader that cannot use the file uses the backup, saying that it did.
    #[test]
    fn a_lost_registry_is_read_from_its_backup() {
        let dir = tempdir("backup");
        let path = dir.join("devices.json");
        write_json_atomic(
            &path,
            &vec![Item {
                name: "first".to_owned(),
            }],
        )
        .expect("first write");
        write_json_atomic(
            &path,
            &vec![Item {
                name: "second".to_owned(),
            }],
        )
        .expect("second write");
        assert_eq!(
            read_json_with_backup::<Vec<Item>>(&path).expect("read"),
            Some((
                vec![Item {
                    name: "second".to_owned()
                }],
                false
            )),
            "the file itself answers while it is readable"
        );

        fs::remove_file(&path).expect("the file is lost");
        let (list, from_backup) = read_json_with_backup::<Vec<Item>>(&path)
            .expect("the backup answers")
            .expect("a backup exists");
        assert!(from_backup, "the reader says which file answered");
        assert_eq!(
            list,
            vec![Item {
                name: "first".to_owned()
            }]
        );
    }

    #[test]
    fn no_file_and_no_backup_is_a_first_start_not_an_error() {
        let dir = tempdir("absent");
        assert_eq!(
            read_json_with_backup::<Vec<Item>>(&dir.join("devices.json")).expect("absent is fine"),
            None
        );
    }
}
