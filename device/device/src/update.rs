//! The update stager: verify a release, and only then put it on disk (design §4.4).
//!
//! The order below is the security property, not an implementation detail:
//!
//! 1. the version is a plain `major.minor.patch`, so it cannot name a path outside the install;
//! 2. the channel is one of the two release channels;
//! 3. the manifest's ed25519 signature verifies over `"{version}\n{channel}\n{sha256}"`;
//! 4. the artifact's sha256 is the one the manifest names;
//! 5. the version is not below the manifest's `minVersion`.
//!
//! Only after all five pass does [`stage`] create or replace the staged artifact and its sidecar.
//! A refused update leaves the filesystem as it was: no artifact, no sidecar, no directory.
//!
//! **The staged pair is the only destination (fix design F3).** [`stage`] takes the install root, not
//! two output paths, and derives exactly `<install>/summrise-device.new.exe` and
//! `<install>/summrise-device.new.json` from it. The app binary, the baseline and the launcher's
//! other files are unreachable from here by construction. After a successful stage [`request_swap`]
//! writes the request the supervisor polls, and [`await_swap_result`] reads the answer.
//!
//! The launcher re-verifies the sidecar before it swaps, so signing the channel into the message
//! means a canary artifact cannot be replayed as a stable one.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use summrise_core::manifest::{Manifest, Version};

/// The staged artifact's file name under the install root.
pub const STAGED_EXE: &str = "summrise-device.new.exe";

/// The staged artifact's sidecar file name under the install root.
pub const STAGED_SIDECAR: &str = "summrise-device.new.json";

/// The request file the supervisor polls (fix design F1).
pub const SWAP_REQUEST: &str = "swap.request.json";

/// The result file the supervisor writes (fix design F1).
pub const SWAP_RESULT: &str = "swap.result.json";

/// The placeholder that ships when no release key was configured at build time.
///
/// All-zero is not a point anybody signs with: [`parse_pubkey`] refuses it by name
/// ([`KeyError::Placeholder`]), so a phase-1 build fails closed before it verifies anything. This is
/// the same seam as `summrise-launch`'s `SUMMRISE_LAUNCHER_PUBKEY`.
pub const PLACEHOLDER_RELEASE_PUBKEY_HEX: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// The release public key, embedded at build time from `SUMMRISE_RELEASE_PUBKEY`.
///
/// The seam is **compile-time**, never a runtime override: a key that the environment of a running
/// service could replace would make the signature check optional. Without the variable the
/// placeholder ships and every staging attempt refuses by name.
pub const RELEASE_PUBKEY_HEX: &str = match option_env!("SUMMRISE_RELEASE_PUBKEY") {
    Some(key) => key,
    None => PLACEHOLDER_RELEASE_PUBKEY_HEX,
};

/// The channels a phase-1 release may name (design §4.4).
pub const CHANNELS: [&str; 2] = ["stable", "canary"];

/// The signed facts the launcher re-verifies, as a file beside the staged artifact.
///
/// This is deliberately the same shape as `summrise_launch::Sidecar`: the stager is the writer and
/// the launcher is the reader, and the wire contract between two binaries is worth a dev-dependency
/// test. `channel` is part of the sidecar because it is part of the signed message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sidecar {
    /// The release version, e.g. `2.0.3`.
    pub version: String,
    /// The release channel the signature was made for, e.g. `stable`.
    pub channel: String,
    /// Lower-hex SHA-256 of the artifact.
    pub sha256: String,
    /// Lower-hex ed25519 signature over `"{version}\n{channel}\n{sha256}"`.
    pub signature: String,
}

/// Why the compile-time release key cannot be used.
///
/// This is separate from [`StageError`] because a `VerifyingKey` cannot represent the placeholder:
/// the refusal has to happen when the key is parsed, before [`stage`] is called.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyError {
    /// The build carries the all-zero placeholder, so no release can be trusted.
    Placeholder,
    /// The configured key is not 32 bytes of hex.
    BadKey,
}

/// Why an update was refused, or could not be staged.
#[derive(Debug)]
pub enum StageError {
    /// The ed25519 signature over `"{version}\n{channel}\n{sha256}"` does not verify.
    Signature,
    /// The artifact's sha256 is not the manifest's.
    Sha,
    /// The version is below `minVersion`, or `minVersion` is not a version.
    Version,
    /// The manifest's channel is not one of [`CHANNELS`].
    Channel,
    /// The version is not a safe, plain `major.minor.patch` name.
    Path,
    /// The staged artifact, sidecar, request or result could not be written or read.
    Io(io::Error),
    /// The supervisor did not answer the request in time; the stage itself succeeded.
    NoResult(String),
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyError::Placeholder => write!(
                f,
                "the device was built with the phase-1 placeholder release key (set \
                 SUMMRISE_RELEASE_PUBKEY at build time to trust releases)"
            ),
            KeyError::BadKey => write!(f, "the release public key is not 32 bytes of hex"),
        }
    }
}

