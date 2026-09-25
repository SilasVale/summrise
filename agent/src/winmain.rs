//! winmain.rs — Windows-only process plumbing (structure refactor A7: moved
//! verbatim from main.rs; zero behavior change). Everything in this module is
//! `#[cfg(windows)]`: the boot self-heal, the kill-on-close child-reaper job,
//! the bounded helper runner, the SCM service entry, and the supervised
//! cloudflared tunnel owner. Non-Windows builds never compile any of it —
//! main.rs keeps only the `winmain::…` call sites, equally cfg-gated.

#![cfg(windows)]

use std::path::PathBuf;

use crate::{log_line, run_server};

/// The SCM service name — deliberately the LEGACY name ("SummriseCommand"): the
/// service was registered under it by the old install path and re-registering
/// under a new name would orphan existing installs. (Moved here with the
/// windows-only plumbing — its only consumers are cfg(windows).)
pub(crate) const SERVICE_NAME: &str = "SummriseCommand";

/// C2 unified process model — the AGENT owns the cloudflared tunnel:
/// spawn-if-absent from the BOXED component (`paths::cloudflared_bin()`,
/// i.e. `<install>\components\cloudflared.exe`) with `--config`
/// `paths::tunnel_file()` (`<install>\etc\tunnel.yml`). No Windows
/// service, no external owner, single supervision path (setup no longer
/// installs the legacy service; an upgrade removes it).
///
/// THE PATHS ARE NAMED THROUGH `paths.rs` AND NOT SPELLED OUT HERE. The
/// comments in this function used to say `tools\` and the code agreed with
/// them, which is how a supervisor spent months polling a directory that no
/// longer exists — see the note at the probe below.
pub(crate) fn supervise_tunnel() {
    // C2: cloudflared is a BOXED COMPONENT and the AGENT owns the tunnel
    // lifecycle (spawn-if-absent on boot). No Windows service, no external
    // owner — this is the single supervision path.
    // Supervision audit #1: the OLD code spawned cloudflared once,
    // fire-and-forget — a tunnel that exited (CF network-fatal, cert
    // churn, OOM) left the device DARK while /api/status kept answering,
    // and provision_tunnel could stack a SECOND concurrent tunnel. One
    // supervisor task now owns the child for the process lifetime:
    // respawn with capped backoff (reset after a healthy minute) and a
    // RESTART when tunnel_ctl's generation bumps (fresh tunnel.yml).
    // CRITICAL (d1 530 incident, round-80): this block runs in main()
    // BEFORE the runtime exists — tokio::spawn HERE PANICKED the service
    // at boot on Windows only (cfg(windows) elided from Linux checks),
    // killing the agent and the tunnel = device unreachable. Own a
    // private current-thread runtime on a plain thread: correct in ANY
    // context, panic-proof placement.
    std::thread::spawn(move || {
        // No install root is captured: the paths.rs helpers resolve the two files this
        // thread needs, per read, and carrying the root here is exactly how the hand-joined
        // legacy paths got in.
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                log_line(&format!("cloudflared supervisor: no runtime: {e}"));
                return;
            }
        };
        rt.block_on(async move {
            use std::time::{Duration, Instant};
            let mut backoff: u64 = 5;
            // Only so the not-staged line is not repeated every 30 s forever.
            let mut warned_absent = false;
            loop {
                // THE PATHS COME FROM `paths.rs`, and that is the whole bug this fixes:
                // these were hand-joined as `install_dir\tools\cloudflared.exe` and
                // `install_dir\tunnel.yml` — the PRE-layout-v2 locations — so on a v2
                // install the `exists()` check below was ALWAYS false and this loop
                // polled every 30 s forever. The tunnel therefore only ever ran when
                // something else started it (a manual `summrise tunnel start`, provisioning
                // from the Settings card), and after an update restarted the agent
                // nothing brought it back: the device went dark (Cloudflare 530) until a
                // human noticed. `cloudflared_bin()`/`tunnel_file()` are registry-first
                // AND carry the legacy-path migration, so the hand-joined copies were
                // also the one place that would not have followed a future move.
                let cf = summrise_agent::paths::cloudflared_bin();
                let cfg = summrise_agent::paths::tunnel_file();
                if !(cf.exists() && cfg.exists()) {
                    // SAY IT ONCE. The original logged NOTHING here, which is why a
                    // supervisor that could never do its job left no trace at all: the
                    // d1 incident was diagnosed from an absent process and a 530, not
                    // from the log. A supervisor that can do nothing must still name the
                    // files it is waiting for.
                    if !warned_absent {
                        log_line(&format!(
                            "cloudflared tunnel: not staged yet — waiting for {} and {} (polling every 30s)",
                            cf.display(),
                            cfg.display()
                        ));
                        warned_absent = true;
                    }
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    continue;
                }
                warned_absent = false;
                let my_gen = summrise_agent::tunnel_ctl::generation();
                // `cloudflared.exe` is a console-subsystem binary, so this
                // supervised spawn asks for the no-console flag before spending
                // the child (the rule lives in `spawn::hidden`). The command is
                // built in the open rather than as a chain because the ask has
                // to be its own statement INSIDE the spawn site's gap for the
                // whole-tree gate to see it — `main.rs` and `tunnel.rs` took
                // the same shape.
                let mut cf_cmd = tokio::process::Command::new(&cf);
                cf_cmd
                    .args(["tunnel", "--config"])
                    .arg(&cfg)
                    .arg("run")
                    .kill_on_drop(true);
                summrise_agent::spawn::hidden(&mut cf_cmd);
                match cf_cmd.spawn() {
                    Ok(mut child) => {
                        log_line("cloudflared tunnel: launched from install dir (supervised)");
                        let started = Instant::now();
                        let mut restarted = false;
                        loop {
                            if let Ok(Some(_)) = child.try_wait() {
                                break;
                            }
                            if summrise_agent::tunnel_ctl::generation() != my_gen {
                                let _ = child.kill().await;
                                let _ = child.wait().await;
                                restarted = true;
                                break;
                            }
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                        if started.elapsed() >= Duration::from_secs(60) {
                            backoff = 5; // survived a healthy minute — reset
                        }
                        if restarted {
                            log_line("cloudflared tunnel: restart requested (re-provisioned)");
                            continue; // immediate respawn on the new config
                        }
                        log_line(&format!(
                            "cloudflared tunnel exited after {}s — respawn in {backoff}s",
                            started.elapsed().as_secs()
                        ));
                    }
                    Err(e) => {
                        log_line(&format!(
                            "cloudflared tunnel: spawn failed: {e} — retry in {backoff}s"
                        ));
                    }
                }
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(60);
            }
        });
    });
}

