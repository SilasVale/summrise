//! Signing a release document with the offline release key.
//!
//! The key is a **file** holding the 32-byte ed25519 seed as hex, and its Unix mode must be
//! `0600`. The seed is never an argument, never printed and never put in an error: the process
//! table and the shell history are not secret stores. On Windows `std` exposes no Unix mode, so
//! [`read_seed`] refuses only when a mode is readable and wrong; the file's ACL is the platform's
//! protection there.
//!
//! The document is written through a random sibling, fsynced and renamed into place, so a reader
//! sees an old document or a complete new one, never a half-written file.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

use crate::document::{
    artifact_message, canonical_message, version_is_plain, Channel, ReleaseDocument, VersionField,
};
use crate::url::{parse_release_url, UrlError};

/// The only mode a key file may have on a platform that exposes Unix modes.
pub const KEY_FILE_MODE: u32 = 0o600;

/// The facts the signer puts into a document, apart from the artifact digest and signature.
#[derive(Clone, Copy, Debug)]
pub struct SignRequest<'a> {
    /// The release version, as `major.minor.patch`.
    pub version: &'a str,
    /// The channel the signature is made for.
    pub channel: Channel,
    /// Where the artifact is fetched from.
    pub artifact_url: &'a str,
    /// The oldest version this release will let a device stay on.
    pub min_version: &'a str,
    /// Millisecond Unix time the document is signed.
    pub issued_at_ms: u64,
}

/// Why the signer refused to produce or write a document.
#[derive(Debug)]
pub enum SignError {
    /// The key file could not be read.
    KeyFile {
        /// The key file that failed.
        path: PathBuf,
        /// The underlying error (never the file's contents).
        source: io::Error,
    },
    /// The key file's mode is readable by someone other than its owner.
    KeyPermissions {
        /// The key file that failed.
        path: PathBuf,
        /// The permission bits as read, masked to `0o777`.
        mode: u32,
    },
    /// The key file is not exactly 64 hex characters (a 32-byte seed).
    Seed,
    /// The artifact could not be read.
    Artifact {
        /// The artifact that failed.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// One of the versions is not the plain `major.minor.patch` the stager accepts.
    Version {
        /// Which version was malformed.
        field: VersionField,
        /// The string as it was written.
        value: String,
    },
    /// The artifact URL is not one the device will fetch.
    Url(UrlError),
    /// The document could not be serialized.
    Serialize(serde_json::Error),
    /// The document could not be written.
    Write {
        /// The destination that failed.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
}

impl std::fmt::Display for SignError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignError::KeyFile { path, source } => {
                write!(f, "cannot read the key file {}: {source}", path.display())
            }
            SignError::KeyPermissions { path, mode } => write!(
                f,
                "the key file {} has mode {mode:04o}; it must be {KEY_FILE_MODE:04o}",
                path.display()
            ),
            SignError::Seed => f.write_str(
                "the key file is not exactly 64 hex characters (a 32-byte ed25519 seed)",
            ),
            SignError::Artifact { path, source } => {
                write!(f, "cannot read the artifact {}: {source}", path.display())
            }
            SignError::Version { field, value } => {
                write!(f, "the {field} {value:?} is not major.minor.patch")
            }
            SignError::Url(error) => write!(f, "the artifact URL is not usable: {error}"),
            SignError::Serialize(error) => {
                write!(f, "cannot serialize the release document: {error}")
            }
            SignError::Write { path, source } => write!(
                f,
                "cannot write the release document {}: {source}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for SignError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SignError::KeyFile { source, .. }
            | SignError::Artifact { source, .. }
            | SignError::Write { source, .. } => Some(source),
            SignError::Serialize(error) => Some(error),
            SignError::Url(error) => Some(error),
            _ => None,
        }
    }
}

