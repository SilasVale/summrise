//! The live activity in words: one sentence per event.
//!
//! The audit index records codes — a tool name, a status, an error kind — because it is a file a
//! machine reads. A person watching the console needs language, and the wording has to be the
//! console's, not the page's: two pages that say two different things about one call is how a
//! dashboard starts lying. Every event the live stream carries therefore gets its `sentence` here.
//!
//! Nothing in this module invents an event: it describes the call the console just proxied, from
//! that call's own arguments. When a call names no arguments it says less rather than guessing, and
//! when a tool has no template yet it says `Used \`tool.name\`` instead of pretending to know what
//! the tool did.

use serde_json::{json, Value};

/// The longest piece of caller data quoted into a sentence.
const EXCERPT: usize = 64;

/// A short, single-line excerpt of caller data: no newlines, no control characters, bounded.
fn excerpt(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|character| {
            if character.is_control() || character == '\n' || character == '\r' {
                ' '
            } else {
                character
            }
        })
        .collect();
    let trimmed = flat.trim();
    if trimmed.chars().count() <= EXCERPT {
        return trimmed.to_owned();
    }
    let cut: String = trimmed.chars().take(EXCERPT - 1).collect();
    format!("{cut}…")
}

fn quoted(key: &str, arguments: &Value) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(excerpt)
}

fn number(key: &str, arguments: &Value) -> Option<u64> {
    arguments.get(key).and_then(Value::as_u64)
}

/// Human bytes: `12 B`, `1.2 kB`, `3.4 MB`.
fn bytes(count: u64) -> String {
    if count < 1_000 {
        format!("{count} B")
    } else if count < 1_000_000 {
        format!("{:.1} kB", count as f64 / 1_000.0)
    } else {
        format!("{:.1} MB", count as f64 / 1_000_000.0)
    }
}

/// A human sentence for one live call.
pub(crate) fn describe(tool: &str, arguments: &Value) -> String {
    match tool {
        "files.read" => match quoted("path", arguments) {
            Some(path) => format!("Read `{path}`"),
            None => "Read a file".to_owned(),
        },
        "files.write" => {
            let path = quoted("path", arguments).unwrap_or_else(|| "a file".to_owned());
            match arguments.get("content").and_then(Value::as_str) {
                Some(content) => format!("Wrote `{path}` ({})", bytes(content.len() as u64)),
                None => format!("Wrote `{path}`"),
            }
        }
        "files.list" => match quoted("path", arguments) {
            Some(path) => format!("Listed `{path}`"),
            None => "Listed a folder".to_owned(),
        },
        "files.mkdir" => match quoted("path", arguments) {
            Some(path) => format!("Created the folder `{path}`"),
            None => "Created a folder".to_owned(),
        },
        "files.delete" => match quoted("path", arguments) {
            Some(path) => format!("Deleted `{path}`"),
            None => "Deleted a file".to_owned(),
        },
        "files.move" | "files.rename" => match (quoted("from", arguments), quoted("to", arguments))
        {
            (Some(from), Some(to)) => format!("Moved `{from}` to `{to}`"),
            _ => "Moved a file".to_owned(),
        },
        "files.shared.list" => "Listed the shared folder".to_owned(),
        "terminal.create" => match quoted("shell", arguments) {
            Some(shell) => format!("Opened a terminal with `{shell}`"),
            None => "Opened a terminal session".to_owned(),
        },
        "terminal.write" => match quoted("data", arguments) {
            Some(data) => format!("Typed `{data}` into a terminal"),
            None => "Typed into a terminal".to_owned(),
        },
        "terminal.read" => match number("wait_ms", arguments) {
            Some(wait) if wait > 0 => "Waited for a terminal's output".to_owned(),
            _ => "Read a terminal's output".to_owned(),
        },
        "terminal.resize" => "Resized a terminal".to_owned(),
        "terminal.close" => "Closed a terminal session".to_owned(),
        "terminal.list" => "Listed the terminal sessions".to_owned(),
        "runs.exec" => match quoted("command", arguments) {
            Some(command) => match arguments.get("args").and_then(Value::as_array) {
                Some(args) if !args.is_empty() => {
                    let joined: Vec<String> =
                        args.iter().filter_map(Value::as_str).map(excerpt).collect();
                    format!("Ran `{command} {}`", joined.join(" "))
                }
                _ => format!("Ran `{command}`"),
            },
            None => "Ran a command".to_owned(),
        },
        "runs.list" => "Read the run history".to_owned(),
        "state.get" => "Read the machine's state".to_owned(),
        "screen.capture" => "Took a screenshot of the screen".to_owned(),
        "screen.capture_display" => "Took a screenshot of one display".to_owned(),
        "system.process_list" => "Listed the machine's processes".to_owned(),
        "system.process_kill" => {
            match quoted("name", arguments).or_else(|| quoted("pid", arguments)) {
                Some(target) => format!("Stopped `{target}`"),
                None => "Stopped a process".to_owned(),
            }
        }
        "system.restart" => "Restarted the machine".to_owned(),
        "system.net_test" => match quoted("host", arguments) {
            Some(host) => format!("Tested the connection to `{host}`"),
            None => "Tested a connection".to_owned(),
        },
        "browser.open" => match quoted("url", arguments) {
            Some(url) => format!("Opened `{url}` in the browser"),
            None => "Opened a browser page".to_owned(),
        },
        "browser.navigate" => match quoted("url", arguments) {
            Some(url) => format!("Went to `{url}`"),
            None => "Navigated the browser".to_owned(),
        },
        "browser.capture" => "Read the browser's page".to_owned(),
        "browser.eval" => "Ran a script in the browser".to_owned(),
        "browser.close" => "Closed a browser page".to_owned(),
        "browser.shutdown" => "Stopped the browser".to_owned(),
        "monitor.add" => "Added a watch".to_owned(),
        "monitor.probe" => "Probed a watched target".to_owned(),
        "monitor.remove" => "Removed a watch".to_owned(),
        "memory.search" => match quoted("query", arguments) {
            Some(query) => format!("Searched its memory for `{query}`"),
            None => "Searched its memory".to_owned(),
        },
        "memory.append" => "Wrote a memory entry".to_owned(),
        "memory.list" => "Read its memory".to_owned(),
        "plugins.list" => "Listed the plugins".to_owned(),
        "host.facts" => "Read the machine's facts".to_owned(),
        "host.report" => "Reported the machine's facts".to_owned(),
        "mesh.status" => "Read the mesh status".to_owned(),
        "tools/list" => "Listed the machine's tools".to_owned(),
        _ => format!("Used `{tool}`"),
    }
}

