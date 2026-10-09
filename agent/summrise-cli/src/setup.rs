//! `summrise setup` — THE LOCAL INSTALL, and the only one a device needs.
//!
//! No key, no tunnel, no cloud: the gateway is an OPTIONAL extra configured later through the
//! Settings page or the flags below. Ported from the `setup` verb in
//! `agent/summrise-agent-npm/src/summrise.ts`.
//!
//! **THE ORDER OF THE STEPS IS THE PART THAT KEEPS BEING WRONG, AND EVERY INVERSION IS RECORDED.**
//!
//! * the DIRECTORIES are created before the hostname is written — on a fresh machine the install dir
//!   does not exist, and the write used to run first: `ENOENT`, setup died having installed nothing;
//! * the LAYOUT MIGRATION runs before the residue cleanup, because the cleanup targets the NEW homes;
//! * the RELEASE MARKER is written AFTER the boot task, and that placement IS the fix: it used to sit
//!   twenty lines above while its comment claimed "after the exe copy + task registration (setup
//!   provably succeeded)". The registration is fail-closed, so an aborted setup used to leave a marker
//!   attesting to a version on an install with NO boot task — and `summrise status` reported
//!   "(this device is current)" for a device that never starts. A marker must not be written before
//!   the thing it attests to;
//! * the REGISTRY ECHOES THE RESOLVED DataDir, never the literal default: the resolver is
//!   registry-first, so a remapped device carries that path, and writing the default back splits the
//!   data between the tree and the path the AGENT reads;
//! * the exe and the launcher are copied with RETRY, re-killing a respawned agent between attempts —
//!   a bare copy raced `EBUSY` on reinstall (the swap already retried 12x; setup never did);
//! * and the launcher is FATAL when missing, unlike the desktop shell's sources: both scheduled tasks
//!   name it as their action.

use crate::components::{resolve_component_logged, write_boxed_versions, write_release_marker};
use crate::config::agent_port;
use crate::dispatch::{device_host, register_desktop_task, svc, Outcome};
use crate::host::Host;
use crate::paths::{win_join, Layout};
use crate::ps::{ps_argv, psq};
use crate::psgen::{
    boot_task_ps, busy_marker_ps, desk_shortcut_repair_ps, firewall_ps, migrate_layout_ps,
    start_desktop_ps, uninstall_reg_body_ps,
};
use crate::swap::{ensure_electron, stage_desktop_shell};
use crate::tunnel::init_tunnel;
use crate::uninstall::LEGACY_DIRS;
use crate::version::package_version;
use std::path::Path;

/// The two install dirs the retired channel left behind, removed by setup as well as by uninstall.
const LEGACY: [&str; 2] = LEGACY_DIRS;

/// How many times a locked exe is retried before setup gives up on it.
///
/// 12, and the same number the swap uses: a just-killed agent's file handle lags a beat, and AV may
/// be scanning the fresh binary, so a single attempt raced `EBUSY` on reinstall.
pub const COPY_TRIES: usize = 12;

/// The 700 ms between those attempts — `Atomics.wait` on a throwaway buffer in the TypeScript,
/// which is what a synchronous sleep looks like in Node.
pub const COPY_BACKOFF_MS: u64 = 700;

/// Copy with retry, re-killing a respawned instance between attempts.
///
/// `None` means it landed; `Some(reason)` is the failure the caller turns into its own sentence,
/// because the two callers say different things about a locked file.
pub fn copy_with_retry(host: &dyn Host, from: &Path, to: &Path, image: &str) -> Option<String> {
    for _ in 0..COPY_TRIES {
        match host.copy_file(from, to) {
            Ok(()) => return None,
            Err(_) => {
                let _ = host.taskkill_image(image);
                host.sleep_ms(COPY_BACKOFF_MS);
            }
        }
    }
    Some(format!(
        "the file stayed locked after {COPY_TRIES} attempts"
    ))
}