/// Windows boot self-heal — runs before the listener binds, idempotent.
///
/// A legacy 0.8.x install (summrise-command.exe + the `SummriseCommand` service and
/// scheduled tasks) can coexist with this binary: the SCM starts the service
/// before the `SummriseAgent` boot task, the old process grabs port 18080, and
/// this server dies on bind — the device silently keeps serving the old
/// version after an upgrade. Repair that here:
///   1. kill every summrise binary that is not THIS install dir (incl. the
///      legacy summrise-command.exe, which is never this exe),
///   2. drop the legacy `SummriseCommand` service + tasks — the `SummriseAgent` boot
///      task is the canonical autostart (a service + task would race for the
///      port at every boot),
///   3. re-register the `SummriseAgent` boot task pointing at this exe + config
///      (fixes a manual file-copy update into a different dir; keeps the
///      unlimited ExecutionTimeLimit so the server never dies after 72h).
///
/// CRITICAL: every child process runs with a hard timeout (run_bounded).
/// An unbounded status() wait here dead-locked the agent on d1 — startup.log
/// showed only "starting" and nothing else, so the server never bound and
/// the device served 502 forever. Self-heal is best-effort: a stuck step
/// must NEVER block the bind.
#[cfg(windows)]
pub(crate) fn self_heal() {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(_) => return, // no exe path, nothing to repair
    };
    let exe_str = exe.to_string_lossy().into_owned();
    // P2-8 dual-source install dir: prefer the centralized
    // paths::install_dir() (registry InstallDir, the single source of truth);
    // fall back to exe.parent() when the registry is missing (dev builds /
    // unregistered installs — paths::install_dir() already falls back to the
    // exe dir itself, so this is belt-and-braces). No legacy-dir probing.
    let install_dir = {
        let d = summrise_agent::paths::install_dir();
        if d.as_os_str().is_empty() {
            exe.parent().map(|p| p.to_path_buf()).unwrap_or_default()
        } else {
            d
        }
    };
    // 0. Half-swap recovery (round-57): the NSIS upgrade swaps via
    //    exe → .bak then .new → exe — a power cut between the two renames
    //    leaves ONLY .bak + .new (no exe). Idempotent, same naming as the swap.
    //
    //    WHAT THIS CANNOT DO, stated because the comment here used to claim
    //    otherwise and the claim made the gap invisible. It said this "runs from
    //    the BOOT task wrapper (which exists independently), so it can repair
    //    before the exe itself is needed". THERE IS NO WRAPPER. `self_heal` is
    //    called from inside this process (`main.rs`), and the boot task's Execute
    //    IS this exe — so the `!exe.exists()` branch below cannot run in the one
    //    situation it was written for. If the swap leaves no exe, nothing starts
    //    the agent and nothing repairs it; the device is offline until someone
    //    touches it.
    //
    //    The branch is still correct and worth keeping: it is reachable when the
    //    exe exists but is a DOUBLED-UP state with a `.bak`/`.new` sibling, and
    //    the renames are checked and logged, which is what makes an actual
    //    half-swap diagnosable from `startup.log` afterwards.
    //
    //    CLOSING THE GAP NEEDS A LAUNCHER THAT IS NOT THE EXE (a scheduled task
    //    running a script, as the update path already uses for the swap). NOT
    //    DONE HERE, deliberately: it is Windows-only boot behaviour that cannot be
    //    exercised on this box, and shipping an untested recovery path is worse
    //    than a documented absence.
    let bak = install_dir.join("summrise-agent.exe.bak");
    let new = install_dir.join("summrise-agent.exe.new");
    // Half-swap failures must be LOUD and recoverable: every rename result
    // is checked, failures land in startup.log as CRITICAL (not hidden
    // behind a blanket 'self-heal: complete'), stale backups are never
    // deleted on a failed swap, and a copy fallback is attempted when a
    // rename fails (transient AV/lock races).
    let mut heal_failed = false;
    if !exe.exists() && bak.exists() {
        match std::fs::rename(&bak, &exe) {
            Ok(()) => log_line("self-heal: half-swap recovery: restored exe from .bak"),
            Err(e) => {
                log_line(&format!(
                    "self-heal: CRITICAL half-swap recovery FAILED: cannot restore {} -> {}: {e} — attempting copy fallback (.bak preserved for manual recovery)",
                    bak.display(),
                    exe.display()
                ));
                match std::fs::copy(&bak, &exe) {
                    Ok(_) => {
                        log_line("self-heal: copy fallback restored the exe from .bak");
                        if let Err(rm_e) = std::fs::remove_file(&bak) {
                            log_line(&format!(
                                "self-heal: warning: cannot remove stale .bak: {rm_e}"
                            ));
                        }
                    }
                    Err(ce) => {
                        log_line(&format!(
                            "self-heal: CRITICAL copy fallback FAILED too: {ce} — device may be unbootable; .bak preserved, NOT claiming success"
                        ));
                        heal_failed = true;
                    }
                }
            }
        }
    }
    if exe.exists() && new.exists() {
        match std::fs::rename(&new, &exe) {
            Ok(()) => {
                log_line("self-heal: half-swap recovery: applied pending .new over exe");
                if let Err(rm_e) = std::fs::remove_file(&bak) {
                    // Stale-copy cleanup only — not fatal if it fails.
                    log_line(&format!(
                        "self-heal: warning: cannot remove stale .bak: {rm_e}"
                    ));
                }
            }
            Err(e) => {
                // Fallback posture = revert: the current exe is untouched
                // (still bootable on the previous build), .bak is KEPT as
                // the rollback, and .new is KEPT for next-boot retry.
                log_line(&format!(
                    "self-heal: CRITICAL half-swap recovery FAILED: cannot apply {} -> {}: {e} — keeping current exe (revert posture), .bak + .new preserved for retry",
                    new.display(),
                    exe.display()
                ));
                heal_failed = true;
            }
        }
    }
    // npm-channel staged leftovers: a FAILED agent_update stages
    // `summrise-agent.new.exe` / boxed `.new` files and
    // returns false WITHOUT launching the swap script — the Rust failure
    // paths and the swap script's own !$ok branch both delete them
    // best-effort, but a power cut between staging and cleanup can still
    // strand them. They must NEVER be applied here: this recovery only
    // understands the NSIS-era `summrise-agent.exe.new` half-swap above (a
    // different filename); applying an npm-era staging of unknown provenance
    // could mix a failed release's components under the old version marker
    // (a stranded boxed `.new` would otherwise be picked up by the
    // NEXT successful swap — version skew). Delete best-effort; the next
    // agent_update re-downloads + re-stages from scratch (safe + retryable).
    // THE LIST IS NOT REPEATED HERE. It used to be, and two of its three entries
    // were pre-v2 spellings while staging writes `components\...` — so this sweep
    // deleted the executable and MISSED both boxed components, which is exactly
    // the version skew the paragraph above promises to prevent. One owner now
    // (`plugins::update::tools::staged_leftovers`), the same one the in-process
    // cleanup uses, so the two cannot disagree about filenames again.
    for stale in summrise_agent::plugins::update::staged_leftovers(&install_dir) {
        if stale.exists() {
            match std::fs::remove_file(&stale) {
                Ok(()) => log_line(&format!(
                    "self-heal: removed stale staged file {}",
                    stale.display()
                )),
                Err(e) => log_line(&format!(
                    "self-heal: warning: cannot remove stale staged file {}: {e}",
                    stale.display()
                )),
            }
        }
    }
    // Layout v2 (ADR 0008): the config lives in etc\ — never the install
    // root. This used to be a hardcoded root join, so EVERY self-heal
    // re-registered the boot task against <install>\config.yaml; the next
    // boot then loaded — and, when absent, bootstrap CREATED — a phantom root
    // config with a fresh token, the device self-registered a SECOND identity,
    // and every subsequent register 409'd against its own console record.
    // Path resolution goes through paths.rs, always.
    let cfg_str = summrise_agent::paths::config_file()
        .to_string_lossy()
        .into_owned();

    // 1. Stale binaries from other installs (they lock the exe AND hold the
    //    port). Runs as SYSTEM at boot; Stop-Process -Force is fine from
    //    there. Never kill processes of THIS install dir — and never this
    //    process. Exclude by PID (not by exe path): a path comparison can
    //    miss an 8.3 short path / empty Path and kill ourselves.
    let self_pid = std::process::id();
    let ps = format!(
        "Get-Process summrise-agent,summrise-command -ErrorAction SilentlyContinue \
         | Where-Object {{ $_.Id -ne {self_pid} -and $_.Path -ne '{exe_str}' }} \
         | Stop-Process -Force"
    );
    run_bounded("self-heal: kill stale procs", {
        let mut c = std::process::Command::new("powershell");
        c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps]);
        summrise_agent::spawn::hidden_std(&mut c);
        c
    });

    // 2. Legacy service + tasks. The boot task below replaces them.
    run_bounded("self-heal: sc stop SummriseCommand", {
        let mut c = std::process::Command::new("sc.exe");
        c.args(["stop", "SummriseCommand"]);
        summrise_agent::spawn::hidden_std(&mut c);
        c
    });
    run_bounded("self-heal: sc delete SummriseCommand", {
        let mut c = std::process::Command::new("sc.exe");
        c.args(["delete", "SummriseCommand"]);
        summrise_agent::spawn::hidden_std(&mut c);
        c
    });
    for name in ["SummriseCommand", "SummriseCommandTray"] {
        run_bounded(&format!("self-heal: schtasks /End {name}"), {
            let mut c = std::process::Command::new("schtasks");
            c.args(["/End", "/TN", name]);
            summrise_agent::spawn::hidden_std(&mut c);
            c
        });
        run_bounded(&format!("self-heal: schtasks /Delete {name}"), {
            let mut c = std::process::Command::new("schtasks");
            c.args(["/Delete", "/TN", name, "/F"]);
            summrise_agent::spawn::hidden_std(&mut c);
            c
        });
    }

    // 3. Boot task at THIS install dir. ExecutionTimeLimit 0 = never kill the
    //    task (the Task Scheduler default of 72h silently stops the server).
    //
    //    THE TASK REVIVES THE AGENT BY ITSELF, which it could not before. `-AtStartup`
    //    alone means a dead agent stays dead until somebody reboots the box or the
    //    ELECTRON SHELL's 5-minute pulse runs — and that pulse lives in a USER SESSION, so
    //    it does not exist on a headless device or when nobody is logged in. Observed:
    //    d1 was unreachable for days while its task sat "Ready".
    //
    //      * a REPETITION trigger every 5 minutes, paired with `MultipleInstances
    //        IgnoreNew`, is "START IF NOT RUNNING" — the task engine ignores the new
    //        instance while the agent is alive, so a healthy agent is never interrupted.
    //        (It also makes the shell's `schtasks /run SummriseAgent` pulse harmless: it can no
    //        longer stack a second agent.)
    //      * `-RestartCount/-RestartInterval` covers the other shape: the task ENDING,
    //        which the repetition alone would leave down for up to five minutes.
    //
    //    This is OS-level and needs no session, which is the point: the agent's ability to
    //    come back must not depend on another process that can also die.
    let script = format!(
        "Register-ScheduledTask -TaskName 'SummriseAgent' \
         -Action (New-ScheduledTaskAction -Execute '{exe_str}' -Argument '\"{cfg_str}\"') \
         -Trigger (New-ScheduledTaskTrigger -AtStartup), (New-ScheduledTaskTrigger -Once -At (Get-Date) -RepetitionInterval (New-TimeSpan -Minutes 5)) \
         -Principal (New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest) \
         -Settings (New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds 0) -MultipleInstances IgnoreNew -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)) -Force"
    );
    run_bounded("self-heal: Register-ScheduledTask SummriseAgent", {
        let mut c = std::process::Command::new("powershell");
        c.args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ]);
        summrise_agent::spawn::hidden_std(&mut c);
        c
    });

    if heal_failed {
        log_line("self-heal: complete WITH ERRORS — see CRITICAL lines above");
    } else {
        log_line("self-heal: complete");
    }
}