/// The evidence for one call: the few facts the page can show when a row is opened.
///
/// Only what the caller themselves sent, bounded and flattened — never the device's whole answer.
/// A tool with nothing worth showing returns `None` rather than an empty object, so the page can say
/// "this row has no evidence" instead of drawing an empty panel.
pub(crate) fn detail(tool: &str, arguments: &Value) -> Option<Value> {
    let text = |key: &str| {
        arguments
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(excerpt)
    };
    let mut out = serde_json::Map::new();
    match tool {
        "runs.exec" => {
            if let Some(command) = text("command") {
                out.insert("command".to_owned(), json!(command));
            }
            let args: Vec<String> = arguments
                .get("args")
                .and_then(Value::as_array)
                .map(|list| list.iter().filter_map(Value::as_str).map(excerpt).collect())
                .unwrap_or_default();
            if !args.is_empty() {
                out.insert("args".to_owned(), json!(args));
            }
            if let Some(timeout) = arguments.get("timeout_ms").and_then(Value::as_u64) {
                out.insert("timeout_ms".to_owned(), json!(timeout));
            }
        }
        "files.read" | "files.write" | "files.list" | "files.mkdir" | "files.delete" => {
            if let Some(path) = text("path") {
                out.insert("path".to_owned(), json!(path));
            }
            if let Some(content) = arguments.get("content").and_then(Value::as_str) {
                out.insert("bytes".to_owned(), json!(content.len()));
            }
        }
        "terminal.create" => {
            if let Some(shell) = text("shell") {
                out.insert("shell".to_owned(), json!(shell));
            }
        }
        "terminal.write" => {
            if let Some(id) = text("id") {
                out.insert("session".to_owned(), json!(id));
            }
            if let Some(input) = text("data") {
                out.insert("input".to_owned(), json!(input));
            }
        }
        "terminal.read" | "terminal.resize" | "terminal.close" => {
            if let Some(id) = text("id") {
                out.insert("session".to_owned(), json!(id));
            }
        }
        "screen.capture" | "screen.capture_display" => {
            if let Some(display) = arguments.get("display").and_then(Value::as_u64) {
                out.insert("display".to_owned(), json!(display));
            }
        }
        "browser.open" | "browser.navigate" => {
            if let Some(url) = text("url") {
                out.insert("url".to_owned(), json!(url));
            }
        }
        "system.net_test" => {
            if let Some(host) = text("host") {
                out.insert("host".to_owned(), json!(host));
            }
        }
        "memory.search" => {
            if let Some(query) = text("query") {
                out.insert("query".to_owned(), json!(query));
            }
        }
        _ => {}
    }
    if out.is_empty() {
        None
    } else {
        Some(Value::Object(out))
    }
}

