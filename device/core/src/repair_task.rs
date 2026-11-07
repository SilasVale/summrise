//! The five-minute `SummriseRepair` task: the action that starts the install's repair script, the
//! decision that an existing registration is wrong, and the `schtasks` arguments that replace it.
//!
//! **A REPAIR TASK THAT CANNOT START IS WORSE THAN NONE.** The task is the safety net that starts
//! `SummriseLaunch` when it is stopped and re-registers it when it is gone — the path a machine takes
//! back from a crash without a person at the keyboard. The 0.3.279 installer wrote it with an
//! unquoted `/tr`, so Task Scheduler stored `Command=C:\Program` + `Arguments=Files\Summrise\repair.cmd`
//! and every five-minute run ended `0x80070002` (file not found) while `repair.cmd` sat beside it.
//! A channel update does not run the installer, so a registration written wrong by one installer
//! survives every later release unless the running product repairs it.
//!
//! This module is the pure half: no file is read, no process is started, no clock is asked. The
//! launcher and the device each run `schtasks` and ask [`plan`] what to do; the command and the
//! registration arguments live here so the two cannot write two different tasks.

use std::path::{Path, PathBuf};

/// The Task Scheduler name the installer registers, and the one the launcher and the app repair.
pub const TASK_NAME: &str = "SummriseRepair";

/// The installer-written script the task starts, beside the launcher.
pub const SCRIPT_NAME: &str = "repair.cmd";

/// The trigger the installer registers: every five minutes.
pub const INTERVAL_MINUTES: u32 = 5;

/// The script's path under the install root.
pub fn script_path(install: &Path) -> PathBuf {
    install.join(SCRIPT_NAME)
}

/// The command a healthy task must carry: the script's path, quoted as one value.
///
/// The quotes are the fix. `schtasks /tr "<path>"` splits a path with a space into a command and
/// arguments; `/tr "\"<path>\""` stores one quoted command, and Task Scheduler starts the script.
/// The 84T's healthy registration reads `"E:\Program Files\Summrise\repair.cmd"`.
pub fn expected_command(install: &Path) -> String {
    format!("\"{}\"", script_path(install).display())
}

/// The two fields of Task Scheduler's `<Exec>` action that decide whether the task can start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    /// The executable, exactly as Task Scheduler stores it — quotes included.
    pub command: String,
    /// The arguments; an empty string for a healthy repair task.
    pub arguments: String,
}

/// Whether the action starts this install's repair script and nothing else.
pub fn action_is_current(install: &Path, action: &Action) -> bool {
    action.command == expected_command(install) && action.arguments.trim().is_empty()
}

/// What to do about the task, from whether the script is installed and what `schtasks` answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskDecision {
    /// No repair script under the install root: this tree is not one the task belongs to.
    NoScript,
    /// The task exists, and its action is the quoted script with no arguments.
    Current,
    /// No task, or one whose action is not the quoted script. `stale` separates the two so a caller
    /// can say "re-registered" from "registered".
    Register { stale: bool },
}

/// Decide from `query`, which is the `/query /xml` answer (`None` when the task is absent).
///
/// A query whose document this build cannot read is treated as stale rather than current: the
/// task is rewritten with the one form the installer and this module agree on. `/f` makes the
/// replacement idempotent, and a task that cannot be read is already not one to trust.
pub fn plan(install: &Path, script_exists: bool, query: Option<&str>) -> TaskDecision {
    if !script_exists {
        return TaskDecision::NoScript;
    }
    match query {
        None => TaskDecision::Register { stale: false },
        Some(xml) => match parse_action(xml) {
            Some(action) if action_is_current(install, &action) => TaskDecision::Current,
            _ => TaskDecision::Register { stale: true },
        },
    }
}

/// The `<Exec>` action in a `schtasks /query /xml` document, or `None` when there is none.
pub fn parse_action(xml: &str) -> Option<Action> {
    Some(Action {
        command: element(xml, "Command")?,
        arguments: element(xml, "Arguments").unwrap_or_default(),
    })
}

/// The text of the first `<name>…</name>` element, with XML's predefined entities decoded.
fn element(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(unescape(&xml[start..end]))
}

