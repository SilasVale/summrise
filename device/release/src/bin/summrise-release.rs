//! `summrise-release sign` — the offline half of distribution (design §4.4, §6).
//!
//! The command is parsed by hand on purpose: the workspace has no argument-parsing dependency, and
//! the signer's one command is small enough that a typed `CliError` is clearer than a framework.
//! The signing seed never appears in `argv`; `--key-file` names a 0600 file whose contents the
//! signer reads and never prints. Offline verification accepts only a caller-trusted public key.

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::process::ExitCode;

use ed25519_dalek::VerifyingKey;

use summrise_release::{
    generate_seed, hex_encode, now_ms, read_seed, revoke_file, sign_file, sign_sidecar,
    verify_artifact_file, verify_document_file, version_is_plain, ArtifactVerificationError,
    Channel, DocumentVerificationError, RevokeError, RevokeRequest, SignError, SignRequest,
};

const SIGN_USAGE: &str = "usage: summrise-release sign --key-file <hex-seed-file> \
                          --artifact <file> --version X.Y.Z --channel stable|canary \
                          --url <url> --min-version X.Y.Z --out <path> \
                          [--allow-plaintext]";

const REVOKE_USAGE: &str = "usage: summrise-release revoke --key-file <hex-seed-file> \
                            (--document <release.json> | --version X.Y.Z --sha256 <hex>) \
                            [--reason <text>] [--from <revocations.json>] --out <path>";

const SIDECAR_USAGE: &str = "usage: summrise-release sidecar --key-file <hex-seed-file> \
                             --artifact <file> --version X.Y.Z --channel stable|canary \
                             --out <path>";

const VERIFY_ARTIFACT_USAGE: &str = "usage: summrise-release verify-artifact --artifact <file> \
    --sidecar <file> --public-key <64-hex-public-key> --version X.Y.Z --channel stable|canary\n\
    Verifies signed bytes/version/channel; does not establish installer kind or installation.";

const VERIFY_DOCUMENT_USAGE: &str = "usage: summrise-release verify-document --document <file> \
    --public-key <64-hex-public-key> --version X.Y.Z --channel stable|canary\n\
    Verifies native policy/artifact signatures, freshness and expected version/channel; \
    does not verify payload bytes or installation state.";

const PUBKEY_USAGE: &str = "usage: summrise-release pubkey --key-file <hex-seed-file>";

const KEYGEN_USAGE: &str = "usage: summrise-release keygen --out <new-seed-file>";

const USAGE: &str = "usage: summrise-release sign ...   (see --help after the subcommand)\n\
                     usage: summrise-release revoke ...\n\
                     usage: summrise-release sidecar ...\n\
                     usage: summrise-release verify-artifact ...\n\
                     usage: summrise-release verify-document ...\n\
                     usage: summrise-release pubkey ...\n\
                     usage: summrise-release keygen ...";

