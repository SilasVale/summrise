//! The `SummriseRepair` task, as the launcher repairs it: one `schtasks` query, one decision, and —
//! only when the action is not the quoted script — one `/create`.
//!
//! The decision and the argument strings are [`summrise_core::repair_task`]'s, so the launcher and
//! the app cannot write two different registrations. What lives here is the launcher's side of the
//! effect: the two argv arrays, the injected runner that makes the sequence testable on Linux, and
//! the outcome the Windows supervisor logs.
//!
//! **A CHANNEL UPDATE CANNOT REPLACE THE LAUNCHER.** The launcher is the stable root of the repair
//! path; only an installer writes it. A wrong registration written by an old installer therefore
//! has to be repaired by whichever process the update did replace — the app — and by a launcher
//! that was installed after the fix. Both call this decision: the launcher at boot and after an
//! apply it performed, the app at its own start.

use std::io;
use std::path::Path;

use summrise_core::repair_task::{self, TaskDecision};

/// The Task Scheduler name, re-exported so the Windows half needs no second import path.
pub use summrise_core::repair_task::TASK_NAME;

/// One `schtasks` call's answer: whether it exited successfully, and what it wrote to stdout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolRun {
    /// `schtasks` exited 0.
    pub success: bool,
    /// Its stdout, which is the task XML for a query.
    pub stdout: String,
}

/// What the launcher did about the task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The task exists and its action is the quoted script.
    Current,
    /// The task was written: `stale` says one existed with a different action.
    Registered { stale: bool },
    /// No `repair.cmd` under the install root, so this tree is not one the task belongs to.
    NoScript,
    /// `schtasks` could not be run or refused the registration. Best effort; the caller logs it.
    Failed(String),
}

/// The `schtasks` arguments that ask for the task's XML, in the one form this module reads.
pub fn query_args() -> Vec<String> {
    ["/query", "/tn", TASK_NAME, "/xml"]
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect()
}

