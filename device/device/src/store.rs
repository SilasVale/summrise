//! The one state document: `<data>/state.json`, written atomically and only by this process.
//!
//! `summrise-core` owns the document's shape; this module owns its storage. The two rules that make
//! the file trustworthy are both here: an absent file is a fresh document rather than an error, and
//! a write is a random `.tmp` + fsync + rename that a reader can never catch half-done.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use summrise_core::{
    Baseline, Install, LastVerdict, RolledBackArtifact, Service, ServiceState, State, Update,
    UpdateOutcome, UpdatePhase, VerdictOutcome, SCHEMA_VERSION,
};

use crate::config::Release;

/// The state document's file name, relative to the data directory (design §4.2).
pub const STATE_FILE: &str = "state.json";

/// The app binary's file name inside the generation the launcher boots.
pub const BASELINE_EXE: &str = "summrise-device.exe";

/// The supervisor's answer to a swap request, relative to the install root (fix design F1).
pub const SWAP_RESULT_FILE: &str = "swap.result.json";

/// The stager's swap request, relative to the install root (fix design F1).
///
/// The CLI writes this file; the app reads it and records `update.phase = staged`. It is the whole
/// reason `--stage`/`--check-update` never open the state document for writing: the durable fact the
/// CLI leaves is a file, and the serving app is the document's only writer.
pub const SWAP_REQUEST_FILE: &str = "swap.request.json";

/// The launcher's append-only repair log, relative to the install root (I3).
///
/// The launcher writes one JSON line per restore from the baseline; the app consumes new complete
/// lines at start. It is the machine record cutover gate 1 asks for, and the launcher — not the app
/// — is its one writer.
pub const REPAIR_LOG_FILE: &str = "repair.log";

/// How often the running app folds a swap request and a swap result into the state document.
///
/// One second, not thirty: the request is transient. The supervisor polls for it every two seconds
/// (`summrise-launch`'s `SWAP_POLL_INTERVAL`), consumes it and removes it, so a slower app poll
/// would usually never see the stage at all and the document would jump straight to the outcome. A
/// request that is missed anyway is not lost: the result still carries the version and the outcome.
pub const SWAP_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// The legacy fixed temporary name a pre-NEW-8 write used. It is not written any more; the tests
/// use it to prove a leftover from the old writer is ignored rather than mistaken for state.
#[cfg(test)]
const LEGACY_TMP_FILE: &str = "state.json.tmp";

/// Why a state document could not be loaded.
#[derive(Debug)]
pub enum StoreError {
    /// The file exists but could not be read.
    Io(io::Error),
    /// The file was read but is not a document this build understands.
    Corrupt(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io(error) => write!(f, "could not read the state file: {error}"),
            StoreError::Corrupt(detail) => write!(f, "the state file is corrupt: {detail}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StoreError::Io(error) => Some(error),
            StoreError::Corrupt(_) => None,
        }
    }
}

/// The state file's directory, and the install root the initial document names.
#[derive(Clone, Debug)]
pub struct Store {
    install: PathBuf,
    data: PathBuf,
    release: Release,
}

impl Store {
    /// A store for the device installed at `install`, writing under `data`, started as `release`.
    pub fn new(install: impl Into<PathBuf>, data: impl Into<PathBuf>, release: Release) -> Self {
        Self {
            install: install.into(),
            data: data.into(),
            release,
        }
    }

    /// `<data>/state.json`.
    pub fn state_path(&self) -> PathBuf {
        self.data.join(STATE_FILE)
    }

    /// The data directory this store writes under.
    pub fn data_dir(&self) -> &Path {
        &self.data
    }

    /// The generation the launcher boots: `<install>/baseline/current/summrise-device.exe`.
    pub fn baseline_exe_path(&self) -> PathBuf {
        self.install
            .join("baseline")
            .join("current")
            .join(BASELINE_EXE)
    }

    /// Whether a state file exists yet.
    pub fn exists(&self) -> bool {
        self.state_path().exists()
    }

    /// Move a state document this build cannot read aside, keeping the bytes as evidence.
    ///
    /// The state is a cache of identity the install can rebuild, so a torn document must not leave a
    /// device degraded until an operator deletes a file. The bytes are kept next to the state as
    /// `state.json.bad` — an earlier `.bad` is replaced, because it is evidence for a previous start.
    pub fn quarantine_state(&self) -> Option<PathBuf> {
        let path = self.state_path();
        let mut name = path.as_os_str().to_owned();
        name.push(".bad");
        let quarantined = PathBuf::from(name);
        let _ = fs::remove_file(&quarantined);
        match fs::rename(&path, &quarantined) {
            Ok(()) => Some(quarantined),
            Err(error) => {
                eprintln!(
                    "summrise-device: could not quarantine {} ({error})",
                    path.display()
                );
                None
            }
        }
    }

    /// The document a device with no state file starts from.
    ///
    /// The release facts are the launcher's argv (fix design F6): `install.version`, the channel and
    /// the baseline pair are the release this process is actually running, not this build's
    /// compile-time version. A standalone development start falls back to the crate version with an
    /// empty digest and the stable channel, which is the only honest answer when no signed pair was
    /// verified and nothing opted into canary.
    pub fn initial(&self) -> State {
        let started_at = now_rfc3339();
        State {
            schema: SCHEMA_VERSION,
            install: Install {
                dir: self.install.display().to_string(),
                version: self.release.version.clone(),
                channel: self.release.channel.clone(),
            },
            baseline: self.baseline_facts(),
            service: Service {
                state: ServiceState::Running,
                pid: std::process::id(),
                started_at: started_at.clone(),
            },
            last_verdict: LastVerdict {
                ts: started_at,
                outcome: VerdictOutcome::Ok,
                detail: format!("started {}", self.release.version),
            },
            sessions: Vec::new(),
            update: Update {
                phase: UpdatePhase::Idle,
                to: None,
                outcome: None,
                last_result_at_ms: None,
            },
            rolled_back: Vec::new(),
            repair_offset: 0,
        }
    }

    /// Record this process's start in `state`, keeping every other fact.
    ///
    /// The pid and the timestamp are liveness, not identity: a restart must replace them or the
    /// document keeps describing the previous run. The release facts are identity: a restart under a
    /// new release must replace the version, channel, baseline and verdict too, or `state.json` keeps
    /// naming a binary (and a channel) that is no longer on disk.
    pub fn refresh_start(&self, state: &mut State) {
        state.install.version = self.release.version.clone();
        state.install.channel = self.release.channel.clone();
        state.baseline = self.baseline_facts();
        state.last_verdict = LastVerdict {
            ts: now_rfc3339(),
            outcome: VerdictOutcome::Ok,
            detail: format!("started {}", self.release.version),
        };
        state.service.state = ServiceState::Running;
        state.service.pid = std::process::id();
        state.service.started_at = now_rfc3339();
    }

    /// Record this process's start without claiming a release: liveness and the verdict only.
    ///
    /// A process started without `--release-version` claims no release pair — it is a development
    /// run, not the supervised app. It must not overwrite the identity facts the launcher recorded:
    /// doing so rewrote `state.json` to the crate version with an empty digest, and the next
    /// scheduled update then judged a release against a version the device does not run. The
    /// document keeps its install and baseline; only the facts about *this process* change.
    pub fn refresh_liveness(&self, state: &mut State) {
        state.last_verdict = LastVerdict {
            ts: now_rfc3339(),
            outcome: VerdictOutcome::Ok,
            detail: format!("started {} (unclaimed start)", state.install.version),
        };
        state.service.state = ServiceState::Running;
        state.service.pid = std::process::id();
        state.service.started_at = now_rfc3339();
    }

