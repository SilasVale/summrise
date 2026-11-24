//! The `SummriseRepair` task from the app's side: its health for the Machine tab, and the app's own
//! repair of a registration an old installer wrote wrong.
//!
//! **THE APP IS THE ONLY UPDATED PROCESS THAT CAN FIX THIS.** The launcher is deliberately never
//! replaced by a channel update — it is the stable root of the repair path — so a wrong `SummriseRepair`
//! registration written by an installer survives every release that does not run a setup. That is the
//! measured 9VH: 0.3.279 wrote `Command=C:\Program` + `Arguments=Files\Summrise\repair.cmd`, every
//! five-minute run ended `0x80070002`, and the machine took 0.3.289–0.3.291 by channel without the
//! task changing. The 0.3.292 app therefore applies the same decision as a fixed launcher at its own
//! start: an action that is not the quoted `"<install>\repair.cmd"` is re-registered with the
//! installer's five-minute trigger. No installer is run.
//!
//! The health facts are the machine's own Task Scheduler answer: whether the task is registered, its
//! last run and result, and its next run. They are read through PowerShell's `Get-ScheduledTask`
//! cmdlets, whose object properties are locale-independent, rather than from the localized text of
//! `schtasks /v`. A read that fails omits the section: "the console could not ask" is not "the task
//! is healthy", and the facts never invent a result.

#[cfg(any(windows, test))]
use std::io;
use std::path::Path;

#[cfg(any(windows, test))]
use summrise_core::repair_task::{self, TaskDecision};

/// One `schtasks` call's answer: whether it exited successfully, and its stdout (the XML for a query).
#[cfg(any(windows, test))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct ToolRun {
    success: bool,
    stdout: String,
}

/// What the app did about the task, for the one log line the start writes.
#[cfg(any(windows, test))]
#[derive(Clone, Debug, PartialEq, Eq)]
enum Ensure {
    Current,
    Registered { stale: bool },
    NoScript,
    Failed(String),
}

/// Ensure the install's repair task starts its quoted `repair.cmd` with the five-minute trigger.
///
/// `run` is the OS effect, so the sequence is testable without Windows. The decision is
/// [`summrise_core::repair_task::plan`]'s — the one the launcher also applies — so the app and the
/// launcher cannot disagree about what a healthy registration is.
#[cfg(any(windows, test))]
fn ensure_with<F>(install: &Path, mut run: F) -> Ensure
where
    F: FnMut(&[String]) -> io::Result<ToolRun>,
{
    let script_exists = repair_task::script_path(install).is_file();
    let answer = if script_exists {
        match run(&[
            "/query".to_owned(),
            "/tn".to_owned(),
            repair_task::TASK_NAME.to_owned(),
            "/xml".to_owned(),
        ]) {
            Ok(answer) if answer.success => Some(answer.stdout),
            Ok(_) => None,
            Err(error) => return Ensure::Failed(format!("cannot run schtasks: {error}")),
        }
    } else {
        None
    };
    match repair_task::plan(install, script_exists, answer.as_deref()) {
        TaskDecision::NoScript => Ensure::NoScript,
        TaskDecision::Current => Ensure::Current,
        TaskDecision::Register { stale } => match run(&repair_task::registration_args(install)) {
            Ok(answer) if answer.success => Ensure::Registered { stale },
            Ok(_) => {
                Ensure::Failed("schtasks refused to register the SummriseRepair task".to_owned())
            }
            Err(error) => Ensure::Failed(format!("cannot run schtasks: {error}")),
        },
    }
}

/// Repair a wrong registration at this start. Best effort, one log line when it changes something.
///
/// A channel update restarts the app under the new version, so this is the update's first moment on
/// the machine — and every later start heals a registration deleted by a person or a policy too. A
/// task whose action is already right is left alone, including a disabled one: the installer's own
/// script documents disabling the task as how a machine is kept down on purpose.
#[cfg(windows)]
pub(crate) fn ensure(install: &Path) {
    match ensure_with(install, run_schtasks) {
        Ensure::Registered { stale: true } => eprintln!(
            "summrise-device: re-registered the SummriseRepair task: its action was not the quoted repair.cmd"
        ),
        Ensure::Registered { stale: false } => {
            eprintln!("summrise-device: registered the missing SummriseRepair task")
        }
        Ensure::Failed(reason) => {
            eprintln!("summrise-device: warning: could not ensure the SummriseRepair task: {reason}")
        }
        Ensure::Current | Ensure::NoScript => {}
    }
}

