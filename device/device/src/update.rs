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
//! Only after all five pass does [`stage`] write the artifact and its sidecar. A refused update
//! leaves the filesystem as it was: no artifact, no sidecar, no directory.
//!
//! **The layout decides the destination (2026-11-05 design).** [`stage`] takes the install root, not
//! two output paths, and reads the one difference between the two layouts: `<install>/current.txt`.
//!
//! - Without it, the pre-versioned pair is written: `<install>/summrise-device.new.exe` and
//!   `<install>/summrise-device.new.json`. [`request_swap`] then writes the request the supervisor
//!   polls, and [`await_swap_result`] reads the answer.
//! - With it, the verified pair goes into `<install>/versions/<version>/` — never over an existing
//!   version directory with different bytes — and [`hand_off`] runs the launcher's
//!   `--install-version` (or `--apply` when the service must restart). A version directory is built
//!   under a temporary name and renamed into place, so a refused download leaves no half-version.
//!
//! The app binary, the baseline and the launcher's other files are unreachable from here by
//! construction. The launcher re-verifies the sidecar before it installs, so signing the channel into
//! the message means a canary artifact cannot be replayed as a stable one.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use summrise_core::manifest::{Manifest, Version};

/// The staged artifact's file name under the install root.
pub const STAGED_EXE: &str = "summrise-device.new.exe";

/// The staged artifact's sidecar file name under the install root.
pub const STAGED_SIDECAR: &str = "summrise-device.new.json";

/// The app binary's name inside a version directory and under a flat install.
pub const APP_EXE: &str = "summrise-device.exe";

/// The signed sidecar's name inside a version directory.
pub const VERSION_SIDECAR: &str = "summrise-device.json";

/// `<install>/versions` — one directory per release, none of them mutable once named.
pub const VERSIONS_DIR: &str = "versions";

/// `<install>/current.txt` — the pointer that makes an install versioned.
pub const CURRENT_POINTER: &str = "current.txt";

/// The stable launcher beside the app; the updater asks it to install a version.
pub const LAUNCHER_EXE: &str = "summrise-launch.exe";

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
    /// A version directory already exists, and its bytes are not this release.
    VersionConflict(String),
    /// The launcher ran and did not install the version; `reason` is what it said.
    Launcher {
        /// The process's exit code, when it exited on its own.
        code: Option<i32>,
        /// The launcher's own sentence, already prefixed by it.
        reason: String,
    },
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
            StageError::VersionConflict(detail) => write!(f, "{detail}"),
            StageError::Launcher { code, reason } => match code {
                Some(code) => write!(
                    f,
                    "the launcher refused the version (exit {code}): {reason}"
                ),
                None => write!(f, "the launcher refused the version: {reason}"),
            },
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

/// Which of the two layouts an install uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// No `<install>/current.txt`: the flat pair plus `swap.request.json` (pre-versioned).
    Legacy,
    /// `<install>/current.txt` exists: one immutable directory per release under `versions/`.
    Versioned,
}

/// Detect the layout by the one file that distinguishes them.
///
/// A pointer that exists but cannot be inspected is still not absent: the launcher refuses the
/// legacy swap unless the pointer is missing, so treating a permission error as "legacy" would pick
/// the one path the launcher will not run. The error makes this a versioned install; the launcher
/// then reports the pointer's own problem.
pub fn layout(install: &Path) -> Layout {
    match fs::metadata(install.join(CURRENT_POINTER)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Layout::Legacy,
        _ => Layout::Versioned,
    }
}

/// Where a successful [`stage`] put the verified pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Staged {
    /// The pre-versioned pair beside the install's flat app: what the supervisor polls for.
    Pair {
        /// `<install>/summrise-device.new.exe`.
        exe: PathBuf,
        /// `<install>/summrise-device.new.json`.
        sidecar: PathBuf,
    },
    /// One release's immutable directory, named by `<install>/current.txt` once installed.
    Version {
        /// `<install>/versions/<version>`.
        dir: PathBuf,
        /// `<install>/versions/<version>/summrise-device.json`.
        sidecar: PathBuf,
    },
}