/// The five entities an XML text node uses. Task Scheduler writes an install path's `&` as `&amp;`
/// and leaves quotes literal, so a command that looks current must be compared after decoding.
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// The `schtasks` arguments that register the task exactly as the installer does.
///
/// One trigger, five minutes, as SYSTEM at the highest run level: the task must be able to start the
/// service, and `/f` makes a repair of an existing registration idempotent. The `/tr` value is the
/// quoted script as ONE argument — the form measured to survive a path with a space.
pub fn registration_args(install: &Path) -> Vec<String> {
    let expected = expected_command(install);
    [
        "/create",
        "/tn",
        TASK_NAME,
        "/tr",
        expected.as_str(),
        "/sc",
        "minute",
        "/mo",
        "5",
        "/ru",
        "SYSTEM",
        "/rl",
        "HIGHEST",
        "/f",
    ]
    .iter()
    .map(|argument| (*argument).to_owned())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path with a space, the shape that measured the bug: the installer's default root.
    fn install() -> PathBuf {
        PathBuf::from("/opt/Program Files/Summrise")
    }

    fn xml(action: &Action) -> String {
        format!(
            concat!(
                r#"<Task version="1.2"><Triggers><TimeTrigger><Repetition>"#,
                r#"<Interval>PT5M</Interval></Repetition></TimeTrigger></Triggers>"#,
                r#"<Actions Context="Author"><Exec><Command>{}</Command>"#,
                r#"<Arguments>{}</Arguments></Exec></Actions></Task>"#,
            ),
            action.command, action.arguments
        )
    }

    /// The action schtasks stores from an unquoted `/tr "<path>"`: command before the first space,
    /// arguments after it. This is the measured `C:\Program` + `Files\Summrise\repair.cmd` pair.
    fn split_at_first_space(quoted: &str) -> Action {
        let unquoted = quoted.trim_matches('"');
        let (command, arguments) = unquoted
            .split_once(' ')
            .expect("the fixture install path has a space");
        Action {
            command: command.to_owned(),
            arguments: arguments.to_owned(),
        }
    }

    #[test]
    fn the_quoted_script_with_no_arguments_is_the_current_action() {
        let install = install();
        let action = Action {
            command: expected_command(&install),
            arguments: String::new(),
        };
        assert!(action_is_current(&install, &action), "{action:?}");
        assert_eq!(
            action.command,
            format!("\"{}\"", script_path(&install).display()),
            "the command is the quoted script path, as one value"
        );
    }

    #[test]
    fn the_split_command_a_wrong_installer_stored_is_stale() {
        let install = install();
        let stored = split_at_first_space(&expected_command(&install));
        assert!(
            !action_is_current(&install, &stored),
            "the split command is the measured 0x80070002 registration: {stored:?}"
        );
    }

    #[test]
    fn a_quoted_command_with_arguments_is_stale() {
        let install = install();
        let action = Action {
            command: expected_command(&install),
            arguments: "/S".to_owned(),
        };
        assert!(!action_is_current(&install, &action));
    }

    #[test]
    fn an_unquoted_command_is_stale_even_without_spaces() {
        let install = install();
        let action = Action {
            command: script_path(&install).display().to_string(),
            arguments: String::new(),
        };
        assert!(
            !action_is_current(&install, &action),
            "the quoted form is the one Task Scheduler can start from a path with a space"
        );
    }

    #[test]
    fn the_registration_is_the_installers_five_minute_system_task() {
        assert_eq!(
            INTERVAL_MINUTES, 5,
            "the installer's trigger is five minutes"
        );
        let install = install();
        let expected = expected_command(&install);
        assert_eq!(
            registration_args(&install),
            vec![
                "/create".to_owned(),
                "/tn".to_owned(),
                "SummriseRepair".to_owned(),
                "/tr".to_owned(),
                expected,
                "/sc".to_owned(),
                "minute".to_owned(),
                "/mo".to_owned(),
                "5".to_owned(),
                "/ru".to_owned(),
                "SYSTEM".to_owned(),
                "/rl".to_owned(),
                "HIGHEST".to_owned(),
                "/f".to_owned(),
            ]
        );
    }

    #[test]
    fn the_decision_needs_the_script_the_task_and_the_quoted_action() {
        let install = install();
        let current = Action {
            command: expected_command(&install),
            arguments: String::new(),
        };
        assert_eq!(plan(&install, false, None), TaskDecision::NoScript);
        assert_eq!(
            plan(&install, true, None),
            TaskDecision::Register { stale: false },
            "no task is registered, not stale"
        );
        assert_eq!(
            plan(&install, true, Some(&xml(&current))),
            TaskDecision::Current
        );
        let stored = split_at_first_space(&expected_command(&install));
        assert_eq!(
            plan(&install, true, Some(&xml(&stored))),
            TaskDecision::Register { stale: true }
        );
        assert_eq!(
            plan(&install, true, Some("<Task><Actions/></Task>")),
            TaskDecision::Register { stale: true },
            "a document with no action is not a task to trust"
        );
    }

    #[test]
    fn the_action_is_read_from_the_exec_and_its_entities_decoded() {
        let document = concat!(
            r#"<?xml version="1.0" encoding="UTF-16"?>"#,
            r#"<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">"#,
            r#"<Actions Context="Author"><Exec>"#,
            r#"<Command>"C:\A&amp;B\repair.cmd"</Command>"#,
            r#"<Arguments></Arguments></Exec></Actions></Task>"#,
        );
        assert_eq!(
            parse_action(document),
            Some(Action {
                command: r#""C:\A&B\repair.cmd""#.to_owned(),
                arguments: String::new(),
            })
        );
        assert_eq!(parse_action("<Task><Actions/></Task>"), None);
        assert_eq!(parse_action("not xml"), None);
    }

    #[test]
    fn the_script_is_beside_the_launcher_in_the_install_root() {
        let install = install();
        assert_eq!(script_path(&install), install.join("repair.cmd"));
        assert_eq!(TASK_NAME, "SummriseRepair");
        assert_eq!(SCRIPT_NAME, "repair.cmd");
    }
}