/// The same start hook's no-op where there is no Task Scheduler (the Linux test build).
#[cfg(not(windows))]
pub(crate) fn ensure(install: &Path) {
    let _ = install;
}

/// The scheduled task's health, as the machine's Task Scheduler reports it, or `None` when no answer
/// could be read. An absent key draws no row in the console; a present `registered: false` is the
/// task being absent, which is a fact and not an unread answer.
pub(crate) fn facts() -> Option<serde_json::Value> {
    #[cfg(windows)]
    {
        read_health()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// The PowerShell that answers with one compact JSON object.
///
/// `Get-ScheduledTask` returns `$null` when the task is absent (with `-ErrorAction SilentlyContinue`,
/// so a missing task is not an error). `Get-ScheduledTaskInfo` gives the three facts the Machine tab
/// names; the two moments are converted to Unix milliseconds here so the console does not have to
/// parse a locale's date format. A never-run task reports `12/30/1899`, and a next run that is absent
/// is `$null`; both become `null`, which the parser omits rather than inventing a moment.
#[cfg(windows)]
const HEALTH_SCRIPT: &str = concat!(
    "$ErrorActionPreference='Stop';",
    "$task=Get-ScheduledTask -TaskName 'SummriseRepair' -ErrorAction SilentlyContinue;",
    "if($null -eq $task){[pscustomobject]@{registered=$false}|ConvertTo-Json -Compress;exit 0};",
    "$info=$task|Get-ScheduledTaskInfo;",
    "$epoch=[datetime]::new(1970,1,1,0,0,0,[System.DateTimeKind]::Utc);",
    "function ms($m){if($null -eq $m){return $null};",
    "$u=([datetime]$m).ToUniversalTime();",
    "if($u.Year -lt 2000){return $null};",
    "[long](($u-$epoch).TotalMilliseconds)};",
    "[pscustomobject]@{registered=$true;",
    "last_run_ms=$(ms $info.LastRunTime);",
    "last_result=[long]$info.LastTaskResult;",
    "next_run_ms=$(ms $info.NextRunTime)}|ConvertTo-Json -Compress"
);

/// The task health document from PowerShell's JSON, or `None` when it is not that document.
#[cfg(any(windows, test))]
fn parse_health(text: &str) -> Option<serde_json::Value> {
    #[derive(serde::Deserialize)]
    struct Health {
        registered: bool,
        #[serde(default)]
        last_run_ms: Option<u64>,
        #[serde(default)]
        last_result: Option<u32>,
        #[serde(default)]
        next_run_ms: Option<u64>,
    }
    let health: Health = serde_json::from_str(text.trim()).ok()?;
    let mut document = serde_json::Map::new();
    document.insert(
        "registered".to_owned(),
        serde_json::Value::Bool(health.registered),
    );
    if health.registered {
        if let Some(last_run_ms) = health.last_run_ms {
            document.insert("last_run_ms".to_owned(), last_run_ms.into());
        }
        if let Some(last_result) = health.last_result {
            document.insert("last_result".to_owned(), last_result.into());
        }
        if let Some(next_run_ms) = health.next_run_ms {
            document.insert("next_run_ms".to_owned(), next_run_ms.into());
        }
    }
    Some(serde_json::Value::Object(document))
}

#[cfg(windows)]
fn read_health() -> Option<serde_json::Value> {
    use base64::Engine as _;
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut utf16 = Vec::with_capacity(HEALTH_SCRIPT.len() * 2);
    for unit in HEALTH_SCRIPT.encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
    let output = std::process::Command::new(powershell())
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_health(&String::from_utf8_lossy(&output.stdout))
}

/// The Windows PowerShell the health read runs: `System32`'s own copy, because a service's PATH is
/// not a user's and a 32-bit process must not be redirected out of it.
#[cfg(windows)]
fn powershell() -> std::path::PathBuf {
    match std::env::var_os("SystemRoot") {
        Some(root) => std::path::PathBuf::from(root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe"),
        None => std::path::PathBuf::from("powershell.exe"),
    }
}

/// Run `schtasks` without a console window and capture its answer.
#[cfg(windows)]
fn run_schtasks(args: &[String]) -> io::Result<ToolRun> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("schtasks")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    Ok(ToolRun {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "summrise-device-repair-task-{}-{}-{tag}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create temp dir");
            TempDir(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn install_with_script(tag: &str) -> (TempDir, PathBuf) {
        let tmp = TempDir::new(tag);
        let install = tmp.path().join("Program Files").join("Summrise");
        std::fs::create_dir_all(&install).expect("create install dir");
        std::fs::write(repair_task::script_path(&install), b"@echo off\r\n")
            .expect("write repair.cmd");
        (tmp, install)
    }

    /// The action schtasks stores from an unquoted `/tr`: the command ends at the first space.
    fn split_action(install: &Path) -> repair_task::Action {
        let quoted = repair_task::expected_command(install);
        let unquoted = quoted.trim_matches('"');
        let (command, arguments) = unquoted
            .split_once(' ')
            .expect("the fixture install path has a space");
        repair_task::Action {
            command: command.to_owned(),
            arguments: arguments.to_owned(),
        }
    }

    fn xml(action: &repair_task::Action) -> String {
        format!(
            concat!(
                r#"<Task version="1.2"><Actions Context="Author"><Exec>"#,
                r#"<Command>{}</Command><Arguments>{}</Arguments>"#,
                r#"</Exec></Actions></Task>"#,
            ),
            action.command, action.arguments
        )
    }

    #[test]
    fn the_app_re_registers_the_measured_split_command() {
        let (_tmp, install) = install_with_script("stale");
        let calls = RefCell::new(Vec::new());
        let mut answers = vec![
            ToolRun {
                success: true,
                stdout: xml(&split_action(&install)),
            },
            ToolRun {
                success: true,
                stdout: String::new(),
            },
        ]
        .into_iter();

        let outcome = ensure_with(&install, |args| {
            calls.borrow_mut().push(args.to_vec());
            Ok(answers.next().expect("one answer per call"))
        });

        assert_eq!(outcome, Ensure::Registered { stale: true });
        assert_eq!(
            calls.into_inner(),
            vec![
                vec![
                    "/query".to_owned(),
                    "/tn".to_owned(),
                    "SummriseRepair".to_owned(),
                    "/xml".to_owned(),
                ],
                repair_task::registration_args(&install),
            ]
        );
    }

    #[test]
    fn a_healthy_registration_is_left_alone() {
        let (_tmp, install) = install_with_script("current");
        let calls = RefCell::new(Vec::new());
        let mut answers = vec![ToolRun {
            success: true,
            stdout: xml(&repair_task::Action {
                command: repair_task::expected_command(&install),
                arguments: String::new(),
            }),
        }]
        .into_iter();

        let outcome = ensure_with(&install, |args| {
            calls.borrow_mut().push(args.to_vec());
            Ok(answers.next().expect("one answer per call"))
        });

        assert_eq!(outcome, Ensure::Current);
        assert_eq!(
            calls.into_inner().len(),
            1,
            "a current registration is only queried"
        );
    }

    #[test]
    fn a_tree_without_the_script_is_not_registered() {
        let tmp = TempDir::new("no-script");
        let install = tmp.path().join("Program Files").join("Summrise");
        std::fs::create_dir_all(&install).expect("create install dir");
        let calls = RefCell::new(Vec::new());

        let outcome = ensure_with(&install, |args| {
            calls.borrow_mut().push(args.to_vec());
            Ok(ToolRun {
                success: true,
                stdout: String::new(),
            })
        });

        assert_eq!(outcome, Ensure::NoScript);
        assert!(calls.into_inner().is_empty());
    }

    #[test]
    fn the_health_of_a_registered_task_carries_its_result_and_next_run() {
        assert_eq!(
            parse_health(
                r#"{"registered":true,"last_run_ms":1794086101000,"last_result":2147942402,"next_run_ms":1794086400000}"#
            ),
            Some(serde_json::json!({
                "registered": true,
                "last_run_ms": 1794086101000_u64,
                "last_result": 2147942402_u32,
                "next_run_ms": 1794086400000_u64,
            }))
        );
    }

    #[test]
    fn an_unregistered_task_reports_only_that() {
        assert_eq!(
            parse_health(r#"{"registered":false}"#),
            Some(serde_json::json!({ "registered": false })),
            "no moment or result is invented for a task that does not exist"
        );
    }

    #[test]
    fn a_task_that_has_not_run_reports_no_moments() {
        assert_eq!(
            parse_health(
                r#"{"registered":true,"last_run_ms":null,"last_result":0,"next_run_ms":null}"#
            ),
            Some(serde_json::json!({ "registered": true, "last_result": 0 }))
        );
    }

    #[test]
    fn a_torn_answer_is_no_fact_rather_than_a_zero() {
        assert_eq!(parse_health("not json"), None);
        assert_eq!(parse_health(""), None);
    }
}