/// The release facts a successful stage produced, and the handoff needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedRelease {
    /// The version the signed manifest names.
    pub version: String,
    /// The digest the signed manifest names (as written, lower- or upper-hex).
    pub sha256: String,
    /// The files the release was written to, which decide how it is handed to the launcher.
    pub staged: Staged,
}

impl StagedRelease {
    /// A release already staged as the legacy pair under `install`.
    ///
    /// A caller that did not run [`stage`] itself (a test fixture, or a durable fact read from disk)
    /// can still name the pair the supervisor consumes without inventing a version directory.
    #[cfg(test)]
    pub fn pair(install: &Path, version: impl Into<String>, sha256: impl Into<String>) -> Self {
        Self {
            version: version.into(),
            sha256: sha256.into(),
            staged: Staged::Pair {
                exe: staged_exe_path(install),
                sidecar: staged_sidecar_path(install),
            },
        }
    }
}

/// Verify a release and, only if every check passes, write it where this install boots from.
///
/// The destination is **not an argument** (fix design F3): both paths are derived from `install`,
/// and [`layout`] chooses between the flat pair and `versions/<version>/`. The artifact bytes are
/// held in memory: the caller has already downloaded them and bounded their size. On any refusal
/// the filesystem is untouched. A version directory is built under a temporary sibling and renamed
/// into place only after both files are written, so no refusal or crash leaves a half-version.
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

    // All checks have passed. The layout decides where: a version directory is built whole and
    // renamed into place; the legacy pair is written staged first and sidecar last, because the
    // sidecar's presence is what marks a complete pair.
    let staged = match layout(install) {
        Layout::Legacy => {
            write_atomic(&staged_exe_path(install), bytes)?;
            write_atomic(&staged_sidecar_path(install), &sidecar_json)?;
            Staged::Pair {
                exe: staged_exe_path(install),
                sidecar: staged_sidecar_path(install),
            }
        }
        Layout::Versioned => stage_version(install, &manifest.version, bytes, &sidecar_json)?,
    };
    Ok(StagedRelease {
        version: manifest.version.clone(),
        sha256: manifest.sha256.clone(),
        staged,
    })
}

/// `<install>/versions` — the parent of every version directory.
pub fn versions_dir(install: &Path) -> PathBuf {
    install.join(VERSIONS_DIR)
}

/// `<install>/versions/<name>` — one release's directory.
pub fn version_dir(install: &Path, name: &str) -> PathBuf {
    versions_dir(install).join(name)
}

/// `<install>/versions/<name>/summrise-device.exe`.
pub fn version_exe(install: &Path, name: &str) -> PathBuf {
    version_dir(install, name).join(APP_EXE)
}

/// `<install>/versions/<name>/summrise-device.json`.
pub fn version_sidecar_path(install: &Path, name: &str) -> PathBuf {
    version_dir(install, name).join(VERSION_SIDECAR)
}

/// `<install>/summrise-launch.exe` — the stable launcher the handoff runs.
pub fn launcher_exe(install: &Path) -> PathBuf {
    install.join(LAUNCHER_EXE)
}

/// Put the verified pair into an immutable version directory, or reuse one that is already there.
///
/// The directory is built under a random temporary sibling and renamed into place once both files
/// are written: a refused download never reaches this function, and a write that fails leaves no
/// `versions/<version>/` behind. An existing directory is reused only when its bytes are this
/// release's; different bytes are refused, because a version directory may be running.
fn stage_version(
    install: &Path,
    name: &str,
    bytes: &[u8],
    sidecar_json: &[u8],
) -> Result<Staged, StageError> {
    let versions = versions_dir(install);
    let dir = version_dir(install, name);
    let exe = version_exe(install, name);
    let sidecar = version_sidecar_path(install, name);

    // A version directory is a real directory here. A symbolic link with this name would make the
    // sidecar write land wherever it points, which is a second destination the stager must not have.
    match fs::symlink_metadata(&dir) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(StageError::VersionConflict(format!(
                "{} is a symbolic link; refusing to write a version through it",
                dir.display()
            )));
        }
        Ok(_) => return reuse_version(name, &dir, &exe, &sidecar, bytes, sidecar_json),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(StageError::Io(error)),
    }

    fs::create_dir_all(&versions)?;
    let temporary = crate::fsutil::create_new_dir(&dir)?;
    let built = (|| -> io::Result<()> {
        write_atomic(&temporary.join(APP_EXE), bytes)?;
        write_atomic(&temporary.join(VERSION_SIDECAR), sidecar_json)?;
        fs::rename(&temporary, &dir)
    })();
    match built {
        Ok(()) => Ok(Staged::Version { dir, sidecar }),
        Err(error) => {
            let _ = fs::remove_dir_all(&temporary);
            // Another stager may have named the version between the check and the rename. The
            // winner's directory is immutable like ours, so reuse it only if it holds our bytes.
            if error.kind() == io::ErrorKind::AlreadyExists && dir.exists() {
                return reuse_version(name, &dir, &exe, &sidecar, bytes, sidecar_json);
            }
            Err(StageError::Io(error))
        }
    }
}