    /// The baseline facts the current release claims: version and digest from the argv, and the
    /// generation path the launcher owns.
    fn baseline_facts(&self) -> Baseline {
        Baseline {
            version: self.release.version.clone(),
            sha256: self.release.sha256.clone(),
            exe: self.baseline_exe_path().display().to_string(),
        }
    }

    /// Read the state document, or a fresh one when there is no file yet.
    ///
    /// Only `state.json` is opened. A `.tmp` left by a write that never reached its rename describes
    /// a document that was never committed, so it is not state — treating it as state is how a
    /// half-written file becomes an answer.
    pub fn load(&self) -> Result<State, StoreError> {
        let path = self.state_path();
        match fs::read(&path) {
            Ok(bytes) => {
                let document = String::from_utf8(bytes).map_err(|_| {
                    StoreError::Corrupt(format!("{} is not valid UTF-8", path.display()))
                })?;
                State::parse(&document)
                    .map_err(|error| StoreError::Corrupt(format!("{}: {error}", path.display())))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(self.initial()),
            Err(error) => Err(StoreError::Io(error)),
        }
    }

    /// Replace the state document atomically.
    ///
    /// The bytes land in a random `.tmp` sibling, are flushed to disk, and are renamed over
    /// `state.json`. The rename is the commit point: before it the old document is the document,
    /// after it the new one is, and there is no instant where a reader sees part of either. The
    /// temporary is `create_new` under an unpredictable name, so a planted `<name>.tmp` link cannot
    /// redirect the write (NEW-8).
    pub fn save(&self, state: &State) -> io::Result<()> {
        fs::create_dir_all(&self.data)?;
        let (temporary, mut file) = crate::fsutil::create_new_sibling(&self.state_path())?;
        if let Err(error) = file
            .write_all(state.to_json().as_bytes())
            .and_then(|()| file.sync_all())
        {
            drop(file);
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        drop(file);
        if let Err(error) = fs::rename(&temporary, self.state_path()) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        Ok(())
    }

    /// `<install>/swap.result.json` — the supervisor's answer to a swap request.
    pub fn swap_result_path(&self) -> PathBuf {
        self.install.join(SWAP_RESULT_FILE)
    }

    /// `<install>/swap.request.json` — the stager's request, which the app reads and the launcher
    /// consumes.
    pub fn swap_request_path(&self) -> PathBuf {
        self.install.join(SWAP_REQUEST_FILE)
    }

    /// `<install>/summrise-device.new.json` — the staged pair's signed sidecar.
    fn staged_sidecar_path(&self) -> PathBuf {
        self.install.join(crate::update::STAGED_SIDECAR)
    }

    /// The digest of the artifact a rollback result names.
    ///
    /// `swap.result.json` carries the outcome, version and time, not the digest. The staged pair the
    /// launcher verified is still on disk after a rollback, and its sidecar is the same artifact the
    /// request named. The version check is the guard: a sidecar for another release is not evidence
    /// about this rollback, and a value that is not a sha256 is not a digest. When neither holds,
    /// this returns `None` rather than inventing a digest the safeguard could not honestly refuse.
    fn rolled_back_artifact(&self, version: &str, at_ms: u64) -> Option<RolledBackArtifact> {
        let bytes = fs::read(self.staged_sidecar_path()).ok()?;
        let sidecar: StagedSidecarFile = serde_json::from_slice(&bytes).ok()?;
        if sidecar.version != version || !is_sha256_hex(&sidecar.sha256) {
            return None;
        }
        Some(RolledBackArtifact {
            sha256: sidecar.sha256,
            version: version.to_owned(),
            at_ms,
        })
    }

    /// `<install>/repair.log` — the launcher's record of restores from the baseline (I3).
    pub fn repair_log_path(&self) -> PathBuf {
        self.install.join(REPAIR_LOG_FILE)
    }

    /// Fold the launcher's new repair records into `state`, returning whether it changed.
    ///
    /// The state document carries the byte offset already consumed, so this is idempotent across a
    /// restart: the same line is recorded once, and a repair appended while the app was down is
    /// recorded at the next start. `lastVerdict` is the operator-facing sentence; `events.jsonl`
    /// carries the name (`repaired_from_baseline`) an automated reader filters on.
    pub fn ingest_repairs(&self, state: &mut State) -> io::Result<bool> {
        let (records, offset) = read_new_repairs(&self.repair_log_path(), state.repair_offset)?;
        if records.is_empty() && offset == state.repair_offset {
            return Ok(false);
        }
        for record in &records {
            state.last_verdict = LastVerdict {
                ts: format_utc(record.ts_ms / 1_000),
                outcome: VerdictOutcome::Repaired,
                detail: format!("repaired from {}", record.from),
            };
            record_repair_event(&self.data, record);
        }
        state.repair_offset = offset;
        Ok(true)
    }

    /// Fold the stager's swap request into `state`, returning whether it changed.
    ///
    /// `--stage` and `--check-update` are not the serving app, so they leave the document alone and
    /// write `<install>/swap.request.json` instead; this is the app's reading of that file, run at
    /// start and on every poll. The phase moves to `staged` and `to` names the requested version.
    ///
    /// A version the document already names is not recorded again: a repeated request (the
    /// supervisor has not consumed it yet, or the same release is staged twice) must not rewrite the
    /// document, and a result the app has already folded in must not be downgraded to `staged`. The
    /// outcome is cleared only when a *new* version is staged.
    pub fn ingest_swap_request(&self, state: &mut State) -> io::Result<bool> {
        let path = self.swap_request_path();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        let request: SwapRequestFile = match serde_json::from_slice(&bytes) {
            Ok(request) => request,
            Err(error) => {
                eprintln!(
                    "summrise-device: ignoring an unusable {} ({error}); it is not a swap request this build understands",
                    path.display()
                );
                return Ok(false);
            }
        };
        if state.update.to.as_deref() == Some(request.version.as_str()) {
            return Ok(false);
        }
        state.update.phase = UpdatePhase::Staged;
        state.update.to = Some(request.version);
        state.update.outcome = None;
        Ok(true)
    }

    /// Fold the supervisor's swap result into `state` if it has not been recorded yet.
    ///
    /// The supervisor owns `swap.result.json`, not the state document; the app is still the only
    /// writer of the document. `last_result_at_ms` is what makes this idempotent across restarts and
    /// the one-second poll: a result at or below the recorded time is already in the document, and a
    /// torn or unknown result is left for a human rather than guessed at.
    ///
    /// Returns whether `state` changed; the caller saves it.
    pub fn apply_swap_result(&self, state: &mut State) -> io::Result<bool> {
        let path = self.swap_result_path();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        let result: SwapResultFile = match serde_json::from_slice(&bytes) {
            Ok(result) => result,
            Err(error) => {
                eprintln!(
                    "summrise-device: ignoring an unusable {} ({error}); it is not a swap result this build understands",
                    path.display()
                );
                return Ok(false);
            }
        };
        if state
            .update
            .last_result_at_ms
            .is_some_and(|recorded| recorded >= result.at_ms)
        {
            return Ok(false);
        }

        let version = result.version.as_deref().unwrap_or("an unnamed release");
        let (outcome, verdict, detail) = match result.outcome {
            SwapOutcomeFile::Applied => (
                UpdateOutcome::Applied,
                VerdictOutcome::Ok,
                format!("updated to {version}"),
            ),
            SwapOutcomeFile::RolledBack => {
                // The result's digest is authoritative: the launcher verified that artifact before
                // the swap, so this records a rollback via `--apply` and one whose staged pair was
                // removed afterwards alike. An older result without the field falls back to the
                // staged sidecar, with a line saying so.
                if let Some(version) = result.version.as_deref() {
                    match result
                        .sha256
                        .as_deref()
                        .filter(|sha256| is_sha256_hex(sha256))
                    {
                        Some(sha256) => state.record_rolled_back(RolledBackArtifact {
                            sha256: sha256.to_owned(),
                            version: version.to_owned(),
                            at_ms: result.at_ms,
                        }),
                        None => {
                            eprintln!(
                                "summrise-device: the rolled-back result for {version} carries no \
                                 digest; falling back to {}",
                                self.staged_sidecar_path().display()
                            );
                            match self.rolled_back_artifact(version, result.at_ms) {
                                Some(artifact) => state.record_rolled_back(artifact),
                                None => eprintln!(
                                    "summrise-device: the rolled-back artifact {version} has no \
                                     digest in {}; the digest safeguard cannot refuse it",
                                    self.staged_sidecar_path().display()
                                ),
                            }
                        }
                    }
                }
                (
                    UpdateOutcome::RolledBack,
                    VerdictOutcome::NeedsAttention,
                    format!("rolled back from {version}"),
                )
            }
            SwapOutcomeFile::Refused => (
                UpdateOutcome::Refused,
                VerdictOutcome::NeedsAttention,
                match result.reason.as_deref() {
                    Some(reason) if !reason.is_empty() => format!("refused: {reason}"),
                    // A refusal means the supervisor installed nothing: saying "repaired" here was
                    // the false evidence I3 removed.
                    _ => "refused: nothing was installed".to_owned(),
                },
            ),
        };
        state.update.phase = UpdatePhase::Idle;
        state.update.outcome = Some(outcome);
        state.update.last_result_at_ms = Some(result.at_ms);
        if let Some(version) = result.version {
            state.update.to = Some(version);
        }
        state.last_verdict = LastVerdict {
            ts: now_rfc3339(),
            outcome: verdict,
            detail,
        };
        Ok(true)
    }

    /// Load the document, fold in a pending swap request and a newer swap result, and save it if
    /// anything changed.
    ///
    /// The request is applied first and the result last: when both files are on disk (the supervisor
    /// wrote the result before removing the request), the outcome is the last word and the document
    /// does not end the poll still saying `staged`.
    ///
    /// A corrupt document is left alone, exactly as at startup: rewriting it would destroy the
    /// evidence `/api/state` is reporting. A tree with no document is not a running app, so nothing
    /// is invented for it either.
    pub fn reconcile_swap(&self) -> io::Result<bool> {
        if !self.exists() {
            return Ok(false);
        }
        let mut state = match self.load() {
            Ok(state) => state,
            Err(error) => {
                eprintln!(
                    "summrise-device: the state document is unusable ({error}); a swap is not recorded"
                );
                return Ok(false);
            }
        };
        let staged = self.ingest_swap_request(&mut state)?;
        let finished = self.apply_swap_result(&mut state)?;
        if !staged && !finished {
            return Ok(false);
        }
        self.save(&state)?;
        Ok(true)
    }
}

/// Start the app's reconciliation of `swap.request.json` and `swap.result.json`.
///
/// The stager writes a request and the supervisor writes a result while the app may still be the old
/// or the new build; a start only records files that were already on disk. Without this thread a
/// request or result written while the app served would sit unrecorded until the next restart. The
/// poll is [`SWAP_POLL_INTERVAL`] because the request is transient: the supervisor consumes it on
/// its own two-second poll. A thread that cannot start is loud but not fatal: both files are still
/// folded in at the next start.
pub fn spawn_swap_result_watcher(store: Store) {
    let builder = std::thread::Builder::new().name("summrise-device-swap-files".to_owned());
    if let Err(error) = builder.spawn(move || loop {
        std::thread::sleep(SWAP_POLL_INTERVAL);
        match store.reconcile_swap() {
            Ok(true) => eprintln!("summrise-device: recorded the swap request or result"),
            Ok(false) => {}
            Err(error) => eprintln!("summrise-device: cannot record the swap: {error}"),
        }
    }) {
        eprintln!(
            "summrise-device: cannot start the swap watcher ({error}); a pending swap is recorded at the next start"
        );
    }
}

/// The stager's request document, as the app reads it (`summrise-device --stage` writes it).
///
/// Only the version is needed to record the stage; `requested_at_ms` and `sha256` are part of the
/// file the launcher consumes, and unknown fields are tolerated so this reader never has to track
/// them.
#[derive(Debug, Deserialize)]
struct SwapRequestFile {
    version: String,
}

/// The supervisor's result document, as the app reads it (`summrise-launch` writes it).
///
/// Unknown fields are tolerated on purpose, and `reason`/`sha256` are optional: the launcher writes
/// them for a rollback or a refusal, and an older or hand-written result without them must still be
/// readable. A result without a digest falls back to the staged sidecar for the artifact's identity.
#[derive(Debug, Deserialize)]
struct SwapResultFile {
    outcome: SwapOutcomeFile,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    at_ms: u64,
}

/// The three outcomes the supervisor can write.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SwapOutcomeFile {
    Applied,
    RolledBack,
    Refused,
}

/// The staged pair's sidecar, as the app reads it for the digest a swap result does not carry.
///
/// Only two fields matter here; the sidecar's channel and signature are the launcher's contract and
/// are tolerated rather than tracked. A sidecar this reader cannot parse is not evidence.
#[derive(Debug, Deserialize)]
struct StagedSidecarFile {
    version: String,
    sha256: String,
}

/// Whether `value` is exactly 64 hex characters (either case).
fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// One repair the launcher recorded: the UTC milliseconds and the restored binary's digest.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RepairRecord {
    ts_ms: u64,
    /// Which generation the bytes came from: `baseline` (the verified current pair) or `previous`
    /// (the retired pair, booted because `current` was unusable).
    from: String,
    sha256: String,
}

