//! THE AGENT'S LIFECYCLE AND THE SHELL'S SELF-PROTECTION — the `schtasks` contract, the watchdog, the
//! retry backoff, the main-window tripwire and the CDP self-check.
//!
//! `main.ts` states the architecture this file serves: the agent's lifecycle is RUST and owned by the
//! `SummriseAgent` scheduled task; the shell "only probes/triggers". Triggering means `schtasks`, and a
//! `schtasks` command line is a FORMAT with a documented trap in it (the `/tr` value re-parsed as a
//! command line, needing its own inner quotes), so it belongs on this side of the boundary where a test
//! can pin it. The `spawn` is the host's.

/// The per-user scheduled task the Settings page's auto-launch toggle manages.
///
/// **THE NAME IS SHARED WITH THE CLI** (`agent/summrise-agent-npm/src/summrise.ts` registers the same
/// task) and nothing compares the two literals; the comment in `main.ts` is the only record. It is a
/// constant here so at least one side of that pair has a single owner.
pub const AUTOSTART_TASK: &str = "SummriseDesktop";
/// The task that starts the agent — "the ONLY sanctioned spawn path", because a `spawn()` of
/// `summrise-agent.exe` from JS was the d1 Chrome-OOM root cause (a bind-failed child wedged on
/// "Press Enter to exit" forever).
pub const AGENT_TASK: &str = "SummriseAgent";
/// The hard bound on a `schtasks` invocation. The SYNC `execSync` variant could hang the main process on
/// a wedged `schtasks` (observed: the electron process died when `setAutoLaunch` ran it inline).
pub const SCHTASKS_TIMEOUT_MS: f64 = 15_000.0;

/// `schtasks /query /tn <task>` — "does the task exist?".
pub fn schtasks_query_args(task: &str) -> Vec<String> {
    vec!["/query".into(), "/tn".into(), task.into()]
}

/// `schtasks /run /tn <task>` — the sanctioned start.
pub fn schtasks_run_args(task: &str) -> Vec<String> {
    vec!["/run".into(), "/tn".into(), task.into()]
}

/// `schtasks /end /tn <task>` — stop a RUNNING task before deleting it, because "a RUNNING task's
/// process tree is terminated on delete" and the current electron instance must survive.
pub fn schtasks_end_args(task: &str) -> Vec<String> {
    vec!["/end".into(), "/tn".into(), task.into()]
}

/// `schtasks /delete /tn <task> /f`.
pub fn schtasks_delete_args(task: &str) -> Vec<String> {
    vec!["/delete".into(), "/tn".into(), task.into(), "/f".into()]
}

/// `schtasks /create …` — the LOGON-ONLY task the auto-launch toggle writes.
///
/// THE `/tr` VALUE IS THE TRAP AND IT IS WHY THIS IS A FUNCTION. `schtasks` re-parses the `/tr` VALUE as
/// a command line, so it needs its OWN inner quotes around the script path: the spawn array keeps the
/// outer argument intact and the embedded `\"` sequences survive to `schtasks`. The original carries
/// them as `\\"` inside a TypeScript template literal, which is the same two characters.
///
/// AND THE SHAPE IS A KNOWN DOWNGRADE, recorded where the downgrade happens rather than rediscovered:
/// `schtasks /create` has no way to express a repetition interval, so what this writes is a LOGON-ONLY
/// task, while the CLI registers TWO triggers (AtLogOn plus a guarded 5-minute pulse). "Toggling
/// auto-launch OFF deletes the hardened task, and toggling it ON again replaces it with one that cannot
/// recover a wedged shell." `summrise update` heals it. The `/ru Administrator` is there because under
/// SYSTEM a spawn-array `schtasks /create` without `/ru` fails with "no mapping between account names
/// and security IDs": the interactive user is not resolvable from the service session.
pub fn schtasks_create_args(task: &str, script: &str) -> Vec<String> {
    vec![
        "/create".into(),
        "/tn".into(),
        task.into(),
        "/tr".into(),
        format!(r#"powershell -NoProfile -ExecutionPolicy Bypass -File \"{script}\""#),
        "/sc".into(),
        "onlogon".into(),
        "/ru".into(),
        "Administrator".into(),
        "/f".into(),
    ]
}

/// What the auto-launch toggle should DO, given what the caller asked for and what exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoLaunchPlan {
    /// `enabled` and the task is absent: write it.
    Create,
    /// `!enabled` and the task exists: end it, then delete it.
    Remove,
    /// Already in the requested state: touch nothing, and do NOT report a failure.
    Nothing,
}

impl AutoLaunchPlan {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Remove => "remove",
            Self::Nothing => "nothing",
        }
    }
}

/// `autoLaunchTaskSet`'s branch. The idempotence is the decision: asking for the state you are already
/// in is a SUCCESS, not a no-op to be apologised for.
pub fn auto_launch_plan(enabled: bool, exists: bool) -> AutoLaunchPlan {
    if enabled && !exists {
        AutoLaunchPlan::Create
    } else if !enabled && exists {
        AutoLaunchPlan::Remove
    } else {
        AutoLaunchPlan::Nothing
    }
}