/// Reuse `<install>/versions/<name>` when it already holds exactly this release.
fn reuse_version(
    name: &str,
    dir: &Path,
    exe: &Path,
    sidecar: &Path,
    bytes: &[u8],
    sidecar_json: &[u8],
) -> Result<Staged, StageError> {
    if !dir.is_dir() {
        return Err(StageError::VersionConflict(format!(
            "{} exists and is not a version directory; refusing to write through it",
            dir.display()
        )));
    }

    match fs::read(exe) {
        Ok(existing) if existing == bytes => {}
        Ok(_) => {
            return Err(StageError::VersionConflict(format!(
                "version {name:?} is already staged at {} with different bytes; refusing to \
                 overwrite an immutable version directory",
                dir.display()
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(StageError::VersionConflict(format!(
                "version {name:?} exists at {} without its app binary; refusing to write into an \
                 incomplete version directory",
                dir.display()
            )));
        }
        Err(error) => return Err(StageError::Io(error)),
    }

    // The exe is complete, so a missing sidecar is the tail of an interrupted install and may be
    // completed; a sidecar naming another release is a different version and is refused.
    match fs::read(sidecar) {
        Ok(existing) if sidecars_match(&existing, sidecar_json) => {}
        Ok(_) => {
            return Err(StageError::VersionConflict(format!(
                "version {name:?} is already staged at {} with a different sidecar; refusing to \
                 rewrite an immutable version directory",
                dir.display()
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            write_atomic(sidecar, sidecar_json)?;
        }
        Err(error) => return Err(StageError::Io(error)),
    }
    Ok(Staged::Version {
        dir: dir.to_path_buf(),
        sidecar: sidecar.to_path_buf(),
    })
}

/// Do two sidecars carry the same signed facts? Formatting and key order do not matter.
fn sidecars_match(existing: &[u8], expected: &[u8]) -> bool {
    match (
        serde_json::from_slice::<Sidecar>(existing),
        serde_json::from_slice::<Sidecar>(expected),
    ) {
        (Ok(existing), Ok(expected)) => existing == expected,
        _ => existing == expected,
    }
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

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// The handoff: the launcher installs a version, or the supervisor polls a request
// ─────────────────────────────────────────────────────────────────────────────────────────────────

/// Which launcher mode installs a staged version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LauncherMode {
    /// `--install-version <dir> <sidecar>`: move the pointer; the owner restarts the child.
    InstallVersion,
    /// `--apply <dir> <sidecar>`: move the pointer and restart the service now.
    Apply,
}

impl LauncherMode {
    /// The mode's command-line flag.
    pub fn flag(self) -> &'static str {
        match self {
            LauncherMode::InstallVersion => "--install-version",
            LauncherMode::Apply => "--apply",
        }
    }
}

/// The options a handoff needs that are not in the release.
///
/// The two layouts need different answers, and only the caller knows whether the service must be
/// restarted as part of this call:
///
/// - a supervised app is restarted by its supervisor once it exits, so [`HandoffOptions::pointer`]
///   (`--install-version`) is enough and does not stop the running service;
/// - an operator-facing one-shot (`--stage`, `--check-update`) must leave the new version running,
///   so [`HandoffOptions::restart`] (`--apply`) stops and restarts the owner.
///
/// A legacy install ignores both: it keeps the supervisor's `swap.request.json` handoff exactly as
/// it was.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HandoffOptions {
    /// Restart the service now (`--apply`) instead of only moving the pointer.
    pub restart: bool,
    /// The data directory the launcher's candidate probe copies; `None` uses its default.
    pub data: Option<PathBuf>,
    /// The port the restarted service is health-checked on; `None` uses the launcher's default.
    pub port: Option<u16>,
}

impl HandoffOptions {
    /// Pointer-only: the one a supervised app uses before it exits for its restart.
    pub fn pointer() -> Self {
        Self::default()
    }

    /// The service-restarting handoff (`--apply`).
    pub fn restart() -> Self {
        Self {
            restart: true,
            ..Self::default()
        }
    }

    /// The data directory the launcher's probe must copy.
    pub fn with_data(mut self, data: Option<PathBuf>) -> Self {
        self.data = data;
        self
    }

    /// The port the restarted service serves on.
    pub fn with_port(mut self, port: Option<u16>) -> Self {
        self.port = port;
        self
    }
}

/// One launcher invocation, as the stager decided it.
///
/// This is a value, not a side effect, so a test can assert exactly which mode and paths the stager
/// chose without starting a process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LauncherCall {
    /// `<install>/summrise-launch.exe`.
    pub launcher: PathBuf,
    /// The mode that installs the version.
    pub mode: LauncherMode,
    /// The install root the launcher is told with `--install`.
    pub install: PathBuf,
    /// `<install>/versions/<name>`.
    pub version_dir: PathBuf,
    /// `<install>/versions/<name>/summrise-device.json`.
    pub sidecar: PathBuf,
    /// The data directory (`--data`), when the caller knows it.
    pub data: Option<PathBuf>,
    /// The service port (`--port`), when the caller knows it.
    pub port: Option<u16>,
}

impl LauncherCall {
    /// The argv after the launcher's own path.
    pub fn arguments(&self) -> Vec<OsString> {
        let mut arguments = vec![
            OsString::from(self.mode.flag()),
            self.version_dir.as_os_str().to_owned(),
            self.sidecar.as_os_str().to_owned(),
            OsString::from("--install"),
            self.install.as_os_str().to_owned(),
        ];
        if let Some(data) = &self.data {
            arguments.push(OsString::from("--data"));
            arguments.push(data.as_os_str().to_owned());
        }
        if let Some(port) = self.port {
            arguments.push(OsString::from("--port"));
            arguments.push(OsString::from(port.to_string()));
        }
        arguments
    }
}

/// Runs one launcher invocation. The seam that lets the stager be tested without a process.
pub trait Launcher {
    /// Install the version named by `call`; `Ok(())` means the launcher exited successfully.
    fn perform(&self, call: &LauncherCall) -> Result<(), StageError>;
}

/// The real launcher: the stable binary beside the app.
pub struct LauncherProcess;

impl Launcher for LauncherProcess {
    fn perform(&self, call: &LauncherCall) -> Result<(), StageError> {
        // The launcher's stderr is captured through a file, not a pipe. `--apply` may leave a
        // console-mode child running with the inherited handle, and a pipe would keep
        // `Command::output` waiting for an EOF that arrives when that child finally exits.
        let (stderr_path, stderr_file) = crate::fsutil::create_new_sibling(
            &call.install.join("launcher.stderr"),
        )
        .map_err(|error| {
            StageError::Io(io::Error::new(
                error.kind(),
                format!(
                    "cannot create a file for the launcher's answer in {}: {error}",
                    call.install.display()
                ),
            ))
        })?;

        let status = Command::new(&call.launcher)
            .args(call.arguments())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr_file))
            .status();
        let reason = fs::read(&stderr_path)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_owned())
            .unwrap_or_default();
        let _ = fs::remove_file(&stderr_path);
        let status = status.map_err(|error| {
            StageError::Io(io::Error::new(
                error.kind(),
                format!(
                    "cannot run the launcher {}: {error}",
                    call.launcher.display()
                ),
            ))
        })?;
        if status.success() {
            return Ok(());
        }
        Err(StageError::Launcher {
            code: status.code(),
            reason: if reason.is_empty() {
                format!("the launcher exited with {status}")
            } else {
                reason
            },
        })
    }
}

