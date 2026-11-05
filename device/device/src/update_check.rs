//! When this device looks for a release, and why.
//!
//! A schedule alone assumes the machine is awake when the timer fires. It is not: the two devices
//! measured on 2026-11-05 sleep when idle, so a scheduled check that fires during sleep never runs
//! and the device can sit on an old release indefinitely. The console channel is the wake signal —
//! it drops when the machine sleeps and reconnects when it wakes — and the service start is the
//! other. Both ask the same function the schedule asks; none of them can bypass the guard.
//!
//! **One function, one guard.** [`UpdateChecker::trigger`] is the only way a check starts. It takes
//! at most one attempt in flight, at most one attempt in any ten minutes, and after a failed probe
//! it refuses every trigger for thirty minutes. The spacing is persisted in `state.json`
//! (`update.lastCheck`), so a crash-looping service does not turn a restart into a check loop.
//!
//! **A check does not fight an install.** A non-idle update phase ([`UpdatePhase`]) means a stage,
//! a swap or a pointer move is in progress; the trigger returns without touching the network.
//!
//! **The decision stays in one place.** This module owns *when*; [`crate::updater`] owns *whether*.
//! The probe is the same `check_update_with` the operator's `--check-update` calls, with the same
//! key, install root and rollback memory, and the same handoff to the launcher.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use summrise_core::{State, UpdateCheckTrigger, UpdatePhase};
use tokio::task::JoinHandle;

use crate::store::Store;
use crate::update_source::UpdateSource;
use crate::updater::{self, UpdateConfig, UpdateError};

/// At most one check in this window, whichever trigger asked for it.
pub const CHECK_SPACING: Duration = Duration::from_secs(10 * 60);

/// After a failed probe, refuse every trigger for this long instead of retrying in a loop.
pub const FAILURE_BACKOFF: Duration = Duration::from_secs(30 * 60);

/// A console connection is a wake only after an absence longer than this. A channel that flaps
/// every few seconds is not a machine coming back from sleep.
pub const CHANNEL_AWAY: Duration = Duration::from_secs(5 * 60);

/// The clock the guard reads. Millisecond Unix time, like `state.json`.
type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// The one decision a check makes, as a future: `Ok(())` when it staged and handed off (or found
/// nothing), the updater's own error when the probe failed.
type ProbeFuture = Pin<Box<dyn Future<Output = Result<(), UpdateError>> + Send>>;

/// The probe: given the state snapshot the guard already read, decide and stage.
type Probe = Arc<dyn Fn(State) -> ProbeFuture + Send + Sync>;

/// How one [`UpdateChecker::trigger`] ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckRun {
    /// A probe ran to completion.
    Ran,
    /// The probe failed; the failure backoff is now in force.
    Failed,
    /// Nothing was attempted, and why.
    Skipped(Skip),
}

/// Why a trigger did nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skip {
    /// Another check owns the slot.
    InFlight,
    /// The last check was less than [`CHECK_SPACING`] ago.
    Recent,
    /// The last probe failed less than [`FAILURE_BACKOFF`] ago.
    Backoff,
    /// An install or pointer switch is in progress.
    Install,
}

/// The guard's mutable state, held only across a decision, never across an await.
struct Guard {
    in_flight: bool,
    last_check_ms: Option<u64>,
    backoff_until_ms: Option<u64>,
    last_channel_connect_ms: Option<u64>,
}

/// The one way this device starts an update check.
pub struct UpdateChecker {
    store: Store,
    clock: Clock,
    probe: Probe,
    guard: Mutex<Guard>,
}

impl UpdateChecker {
    /// A checker over `store` that reads `clock` and asks `probe` to decide.
    pub fn new(store: Store, clock: Clock, probe: Probe) -> Self {
        // The spacing survives a restart: the last check is in the state document, not in this
        // process's memory. A service that crash-loops must not check once per loop.
        let last_check_ms = store
            .load()
            .ok()
            .and_then(|state| state.update.last_check)
            .map(|check| check.at_ms);
        Self {
            store,
            clock,
            probe,
            guard: Mutex::new(Guard {
                in_flight: false,
                last_check_ms,
                backoff_until_ms: None,
                last_channel_connect_ms: None,
            }),
        }
    }

    /// The production checker: the signed-document probe, the system clock, the device's own facts.
    pub fn production(
        store: Store,
        install: PathBuf,
        data: PathBuf,
        source: UpdateSource,
        cfg: UpdateConfig,
        supervised: bool,
        port: u16,
    ) -> Self {
        let clock: Clock = Arc::new(crate::update::now_ms);
        let probe = live_probe(install, data, source, cfg, supervised, port);
        Self::new(store, clock, probe)
    }