impl std::error::Error for KeyError {}

impl fmt::Display for StageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StageError::Signature => write!(f, "the release signature does not verify"),
            StageError::Sha => write!(f, "the artifact does not hash to the manifest's sha256"),
            StageError::Version => write!(
                f,
                "the release is below the manifest's minVersion, or minVersion is not a version"
            ),
            StageError::Channel => write!(f, "the manifest names an unknown release channel"),
            StageError::Path => write!(f, "the release version is not a plain major.minor.patch"),
            StageError::Io(error) => write!(f, "could not stage the update: {error}"),
            StageError::NoResult(detail) => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for StageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StageError::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for StageError {
    fn from(error: io::Error) -> Self {
        StageError::Io(error)
    }
}

/// Parse a release public key from its lower- or upper-hex form.
///
/// The placeholder is refused by name rather than left to fail as a bad curve point: "this build
/// cannot trust releases" and "this key is malformed" have different fixes, and a reader must be
/// able to tell them apart.
pub fn parse_pubkey(hex: &str) -> Result<VerifyingKey, KeyError> {
    if hex.eq_ignore_ascii_case(PLACEHOLDER_RELEASE_PUBKEY_HEX) {
        return Err(KeyError::Placeholder);
    }
    let bytes = hex_decode(hex).ok_or(KeyError::BadKey)?;
    let bytes: [u8; 32] = bytes.try_into().map_err(|_| KeyError::BadKey)?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| KeyError::BadKey)
}

/// The embedded release key, through the same parse as a test key.
pub fn release_pubkey() -> Result<VerifyingKey, KeyError> {
    parse_pubkey(RELEASE_PUBKEY_HEX)
}

/// The release facts a successful stage produced, and the request needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedRelease {
    /// The version the signed manifest names.
    pub version: String,
    /// The digest the signed manifest names (as written, lower- or upper-hex).
    pub sha256: String,
}

/// Verify a release and, only if every check passes, write the canonical staged pair.
///
/// The destination is **not an argument** (fix design F3): both paths are derived from `install`.
/// The artifact bytes are held in memory: the caller has already downloaded them and bounded their
/// size. On any refusal the filesystem is untouched. On success the writes are `.tmp` + fsync +
/// rename, staged first and sidecar last, so the sidecar's presence marks a complete pair.
pub fn stage(
    bytes: &[u8],
    manifest: &Manifest,
    pubkey: &VerifyingKey,
    install: &Path,
) -> Result<StagedRelease, StageError> {
    // 1. The version is a plain x.y.z and cannot name a path. This runs before the signature so a
    //    hostile version string never reaches a path join, however this function grows. The signer
    //    uses the same predicate, so it cannot emit a version this step will refuse.
    if !summrise_release::version_is_plain(&manifest.version) {
        return Err(StageError::Path);
    }

    // 2. The channel is one of the release channels. It is signed, but a signed unknown channel is
    //    still a manifest this build must not act on.
    if !CHANNELS.contains(&manifest.channel.as_str()) {
        return Err(StageError::Channel);
    }

    // 3. The signature covers all three facts, so a release cannot be moved between versions or
    //    channels after it was signed.
    verify_manifest_signature(manifest, pubkey)?;

    // 4. The bytes are what the signed manifest names.
    if !sha256_hex(bytes).eq_ignore_ascii_case(&manifest.sha256) {
        return Err(StageError::Sha);
    }

    // 5. The version policy is applied last: a signed artifact is genuine even when it is too old,
    //    and the refusal should say that rather than claim a forgery.
    let version = Version::parse(&manifest.version).map_err(|_| StageError::Path)?;
    let min_version = Version::parse(&manifest.min_version).map_err(|_| StageError::Version)?;
    if version < min_version {
        return Err(StageError::Version);
    }

    let sidecar_value = Sidecar {
        version: manifest.version.clone(),
        channel: manifest.channel.clone(),
        sha256: manifest.sha256.clone(),
        signature: manifest.signature.clone(),
    };
    let sidecar_json = serde_json::to_vec(&sidecar_value)
        .map_err(|error| StageError::Io(io::Error::other(error)))?;

    // All checks have passed. Staged first, sidecar last: the launcher consumes a pair, and the
    // sidecar is the marker that the pair is complete.
    write_atomic(&staged_exe_path(install), bytes)?;
    write_atomic(&staged_sidecar_path(install), &sidecar_json)?;
    Ok(StagedRelease {
        version: manifest.version.clone(),
        sha256: manifest.sha256.clone(),
    })
}

