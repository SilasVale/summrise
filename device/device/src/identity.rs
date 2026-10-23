//! The device's mesh identity: `<data>/identity.key`.
//!
//! The mesh names a peer by its Ed25519 public key in hex, so the device needs one keypair that
//! outlives a restart: the file holds the 32-byte seed in hex, is minted on first use, and its
//! public half is this device's peer id. It is the same key a console grant must name, so a device
//! that has one identity has one name on both paths.
//!
//! The seed is the device's private key, and the file is created owner-only (0600 on Unix). A file
//! with a looser mode is refused by name rather than read: a world-readable private key is a
//! compromised identity, and this is the only place the device can say so. The refusal is loud
//! because the alternative — serving a mesh with a key anybody on the box can copy — is silent.
//!
//! Windows has no mode; the equivalent is a protected DACL granting `FILE_ALL_ACCESS` to SYSTEM and
//! Administrators only. `summrise_winsec` installs it before the seed is written and reads it back,
//! and the read path runs the same read-back before parsing the file, so both a file this build
//! minted and one copied in from elsewhere are held to the same rule (review C1).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use summrise_mesh::{keys, Identity};

/// The file the device's Ed25519 seed is kept in, under `<data>`.
pub(crate) const IDENTITY_FILE: &str = "identity.key";

/// The seed's length: an Ed25519 private key is 32 bytes.
const SEED_BYTES: usize = 32;

/// Why the identity file could not be used.
///
/// Every variant is a refusal: nothing here defaults to a usable key, because a device that
/// silently minted a second identity would stop being the peer the console registered.
#[derive(Debug)]
pub(crate) enum IdentityError {
    /// The file could not be read.
    Read {
        /// The path as configured, so the message names the file.
        path: PathBuf,
        /// The OS error.
        source: io::Error,
    },
    /// The file could not be created or written.
    Write {
        /// The path as configured.
        path: PathBuf,
        /// The OS error.
        source: io::Error,
    },
    /// The path exists but is not a regular file.
    NotAFile {
        /// The path as configured.
        path: PathBuf,
    },
    /// The contents are not one 32-byte seed in hex.
    Malformed {
        /// The path as configured.
        path: PathBuf,
    },
    /// The file is readable by more than its owner.
    ///
    /// Unix only: the variant exists exactly where the mode can be read, so a Windows build has no
    /// dead arm claiming a check it never makes.
    #[cfg(unix)]
    Insecure {
        /// The path as configured.
        path: PathBuf,
        /// The mode found, so the operator can compare it with 0600.
        mode: u32,
    },
    /// Windows could not install or read back the identity's DACL.
    ///
    /// A key whose ACL allows any principal but SYSTEM and Administrators is a compromised identity
    /// in exactly the sense the Unix mode check refuses, so this is a refusal, not a warning.
    #[cfg(windows)]
    Protect {
        /// The path as configured.
        path: PathBuf,
        /// The Win32 refusal, which already names the path.
        detail: String,
    },
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IdentityError::Read { path, source } => {
                write!(f, "cannot read the identity {}: {source}", path.display())
            }
            IdentityError::Write { path, source } => {
                write!(f, "cannot write the identity {}: {source}", path.display())
            }
            IdentityError::NotAFile { path } => {
                write!(f, "the identity {} is not a regular file", path.display())
            }
            IdentityError::Malformed { path } => write!(
                f,
                "the identity {} must be one 32-byte Ed25519 seed in hex (64 characters)",
                path.display()
            ),
            #[cfg(unix)]
            IdentityError::Insecure { path, mode } => write!(
                f,
                "the identity {} is mode {mode:04o}; it is this device's private key and must be 0600",
                path.display()
            ),
            #[cfg(windows)]
            IdentityError::Protect { path, detail } => write!(
                f,
                "the identity {} is not protected ({detail}); it is this device's private key \
                 and only SYSTEM and Administrators may read it",
                path.display()
            ),
        }
    }
}

impl std::error::Error for IdentityError {}

/// Load the device's identity, minting it when there is none.
///
/// The mint is exclusive (`create_new`): a file that appears between the read and the write wins,
/// because a key already published as this device's peer id must not be replaced by a second one.
pub(crate) fn load_or_mint(data: &Path) -> Result<Identity, IdentityError> {
    let path = data.join(IDENTITY_FILE);
    match read(&path)? {
        Some(identity) => Ok(identity),
        None => mint(&path),
    }
}