/// THE SELF-HEAL WATCHDOG'S GATE: `misses >= 5 && Date.now() - lastAutoStartAt > 5 minutes`.
///
/// Both halves are decisions with a measurement behind them. Five consecutive failed probes is "~60 s of
/// silence — comfortably past an update swap's ~10 s stop/restart window, so no race with the sanctioned
/// updater"; and "at most once per 5 minutes" is the bound on how often the shell will run the only
/// sanctioned spawn path itself. `lastAutoStartAt` starts at 0, so the first crossing always fires.
pub const AUTO_START_AFTER_MISSES: f64 = 5.0;
/// The minimum gap between two watchdog-started agents.
pub const AUTO_START_MIN_GAP_MS: f64 = 5.0 * 60.0 * 1000.0;

/// The watchdog's predicate.
pub fn watchdog_should_start(misses: f64, last_start_at: f64, now: f64) -> bool {
    misses >= AUTO_START_AFTER_MISSES && now - last_start_at > AUTO_START_MIN_GAP_MS
}

/// The one line the watchdog leaves behind. It is a format rather than a concatenation at the call site
/// because it is the ONLY evidence that the shell started the agent by itself, and `error` is the host's
/// `schtasks` message, reported verbatim.
pub fn watchdog_log(ok: bool, error: Option<&str>) -> String {
    let outcome = if ok {
        "ok".to_string()
    } else {
        format!("failed: {}", error.unwrap_or(""))
    };
    format!("[summrise] agent watchdog: schtasks /run {AGENT_TASK} \u{2192} {outcome}")
}

/// The first retry delay, and the value the backoff resets to after a successful probe.
pub const RETRY_START_MS: f64 = 2_000.0;
/// The backoff's ceiling: "2s → 4s → 8s → 30s cap, so a dead agent doesn't spam retries".
pub const RETRY_CAP_MS: f64 = 30_000.0;

/// `nextRetry()` — double, capped.
pub fn next_retry_ms(current: f64) -> f64 {
    (current * 2.0).min(RETRY_CAP_MS)
}

/// The tripwire's re-arm delay after it snaps the main window back.
pub const TRIPWIRE_BACKOFF_MS: f64 = 2_000.0;

/// `if (!isMainFrame || errorCode === -3) return;` — should a load failure schedule a retry?
///
/// `-3` is `ERR_ABORTED`, and the exclusion is a MEASURED fix: "a same-URL reload dispatches
/// `did-fail-load(-3, ERR_ABORTED)` on the MAIN frame, and subframe failures fired too — each stacked
/// ANOTHER `loadDesktop` chain (accelerated miss counting + duplicate loadURLs)." So only a real
/// main-frame failure retries.
pub fn should_retry_load(is_main_frame: bool, error_code: f64) -> bool {
    is_main_frame && error_code != -3.0
}

/// `win?.webContents.getURL().startsWith("data:text/html")` — is the wait page already showing?
///
/// "DO NOT RE-LOAD A PAGE THAT IS ALREADY SHOWING": the reload wiped the Start Agent button's answer and
/// reset the evidence line, "which is what made the only control on this screen unfalsifiable".
///
/// It is a prefix test and it is a SAFE one, which is worth stating because the shell has been bitten by
/// the other kind: the URL it tests is one the shell itself built
/// (`data:text/html;charset=utf-8,` + the encoded document), so no page can make itself match.
pub fn is_wait_page(url: &str) -> bool {
    url.starts_with("data:text/html")
}

/// `typeof r.statusCode === "number" && r.statusCode > 0` — did the agent's accept loop answer at all?
///
/// **NOT `=== 200`, AND REQUIRING 200 WAS A SELF-CAUGHT REGRESSION**: `/api/status` is token-gated, so a
/// HEALTHY agent answers 401 to the shell's credential-less probe; requiring 200 "made a live agent look
/// dead and sent the watchdog into a restart loop". ANY HTTP response proves the accept loop is alive —
/// which is the exact thing the raw TCP probe could not see, since a wedged-but-listening agent holds
/// the port.
pub fn status_is_alive(status_code: Option<f64>) -> bool {
    matches!(status_code, Some(code) if code > 0.0)
}

/// The three probe budgets, each a different question.
/// The boot probe: short, because it runs before the first window is shown.
pub const AGENT_PROBE_BOOT_MS: f64 = 800.0;
/// The tray's probe: one poll of a 30 s timer.
pub const AGENT_PROBE_TRAY_MS: f64 = 1_000.0;
/// The default probe budget.
pub const AGENT_PROBE_DEFAULT_MS: f64 = 2_000.0;
/// The `/api/status` fetch behind the tray's vitals.
pub const STATUS_FETCH_MS: f64 = 2_500.0;
/// The `/api/workspace/harnesses` fetch at boot.
pub const HARNESS_FETCH_MS: f64 = 4_000.0;
/// The menu flush delay after `did-finish-load`, "so the SPA's React effect has time to register the
/// `summrise-menu` listener".
pub const MENU_FLUSH_MS: f64 = 1_000.0;
/// The tray's poll interval.
pub const TRAY_POLL_MS: f64 = 30_000.0;
/// The CDP self-check's fetch budget.
pub const CDP_CHECK_MS: f64 = 3_000.0;

