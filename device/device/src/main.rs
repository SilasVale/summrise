//! `summrise-device` — the app runtime (design §4.1, §4.2).
//!
//! The launcher owns the app binary; this process owns everything the app is for: the state file,
//! the loopback API, the update stager and the swap request. It reads its configuration once,
//! records its start, and serves until it is stopped. When `--console-url` and
//! `--console-token-file` are both configured it also keeps the outbound console channel, which is
//! the operator's remote MCP path (design §5.2); when `--mesh-broker` is configured it joins the
//! mesh, publishes where it can be reached and serves the same MCP surface to a peer (the same
//! section's target). Without those flags the device is local-only, as before.
//! The hidden `--stage` mode runs the stager, asks the supervisor to swap, and waits for its answer,
//! for the Windows lifecycle test and for a human reproducing a refusal without an HTTP client. The
//! hidden `--check-update <url>` mode fetches a signed release document, verifies and stages it, and
//! then asks the supervisor for the same swap. The `--enroll <console-url> <code>` mode registers
//! this device with the operator's console before a supervised install: it writes the registration
//! token and merges the console link the service reads at start.

#![forbid(unsafe_code)]

mod capabilities;
mod config;
mod console;
mod enroll;
mod fsutil;
mod http;
mod identity;
mod lock;
mod mcp;
mod mesh;
mod remote;
mod store;
mod update;
mod update_source;
mod updater;
mod watchdog;

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use ed25519_dalek::VerifyingKey;
use summrise_core::manifest::Manifest;
use summrise_core::RolledBackArtifact;

use config::Config;
use store::Store;
use update::{KeyError, StagedRelease, SwapOutcome, SwapResult};

/// The longest `--stage` waits for the supervisor's answer (fix design F1: at most 60 s).
const SWAP_RESULT_TIMEOUT: Duration = Duration::from_secs(60);

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.first().and_then(|argument| argument.to_str()) == Some("--stage") {
        return match stage_from_args(&args[1..]) {
            Ok(result) => {
                report_swap(&result);
                ExitCode::from(result.outcome.exit_code())
            }
            Err(StageModeError::Usage) => {
                eprintln!(
                    "summrise-device: --stage needs <artifact> <manifest.json> \
                     [--install <install>] [--data <data>] [--force-reinstall <sha256>] \
                     (the rolled-back guard reads the state document only when --data is given)"
                );
                ExitCode::from(2)
            }
            Err(StageModeError::Refused(reason)) => {
                eprintln!("summrise-device: refusing the update: {reason}");
                ExitCode::from(4)
            }
            Err(StageModeError::Failed(error)) => {
                eprintln!("summrise-device: {error}");
                ExitCode::FAILURE
            }
        };
    }
    if args.first().and_then(|argument| argument.to_str()) == Some("--check-update") {
        let result = check_update_from_args(&args[1..]).await;
        match &result {
            Ok(outcome) => report_check_update(outcome),
            Err(CheckUpdateModeError::Usage) => eprintln!(
                "summrise-device: --check-update needs <url> [--channel stable|canary] \
                 [--revocations <url>] [--install <install>] [--data <data>] \
                 [--force-reinstall <sha256>]"
            ),
            Err(CheckUpdateModeError::MissingData) => eprintln!(
                "summrise-device: cannot read the installed release and its rollback memory; \
                 pass --data <data> (or set SUMMRISE_DATA) naming the directory that holds \
                 state.json — <install>/baseline/current/{BASELINE_SIDECAR} carries no rollback \
                 memory, so it cannot replace the state document"
            ),
            Err(CheckUpdateModeError::Refused(reason))
            | Err(CheckUpdateModeError::Failed(reason)) => {
                eprintln!("summrise-device: {reason}");
            }
        }
        return ExitCode::from(check_update_exit_code(&result));
    }
    if args.first().and_then(|argument| argument.to_str()) == Some("--set-update-source") {
        return set_update_source_from_args(&args[1..]);
    }
    if args.first().and_then(|argument| argument.to_str()) == Some("--update-source-clear") {
        return clear_update_source_from_args(&args[1..]);
    }
    if args.first().and_then(|argument| argument.to_str()) == Some("--enroll") {
        return match enroll::enroll_from_args(&args[1..]).await {
            Ok(enrollment) => {
                println!("enrolled {}", enrollment.id);
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("summrise-device: {error}");
                ExitCode::from(error.exit_code())
            }
        };
    }

    let config = match Config::from_args() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("summrise-device: {error}");
            return ExitCode::from(2);
        }
    };

    match run(config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("summrise-device: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Print the supervisor's answer; the exit code is the machine-readable half of the contract.
fn report_swap(result: &SwapResult) {
    let version = result.version.as_deref().unwrap_or("(no version)");
    match result.outcome {
        SwapOutcome::Applied => println!("applied {version}"),
        SwapOutcome::RolledBack => println!(
            "rolled_back {version}: {}",
            result.reason.as_deref().unwrap_or("(no reason given)")
        ),
        SwapOutcome::Refused => println!(
            "refused: {}",
            result.reason.as_deref().unwrap_or("(no reason given)")
        ),
    }
}

/// What one `--check-update` run produced.
///
/// "Already current" is a lane outcome, not an error: it exits 0 and prints a machine-readable
/// `up_to_date <version>`. A second run of the same check is therefore a safe, successful no-op
/// rather than a refusal the operator has to explain.
#[derive(Clone, Debug, PartialEq, Eq)]
enum CheckUpdateOutcome {
    /// The verified document names the release this device already runs.
    UpToDate {
        /// The installed version the document named.
        version: String,
    },
    /// The supervisor answered a swap request.
    Swap(SwapResult),
}

/// The line an already-current device prints. A script distinguishes it from `applied`.
fn up_to_date_line(version: &str) -> String {
    format!("up_to_date {version}")
}

/// Print a completed check: the `up_to_date` line, or the supervisor's answer unchanged.
fn report_check_update(outcome: &CheckUpdateOutcome) {
    match outcome {
        CheckUpdateOutcome::UpToDate { version } => println!("{}", up_to_date_line(version)),
        CheckUpdateOutcome::Swap(result) => report_swap(result),
    }
}

/// Why the hidden staging mode could not run.
#[derive(Debug, PartialEq, Eq)]
enum StageModeError {
    /// The command line is not `<artifact> <manifest.json> [--install <install>]
    /// [--data <data>] [--force-reinstall <sha256>]`.
    Usage,
    /// A path could not be read, the manifest could not be parsed, or the release was refused.
    Failed(String),
    /// The artifact's digest is in the device's rollback memory and the operator did not name it
    /// with `--force-reinstall`. This is a policy answer, so the mode exits 4 like `--check-update`.
    Refused(String),
}

/// One parsed `--stage` command line.
#[derive(Clone, Debug, PartialEq, Eq)]
struct StageInvocation {
    /// The downloaded bytes to verify and stage.
    artifact: PathBuf,
    /// The signed manifest that says what the artifact is.
    manifest: PathBuf,
    /// The install root whose canonical pair is the only destination (fix design F3).
    install: PathBuf,
    /// The data directory holding `state.json` and its rollback memory, when the operator named it.
    data: Option<PathBuf>,
    /// The one digest the operator explicitly allows again, as 64 lower-hex characters.
    force_reinstall: Option<String>,
}

/// The hidden `--stage <artifact> <manifest.json> [--install <install>] [--data <data>]
/// [--force-reinstall <sha256>]` mode.
///
/// This is not a second update path: the artifact is verified, written only to the canonical staged
/// pair under `<install>`, and then the supervisor is asked to swap. The install is not a destination
/// — the two file names are fixed — so a caller cannot make the app overwrite the app binary or a
/// baseline sidecar (fix design F3). The key is the same compile-time seam the service uses, so the
/// lifecycle test exercises the real verifier rather than a test-only one.
///
/// **The rollback guard needs `--data`.** `state.json` is the only place the device remembers an
/// artifact it rolled back; when `--data` names the directory that holds it, this mode refuses a
/// digest in that list (exit 4) unless `--force-reinstall <sha256>` names exactly that digest, and
/// audits the override before staging. Without `--data` the mode cannot see the memory and stages the
/// artifact — the guard is opt-in by naming the state document, and the caller that omits it is
/// saying it does not have one.
///
/// **This mode never writes the state document.** The serving app is its only writer; the durable
/// facts this mode leaves are the staged pair and `<install>/swap.request.json`. The app reads the
/// request at its next start or poll and records `update.phase = staged` itself.
fn stage_from_args(args: &[OsString]) -> Result<SwapResult, StageModeError> {
    stage_from_args_with_key(args, update::release_pubkey())
}

/// The testable half of [`stage_from_args`]: the key is a parameter so a test can drive the whole
/// mode with the fixed fixture key, exactly as `--check-update`'s key seam does.
fn stage_from_args_with_key(
    args: &[OsString],
    pubkey: Result<VerifyingKey, KeyError>,
) -> Result<SwapResult, StageModeError> {
    let invocation = parse_stage_args(args)?;
    let staged = stage_with_key(&invocation, pubkey)?;
    let requested_at_ms = update::request_swap(&invocation.install, &staged)
        .map_err(|error| StageModeError::Failed(format!("cannot request the swap: {error}")))?;

    update::await_swap_result(&invocation.install, requested_at_ms, SWAP_RESULT_TIMEOUT)
        .map_err(|error| StageModeError::Failed(format!("the swap was not answered: {error}")))
}

/// Parse the stage command line.
///
/// `<install>` comes from `--install`, then `SUMMRISE_INSTALL`, then the directory this binary lives
/// in — an installed app always lives at `<install>/summrise-device.exe`. There is deliberately no
/// argument that names either output file. `--data` is accepted and ignored: this mode leaves no
/// state document for a data directory to matter to.
fn parse_stage_args(args: &[OsString]) -> Result<StageInvocation, StageModeError> {
    let mut artifact = None;
    let mut manifest = None;
    let mut install = None;
    let mut data = None;
    let mut force_reinstall = None;

    let mut arguments = args.iter();
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--install") => {
                let value = arguments.next().ok_or(StageModeError::Usage)?;
                install = Some(PathBuf::from(value));
            }
            Some("--data") => {
                // The guard's input, not an ignored compatibility flag: the state document is where
                // the rolled-back digest lives (review I3).
                let value = arguments.next().ok_or(StageModeError::Usage)?;
                data = Some(PathBuf::from(value));
            }
            Some("--force-reinstall") => {
                let value = arguments.next().ok_or(StageModeError::Usage)?;
                let text = value.to_str().ok_or(StageModeError::Usage)?;
                if !is_sha256_hex(text) {
                    return Err(StageModeError::Usage);
                }
                force_reinstall = Some(text.to_ascii_lowercase());
            }
            Some(flag) if flag.starts_with("--") => return Err(StageModeError::Usage),
            _ => {
                if artifact.is_none() {
                    artifact = Some(PathBuf::from(argument));
                } else if manifest.is_none() {
                    manifest = Some(PathBuf::from(argument));
                } else {
                    return Err(StageModeError::Usage);
                }
            }
        }
    }

    let (Some(artifact), Some(manifest)) = (artifact, manifest) else {
        return Err(StageModeError::Usage);
    };
    let install = install
        .or_else(|| std::env::var_os("SUMMRISE_INSTALL").map(PathBuf::from))
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(Path::to_path_buf))
        })
        .ok_or(StageModeError::Usage)?;

    Ok(StageInvocation {
        artifact,
        manifest,
        install,
        data,
        force_reinstall,
    })
}