/// One line of `<install>/repair.log`, as the launcher writes it.
#[derive(Deserialize)]
struct RepairLine {
    ts_ms: u64,
    kind: String,
    #[serde(rename = "from")]
    from: String,
    sha256: String,
}

/// Read the complete lines after `offset` and return them with the offset just past the last one.
///
/// Two facts make this safe against a live writer and a replaced file:
///
/// * **only newline-terminated lines are consumed.** A line the launcher is still writing has no
///   newline yet; parsing it would fail and, worse, a partial line that happened to parse would be
///   recorded as a repair that did not finish. The returned offset stops before it, so the line is
///   consumed after the newline lands.
/// * an offset past the end of the file means the log was rotated or replaced. Treating it as an
///   error would leave the device permanently blind; reading from zero records the current file.
///
/// A malformed *complete* line is skipped but its bytes are consumed, so one bad line cannot wedge
/// every later repair behind it.
fn read_new_repairs(path: &Path, offset: u64) -> io::Result<(Vec<RepairRecord>, u64)> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), offset)),
        Err(error) => return Err(error),
    };
    let start = usize::try_from(offset).unwrap_or(usize::MAX);
    let start = if start > bytes.len() { 0 } else { start };
    let mut records = Vec::new();
    let mut consumed = start;
    for line in bytes[start..].split_inclusive(|byte| *byte == b'\n') {
        if !line.ends_with(b"\n") {
            break;
        }
        consumed += line.len();
        let Ok(parsed) = serde_json::from_slice::<RepairLine>(line) else {
            continue;
        };
        // Two sources are repairs this build understands: the verified current pair, and the
        // retired pair the supervisor booted because `current` was unusable. Anything else is a
        // record from a newer build or a damaged line, and is skipped rather than guessed at.
        if parsed.kind == "repaired" && matches!(parsed.from.as_str(), "baseline" | "previous") {
            records.push(RepairRecord {
                ts_ms: parsed.ts_ms,
                from: parsed.from,
                sha256: parsed.sha256,
            });
        }
    }
    Ok((records, consumed as u64))
}

