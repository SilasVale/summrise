//! `summrise-launch` — the one process that writes the app binary (design §4.1, §4.4).
//!
//! The launcher exists for one reason: the process that repairs a missing app binary cannot be the
//! app binary. It is the root of the repair path, so the pure half below is deliberately boring:
//!
//! 1. **verify or restore** at every start — the app must hash to the current baseline generation's
//!    sha256, or that generation is copied over it (`current`, then `previous`, then a human);
//! 2. **supervise** the app child — at most three *consecutive* restarts; a child that stays up for
//!    [`STABLE_AFTER_MS`] resets the budget;
//! 3. **swap** through one staged pair and a baseline transaction — `pending` is written and synced,
//!    the app is replaced from it, two build-bound health probes must pass, and only then do
//!    `current` and `previous` rotate (fix design F1/F2/F4);
//! 4. **version an install** (2026-11-05 design) — a release lives in
//!    `<install>/versions/<name>/`, `current.txt` names the one to run, and an update is one atomic
//!    pointer write after the candidate has proven it can serve. Nothing running is ever overwritten.
//!
//! Everything in this module is pure or ordinary `std` I/O, so it is exercised by `cargo test` on
//! Linux. The SCM, process termination and the supervisor loop live in the Windows binary.

#![forbid(unsafe_code)]

mod paths;
mod self_check;

pub use paths::{
    app_exe, app_pid_file, baseline_dir, baseline_exe, baseline_sidecar, console_link,
    current_pointer, flat_exe, flat_sidecar, generation_dir, generation_exe, generation_sidecar,
    install_token, launcher_pid_file, previous_pointer, repair_log, staged_exe, staged_sidecar,
    swap_request, swap_result, version_dir, version_exe, version_sidecar, versions_dir, APP_EXE,
    BASELINE_DIR, BASELINE_SIDECAR, CONSOLE_FILE, CURRENT_DIR, CURRENT_POINTER, FLAT_SIDECAR,
    LAUNCHER_EXE, PENDING_DIR, PREVIOUS_DIR, PREVIOUS_POINTER, REPAIR_LOG, STAGING_DIR,
    VERSIONS_DIR, VERSION_SIDECAR,
};
pub use self_check::{inspect, Check, Report};

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The placeholder that ships when no release key was configured at build time.
///
/// All-zero is not a key anybody signs with: [`parse_pubkey`] refuses it by name
/// ([`SigError::Placeholder`]), so a swap is refused and a phase-1 update is a full reinstall.
pub const PLACEHOLDER_PUBKEY_HEX: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// The launcher public key, embedded at build time.
///
/// This key owns the **first generation**: the `--seed-sidecar` the installer is given, and the
/// baseline pair it copies into `baseline/current`. The launcher is never replaced by an update, so
/// changing this key is the deliberate full-reinstall boundary described in the phase-1 design.
///
/// The seam is **compile-time** (`SUMMRISE_LAUNCHER_PUBKEY` in the build environment), never a
/// runtime override: a key that can be swapped by the environment of a running service would make
/// the signature check optional. Without the variable the placeholder ships, and every seed is
/// refused by name.
pub const LAUNCHER_PUBKEY_HEX: &str = match option_env!("SUMMRISE_LAUNCHER_PUBKEY") {
    Some(key) => key,
    None => PLACEHOLDER_PUBKEY_HEX,
};

/// The release public key, embedded at build time.
///
/// This key owns **updates**: the app writes `<install>/summrise-device.new.json` from a
/// release-key-signed manifest, and the launcher verifies that staged pair before it stops the
/// running app. It is a second compile-time seam (`SUMMRISE_RELEASE_PUBKEY`) because one key cannot
/// be both the install boundary and the rotating release key. Without it, updates fail closed with
/// the placeholder refusal while the seed baseline still works.
pub const RELEASE_PUBKEY_HEX: &str = match option_env!("SUMMRISE_RELEASE_PUBKEY") {
    Some(key) => key,
    None => PLACEHOLDER_PUBKEY_HEX,
};

/// Port the app serves on when neither `--port` nor `SUMMRISE_PORT` is set. `7737` since 2026-11-04
/// (the operator's fleet convention; `18080` before it).
pub const DEFAULT_PORT: u16 = 7_737;

/// Number of random bytes in the install token (design §9.5; review I9).
pub const TOKEN_BYTES: usize = 32;

/// Copies allowed when repairing the app from a baseline generation before a human is needed.
pub const RESTORE_ATTEMPTS: u8 = 3;

/// Restarts allowed after consecutive child failures before the supervisor gives up.
pub const MAX_RESTARTS: u8 = 3;

/// Backoff before each restart, in order. The budget indexes this table; past its end, give up.
pub const RESTART_BACKOFF_MS: [u64; MAX_RESTARTS as usize] = [1_000, 2_000, 4_000];

/// How long a child must stay alive for its exit to not count as a consecutive failure.
pub const STABLE_AFTER_MS: u64 = 60_000;

/// A swap request older than this is discarded with `outcome:"refused"` (fix design F1).
pub const SWAP_REQUEST_TIMEOUT_MS: u64 = 10 * 60 * 1_000;

/// How many matching health answers in a row prove a newly started build.
pub const HEALTH_CONSECUTIVE_PROBES: u8 = 2;

/// How long a candidate gets to answer `/api/health` twice on its scratch port.
pub const CANDIDATE_HEALTH_TIMEOUT: Duration = Duration::from_secs(30);

/// The `JOB_OBJECT_LIMIT_*` flags of the supervisor's child job (Win32's fixed values: 0x2000
/// `KILL_ON_JOB_CLOSE`, 0x1000 `SILENT_BREAKAWAY_OK`).
///
/// `KILL_ON_JOB_CLOSE` is the promise to the SCM: a supervisor that dies takes the app it was
/// supervising with it, so the next start finds no orphan holding the port.
///
/// `SILENT_BREAKAWAY_OK` narrows that promise to the app itself. Without it a process the app
/// starts — a `runs.exec` command, the installer on an upgrade driven through the device — is in
/// the job too, and stopping the service closes the job and kills the very installer that asked for
/// the stop, before it can start the service again (measured 2026-11-06 on DESKTOP-9VH92BJ: a
/// silent install run through the device's own shell left `SummriseLaunch` stopped). The app's
/// descendants already have their own lifetimes (the browser owns a kill-on-close job, and a
/// console session ends with its pseudoconsole); the supervisor's job never promised to be their
/// custodian.
pub const CHILD_JOB_LIMITS: u32 = 0x2000 | 0x1000;

/// What a start-up verification found (or did).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Restore {
    /// The app binary exists and hashes to the baseline's sha256.
    Ok,
    /// The app was missing, empty or corrupt, and the baseline generation was copied over it.
    Restored,
    /// The launcher cannot repair this by itself; a human is needed. The string says why.
    NeedsHuman(String),
}

/// What the supervisor should do after the app child has exited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Start the child; nothing has failed yet.
    Start,
    /// Wait this long, then start the child again.
    RetryAfterMs(u64),
    /// Three consecutive restarts have already been tried; stop and surface the failure.
    GiveUp,
}

/// The exit code the launcher reports for a child that has finished. A give-up must look like a
/// failure to the SCM, so zero and Windows' negative crash codes map to 1; a positive code is kept.
/// A planned restart (exit 0) is retried by the budget, not reported as a failure.
pub fn failure_code(code: Option<i32>) -> u32 {
    code.map_or(1, |code| code.max(1) as u32)
}

/// Whether two Windows image paths name the same file. The kernel may report the verbatim form
/// (`\\?\C:\...`); the install's path is the ordinary form, so both are compared stripped of that
/// prefix and case-insensitively. This is a path comparison, not a file-name one: a second install
/// with the same executable name must not be mistaken for this one.
pub fn same_image(actual: &Path, expected: &Path) -> bool {
    let trim = |path: &Path| -> String {
        let text = path.to_string_lossy().into_owned();
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
    };
    trim(actual).eq_ignore_ascii_case(&trim(expected))
}

/// The consecutive-restart budget.
///
/// The budget is deliberately not a lifetime counter: a device that restarted once a month must
/// not give up on the fourth restart. A child that was alive for [`STABLE_AFTER_MS`] resets it,
/// because that exit is the start of a new failure sequence, not the end of an old one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RestartBudget {
    attempts: u8,
}

impl RestartBudget {
    pub fn new() -> RestartBudget {
        RestartBudget::default()
    }

    /// Restarts already spent in the current consecutive-failure sequence.
    pub fn attempts(&self) -> u8 {
        self.attempts
    }

    /// Account for one child exit and return the next action.
    ///
    /// `alive_for` is how long that child lived. A stable child clears the sequence before the
    /// action is chosen; only an actual retry consumes budget, so the action after the third
    /// restart is [`Action::GiveUp`] and stays there until a start succeeds.
    pub fn on_child_exit(&mut self, alive_for: Duration) -> Action {
        if alive_for >= Duration::from_millis(STABLE_AFTER_MS) {
            self.attempts = 0;
        }
        let action = next_action(self.attempts, true);
        if matches!(action, Action::RetryAfterMs(_)) {
            self.attempts = self.attempts.saturating_add(1);
        }
        action
    }
}

/// The signed facts about one build.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sidecar {
    /// The release version, e.g. `2.0.3`.
    pub version: String,
    /// The release channel the signature was made for, e.g. `stable`.
    pub channel: String,
    /// Lower-hex SHA-256 of the binary.
    pub sha256: String,
    /// Lower-hex ed25519 signature over `"{version}\n{channel}\n{sha256}"`.
    pub signature: String,
}

/// Why a sidecar's signature or shape could not be accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SigError {
    /// The launcher was built with the placeholder key, so no release can be trusted.
    Placeholder,
    /// The embedded/test public key is not 32 bytes of hex.
    BadPubkey,
    /// The version is empty or contains a newline, which would make the signed message ambiguous.
    MalformedVersion(String),
    /// The channel is empty or contains a newline, which would make the signed message ambiguous.
    MalformedChannel(String),
    /// The digest is not 64 hex characters.
    MalformedSha(String),
    /// The signature is not 64 bytes of hex.
    MalformedSignature(String),
    /// The signature is well-formed but does not verify over `"{version}\n{channel}\n{sha256}"`.
    BadSignature,
}

impl fmt::Display for SigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SigError::Placeholder => write!(
                f,
                "the launcher was built with the phase-1 placeholder release key (set \
                 SUMMRISE_LAUNCHER_PUBKEY for the seed baseline and SUMMRISE_RELEASE_PUBKEY for \
                 updates at build time)"
            ),
            SigError::BadPubkey => write!(f, "public key is not 32 bytes of hex"),
            SigError::MalformedVersion(version) => {
                write!(f, "version is empty or contains a newline: {version:?}")
            }
            SigError::MalformedChannel(channel) => {
                write!(f, "channel is empty or contains a newline: {channel:?}")
            }
            SigError::MalformedSha(sha) => write!(f, "sha256 is not 64 hex characters: {sha:?}"),
            SigError::MalformedSignature(sig) => {
                write!(f, "signature is not 64 bytes of hex: {sig:?}")
            }
            SigError::BadSignature => write!(f, "signature does not verify"),
        }
    }
}

impl std::error::Error for SigError {}

/// The launcher's own arguments: where the install lives, where mutable data lives, and the port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// The install root; the app binary and the baseline live here.
    pub install: PathBuf,
    /// The writable data root; pid files live here.
    pub data: PathBuf,
    /// The loopback port the app serves on.
    pub port: u16,
}

/// Parse `--install`, `--data` and `--port`, defaulting the port from `env_port` and then
/// [`DEFAULT_PORT`], and the data directory to `<install>/data`.
pub fn parse_config(
    args: &[String],
    default_install: &Path,
    env_port: Option<&str>,
) -> Result<Config, String> {
    let mut install = default_install.to_path_buf();
    let mut data: Option<PathBuf> = None;
    let mut flag_port: Option<u16> = None;

    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("{flag} needs a value"))?;
        match flag {
            "--install" => install = PathBuf::from(value),
            "--data" => data = Some(PathBuf::from(value)),
            "--port" => flag_port = Some(parse_port(value)?),
            other => return Err(format!("unknown argument: {other}")),
        }
        index += 2;
    }

    let port = match flag_port {
        Some(port) => port,
        None => match env_port.map(str::trim).filter(|value| !value.is_empty()) {
            Some(value) => {
                parse_port(value).map_err(|_| format!("invalid SUMMRISE_PORT: {value:?}"))?
            }
            None => DEFAULT_PORT,
        },
    };
    let data = data.unwrap_or_else(|| install.join("data"));
    Ok(Config {
        install,
        data,
        port,
    })
}

fn parse_port(value: &str) -> Result<u16, String> {
    let port: u16 = value
        .parse()
        .map_err(|_| format!("invalid port: {value:?}"))?;
    if port == 0 {
        return Err("port must be between 1 and 65535".to_string());
    }
    Ok(port)
}

/// The remote link `--install-service` and `run` persist at `<data>/console.json`.
///
/// The file keeps its name for compatibility, and it carries the facts a supervised install
/// cannot put on the service's argv: the console's (URL and registration file) and the mesh's
/// (rendezvous broker, relay floor, and STUN reflector). On a supervised install neither a flag
/// nor the `SUMMRISE_*` environment exists — the SCM starts a fixed argv and `--install-service`
/// cannot set the service's environment — so this file is the one path that arms the console or
/// the mesh without starting a second app (C3).
///
/// Every field is optional and an absent one is omitted from the JSON rather than written as
/// `null`: an install with no mesh must not leave a file that claims one, and the app's own
/// resolution applies the same rule in reverse.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsoleLink {
    /// The console's base URL, when this install has a console.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The file holding the registration response (`id`, `token`) the console returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_file: Option<PathBuf>,
    /// The mesh rendezvous broker's origin, when this install joins the mesh.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh_broker: Option<String>,
    /// The mesh relay floor's address, when one is configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh_relay: Option<String>,
    /// The mesh STUN reflector's address, when one is configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh_stun: Option<String>,
    /// A second files root (`shared`, e.g. `H:\\tmp`) for supervised installs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_shared_root: Option<PathBuf>,
}

/// Check a broker value is the plain-HTTP origin the rendezvous client dials.
///
/// The launcher refuses the spellings the app would refuse at startup: a supervised install that
/// persists an unusable broker registers a service that can only fail, and that failure belongs in
/// the installer's output, not in the service log an operator reads later. The rule is the app's
/// (`http://`, an authority, no path/query/fragment); this is a syntactic check, not a reachability
/// test — the rendezvous still decides whether the origin answers.
fn validate_mesh_broker(broker: &str) -> Result<(), String> {
    let text = broker.trim();
    let Some(authority) = text.strip_prefix("http://") else {
        return Err(format!(
            "--mesh-broker expects an http://host:port origin, got {text:?}"
        ));
    };
    // The app accepts an origin written with its empty path as either "" or "/"; every other path
    // is a URL the rendezvous client refuses.
    let authority = authority.strip_suffix('/').unwrap_or(authority);
    let origin = !authority.is_empty()
        && !authority.starts_with('/')
        && !authority.contains('/')
        && !authority.contains('?')
        && !authority.contains('#')
        && !authority.chars().any(char::is_whitespace);
    if !origin {
        return Err(format!(
            "--mesh-broker expects an http://host:port origin, got {text:?}"
        ));
    }
    Ok(())
}

/// Check a relay value is the socket address the app parses.
///
/// A relay is a floor a rendezvous leads to, never a hostname the device resolves at startup: the
/// app parses it as [`SocketAddr`], so the launcher refuses the same spellings before persisting
/// one the service could not use.
fn validate_mesh_relay(relay: &str) -> Result<(), String> {
    let text = relay.trim();
    if text.parse::<SocketAddr>().is_err() {
        return Err(format!("--mesh-relay expects host:port, got {text:?}"));
    }
    Ok(())
}

/// Check a STUN reflector value is the socket address the app parses.
///
/// A reflector is an address, not a hostname: the app parses it as [`SocketAddr`], so the
/// launcher refuses the same spellings before persisting one the service could not use.
fn validate_mesh_stun(stun: &str) -> Result<(), String> {
    let text = stun.trim();
    if text.parse::<SocketAddr>().is_err() {
        return Err(format!("--mesh-stun expects host:port, got {text:?}"));
    }
    Ok(())
}

/// Split `--console-url` / `--console-token-file` out of a launcher argv.
///
/// The two console flags are all-or-nothing: a URL without a token file would arm a channel with no
/// credential, and the app would refuse the pair anyway. The mesh flags are the same pair rule in
/// the other direction: a broker alone is a direct-only mesh, but a relay without a broker could
/// never be reached. The rest of the arguments are returned untouched for [`parse_config`], so the
/// launcher keeps one spelling of install/data/port.
pub fn split_console_args(args: &[String]) -> Result<(Vec<String>, Option<ConsoleLink>), String> {
    let mut rest = Vec::with_capacity(args.len());
    let mut url: Option<String> = None;
    let mut token_file: Option<PathBuf> = None;
    let mut mesh_broker: Option<String> = None;
    let mut mesh_relay: Option<String> = None;
    let mut mesh_stun: Option<String> = None;
    let mut files_shared_root: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--console-url" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "--console-url needs a value".to_string())?;
                url = Some(value.clone());
                index += 2;
            }
            "--console-token-file" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "--console-token-file needs a value".to_string())?;
                token_file = Some(PathBuf::from(value));
                index += 2;
            }
            "--mesh-broker" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "--mesh-broker needs a value".to_string())?;
                mesh_broker = Some(value.clone());
                index += 2;
            }
            "--mesh-relay" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "--mesh-relay needs a value".to_string())?;
                mesh_relay = Some(value.clone());
                index += 2;
            }
            "--mesh-stun" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "--mesh-stun needs a value".to_string())?;
                mesh_stun = Some(value.clone());
                index += 2;
            }
            "--files-shared-root" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "--files-shared-root needs a value".to_string())?;
                if value.trim().is_empty() {
                    return Err("--files-shared-root is empty".to_string());
                }
                files_shared_root = Some(PathBuf::from(value));
                index += 2;
            }
            _ => {
                rest.push(args[index].clone());
                index += 1;
            }
        }
    }

    // The console pair is all-or-nothing: a URL without a token file would arm a channel with no
    // credential, and the app refuses that pair too.
    match (&url, &token_file) {
        (Some(url), Some(token_file)) => {
            if url.trim().is_empty() {
                return Err("--console-url is empty".to_string());
            }
            if token_file.as_os_str().is_empty() {
                return Err("--console-token-file is empty".to_string());
            }
        }
        (Some(_), None) => {
            return Err(
                "--console-url needs --console-token-file as well: the console link is \
                 all-or-nothing"
                    .to_string(),
            )
        }
        (None, Some(_)) => {
            return Err(
                "--console-token-file needs --console-url as well: the console link is \
                 all-or-nothing"
                    .to_string(),
            )
        }
        (None, None) => {}
    }
    // The relay is the floor a rendezvous fallback lands on, and a STUN reflector only names the
    // address the rendezvous publishes. Without a broker neither could ever be used: refused here
    // for the same reason the app refuses it.
    if (mesh_relay.is_some() || mesh_stun.is_some()) && mesh_broker.is_none() {
        return Err(
            "--mesh-relay and --mesh-stun need --mesh-broker as well: a relay is the floor a \
             rendezvous leads to, and a reflector names an address the rendezvous publishes"
                .to_string(),
        );
    }
    if let Some(broker) = &mesh_broker {
        validate_mesh_broker(broker)?;
    }
    if let Some(relay) = &mesh_relay {
        validate_mesh_relay(relay)?;
    }
    if let Some(stun) = &mesh_stun {
        validate_mesh_stun(stun)?;
    }

    if url.is_none()
        && token_file.is_none()
        && mesh_broker.is_none()
        && mesh_relay.is_none()
        && mesh_stun.is_none()
        && files_shared_root.is_none()
    {
        // Nothing to persist: an install with no remote facts leaves any previous link alone (the
        // operator's only disarm is to remove the file).
        return Ok((rest, None));
    }
    Ok((
        rest,
        Some(ConsoleLink {
            url,
            token_file,
            mesh_broker,
            mesh_relay,
            mesh_stun,
            files_shared_root,
        }),
    ))
}

/// Write the console link at `<data>/console.json`, owner-only on Unix.
///
/// It is a credential path (and on a shared box any reader could repoint the device's channel), so
/// the mode is 0600 at creation *and* after the rename, which is the same fail-closed shape the
/// installer uses for the token. The write is a random temporary sibling + rename: a crash can only
/// leave the previous link or a complete new one.
pub fn write_console_link(data: &Path, link: &ConsoleLink) -> Result<(), String> {
    fs::create_dir_all(data)
        .map_err(|error| format!("cannot create {}: {error}", data.display()))?;
    let path = console_link(data);
    let bytes = serde_json::to_vec(link)
        .map_err(|error| format!("cannot serialize the console link: {error}"))?;
    let (temporary, mut file) = create_temp_sibling(&path).map_err(|error| {
        format!(
            "cannot create the console link under {}: {error}",
            data.display()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(error) = fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600)) {
            let _ = fs::remove_file(&temporary);
            return Err(format!(
                "cannot set the console link's mode to 0600 ({}): {error}",
                temporary.display()
            ));
        }
    }
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(format!("cannot write {}: {error}", temporary.display()));
    }
    drop(file);
    if let Err(error) = fs::rename(&temporary, &path) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("cannot install {}: {error}", path.display()));
    }
    Ok(())
}

/// The link a launcher run writes: the operator's facts, with the token path made absolute and an
/// existing STUN reflector kept when the rewrite names the mesh but not a reflector.
///
/// A rewrite is a replacement, not a merge: the flags the operator passed are the link. The
/// reflector is the one exception because it is a mesh fact a broker/relay rewrite has no reason to
/// drop — losing it silently disables the direct path the previous install configured. A rewrite
/// that names a new reflector replaces it; one that names the mesh without a reflector keeps the
/// existing one, and removing the file is still how the operator disarms everything.
pub fn link_to_persist(data: &Path, link: &ConsoleLink) -> Result<ConsoleLink, String> {
    let mut link = ConsoleLink {
        url: link.url.clone(),
        token_file: link.token_file.as_deref().map(absolute),
        mesh_broker: link.mesh_broker.clone(),
        mesh_relay: link.mesh_relay.clone(),
        mesh_stun: link.mesh_stun.clone(),
        files_shared_root: link.files_shared_root.as_deref().map(absolute),
    };
    if link.mesh_broker.is_some() && link.mesh_stun.is_none() {
        link.mesh_stun = read_persisted_mesh_stun(data)?;
    }
    Ok(link)
}

/// The `mesh_stun` a previous launcher run persisted, if any.
///
/// Read leniently: `--enroll` preserves fields this launcher does not know, and an unknown field
/// must not make a rewrite fail. Only the field this launcher needs to carry forward is read.
fn read_persisted_mesh_stun(data: &Path) -> Result<Option<String>, String> {
    #[derive(Deserialize)]
    struct Persisted {
        #[serde(default)]
        mesh_stun: Option<String>,
    }

    let path = console_link(data);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let persisted: Persisted = serde_json::from_slice(&bytes)
        .map_err(|error| format!("cannot parse {}: {error}", path.display()))?;
    Ok(persisted.mesh_stun)
}

/// The absolute spelling of a path, or the path itself when the OS refuses one.
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The SCM service's argv for this install, as values rather than a hand-built vector.
///
/// One spelling shared by the installer (which registers it), the service (which parses it), and
/// the tests: the data directory on this argv is also where `<data>/console.json` lives, and the
/// two must name the same directory.
pub fn service_launch_arguments(install: &Path, data: &Path, port: u16) -> Vec<std::ffi::OsString> {
    vec![
        std::ffi::OsString::from("service"),
        std::ffi::OsString::from("--install"),
        install.as_os_str().to_os_string(),
        std::ffi::OsString::from("--data"),
        data.as_os_str().to_os_string(),
        std::ffi::OsString::from("--port"),
        std::ffi::OsString::from(port.to_string()),
    ]
}

/// The app child's argv: where it lives, where it writes, and the release identity it must report.
///
/// Wait until the loopback port can actually be **bound**.
///
/// A connect probe answers "is something accepting", which is not the question a restart asks: on
/// Windows a bind fails while the previous listener's connections are still in the kernel (Rust sets
/// `SO_REUSEADDR` only on Unix), and a connect probe cannot see that — the 2026-11-04 canary swap
/// killed the app, spawned its replacement into exactly that window, and the fleet went dark. This
/// binds, drops, and answers the one question that matters.
pub fn wait_for_bindable_port(port: u16, timeout: Duration) -> Result<(), String> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + timeout;
    loop {
        match std::net::TcpListener::bind(address) {
            Ok(listener) => {
                drop(listener);
                return Ok(());
            }
            Err(error) => {
                if Instant::now() >= deadline {
                    return Err(format!(
                        "port {port} is still not bindable after {}s: {error}",
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// The three release facts come from the sidecar the launcher actually booted (F6 plus the channel
/// fix): `install.version`, the baseline digest and `install.channel` in `state.json`, and
/// `/api/health`, are all readings of this one argv. A canary sidecar therefore cannot record a
/// stable install, and a restart under a stable sidecar flips the record back.
pub fn app_arguments(config: &Config, sidecar: &Sidecar) -> Vec<std::ffi::OsString> {
    vec![
        std::ffi::OsString::from("--install"),
        config.install.as_os_str().to_os_string(),
        std::ffi::OsString::from("--data"),
        config.data.as_os_str().to_os_string(),
        std::ffi::OsString::from("--port"),
        std::ffi::OsString::from(config.port.to_string()),
        std::ffi::OsString::from("--release-version"),
        std::ffi::OsString::from(&sidecar.version),
        std::ffi::OsString::from("--release-sha"),
        std::ffi::OsString::from(&sidecar.sha256),
        std::ffi::OsString::from("--release-channel"),
        std::ffi::OsString::from(&sidecar.channel),
        // The token path travels with the install, not with this binary: a non-default `--data`
        // must keep naming the same secret the installer minted.
        std::ffi::OsString::from("--token-file"),
        install_token(&config.data).as_os_str().to_os_string(),
    ]
}

/// Split the arguments that follow `service` into the SCM service name and the launcher's own flags.
///
/// The name is an argument rather than a constant because the SCM's dispatcher table is keyed by it:
/// a process whose `StartServiceCtrlDispatcher` table says `SummriseLaunch` cannot be started as a
/// service registered under any other name — the dispatcher refuses with `ERROR_SERVICE_NOT_IN_EXE`
/// and the start fails with no useful log. The Windows lifecycle test registers `SummriseDeviceTest`
/// through `sc.exe` to prove that a registration we did not write still runs; that requires this
/// seam. The default stays the installer's name, so an ordinary service is unchanged.
///
/// Everything except `--service-name <NAME>` is passed through untouched for [`parse_config`] to
/// accept or refuse, keeping one spelling of the install/data/port flags.
pub fn parse_service_args(
    args: &[String],
    default_name: &str,
) -> Result<(String, Vec<String>), String> {
    let mut name = default_name.to_string();
    let mut flags = Vec::with_capacity(args.len());
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--service-name" {
            let value = args
                .get(index + 1)
                .ok_or_else(|| "--service-name needs a value".to_string())?;
            // A value that starts with `-` is the next flag, not a name; accepting it would silently
            // eat `--install` and leave the launcher with no install directory.
            if value.is_empty() || value.starts_with('-') {
                return Err(format!(
                    "--service-name needs a service name, got {value:?}"
                ));
            }
            name = value.clone();
            index += 2;
        } else {
            flags.push(args[index].clone());
            index += 1;
        }
    }
    Ok((name, flags))
}

/// Lower-hex SHA-256 of `bytes`.
pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

/// Lower-hex SHA-256 of a file, streamed so the app binary is never held in memory.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

/// Lower-hex encoding, used for digests and signatures in sidecars.
pub fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing to a String cannot fail.
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    let (pairs, remainder) = text.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return None;
    }
    pairs
        .iter()
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16)?;
            let low = (pair[1] as char).to_digit(16)?;
            Some((high * 16 + low) as u8)
        })
        .collect()
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Decode the embedded release public key (or a test key) from its hex form.
///
/// The placeholder is refused by name rather than left to fail as a bad point: the caller must be
/// able to say "this build cannot trust releases" instead of "this key is malformed", because the
/// two have different fixes (rebuild with a key vs. repair a sidecar).
pub fn parse_pubkey(hex: &str) -> Result<VerifyingKey, SigError> {
    if hex.eq_ignore_ascii_case(PLACEHOLDER_PUBKEY_HEX) {
        return Err(SigError::Placeholder);
    }
    let bytes = hex_decode(hex).ok_or(SigError::BadPubkey)?;
    let bytes: [u8; 32] = bytes.try_into().map_err(|_| SigError::BadPubkey)?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| SigError::BadPubkey)
}

/// Verify a sidecar's ed25519 signature over exactly `"{version}\n{channel}\n{sha256}"`.
///
/// The newlines are part of the signed message so a version or a channel cannot be moved into the
/// digest and produce a second valid reading of the same signature. The three fields are
/// shape-checked first, so `BadSignature` means a real signature over other bytes, not a typo in a
/// hex string or a smuggled separator. The channel is part of the release's signed identity: a
/// canary artifact cannot be replayed as stable.
pub fn verify_signature(sidecar: &Sidecar, pubkey: &VerifyingKey) -> Result<(), SigError> {
    if sidecar.version.is_empty()
        || sidecar.version.contains('\n')
        || sidecar.version.contains('\r')
    {
        return Err(SigError::MalformedVersion(sidecar.version.clone()));
    }
    if sidecar.channel.is_empty()
        || sidecar.channel.contains('\n')
        || sidecar.channel.contains('\r')
    {
        return Err(SigError::MalformedChannel(sidecar.channel.clone()));
    }
    if !is_sha256_hex(&sidecar.sha256) {
        return Err(SigError::MalformedSha(sidecar.sha256.clone()));
    }
    let signature = hex_decode(&sidecar.signature)
        .filter(|bytes| bytes.len() == 64)
        .ok_or_else(|| SigError::MalformedSignature(sidecar.signature.clone()))?;
    let signature = Signature::from_slice(&signature)
        .map_err(|_| SigError::MalformedSignature(sidecar.signature.clone()))?;
    let message = format!(
        "{}\n{}\n{}",
        sidecar.version, sidecar.channel, sidecar.sha256
    );
    pubkey
        .verify_strict(message.as_bytes(), &signature)
        .map_err(|_| SigError::BadSignature)
}

/// Read a sidecar JSON file.
/// The most a sidecar may be. A sidecar is a version, a digest, a channel and a signature — a few
/// hundred bytes; anything larger is not this file.
pub const MAX_SIDECAR_BYTES: u64 = 64 * 1024;

/// The most a swap request may be: two digests, a version and a timestamp.
pub const MAX_SWAP_REQUEST_BYTES: u64 = 16 * 1024;

/// What a bounded read found.
enum CappedRead {
    Bytes(Vec<u8>),
    /// The file is not there. A caller decides whether that is an absent request or a broken sidecar.
    Missing,
    Failed(String),
}