/// The testable half of [`stage_from_args`]: the key is a parameter so a test can supply the fixed
/// fixture key without rebuilding the binary with `SUMMRISE_RELEASE_PUBKEY`.
fn stage_with_key(
    invocation: &StageInvocation,
    pubkey: Result<VerifyingKey, KeyError>,
) -> Result<StagedRelease, StageModeError> {
    // Fail closed before reading a file an untrusted manifest could have named: a placeholder build
    // cannot stage anything, and its refusal should say that rather than report a parse error.
    let pubkey =
        pubkey.map_err(|error| StageModeError::Failed(format!("refusing the update: {error}")))?;

    let bytes = fs::read(&invocation.artifact).map_err(|error| {
        StageModeError::Failed(format!(
            "cannot read {}: {error}",
            invocation.artifact.display()
        ))
    })?;
    let document = fs::read(&invocation.manifest).map_err(|error| {
        StageModeError::Failed(format!(
            "cannot read {}: {error}",
            invocation.manifest.display()
        ))
    })?;
    let manifest: Manifest = serde_json::from_slice(&document).map_err(|error| {
        StageModeError::Failed(format!(
            "cannot parse {}: {error}",
            invocation.manifest.display()
        ))
    })?;

    // The digest guard sits before the stager so a remembered artifact is refused without being
    // written to the canonical pair. The bytes and the signed manifest must agree first: a manifest
    // naming a different digest is the stager's `Sha` refusal, not evidence about this artifact.
    let digest = summrise_release::sha256_hex(&bytes);
    let mut forced_override = None;
    if digest.eq_ignore_ascii_case(&manifest.sha256) {
        if let Some(data) = &invocation.data {
            let rolled_back = read_rolled_back(data, &invocation.install)?;
            if let Some(artifact) = rolled_back
                .iter()
                .find(|artifact| artifact.sha256.eq_ignore_ascii_case(&digest))
            {
                let forced = invocation
                    .force_reinstall
                    .as_deref()
                    .is_some_and(|sha256| sha256.eq_ignore_ascii_case(&digest));
                if !forced {
                    return Err(StageModeError::Refused(format!(
                        "this device rolled back the artifact {digest} (version {}, at {} ms); \
                         pass --force-reinstall {digest} to install it anyway",
                        artifact.version, artifact.at_ms
                    )));
                }
                forced_override = Some((data.clone(), artifact.at_ms));
            }
        }
    }

    let staged = update::stage(&bytes, &manifest, &pubkey, &invocation.install)
        .map_err(|error| StageModeError::Failed(format!("refusing the update: {error}")))?;

    // The override is audited only after the release verified: an audit line is evidence that a
    // checked artifact was allowed again, not a record of a malformed one. The stage is already on
    // disk, but no swap request has been written, so an audit failure stops the update.
    if let Some((data, rolled_back_at_ms)) = forced_override {
        store::record_force_reinstall(&data, &manifest.version, &digest, rolled_back_at_ms)
            .map_err(|error| {
                StageModeError::Failed(format!(
                    "cannot audit the force-reinstall override: {error}"
                ))
            })?;
    }
    Ok(staged)
}

/// Read the rollback memory from `<data>/state.json`.
///
/// A `--stage` run that names `--data` has asked for the guard, so a state document it cannot read is
/// a refusal rather than an empty list: staging without the memory is exactly what the guard exists
/// to stop.
fn read_rolled_back(
    data: &Path,
    install: &Path,
) -> Result<Vec<RolledBackArtifact>, StageModeError> {
    let state_path = Store::new(install, data, config::Release::standalone()).state_path();
    let bytes = fs::read(&state_path).map_err(|error| {
        StageModeError::Failed(format!(
            "cannot read the rollback memory from {}: {error}; the rolled-back guard needs \
             --data to name the state document",
            state_path.display()
        ))
    })?;
    let document = String::from_utf8(bytes).map_err(|_| {
        StageModeError::Failed(format!(
            "the state document {} is not valid UTF-8",
            state_path.display()
        ))
    })?;
    let state = summrise_core::State::parse(&document).map_err(|error| {
        StageModeError::Failed(format!(
            "cannot read the rollback memory from {}: {error}",
            state_path.display()
        ))
    })?;
    Ok(state.rolled_back)
}

/// Why the hidden `--check-update` mode could not run.
#[derive(Debug, PartialEq, Eq)]
enum CheckUpdateModeError {
    /// The command line is not `<url> [--channel stable|canary] [--install <install>]`, or the URL
    /// is not an absolute https (or loopback http) URL. A bad command line exits 2, like `--stage`'s
    /// usage errors.
    Usage,
    /// There is no state document to read: no `--data`/`SUMMRISE_DATA` was named, or the file it
    /// names is absent. The rollback memory lives only there, so the baseline sidecar cannot stand in
    /// for it. The message names `--data`, and the mode exits 2. (A document that exists but cannot
    /// be inspected or parsed is [`CheckUpdateModeError::Failed`], exit 1 — it is a broken device,
    /// not a missing argument.)
    MissingData,
    /// The document verified but this device must not install it: the channel is not one it follows,
    /// or the release is older than the installed version, or its own `min_version` is above its
    /// version. This is a policy answer, not a failed attempt — nothing was staged and nothing will
    /// be retried, so it exits 4 like the supervisor's own `refused` and stays distinguishable from
    /// a broken update (exit 1) in a script. An **equal** version is not this variant: it is
    /// [`CheckUpdateOutcome::UpToDate`] and exits 0.
    Refused(String),
    /// The document could not be fetched or verified, the artifact could not be downloaded, the
    /// release could not be staged, or the supervisor did not answer.
    Failed(String),
}

/// One parsed `--check-update` command line.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CheckUpdateInvocation {
    /// The release document to fetch.
    url: String,
    /// Accept a plaintext release host on a tunnel the operator names.
    allow_plaintext: bool,
    /// An explicit revocations list URL. `None` reads the artifact's sibling `revocations.json`.
    revocations: Option<String>,
    /// The channel this device follows; `stable` unless the operator says otherwise.
    channel: summrise_release::Channel,
    /// The install root to stage under; `None` means the running app's own directory.
    install: Option<PathBuf>,
    /// The data directory holding `state.json`, when the serving app's `--data` was given. `None`
    /// means `SUMMRISE_DATA`; it never means `<install>/data`, which is not the launcher's data
    /// directory.
    data: Option<PathBuf>,
    /// The exact digest an operator has explicitly allowed to be installed again.
    ///
    /// This overrides only the artifact-level refusal (a digest that rolled back here), never the
    /// channel or version policy; the digest is checked against the document, not trusted blindly.
    force_reinstall: Option<String>,
}

/// Parse the `--check-update <url> [--channel stable|canary] [--install <install>]
/// [--data <data>] [--force-reinstall <sha256>]` line.
///
/// The URL is parsed here, before any request is made, so a malformed address or a plain-http
/// non-loopback address is a usage error (exit 2) rather than a transport failure after a document
/// was already trusted. `--data` is the serving app's flag, repeated here because the version policy
/// needs the release this device is actually running and the rollback memory only the state document
/// carries. `--force-reinstall` is a digest, validated as one here so a typo is a usage error rather
/// than an override that silently matches nothing.
fn parse_check_update_args(
    args: &[OsString],
) -> Result<CheckUpdateInvocation, CheckUpdateModeError> {
    let mut url = None;
    let mut revocations = None;
    let mut channel = summrise_release::Channel::Stable;
    let mut install = None;
    let mut data = None;
    let mut force_reinstall = None;

    let mut allow_plaintext = false;
    let mut arguments = args.iter();
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--channel") => {
                let value = arguments.next().ok_or(CheckUpdateModeError::Usage)?;
                let text = value.to_str().ok_or(CheckUpdateModeError::Usage)?;
                channel = text
                    .parse::<summrise_release::Channel>()
                    .map_err(|_| CheckUpdateModeError::Usage)?;
            }
            Some("--revocations") => {
                let value = arguments.next().ok_or(CheckUpdateModeError::Usage)?;
                let text = value.to_str().ok_or(CheckUpdateModeError::Usage)?;
                revocations = Some(text.to_owned());
            }
            Some("--allow-plaintext") => allow_plaintext = true,
            Some("--install") => {
                let value = arguments.next().ok_or(CheckUpdateModeError::Usage)?;
                install = Some(PathBuf::from(value));
            }
            Some("--data") => {
                let value = arguments.next().ok_or(CheckUpdateModeError::Usage)?;
                data = Some(PathBuf::from(value));
            }
            Some("--force-reinstall") => {
                let value = arguments.next().ok_or(CheckUpdateModeError::Usage)?;
                let text = value.to_str().ok_or(CheckUpdateModeError::Usage)?;
                if !is_sha256_hex(text) {
                    return Err(CheckUpdateModeError::Usage);
                }
                force_reinstall = Some(text.to_ascii_lowercase());
            }
            Some(flag) if flag.starts_with("--") => return Err(CheckUpdateModeError::Usage),
            _ => {
                if url.is_some() {
                    return Err(CheckUpdateModeError::Usage);
                }
                url = Some(
                    argument
                        .to_str()
                        .ok_or(CheckUpdateModeError::Usage)?
                        .to_owned(),
                );
            }
        }
    }

    let url = url.ok_or(CheckUpdateModeError::Usage)?;
    // Validate both URLs after the whole command line is read, so `--allow-plaintext` may appear
    // before or after the URL it applies to.
    updater::http_url(&url, allow_plaintext).map_err(|_| CheckUpdateModeError::Usage)?;
    if let Some(revocations) = &revocations {
        updater::http_url(revocations, allow_plaintext).map_err(|_| CheckUpdateModeError::Usage)?;
    }
    Ok(CheckUpdateInvocation {
        url,
        allow_plaintext,
        revocations,
        channel,
        install,
        data,
        force_reinstall,
    })
}

/// Whether `value` is exactly 64 hex characters (either case).
fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// The launcher's signed sidecar under `<install>/baseline/current/`.
const BASELINE_SIDECAR: &str = "baseline.json";

/// The data directory named on the command line, if one was named.
///
/// `--data`, then `SUMMRISE_DATA`: both are the serving app's contract, and neither is guessed from
/// `<install>`. A state document found at a guessed path can name a different release than the one
/// the launcher is actually serving, which is exactly how a downgrade slips past the version policy.
fn explicit_data_dir(invocation: &CheckUpdateInvocation) -> Option<PathBuf> {
    invocation
        .data
        .clone()
        .or_else(|| std::env::var_os("SUMMRISE_DATA").map(PathBuf::from))
}

/// The local facts the update decision needs: the release this device runs and the artifacts it
/// remembers failing here.
struct InstalledRelease {
    /// The running release's version, which the version policy compares against.
    version: String,
    /// Artifacts the supervisor rolled back on this device, newest last.
    rolled_back: Vec<RolledBackArtifact>,
}

/// Read the installed release and the rollback memory from a state document.
///
/// [`Store::load`] treats a missing file as an initial document, whose version is this binary's
/// crate version. That convenience is wrong here: the installed version must come from the file or
/// not at all. The caller has already checked the path with `fs::metadata`; this read is the one that
/// must succeed. The read is all this mode does with the document: `--check-update` never writes it.
fn state_facts(path: &Path) -> Result<InstalledRelease, CheckUpdateModeError> {
    let bytes = fs::read(path).map_err(|error| {
        CheckUpdateModeError::Failed(format!(
            "cannot read the installed version from {}: {error}",
            path.display()
        ))
    })?;
    let document = String::from_utf8(bytes).map_err(|_| {
        CheckUpdateModeError::Failed(format!(
            "the state document {} is not valid UTF-8",
            path.display()
        ))
    })?;
    let state = summrise_core::State::parse(&document).map_err(|error| {
        CheckUpdateModeError::Failed(format!(
            "cannot read the installed version from {}: {error}",
            path.display()
        ))
    })?;
    Ok(InstalledRelease {
        version: state.install.version,
        rolled_back: state.rolled_back,
    })
}