    /// The service-start trigger. The check is a task, not a delay in the start path: a mirror that
    /// takes minutes must not keep the device from serving.
    pub fn start(self: &Arc<Self>) -> JoinHandle<()> {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            this.trigger(UpdateCheckTrigger::Start).await;
        })
    }

    /// Note a successful console connection and, if it is a wake, ask for a check.
    ///
    /// The first connection after a start is a wake (the process has no earlier connection to
    /// compare). Afterwards a connection is a wake only when the channel was away for more than
    /// [`CHANNEL_AWAY`]: a reconnect a second after a dropped socket is the network, not the machine
    /// coming back. The returned handle is the spawned check, for a test that wants to await it; the
    /// console drops it.
    pub fn note_channel_connected(self: &Arc<Self>) -> Option<JoinHandle<()>> {
        let now = (self.clock)();
        let wake = {
            let mut guard = self.guard.lock().expect("the update guard is not poisoned");
            let wake = match guard.last_channel_connect_ms {
                None => true,
                Some(last) => now.saturating_sub(last) > millis(CHANNEL_AWAY),
            };
            guard.last_channel_connect_ms = Some(now);
            wake
        };
        if !wake {
            return None;
        }
        let this = Arc::clone(self);
        Some(tokio::spawn(async move {
            this.trigger(UpdateCheckTrigger::Channel).await;
        }))
    }

    /// The one entry point every trigger uses.
    pub async fn trigger(&self, trigger: UpdateCheckTrigger) -> CheckRun {
        let now = (self.clock)();
        {
            let mut guard = self.guard.lock().expect("the update guard is not poisoned");
            if guard.in_flight {
                return CheckRun::Skipped(Skip::InFlight);
            }
            if guard.backoff_until_ms.is_some_and(|until| now < until) {
                return CheckRun::Skipped(Skip::Backoff);
            }
            if guard
                .last_check_ms
                .is_some_and(|last| now < last.saturating_add(millis(CHECK_SPACING)))
            {
                return CheckRun::Skipped(Skip::Recent);
            }
            guard.in_flight = true;
        }

        // The state carries the facts the decision needs — the version this device runs and the
        // artifacts it rolled back. A state that cannot be read is not a check that found nothing:
        // staging without the rollback memory is the bypass the review called I3, so this fails and
        // takes the backoff rather than guessing at a decision on a fact the device does not have.
        let state = match self.store.load() {
            Ok(state) => state,
            Err(error) => {
                eprintln!(
                    "summrise-device: update check ({}) skipped: cannot read the state: {error}",
                    trigger.as_str()
                );
                self.finish_failure(now);
                return CheckRun::Failed;
            }
        };
        if state.update.phase != UpdatePhase::Idle {
            self.guard
                .lock()
                .expect("the update guard is not poisoned")
                .in_flight = false;
            return CheckRun::Skipped(Skip::Install);
        }

        // The attempt is recorded before the network, not after: an update that exits the process
        // (a supervised pointer move) must still leave the line that says it checked.
        {
            let mut guard = self.guard.lock().expect("the update guard is not poisoned");
            guard.last_check_ms = Some(now);
        }
        if let Err(error) = self.store.record_update_check(now, trigger) {
            eprintln!("summrise-device: cannot record the update check: {error}");
        }

        match (self.probe)(state).await {
            Ok(()) => {
                let mut guard = self.guard.lock().expect("the update guard is not poisoned");
                guard.in_flight = false;
                guard.backoff_until_ms = None;
                CheckRun::Ran
            }
            Err(error) => {
                eprintln!(
                    "summrise-device: update check ({}): {error}",
                    trigger.as_str()
                );
                self.finish_failure(now);
                CheckRun::Failed
            }
        }
    }

    /// Release the slot and start the failure backoff.
    fn finish_failure(&self, now: u64) {
        let mut guard = self.guard.lock().expect("the update guard is not poisoned");
        guard.in_flight = false;
        guard.backoff_until_ms = Some(now.saturating_add(millis(FAILURE_BACKOFF)));
    }
}

