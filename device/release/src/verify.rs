//! Offline verification of caller-trusted, signed artifact bytes.
//!
//! The existing sidecar binds version, channel and digest. It does not bind an artifact kind or
//! filename, validate Authenticode/installer payloads, or prove an installation succeeds.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::sign::hex_decode;
use crate::{version_is_plain, ArtifactSidecar, Channel, ReleaseDocument, ReleaseError};

/// The largest artifact accepted by the offline verifier: 128 MiB.
pub const MAX_ARTIFACT_BYTES: u64 = 128 * 1024 * 1024;
/// The largest sidecar accepted by the offline verifier: 64 KiB.
pub const MAX_SIDECAR_BYTES: u64 = 64 * 1024;
/// The native updater's default document bound, also used for offline document verification.
pub const MAX_DOCUMENT_BYTES: u64 = 1024 * 1024;

/// Why an artifact or its attestation could not be verified.
#[derive(Debug)]
pub enum ArtifactVerificationError {
    /// A local input could not be opened, read or inspected.
    Io { path: PathBuf, source: io::Error },
    /// Inputs must be regular files, not directories, links or special files.
    NotRegularFile { path: PathBuf },
    /// A local input is empty or exceeds its fixed bound.
    InvalidLength {
        path: PathBuf,
        bytes: u64,
        maximum: u64,
    },
    /// The input's length changed between inspection and the completed read.
    ChangedLength { path: PathBuf },
    /// The sidecar is not the existing strict JSON shape.
    Sidecar(serde_json::Error),
    /// The caller's expected version is not plain major.minor.patch.
    InvalidExpectedVersion,
    /// The sidecar version is not plain major.minor.patch.
    InvalidVersion,
    /// The sidecar digest is not a 32-byte hex SHA-256.
    InvalidDigest,
    /// The sidecar signature is not a 64-byte hex Ed25519 signature.
    InvalidSignature,
    /// The signature is not valid under the caller's trusted public key.
    Signature,
    /// The signed version differs from the caller's expected version.
    VersionMismatch { expected: String, actual: String },
    /// The signed channel differs from the caller's expected channel.
    ChannelMismatch { expected: Channel, actual: Channel },
    /// The artifact bytes do not match the signed digest.
    DigestMismatch,
}

impl fmt::Display for ArtifactVerificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            Self::NotRegularFile { path } => write!(f, "{} is not a regular file", path.display()),
            Self::InvalidLength {
                path,
                bytes,
                maximum,
            } => write!(
                f,
                "{} has {bytes} bytes; expected 1..={maximum}",
                path.display()
            ),
            Self::ChangedLength { path } => {
                write!(f, "{} changed length while being read", path.display())
            }
            Self::Sidecar(error) => write!(f, "malformed artifact sidecar: {error}"),
            Self::InvalidExpectedVersion => {
                f.write_str("the expected version is not major.minor.patch")
            }
            Self::InvalidVersion => f.write_str("the sidecar version is not major.minor.patch"),
            Self::InvalidDigest => f.write_str("the sidecar sha256 is not 64 hex characters"),
            Self::InvalidSignature => {
                f.write_str("the sidecar signature is not 128 hex characters")
            }
            Self::Signature => {
                f.write_str("the artifact signature does not verify with the trusted key")
            }
            Self::VersionMismatch { expected, actual } => write!(
                f,
                "signed version {actual} differs from expected {expected}"
            ),
            Self::ChannelMismatch { expected, actual } => write!(
                f,
                "signed channel {actual} differs from expected {expected}"
            ),
            Self::DigestMismatch => {
                f.write_str("the artifact bytes do not match the signed sha256")
            }
        }
    }
}

impl std::error::Error for ArtifactVerificationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Sidecar(source) => Some(source),
            _ => None,
        }
    }
}