/// A human sentence for one stored audit line, when the event's arguments are no longer available.
///
/// The snapshot that opens the live stream comes from the audit file, which never carried bodies, so
/// the sentence is the generic one for that kind. A live event always uses [`describe`] with the
/// call's own arguments instead of this.
pub(crate) fn line_sentence(line: &Value) -> String {
    let kind = line.get("kind").and_then(Value::as_str).unwrap_or("event");
    let tool = line.get("tool").and_then(Value::as_str);
    let status = line.get("status").and_then(Value::as_u64).unwrap_or(0);
    let failed = status >= 400;
    match kind {
        "mcp" => match tool {
            // A stored line has no arguments, so it gets the template's own generic sentence — never
            // the raw tool name, which is the index's language and not a person's.
            Some(tool) if failed => format!("`{tool}` failed"),
            Some(tool) => describe(tool, &Value::Null),
            None if failed => "A tools request failed".to_owned(),
            None => "Listed the machine's tools".to_owned(),
        },
        "grant" => "Issued a grant".to_owned(),
        "token" => "Issued a client token".to_owned(),
        "token_revoke" => "Revoked a client token".to_owned(),
        "register" => "A machine registered".to_owned(),
        "enroll" => "Created an enrollment code".to_owned(),
        "denied" => match tool {
            Some(tool) => format!("Refused `{tool}`"),
            None => "Refused a request".to_owned(),
        },
        _ => "An operator action".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_read_names_the_file() {
        let sentence = describe("files.read", &json!({ "path": "notes/todo.md" }));
        assert_eq!(sentence, "Read `notes/todo.md`");
    }

    #[test]
    fn a_run_names_the_command_and_its_arguments() {
        let sentence = describe(
            "runs.exec",
            &json!({ "command": "cargo", "args": ["test", "-p", "console"] }),
        );
        assert_eq!(sentence, "Ran `cargo test -p console`");
    }

    #[test]
    fn a_write_says_how_much() {
        let sentence = describe(
            "files.write",
            &json!({ "path": "a.txt", "content": "hello" }),
        );
        assert_eq!(sentence, "Wrote `a.txt` (5 B)");
    }

    #[test]
    fn caller_data_is_flattened_and_bounded() {
        let sentence = describe(
            "terminal.write",
            &json!({ "data": "echo hello\r\necho there\r\necho and-a-very-long-line-that-goes-on-and-on-and-on-past-the-limit" }),
        );
        assert!(!sentence.contains('\n'), "{sentence}");
        assert!(!sentence.contains('\r'), "{sentence}");
        assert!(
            sentence.ends_with("…` into a terminal") || sentence.contains("…"),
            "{sentence}"
        );
    }

    #[test]
    fn a_call_without_arguments_says_less_rather_than_guessing() {
        assert_eq!(describe("files.read", &json!({})), "Read a file");
        assert_eq!(
            describe("runs.exec", &json!({ "command": "hostname" })),
            "Ran `hostname`"
        );
    }

    #[test]
    fn an_unknown_tool_is_named_not_invented() {
        assert_eq!(
            describe("plugins.install", &json!({})),
            "Used `plugins.install`"
        );
    }

    #[test]
    fn a_stored_line_gets_the_sentence_for_its_tool_not_a_raw_name() {
        assert_eq!(
            line_sentence(&json!({ "kind": "mcp", "tool": "runs.exec", "status": 200 })),
            "Ran a command"
        );
        assert_eq!(
            line_sentence(&json!({ "kind": "mcp", "tool": "host.facts", "status": 200 })),
            "Read the machine's facts"
        );
    }

    #[test]
    fn a_stored_line_gets_the_generic_sentence_for_its_kind() {
        assert_eq!(
            line_sentence(&json!({ "kind": "mcp", "tool": "terminal.list", "status": 200 })),
            "Listed the terminal sessions"
        );
        assert_eq!(
            line_sentence(&json!({ "kind": "mcp", "tool": "runs.exec", "status": 500 })),
            "`runs.exec` failed"
        );
        assert_eq!(
            line_sentence(&json!({ "kind": "grant", "status": 200 })),
            "Issued a grant"
        );
    }
}

#[cfg(test)]
mod detail_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_run_carries_its_command_and_arguments() {
        let detail = detail(
            "runs.exec",
            &json!({ "command": "cargo", "args": ["test", "-p", "console"] }),
        )
        .expect("a run has evidence");
        assert_eq!(detail["command"], json!("cargo"));
        assert_eq!(detail["args"], json!(["test", "-p", "console"]));
    }

    #[test]
    fn a_file_carries_its_path_and_size() {
        let detail = detail(
            "files.write",
            &json!({ "path": "a.txt", "content": "hello" }),
        )
        .unwrap();
        assert_eq!(detail["path"], json!("a.txt"));
        assert_eq!(detail["bytes"], json!(5));
    }

    #[test]
    fn a_terminal_write_carries_the_session_and_the_input() {
        let detail = detail(
            "terminal.write",
            &json!({ "id": "session-2", "data": "npm test" }),
        )
        .unwrap();
        assert_eq!(detail["session"], json!("session-2"));
        assert_eq!(detail["input"], json!("npm test"));
    }

    #[test]
    fn a_tool_with_nothing_to_show_has_no_evidence() {
        assert!(detail("tools/list", &json!({})).is_none());
    }
}