/// The real probe: verify the signed document, stage the release, hand it to the launcher.
fn live_probe(
    install: PathBuf,
    data: PathBuf,
    source: UpdateSource,
    cfg: UpdateConfig,
    supervised: bool,
    port: u16,
) -> Probe {
    Arc::new(move |state: State| {
        let install = install.clone();
        let data = data.clone();
        let source = source.clone();
        Box::pin(async move {
            let pubkey = crate::update::release_pubkey().map_err(UpdateError::Key)?;
            let policy = updater::ReinstallPolicy {
                rolled_back: &state.rolled_back,
                force_reinstall: None,
            };
            let outcome = updater::check_update_with(
                &source.url,
                None,
                source.channel,
                &state.install.version,
                &install,
                &cfg,
                policy,
                &pubkey,
            )
            .await?;
            // A STAGED RELEASE IS NOT AN UPDATE UNTIL IT IS HANDED OFF.
            if let updater::CheckOutcome::Staged(staged) = outcome {
                hand_off_after_staging(&install, &data, &staged, supervised, port);
            }
            Ok(())
        }) as ProbeFuture
    })
}

/// Hand a release the stager has just written to whatever installs it on this layout.
pub(crate) fn hand_off_after_staging(
    install: &Path,
    data: &Path,
    staged: &crate::update::StagedRelease,
    supervised: bool,
    port: u16,
) {
    let options = crate::update::HandoffOptions::pointer()
        .with_data(Some(data.to_path_buf()))
        .with_port(Some(port));
    match crate::update::hand_off(install, staged, &options) {
        Ok(crate::update::Handoff::Swap { .. }) => eprintln!(
            "summrise-device: staged {} and asked the supervisor for the swap",
            staged.version
        ),
        Ok(crate::update::Handoff::Installed { .. }) => {
            eprintln!(
                "summrise-device: staged {} and installed it; {}",
                staged.version,
                if supervised {
                    "restarting from the new pointer"
                } else {
                    "a restart is required to run it"
                }
            );
            if supervised {
                crate::restart::exit_soon()();
            }
        }
        Err(error) => eprintln!(
            "summrise-device: staged {} but cannot install it: {error}",
            staged.version
        ),
    }
}

/// A checker for a test in another module: it counts probes and always succeeds.
#[cfg(test)]
pub(crate) fn test_checker(
    store: Store,
    probes: Arc<std::sync::atomic::AtomicUsize>,
) -> Arc<UpdateChecker> {
    use std::sync::atomic::Ordering;

    let clock: Clock = Arc::new(|| 1_000);
    let probe: Probe = Arc::new(move |_state: State| {
        probes.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) }) as ProbeFuture
    });
    Arc::new(UpdateChecker::new(store, clock, probe))
}