/// The hidden `--set-update-source` mode: write `<data>/update.json` so the serving app checks on a
/// schedule.
///
/// A device with no source never looks for a release on its own; this is how an operator (or an
/// installer) gives it one without editing the service's argv. The file is validated here, so a
/// typo is a usage error at the moment it is written rather than a startup failure later.
fn set_update_source_from_args(args: &[OsString]) -> ExitCode {
    let mut url = None;
    let mut channel = summrise_release::Channel::Stable;
    let mut interval_s = update_source::DEFAULT_INTERVAL_S;
    let mut allow_plaintext = false;
    let mut data = None;
    let mut arguments = args.iter();
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--channel") => match arguments.next().and_then(|value| value.to_str()) {
                Some(text) => match text.parse::<summrise_release::Channel>() {
                    Ok(parsed) => channel = parsed,
                    Err(_) => return usage_update_source(),
                },
                None => return usage_update_source(),
            },
            Some("--interval") => match arguments.next().and_then(|value| value.to_str()) {
                Some(text) => match text.parse::<u64>() {
                    Ok(parsed) => interval_s = parsed,
                    Err(_) => return usage_update_source(),
                },
                None => return usage_update_source(),
            },
            Some("--allow-plaintext") => allow_plaintext = true,
            Some("--data") => match arguments.next() {
                Some(value) => data = Some(PathBuf::from(value)),
                None => return usage_update_source(),
            },
            Some(flag) if flag.starts_with("--") => return usage_update_source(),
            _ => match argument.to_str() {
                Some(text) if url.is_none() => url = Some(text.to_owned()),
                Some(_) => return usage_update_source(),
                None => return usage_update_source(),
            },
        }
    }
    let Some(url) = url else {
        return usage_update_source();
    };
    let data = data.or_else(|| std::env::var_os("SUMMRISE_DATA").map(PathBuf::from));
    let Some(data) = data else {
        eprintln!(
            "summrise-device: --set-update-source needs --data <data> (or SUMMRISE_DATA) naming \
             the directory that holds state.json"
        );
        return ExitCode::from(2);
    };
    let source = update_source::UpdateSource {
        url,
        channel,
        interval_s,
        allow_plaintext,
    };
    match update_source::UpdateSource::write(&data, &source) {
        Ok(path) => {
            println!("update source written to {}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("summrise-device: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The hidden `--update-source-clear` mode: stop checking on a schedule.
fn clear_update_source_from_args(args: &[OsString]) -> ExitCode {
    let mut data = None;
    let mut arguments = args.iter();
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--data") => match arguments.next() {
                Some(value) => data = Some(PathBuf::from(value)),
                None => return ExitCode::from(2),
            },
            _ => return ExitCode::from(2),
        }
    }
    let data = data.or_else(|| std::env::var_os("SUMMRISE_DATA").map(PathBuf::from));
    let Some(data) = data else {
        eprintln!("summrise-device: --update-source-clear needs --data <data>");
        return ExitCode::from(2);
    };
    let path = update_source::UpdateSource::path(&data);
    match std::fs::remove_file(&path) {
        Ok(()) => {
            println!("update source removed: {}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!("no update source at {}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("summrise-device: cannot remove {}: {error}", path.display());
            ExitCode::FAILURE
        }
    }
}

fn usage_update_source() -> ExitCode {
    eprintln!(
        "summrise-device: --set-update-source needs <url> [--channel stable|canary] \
         [--interval <seconds>, {min}..{max}] [--allow-plaintext] [--data <data>]",
        min = update_source::MIN_INTERVAL_S,
        max = update_source::MAX_INTERVAL_S
    );
    ExitCode::from(2)
}

/// The release this device is running and what it remembers failing, which the update decision
/// needs.
///
/// **The state document is required.** The rollback memory lives only in `state.json`: the launcher's
/// signed `<install>/baseline/current/baseline.json` carries a version but no history, and an explicit
/// version carries neither. Falling back to either would silently hand the digest guard an empty
/// list, which is the bypass review I3 found. A missing or unreadable state document is therefore a
/// refusal (exit 2) naming `--data`; it is never an empty memory.
///
/// The build's own `CARGO_PKG_VERSION` is never a fallback either: it is the version the signer was
/// compiled at, not the version the device runs. Existence is decided with `fs::metadata`, not
/// `Path::exists`: a permission error is a failure to report, not an absent document.
fn installed_release(
    invocation: &CheckUpdateInvocation,
    install: &Path,
) -> Result<InstalledRelease, CheckUpdateModeError> {
    let Some(data) = explicit_data_dir(invocation) else {
        return Err(CheckUpdateModeError::MissingData);
    };
    let state_path = Store::new(install, data, config::Release::standalone()).state_path();
    match fs::metadata(&state_path) {
        Ok(_) => state_facts(&state_path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Err(CheckUpdateModeError::MissingData)
        }
        Err(error) => Err(CheckUpdateModeError::Failed(format!(
            "cannot inspect the state document {}: {error}",
            state_path.display()
        ))),
    }
}

/// The hidden `--check-update` mode: fetch, verify, stage, then ask the supervisor and wait.
///
/// `--install` names the directory whose canonical staged pair is the destination (and whose
/// `swap.request.json` the supervisor polls); without it, the app's own directory is used.
async fn check_update_from_args(
    args: &[OsString],
) -> Result<CheckUpdateOutcome, CheckUpdateModeError> {
    check_update_from_args_with_key(args, update::release_pubkey()).await
}

/// The testable half of [`check_update_from_args`]: the release key is an argument, so a test can
/// drive the whole mode with the fixture key the stager's tests use.
async fn check_update_from_args_with_key(
    args: &[OsString],
    pubkey: Result<VerifyingKey, KeyError>,
) -> Result<CheckUpdateOutcome, CheckUpdateModeError> {
    let invocation = parse_check_update_args(args)?;
    let install = match &invocation.install {
        Some(install) => install.clone(),
        None => updater::install_root().map_err(|error| {
            CheckUpdateModeError::Failed(format!("cannot find the install directory: {error}"))
        })?,
    };
    let installed = installed_release(&invocation, &install)?;
    let pubkey = pubkey
        .map_err(|error| CheckUpdateModeError::Failed(format!("refusing the update: {error}")))?;
    let cfg = updater::UpdateConfig {
        allow_plaintext: invocation.allow_plaintext,
        ..updater::UpdateConfig::default()
    };
    let policy = updater::ReinstallPolicy {
        rolled_back: &installed.rolled_back,
        force_reinstall: invocation.force_reinstall.as_deref(),
    };

    let checked = if invocation.install.is_some() {
        updater::check_update_with(
            &invocation.url,
            invocation.revocations.as_deref(),
            invocation.channel,
            &installed.version,
            &install,
            &cfg,
            policy,
            &pubkey,
        )
        .await
    } else {
        updater::check_update(
            &invocation.url,
            invocation.revocations.as_deref(),
            invocation.channel,
            &installed.version,
            &cfg,
            policy,
        )
        .await
    };
    let staged = match checked {
        // The document verified and names the installed version. Nothing is staged and no swap is
        // requested: the state document, the baseline and the running process all stay as they are.
        Ok(updater::CheckOutcome::UpToDate) => {
            return Ok(CheckUpdateOutcome::UpToDate {
                version: installed.version,
            });
        }
        Ok(updater::CheckOutcome::Staged(staged)) => staged,
        // `select` refused the document: the signature is genuine, but this device is not the
        // release's audience. That answer needs no retry and no investigation — it is the lane's
        // fourth outcome, not a failure to fetch.
        Err(updater::UpdateError::Refused(reason)) => {
            return Err(CheckUpdateModeError::Refused(format!(
                "this device refuses the release: {reason}"
            )));
        }
        // The artifact-level refusal is the same kind of policy answer: this exact build failed
        // here. The error's Display names the version, the digest and when it failed.
        Err(error @ updater::UpdateError::RolledBack { .. })
        | Err(error @ updater::UpdateError::Revoked { .. }) => {
            return Err(CheckUpdateModeError::Refused(format!(
                "this device refuses the release: {error}"
            )));
        }
        Err(other) => {
            return Err(CheckUpdateModeError::Failed(format!(
                "the update was not staged: {other}"
            )));
        }
    };

    // An override is an operator action, so it is audited before the supervisor is asked to swap.
    // The updater has already checked that this is the exact digest the operator named; here the
    // record names the artifact that failed and when, so the audit explains why an exception was
    // made. An audit that cannot be written stops the update rather than leaving an unrecorded one.
    if let Some(forced) = &invocation.force_reinstall {
        if forced.eq_ignore_ascii_case(&staged.sha256) {
            if let Some(artifact) = installed
                .rolled_back
                .iter()
                .find(|artifact| artifact.sha256.eq_ignore_ascii_case(&staged.sha256))
            {
                let data = explicit_data_dir(&invocation).ok_or_else(|| {
                    CheckUpdateModeError::Failed(
                        "cannot audit the force-reinstall override: no data directory".to_owned(),
                    )
                })?;
                store::record_force_reinstall(
                    &data,
                    &staged.version,
                    &staged.sha256,
                    artifact.at_ms,
                )
                .map_err(|error| {
                    CheckUpdateModeError::Failed(format!(
                        "cannot audit the force-reinstall override: {error}"
                    ))
                })?;
            }
        }
    }

    // The stage is not recorded here: the serving app is the state document's only writer, and this
    // mode may run while it serves. The durable fact left behind is `swap.request.json`, which the
    // app folds into `update.phase = staged` at its next start or poll; the supervisor's result is
    // folded in the same way when the swap finishes. A refusal is therefore visible in the state the
    // console reads, without this process ever opening the document for writing.
    let requested_at_ms = update::request_swap(&install, &staged).map_err(|error| {
        CheckUpdateModeError::Failed(format!("cannot request the swap: {error}"))
    })?;
    let swap = update::await_swap_result(&install, requested_at_ms, SWAP_RESULT_TIMEOUT).map_err(
        |error| CheckUpdateModeError::Failed(format!("the swap was not answered: {error}")),
    )?;
    Ok(CheckUpdateOutcome::Swap(swap))
}

/// The `--check-update` exit contract: 0 applied **or already current**, 3 rolled back, 4 refused,
/// 1 error, 2 usage.
///
/// The supervisor's verdict is the same `SwapOutcome::exit_code` `--stage` returns. An up-to-date
/// document is a success with nothing to do, so it shares 0 with `applied` and is distinguished by
/// its `up_to_date <version>` line. A refused *document* exits 4: the release is not for this
/// device (wrong channel, older than installed, or malformed policy), which is the same answer as a
/// refused swap, while 1 is reserved for an attempt that could be retried or investigated.
fn check_update_exit_code(result: &Result<CheckUpdateOutcome, CheckUpdateModeError>) -> u8 {
    match result {
        Ok(CheckUpdateOutcome::UpToDate { .. }) => 0,
        Ok(CheckUpdateOutcome::Swap(result)) => result.outcome.exit_code(),
        Err(CheckUpdateModeError::Usage) => 2,
        Err(CheckUpdateModeError::MissingData) => 2,
        Err(CheckUpdateModeError::Refused(_)) => 4,
        Err(CheckUpdateModeError::Failed(_)) => 1,
    }
}

async fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    // The app owns `state.json`; two serving apps on one data directory would be two writers with
    // no arbitration between their read-modify-write saves. The lock is taken before the first
    // read or write and held for the whole run (dropping it closes the handle, which releases it).
    let _lock = lock::DeviceLock::acquire(&config.data)?;
    let store = Store::new(&config.install, &config.data, config.release());
    ensure_state(&store)?;
    // C1: a fresh install has no `<data>/workspace`, and no earlier step creates it. The files
    // capability's root is the device's own directory, created here before anything can call it;
    // otherwise every `files.*` call on a new device is `NotFound` with no MCP tool to fix it.
    ensure_files_root(&config)?;
    // I1: a swap can finish while this app keeps serving (a failed candidate rolls back to this
    // build, or an applied build is this one after a restart). The start reads both swap files once;
    // this thread reads them every second so neither a pending stage nor a finished swap stays
    // stale between restarts (the request is transient — the supervisor consumes it on a 2 s poll).
    store::spawn_swap_result_watcher(store.clone());
    // A configured console link is loaded before serving: an unreadable registration is a startup
    // error naming the file, not a device that silently cannot be reached remotely.
    let console = console::ConsoleEndpoint::from_config(&config)?;
    // The mesh link is built before serving for the same reason: a broker URL the rendezvous
    // cannot speak, or an identity file that is not this device's key, is a startup error naming
    // the fact rather than a device that silently serves nothing on the mesh. `None` — no
    // `--mesh-broker` — starts nothing.
    let mesh = mesh::MeshLink::from_config(&config)?;
    // A device told where its releases live checks on its own schedule; a device told nothing is
    // exactly what it was before this existed. An unreadable source is a startup error naming the
    // file rather than a device that silently never updates.
    if let Some(source) = update_source::UpdateSource::load(&config.data)? {
        // The source's transport choice is the check's transport choice: a scheduler that fetched
        // with the default (https-only) config would refuse the very mirror the operator named with
        // `--allow-plaintext`, and every interval would log the same refusal.
        let update_config = updater::UpdateConfig {
            allow_plaintext: source.allow_plaintext,
            ..updater::UpdateConfig::default()
        };
        update_source::spawn(
            store.clone(),
            config.install.clone(),
            config.data.clone(),
            source,
            update_config,
        );
    }
    http::serve(config, store, console, mesh).await?;
    Ok(())
}

/// Why the device cannot serve: the startup facts it owns could not be established.
#[derive(Debug)]
enum StartupError {
    /// The files capability's sandbox root could not be created.
    FilesRoot {
        /// The configured root, as the operator sees it in the error and the config.
        path: PathBuf,
        /// The filesystem's refusal.
        source: io::Error,
    },
}

impl std::fmt::Display for StartupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartupError::FilesRoot { path, source } => write!(
                f,
                "cannot create the files root {}: {source}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for StartupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StartupError::FilesRoot { source, .. } => Some(source),
        }
    }
}

/// Create the `files` capability's root when a fresh install does not have it.
///
/// A restart with the root already present is a success, not an error. A root path that exists as
/// a file, or whose parents cannot be created, is a typed startup error that names the configured
/// path: the device refuses to serve a files capability whose sandbox cannot be established.
fn ensure_files_root(config: &Config) -> Result<(), StartupError> {
    fs::create_dir_all(&config.files_root).map_err(|source| StartupError::FilesRoot {
        path: config.files_root.clone(),
        source,
    })
}