/// Read the signing key from a key file, checking its mode before its contents.
pub fn read_seed(path: &Path) -> Result<SigningKey, SignError> {
    let metadata = fs::metadata(path).map_err(|source| SignError::KeyFile {
        path: path.to_owned(),
        source,
    })?;
    if let Some(mode) = unix_mode(&metadata) {
        if mode != KEY_FILE_MODE {
            return Err(SignError::KeyPermissions {
                path: path.to_owned(),
                mode,
            });
        }
    }

    let text = fs::read_to_string(path).map_err(|source| SignError::KeyFile {
        path: path.to_owned(),
        source,
    })?;
    seed_from_hex(text.trim())
}

/// Parse a 32-byte ed25519 seed from 64 hex characters.
pub fn seed_from_hex(text: &str) -> Result<SigningKey, SignError> {
    let bytes = hex_decode(text).ok_or(SignError::Seed)?;
    let seed: [u8; 32] = bytes.try_into().map_err(|_| SignError::Seed)?;
    Ok(SigningKey::from_bytes(&seed))
}

/// Lower-hex SHA-256 of the artifact.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

/// Sign one artifact, producing the document a device will verify.
///
/// The signer produces both signatures: the policy signature over every field the device acts on,
/// and the artifact signature over the stager's older three-part message. It refuses first, using
/// the stager's own version shape and the device's own URL rule, so it cannot emit a document that
/// fetches and verifies on a device only to fail at `stage`.
pub fn sign_document(
    key: &SigningKey,
    artifact: &[u8],
    request: &SignRequest<'_>,
) -> Result<ReleaseDocument, SignError> {
    validate_request(request)?;

    let mut document = ReleaseDocument {
        version: request.version.to_owned(),
        channel: request.channel,
        sha256: sha256_hex(artifact),
        artifact_url: request.artifact_url.to_owned(),
        min_version: request.min_version.to_owned(),
        issued_at_ms: request.issued_at_ms,
        artifact_signature: String::new(),
        signature: String::new(),
    };
    let artifact_signature = key.sign(artifact_message(&document).as_bytes());
    let signature = key.sign(canonical_message(&document).as_bytes());
    document.artifact_signature = hex_encode(&artifact_signature.to_bytes());
    document.signature = hex_encode(&signature.to_bytes());
    Ok(document)
}

/// Validate the facts the signer is about to sign, before any file is read or written.
fn validate_request(request: &SignRequest<'_>) -> Result<(), SignError> {
    validate_version(VersionField::Release, request.version)?;
    validate_version(VersionField::Minimum, request.min_version)?;
    parse_release_url(request.artifact_url).map_err(SignError::Url)?;
    Ok(())
}

/// Read the key and the artifact, sign, and write the document atomically.
pub fn sign_file(
    key_file: &Path,
    artifact_file: &Path,
    request: &SignRequest<'_>,
    out: &Path,
) -> Result<ReleaseDocument, SignError> {
    validate_request(request)?;
    let key = read_seed(key_file)?;
    let artifact = fs::read(artifact_file).map_err(|source| SignError::Artifact {
        path: artifact_file.to_owned(),
        source,
    })?;
    let document = sign_document(&key, &artifact, request)?;

    let mut bytes = serde_json::to_vec_pretty(&document).map_err(SignError::Serialize)?;
    bytes.push(b'\n');
    write_atomic(out, &bytes).map_err(|source| SignError::Write {
        path: out.to_owned(),
        source,
    })?;

    Ok(document)
}

/// Millisecond Unix time, saturating at the epoch for a clock before it.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

/// The permission bits of `metadata` where the platform exposes a Unix mode.
#[cfg(unix)]
fn unix_mode(metadata: &fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;

    Some(metadata.permissions().mode() & 0o777)
}

/// Windows exposes no Unix mode through `std`; its ACL is the protection that matters there.
#[cfg(not(unix))]
fn unix_mode(_metadata: &fs::Metadata) -> Option<u32> {
    None
}