/// What a successful handoff did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Handoff {
    /// A legacy install: the request the supervisor polls was written at this time.
    Swap {
        /// The request's timestamp, which [`await_swap_result`] needs.
        requested_at_ms: u64,
    },
    /// A versioned install: the launcher installed the version in this mode.
    Installed {
        /// The mode the launcher ran.
        mode: LauncherMode,
    },
}

/// Hand a staged release to whatever installs it on this layout.
///
/// A legacy install writes `swap.request.json` exactly as before. A versioned install runs the
/// launcher's `--install-version` (or `--apply`), and the launcher's refusal is returned with its
/// own reason. No failure path here removes the staged version or touches `current.txt`: the
/// launcher owns the pointer, and a version directory that failed its probe stays on disk for the
/// next attempt or an operator's inspection.
pub fn hand_off(
    install: &Path,
    release: &StagedRelease,
    options: &HandoffOptions,
) -> Result<Handoff, StageError> {
    hand_off_with(install, release, options, &LauncherProcess)
}

/// [`hand_off`] with the launcher supplied, so a test can assert the call without a process.
pub fn hand_off_with(
    install: &Path,
    release: &StagedRelease,
    options: &HandoffOptions,
    launcher: &dyn Launcher,
) -> Result<Handoff, StageError> {
    let (version_dir, sidecar) = match &release.staged {
        Staged::Pair { .. } => {
            return Ok(Handoff::Swap {
                requested_at_ms: request_swap(install, release)?,
            });
        }
        Staged::Version { dir, sidecar } => (dir.clone(), sidecar.clone()),
    };

    let mode = if options.restart {
        LauncherMode::Apply
    } else {
        LauncherMode::InstallVersion
    };
    let call = LauncherCall {
        launcher: launcher_exe(install),
        mode,
        install: install.to_path_buf(),
        version_dir,
        sidecar,
        data: options.data.clone(),
        port: options.port,
    };
    launcher.perform(&call)?;
    Ok(Handoff::Installed { mode })
}

