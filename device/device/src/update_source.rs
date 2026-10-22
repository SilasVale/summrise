//! When a device looks for a release on its own.
//!
//! The updater is a one-shot check ([`crate::updater::check_update`]): it verifies a document, fetches
//! the artifact, stages it and asks the supervisor for the swap. Something has to run it again
//! tomorrow, and a device that only updates when a human types a command is a device that stops
//! receiving fixes the week the human is on holiday. The old agent polled; so does this.
//!
//! **The source is a file, not a flag.** The service is started by the SCM with the installer's argv,
//! so a flag would be frozen at install time and would have to survive every service edit; a file
//! under `<data>` is written once by an operator or the installer and read at every start. It is not
//! a secret, so it is not protected like one — but it names a URL that the device will fetch, which
//! is why it is validated with the same rule the one-shot check uses.
//!
//! **The decision stays in one place.** This module owns *when*; `updater` owns *whether*. Nothing
//! here verifies a signature or decides a version — it calls the same function the operator's
//! `--check-update` calls, with the same key, the same install root and the same rollback memory
//! from `state.json`. A second decision path would be a second set of rules to keep in step.
//!
//! **A fleet does not stampede.** The first check waits a jittered delay derived from the device id,
//! so a hundred machines that reboot together do not ask the mirror for the same document in the
//! same second. The delay is stable per device (a hash of its id), not random per boot: two restarts
//! in a minute must not produce two checks in a minute.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use summrise_release::{parse_release_url_allowing_plaintext, Channel};
use tokio::task::JoinHandle;

use crate::store::Store;
use crate::updater::{self, UpdateConfig, UpdateError};

/// The name of the source file under the data directory.
pub const FILE: &str = "update.json";

/// The shortest interval a source may ask for.
///
/// Five minutes is the floor because a document costs a request and an artifact costs tens of
/// megabytes: a source that asked for every second would be a denial of service aimed at the mirror.
pub const MIN_INTERVAL_S: u64 = 300;

/// The longest interval a source may ask for: a week without a check is not a schedule.
pub const MAX_INTERVAL_S: u64 = 7 * 24 * 60 * 60;

/// The interval used when a source does not name one.
pub const DEFAULT_INTERVAL_S: u64 = 6 * 60 * 60;

/// Where and how often this device looks for a release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateSource {
    /// The release document's URL.
    pub url: String,
    /// The channel this device follows.
    pub channel: Channel,
    /// How many seconds between checks.
    #[serde(default = "default_interval_s")]
    pub interval_s: u64,
    /// Accept a plaintext mirror on a tunnel the operator names.
    #[serde(default)]
    pub allow_plaintext: bool,
}

const fn default_interval_s() -> u64 {
    DEFAULT_INTERVAL_S
}

impl UpdateSource {
    /// Parse and validate a source document.
    pub fn parse(text: &str) -> Result<Self, SourceError> {
        let source: Self = serde_json::from_str(text).map_err(SourceError::Parse)?;
        source.validate()?;
        Ok(source)
    }

    /// The rules a source must satisfy before the device will act on it.
    pub fn validate(&self) -> Result<(), SourceError> {
        parse_release_url_allowing_plaintext(&self.url, self.allow_plaintext)
            .map_err(|_| SourceError::Invalid(format!("not a release URL: {}", self.url)))?;
        if !(MIN_INTERVAL_S..=MAX_INTERVAL_S).contains(&self.interval_s) {
            return Err(SourceError::Invalid(format!(
                "interval_s must be between {MIN_INTERVAL_S} and {MAX_INTERVAL_S}, not {}",
                self.interval_s
            )));
        }
        Ok(())
    }

    /// The path of the source file under `data`.
    pub fn path(data: &Path) -> PathBuf {
        data.join(FILE)
    }

    /// Read `<data>/update.json`; `Ok(None)` means the device does not self-update.
    ///
    /// A file that exists and does not validate is an error, not `None`: a device whose operator
    /// asked for updates and whose source is malformed must say so once at startup rather than
    /// silently never update.
    pub fn load(data: &Path) -> Result<Option<Self>, SourceError> {
        let path = Self::path(data);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(SourceError::Io { path, source }),
        };
        Self::parse(&text).map(Some)
    }

    /// Write a source document, replacing the previous one atomically.
    pub fn write(data: &Path, source: &Self) -> Result<PathBuf, SourceError> {
        source.validate()?;
        let path = Self::path(data);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| SourceError::Io {
                path: parent.to_owned(),
                source,
            })?;
        }
        let text = serde_json::to_string_pretty(source).map_err(SourceError::Serialize)?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, text.as_bytes()).map_err(|source| SourceError::Io {
            path: temporary.clone(),
            source,
        })?;
        std::fs::rename(&temporary, &path).map_err(|source| SourceError::Io { path, source })?;
        Ok(Self::path(data))
    }

    /// The interval as a duration.
    pub fn interval(&self) -> Duration {
        Duration::from_secs(self.interval_s)
    }
}

/// The first check waits this long: a fraction of the interval, spread by the device id.
///
/// `seed` is the device id's first bytes; the modulo keeps the delay inside one interval and stable
/// across restarts of the same device.
pub fn first_delay(interval: Duration, seed: u64) -> Duration {
    let seconds = interval.as_secs().clamp(1, 3_600);
    let spread = seed % seconds.max(1);
    Duration::from_secs(spread)
}

