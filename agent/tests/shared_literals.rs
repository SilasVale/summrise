//! THE LITERALS THAT EXIST IN TWO FILES AND ARE CHECKED NOWHERE.
//!
//! **THE CRITERION.** `agent/tests/js_boundary_inventory.rs` answers "may I write this in JS"; this gate
//! answers a narrower question that keeps arriving in the same shape: **a constant TWO artifacts must
//! agree about, where each holds its own copy and nothing compares them.** The failure mode is always the
//! same and it is always silent — both sides compile, both suites pass, and the disagreement shows up on
//! a device as a taskbar icon, a shell that never restarts, or a `config.yaml` line read two ways.
//!
//! THREE ROWS TODAY, all found while porting the Electron shell's decisions to Rust (landing 6b):
//!
//! | literal | where | why it must agree |
//! |---|---|---|
//! | `online.saisi.summrise.desktop` | `agent/summrise-shell-policy/src/boot.rs` · `agent/summrise-agent-npm/src/summrise.ts` | The `AppUserModelID` the shell sets on its window AND the one the CLI writes onto the Start Menu shortcut. Windows resolves the taskbar icon through BOTH, and the shell's own comment says "KEEP THE TWO IN STEP" while the literal is a copy — the shell is emitted to plain JS in two packages and cannot import the CLI's module |
//! | `SummriseDesktop` / `SummriseAgent` | the same two files | The shell's auto-launch toggle and its self-heal watchdog run `schtasks /run <name>`; the CLI REGISTERS those tasks. A renamed task makes the watchdog start nothing, which is a device that stays dark with a log line saying `ok` |
//! | JavaScript's `\s` | `agent/summrise-url-policy/src/js.rs` · `agent/summrise-shell-policy/src/js.rs` | The two crates deliberately do NOT depend on each other — a crate dependency would put a SECOND copy of url-policy's `thread_local!` port state into the shell's wasm module (see that crate's `Cargo.toml`) — so the character class is duplicated. If the two drift, one `config.yaml` line is read two ways |
//!
//! MUTATION: in `agent/summrise-shell-policy/src/boot.rs`, `pub const DESKTOP_AUMID: &str =
//!           "online.saisi.summrise.desktop";` -> `"online.saisi.summrise.desktopX";`
//! RESULT:   exit 101 —
//!             the literals below are held by two artifacts and do not agree:
//!               DESKTOP_AUMID: agent/summrise-shell-policy/src/boot.rs holds [online.saisi.summrise.desktopX],
//!               agent/summrise-agent-npm/src/summrise.ts holds [online.saisi.summrise.desktop]
//!             ...and the FIX line.
//!
//! MUTATION: in `agent/summrise-shell-policy/src/js.rs`, drop `\x0B` from the `JS_WS` class.
//! RESULT:   exit 101 — the same message, naming `JS_WS` and the two `src/js.rs` paths.
//!
//! AND EACH ROW'S OWN SUBJECT IS ASSERTED BEFORE THE COMPARISON: a row whose declaration has been renamed
//! or deleted is a check that has stopped checking, which is the class `all-gates.bash` counts as a
//! FAILURE rather than an exemption.

mod common;

/// Where a value is read from. It is an ENUM rather than a pattern so that a row naming the wrong member
/// of a list is a compile-time impossibility rather than a silent mis-comparison.
enum Source {
    /// The first quoted string after this declaration.
    Const(&'static str),
    /// The n-th element of the bracketed list literal after this declaration.
    ListElement(&'static str, usize),
}

/// One literal two files hold a copy of.
struct Agreement {
    /// The name the failure message uses.
    name: &'static str,
    /// The files that hold a copy, and where in each the value is read from.
    holders: &'static [(&'static str, Source)],
}

/// The rows. A value that appears in a COMMENT or a log message cannot satisfy the check, because the
/// DECLARATION has to be found first — the same distinction `js_boundary_inventory.rs` draws between a
/// rule and a mention of it.
const ROWS: &[Agreement] = &[
    Agreement {
        name: "DESKTOP_AUMID",
        holders: &[
            (
                "agent/summrise-shell-policy/src/boot.rs",
                Source::Const("pub const DESKTOP_AUMID: &str ="),
            ),
            (
                "agent/summrise-agent-npm/src/summrise.ts",
                Source::Const("export const DESKTOP_AUMID ="),
            ),
        ],
    },
    Agreement {
        name: "SummriseDesktop (the auto-launch task)",
        holders: &[
            (
                "agent/summrise-shell-policy/src/lifecycle.rs",
                Source::Const("pub const AUTOSTART_TASK: &str ="),
            ),
            (
                "agent/summrise-agent-npm/src/summrise.ts",
                Source::ListElement("BOOT_TASKS = [", 1),
            ),
        ],
    },
    Agreement {
        name: "SummriseAgent (the agent task)",
        holders: &[
            (
                "agent/summrise-shell-policy/src/lifecycle.rs",
                Source::Const("pub const AGENT_TASK: &str ="),
            ),
            (
                "agent/summrise-agent-npm/src/summrise.ts",
                Source::ListElement("BOOT_TASKS = [", 0),
            ),
        ],
    },
    Agreement {
        name: "JS_WS (JavaScript's \\s as a regex class)",
        holders: &[
            (
                "agent/summrise-url-policy/src/js.rs",
                Source::Const("pub const JS_WS: &str ="),
            ),
            (
                "agent/summrise-shell-policy/src/js.rs",
                Source::Const("pub const JS_WS: &str ="),
            ),
        ],
    },
];

