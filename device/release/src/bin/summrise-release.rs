//! `summrise-release sign` — the offline half of distribution (design §4.4, §6).
//!
//! The command is parsed by hand on purpose: the workspace has no argument-parsing dependency, and
//! the signer's one command is small enough that a typed `CliError` is clearer than a framework.
//! The key never appears in `argv`; `--key-file` names a 0600 file whose contents the signer reads
//! and never prints.

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::process::ExitCode;

use summrise_release::{
    now_ms, revoke_file, sign_file, version_is_plain, Channel, RevokeError, RevokeRequest,
    SignError, SignRequest,
};

const SIGN_USAGE: &str = "usage: summrise-release sign --key-file <hex-seed-file> \
                          --artifact <file> --version X.Y.Z --channel stable|canary \
                          --url <url> --min-version X.Y.Z --out <path>";

const REVOKE_USAGE: &str = "usage: summrise-release revoke --key-file <hex-seed-file> \
                            (--document <release.json> | --version X.Y.Z --sha256 <hex>) \
                            [--reason <text>] [--from <revocations.json>] --out <path>";

const USAGE: &str = "usage: summrise-release sign ...   (see --help after the subcommand)\n\
                     usage: summrise-release revoke ...";

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
    ConflictingSource,
    Sign(SignError),
    Revoke(RevokeError),
}

impl CliError {
    fn is_usage(&self) -> bool {
        match self {
            CliError::Sign(_) => false,
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
            CliError::MissingCommand => f.write_str("missing the `sign` or `revoke` subcommand"),
            CliError::UnknownCommand(command) => {
                write!(
                    f,
                    "unknown subcommand {command:?}; expected `sign` or `revoke`"
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
