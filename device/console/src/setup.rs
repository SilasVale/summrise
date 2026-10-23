//! `--setup`: the two secrets a console needs, made the first time and never overwritten.
//!
//! A fresh deployment used to mean "create an operator token and a 32-byte Ed25519 seed by hand, put
//! them where the flags point, and hope the format was right". The formats are the console's, so the
//! console can write them: this is the difference between a runbook step that is a paragraph and one
//! that is a command.
//!
//! **Never overwrites.** An existing token or seed is the operator's; replacing it would invalidate
//! every session or every grant that was ever signed, and the caller asked to set up an empty
//! directory, not to rotate. A file that already exists is reported and left alone.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use rand_core::{OsRng, RngCore};

/// The token file's name inside the data directory.
pub const OPERATOR_TOKEN_FILE: &str = "operator.token";
/// The grant seed's file name inside the data directory.
pub const GRANT_SEED_FILE: &str = "grant.seed";

/// What a setup run did, file by file.
#[derive(Debug, PartialEq, Eq)]
pub struct Setup {
    /// The data directory that was ensured.
    pub data: PathBuf,
    /// The operator token, present only when this run created it.
    pub token: Option<String>,
    /// Whether the grant seed was created by this run.
    pub seed_created: bool,
}

/// Create whichever of the two secrets is missing under `data`.
///
/// The token is 32 bytes of OS randomness as hex: a caller that guesses it has to guess 256 bits. The
/// seed is the same 32 bytes in hex, which is the format `--grant-key-file` already parses.
pub fn run(data: &Path) -> std::io::Result<Setup> {
    crate::store::create_data_dir(data)?;

    let token_path = data.join(OPERATOR_TOKEN_FILE);
    let token = if token_path.exists() {
        None
    } else {
        let token = hex32(&mut OsRng);
        write_secret(&token_path, &token)?;
        Some(token)
    };

    let seed_path = data.join(GRANT_SEED_FILE);
    let seed_created = if seed_path.exists() {
        false
    } else {
        write_secret(&seed_path, &hex32(&mut OsRng))?;
        true
    };

    Ok(Setup {
        data: data.to_path_buf(),
        token,
        seed_created,
    })
}

/// 32 random bytes as lower-case hex, the shape both files use.
fn hex32(rng: &mut impl RngCore) -> String {
    let mut bytes = [0u8; 32];
    rng.fill_bytes(&mut bytes);
    let mut text = String::with_capacity(64);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

/// Write a secret file that only its owner can read, replacing nothing.
fn write_secret(path: &Path, value: &str) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // `mode` applies at creation; pin it again so a loosened umask cannot leave a readable token.
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    writeln!(file, "{value}")?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("summrise-setup-{}-{}", std::process::id(), tag));
        let _ = fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn a_first_run_creates_both_secrets_and_reports_the_token_once() {
        let data = tempdir("first");
        let setup = run(&data).expect("setup writes the secrets");

        assert!(setup.seed_created);
        let token = setup.token.expect("the token is reported once");
        assert_eq!(token.len(), 64, "32 bytes as hex");
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));

        let written = fs::read_to_string(data.join(OPERATOR_TOKEN_FILE)).expect("token file");
        assert_eq!(written.trim(), token, "the file holds what was reported");
        let seed = fs::read_to_string(data.join(GRANT_SEED_FILE)).expect("seed file");
        assert_eq!(seed.trim().len(), 64, "the seed is 32 bytes as hex");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(data.join(OPERATOR_TOKEN_FILE))
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "a token only its owner can read");
        }
        fs::remove_dir_all(&data).ok();
    }

    /// A second run is a no-op: rotating an operator token would end every live session, and the
    /// caller asked to set up a directory, not to rotate a secret.
    #[test]
    fn a_second_run_replaces_nothing() {
        let data = tempdir("second");
        let first = run(&data).expect("first setup");
        let token_before = fs::read_to_string(data.join(OPERATOR_TOKEN_FILE)).expect("token");
        let seed_before = fs::read_to_string(data.join(GRANT_SEED_FILE)).expect("seed");

        let second = run(&data).expect("second setup");

        assert_eq!(second.token, None, "an existing token is not reissued");
        assert!(!second.seed_created, "an existing seed is not replaced");
        assert_eq!(
            fs::read_to_string(data.join(OPERATOR_TOKEN_FILE)).expect("token"),
            token_before
        );
        assert_eq!(
            fs::read_to_string(data.join(GRANT_SEED_FILE)).expect("seed"),
            seed_before
        );
        assert!(first.token.is_some());
        fs::remove_dir_all(&data).ok();
    }
}