/// Create a brand-new temporary sibling of `path` with an unpredictable name.
fn create_new_sibling(path: &Path) -> io::Result<(PathBuf, File)> {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let name = path
        .file_name()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} has no file name", path.display()),
            )
        })?
        .to_string_lossy()
        .into_owned();
    for _ in 0..16 {
        // `RandomState` is seeded from OS entropy, so the nonce is not derivable from the clock or
        // the pid; `create_new` makes even a guessed name fail rather than follow a planted link.
        let nonce = RandomState::new().build_hasher().finish();
        let temporary = path.with_file_name(format!(".{name}.{nonce:016x}.tmp"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "cannot create a temporary file beside {} after 16 attempts",
            path.display()
        ),
    ))
}

/// Write `bytes` to `path` through a random sibling, fsynced and renamed.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let (temporary, mut file) = create_new_sibling(path)?;
    let result = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Lower-hex encoding, used for the digest and the signature in the document.
pub fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing to a String cannot fail.
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Decode a hex string; `None` for an odd length, non-ASCII or a non-hex character.
pub(crate) fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    let mut bytes = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().as_chunks::<2>().0 {
        let high = (pair[0] as char).to_digit(16)?;
        let low = (pair[1] as char).to_digit(16)?;
        bytes.push((high * 16 + low) as u8);
    }
    Some(bytes)
}

/// Validate a version with the stager's own rule, before it is signed.
fn validate_version(field: VersionField, value: &str) -> Result<(), SignError> {
    if version_is_plain(value) {
        Ok(())
    } else {
        Err(SignError::Version {
            field,
            value: value.to_owned(),
        })
    }
}

/// The first-generation sidecar the launcher verifies before it seeds `baseline/current`.
///
/// The message is the older one the launcher already checks — `"{version}\n{channel}\n{sha256}"` —
/// and it is signed by the **launcher** key, not the release key: the first generation is the root
/// of trust, and an update key must not be able to mint one. This is the same shape the app writes
/// for a staged update, which is why the launcher has one verdict for both.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactSidecar {
    /// The release version, e.g. `0.1.1`.
    pub version: String,
    /// The channel the signature was made for.
    pub channel: Channel,
    /// Lower-hex SHA-256 of the binary.
    pub sha256: String,
    /// Lower-hex ed25519 signature over `"{version}\n{channel}\n{sha256}"`.
    pub signature: String,
}