/// Put this process into a kill-on-close Job Object: every child we spawn
/// (PTY shells, SSH/serial sessions, playwright-mcp, short-lived helpers)
/// inherits membership, and when the agent exits for ANY reason the kernel
/// closes our job handle and terminates them all. Nested jobs (Win8+) make
/// this safe under Task Scheduler's own job wrapper.
#[cfg(windows)]
pub(crate) fn setup_child_reaper_job() {
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    unsafe {
        // WAS THIS PROCESS ALREADY IN A JOB? Recorded as a FACT, deliberately
        // without a verdict, because two very different situations produce it:
        // Task Scheduler wraps its tasks in a job (benign — see this function's
        // own note), and so does any process tree spawned from an agent-hosted
        // PTY shell (NOT benign: every child the agent spawns inherits its
        // kill-on-close job, so a second summrise-agent started that way nests inside
        // the first one's job and can take the RUNNING agent down with it).
        //
        // Observed on a device: launching a test build from an agent PTY killed
        // the live agent, which the watchdog then restarted. Diagnosing that cost
        // far longer than it should have, because this path was silent.
        //
        // One factual line, not a warning: startup.log already carries several
        // per boot, and a line that fires on every normal start would be
        // wallpaper. The actionable guidance lives in agent/AGENTS.md.
        let mut already: windows_sys::Win32::Foundation::BOOL = 0;
        if IsProcessInJob(GetCurrentProcess(), 0, &mut already) != 0 && already != 0 {
            log_line("child-reaper job: process was ALREADY in a job at startup (parent job limits apply; Task Scheduler does this, and so does an agent-hosted PTY shell)");
        }
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job == 0 {
            log_line("child-reaper job: CreateJobObject failed — update orphans possible");
            return;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
        {
            log_line("child-reaper job: SetInformation failed — orphans possible on update");
            return;
        }
        if AssignProcessToJobObject(job, GetCurrentProcess()) == 0 {
            log_line("child-reaper job: AssignProcess failed — nested jobs unsupported?");
            return;
        }
        // The handle is intentionally never closed: it lives until process
        // exit, whose implicit CloseHandle triggers the kill-on-close.
        log_line("child-reaper job: active — children die with the agent");
    }
}

/// Run a Windows helper process with a hard 30s timeout. Self-heal must never
/// block the bind: a stuck PowerShell/schtasks would otherwise dead-lock the
/// agent at every boot (this actually happened on d1). Logs start/done/timeout
/// to startup.log so a stuck step is visible next boot.
#[cfg(windows)]
fn run_bounded(what: &str, mut cmd: std::process::Command) {
    use std::time::Duration;
    use wait_timeout::ChildExt as _;

    log_line(&format!("{what} …"));
    match cmd.spawn() {
        Ok(mut child) => match child.wait_timeout(Duration::from_secs(30)) {
            Ok(Some(_)) => log_line(&format!("{what} ok")),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                log_line(&format!("{what} TIMED OUT — killed, continuing"));
            }
            Err(e) => log_line(&format!("{what} wait error: {e}")),
        },
        Err(e) => log_line(&format!("{what} spawn failed: {e}")),
    }
}