/// Read at most `limit` bytes of `path`, refusing a file that is larger.
///
/// The staged sidecar and the swap request live in the install directory, which the app can write.
/// A supervisor that reads whatever sits there into memory on every two-second poll trusts the app
/// to be small; a multi-gigabyte file (or a hardlink to one) would be the whole machine's memory.
fn read_capped(path: &Path, limit: u64) -> CappedRead {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return CappedRead::Missing,
        Err(error) => {
            return CappedRead::Failed(format!("cannot read {}: {error}", path.display()))
        }
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(error) => {
            return CappedRead::Failed(format!("cannot stat {}: {error}", path.display()))
        }
    };
    if metadata.len() > limit {
        return CappedRead::Failed(format!(
            "{} is {} bytes; the limit is {limit}",
            path.display(),
            metadata.len()
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    if let Err(error) = std::io::Read::take(file, limit).read_to_end(&mut bytes) {
        return CappedRead::Failed(format!("cannot read {}: {error}", path.display()));
    }
    CappedRead::Bytes(bytes)
}

pub fn load_sidecar(path: &Path) -> Result<Sidecar, String> {
    let bytes = match read_capped(path, MAX_SIDECAR_BYTES) {
        CappedRead::Bytes(bytes) => bytes,
        CappedRead::Missing => return Err(format!("cannot read {}: not found", path.display())),
        CappedRead::Failed(message) => return Err(message),
    };
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("cannot parse sidecar {}: {error}", path.display()))
}

/// Create a brand-new temporary sibling of `path` with an unpredictable name.
///
/// `create_new` is the security property (NEW-8): the old fixed `.tmp` name could be pre-planted as
/// a symlink and `File::create` would follow it, writing the release bytes through the link. A
/// random suffix means the name cannot be guessed, and `create_new` refuses even an exact match, so
/// a planted file can only make the write retry under another name — never redirect it. The name
/// keeps the destination's name so a human reading a directory can tell what a leftover belongs to.
fn create_temp_sibling(path: &Path) -> io::Result<(PathBuf, File)> {
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
        // `RandomState` is seeded from OS entropy per hasher, so the nonce is not derivable from the
        // clock or the pid; the retry loop covers the (astronomically unlikely) collision.
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

/// Flush a directory entry after a rename.
///
/// On Unix this is the fsync that makes a directory rename durable. Windows has no portable
/// directory fsync: a directory is opened with `FILE_FLAG_BACKUP_SEMANTICS` and `FlushFileBuffers`
/// is attempted; some filesystems refuse it by handle, and NTFS's journal still orders the rename,
/// so the two documented "not supported" errors are tolerated rather than turning every swap into a
/// rollback.
fn sync_dir(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        const ERROR_ACCESS_DENIED: i32 = 5;
        const ERROR_INVALID_HANDLE: i32 = 6;
        let file = match fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
        {
            Ok(file) => file,
            Err(error) if error.raw_os_error() == Some(ERROR_ACCESS_DENIED) => return Ok(()),
            Err(error) => return Err(error),
        };
        match file.sync_all() {
            Ok(()) => Ok(()),
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(ERROR_INVALID_HANDLE) | Some(ERROR_ACCESS_DENIED)
                ) =>
            {
                // Not every Windows filesystem/volume exposes a handle-flush for directories.
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
    #[cfg(not(windows))]
    {
        File::open(path)?.sync_all()
    }
}

/// Rename through a short retry window on Windows.
///
/// `TerminateProcess`/`Child::wait` signal that a process is gone, but Windows can keep its image
/// mapped for a moment (an antivirus filter is the usual owner of that delay). Replacing the app
/// binary immediately then fails with a sharing violation, which would turn a healthy update into a
/// rollback. Twenty 100 ms attempts cover the observed teardown without hiding a real permission
/// error; on Unix the predicate is false and the rename is attempted exactly once.
fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    retry_transient(
        || fs::rename(from, to),
        replace_may_be_transient,
        REPLACE_ATTEMPTS,
    )
}

/// The swap's copy, with the same teardown window as the rename: a killed child's image (or an
/// antivirus filter) can hold the destination — or the freshly written source — for longer than a
/// single attempt allows, and a ten-second budget is far below the swap's health deadline.
fn copy_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    retry_transient(
        || copy_file_synced(from, to),
        replace_may_be_transient,
        REPLACE_ATTEMPTS,
    )
}

/// Twenty attempts of one second, in hundred-millisecond steps: the observed teardown window.
const REPLACE_ATTEMPTS: u32 = 100;

/// Run `op`, retrying while `transient` says the error can pass and attempts remain.
fn retry_transient<T>(
    mut op: impl FnMut() -> io::Result<T>,
    mut transient: impl FnMut(&io::Error) -> bool,
    attempts: u32,
) -> io::Result<T> {
    let mut attempt = 0u32;
    loop {
        match op() {
            Ok(value) => return Ok(value),
            Err(error) if attempt < attempts && transient(&error) => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(windows)]
fn replace_may_be_transient(error: &io::Error) -> bool {
    // ERROR_ACCESS_DENIED / ERROR_SHARING_VIOLATION, both of which a closing image can produce.
    error.kind() == io::ErrorKind::PermissionDenied
        || matches!(error.raw_os_error(), Some(5) | Some(32))
}

#[cfg(not(windows))]
fn replace_may_be_transient(_error: &io::Error) -> bool {
    false
}

/// Write `bytes` to `path` atomically: random `.tmp`, `sync_all`, rename, then flush the directory.
fn write_atomic_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let (tmp, mut file) = create_temp_sibling(path)?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    drop(file);
    if let Err(error) = rename_with_retry(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    if let Some(parent) = path.parent() {
        sync_dir(parent)?;
    }
    Ok(())
}

/// Copy `from` to `to` through a random `.tmp` sibling, syncing the bytes before the rename so a
/// crash cannot leave a half-written app binary behind.
fn copy_file_synced(from: &Path, to: &Path) -> io::Result<()> {
    if from == to {
        return Ok(());
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    let (tmp, mut target) = create_temp_sibling(to)?;
    let copied = (|| -> io::Result<()> {
        let mut source = File::open(from)?;
        io::copy(&mut source, &mut target)?;
        target.sync_all()
    })();
    drop(target);
    if let Err(error) = copied {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    if let Err(error) = rename_with_retry(&tmp, to) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    if let Some(parent) = to.parent() {
        sync_dir(parent)?;
    }
    Ok(())
}

/// Write a sidecar JSON file atomically, with the same `.tmp` + `sync_all` + rename discipline as
/// the app's state file.
pub fn save_sidecar(path: &Path, sidecar: &Sidecar) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(sidecar).map_err(io::Error::other)?;
    write_atomic_bytes(path, &bytes)
}

/// One baseline generation, already checked for shape and hash consistency.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Baseline {
    /// Which directory this pair came from.
    pub generation: Generation,
    /// `<install>/baseline/<generation>/summrise-device.exe`.
    pub exe: PathBuf,
    /// `<install>/baseline/<generation>/baseline.json`.
    pub sidecar_path: PathBuf,
    /// The parsed, signed facts from that sidecar.
    pub sidecar: Sidecar,
}

/// The three generations of the baseline transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Generation {
    /// The pair that boots.
    Current,
    /// The pair a failed update returns to.
    Previous,
    /// A written but not yet proven candidate.
    Pending,
}

impl Generation {
    pub fn dir_name(self) -> &'static str {
        match self {
            Generation::Current => CURRENT_DIR,
            Generation::Previous => PREVIOUS_DIR,
            Generation::Pending => PENDING_DIR,
        }
    }

    /// The spelling used in human-facing diagnostics, including the supervisor's result file.
    pub fn label(self) -> &'static str {
        self.dir_name()
    }
}

/// What [`resolve_baseline`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BaselineChoice {
    /// `current` is a consistent pair; boot it.
    Current(Baseline),
    /// `current` is unusable, but `previous` is consistent; boot that and repair `current`.
    Previous(Baseline),
    /// Neither generation can be trusted; a human is needed.
    NeedsHuman(String),
}

/// Inspect one generation: both files present, the sidecar parses, and the binary hashes to the
/// sidecar's sha256. A signature is deliberately not re-checked here: a baseline pair was admitted
/// by [`validate_staged_pair`] before it was written, and this check must still be able to answer
/// for an install created by the installer rather than by a swap.
pub(crate) fn inspect_generation(
    install: &Path,
    generation: Generation,
) -> Result<Baseline, String> {
    let exe = generation_exe(install, generation.dir_name());
    let sidecar_path = generation_sidecar(install, generation.dir_name());
    let sidecar = load_sidecar(&sidecar_path).map_err(|error| {
        format!(
            "the {} baseline sidecar is unusable: {error}",
            generation.label()
        )
    })?;
    if !sidecar.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !is_sha256_hex(&sidecar.sha256)
    {
        return Err(format!(
            "the {} baseline sidecar's sha256 is not 64 hex characters: {:?}",
            generation.label(),
            sidecar.sha256
        ));
    }
    let metadata = match fs::metadata(&exe) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(format!(
                "the {} baseline binary is missing ({})",
                generation.label(),
                exe.display()
            ))
        }
        Err(error) => {
            return Err(format!(
                "the {} baseline binary {} is unreadable: {error}",
                generation.label(),
                exe.display()
            ))
        }
    };
    if !metadata.is_file() {
        return Err(format!(
            "the {} baseline binary {} is not a regular file",
            generation.label(),
            exe.display()
        ));
    }
    if metadata.len() == 0 {
        return Err(format!(
            "the {} baseline binary {} is zero-length",
            generation.label(),
            exe.display()
        ));
    }
    match sha256_file(&exe) {
        Ok(hash) if hash.eq_ignore_ascii_case(&sidecar.sha256) => Ok(Baseline {
            generation,
            exe,
            sidecar_path,
            sidecar,
        }),
        Ok(hash) => Err(format!(
            "the {} baseline binary {} hashes to {hash}, not the sidecar's {}",
            generation.label(),
            exe.display(),
            sidecar.sha256
        )),
        Err(error) => Err(format!(
            "the {} baseline binary {} cannot be hashed: {error}",
            generation.label(),
            exe.display()
        )),
    }
}

/// Resolve the generation to boot: `current`, then `previous`, never `pending`.
///
/// `pending` is not a candidate here by construction: a generation is only promoted by a health
/// probe, and a leftover `pending` is deleted unread at supervisor start.
pub fn resolve_baseline(install: &Path) -> BaselineChoice {
    // The baseline directory is the supervisor's own; an unkeyed build cannot check a signature at
    // all and says so by shipping the placeholder, which no real sidecar was signed with. The
    // hash-consistent check is then the whole answer, exactly as before this seam existed.
    if LAUNCHER_PUBKEY_HEX == PLACEHOLDER_PUBKEY_HEX && RELEASE_PUBKEY_HEX == PLACEHOLDER_PUBKEY_HEX
    {
        resolve_baseline_hash_only(install)
    } else {
        resolve_baseline_signed(install)
    }
}

/// [`resolve_baseline`] for a keyed build: a generation boots only when its sidecar verifies.
///
/// **TWO KEYS OWN THE BASELINE, NOT ONE.** The first generation is seeded by the installer and signed
/// with the launcher key; every generation after it is promoted from `summrise-device.new.json`, which
/// the *release* key signed and the supervisor verified at staging time. Checking only the launcher key
/// here (as the first version of this function did) made a committed update unbootable: at the next
/// supervisor start the new generation was called untrusted, the old one was restored over it, and a
/// second update in the same supervisor lifetime left both generations release-signed and nothing
/// bootable at all. Either key is this build's; a sidecar signed by neither is not.
pub fn resolve_baseline_signed(install: &Path) -> BaselineChoice {
    resolve_baseline_signed_with(install, &[LAUNCHER_PUBKEY_HEX, RELEASE_PUBKEY_HEX])
}

/// [`resolve_baseline_signed`] against an explicit list of keys — the test seam, and the form a build
/// with only one key embedded still works through.
pub fn resolve_baseline_signed_with(install: &Path, pubkeys_hex: &[&str]) -> BaselineChoice {
    let inspect = |generation: Generation| -> Result<Baseline, String> {
        let baseline = inspect_generation(install, generation)?;
        verify_any_signature(&baseline.sidecar, pubkeys_hex).map_err(|error| {
            format!(
                "the {} baseline is not signed by this build: {error}",
                generation.label()
            )
        })?;
        Ok(baseline)
    };
    match inspect(Generation::Current) {
        Ok(baseline) => BaselineChoice::Current(baseline),
        Err(current_error) => match inspect(Generation::Previous) {
            Ok(baseline) => BaselineChoice::Previous(baseline),
            Err(previous_error) => BaselineChoice::NeedsHuman(format!(
                "no baseline generation can be trusted: current: {current_error}; previous: {previous_error}"
            )),
        },
    }
}

/// The hash-consistent answer, for a build with no launcher key to check against.
fn resolve_baseline_hash_only(install: &Path) -> BaselineChoice {
    match inspect_generation(install, Generation::Current) {
        Ok(baseline) => BaselineChoice::Current(baseline),
        Err(current_error) => match inspect_generation(install, Generation::Previous) {
            Ok(baseline) => BaselineChoice::Previous(baseline),
            Err(previous_error) => BaselineChoice::NeedsHuman(format!(
                "no baseline generation can be trusted: current: {current_error}; previous: {previous_error}"
            )),
        },
    }
}

impl BaselineChoice {
    /// The pair to boot, when one exists.
    pub fn baseline(&self) -> Option<&Baseline> {
        match self {
            BaselineChoice::Current(baseline) | BaselineChoice::Previous(baseline) => {
                Some(baseline)
            }
            BaselineChoice::NeedsHuman(_) => None,
        }
    }
}

/// What [`ensure_baseline`] did before the installer registered the service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaselineInstall {
    /// `baseline/current` was already a complete, hash-consistent pair; nothing was written.
    Kept,
    /// The pre-generation flat pair was copied into `baseline/current`.
    Migrated,
    /// `baseline/current` was seeded from the install's app binary and `--seed-sidecar`.
    Seeded,
    /// `baseline/current` was replaced by the installer's pair; the old generation is `previous`.
    Replaced,
}

/// Whether any of `pubkeys_hex` signed `sidecar`, reporting the last refusal when none did.
fn verify_any_signature(sidecar: &Sidecar, pubkeys_hex: &[&str]) -> Result<(), String> {
    let mut last = String::from("no key is embedded in this build");
    for key in pubkeys_hex {
        if *key == PLACEHOLDER_PUBKEY_HEX {
            continue;
        }
        match verify_seed_signature(sidecar, key) {
            Ok(()) => return Ok(()),
            Err(error) => last = error,
        }
    }
    Err(last)
}

/// The one place the first-generation signature is checked, named for the failure it prevents (C1).
fn verify_seed_signature(sidecar: &Sidecar, pubkey_hex: &str) -> Result<(), String> {
    let pubkey =
        parse_pubkey(pubkey_hex).map_err(|error| format!("refusing the seed baseline: {error}"))?;
    verify_signature(sidecar, &pubkey).map_err(|error| {
        // C1: one key owns the first generation. Naming the key (and the build-time variable that
        // carries it) is what turns "signature does not verify" from a key-regeneration session
        // into "you signed the seed with the release key".
        format!(
            "refusing the seed baseline: the seed sidecar must be signed with the launcher key \
             (SUMMRISE_LAUNCHER_PUBKEY), not the release key: {error}"
        )
    })
}

/// Make sure `baseline/current` is a complete pair **before** the service is registered.
///
/// The installer is the last moment a device can get a bootable generation from one process; the
/// supervisor cannot build one from nothing. Three cases, in order (NEW-1):
///
/// 1. `current` already verifies → leave it alone (`Kept`), including its signature, which the
///    supervisor does not re-check;
/// 2. the pre-generation flat pair `<install>/baseline/summrise-device.exe` +
///    `summrise-device.json` exists → migrate it into `current`, renaming the sidecar to
///    `baseline.json`; the sha256 gates the copy, so a flat pair whose bytes and sidecar disagree is
///    refused rather than promoted;
/// 3. otherwise seed from the app binary the installer downloaded plus the sidecar it shipped, passed
///    as `--seed-sidecar`: the sidecar's **signature** and the binary's sha256 must both verify with
///    the embedded launcher key.
///
/// No fourth case exists on purpose: if none of the three can produce a pair, the install fails with
/// a non-zero exit and nothing is registered, because a service that starts into `NeedsHuman` is an
/// install that lied about succeeding.
pub fn ensure_baseline(
    install: &Path,
    seed_sidecar: Option<&Path>,
    pubkey_hex: &str,
    replace: bool,
) -> Result<BaselineInstall, String> {
    // AN INSTALLER UPGRADE REPLACES THE GENERATION. The default keeps a proven current generation so
    // a re-run cannot clobber a rollback chain; `replace` is the installer saying "the tree I just
    // wrote is the version this machine should boot". The old generation rotates to `previous`, so
    // the swap's rollback still has somewhere to go.
    if replace {
        let Some(seed_sidecar) = seed_sidecar else {
            return Err(
                "--replace-baseline needs --seed-sidecar: the new generation must be signed"
                    .to_string(),
            );
        };
        let sidecar = load_sidecar(seed_sidecar)
            .map_err(|error| format!("cannot read the seed sidecar: {error}"))?;
        verify_seed_signature(&sidecar, pubkey_hex)?;
        let app = app_exe(install);
        if !app.is_file() {
            return Err(format!(
                "{} is missing; the installer must place the app before it replaces the baseline",
                app.display()
            ));
        }
        // A fresh install is a seed, not a replacement: saying "replaced" with nothing to rotate
        // would be the kind of log line that sends an operator looking for a previous generation
        // that never existed.
        let had_current = inspect_generation(install, Generation::Current).is_ok();
        if had_current {
            let previous = generation_dir(install, PREVIOUS_DIR);
            let _ = fs::remove_dir_all(&previous);
            let current = generation_dir(install, CURRENT_DIR);
            fs::rename(&current, &previous).map_err(|error| {
                format!(
                    "cannot rotate {} to {}: {error}",
                    current.display(),
                    previous.display()
                )
            })?;
        }
        install_generation_pair(install, &app, &sidecar)
            .map_err(|error| format!("cannot replace the baseline: {error}"))?;
        return Ok(if had_current {
            BaselineInstall::Replaced
        } else {
            BaselineInstall::Seeded
        });
    }

    if inspect_generation(install, Generation::Current).is_ok() {
        return Ok(BaselineInstall::Kept);
    }

    let legacy_exe = flat_exe(install);
    let legacy_sidecar = flat_sidecar(install);
    if legacy_exe.is_file() && legacy_sidecar.is_file() {
        let sidecar = load_sidecar(&legacy_sidecar)
            .map_err(|error| format!("cannot migrate the pre-generation baseline: {error}"))?;
        // A migration is a seed by another name: the same key must have signed it, or the first
        // generation would be whichever sidecar happened to sit beside the install.
        verify_seed_signature(&sidecar, pubkey_hex)
            .map_err(|error| format!("cannot migrate the pre-generation baseline: {error}"))?;
        install_generation_pair(install, &legacy_exe, &sidecar)
            .map_err(|error| format!("cannot migrate the pre-generation baseline: {error}"))?;
        return Ok(BaselineInstall::Migrated);
    }

    let Some(seed_sidecar) = seed_sidecar else {
        return Err(format!(
            "{} has no usable baseline generation and no pre-generation pair; the installer must \
             pass --seed-sidecar <path> so a fresh install can seed baseline/current",
            install.display()
        ));
    };
    let sidecar = load_sidecar(seed_sidecar)
        .map_err(|error| format!("cannot read the seed sidecar: {error}"))?;
    verify_seed_signature(&sidecar, pubkey_hex)?;
    let app = app_exe(install);
    if !app.is_file() {
        return Err(format!(
            "cannot seed the baseline: the app binary {} the sidecar describes is missing",
            app.display()
        ));
    }
    install_generation_pair(install, &app, &sidecar)
        .map_err(|error| format!("cannot seed the baseline: {error}"))?;
    Ok(BaselineInstall::Seeded)
}

/// Write `source_exe` and `sidecar` into `baseline/current`, proving the bytes before and after the
/// copy. The sidecar goes first and both files are synced and the directory flushed, so a kill can
/// only leave a pair whose hash disagrees — which `resolve_baseline` reports as unusable rather than
/// booting.
fn install_generation_pair(
    install: &Path,
    source_exe: &Path,
    sidecar: &Sidecar,
) -> Result<(), String> {
    install_generation_pair_in(install, source_exe, sidecar, CURRENT_DIR)
}

/// Write one complete generation pair under `dir_name` of the baseline directory, verifying both the
/// source and the copy against the sidecar's digest.
fn install_generation_pair_in(
    install: &Path,
    source_exe: &Path,
    sidecar: &Sidecar,
    dir_name: &str,
) -> Result<(), String> {
    match sha256_file(source_exe) {
        Ok(hash) if hash.eq_ignore_ascii_case(&sidecar.sha256) => {}
        Ok(hash) => {
            return Err(format!(
                "{} hashes to {hash}, not the sidecar's {}",
                source_exe.display(),
                sidecar.sha256
            ))
        }
        Err(error) => return Err(format!("cannot hash {}: {error}", source_exe.display())),
    }

    let current = generation_dir(install, dir_name);
    fs::create_dir_all(&current)
        .map_err(|error| format!("cannot create {}: {error}", current.display()))?;
    save_sidecar(&generation_sidecar(install, dir_name), sidecar)
        .map_err(|error| format!("cannot write the sidecar of {}: {error}", current.display()))?;
    let current_exe = generation_exe(install, dir_name);
    copy_file_synced(source_exe, &current_exe)
        .map_err(|error| format!("cannot write {}: {error}", current_exe.display()))?;
    match sha256_file(&current_exe) {
        Ok(hash) if hash.eq_ignore_ascii_case(&sidecar.sha256) => {}
        Ok(hash) => {
            return Err(format!(
                "the written current binary hashes to {hash}, not the sidecar's {}",
                sidecar.sha256
            ))
        }
        Err(error) => {
            return Err(format!(
                "cannot hash the written current binary {}: {error}",
                current_exe.display()
            ))
        }
    }
    sync_dir(&current).map_err(|error| format!("cannot flush {}: {error}", current.display()))?;
    Ok(())
}

/// Delete a leftover `pending` generation. Called at supervisor start, before anything reads the
/// baseline: a generation that was never proven by a probe must not be promoted by a restart.
pub fn discard_pending(install: &Path) -> io::Result<()> {
    let pending = generation_dir(install, PENDING_DIR);
    match fs::remove_dir_all(&pending) {
        Ok(()) => {
            if let Some(parent) = pending.parent() {
                sync_dir(parent)?;
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Write the staged pair into `baseline/pending` and prove the copy before the app is touched.
///
/// The sidecar goes first and both files are `sync_all`ed before their renames; the generation is
/// only a pair when both are present and agree. This is the adoption half of F2.
pub fn stage_pending(install: &Path, source_exe: &Path, sidecar: &Sidecar) -> Result<(), String> {
    let baseline = baseline_dir(install);
    fs::create_dir_all(&baseline)
        .map_err(|error| format!("cannot create {}: {error}", baseline.display()))?;
    let pending = generation_dir(install, PENDING_DIR);
    if pending.exists() {
        fs::remove_dir_all(&pending)
            .map_err(|error| format!("cannot clear {}: {error}", pending.display()))?;
    }
    fs::create_dir_all(&pending)
        .map_err(|error| format!("cannot create {}: {error}", pending.display()))?;

    save_sidecar(&generation_sidecar(install, PENDING_DIR), sidecar)
        .map_err(|error| format!("cannot write the pending sidecar: {error}"))?;
    let pending_exe = generation_exe(install, PENDING_DIR);
    copy_file_synced(source_exe, &pending_exe)
        .map_err(|error| format!("cannot stage {}: {error}", pending_exe.display()))?;

    match sha256_file(&pending_exe) {
        Ok(hash) if hash.eq_ignore_ascii_case(&sidecar.sha256) => {}
        Ok(hash) => {
            let _ = fs::remove_dir_all(&pending);
            return Err(format!(
                "the staged copy hashes to {hash}, not the signed {}",
                sidecar.sha256
            ));
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&pending);
            return Err(format!(
                "cannot hash the staged copy {}: {error}",
                pending_exe.display()
            ));
        }
    }
    sync_dir(&pending).map_err(|error| format!("cannot flush {}: {error}", pending.display()))?;
    sync_dir(&baseline).map_err(|error| format!("cannot flush {}: {error}", baseline.display()))?;
    Ok(())
}

/// Copy the proven `pending` binary over the app binary.
///
/// The caller has stopped the child. The pair is re-hashed first: staging happened before the stop,
/// so this is the time-of-check/time-of-use check for the swap itself.
pub fn install_pending_over_app(install: &Path, sidecar: &Sidecar) -> Result<(), String> {
    let pending_exe = generation_exe(install, PENDING_DIR);
    match sha256_file(&pending_exe) {
        Ok(hash) if hash.eq_ignore_ascii_case(&sidecar.sha256) => {}
        Ok(hash) => {
            return Err(format!(
                "the pending binary changed after it was staged ({hash} != {})",
                sidecar.sha256
            ))
        }
        Err(error) => {
            return Err(format!(
                "cannot hash the pending binary {}: {error}",
                pending_exe.display()
            ))
        }
    }
    let app = app_exe(install);
    copy_with_retry(&pending_exe, &app)
        .map_err(|error| format!("cannot install {}: {error}", app.display()))?;
    match sha256_file(&app) {
        Ok(hash) if hash.eq_ignore_ascii_case(&sidecar.sha256) => Ok(()),
        Ok(hash) => Err(format!(
            "the installed app hashes to {hash}, not the signed {}",
            sidecar.sha256
        )),
        Err(error) => Err(format!("cannot hash {}: {error}", app.display())),
    }
}

/// Commit a successfully probed `pending` generation: delete `previous`, rotate
/// `current` → `previous`, then `pending` → `current`.
///
/// The order is the transaction. A crash after the first rename leaves `previous` bootable and a
/// `pending` that the next start deletes; a crash before it leaves `current` untouched. If the
/// second rename fails, the first is reversed so the bootable pair stays in `current`.
pub fn commit_pending(install: &Path) -> Result<(), String> {
    let baseline = baseline_dir(install);
    let current = generation_dir(install, CURRENT_DIR);
    let previous = generation_dir(install, PREVIOUS_DIR);
    let pending = generation_dir(install, PENDING_DIR);

    if !pending.is_dir() {
        return Err(format!(
            "cannot commit: the pending generation {} is missing",
            pending.display()
        ));
    }
    if previous.exists() {
        fs::remove_dir_all(&previous)
            .map_err(|error| format!("cannot remove {}: {error}", previous.display()))?;
        sync_dir(&baseline)
            .map_err(|error| format!("cannot flush {}: {error}", baseline.display()))?;
    }
    if current.exists() {
        rename_with_retry(&current, &previous).map_err(|error| {
            format!(
                "cannot rotate {} to {}: {error}",
                current.display(),
                previous.display()
            )
        })?;
        sync_dir(&baseline)
            .map_err(|error| format!("cannot flush {}: {error}", baseline.display()))?;
    }
    if let Err(error) = rename_with_retry(&pending, &current) {
        if !current.exists() && previous.exists() {
            let _ = rename_with_retry(&previous, &current);
            let _ = sync_dir(&baseline);
        }
        return Err(format!(
            "cannot promote {} to {}: {error}",
            pending.display(),
            current.display()
        ));
    }
    sync_dir(&baseline).map_err(|error| format!("cannot flush {}: {error}", baseline.display()))?;
    sync_dir(&current).map_err(|error| format!("cannot flush {}: {error}", current.display()))?;
    Ok(())
}

/// Copy `previous` back over `current` as a whole pair.
///
/// This is the fallback repair and the rollback primitive: the two files move together, never one
/// of them, so a crash can only leave a pair whose hash disagrees (and thus resolves `previous`
/// again) or the copied pair.
pub fn restore_previous_into_current(install: &Path) -> Result<Baseline, String> {
    let previous = match inspect_generation(install, Generation::Previous) {
        Ok(previous) => previous,
        Err(error) => return Err(format!("cannot restore from previous: {error}")),
    };
    let current = generation_dir(install, CURRENT_DIR);
    fs::create_dir_all(&current)
        .map_err(|error| format!("cannot create {}: {error}", current.display()))?;
    save_sidecar(&generation_sidecar(install, CURRENT_DIR), &previous.sidecar)
        .map_err(|error| format!("cannot write the current sidecar: {error}"))?;
    copy_file_synced(&previous.exe, &generation_exe(install, CURRENT_DIR))
        .map_err(|error| format!("cannot write the current binary: {error}"))?;
    sync_dir(&current).map_err(|error| format!("cannot flush {}: {error}", current.display()))?;
    inspect_generation(install, Generation::Current)
        .map_err(|error| format!("the restored current generation is inconsistent: {error}"))
}

/// Resolve and install the verified app bytes, repairing from the latest trusted generation.
///
/// Returns the baseline the app now matches. Used by a rollback after a failed probe and by the
/// supervisor's start when `current` itself is unusable. It writes **no** `repair.log` record: a swap
/// rollback's outcome is the swap result, and the startup caller decides separately whether a repair
/// happened (see [`verify_or_restore_at_startup`]).
pub fn restore_current_app(install: &Path) -> Result<Baseline, String> {
    let baseline = match resolve_baseline(install) {
        BaselineChoice::Current(baseline) => baseline,
        BaselineChoice::Previous(_) => restore_previous_into_current(install)?,
        BaselineChoice::NeedsHuman(reason) => return Err(reason),
    };
    match verify_or_restore(install, &baseline) {
        Restore::Ok | Restore::Restored => Ok(baseline),
        Restore::NeedsHuman(reason) => Err(reason),
    }
}

/// Boot the retired generation and record it: the startup half of "current was unusable".
///
/// Returns the generation that was booted after rebuilding `current` from it. The record is written
/// **before** the caller starts the child, so a device that comes back from the previous generation
/// always leaves the evidence behind; a record that cannot be written is loud but does not turn a
/// repaired device into a broken one.
pub fn restore_from_previous_at_startup(install: &Path) -> Result<Baseline, String> {
    let baseline = restore_current_app(install)?;
    if let Err(error) = append_repair_record(install, "previous", &baseline.sidecar.sha256) {
        eprintln!(
            "launch: warning: restored the app from the previous generation but cannot append {}: {error}",
            repair_log(install).display()
        );
    }
    Ok(baseline)
}

/// Verify the app against a baseline generation; restore it from that generation when it is missing,
/// empty or corrupt. Never returns before the app either matches the expected hash or a human is
/// named.
///
/// This function is deliberately silent: it is also the rollback's restore path, and a rollback
/// during a swap is **not** a self-repair — its outcome is the swap result ("rolled back from X"),
/// which the app records. Writing `repair.log` here would let the repair verdict overwrite that more
/// specific outcome on the next start (round 2). Only [`verify_or_restore_at_startup`] records.
pub fn verify_or_restore(install: &Path, baseline: &Baseline) -> Restore {
    let app = app_exe(install);
    let defect = match app_defect(&app, &baseline.sidecar.sha256) {
        None => return Restore::Ok,
        Some(defect) => defect,
    };

    if !baseline.exe.is_file() {
        return Restore::NeedsHuman(format!(
            "the {} baseline binary is missing ({}) and the app is {defect}",
            baseline.generation.label(),
            baseline.exe.display()
        ));
    }
    match sha256_file(&baseline.exe) {
        Ok(hash) if hash.eq_ignore_ascii_case(&baseline.sidecar.sha256) => {}
        Ok(hash) => {
            return Restore::NeedsHuman(format!(
                "the {} baseline hash {hash} does not match the sidecar {} (app: {defect})",
                baseline.generation.label(),
                baseline.sidecar.sha256
            ))
        }
        Err(error) => {
            return Restore::NeedsHuman(format!(
                "the {} baseline {} cannot be read: {error}",
                baseline.generation.label(),
                baseline.exe.display()
            ))
        }
    }

    let mut last = String::from("no attempt was made");
    for attempt in 1..=RESTORE_ATTEMPTS {
        match copy_file_synced(&baseline.exe, &app) {
            Ok(()) => match sha256_file(&app) {
                Ok(hash) if hash.eq_ignore_ascii_case(&baseline.sidecar.sha256) => {
                    return Restore::Restored;
                }
                Ok(hash) => last = format!("attempt {attempt}: copied hash {hash} did not match"),
                Err(error) => last = format!("attempt {attempt}: re-hash failed: {error}"),
            },
            Err(error) => last = format!("attempt {attempt}: copy failed: {error}"),
        }
    }
    Restore::NeedsHuman(format!(
        "could not restore {} from {} after {RESTORE_ATTEMPTS} attempts ({last})",
        app.display(),
        baseline.exe.display()
    ))
}

/// The **startup** half of the repair path: verify or restore, and record a real self-repair.
///
/// This is the only caller that writes `repair.log`. A swap rollback restores through
/// [`verify_or_restore`] directly (via [`restore_current_app`]) and must not write a repair record:
/// it already has a more specific outcome in `swap.result.json`, and the app ingests both at the next
/// start. A log that cannot be written must not turn a repaired device into a broken one, but it is
/// loud because the record is the point.
pub fn verify_or_restore_at_startup(install: &Path, baseline: &Baseline) -> Restore {
    match verify_or_restore(install, baseline) {
        Restore::Restored => {
            if let Err(error) = append_repair_log(install, &baseline.sidecar.sha256) {
                eprintln!(
                    "launch: warning: restored the app but cannot append {}: {error}",
                    repair_log(install).display()
                );
            }
            Restore::Restored
        }
        other => other,
    }
}

/// Append one repair record to `<install>/repair.log`.
///
/// The line is exactly `{"ts_ms":…,"kind":"repaired","from":"baseline","sha256":…}` — the shape the
/// app's reader consumes. `sync_all` makes the evidence durable before the app starts and quotes it:
/// a record that dies in the page cache is a record the canary cannot show.
pub fn append_repair_log(install: &Path, sha256: &str) -> io::Result<()> {
    append_repair_record(install, "baseline", sha256)
}

/// Append one repair record that names **which generation** the bytes came from.
///
/// `baseline` is the normal repair: the app copy no longer matched the verified current generation
/// and was rewritten from it. `previous` is the boot-time substitution: `current` itself was
/// unusable, so the supervisor booted the retired generation and rebuilt `current` from it. Before
/// this existed the second case was silent — the device came back, and nothing on disk said why or
/// from which build (`2026-10-22`: a deleted `current\summrise-device.exe` was repaired to the
/// previous generation and left no record at all).
pub fn append_repair_record(install: &Path, from: &'static str, sha256: &str) -> io::Result<()> {
    #[derive(Serialize)]
    struct RepairRecord<'a> {
        ts_ms: u64,
        kind: &'static str,
        from: &'static str,
        sha256: &'a str,
    }

    let record = RepairRecord {
        ts_ms: now_ms(),
        kind: "repaired",
        from,
        sha256,
    };
    let mut line = serde_json::to_string(&record).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("cannot serialize the repair record: {error}"),
        )
    })?;
    line.push('\n');
    let path = repair_log(install);
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(line.as_bytes())?;
    file.sync_all()
}