/// Milliseconds in a duration, saturating rather than panicking on an absurd value.
fn millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

    use summrise_core::UpdateCheck;

    use crate::config::Release;
    use crate::test_support::TempDir;

    struct Harness {
        dir: TempDir,
        store: Store,
        now: Arc<AtomicU64>,
        probes: Arc<AtomicUsize>,
        fail: Arc<AtomicBool>,
        gate: Arc<tokio::sync::Mutex<()>>,
        started: Arc<tokio::sync::Notify>,
        checker: Arc<UpdateChecker>,
    }

    impl Harness {
        fn new(start_ms: u64) -> Self {
            Self::seeded(start_ms, None)
        }

        /// A checker over a state document whose last check is `last_check_at` (or never).
        fn seeded(start_ms: u64, last_check_at: Option<u64>) -> Self {
            let dir = TempDir::new("update-check");
            let store = Store::new(dir.join("install"), dir.join("data"), Release::standalone());
            let mut state = store.initial();
            state.update.last_check = last_check_at.map(|at_ms| UpdateCheck {
                at_ms,
                trigger: UpdateCheckTrigger::Schedule,
            });
            store.save(&state).expect("the fixture document saves");

            let now = Arc::new(AtomicU64::new(start_ms));
            let probes = Arc::new(AtomicUsize::new(0));
            let fail = Arc::new(AtomicBool::new(false));
            let gate = Arc::new(tokio::sync::Mutex::new(()));
            let started = Arc::new(tokio::sync::Notify::new());

            let clock: Clock = {
                let now = Arc::clone(&now);
                Arc::new(move || now.load(Ordering::SeqCst))
            };
            let probe: Probe = {
                let probes = Arc::clone(&probes);
                let fail = Arc::clone(&fail);
                let gate = Arc::clone(&gate);
                let started = Arc::clone(&started);
                Arc::new(move |_state: State| {
                    probes.fetch_add(1, Ordering::SeqCst);
                    started.notify_one();
                    let fail = fail.load(Ordering::SeqCst);
                    let gate = Arc::clone(&gate);
                    Box::pin(async move {
                        let _slot = gate.lock().await;
                        if fail {
                            Err(UpdateError::InvalidUrl(
                                "the test mirror is down".to_owned(),
                            ))
                        } else {
                            Ok(())
                        }
                    }) as ProbeFuture
                })
            };
            let checker = Arc::new(UpdateChecker::new(store.clone(), clock, probe));
            Self {
                dir,
                store,
                now,
                probes,
                fail,
                gate,
                started,
                checker,
            }
        }

        fn probes(&self) -> usize {
            self.probes.load(Ordering::SeqCst)
        }

        fn advance(&self, by_ms: u64) {
            self.now.fetch_add(by_ms, Ordering::SeqCst);
        }

        fn set_phase(&self, phase: UpdatePhase) {
            let mut state = self.store.load().expect("the state is readable");
            state.update.phase = phase;
            self.store.save(&state).expect("the state is writable");
        }

        fn last_check(&self) -> Option<UpdateCheck> {
            self.store
                .load()
                .expect("the state is readable")
                .update
                .last_check
        }

        fn install(&self) -> PathBuf {
            self.dir.join("install")
        }
    }

    #[tokio::test]
    async fn a_start_runs_one_check() {
        let harness = Harness::new(1_000);

        harness
            .checker
            .start()
            .await
            .expect("the start check finishes");

        assert_eq!(harness.probes(), 1);
        assert_eq!(
            harness.last_check(),
            Some(UpdateCheck {
                at_ms: 1_000,
                trigger: UpdateCheckTrigger::Start,
            })
        );
    }

    #[tokio::test]
    async fn two_channel_connects_inside_ten_minutes_run_one_check() {
        let harness = Harness::new(0);

        let first = harness
            .checker
            .note_channel_connected()
            .expect("the first connection after a start is a wake");
        first.await.expect("the channel check finishes");
        assert_eq!(harness.probes(), 1);

        harness.advance(6 * 60_000);
        let second = harness
            .checker
            .note_channel_connected()
            .expect("six minutes away is a wake");
        second.await.expect("the channel check finishes");
        assert_eq!(
            harness.probes(),
            1,
            "the ten-minute spacing window vetoes the second check"
        );
    }

    #[tokio::test]
    async fn a_channel_away_for_only_four_minutes_is_not_a_wake() {
        let harness = Harness::new(0);
        let first = harness
            .checker
            .note_channel_connected()
            .expect("the first connection after a start is a wake");
        first.await.expect("the channel check finishes");

        harness.advance(4 * 60_000);
        assert!(
            harness.checker.note_channel_connected().is_none(),
            "four minutes away is not a wake"
        );

        // Four is not six: the boundary is strict, and five exactly is still not enough.
        harness.advance(60_000);
        assert!(
            harness.checker.note_channel_connected().is_none(),
            "exactly five minutes away is not more than five"
        );
    }

    #[tokio::test]
    async fn a_connect_after_six_minutes_away_runs_one_check() {
        let harness = Harness::new(0);
        // The channel was connected when an install started, so the first connection asked and the
        // install guard said no. That is not a check and must not consume the spacing window: six
        // minutes later, on the far side of the absence, the reconnect is the machine's first real
        // chance to look.
        harness.set_phase(UpdatePhase::Staged);
        let first = harness
            .checker
            .note_channel_connected()
            .expect("the first connection is a wake");
        first.await.expect("the skipped check task finishes");
        assert_eq!(harness.probes(), 0);

        harness.set_phase(UpdatePhase::Idle);
        harness.advance(6 * 60_000);
        let second = harness
            .checker
            .note_channel_connected()
            .expect("six minutes away is a wake");
        second.await.expect("the channel check finishes");

        assert_eq!(harness.probes(), 1);
        assert_eq!(
            harness.last_check().map(|check| check.trigger),
            Some(UpdateCheckTrigger::Channel)
        );
    }

    #[tokio::test]
    async fn a_failure_sets_the_thirty_minute_backoff() {
        let harness = Harness::new(0);
        harness.fail.store(true, Ordering::SeqCst);

        assert_eq!(
            harness.checker.trigger(UpdateCheckTrigger::Start).await,
            CheckRun::Failed
        );
        assert_eq!(harness.probes(), 1);

        harness.advance(29 * 60_000);
        assert_eq!(
            harness.checker.trigger(UpdateCheckTrigger::Channel).await,
            CheckRun::Skipped(Skip::Backoff)
        );
        assert_eq!(harness.probes(), 1, "the backoff must not probe");

        harness.fail.store(false, Ordering::SeqCst);
        harness.advance(60_000);
        assert_eq!(
            harness.checker.trigger(UpdateCheckTrigger::Schedule).await,
            CheckRun::Ran
        );
        assert_eq!(harness.probes(), 2);
    }

    #[tokio::test]
    async fn a_check_while_an_install_is_in_progress_does_nothing() {
        let harness = Harness::new(0);
        harness.set_phase(UpdatePhase::Staged);

        assert_eq!(
            harness.checker.trigger(UpdateCheckTrigger::Start).await,
            CheckRun::Skipped(Skip::Install)
        );
        assert_eq!(harness.probes(), 0);
        assert_eq!(harness.last_check(), None, "a skipped check is not a check");

        harness.set_phase(UpdatePhase::Idle);
        assert_eq!(
            harness.checker.trigger(UpdateCheckTrigger::Start).await,
            CheckRun::Ran,
            "the skipped attempt must not consume the spacing window"
        );
        assert_eq!(harness.probes(), 1);
    }

    #[tokio::test]
    async fn the_recorded_reason_matches_the_trigger() {
        let harness = Harness::new(100);

        assert_eq!(
            harness.checker.trigger(UpdateCheckTrigger::Start).await,
            CheckRun::Ran
        );
        assert_eq!(
            harness.last_check(),
            Some(UpdateCheck {
                at_ms: 100,
                trigger: UpdateCheckTrigger::Start,
            })
        );

        harness.advance(10 * 60_000 + 1);
        assert_eq!(
            harness.checker.trigger(UpdateCheckTrigger::Channel).await,
            CheckRun::Ran
        );
        assert_eq!(
            harness.last_check(),
            Some(UpdateCheck {
                at_ms: 10 * 60_000 + 101,
                trigger: UpdateCheckTrigger::Channel,
            })
        );

        harness.advance(10 * 60_000 + 1);
        assert_eq!(
            harness.checker.trigger(UpdateCheckTrigger::Schedule).await,
            CheckRun::Ran
        );
        assert_eq!(
            harness.last_check(),
            Some(UpdateCheck {
                at_ms: 2 * (10 * 60_000) + 102,
                trigger: UpdateCheckTrigger::Schedule,
            })
        );
    }

    #[tokio::test]
    async fn two_checks_never_run_at_the_same_time() {
        let harness = Harness::new(0);
        let held = harness.gate.lock().await;
        let checker = Arc::clone(&harness.checker);
        let first = tokio::spawn(async move { checker.trigger(UpdateCheckTrigger::Start).await });

        harness.started.notified().await;
        assert_eq!(
            harness.checker.trigger(UpdateCheckTrigger::Channel).await,
            CheckRun::Skipped(Skip::InFlight)
        );

        drop(held);
        assert_eq!(
            first.await.expect("the first check finishes"),
            CheckRun::Ran
        );
        assert_eq!(harness.probes(), 1);
    }

    #[tokio::test]
    async fn a_restart_inside_the_spacing_window_does_not_check_again() {
        // The last check is in the document, so a crash-loop's second start is not a second check.
        let harness = Harness::seeded(5 * 60_000, Some(0));

        assert_eq!(
            harness.checker.trigger(UpdateCheckTrigger::Start).await,
            CheckRun::Skipped(Skip::Recent)
        );
        assert_eq!(harness.probes(), 0);
    }

    #[test]
    fn a_staged_legacy_release_asks_the_supervisor_for_the_swap() {
        let dir = TempDir::new("update-check-handoff");
        let staged =
            crate::update::StagedRelease::pair(&dir.join("install"), "2.0.1", "ab".repeat(32));
        hand_off_after_staging(
            &dir.join("install"),
            &dir.join("data"),
            &staged,
            false,
            18_080,
        );
        let request = std::fs::read(dir.join("install").join(crate::store::SWAP_REQUEST_FILE))
            .expect("the supervisor is asked; a staged release alone is not an update");
        let parsed: serde_json::Value =
            serde_json::from_slice(&request).expect("the request is JSON");
        assert_eq!(parsed["version"], "2.0.1");
        assert_eq!(parsed["sha256"], "ab".repeat(32));
        assert!(parsed["requested_at_ms"].as_u64().is_some_and(|ms| ms > 0));
    }

    #[tokio::test]
    async fn an_up_to_date_check_writes_no_update() {
        let harness = Harness::new(0);

        assert_eq!(
            harness.checker.trigger(UpdateCheckTrigger::Start).await,
            CheckRun::Ran
        );

        let state = harness.store.load().expect("the state is readable");
        assert_eq!(state.update.phase, UpdatePhase::Idle);
        assert_eq!(state.update.to, None);
        assert!(state.update.last_check.is_some());
        let install = harness.install();
        assert!(!install.join(crate::update::STAGED_EXE).exists());
        assert!(!install.join(crate::update::STAGED_SIDECAR).exists());
        assert!(!install.join(crate::store::SWAP_REQUEST_FILE).exists());
    }
}