// Generates `ffi_service_main`, an `extern "system" fn(u32, *mut *mut u16)`
// that the SCM calls and which forwards the service args to `run_service`.
#[cfg(windows)]
windows_service::define_windows_service!(ffi_service_main, run_service);

/// SCM probe, owned by winmain so main.rs never touches windows_service
/// directly (A7 boundary): start the service dispatcher; `true` means the
/// SCM launched us and took over (caller must return), `false` means a
/// normal console launch — fall through. The macro-generated
/// `ffi_service_main` is module-private, so the call site cannot live in
/// main.rs.
#[cfg(windows)]
pub(crate) fn started_by_scm() -> bool {
    windows_service::service_dispatcher::start(SERVICE_NAME, ffi_service_main).is_ok()
}

/// Windows service entry point: register SCM control handling, report RUNNING,
/// run the server on a dedicated tokio runtime, then stop cleanly when told to.
/// Returns `()` because the SCM bootstrap macro discards the return value; any
/// error is logged and the service simply fails to start.
#[cfg(windows)]
fn run_service(_args: Vec<std::ffi::OsString>) {
    use std::sync::mpsc;
    use std::time::Duration;
    use windows_service::service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};

    // Channel so the SCM control handler can signal this thread to stop.
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = stop_tx.send(());
                ServiceControlHandlerResult::NoError
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = match service_control_handler::register(SERVICE_NAME, event_handler) {
        Ok(h) => h,
        Err(e) => {
            eout!("ERROR: failed to register service control handler: {e}");
            return;
        }
    };

    let running = ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        exit_code: ServiceExitCode::NO_ERROR,
        checkpoint: 0,
        wait_hint: Duration::from_secs(0),
        process_id: None,
    };
    if let Err(e) = status_handle.set_service_status(running) {
        eout!("ERROR: failed to report RUNNING: {e}");
        return;
    }

    // round-120: the SCM does NOT pass the binPath "<config>" argument to
    // ServiceMain — lpServiceArgVectors are the StartService args only (empty
    // for auto-start at boot). The old args.first() was therefore always
    // empty, so the path fell back to a RELATIVE "config.yaml" which resolved
    // against the service CWD (C:\Windows\System32) — bootstrap CREATED a
    // phantom default config there with a fresh unknown token, and every
    // client 401'd while the real install-dir config was never loaded. Read
    // the process command line (env::args carries the binPath param) and fall
    // back to the exe's own directory (never a relative path).
    let config_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| {
            // Layout v2: fall back to the canonical etc\config.yaml — NOT the
            // install root (a root join here is what produced the ADR 0008
            // phantom config the self-heal path above used to create).
            let cfg = summrise_agent::paths::config_file();
            (!cfg.as_os_str().is_empty()).then_some(cfg)
        })
        .unwrap_or_else(|| PathBuf::from("config.yaml"));

    // Run the async server on its own tokio runtime — this thread must stay free
    // to answer SCM control requests.
    // round-120: a panic on this thread (Runtime::new().expect, or any panic
    // in run_server) previously unwound only the thread — the service stayed
    // 'Running' with a dead server and the SCM recovery actions never fired.
    // Report the failure so the SCM sees a stopped service and restarts it.
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                eout!("ERROR: failed to create service tokio runtime: {e}");
                let stopped = ServiceStatus {
                    service_type: ServiceType::OWN_PROCESS,
                    current_state: ServiceState::Stopped,
                    controls_accepted: ServiceControlAccept::empty(),
                    exit_code: ServiceExitCode::ServiceSpecific(1),
                    checkpoint: 0,
                    wait_hint: Duration::from_secs(0),
                    process_id: None,
                };
                let _ = status_handle.set_service_status(stopped);
                return;
            }
        };
        rt.block_on(run_server(config_path));
    });

    // Block until the SCM asks us to stop.
    let _ = stop_rx.recv();

    let stopped = ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::NO_ERROR,
        checkpoint: 0,
        wait_hint: Duration::from_secs(0),
        process_id: None,
    };
    let _ = status_handle.set_service_status(stopped);
}