/// The state document the serving app writes under `<data>`.
pub const STATE_FILE: &str = "state.json";

/// Read the rollback memory from `<data>/state.json`.
///
/// The launcher's operator-only `--apply` path is the last resort when no supervisor is serving, so
/// it must see the device's memory before it installs an artifact this device already rejected. A
/// document it cannot read is reported; the caller decides (review I3: the operator is not blocked,
/// but silence is not allowed either).
pub fn read_rolled_back(data: &Path) -> Result<Vec<summrise_core::RolledBackArtifact>, String> {
    let path = data.join(STATE_FILE);
    let bytes = fs::read(&path).map_err(|error| {
        format!(
            "cannot read the rollback memory {}: {error}",
            path.display()
        )
    })?;
    let document = String::from_utf8(bytes).map_err(|_| {
        format!(
            "the state document {} is not valid UTF-8, so its rollback memory cannot be read",
            path.display()
        )
    })?;
    let state = summrise_core::State::parse(&document).map_err(|error| {
        format!(
            "cannot read the rollback memory {}: {error}",
            path.display()
        )
    })?;
    Ok(state.rolled_back)
}

/// The remembered rollback of `sha256` at `<data>/state.json`, or `None` when it is not there.
pub fn remembered_rollback(
    data: &Path,
    sha256: &str,
) -> Result<Option<summrise_core::RolledBackArtifact>, String> {
    Ok(read_rolled_back(data)?
        .into_iter()
        .find(|artifact| artifact.sha256.eq_ignore_ascii_case(sha256)))
}

/// Append the operator's `--apply` exception to `<data>/events.jsonl`.
///
/// `--apply` does not block on a remembered digest — the operator is the last resort — so the audit
/// line is how the exception survives the command that made it.
pub fn append_operator_rollback_event(
    data: &Path,
    version: &str,
    sha256: &str,
    rolled_back_at_ms: u64,
) -> io::Result<()> {
    let line = serde_json::json!({
        "ts_ms": now_ms(),
        "event": "operator_apply_rolled_back",
        "version": version,
        "sha256": sha256,
        "rolled_back_at_ms": rolled_back_at_ms,
    });
    let mut text = line.to_string();
    text.push('\n');
    fs::create_dir_all(data)?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data.join("events.jsonl"))?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

fn app_defect(app: &Path, expected_sha256: &str) -> Option<String> {
    let metadata = match fs::metadata(app) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Some("missing".to_string())
        }
        Err(error) => return Some(format!("unreadable: {error}")),
    };
    if !metadata.is_file() {
        return Some("not a regular file".to_string());
    }
    if metadata.len() == 0 {
        return Some("zero-length".to_string());
    }
    match sha256_file(app) {
        Ok(hash) if hash.eq_ignore_ascii_case(expected_sha256) => None,
        Ok(hash) => Some(format!("sha256 {hash} does not match {expected_sha256}")),
        Err(error) => Some(format!("unreadable: {error}")),
    }
}

/// Decide whether to start, wait and retry, or give up.
///
/// `child_exited` is false only before any child has been attempted: that is the first start. Once a
/// child has exited (or a spawn failed), `attempts` counts the restarts already performed in the
/// current consecutive-failure sequence, so `attempts < 3` yields the next backoff and
/// `attempts >= 3` gives up — at most three restarts. [`RestartBudget`] owns the sequence and the
/// 60-second reset.
pub fn next_action(attempts: u8, child_exited: bool) -> Action {
    if !child_exited {
        return Action::Start;
    }
    RESTART_BACKOFF_MS
        .get(attempts as usize)
        .map(|backoff| Action::RetryAfterMs(*backoff))
        .unwrap_or(Action::GiveUp)
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// Versioned directories and the current pointer (2026-11-05 design)
// ─────────────────────────────────────────────────────────────────────────────────────────────────

/// The longest version-directory name this launcher will write into a pointer.
pub const MAX_VERSION_NAME: usize = 128;

/// The most a pointer file may hold. A pointer is one version-directory name.
pub const MAX_POINTER_BYTES: u64 = 4 * 1024;

/// Consecutive failed starts of the current version before the supervisor rolls the pointer back.
///
/// One failure is an accident; two in a row is a build that cannot start. The rule is deliberately
/// small: the rollback is a pointer write, so the cost of being wrong is one more restart.
pub const ROLLBACK_AFTER_FAILED_STARTS: u8 = 2;

/// What a pointer file says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pointer {
    /// No file: a pre-versioned install, or no previous version recorded. Never an error.
    Missing,
    /// A well-formed version-directory name. It may still name a directory that is not there.
    Version(String),
    /// Present but unusable. It never silently becomes a name, and it never silently becomes
    /// "missing": a corrupt pointer is a fact an operator has to see.
    Invalid(String),
}

/// One version directory, already checked for hash consistency.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    /// The directory name under `<install>/versions`; this is what `current.txt` holds.
    pub name: String,
    /// `<install>/versions/<name>`.
    pub dir: PathBuf,
    /// `<install>/versions/<name>/summrise-device.exe`.
    pub exe: PathBuf,
    /// `<install>/versions/<name>/summrise-device.json`.
    pub sidecar_path: PathBuf,
    /// The signed release facts from that sidecar.
    pub sidecar: Sidecar,
}

/// Where the supervisor starts the app from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppLocation {
    /// A version directory named by `current.txt`.
    Version(Version),
    /// A pre-versioned install: the flat app, repaired from the baseline. The sidecar is the
    /// baseline's; `exe` is `<install>/summrise-device.exe`, not the baseline copy.
    Legacy { exe: PathBuf, baseline: Baseline },
}

impl AppLocation {
    /// The binary to spawn.
    pub fn exe(&self) -> &Path {
        match self {
            AppLocation::Version(version) => &version.exe,
            AppLocation::Legacy { exe, .. } => exe,
        }
    }

    /// The signed facts the child is started with.
    pub fn sidecar(&self) -> &Sidecar {
        match self {
            AppLocation::Version(version) => &version.sidecar,
            AppLocation::Legacy { baseline, .. } => &baseline.sidecar,
        }
    }

    /// Whether this is the pre-versioned flat layout.
    pub fn is_legacy(&self) -> bool {
        matches!(self, AppLocation::Legacy { .. })
    }
}

impl Config {
    /// Where this install's app lives, resolved through `current.txt` (the supervisor's question).
    pub fn app(&self) -> Result<AppLocation, String> {
        locate_app(&self.install)
    }
}

/// Check one path component is a version-directory name this launcher may write.
///
/// The pointer is attacker-writable by anything that can write the install directory, and it is
/// interpolated into a path the supervisor runs: a name with a separator, `..`, a drive letter, or a
/// reserved Windows character is refused rather than normalized. A leading dot is reserved for the
/// launcher's own temporary files.
pub fn validate_version_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("the version directory name is empty".to_string());
    }
    if name.len() > MAX_VERSION_NAME {
        return Err(format!(
            "the version directory name is {} bytes; the limit is {MAX_VERSION_NAME}",
            name.len()
        ));
    }
    if name.starts_with('.') || name.ends_with('.') || name.ends_with(' ') {
        return Err(format!(
            "the version directory name {name:?} must not begin with a dot or end with a dot or a \
             space"
        ));
    }
    for character in name.chars() {
        if character.is_control()
            || matches!(
                character,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            )
        {
            return Err(format!(
                "the version directory name {name:?} contains {character:?}, which is not a file \
                 name character"
            ));
        }
    }
    Ok(())
}

/// Parse one pointer line: either a bare version name (`0.3.274`) or the layout's spelling
/// (`versions/0.3.274`, `versions\0.3.274`). Only the last path component is ever used.
fn parse_pointer(bytes: &[u8], path: &Path) -> Pointer {
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            return Pointer::Invalid(format!("{} is not UTF-8: {error}", path.display()));
        }
    };
    let line = text.trim();
    if line.is_empty() {
        return Pointer::Invalid(format!("{} is empty", path.display()));
    }
    if line.contains('\n') || line.contains('\r') {
        return Pointer::Invalid(format!(
            "{} has more than one line: {line:?}",
            path.display()
        ));
    }
    let mut name = line;
    if let Some(head) = line.get(..8) {
        if head.eq_ignore_ascii_case(VERSIONS_DIR) {
            let rest = &line[8..];
            name = rest
                .strip_prefix('/')
                .or_else(|| rest.strip_prefix('\\'))
                .unwrap_or(rest);
        }
    }
    match validate_version_name(name) {
        Ok(()) => Pointer::Version(name.to_string()),
        Err(error) => Pointer::Invalid(format!("{}: {error}", path.display())),
    }
}

/// Read and parse one pointer file.
fn read_pointer(path: &Path) -> Pointer {
    match read_capped(path, MAX_POINTER_BYTES) {
        CappedRead::Missing => Pointer::Missing,
        CappedRead::Failed(reason) => Pointer::Invalid(reason),
        CappedRead::Bytes(bytes) => parse_pointer(&bytes, path),
    }
}

/// `<install>/current.txt`, as read.
pub fn read_current_pointer(install: &Path) -> Pointer {
    read_pointer(&current_pointer(install))
}

/// `<install>/previous.txt`, as read.
pub fn read_previous_pointer(install: &Path) -> Pointer {
    read_pointer(&previous_pointer(install))
}

/// A pointer write that has reached its temporary file and not yet its rename.
///
/// Dropping it is a crash between the two: the temporary file is removed and the old pointer is
/// still the one every reader sees. [`PointerWrite::commit`] is the one atomic step.
#[derive(Debug)]
pub struct PointerWrite {
    tmp: Option<PathBuf>,
    path: PathBuf,
}

impl PointerWrite {
    /// Rename the staged temporary file over the pointer and flush the directory entry.
    pub fn commit(mut self) -> io::Result<()> {
        let Some(tmp) = self.tmp.take() else {
            return Ok(());
        };
        if let Err(error) = rename_with_retry(&tmp, &self.path) {
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
        if let Some(parent) = self.path.parent() {
            sync_dir(parent)?;
        }
        Ok(())
    }
}

impl Drop for PointerWrite {
    fn drop(&mut self) {
        if let Some(tmp) = self.tmp.take() {
            let _ = fs::remove_file(tmp);
        }
    }
}

/// Stage a pointer write: a random temporary sibling, the new line, `sync_all`.
///
/// Nothing about the pointer changes until [`PointerWrite::commit`] renames this file over it.
pub fn stage_pointer_write(path: &Path, name: &str) -> io::Result<PointerWrite> {
    validate_version_name(name)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let (tmp, mut file) = create_temp_sibling(path)?;
    let bytes = format!("{name}\n");
    if let Err(error) = file
        .write_all(bytes.as_bytes())
        .and_then(|()| file.sync_all())
    {
        drop(file);
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    drop(file);
    Ok(PointerWrite {
        tmp: Some(tmp),
        path: path.to_path_buf(),
    })
}

/// Write one pointer file atomically.
pub fn write_pointer(path: &Path, name: &str) -> io::Result<()> {
    stage_pointer_write(path, name)?.commit()
}

/// Write `<install>/current.txt`.
pub fn write_current_pointer(install: &Path, name: &str) -> io::Result<()> {
    write_pointer(&current_pointer(install), name)
}

/// Write `<install>/previous.txt`.
pub fn write_previous_pointer(install: &Path, name: &str) -> io::Result<()> {
    write_pointer(&previous_pointer(install), name)
}

/// Stage `<install>/current.txt` without making it visible.
pub fn stage_current_pointer(install: &Path, name: &str) -> io::Result<PointerWrite> {
    stage_pointer_write(&current_pointer(install), name)
}

/// Remove a pointer, treating "already gone" as success.
fn remove_pointer(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Say once per process that this install is still the pre-versioned flat layout.
fn note_legacy_install(install: &Path) {
    use std::sync::atomic::{AtomicBool, Ordering};

    static NOTED: AtomicBool = AtomicBool::new(false);
    if !NOTED.swap(true, Ordering::Relaxed) {
        eprintln!(
            "launch: no {} under {}; running the pre-versioned flat app (the installer migrates \
             this install to versions/)",
            CURRENT_POINTER,
            install.display()
        );
    }
}

/// Inspect one version directory: the pair present, the sidecar parses, the binary hashes to it,
/// and (in a keyed build) the sidecar is signed by one of the keys this build trusts.
///
/// The supervisor calls this at every start rather than trusting the pointer: the pointer chooses a
/// directory, and the directory is re-proven against its own sidecar before its bytes are executed.
pub fn inspect_version(install: &Path, name: &str) -> Result<Version, String> {
    validate_version_name(name).map_err(|error| format!("refusing version {name:?}: {error}"))?;
    let dir = version_dir(install, name);
    if !dir.is_dir() {
        return Err(format!(
            "the current pointer names version {name:?}, but its version directory {} is missing",
            dir.display()
        ));
    }
    // A pointer is a name, not an indirection: a version directory or binary that is a link could
    // redirect the supervisor to bytes no sidecar in this install describes.
    let dir_link = fs::symlink_metadata(&dir)
        .map_err(|error| format!("cannot inspect {}: {error}", dir.display()))?;
    if dir_link.file_type().is_symlink() {
        return Err(format!(
            "the version directory {} is a symbolic link; refusing to run through it",
            dir.display()
        ));
    }
    let sidecar_path = version_sidecar(install, name);
    let sidecar = load_sidecar(&sidecar_path).map_err(|error| {
        format!(
            "the sidecar of version {name:?} at {} is unusable: {error}",
            sidecar_path.display()
        )
    })?;
    if !is_sha256_hex(&sidecar.sha256) {
        return Err(format!(
            "the sidecar of version {name:?} has a sha256 that is not 64 hex characters: {:?}",
            sidecar.sha256
        ));
    }
    let exe = version_exe(install, name);
    let metadata = match fs::symlink_metadata(&exe) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(format!(
                "the binary of version {name:?} is missing ({})",
                exe.display()
            ));
        }
        Err(error) => {
            return Err(format!(
                "the binary of version {name:?} ({}) is unreadable: {error}",
                exe.display()
            ));
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "the binary of version {name:?} ({}) is a symbolic link; refusing to run it",
            exe.display()
        ));
    }
    if !metadata.is_file() {
        return Err(format!(
            "the binary of version {name:?} ({}) is not a regular file",
            exe.display()
        ));
    }
    if metadata.len() == 0 {
        return Err(format!(
            "the binary of version {name:?} ({}) is zero-length",
            exe.display()
        ));
    }
    match sha256_file(&exe) {
        Ok(hash) if hash.eq_ignore_ascii_case(&sidecar.sha256) => {}
        Ok(hash) => {
            return Err(format!(
                "the binary of version {name:?} ({}) hashes to {hash}, not the sidecar's {}",
                exe.display(),
                sidecar.sha256
            ));
        }
        Err(error) => {
            return Err(format!(
                "the binary of version {name:?} ({}) cannot be hashed: {error}",
                exe.display()
            ));
        }
    }
    if !(LAUNCHER_PUBKEY_HEX == PLACEHOLDER_PUBKEY_HEX
        && RELEASE_PUBKEY_HEX == PLACEHOLDER_PUBKEY_HEX)
    {
        verify_any_signature(&sidecar, &[LAUNCHER_PUBKEY_HEX, RELEASE_PUBKEY_HEX]).map_err(
            |error| format!("the sidecar of version {name:?} is not signed by this build: {error}"),
        )?;
    }
    Ok(Version {
        name: name.to_string(),
        dir,
        exe,
        sidecar_path,
        sidecar,
    })
}

/// Resolve the app to start: `current.txt` first, the pre-versioned flat app when it is absent.
///
/// A pointer that is present but names a missing or inconsistent directory is an **error**, not a
/// silent fallback: falling back to an older flat binary would serve a build nobody chose. The
/// supervisor turns that error into a failed start, and two of those roll the pointer back to
/// `previous.txt`.
pub fn locate_app(install: &Path) -> Result<AppLocation, String> {
    match read_current_pointer(install) {
        Pointer::Version(name) => inspect_version(install, &name).map(AppLocation::Version),
        Pointer::Missing => {
            note_legacy_install(install);
            match resolve_baseline(install) {
                BaselineChoice::Current(baseline) | BaselineChoice::Previous(baseline) => {
                    Ok(AppLocation::Legacy {
                        exe: app_exe(install),
                        baseline,
                    })
                }
                BaselineChoice::NeedsHuman(reason) => Err(reason),
            }
        }
        Pointer::Invalid(reason) => Err(format!(
            "{} is unusable: {reason}",
            current_pointer(install).display()
        )),
    }
}

/// What one successful switch produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledVersion {
    /// The version directory now named by `current.txt`.
    pub version: Version,
    /// The version directory that was current before the switch, if one was recorded.
    pub previous: Option<String>,
}

/// Proves a candidate version can serve before the pointer names it.
///
/// The shipped implementation (in the Windows half) starts the candidate on a scratch port with a
/// copy of the data directory, requires two matching `/api/health` answers and kills it. Tests and
/// non-Windows builds supply their own; the install transaction only needs the verdict.
pub trait VersionProbe {
    /// How a candidate proves it can serve. Returning `Ok` is the permission to make it current.
    fn probe(&self, config: &Config, version: &Version) -> Result<(), String>;
}

/// Put a verified pair into `<install>/versions/<name>` without overwriting a version that is
/// already there.
///
/// A version directory is immutable once it exists: the same bytes may be reused (an installer
/// re-run), but different bytes are refused. Overwriting is exactly the failure this layout exists
/// to remove — a running image must never be replaced under its process.
fn stage_version(install: &Path, name: &str, pair: &StagedPair) -> Result<Version, String> {
    validate_version_name(name)?;
    let dir = version_dir(install, name);
    let exe = version_exe(install, name);
    let sidecar_path = version_sidecar(install, name);

    if dir.exists() {
        let metadata = fs::symlink_metadata(&dir)
            .map_err(|error| format!("cannot inspect {}: {error}", dir.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "the version directory {} is a symbolic link; refusing to write through it",
                dir.display()
            ));
        }
    }

    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|error| {
            format!(
                "cannot create the version directory {}: {error}",
                dir.display()
            )
        })?;
        copy_file_synced(&pair.exe, &exe).map_err(|error| {
            format!("cannot copy the candidate into {}: {error}", exe.display())
        })?;
        save_sidecar(&sidecar_path, &pair.sidecar).map_err(|error| {
            format!(
                "cannot write the version sidecar {}: {error}",
                sidecar_path.display()
            )
        })?;
        if let Some(parent) = dir.parent() {
            let _ = sync_dir(parent);
        }
    } else {
        if !dir.is_dir() {
            return Err(format!(
                "{} exists and is not a version directory",
                dir.display()
            ));
        }
        if exe.exists() {
            let existing = sha256_file(&exe).map_err(|error| {
                format!(
                    "cannot hash the installed version {}: {error}",
                    exe.display()
                )
            })?;
            if !existing.eq_ignore_ascii_case(&pair.sidecar.sha256) {
                return Err(format!(
                    "version {name:?} is already installed at {} with different bytes; refusing \
                     to overwrite a version directory that may be running",
                    exe.display()
                ));
            }
        } else {
            copy_file_synced(&pair.exe, &exe).map_err(|error| {
                format!("cannot copy the candidate into {}: {error}", exe.display())
            })?;
        }
        if sidecar_path.exists() {
            let on_disk = load_sidecar(&sidecar_path).map_err(|error| {
                format!("the existing sidecar of version {name:?} is unusable: {error}")
            })?;
            if on_disk.version != pair.sidecar.version
                || !on_disk.sha256.eq_ignore_ascii_case(&pair.sidecar.sha256)
            {
                return Err(format!(
                    "version {name:?} is already installed with a different sidecar at {}; \
                     refusing to rewrite it",
                    sidecar_path.display()
                ));
            }
        } else {
            save_sidecar(&sidecar_path, &pair.sidecar).map_err(|error| {
                format!(
                    "cannot write the version sidecar {}: {error}",
                    sidecar_path.display()
                )
            })?;
        }
    }

    // Probe and switch the bytes that are on disk, not the ones that were in memory when the pair
    // was verified. `inspect_version` re-hashes the directory we are about to name.
    inspect_version(install, name)
}

/// Verify, stage, and health-check a candidate; the pointer is not touched.
///
/// This is the half of the transaction that is safe while the old version runs: the candidate is
/// copied into its own directory and started on a scratch port, and the live service is not
/// stopped. [`commit_version`] is the one step that changes what boots.
pub fn prepare_version(
    config: &Config,
    name: &str,
    pair: &StagedPair,
    probe: &dyn VersionProbe,
) -> Result<Version, String> {
    let version = stage_version(&config.install, name, pair)?;
    probe.probe(config, &version).map_err(|error| {
        format!(
            "version {} failed its health check and was not made current: {error}",
            version.name
        )
    })?;
    Ok(version)
}

/// Point `current.txt` at a version that has already passed its probe.
///
/// The previous name is written first, atomically, so a crash between the two writes leaves the old
/// pointer in charge with the rollback target already recorded. `previous.txt` is only changed when
/// the current name changes; reinstalling the same version keeps the older rollback target.
pub fn commit_version(install: &Path, version: &Version) -> Result<InstalledVersion, String> {
    let on_disk = inspect_version(install, &version.name)?;
    if on_disk.sidecar.version != version.sidecar.version
        || !on_disk
            .sidecar
            .sha256
            .eq_ignore_ascii_case(&version.sidecar.sha256)
    {
        return Err(format!(
            "version {} changed between its health check and the switch; refusing to point at it",
            version.name
        ));
    }

    let previous = match read_current_pointer(install) {
        Pointer::Version(old) if old == version.name => match read_previous_pointer(install) {
            Pointer::Version(previous) => Some(previous),
            _ => None,
        },
        Pointer::Version(old) => {
            write_previous_pointer(install, &old).map_err(|error| {
                format!(
                    "cannot record {} before the switch: {error}",
                    previous_pointer(install).display()
                )
            })?;
            Some(old)
        }
        Pointer::Missing | Pointer::Invalid(_) => {
            remove_pointer(&previous_pointer(install)).map_err(|error| {
                format!(
                    "cannot clear the stale {} before the switch: {error}",
                    previous_pointer(install).display()
                )
            })?;
            None
        }
    };

    write_current_pointer(install, &version.name).map_err(|error| {
        format!(
            "cannot write {}: {error}",
            current_pointer(install).display()
        )
    })?;
    Ok(InstalledVersion {
        version: on_disk,
        previous,
    })
}

/// The whole switch: verify-and-stage-and-probe, then point `current.txt` at the candidate.
///
/// Any failure before the final rename leaves the pointer exactly as it was.
pub fn install_version(
    config: &Config,
    name: &str,
    pair: &StagedPair,
    probe: &dyn VersionProbe,
) -> Result<InstalledVersion, String> {
    let version = prepare_version(config, name, pair, probe)?;
    commit_version(&config.install, &version)
}

/// The whole switch when the process must follow the pointer: install, restart the owner, and
/// require the restarted process to report the version that was just made current.
///
/// [`install_version`] is pointer-only by design: it proves the candidate on a scratch port and
/// moves `current.txt`, but the process already serving keeps the version it booted. An install
/// that stops there leaves `current.txt` naming `name` while `/api/health` still names the old
/// version. This function adds the missing half through [`MigrationHost`] — the seam
/// `--migrate-layout` already uses for "restart, then require two matching health answers" — so
/// the Windows half supplies only the SCM/child effects.
///
/// The order is the contract:
///
/// 1. stage and probe the candidate; no pointer, no restart on failure;
/// 2. commit `current.txt` (and `previous.txt`), so the restart boots the new version;
/// 3. restart the owner and require the restarted process to answer `/api/health` with
///    `name`'s sidecar version.
///
/// A failure in steps 2 or 3 is an error, but the pointers are deliberately left where step 2 put
/// them: `current.txt` names `name`, `previous.txt` names the version it replaced, and that
/// version's directory is untouched. The operator can therefore roll back with one pointer write,
/// and the old version is still on disk to boot.
pub fn install_version_and_restart(
    config: &Config,
    name: &str,
    pair: &StagedPair,
    probe: &dyn VersionProbe,
    host: &dyn MigrationHost,
) -> Result<InstalledVersion, String> {
    let installed = install_version(config, name, pair, probe)?;
    let version = installed.version.sidecar.version.clone();
    let rollback = match installed.previous.as_deref() {
        Some(previous) => format!(
            "{} still names {previous} for a rollback",
            previous_pointer(&config.install).display()
        ),
        None => "there was no previous version to roll back to".to_string(),
    };
    let pid = host.restart(config).map_err(|error| {
        format!(
            "version {version} is current in {}, but the owner could not be restarted: {error}; \
             {rollback}",
            current_pointer(&config.install).display()
        )
    })?;
    host.require_healthy(config, &version, pid)
        .map_err(|error| {
            format!(
                "version {version} is current in {}, but the restarted process did not report it: \
             {error}; {rollback}",
                current_pointer(&config.install).display()
            )
        })?;
    Ok(installed)
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// The candidate probe: a copy of the data directory, a scratch port, two health answers
// ─────────────────────────────────────────────────────────────────────────────────────────────────

/// The files under `<data>` that the **running app owns**, which a candidate probe must not copy.
///
/// The probe gives a candidate a private copy of the data directory so it can start on a scratch
/// port without touching the live tree. Four entries must stay behind, because they describe or
/// belong to the process that is still serving — copying them either cannot work or would make the
/// candidate believe it inherited the live app's identity:
///
/// - `device.lock` — `device/device/src/lock.rs` (`LOCK_FILE`). The serving app opens it and holds
///   an **exclusive OS lock** (`LockFileEx` on Windows) for its whole lifetime. On Windows that lock
///   is mandatory: a read of the file fails with "another program has locked a part of the file"
///   (os error 33), which is exactly how every upgrade was refused before 2026-11-05. The candidate
///   must create its own lock instead.
/// - `device.pid` — `device/device/src/lock.rs` (`PID_FILE`). It names the process holding the lock.
///   A copy would claim the live app's pid for a candidate that is a different process, so a refusal
///   or a diagnostic would name the wrong one. The candidate writes its own when it takes its lock.
/// - `device.port` — `device/device/src/main.rs` writes it after binding. It is the one source the
/// - `device.port` — `device/device/src/main.rs` writes it after binding. It is the discovery file
///   the tray and browser helper read to find the **live** app's port; the candidate binds a scratch
///   port and writes its own, so a copy would be a stale fact about another process (and the
///   candidate overwrites it anyway). The candidate writes its own.
/// - `restarts.jsonl` — `device/device/src/restarts.rs` (`FILE`). It is the serving app's boot
///   journal, rewritten at every boot. Copying it would give the candidate boot records from another
///   process (with the live app's pids and versions); the candidate starts with its own journal.
///
/// Only entries at the root of `<data>` are skipped: the same names deeper in the tree belong to a
/// plugin or a workspace, not to the app. The list is the one place these four names are spelled;
/// the probe copy and its tests both read it.
pub const RUNNING_APP_FILES: [&str; 4] =
    ["device.lock", "device.pid", "device.port", "restarts.jsonl"];

/// Whether `name` is a data-directory entry owned by the running app ([`RUNNING_APP_FILES`]).
pub fn is_running_app_file(name: &std::ffi::OsStr) -> bool {
    RUNNING_APP_FILES
        .iter()
        .any(|owned| name == std::ffi::OsStr::new(owned))
}

/// The OS effects a candidate probe needs, so the probe itself stays testable on Linux.
///
/// The shipped implementation starts the real candidate and keeps its `Child`; a test supplies a
/// peer that serves two health answers. The probe owns the scratch directory and the port; the host
/// only starts and stops the process.
pub trait CandidateHost {
    /// Start `version` against `config` — a scratch data directory and a scratch port — and return
    /// its pid.
    fn start(&self, config: &Config, version: &Version) -> Result<u32, String>;

    /// Stop the candidate [`CandidateHost::start`] returned. Called on success and on failure.
    fn stop(&self, config: &Config, pid: u32);
}

/// Start `version` on a scratch port with a copy of the data directory, require two matching health
/// answers, and kill it. The live child keeps serving the old version for the whole check.
///
/// The copy excludes [`RUNNING_APP_FILES`] so the probe works while the serving app holds its lock.
/// A missing live data directory is an empty probe directory (a fresh install has none).
pub fn probe_candidate(
    config: &Config,
    version: &Version,
    host: &dyn CandidateHost,
) -> Result<(), String> {
    let scratch = scratch_dir(&version.name);
    let scratch_data = scratch.join("data");
    let port = free_loopback_port()?;
    let result = (|| -> Result<(), String> {
        copy_probe_data(&config.data, &scratch_data)?;
        let scratch_config = Config {
            install: config.install.clone(),
            data: scratch_data,
            port,
        };
        let pid = host.start(&scratch_config, version)?;
        let healthy = wait_healthy(
            port,
            &version.sidecar.version,
            pid,
            CANDIDATE_HEALTH_TIMEOUT,
            || false,
        );
        host.stop(&scratch_config, pid);
        if healthy {
            Ok(())
        } else {
            Err(format!(
                "version {} (pid {pid}) did not answer /api/health twice within {} s on scratch port {port}",
                version.name,
                CANDIDATE_HEALTH_TIMEOUT.as_secs()
            ))
        }
    })();
    if let Err(error) = fs::remove_dir_all(&scratch) {
        if error.kind() != io::ErrorKind::NotFound {
            eprintln!(
                "launch: warning: cannot remove the probe directory {}: {error}",
                scratch.display()
            );
        }
    }
    result
}

/// A fresh probe directory under the OS temp: outside the install, so a probe can never touch a
/// version directory or the live data.
fn scratch_dir(version: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "summrise-candidate-{}-{}-{nonce}-{version}",
        std::process::id(),
        now_ms()
    ))
}