/// Ensure the install's repair task starts its quoted `repair.cmd` with the five-minute trigger.
///
/// `run` is the OS effect: the shipped launcher passes a `schtasks` invocation, a test passes a
/// recorder. A query that exits non-zero means "no such task" and is registered; a query that cannot
/// be started at all is a failure and registers nothing, because a broken `schtasks` would fail the
/// create too. A task that already has the quoted action is left exactly as it is — including a
/// deliberately disabled one, which is how the installer's own comment says a machine is kept down.
pub fn ensure<F>(install: &Path, mut run: F) -> Outcome
where
    F: FnMut(&[String]) -> io::Result<ToolRun>,
{
    let script_exists = repair_task::script_path(install).is_file();
    let answer = if script_exists {
        match run(&query_args()) {
            Ok(answer) if answer.success => Some(answer.stdout),
            Ok(_) => None,
            Err(error) => return Outcome::Failed(format!("cannot run schtasks: {error}")),
        }
    } else {
        None
    };
    match repair_task::plan(install, script_exists, answer.as_deref()) {
        TaskDecision::NoScript => Outcome::NoScript,
        TaskDecision::Current => Outcome::Current,
        TaskDecision::Register { stale } => match run(&repair_task::registration_args(install)) {
            Ok(answer) if answer.success => Outcome::Registered { stale },
            Ok(_) => {
                Outcome::Failed("schtasks refused to register the SummriseRepair task".to_owned())
            }
            Err(error) => Outcome::Failed(format!("cannot run schtasks: {error}")),
        },
    }
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
                "summrise-launch-repair-task-{}-{}-{tag}",
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

    /// An install with a `repair.cmd` under a path that has a space, the measured shape.
    fn install_with_script(tag: &str) -> (TempDir, PathBuf) {
        let tmp = TempDir::new(tag);
        let install = tmp.path().join("Program Files").join("Summrise");
        std::fs::create_dir_all(&install).expect("create install dir");
        std::fs::write(repair_task::script_path(&install), b"@echo off\r\n")
            .expect("write repair.cmd");
        (tmp, install)
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

    fn current_action(install: &Path) -> repair_task::Action {
        repair_task::Action {
            command: repair_task::expected_command(install),
            arguments: String::new(),
        }
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

    fn run_ok(stdout: &str) -> ToolRun {
        ToolRun {
            success: true,
            stdout: stdout.to_owned(),
        }
    }

    fn refusal() -> ToolRun {
        ToolRun {
            success: false,
            stdout: String::new(),
        }
    }

    #[test]
    fn a_stale_split_command_is_re_registered_with_the_five_minute_trigger() {
        let (_tmp, install) = install_with_script("stale");
        let calls = RefCell::new(Vec::new());
        let mut answers = vec![run_ok(&xml(&split_action(&install))), run_ok("")].into_iter();

        let outcome = ensure(&install, |args| {
            calls.borrow_mut().push(args.to_vec());
            Ok(answers.next().expect("one answer per call"))
        });

        assert_eq!(outcome, Outcome::Registered { stale: true });
        let calls = calls.into_inner();
        assert_eq!(
            calls,
            vec![query_args(), repair_task::registration_args(&install)],
            "a stale command is followed by exactly one registration"
        );
        let create = &calls[1];
        let expected = repair_task::expected_command(&install);
        assert_eq!(
            create[create.iter().position(|arg| arg == "/tr").unwrap() + 1],
            expected,
            "the new action is the quoted script as one argument: {create:?}"
        );
        assert_eq!(
            create[create.iter().position(|arg| arg == "/sc").unwrap() + 1],
            "minute"
        );
        assert_eq!(
            create[create.iter().position(|arg| arg == "/mo").unwrap() + 1],
            "5",
            "the repaired task keeps the five-minute trigger: {create:?}"
        );
        assert!(create.iter().any(|arg| arg == "/f"), "{create:?}");
    }

    #[test]
    fn a_missing_task_is_registered_as_missing_not_stale() {
        let (_tmp, install) = install_with_script("missing");
        let calls = RefCell::new(Vec::new());
        let mut answers = vec![refusal(), run_ok("")].into_iter();

        let outcome = ensure(&install, |args| {
            calls.borrow_mut().push(args.to_vec());
            Ok(answers.next().expect("one answer per call"))
        });

        assert_eq!(outcome, Outcome::Registered { stale: false });
        assert_eq!(
            calls.into_inner(),
            vec![query_args(), repair_task::registration_args(&install)]
        );
    }

    #[test]
    fn a_current_task_is_left_alone() {
        let (_tmp, install) = install_with_script("current");
        let calls = RefCell::new(Vec::new());
        let mut answers = vec![run_ok(&xml(&current_action(&install)))].into_iter();

        let outcome = ensure(&install, |args| {
            calls.borrow_mut().push(args.to_vec());
            Ok(answers.next().expect("one answer per call"))
        });

        assert_eq!(outcome, Outcome::Current);
        assert_eq!(calls.into_inner(), vec![query_args()], "no rewrite");
    }

    #[test]
    fn a_missing_repair_script_is_not_this_task_to_register() {
        let tmp = TempDir::new("no-script");
        let install = tmp.path().join("Program Files").join("Summrise");
        std::fs::create_dir_all(&install).expect("create install dir");
        let calls = RefCell::new(Vec::new());

        let outcome = ensure(&install, |args| {
            calls.borrow_mut().push(args.to_vec());
            Ok(run_ok(""))
        });

        assert_eq!(outcome, Outcome::NoScript);
        assert!(
            calls.into_inner().is_empty(),
            "a tree the installer did not write gets no scheduled task and no query"
        );
    }

    #[test]
    fn a_query_that_cannot_start_registers_nothing() {
        let (_tmp, install) = install_with_script("query-broken");
        let calls = RefCell::new(Vec::new());

        let outcome = ensure(&install, |args| {
            calls.borrow_mut().push(args.to_vec());
            Err(io::Error::new(io::ErrorKind::NotFound, "no schtasks"))
        });

        assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
        assert_eq!(
            calls.into_inner(),
            vec![query_args()],
            "a broken query must not be followed by a create against the same broken tool"
        );
    }

    #[test]
    fn a_refused_registration_is_reported() {
        let (_tmp, install) = install_with_script("create-refused");
        let mut answers = vec![run_ok(&xml(&split_action(&install))), refusal()].into_iter();

        let outcome = ensure(&install, |_| {
            Ok(answers.next().expect("one answer per call"))
        });

        assert!(
            matches!(outcome, Outcome::Failed(ref reason) if reason.contains("refused")),
            "{outcome:?}"
        );
    }

    #[test]
    fn the_query_asks_for_the_named_task_as_xml() {
        assert_eq!(
            query_args(),
            vec!["/query", "/tn", "SummriseRepair", "/xml"]
        );
    }
}