/// Write the initial state document when there is none, record every later start, and fold in the
/// swap files that were written while no app was recording.
///
/// An existing document is refreshed in place: liveness and the release facts follow this process,
/// and sessions are kept. A corrupt document is never replaced: startup's job is to serve the API
/// that can report the corruption, and rewriting the file would destroy the evidence the report is
/// about. The swap request and result are applied after `refresh_start`, so their phase and verdict
/// win over "started".
fn ensure_state(store: &Store) -> std::io::Result<()> {
    let mut state = if store.exists() {
        match store.load() {
            Ok(mut state) => {
                store.refresh_start(&mut state);
                state
            }
            Err(error) => {
                eprintln!(
                    "summrise-device: the state file is unusable ({error}); /api/state will answer 500 until it is repaired"
                );
                return Ok(());
            }
        }
    } else {
        store.initial()
    };
    // I3: a repair the launcher made while this app was down is a fact in the document, not a
    // sentence in a log nobody reads. Ingest runs before the swap files so a rolled-back update's
    // verdict ("rolled back from X") stays the last word on a start that both repaired and rolled
    // back. The request is folded before the result so a start that finds both ends at the outcome.
    store.ingest_repairs(&mut state)?;
    store.ingest_swap_request(&mut state)?;
    store.apply_swap_result(&mut state)?;
    store.save(&state)
}

#[cfg(test)]
mod test_support {
    //! Test-only filesystem and HTTP plumbing shared by the module tests.

    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Arc;

    use axum::{routing::get, Json, Router};
    use ed25519_dalek::SigningKey;
    use summrise_release::sign::{sign_document, SignRequest};
    use summrise_release::Channel;
    use tokio::net::TcpListener;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A local release server: the signed document and its artifact on one origin.
    pub(crate) struct ReleaseServer {
        pub(crate) document_url: String,
        pub(crate) artifact_hits: Arc<AtomicUsize>,
        _handle: tokio::task::JoinHandle<()>,
    }

    /// Sign `artifact` for `version`/`channel` and serve the document and the artifact on loopback.
    pub(crate) async fn serve_release(
        key: &SigningKey,
        version: &str,
        channel: Channel,
        min_version: &str,
        artifact: Vec<u8>,
    ) -> ReleaseServer {
        serve_release_impl(key, version, channel, min_version, artifact, None).await
    }

    /// The same server, plus a list at the artifact's sibling `/revocations.json`.
    pub(crate) async fn serve_release_with_revocations(
        key: &SigningKey,
        version: &str,
        channel: Channel,
        min_version: &str,
        artifact: Vec<u8>,
        list: Vec<u8>,
    ) -> ReleaseServer {
        serve_release_impl(key, version, channel, min_version, artifact, Some(list)).await
    }

    async fn serve_release_impl(
        key: &SigningKey,
        version: &str,
        channel: Channel,
        min_version: &str,
        artifact: Vec<u8>,
        list: Option<Vec<u8>>,
    ) -> ReleaseServer {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let base = format!("http://{}", listener.local_addr().expect("an address"));
        let artifact_url = format!("{base}/artifact");
        let document = sign_document(
            key,
            &artifact,
            &SignRequest {
                version,
                channel,
                artifact_url: &artifact_url,
                min_version,
                issued_at_ms: summrise_release::now_ms(),
                allow_plaintext: false,
            },
        )
        .expect("the fixture document signs");
        let hits = Arc::new(AtomicUsize::new(0));
        let handler_hits = Arc::clone(&hits);
        let app = Router::new()
            .route(
                "/release.json",
                get(move || {
                    let document = document.clone();
                    async move { Json(document) }
                }),
            )
            .route(
                "/artifact",
                get(move || {
                    handler_hits.fetch_add(1, Ordering::SeqCst);
                    let artifact = artifact.clone();
                    async move { artifact }
                }),
            );
        let app = match list {
            Some(bytes) => app.route(
                "/revocations.json",
                get(move || {
                    let bytes = bytes.clone();
                    async move { bytes }
                }),
            ),
            None => app,
        };
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        ReleaseServer {
            document_url: format!("{base}/release.json"),
            artifact_hits: hits,
            _handle: handle,
        }
    }