/// Copy the live data directory for the probe, minus [`RUNNING_APP_FILES`].
///
/// The candidate must not write the real `<data>`: state, the install token and the rollback memory
/// all live there. Entries that are neither a regular file nor a directory (a symlink, a device) are
/// skipped rather than followed.
pub fn copy_probe_data(from: &Path, to: &Path) -> Result<(), String> {
    copy_data_dir(from, to, true)
}

fn copy_data_dir(from: &Path, to: &Path, at_root: bool) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|error| {
        format!(
            "cannot create the probe data directory {}: {error}",
            to.display()
        )
    })?;
    let entries = match fs::read_dir(from) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "cannot read the data directory {}: {error}",
                from.display()
            ));
        }
    };
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!("cannot read the data directory {}: {error}", from.display())
        })?;
        if at_root && is_running_app_file(&entry.file_name()) {
            continue;
        }
        let target = to.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| format!("cannot inspect {}: {error}", entry.path().display()))?;
        if file_type.is_dir() {
            copy_data_dir(&entry.path(), &target, false)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target)
                .map_err(|error| format!("cannot copy {}: {error}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// Reserve a loopback port for the candidate: bind port 0, read what the kernel chose, release it.
fn free_loopback_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("cannot reserve a scratch port: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("cannot read the scratch port: {error}"))?
        .port();
    drop(listener);
    Ok(port)
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// The one-time migration from a flat install (2026-11-05 design)
// ─────────────────────────────────────────────────────────────────────────────────────────────────

/// What [`migrate_layout`] found, or did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Migration {
    /// `<install>/current.txt` already exists: the machine has been migrated. Nothing was touched,
    /// so the installer can call the mode on every launch.
    AlreadyMigrated {
        /// What the pointer said when it was found; reported, never repaired here.
        current: Pointer,
    },
    /// The flat install now boots from `versions/<name>/`, named by `current.txt`.
    Migrated {
        /// The version directory the pointer names.
        version: Version,
    },
}

/// The OS effects `--migrate-layout` needs: the launcher core stays testable on Linux.
///
/// The Windows implementation stops the service (or the console supervisor) and starts it again
/// from whatever `current.txt` now names; the tests implement this with a counter.
pub trait MigrationHost {
    /// Stop the install's running app and start it again from the layout the install now has.
    ///
    /// Returns the pid of the new child, so the health probe can require that exact process.
    fn restart(&self, config: &Config) -> Result<u32, String>;

    /// Require two matching `/api/health` answers from `pid` reporting `version`.
    ///
    /// [`HEALTH_CONSECUTIVE_PROBES`] consecutive answers are the rule, exactly as `--apply` uses.
    fn require_healthy(&self, config: &Config, version: &str, pid: u32) -> Result<(), String>;
}

/// `--migrate-layout`: turn a pre-versioned flat install into `versions/<name>/` + `current.txt`.
///
/// The one-way, one-time move the installer makes on a machine that still runs
/// `<install>/summrise-device.exe` repaired from `baseline/current`:
///
/// 1. a `current.txt` already there means the migration happened; report it and change nothing;
/// 2. take the flat binary and a trusted sidecar (the installer's `--sidecar`, else the baseline
///    generation the launcher already verifies) and copy both into `versions/<version>/` through a
///    temporary directory, so a half-written version is never named;
/// 3. write `current.txt` (temp file + rename; no `previous.txt`: the first migration has no older
///    version to return to);
/// 4. restart the owner and require two matching `/api/health` answers.
///
/// A failure after step 3 removes the pointer again and restarts the flat layout, so a machine
/// whose new layout cannot serve stays on the layout it had. The flat exe and baseline are never
/// removed: they are the fallback for one release.
pub fn migrate_layout(
    config: &Config,
    installer_sidecar: Option<&Path>,
    pubkeys_hex: &[&str],
    host: &dyn MigrationHost,
) -> Result<Migration, String> {
    let install = &config.install;
    let pointer = current_pointer(install);
    if pointer.exists() {
        return Ok(Migration::AlreadyMigrated {
            current: read_current_pointer(install),
        });
    }

    let (sidecar, sidecar_path) = migration_sidecar(install, installer_sidecar, pubkeys_hex)?;
    let name = sidecar.version.clone();
    validate_version_name(&name).map_err(|error| {
        format!("the sidecar's version {name:?} cannot name a version directory: {error}")
    })?;
    let source = app_exe(install);
    if !source.is_file() {
        return Err(format!(
            "the flat app {} is missing; the supervisor repairs it at its next start, so the \
             migration is refused rather than writing a version directory from nothing",
            source.display()
        ));
    }
    match sha256_file(&source) {
        Ok(hash) if hash.eq_ignore_ascii_case(&sidecar.sha256) => {}
        Ok(hash) => {
            return Err(format!(
                "the flat app {} hashes to {hash}, not the trusted sidecar's {}; refusing to \
                 promote bytes the sidecar does not describe",
                source.display(),
                sidecar.sha256
            ))
        }
        Err(error) => {
            return Err(format!(
                "cannot hash the flat app {}: {error}",
                source.display()
            ))
        }
    }

    let pair = StagedPair {
        exe: source,
        sidecar_path,
        sidecar,
    };
    let version = stage_migration_version(install, &name, &pair)?;
    write_current_pointer(install, &name).map_err(|error| {
        format!(
            "the version directory is ready, but {} could not be written: {error}; the flat \
             layout is still in charge",
            pointer.display()
        )
    })?;

    let switched = (|| -> Result<(), String> {
        let pid = host.restart(config)?;
        host.require_healthy(config, &version.sidecar.version, pid)
    })();
    match switched {
        Ok(()) => Ok(Migration::Migrated { version }),
        Err(reason) => {
            // The machine must stay on the layout it had. The pointer is the one fact that made
            // the new layout boot, so it goes first; the restart then brings the flat app back.
            let removal = remove_pointer(&pointer);
            let fallback = host.restart(config);
            let mut message = format!(
                "{reason}; {} was removed so the flat layout stays in charge",
                pointer.display()
            );
            if let Err(error) = removal {
                message.push_str(&format!(
                    "; CRITICAL: {} could not be removed ({error}), so the unproven version may be booted",
                    pointer.display()
                ));
            }
            match fallback {
                Ok(_) => message.push_str(" and the flat app was started again"),
                Err(error) => message.push_str(&format!(
                    "; CRITICAL: the flat app could not be started again ({error})"
                )),
            }
            Err(message)
        }
    }
}

/// The sidecar a migration trusts, and where it was read.
///
/// The installer's `--sidecar` wins when it passed one; otherwise the baseline generation the
/// supervisor already boots from is the trusted description of the flat binary. Either way the
/// signature must verify with a key this build carries before the sidecar can name a version.
fn migration_sidecar(
    install: &Path,
    installer_sidecar: Option<&Path>,
    pubkeys_hex: &[&str],
) -> Result<(Sidecar, PathBuf), String> {
    if let Some(path) = installer_sidecar {
        let sidecar = load_sidecar(path).map_err(|error| {
            format!(
                "cannot read the installer's sidecar {}: {error}",
                path.display()
            )
        })?;
        verify_migration_signature(&sidecar, pubkeys_hex).map_err(|error| {
            format!(
                "the installer's sidecar {} is not trusted: {error}",
                path.display()
            )
        })?;
        return Ok((sidecar, path.to_path_buf()));
    }

    let keyed = pubkeys_hex.iter().any(|key| *key != PLACEHOLDER_PUBKEY_HEX);
    let choice = if keyed {
        resolve_baseline_signed_with(install, pubkeys_hex)
    } else {
        // An unkeyed build cannot check a signature; the hash-consistent baseline is the whole
        // answer, exactly as `resolve_baseline` treats it for every other reader.
        resolve_baseline(install)
    };
    match choice {
        BaselineChoice::Current(baseline) | BaselineChoice::Previous(baseline) => {
            Ok((baseline.sidecar, baseline.sidecar_path))
        }
        BaselineChoice::NeedsHuman(reason) => {
            Err(format!("cannot migrate the flat install: {reason}"))
        }
    }
}

/// Verify a migration sidecar, unless this build carries no key at all.
fn verify_migration_signature(sidecar: &Sidecar, pubkeys_hex: &[&str]) -> Result<(), String> {
    if !pubkeys_hex.iter().any(|key| *key != PLACEHOLDER_PUBKEY_HEX) {
        return Ok(());
    }
    verify_any_signature(sidecar, pubkeys_hex)
}

/// Create one fresh directory with an unpredictable name under `parent`.
///
/// The same `RandomState` nonce and `create_new` discipline as [`create_temp_sibling`], for a
/// directory: a migration is built here and renamed into `versions/<name>` only when complete.
fn create_temp_dir(parent: &Path, tag: &str) -> io::Result<PathBuf> {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    for _ in 0..16 {
        let nonce = RandomState::new().build_hasher().finish();
        let directory = parent.join(format!(".{tag}.{nonce:016x}.tmp"));
        match fs::create_dir(&directory) {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "cannot create a temporary directory under {} after 16 attempts",
            parent.display()
        ),
    ))
}

/// Reuse a version directory that already exists, or refuse because it is not the same release.
///
/// A version directory is immutable once named: the migration is allowed to find its own earlier
/// attempt (or the device's stager having put the same release there) and reuse it, but different
/// bytes are never written over a directory a running process may be executing.
fn reuse_existing_version(
    install: &Path,
    name: &str,
    sidecar: &Sidecar,
) -> Result<Version, String> {
    let dir = version_dir(install, name);
    let metadata = fs::symlink_metadata(&dir)
        .map_err(|error| format!("cannot inspect {}: {error}", dir.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "version {name:?} already exists at {} as a symbolic link; refusing to write through it",
            dir.display()
        ));
    }
    if !metadata.is_dir() {
        return Err(format!(
            "version {name:?} already exists at {} and is not a directory",
            dir.display()
        ));
    }
    let exe = version_exe(install, name);
    match sha256_file(&exe) {
        Ok(hash) if hash.eq_ignore_ascii_case(&sidecar.sha256) => {}
        Ok(_) => {
            return Err(format!(
                "version {name:?} is already installed at {} with different bytes; refusing to \
                 overwrite a version directory that may be running",
                exe.display()
            ))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(format!(
                "version {name:?} already exists at {} without {}, so it is not a complete \
                 version; refusing to overwrite it",
                dir.display(),
                APP_EXE
            ))
        }
        Err(error) => {
            return Err(format!(
            "version {name:?} is already installed at {} but its binary cannot be read: {error}",
            exe.display()
        ))
        }
    }
    let on_disk = load_sidecar(&version_sidecar(install, name)).map_err(|error| {
        format!(
            "version {name:?} is already installed at {} but its sidecar is unusable: {error}",
            dir.display()
        )
    })?;
    if on_disk.version != sidecar.version || !on_disk.sha256.eq_ignore_ascii_case(&sidecar.sha256) {
        return Err(format!(
            "version {name:?} is already installed with a different sidecar at {}; refusing to \
             rewrite it",
            version_sidecar(install, name).display()
        ));
    }
    inspect_version(install, name)
}

/// Copy a verified flat pair into `versions/<name>` through a temporary directory, then rename it.
///
/// The rename is the only step that makes the directory visible, so a kill leaves either no
/// `versions/<name>` or a complete one. An existing directory with the same bytes is reused; one
/// with different bytes is a refusal, never an overwrite.
fn stage_migration_version(
    install: &Path,
    name: &str,
    pair: &StagedPair,
) -> Result<Version, String> {
    validate_version_name(name)?;
    if !is_sha256_hex(&pair.sidecar.sha256) {
        return Err(format!(
            "the trusted sidecar's sha256 is not 64 hex characters: {:?}",
            pair.sidecar.sha256
        ));
    }
    let versions = versions_dir(install);
    fs::create_dir_all(&versions).map_err(|error| {
        format!(
            "cannot create the versions directory {}: {error}",
            versions.display()
        )
    })?;
    let target = version_dir(install, name);
    if target.exists() {
        return reuse_existing_version(install, name, &pair.sidecar);
    }

    let staging = create_temp_dir(&versions, "migrating").map_err(|error| {
        format!(
            "cannot create a staging directory under {}: {error}",
            versions.display()
        )
    })?;
    let built = (|| -> Result<(), String> {
        save_sidecar(&staging.join(VERSION_SIDECAR), &pair.sidecar).map_err(|error| {
            format!(
                "cannot write the version sidecar in {}: {error}",
                staging.display()
            )
        })?;
        let staged_exe = staging.join(APP_EXE);
        copy_file_synced(&pair.exe, &staged_exe).map_err(|error| {
            format!(
                "cannot copy {} into {}: {error}",
                pair.exe.display(),
                staging.display()
            )
        })?;
        match sha256_file(&staged_exe) {
            Ok(hash) if hash.eq_ignore_ascii_case(&pair.sidecar.sha256) => Ok(()),
            Ok(hash) => Err(format!(
                "the copied version binary hashes to {hash}, not the sidecar's {}",
                pair.sidecar.sha256
            )),
            Err(error) => Err(format!(
                "cannot hash the copied version binary {}: {error}",
                staged_exe.display()
            )),
        }?;
        sync_dir(&staging).map_err(|error| format!("cannot flush {}: {error}", staging.display()))
    })();
    if let Err(error) = built {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }

    if let Err(error) = fs::rename(&staging, &target) {
        let _ = fs::remove_dir_all(&staging);
        if target.exists() {
            // Another writer won the rename. Reuse its directory only when it is this release.
            return reuse_existing_version(install, name, &pair.sidecar).map_err(|conflict| {
                format!(
                    "cannot move the staged version into {}: {error}; {conflict}",
                    target.display()
                )
            });
        }
        return Err(format!(
            "cannot move the staged version into {}: {error}",
            target.display()
        ));
    }
    sync_dir(&versions).map_err(|error| {
        format!(
            "the version directory {} is in place but {} could not be flushed: {error}",
            target.display(),
            versions.display()
        )
    })?;
    inspect_version(install, name)
}

/// The pointer move a rollback performed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PointerRollback {
    /// What `current.txt` named before the rollback, when it named anything.
    pub from: Option<String>,
    /// The version directory the pointer now names.
    pub to: String,
}

/// Write `current.txt` back to the version `previous.txt` recorded.
///
/// The rollback is verified before it is written: a previous pointer that names a missing or
/// inconsistent directory is refused, so a bad rollback cannot replace a bad current. On success
/// `previous.txt` is consumed, so the next failure has no target and cannot bounce between two
/// versions forever.
pub fn roll_back_pointer(install: &Path) -> Result<Option<PointerRollback>, String> {
    let previous = match read_previous_pointer(install) {
        Pointer::Version(name) => name,
        Pointer::Missing => return Ok(None),
        Pointer::Invalid(reason) => {
            return Err(format!(
                "{} is unusable: {reason}",
                previous_pointer(install).display()
            ));
        }
    };
    let current = match read_current_pointer(install) {
        Pointer::Version(name) => Some(name),
        Pointer::Missing | Pointer::Invalid(_) => None,
    };
    if current.as_deref() == Some(previous.as_str()) {
        // A crash after the rollback's pointer write but before its cleanup. Nothing to move.
        remove_pointer(&previous_pointer(install)).map_err(|error| {
            format!(
                "cannot remove the consumed {}: {error}",
                previous_pointer(install).display()
            )
        })?;
        return Ok(None);
    }
    inspect_version(install, &previous)
        .map_err(|error| format!("cannot roll back to version {previous:?}: {error}"))?;
    write_current_pointer(install, &previous).map_err(|error| {
        format!(
            "cannot roll back to version {previous:?}: writing {} failed: {error}",
            current_pointer(install).display()
        )
    })?;
    remove_pointer(&previous_pointer(install)).map_err(|error| {
        format!(
            "the rollback is in place, but {} cannot be removed: {error}",
            previous_pointer(install).display()
        )
    })?;
    Ok(Some(PointerRollback {
        from: current,
        to: previous,
    }))
}

/// Counts consecutive failed starts of the version `current.txt` names.
///
/// The supervisor feeds it every start that failed (spawn error, or a child that died before it was
/// stable) and every child that stayed up ([`StartWatch::on_stable`] resets). At
/// [`ROLLBACK_AFTER_FAILED_STARTS`] it performs the pointer rollback and resets, so the previous
/// version gets a fresh restart budget instead of inheriting the failed one's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StartWatch {
    failed_starts: u8,
}

impl StartWatch {
    /// A watch with no failures yet.
    pub fn new() -> StartWatch {
        StartWatch::default()
    }

    /// How many consecutive failed starts have been recorded.
    pub fn failed_starts(&self) -> u8 {
        self.failed_starts
    }

    /// A child stayed up long enough to count as a successful start.
    pub fn on_stable(&mut self) {
        self.failed_starts = 0;
    }

    /// Record one failed start, and roll the pointer back when the rule says so.
    ///
    /// A pre-versioned install has no pointer to write, so failures there stay with the existing
    /// restart budget.
    pub fn on_failed_start(&mut self, install: &Path) -> Result<Option<PointerRollback>, String> {
        if matches!(read_current_pointer(install), Pointer::Missing) {
            return Ok(None);
        }
        self.failed_starts = self.failed_starts.saturating_add(1);
        if self.failed_starts < ROLLBACK_AFTER_FAILED_STARTS {
            return Ok(None);
        }
        self.failed_starts = 0;
        roll_back_pointer(install)
    }
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// The swap request and its result
// ─────────────────────────────────────────────────────────────────────────────────────────────────

/// The app's request that the supervisor perform one swap (fix design F1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwapRequest {
    /// Millisecond Unix time the app wrote the request.
    pub requested_at_ms: u64,
    /// The release version the app staged; must equal the sidecar's.
    pub version: String,
    /// The binary digest the app staged; must equal the sidecar's.
    pub sha256: String,
}

/// The parser's answer for a request file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwapRequestState {
    /// No request file.
    Missing,
    /// A fresh, well-formed request.
    Valid(SwapRequest),
    /// A request past [`SWAP_REQUEST_TIMEOUT_MS`]; never retried forever.
    TooOld { request: SwapRequest, age_ms: u64 },
    /// Torn, unparsable or structurally invalid; discarded with `outcome:"refused"`.
    Invalid(String),
}

/// Parse a request from bytes and judge its age against `now_ms`.
pub fn parse_swap_request(bytes: &[u8], now_ms: u64) -> SwapRequestState {
    let request: SwapRequest = match serde_json::from_slice(bytes) {
        Ok(request) => request,
        Err(error) => return SwapRequestState::Invalid(format!("cannot parse: {error}")),
    };
    if request.version.is_empty()
        || request.version.contains('\n')
        || request.version.contains('\r')
    {
        return SwapRequestState::Invalid(format!(
            "the request's version is empty or contains a newline: {:?}",
            request.version
        ));
    }
    if !is_sha256_hex(&request.sha256) {
        return SwapRequestState::Invalid(format!(
            "the request's sha256 is not 64 hex characters: {:?}",
            request.sha256
        ));
    }
    let age_ms = now_ms.saturating_sub(request.requested_at_ms);
    if age_ms > SWAP_REQUEST_TIMEOUT_MS {
        SwapRequestState::TooOld { request, age_ms }
    } else {
        SwapRequestState::Valid(request)
    }
}

/// Read a request file and judge its age. Only "not found" means missing; any other read error is a
/// malformed request, because pretending it is absent would retry it forever.
pub fn read_swap_request(path: &Path, now_ms: u64) -> SwapRequestState {
    match read_capped(path, MAX_SWAP_REQUEST_BYTES) {
        CappedRead::Bytes(bytes) => parse_swap_request(&bytes, now_ms),
        CappedRead::Missing => SwapRequestState::Missing,
        CappedRead::Failed(message) => SwapRequestState::Invalid(message),
    }
}

/// What the supervisor did with a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwapOutcome {
    /// The new build passed both probes and is now `current`.
    Applied,
    /// The new build failed; the previous generation is `current` and is serving again.
    RolledBack,
    /// The request or its staged pair was not usable; nothing was stopped.
    Refused,
}

/// The supervisor's answer, written atomically to `<install>/swap.result.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwapResult {
    /// What happened.
    pub outcome: SwapOutcome,
    /// The release version the request named, for `applied` and `rolled_back`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The artifact's digest, when the supervisor verified one.
    ///
    /// Written for `applied` and `rolled_back` — the staged pair is verified before the swap — and
    /// for a `refused` result when validation had a verified pair to name. Absent when the request
    /// was unusable before any artifact was read. The app keys its rollback memory on this digest,
    /// so a rollback via `--apply` is recorded like any other.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Millisecond Unix time the result was written.
    pub at_ms: u64,
    /// Why a request was refused, or why a rollback happened.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl SwapResult {
    pub fn applied(
        version: impl Into<String>,
        sha256: impl Into<String>,
        at_ms: u64,
    ) -> SwapResult {
        SwapResult {
            outcome: SwapOutcome::Applied,
            version: Some(version.into()),
            sha256: Some(sha256.into()),
            at_ms,
            reason: None,
        }
    }

    pub fn rolled_back(
        version: impl Into<String>,
        sha256: impl Into<String>,
        at_ms: u64,
        reason: impl Into<String>,
    ) -> SwapResult {
        SwapResult {
            outcome: SwapOutcome::RolledBack,
            version: Some(version.into()),
            sha256: Some(sha256.into()),
            at_ms,
            reason: Some(reason.into()),
        }
    }

    pub fn refused(at_ms: u64, reason: impl Into<String>) -> SwapResult {
        SwapResult {
            outcome: SwapOutcome::Refused,
            version: None,
            sha256: None,
            at_ms,
            reason: Some(reason.into()),
        }
    }

    /// A refusal after a staged pair verified: the digest names the artifact that was not installed.
    pub fn refused_verified(
        sha256: impl Into<String>,
        at_ms: u64,
        reason: impl Into<String>,
    ) -> SwapResult {
        SwapResult {
            outcome: SwapOutcome::Refused,
            version: None,
            sha256: Some(sha256.into()),
            at_ms,
            reason: Some(reason.into()),
        }
    }
}

/// Write a result with the same atomic discipline as every other launcher file.
pub fn write_swap_result(path: &Path, result: &SwapResult) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(result).map_err(io::Error::other)?;
    write_atomic_bytes(path, &bytes)
}

/// Millisecond Unix time, saturating at zero for a clock before the epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// The install token
// ─────────────────────────────────────────────────────────────────────────────────────────────────

/// What the installer should do with `<data>/token`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenAction {
    /// The file is absent: mint 32 random bytes, hex + newline, and protect its DACL.
    Create,
    /// The file exists: leave its bytes and ACL untouched (an operator may have rotated it).
    Keep,
}

/// The mint-once decision. It is a pure function so the idempotence rule is testable without an
/// elevated installer; the Windows half owns the CSPRNG and the DACL.
pub fn token_action(exists: bool) -> TokenAction {
    if exists {
        TokenAction::Keep
    } else {
        TokenAction::Create
    }
}

/// The token file's exact contents: 32 random bytes as 64 lower-hex characters and a newline.
pub fn token_text(bytes: &[u8; TOKEN_BYTES]) -> String {
    format!("{}\n", hex_encode(bytes))
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// The staged pair
// ─────────────────────────────────────────────────────────────────────────────────────────────────

/// A staged artifact whose signature and digest have been checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedPair {
    /// The artifact path the pair was read from.
    pub exe: PathBuf,
    /// The sidecar path the pair was read from.
    pub sidecar_path: PathBuf,
    /// The verified signed facts.
    pub sidecar: Sidecar,
}

/// Verify a staged binary and sidecar with the **release** key.
///
/// This is the one call every update path makes (`swap.request.json` and the operator's `--apply`):
/// the app writes the staged sidecar from a manifest it verified with its own
/// `SUMMRISE_RELEASE_PUBKEY`, so the launcher must check it with that same key. Using the launcher
/// key here would refuse every genuine update on a correctly two-keyed install (C1).
pub fn validate_staged_update(
    staged_exe: &Path,
    staged_sidecar: &Path,
) -> Result<StagedPair, String> {
    validate_staged_pair(staged_exe, staged_sidecar, RELEASE_PUBKEY_HEX)
}

/// The keys that may sign a version directory handed to `--install-version`.
///
/// Two producers stage a version directory, and they are signed by different compile-time keys:
///
/// - the **device's updater** stages `versions/<release>/` from a release document and signs with
///   the release key ([`validate_staged_update`]);
/// - the **installer** stages `versions/<setup version>/` from the pair the setup ships, whose
///   sidecar is the first-generation sidecar signed with the launcher key.
///
/// Both keys are embedded in this binary at build time and neither can be supplied at runtime, so
/// accepting either here does not make the signature optional. What makes a candidate current is
/// still the probe, not the signature alone. `--apply` stays release-key-only: it is the updater's
/// path and never runs from a setup.
pub const INSTALL_VERSION_PUBKEYS: [&str; 2] = [RELEASE_PUBKEY_HEX, LAUNCHER_PUBKEY_HEX];

/// Verify an `--install-version` pair with either key that stages a version directory.
pub fn validate_install_version(
    staged_exe: &Path,
    staged_sidecar: &Path,
) -> Result<StagedPair, String> {
    validate_install_version_with(staged_exe, staged_sidecar, &INSTALL_VERSION_PUBKEYS)
}

/// [`validate_install_version`] with an explicit key list, for the two-key tests.
///
/// Every key is tried; the reported reason names each refusal, so a pair signed by the wrong key
/// says so instead of hiding behind the first attempt's error.
pub fn validate_install_version_with(
    staged_exe: &Path,
    staged_sidecar: &Path,
    pubkeys_hex: &[&str],
) -> Result<StagedPair, String> {
    let mut reasons = Vec::with_capacity(pubkeys_hex.len());
    for key in pubkeys_hex {
        match validate_staged_pair(staged_exe, staged_sidecar, key) {
            Ok(pair) => return Ok(pair),
            Err(reason) => reasons.push(reason),
        }
    }
    Err(reasons.join("; "))
}

/// Verify a staged binary and sidecar against an explicit key.
///
/// Production callers use [`validate_staged_update`] (the release key); this form exists for the
/// two-key tests and for `--apply`'s historical entry point.
pub fn validate_staged_pair(
    staged_exe: &Path,
    staged_sidecar: &Path,
    pubkey_hex: &str,
) -> Result<StagedPair, String> {
    let pubkey =
        parse_pubkey(pubkey_hex).map_err(|error| format!("refusing the update: {error}"))?;
    let sidecar =
        load_sidecar(staged_sidecar).map_err(|error| format!("refusing the update: {error}"))?;
    verify_signature(&sidecar, &pubkey).map_err(|error| format!("refusing the update: {error}"))?;
    if !staged_exe.is_file() {
        return Err(format!(
            "refusing the update: staged artifact {} is missing",
            staged_exe.display()
        ));
    }
    match sha256_file(staged_exe) {
        Ok(hash) if hash.eq_ignore_ascii_case(&sidecar.sha256) => Ok(StagedPair {
            exe: staged_exe.to_path_buf(),
            sidecar_path: staged_sidecar.to_path_buf(),
            sidecar,
        }),
        Ok(hash) => Err(format!(
            "refusing the update: staged hash {hash} does not match {}",
            sidecar.sha256
        )),
        Err(error) => Err(format!(
            "refusing the update: cannot hash {}: {error}",
            staged_exe.display()
        )),
    }
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// The build-bound health probe
// ─────────────────────────────────────────────────────────────────────────────────────────────────

/// The app's `/api/health` document: the release version and the pid that is serving.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct HealthDocument {
    /// `"ok"` when the app is serving.
    pub status: String,
    /// The release version from the child's `--release-version` argv.
    pub version: String,
    /// The serving process.
    pub pid: u32,
}

/// Why a health answer did not prove the started build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HealthError {
    /// The connection could not be opened or written.
    Unreachable(String),
    /// The response was unreadable or did not carry the health JSON document.
    Malformed(String),
    /// The status line was not 200.
    Status(u16),
    /// The body was 200 JSON but not the healthy status.
    NotOk(String),
    /// The serving build is not the one the launcher started.
    WrongVersion { expected: String, got: String },
    /// Another process (usually the old app) is answering the port.
    WrongPid { expected: u32, got: u32 },
}

impl fmt::Display for HealthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HealthError::Unreachable(error) => write!(f, "health is unreachable: {error}"),
            HealthError::Malformed(error) => write!(f, "health is malformed: {error}"),
            HealthError::Status(status) => write!(f, "/api/health answered {status}"),
            HealthError::NotOk(status) => write!(f, "/api/health status is {status:?}, not \"ok\""),
            HealthError::WrongVersion { expected, got } => {
                write!(f, "/api/health reports version {got:?}, not {expected:?}")
            }
            HealthError::WrongPid { expected, got } => {
                write!(
                    f,
                    "/api/health reports pid {got}, not the started child {expected}"
                )
            }
        }
    }
}

impl std::error::Error for HealthError {}

/// Parse one HTTP response and require it to prove `expected_version` served from
/// `expected_pid`. The body, not just the status line, is the point (fix design F4).
pub fn parse_health_response(
    response: &[u8],
    expected_version: &str,
    expected_pid: u32,
) -> Result<HealthDocument, HealthError> {
    let (head, body) = match response.windows(4).position(|window| window == b"\r\n\r\n") {
        Some(index) => (&response[..index], &response[index + 4..]),
        None => {
            return Err(HealthError::Malformed(
                "the response has no header/body separator".to_string(),
            ))
        }
    };
    let head = String::from_utf8_lossy(head);
    let status_line = head.lines().next().unwrap_or_default();
    let mut parts = status_line.split_whitespace();
    let protocol = parts.next().unwrap_or_default();
    if protocol != "HTTP/1.0" && protocol != "HTTP/1.1" {
        return Err(HealthError::Malformed(format!(
            "the status line is not HTTP: {status_line:?}"
        )));
    }
    let status: u16 = parts
        .next()
        .and_then(|status| status.parse().ok())
        .ok_or_else(|| HealthError::Malformed(format!("the status line: {status_line:?}")))?;
    if status != 200 {
        return Err(HealthError::Status(status));
    }
    let document: HealthDocument = serde_json::from_slice(body).map_err(|error| {
        HealthError::Malformed(format!(
            "the body is not a health document ({error}): {:?}",
            String::from_utf8_lossy(body)
        ))
    })?;
    if document.status != "ok" {
        return Err(HealthError::NotOk(document.status));
    }
    if document.version != expected_version {
        return Err(HealthError::WrongVersion {
            expected: expected_version.to_string(),
            got: document.version,
        });
    }
    if document.pid != expected_pid {
        return Err(HealthError::WrongPid {
            expected: expected_pid,
            got: document.pid,
        });
    }
    Ok(document)
}

/// One unauthenticated probe of the app's health endpoint, judged against the expected build.
pub fn probe_health(
    port: u16,
    expected_version: &str,
    expected_pid: u32,
) -> Result<HealthDocument, HealthError> {
    const MAX_RESPONSE_BYTES: usize = 64 * 1024;

    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(500))
        .map_err(|error| HealthError::Unreachable(error.to_string()))?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(1_000)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    stream
        .write_all(b"GET /api/health HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .map_err(|error| HealthError::Unreachable(error.to_string()))?;

    let mut response = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if response.len() + read > MAX_RESPONSE_BYTES {
                    return Err(HealthError::Malformed(
                        "the response is larger than 64 KiB".to_string(),
                    ));
                }
                response.extend_from_slice(&buffer[..read]);
            }
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut =>
            {
                break
            }
            Err(error) => return Err(HealthError::Unreachable(error.to_string())),
        }
    }
    parse_health_response(&response, expected_version, expected_pid)
}