/// Why a source could not be read, parsed or written.
#[derive(Debug)]
pub enum SourceError {
    /// The file could not be read or written.
    Io {
        /// The path involved.
        path: PathBuf,
        /// The filesystem's refusal.
        source: io::Error,
    },
    /// The document is not JSON of the expected shape.
    Parse(serde_json::Error),
    /// The document is JSON but breaks a rule.
    Invalid(String),
    /// The document could be parsed but not serialized (a bug, not an operator error).
    Serialize(serde_json::Error),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceError::Io { path, source } => {
                write!(
                    f,
                    "cannot use the update source {}: {source}",
                    path.display()
                )
            }
            SourceError::Parse(error) => write!(f, "the update source is not valid JSON: {error}"),
            SourceError::Invalid(detail) => write!(f, "the update source is invalid: {detail}"),
            SourceError::Serialize(error) => write!(f, "cannot encode the update source: {error}"),
        }
    }
}

impl std::error::Error for SourceError {}

/// Run the schedule for `source` until the process ends.
///
/// The returned handle is dropped by the caller; the task lives as long as the runtime. A failed
/// check is one log line and the next interval — an unreachable mirror must not stop the device
/// from serving, and an update refused by policy must not be retried inside one interval.
pub fn spawn(
    store: Store,
    install: PathBuf,
    data: PathBuf,
    source: UpdateSource,
    cfg: UpdateConfig,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let seed = device_seed(&data);
        tokio::time::sleep(first_delay(source.interval(), seed)).await;
        loop {
            match check_once(&store, &install, &source, &cfg).await {
                Ok(()) => {}
                Err(error) => eprintln!("summrise-device: scheduled update check: {error}"),
            }
            tokio::time::sleep(source.interval()).await;
        }
    })
}

/// One scheduled check: the same call the operator's one-shot makes, with the state's memory.
pub(crate) async fn check_once(
    store: &Store,
    install: &Path,
    source: &UpdateSource,
    cfg: &UpdateConfig,
) -> Result<(), UpdateError> {
    // The state carries both facts the decision needs — the version this device runs and the
    // artifacts it rolled back. A state that cannot be read skips this round and says so: staging
    // without the rollback memory is the bypass the review called I3, and guessing the version is
    // a decision made on a fact this device does not have.
    let state = match store.load() {
        Ok(state) => state,
        Err(error) => {
            eprintln!(
                "summrise-device: scheduled update check skipped: cannot read the state: {error}"
            );
            return Ok(());
        }
    };
    let pubkey = crate::update::release_pubkey().map_err(UpdateError::Key)?;
    let policy = updater::ReinstallPolicy {
        rolled_back: &state.rolled_back,
        force_reinstall: None,
    };
    updater::check_update_with(
        &source.url,
        None,
        source.channel,
        &state.install.version,
        install,
        cfg,
        policy,
        &pubkey,
    )
    .await
    .map(|_| ())
}

/// A stable per-device number for the first delay: the identity file's bytes, or the data path.
fn device_seed(data: &Path) -> u64 {
    let identity = data.join(crate::identity::IDENTITY_FILE);
    let bytes = std::fs::read(identity).unwrap_or_default();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes.iter().chain(data.as_os_str().as_encoded_bytes()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> UpdateSource {
        UpdateSource {
            url: "https://mirror.example.test/release.json".to_owned(),
            channel: Channel::Canary,
            interval_s: DEFAULT_INTERVAL_S,
            allow_plaintext: false,
        }
    }

    #[test]
    fn a_valid_source_round_trips_through_its_file() {
        let dir = tempdir();
        let source = valid();
        let path = UpdateSource::write(&dir, &source).expect("write");
        assert!(path.ends_with(FILE));
        let loaded = UpdateSource::load(&dir).expect("load").expect("present");
        assert_eq!(loaded, source);
    }

    #[test]
    fn an_absent_source_is_no_schedule_rather_than_an_error() {
        let dir = tempdir();
        assert_eq!(UpdateSource::load(&dir).expect("load"), None);
    }

    #[test]
    fn a_malformed_source_is_named_rather_than_ignored() {
        let dir = tempdir();
        std::fs::write(UpdateSource::path(&dir), b"{\"url\": \"not a url\"}").expect("write");
        let error = UpdateSource::load(&dir).expect_err("a malformed source is an error");
        assert!(
            matches!(error, SourceError::Parse(_) | SourceError::Invalid(_)),
            "{error:?}"
        );
    }

    #[test]
    fn the_interval_is_bounded_in_both_directions() {
        let mut source = valid();
        source.interval_s = MIN_INTERVAL_S - 1;
        assert!(source.validate().is_err(), "too fast");
        source.interval_s = MAX_INTERVAL_S + 1;
        assert!(source.validate().is_err(), "too slow");
        source.interval_s = MIN_INTERVAL_S;
        assert!(source.validate().is_ok());
    }

    #[test]
    fn plaintext_needs_the_opt_in() {
        let mut source = valid();
        source.url = "http://mirror.example.test/release.json".to_owned();
        assert!(
            source.validate().is_err(),
            "plain http is refused by default"
        );
        source.allow_plaintext = true;
        assert!(source.validate().is_ok(), "the operator can name a tunnel");
    }

    #[test]
    fn the_first_delay_is_inside_the_interval_and_stable() {
        let interval = Duration::from_secs(600);
        assert_eq!(first_delay(interval, 7), first_delay(interval, 7));
        assert!(first_delay(interval, 599) < interval);
        assert!(first_delay(interval, 0) < interval);
    }

    /// A directory under the system temp root that removes itself, named for this test process.
    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "summrise-update-source-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&path).expect("temp dir");
        path
    }
}