/// Verify a regular artifact file against its sidecar, a caller-trusted key and expected facts.
///
/// Only the fixed existing `version\nchannel\nsha256` message is accepted. Its digest's original
/// spelling is signed verbatim; the decoded digest is compared with the actual streamed SHA-256.
/// Signature verification precedes accepting the signed version/channel or reading the artifact.
/// Reads are bounded and no file is written. Success covers these bytes and signed facts only,
/// with no installer-kind, payload, Authenticode or installed-state claim.
pub fn verify_artifact_file(
    artifact: &Path,
    sidecar: &Path,
    key: &VerifyingKey,
    expected_version: &str,
    expected_channel: Channel,
) -> Result<ArtifactSidecar, ArtifactVerificationError> {
    if !version_is_plain(expected_version) {
        return Err(ArtifactVerificationError::InvalidExpectedVersion);
    }
    let (mut file, length) = open_regular(sidecar, MAX_SIDECAR_BYTES)?;
    let mut bytes = Vec::with_capacity(length as usize);
    file.by_ref()
        .take(MAX_SIDECAR_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| io_error(sidecar, source))?;
    check_read(
        &file,
        sidecar,
        length,
        bytes.len() as u64,
        MAX_SIDECAR_BYTES,
    )?;
    let metadata: ArtifactSidecar =
        serde_json::from_slice(&bytes).map_err(ArtifactVerificationError::Sidecar)?;
    if !version_is_plain(&metadata.version) {
        return Err(ArtifactVerificationError::InvalidVersion);
    }
    let digest: [u8; 32] = hex_decode(&metadata.sha256)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(ArtifactVerificationError::InvalidDigest)?;
    let signature = hex_decode(&metadata.signature)
        .filter(|bytes| bytes.len() == 64)
        .and_then(|bytes| Signature::from_slice(&bytes).ok())
        .ok_or(ArtifactVerificationError::InvalidSignature)?;
    let message = format!(
        "{}\n{}\n{}",
        metadata.version, metadata.channel, metadata.sha256
    );
    key.verify_strict(message.as_bytes(), &signature)
        .map_err(|_| ArtifactVerificationError::Signature)?;
    if metadata.version != expected_version {
        return Err(ArtifactVerificationError::VersionMismatch {
            expected: expected_version.to_owned(),
            actual: metadata.version,
        });
    }
    if metadata.channel != expected_channel {
        return Err(ArtifactVerificationError::ChannelMismatch {
            expected: expected_channel,
            actual: metadata.channel,
        });
    }

    let (mut file, length) = open_regular(artifact, MAX_ARTIFACT_BYTES)?;
    let mut bounded = file.by_ref().take(MAX_ARTIFACT_BYTES + 1);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut read = 0;
    loop {
        let count = bounded
            .read(&mut buffer)
            .map_err(|source| io_error(artifact, source))?;
        if count == 0 {
            break;
        }
        read += count as u64;
        hasher.update(&buffer[..count]);
    }
    check_read(&file, artifact, length, read, MAX_ARTIFACT_BYTES)?;
    if hasher.finalize().as_slice() != digest {
        return Err(ArtifactVerificationError::DigestMismatch);
    }
    Ok(metadata)
}

/// Why a bounded offline release-document verification failed.
#[derive(Debug)]
pub enum DocumentVerificationError {
    /// The existing regular-file or bounded-read guard refused the input.
    Input(ArtifactVerificationError),
    /// The bytes are not the strict native release-document JSON shape.
    Json(serde_json::Error),
    /// The existing two-signature/freshness verifier refused the document.
    Native(ReleaseError),
    /// The caller's expected version is not plain major.minor.patch.
    InvalidExpectedVersion,
    /// The authenticated version differs from the caller's expectation.
    VersionMismatch { expected: String, actual: String },
    /// The authenticated channel differs from the caller's expectation.
    ChannelMismatch { expected: Channel, actual: Channel },
}

impl fmt::Display for DocumentVerificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(error) => write!(f, "{error}"),
            Self::Json(error) => write!(f, "malformed release document: {error}"),
            Self::Native(error) => write!(f, "{error}"),
            Self::InvalidExpectedVersion => {
                f.write_str("the expected version is not major.minor.patch")
            }
            Self::VersionMismatch { expected, actual } => write!(
                f,
                "signed version {actual} differs from expected {expected}"
            ),
            Self::ChannelMismatch { expected, actual } => write!(
                f,
                "signed channel {actual} differs from expected {expected}"
            ),
        }
    }
}