/// Append the operator's `force_reinstall` override to `<data>/events.jsonl`.
///
/// The event log is the device's append-only audit. An override is an exception to a policy
/// refusal, so it is recorded with the version, the digest and the rollback time that justified it.
/// Unlike the best-effort repair event, this returns its error: an override that cannot be audited
/// must not be applied.
pub(crate) fn record_force_reinstall(
    data: &Path,
    version: &str,
    sha256: &str,
    rolled_back_at_ms: u64,
) -> io::Result<()> {
    let line = serde_json::json!({
        "ts_ms": now_ms(),
        "event": "force_reinstall",
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

/// Append the `repaired_from_baseline` event to `<data>/events.jsonl`, best effort.
///
/// The verdict in the state document is the operator-facing record; a failed event write must not
/// stop the device from starting, but it is loud because the automated evidence would be missing.
fn record_repair_event(data: &Path, record: &RepairRecord) {
    // The event name carries the source: a device repaired from the retired generation is a
    // different fact from one whose app copy was rewritten from the verified pair.
    let line = serde_json::json!({
        "ts_ms": record.ts_ms,
        "event": format!("repaired_from_{}", record.from),
        "from": record.from,
        "sha256": record.sha256,
    });
    let mut text = line.to_string();
    text.push('\n');
    if let Err(error) = fs::create_dir_all(data) {
        eprintln!(
            "summrise-device: cannot create {} for the repair event: {error}",
            data.display()
        );
        return;
    }
    match fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data.join("events.jsonl"))
    {
        Ok(mut file) => {
            if let Err(error) = file.write_all(text.as_bytes()) {
                eprintln!("summrise-device: cannot record the repair event: {error}");
            }
        }
        Err(error) => eprintln!("summrise-device: cannot open the event log: {error}"),
    }
}

/// The current time, in the document's RFC 3339 UTC form.
fn now_rfc3339() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    format_utc(seconds)
}

/// The current time in milliseconds since the Unix epoch, saturating at zero for a clock before it.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

/// An RFC 3339 UTC timestamp for `unix_seconds`.
///
/// `core` owns no clock and this crate has no date dependency; the document's two timestamps are
/// the only place a date is needed, so the conversion (Howard Hinnant's civil-from-days) lives here
/// with its own test instead of adding a calendar to the dependency tree.
pub(crate) fn format_utc(unix_seconds: u64) -> String {
    let days = (unix_seconds / 86_400) as i64;
    let second_of_day = unix_seconds % 86_400;
    let (year, month, day) = civil_from_days(days);

    let hour = second_of_day / 3_600;
    let minute = (second_of_day % 3_600) / 60;
    let second = second_of_day % 60;

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// `(year, month, day)` for a count of days since 1970-01-01, proleptic Gregorian.
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let shifted = days_since_epoch + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };

    (
        if month <= 2 { year + 1 } else { year },
        month as u32,
        day as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    use summrise_release::Channel;

    use summrise_core::{
        Baseline, Install, LastVerdict, Service, ServiceState, SessionKind, SessionRow,
        SessionState, Update, UpdatePhase, VerdictOutcome, MAX_ROLLED_BACK, SCHEMA_VERSION,
    };

    /// A complete document, so a round trip compares real fields rather than defaults.
    fn sample_state(store: &Store) -> State {
        let mut state = store.initial();
        state.install = Install {
            dir: store.install.display().to_string(),
            version: "2.0.3".to_owned(),
            channel: "canary".to_owned(),
        };
        state.baseline = Baseline {
            version: "2.0.3".to_owned(),
            sha256: "b1946ac92492d2347c6235b4d2611184".to_owned(),
            exe: "baseline/summrise-device.exe".to_owned(),
        };
        state.service = Service {
            state: ServiceState::Running,
            pid: 1234,
            started_at: "2026-10-20T12:00:00Z".to_owned(),
        };
        state.last_verdict = LastVerdict {
            ts: "2026-10-20T12:00:00Z".to_owned(),
            outcome: VerdictOutcome::Repaired,
            detail: "restored the app from the baseline".to_owned(),
        };
        state.sessions = vec![SessionRow {
            id: "session-1".to_owned(),
            kind: SessionKind::Pty,
            state: SessionState::Attached,
        }];
        state.update = Update {
            phase: UpdatePhase::Staged,
            to: Some("2.0.4".to_owned()),
            outcome: None,
            last_result_at_ms: None,
        };
        state
    }

    fn store_in(dir: &TempDir) -> (Store, PathBuf) {
        let data = dir.join("data");
        (
            Store::new(dir.join("install"), &data, Release::standalone()),
            data,
        )
    }

    fn release(version: &str, sha256: &str) -> Release {
        release_on(Channel::Stable, version, sha256)
    }

    fn release_on(channel: Channel, version: &str, sha256: &str) -> Release {
        Release {
            version: version.to_owned(),
            sha256: sha256.to_owned(),
            channel: channel.as_str().to_owned(),
        }
    }

    #[test]
    fn a_saved_document_round_trips() {
        let dir = TempDir::new("round-trip");
        let (store, _) = store_in(&dir);
        let state = sample_state(&store);

        store.save(&state).expect("the document saves");
        let read = store.load().expect("the document loads");

        assert_eq!(read, state);
    }

    #[test]
    fn an_absent_file_loads_the_initial_document() {
        let dir = TempDir::new("absent");
        let (store, _) = store_in(&dir);

        let state = store.load().expect("an absent file is not an error");

        assert_eq!(state.schema, SCHEMA_VERSION);
        assert_eq!(state.install.dir, dir.join("install").display().to_string());
        assert_eq!(state.install.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(state.install.channel, "stable");
        assert_eq!(
            state.baseline.version,
            env!("CARGO_PKG_VERSION"),
            "a standalone start's release is this build's version"
        );
        assert_eq!(
            state.baseline.sha256, "",
            "a standalone start has no verified baseline digest"
        );
        assert_eq!(
            state.baseline.exe,
            store.baseline_exe_path().display().to_string()
        );
        assert_eq!(state.service.state, ServiceState::Running);
        assert_eq!(state.service.pid, std::process::id());
        assert!(
            state.service.started_at.ends_with('Z'),
            "the document's timestamp is UTC: {}",
            state.service.started_at
        );
        assert_eq!(state.last_verdict.outcome, VerdictOutcome::Ok);
        assert_eq!(
            state.last_verdict.detail,
            format!("started {}", env!("CARGO_PKG_VERSION"))
        );
        assert!(state.sessions.is_empty());
        assert_eq!(state.update.phase, UpdatePhase::Idle);
        assert_eq!(state.update.to, None);
    }

    #[test]
    fn a_start_writes_the_release_facts_the_launcher_passed() {
        let dir = TempDir::new("release-facts");
        let data = dir.join("data");
        let store = Store::new(
            dir.join("install"),
            &data,
            release("2.0.3", "b1946ac92492d2347c6235b4d2611184"),
        );

        let state = store.initial();
        assert_ne!(state.install.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(state.install.version, "2.0.3");
        assert_eq!(state.baseline.version, "2.0.3");
        assert_eq!(state.baseline.sha256, "b1946ac92492d2347c6235b4d2611184");
        assert_eq!(
            state.baseline.exe,
            store.baseline_exe_path().display().to_string()
        );
        assert_eq!(state.last_verdict.detail, "started 2.0.3");

        // A restart under a new release must replace the old release facts, not preserve them.
        let mut stale = state;
        stale.install.version = "0.1.0".to_owned();
        stale.baseline.version = "0.1.0".to_owned();
        stale.baseline.sha256 = "00".repeat(32);
        stale.last_verdict.outcome = VerdictOutcome::NeedsAttention;
        stale.sessions = vec![SessionRow {
            id: "session-1".to_owned(),
            kind: SessionKind::Pty,
            state: SessionState::Attached,
        }];

        store.refresh_start(&mut stale);

        assert_eq!(stale.install.version, "2.0.3");
        assert_eq!(stale.baseline.version, "2.0.3");
        assert_eq!(stale.baseline.sha256, "b1946ac92492d2347c6235b4d2611184");
        assert_eq!(stale.last_verdict.detail, "started 2.0.3");
        assert_eq!(stale.last_verdict.outcome, VerdictOutcome::Ok);
        assert_eq!(
            stale.sessions.len(),
            1,
            "release facts are refreshed; sessions are the document's other writer-free facts"
        );
        assert_eq!(stale.service.pid, std::process::id());
    }

    #[test]
    fn a_canary_start_records_the_channel_the_sidecar_names() {
        let dir = TempDir::new("canary-channel");
        let store = Store::new(
            dir.join("install"),
            dir.join("data"),
            release_on(Channel::Canary, "2.0.3", "b1946ac92492d2347c6235b4d2611184"),
        );

        let state = store.initial();

        assert_eq!(
            state.install.channel, "canary",
            "a canary build must not be labelled stable in the state document"
        );
    }

    #[test]
    fn a_restart_under_a_stable_sidecar_flips_the_recorded_channel_back() {
        let dir = TempDir::new("channel-flip");
        let canary = Store::new(
            dir.join("install"),
            dir.join("data"),
            release_on(Channel::Canary, "2.0.3", "b1946ac92492d2347c6235b4d2611184"),
        );
        let mut state = canary.initial();
        assert_eq!(state.install.channel, "canary");

        let stable = Store::new(
            dir.join("install"),
            dir.join("data"),
            release("2.0.3", "b1946ac92492d2347c6235b4d2611184"),
        );
        stable.refresh_start(&mut state);

        assert_eq!(
            state.install.channel, "stable",
            "the channel is a release fact and follows the sidecar this start booted"
        );
    }

    #[test]
    fn an_unclaimed_start_keeps_the_identity_the_launcher_recorded() {
        let dir = TempDir::new("store-unclaimed-start");
        // The document the launcher's install left behind: 2.0.3 on canary.
        let recorded = Store::new(
            dir.join("install"),
            dir.join("data"),
            release("2.0.3", "b1946ac92492d2347c6235b4d2611184"),
        );
        let mut state = recorded.initial();
        state.install.channel = "canary".to_owned();
        state.service.pid = 111;
        state.service.started_at = "2026-01-01T00:00:00Z".to_owned();

        // A development run: no `--release-version`, so the standalone release claims nothing.
        let standalone = Store::new(
            dir.join("install"),
            dir.join("data"),
            crate::store::Release::standalone(),
        );
        standalone.refresh_liveness(&mut state);

        assert_eq!(state.install.version, "2.0.3", "the identity survives");
        assert_eq!(state.install.channel, "canary", "the channel survives");
        assert_eq!(
            state.baseline.sha256, "b1946ac92492d2347c6235b4d2611184",
            "the baseline digest survives: a process that claims no release cannot replace it"
        );
        assert_ne!(state.service.pid, 111, "the liveness is this process");
        assert_eq!(state.last_verdict.outcome, VerdictOutcome::Ok);
        assert!(
            state.last_verdict.detail.contains("unclaimed"),
            "the verdict says what kind of start this was: {}",
            state.last_verdict.detail
        );
    }

    /// The recovery a torn document needs: the bytes are kept as evidence, and the next load starts
    /// from the document the install can rebuild instead of requiring an operator to delete a file.
    #[test]
    fn a_corrupt_document_is_quarantined_before_a_fresh_start() {
        let dir = std::env::temp_dir().join(format!("summrise-store-q-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let store = Store::new(&dir, &dir, Release::standalone());
        std::fs::write(store.state_path(), b"{broken").expect("write a torn document");

        let quarantined = store.quarantine_state().expect("the file is moved aside");
        assert!(quarantined.exists(), "the bytes are kept as evidence");
        assert!(
            !store.state_path().exists(),
            "the corrupt document is out of the way"
        );
        let fresh = store
            .load()
            .expect("a missing document starts from the install's facts");
        assert_eq!(fresh.install.dir, dir.display().to_string());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_corrupt_document_is_an_error_not_an_absent_file() {
        let dir = TempDir::new("corrupt");
        let (store, data) = store_in(&dir);
        std::fs::create_dir_all(&data).expect("the data directory is creatable");

        std::fs::write(data.join(STATE_FILE), "{ this is not a state document")
            .expect("the fixture is written");
        let error = store.load().expect_err("corrupt JSON is not a fresh start");
        assert!(
            matches!(error, StoreError::Corrupt(_)),
            "a parse failure is Corrupt, got {error:?}"
        );

        // A document from a future schema is refused for the same reason: this build cannot say
        // what its fields mean, and guessing is how a reader shows a confident wrong answer.
        let future = serde_json::json!({
            "schema": SCHEMA_VERSION + 1,
            "install": { "dir": "", "version": "", "channel": "" },
            "baseline": { "version": "", "sha256": "", "exe": "" },
            "service": { "state": "running", "pid": 1, "startedAt": "" },
            "lastVerdict": { "ts": "", "outcome": "ok", "detail": "" },
            "sessions": [],
            "update": { "phase": "idle", "to": null }
        });
        std::fs::write(data.join(STATE_FILE), future.to_string()).expect("the fixture is written");
        let error = store
            .load()
            .expect_err("an unknown schema is not this build's state");
        assert!(matches!(error, StoreError::Corrupt(_)));
    }

    #[test]
    fn a_leftover_temporary_file_is_never_read_as_state() {
        let dir = TempDir::new("leftover-tmp");
        let (store, data) = store_in(&dir);
        std::fs::create_dir_all(&data).expect("the data directory is creatable");
        std::fs::write(data.join(LEGACY_TMP_FILE), "half a document, never renamed")
            .expect("the leftover is written");

        let fresh = store
            .load()
            .expect("a leftover .tmp is not the state file and not an error");
        assert_eq!(fresh.schema, SCHEMA_VERSION);
        assert!(fresh.sessions.is_empty());

        let state = sample_state(&store);
        store.save(&state).expect("the document saves");
        assert_eq!(store.load().expect("the document loads"), state);
        assert!(
            data.join(LEGACY_TMP_FILE).exists(),
            "the save never writes the legacy fixed name, so it cannot remove or follow it"
        );
        let leftovers: Vec<String> = std::fs::read_dir(&data)
            .expect("the data directory is readable")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp") && name != LEGACY_TMP_FILE)
            .collect();
        assert!(
            leftovers.is_empty(),
            "the save renames its random temporary file away; leftovers: {leftovers:?}"
        );
    }

    #[test]
    fn a_temporary_file_never_shadows_a_valid_document() {
        let dir = TempDir::new("shadow");
        let (store, data) = store_in(&dir);
        let state = sample_state(&store);
        store.save(&state).expect("the document saves");
        std::fs::write(data.join(LEGACY_TMP_FILE), "a newer half-write")
            .expect("the fixture is written");

        assert_eq!(
            store.load().expect("the state file is read, not the tmp"),
            state
        );
    }

    #[test]
    fn saving_creates_the_data_directory() {
        let dir = TempDir::new("mkdir");
        let data = dir.join("nested").join("data");
        let store = Store::new(dir.join("install"), &data, Release::standalone());
        let state = sample_state(&store);

        store.save(&state).expect("the document saves");

        assert!(data.join(STATE_FILE).is_file());
    }

    // ─────────────────────────────────────────────────────────────────────────────────────────────
    // The supervisor's swap result (I1)
    // ─────────────────────────────────────────────────────────────────────────────────────────────

    fn write_result(store: &Store, outcome: &str, version: Option<&str>, at_ms: u64) {
        write_result_with_sha(store, outcome, version, None, at_ms);
    }

    fn write_result_with_sha(
        store: &Store,
        outcome: &str,
        version: Option<&str>,
        sha256: Option<&str>,
        at_ms: u64,
    ) {
        let mut document = serde_json::json!({ "outcome": outcome, "at_ms": at_ms });
        if let Some(version) = version {
            document["version"] = serde_json::json!(version);
        }
        if let Some(sha256) = sha256 {
            document["sha256"] = serde_json::json!(sha256);
        }
        if outcome != "applied" {
            document["reason"] = serde_json::json!("the candidate did not serve health");
        }
        let path = store.swap_result_path();
        std::fs::create_dir_all(path.parent().expect("the result has a parent"))
            .expect("the install directory is creatable");
        std::fs::write(path, document.to_string()).expect("the supervisor's result is written");
    }

    /// The request the CLI leaves behind, exactly as `summrise_device::update::request_swap` writes
    /// it (the stager no longer touches the state document).
    fn write_request(store: &Store, version: &str, requested_at_ms: u64) {
        let path = store.swap_request_path();
        std::fs::create_dir_all(path.parent().expect("the request has a parent"))
            .expect("the install directory is creatable");
        std::fs::write(
            path,
            serde_json::json!({
                "requested_at_ms": requested_at_ms,
                "version": version,
                "sha256": "b1946ac92492d2347c6235b4d2611184",
            })
            .to_string(),
        )
        .expect("the request is written");
    }

    fn saved(store: &Store) -> State {
        let mut state = store.initial();
        store.save(&state).expect("the fixture document saves");
        state.sessions.clear();
        state
    }

    #[test]
    fn a_stage_request_sets_the_phase_and_the_target() {
        let dir = TempDir::new("stage-request");
        let (store, _) = store_in(&dir);
        saved(&store);
        write_request(&store, "2.0.4", 1_000);

        let mut state = store.load().expect("the document loads");
        assert!(
            store
                .ingest_swap_request(&mut state)
                .expect("the request reads"),
            "a request for a version the document does not name changes it"
        );
        store.save(&state).expect("the document saves");

        let state = store.load().expect("the document loads");
        assert_eq!(state.update.phase, UpdatePhase::Staged);
        assert_eq!(state.update.to.as_deref(), Some("2.0.4"));
        assert_eq!(state.update.outcome, None);
    }

    #[test]
    fn the_same_stage_request_is_not_recorded_twice() {
        let dir = TempDir::new("stage-idempotent");
        let (store, _) = store_in(&dir);
        saved(&store);
        write_request(&store, "2.0.4", 1_000);

        let mut state = store.load().expect("the document loads");
        assert!(store
            .ingest_swap_request(&mut state)
            .expect("the request reads"));
        store.save(&state).expect("the document saves");

        let mut again = store.load().expect("the document loads");
        assert!(
            !store
                .ingest_swap_request(&mut again)
                .expect("the second read is quiet"),
            "the document already names 2.0.4; a repeated request is not a second record"
        );
        assert_eq!(again.update.phase, UpdatePhase::Staged);
        assert_eq!(again.update.to.as_deref(), Some("2.0.4"));
    }

    #[test]
    fn a_swap_request_never_invents_a_document_when_the_app_was_never_started() {
        let dir = TempDir::new("stage-no-document");
        let (store, _) = store_in(&dir);
        write_request(&store, "2.0.4", 1_000);

        assert!(
            !store
                .reconcile_swap()
                .expect("a request without a document is not an I/O failure"),
            "the stager is not the app serving the device, so it must not write a fresh document"
        );
        assert!(
            !store.exists(),
            "no state document is invented for a swap request"
        );
    }

    #[test]
    fn reconcile_swap_records_the_request_then_the_result() {
        let dir = TempDir::new("swap-both");
        let (store, _) = store_in(&dir);
        saved(&store);
        write_request(&store, "2.0.4", 1_000);
        write_result(&store, "applied", Some("2.0.4"), 1_001);

        assert!(
            store.reconcile_swap().expect("both files read"),
            "the request and the result are folded into one saved document"
        );

        let state = store.load().expect("the document loads");
        assert_eq!(
            state.update.phase,
            UpdatePhase::Idle,
            "the result is the last word on a swap that finished"
        );
        assert_eq!(state.update.outcome, Some(UpdateOutcome::Applied));
        assert_eq!(state.update.to.as_deref(), Some("2.0.4"));
        assert_eq!(state.update.last_result_at_ms, Some(1_001));
        assert_eq!(state.last_verdict.detail, "updated to 2.0.4");
    }

    #[test]
    fn an_unusable_swap_request_is_ignored_rather_than_guessed_at() {
        let dir = TempDir::new("request-torn");
        let (store, _) = store_in(&dir);
        saved(&store);
        let path = store.swap_request_path();
        std::fs::create_dir_all(path.parent().expect("the request has a parent"))
            .expect("the install directory is creatable");
        std::fs::write(&path, "{ not a request").expect("the torn request is written");

        assert!(
            !store
                .reconcile_swap()
                .expect("a torn request is not an I/O failure"),
            "an unreadable request changes nothing"
        );
        let state = store.load().expect("the document loads");
        assert_eq!(state.update.phase, UpdatePhase::Idle);
        assert_eq!(state.update.to, None);
    }

    #[test]
    fn an_applied_result_is_recorded_once_and_becomes_the_last_verdict() {
        let dir = TempDir::new("result-applied");
        let (store, _) = store_in(&dir);
        saved(&store);
        write_result(&store, "applied", Some("2.0.4"), 1_000);

        let mut state = store.load().expect("the document loads");
        assert!(store
            .apply_swap_result(&mut state)
            .expect("the result reads"));
        store.save(&state).expect("the document saves");

        let state = store.load().expect("the document loads");
        assert_eq!(state.update.phase, UpdatePhase::Idle);
        assert_eq!(state.update.outcome, Some(UpdateOutcome::Applied));
        assert_eq!(state.update.last_result_at_ms, Some(1_000));
        assert_eq!(state.update.to.as_deref(), Some("2.0.4"));
        assert_eq!(state.last_verdict.outcome, VerdictOutcome::Ok);
        assert_eq!(state.last_verdict.detail, "updated to 2.0.4");

        // The same result is not applied twice, even after a restart.
        let mut again = store.load().expect("the document loads");
        assert!(
            !store
                .apply_swap_result(&mut again)
                .expect("the result reads"),
            "at_ms is the idempotence key"
        );
    }

    #[test]
    fn a_rolled_back_result_names_the_version_and_needs_attention() {
        let dir = TempDir::new("result-rolled-back");
        let (store, _) = store_in(&dir);
        saved(&store);
        write_result(&store, "rolled_back", Some("2.0.5"), 2_000);

        let mut state = store.load().expect("the document loads");
        assert!(store
            .apply_swap_result(&mut state)
            .expect("the result reads"));
        store.save(&state).expect("the document saves");

        let state = store.load().expect("the document loads");
        assert_eq!(state.update.outcome, Some(UpdateOutcome::RolledBack));
        assert_eq!(state.update.last_result_at_ms, Some(2_000));
        assert_eq!(state.last_verdict.outcome, VerdictOutcome::NeedsAttention);
        assert_eq!(state.last_verdict.detail, "rolled back from 2.0.5");
    }

    /// An older result (written before the launcher put the digest in it) falls back to the staged
    /// pair's sidecar, which is the artifact the launcher verified before the swap.
    fn write_staged_sidecar(store: &Store, version: &str, sha256: &str) {
        let path = store.install.join(crate::update::STAGED_SIDECAR);
        std::fs::create_dir_all(path.parent().expect("the sidecar has a parent"))
            .expect("the install directory is creatable");
        std::fs::write(
            path,
            serde_json::json!({
                "version": version,
                "channel": "stable",
                "sha256": sha256,
                "signature": "00".repeat(64),
            })
            .to_string(),
        )
        .expect("the staged sidecar is writable");
    }

    #[test]
    fn a_rolled_back_result_records_the_artifact_digest() {
        let dir = TempDir::new("result-rolled-back-digest");
        let (store, _) = store_in(&dir);
        saved(&store);
        let sha256 = "ab".repeat(32);
        // No staged sidecar: the result itself carries the digest the launcher verified, which is
        // what makes a rollback via `--apply` (or one whose pair was removed) recordable.
        write_result_with_sha(&store, "rolled_back", Some("2.0.5"), Some(&sha256), 2_000);

        let mut state = store.load().expect("the document loads");
        assert!(store
            .apply_swap_result(&mut state)
            .expect("the result reads"));
        store.save(&state).expect("the document saves");

        let state = store.load().expect("the document loads");
        assert_eq!(state.rolled_back.len(), 1);
        assert_eq!(state.rolled_back[0].sha256, sha256);
        assert_eq!(state.rolled_back[0].version, "2.0.5");
        assert_eq!(state.rolled_back[0].at_ms, 2_000);
    }

    #[test]
    fn an_old_rolled_back_result_falls_back_to_the_staged_sidecar() {
        let dir = TempDir::new("result-rolled-back-fallback");
        let (store, _) = store_in(&dir);
        saved(&store);
        let sha256 = "bc".repeat(32);
        write_staged_sidecar(&store, "2.0.5", &sha256);
        write_result(&store, "rolled_back", Some("2.0.5"), 2_000);

        let mut state = store.load().expect("the document loads");
        assert!(store
            .apply_swap_result(&mut state)
            .expect("the result reads"));
        store.save(&state).expect("the document saves");

        let state = store.load().expect("the document loads");
        assert_eq!(state.rolled_back.len(), 1);
        assert_eq!(state.rolled_back[0].sha256, sha256);
        assert_eq!(state.rolled_back[0].version, "2.0.5");
        assert_eq!(state.rolled_back[0].at_ms, 2_000);
    }

    #[test]
    fn the_same_rolled_back_artifact_is_not_recorded_twice() {
        let dir = TempDir::new("result-rolled-back-duplicate");
        let (store, _) = store_in(&dir);
        saved(&store);
        let sha256 = "cd".repeat(32);

        for at_ms in [2_000, 3_000] {
            write_result_with_sha(&store, "rolled_back", Some("2.0.5"), Some(&sha256), at_ms);
            let mut state = store.load().expect("the document loads");
            assert!(store
                .apply_swap_result(&mut state)
                .expect("the result reads"));
            store.save(&state).expect("the document saves");
        }

        let state = store.load().expect("the document loads");
        assert_eq!(
            state.rolled_back.len(),
            1,
            "one digest is one artifact, however many results name it"
        );
        assert_eq!(
            state.rolled_back[0].at_ms, 2_000,
            "the first failure keeps its time"
        );
    }

    #[test]
    fn the_rolled_back_list_keeps_only_the_newest_sixteen() {
        let dir = TempDir::new("result-rolled-back-cap");
        let (store, _) = store_in(&dir);
        saved(&store);

        for index in 0..=16u64 {
            let sha256 = format!("{index:064x}");
            write_result_with_sha(
                &store,
                "rolled_back",
                Some("2.0.5"),
                Some(&sha256),
                2_000 + index,
            );
            let mut state = store.load().expect("the document loads");
            assert!(store
                .apply_swap_result(&mut state)
                .expect("the result reads"));
            store.save(&state).expect("the document saves");
        }

        let state = store.load().expect("the document loads");
        assert_eq!(state.rolled_back.len(), MAX_ROLLED_BACK);
        assert_eq!(
            state.rolled_back[0].sha256,
            format!("{:064x}", 1),
            "the oldest artifact is the one evicted"
        );
        assert_eq!(
            state.rolled_back[MAX_ROLLED_BACK - 1].sha256,
            format!("{:064x}", 16),
            "the newest artifact is kept"
        );
    }

    #[test]
    fn a_rollback_without_its_staged_artifact_is_not_guessed_at() {
        let dir = TempDir::new("result-rolled-back-no-sidecar");
        let (store, _) = store_in(&dir);
        saved(&store);
        write_result(&store, "rolled_back", Some("2.0.5"), 2_000);

        let mut state = store.load().expect("the document loads");
        assert!(store
            .apply_swap_result(&mut state)
            .expect("the result reads"));
        store.save(&state).expect("the document saves");

        let state = store.load().expect("the document loads");
        assert!(
            state.rolled_back.is_empty(),
            "without the artifact's digest, the safeguard records no claim it cannot prove"
        );
        assert_eq!(state.update.outcome, Some(UpdateOutcome::RolledBack));
    }

    #[test]
    fn the_rolled_back_list_survives_a_restart() {
        let dir = TempDir::new("result-rolled-back-restart");
        let (store, _) = store_in(&dir);
        let mut state = saved(&store);
        state.record_rolled_back(RolledBackArtifact {
            sha256: "ef".repeat(32),
            version: "2.0.5".to_owned(),
            at_ms: 2_000,
        });
        store.save(&state).expect("the document saves");

        let reread = store.load().expect("the document loads after a restart");

        assert_eq!(reread.rolled_back, state.rolled_back);
    }

    #[test]
    fn a_refused_result_does_not_claim_a_repair() {
        let dir = TempDir::new("result-refused");
        let (store, _) = store_in(&dir);
        saved(&store);
        write_result(&store, "refused", None, 3_000);

        let mut state = store.load().expect("the document loads");
        assert!(store
            .apply_swap_result(&mut state)
            .expect("the result reads"));
        store.save(&state).expect("the document saves");

        let state = store.load().expect("the document loads");
        assert_eq!(state.update.outcome, Some(UpdateOutcome::Refused));
        assert_eq!(
            state.last_verdict.outcome,
            VerdictOutcome::NeedsAttention,
            "a refusal installed nothing; it is not a repair (I3)"
        );
        assert_eq!(
            state.last_verdict.detail,
            "refused: the candidate did not serve health"
        );
    }

    #[test]
    fn an_older_result_never_overwrites_a_newer_record() {
        let dir = TempDir::new("result-order");
        let (store, _) = store_in(&dir);
        saved(&store);
        write_result(&store, "applied", Some("2.0.4"), 5_000);
        let mut state = store.load().expect("the document loads");
        assert!(store
            .apply_swap_result(&mut state)
            .expect("the result reads"));
        store.save(&state).expect("the document saves");

        write_result(&store, "rolled_back", Some("2.0.6"), 4_999);
        let mut state = store.load().expect("the document loads");
        assert!(
            !store
                .apply_swap_result(&mut state)
                .expect("the result reads"),
            "a result older than the recorded one is already history"
        );
        assert_eq!(state.update.outcome, Some(UpdateOutcome::Applied));
    }

    #[test]
    fn a_torn_result_is_ignored_rather_than_guessed_at() {
        let dir = TempDir::new("result-torn");
        let (store, _) = store_in(&dir);
        saved(&store);
        let result_path = store.swap_result_path();
        std::fs::create_dir_all(result_path.parent().expect("the result has a parent"))
            .expect("the install directory is creatable");
        std::fs::write(result_path, "{ not a result").expect("the torn result is written");

        assert!(
            !store
                .reconcile_swap()
                .expect("a torn result is not an I/O failure"),
            "an unreadable result changes nothing"
        );
        let state = store.load().expect("the document loads");
        assert_eq!(state.update.outcome, None);
    }

    #[test]
    fn the_watcher_reconciles_a_result_written_while_the_app_serves() {
        let dir = TempDir::new("result-watch");
        let (store, _) = store_in(&dir);
        saved(&store);
        write_result(&store, "applied", Some("2.0.4"), 7_000);

        assert!(
            store.reconcile_swap().expect("the result reads"),
            "the reconciliation saves the changed document"
        );
        assert!(
            !store.reconcile_swap().expect("the second read is quiet"),
            "the recorded at_ms makes the poll idempotent"
        );
        let state = store.load().expect("the document loads");
        assert_eq!(state.update.outcome, Some(UpdateOutcome::Applied));
    }

    #[test]
    fn a_timestamp_is_written_as_rfc3339_utc() {
        let cases = [
            (0, "1970-01-01T00:00:00Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
            (1_700_000_000, "2023-11-14T22:13:20Z"),
            (1_900_000_000, "2030-03-17T17:46:40Z"),
        ];

        for (seconds, expected) in cases {
            assert_eq!(
                format_utc(seconds),
                expected,
                "{seconds} seconds after the epoch"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────────────────────
    // I3: the launcher's repair log
    // ─────────────────────────────────────────────────────────────────────────────────────────────

    fn repair_line(ts_ms: u64, sha256: &str) -> String {
        let mut line = serde_json::json!({
            "ts_ms": ts_ms,
            "kind": "repaired",
            "from": "baseline",
            "sha256": sha256,
        })
        .to_string();
        line.push('\n');
        line
    }

    fn append_repairs(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().expect("the repair log has a parent"))
            .expect("the install directory is creatable");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("the repair log is writable");
        file.write_all(text.as_bytes())
            .expect("the repair line is writable");
    }

    /// The reader's contract: only new bytes, and only complete lines. A line the launcher is still
    /// writing must not be parsed (it would lose a `}` and be skipped forever); it is consumed once
    /// its newline lands.
    #[test]
    fn the_repair_reader_consumes_complete_lines_only_and_returns_the_new_offset() {
        let dir = TempDir::new("repair-reader");
        let path = dir.join("repair.log");
        let first = repair_line(1_000, &"aa".repeat(32));
        let second = repair_line(2_000, &"bb".repeat(32));
        let partial =
            r#"{"ts_ms":3000,"kind":"repaired","from":"baseline","sha256":"cccc"#.to_string();
        fs::write(&path, format!("{first}{second}{partial}")).expect("the fixture is written");

        let (records, offset) = read_new_repairs(&path, 0).expect("the log reads");
        assert_eq!(records.len(), 2, "the partial line is not consumed");
        assert_eq!(records[0].ts_ms, 1_000);
        assert_eq!(records[0].sha256, "aa".repeat(32));
        assert_eq!(records[1].ts_ms, 2_000);
        assert_eq!(
            offset,
            (first.len() + second.len()) as u64,
            "the offset is the byte after the last complete line"
        );

        let (records, unchanged) = read_new_repairs(&path, offset).expect("the log reads again");
        assert!(records.is_empty(), "a partial line yields no record");
        assert_eq!(unchanged, offset, "and it does not advance the offset");

        append_repairs(&path, "dd\"}\n");
        let (records, advanced) = read_new_repairs(&path, offset).expect("the log reads again");
        assert_eq!(records.len(), 1, "the completed line is consumed");
        assert_eq!(records[0].sha256, "ccccdd");
        assert_eq!(
            advanced,
            offset + (partial.len() + 5) as u64,
            "the offset covers the completed line"
        );
    }

    #[test]
    fn a_repair_offset_past_the_end_is_treated_as_a_rotated_log() {
        let dir = TempDir::new("repair-reader-truncated");
        let path = dir.join("repair.log");
        let line = repair_line(5_000, &"ee".repeat(32));
        fs::write(&path, &line).expect("the fixture is written");

        let (records, offset) =
            read_new_repairs(&path, 99_999).expect("a shorter file is not an error");
        assert_eq!(
            records.len(),
            1,
            "a stale offset (rotation, replacement) reads from the start"
        );
        assert_eq!(offset, line.len() as u64);
    }

    /// The verdict and the event log are the evidence gate 1 names, and the offset in the state
    /// document is what keeps a restart from recording the same repair twice.
    #[test]
    fn a_start_ingests_new_repairs_into_the_verdict_and_the_event_log() {
        let dir = TempDir::new("repair-ingest");
        let (store, _) = store_in(&dir);
        saved(&store);
        let sha = "ab".repeat(32);
        append_repairs(
            &store.repair_log_path(),
            &repair_line(1_700_000_000_000, &sha),
        );

        let mut state = store.load().expect("the document loads");
        assert!(
            store.ingest_repairs(&mut state).expect("the log is read"),
            "a new repair changes the document"
        );
        store.save(&state).expect("the document saves");

        let state = store.load().expect("the document loads");
        assert_eq!(state.last_verdict.outcome, VerdictOutcome::Repaired);
        assert!(
            state.last_verdict.detail.contains("repaired from baseline"),
            "the verdict is the sentence the operator quotes: {}",
            state.last_verdict.detail
        );
        assert!(state.last_verdict.ts.starts_with("2023-11-14T22:13:20Z"));
        assert!(state.repair_offset > 0, "the document tracks its position");

        let events_path = dir.join("data").join("events.jsonl");
        let events = fs::read_to_string(&events_path).expect("the event log is written");
        assert_eq!(events.lines().count(), 1);
        let event: serde_json::Value =
            serde_json::from_str(events.trim()).expect("the event is JSON");
        assert_eq!(event["event"], "repaired_from_baseline");
        assert_eq!(event["from"], "baseline");
        assert_eq!(event["sha256"], sha);
        assert_eq!(event["ts_ms"], 1_700_000_000_000u64);

        let mut state = store.load().expect("the document loads");
        assert!(
            !store
                .ingest_repairs(&mut state)
                .expect("the second read is quiet"),
            "the offset makes the ingest idempotent across restarts"
        );
        let events = fs::read_to_string(&events_path).expect("the event log is written");
        assert_eq!(
            events.lines().count(),
            1,
            "a restart does not record the same repair again"
        );
    }

    /// A device that booted the retired generation says so: the sentence, the event name and the
    /// source are all the previous generation, not the verified current pair.
    #[test]
    fn a_start_that_booted_the_previous_generation_says_previous() {
        let dir = TempDir::new("repair-ingest-previous");
        let (store, _) = store_in(&dir);
        saved(&store);
        let sha = "cd".repeat(32);
        let line = serde_json::json!({
            "ts_ms": 1_700_000_000_000u64,
            "kind": "repaired",
            "from": "previous",
            "sha256": sha,
        })
        .to_string()
            + "\n";
        append_repairs(&store.repair_log_path(), &line);

        let mut state = store.load().expect("the document loads");
        assert!(store.ingest_repairs(&mut state).expect("the log is read"));
        assert_eq!(
            state.last_verdict.detail, "repaired from previous",
            "the verdict names the generation that was booted"
        );
        let events = fs::read_to_string(dir.join("data").join("events.jsonl"))
            .expect("the event log is written");
        let event: serde_json::Value = serde_json::from_str(events.trim()).expect("JSON");
        assert_eq!(event["event"], "repaired_from_previous");
        assert_eq!(event["from"], "previous");
    }
}