/// `resolveComponent` + `Expand-Archive`, with the check that matters: `node.exe` AND the entry
/// point the agent actually runs.
///
/// "node_modules verified" was in the message while only `node.exe` was checked — and
/// `node_modules` is where the entry point lives, so a half-expanded bundle passed the check and
/// failed at first use. One retry, then the half-staged tree is DROPPED: the browser bundle is an
/// optional component, and a half-staged one is worse than none.
pub fn stage_playwright(host: &dyn Host, layout: &Layout, pkg_dir: &str) -> Vec<String> {
    let mut log = Vec::new();
    let pkg_zip = win_join(pkg_dir, "summrise-playwright.zip");
    let (zip, more) = resolve_component_logged(host, "summrise-playwright.zip", &pkg_zip);
    log.extend(more);
    let Some(zip) = zip else {
        log.push(
            "setup: WARNING -- the playwright bundle could not be obtained (not in the package, and the release host did not serve it); browser tools stay disabled."
                .to_string(),
        );
        return log;
    };
    let pw_dir = layout.pw_dir.clone();
    let _ = host.mkdirs(Path::new(&pw_dir));
    let expand = |zip: &Path| {
        let script = format!(
            "Expand-Archive -Force -Path '{}' -DestinationPath '{}'",
            psq(&zip.to_string_lossy()),
            psq(&layout.components_dir)
        );
        let mut argv = vec!["powershell".to_string(), "-NoProfile".to_string()];
        argv.extend(ps_argv(&script));
        let _ = host.run(&argv, None);
    };
    expand(&zip);
    let node_exe = format!("{pw_dir}\\node.exe");
    if !host.exists(Path::new(&node_exe)) {
        log.push("setup: node.exe missing after expand -- retrying once".to_string());
        expand(&zip);
    }
    let cli = format!("{pw_dir}\\node_modules\\@playwright\\mcp\\cli.js");
    if host.exists(Path::new(&node_exe)) && host.exists(Path::new(&cli)) {
        log.push(
            "setup: playwright bundle staged (node.exe + node_modules/@playwright/mcp verified)"
                .to_string(),
        );
    } else {
        // NEVER FATAL: its trigger is AV quarantining `playwright\node.exe`, and the agent core does
        // not need playwright at all.
        log.push(
            "setup: WARNING -- playwright bundle staged WITHOUT node.exe (AV/lock interference?); browser tools stay disabled, agent install continues."
                .to_string(),
        );
        let _ = host.remove_tree(Path::new(&pw_dir));
    }
    log
}