/// Read the identity the file holds, or `None` when there is no file yet.
fn read(path: &Path) -> Result<Option<Identity>, IdentityError> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(IdentityError::Read {
                path: path.to_path_buf(),
                source,
            })
        }
    };
    if !metadata.is_file() {
        return Err(IdentityError::NotAFile {
            path: path.to_path_buf(),
        });
    }
    // Before the bytes are read, not after: a private key that anybody on the box can read is
    // refused whether or not this process would have gone on to use it.
    enforce_owner_only(path, &metadata)?;
    let text = fs::read_to_string(path).map_err(|source| IdentityError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(Some(Identity::from_private_bytes(parse_seed(path, &text)?)))
}

/// Mint a fresh identity and write it exclusively.
fn mint(path: &Path) -> Result<Identity, IdentityError> {
    let identity = Identity::generate();
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // The umask may narrow this and never widen it, so the file is owner-only from the instant
        // it exists — there is no window in which a private key is world-readable.
        options.mode(0o600);
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        // Minted between the read and here. The file that exists is this device's identity: a key
        // that may already be published as its peer id must not be replaced by this candidate.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return read(path)?.ok_or_else(|| IdentityError::Write {
                path: path.to_path_buf(),
                source: error,
            })
        }
        Err(source) => {
            return Err(IdentityError::Write {
                path: path.to_path_buf(),
                source,
            })
        }
    };
    // Windows: replace the DACL before the seed is written, so the private key never exists under
    // the data directory's inherited ACL. A protection failure removes the empty file and refuses;
    // the read-back inside `protect_file` is what makes this a fact rather than a request.
    #[cfg(windows)]
    if let Err(detail) = summrise_winsec::protect_file(path) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(IdentityError::Protect {
            path: path.to_path_buf(),
            detail,
        });
    }
    let seed = keys::hex_encode(&identity.private_bytes());
    file.write_all(seed.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|source| IdentityError::Write {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(identity)
}

/// Parse exactly one 32-byte seed, either hex case, with the trailing newline optional.
fn parse_seed(path: &Path, text: &str) -> Result<[u8; SEED_BYTES], IdentityError> {
    let malformed = || IdentityError::Malformed {
        path: path.to_path_buf(),
    };
    let raw = keys::hex_decode(text.trim()).map_err(|_| malformed())?;
    raw.try_into().map_err(|_| malformed())
}

/// Refuse an identity file that more than its owner can read (Unix).
#[cfg(unix)]
fn enforce_owner_only(path: &Path, metadata: &fs::Metadata) -> Result<(), IdentityError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(IdentityError::Insecure {
            path: path.to_path_buf(),
            mode,
        });
    }
    Ok(())
}

/// Refuse an identity whose Windows DACL allows anyone but SYSTEM and Administrators.
///
/// This is the read half of the same check `protect_file` performs after hardening: a key copied from
/// a backup, restored from another machine, or written by a build that predates the protection must
/// not be used just because it parses.
#[cfg(windows)]
fn enforce_owner_only(path: &Path, _metadata: &fs::Metadata) -> Result<(), IdentityError> {
    summrise_winsec::verify(path).map_err(|detail| IdentityError::Protect {
        path: path.to_path_buf(),
        detail,
    })
}