    /// Whether `pid` is no longer running.
    ///
    /// A signalled process can be a zombie: `kill(pid, 0)` still succeeds until it is reaped, and
    /// in a container whose PID 1 does not reap that can be indefinitely. Linux's
    /// `/proc/<pid>/stat` state `Z` is the honest answer there; on other Unix the signal check is
    /// all there is.
    #[cfg(unix)]
    pub(crate) fn process_is_dead(pid: u32) -> bool {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;

        // pid 0 means "this process group" to kill(2); pid 1 is init. Neither is a child a test
        // started.
        if pid <= 1 {
            return true;
        }
        if kill(Pid::from_raw(pid as i32), None::<Signal>).is_err() {
            return true;
        }
        #[cfg(target_os = "linux")]
        {
            // `pid (comm) state ppid ...`; comm may contain spaces or parentheses, so the state is
            // read after the last ')'. On a machine whose PID 1 does not reap, a signalled child is
            // a zombie forever and only this field says it is not running.
            if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                if let Some((_, fields)) = stat.rsplit_once(')') {
                    if fields.split_whitespace().next() == Some("Z") {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Wait up to `timeout` in 10 ms steps for `pid` to stop running; true when it did.
    ///
    /// The deadline is the assertion point: a kernel that has not finished tearing a process down
    /// gets the whole window, and a zombie (signalled but not reaped) counts as not running.
    #[cfg(unix)]
    pub(crate) async fn wait_until_dead(pid: u32, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if process_is_dead(pid) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    /// A uniquely named directory under the system temp directory, removed when dropped.
    pub(crate) struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        pub(crate) fn new(label: &str) -> Self {
            let unique = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "summrise-device-test-{}-{unique}-{label}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("the test directory is creatable");
            Self { path }
        }

        pub(crate) fn join(&self, name: &str) -> PathBuf {
            self.path.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ed25519_dalek::SigningKey;
    use summrise_core::{
        RolledBackArtifact, ServiceState, UpdateOutcome, UpdatePhase, VerdictOutcome,
        SCHEMA_VERSION,
    };

    use crate::config::Release;
    use crate::test_support::{serve_release, serve_release_with_revocations, TempDir};
    use crate::update::fixtures::TEST_SEED;

    fn store_in(dir: &TempDir) -> Store {
        Store::new(dir.join("install"), dir.join("data"), Release::standalone())
    }

    #[test]
    fn an_absent_state_file_is_created_at_startup() {
        let dir = TempDir::new("startup-absent");
        let store = store_in(&dir);

        ensure_state(&store).expect("startup can write the initial document");

        assert!(store.exists());
        let state = store.load().expect("what startup wrote is a document");
        assert_eq!(state.schema, SCHEMA_VERSION);
        assert_eq!(state.install.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(state.service.state, ServiceState::Running);
    }

    #[test]
    fn startup_creates_the_files_root_a_fresh_install_lacks() {
        let dir = TempDir::new("startup-files-root");
        let config = Config::parse(
            [
                OsString::from("--install"),
                dir.join("install").into_os_string(),
                OsString::from("--data"),
                dir.join("data").into_os_string(),
            ],
            |_| None,
        )
        .expect("install and data are enough");
        assert!(
            !config.files_root.exists(),
            "a fresh install has no workspace, which is what C1 was about"
        );

        ensure_files_root(&config).expect("startup creates the files capability's root");
        assert!(config.files_root.is_dir());

        ensure_files_root(&config).expect("a restart with the root already present is a success");
    }

    #[test]
    fn a_files_root_that_is_a_file_is_a_typed_startup_error() {
        let dir = TempDir::new("startup-files-root-file");
        let config = Config::parse(
            [
                OsString::from("--install"),
                dir.join("install").into_os_string(),
                OsString::from("--data"),
                dir.join("data").into_os_string(),
            ],
            |_| None,
        )
        .expect("install and data are enough");
        fs::create_dir_all(config.files_root.parent().expect("the data directory"))
            .expect("the data directory is creatable");
        fs::write(&config.files_root, b"not a directory").expect("the blocking file");

        let error = ensure_files_root(&config).expect_err("a file cannot become the files root");

        assert!(
            matches!(&error, StartupError::FilesRoot { path, .. } if path == &config.files_root),
            "the typed error names the configured root: {error:?}"
        );
        assert!(
            error
                .to_string()
                .contains(&config.files_root.display().to_string()),
            "the message names the path the operator has to fix: {error}"
        );
    }

    #[test]
    fn a_start_records_the_release_flags_not_the_crate_version() {
        let dir = TempDir::new("startup-release-flags");
        let config = Config::parse(
            [
                "--install",
                "/opt/summrise",
                "--data",
                "/var/lib/summrise",
                "--release-version",
                "9.9.9",
                "--release-sha",
                "b1946ac92492d2347c6235b4d2611184",
            ]
            .into_iter()
            .map(OsString::from),
            |_| None,
        )
        .expect("the launcher's command line parses");
        assert_ne!(config.release().version, env!("CARGO_PKG_VERSION"));
        let store = Store::new(dir.join("install"), dir.join("data"), config.release());

        ensure_state(&store).expect("startup can write the initial document");

        let state = store.load().expect("what startup wrote is a document");
        assert_eq!(state.install.version, "9.9.9");
        assert_eq!(state.baseline.version, "9.9.9");
        assert_eq!(state.baseline.sha256, "b1946ac92492d2347c6235b4d2611184");
        assert_eq!(
            state.baseline.exe,
            store.baseline_exe_path().display().to_string()
        );
        assert_eq!(state.last_verdict.detail, "started 9.9.9");
    }

    #[test]
    fn a_restart_refreshes_the_service_pid_started_at_and_release_facts() {
        let dir = TempDir::new("startup-refresh");
        let store = store_in(&dir);
        let mut state = store.initial();
        state.install.version = "9.9.9".to_owned();
        state.baseline.version = "9.9.9".to_owned();
        state.baseline.sha256 = "00".repeat(32);
        state.last_verdict.detail = "first start".to_owned();
        state.service.pid = 1;
        state.service.started_at = "1970-01-01T00:00:00Z".to_owned();
        state.service.state = ServiceState::Stopped;
        store.save(&state).expect("the fixture is written");

        ensure_state(&store).expect("startup records the new start");

        let after = store.load().expect("the document is readable");
        assert_eq!(
            after.install.version,
            env!("CARGO_PKG_VERSION"),
            "a restart records the release it was started as, not a stale install fact"
        );
        assert_eq!(after.baseline.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(
            after.baseline.sha256, "",
            "the standalone fixture has no verified digest"
        );
        assert_eq!(
            after.last_verdict.detail,
            format!("started {}", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(
            after.service.pid,
            std::process::id(),
            "the document names the live process, not the previous one"
        );
        assert_ne!(
            after.service.started_at, "1970-01-01T00:00:00Z",
            "the start time is this start's, not the previous one's"
        );
        assert!(
            after.service.started_at.ends_with('Z'),
            "the refreshed timestamp stays RFC 3339 UTC: {}",
            after.service.started_at
        );
        assert_eq!(after.service.state, ServiceState::Running);
    }

    #[test]
    fn a_corrupt_state_file_is_left_for_the_api_to_report() {
        let dir = TempDir::new("startup-corrupt");
        let store = store_in(&dir);
        std::fs::create_dir_all(dir.join("data")).expect("the data directory is creatable");
        std::fs::write(store.state_path(), "{ not a document").expect("the fixture is written");
        let before = std::fs::read_to_string(store.state_path()).expect("the fixture is readable");

        ensure_state(&store).expect("a corrupt document does not stop the process");

        assert_eq!(
            std::fs::read_to_string(store.state_path()).expect("the document is readable"),
            before,
            "replacing the file would destroy the evidence the 500 is about"
        );
    }

    /// I3 wiring: a start consumes the launcher's repair records. The reader and the ingest have
    /// their own tests; this proves `ensure_state` actually runs it and that the verdict reaches the
    /// document the operator quotes.
    #[test]
    fn a_start_ingests_the_launchers_repair_record() {
        let dir = TempDir::new("startup-repair");
        let store = store_in(&dir);
        ensure_state(&store).expect("the first start writes the document");
        let sha = "cd".repeat(32);
        let line = format!(
            r#"{{"ts_ms":1700000000000,"kind":"repaired","from":"baseline","sha256":"{sha}"}}"#
        );
        std::fs::create_dir_all(store.repair_log_path().parent().unwrap())
            .expect("the install directory is creatable");
        std::fs::write(store.repair_log_path(), format!("{line}\n"))
            .expect("the launcher's repair log is written");

        ensure_state(&store).expect("the next start ingests the repair");

        let state = store.load().expect("the document loads");
        assert_eq!(state.last_verdict.outcome, VerdictOutcome::Repaired);
        assert!(
            state.last_verdict.detail.contains("repaired from baseline"),
            "the verdict is the evidence sentence: {}",
            state.last_verdict.detail
        );
        assert!(
            state.repair_offset > 0,
            "the document tracks the log position"
        );
        let events = std::fs::read_to_string(dir.join("data").join("events.jsonl"))
            .expect("the event log is written");
        assert!(
            events.contains("repaired_from_baseline"),
            "the event log carries the same fact: {events}"
        );
    }

    fn stage_args(artifact: &Path, manifest: &Path, install: &Path) -> Vec<OsString> {
        vec![
            artifact.to_path_buf().into_os_string(),
            manifest.to_path_buf().into_os_string(),
            OsString::from("--install"),
            install.to_path_buf().into_os_string(),
        ]
    }

    fn stage_invocation(artifact: &Path, manifest: &Path, install: &Path) -> StageInvocation {
        StageInvocation {
            artifact: artifact.to_path_buf(),
            manifest: manifest.to_path_buf(),
            install: install.to_path_buf(),
            data: None,
            force_reinstall: None,
        }
    }

    #[test]
    fn the_stage_command_line_names_the_install_not_the_destinations() {
        let dir = TempDir::new("stage-parse");
        let artifact = dir.join("download.exe");
        let manifest = dir.join("release.json");
        let install = dir.join("install");

        let parsed = parse_stage_args(&stage_args(&artifact, &manifest, &install))
            .expect("two paths and --install parse");

        assert_eq!(
            parsed,
            StageInvocation {
                artifact,
                manifest,
                install: install.clone(),
                data: None,
                force_reinstall: None,
            }
        );
    }

    #[test]
    fn the_stage_command_line_refuses_a_third_path_or_an_unknown_flag() {
        let install = OsString::from("--install");
        let directory = OsString::from("/opt/summrise");
        let artifact = OsString::from("artifact.exe");
        let manifest = OsString::from("release.json");
        let extra = OsString::from("summrise-device.new.json");

        for args in [
            vec![artifact.clone()],
            vec![artifact.clone(), manifest.clone(), extra],
            vec![
                artifact.clone(),
                manifest.clone(),
                OsString::from("--verbose"),
            ],
            vec![artifact.clone(), manifest.clone(), install.clone()],
            vec![
                artifact.clone(),
                manifest.clone(),
                install.clone(),
                directory,
                OsString::from("--verbose"),
            ],
        ] {
            assert_eq!(
                parse_stage_args(&args),
                Err(StageModeError::Usage),
                "{args:?} must be refused"
            );
        }
    }

    #[test]
    fn the_hidden_stage_mode_writes_only_the_canonical_pair() {
        use crate::update::fixtures::{manifest_for, signing_key};

        let dir = TempDir::new("stage-cli");
        let artifact = dir.join("download/summrise-device.new.exe");
        let manifest_path = dir.join("release.json");
        let install = dir.join("install");
        let staged_path = install.join(update::STAGED_EXE);
        let sidecar_path = install.join(update::STAGED_SIDECAR);
        std::fs::create_dir_all(artifact.parent().expect("the download has a parent"))
            .expect("the download directory is creatable");
        let key = signing_key();
        let bytes = b"the downloaded artifact";
        std::fs::write(&artifact, bytes).expect("the fixture artifact is writable");
        let manifest = manifest_for("2.0.3", "stable", bytes, &key, "2.0.0");
        let document = serde_json::to_vec(&manifest).expect("a manifest serializes");
        std::fs::write(&manifest_path, document).expect("the fixture manifest is writable");

        let staged = stage_with_key(
            &stage_invocation(&artifact, &manifest_path, &install),
            Ok(key.verifying_key()),
        )
        .expect("the hidden mode stages a valid release");

        assert_eq!(staged.version, "2.0.3");
        assert_eq!(staged.sha256, manifest.sha256);
        assert_eq!(
            std::fs::read(&artifact).expect("the artifact is readable"),
            bytes,
            "the download is read, never replaced"
        );
        assert_eq!(
            std::fs::read(&staged_path).expect("the staged artifact is readable"),
            bytes
        );
        let sidecar: summrise_launch::Sidecar =
            serde_json::from_slice(&std::fs::read(&sidecar_path).expect("the sidecar is readable"))
                .expect("the sidecar is the launcher's type");
        assert_eq!(sidecar.version, "2.0.3");
        assert_eq!(sidecar.channel, "stable");
        let mut names: Vec<String> = std::fs::read_dir(&install)
            .expect("the install exists")
            .map(|entry| {
                entry
                    .expect("the entry is readable")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                update::STAGED_EXE.to_owned(),
                update::STAGED_SIDECAR.to_owned()
            ]
        );
    }

    /// Answer the next swap request the way a supervisor would: wait for the request file, then
    /// write an `applied` result the CLI is waiting for. The request is left in place (the real
    /// supervisor removes it), so a test can assert the durable fact the stager leaves behind.
    fn answer_swap_request(install: &Path, version: &str) -> std::thread::JoinHandle<()> {
        let request = install.join(update::SWAP_REQUEST);
        let result = install.join(update::SWAP_RESULT);
        let version = version.to_owned();
        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while std::time::Instant::now() < deadline {
                if let Ok(bytes) = fs::read(&request) {
                    if let Ok(document) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                        let at_ms = document["requested_at_ms"].as_u64().unwrap_or(0) + 1;
                        fs::write(
                            &result,
                            serde_json::json!({
                                "outcome": "applied",
                                "version": version,
                                "at_ms": at_ms,
                            })
                            .to_string(),
                        )
                        .expect("the fake supervisor's result is writable");
                        return;
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("the stager never wrote its swap request");
        })
    }

    /// A valid artifact, a manifest signed by the fixture key, and a saved state document, for the
    /// `--stage` tests that drive the whole mode.
    fn stage_fixture(
        label: &str,
    ) -> (
        TempDir,
        SigningKey,
        PathBuf,
        PathBuf,
        PathBuf,
        PathBuf,
        Store,
    ) {
        use crate::update::fixtures::{manifest_for, signing_key};

        let dir = TempDir::new(label);
        let artifact = dir.join("download/summrise-device.new.exe");
        let manifest_path = dir.join("release.json");
        let install = dir.join("install");
        let data = dir.join("data");
        std::fs::create_dir_all(artifact.parent().expect("the download has a parent"))
            .expect("the download directory is creatable");
        let key = signing_key();
        let bytes = b"the downloaded artifact";
        std::fs::write(&artifact, bytes).expect("the fixture artifact is writable");
        let manifest = manifest_for("2.0.3", "stable", bytes, &key, "2.0.0");
        std::fs::write(
            &manifest_path,
            serde_json::to_vec(&manifest).expect("a manifest serializes"),
        )
        .expect("the fixture manifest is writable");

        let store = Store::new(&install, &data, Release::standalone());
        store
            .save(&store.initial())
            .expect("the fixture state is writable");
        (dir, key, artifact, manifest_path, install, data, store)
    }

    fn stage_args_with_data(
        artifact: &Path,
        manifest: &Path,
        install: &Path,
        data: &Path,
    ) -> Vec<OsString> {
        vec![
            artifact.to_path_buf().into_os_string(),
            manifest.to_path_buf().into_os_string(),
            OsString::from("--install"),
            install.to_path_buf().into_os_string(),
            OsString::from("--data"),
            data.to_path_buf().into_os_string(),
        ]
    }

    #[test]
    fn the_stage_mode_leaves_the_state_document_alone() {
        let (_dir, key, artifact, manifest, install, data, store) =
            stage_fixture("stage-state-untouched");
        let before = std::fs::read(store.state_path()).expect("the fixture state is readable");
        let args = stage_args_with_data(&artifact, &manifest, &install, &data);

        let supervisor = answer_swap_request(&install, "2.0.3");
        let result = stage_from_args_with_key(&args, Ok(key.verifying_key()))
            .expect("the fake supervisor answers the request");
        supervisor.join().expect("the fake supervisor finishes");

        assert_eq!(result.outcome, SwapOutcome::Applied);
        assert!(
            store.swap_request_path().exists(),
            "the request file is the durable fact the CLI leaves for the serving app"
        );
        assert_eq!(
            std::fs::read(store.state_path()).expect("the state document is readable"),
            before,
            "the CLI never opens the state document for writing"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_stage_mode_succeeds_with_a_read_only_state_document() {
        use std::os::unix::fs::PermissionsExt;

        let (_dir, key, artifact, manifest, install, data, store) =
            stage_fixture("stage-state-read-only");
        std::fs::set_permissions(store.state_path(), fs::Permissions::from_mode(0o400))
            .expect("the fixture state can be made read-only");
        let before = std::fs::read(store.state_path()).expect("the fixture state is readable");
        let args = stage_args_with_data(&artifact, &manifest, &install, &data);

        let supervisor = answer_swap_request(&install, "2.0.3");
        let result = stage_from_args_with_key(&args, Ok(key.verifying_key()))
            .expect("a read-only state document cannot stop a stage that never writes it");
        supervisor.join().expect("the fake supervisor finishes");

        assert_eq!(result.outcome, SwapOutcome::Applied);
        assert!(
            store.swap_request_path().exists(),
            "the request file is written even when the state document is not writable"
        );
        assert_eq!(
            std::fs::read(store.state_path()).expect("the state document is readable"),
            before,
            "the document's bytes are untouched"
        );
        assert_eq!(
            std::fs::metadata(store.state_path())
                .expect("the document's metadata")
                .permissions()
                .mode()
                & 0o777,
            0o400,
            "the mode is untouched too"
        );
    }

    /// Seed the fixture state with the artifact this fixture's manifest names as rolled back here.
    fn remember_the_staged_artifact_failing(
        store: &Store,
        manifest_path: &Path,
    ) -> RolledBackArtifact {
        let manifest: Manifest = serde_json::from_slice(
            &std::fs::read(manifest_path).expect("the manifest is readable"),
        )
        .expect("the manifest is JSON");
        let artifact = RolledBackArtifact {
            sha256: manifest.sha256,
            version: manifest.version,
            at_ms: 1_700_000_000_000,
        };
        let mut state = store.load().expect("the fixture state is readable");
        state.install.version = "2.0.0".to_owned();
        state.record_rolled_back(artifact.clone());
        store.save(&state).expect("the fixture state is writable");
        artifact
    }

    #[test]
    fn the_stage_mode_refuses_a_digest_the_state_document_remembers() {
        let (_dir, key, artifact, manifest, install, data, store) =
            stage_fixture("stage-rolled-back");
        let remembered = remember_the_staged_artifact_failing(&store, &manifest);

        let error = stage_from_args_with_key(
            &stage_args_with_data(&artifact, &manifest, &install, &data),
            Ok(key.verifying_key()),
        )
        .expect_err("a digest this device rolled back is refused without --force-reinstall");

        match error {
            StageModeError::Refused(message) => {
                assert!(
                    message.contains(&remembered.sha256),
                    "the refusal names the digest: {message}"
                );
                assert!(
                    message.contains("2.0.3"),
                    "the refusal names the version: {message}"
                );
                assert!(
                    message.contains("--force-reinstall"),
                    "the refusal says how an operator overrides it: {message}"
                );
            }
            other => panic!("a remembered digest is a policy refusal (exit 4): {other:?}"),
        }
        assert!(
            !install.join(update::STAGED_EXE).exists(),
            "the refused artifact never reaches the canonical staged pair"
        );
        assert!(
            !install.join(update::SWAP_REQUEST).exists(),
            "the refused artifact asks no supervisor to swap"
        );
        assert!(
            !data.join("events.jsonl").exists(),
            "a refusal is not an override and writes no audit line"
        );
    }

    #[test]
    fn the_stage_mode_force_reinstall_overrides_and_audits_the_remembered_digest() {
        let (_dir, key, artifact, manifest, install, data, store) = stage_fixture("stage-force");
        let remembered = remember_the_staged_artifact_failing(&store, &manifest);
        let mut args = stage_args_with_data(&artifact, &manifest, &install, &data);
        args.push(OsString::from("--force-reinstall"));
        args.push(OsString::from(&remembered.sha256));

        let supervisor = answer_swap_request(&install, "2.0.3");
        let result = stage_from_args_with_key(&args, Ok(key.verifying_key()))
            .expect("the operator's exact digest overrides the refusal");
        supervisor.join().expect("the fake supervisor finishes");

        assert_eq!(result.outcome, SwapOutcome::Applied);
        assert!(
            install.join(update::STAGED_EXE).exists(),
            "the overridden artifact is staged and swapped"
        );
        let audit = std::fs::read_to_string(data.join("events.jsonl"))
            .expect("the override is audited in the device's event log");
        let line: serde_json::Value = serde_json::from_str(
            audit
                .lines()
                .next()
                .expect("the audit log carries the override line"),
        )
        .expect("the audit line is JSON");
        assert_eq!(line["event"], "force_reinstall");
        assert_eq!(line["version"], "2.0.3");
        assert_eq!(line["sha256"], serde_json::json!(remembered.sha256));
        assert_eq!(line["rolled_back_at_ms"], remembered.at_ms);
    }

    #[test]
    fn the_stage_mode_force_reinstall_for_another_digest_does_not_override() {
        let (_dir, key, artifact, manifest, install, data, store) =
            stage_fixture("stage-force-other");
        remember_the_staged_artifact_failing(&store, &manifest);
        let mut args = stage_args_with_data(&artifact, &manifest, &install, &data);
        args.push(OsString::from("--force-reinstall"));
        args.push(OsString::from("cd".repeat(32)));

        let error = stage_from_args_with_key(&args, Ok(key.verifying_key()))
            .expect_err("only the exact digest an operator names is allowed again");
        assert!(
            matches!(error, StageModeError::Refused(_)),
            "a different digest leaves the refusal in place: {error:?}"
        );
        assert!(!install.join(update::STAGED_EXE).exists());
    }

    #[test]
    fn the_stage_mode_without_data_defers_the_rollback_guard() {
        let (_dir, key, artifact, manifest, install, data, store) = stage_fixture("stage-no-data");
        let remembered = remember_the_staged_artifact_failing(&store, &manifest);

        // No `--data`: the mode cannot see the memory, so it stages. This is the documented opt-in
        // that the help names, not a silent bypass of the guard.
        let supervisor = answer_swap_request(&install, "2.0.3");
        let result = stage_from_args_with_key(
            &stage_args(&artifact, &manifest, &install),
            Ok(key.verifying_key()),
        )
        .expect("a stage without --data does not consult the rollback memory");
        supervisor.join().expect("the fake supervisor finishes");

        assert_eq!(result.outcome, SwapOutcome::Applied);
        assert_eq!(
            std::fs::read(install.join(update::STAGED_EXE)).expect("the artifact is staged"),
            std::fs::read(&artifact).expect("the artifact is readable"),
            "the remembered digest {remembered:?} does not block a stage with no data directory"
        );
        assert!(
            !data.join("events.jsonl").exists(),
            "no override was audited"
        );
    }

    #[test]
    fn the_stage_mode_refuses_when_the_data_it_named_has_no_readable_state() {
        let (_dir, key, artifact, manifest, install, data, _store) =
            stage_fixture("stage-missing-state");
        std::fs::remove_file(Store::new(&install, &data, Release::standalone()).state_path())
            .expect("the fixture state is removable");

        let error = stage_from_args_with_key(
            &stage_args_with_data(&artifact, &manifest, &install, &data),
            Ok(key.verifying_key()),
        )
        .expect_err("--data names the rollback memory; a state it cannot read is a refusal");
        match error {
            StageModeError::Failed(message) => assert!(
                message.contains("state.json") && message.contains("--data"),
                "the refusal names the document and the flag: {message}"
            ),
            other => {
                panic!("an unreadable state is a failed stage, not a policy refusal: {other:?}")
            }
        }
        assert!(!install.join(update::STAGED_EXE).exists());
    }

    #[test]
    fn the_hidden_stage_mode_writes_nothing_on_a_bad_signature() {
        use crate::update::fixtures::{manifest_for, signing_key};

        let dir = TempDir::new("stage-cli-bad-signature");
        let artifact = dir.join("download.exe");
        let manifest_path = dir.join("release.json");
        let install = dir.join("install");
        let key = signing_key();
        let bytes = b"the downloaded artifact";
        std::fs::write(&artifact, bytes).expect("the fixture artifact is writable");
        let mut manifest = manifest_for("2.0.3", "stable", bytes, &key, "2.0.0");
        manifest.signature = "00".repeat(64);
        std::fs::write(
            &manifest_path,
            serde_json::to_vec(&manifest).expect("a manifest serializes"),
        )
        .expect("the fixture manifest is writable");

        let result = stage_with_key(
            &stage_invocation(&artifact, &manifest_path, &install),
            Ok(key.verifying_key()),
        );

        assert!(
            matches!(result, Err(StageModeError::Failed(ref message)) if message.contains("does not verify")),
            "{result:?}"
        );
        assert!(
            !install.exists(),
            "a refusal must not even create the install directory, let alone write a pair"
        );
    }

    #[test]
    fn the_hidden_stage_mode_refuses_the_placeholder_key_before_reading_anything() {
        use crate::update::KeyError;

        // The paths do not exist: a placeholder build must refuse by name rather than fail on I/O.
        let invocation = StageInvocation {
            artifact: PathBuf::from("missing-artifact"),
            manifest: PathBuf::from("missing-manifest.json"),
            install: PathBuf::from("missing-install"),
            data: None,
            force_reinstall: None,
        };

        let result = stage_with_key(&invocation, Err(KeyError::Placeholder));

        assert!(
            matches!(result, Err(StageModeError::Failed(ref message)) if message.contains("placeholder release key")),
            "{result:?}"
        );
    }

    #[test]
    fn the_stage_command_line_parses_the_data_directory_and_an_exact_override() {
        let sha256 = "ab".repeat(32);
        let parsed = parse_stage_args(&[
            OsString::from("artifact.exe"),
            OsString::from("release.json"),
            OsString::from("--install"),
            OsString::from("/opt/summrise"),
            OsString::from("--data"),
            OsString::from("/var/lib/summrise"),
            OsString::from("--force-reinstall"),
            OsString::from(&sha256),
        ])
        .expect("--data and --force-reinstall are part of the guard's command line");

        assert_eq!(parsed.install, PathBuf::from("/opt/summrise"));
        assert_eq!(
            parsed,
            StageInvocation {
                artifact: PathBuf::from("artifact.exe"),
                manifest: PathBuf::from("release.json"),
                install: PathBuf::from("/opt/summrise"),
                data: Some(PathBuf::from("/var/lib/summrise")),
                force_reinstall: Some(sha256),
            },
            "the data directory names the rollback memory; the digest names the one allowed override"
        );
        assert_eq!(
            parse_stage_args(&[
                OsString::from("artifact.exe"),
                OsString::from("release.json"),
                OsString::from("--force-reinstall"),
                OsString::from("not-a-sha256"),
            ]),
            Err(StageModeError::Usage),
            "a malformed override is a usage error, not an override that matches nothing"
        );
    }

    #[test]
    fn a_start_records_a_pending_stage_request_written_while_no_app_was_serving() {
        let dir = TempDir::new("startup-request");
        let store = store_in(&dir);
        store.save(&store.initial()).expect("the fixture saves");
        let request_path = store.swap_request_path();
        std::fs::create_dir_all(request_path.parent().expect("the request has a parent"))
            .expect("the install directory is creatable");
        std::fs::write(
            request_path,
            serde_json::json!({
                "requested_at_ms": 1_000,
                "version": "2.0.4",
                "sha256": "b1946ac92492d2347c6235b4d2611184",
            })
            .to_string(),
        )
        .expect("the request is written");

        ensure_state(&store).expect("startup records the pending stage");

        let state = store.load().expect("the document loads");
        assert_eq!(
            state.update.phase,
            UpdatePhase::Staged,
            "the CLI left a request, not a state write; the app records the stage it names"
        );
        assert_eq!(state.update.to.as_deref(), Some("2.0.4"));
        assert_eq!(state.update.outcome, None);
    }

    #[test]
    fn a_start_records_a_swap_result_written_while_no_app_was_serving() {
        let dir = TempDir::new("startup-result");
        let store = store_in(&dir);
        store.save(&store.initial()).expect("the fixture saves");
        let result = serde_json::json!({
            "outcome": "applied",
            "version": "2.0.4",
            "at_ms": 1_234,
        });
        let result_path = store.swap_result_path();
        std::fs::create_dir_all(result_path.parent().expect("the result has a parent"))
            .expect("the install directory is creatable");
        std::fs::write(result_path, result.to_string()).expect("the result is written");
        // The request the stager left behind is still on disk (a supervisor that finished without
        // removing it, or a crash between the two writes). The result must be the last word.
        let request_path = store.swap_request_path();
        std::fs::write(
            request_path,
            serde_json::json!({
                "requested_at_ms": 1_233,
                "version": "2.0.4",
                "sha256": "b1946ac92492d2347c6235b4d2611184",
            })
            .to_string(),
        )
        .expect("the request is written");

        ensure_state(&store).expect("startup records both the start and the swap files");

        let state = store.load().expect("the document loads");
        assert_eq!(state.update.outcome, Some(UpdateOutcome::Applied));
        assert_eq!(state.update.last_result_at_ms, Some(1_234));
        assert_eq!(
            state.update.phase,
            UpdatePhase::Idle,
            "a finished swap is not left saying staged"
        );
        assert_eq!(state.last_verdict.detail, "updated to 2.0.4");
        assert_eq!(
            state.last_verdict.outcome,
            summrise_core::VerdictOutcome::Ok
        );
    }

    #[test]
    fn the_check_update_command_line_parses_every_flag() {
        let sha256 = "ab".repeat(32);
        let parsed = parse_check_update_args(&[
            OsString::from("https://releases.example.test/release.json"),
            OsString::from("--channel"),
            OsString::from("canary"),
            OsString::from("--install"),
            OsString::from("/opt/summrise"),
            OsString::from("--data"),
            OsString::from("/var/lib/summrise"),
            OsString::from("--revocations"),
            OsString::from("https://releases.example.test/revocations.json"),
            OsString::from("--force-reinstall"),
            OsString::from(&sha256),
        ])
        .expect("a check-update line parses");

        assert_eq!(
            parsed,
            CheckUpdateInvocation {
                url: "https://releases.example.test/release.json".to_owned(),
                allow_plaintext: false,
                revocations: Some("https://releases.example.test/revocations.json".to_owned()),
                channel: summrise_release::Channel::Canary,
                install: Some(PathBuf::from("/opt/summrise")),
                data: Some(PathBuf::from("/var/lib/summrise")),
                force_reinstall: Some(sha256),
            }
        );
    }

    #[test]
    fn the_check_update_command_line_defaults_to_stable_and_its_own_install() {
        let parsed =
            parse_check_update_args(&[OsString::from("http://127.0.0.1:8080/release.json")])
                .expect("a URL is enough");

        assert_eq!(parsed.channel, summrise_release::Channel::Stable);
        assert_eq!(parsed.install, None);
        assert_eq!(parsed.data, None);
        assert_eq!(
            parsed.force_reinstall, None,
            "no digest is overridden unless the operator names one"
        );
    }

    #[test]
    fn a_malformed_check_update_url_is_a_usage_error_not_a_panic() {
        for url in [
            "",
            "not a url",
            "ftp://example.test/release.json",
            "file:///release.json",
            "https://",
            "http://example.test/release.json",
        ] {
            assert_eq!(
                parse_check_update_args(&[OsString::from(url)]),
                Err(CheckUpdateModeError::Usage),
                "{url:?} must be a typed usage error"
            );
        }
    }

    #[test]
    fn a_bad_check_update_command_line_is_a_usage_error() {
        let url = OsString::from("https://releases.example.test/release.json");
        for args in [
            vec![],
            vec![OsString::from("--channel")],
            vec![
                url.clone(),
                OsString::from("--channel"),
                OsString::from("beta"),
            ],
            vec![url.clone(), OsString::from("--install")],
            vec![url.clone(), OsString::from("--data")],
            vec![url.clone(), OsString::from("--release-version")],
            vec![url.clone(), OsString::from("--force-reinstall")],
            vec![
                url.clone(),
                OsString::from("--force-reinstall"),
                OsString::from("not-a-sha256"),
            ],
            vec![url.clone(), OsString::from("--revocations")],
            vec![
                url.clone(),
                OsString::from("--revocations"),
                OsString::from("http://example.test/revocations.json"),
            ],
            vec![url.clone(), OsString::from("--verbose")],
            vec![
                url.clone(),
                OsString::from("https://other.test/release.json"),
            ],
        ] {
            assert_eq!(
                parse_check_update_args(&args),
                Err(CheckUpdateModeError::Usage),
                "{args:?} must be refused"
            );
        }
    }

    /// The version-only view of [`installed_release`].
    fn installed_version(
        invocation: &CheckUpdateInvocation,
        install: &Path,
    ) -> Result<String, CheckUpdateModeError> {
        installed_release(invocation, install).map(|release| release.version)
    }

    #[test]
    fn the_installed_version_comes_only_from_the_state_document() {
        let dir = TempDir::new("installed-version");
        let install = dir.join("install");
        let data = dir.join("data");
        let invocation = || CheckUpdateInvocation {
            url: "https://example.test/release.json".to_owned(),
            allow_plaintext: false,
            revocations: None,
            channel: summrise_release::Channel::Stable,
            install: Some(install.clone()),
            data: Some(data.clone()),
            force_reinstall: None,
        };

        assert_eq!(
            installed_version(&invocation(), &install),
            Err(CheckUpdateModeError::MissingData),
            "without a state document the mode must refuse, not fall back to a version"
        );

        let store = Store::new(&install, &data, Release::standalone());
        let mut state = store.initial();
        state.install.version = "2.0.0".to_owned();
        store.save(&state).expect("the fixture state is writable");

        assert_eq!(
            installed_version(&invocation(), &install).expect("the state document is the answer"),
            "2.0.0"
        );

        std::fs::write(store.state_path(), "{ not a document")
            .expect("the corrupt fixture is writable");
        assert!(
            matches!(
                installed_version(&invocation(), &install),
                Err(CheckUpdateModeError::Failed(_))
            ),
            "an existing state document that cannot be read is not a reason to guess a version"
        );

        // A directory at the state path is not "absent" either: fs::metadata succeeds and the read
        // reports the malformed source, rather than falling through to another version.
        std::fs::remove_file(store.state_path()).expect("the corrupt fixture is removable");
        std::fs::create_dir_all(store.state_path()).expect("the directory fixture is creatable");
        assert!(
            matches!(
                installed_version(&invocation(), &install),
                Err(CheckUpdateModeError::Failed(_))
            ),
            "a state path that cannot be read must not be treated as missing"
        );
    }

    fn write_baseline_sidecar(install: &Path, version: &str) {
        let path = install
            .join("baseline")
            .join("current")
            .join(BASELINE_SIDECAR);
        std::fs::create_dir_all(path.parent().expect("the sidecar has a parent"))
            .expect("the baseline directory is creatable");
        std::fs::write(
            &path,
            serde_json::json!({
                "version": version,
                "channel": "stable",
                "sha256": "0".repeat(64),
                "signature": "0".repeat(128),
            })
            .to_string(),
        )
        .expect("the baseline sidecar is writable");
    }

    #[test]
    fn the_baseline_sidecar_cannot_stand_in_for_the_rollback_memory() {
        let dir = TempDir::new("installed-baseline");
        let install = dir.join("install");
        write_baseline_sidecar(&install, "2.0.0");

        // A state document at the old guessed `<install>/data` path must not be read: the launcher's
        // data directory is not derivable from the install root — but that is not a licence to use
        // the baseline's version and an empty rollback list either (review I3).
        let guessed = Store::new(&install, install.join("data"), Release::standalone());
        let mut guessed_state = guessed.initial();
        guessed_state.install.version = "9.9.9".to_owned();
        guessed
            .save(&guessed_state)
            .expect("the decoy state is writable");

        let invocation = CheckUpdateInvocation {
            url: "https://example.test/release.json".to_owned(),
            allow_plaintext: false,
            revocations: None,
            channel: summrise_release::Channel::Stable,
            install: Some(install.clone()),
            data: None,
            force_reinstall: None,
        };

        assert_eq!(
            installed_version(&invocation, &install),
            Err(CheckUpdateModeError::MissingData),
            "the baseline sidecar has no rollback memory, so it is not a substitute"
        );
    }

    #[test]
    fn the_installed_version_refuses_rather_than_using_the_crate_version() {
        let dir = TempDir::new("installed-missing");
        let install = dir.join("install");
        let invocation = CheckUpdateInvocation {
            url: "https://example.test/release.json".to_owned(),
            allow_plaintext: false,
            revocations: None,
            channel: summrise_release::Channel::Stable,
            install: Some(install.clone()),
            data: None,
            force_reinstall: None,
        };

        let result = installed_version(&invocation, &install);

        assert_eq!(result, Err(CheckUpdateModeError::MissingData));
        assert_eq!(
            check_update_exit_code(&Err(CheckUpdateModeError::MissingData)),
            2,
            "an unknown installed version is a usage error, not a policy refusal"
        );
    }

    fn check_update_args(url: &str, install: &Path, data: &Path) -> Vec<OsString> {
        vec![
            OsString::from(url),
            OsString::from("--install"),
            install.as_os_str().to_owned(),
            OsString::from("--data"),
            data.as_os_str().to_owned(),
        ]
    }

    /// The same line with an explicit `--channel`, for the canary half of the rule.
    fn check_update_args_on(
        url: &str,
        channel: &str,
        install: &Path,
        data: &Path,
    ) -> Vec<OsString> {
        let mut args = check_update_args(url, install, data);
        args.push(OsString::from("--channel"));
        args.push(OsString::from(channel));
        args
    }

    /// The supervisor's answer from a completed check; a panic names the other outcome.
    fn swap(result: CheckUpdateOutcome) -> SwapResult {
        match result {
            CheckUpdateOutcome::Swap(swap) => swap,
            other => panic!("expected the supervisor's answer, got {other:?}"),
        }
    }

    /// Write an answer that cannot be stale: its timestamp is later than any request this process
    /// can write, so `request_swap` keeps it and `await_swap_result` returns it immediately.
    fn write_instant_swap_answer(install: &Path) {
        std::fs::write(
            install.join(update::SWAP_RESULT),
            format!(
                r#"{{"outcome":"applied","version":"1.9.0","at_ms":{}}}"#,
                u64::MAX
            ),
        )
        .expect("the fixture swap answer is writable");
    }

    #[tokio::test]
    async fn the_check_update_mode_refuses_the_installed_versions_downgrade() {
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"an older release".to_vec();
        let server = serve_release(
            &key,
            "1.9.0",
            summrise_release::Channel::Stable,
            "1.0.0",
            artifact.clone(),
        )
        .await;

        // The state document says this device is running 2.0.0, even though this test binary was
        // built at CARGO_PKG_VERSION. The state is what the version policy must compare against.
        let dir = TempDir::new("check-update-installed-state");
        let install = dir.join("install");
        let data = dir.join("data");
        std::fs::create_dir_all(&install).expect("the install directory is creatable");
        let store = Store::new(&install, &data, Release::standalone());
        let mut state = store.initial();
        state.install.version = "2.0.0".to_owned();
        store.save(&state).expect("the fixture state is writable");
        write_instant_swap_answer(&install);

        let refused = check_update_from_args_with_key(
            &check_update_args(&server.document_url, &install, &data),
            Ok(key.verifying_key()),
        )
        .await;

        assert!(
            matches!(refused, Err(CheckUpdateModeError::Refused(ref message))
                if message.contains("older than the installed version")),
            "{refused:?}"
        );
        assert_eq!(
            check_update_exit_code(&refused),
            4,
            "a downgrade is `not for this device` (4), not a failed attempt (1)"
        );
        assert!(
            !install.join(update::STAGED_EXE).exists(),
            "the downgrade must not be staged"
        );
    }

    #[tokio::test]
    async fn the_check_update_mode_refuses_a_canary_document_on_a_stable_device() {
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"a canary release".to_vec();
        let server = serve_release(
            &key,
            "1.9.0",
            summrise_release::Channel::Canary,
            "1.0.0",
            artifact,
        )
        .await;

        let dir = TempDir::new("check-update-canary-mode");
        let install = dir.join("install");
        let data = dir.join("data");
        std::fs::create_dir_all(&install).expect("the install directory is creatable");
        // The channel rule is reached only after the installed version is known; the fixture is a
        // device running 2.0.0, so the canary document is refused for its channel, not for a
        // missing local fact.
        let store = Store::new(&install, &data, Release::standalone());
        let mut state = store.initial();
        state.install.version = "2.0.0".to_owned();
        store.save(&state).expect("the fixture state is writable");

        let refused = check_update_from_args_with_key(
            &check_update_args(&server.document_url, &install, &data),
            Ok(key.verifying_key()),
        )
        .await;

        assert!(
            matches!(refused, Err(CheckUpdateModeError::Refused(ref message))
                if message.contains("does not accept the release's channel")),
            "{refused:?}"
        );
        assert_eq!(
            check_update_exit_code(&refused),
            4,
            "a wrong-channel release is `not for this device` (4), not a failed attempt (1)"
        );
        assert_eq!(
            server
                .artifact_hits
                .load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the channel is decided before the artifact is downloaded"
        );
        assert!(
            !install.join(update::STAGED_EXE).exists(),
            "a channel refusal must not stage anything"
        );
    }

    /// The revocation gate is the fourth exit-4 case: a genuine document for a withdrawn artifact is
    /// a policy answer, not a failed fetch, and nothing is staged.
    #[tokio::test]
    async fn the_check_update_mode_refuses_a_revoked_release_with_exit_4() {
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the withdrawn release".to_vec();
        let sha256 = summrise_release::sha256_hex(&artifact);
        let list = summrise_release::Revocations::sign(
            &key,
            summrise_release::now_ms(),
            vec![summrise_release::RevokedRelease {
                version: "2.0.3".to_owned(),
                sha256: sha256.clone(),
                reason: Some("boot loop on Windows 11".to_owned()),
            }],
        )
        .expect("the fixture list signs");
        let server = serve_release_with_revocations(
            &key,
            "2.0.3",
            summrise_release::Channel::Stable,
            "1.0.0",
            artifact,
            serde_json::to_vec(&list).expect("the list serializes"),
        )
        .await;

        let dir = TempDir::new("check-update-revoked-mode");
        let install = dir.join("install");
        let data = dir.join("data");
        std::fs::create_dir_all(&install).expect("the install directory is creatable");
        let store = Store::new(&install, &data, Release::standalone());
        let mut state = store.initial();
        state.install.version = "2.0.0".to_owned();
        store.save(&state).expect("the fixture state is writable");

        let refused = check_update_from_args_with_key(
            &check_update_args(&server.document_url, &install, &data),
            Ok(key.verifying_key()),
        )
        .await;

        match &refused {
            Err(CheckUpdateModeError::Refused(message)) => {
                assert!(
                    message.contains("2.0.3"),
                    "the refusal names the withdrawn version: {message}"
                );
                assert!(
                    message.contains("boot loop on Windows 11"),
                    "the refusal names the operator's reason: {message}"
                );
            }
            other => panic!("a revoked release is a policy refusal: {other:?}"),
        }
        assert_eq!(
            check_update_exit_code(&refused),
            4,
            "a revoked release is `not for this device`, like a wrong channel"
        );
        assert!(
            !install.join(update::STAGED_EXE).exists(),
            "a revoked release must not be staged"
        );
        assert!(
            !install.join(update::SWAP_REQUEST).exists(),
            "a revoked release must not ask the supervisor to swap"
        );
    }

    /// A device running 2.0.0 that remembers one failed 2.0.3 artifact.
    fn state_with_rolled_back(store: &Store, sha256: &str, at_ms: u64) {
        let mut state = store.initial();
        state.install.version = "2.0.0".to_owned();
        state.record_rolled_back(RolledBackArtifact {
            sha256: sha256.to_owned(),
            version: "2.0.3".to_owned(),
            at_ms,
        });
        store.save(&state).expect("the fixture state is writable");
    }

    fn force_reinstall_args(url: &str, install: &Path, data: &Path, sha256: &str) -> Vec<OsString> {
        let mut args = check_update_args(url, install, data);
        args.push(OsString::from("--force-reinstall"));
        args.push(OsString::from(sha256));
        args
    }

    #[tokio::test]
    async fn the_check_update_mode_refuses_a_rolled_back_artifact_and_names_it() {
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the artifact that failed here".to_vec();
        let server = serve_release(
            &key,
            "2.0.3",
            summrise_release::Channel::Stable,
            "2.0.0",
            artifact.clone(),
        )
        .await;
        let sha256 = summrise_release::sha256_hex(&artifact);

        let dir = TempDir::new("check-update-rolled-back-mode");
        let install = dir.join("install");
        let data = dir.join("data");
        std::fs::create_dir_all(&install).expect("the install directory is creatable");
        let store = Store::new(&install, &data, Release::standalone());
        state_with_rolled_back(&store, &sha256, 1_700_000_000_000);

        let refused = check_update_from_args_with_key(
            &check_update_args(&server.document_url, &install, &data),
            Ok(key.verifying_key()),
        )
        .await;

        match &refused {
            Err(CheckUpdateModeError::Refused(message)) => {
                assert!(
                    message.contains("2.0.3"),
                    "the refusal names the version: {message}"
                );
                assert!(
                    message.contains("2023-11-14T22:13:20Z"),
                    "the refusal names when it failed: {message}"
                );
            }
            other => panic!("a rolled-back digest must be a policy refusal: {other:?}"),
        }
        assert_eq!(
            check_update_exit_code(&refused),
            4,
            "a build that failed here is a policy answer, not a failed attempt"
        );
        assert_eq!(
            server
                .artifact_hits
                .load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the digest is checked before the artifact is downloaded"
        );
        assert!(
            !install.join(update::STAGED_EXE).exists(),
            "a rolled-back artifact must not be staged again"
        );
    }

    #[tokio::test]
    async fn the_check_update_mode_allows_a_rebuilt_same_version() {
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the rebuilt artifact".to_vec();
        let server = serve_release(
            &key,
            "2.0.3",
            summrise_release::Channel::Stable,
            "2.0.0",
            artifact.clone(),
        )
        .await;

        let dir = TempDir::new("check-update-rebuilt-mode");
        let install = dir.join("install");
        let data = dir.join("data");
        std::fs::create_dir_all(&install).expect("the install directory is creatable");
        let store = Store::new(&install, &data, Release::standalone());
        state_with_rolled_back(&store, &"ab".repeat(32), 1_700_000_000_000);
        write_instant_swap_answer(&install);

        let result = check_update_from_args_with_key(
            &check_update_args(&server.document_url, &install, &data),
            Ok(key.verifying_key()),
        )
        .await
        .expect("a different artifact with the same version is not the one that failed");

        assert_eq!(swap(result).outcome, SwapOutcome::Applied);
        assert_eq!(
            std::fs::read(install.join(update::STAGED_EXE)).expect("the artifact is staged"),
            artifact
        );
    }

    #[tokio::test]
    async fn force_reinstall_overrides_the_rolled_back_artifact_and_audits_it() {
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the artifact the operator fixed".to_vec();
        let server = serve_release(
            &key,
            "2.0.3",
            summrise_release::Channel::Stable,
            "2.0.0",
            artifact.clone(),
        )
        .await;
        let sha256 = summrise_release::sha256_hex(&artifact);

        let dir = TempDir::new("check-update-force-mode");
        let install = dir.join("install");
        let data = dir.join("data");
        std::fs::create_dir_all(&install).expect("the install directory is creatable");
        let store = Store::new(&install, &data, Release::standalone());
        state_with_rolled_back(&store, &sha256, 1_700_000_000_000);
        write_instant_swap_answer(&install);

        let result = check_update_from_args_with_key(
            &force_reinstall_args(&server.document_url, &install, &data, &sha256),
            Ok(key.verifying_key()),
        )
        .await
        .expect("the operator's exact digest overrides the refusal");

        assert_eq!(swap(result).outcome, SwapOutcome::Applied);
        let audit = std::fs::read_to_string(data.join("events.jsonl"))
            .expect("the override is audited in the device's event log");
        let line: serde_json::Value = serde_json::from_str(
            audit
                .lines()
                .next()
                .expect("the audit log carries the override line"),
        )
        .expect("the audit line is JSON");
        assert_eq!(line["event"], "force_reinstall");
        assert_eq!(line["version"], "2.0.3");
        assert_eq!(line["sha256"], serde_json::json!(sha256));
        assert_eq!(line["rolled_back_at_ms"], 1_700_000_000_000u64);
    }

    #[tokio::test]
    async fn the_check_update_mode_stages_a_stable_document_on_a_canary_device() {
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"a stable release a canary device follows".to_vec();
        let server = serve_release(
            &key,
            "2.0.3",
            summrise_release::Channel::Stable,
            "2.0.0",
            artifact.clone(),
        )
        .await;

        let dir = TempDir::new("check-update-canary-device");
        let install = dir.join("install");
        let data = dir.join("data");
        std::fs::create_dir_all(&install).expect("the install directory is creatable");
        let store = Store::new(&install, &data, Release::standalone());
        let mut state = store.initial();
        state.install.version = "2.0.0".to_owned();
        store.save(&state).expect("the fixture state is writable");
        write_instant_swap_answer(&install);

        let result = check_update_from_args_with_key(
            &check_update_args_on(&server.document_url, "canary", &install, &data),
            Ok(key.verifying_key()),
        )
        .await
        .expect("a canary device accepts both channels");

        assert_eq!(swap(result).outcome, SwapOutcome::Applied);
        assert_eq!(
            std::fs::read(install.join(update::STAGED_EXE)).expect("the artifact is staged"),
            artifact,
            "the stable document reached the stager on a canary device"
        );
        assert_eq!(
            server
                .artifact_hits
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    #[tokio::test]
    async fn the_check_update_mode_reports_up_to_date_and_changes_nothing() {
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the release already installed".to_vec();
        let server = serve_release(
            &key,
            "2.0.3",
            summrise_release::Channel::Stable,
            "2.0.0",
            artifact,
        )
        .await;

        let dir = TempDir::new("check-update-up-to-date");
        let install = dir.join("install");
        let data = dir.join("data");
        std::fs::create_dir_all(&install).expect("the install directory is creatable");
        let store = Store::new(&install, &data, Release::standalone());
        let mut state = store.initial();
        state.install.version = "2.0.3".to_owned();
        store.save(&state).expect("the fixture state is writable");

        let result = check_update_from_args_with_key(
            &check_update_args(&server.document_url, &install, &data),
            Ok(key.verifying_key()),
        )
        .await
        .expect("a document naming the installed version is an answer");

        assert!(
            matches!(&result, CheckUpdateOutcome::UpToDate { version } if version == "2.0.3"),
            "{result:?}"
        );
        assert_eq!(
            check_update_exit_code(&Ok(result)),
            0,
            "already current is exit 0, so re-running a canary check reads as success"
        );
        assert_eq!(up_to_date_line("2.0.3"), "up_to_date 2.0.3");
        assert_eq!(
            server
                .artifact_hits
                .load(std::sync::atomic::Ordering::SeqCst),
            0,
            "nothing to do means the artifact is not downloaded"
        );
        assert!(
            !install.join(update::STAGED_EXE).exists(),
            "up to date must not stage an artifact"
        );
        assert!(
            !install.join(update::SWAP_REQUEST).exists(),
            "up to date must not ask the supervisor for a swap"
        );
        let after = store.load().expect("the state document is readable");
        assert_eq!(
            after.update.phase,
            UpdatePhase::Idle,
            "the update record is untouched by an up-to-date check"
        );
        assert_eq!(after.install.version, "2.0.3");
    }

    #[tokio::test]
    async fn the_check_update_mode_refuses_a_device_whose_state_document_it_cannot_read() {
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the release artifact".to_vec();
        let server = serve_release(
            &key,
            "2.0.3",
            summrise_release::Channel::Stable,
            "2.0.0",
            artifact,
        )
        .await;

        let dir = TempDir::new("check-update-baseline-mode");
        let install = dir.join("install");
        std::fs::create_dir_all(&install).expect("the install directory is creatable");
        // A device with a baseline but no state document is exactly the bypass I3 found: the
        // baseline can name the installed version, but it carries no rollback memory.
        write_baseline_sidecar(&install, "2.0.2");
        write_instant_swap_answer(&install);

        let result = check_update_from_args_with_key(
            &[
                OsString::from(&server.document_url),
                OsString::from("--install"),
                install.as_os_str().to_owned(),
            ],
            Ok(key.verifying_key()),
        )
        .await;

        assert_eq!(
            result,
            Err(CheckUpdateModeError::MissingData),
            "without a readable state document the mode refuses instead of reporting an empty memory"
        );
        assert_eq!(
            check_update_exit_code(&result),
            2,
            "a missing state document is a usage error that names --data"
        );
        assert_eq!(
            server
                .artifact_hits
                .load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the refusal happens before the artifact is fetched"
        );
        assert!(
            !install.join(update::STAGED_EXE).exists(),
            "a refused check stages nothing"
        );
        assert!(
            !install.join(update::SWAP_REQUEST).exists(),
            "a refused check asks no supervisor to swap"
        );
    }

    #[tokio::test]
    async fn the_check_update_mode_refuses_when_no_installed_version_is_known() {
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the release artifact".to_vec();
        let server = serve_release(
            &key,
            "2.0.3",
            summrise_release::Channel::Stable,
            "2.0.0",
            artifact,
        )
        .await;

        let dir = TempDir::new("check-update-no-version");
        let install = dir.join("install");
        std::fs::create_dir_all(&install).expect("the install directory is creatable");

        let result = check_update_from_args_with_key(
            &[
                OsString::from(&server.document_url),
                OsString::from("--install"),
                install.as_os_str().to_owned(),
            ],
            Ok(key.verifying_key()),
        )
        .await;

        assert_eq!(result, Err(CheckUpdateModeError::MissingData));
        assert_eq!(
            check_update_exit_code(&result),
            2,
            "an unknown installed version is a usage error, not a policy refusal"
        );
        assert_eq!(
            server
                .artifact_hits
                .load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the mode must not fetch the artifact before it knows the installed version"
        );
        assert!(
            !install.join(update::STAGED_EXE).exists(),
            "nothing may be staged when the local version is unknown"
        );
    }

    #[tokio::test]
    async fn the_check_update_mode_leaves_the_state_document_alone() {
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the release artifact".to_vec();
        let server = serve_release(
            &key,
            "2.0.3",
            summrise_release::Channel::Stable,
            "2.0.0",
            artifact,
        )
        .await;

        let dir = TempDir::new("check-update-leaves-state");
        let install = dir.join("install");
        let data = dir.join("data");
        std::fs::create_dir_all(&install).expect("the install directory is creatable");
        let store = Store::new(&install, &data, Release::standalone());
        let mut state = store.initial();
        state.install.version = "2.0.0".to_owned();
        store.save(&state).expect("the fixture state is writable");
        let before = std::fs::read(store.state_path()).expect("the fixture state is readable");

        // The supervisor's answer is already on disk, so the mode does not wait.
        std::fs::write(
            install.join(update::SWAP_RESULT),
            serde_json::json!({
                "outcome": "refused",
                "version": "2.0.3",
                "at_ms": u64::MAX,
                "reason": "the lane's fixture refusal",
            })
            .to_string(),
        )
        .expect("the fixture swap answer is writable");

        let result = check_update_from_args_with_key(
            &check_update_args(&server.document_url, &install, &data),
            Ok(key.verifying_key()),
        )
        .await
        .expect("the supervisor's refusal is still an answer");

        assert_eq!(
            check_update_exit_code(&Ok(result.clone())),
            4,
            "the supervisor's refusal is the lane's 4"
        );
        assert_eq!(swap(result).outcome, SwapOutcome::Refused);
        assert!(
            install.join(update::SWAP_REQUEST).exists(),
            "the request is the durable fact the CLI leaves for the serving app"
        );
        assert_eq!(
            std::fs::read(store.state_path()).expect("the state document is readable"),
            before,
            "the CLI never writes the state document: the app folds the request in at its next poll"
        );
        let after = store.load().expect("the state document is readable");
        assert_eq!(
            after.update.phase,
            UpdatePhase::Idle,
            "nothing recorded the stage because the serving app was not the one running"
        );
        assert_eq!(after.update.to, None);
    }

    #[tokio::test]
    async fn the_check_update_mode_leaves_no_stage_when_the_request_cannot_be_written() {
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the release artifact".to_vec();
        let server = serve_release(
            &key,
            "2.0.3",
            summrise_release::Channel::Stable,
            "2.0.0",
            artifact,
        )
        .await;

        let dir = TempDir::new("check-update-failed-request");
        let install = dir.join("install");
        let data = dir.join("data");
        std::fs::create_dir_all(&install).expect("the install directory is creatable");
        let store = Store::new(&install, &data, Release::standalone());
        let mut state = store.initial();
        state.install.version = "2.0.0".to_owned();
        store.save(&state).expect("the fixture state is writable");
        let before = std::fs::read(store.state_path()).expect("the fixture state is readable");

        // A directory at the request path makes the atomic write fail. Nothing was staged, so the
        // document must say exactly what it said before: there is no state write to leave behind.
        std::fs::create_dir_all(install.join(update::SWAP_REQUEST))
            .expect("the blocking directory is creatable");

        let result = check_update_from_args_with_key(
            &check_update_args(&server.document_url, &install, &data),
            Ok(key.verifying_key()),
        )
        .await;

        assert!(
            matches!(result, Err(CheckUpdateModeError::Failed(ref message))
                if message.contains("cannot request the swap")),
            "{result:?}"
        );
        assert_eq!(
            std::fs::read(store.state_path()).expect("the state document is readable"),
            before,
            "a failed request leaves the document exactly as it was"
        );
        let after = store.load().expect("the state document is readable");
        assert_eq!(after.update.phase, UpdatePhase::Idle);
        assert_eq!(after.update.to, None);
    }

    #[test]
    fn the_check_update_mode_exit_codes_are_the_lane_contract() {
        let result = |outcome| SwapResult {
            outcome,
            version: Some("2.0.3".to_owned()),
            sha256: None,
            at_ms: 1,
            reason: None,
        };

        assert_eq!(
            check_update_exit_code(&Ok(CheckUpdateOutcome::Swap(result(SwapOutcome::Applied)))),
            0
        );
        assert_eq!(
            check_update_exit_code(&Ok(CheckUpdateOutcome::Swap(result(
                SwapOutcome::RolledBack
            )))),
            3
        );
        assert_eq!(
            check_update_exit_code(&Ok(CheckUpdateOutcome::Swap(result(SwapOutcome::Refused)))),
            4
        );
        assert_eq!(
            check_update_exit_code(&Ok(CheckUpdateOutcome::UpToDate {
                version: "2.0.3".to_owned()
            })),
            0,
            "an already-current device is a success: the lane's fourth outcome is exit 0"
        );
        assert_eq!(up_to_date_line("2.0.3"), "up_to_date 2.0.3");
        assert_eq!(
            check_update_exit_code(&Err(CheckUpdateModeError::Refused(
                "this device does not accept the release's channel".to_owned()
            ))),
            4,
            "a refused document and a refused swap are the same lane answer"
        );
        assert_eq!(
            check_update_exit_code(&Err(CheckUpdateModeError::Usage)),
            2,
            "a malformed command line is a usage error, not a failed update"
        );
        assert_eq!(
            check_update_exit_code(&Err(CheckUpdateModeError::Failed(
                "the release was not staged".to_owned()
            ))),
            1
        );
    }
}