/// Poll until [`HEALTH_CONSECUTIVE_PROBES`] matching answers in a row arrive, or `timeout` elapses.
///
/// A single 200 is not proof: an old app (or an unrelated process) can still answer the port. The
/// pid and version bind the answer to the child the supervisor started.
pub fn wait_healthy(
    port: u16,
    expected_version: &str,
    expected_pid: u32,
    timeout: Duration,
    should_stop: impl Fn() -> bool,
) -> bool {
    let deadline = Instant::now() + timeout;
    let mut consecutive = 0u8;
    loop {
        // A service stop must not wait out a ten-second probe while the SCM counts down.
        if should_stop() {
            return false;
        }
        match probe_health(port, expected_version, expected_pid) {
            Ok(_) => {
                consecutive = consecutive.saturating_add(1);
                if consecutive >= HEALTH_CONSECUTIVE_PROBES {
                    return true;
                }
            }
            Err(_) => consecutive = 0,
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_port_held_by_a_listener_is_not_bindable_until_it_is_released() {
        let held = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a listener");
        let port = held.local_addr().expect("address").port();
        assert!(
            wait_for_bindable_port(port, Duration::from_millis(300)).is_err(),
            "a held port must not read as bindable"
        );
        drop(held);
        assert!(
            wait_for_bindable_port(port, Duration::from_secs(5)).is_ok(),
            "a released port must read as bindable"
        );
    }

    use super::*;

    #[test]
    fn a_give_up_looks_like_a_failure_to_the_scm() {
        assert_eq!(
            failure_code(None),
            1,
            "a signal has no code: report a failure"
        );
        assert_eq!(
            failure_code(Some(0)),
            1,
            "zero on give-up is not a clean stop"
        );
        assert_eq!(failure_code(Some(7)), 7, "a real exit code is kept");
        assert_eq!(
            failure_code(Some(-1073741819)),
            1,
            "0xC0000005 as a signed int must not become 0"
        );
    }

    #[test]
    fn the_child_job_kills_the_app_but_not_the_processes_it_started() {
        // The two Win32 bits are named in CHILD_JOB_LIMITS' doc; assert the exact pair, because
        // dropping either one changes the lifecycle in opposite directions: without the first a
        // dead supervisor orphans the app, without the second stopping the service kills the
        // installer that is replacing it.
        assert_eq!(
            CHILD_JOB_LIMITS & 0x2000,
            0x2000,
            "KILL_ON_JOB_CLOSE keeps a dead supervisor's app from surviving"
        );
        assert_eq!(
            CHILD_JOB_LIMITS & 0x1000,
            0x1000,
            "SILENT_BREAKAWAY_OK keeps the app's children (the installer) out of the job"
        );
        assert_eq!(
            CHILD_JOB_LIMITS & !0x3000,
            0,
            "the supervisor's job asks for no other limit"
        );
    }

    #[test]
    fn the_image_check_is_a_path_not_a_name() {
        let install = Path::new(r"C:\Summrise\summrise-device.exe");
        assert!(same_image(install, install), "the same path matches");
        assert!(same_image(
            install,
            Path::new(r"c:\summrise\SUMMRISE-DEVICE.EXE")
        ));
        assert!(same_image(
            Path::new(r"\\?\C:\Summrise\summrise-device.exe"),
            install
        ));
        assert!(
            !same_image(Path::new(r"D:\Summrise\summrise-device.exe"), install),
            "a second install with the same name is not ours"
        );
    }
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;
    use std::cell::RefCell;
    use std::fs;
    use std::net::TcpListener;

    #[test]
    fn a_transient_replace_error_is_retried_until_the_teardown_passes() {
        let mut calls = 0u32;
        let value = retry_transient(
            || {
                calls += 1;
                if calls < 3 {
                    Err(io::Error::from_raw_os_error(32))
                } else {
                    Ok(calls)
                }
            },
            |_| true,
            5,
        )
        .expect("the third attempt succeeds");
        assert_eq!(value, 3, "the operation runs until it succeeds");
        assert_eq!(calls, 3);
    }

    #[test]
    fn a_permanent_error_is_not_retried() {
        let mut calls = 0u32;
        let error = retry_transient(
            || -> io::Result<()> {
                calls += 1;
                Err(io::Error::from_raw_os_error(32))
            },
            |_| false,
            5,
        )
        .expect_err("a non-transient error is returned");
        assert_eq!(calls, 1, "one attempt, not five");
        assert_eq!(error.raw_os_error(), Some(32));
    }

    #[test]
    fn the_retry_budget_is_finite() {
        let mut calls = 0u32;
        let _ = retry_transient(
            || -> io::Result<()> {
                calls += 1;
                Err(io::Error::from_raw_os_error(5))
            },
            |_| true,
            3,
        )
        .expect_err("the budget runs out");
        assert_eq!(calls, 4, "the first attempt plus three retries");
    }
    use std::sync::atomic::{AtomicU32, Ordering};

    // A fixed seed stands in for the release signing key. The production constant is still the
    // phase-1 placeholder (see LAUNCHER_PUBKEY_HEX); these tests prove the verifier itself.
    const TEST_SEED: [u8; 32] = [7u8; 32];
    const OTHER_SEED: [u8; 32] = [9u8; 32];

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&TEST_SEED)
    }

    fn sidecar_for(version: &str, bytes: &[u8], key: &SigningKey) -> Sidecar {
        let sha256 = sha256_bytes(bytes);
        let channel = "stable";
        let message = format!("{version}\n{channel}\n{sha256}");
        let signature = key.sign(message.as_bytes());
        Sidecar {
            version: version.to_string(),
            channel: channel.to_string(),
            sha256,
            signature: hex_encode(&signature.to_bytes()),
        }
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "summrise-launch-{}-{}-{tag}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp dir");
            TempDir(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn install_dir(tmp: &TempDir, name: &str) -> PathBuf {
        let install = tmp.path().join(name);
        fs::create_dir_all(&install).expect("create install dir");
        install
    }

    /// Writes a consistent generation: the binary's bytes and the signed sidecar that describes them.
    fn write_generation(
        install: &Path,
        generation: Generation,
        version: &str,
        bytes: &[u8],
    ) -> Baseline {
        let dir = generation_dir(install, generation.dir_name());
        fs::create_dir_all(&dir).expect("create generation dir");
        fs::write(generation_exe(install, generation.dir_name()), bytes)
            .expect("write generation exe");
        let sidecar = sidecar_for(version, bytes, &signing_key());
        save_sidecar(
            &generation_sidecar(install, generation.dir_name()),
            &sidecar,
        )
        .expect("write generation sidecar");
        Baseline {
            generation,
            exe: generation_exe(install, generation.dir_name()),
            sidecar_path: generation_sidecar(install, generation.dir_name()),
            sidecar,
        }
    }

    fn assert_needs_human(choice: BaselineChoice, needle: &str) {
        match choice {
            BaselineChoice::NeedsHuman(reason) => assert!(
                reason.contains(needle),
                "the reason should contain {needle:?}: {reason}"
            ),
            other => panic!("expected NeedsHuman({needle:?}), got {other:?}"),
        }
    }

    #[test]
    fn sha256_bytes_matches_the_known_vector() {
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────────────────────────
    // Generation resolution
    // ─────────────────────────────────────────────────────────────────────────────────────────────

    #[test]
    fn current_wins_when_both_generations_are_consistent() {
        let tmp = TempDir::new("resolve-current");
        let install = install_dir(&tmp, "install");
        write_generation(&install, Generation::Previous, "2.0.2", b"previous bytes");
        write_generation(&install, Generation::Current, "2.0.3", b"current bytes");

        let choice = resolve_baseline(&install);
        match &choice {
            BaselineChoice::Current(baseline) => {
                assert_eq!(baseline.sidecar.version, "2.0.3");
                assert_eq!(baseline.generation, Generation::Current);
            }
            other => panic!("expected Current, got {other:?}"),
        }
    }

    #[test]
    fn previous_is_used_when_current_is_missing() {
        let tmp = TempDir::new("resolve-missing");
        let install = install_dir(&tmp, "install");
        write_generation(&install, Generation::Previous, "2.0.2", b"previous bytes");

        let choice = resolve_baseline(&install);
        match &choice {
            BaselineChoice::Previous(baseline) => {
                assert_eq!(baseline.sidecar.version, "2.0.2");
                assert_eq!(baseline.generation, Generation::Previous);
            }
            other => panic!("expected Previous, got {other:?}"),
        }
    }

    #[test]
    fn previous_is_used_when_current_pair_disagrees() {
        let tmp = TempDir::new("resolve-inconsistent");
        let install = install_dir(&tmp, "install");
        write_generation(&install, Generation::Previous, "2.0.2", b"previous bytes");
        // A current sidecar that describes bytes the binary does not contain.
        write_generation(&install, Generation::Current, "2.0.3", b"current bytes");
        fs::write(
            generation_exe(&install, CURRENT_DIR),
            b"tampered current bytes",
        )
        .unwrap();

        match resolve_baseline(&install) {
            BaselineChoice::Previous(baseline) => {
                assert_eq!(baseline.sidecar.version, "2.0.2");
            }
            other => panic!("expected Previous, got {other:?}"),
        }
    }

    #[test]
    fn a_zero_length_current_binary_falls_back_to_previous() {
        let tmp = TempDir::new("resolve-empty");
        let install = install_dir(&tmp, "install");
        write_generation(&install, Generation::Previous, "2.0.2", b"previous bytes");
        write_generation(&install, Generation::Current, "2.0.3", b"current bytes");
        fs::write(generation_exe(&install, CURRENT_DIR), b"").unwrap();

        match resolve_baseline(&install) {
            BaselineChoice::Previous(baseline) => assert_eq!(baseline.sidecar.version, "2.0.2"),
            other => panic!("expected Previous, got {other:?}"),
        }
    }

    #[test]
    fn pending_is_never_selected_as_a_baseline() {
        let tmp = TempDir::new("resolve-pending");
        let install = install_dir(&tmp, "install");
        // `pending` is perfectly consistent, and still must not be booted: nothing probed it.
        write_generation(&install, Generation::Pending, "2.0.4", b"pending bytes");

        assert_needs_human(resolve_baseline(&install), "no baseline generation");
    }

    #[test]
    fn pending_does_not_displace_a_consistent_current() {
        let tmp = TempDir::new("resolve-pending-current");
        let install = install_dir(&tmp, "install");
        write_generation(&install, Generation::Current, "2.0.3", b"current bytes");
        write_generation(&install, Generation::Pending, "2.0.4", b"pending bytes");

        match resolve_baseline(&install) {
            BaselineChoice::Current(baseline) => assert_eq!(baseline.sidecar.version, "2.0.3"),
            other => panic!("expected Current, got {other:?}"),
        }
    }

    #[test]
    fn neither_generation_consistent_needs_a_human_and_names_both() {
        let tmp = TempDir::new("resolve-none");
        let install = install_dir(&tmp, "install");
        write_generation(&install, Generation::Current, "2.0.3", b"current bytes");
        fs::write(generation_exe(&install, CURRENT_DIR), b"corrupt").unwrap();

        let choice = resolve_baseline(&install);
        match choice {
            BaselineChoice::NeedsHuman(reason) => {
                assert!(reason.contains("current"), "reason: {reason}");
                assert!(reason.contains("previous"), "reason: {reason}");
            }
            other => panic!("expected NeedsHuman, got {other:?}"),
        }
    }

    #[test]
    fn an_unparsable_current_sidecar_falls_back_to_previous() {
        let tmp = TempDir::new("resolve-garbage");
        let install = install_dir(&tmp, "install");
        write_generation(&install, Generation::Previous, "2.0.2", b"previous bytes");
        write_generation(&install, Generation::Current, "2.0.3", b"current bytes");
        fs::write(generation_sidecar(&install, CURRENT_DIR), b"{ not json").unwrap();

        match resolve_baseline(&install) {
            BaselineChoice::Previous(baseline) => assert_eq!(baseline.sidecar.version, "2.0.2"),
            other => panic!("expected Previous, got {other:?}"),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────────────────────
    // The installer's baseline step (NEW-1)
    // ─────────────────────────────────────────────────────────────────────────────────────────────

    fn test_pubkey_hex() -> String {
        hex_encode(signing_key().verifying_key().as_bytes())
    }

    #[test]
    fn a_complete_current_generation_is_kept_without_reading_the_seed() {
        let tmp = TempDir::new("ensure-kept");
        let install = install_dir(&tmp, "install");
        write_generation(&install, Generation::Current, "2.0.3", b"current bytes");
        let sidecar_before = fs::read(generation_sidecar(&install, CURRENT_DIR)).unwrap();

        let outcome = ensure_baseline(
            &install,
            Some(Path::new("/does/not/exist/seed.json")),
            &test_pubkey_hex(),
            false,
        )
        .expect("a complete current generation is the first case");

        assert_eq!(outcome, BaselineInstall::Kept);
        assert_eq!(
            fs::read(generation_sidecar(&install, CURRENT_DIR)).unwrap(),
            sidecar_before,
            "the installer leaves a verified current generation alone"
        );
    }

    #[test]
    fn the_flat_pair_is_migrated_into_current_with_the_generation_names() {
        let tmp = TempDir::new("ensure-migrate");
        let install = install_dir(&tmp, "install");
        fs::create_dir_all(baseline_dir(&install)).unwrap();
        fs::write(flat_exe(&install), b"flat bytes").unwrap();
        let sidecar = sidecar_for("2.0.3", b"flat bytes", &signing_key());
        save_sidecar(&flat_sidecar(&install), &sidecar).unwrap();

        let outcome = ensure_baseline(&install, None, &test_pubkey_hex(), false)
            .expect("the flat pair migrates without a seed");

        assert_eq!(outcome, BaselineInstall::Migrated);
        match resolve_baseline(&install) {
            BaselineChoice::Current(current) => {
                assert_eq!(current.sidecar.version, "2.0.3");
                assert_eq!(
                    current.sidecar_path,
                    generation_sidecar(&install, CURRENT_DIR),
                    "the flat sidecar was renamed to baseline.json"
                );
                assert_eq!(fs::read(&current.exe).unwrap(), b"flat bytes");
            }
            other => panic!("expected a migrated Current generation, got {other:?}"),
        }
        assert!(
            flat_exe(&install).is_file(),
            "migration copies; it does not delete the old pair"
        );
    }

    /// A migration is a seed by another name: the launcher key must have signed the pair, or the
    /// "first generation" would be whichever sidecar sat beside the install.
    #[test]
    fn a_flat_pair_signed_by_another_key_is_not_migrated() {
        let tmp = TempDir::new("ensure-migrate-foreign");
        let install = install_dir(&tmp, "install");
        fs::create_dir_all(baseline_dir(&install)).unwrap();
        fs::write(flat_exe(&install), b"flat bytes").unwrap();
        let sidecar = sidecar_for("2.0.3", b"flat bytes", &signing_key());
        save_sidecar(&flat_sidecar(&install), &sidecar).unwrap();

        let other = SigningKey::from_bytes(&[0x77; 32]);
        let other_hex =
            other
                .verifying_key()
                .as_bytes()
                .iter()
                .fold(String::new(), |mut hex, byte| {
                    hex.push_str(&format!("{byte:02x}"));
                    hex
                });
        let error = ensure_baseline(&install, None, &other_hex, false)
            .expect_err("a foreign signature must not become the first generation");
        assert!(
            error.contains("cannot migrate the pre-generation baseline"),
            "the refusal names the step and the key: {error}"
        );
        assert!(
            !generation_dir(&install, CURRENT_DIR).exists(),
            "nothing was promoted"
        );
    }

    /// The supervisor boots a generation only when its sidecar verifies against the launcher key.
    #[test]
    fn a_generation_must_verify_before_it_can_boot() {
        let tmp = TempDir::new("resolve-signed");
        let install = install_dir(&tmp, "install");
        fs::create_dir_all(baseline_dir(&install)).unwrap();
        fs::write(flat_exe(&install), b"flat bytes").unwrap();
        let sidecar = sidecar_for("2.0.3", b"flat bytes", &signing_key());
        save_sidecar(&flat_sidecar(&install), &sidecar).unwrap();
        ensure_baseline(&install, None, &test_pubkey_hex(), false).expect("the pair is seeded");

        assert!(
            matches!(
                resolve_baseline_signed_with(&install, &[&test_pubkey_hex()]),
                BaselineChoice::Current(_)
            ),
            "the launcher's own signature is bootable"
        );
        let other = SigningKey::from_bytes(&[0x66; 32]);
        let other_hex =
            other
                .verifying_key()
                .as_bytes()
                .iter()
                .fold(String::new(), |mut hex, byte| {
                    hex.push_str(&format!("{byte:02x}"));
                    hex
                });
        match resolve_baseline_signed_with(&install, &[&other_hex]) {
            BaselineChoice::NeedsHuman(reason) => assert!(
                reason.contains("not signed by this build"),
                "the refusal says why: {reason}"
            ),
            other => panic!("an unsigned generation must not boot: {other:?}"),
        }
    }

    /// A COMMITTED UPDATE IS RELEASE-SIGNED, AND IT MUST STILL BOOT. The first version of the boot
    /// check only accepted the launcher key, so the generation a successful update promoted was called
    /// untrusted at the next start, the old one was restored over it, and two updates in one supervisor
    /// lifetime left nothing bootable. Both keys are this build's.
    #[test]
    fn a_release_signed_generation_boots_like_the_launcher_signed_one() {
        let tmp = TempDir::new("two-key-baseline");
        let install = install_dir(&tmp, "install");
        fs::create_dir_all(baseline_dir(&install)).unwrap();
        fs::write(flat_exe(&install), b"seed bytes").unwrap();
        let seed = sidecar_for("1.0.0", b"seed bytes", &signing_key());
        save_sidecar(&flat_sidecar(&install), &seed).unwrap();
        ensure_baseline(&install, None, &test_pubkey_hex(), false).expect("the launcher seeds");

        // The update: a pair signed with the release key, promoted into `baseline/current` exactly as
        // `commit_pending` does after the probe.
        let release_key = SigningKey::from_bytes(&[0x5a; 32]);
        let release_hex =
            release_key
                .verifying_key()
                .as_bytes()
                .iter()
                .fold(String::new(), |mut hex, byte| {
                    hex.push_str(&format!("{byte:02x}"));
                    hex
                });
        fs::write(app_exe(&install), b"updated bytes").unwrap();
        let updated = sidecar_for("2.0.0", b"updated bytes", &release_key);
        install_generation_pair(&install, &app_exe(&install), &updated)
            .expect("the promoted pair is written");

        let launcher_hex = test_pubkey_hex();
        match resolve_baseline_signed_with(&install, &[&launcher_hex, &release_hex]) {
            BaselineChoice::Current(current) => assert_eq!(current.sidecar.version, "2.0.0"),
            other => panic!("a release-signed generation must boot: {other:?}"),
        }
        // And the bug it pins: the launcher key alone does not accept it.
        assert!(matches!(
            resolve_baseline_signed_with(&install, &[&launcher_hex]),
            BaselineChoice::NeedsHuman(_)
        ));
    }

    /// The staged sidecar and the swap request are app-writable; the supervisor must not read
    /// whatever sits there into memory on every poll.
    #[test]
    fn an_oversized_sidecar_or_request_is_refused_without_a_full_read() {
        let tmp = TempDir::new("capped-reads");
        let install = install_dir(&tmp, "install");
        fs::create_dir_all(&install).unwrap();
        let big = MAX_SIDECAR_BYTES + 1;

        let sidecar_path = install.join("summrise-device.new.json");
        fs::write(&sidecar_path, vec![b' '; big as usize]).unwrap();
        let error = load_sidecar(&sidecar_path).expect_err("a sidecar has a size limit");
        assert!(
            error.contains("the limit is"),
            "the refusal names the bound: {error}"
        );

        let request_path = install.join("swap.request.json");
        fs::write(
            &request_path,
            vec![b' '; (MAX_SWAP_REQUEST_BYTES + 1) as usize],
        )
        .unwrap();
        match read_swap_request(&request_path, 1) {
            SwapRequestState::Invalid(message) => {
                assert!(message.contains("the limit is"), "{message}")
            }
            other => panic!("an oversized request is invalid, not missing: {other:?}"),
        }
    }

    #[test]
    fn a_flat_pair_whose_hash_disagrees_is_refused_not_promoted() {
        let tmp = TempDir::new("ensure-migrate-bad");
        let install = install_dir(&tmp, "install");
        fs::create_dir_all(baseline_dir(&install)).unwrap();
        fs::write(flat_exe(&install), b"flat bytes").unwrap();
        let sidecar = sidecar_for("2.0.3", b"other bytes", &signing_key());
        save_sidecar(&flat_sidecar(&install), &sidecar).unwrap();

        let error = ensure_baseline(&install, None, &test_pubkey_hex(), false)
            .expect_err("a flat pair that does not hash must be refused");

        assert!(error.contains("hashes to"), "error: {error}");
        assert!(
            !generation_dir(&install, CURRENT_DIR).exists(),
            "a refused migration writes no current generation"
        );
    }

    #[test]
    fn a_missing_baseline_is_seeded_from_the_app_and_the_shipped_sidecar() {
        let tmp = TempDir::new("ensure-seed");
        let install = install_dir(&tmp, "install");
        fs::write(app_exe(&install), b"downloaded app bytes").unwrap();
        let seed = tmp.path().join("seed-baseline.json");
        save_sidecar(
            &seed,
            &sidecar_for("2.0.3", b"downloaded app bytes", &signing_key()),
        )
        .unwrap();

        let outcome = ensure_baseline(&install, Some(&seed), &test_pubkey_hex(), false)
            .expect("the signed seed creates the first generation");

        assert_eq!(outcome, BaselineInstall::Seeded);
        match resolve_baseline(&install) {
            BaselineChoice::Current(current) => {
                assert_eq!(current.sidecar.version, "2.0.3");
                assert_eq!(
                    current.sidecar.sha256,
                    sha256_bytes(b"downloaded app bytes")
                );
                assert_eq!(fs::read(&current.exe).unwrap(), b"downloaded app bytes");
            }
            other => panic!("expected a seeded Current generation, got {other:?}"),
        }
        assert!(app_exe(&install).is_file(), "seeding copies, not moves");
    }

    /// C1: the seed sidecar has exactly one owner, the launcher key. A sidecar signed with the
    /// release key (the key that signs release documents and updates) must be refused, and the
    /// refusal must name the key the operator has to sign with — otherwise the error reads as a
    /// corrupt signature and the next hour goes into regenerating keys.
    #[test]
    fn a_seed_signed_by_the_release_key_is_refused_naming_the_launcher_key() {
        let tmp = TempDir::new("ensure-seed-bad-key");
        let install = install_dir(&tmp, "install");
        fs::write(app_exe(&install), b"downloaded app bytes").unwrap();
        let seed = tmp.path().join("seed-baseline.json");
        let release_key = SigningKey::from_bytes(&OTHER_SEED);
        save_sidecar(
            &seed,
            &sidecar_for("2.0.3", b"downloaded app bytes", &release_key),
        )
        .unwrap();

        let error = ensure_baseline(&install, Some(&seed), &test_pubkey_hex(), false)
            .expect_err("a release-key-signed seed must not become the baseline");

        assert!(
            error.contains("launcher key"),
            "the refusal must name the launcher key: {error}"
        );
        assert!(
            error.contains("SUMMRISE_LAUNCHER_PUBKEY"),
            "the refusal must name the build-time variable that owns the seed: {error}"
        );
        assert!(!generation_dir(&install, CURRENT_DIR).exists());
    }

    #[test]
    fn a_tree_with_no_baseline_and_no_seed_is_refused_with_the_flag_to_pass() {
        let tmp = TempDir::new("ensure-nothing");
        let install = install_dir(&tmp, "install");
        fs::write(app_exe(&install), b"downloaded app bytes").unwrap();

        let error = ensure_baseline(&install, None, &test_pubkey_hex(), false)
            .expect_err("without a seed there is no pair to create");

        assert!(error.contains("--seed-sidecar"), "error: {error}");
        assert!(!generation_dir(&install, CURRENT_DIR).exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_planted_tmp_link_cannot_redirect_an_atomic_write_or_copy() {
        use std::os::unix::fs::symlink;

        let tmp = TempDir::new("atomic-link");
        let install = install_dir(&tmp, "install");
        let victim = install.join("victim.txt");
        fs::write(&victim, b"untouched").unwrap();

        // The old writer's fixed name; planting it as a link used to make the write go through it.
        let target = generation_sidecar(&install, CURRENT_DIR);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        symlink(&victim, target.with_file_name("baseline.json.tmp")).unwrap();
        save_sidecar(
            &target,
            &sidecar_for("2.0.3", b"current bytes", &signing_key()),
        )
        .expect("the sidecar write succeeds under a different temp name");
        assert_eq!(fs::read(&victim).unwrap(), b"untouched");
        assert_eq!(load_sidecar(&target).unwrap().version, "2.0.3");

        // The copy half: a planted `<app>.tmp` must not receive the restored binary.
        fs::write(generation_exe(&install, CURRENT_DIR), b"current bytes").unwrap();
        let app = app_exe(&install);
        symlink(&victim, app.with_file_name("summrise-device.exe.tmp")).unwrap();
        let baseline = match resolve_baseline(&install) {
            BaselineChoice::Current(baseline) => baseline,
            other => panic!("expected Current, got {other:?}"),
        };
        assert_eq!(verify_or_restore(&install, &baseline), Restore::Restored);
        assert_eq!(fs::read(&victim).unwrap(), b"untouched");
        assert_eq!(fs::read(&app).unwrap(), b"current bytes");
    }

    #[test]
    fn restore_previous_into_current_restores_the_whole_pair() {
        let tmp = TempDir::new("restore-previous");
        let install = install_dir(&tmp, "install");
        let previous = write_generation(&install, Generation::Previous, "2.0.2", b"previous bytes");
        write_generation(&install, Generation::Current, "2.0.3", b"current bytes");
        fs::write(generation_exe(&install, CURRENT_DIR), b"corrupt").unwrap();

        let restored = restore_previous_into_current(&install).expect("restore");
        assert_eq!(restored.generation, Generation::Current);
        assert_eq!(restored.sidecar.version, "2.0.2");
        assert_eq!(
            fs::read(generation_exe(&install, CURRENT_DIR)).unwrap(),
            b"previous bytes",
            "the binary came from previous"
        );
        assert_eq!(
            load_sidecar(&generation_sidecar(&install, CURRENT_DIR))
                .unwrap()
                .version,
            "2.0.2",
            "the sidecar came from previous, so the pair agrees"
        );
        assert!(
            generation_dir(&install, PREVIOUS_DIR).is_dir(),
            "the fallback generation is kept, not consumed"
        );
        assert_eq!(previous.sidecar.sha256, restored.sidecar.sha256);
    }

    #[test]
    fn restore_previous_refuses_an_inconsistent_previous() {
        let tmp = TempDir::new("restore-previous-bad");
        let install = install_dir(&tmp, "install");
        write_generation(&install, Generation::Previous, "2.0.2", b"previous bytes");
        fs::write(generation_exe(&install, PREVIOUS_DIR), b"tampered").unwrap();

        assert!(restore_previous_into_current(&install).is_err());
        assert!(
            !generation_dir(&install, CURRENT_DIR).is_dir(),
            "an untrusted generation must not be copied into current"
        );
    }

    #[test]
    fn the_pending_generation_is_written_as_a_pair_and_can_be_committed() {
        let tmp = TempDir::new("pending-commit");
        let install = install_dir(&tmp, "install");
        write_generation(&install, Generation::Current, "2.0.3", b"old bytes");
        let staged = tmp.path().join("staged.exe");
        fs::write(&staged, b"new bytes").unwrap();
        let sidecar = sidecar_for("2.0.4", b"new bytes", &signing_key());

        stage_pending(&install, &staged, &sidecar).expect("stage");
        assert_eq!(
            fs::read(generation_exe(&install, PENDING_DIR)).unwrap(),
            b"new bytes"
        );
        assert_eq!(
            load_sidecar(&generation_sidecar(&install, PENDING_DIR))
                .expect("pending sidecar")
                .version,
            "2.0.4"
        );

        install_pending_over_app(&install, &sidecar).expect("install over app");
        assert_eq!(fs::read(app_exe(&install)).unwrap(), b"new bytes");

        commit_pending(&install).expect("commit");
        assert!(!generation_dir(&install, PENDING_DIR).exists());
        assert_eq!(
            load_sidecar(&generation_sidecar(&install, CURRENT_DIR))
                .unwrap()
                .version,
            "2.0.4"
        );
        assert_eq!(
            fs::read(generation_exe(&install, CURRENT_DIR)).unwrap(),
            b"new bytes"
        );
        assert_eq!(
            load_sidecar(&generation_sidecar(&install, PREVIOUS_DIR))
                .unwrap()
                .version,
            "2.0.3",
            "the generation being replaced became previous"
        );
        assert_eq!(
            fs::read(generation_exe(&install, PREVIOUS_DIR)).unwrap(),
            b"old bytes"
        );
    }

    #[test]
    fn staging_rejects_bytes_that_do_not_match_the_signed_digest() {
        let tmp = TempDir::new("pending-toctou");
        let install = install_dir(&tmp, "install");
        write_generation(&install, Generation::Current, "2.0.3", b"old bytes");
        let staged = tmp.path().join("staged.exe");
        fs::write(&staged, b"changed after verification").unwrap();
        let sidecar = sidecar_for("2.0.4", b"the bytes the signature covers", &signing_key());

        assert!(stage_pending(&install, &staged, &sidecar).is_err());
        assert!(
            !generation_dir(&install, PENDING_DIR).exists(),
            "a mismatched copy must not leave a pending generation behind"
        );
    }

    #[test]
    fn a_rollback_restores_the_pre_update_generation_and_keeps_previous() {
        let tmp = TempDir::new("rollback-generation");
        let install = install_dir(&tmp, "install");
        write_generation(&install, Generation::Previous, "2.0.2", b"older bytes");
        write_generation(&install, Generation::Current, "2.0.3", b"old bytes");
        let staged = tmp.path().join("staged.exe");
        fs::write(&staged, b"new bytes").unwrap();
        let sidecar = sidecar_for("2.0.4", b"new bytes", &signing_key());

        stage_pending(&install, &staged, &sidecar).expect("stage");
        install_pending_over_app(&install, &sidecar).expect("install the candidate");
        assert_eq!(fs::read(app_exe(&install)).unwrap(), b"new bytes");

        // This is what roll_back does after a failed probe: the pending candidate is discarded and
        // the generation that was current before the attempt is installed again.
        discard_pending(&install).expect("discard");
        let restored = restore_current_app(&install).expect("restore");

        assert_eq!(restored.generation, Generation::Current);
        assert_eq!(restored.sidecar.version, "2.0.3");
        assert_eq!(fs::read(app_exe(&install)).unwrap(), b"old bytes");
        assert_eq!(
            load_sidecar(&generation_sidecar(&install, PREVIOUS_DIR))
                .unwrap()
                .version,
            "2.0.2",
            "the older fallback generation is not consumed by the rollback"
        );
    }

    #[test]
    fn discard_pending_removes_an_unproven_generation() {
        let tmp = TempDir::new("pending-discard");
        let install = install_dir(&tmp, "install");
        write_generation(&install, Generation::Current, "2.0.3", b"old bytes");
        let staged = tmp.path().join("staged.exe");
        fs::write(&staged, b"new bytes").unwrap();
        let sidecar = sidecar_for("2.0.4", b"new bytes", &signing_key());
        stage_pending(&install, &staged, &sidecar).expect("stage");

        discard_pending(&install).expect("discard");

        assert!(!generation_dir(&install, PENDING_DIR).exists());
        assert_eq!(
            load_sidecar(&generation_sidecar(&install, CURRENT_DIR))
                .unwrap()
                .version,
            "2.0.3",
            "discarding pending must not promote it"
        );
    }

    #[test]
    fn commit_without_a_pending_generation_is_an_error() {
        let tmp = TempDir::new("commit-missing");
        let install = install_dir(&tmp, "install");
        write_generation(&install, Generation::Current, "2.0.3", b"old bytes");
        assert!(commit_pending(&install).is_err());
    }

    // ─────────────────────────────────────────────────────────────────────────────────────────────
    // Self-repair from the resolved baseline
    // ─────────────────────────────────────────────────────────────────────────────────────────────

    #[test]
    fn restore_copies_the_baseline_when_the_app_is_missing() {
        let tmp = TempDir::new("restore-missing");
        let install = install_dir(&tmp, "install");
        let baseline = write_generation(
            &install,
            Generation::Current,
            "2.0.3",
            b"the trusted baseline",
        );

        assert_eq!(verify_or_restore(&install, &baseline), Restore::Restored);
        assert_eq!(
            fs::read(app_exe(&install)).expect("app exists"),
            b"the trusted baseline"
        );
        assert_eq!(
            sha256_bytes(&fs::read(app_exe(&install)).unwrap()),
            baseline.sidecar.sha256
        );
        assert!(baseline.exe.is_file(), "the baseline is kept, not moved");
    }

    /// I3: a real repair leaves a machine record. The app consumes this log at start; without it,
    /// cutover gate 1's "one deleted-exe repair" evidence does not exist.
    #[test]
    fn a_restored_app_appends_one_repair_record_and_a_healthy_app_appends_none() {
        let tmp = TempDir::new("repair-log");
        let install = install_dir(&tmp, "install");
        let baseline = write_generation(
            &install,
            Generation::Current,
            "2.0.3",
            b"the trusted baseline",
        );

        // The app already matches: nothing was repaired, so nothing is recorded.
        fs::copy(&baseline.exe, app_exe(&install)).expect("the fixture app is the baseline bytes");
        assert_eq!(
            verify_or_restore_at_startup(&install, &baseline),
            Restore::Ok
        );
        assert!(
            !repair_log(&install).exists(),
            "a healthy start must not invent a repair"
        );

        fs::remove_file(app_exe(&install)).expect("the fixture app is deleted");
        assert_eq!(
            verify_or_restore_at_startup(&install, &baseline),
            Restore::Restored
        );

        let text = fs::read_to_string(repair_log(&install)).expect("the repair log is written");
        assert_eq!(text.lines().count(), 1, "one repair, one line: {text}");
        let record: serde_json::Value =
            serde_json::from_str(text.trim()).expect("the repair record is JSON");
        assert_eq!(record["kind"], "repaired");
        assert_eq!(record["from"], "baseline");
        assert_eq!(
            record["sha256"], baseline.sidecar.sha256,
            "the record names the bytes that were restored"
        );
        assert!(
            record["ts_ms"].as_u64().is_some_and(|ts| ts > 0),
            "the record is timestamped: {record}"
        );

        // Append-only: a second repair is a second line, never a rewrite of the first.
        fs::remove_file(app_exe(&install)).expect("the fixture app is deleted again");
        assert_eq!(
            verify_or_restore_at_startup(&install, &baseline),
            Restore::Restored
        );
        let text = fs::read_to_string(repair_log(&install)).expect("the repair log is written");
        assert_eq!(text.lines().count(), 2, "repairs append: {text}");
    }

    /// I3 precedence (round 2): a rollback during a swap restores the app through the same code as
    /// a startup repair, but it is not a self-repair. Its outcome is the swap result ("rolled back
    /// from X"); writing a repair record too would let the repair verdict overwrite the more
    /// specific one on the next start.
    #[test]
    fn a_swap_rollback_restore_writes_no_repair_record() {
        let tmp = TempDir::new("rollback-no-repair");
        let install = install_dir(&tmp, "install");
        let baseline = write_generation(
            &install,
            Generation::Current,
            "2.0.3",
            b"the trusted baseline",
        );
        fs::write(app_exe(&install), b"the failed candidate")
            .expect("the failed candidate is installed over the app");

        let restored =
            restore_current_app(&install).expect("the rollback restores the trusted pair");

        assert_eq!(restored.sidecar.version, baseline.sidecar.version);
        assert_eq!(
            fs::read(app_exe(&install)).expect("the restored app"),
            b"the trusted baseline",
            "the rollback put the trusted bytes back"
        );
        assert!(
            !repair_log(&install).exists(),
            "a swap rollback is recorded by swap.result.json, not by repair.log"
        );
    }

    #[test]
    fn restore_replaces_a_zero_length_app() {
        let tmp = TempDir::new("restore-empty");
        let install = install_dir(&tmp, "install");
        let baseline = write_generation(&install, Generation::Current, "2.0.3", b"baseline bytes");
        fs::write(app_exe(&install), b"").unwrap();

        assert_eq!(verify_or_restore(&install, &baseline), Restore::Restored);
        assert_eq!(fs::read(app_exe(&install)).unwrap(), b"baseline bytes");
    }

    #[test]
    fn restore_replaces_a_corrupt_app() {
        let tmp = TempDir::new("restore-corrupt");
        let install = install_dir(&tmp, "install");
        let baseline = write_generation(&install, Generation::Current, "2.0.3", b"baseline bytes");
        fs::write(app_exe(&install), b"something else entirely").unwrap();

        assert_eq!(verify_or_restore(&install, &baseline), Restore::Restored);
        assert_eq!(fs::read(app_exe(&install)).unwrap(), b"baseline bytes");
    }

    #[test]
    fn matching_app_is_left_alone() {
        let tmp = TempDir::new("restore-ok");
        let install = install_dir(&tmp, "install");
        let baseline = write_generation(&install, Generation::Current, "2.0.3", b"baseline bytes");
        fs::write(app_exe(&install), b"baseline bytes").unwrap();
        let before = fs::metadata(app_exe(&install)).unwrap().modified().ok();

        assert_eq!(verify_or_restore(&install, &baseline), Restore::Ok);
        let after = fs::metadata(app_exe(&install)).unwrap().modified().ok();
        assert_eq!(before, after, "a valid app must not be rewritten");
    }

    #[test]
    fn a_baseline_that_does_not_match_the_sidecar_needs_a_human() {
        let tmp = TempDir::new("baseline-wrong");
        let install = install_dir(&tmp, "install");
        write_generation(
            &install,
            Generation::Current,
            "2.0.3",
            b"actual baseline bytes",
        );
        // The sidecar names different bytes, so the only copy we could repair from is untrusted.
        let sidecar = sidecar_for("2.0.3", b"expected baseline bytes", &signing_key());
        save_sidecar(&generation_sidecar(&install, CURRENT_DIR), &sidecar).unwrap();

        assert_needs_human(resolve_baseline(&install), "hashes to");
        assert!(
            !app_exe(&install).exists(),
            "an untrusted baseline must not be copied over the app"
        );
    }

    #[test]
    fn a_missing_baseline_needs_a_human() {
        let tmp = TempDir::new("baseline-missing");
        let install = install_dir(&tmp, "install");

        assert_needs_human(resolve_baseline(&install), "missing");
        assert!(!app_exe(&install).exists());
    }

    #[test]
    fn restore_works_when_the_install_path_contains_spaces() {
        let tmp = TempDir::new("spaces");
        let install = install_dir(&tmp, "Program Files/Summrise");
        assert!(install.to_string_lossy().contains(' '));
        let baseline = write_generation(
            &install,
            Generation::Current,
            "2.0.3",
            b"baseline in a spaced path",
        );

        assert_eq!(verify_or_restore(&install, &baseline), Restore::Restored);
        assert_eq!(
            fs::read(app_exe(&install)).unwrap(),
            b"baseline in a spaced path"
        );
    }

    #[test]
    fn a_repair_that_cannot_write_tries_three_times_then_needs_a_human() {
        let tmp = TempDir::new("restore-retry");
        let install = install_dir(&tmp, "install");
        let baseline = write_generation(&install, Generation::Current, "2.0.3", b"baseline bytes");
        // A directory at the app path makes every copy attempt fail, without relying on file
        // permissions (which are a no-op for root in CI containers).
        fs::create_dir_all(app_exe(&install)).expect("block the app path");

        match verify_or_restore(&install, &baseline) {
            Restore::NeedsHuman(why) => {
                assert!(
                    why.contains("after 3 attempts"),
                    "retry count is part of the report: {why}"
                );
            }
            other => panic!("expected NeedsHuman, got {other:?}"),
        }
    }

    #[test]
    fn restoring_the_current_app_repairs_an_inconsistent_current_from_previous() {
        let tmp = TempDir::new("restore-current");
        let install = install_dir(&tmp, "install");
        write_generation(&install, Generation::Previous, "2.0.2", b"previous bytes");
        write_generation(&install, Generation::Current, "2.0.3", b"current bytes");
        fs::write(generation_exe(&install, CURRENT_DIR), b"corrupt current").unwrap();

        let baseline = restore_current_app(&install).expect("repair from previous");
        assert_eq!(baseline.generation, Generation::Current);
        assert_eq!(baseline.sidecar.version, "2.0.2");
        assert_eq!(fs::read(app_exe(&install)).unwrap(), b"previous bytes");
    }

    /// The observed gap (2026-10-22): the supervisor booted the previous generation after the
    /// current image was deleted, and **no record said so**. The device came back and the evidence
    /// gate 1 asks for did not exist.
    #[test]
    fn booting_the_previous_generation_records_which_generation_was_booted() {
        let tmp = TempDir::new("boot-previous-record");
        let install = install_dir(&tmp, "install");
        let previous = write_generation(&install, Generation::Previous, "2.0.2", b"previous bytes");
        write_generation(&install, Generation::Current, "2.0.3", b"current bytes");
        fs::remove_file(generation_exe(&install, CURRENT_DIR))
            .expect("the current generation's image is deleted");

        let baseline = restore_from_previous_at_startup(&install).expect("boot previous");
        assert_eq!(baseline.sidecar.version, "2.0.2");

        let text = fs::read_to_string(repair_log(&install)).expect("a record exists");
        assert_eq!(text.lines().count(), 1, "one repair, one line: {text}");
        let record: serde_json::Value =
            serde_json::from_str(text.trim()).expect("the record is JSON");
        assert_eq!(record["kind"], "repaired");
        assert_eq!(
            record["from"], "previous",
            "the record says which generation was booted, not just that something happened"
        );
        assert_eq!(record["sha256"], previous.sidecar.sha256);
    }

    // ─────────────────────────────────────────────────────────────────────────────────────────────
    // The restart budget
    // ─────────────────────────────────────────────────────────────────────────────────────────────

    #[test]
    fn next_action_allows_three_restarts_and_then_gives_up() {
        assert_eq!(next_action(0, false), Action::Start);
        assert_eq!(next_action(0, true), Action::RetryAfterMs(1_000));
        assert_eq!(next_action(1, true), Action::RetryAfterMs(2_000));
        assert_eq!(next_action(2, true), Action::RetryAfterMs(4_000));
        assert_eq!(next_action(3, true), Action::GiveUp);
        assert_eq!(
            next_action(u8::MAX, true),
            Action::GiveUp,
            "the guard must not wrap or index past the table"
        );
    }

    #[test]
    fn the_restart_budget_gives_up_on_consecutive_failures() {
        let mut budget = RestartBudget::new();
        assert_eq!(
            budget.on_child_exit(Duration::from_secs(1)),
            Action::RetryAfterMs(1_000)
        );
        assert_eq!(budget.attempts(), 1);
        assert_eq!(
            budget.on_child_exit(Duration::from_secs(1)),
            Action::RetryAfterMs(2_000)
        );
        assert_eq!(budget.attempts(), 2);
        assert_eq!(
            budget.on_child_exit(Duration::from_secs(1)),
            Action::RetryAfterMs(4_000)
        );
        assert_eq!(budget.attempts(), 3);
        assert_eq!(budget.on_child_exit(Duration::from_secs(1)), Action::GiveUp);
        assert_eq!(budget.attempts(), 3);
    }

    #[test]
    fn the_restart_budget_resets_after_a_stable_child() {
        let mut budget = RestartBudget::new();
        for _ in 0..MAX_RESTARTS {
            assert!(matches!(
                budget.on_child_exit(Duration::from_secs(1)),
                Action::RetryAfterMs(_)
            ));
        }
        assert_eq!(budget.attempts(), 3);
        // The next child lived long enough that its exit starts a new sequence, not give-up.
        assert_eq!(
            budget.on_child_exit(Duration::from_millis(STABLE_AFTER_MS)),
            Action::RetryAfterMs(1_000)
        );
        assert_eq!(budget.attempts(), 1, "a stable life resets the sequence");
        // And a second stable child does it again.
        assert_eq!(
            budget.on_child_exit(Duration::from_secs(120)),
            Action::RetryAfterMs(1_000)
        );
        assert_eq!(budget.attempts(), 1);
    }

    #[test]
    fn the_restart_budget_is_not_reset_one_millisecond_before_stable() {
        let mut budget = RestartBudget::new();
        for _ in 0..MAX_RESTARTS {
            budget.on_child_exit(Duration::from_secs(1));
        }
        assert_eq!(
            budget.on_child_exit(Duration::from_millis(STABLE_AFTER_MS - 1)),
            Action::GiveUp
        );
    }

    // ─────────────────────────────────────────────────────────────────────────────────────────────
    // The request parser and the result shape
    // ─────────────────────────────────────────────────────────────────────────────────────────────

    fn request_bytes(at_ms: u64, version: &str, sha256: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "requested_at_ms": at_ms,
            "version": version,
            "sha256": sha256,
        }))
        .unwrap()
    }

    #[test]
    fn a_fresh_request_parses() {
        let sha = sha256_bytes(b"new release");
        let bytes = request_bytes(1_000_000, "2.0.4", &sha);
        match parse_swap_request(&bytes, 1_000_500) {
            SwapRequestState::Valid(request) => {
                assert_eq!(request.version, "2.0.4");
                assert_eq!(request.sha256, sha);
            }
            other => panic!("expected Valid, got {other:?}"),
        }
    }

    #[test]
    fn a_request_at_the_ten_minute_boundary_is_still_valid() {
        let sha = sha256_bytes(b"new release");
        let bytes = request_bytes(1_000_000, "2.0.4", &sha);
        assert!(matches!(
            parse_swap_request(&bytes, 1_000_000 + SWAP_REQUEST_TIMEOUT_MS),
            SwapRequestState::Valid(_)
        ));
    }

    #[test]
    fn an_old_request_is_refused_and_never_retried() {
        let sha = sha256_bytes(b"new release");
        let bytes = request_bytes(1_000_000, "2.0.4", &sha);
        match parse_swap_request(&bytes, 1_000_000 + SWAP_REQUEST_TIMEOUT_MS + 1) {
            SwapRequestState::TooOld { request, age_ms } => {
                assert_eq!(request.version, "2.0.4");
                assert_eq!(age_ms, SWAP_REQUEST_TIMEOUT_MS + 1);
            }
            other => panic!("expected TooOld, got {other:?}"),
        }
    }

    #[test]
    fn a_garbage_request_is_refused() {
        assert!(matches!(
            parse_swap_request(b"", 1_000_000),
            SwapRequestState::Invalid(_)
        ));
        assert!(matches!(
            parse_swap_request(b"{not json", 1_000_000),
            SwapRequestState::Invalid(_)
        ));
        // Unknown fields are not silently ignored: the request is a contract, not a bag.
        let unknown = br#"{"requested_at_ms":5,"version":"2.0.4","sha256":"0000000000000000000000000000000000000000000000000000000000000000","extra":1}"#;
        assert!(matches!(
            parse_swap_request(unknown, 1_000_000),
            SwapRequestState::Invalid(_)
        ));
        // A version that would make the signed message ambiguous is not a request.
        let newline = serde_json::to_vec(&json!({
            "requested_at_ms": 5,
            "version": "2.0.4\nx",
            "sha256": sha256_bytes(b"x"),
        }))
        .unwrap();
        assert!(matches!(
            parse_swap_request(&newline, 1_000_000),
            SwapRequestState::Invalid(_)
        ));
        // Nor is a malformed digest.
        let bad_sha = request_bytes(5, "2.0.4", "not-a-digest");
        assert!(matches!(
            parse_swap_request(&bad_sha, 1_000_000),
            SwapRequestState::Invalid(_)
        ));
    }

    #[test]
    fn a_missing_request_file_is_missing_not_malformed() {
        let tmp = TempDir::new("request-missing");
        assert_eq!(
            read_swap_request(&tmp.path().join("swap.request.json"), 1_000_000),
            SwapRequestState::Missing
        );
    }

    #[test]
    fn the_result_json_is_the_documented_contract() {
        let sha256 = "ab".repeat(32);
        let applied = serde_json::to_string(&SwapResult::applied("2.0.4", &sha256, 42)).unwrap();
        assert_eq!(
            applied,
            format!(r#"{{"outcome":"applied","version":"2.0.4","sha256":"{sha256}","at_ms":42}}"#)
        );

        let rolled_back = serde_json::to_string(&SwapResult::rolled_back(
            "2.0.4",
            &sha256,
            43,
            "health failed",
        ))
        .unwrap();
        assert_eq!(
            rolled_back,
            format!(
                r#"{{"outcome":"rolled_back","version":"2.0.4","sha256":"{sha256}","at_ms":43,"reason":"health failed"}}"#
            )
        );

        let refused = serde_json::to_string(&SwapResult::refused(44, "request was old")).unwrap();
        assert_eq!(
            refused, r#"{"outcome":"refused","at_ms":44,"reason":"request was old"}"#,
            "a request that never reached a verified pair has no digest to name"
        );

        let refused_verified = serde_json::to_string(&SwapResult::refused_verified(
            &sha256,
            45,
            "the request named another version",
        ))
        .unwrap();
        assert_eq!(
            refused_verified,
            format!(
                r#"{{"outcome":"refused","sha256":"{sha256}","at_ms":45,"reason":"the request named another version"}}"#
            ),
            "a refusal after verification names the artifact it did not install"
        );
    }

    #[test]
    fn a_result_round_trips_through_its_file() {
        let tmp = TempDir::new("result-file");
        let path = tmp.path().join("swap.result.json");
        let result = SwapResult::applied("2.0.4", "cd".repeat(32), 42);
        write_swap_result(&path, &result).expect("write");
        let parsed: SwapResult = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(parsed, result);
    }

    // ─────────────────────────────────────────────────────────────────────────────────────────────
    // Health: the body, not the status line
    // ─────────────────────────────────────────────────────────────────────────────────────────────

    fn health_response(version: &str, pid: u32) -> String {
        let body = format!(r#"{{"status":"ok","version":"{version}","pid":{pid}}}"#);
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[test]
    fn health_json_proves_the_expected_build() {
        let response = health_response("2.0.4", 4321);
        let document = parse_health_response(response.as_bytes(), "2.0.4", 4321).expect("healthy");
        assert_eq!(document.status, "ok");
        assert_eq!(document.version, "2.0.4");
        assert_eq!(document.pid, 4321);
    }

    #[test]
    fn health_json_with_the_wrong_version_is_not_proof() {
        let response = health_response("2.0.3", 4321);
        match parse_health_response(response.as_bytes(), "2.0.4", 4321) {
            Err(HealthError::WrongVersion { expected, got }) => {
                assert_eq!(expected, "2.0.4");
                assert_eq!(got, "2.0.3");
            }
            other => panic!("expected WrongVersion, got {other:?}"),
        }
    }

    #[test]
    fn health_json_with_the_wrong_pid_is_not_proof() {
        let response = health_response("2.0.4", 1111);
        match parse_health_response(response.as_bytes(), "2.0.4", 4321) {
            Err(HealthError::WrongPid { expected, got }) => {
                assert_eq!(expected, 4321);
                assert_eq!(got, 1111);
            }
            other => panic!("expected WrongPid, got {other:?}"),
        }
    }

    #[test]
    fn health_json_with_a_non_ok_status_is_not_proof() {
        let body = r#"{"status":"degraded","version":"2.0.4","pid":1}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        assert!(matches!(
            parse_health_response(response.as_bytes(), "2.0.4", 1),
            Err(HealthError::NotOk(status)) if status == "degraded"
        ));
    }

    #[test]
    fn a_non_200_health_answer_is_not_proof() {
        let body = r#"{"status":"ok","version":"2.0.4","pid":1}"#;
        let response = format!(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        assert!(matches!(
            parse_health_response(response.as_bytes(), "2.0.4", 1),
            Err(HealthError::Status(503))
        ));
    }

    #[test]
    fn a_non_json_health_body_is_not_proof() {
        let body = "OK";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        assert!(matches!(
            parse_health_response(response.as_bytes(), "2.0.4", 1),
            Err(HealthError::Malformed(_))
        ));
    }

    fn serve_health(responses: Vec<String>) -> (u16, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(_) => break,
                };
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (port, server)
    }

    #[test]
    fn two_matching_health_answers_prove_the_build() {
        let (port, server) = serve_health(vec![
            health_response("2.0.4", 4321),
            health_response("2.0.4", 4321),
        ]);
        assert!(wait_healthy(
            port,
            "2.0.4",
            4321,
            Duration::from_secs(2),
            || false
        ));
        server.join().unwrap();
    }

    #[test]
    fn one_health_answer_is_not_two_consecutive_probes() {
        let (port, server) = serve_health(vec![health_response("2.0.4", 4321)]);
        assert!(
            !wait_healthy(port, "2.0.4", 4321, Duration::from_millis(700), || false),
            "a single 200 must not satisfy the two-probe requirement"
        );
        server.join().unwrap();
    }

    #[test]
    fn a_stop_interrupts_the_health_wait() {
        // Port 1 never answers; the stop must end the wait without spending its ten seconds.
        let started = Instant::now();
        assert!(!wait_healthy(
            1,
            "2.0.4",
            4321,
            Duration::from_secs(10),
            || true
        ));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the stop returned in {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn health_answers_from_the_wrong_build_never_pass() {
        let (port, server) = serve_health(vec![
            health_response("2.0.3", 4321),
            health_response("2.0.3", 4321),
        ]);
        assert!(!wait_healthy(
            port,
            "2.0.4",
            4321,
            Duration::from_millis(700),
            || false
        ));
        server.join().unwrap();
    }

    #[test]
    fn the_install_token_is_64_hex_characters_and_a_newline() {
        let zeroes = token_text(&[0u8; TOKEN_BYTES]);
        assert_eq!(
            zeroes.len(),
            65,
            "32 bytes become 64 hex characters plus a newline"
        );
        assert_eq!(zeroes, format!("{}\n", "0".repeat(64)));
        assert_eq!(zeroes.as_bytes()[64], b'\n');
        assert!(zeroes.as_bytes()[..64]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));

        let mut bytes = [0u8; TOKEN_BYTES];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = (index as u8).wrapping_mul(7).wrapping_add(1);
        }
        let text = token_text(&bytes);
        assert_eq!(text.len(), 65);
        assert!(
            text[..64].bytes().all(|byte| byte.is_ascii_hexdigit()),
            "the token is lower-hex: {text:?}"
        );
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn the_install_token_is_minted_only_when_absent() {
        assert_eq!(token_action(false), TokenAction::Create);
        assert_eq!(
            token_action(true),
            TokenAction::Keep,
            "an existing token must not be rewritten (the installer is idempotent)"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────────────────────────
    // Keys, sidecars and configuration (unchanged contracts)
    // ─────────────────────────────────────────────────────────────────────────────────────────────

    #[test]
    fn a_valid_hex_key_parses() {
        let key = signing_key();
        let hex = hex_encode(&key.verifying_key().to_bytes());
        assert_eq!(
            parse_pubkey(&hex).expect("the test key is a valid curve point"),
            key.verifying_key()
        );
    }

    #[test]
    fn a_malformed_key_is_a_typed_error() {
        assert_eq!(parse_pubkey("zz"), Err(SigError::BadPubkey));
        assert_eq!(parse_pubkey("0011"), Err(SigError::BadPubkey));
        assert_eq!(parse_pubkey("0"), Err(SigError::BadPubkey));
    }

    #[test]
    fn the_placeholder_key_is_refused_by_name() {
        assert_eq!(
            parse_pubkey(PLACEHOLDER_PUBKEY_HEX),
            Err(SigError::Placeholder),
            "a placeholder build must say so, not report a malformed key"
        );
    }

    #[test]
    fn the_build_time_key_seam_is_what_the_binary_embeds() {
        match option_env!("SUMMRISE_LAUNCHER_PUBKEY") {
            Some(key) => assert_eq!(LAUNCHER_PUBKEY_HEX, key),
            None => {
                assert_eq!(LAUNCHER_PUBKEY_HEX, PLACEHOLDER_PUBKEY_HEX);
                assert_eq!(
                    parse_pubkey(LAUNCHER_PUBKEY_HEX),
                    Err(SigError::Placeholder)
                );
            }
        }
    }

    #[test]
    fn a_genuine_sidecar_verifies() {
        let sidecar = sidecar_for("2.0.3", b"binary bytes", &signing_key());
        assert_eq!(
            verify_signature(&sidecar, &signing_key().verifying_key()),
            Ok(())
        );
    }

    #[test]
    fn a_tampered_version_does_not_verify() {
        let mut sidecar = sidecar_for("2.0.3", b"binary bytes", &signing_key());
        sidecar.version = "2.0.4".to_string();
        assert_eq!(
            verify_signature(&sidecar, &signing_key().verifying_key()),
            Err(SigError::BadSignature)
        );
    }

    #[test]
    fn a_tampered_channel_does_not_verify() {
        let mut sidecar = sidecar_for("2.0.3", b"binary bytes", &signing_key());
        sidecar.channel = "canary".to_string();
        assert_eq!(
            verify_signature(&sidecar, &signing_key().verifying_key()),
            Err(SigError::BadSignature),
            "the channel is part of the signed message"
        );
    }

    #[test]
    fn a_channel_with_a_newline_is_refused() {
        let mut sidecar = sidecar_for("2.0.3", b"binary bytes", &signing_key());
        sidecar.channel = "stable\nand-more".to_string();
        assert!(matches!(
            verify_signature(&sidecar, &signing_key().verifying_key()),
            Err(SigError::MalformedChannel(_))
        ));
    }

    #[test]
    fn a_tampered_hash_does_not_verify() {
        let mut sidecar = sidecar_for("2.0.3", b"binary bytes", &signing_key());
        let mut sha = sidecar.sha256.clone();
        sha.replace_range(0..1, if sha.starts_with('0') { "1" } else { "0" });
        sidecar.sha256 = sha;
        assert_eq!(
            verify_signature(&sidecar, &signing_key().verifying_key()),
            Err(SigError::BadSignature)
        );
    }

    #[test]
    fn another_key_does_not_verify() {
        let sidecar = sidecar_for("2.0.3", b"binary bytes", &signing_key());
        let other = SigningKey::from_bytes(&OTHER_SEED);
        assert_eq!(
            verify_signature(&sidecar, &other.verifying_key()),
            Err(SigError::BadSignature)
        );
    }

    #[test]
    fn a_malformed_signature_is_named_as_such() {
        let mut sidecar = sidecar_for("2.0.3", b"binary bytes", &signing_key());
        sidecar.signature = "zz".to_string();
        assert!(matches!(
            verify_signature(&sidecar, &signing_key().verifying_key()),
            Err(SigError::MalformedSignature(_))
        ));
    }

    #[test]
    fn a_malformed_hash_is_named_as_such() {
        let mut sidecar = sidecar_for("2.0.3", b"binary bytes", &signing_key());
        sidecar.sha256 = "not-a-digest".to_string();
        assert!(matches!(
            verify_signature(&sidecar, &signing_key().verifying_key()),
            Err(SigError::MalformedSha(_))
        ));
    }

    #[test]
    fn a_version_with_a_newline_is_refused() {
        let mut sidecar = sidecar_for("2.0.3", b"binary bytes", &signing_key());
        sidecar.version = "2.0.3\nand-more".to_string();
        assert!(matches!(
            verify_signature(&sidecar, &signing_key().verifying_key()),
            Err(SigError::MalformedVersion(_))
        ));
    }

    #[test]
    fn sidecar_round_trips_through_json() {
        let tmp = TempDir::new("sidecar");
        let path = tmp.path().join("sidecar.json");
        let sidecar = sidecar_for("2.0.3", b"binary bytes", &signing_key());
        save_sidecar(&path, &sidecar).expect("save");
        assert_eq!(load_sidecar(&path).expect("load"), sidecar);
    }

    #[test]
    fn a_staged_pair_passes_its_signature_and_digest_checks() {
        let tmp = TempDir::new("staged-pair");
        let exe = tmp.path().join("summrise-device.new.exe");
        let sidecar_path = tmp.path().join("summrise-device.new.json");
        fs::write(&exe, b"the new build").unwrap();
        let sidecar = sidecar_for("2.0.4", b"the new build", &signing_key());
        save_sidecar(&sidecar_path, &sidecar).unwrap();
        let pubkey = hex_encode(signing_key().verifying_key().as_bytes());

        let pair = validate_staged_pair(&exe, &sidecar_path, &pubkey).expect("valid pair");
        assert_eq!(pair.sidecar.version, "2.0.4");
        assert_eq!(pair.exe, exe);
    }

    #[test]
    fn a_staged_pair_with_a_bad_digest_is_refused_before_it_is_used() {
        let tmp = TempDir::new("staged-bad-digest");
        let exe = tmp.path().join("summrise-device.new.exe");
        let sidecar_path = tmp.path().join("summrise-device.new.json");
        fs::write(&exe, b"the new build").unwrap();
        let sidecar = sidecar_for("2.0.4", b"different bytes", &signing_key());
        save_sidecar(&sidecar_path, &sidecar).unwrap();
        let pubkey = hex_encode(signing_key().verifying_key().as_bytes());

        let error = validate_staged_pair(&exe, &sidecar_path, &pubkey).unwrap_err();
        assert!(error.contains("does not match"), "{error}");
    }

    #[test]
    fn an_unkeyed_build_refuses_a_staged_pair_by_name() {
        let tmp = TempDir::new("staged-placeholder");
        let exe = tmp.path().join("summrise-device.new.exe");
        let sidecar_path = tmp.path().join("summrise-device.new.json");
        fs::write(&exe, b"the new build").unwrap();
        let sidecar = sidecar_for("2.0.4", b"the new build", &signing_key());
        save_sidecar(&sidecar_path, &sidecar).unwrap();

        let error = validate_staged_pair(&exe, &sidecar_path, PLACEHOLDER_PUBKEY_HEX).unwrap_err();
        assert!(error.contains("placeholder release key"), "{error}");
    }

    /// C1, the other direction: the launcher key owns the first generation, the release key owns
    /// every update. A staged sidecar the app wrote from a release-key-signed manifest therefore
    /// cannot be accepted under the launcher key, and a sidecar signed with the launcher key cannot
    /// pass the staged check.
    #[test]
    fn a_staged_sidecar_signed_with_the_launcher_key_does_not_verify_with_the_release_key() {
        let tmp = TempDir::new("staged-launcher-key");
        let exe = tmp.path().join("summrise-device.new.exe");
        let sidecar_path = tmp.path().join("summrise-device.new.json");
        fs::write(&exe, b"the new build").unwrap();
        let launcher_key = SigningKey::from_bytes(&TEST_SEED);
        let release_key = SigningKey::from_bytes(&OTHER_SEED);
        let sidecar = sidecar_for("2.0.4", b"the new build", &launcher_key);
        save_sidecar(&sidecar_path, &sidecar).unwrap();
        let release_hex = hex_encode(release_key.verifying_key().as_bytes());

        let error = validate_staged_pair(&exe, &sidecar_path, &release_hex)
            .expect_err("a launcher-key-signed update must not pass the staged check");
        assert!(
            error.contains("signature does not verify"),
            "the staged check must report a signature, not a digest: {error}"
        );

        // The same pair signed by the release key is the one the staged check accepts: the refusal
        // above is the key, not the artifact.
        save_sidecar(
            &sidecar_path,
            &sidecar_for("2.0.4", b"the new build", &release_key),
        )
        .unwrap();
        assert!(validate_staged_pair(&exe, &sidecar_path, &release_hex).is_ok());
    }

    /// The release key is its own compile-time seam, so unkeyed builds fail closed on updates even
    /// when the launcher key is configured (and the seed baseline still works).
    #[test]
    fn the_staged_check_uses_the_release_key_seam() {
        match option_env!("SUMMRISE_RELEASE_PUBKEY") {
            Some(key) => assert_eq!(RELEASE_PUBKEY_HEX, key),
            None => {
                assert_eq!(RELEASE_PUBKEY_HEX, PLACEHOLDER_PUBKEY_HEX);
                let tmp = TempDir::new("staged-release-seam");
                let exe = tmp.path().join("summrise-device.new.exe");
                let sidecar_path = tmp.path().join("summrise-device.new.json");
                fs::write(&exe, b"the new build").unwrap();
                let sidecar = sidecar_for("2.0.4", b"the new build", &signing_key());
                save_sidecar(&sidecar_path, &sidecar).unwrap();

                let error = validate_staged_update(&exe, &sidecar_path).unwrap_err();
                assert!(
                    error.contains("placeholder release key"),
                    "an unkeyed launcher must refuse updates by name: {error}"
                );
            }
        }
    }

    #[test]
    fn port_defaults_to_7737() {
        let config = parse_config(&[], Path::new("/opt/Summrise"), None).unwrap();
        assert_eq!(config.port, DEFAULT_PORT);
        assert_eq!(config.data, Path::new("/opt/Summrise/data"));
    }

    #[test]
    fn port_comes_from_the_environment_when_no_flag_is_given() {
        let config = parse_config(&[], Path::new("/opt/Summrise"), Some("19000")).unwrap();
        assert_eq!(config.port, 19000);
    }

    #[test]
    fn the_port_flag_beats_the_environment() {
        let args = vec!["--port".to_string(), "19001".to_string()];
        let config = parse_config(&args, Path::new("/opt/Summrise"), Some("19000")).unwrap();
        assert_eq!(config.port, 19001);
    }

    #[test]
    fn install_and_data_flags_are_read() {
        let args = vec![
            "--install".to_string(),
            "/srv/Summrise".to_string(),
            "--data".to_string(),
            "/var/lib/summrise".to_string(),
        ];
        let config = parse_config(&args, Path::new("/opt/Summrise"), None).unwrap();
        assert_eq!(config.install, Path::new("/srv/Summrise"));
        assert_eq!(config.data, Path::new("/var/lib/summrise"));
    }

    #[test]
    fn an_invalid_port_is_an_error() {
        let args = vec!["--port".to_string(), "not-a-port".to_string()];
        assert!(parse_config(&args, Path::new("/opt/Summrise"), None).is_err());
        let args = vec!["--port".to_string(), "0".to_string()];
        assert!(parse_config(&args, Path::new("/opt/Summrise"), None).is_err());
    }

    #[test]
    fn the_service_name_defaults_to_the_installers_name() {
        let args = vec![
            "--install".to_string(),
            "/srv/Summrise".to_string(),
            "--port".to_string(),
            "19001".to_string(),
        ];
        let (name, flags) = parse_service_args(&args, "SummriseLaunch").unwrap();
        assert_eq!(name, "SummriseLaunch");
        assert_eq!(flags, args, "nothing but the name flag is consumed here");
    }

    /// C3: a supervised install must be armable without starting a second app. The launcher takes
    /// the two console flags itself, persists them beside the install token, and the app reads that
    /// file; the service argv does not need them.
    #[test]
    fn the_console_flags_are_split_from_the_launcher_args_and_persisted() {
        let args = vec![
            "--install".to_string(),
            "/srv/Summrise".to_string(),
            "--data".to_string(),
            "/var/lib/summrise".to_string(),
            "--console-url".to_string(),
            "https://console.example.test".to_string(),
            "--console-token-file".to_string(),
            "/var/lib/summrise/console-token.json".to_string(),
        ];

        let (rest, link) = split_console_args(&args).expect("the console flags parse");
        assert_eq!(
            rest,
            vec![
                "--install".to_string(),
                "/srv/Summrise".to_string(),
                "--data".to_string(),
                "/var/lib/summrise".to_string(),
            ],
            "the app's own flags are what is left for parse_config"
        );
        let link = link.expect("both flags make a console link");
        assert_eq!(link.url.as_deref(), Some("https://console.example.test"));
        assert_eq!(
            link.token_file.as_deref(),
            Some(Path::new("/var/lib/summrise/console-token.json"))
        );
        assert_eq!(
            (link.mesh_broker, link.mesh_relay, link.mesh_stun),
            (None, None, None),
            "an install with no mesh writes no mesh facts"
        );
    }

    /// A supervised install may join the mesh without a console, and vice versa: one file carries
    /// both, and only the facts the operator named.
    #[test]
    fn the_mesh_flags_are_split_out_and_persisted_without_a_console() {
        let args = vec![
            "--install".to_string(),
            "/srv/Summrise".to_string(),
            "--data".to_string(),
            "/var/lib/summrise".to_string(),
            "--mesh-broker".to_string(),
            "http://127.0.0.1:8080".to_string(),
            "--mesh-relay".to_string(),
            "127.0.0.1:9000".to_string(),
            "--mesh-stun".to_string(),
            "203.0.113.10:3478".to_string(),
        ];

        let (rest, link) = split_console_args(&args).expect("the mesh flags parse");
        assert_eq!(
            rest,
            vec![
                "--install".to_string(),
                "/srv/Summrise".to_string(),
                "--data".to_string(),
                "/var/lib/summrise".to_string(),
            ],
            "the mesh flags reach the file, not the service argv"
        );
        let link = link.expect("the mesh facts make a link");
        assert_eq!(link.mesh_broker.as_deref(), Some("http://127.0.0.1:8080"));
        assert_eq!(link.mesh_relay.as_deref(), Some("127.0.0.1:9000"));
        assert_eq!(
            link.mesh_stun.as_deref(),
            Some("203.0.113.10:3478"),
            "the reflector reaches the file, not the service argv"
        );
        assert_eq!(
            (link.url, link.token_file),
            (None, None),
            "a mesh-only install names no console"
        );
    }

    #[test]
    fn a_mesh_relay_without_a_broker_is_refused() {
        let args = vec!["--mesh-relay".to_string(), "127.0.0.1:9000".to_string()];
        let error =
            split_console_args(&args).expect_err("a relay with no rendezvous can never be reached");
        assert!(error.contains("--mesh-broker"), "{error}");
    }

    #[test]
    fn a_mesh_stun_without_a_broker_is_refused() {
        let args = vec!["--mesh-stun".to_string(), "203.0.113.10:3478".to_string()];
        let error = split_console_args(&args)
            .expect_err("a reflector with nowhere to publish its address is a half-fact");
        assert!(error.contains("--mesh-broker"), "{error}");
    }

    #[test]
    fn a_mesh_broker_without_a_relay_is_a_direct_only_link() {
        let args = vec![
            "--mesh-broker".to_string(),
            "http://127.0.0.1:8080".to_string(),
        ];
        let (rest, link) = split_console_args(&args).expect("a broker alone is complete");
        assert!(rest.is_empty());
        let link = link.expect("a broker alone makes a link");
        assert_eq!(link.mesh_broker.as_deref(), Some("http://127.0.0.1:8080"));
        assert_eq!(link.mesh_relay, None);
        assert_eq!(link.mesh_stun, None);
    }

    /// The launcher refuses a broker the rendezvous client would refuse later: the installer would
    /// otherwise register a service that can only fail at startup, with the failure in the service
    /// log rather than in the operator's `--install-service` output.
    #[test]
    fn a_mesh_broker_that_is_not_an_http_origin_is_refused() {
        for broker in [
            "",
            "127.0.0.1:8080",
            "https://broker.example.test",
            "ftp://broker.example.test",
            "http://",
            "http://broker.example.test/path",
            "http://broker.example.test?query",
        ] {
            let args = vec!["--mesh-broker".to_string(), broker.to_string()];
            let error = split_console_args(&args)
                .expect_err("a broker origin the rendezvous cannot use is refused");
            assert!(
                error.contains("--mesh-broker"),
                "{broker:?} must be refused by the flag that names it: {error}"
            );
        }
    }

    /// The relay is a socket address in the app, so the launcher refuses the same spellings rather
    /// than persisting one that can only fail when the service next starts.
    #[test]
    fn a_mesh_relay_that_is_not_a_socket_address_is_refused() {
        let args = vec![
            "--mesh-broker".to_string(),
            "http://127.0.0.1:8080".to_string(),
            "--mesh-relay".to_string(),
            "not-an-address".to_string(),
        ];
        let error = split_console_args(&args)
            .expect_err("a relay the app cannot dial is refused before it is persisted");
        assert!(error.contains("--mesh-relay"), "{error}");
    }

    /// The reflector is a socket address in the app, so the launcher refuses the same spellings
    /// rather than persisting one that can only fail when the service next starts.
    #[test]
    fn a_mesh_stun_that_is_not_a_socket_address_is_refused() {
        let args = vec![
            "--mesh-broker".to_string(),
            "http://127.0.0.1:8080".to_string(),
            "--mesh-stun".to_string(),
            "not-an-address".to_string(),
        ];
        let error = split_console_args(&args)
            .expect_err("a reflector the app cannot dial is refused before it is persisted");
        assert!(error.contains("--mesh-stun"), "{error}");
    }

    /// An unknown flag is not a launcher flag and must still reach `parse_config`, which refuses it
    /// by name. Splitting the remote flags must not become a place unknown arguments disappear.
    #[test]
    fn an_unknown_flag_still_reaches_parse_config_and_is_refused() {
        let args = vec![
            "--install".to_string(),
            "/srv/Summrise".to_string(),
            "--mesh-broker".to_string(),
            "http://127.0.0.1:8080".to_string(),
            "--bogus".to_string(),
            "value".to_string(),
        ];

        let (rest, link) = split_console_args(&args).expect("the known flags parse");
        assert!(link.is_some(), "the mesh flag is still persisted");
        let error = parse_config(&rest, Path::new("/opt/Summrise"), None)
            .expect_err("the unknown flag is not a launcher flag");
        assert!(error.contains("--bogus"), "{error}");
    }

    #[test]
    fn a_lone_console_flag_is_a_configuration_error() {
        let args = vec![
            "--console-url".to_string(),
            "https://console.example.test".to_string(),
        ];
        let error =
            split_console_args(&args).expect_err("a URL without a token file is incomplete");
        assert!(error.contains("--console-token-file"), "{error}");
    }

    #[test]
    fn the_service_argv_names_the_install_data_and_port_the_app_is_given() {
        use std::ffi::OsString;

        let install = Path::new("/srv/Summrise");
        let data = Path::new("/var/lib/summrise");
        let argv = service_launch_arguments(install, data, 19_001);
        assert_eq!(
            argv,
            vec![
                OsString::from("service"),
                OsString::from("--install"),
                install.as_os_str().to_os_string(),
                OsString::from("--data"),
                data.as_os_str().to_os_string(),
                OsString::from("--port"),
                OsString::from("19001"),
            ]
        );
        assert_eq!(
            console_link(data),
            Path::new("/var/lib/summrise/console.json"),
            "the console link travels with the data directory the argv names"
        );
    }

    #[test]
    fn the_app_child_argv_carries_the_release_facts_from_the_booted_sidecar() {
        use std::ffi::OsString;

        let config = Config {
            install: PathBuf::from("/srv/Summrise"),
            data: PathBuf::from("/var/lib/summrise"),
            port: 19_001,
        };
        let sidecar = Sidecar {
            version: "2.0.3".to_owned(),
            channel: "canary".to_owned(),
            sha256: "b1946ac92492d2347c6235b4d2611184".to_owned(),
            signature: "00".repeat(64),
        };

        let argv = app_arguments(&config, &sidecar);

        assert_eq!(
            argv,
            vec![
                OsString::from("--install"),
                config.install.as_os_str().to_os_string(),
                OsString::from("--data"),
                config.data.as_os_str().to_os_string(),
                OsString::from("--port"),
                OsString::from("19001"),
                OsString::from("--release-version"),
                OsString::from("2.0.3"),
                OsString::from("--release-sha"),
                OsString::from("b1946ac92492d2347c6235b4d2611184"),
                OsString::from("--release-channel"),
                OsString::from("canary"),
                OsString::from("--token-file"),
                install_token(&config.data).as_os_str().to_os_string(),
            ],
            "the app records one release identity: version, digest and channel from the sidecar \
             the launcher actually booted"
        );
    }

    #[test]
    fn a_mesh_only_link_is_written_without_console_fields() {
        let tmp = TempDir::new("mesh-link");
        let data = tmp.path().join("data");
        let link = ConsoleLink {
            url: None,
            token_file: None,
            mesh_broker: Some("http://127.0.0.1:8080".to_string()),
            mesh_relay: Some("127.0.0.1:9000".to_string()),
            mesh_stun: Some("203.0.113.10:3478".to_string()),
            files_shared_root: None,
        };

        write_console_link(&data, &link).expect("the mesh link is writable");

        let written = fs::read_to_string(console_link(&data)).expect("the link is readable");
        let parsed: serde_json::Value = serde_json::from_str(&written).expect("the link is JSON");
        assert_eq!(parsed["mesh_broker"], "http://127.0.0.1:8080");
        assert_eq!(parsed["mesh_relay"], "127.0.0.1:9000");
        assert_eq!(parsed["mesh_stun"], "203.0.113.10:3478");
        assert!(
            parsed.get("url").is_none() && parsed.get("token_file").is_none(),
            "a mesh-only install names no console: {written}"
        );
        let round_trip: ConsoleLink =
            serde_json::from_str(&written).expect("the link the app reads is this type");
        assert_eq!(
            round_trip, link,
            "the file round-trips through the same type"
        );
    }

    /// The `--install-service` link path: the three mesh flags are split from the launcher argv,
    /// prepared for persistence, and written together. This is the same pure path the Windows
    /// installer calls before registering the service.
    #[test]
    fn an_install_service_link_with_three_mesh_flags_writes_all_three() {
        let args = vec![
            "--mesh-broker".to_string(),
            "http://127.0.0.1:8080".to_string(),
            "--mesh-relay".to_string(),
            "127.0.0.1:9000".to_string(),
            "--mesh-stun".to_string(),
            "203.0.113.10:3478".to_string(),
        ];
        let (rest, link) = split_console_args(&args).expect("the three mesh flags parse");
        assert!(rest.is_empty(), "the launcher owns all three flags");
        let link = link.expect("three mesh flags make a link");

        let tmp = TempDir::new("install-service-three-mesh");
        let data = tmp.path().join("data");
        let link = link_to_persist(&data, &link).expect("the link is prepared");
        write_console_link(&data, &link).expect("the link is written");

        let written: ConsoleLink = serde_json::from_str(
            &fs::read_to_string(console_link(&data)).expect("the link is readable"),
        )
        .expect("the link is the launcher's type");
        assert_eq!(
            written.mesh_broker.as_deref(),
            Some("http://127.0.0.1:8080")
        );
        assert_eq!(written.mesh_relay.as_deref(), Some("127.0.0.1:9000"));
        assert_eq!(written.mesh_stun.as_deref(), Some("203.0.113.10:3478"));
    }

    #[test]
    fn a_files_shared_root_is_split_and_persisted() {
        let args = vec![
            "--files-shared-root".to_string(),
            "/srv/summrise/shared".to_string(),
        ];
        let (rest, link) = split_console_args(&args).expect("the files root parses");
        assert!(rest.is_empty(), "the launcher owns the files flag");
        let link = link.expect("a files root alone is a link to persist");
        assert_eq!(
            link.files_shared_root.as_deref(),
            Some(std::path::Path::new("/srv/summrise/shared"))
        );

        let tmp = TempDir::new("files-shared-link");
        let data = tmp.path().join("data");
        let prepared = link_to_persist(&data, &link).expect("the root is made absolute");
        write_console_link(&data, &prepared).expect("the link is written");
        let written: ConsoleLink = serde_json::from_str(
            &fs::read_to_string(console_link(&data)).expect("the link is readable"),
        )
        .expect("the link is the launcher's type");
        assert_eq!(
            written.files_shared_root.as_deref(),
            Some(std::path::Path::new("/srv/summrise/shared"))
        );
    }

    /// A rewrite that names the mesh but not a reflector must carry the installed one forward:
    /// dropping it would silently disable the direct path the previous install configured.
    #[test]
    fn a_mesh_rewrite_keeps_the_persisted_stun_reflector() {
        let tmp = TempDir::new("mesh-stun-preserve");
        let data = tmp.path().join("data");
        let installed = ConsoleLink {
            url: None,
            token_file: None,
            mesh_broker: Some("http://127.0.0.1:8080".to_string()),
            mesh_relay: Some("127.0.0.1:9000".to_string()),
            mesh_stun: Some("203.0.113.10:3478".to_string()),
            files_shared_root: None,
        };
        write_console_link(&data, &installed).expect("the installed link is writable");

        let rewrite = ConsoleLink {
            url: None,
            token_file: None,
            mesh_broker: Some("http://127.0.0.1:9090".to_string()),
            mesh_relay: Some("127.0.0.1:9001".to_string()),
            mesh_stun: None,
            files_shared_root: None,
        };
        let prepared = link_to_persist(&data, &rewrite).expect("the rewrite is prepared");
        assert_eq!(
            prepared.mesh_stun.as_deref(),
            Some("203.0.113.10:3478"),
            "the reflector survives a broker/relay rewrite"
        );
        write_console_link(&data, &prepared).expect("the rewrite is writable");
        let written: ConsoleLink = serde_json::from_str(
            &fs::read_to_string(console_link(&data)).expect("the link is readable"),
        )
        .expect("the link is the launcher's type");
        assert_eq!(written.mesh_stun.as_deref(), Some("203.0.113.10:3478"));

        let replaced = ConsoleLink {
            mesh_stun: Some("198.51.100.20:3478".to_string()),
            ..rewrite
        };
        let prepared = link_to_persist(&data, &replaced).expect("the replacement is prepared");
        assert_eq!(
            prepared.mesh_stun.as_deref(),
            Some("198.51.100.20:3478"),
            "a named reflector replaces the installed one"
        );
    }

    /// A console-only rewrite must not graft a reflector onto a link with no broker: the app would
    /// refuse the file as a half-fact.
    #[test]
    fn a_console_only_rewrite_does_not_graft_a_mesh_stun() {
        let tmp = TempDir::new("mesh-stun-no-graft");
        let data = tmp.path().join("data");
        write_console_link(
            &data,
            &ConsoleLink {
                url: None,
                token_file: None,
                mesh_broker: Some("http://127.0.0.1:8080".to_string()),
                mesh_relay: None,
                mesh_stun: Some("203.0.113.10:3478".to_string()),
                files_shared_root: None,
            },
        )
        .expect("the installed link is writable");

        let rewrite = ConsoleLink {
            url: Some("https://console.example.test".to_string()),
            token_file: Some(data.join("console-token.json")),
            mesh_broker: None,
            mesh_relay: None,
            mesh_stun: None,
            files_shared_root: None,
        };
        let prepared = link_to_persist(&data, &rewrite).expect("the rewrite is prepared");
        assert_eq!(prepared.mesh_stun, None);
    }

    /// Every combination of the two features is written as the file describes: the fields the
    /// operator named are present, and every absent field is omitted rather than written as null.
    #[test]
    fn every_combination_of_remote_facts_is_written_with_only_the_named_fields() {
        let console = [
            "--console-url",
            "https://console.example.test",
            "--console-token-file",
            "/var/lib/summrise/console-token.json",
        ];
        let mesh = [
            "--mesh-broker",
            "http://127.0.0.1:8080",
            "--mesh-relay",
            "127.0.0.1:9000",
            "--mesh-stun",
            "203.0.113.10:3478",
        ];
        let cases: [(&str, Vec<&str>, &[&str]); 5] = [
            ("console only", console.to_vec(), &["url", "token_file"]),
            (
                "mesh only",
                mesh.to_vec(),
                &["mesh_broker", "mesh_relay", "mesh_stun"],
            ),
            (
                "console and mesh",
                console.iter().chain(mesh.iter()).copied().collect(),
                &[
                    "url",
                    "token_file",
                    "mesh_broker",
                    "mesh_relay",
                    "mesh_stun",
                ],
            ),
            (
                "broker without relay",
                vec!["--mesh-broker", "http://127.0.0.1:8080"],
                &["mesh_broker"],
            ),
            (
                "broker and reflector without relay",
                vec![
                    "--mesh-broker",
                    "http://127.0.0.1:8080",
                    "--mesh-stun",
                    "203.0.113.10:3478",
                ],
                &["mesh_broker", "mesh_stun"],
            ),
        ];

        for (label, args, expected) in cases {
            let args: Vec<String> = args.into_iter().map(str::to_owned).collect();
            let (rest, link) = split_console_args(&args).expect(label);
            assert!(rest.is_empty(), "{label}: the launcher owns these flags");
            let link = link.unwrap_or_else(|| panic!("{label}: a named fact makes a link"));

            let tmp = TempDir::new(&format!("link-{}", label.replace(' ', "-")));
            let data = tmp.path().join("data");
            write_console_link(&data, &link).expect(label);
            let written = fs::read_to_string(console_link(&data)).expect(label);
            let parsed: serde_json::Value = serde_json::from_str(&written).expect(label);

            let mut keys: Vec<&str> = parsed
                .as_object()
                .expect(label)
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            let mut expected = expected.to_vec();
            expected.sort_unstable();
            assert_eq!(
                keys, expected,
                "{label}: absent facts are omitted: {written}"
            );
        }
    }

    #[test]
    fn the_console_link_is_written_beside_the_install_token() {
        let tmp = TempDir::new("console-link");
        let data = tmp.path().join("data");
        let link = ConsoleLink {
            url: Some("https://console.example.test".to_string()),
            token_file: Some(data.join("console-token.json")),
            mesh_broker: None,
            mesh_relay: None,
            mesh_stun: None,
            files_shared_root: None,
        };

        write_console_link(&data, &link).expect("the console link is writable");

        let path = console_link(&data);
        let written = fs::read_to_string(&path).expect("the console link is readable");
        let parsed: serde_json::Value =
            serde_json::from_str(&written).expect("the console link is JSON");
        assert_eq!(parsed["url"], "https://console.example.test");
        assert_eq!(
            parsed["token_file"],
            serde_json::json!(link
                .token_file
                .as_deref()
                .expect("the fixture names a token file")
                .to_string_lossy()),
            "the file names the token file the console registration was written to"
        );
        assert!(
            parsed.get("mesh_broker").is_none()
                && parsed.get("mesh_relay").is_none()
                && parsed.get("mesh_stun").is_none(),
            "an install with no mesh writes no mesh fact at all, not null: {written}"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path)
                .expect("the link's metadata")
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "the console token path is a credential; the link must be owner-only"
            );
        }
    }

    #[test]
    fn a_foreign_registration_names_its_service_for_the_dispatcher() {
        // A service created by `sc.exe` under another name must appear in the dispatcher's table by
        // that name, or StartServiceCtrlDispatcher refuses the start with ERROR_SERVICE_NOT_IN_EXE.
        let args = vec![
            "--service-name".to_string(),
            "SummriseDeviceTest".to_string(),
            "--install".to_string(),
            r"C:\Summrise".to_string(),
            "--data".to_string(),
            r"C:\Summrise\data".to_string(),
            "--port".to_string(),
            "19002".to_string(),
        ];
        let (name, flags) = parse_service_args(&args, "SummriseLaunch").unwrap();
        assert_eq!(name, "SummriseDeviceTest");
        assert_eq!(
            flags,
            vec![
                "--install".to_string(),
                r"C:\Summrise".to_string(),
                "--data".to_string(),
                r"C:\Summrise\data".to_string(),
                "--port".to_string(),
                "19002".to_string(),
            ]
        );
        // And the flags that remain are the ones parse_config accepts.
        let config = parse_config(&flags, Path::new("/opt/Summrise"), None).unwrap();
        assert_eq!(config.install, Path::new(r"C:\Summrise"));
        assert_eq!(config.port, 19002);
    }

    #[test]
    fn a_service_name_flag_without_a_value_is_an_error() {
        let args = vec!["--service-name".to_string()];
        assert!(parse_service_args(&args, "SummriseLaunch").is_err());
    }

    /// A valid state document carrying exactly one rolled-back artifact.
    fn state_document_with_rolled_back(sha256: &str, version: &str, at_ms: u64) -> String {
        json!({
            "schema": 1,
            "install": {
                "dir": r"C:\Summrise",
                "version": "1.0.0",
                "channel": "stable",
            },
            "baseline": {
                "version": "1.0.0",
                "sha256": "0".repeat(64),
                "exe": r"C:\Summrise\baseline\current\summrise-device.exe",
            },
            "service": {
                "state": "stopped",
                "pid": 0,
                "startedAt": "2023-11-14T22:13:20Z",
            },
            "lastVerdict": {
                "ts": "2023-11-14T22:13:20Z",
                "outcome": "needs_attention",
                "detail": "rolled back from 2.0.3",
            },
            "sessions": [],
            "update": {"phase": "idle", "to": null},
            "rolledBack": [{
                "sha256": sha256,
                "version": version,
                "atMs": at_ms,
            }],
        })
        .to_string()
    }

    #[test]
    fn the_launcher_reads_the_rollback_memory_from_the_state_document() {
        let tmp = TempDir::new("rollback-memory");
        let data = tmp.path().join("data");
        fs::create_dir_all(&data).expect("create the data directory");
        let sha256 = "ab".repeat(32);
        fs::write(
            data.join(STATE_FILE),
            state_document_with_rolled_back(&sha256, "2.0.3", 1_700_000_000_000),
        )
        .expect("the fixture state is writable");

        let found = remembered_rollback(&data, &sha256.to_ascii_uppercase())
            .expect("the state document is readable")
            .expect("the digest is in the memory");
        assert_eq!(found.sha256, sha256);
        assert_eq!(found.version, "2.0.3");
        assert_eq!(found.at_ms, 1_700_000_000_000);
        assert_eq!(
            remembered_rollback(&data, &"cd".repeat(32)).expect("the state is readable"),
            None,
            "a digest the device did not roll back is not remembered"
        );
    }

    #[test]
    fn a_missing_state_document_is_reported_not_treated_as_an_empty_memory() {
        let tmp = TempDir::new("rollback-missing");
        let data = tmp.path().join("data");
        fs::create_dir_all(&data).expect("create the data directory");

        let error = remembered_rollback(&data, &"ab".repeat(32))
            .expect_err("a missing state document is not an empty rollback list");
        assert!(
            error.contains(STATE_FILE) && error.contains(&data.display().to_string()),
            "the refusal names the document and the directory: {error}"
        );
    }

    #[test]
    fn the_operator_override_is_appended_to_the_audit_log() {
        let tmp = TempDir::new("operator-audit");
        let data = tmp.path().join("data");
        let sha256 = "ef".repeat(32);

        append_operator_rollback_event(&data, "2.0.3", &sha256, 1_700_000_000_000)
            .expect("the audit log is writable");

        let text =
            fs::read_to_string(data.join("events.jsonl")).expect("the audit line is on disk");
        let line: serde_json::Value =
            serde_json::from_str(text.lines().next().expect("the audit line exists"))
                .expect("the audit line is JSON");
        assert_eq!(line["event"], "operator_apply_rolled_back");
        assert_eq!(line["version"], "2.0.3");
        assert_eq!(line["sha256"], json!(sha256));
        assert_eq!(line["rolled_back_at_ms"], 1_700_000_000_000u64);
        assert!(
            line["ts_ms"].as_u64().is_some_and(|ts| ts > 0),
            "the audit line carries when the operator overrode the memory: {line}"
        );
    }

    #[test]
    fn a_service_name_that_would_swallow_the_next_flag_is_an_error() {
        // `--service-name --install …` must not name the service `--install` and then lose the
        // install directory; the flag's value is a name, not a flag.
        let args = vec!["--service-name".to_string(), "--install".to_string()];
        assert!(parse_service_args(&args, "SummriseLaunch").is_err());
        let empty = vec!["--service-name".to_string(), String::new()];
        assert!(parse_service_args(&empty, "SummriseLaunch").is_err());
    }

    #[test]
    fn an_installer_upgrade_replaces_the_generation_and_keeps_the_old_one_as_previous() {
        let tmp = TempDir::new("ensure-replace");
        let install = install_dir(&tmp, "install");
        fs::write(app_exe(&install), b"the app bytes").unwrap();
        let first = tmp.path().join("seed-1.json");
        save_sidecar(
            &first,
            &sidecar_for("0.1.0", b"the app bytes", &signing_key()),
        )
        .unwrap();
        let outcome = ensure_baseline(&install, Some(&first), &test_pubkey_hex(), false)
            .expect("the first install seeds");
        assert_eq!(outcome, BaselineInstall::Seeded);

        // The installer wrote a new sidecar for the same bytes and asked for a replacement.
        let second = tmp.path().join("seed-2.json");
        save_sidecar(
            &second,
            &sidecar_for("0.2.0", b"the app bytes", &signing_key()),
        )
        .unwrap();
        let outcome = ensure_baseline(&install, Some(&second), &test_pubkey_hex(), true)
            .expect("an installer upgrade replaces the generation");
        assert_eq!(outcome, BaselineInstall::Replaced);

        let current = load_sidecar(&generation_sidecar(&install, CURRENT_DIR)).unwrap();
        assert_eq!(
            current.version, "0.2.0",
            "current is the version just installed"
        );
        let previous = load_sidecar(&generation_sidecar(&install, PREVIOUS_DIR))
            .expect("the replaced generation is kept as previous, so a rollback has a target");
        assert_eq!(previous.version, "0.1.0");
    }

    #[test]
    fn a_replace_without_a_signed_sidecar_is_refused() {
        let tmp = TempDir::new("ensure-replace-refused");
        let install = install_dir(&tmp, "install");
        fs::write(app_exe(&install), b"the app bytes").unwrap();
        let error = ensure_baseline(&install, None, &test_pubkey_hex(), true)
            .expect_err("a replacement needs the sidecar that names the new generation");
        assert!(error.contains("--replace-baseline"), "{error}");
    }

    #[test]
    fn a_replace_on_a_fresh_tree_is_a_seed_not_a_replacement() {
        let tmp = TempDir::new("ensure-replace-fresh");
        let install = install_dir(&tmp, "install");
        fs::write(app_exe(&install), b"fresh app bytes").unwrap();
        let seed = tmp.path().join("seed.json");
        save_sidecar(
            &seed,
            &sidecar_for("1.0.0", b"fresh app bytes", &signing_key()),
        )
        .unwrap();
        let outcome = ensure_baseline(&install, Some(&seed), &test_pubkey_hex(), true)
            .expect("a fresh tree with the installer's pair seeds");
        assert_eq!(
            outcome,
            BaselineInstall::Seeded,
            "there was no generation to replace, so the log must not claim one was rotated"
        );
        assert!(
            !generation_dir(&install, PREVIOUS_DIR).exists(),
            "a fresh install leaves no previous generation behind"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────────────────────────
    // Versioned directories and the current pointer (2026-11-05 design)
    // ─────────────────────────────────────────────────────────────────────────────────────────────

    /// A version directory as `--install-version` leaves it: the app binary beside its sidecar.
    fn write_version(install: &Path, name: &str, bytes: &[u8]) -> Version {
        let dir = version_dir(install, name);
        fs::create_dir_all(&dir).expect("create version dir");
        fs::write(version_exe(install, name), bytes).expect("write version exe");
        let sidecar = sidecar_for(name, bytes, &signing_key());
        save_sidecar(&version_sidecar(install, name), &sidecar).expect("write version sidecar");
        Version {
            name: name.to_string(),
            dir,
            exe: version_exe(install, name),
            sidecar_path: version_sidecar(install, name),
            sidecar,
        }
    }

    /// A candidate pair outside the install, as the stager leaves it for `--apply`.
    fn stage_candidate(tmp: &TempDir, name: &str, bytes: &[u8]) -> (PathBuf, PathBuf, StagedPair) {
        let dir = tmp.path().join("staged").join(name);
        fs::create_dir_all(&dir).expect("create staged dir");
        let exe = dir.join(APP_EXE);
        fs::write(&exe, bytes).expect("write staged exe");
        let sidecar = sidecar_for(name, bytes, &signing_key());
        let sidecar_path = dir.join(VERSION_SIDECAR);
        save_sidecar(&sidecar_path, &sidecar).expect("write staged sidecar");
        let pair = validate_staged_pair(&exe, &sidecar_path, &test_pubkey_hex())
            .expect("the test candidate verifies");
        (exe, sidecar_path, pair)
    }

    struct Probe(Result<(), String>);

    impl VersionProbe for Probe {
        fn probe(&self, _config: &Config, _version: &Version) -> Result<(), String> {
            match &self.0 {
                Ok(()) => Ok(()),
                Err(reason) => Err(reason.clone()),
            }
        }
    }

    fn config_for(install: &Path) -> Config {
        Config {
            install: install.to_path_buf(),
            data: install.join("data"),
            port: 7_737,
        }
    }

    #[test]
    fn a_pointer_naming_a_version_directory_makes_that_directory_the_app() {
        let tmp = TempDir::new("pointer-locate");
        let install = install_dir(&tmp, "install");
        write_version(&install, "0.3.274", b"the 0.3.274 build");

        for spelled in ["versions/0.3.274\n", "0.3.274\n", "versions\\0.3.274\r\n"] {
            fs::write(current_pointer(&install), spelled).expect("write the pointer");
            let app = locate_app(&install).expect("the pointer resolves");
            assert_eq!(
                app.exe(),
                version_exe(&install, "0.3.274"),
                "{spelled:?} must name versions/0.3.274"
            );
            assert_eq!(app.sidecar().version, "0.3.274");
            assert!(!app.is_legacy());
        }
    }

    #[test]
    fn a_missing_pointer_falls_back_to_the_flat_app() {
        let tmp = TempDir::new("pointer-legacy");
        let install = install_dir(&tmp, "install");
        write_generation(&install, Generation::Current, "0.3.271", b"legacy bytes");
        fs::write(app_exe(&install), b"legacy bytes").expect("copy the flat app");
        assert!(
            !current_pointer(&install).exists(),
            "an un-migrated install has no pointer"
        );

        let app = locate_app(&install).expect("the legacy install still resolves");
        assert_eq!(app.exe(), app_exe(&install));
        assert_eq!(app.sidecar().version, "0.3.271");
        assert!(app.is_legacy());
    }

    #[test]
    fn a_pointer_naming_a_missing_version_directory_is_refused_not_silently_ignored() {
        let tmp = TempDir::new("pointer-missing-version");
        let install = install_dir(&tmp, "install");
        fs::write(current_pointer(&install), "0.3.274\n").expect("write the pointer");
        fs::write(app_exe(&install), b"a stale flat app").expect("copy the flat app");

        let error =
            locate_app(&install).expect_err("a named but missing version is not the flat app");
        assert!(error.contains("0.3.274"), "{error}");
        assert!(
            error.contains("missing") || error.contains("version directory"),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_version_directory_that_is_a_symlink_is_refused() {
        use std::os::unix::fs::symlink;

        let tmp = TempDir::new("pointer-symlink");
        let install = install_dir(&tmp, "install");
        let real = tmp.path().join("real-version");
        fs::create_dir_all(&real).expect("the real version directory");
        fs::write(real.join(APP_EXE), b"redirected bytes").expect("the real exe");
        save_sidecar(
            &real.join(VERSION_SIDECAR),
            &sidecar_for("0.3.274", b"redirected bytes", &signing_key()),
        )
        .expect("the real sidecar");
        fs::create_dir_all(versions_dir(&install)).expect("versions dir");
        symlink(&real, version_dir(&install, "0.3.274")).expect("the link");
        fs::write(current_pointer(&install), "0.3.274\n").expect("the pointer");

        let error = locate_app(&install).expect_err("a linked version directory is not a version");
        assert!(error.contains("symbolic link"), "{error}");
    }

    #[test]
    fn a_pointer_that_escapes_the_versions_directory_is_refused() {
        let tmp = TempDir::new("pointer-escape");
        let install = install_dir(&tmp, "install");
        for spelled in [
            "../outside\n",
            "versions/../outside\n",
            "/etc/passwd\n",
            "\n",
        ] {
            fs::write(current_pointer(&install), spelled).expect("write the pointer");
            let error = locate_app(&install).expect_err("an escaping pointer is unusable");
            assert!(
                error.contains("current.txt"),
                "{spelled:?} must be refused as a pointer: {error}"
            );
        }
    }

    #[test]
    fn a_candidate_that_fails_its_health_check_leaves_the_pointer_alone() {
        let tmp = TempDir::new("install-version-probe-fails");
        let install = install_dir(&tmp, "install");
        write_version(&install, "0.3.271", b"the running build");
        write_current_pointer(&install, "0.3.271").expect("write the pointer");
        let (_exe, _sidecar, pair) = stage_candidate(&tmp, "0.3.274", b"the unproven build");

        let error = install_version(
            &config_for(&install),
            "0.3.274",
            &pair,
            &Probe(Err("it never answered".to_string())),
        )
        .expect_err("a candidate that cannot serve is not installed");
        assert!(error.contains("health"), "{error}");
        assert_eq!(
            read_current_pointer(&install),
            Pointer::Version("0.3.271".to_string()),
            "the pointer is untouched on a failed probe"
        );
    }

    #[test]
    fn a_healthy_candidate_becomes_current_and_the_old_version_is_kept() {
        let tmp = TempDir::new("install-version-ok");
        let install = install_dir(&tmp, "install");
        write_version(&install, "0.3.271", b"the running build");
        write_current_pointer(&install, "0.3.271").expect("write the pointer");
        let (_exe, _sidecar, pair) = stage_candidate(&tmp, "0.3.274", b"the proven build");

        let installed = install_version(&config_for(&install), "0.3.274", &pair, &Probe(Ok(())))
            .expect("a healthy candidate is installed");
        assert_eq!(installed.version.name, "0.3.274");
        assert_eq!(installed.previous.as_deref(), Some("0.3.271"));
        assert_eq!(
            read_current_pointer(&install),
            Pointer::Version("0.3.274".to_string())
        );
        assert_eq!(
            read_previous_pointer(&install),
            Pointer::Version("0.3.271".to_string()),
            "the pointer to the version that was current before the update"
        );
        assert_eq!(
            fs::read(version_exe(&install, "0.3.274")).expect("the installed exe"),
            b"the proven build"
        );
        assert!(
            version_exe(&install, "0.3.271").exists(),
            "the previous version stays on disk for the rollback"
        );
    }

    #[test]
    fn installing_the_same_name_never_overwrites_a_version_directory() {
        let tmp = TempDir::new("install-version-collision");
        let install = install_dir(&tmp, "install");
        write_version(&install, "0.3.274", b"the running build");
        write_current_pointer(&install, "0.3.274").expect("write the pointer");
        let (_exe, _sidecar, pair) = stage_candidate(&tmp, "0.3.274", b"different bytes");

        let error = install_version(&config_for(&install), "0.3.274", &pair, &Probe(Ok(())))
            .expect_err("an existing version directory is immutable");
        assert!(error.contains("already"), "{error}");
        assert_eq!(
            fs::read(version_exe(&install, "0.3.274")).expect("the running exe"),
            b"the running build"
        );
    }

    /// The OS effects of "install, then hand the process over": counts the restart, records what
    /// `current.txt` named when the restart was requested, and returns the health verdict the
    /// restarted process gave.
    struct RestartingOwner {
        install: PathBuf,
        health: Result<(), String>,
        restarts: std::cell::Cell<u32>,
        health_checks: std::cell::Cell<u32>,
        pointer_at_restart: RefCell<Option<(Pointer, Pointer)>>,
    }

    impl RestartingOwner {
        fn new(install: &Path, health: Result<(), String>) -> RestartingOwner {
            RestartingOwner {
                install: install.to_path_buf(),
                health,
                restarts: std::cell::Cell::new(0),
                health_checks: std::cell::Cell::new(0),
                pointer_at_restart: RefCell::new(None),
            }
        }
    }

    impl MigrationHost for RestartingOwner {
        fn restart(&self, _config: &Config) -> Result<u32, String> {
            self.restarts.set(self.restarts.get() + 1);
            *self.pointer_at_restart.borrow_mut() = Some((
                read_current_pointer(&self.install),
                read_previous_pointer(&self.install),
            ));
            Ok(4_242)
        }

        fn require_healthy(&self, _config: &Config, version: &str, pid: u32) -> Result<(), String> {
            self.health_checks.set(self.health_checks.get() + 1);
            assert_eq!(
                pid, 4_242,
                "health is asked about the process the restart started"
            );
            assert_eq!(
                version, "0.3.274",
                "health is asked about the version installed"
            );
            self.health.clone()
        }
    }

    #[test]
    fn the_restart_after_an_install_boots_the_version_the_pointer_names() {
        let tmp = TempDir::new("install-version-restart");
        let install = install_dir(&tmp, "install");
        write_version(&install, "0.3.271", b"the running build");
        write_current_pointer(&install, "0.3.271").expect("write the pointer");
        let (_exe, _sidecar, pair) = stage_candidate(&tmp, "0.3.274", b"the proven build");
        let owner = RestartingOwner::new(&install, Ok(()));

        let installed = install_version_and_restart(
            &config_for(&install),
            "0.3.274",
            &pair,
            &Probe(Ok(())),
            &owner,
        )
        .expect("a proven candidate the restarted process reports is installed");

        assert_eq!(installed.previous.as_deref(), Some("0.3.271"));
        assert_eq!(
            owner.restarts.get(),
            1,
            "the owner is restarted exactly once"
        );
        assert_eq!(
            owner.health_checks.get(),
            1,
            "the restarted process is required to answer"
        );
        let (current, previous) = owner
            .pointer_at_restart
            .borrow()
            .clone()
            .expect("the restart happened");
        assert_eq!(
            current,
            Pointer::Version("0.3.274".to_string()),
            "the pointer moves before the restart, so the restart boots the new version"
        );
        assert_eq!(previous, Pointer::Version("0.3.271".to_string()));
    }

    #[test]
    fn a_restart_that_reports_the_old_version_is_a_failure_and_the_commit_is_left_for_rollback() {
        let tmp = TempDir::new("install-version-restart-old");
        let install = install_dir(&tmp, "install");
        write_version(&install, "0.3.271", b"the running build");
        write_current_pointer(&install, "0.3.271").expect("write the pointer");
        let (_exe, _sidecar, pair) = stage_candidate(&tmp, "0.3.274", b"the proven build");
        let owner = RestartingOwner::new(
            &install,
            Err("the restarted app reports 0.3.271, not 0.3.274".to_string()),
        );

        let error = install_version_and_restart(
            &config_for(&install),
            "0.3.274",
            &pair,
            &Probe(Ok(())),
            &owner,
        )
        .expect_err("a service that still reports the old version is not a successful install");

        assert!(error.contains("0.3.271"), "{error}");
        assert_eq!(
            read_current_pointer(&install),
            Pointer::Version("0.3.274".to_string()),
            "the commit is left where it was: the operator's rollback is one pointer write"
        );
        assert_eq!(
            read_previous_pointer(&install),
            Pointer::Version("0.3.271".to_string()),
            "previous.txt still names the version that was running"
        );
        assert!(
            version_exe(&install, "0.3.271").is_file(),
            "the old version stays on disk for that rollback"
        );
        assert_eq!(
            owner.restarts.get(),
            1,
            "the failed restart is not repeated behind the operator's back"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────────────────────────
    // The candidate probe and the files the running app owns (2026-11-05)
    // ─────────────────────────────────────────────────────────────────────────────────────────────

    fn entry_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("the directory is readable")
            .map(|entry| {
                entry
                    .expect("a directory entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    /// A candidate host that behaves like the real app for the one thing the probe needs: it writes
    /// its own lock/pid/port into the probe directory and answers `/api/health` twice.
    struct RecordingCandidate {
        pid: u32,
        seen_at_start: RefCell<Option<Vec<String>>>,
        seen_at_stop: RefCell<Option<Vec<String>>>,
        server: RefCell<Option<std::thread::JoinHandle<()>>>,
    }

    impl RecordingCandidate {
        fn new(pid: u32) -> RecordingCandidate {
            RecordingCandidate {
                pid,
                seen_at_start: RefCell::new(None),
                seen_at_stop: RefCell::new(None),
                server: RefCell::new(None),
            }
        }
    }

    impl CandidateHost for RecordingCandidate {
        fn start(&self, config: &Config, version: &Version) -> Result<u32, String> {
            *self.seen_at_start.borrow_mut() = Some(entry_names(&config.data));
            // The real candidate creates these itself; the probe directory must be empty of the
            // live app's copies so there is nothing stale to inherit.
            fs::write(config.data.join("device.lock"), "the candidate's own lock")
                .map_err(|error| format!("the fake candidate cannot take its lock: {error}"))?;
            fs::write(config.data.join("device.pid"), self.pid.to_string())
                .map_err(|error| format!("the fake candidate cannot record its pid: {error}"))?;
            fs::write(config.data.join("device.port"), config.port.to_string())
                .map_err(|error| format!("the fake candidate cannot record its port: {error}"))?;
            let listener = TcpListener::bind(("127.0.0.1", config.port))
                .map_err(|error| format!("the fake candidate cannot bind: {error}"))?;
            let version = version.sidecar.version.clone();
            let pid = self.pid;
            *self.server.borrow_mut() = Some(std::thread::spawn(move || {
                for _ in 0..HEALTH_CONSECUTIVE_PROBES {
                    let Ok((mut stream, _)) = listener.accept() else {
                        return;
                    };
                    let mut request = [0u8; 1024];
                    let _ = stream.read(&mut request);
                    let _ = stream.write_all(health_response(&version, pid).as_bytes());
                }
            }));
            Ok(pid)
        }

        fn stop(&self, config: &Config, _pid: u32) {
            *self.seen_at_stop.borrow_mut() = Some(entry_names(&config.data));
            if let Some(server) = self.server.borrow_mut().take() {
                let _ = server.join();
            }
        }
    }

    #[test]
    fn the_probe_copy_skips_exactly_the_running_apps_files() {
        let tmp = TempDir::new("probe-copy");
        let live = tmp.path().join("data");
        fs::create_dir_all(live.join("workspace")).expect("the live data directory");
        for owned in RUNNING_APP_FILES {
            fs::write(live.join(owned), owned).expect("the running app's file");
        }
        fs::write(live.join("state.json"), b"{}").expect("the state document");
        fs::write(live.join("token"), b"the install token").expect("the token");
        fs::write(
            live.join("workspace").join("device.lock"),
            b"a plugin's file",
        )
        .expect("a nested file with an owned name");

        let probe = tmp.path().join("probe");
        copy_probe_data(&live, &probe).expect("a live data directory copies");

        assert_eq!(
            entry_names(&probe),
            vec![
                "state.json".to_string(),
                "token".to_string(),
                "workspace".to_string()
            ],
            "only the app-owned entries are left behind"
        );
        for owned in RUNNING_APP_FILES {
            assert!(
                !probe.join(owned).exists(),
                "{owned} was copied out of the live data directory"
            );
        }
        assert!(
            probe.join("workspace").join("device.lock").is_file(),
            "an owned name deeper in the tree belongs to a workspace, not to the app"
        );
        // The list is the one source of truth; these are the four names it must carry, read from
        // device/device/src/lock.rs, main.rs and restarts.rs.
        assert_eq!(
            RUNNING_APP_FILES,
            ["device.lock", "device.pid", "device.port", "restarts.jsonl"],
            "the excluded set is exactly the files the running app owns"
        );
    }

    #[test]
    fn a_probe_succeeds_while_the_live_data_directory_is_locked() {
        let tmp = TempDir::new("probe-locked");
        let live = tmp.path().join("data");
        fs::create_dir_all(&live).expect("the live data directory");
        fs::write(live.join("state.json"), b"{}").expect("the state document");
        fs::write(live.join("token"), b"the install token").expect("the token");
        fs::write(live.join("restarts.jsonl"), b"{\"pid\":1}\n").expect("the boot journal");
        fs::write(live.join("device.pid"), "1\n").expect("the live pid");
        fs::write(live.join("device.port"), "7737\n").expect("the live port");
        let live_lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(live.join("device.lock"))
            .expect("the live lock file");
        live_lock.lock().expect("the live app holds its lock");

        let install = install_dir(&tmp, "install");
        let bytes = b"the candidate build";
        let version = Version {
            name: "0.3.276".to_string(),
            dir: version_dir(&install, "0.3.276"),
            exe: version_exe(&install, "0.3.276"),
            sidecar_path: version_sidecar(&install, "0.3.276"),
            sidecar: sidecar_for("0.3.276", bytes, &signing_key()),
        };
        let config = Config {
            install: install.clone(),
            data: live.clone(),
            port: 7_737,
        };
        let host = RecordingCandidate::new(4_321);

        probe_candidate(&config, &version, &host)
            .expect("a locked live device.lock must not stop the candidate probe");

        let seen = host
            .seen_at_start
            .borrow()
            .clone()
            .expect("the candidate was started");
        for owned in RUNNING_APP_FILES {
            assert!(
                !seen.iter().any(|name| name == owned),
                "the candidate inherited the live {owned}: {seen:?}"
            );
        }
        assert!(
            seen.iter().any(|name| name == "state.json"),
            "the candidate still gets the data it needs to serve: {seen:?}"
        );
        let stopped = host
            .seen_at_stop
            .borrow()
            .clone()
            .expect("the candidate was stopped");
        for own in ["device.lock", "device.pid", "device.port"] {
            assert!(
                stopped.iter().any(|name| name == own),
                "the candidate created its own {own}: {stopped:?}"
            );
        }
        assert!(
            !stopped.iter().any(|name| name == "restarts.jsonl"),
            "the candidate did not inherit the live boot journal: {stopped:?}"
        );
        let second = fs::File::open(live.join("device.lock")).expect("the live lock file opens");
        assert!(
            second.try_lock().is_err(),
            "the live lock is still held exclusively while the probe runs"
        );
    }

    #[test]
    fn the_upgrade_mode_accepts_the_installers_launcher_signed_pair() {
        let tmp = TempDir::new("installer-signed-version");
        let staged = tmp.path().join("versions").join("0.3.276");
        fs::create_dir_all(&staged).expect("the staged version directory");
        let bytes = b"the installer's app";
        let exe = staged.join(APP_EXE);
        fs::write(&exe, bytes).expect("the staged app");
        let sidecar_path = staged.join(VERSION_SIDECAR);
        save_sidecar(
            &sidecar_path,
            &sidecar_for("0.3.276", bytes, &signing_key()),
        )
        .expect("the installer's sidecar");
        let pubkey = test_pubkey_hex();

        let pair = validate_install_version_with(&exe, &sidecar_path, &[&pubkey])
            .expect("the launcher key that signs the installer's pair is accepted");
        assert_eq!(pair.sidecar.version, "0.3.276");

        // A pair signed by a key this build does not carry is still refused by name.
        let other = SigningKey::from_bytes(&[9u8; 32]);
        save_sidecar(&sidecar_path, &sidecar_for("0.3.276", bytes, &other))
            .expect("a foreign sidecar");
        let error = validate_install_version_with(&exe, &sidecar_path, &[&pubkey])
            .expect_err("a foreign signature is refused");
        assert!(error.contains("signature"), "{error}");
    }

    #[test]
    fn a_pointer_write_is_a_temp_file_then_a_rename() {
        let tmp = TempDir::new("pointer-atomic");
        let install = install_dir(&tmp, "install");
        write_current_pointer(&install, "0.3.271").expect("write the old pointer");

        let staged = stage_current_pointer(&install, "0.3.274").expect("stage the new pointer");
        assert_eq!(
            read_current_pointer(&install),
            Pointer::Version("0.3.271".to_string()),
            "the staged write is not visible before its rename"
        );
        drop(staged); // the crash between the temp write and the rename

        assert_eq!(
            read_current_pointer(&install),
            Pointer::Version("0.3.271".to_string()),
            "a crash before the rename leaves the old pointer readable"
        );
        assert!(
            !fs::read_dir(&install).expect("the install").any(|entry| {
                entry
                    .expect("a directory entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".current.txt.")
            }),
            "the dropped stage removes its temporary file"
        );

        let staged = stage_current_pointer(&install, "0.3.274").expect("stage again");
        staged.commit().expect("commit the rename");
        assert_eq!(
            read_current_pointer(&install),
            Pointer::Version("0.3.274".to_string())
        );
    }

    #[test]
    fn two_failed_starts_of_the_current_version_roll_the_pointer_back() {
        let tmp = TempDir::new("pointer-rollback");
        let install = install_dir(&tmp, "install");
        write_version(&install, "0.3.271", b"the known-good build");
        write_version(&install, "0.3.274", b"the build that will not start");
        write_current_pointer(&install, "0.3.274").expect("write the pointer");
        write_previous_pointer(&install, "0.3.271").expect("write the previous pointer");

        let mut watch = StartWatch::new();
        assert!(watch
            .on_failed_start(&install)
            .expect("the first failure is not rollback-worthy")
            .is_none());
        assert_eq!(watch.failed_starts(), 1);
        assert_eq!(
            read_current_pointer(&install),
            Pointer::Version("0.3.274".to_string()),
            "one failed start is not a pattern"
        );

        let rolled = watch
            .on_failed_start(&install)
            .expect("the second failure rolls back")
            .expect("a previous version was recorded");
        assert_eq!(rolled.from.as_deref(), Some("0.3.274"));
        assert_eq!(rolled.to, "0.3.271");
        assert_eq!(watch.failed_starts(), 0, "the rollback resets the count");
        assert_eq!(
            read_current_pointer(&install),
            Pointer::Version("0.3.271".to_string())
        );
        assert_eq!(
            read_previous_pointer(&install),
            Pointer::Missing,
            "the previous pointer is consumed, so a second rollback cannot bounce back"
        );
    }

    #[test]
    fn a_stable_start_forgets_the_failures_before_it() {
        let tmp = TempDir::new("pointer-stable");
        let install = install_dir(&tmp, "install");
        write_version(&install, "0.3.271", b"the known-good build");
        write_version(&install, "0.3.274", b"the current build");
        write_current_pointer(&install, "0.3.274").expect("write the pointer");
        write_previous_pointer(&install, "0.3.271").expect("write the previous pointer");

        let mut watch = StartWatch::new();
        watch.on_failed_start(&install).expect("recorded");
        watch.on_stable();
        assert_eq!(watch.failed_starts(), 0);
        assert!(
            watch.on_failed_start(&install).expect("recorded").is_none(),
            "a failure after a stable run is the first of a new run"
        );
        assert_eq!(
            read_current_pointer(&install),
            Pointer::Version("0.3.274".to_string())
        );
    }

    // ─────────────────────────────────────────────────────────────────────────────────────────────
    // The one-time migration from a flat install (2026-11-05 design)
    // ─────────────────────────────────────────────────────────────────────────────────────────────

    /// A pre-versioned install: the flat app the supervisor runs, beside the baseline generation
    /// the launcher repairs it from. This is exactly the tree the installer leaves before the
    /// migration step.
    fn flat_install(tmp: &TempDir, version: &str, bytes: &[u8]) -> PathBuf {
        let install = install_dir(tmp, "install");
        fs::write(app_exe(&install), bytes).expect("write the flat app");
        write_generation(&install, Generation::Current, version, bytes);
        install
    }

    /// The OS effects of a migration, without an SCM: one restart counter and a health verdict.
    struct MigrationProbe {
        healthy: bool,
        restarts: std::cell::Cell<u32>,
        health_checks: std::cell::Cell<u32>,
    }

    impl MigrationProbe {
        fn new(healthy: bool) -> MigrationProbe {
            MigrationProbe {
                healthy,
                restarts: std::cell::Cell::new(0),
                health_checks: std::cell::Cell::new(0),
            }
        }
    }

    impl MigrationHost for MigrationProbe {
        fn restart(&self, _config: &Config) -> Result<u32, String> {
            self.restarts.set(self.restarts.get() + 1);
            Ok(4_242)
        }

        fn require_healthy(
            &self,
            _config: &Config,
            _version: &str,
            pid: u32,
        ) -> Result<(), String> {
            self.health_checks.set(self.health_checks.get() + 1);
            assert_eq!(
                pid, 4_242,
                "health is asked about the process restart started"
            );
            if self.healthy {
                Ok(())
            } else {
                Err("the app did not answer /api/health twice".to_string())
            }
        }
    }

    #[test]
    fn a_flat_install_migrates_into_a_version_directory_and_keeps_the_flat_fallback() {
        let tmp = TempDir::new("migrate-flat");
        let install = flat_install(&tmp, "0.3.274", b"the flat build");
        let host = MigrationProbe::new(true);

        let outcome = migrate_layout(
            &config_for(&install),
            None,
            &[test_pubkey_hex().as_str()],
            &host,
        )
        .expect("a flat install with a trusted baseline migrates");

        let version = match outcome {
            Migration::Migrated { version } => version,
            other => panic!("expected Migrated, got {other:?}"),
        };
        assert_eq!(version.name, "0.3.274");
        assert_eq!(
            fs::read(version_exe(&install, "0.3.274")).expect("the version exe"),
            b"the flat build"
        );
        assert_eq!(
            load_sidecar(&version_sidecar(&install, "0.3.274"))
                .expect("the version sidecar")
                .version,
            "0.3.274"
        );
        assert_eq!(
            fs::read_to_string(current_pointer(&install)).expect("the pointer"),
            "0.3.274\n",
            "the pointer names the directory, one line"
        );
        assert!(
            app_exe(&install).is_file(),
            "the flat exe stays as the one-release fallback"
        );
        assert!(
            generation_exe(&install, CURRENT_DIR).is_file(),
            "the baseline stays as the fallback the flat app boots from"
        );
        assert!(
            !previous_pointer(&install).exists(),
            "the first migration has no previous version to record"
        );
        let leftovers: Vec<String> = fs::read_dir(versions_dir(&install))
            .expect("the versions directory")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .filter(|name| name.starts_with('.'))
            .collect();
        assert!(
            leftovers.is_empty(),
            "the staging directory is renamed into place, not left behind: {leftovers:?}"
        );
        assert_eq!(host.restarts.get(), 1, "the owner restarted once");
        assert_eq!(host.health_checks.get(), 1, "health was required once");
    }

    #[test]
    fn a_second_migration_is_a_no_op_and_leaves_the_pointer_alone() {
        let tmp = TempDir::new("migrate-twice");
        let install = flat_install(&tmp, "0.3.274", b"the flat build");
        let host = MigrationProbe::new(true);
        let config = config_for(&install);
        migrate_layout(&config, None, &[test_pubkey_hex().as_str()], &host)
            .expect("the first migration");
        let pointer_before = fs::read(current_pointer(&install)).expect("the pointer");
        let restarts_before = host.restarts.get();

        let second = migrate_layout(&config, None, &[test_pubkey_hex().as_str()], &host)
            .expect("a migrated install is not an error");

        assert!(
            matches!(second, Migration::AlreadyMigrated { .. }),
            "the second run must report that it did nothing: {second:?}"
        );
        assert_eq!(
            fs::read(current_pointer(&install)).expect("the pointer"),
            pointer_before,
            "an idempotent run does not rewrite or move the pointer"
        );
        assert_eq!(
            host.restarts.get(),
            restarts_before,
            "an idempotent run does not restart the service"
        );
    }

    #[test]
    fn a_failed_health_check_removes_the_pointer_and_returns_to_the_flat_layout() {
        let tmp = TempDir::new("migrate-unhealthy");
        let install = flat_install(&tmp, "0.3.274", b"the flat build");
        let host = MigrationProbe::new(false);

        let error = migrate_layout(
            &config_for(&install),
            None,
            &[test_pubkey_hex().as_str()],
            &host,
        )
        .expect_err("a version that cannot serve must not stay current");

        assert!(error.contains("health"), "{error}");
        assert!(
            !current_pointer(&install).exists(),
            "the pointer is removed so the flat layout is in charge again"
        );
        assert_eq!(
            host.restarts.get(),
            2,
            "one restart into the version, one back into the flat layout"
        );
        assert!(
            app_exe(&install).is_file(),
            "the flat exe was never removed"
        );
        match locate_app(&install).expect("the flat install still resolves") {
            AppLocation::Legacy { exe, .. } => assert_eq!(exe, app_exe(&install)),
            other => panic!("expected the flat layout after the failure, got {other:?}"),
        }
    }

    #[test]
    fn an_existing_version_directory_with_different_bytes_is_never_overwritten() {
        let tmp = TempDir::new("migrate-conflict");
        let install = flat_install(&tmp, "0.3.274", b"the flat build");
        let existing = write_version(&install, "0.3.274", b"a different build already installed");
        let sidecar_before = fs::read(&existing.sidecar_path).expect("the existing sidecar");
        let host = MigrationProbe::new(true);

        let error = migrate_layout(
            &config_for(&install),
            None,
            &[test_pubkey_hex().as_str()],
            &host,
        )
        .expect_err("a conflicting version directory is a refusal");

        assert!(
            error.contains("already") || error.contains("different"),
            "the reason names the conflict: {error}"
        );
        assert!(
            !current_pointer(&install).exists(),
            "a conflict must not write a pointer"
        );
        assert_eq!(
            fs::read(&existing.exe).expect("the existing version"),
            b"a different build already installed",
            "the existing version directory is untouched"
        );
        assert_eq!(
            fs::read(&existing.sidecar_path).expect("the existing sidecar"),
            sidecar_before,
            "the existing sidecar is untouched"
        );
        assert_eq!(host.restarts.get(), 0, "nothing was restarted");
    }

    #[test]
    fn the_installers_sidecar_is_preferred_over_the_baseline_one() {
        let tmp = TempDir::new("migrate-sidecar");
        let install = flat_install(&tmp, "0.3.271", b"the flat build");
        let installer_sidecar = tmp.path().join("summrise-device.sidecar.json");
        save_sidecar(
            &installer_sidecar,
            &sidecar_for("0.3.274", b"the flat build", &signing_key()),
        )
        .expect("the installer's sidecar");
        let host = MigrationProbe::new(true);

        let outcome = migrate_layout(
            &config_for(&install),
            Some(&installer_sidecar),
            &[test_pubkey_hex().as_str()],
            &host,
        )
        .expect("the installer's sidecar names the version directory");

        let version = match outcome {
            Migration::Migrated { version } => version,
            other => panic!("expected Migrated, got {other:?}"),
        };
        assert_eq!(
            version.name, "0.3.274",
            "the installer's sidecar wins over baseline/current"
        );
        assert!(
            !version_dir(&install, "0.3.271").exists(),
            "the baseline's version is not the one that becomes current"
        );
        assert_eq!(
            fs::read_to_string(current_pointer(&install)).expect("the pointer"),
            "0.3.274\n"
        );
    }
}