/// `<install>/summrise-device.new.exe` — the one artifact path the stager can write.
pub fn staged_exe_path(install: &Path) -> PathBuf {
    install.join(STAGED_EXE)
}

/// `<install>/summrise-device.new.json` — the sidecar beside [`staged_exe_path`].
pub fn staged_sidecar_path(install: &Path) -> PathBuf {
    install.join(STAGED_SIDECAR)
}

/// `<install>/swap.request.json` — the request the supervisor polls.
pub fn swap_request_path(install: &Path) -> PathBuf {
    install.join(SWAP_REQUEST)
}

/// `<install>/swap.result.json` — the supervisor's answer.
pub fn swap_result_path(install: &Path) -> PathBuf {
    install.join(SWAP_RESULT)
}

/// The request document the launcher's `SwapRequest` parses: exactly these three fields.
///
/// The reader's side is the launcher's (`#[serde(deny_unknown_fields)]` there); this type only has
/// to write what that reader accepts, which the test cross-checks with `summrise_launch`.
#[derive(Serialize)]
struct SwapRequest {
    requested_at_ms: u64,
    version: String,
    sha256: String,
}

/// What the supervisor did with a request (fix design F1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwapOutcome {
    /// The new build passed both probes and is now `current`.
    Applied,
    /// The new build failed; the previous generation is `current` and is serving again.
    RolledBack,
    /// The request or its staged pair was not usable; nothing was stopped.
    Refused,
}

impl SwapOutcome {
    /// The exit code of `summrise-device --stage`, the machine-readable half of the lane contract.
    ///
    /// 0 for `applied`, 3 for `rolled_back`, 4 for `refused`. Everything else (a stage refusal, a
    /// usage error, a missing result) keeps the process's usual 1/2 codes, so a caller can tell
    /// "the supervisor decided" from "the device never asked".
    pub fn exit_code(self) -> u8 {
        match self {
            SwapOutcome::Applied => 0,
            SwapOutcome::RolledBack => 3,
            SwapOutcome::Refused => 4,
        }
    }
}

/// The supervisor's answer, as `<install>/swap.result.json` carries it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwapResult {
    /// What happened.
    pub outcome: SwapOutcome,
    /// The release version the request named; absent for `refused`.
    #[serde(default)]
    pub version: Option<String>,
    /// The artifact's digest, when the supervisor verified one; absent on an older result or a
    /// refusal that never reached a staged pair.
    #[serde(default)]
    pub sha256: Option<String>,
    /// Millisecond Unix time the supervisor wrote the result.
    pub at_ms: u64,
    /// Why the update rolled back or was refused; absent for `applied`.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Ask the supervisor to swap, after a successful [`stage`].
///
/// A stale result is one the supervisor wrote before this request existed; it is removed first so a
/// waiter cannot mistake it for this swap's answer. The request itself is written atomically, exactly
/// as the launcher's `parse_swap_request` expects it.
pub fn request_swap(install: &Path, release: &StagedRelease) -> Result<u64, StageError> {
    let requested_at_ms = now_ms();
    discard_stale_result(install, requested_at_ms)?;

    let request = SwapRequest {
        requested_at_ms,
        version: release.version.clone(),
        sha256: release.sha256.clone(),
    };
    let document =
        serde_json::to_vec(&request).map_err(|error| StageError::Io(io::Error::other(error)))?;
    write_atomic(&swap_request_path(install), &document)?;
    Ok(requested_at_ms)
}

