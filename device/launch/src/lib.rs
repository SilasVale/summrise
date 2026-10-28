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
//!    `current` and `previous` rotate (fix design F1/F2/F4).
//!
//! Everything in this module is pure or ordinary `std` I/O, so it is exercised by `cargo test` on
//! Linux. The SCM, process termination and the supervisor loop live in the Windows binary.

#![forbid(unsafe_code)]

mod paths;
mod self_check;

pub use paths::{
    app_exe, app_pid_file, baseline_dir, baseline_exe, baseline_sidecar, console_link, flat_exe,
    flat_sidecar, generation_dir, generation_exe, generation_sidecar, install_token,
    launcher_pid_file, repair_log, staged_exe, staged_sidecar, swap_request, swap_result, APP_EXE,
    BASELINE_DIR, BASELINE_SIDECAR, CONSOLE_FILE, CURRENT_DIR, FLAT_SIDECAR, LAUNCHER_EXE,
    PENDING_DIR, PREVIOUS_DIR, REPAIR_LOG, STAGING_DIR,
};
pub use self_check::{inspect, Check, Report};

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
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

/// Port the app serves on when neither `--port` nor `SUMMRISE_PORT` is set.
pub const DEFAULT_PORT: u16 = 18080;

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
    fn port_defaults_to_18080() {
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
        };
        write_console_link(&data, &installed).expect("the installed link is writable");

        let rewrite = ConsoleLink {
            url: None,
            token_file: None,
            mesh_broker: Some("http://127.0.0.1:9090".to_string()),
            mesh_relay: Some("127.0.0.1:9001".to_string()),
            mesh_stun: None,
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
            },
        )
        .expect("the installed link is writable");

        let rewrite = ConsoleLink {
            url: Some("https://console.example.test".to_string()),
            token_file: Some(data.join("console-token.json")),
            mesh_broker: None,
            mesh_relay: None,
            mesh_stun: None,
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
}