/// Sign an artifact for the first generation, with the launcher key.
///
/// The key file is read exactly like [`sign_document`]'s: a 0600 file of 64 hex characters, never an
/// argument and never printed. The write is the same atomic sibling-and-rename, so a crash leaves the
/// previous sidecar or a complete new one.
pub fn sign_sidecar(
    key_file: &Path,
    artifact: &Path,
    version: &str,
    channel: Channel,
    out: &Path,
) -> Result<ArtifactSidecar, SignError> {
    if !version_is_plain(version) {
        return Err(SignError::Version {
            field: VersionField::Release,
            value: version.to_owned(),
        });
    }
    let key = read_seed(key_file)?;
    let bytes = fs::read(artifact).map_err(|source| SignError::Artifact {
        path: artifact.to_owned(),
        source,
    })?;
    let sha256 = sha256_hex(&bytes);
    let message = format!("{version}\n{}\n{sha256}", channel.as_str());
    let signature = key.sign(message.as_bytes());
    let sidecar = ArtifactSidecar {
        version: version.to_owned(),
        channel,
        sha256,
        signature: hex_encode(&signature.to_bytes()),
    };
    let mut bytes = serde_json::to_vec_pretty(&sidecar).map_err(SignError::Serialize)?;
    bytes.push(b'\n');
    write_atomic(out, &bytes).map_err(|source| SignError::Write {
        path: out.to_owned(),
        source,
    })?;
    Ok(sidecar)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SEED: [u8; 32] = [7u8; 32];

    fn key() -> SigningKey {
        SigningKey::from_bytes(&TEST_SEED)
    }

    fn request<'a>(version: &'a str, min_version: &'a str) -> SignRequest<'a> {
        SignRequest {
            version,
            channel: Channel::Stable,
            artifact_url: "https://example.test/summrise-device.exe",
            min_version,
            issued_at_ms: now_ms(),
        }
    }

    /// A directory under the system temp root that removes itself, named for this test process.
    struct TestDir(std::path::PathBuf);

    impl TestDir {
        fn new(tag: &str) -> TestDir {
            use std::sync::atomic::{AtomicU32, Ordering};
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "summrise-release-test-{}-{}-{tag}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create the test directory");
            TestDir(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Write `bytes` as the artifact and a 0600 key file holding `TEST_SEED`.
    fn artifact_and_key() -> (TestDir, std::path::PathBuf, std::path::PathBuf) {
        let dir = TestDir::new("sidecar");
        let artifact = dir.path().join("summrise-device.exe");
        fs::write(&artifact, b"the artifact bytes").expect("write the artifact");
        let key_file = dir.path().join("launcher.seed");
        fs::write(&key_file, hex_encode(&TEST_SEED)).expect("write the seed");
        set_private(&key_file);
        (dir, artifact, key_file)
    }

    fn set_private(path: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("0600");
        }
        #[cfg(not(unix))]
        let _ = path;
    }

    #[test]
    fn a_sidecar_signature_verifies_over_the_documented_message() {
        let (_dir, artifact, key_file) = artifact_and_key();
        let out = artifact.with_file_name("summrise-device.sidecar.json");
        let sidecar = sign_sidecar(&key_file, &artifact, "0.1.1", Channel::Canary, &out)
            .expect("sign the sidecar");
        assert_eq!(sidecar.version, "0.1.1");
        assert_eq!(sidecar.channel, Channel::Canary);
        assert_eq!(
            sidecar.sha256,
            sha256_hex(b"the artifact bytes"),
            "the digest is the artifact's"
        );
        let message = format!("0.1.1\ncanary\n{}", sidecar.sha256);
        use ed25519_dalek::Verifier;
        let signature_bytes = hex_decode(&sidecar.signature).expect("the signature is hex");
        let signature = ed25519_dalek::Signature::from_slice(&signature_bytes).expect("64 bytes");
        key()
            .verifying_key()
            .verify(message.as_bytes(), &signature)
            .expect(
                "the signature covers version, channel and digest — the launcher's exact message",
            );
        let bytes = fs::read(&out).expect("read the sidecar");
        let written: ArtifactSidecar = serde_json::from_slice(&bytes).expect("parse");
        assert_eq!(written, sidecar, "the file is the value that was signed");
        // The launcher's own type must accept the file: one shape, two readers.
        let launcher_view: summrise_launch::Sidecar =
            serde_json::from_slice(&bytes).expect("the launcher parses the sidecar");
        assert_eq!(launcher_view.version, "0.1.1");
        assert_eq!(launcher_view.channel, "canary");
        assert_eq!(launcher_view.sha256, sidecar.sha256);
    }

    #[test]
    fn a_changed_digest_breaks_the_sidecar_signature() {
        let (_dir, artifact, key_file) = artifact_and_key();
        let out = artifact.with_file_name("sidecar.json");
        let sidecar =
            sign_sidecar(&key_file, &artifact, "0.1.1", Channel::Stable, &out).expect("sign");
        let message = format!("0.1.1\nstable\n{}", "0".repeat(64));
        use ed25519_dalek::Verifier;
        let signature =
            ed25519_dalek::Signature::from_slice(&hex_decode(&sidecar.signature).expect("hex"))
                .expect("64 bytes");
        assert!(
            key()
                .verifying_key()
                .verify(message.as_bytes(), &signature)
                .is_err(),
            "a different digest must not verify against the signed message"
        );
    }

    #[test]
    fn a_sidecar_needs_a_plain_version() {
        let (_dir, artifact, key_file) = artifact_and_key();
        let out = artifact.with_file_name("sidecar.json");
        let error = sign_sidecar(&key_file, &artifact, "v0.1.1", Channel::Canary, &out)
            .expect_err("a tag-style version is refused");
        assert!(matches!(error, SignError::Version { .. }), "{error:?}");
    }

    #[test]
    fn the_artifact_digest_is_the_known_lowercase_hex() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let digest = sha256_hex(b"the release bytes");
        assert!(
            digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "the digest is lowercase hex: {digest}"
        );
    }

    #[test]
    fn a_signed_document_verifies_and_carries_the_artifact_digest() {
        let key = key();
        let artifact = b"the release bytes";

        let document =
            sign_document(&key, artifact, &request("2.0.3", "2.0.0")).expect("signing succeeds");

        assert_eq!(document.sha256, sha256_hex(artifact));
        assert_eq!(
            document.sha256,
            "86c0ea10853cdebc340b41ccd1a151ff282ec570bad37f52fa2321dd34b7479f"
        );
        assert_eq!(
            crate::document::verify(&document, &key.verifying_key()),
            Ok(())
        );
        assert_eq!(document.signature.len(), 128);
        assert_eq!(document.artifact_signature.len(), 128);
        assert_ne!(
            document.signature, document.artifact_signature,
            "the policy and artifact messages are different, so their signatures are too"
        );
    }

    #[test]
    fn a_bad_seed_is_refused() {
        for text in ["", "not-hex", "0", "zz", &"7".repeat(62), &"7".repeat(66)] {
            assert!(
                matches!(seed_from_hex(text), Err(SignError::Seed)),
                "{text:?} must not be a seed"
            );
        }
        assert!(seed_from_hex(&"7".repeat(64)).is_ok());
    }

    #[test]
    fn every_version_the_stager_cannot_name_is_refused() {
        let key = key();
        let artifact = b"the release bytes";

        for version in ["+1.0.0", "1.0", "v1.0.0", "1.0.0-rc.1"] {
            let result = sign_document(&key, artifact, &request(version, "2.0.0"));
            assert!(
                matches!(
                    result,
                    Err(SignError::Version {
                        field: VersionField::Release,
                        ref value,
                    }) if value == version
                ),
                "{version:?} must be refused, not signed: {result:?}"
            );
        }
    }

    #[test]
    fn a_malformed_version_is_refused_before_signing() {
        let key = key();
        let artifact = b"the release bytes";

        let bad_release = sign_document(&key, artifact, &request("2.0", "2.0.0"));
        assert!(
            matches!(
                bad_release,
                Err(SignError::Version {
                    field: VersionField::Release,
                    ..
                })
            ),
            "{bad_release:?}"
        );

        let bad_floor = sign_document(&key, artifact, &request("2.0.3", "2.0.0.1"));
        assert!(
            matches!(
                bad_floor,
                Err(SignError::Version {
                    field: VersionField::Minimum,
                    ..
                })
            ),
            "{bad_floor:?}"
        );
    }

    #[test]
    fn urls_the_device_refuses_are_refused_before_signing() {
        let key = key();

        for url in [
            "",
            "not a url",
            "ftp://example.test/summrise-device.exe",
            "http://example.test/summrise-device.exe",
        ] {
            let mut request = request("2.0.3", "2.0.0");
            request.artifact_url = url;
            assert!(
                matches!(
                    sign_document(&key, b"the release bytes", &request),
                    Err(SignError::Url(_))
                ),
                "{url:?} must be refused, not signed"
            );
        }

        // The lane's loopback fixture is the one plain-http exception the device allows.
        let mut request = request("2.0.3", "2.0.0");
        request.artifact_url = "http://127.0.0.1:8080/summrise-device.exe";
        assert!(sign_document(&key, b"the release bytes", &request).is_ok());
    }
}