/// Neither a mode nor an ACL this crate can read.
#[cfg(not(any(unix, windows)))]
fn enforce_owner_only(_path: &Path, _metadata: &fs::Metadata) -> Result<(), IdentityError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::test_support::TempDir;

    /// A data directory that exists, the way the app's own does before the identity is loaded.
    fn data_dir(label: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new(label);
        let data = dir.join("data");
        fs::create_dir_all(&data).expect("the data directory is creatable");
        (dir, data)
    }

    fn identity_path(data: &Path) -> PathBuf {
        data.join(IDENTITY_FILE)
    }

    /// Write `text` to the identity path with the mode the device itself would mint, so a test of
    /// the file's *contents* is not refused for the file's mode.
    fn write_identity(path: &Path, text: &str) {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path).expect("the fixture is creatable");
        file.write_all(text.as_bytes())
            .expect("the fixture is written");
        drop(file);
        // Windows reads the DACL before the bytes: the fixture has to be protected the way the
        // device's own mint protects it, or the test would be asserting the refusal instead.
        #[cfg(windows)]
        summrise_winsec::protect_file(path).expect("the fixture identity is protected");
    }

    #[test]
    fn the_identity_is_minted_on_first_use_and_reused_after() {
        let (_dir, data) = data_dir("identity-mint");
        assert!(
            !identity_path(&data).exists(),
            "nothing exists before the first use"
        );

        let first = load_or_mint(&data).expect("the first use mints an identity");
        let written = fs::read_to_string(identity_path(&data)).expect("the file is on disk");

        let second = load_or_mint(&data).expect("the second use reads the same file");
        assert_eq!(
            first.public_hex(),
            second.public_hex(),
            "the device keeps one identity across restarts"
        );
        assert_eq!(
            fs::read_to_string(identity_path(&data)).expect("the file is still there"),
            written,
            "an existing identity is never rewritten"
        );
    }

    #[test]
    fn the_file_is_one_32_byte_seed_in_hex() {
        let (_dir, data) = data_dir("identity-seed");
        load_or_mint(&data).expect("the identity is minted");

        let text = fs::read_to_string(identity_path(&data)).expect("the file is readable");
        let digits = text.trim();
        assert_eq!(digits.len(), 64, "a 32-byte seed is 64 hex characters");
        assert!(
            digits.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "the seed is hex: {digits:?}"
        );
        assert_eq!(
            keys::hex_decode(digits).expect("the seed decodes").len(),
            SEED_BYTES
        );
    }

    #[test]
    fn the_peer_id_is_the_public_key_hex_of_the_seed_on_disk() {
        let (_dir, data) = data_dir("identity-peer-id");
        let identity = load_or_mint(&data).expect("the identity is minted");

        let text = fs::read_to_string(identity_path(&data)).expect("the file is readable");
        let seed: [u8; SEED_BYTES] = keys::hex_decode(text.trim())
            .expect("the seed decodes")
            .try_into()
            .expect("the seed is 32 bytes");
        let from_disk = Identity::from_private_bytes(seed);

        assert_eq!(
            identity.public_hex(),
            from_disk.public_hex(),
            "the in-memory identity is the key the file holds"
        );
        assert_eq!(
            identity.public_hex(),
            keys::hex_encode(&from_disk.public_bytes()),
            "the peer id is the public key in hex, which is how the mesh names a peer"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_created_owner_only_and_a_looser_mode_is_refused() {
        use std::os::unix::fs::PermissionsExt;

        let (_dir, data) = data_dir("identity-mode");
        load_or_mint(&data).expect("the identity is minted");

        let path = identity_path(&data);
        let mode = fs::metadata(&path)
            .expect("the file exists")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "a private key is owner-only: {mode:04o}");

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
            .expect("the mode is settable");
        let error = load_or_mint(&data).expect_err("a world-readable identity is refused");
        assert!(
            matches!(error, IdentityError::Insecure { mode: 0o644, .. }),
            "the refusal names the mode it found: {error}"
        );
        assert!(
            error.to_string().contains("0600"),
            "the refusal names the mode it needs: {error}"
        );
    }

    #[test]
    fn a_file_that_is_not_one_seed_is_refused() {
        let (_dir, data) = data_dir("identity-malformed");
        let path = identity_path(&data);
        let cases = [
            String::new(),
            "00".to_owned(),
            "zz".repeat(32),
            "ab".repeat(31),
            "ab".repeat(33),
        ];
        for case in cases {
            write_identity(&path, &case);
            let error =
                load_or_mint(&data).expect_err("a file that is not one seed is not an identity");
            assert!(
                matches!(error, IdentityError::Malformed { .. }),
                "{case:?}: {error}"
            );
        }
    }

    #[test]
    fn a_directory_where_the_file_belongs_is_refused() {
        let (_dir, data) = data_dir("identity-directory");
        fs::create_dir(identity_path(&data)).expect("the directory is creatable");

        let error = load_or_mint(&data).expect_err("a directory is not an identity");
        assert!(
            matches!(error, IdentityError::NotAFile { .. }),
            "the refusal names what it found: {error}"
        );
    }
}