/// The tripwire's allow-list: `isDesktopSpaUrl(url) || url.startsWith("data:") || url === "about:blank"`.
///
/// THE TRIPWIRE ITSELF IS A DEVICE-CAUGHT FIX (round-258): CDP-driven navigation BYPASSES
/// `will-navigate`, and "a `browser_navigate` call hijacked the main window to qq.com and the panel
/// vanished". `desktop_spa` is that predicate's answer, evaluated by the host — it lives in the
/// url-policy crate over that crate's port state and this one must not hold a second copy (see
/// `Cargo.toml`). The two LITERAL carve-outs are this crate's, and they are here rather than at the call
/// site because the sequence
///
/// ```text
///   if (… || url === "about:blank") return;
///   if (url === "about:blank") return;
/// ```
///
/// in `main.ts` states the blank case TWICE. The second one is UNREACHABLE — the first already returns
/// for it — and one function is where that stopped being invisible. The port deleted it.
pub fn tripwire_allows(url: &str, desktop_spa: bool) -> bool {
    desktop_spa || url.starts_with("data:") || url == crate::sessions::ABOUT_BLANK
}

/// The tripwire's log line: `url.slice(0, 80)`, counted in JavaScript's units.
pub fn tripwire_log(url: &str) -> String {
    format!(
        "[summrise] main-window tripwire: blocked stray navigation to {}",
        crate::js::truncate_utf16(url, LOG_URL_UNITS)
    )
}

/// How much of a stray URL the log keeps, in UTF-16 code units (`url.slice(0, 80)`).
pub const LOG_URL_UNITS: usize = 80;
/// How much of a foreign `User-Agent` the CDP warning prints (`ua.slice(0, 60)`).
pub const CDP_UA_UNITS: usize = 60;
/// The marker the shell's own Chromium puts in its `User-Agent`.
pub const CDP_UA_MARKER: &str = "summrise-desktop-electron";

/// `(j["User-Agent"] || "").includes("summrise-desktop-electron")` — does port 9333 belong to THIS app?
///
/// The self-check exists because "if 9333 is occupied by another process (a second browser/electron),
/// `remote-debugging-port` silently fails and AI driving would hit the WRONG target". The ANSWER is the
/// decision; the `/json/version` fetch is the host's.
pub fn cdp_user_agent_is_ours(user_agent: Option<&str>) -> bool {
    crate::js::includes(user_agent.unwrap_or(""), CDP_UA_MARKER)
}

/// `(j["User-Agent"] || "")` out of the CDP `/json/version` document.
///
/// The FALSY TEST IS THE DECISION and it is why this is not left at the call site: an absent
/// `User-Agent` key, a `null` value, an empty string and a non-string value all answer the empty string
/// here — and `""` fails [`cdp_user_agent_is_ours`], which is the correct verdict for a version document
/// that does not say whose it is. A body that does not parse is the same answer.
pub fn cdp_user_agent(version_json: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(version_json) {
        Ok(value) => match value.get("User-Agent") {
            Some(serde_json::Value::String(text)) => text.clone(),
            _ => String::new(),
        },
        Err(_) => String::new(),
    }
}

/// `cdp_user_agent_is_ours(Some(&ua))` where the answer is the empty string — the shape `main.ts` reads
/// after [`cdp_user_agent`], kept as one call so the two cannot be composed wrongly.
pub fn cdp_self_check_owns_port(version_json: &str) -> bool {
    cdp_user_agent_is_ours(Some(&cdp_user_agent(version_json)))
}

/// The three CDP self-check messages, verbatim from `main.ts` — including the ellipsis and the em dash,
/// because these lines are read off a device's log by a person deciding whether AI driving is safe.
pub fn cdp_self_check_ok(port: u16) -> String {
    format!("[summrise] CDP self-check OK: 127.0.0.1:{port} is this app")
}

/// The warning that another browser owns the port.
pub fn cdp_warning_foreign(port: u16, user_agent: &str) -> String {
    format!(
        "[summrise] CDP WARNING: port {port} answered by another browser (UA={}\u{2026}) \u{2014} AI driving may target the wrong process",
        crate::js::truncate_utf16(user_agent, CDP_UA_UNITS)
    )
}

/// The warning that the endpoint answered but not with a version document.
pub fn cdp_warning_not_responding(port: u16) -> String {
    format!("[summrise] CDP WARNING: {port} not responding \u{2014} remote debugging may be off")
}

/// The warning that the endpoint could not be reached at all.
pub fn cdp_warning_unreachable(port: u16) -> String {
    format!("[summrise] CDP WARNING: could not reach {port} \u{2014} remote debugging may be off")
}