impl std::error::Error for DocumentVerificationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Input(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Native(error) => Some(error),
            _ => None,
        }
    }
}

/// Authenticate a strict local document with the production two-signature/freshness verifier.
///
/// This reads at most one MiB from a nonempty regular file and writes nothing. Authenticated
/// version/channel must match exactly. It does not fetch or hash the named payload, apply installed
/// version/revocation policy, validate transport URLs, or establish installer/installation state.
pub fn verify_document_file(
    document: &Path,
    key: &VerifyingKey,
    expected_version: &str,
    expected_channel: Channel,
) -> Result<ReleaseDocument, DocumentVerificationError> {
    if !version_is_plain(expected_version) {
        return Err(DocumentVerificationError::InvalidExpectedVersion);
    }
    let (mut file, length) =
        open_regular(document, MAX_DOCUMENT_BYTES).map_err(DocumentVerificationError::Input)?;
    let mut bytes = Vec::with_capacity(length as usize);
    file.by_ref()
        .take(MAX_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| DocumentVerificationError::Input(io_error(document, source)))?;
    check_read(
        &file,
        document,
        length,
        bytes.len() as u64,
        MAX_DOCUMENT_BYTES,
    )
    .map_err(DocumentVerificationError::Input)?;
    let metadata: ReleaseDocument =
        serde_json::from_slice(&bytes).map_err(DocumentVerificationError::Json)?;
    crate::document::verify(&metadata, key).map_err(DocumentVerificationError::Native)?;
    if metadata.version != expected_version {
        return Err(DocumentVerificationError::VersionMismatch {
            expected: expected_version.to_owned(),
            actual: metadata.version,
        });
    }
    if metadata.channel != expected_channel {
        return Err(DocumentVerificationError::ChannelMismatch {
            expected: expected_channel,
            actual: metadata.channel,
        });
    }
    Ok(metadata)
}

fn io_error(path: &Path, source: io::Error) -> ArtifactVerificationError {
    ArtifactVerificationError::Io {
        path: path.to_owned(),
        source,
    }
}

fn open_regular(path: &Path, maximum: u64) -> Result<(File, u64), ArtifactVerificationError> {
    // Inspect before opening to reject special files such as FIFOs without blocking on them.
    let before = fs::symlink_metadata(path).map_err(|source| io_error(path, source))?;
    check_metadata(&before, path, maximum)?;
    let file = File::open(path).map_err(|source| io_error(path, source))?;
    let opened = file.metadata().map_err(|source| io_error(path, source))?;
    check_metadata(&opened, path, maximum)?;
    if before.len() != opened.len() {
        return Err(ArtifactVerificationError::ChangedLength {
            path: path.to_owned(),
        });
    }
    Ok((file, opened.len()))
}

fn check_metadata(
    metadata: &fs::Metadata,
    path: &Path,
    maximum: u64,
) -> Result<(), ArtifactVerificationError> {
    if !metadata.is_file() {
        return Err(ArtifactVerificationError::NotRegularFile {
            path: path.to_owned(),
        });
    }
    if metadata.len() == 0 || metadata.len() > maximum {
        return Err(ArtifactVerificationError::InvalidLength {
            path: path.to_owned(),
            bytes: metadata.len(),
            maximum,
        });
    }
    Ok(())
}

fn check_read(
    file: &File,
    path: &Path,
    expected: u64,
    read: u64,
    maximum: u64,
) -> Result<(), ArtifactVerificationError> {
    if read > maximum {
        return Err(ArtifactVerificationError::InvalidLength {
            path: path.to_owned(),
            bytes: read,
            maximum,
        });
    }
    let opened = file.metadata().map_err(|source| io_error(path, source))?;
    let current = fs::symlink_metadata(path).map_err(|source| io_error(path, source))?;
    if !current.is_file()
        || opened.len() != expected
        || current.len() != expected
        || read != expected
    {
        return Err(ArtifactVerificationError::ChangedLength {
            path: path.to_owned(),
        });
    }
    Ok(())
}