#[derive(Debug)]
enum CliError {
    MissingCommand,
    UnknownCommand(String),
    NonUnicode,
    MissingFlag(&'static str),
    DuplicateFlag(&'static str),
    MissingValue(&'static str),
    UnknownFlag(String),
    UnexpectedArgument(String),
    InvalidChannel(String),
    InvalidVersion(String),
    InvalidSha256(String),
    InvalidPublicKey,
    VerifyArtifact(ArtifactVerificationError),
    VerifyDocument(DocumentVerificationError),
    ConflictingSource,
    Sign(SignError),
    Revoke(RevokeError),
}

impl CliError {
    fn is_usage(&self) -> bool {
        match self {
            CliError::Sign(_) | CliError::VerifyArtifact(_) | CliError::VerifyDocument(_) => false,
            CliError::Revoke(error) => error.is_usage(),
            _ => true,
        }
    }

    fn exit_code(&self) -> u8 {
        if self.is_usage() {
            2
        } else {
            1
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::MissingCommand => f.write_str(
                "missing the `sign`, `revoke`, `sidecar`, `verify-artifact`, `verify-document`, `pubkey` or `keygen` subcommand",
            ),
            CliError::UnknownCommand(command) => {
                write!(
                    f,
                    "unknown subcommand {command:?}; expected `sign`, `revoke`, `sidecar`, `verify-artifact`, `verify-document`, `pubkey` or `keygen`"
                )
            }
            CliError::NonUnicode => f.write_str("an argument is not valid UTF-8"),
            CliError::MissingFlag(flag) => write!(f, "missing required flag {flag}"),
            CliError::DuplicateFlag(flag) => write!(f, "flag {flag} is given more than once"),
            CliError::MissingValue(flag) => write!(f, "flag {flag} needs a value"),
            CliError::UnknownFlag(flag) => write!(f, "unknown flag {flag}"),
            CliError::UnexpectedArgument(argument) => {
                write!(f, "unexpected argument {argument:?}")
            }
            CliError::InvalidChannel(channel) => {
                write!(f, "unknown channel {channel:?}; expected stable or canary")
            }
            CliError::InvalidVersion(version) => {
                write!(
                    f,
                    "the release version {version:?} is not major.minor.patch"
                )
            }
            CliError::InvalidSha256(sha256) => {
                write!(f, "the artifact sha256 {sha256:?} is not 64 hex characters")
            }
            CliError::InvalidPublicKey => f.write_str(
                "the public key is not a valid 64-hex-character Ed25519 public key",
            ),
            CliError::VerifyArtifact(error) => write!(f, "cannot verify the artifact: {error}"),
            CliError::VerifyDocument(error) => write!(f, "cannot verify the document: {error}"),
            CliError::ConflictingSource => {
                f.write_str("give either --document or --version/--sha256, not both")
            }
            CliError::Sign(error) => write!(f, "cannot sign the release: {error}"),
            CliError::Revoke(error) => write!(f, "cannot revoke the release: {error}"),
        }
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CliError::Sign(error) => Some(error),
            CliError::Revoke(error) => Some(error),
            CliError::VerifyArtifact(error) => Some(error),
            CliError::VerifyDocument(error) => Some(error),
            _ => None,
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("summrise-release: {error}");
            if error.is_usage() {
                eprintln!("{USAGE}");
            }
            ExitCode::from(error.exit_code())
        }
    }
}

fn run() -> Result<(), CliError> {
    let mut args = std::env::args_os().skip(1);
    let command = args.next().ok_or(CliError::MissingCommand)?;
    let command = command.to_str().ok_or(CliError::NonUnicode)?;

    match command {
        "sign" => sign(args),
        "revoke" => revoke(args),
        "sidecar" => sidecar(args),
        "verify-artifact" => verify_artifact(args),
        "verify-document" => verify_document(args),
        "pubkey" => pubkey(args),
        "keygen" => keygen(args),
        "--help" | "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(CliError::UnknownCommand(other.to_owned())),
    }
}

fn sign<I>(mut args: I) -> Result<(), CliError>
where
    I: Iterator<Item = OsString>,
{
    let mut key_file = None;
    let mut artifact = None;
    let mut version = None;
    let mut channel = None;
    let mut artifact_url = None;
    let mut min_version = None;
    let mut out = None;
    let mut allow_plaintext = false;

    while let Some(flag) = args.next() {
        let flag = flag.to_str().ok_or(CliError::NonUnicode)?;
        match flag {
            "--allow-plaintext" => allow_plaintext = true,
            "--key-file" => {
                set_path(
                    &mut key_file,
                    next_value(&mut args, "--key-file")?,
                    "--key-file",
                )?;
            }
            "--artifact" => {
                set_path(
                    &mut artifact,
                    next_value(&mut args, "--artifact")?,
                    "--artifact",
                )?;
            }
            "--out" => set_path(&mut out, next_value(&mut args, "--out")?, "--out")?,
            "--version" => {
                set_text(
                    &mut version,
                    next_value(&mut args, "--version")?,
                    "--version",
                )?;
            }
            "--min-version" => {
                set_text(
                    &mut min_version,
                    next_value(&mut args, "--min-version")?,
                    "--min-version",
                )?;
            }
            "--url" => {
                set_text(&mut artifact_url, next_value(&mut args, "--url")?, "--url")?;
            }
            "--channel" => {
                if channel.is_some() {
                    return Err(CliError::DuplicateFlag("--channel"));
                }
                let text = next_text(&mut args, "--channel")?;
                let parsed = text
                    .parse::<Channel>()
                    .map_err(|_| CliError::InvalidChannel(text))?;
                channel = Some(parsed);
            }
            "--help" | "-h" => {
                println!("{SIGN_USAGE}");
                return Ok(());
            }
            other if other.starts_with('-') => {
                return Err(CliError::UnknownFlag(other.to_owned()));
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    let key_file = key_file.ok_or(CliError::MissingFlag("--key-file"))?;
    let artifact = artifact.ok_or(CliError::MissingFlag("--artifact"))?;
    let version = version.ok_or(CliError::MissingFlag("--version"))?;
    let channel = channel.ok_or(CliError::MissingFlag("--channel"))?;
    let artifact_url = artifact_url.ok_or(CliError::MissingFlag("--url"))?;
    let min_version = min_version.ok_or(CliError::MissingFlag("--min-version"))?;
    let out = out.ok_or(CliError::MissingFlag("--out"))?;

    let request = SignRequest {
        version: &version,
        channel,
        artifact_url: &artifact_url,
        min_version: &min_version,
        issued_at_ms: now_ms(),
        allow_plaintext,
    };
    let document = sign_file(&key_file, &artifact, &request, &out).map_err(CliError::Sign)?;

    println!(
        "signed {} ({}) -> {}",
        document.version,
        document.channel,
        out.display()
    );
    Ok(())
}

/// The `keygen` subcommand: a new seed where it will live, and its public half on stdout.
fn keygen<I>(mut args: I) -> Result<(), CliError>
where
    I: Iterator<Item = OsString>,
{
    let mut out = None;
    while let Some(flag) = args.next() {
        let flag = flag.to_str().ok_or(CliError::NonUnicode)?;
        match flag {
            "--out" => set_path(&mut out, next_value(&mut args, "--out")?, "--out")?,
            "--help" | "-h" => {
                println!("{KEYGEN_USAGE}");
                return Ok(());
            }
            other if other.starts_with('-') => {
                return Err(CliError::UnknownFlag(other.to_owned()));
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }
    let out = out.ok_or(CliError::MissingFlag("--out"))?;
    let public = generate_seed(&out).map_err(CliError::Sign)?;
    println!("{public}");
    Ok(())
}

/// The `pubkey` subcommand: the public half of a seed file, for a build that must embed it.
///
/// The workflow derives the compile-time keys from the seeds it already holds, so the build key and
/// the signing key cannot drift: a second copy of a public key in a workflow file is a copy that
/// goes stale the day the key is rotated.
fn pubkey<I>(mut args: I) -> Result<(), CliError>
where
    I: Iterator<Item = OsString>,
{
    let mut key_file = None;
    while let Some(flag) = args.next() {
        let flag = flag.to_str().ok_or(CliError::NonUnicode)?;
        match flag {
            "--key-file" => set_path(
                &mut key_file,
                next_value(&mut args, "--key-file")?,
                "--key-file",
            )?,
            "--help" | "-h" => {
                println!("{PUBKEY_USAGE}");
                return Ok(());
            }
            other if other.starts_with('-') => {
                return Err(CliError::UnknownFlag(other.to_owned()));
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }
    let key_file = key_file.ok_or(CliError::MissingFlag("--key-file"))?;
    let key = read_seed(&key_file).map_err(CliError::Sign)?;
    println!("{}", hex_encode(key.verifying_key().as_bytes()));
    Ok(())
}

/// The `sidecar` subcommand: sign an artifact for the first generation, with the launcher key.
fn sidecar<I>(mut args: I) -> Result<(), CliError>
where
    I: Iterator<Item = OsString>,
{
    let mut key_file = None;
    let mut artifact = None;
    let mut version = None;
    let mut channel = None;
    let mut out = None;

    while let Some(flag) = args.next() {
        let flag = flag.to_str().ok_or(CliError::NonUnicode)?;
        match flag {
            "--key-file" => set_path(
                &mut key_file,
                next_value(&mut args, "--key-file")?,
                "--key-file",
            )?,
            "--artifact" => set_path(
                &mut artifact,
                next_value(&mut args, "--artifact")?,
                "--artifact",
            )?,
            "--out" => set_path(&mut out, next_value(&mut args, "--out")?, "--out")?,
            "--version" => set_text(
                &mut version,
                next_value(&mut args, "--version")?,
                "--version",
            )?,
            "--channel" => {
                if channel.is_some() {
                    return Err(CliError::DuplicateFlag("--channel"));
                }
                let text = next_text(&mut args, "--channel")?;
                let parsed = text
                    .parse::<Channel>()
                    .map_err(|_| CliError::InvalidChannel(text))?;
                channel = Some(parsed);
            }
            "--help" | "-h" => {
                println!("{SIDECAR_USAGE}");
                return Ok(());
            }
            other if other.starts_with('-') => {
                return Err(CliError::UnknownFlag(other.to_owned()));
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    let key_file = key_file.ok_or(CliError::MissingFlag("--key-file"))?;
    let artifact = artifact.ok_or(CliError::MissingFlag("--artifact"))?;
    let version = version.ok_or(CliError::MissingFlag("--version"))?;
    let channel = channel.ok_or(CliError::MissingFlag("--channel"))?;
    let out = out.ok_or(CliError::MissingFlag("--out"))?;

    let sidecar =
        sign_sidecar(&key_file, &artifact, &version, channel, &out).map_err(CliError::Sign)?;
    println!(
        "signed the sidecar for {} ({}) -> {}",
        sidecar.version,
        sidecar.channel,
        out.display()
    );
    Ok(())
}

/// Offline verification of signed bytes, with expectations supplied by the caller.
fn verify_artifact<I>(mut args: I) -> Result<(), CliError>
where
    I: Iterator<Item = OsString>,
{
    let mut artifact = None;
    let mut sidecar = None;
    let mut public_key = None;
    let mut version = None;
    let mut channel = None;
    while let Some(flag) = args.next() {
        let flag = flag.to_str().ok_or(CliError::NonUnicode)?;
        match flag {
            "--artifact" => set_path(
                &mut artifact,
                next_value(&mut args, "--artifact")?,
                "--artifact",
            )?,
            "--sidecar" => set_path(
                &mut sidecar,
                next_value(&mut args, "--sidecar")?,
                "--sidecar",
            )?,
            "--public-key" => set_text(
                &mut public_key,
                next_value(&mut args, "--public-key")?,
                "--public-key",
            )?,
            "--version" => set_text(
                &mut version,
                next_value(&mut args, "--version")?,
                "--version",
            )?,
            "--channel" => {
                if channel.is_some() {
                    return Err(CliError::DuplicateFlag("--channel"));
                }
                let text = next_text(&mut args, "--channel")?;
                channel = Some(
                    text.parse::<Channel>()
                        .map_err(|_| CliError::InvalidChannel(text))?,
                );
            }
            "--help" | "-h" => {
                println!("{VERIFY_ARTIFACT_USAGE}");
                return Ok(());
            }
            other if other.starts_with('-') => return Err(CliError::UnknownFlag(other.to_owned())),
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }
    let artifact = artifact.ok_or(CliError::MissingFlag("--artifact"))?;
    let sidecar = sidecar.ok_or(CliError::MissingFlag("--sidecar"))?;
    let public_key = public_key.ok_or(CliError::MissingFlag("--public-key"))?;
    let version = version.ok_or(CliError::MissingFlag("--version"))?;
    let channel = channel.ok_or(CliError::MissingFlag("--channel"))?;
    if !version_is_plain(&version) {
        return Err(CliError::InvalidVersion(version));
    }
    let key = parse_public_key(&public_key)?;
    let verified = verify_artifact_file(&artifact, &sidecar, &key, &version, channel)
        .map_err(CliError::VerifyArtifact)?;
    println!(
        "verified artifact {} ({}) sha256 {}",
        verified.version, verified.channel, verified.sha256
    );
    Ok(())
}

/// Offline authentication of both native document signatures and freshness, with exact expectations.
fn verify_document<I>(mut args: I) -> Result<(), CliError>
where
    I: Iterator<Item = OsString>,
{
    let mut document = None;
    let mut public_key = None;
    let mut version = None;
    let mut channel = None;
    while let Some(flag) = args.next() {
        let flag = flag.to_str().ok_or(CliError::NonUnicode)?;
        match flag {
            "--document" => set_path(
                &mut document,
                next_value(&mut args, "--document")?,
                "--document",
            )?,
            "--public-key" => set_text(
                &mut public_key,
                next_value(&mut args, "--public-key")?,
                "--public-key",
            )?,
            "--version" => set_text(
                &mut version,
                next_value(&mut args, "--version")?,
                "--version",
            )?,
            "--channel" => {
                if channel.is_some() {
                    return Err(CliError::DuplicateFlag("--channel"));
                }
                let text = next_text(&mut args, "--channel")?;
                channel = Some(
                    text.parse::<Channel>()
                        .map_err(|_| CliError::InvalidChannel(text))?,
                );
            }
            "--help" | "-h" => {
                println!("{VERIFY_DOCUMENT_USAGE}");
                return Ok(());
            }
            other if other.starts_with('-') => return Err(CliError::UnknownFlag(other.to_owned())),
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }
    let document = document.ok_or(CliError::MissingFlag("--document"))?;
    let public_key = public_key.ok_or(CliError::MissingFlag("--public-key"))?;
    let version = version.ok_or(CliError::MissingFlag("--version"))?;
    let channel = channel.ok_or(CliError::MissingFlag("--channel"))?;
    if !version_is_plain(&version) {
        return Err(CliError::InvalidVersion(version));
    }
    let key = parse_public_key(&public_key)?;
    let verified = verify_document_file(&document, &key, &version, channel)
        .map_err(CliError::VerifyDocument)?;
    println!(
        "verified document {} ({}) sha256 {} (policy and artifact signatures; freshness checked)",
        verified.version, verified.channel, verified.sha256
    );
    Ok(())
}

fn parse_public_key(text: &str) -> Result<VerifyingKey, CliError> {
    if !is_sha256_hex(text) {
        return Err(CliError::InvalidPublicKey);
    }
    let mut bytes = [0u8; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16)
            .map_err(|_| CliError::InvalidPublicKey)?;
    }
    VerifyingKey::from_bytes(&bytes).map_err(|_| CliError::InvalidPublicKey)
}

/// The `revoke` subcommand: append one withdrawn release and sign the list with the release key.
fn revoke<I>(mut args: I) -> Result<(), CliError>
where
    I: Iterator<Item = OsString>,
{
    let mut key_file = None;
    let mut out = None;
    let mut from = None;
    let mut document = None;
    let mut version = None;
    let mut sha256 = None;
    let mut reason = None;

    while let Some(flag) = args.next() {
        let flag = flag.to_str().ok_or(CliError::NonUnicode)?;
        match flag {
            "--key-file" => {
                set_path(
                    &mut key_file,
                    next_value(&mut args, "--key-file")?,
                    "--key-file",
                )?;
            }
            "--out" => set_path(&mut out, next_value(&mut args, "--out")?, "--out")?,
            "--from" => set_path(&mut from, next_value(&mut args, "--from")?, "--from")?,
            "--document" => {
                set_path(
                    &mut document,
                    next_value(&mut args, "--document")?,
                    "--document",
                )?;
            }
            "--version" => {
                set_text(
                    &mut version,
                    next_value(&mut args, "--version")?,
                    "--version",
                )?;
            }
            "--sha256" => {
                set_text(&mut sha256, next_value(&mut args, "--sha256")?, "--sha256")?;
            }
            "--reason" => {
                set_text(&mut reason, next_value(&mut args, "--reason")?, "--reason")?;
            }
            "--help" | "-h" => {
                println!("{REVOKE_USAGE}");
                return Ok(());
            }
            other if other.starts_with('-') => {
                return Err(CliError::UnknownFlag(other.to_owned()));
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    let key_file = key_file.ok_or(CliError::MissingFlag("--key-file"))?;
    let out = out.ok_or(CliError::MissingFlag("--out"))?;

    if document.is_some() && (version.is_some() || sha256.is_some()) {
        return Err(CliError::ConflictingSource);
    }
    if document.is_none() {
        let text = version
            .as_deref()
            .ok_or(CliError::MissingFlag("--version"))?;
        if !version_is_plain(text) {
            return Err(CliError::InvalidVersion(text.to_owned()));
        }
        let digest = sha256.as_deref().ok_or(CliError::MissingFlag("--sha256"))?;
        if !is_sha256_hex(digest) {
            return Err(CliError::InvalidSha256(digest.to_owned()));
        }
    }

    let request = RevokeRequest {
        document: document.as_deref(),
        version: version.as_deref(),
        sha256: sha256.as_deref(),
        reason: reason.as_deref(),
        from: from.as_deref(),
        key_file: &key_file,
        out: &out,
    };
    let list = revoke_file(&request).map_err(CliError::Revoke)?;
    let entry = list
        .revoked
        .last()
        .expect("a revoke always appends one entry");

    println!(
        "revoked {} ({}) -> {} ({} entries)",
        entry.version,
        entry.sha256,
        out.display(),
        list.revoked.len()
    );
    Ok(())
}

/// Whether `value` is exactly 64 hex characters (either case).
fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn next_value<I>(args: &mut I, flag: &'static str) -> Result<OsString, CliError>
where
    I: Iterator<Item = OsString>,
{
    args.next().ok_or(CliError::MissingValue(flag))
}

fn next_text<I>(args: &mut I, flag: &'static str) -> Result<String, CliError>
where
    I: Iterator<Item = OsString>,
{
    next_value(args, flag)?
        .into_string()
        .map_err(|_| CliError::NonUnicode)
}

fn set_path(
    slot: &mut Option<PathBuf>,
    value: OsString,
    flag: &'static str,
) -> Result<(), CliError> {
    if slot.is_some() {
        return Err(CliError::DuplicateFlag(flag));
    }
    *slot = Some(PathBuf::from(value));
    Ok(())
}

fn set_text(
    slot: &mut Option<String>,
    value: OsString,
    flag: &'static str,
) -> Result<(), CliError> {
    if slot.is_some() {
        return Err(CliError::DuplicateFlag(flag));
    }
    *slot = Some(value.into_string().map_err(|_| CliError::NonUnicode)?);
    Ok(())
}