/// Wait up to `timeout` for the supervisor's answer to the request written at `requested_at_ms`.
///
/// A result older than the request belongs to a previous swap (the supervisor may not have removed
/// it yet); it is deleted and the wait continues. An unparsable result is never an answer either: the
/// supervisor writes atomically, so a reader that sees garbage is looking at a leftover, not a torn
/// commit, and waiting for the real one is the only way to learn the outcome.
pub fn await_swap_result(
    install: &Path,
    requested_at_ms: u64,
    timeout: Duration,
) -> Result<SwapResult, StageError> {
    let path = swap_result_path(install);
    let deadline = Instant::now() + timeout;

    loop {
        match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<SwapResult>(&bytes) {
                Ok(result) if result.at_ms >= requested_at_ms => return Ok(result),
                Ok(_) => {
                    let _ = fs::remove_file(&path);
                }
                Err(_) => {}
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(StageError::Io(error)),
        }

        if Instant::now() >= deadline {
            return Err(StageError::NoResult(format!(
                "the supervisor did not write {} within {} ms",
                path.display(),
                timeout.as_millis()
            )));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Delete a result that cannot be this request's answer, before the request is written.
fn discard_stale_result(install: &Path, requested_at_ms: u64) -> Result<(), StageError> {
    let path = swap_result_path(install);
    match fs::read(&path) {
        Ok(bytes) => {
            let stale = match serde_json::from_slice::<SwapResult>(&bytes) {
                Ok(result) => result.at_ms < requested_at_ms,
                Err(_) => true,
            };
            if stale {
                fs::remove_file(&path)?;
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StageError::Io(error)),
    }
}

/// Millisecond Unix time, saturating at the epoch for a clock before it.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

/// Verify the manifest's signature over exactly `"{version}\n{channel}\n{sha256}"`.
fn verify_manifest_signature(manifest: &Manifest, pubkey: &VerifyingKey) -> Result<(), StageError> {
    let bytes = hex_decode(&manifest.signature)
        .filter(|bytes| bytes.len() == 64)
        .ok_or(StageError::Signature)?;
    let signature = Signature::from_slice(&bytes).map_err(|_| StageError::Signature)?;
    let message = format!(
        "{}\n{}\n{}",
        manifest.version, manifest.channel, manifest.sha256
    );
    pubkey
        .verify_strict(message.as_bytes(), &signature)
        .map_err(|_| StageError::Signature)
}

/// Write `bytes` to `path` through a random sibling, fsynced and renamed.
///
/// The rename is the commit point. A write that fails before it leaves the old file in place and
/// removes the temporary one; a write that fails after it has already produced the new file. The
/// temporary is created with `create_new` under an unpredictable name (NEW-8), so a planted
/// `<name>.tmp` link cannot receive the bytes.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let (temporary, mut file) = crate::fsutil::create_new_sibling(path)?;
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

/// Lower-hex SHA-256 of `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

/// Decode a hex string; `None` for an odd length, non-ASCII or a non-hex character.
fn hex_decode(text: &str) -> Option<Vec<u8>> {
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

/// Lower-hex encoding, used for digests and signatures in the sidecar.
fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing to a String cannot fail.
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Test fixtures for a release: one fixed signing key and the manifests it signs.
///
/// `--stage`'s tests need the same fixtures as the stager's, and a second hand-rolled signer would
/// be a second spelling of the signed message — exactly the thing this module exists to keep single.
#[cfg(test)]
pub(crate) mod fixtures {
    use super::{hex_encode, sha256_hex, Manifest};
    use ed25519_dalek::{Signer, SigningKey};

    /// A fixed seed stands in for the release signing key.
    pub(crate) const TEST_SEED: [u8; 32] = [7u8; 32];

    /// The test signing key.
    pub(crate) fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&TEST_SEED)
    }

    /// A manifest signed over exactly the message the stager verifies.
    pub(crate) fn manifest_for(
        version: &str,
        channel: &str,
        bytes: &[u8],
        key: &SigningKey,
        min_version: &str,
    ) -> Manifest {
        let sha256 = sha256_hex(bytes);
        let message = format!("{version}\n{channel}\n{sha256}");
        let signature = key.sign(message.as_bytes());
        Manifest {
            version: version.to_owned(),
            channel: channel.to_owned(),
            sha256,
            signature: hex_encode(&signature.to_bytes()),
            min_version: min_version.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use summrise_launch::{
        parse_swap_request, verify_signature as launcher_verify, Sidecar as LauncherSidecar,
        SwapRequestState,
    };

    use crate::test_support::TempDir;
    use crate::update::fixtures::{manifest_for, signing_key};

    fn paths(dir: &TempDir) -> (PathBuf, PathBuf, PathBuf) {
        let install = dir.join("install");
        (
            install.clone(),
            staged_exe_path(&install),
            staged_sidecar_path(&install),
        )
    }

    fn assert_nothing_staged(dir: &TempDir, staged: &Path, sidecar: &Path) {
        assert!(!staged.exists(), "no artifact may be staged on a refusal");
        assert!(!sidecar.exists(), "no sidecar may be written on a refusal");
        assert!(
            !dir.join("install").exists(),
            "a refusal must not even create the install directory"
        );
    }

    #[test]
    fn a_valid_artifact_is_staged_and_its_sidecar_verifies_in_the_launcher() {
        let dir = TempDir::new("stage-valid");
        let (install, staged, sidecar_path) = paths(&dir);
        let key = signing_key();
        let bytes = b"the release bytes";
        let manifest = manifest_for("2.0.3", "stable", bytes, &key, "2.0.0");

        stage(bytes, &manifest, &key.verifying_key(), &install).expect("a valid release stages");

        assert_eq!(fs::read(&staged).expect("the artifact is readable"), bytes);
        let sidecar: LauncherSidecar =
            serde_json::from_slice(&fs::read(&sidecar_path).expect("the sidecar is readable"))
                .expect("the launcher's Sidecar reads what the stager wrote");
        assert_eq!(sidecar.version, "2.0.3");
        assert_eq!(sidecar.channel, "stable");
        assert_eq!(sidecar.sha256, manifest.sha256);
        assert_eq!(sidecar.signature, manifest.signature);
        assert_eq!(
            launcher_verify(&sidecar, &key.verifying_key()),
            Ok(()),
            "the launcher must accept the sidecar the stager wrote"
        );
    }

    #[test]
    fn a_bad_signature_stages_nothing() {
        let dir = TempDir::new("stage-bad-signature");
        let (install, staged, sidecar_path) = paths(&dir);
        let key = signing_key();
        let bytes = b"the release bytes";
        let mut manifest = manifest_for("2.0.3", "stable", bytes, &key, "2.0.0");
        manifest.signature = "00".repeat(64);

        let result = stage(bytes, &manifest, &key.verifying_key(), &install);

        assert!(matches!(result, Err(StageError::Signature)), "{result:?}");
        assert_nothing_staged(&dir, &staged, &sidecar_path);
    }

    #[test]
    fn a_sha_mismatch_stages_nothing() {
        let dir = TempDir::new("stage-bad-sha");
        let (install, staged, sidecar_path) = paths(&dir);
        let key = signing_key();
        let manifest = manifest_for("2.0.3", "stable", b"expected bytes", &key, "2.0.0");

        let result = stage(b"tampered bytes", &manifest, &key.verifying_key(), &install);

        assert!(matches!(result, Err(StageError::Sha)), "{result:?}");
        assert_nothing_staged(&dir, &staged, &sidecar_path);
    }

    #[test]
    fn a_version_below_min_version_is_refused() {
        let dir = TempDir::new("stage-min-version");
        let (install, staged, sidecar_path) = paths(&dir);
        let key = signing_key();
        let bytes = b"the release bytes";
        let manifest = manifest_for("2.0.2", "stable", bytes, &key, "2.0.3");

        let result = stage(bytes, &manifest, &key.verifying_key(), &install);

        assert!(matches!(result, Err(StageError::Version)), "{result:?}");
        assert_nothing_staged(&dir, &staged, &sidecar_path);
    }

    #[test]
    fn a_version_equal_to_min_version_is_allowed() {
        let dir = TempDir::new("stage-min-equal");
        let (install, _, _) = paths(&dir);
        let key = signing_key();
        let bytes = b"the release bytes";
        let manifest = manifest_for("2.0.3", "stable", bytes, &key, "2.0.3");

        stage(bytes, &manifest, &key.verifying_key(), &install)
            .expect("minVersion is the oldest version allowed, not the newest refused");
    }

    #[test]
    fn a_malformed_min_version_is_refused() {
        let dir = TempDir::new("stage-min-shape");
        let (install, staged, sidecar_path) = paths(&dir);
        let key = signing_key();
        let bytes = b"the release bytes";
        let manifest = manifest_for("2.0.3", "stable", bytes, &key, "2.0");

        let result = stage(bytes, &manifest, &key.verifying_key(), &install);

        assert!(matches!(result, Err(StageError::Version)), "{result:?}");
        assert_nothing_staged(&dir, &staged, &sidecar_path);
    }

    #[test]
    fn a_version_that_escapes_the_install_is_a_path_error() {
        let key = signing_key();
        let bytes = b"the release bytes";

        for (index, version) in [
            "../evil",
            "..\\evil",
            "2.0.3/evil",
            "2.0.3\\evil",
            "2.0.3..evil",
            "..",
        ]
        .into_iter()
        .enumerate()
        {
            let dir = TempDir::new(&format!("stage-path-{index}"));
            let (install, staged, sidecar_path) = paths(&dir);
            let manifest = manifest_for(version, "stable", bytes, &key, "0.0.1");

            let result = stage(bytes, &manifest, &key.verifying_key(), &install);

            assert!(
                matches!(result, Err(StageError::Path)),
                "{version:?} must be a Path refusal, got {result:?}"
            );
            assert_nothing_staged(&dir, &staged, &sidecar_path);
        }
    }

    #[test]
    fn a_version_that_is_not_a_plain_x_y_z_is_a_path_error() {
        let key = signing_key();
        let bytes = b"the release bytes";

        for (index, version) in [
            "", "2", "2.0", "2.0.3.4", "v2.0.3", "+2.0.3", "2.0.x", "2.0.-1", " 2.0.3", "2.0.3 ",
        ]
        .into_iter()
        .enumerate()
        {
            let dir = TempDir::new(&format!("stage-shape-{index}"));
            let (install, staged, sidecar_path) = paths(&dir);
            let manifest = manifest_for(version, "stable", bytes, &key, "0.0.1");

            let result = stage(bytes, &manifest, &key.verifying_key(), &install);

            assert!(
                matches!(result, Err(StageError::Path)),
                "{version:?} must be a Path refusal, got {result:?}"
            );
            assert_nothing_staged(&dir, &staged, &sidecar_path);
        }
    }

    #[test]
    fn a_prerelease_version_is_refused_in_phase_1() {
        let dir = TempDir::new("stage-prerelease");
        let (install, staged, sidecar_path) = paths(&dir);
        let key = signing_key();
        let bytes = b"the release bytes";
        let manifest = manifest_for("2.0.3-rc.1", "canary", bytes, &key, "2.0.0");

        let result = stage(bytes, &manifest, &key.verifying_key(), &install);

        assert!(
            matches!(result, Err(StageError::Path)),
            "phase 1 is strict x.y.z, got {result:?}"
        );
        assert_nothing_staged(&dir, &staged, &sidecar_path);
    }

    #[test]
    fn an_unknown_channel_is_refused() {
        let key = signing_key();
        let bytes = b"the release bytes";

        for (index, channel) in ["beta", "stable ", "STABLE", ""].into_iter().enumerate() {
            let dir = TempDir::new(&format!("stage-channel-{index}"));
            let (install, staged, sidecar_path) = paths(&dir);
            let manifest = manifest_for("2.0.3", channel, bytes, &key, "2.0.0");

            let result = stage(bytes, &manifest, &key.verifying_key(), &install);

            assert!(
                matches!(result, Err(StageError::Channel)),
                "{channel:?} must be a Channel refusal, got {result:?}"
            );
            assert_nothing_staged(&dir, &staged, &sidecar_path);
        }
    }

    #[test]
    fn the_channel_is_part_of_the_signature() {
        let dir = TempDir::new("stage-channel-signature");
        let (install, staged, sidecar_path) = paths(&dir);
        let key = signing_key();
        let bytes = b"the release bytes";
        let mut manifest = manifest_for("2.0.3", "stable", bytes, &key, "2.0.0");
        // Both channels are valid; only the signed one may be accepted.
        manifest.channel = "canary".to_owned();

        let result = stage(bytes, &manifest, &key.verifying_key(), &install);

        assert!(
            matches!(result, Err(StageError::Signature)),
            "moving a release between valid channels must break the signature: {result:?}"
        );
        assert_nothing_staged(&dir, &staged, &sidecar_path);
    }

    #[test]
    fn the_signature_is_checked_before_the_digest_and_the_version() {
        let key = signing_key();

        // A forged signature wins over a wrong digest and a below-minimum version: the signature is
        // the gate, and the order is what makes the error honest.
        let dir = TempDir::new("stage-order-signature");
        let (install, staged, sidecar_path) = paths(&dir);
        let mut manifest = manifest_for("2.0.2", "stable", b"expected", &key, "9.9.9");
        manifest.signature = "00".repeat(64);
        let result = stage(b"tampered", &manifest, &key.verifying_key(), &install);
        assert!(matches!(result, Err(StageError::Signature)), "{result:?}");
        assert_nothing_staged(&dir, &staged, &sidecar_path);

        // A genuine signature over a digest that does not match the bytes wins over the version:
        // the bytes are verified before the version policy is consulted.
        let dir = TempDir::new("stage-order-sha");
        let (install, staged, sidecar_path) = paths(&dir);
        let manifest = manifest_for("2.0.2", "stable", b"expected", &key, "9.9.9");
        let result = stage(b"tampered", &manifest, &key.verifying_key(), &install);
        assert!(matches!(result, Err(StageError::Sha)), "{result:?}");
        assert_nothing_staged(&dir, &staged, &sidecar_path);
    }

    #[test]
    fn staging_overwrites_a_previous_pair() {
        let dir = TempDir::new("stage-overwrite");
        let (install, staged, sidecar_path) = paths(&dir);
        fs::create_dir_all(staged.parent().expect("the staged path has a parent"))
            .expect("the fixture directory is creatable");
        fs::write(&staged, b"an older artifact").expect("the old artifact is writable");
        fs::write(&sidecar_path, b"{\"version\":\"1.0.0\"}").expect("the old sidecar is writable");
        let key = signing_key();
        let bytes = b"the new artifact";
        let manifest = manifest_for("2.0.4", "canary", bytes, &key, "2.0.0");

        stage(bytes, &manifest, &key.verifying_key(), &install)
            .expect("a valid release replaces the pair");

        assert_eq!(fs::read(&staged).expect("the artifact is readable"), bytes);
        let sidecar: LauncherSidecar =
            serde_json::from_slice(&fs::read(&sidecar_path).expect("the sidecar is readable"))
                .expect("the replacement is a launcher sidecar");
        assert_eq!(sidecar.version, "2.0.4");
        assert_eq!(sidecar.channel, "canary");
    }

    #[test]
    fn staging_writes_only_the_canonical_pair_under_the_install() {
        let dir = TempDir::new("stage-paths-not-an-input");
        let install = dir.join("install");
        // The artifact lives nowhere near the install: `stage` reads it, and the only writes it can
        // perform are the two names below.
        let artifact = dir.join("download/summrise-device.new.exe");
        fs::create_dir_all(
            artifact
                .parent()
                .expect("the download directory has a parent"),
        )
        .expect("the download directory is creatable");
        let key = signing_key();
        let bytes = b"the downloaded artifact";
        fs::write(&artifact, bytes).expect("the fixture artifact is writable");
        let manifest = manifest_for("2.0.3", "stable", bytes, &key, "2.0.0");

        stage(bytes, &manifest, &key.verifying_key(), &install).expect("a valid release stages");

        let mut names: Vec<String> = fs::read_dir(&install)
            .expect("the install exists after a successful stage")
            .map(|entry| {
                entry
                    .expect("the install entry is readable")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        assert_eq!(
            names,
            [STAGED_EXE.to_owned(), STAGED_SIDECAR.to_owned()],
            "the stager may write the staged pair and nothing else"
        );
        assert_eq!(
            fs::read(&artifact).expect("the artifact is still readable"),
            bytes,
            "the source artifact is read, never a destination"
        );
        assert!(
            !swap_request_path(&install).exists(),
            "asking the supervisor to swap is a separate step; a stage is not a swap"
        );
    }

    #[test]
    fn a_request_is_what_the_launcher_parses_and_a_stale_result_is_removed() {
        let dir = TempDir::new("request-wire");
        let install = dir.join("install");
        fs::create_dir_all(&install).expect("the install directory is creatable");
        fs::write(
            swap_result_path(&install),
            br#"{"outcome":"applied","version":"1.0.0","at_ms":100}"#,
        )
        .expect("the stale result is writable");
        let sha256 = "ab".repeat(32);
        let release = StagedRelease {
            version: "2.0.3".to_owned(),
            sha256,
        };

        let requested_at_ms =
            request_swap(&install, &release).expect("the request is written atomically");

        assert!(
            requested_at_ms > 100,
            "the fixture result predates this request"
        );
        assert!(
            !swap_result_path(&install).exists(),
            "a result written before the request must not be read as this swap's answer"
        );
        let bytes = fs::read(swap_request_path(&install)).expect("the request is readable");
        match parse_swap_request(&bytes, requested_at_ms) {
            SwapRequestState::Valid(request) => {
                assert_eq!(request.requested_at_ms, requested_at_ms);
                assert_eq!(request.version, "2.0.3");
                assert_eq!(request.sha256, release.sha256);
            }
            other => panic!("the launcher must parse what the app wrote: {other:?}"),
        }
        let document: serde_json::Value =
            serde_json::from_slice(&bytes).expect("the request is JSON");
        let mut keys: Vec<&str> = document
            .as_object()
            .expect("the request is an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["requested_at_ms", "sha256", "version"]);
    }

    #[test]
    fn a_result_is_the_supervisors_contract_and_its_exit_code() {
        let dir = TempDir::new("result-wire");
        let install = dir.join("install");
        fs::create_dir_all(&install).expect("the install directory is creatable");
        let sha256 = "ab".repeat(32);
        let cases = [
            (
                format!(
                    r#"{{"outcome":"applied","version":"2.0.3","sha256":"{sha256}","at_ms":200}}"#
                ),
                SwapOutcome::Applied,
                Some("2.0.3"),
                Some(sha256.as_str()),
                0,
            ),
            (
                format!(
                    r#"{{"outcome":"rolled_back","version":"2.0.3","sha256":"{sha256}","at_ms":200,"reason":"no health"}}"#
                ),
                SwapOutcome::RolledBack,
                Some("2.0.3"),
                Some(sha256.as_str()),
                3,
            ),
            (
                r#"{"outcome":"refused","at_ms":200,"reason":"bad signature"}"#.to_owned(),
                SwapOutcome::Refused,
                None,
                None,
                4,
            ),
            (
                format!(
                    r#"{{"outcome":"refused","sha256":"{sha256}","at_ms":200,"reason":"the request named another version"}}"#
                ),
                SwapOutcome::Refused,
                None,
                Some(sha256.as_str()),
                4,
            ),
        ];

        for (document, outcome, version, sha, code) in cases {
            fs::write(swap_result_path(&install), &document).expect("the result is writable");
            let result = await_swap_result(&install, 100, Duration::from_millis(50))
                .expect("the result is this request's answer");

            assert_eq!(result.outcome, outcome, "{document}");
            assert_eq!(result.version.as_deref(), version, "{document}");
            assert_eq!(result.sha256.as_deref(), sha, "{document}");
            assert_eq!(result.outcome.exit_code(), code, "{document}");
        }

        for document in [
            r#"{"outcome":"applied","version":"2.0.3","at_ms":200,"extra":true}"#,
            r#"{"outcome":"applied","version":"2.0.3"}"#,
            r#"{"outcome":"unknown","at_ms":200}"#,
        ] {
            assert!(
                serde_json::from_str::<SwapResult>(document).is_err(),
                "a result the launcher never writes must not parse: {document}"
            );
        }
    }

    #[test]
    fn waiting_ignores_a_result_older_than_the_request() {
        let dir = TempDir::new("result-stale");
        let install = dir.join("install");
        fs::create_dir_all(&install).expect("the install directory is creatable");
        fs::write(
            swap_result_path(&install),
            br#"{"outcome":"applied","version":"1.0.0","at_ms":50}"#,
        )
        .expect("the stale result is writable");

        let result = await_swap_result(&install, 100, Duration::from_millis(250));

        assert!(
            matches!(result, Err(StageError::NoResult(_))),
            "a result from before the request is not an answer: {result:?}"
        );
        assert!(
            !swap_result_path(&install).exists(),
            "the stale result is discarded while waiting"
        );
    }

    #[test]
    fn an_unanswered_request_times_out_instead_of_waiting_forever() {
        let dir = TempDir::new("result-timeout");
        let install = dir.join("install");
        fs::create_dir_all(&install).expect("the install directory is creatable");

        let result = await_swap_result(&install, 100, Duration::from_millis(50));

        assert!(
            matches!(result, Err(StageError::NoResult(ref detail)) if detail.contains("50 ms")),
            "{result:?}"
        );
    }

    #[test]
    fn a_valid_public_key_parses() {
        let key = signing_key();
        let hex = hex_encode(&key.verifying_key().to_bytes());
        assert_eq!(parse_pubkey(&hex), Ok(key.verifying_key()));
    }

    #[test]
    fn a_malformed_public_key_is_named_as_such() {
        for hex in ["", "zz", "0011", "00", &"0".repeat(63)] {
            assert_eq!(parse_pubkey(hex), Err(KeyError::BadKey), "{hex:?}");
        }
    }

    #[test]
    fn the_placeholder_key_is_refused_by_name_without_panicking() {
        assert_eq!(
            parse_pubkey(PLACEHOLDER_RELEASE_PUBKEY_HEX),
            Err(KeyError::Placeholder),
            "a placeholder build must say so, not report a malformed key"
        );
        assert_eq!(
            parse_pubkey(&PLACEHOLDER_RELEASE_PUBKEY_HEX.to_uppercase()),
            Err(KeyError::Placeholder),
            "the refusal does not depend on hex case"
        );

        match option_env!("SUMMRISE_RELEASE_PUBKEY") {
            None => assert_eq!(
                release_pubkey(),
                Err(KeyError::Placeholder),
                "an unconfigured build fails closed instead of panicking"
            ),
            Some(_) => {
                // The seam test below covers the configured case.
            }
        }
    }

    #[test]
    fn the_build_time_key_seam_is_what_the_binary_embeds() {
        match option_env!("SUMMRISE_RELEASE_PUBKEY") {
            Some(key) => {
                assert_eq!(RELEASE_PUBKEY_HEX, key);
                assert!(release_pubkey().is_ok(), "the configured key parses");
            }
            None => {
                assert_eq!(RELEASE_PUBKEY_HEX, PLACEHOLDER_RELEASE_PUBKEY_HEX);
                assert_eq!(release_pubkey(), Err(KeyError::Placeholder));
            }
        }
    }
}