/// Millisecond Unix time, saturating at the epoch for a clock before it.
pub(crate) fn now_ms() -> u64 {
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

    /// An install whose launcher owns `<install>/current.txt`: the versioned layout.
    fn pointer_install(dir: &TempDir, current: &str) -> PathBuf {
        let install = dir.join("install");
        fs::create_dir_all(&install).expect("the install directory is creatable");
        fs::write(install.join("current.txt"), format!("{current}\n"))
            .expect("the pointer is writable");
        install
    }

    /// The entries of `versions/`, or none when the directory does not exist.
    fn version_names(install: &Path) -> Vec<String> {
        match fs::read_dir(install.join("versions")) {
            Ok(entries) => {
                let mut names: Vec<String> = entries
                    .map(|entry| {
                        entry
                            .expect("the version entry is readable")
                            .file_name()
                            .to_string_lossy()
                            .into_owned()
                    })
                    .collect();
                names.sort();
                names
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("cannot list versions/: {error}"),
        }
    }

    #[test]
    fn a_pointer_managed_install_stages_into_a_version_directory() {
        let dir = TempDir::new("stage-versioned");
        let install = pointer_install(&dir, "1.0.0");
        let key = signing_key();
        let bytes = b"the release bytes";
        let manifest = manifest_for("2.0.3", "stable", bytes, &key, "2.0.0");

        let released = stage(bytes, &manifest, &key.verifying_key(), &install)
            .expect("a pointer-managed install stages");

        let version_dir = install.join("versions").join("2.0.3");
        assert_eq!(
            fs::read(version_dir.join("summrise-device.exe")).expect("the version exe is readable"),
            bytes,
            "the verified artifact is the version directory's exe"
        );
        let sidecar: LauncherSidecar = serde_json::from_slice(
            &fs::read(version_dir.join("summrise-device.json"))
                .expect("the version sidecar is readable"),
        )
        .expect("the launcher's Sidecar reads what the stager wrote");
        assert_eq!(sidecar.version, "2.0.3");
        assert_eq!(sidecar.sha256, manifest.sha256);
        assert_eq!(
            launcher_verify(&sidecar, &key.verifying_key()),
            Ok(()),
            "the launcher must accept the sidecar the stager wrote"
        );
        assert_eq!(released.version, "2.0.3");
        assert_eq!(released.sha256, manifest.sha256);
        assert!(
            !install.join(STAGED_EXE).exists() && !install.join(STAGED_SIDECAR).exists(),
            "the versioned layout does not write the legacy flat pair"
        );
        assert_eq!(
            fs::read_to_string(install.join("current.txt")).expect("the pointer is readable"),
            "1.0.0\n",
            "staging is not a switch: the pointer stays where it was"
        );
        assert_eq!(
            version_names(&install),
            ["2.0.3".to_owned()],
            "a completed stage leaves no half-version behind"
        );
    }

    #[test]
    fn a_second_identical_stage_reuses_the_version_directory() {
        let dir = TempDir::new("stage-versioned-reuse");
        let install = pointer_install(&dir, "1.0.0");
        let key = signing_key();
        let bytes = b"the release bytes";
        let manifest = manifest_for("2.0.3", "stable", bytes, &key, "2.0.0");

        let first = stage(bytes, &manifest, &key.verifying_key(), &install)
            .expect("the first stage writes the version");
        let second = stage(bytes, &manifest, &key.verifying_key(), &install)
            .expect("the same bytes may be staged again");

        assert_eq!(
            first, second,
            "the same version and digest is the same staged release"
        );
        assert_eq!(
            version_names(&install),
            ["2.0.3".to_owned()],
            "a re-stage reuses the directory instead of writing a second one"
        );
        assert_eq!(
            fs::read(install.join("versions/2.0.3/summrise-device.exe"))
                .expect("the version exe is readable"),
            bytes
        );
    }

    #[test]
    fn a_refused_download_leaves_no_half_version() {
        let dir = TempDir::new("stage-versioned-bad-sha");
        let install = pointer_install(&dir, "1.0.0");
        let key = signing_key();
        let manifest = manifest_for("2.0.3", "stable", b"expected bytes", &key, "2.0.0");

        let result = stage(b"tampered bytes", &manifest, &key.verifying_key(), &install);

        assert!(matches!(result, Err(StageError::Sha)), "{result:?}");
        assert!(
            version_names(&install).is_empty(),
            "a refused download writes no version directory at all"
        );
        assert_eq!(
            fs::read_to_string(install.join("current.txt")).expect("the pointer is readable"),
            "1.0.0\n",
            "a refusal never moves the pointer"
        );
    }

    #[test]
    fn an_existing_version_with_different_bytes_is_never_overwritten() {
        let dir = TempDir::new("stage-versioned-immutable");
        let install = pointer_install(&dir, "1.0.0");
        let key = signing_key();
        let original = b"the release bytes";
        let manifest = manifest_for("2.0.3", "stable", original, &key, "2.0.0");
        stage(original, &manifest, &key.verifying_key(), &install)
            .expect("the first stage writes the version");

        // The same version rebuilt with different bytes: a valid signature, but not the bytes that
        // are already installed. A version directory may be running, so it is never overwritten.
        let rebuilt = b"the rebuilt bytes";
        let rebuilt_manifest = manifest_for("2.0.3", "stable", rebuilt, &key, "2.0.0");
        let result = stage(rebuilt, &rebuilt_manifest, &key.verifying_key(), &install);

        assert!(
            matches!(result, Err(StageError::VersionConflict(_))),
            "different bytes at the same version must be refused: {result:?}"
        );
        assert_eq!(
            fs::read(install.join("versions/2.0.3/summrise-device.exe"))
                .expect("the installed version is readable"),
            original,
            "the installed version's bytes are untouched"
        );
        assert_eq!(version_names(&install), ["2.0.3".to_owned()]);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_version_directory_is_refused() {
        let dir = TempDir::new("stage-versioned-symlink");
        let install = pointer_install(&dir, "1.0.0");
        let target = dir.join("elsewhere");
        fs::create_dir_all(&target).expect("the link target is creatable");
        fs::create_dir_all(install.join(VERSIONS_DIR)).expect("versions/ is creatable");
        std::os::unix::fs::symlink(&target, install.join("versions").join("2.0.3"))
            .expect("the fixture link is creatable");
        let key = signing_key();
        let bytes = b"the release bytes";
        let manifest = manifest_for("2.0.3", "stable", bytes, &key, "2.0.0");

        let result = stage(bytes, &manifest, &key.verifying_key(), &install);

        assert!(
            matches!(result, Err(StageError::VersionConflict(_))),
            "a version directory may not be a link to somewhere else: {result:?}"
        );
        assert!(
            !target.join(APP_EXE).exists(),
            "the stager wrote nothing through the link"
        );
    }

    /// A launcher that records the call it was asked to run.
    #[derive(Default)]
    struct RecordingLauncher {
        calls: std::cell::RefCell<Vec<LauncherCall>>,
    }

    impl Launcher for RecordingLauncher {
        fn perform(&self, call: &LauncherCall) -> Result<(), StageError> {
            self.calls.borrow_mut().push(call.clone());
            Ok(())
        }
    }

    /// A launcher that refuses the version, as a failed health probe does.
    struct RefusingLauncher;

    impl Launcher for RefusingLauncher {
        fn perform(&self, _call: &LauncherCall) -> Result<(), StageError> {
            Err(StageError::Launcher {
                code: Some(1),
                reason: "launch: version 2.0.3 failed its health check and was not made current"
                    .to_owned(),
            })
        }
    }

    fn argv(call: &LauncherCall) -> Vec<String> {
        call.arguments()
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn a_pointer_managed_install_hands_the_version_to_the_launcher() {
        let dir = TempDir::new("handoff-versioned");
        let install = pointer_install(&dir, "1.0.0");
        let data = dir.join("data");
        let key = signing_key();
        let bytes = b"the release bytes";
        let manifest = manifest_for("2.0.3", "stable", bytes, &key, "2.0.0");
        let released = stage(bytes, &manifest, &key.verifying_key(), &install)
            .expect("a pointer-managed install stages");

        let launcher = RecordingLauncher::default();
        let options = HandoffOptions::pointer()
            .with_data(Some(data.clone()))
            .with_port(Some(18_080));
        let handoff = hand_off_with(&install, &released, &options, &launcher)
            .expect("the launcher accepts the staged version");

        assert_eq!(
            handoff,
            Handoff::Installed {
                mode: LauncherMode::InstallVersion
            },
            "a supervised handoff only asks the launcher to move the pointer"
        );
        let calls = launcher.calls.borrow();
        assert_eq!(calls.len(), 1, "one handoff is one launcher call");
        let call = &calls[0];
        assert_eq!(call.launcher, install.join(LAUNCHER_EXE));
        assert_eq!(call.version_dir, install.join("versions").join("2.0.3"));
        assert_eq!(
            call.sidecar,
            install.join("versions").join("2.0.3").join(VERSION_SIDECAR)
        );
        assert_eq!(call.install, install);
        assert_eq!(call.data.as_deref(), Some(data.as_path()));
        assert_eq!(call.port, Some(18_080));

        let arguments = argv(call);
        assert_eq!(arguments[0], "--install-version");
        assert_eq!(arguments[1], call.version_dir.display().to_string());
        assert_eq!(arguments[2], call.sidecar.display().to_string());
        let expected = vec![
            "--install".to_owned(),
            install.display().to_string(),
            "--data".to_owned(),
            data.display().to_string(),
            "--port".to_owned(),
            "18080".to_owned(),
        ];
        assert_eq!(&arguments[3..], expected.as_slice());
        assert!(
            !swap_request_path(&install).exists(),
            "a versioned handoff never writes the legacy request"
        );
        assert_eq!(
            fs::read_to_string(install.join("current.txt")).expect("the pointer is readable"),
            "1.0.0\n",
            "the launcher moves the pointer; the stager's call does not"
        );
    }

    #[test]
    fn a_restart_required_hand_off_runs_apply() {
        let dir = TempDir::new("handoff-apply");
        let install = pointer_install(&dir, "1.0.0");
        let key = signing_key();
        let bytes = b"the release bytes";
        let manifest = manifest_for("2.0.3", "stable", bytes, &key, "2.0.0");
        let released = stage(bytes, &manifest, &key.verifying_key(), &install)
            .expect("a pointer-managed install stages");

        let launcher = RecordingLauncher::default();
        let handoff = hand_off_with(&install, &released, &HandoffOptions::restart(), &launcher)
            .expect("the launcher applies the staged version");

        assert_eq!(
            handoff,
            Handoff::Installed {
                mode: LauncherMode::Apply
            },
            "when the service must restart, the handoff is --apply"
        );
        assert_eq!(argv(&launcher.calls.borrow()[0])[0], "--apply");
    }

    #[test]
    fn a_legacy_install_still_writes_the_swap_request() {
        let dir = TempDir::new("handoff-legacy");
        let install = dir.join("install");
        let key = signing_key();
        let bytes = b"the release bytes";
        let manifest = manifest_for("2.0.3", "stable", bytes, &key, "2.0.0");
        let released = stage(bytes, &manifest, &key.verifying_key(), &install)
            .expect("a legacy install stages");

        let launcher = RecordingLauncher::default();
        let handoff = hand_off_with(&install, &released, &HandoffOptions::restart(), &launcher)
            .expect("a legacy stage still asks the supervisor");

        let Handoff::Swap { requested_at_ms } = handoff else {
            panic!("a legacy install has no launcher mode: {handoff:?}");
        };
        assert!(
            launcher.calls.borrow().is_empty(),
            "a legacy install never runs the version launcher"
        );
        assert!(
            !install.join(VERSIONS_DIR).exists(),
            "a legacy install writes no versions/ directory"
        );
        let request = fs::read(swap_request_path(&install)).expect("the request is readable");
        match parse_swap_request(&request, requested_at_ms) {
            SwapRequestState::Valid(request) => {
                assert_eq!(request.version, "2.0.3");
                assert_eq!(request.sha256, released.sha256);
            }
            other => panic!("the launcher must parse what the stager wrote: {other:?}"),
        }
    }

    #[test]
    fn a_refused_probe_keeps_the_staged_version_and_reports_the_reason() {
        let dir = TempDir::new("handoff-refused");
        let install = pointer_install(&dir, "1.0.0");
        let key = signing_key();
        let bytes = b"the release bytes";
        let manifest = manifest_for("2.0.3", "stable", bytes, &key, "2.0.0");
        let released = stage(bytes, &manifest, &key.verifying_key(), &install)
            .expect("a pointer-managed install stages");

        let error = hand_off_with(
            &install,
            &released,
            &HandoffOptions::pointer(),
            &RefusingLauncher,
        )
        .expect_err("a failed probe is not an install");

        assert!(
            error.to_string().contains("failed its health check"),
            "the launcher's reason is reported: {error}"
        );
        assert_eq!(
            fs::read(install.join("versions/2.0.3/summrise-device.exe"))
                .expect("the staged version is readable"),
            bytes,
            "a refused probe must not delete the staged version"
        );
        assert_eq!(
            fs::read_to_string(install.join("current.txt")).expect("the pointer is readable"),
            "1.0.0\n",
            "the pointer is the launcher's to move, and it did not move it"
        );
        assert!(
            !swap_request_path(&install).exists(),
            "a versioned handoff does not fall back to the legacy request"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_real_launcher_process_receives_the_mode_and_its_reason_is_reported() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new("handoff-process");
        let install = pointer_install(&dir, "1.0.0");
        let launcher_path = install.join(LAUNCHER_EXE);
        fs::write(
            &launcher_path,
            "#!/bin/sh\necho \"launch: refused $1\" >&2\nexit 1\n",
        )
        .expect("the fake launcher is writable");
        fs::set_permissions(&launcher_path, fs::Permissions::from_mode(0o755))
            .expect("the fake launcher is executable");
        let key = signing_key();
        let bytes = b"the release bytes";
        let manifest = manifest_for("2.0.3", "stable", bytes, &key, "2.0.0");
        let released = stage(bytes, &manifest, &key.verifying_key(), &install)
            .expect("a pointer-managed install stages");

        let error = hand_off(&install, &released, &HandoffOptions::pointer())
            .expect_err("the fake launcher refuses");

        let message = error.to_string();
        assert!(
            message.contains("launch: refused --install-version"),
            "the launcher is run in --install-version mode and its stderr is the reason: {message}"
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
        let release = StagedRelease::pair(&install, "2.0.3", sha256);

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