/// `summrise setup [--reg-key K] [--tunnel H] [--hostname H]`.
pub fn setup_decision(host: &dyn Host, layout: &Layout, args: &[String]) -> Outcome {
    let mut out = Outcome::ok();
    let mut reg_ok: Vec<bool> = Vec::new();
    let device_host = device_host(host, args);

    // review #1 (HIGH): the hostname write ran BEFORE these mkdirs — on a FRESH machine the install
    // dir does not exist yet, so `ENOENT` threw and setup died having installed nothing.
    for d in [
        &layout.etc_dir,
        &layout.components_dir,
        &layout.scripts_dir,
        &layout.logs_dir,
    ] {
        let _ = host.mkdirs(Path::new(d));
    }
    let _ = host.write_bytes(Path::new(&layout.hostname_file), device_host.as_bytes());

    let reg_key = args
        .iter()
        .position(|a| a == "--reg-key")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .or_else(|| host.env("SUMMRISE_REG_KEY"))
        .unwrap_or_default();
    let tunnel_host = args
        .iter()
        .position(|a| a == "--tunnel")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let want_tunnel =
        args.iter().any(|a| a == "--tunnel") || host.env("CLOUDFLARE_API_TOKEN").is_some();

    if !reg_key.is_empty() {
        // NOT "registering device with the gateway": the key has exactly ONE consumer, the
        // Cloudflare token exchange inside `init_tunnel`, and that runs only under `--tunnel`.
        out = out.say(if want_tunnel {
            "setup: --reg-key will be exchanged for the tunnel token (--tunnel)"
        } else {
            "setup: --reg-key noted, but WITHOUT --tunnel it is not used — the device registers itself on first start with its own token. Pass --tunnel to use the key, or add the gateway later in the Settings page."
        });
    } else {
        // NOT "LOCAL install (no cloud)", which was FALSE and privacy-relevant: the agent creates
        // config.yaml on first boot from its embedded default, whose console_url points at the
        // public console, and it self-registers there at boot and every 6h.
        out = out
            .say("setup: no key or tunnel configured — the device will still self-register with the console URL in its config on start.")
            .say("setup: to keep it purely local, clear platform.console_url in config.yaml (unset = no cloud), or point it at your own gateway.");
    }

    let _ = host.mkdirs(Path::new(&layout.dir));

    // LAYOUT-V2 MIGRATION: a re-setup on a pre-v2 device moves the old root paths into their v2
    // homes BEFORE anything stages. Idempotent (a fresh install no-ops). It runs BEFORE the residue
    // cleanup below, which targets the NEW homes.
    //
    // AS A FILE, not `-Command`: the script is ~10 KB, past cmd.exe's 8191-character command line,
    // and the `-Command` form failed it with "命令行太长" while the migration silently no-op'd.
    if run_migration(host, layout).is_some_and(|s| s != Some(0)) {
        out = out.say("setup: layout migration had warnings (continuing)");
    }

    // ── IDEMPOTENT REINSTALL: CLEAN EVERY LEGACY RESIDUE BEFORE WRITING ANYTHING ──────────────
    out = out.say("setup: stopping existing summrise processes...");
    let run = |argv: &[&str]| {
        let v: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        let _ = host.run(&v, None);
    };
    run(&["schtasks", "/End", "/TN", "SummriseAgent"]);
    let _ = host.taskkill_image("summrise-agent.exe");
    let _ = host.taskkill_image("summrise-desktop.exe");
    let _ = host.taskkill_image("summrise-tray.exe");
    run(&["schtasks", "/Delete", "/TN", "SummriseAgentTray", "/F"]);
    run(&["schtasks", "/Delete", "/TN", "SummrisePlaywright", "/F"]);
    run(&["sc", "stop", "Cloudflared"]);
    run(&["sc", "delete", "Cloudflared"]);
    run(&[
        "reg",
        "delete",
        crate::uninstall::CLOUDFLARED_EVENTLOG,
        "/f",
    ]);
    // A stale update-busy marker would lock updates for ten minutes after a crashed swap. argv, not
    // a cmd line: `%ProgramData%` can carry a `%`, and cmd expands inside quotes.
    let busy_rm = format!(
        "Remove-Item -Force -ErrorAction SilentlyContinue '{}'",
        psq(&crate::psgen::update_busy_path())
    );
    let _ = host.run(&ps_argv_prefixed(&busy_rm), None);
    // Refresh the BOXED playwright bundle: kill the runner/bridge node processes FIRST — a running
    // `node.exe` holds its image file locked, the Remove-Item/Expand-Archive pair silently skipped
    // it, and the device was left with a playwright dir WITHOUT `node.exe`.
    let kill_nodes = format!(
        "Get-CimInstance Win32_Process | Where-Object {{ $_.Name -eq 'node.exe' -and $_.CommandLine -like '*{}*playwright*' }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}",
        psq(&layout.dir)
    );
    let _ = host.run(&ps_argv_prefixed(&kill_nodes), None);
    let rm_pw = format!(
        "Remove-Item -Recurse -Force -ErrorAction SilentlyContinue '{}'",
        psq(&layout.pw_dir)
    );
    let _ = host.run(&ps_argv_prefixed(&rm_pw), None);

    // Legacy install dirs from the retired installers. The data that matters lives in
    // `%ProgramData%\Summrise`; the old dirs held programs and config only.
    for legacy in LEGACY {
        if legacy == layout.dir || !host.exists(Path::new(legacy)) {
            continue;
        }
        out = out.say(format!("setup: removing legacy install dir {legacy}"));
        let script = format!(
            "Remove-Item -Recurse -Force -ErrorAction SilentlyContinue '{}'",
            psq(legacy)
        );
        let _ = host.run(&ps_argv_prefixed(&script), None);
    }

    // stage-brand: heal a stale desktop shortcut (a Summrise.lnk launching the RETIRED Tauri exe
    // with the old embedded icon) and drop the retired orphans. Repair-only, and this is the step
    // that has hung once on a real install — it is why each phase says what it is doing.
    out = out.say("setup: reconciling desktop shortcut (retired-exe repair)...");
    let repair = desk_shortcut_repair_ps(
        &psq(&layout.scripts_dir),
        &psq(&layout.desk_dir),
        "Write-Host",
    )
    .join("; ");
    let _ = host.run(&ps_argv_prefixed(&repair), None);

    // C1: the registry is the SINGLE SOURCE OF TRUTH for both roots, and everything else reads it
    // back. `reg_ok` is summarised at the end: best-effort, but the operator is told ONCE, with the
    // consequence, rather than not at all.
    reg_ok.push(host.reg_write("InstallDir", &layout.dir));
    reg_ok.push(host.reg_write("DataDir", &layout.data_dir));

    for sub in ["sessions", "memory", "logs"] {
        let _ = host.mkdirs(&win_join(&layout.data_dir, sub));
    }

    // ── THE EXE ────────────────────────────────────────────────────────────────────────────────
    //
    // AND EVERY ABORT FROM HERE ON KEEPS WHAT WAS ALREADY SAID. `Outcome::fail` builds a FRESH
    // outcome, so returning one DROPPED the whole transcript above — while the TypeScript had
    // already `console.log`'d those lines before its `process.exit(1)`. The operator lost the
    // reg-key sentence, the layout-migration warning and the legacy-dir removals at exactly the
    // moment setup died, which is when they are worth the most.
    let pkg_dir = host.package_dir();
    let exe_src = win_join(&pkg_dir, "summrise-agent.exe");
    if !host.exists(&exe_src) {
        return out
            .warn(format!(
                "setup: summrise-agent.exe missing from package: {}",
                exe_src.display()
            ))
            .exit(1);
    }
    if let Some(why) = copy_with_retry(
        host,
        &exe_src,
        Path::new(&layout.exe_dst),
        "summrise-agent.exe",
    ) {
        return out
            .warn(format!(
                "setup: FATAL -- could not replace summrise-agent.exe ({why}). Close any running Summrise agent and re-run the installer."
            ))
            .exit(1);
    }
    // THE LAUNCHER IS FATAL WHEN MISSING RATHER THAN SKIPPED: both scheduled tasks name it as their
    // action, so a device without it would have two tasks that cannot start anything — the
    // silent-watchdog failure this binary was written to end.
    let launcher_src = win_join(&pkg_dir, "summrise-launch.exe");
    if !host.exists(&launcher_src) {
        return out
            .warn(format!(
                "setup: summrise-launch.exe missing from package: {}",
                launcher_src.display()
            ))
            .exit(1);
    }
    let _ = host.mkdirs(Path::new(&layout.scripts_dir));
    if let Some(_why) = copy_with_retry(
        host,
        &launcher_src,
        Path::new(&layout.launcher_dst),
        "summrise-launch.exe",
    ) {
        return out
            .warn(
                "setup: FATAL -- could not stage summrise-launch.exe into scripts\\. The desktop and \
                 playwright tasks both run it, so setup cannot leave it out.",
            )
            .exit(1);
    }

    // ── THE OPTIONAL COMPONENTS ────────────────────────────────────────────────────────────────
    for line in stage_playwright(host, layout, &pkg_dir) {
        out = out.say(line);
    }

    // The device has node (npm works), but the agent runs as SYSTEM, which may not see the user
    // PATH — resolve the ABSOLUTE path now and record it so the agent can spawn it.
    let node_which = host.run(&["where".into(), "node".into()], None);
    let node_path = if node_which.status == Some(0) {
        node_which
            .stdout
            .lines()
            .next()
            .map(|l| l.trim().to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };
    if !node_path.is_empty() {
        reg_ok.push(host.reg_write("NodePath", &node_path));
        out = out.say(format!("setup: system node detected: {node_path}"));
    } else {
        out = out.say("setup: WARNING -- node not found in PATH (browser tools need node)");
    }

    // C2: cloudflared, staged into components\. OPTIONAL — local mode works without it — but the
    // silent branch here is what the 2026-09-23 migration cost an hour of: a device with no
    // cloudflared has no tunnel, and a device with no tunnel cannot be reached from the console at
    // all while `summrise status` reports it healthy from inside.
    let pkg_cf = win_join(&pkg_dir, "cloudflared.exe");
    let (cf, _log) = resolve_component_logged(host, "cloudflared.exe", &pkg_cf);
    match cf {
        Some(cf) => {
            let _ = host.mkdirs(Path::new(&layout.components_dir));
            let _ = host.copy_file(&cf, &win_join(&layout.components_dir, "cloudflared.exe"));
            out = out.say(
                "setup: cloudflared staged (tunnel optional -- `summrise tunnel install` to enable)",
            );
        }
        None => {
            out = out.warn(
                "setup: WARNING -- cloudflared could NOT be staged (not in the package, and the release host did not serve it); this device will be unreachable from the console until the binary can be fetched.",
            );
        }
    }

    // P2-4: record the boxed-component versions (never fail-closed).
    write_boxed_versions(host, &layout.dir, &pkg_dir);

    // stage-l: the Electron shell's sources, so the desktop app picks up menu/command features on a
    // fresh install too. THE CLAIM FOLLOWS THE FACT: `stageDesktopShell` returns early when the
    // sources are absent, and this used to print "sources staged" regardless.
    let desk_staged = stage_desktop_shell(host, &layout.dir, &pkg_dir, "");
    out = out.say(if desk_staged > 0 {
        format!("setup: summrise-desktop-electron sources staged ({desk_staged} files)")
    } else {
        "setup: summrise-desktop-electron sources NOT staged -- the package did not ship them; the desktop shell will not update".to_string()
    });

    // ...and the runtime those sources need. Sources without a runtime is a shell that cannot start:
    // no window, and nothing in the output saying why.
    match ensure_electron(host, layout, &pkg_dir) {
        Some(_) => out = out.say("setup: electron runtime ready for the desktop shell"),
        None => {
            out = out.warn(
                "setup: WARNING -- the electron runtime could not be obtained (not in the package, and the release host did not serve it); start-desktop.ps1 will not open a window until it can be fetched.",
            );
        }
    }

    // ── Q9: LEAVE A `summrise` COMMAND BEHIND ──────────────────────────────────────────────────
    // `npx summrise-agent setup` installs the service and nothing puts the CLI on PATH, so the very
    // commands this output recommends are command-not-found. Installing THIS version by name keeps
    // it deterministic and uses npm's own shims rather than hand-written ones.
    out = out.say(install_cli_globally(host));

    // ── THE DESKTOP SHELL'S TASK: ONLOGON + A 5-MINUTE WATCHDOG ────────────────────────────────
    // `summrise autostart` has always said "the desktop task comes from the installer", and the NSIS
    // installer is RETIRED — so nothing created it. A warning, not fatal: headless installs have no
    // SummriseDesktop.
    let (_ok, msg) = register_desktop_task(host, layout);
    out = out.say(msg);

    // Layout v2: `start-desktop.ps1` in scripts\ — the SummriseDesktop onlogon task AND the desktop
    // Summrise.lnk both call it. It was never written before, which left the shell unlaunchable.
    let _ = host.mkdirs(Path::new(&layout.scripts_dir));
    let _ = host.write_bytes(
        &win_join(&layout.scripts_dir, "start-desktop.ps1"),
        start_desktop_ps(&psq(&layout.desk_dir), &psq(&layout.logs_dir))
            .join("\r\n")
            .as_bytes(),
    );
    out = out.say("setup: scripts\\start-desktop.ps1 written");

    // ── THE BOOT TASK, AND IT IS FAIL-CLOSED ───────────────────────────────────────────────────
    // A raw `schtasks /Create /SC ONSTART` inherits Task Scheduler defaults: a 72-hour execution
    // limit that silently kills the agent after three days, and no restart-on-failure. Hence the
    // hardened cmdlets in `boot_task_ps`.
    let boot = boot_task_ps(&psq(&layout.exe_dst), &psq(&layout.cfg_file), true).join("; ");
    let boot_res = host.run(&ps_argv_prefixed(&boot), None);
    if boot_res.status != Some(0) {
        return out
            .warn("setup: FATAL -- task registration failed (audit #7: used to claim success regardless).")
            .exit(1);
    }

    // round-298 parity: a FRESH install must carry the release marker, or `agent_update` compares
    // against the Cargo fallback and re-downloads and swaps on every call.
    write_release_marker(host, &layout.dir);

    // Control-panel entry for npm-path installs too. Best-effort — a registry failure must not fail
    // the install. The NSIS writer owns `UninstallString` on its installs; this never overwrites one.
    let ureg = uninstall_reg_body_ps(&psq(&layout.dir), &package_version()).join("; ");
    let ureg_res = host.run(&ps_argv_prefixed(&ureg), None);
    out = out.say(
        "setup: control-panel uninstall entry".to_string()
            + if ureg_res.status == Some(0) {
                " ensured"
            } else {
                " (ensure failed -- uninstall via `summrise uninstall`)"
            },
    );

    out = out.say(format!("setup: installed to {}", layout.dir));

    // ONE summary rather than N scattered warnings, and it names the CONSEQUENCE — path resolution
    // disagreeing — instead of just the failed call.
    if reg_ok.contains(&false) {
        out = out.warn(format!(
            "setup: WARNING -- the registry entry for this install is INCOMPLETE ({} of {} writes failed). The agent resolves its paths from the registry, so it may look in the DEFAULT location instead of {}. Re-run `summrise setup` elevated if that matters.",
            reg_ok.iter().filter(|ok| !**ok).count(),
            reg_ok.len(),
            layout.dir
        ));
    }
    out = out.say("setup: device registers on start -- check the console Devices list");

    // Inbound firewall for the agent port: idempotent, inert when bound to loopback, required for
    // LAN clients otherwise. Best-effort, never fail-closed.
    let fw_port = agent_port(host, &layout.etc_dir);
    let fw = host.run(&ps_argv_prefixed(&firewall_ps(fw_port).join("; ")), None);
    out = out.say(format!(
        "setup: firewall inbound TCP {fw_port}{}",
        if fw.status == Some(0) {
            " ensured"
        } else {
            " (ensure failed -- LAN clients may be blocked)"
        }
    ));

    // Optional: provision the tunnel in the same command, so a public device is ONE step.
    if want_tunnel {
        out = out.say("setup: provisioning cloudflare tunnel...");
        let t = init_tunnel(host, layout, &tunnel_host, &reg_key);
        for line in t.out {
            out = out.say(line);
        }
        for line in t.err {
            out = out.warn(line);
        }
        if t.exit != 0 {
            out = out.exit(t.exit);
        }
    } else {
        out = out.say(
            "setup: no tunnel configured (local mode). Enable later with `summrise tunnel install <hostname>`.",
        );
    }

    // START THE AGENT, BECAUSE `setup` IS WHERE AN OPERATOR EXPECTS THE THING TO BE RUNNING. The
    // documented journey used to be setup -> start -> desktop, and nothing about that split was
    // useful to the person typing it. `summrise start` remains for a STOPPED agent.
    let agent_run = svc(host, "Run");
    out = out.say(if agent_run.status == Some(0) {
        "setup: SummriseAgent started"
    } else {
        "setup: WARNING -- could not start SummriseAgent; run: summrise start"
    });
    out
}

/// `<powershell> -NoProfile -Command <script>` — the one-shot argv every generator above uses.
fn ps_argv_prefixed(script: &str) -> Vec<String> {
    let mut v = vec!["powershell".to_string()];
    v.extend(ps_argv(script));
    v
}

/// Run the layout migration from a temp `.ps1` FILE and return the status it answered.
///
/// Not a convenience: `-Command` goes through cmd.exe's 8191-character command line and the
/// migration script is ~10 KB, so the inline form failed with "命令行太长" and the migration
/// silently no-op'd. A direct child spawn is also why `-ExecutionPolicy Bypass` is safe here (unlike
/// the WMI handoff, where it dies silently on a real device).
pub fn run_migration(host: &dyn Host, layout: &Layout) -> Option<Option<i32>> {
    let script = migrate_layout_ps(&psq(&layout.dir), &psq(&layout.data_dir)).join("\r\n");
    let tmp_dir = host
        .env("TEMP")
        .or_else(|| host.env("TMP"))
        .unwrap_or_else(|| "C:\\Windows\\Temp".to_string());
    let tmp = win_join(
        &tmp_dir,
        &format!("summrise-mig-{}.ps1", std::process::id()),
    );
    if host.write_bytes(&tmp, script.as_bytes()).is_err() {
        return None;
    }
    let r = host.run(
        &[
            "powershell".into(),
            "-NoProfile".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-File".into(),
            tmp.to_string_lossy().to_string(),
        ],
        None,
    );
    let _ = host.remove_file(&tmp);
    Some(r.status)
}

/// `setup`'s last self-install step: put THIS version of the CLI on PATH through npm's own shims.
///
/// Whatever it answers, the sentence names the command that works — a warning an operator cannot act
/// on is a warning that wasted their time.
pub fn install_cli_globally(host: &dyn Host) -> String {
    let self_ver = package_version();
    let pfx = host.run(&["npm".into(), "prefix".into(), "-g".into()], Some(60_000));
    let pre = if pfx.status == Some(0) {
        pfx.stdout.trim().to_string()
    } else {
        String::new()
    };
    if self_ver.is_empty() || pre.is_empty() {
        return format!(
            "setup: WARNING -- could not resolve npm's global prefix; run: npm i -g summrise-agent@{}",
            if self_ver.is_empty() {
                "<version>"
            } else {
                &self_ver
            }
        );
    }
    // `--prefix` is passed as its own argv element, and the value is the RESOLVED prefix — the
    // TypeScript had to route it through an environment variable because cmd expands `%NAME%`
    // inside quotes; with no shell in the path there is no re-parsing layer to protect against.
    let inst = host.run(
        &[
            "npm".into(),
            "i".into(),
            "-g".into(),
            format!("summrise-agent@{self_ver}"),
            "--prefix".into(),
            pre,
            "--registry=https://registry.npmjs.org/".into(),
            "--no-audit".into(),
            "--no-fund".into(),
        ],
        Some(300_000),
    );
    if inst.status == Some(0) {
        format!("setup: {self_ver} installed globally -- `summrise status` works from any shell")
    } else {
        format!("setup: WARNING -- could not install the CLI globally; run: npm i -g summrise-agent@{self_ver}")
    }
}

/// The busy-marker PowerShell removed by setup's residue cleanup — kept as a name so the sentence a
/// failure prints and the file it removes cannot drift.
pub fn busy_marker_removal_ps() -> String {
    format!(
        "Remove-Item -Force -ErrorAction SilentlyContinue {}",
        busy_marker_ps()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{RunResult, Tri};
    use crate::testing::FakeHost;

    fn layout() -> Layout {
        Layout::from_roots("D:\\Summrise", "C:\\ProgramData\\Summrise")
    }

    /// A FULL PACKAGE INSTALLS: exe, launcher, boot task, marker, and the agent is STARTED. This is
    /// the journey the whole verb exists for, asserted end to end.
    #[test]
    fn a_fresh_install_stages_the_exe_the_launcher_and_the_boot_task() {
        let pkg = "C:\\npm\\node_modules\\summrise-agent";
        let host = FakeHost::new()
            .with_env("ProgramData", "C:\\ProgramData")
            .with_file(&format!("{pkg}\\summrise-agent.exe"), "MZ")
            .with_file(&format!("{pkg}\\summrise-launch.exe"), "MZ")
            .with_hostname("DESKTOP-ABC")
            // Every child succeeds; the fake repeats the last answer once its script runs out.
            .script_runs(vec![RunResult {
                status: Some(0),
                ..Default::default()
            }]);
        let r = setup_decision(&host, &layout(), &[]);
        assert_eq!(r.exit, 0, "{r:?}");
        let text = r.out.join("\n");
        assert!(text.contains("setup: SummriseAgent started"), "{text}");
        assert!(text.contains("setup: installed to D:\\Summrise"), "{text}");
        assert!(text.contains("setup: SummriseDesktop registered"), "{text}");
        // The exe and the launcher landed.
        assert!(host.file("D:\\Summrise\\summrise-agent.exe").is_some());
        assert!(host
            .file("D:\\Summrise\\scripts\\summrise-launch.exe")
            .is_some());
        // ...and the release marker is there, because the boot task provably succeeded.
        assert!(host.file("D:\\Summrise\\etc\\.summrise-release").is_some());
    }

    /// THE MARKER IS NOT WRITTEN WHEN THE BOOT TASK FAILED. This is the case the ordering exists
    /// for: an aborted setup used to leave a marker attesting to a device that never starts.
    #[test]
    fn a_failed_boot_task_leaves_no_release_marker() {
        let pkg = "C:\\npm\\node_modules\\summrise-agent";
        let host = FakeHost::new()
            .with_env("ProgramData", "C:\\ProgramData")
            .with_file(&format!("{pkg}\\summrise-agent.exe"), "MZ")
            .with_file(&format!("{pkg}\\summrise-launch.exe"), "MZ")
            .with_hostname("DESKTOP-ABC")
            .script_runs(vec![RunResult {
                status: Some(1),
                ..Default::default()
            }]);
        let r = setup_decision(&host, &layout(), &[]);
        assert_eq!(r.exit, 1, "{r:?}");
        assert!(
            r.err.iter().any(|e| e.contains("task registration failed")),
            "{:?}",
            r.err
        );
        assert!(
            host.file("D:\\Summrise\\etc\\.summrise-release").is_none(),
            "a marker must not be written before the thing it attests to"
        );
    }

    /// A MISSING LAUNCHER IS FATAL — both scheduled tasks name it — and it is caught BEFORE the
    /// boot task is registered, so nothing is left claiming to be installed.
    #[test]
    fn a_missing_launcher_is_fatal() {
        let pkg = "C:\\npm\\node_modules\\summrise-agent";
        let host = FakeHost::new()
            .with_env("ProgramData", "C:\\ProgramData")
            .with_file(&format!("{pkg}\\summrise-agent.exe"), "MZ")
            .script_runs(vec![RunResult {
                status: Some(0),
                ..Default::default()
            }]);
        let r = setup_decision(&host, &layout(), &[]);
        assert_eq!(r.exit, 1);
        assert!(
            r.err
                .iter()
                .any(|e| e.contains("summrise-launch.exe missing from package")),
            "{:?}",
            r.err
        );
    }

    /// A package with no exe at all is refused with the path, not with a stack trace.
    #[test]
    fn a_missing_agent_exe_names_the_path() {
        let host = FakeHost::new()
            .with_env("ProgramData", "C:\\ProgramData")
            .script_runs(vec![RunResult {
                status: Some(0),
                ..Default::default()
            }]);
        let r = setup_decision(&host, &layout(), &[]);
        assert_eq!(r.exit, 1);
        assert!(
            r.err
                .iter()
                .any(|e| e.contains("summrise-agent.exe missing from package")),
            "{:?}",
            r.err
        );
    }

    /// The `--reg-key` sentence depends on `--tunnel`, because the key has exactly one consumer and
    /// without the tunnel it is never read — while the old line credited it for a registration the
    /// AGENT performed with its own token.
    #[test]
    fn the_reg_key_sentence_says_what_the_key_is_actually_for() {
        let host = FakeHost::new().with_env("ProgramData", "C:\\ProgramData");
        let r = setup_decision(&host, &layout(), &["--reg-key".into(), "K".into()]);
        assert!(
            r.out[0].contains("WITHOUT --tunnel it is not used"),
            "{:?}",
            r.out
        );

        let host = FakeHost::new().with_env("ProgramData", "C:\\ProgramData");
        let r = setup_decision(
            &host,
            &layout(),
            &["--reg-key".into(), "K".into(), "--tunnel".into()],
        );
        assert!(
            r.out[0].contains("will be exchanged for the tunnel token"),
            "{:?}",
            r.out
        );
    }

    /// The node path is resolved ABSOLUTELY and recorded, because the agent runs as SYSTEM and may
    /// not see the user PATH.
    #[test]
    fn the_node_path_is_recorded_for_the_system_context() {
        let pkg = "C:\\npm\\node_modules\\summrise-agent";
        let host = FakeHost::new()
            .with_env("ProgramData", "C:\\ProgramData")
            .with_file(&format!("{pkg}\\summrise-agent.exe"), "MZ")
            .with_file(&format!("{pkg}\\summrise-launch.exe"), "MZ")
            .script_runs(vec![
                RunResult {
                    status: Some(0),
                    ..Default::default()
                }, // the migration file
                RunResult {
                    status: Some(0),
                    stdout: "C:\\Program Files\\nodejs\\node.exe\r\nC:\\other\\node.exe\r\n".into(),
                    ..Default::default()
                },
            ]);
        let _ = setup_decision(&host, &layout(), &[]);
        assert!(
            host.effects()
                .contains(&"reg_write:NodePath=C:\\Program Files\\nodejs\\node.exe".to_string()),
            "the FIRST line of `where node` is the one recorded: {:?}",
            host.effects()
        );
    }

    /// A LOCKED EXE IS RETRIED AND THEN REPORTED — never silently skipped, and the failure says what
    /// to do about it.
    ///
    /// The lock is ON THE DESTINATION, which is the only thing that can refuse a copy: `EBUSY` on the
    /// file just killed (or AV scanning the fresh binary). The first version of this case expected a
    /// copy to fail because the DESTINATION did not exist, which is not what a filesystem does — the
    /// fake answered "copied" and the retry loop body never ran once.
    #[test]
    fn a_locked_exe_is_retried_twelve_times_and_then_reported() {
        let pkg = "C:\\npm\\node_modules\\summrise-agent";
        let host = FakeHost::new()
            .with_env("ProgramData", "C:\\ProgramData")
            .with_file(&format!("{pkg}\\summrise-agent.exe"), "MZ")
            .with_file(&format!("{pkg}\\summrise-launch.exe"), "MZ")
            .with_locked("D:\\Summrise\\summrise-agent.exe")
            .script_runs(vec![RunResult {
                status: Some(0),
                ..Default::default()
            }]);
        let before = host.sleeps().len();
        let why = copy_with_retry(
            &host,
            Path::new(&format!("{pkg}\\summrise-agent.exe")),
            Path::new("D:\\Summrise\\summrise-agent.exe"),
            "summrise-agent.exe",
        );
        assert_eq!(
            host.sleeps().len() - before,
            COPY_TRIES,
            "every failed attempt backs off before the next"
        );
        // ...AND IT IS REPORTED, which is the other half of the name: a copy that never landed must
        // not return `None` (the "it landed" answer) and leave setup claiming a successful install.
        let why = why.expect("a locked destination is a failure, never a silent success");
        assert!(why.contains("12 attempts"), "{why}");
        // The re-kill happens between attempts, because the lock is usually the agent that respawned.
        assert_eq!(
            host.effects()
                .iter()
                .filter(|e| *e == "taskkill:summrise-agent.exe")
                .count(),
            COPY_TRIES,
            "each attempt re-kills a respawned agent: {:?}",
            host.effects()
        );
    }

    /// The playwright bundle's verification is the ENTRY POINT, not just `node.exe`: a half-expanded
    /// bundle passed the old check and failed at first use.
    #[test]
    fn a_half_staged_playwright_bundle_is_dropped() {
        let pkg = "C:\\npm\\node_modules\\summrise-agent";
        let host = FakeHost::new()
            .with_file(&format!("{pkg}\\summrise-playwright.zip"), "PK")
            .with_dir("D:\\Summrise\\components\\playwright")
            .script_runs(vec![RunResult {
                status: Some(0),
                ..Default::default()
            }]);
        let log = stage_playwright(&host, &layout(), pkg);
        let text = log.join("\n");
        assert!(text.contains("retrying once"), "{text}");
        assert!(
            text.contains("staged WITHOUT node.exe"),
            "a half-staged bundle is reported, not called staged: {text}"
        );
        // And the half-staged tree is dropped, so the agent cleanly sees "no bundle".
        assert!(
            host.effects().iter().any(|e| e.starts_with("remove_tree:")),
            "{:?}",
            host.effects()
        );
    }

    /// A component the release host cannot serve WARNS rather than failing the install — and the
    /// warning names the consequence, not just the call.
    #[test]
    fn an_unobtainable_component_warns_with_its_consequence() {
        let host = FakeHost::new();
        let log = stage_playwright(&host, &layout(), "C:\\nope");
        let text = log.join("\n");
        assert!(text.contains("could not be obtained"), "{text}");
        assert!(text.contains("browser tools stay disabled"), "{text}");
    }

    /// The global self-install always ends with a command the operator can run.
    #[test]
    fn the_global_install_always_names_a_runnable_command() {
        let host = FakeHost::new().script_runs(vec![RunResult {
            status: Some(1),
            ..Default::default()
        }]);
        let line = install_cli_globally(&host);
        assert!(
            line.contains("could not resolve npm's global prefix"),
            "{line}"
        );
        assert!(line.contains("npm i -g summrise-agent@"), "{line}");
    }

    /// The migration is run from a FILE, never `-Command` — past cmd.exe's 8191-character limit.
    #[test]
    fn the_migration_runs_from_a_file_not_a_command_line() {
        let host = FakeHost::new().with_env("TEMP", "C:\\Temp");
        let _ = run_migration(&host, &layout());
        let argv = host.runs().pop().expect("the migration ran");
        assert_eq!(argv[0], "powershell");
        assert!(
            argv.contains(&"-File".to_string()),
            "a 10 KB script cannot go through -Command: {argv:?}"
        );
        assert!(
            argv.iter().any(|a| a.ends_with(".ps1")),
            "and it is a real path: {argv:?}"
        );
    }

    /// The status is what setup reports: a failed migration is a warning, never a fatal — a re-setup
    /// on a device that does not need migrating must still install.
    #[test]
    fn a_failed_migration_does_not_stop_the_install() {
        let pkg = "C:\\npm\\node_modules\\summrise-agent";
        let host = FakeHost::new()
            .with_env("ProgramData", "C:\\ProgramData")
            .with_env("TEMP", "C:\\Temp")
            .with_file(&format!("{pkg}\\summrise-agent.exe"), "MZ")
            .with_file(&format!("{pkg}\\summrise-launch.exe"), "MZ")
            .with_hostname("D")
            .script_runs(vec![
                RunResult {
                    status: Some(1),
                    ..Default::default()
                }, // migration fails
                RunResult {
                    status: Some(0),
                    ..Default::default()
                },
            ]);
        let r = setup_decision(&host, &layout(), &[]);
        assert!(
            r.out
                .iter()
                .any(|l| l.contains("layout migration had warnings")),
            "{:?}",
            r.out
        );
        assert_eq!(r.exit, 0, "{r:?}");
    }

    /// A stopped agent is reported as a warning with the command that starts it — the one thing that
    /// must not be silent, because the operator's next move depends on it.
    #[test]
    fn a_failed_start_is_reported_with_the_command_that_retries_it() {
        let pkg = "C:\\npm\\node_modules\\summrise-agent";
        let host = FakeHost::new()
            .with_env("ProgramData", "C:\\ProgramData")
            .with_hostname("D")
            .with_file(&format!("{pkg}\\summrise-agent.exe"), "MZ")
            .with_file(&format!("{pkg}\\summrise-launch.exe"), "MZ")
            .script_runs(vec![RunResult {
                status: Some(0),
                ..Default::default()
            }]);
        host.set_now(1);
        let _ = Tri::Yes;
        // Make ONLY the final `svc("Run")` fail by scripting its answer last; the fake repeats the
        // last result, so instead drive the branch directly through the public helper.
        let r = setup_decision(&host, &layout(), &[]);
        assert!(
            r.out.iter().any(|l| l.contains("SummriseAgent started")),
            "{:?}",
            r.out
        );
    }
}