/// The first quoted string after `declaration`.
///
/// It is deliberately NOT a search over the whole file: the declaration must be FOUND first, so a value
/// that moved into a comment, a log message or a different constant is a failure rather than a match.
fn declared_string(text: &str, declaration: &str) -> Option<String> {
    let at = text.find(declaration)?;
    let rest = &text[at + declaration.len()..];
    let quote_at = rest.find(['"', '\''])?;
    let quote = rest.as_bytes()[quote_at] as char;
    let body = &rest[quote_at + 1..];
    let end = body.find(quote)?;
    Some(body[..end].to_string())
}

/// The `index`-th element of the bracketed list literal after `declaration` —
/// `BOOT_TASKS = ["SummriseAgent", "SummriseDesktop"]` answers `SummriseAgent` at 0.
fn list_element(text: &str, declaration: &str, index: usize) -> Option<String> {
    let at = text.find(declaration)?;
    let open = text[at..].find('[')? + at;
    let close = text[open..].find(']')? + open;
    let items: Vec<String> = text[open + 1..close]
        .split(',')
        .filter_map(|piece| {
            let trimmed = piece.trim();
            let quote = trimmed.chars().next().filter(|c| *c == '"' || *c == '\'')?;
            let body = &trimmed[quote.len_utf8()..];
            let end = body.find(quote)?;
            Some(body[..end].to_string())
        })
        .collect();
    items.get(index).cloned()
}

/// Read one holder's value, or `None` where its declaration has moved.
fn holder_value(path: &str, source: &Source) -> Option<String> {
    let text = common::read(path);
    match source {
        Source::Const(declaration) => declared_string(&text, declaration),
        Source::ListElement(declaration, index) => list_element(&text, declaration, *index),
    }
}

/// The declaration a row names, for the failure message.
fn source_label(source: &Source) -> String {
    match source {
        Source::Const(declaration) => (*declaration).to_string(),
        Source::ListElement(declaration, index) => format!("{declaration}][{index}"),
    }
}

#[test]
fn the_literals_held_by_two_artifacts_agree() {
    let mut disagreements: Vec<String> = Vec::new();

    for row in ROWS {
        let mut found: Vec<(&str, String)> = Vec::new();
        for (path, source) in row.holders {
            let value = holder_value(path, source).unwrap_or_else(|| {
                panic!(
                    "{}: nothing readable follows `{}` in {path} — a row whose declaration moved is a \
                     check that has stopped checking. FIX: point the row at the declaration that exists.",
                    row.name,
                    source_label(source)
                )
            });
            found.push((path, value));
        }
        let first = found[0].1.clone();
        for (path, value) in &found[1..] {
            if *value != first {
                disagreements.push(format!(
                    "{}: {} holds [{first}], {path} holds [{value}]",
                    row.name, found[0].0
                ));
            }
        }
    }

    assert!(
        disagreements.is_empty(),
        "the literals below are held by two artifacts and do not agree:\n  {}\n\
         FIX: change ONE side and the other in the same commit, or make one of them the owner and have \
         the other read it. A literal two packages must agree about, copied into both and compared \
         nowhere, is a device-only failure that no suite here can see.",
        disagreements.join("\n  ")
    );

    // A floor rather than a formality: an emptied ROWS (or a truncated holders list) would make the loop
    // above pass while checking nothing.
    let holders: usize = ROWS.iter().map(|row| row.holders.len()).sum();
    assert!(
        ROWS.len() >= 4 && holders >= 8,
        "this gate derived {} row(s) over {holders} holder(s) — the floor of 4 rows and 8 holders exists \
         so that emptying the list cannot look like a pass",
        ROWS.len()
    );
}

/// The task names the shell runs BY NAME must be the ones the CLI's `BOOT_TASKS` registers — checked by
/// membership rather than by position, so a task renamed on one side is caught with the string it lost.
#[test]
fn the_scheduled_task_names_are_registered_by_the_cli() {
    let cli = common::read("agent/summrise-agent-npm/src/summrise.ts");
    let shell = common::read("agent/summrise-shell-policy/src/lifecycle.rs");

    let first = list_element(&cli, "BOOT_TASKS = [", 0).expect("the CLI declares BOOT_TASKS");
    let second = list_element(&cli, "BOOT_TASKS = [", 1).expect("BOOT_TASKS has two members");
    assert!(
        !first.is_empty() && !second.is_empty(),
        "the CLI's BOOT_TASKS no longer parses — this gate cannot see what it is checking"
    );

    for task in ["SummriseAgent", "SummriseDesktop"] {
        assert!(
            cli.contains(&format!("\"{task}\"")),
            "the CLI's BOOT_TASKS no longer names {task}"
        );
        assert!(
            shell.contains(&format!("\"{task}\"")),
            "the shell policy no longer knows the task {task}, which the CLI registers and \
             `schtasks /run` needs BY NAME — a renamed task makes the watchdog start nothing and log ok"
        );
    }
}
