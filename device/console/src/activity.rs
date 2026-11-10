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

/// Tool names the registry no longer declares but a device on an older release can still report.
///
/// A name listed here is an alias, not a current tool: its sentence in [`describe`] is kept only
/// because a shipped device can still send it, and the comment beside the name says which release
/// last declared it. The list is empty: every template that named a tool outside the registry
/// (`files.delete`, `system.restart`, …) named a tool no release ever declared, so none was kept.
#[cfg(test)]
const ALIASES: &[&str] = &[];

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
        "terminal.screen" => match number("lines", arguments) {
            Some(lines) if lines > 0 => format!("Read the last {lines} lines of a terminal"),
            _ => "Read the end of a terminal's output".to_owned(),
        },
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
        "runs.get" => "Read one run's record".to_owned(),
        "state.get" => "Read the machine's state".to_owned(),
        "screen.capture" => "Took a screenshot of the screen".to_owned(),
        "system.process_list" => "Listed the machine's processes".to_owned(),
        "system.process_kill" => {
            match quoted("name", arguments).or_else(|| quoted("pid", arguments)) {
                Some(target) => format!("Stopped `{target}`"),
                None => "Stopped a process".to_owned(),
            }
        }
        "system.net_test" => match quoted("host", arguments) {
            Some(host) => format!("Tested the connection to `{host}`"),
            None => "Tested a connection".to_owned(),
        },
        "browser.list" => "Listed the browser's pages".to_owned(),
        "browser.open" => match quoted("url", arguments) {
            Some(url) => format!("Opened `{url}` in the browser"),
            None => "Opened a browser page".to_owned(),
        },
        "browser.navigate" => match quoted("url", arguments) {
            Some(url) => format!("Went to `{url}`"),
            None => "Navigated the browser".to_owned(),
        },
        "browser.eval" => "Ran a script in the browser".to_owned(),
        "browser.screenshot" => "Took a screenshot of a browser page".to_owned(),
        "browser.cast" => "Took a frame of a live browser page".to_owned(),
        "browser.close" => "Closed a browser page".to_owned(),
        "browser.shutdown" => "Stopped the browser".to_owned(),
        // A forward rule is the console's own subject: a panel shows these rows, so the sentence
        // names the RULE, not the mechanism. The reader of an activity line wants to know which
        // rule changed, and `quoted("name", …)` is what carries that when the arguments have it.
        "forward.add" => match quoted("name", arguments) {
            Some(name) => format!("Forwarded a route as `{name}`"),
            None => "Added a forward rule".to_owned(),
        },
        "forward.remove" => match quoted("name", arguments) {
            Some(name) => format!("Removed the forward rule `{name}`"),
            None => "Removed a forward rule".to_owned(),
        },
        "forward.list" => "Listed the forward rules".to_owned(),
        "monitor.add" => "Added a watch".to_owned(),
        "monitor.remove" => "Removed a watch".to_owned(),
        "monitor.list" => "Listed the watched targets".to_owned(),
        "monitor.probe" => "Probed a watched target".to_owned(),
        "monitor.status" => "Read a watch's status".to_owned(),
        "memory.append" => "Wrote a memory entry".to_owned(),
        "memory.update" => match quoted("id", arguments) {
            Some(id) => format!("Rewrote memory entry `{id}`"),
            None => "Rewrote a memory entry".to_owned(),
        },
        "memory.delete" => match quoted("id", arguments) {
            Some(id) => format!("Deleted memory entry `{id}`"),
            None => "Deleted a memory entry".to_owned(),
        },
        "memory.search" => match quoted("query", arguments) {
            Some(query) => format!("Searched its memory for `{query}`"),
            None => "Searched its memory".to_owned(),
        },
        "memory.list" => "Read its memory".to_owned(),
        "memory.export" => "Exported its memory".to_owned(),
        "plugins.list" => "Listed the plugins".to_owned(),
        "plugins.inspect" => match quoted("path", arguments) {
            Some(path) => format!("Checked the plugin folder `{path}`"),
            None => "Checked a plugin folder".to_owned(),
        },
        "plugins.add" => match quoted("path", arguments) {
            Some(path) => format!("Installed the plugin `{path}`"),
            None => "Installed a plugin".to_owned(),
        },
        "plugins.remove" => match quoted("namespace", arguments) {
            Some(namespace) => format!("Removed the plugin `{namespace}`"),
            None => "Removed a plugin".to_owned(),
        },
        "plugins.restart" => match quoted("namespace", arguments) {
            Some(namespace) => format!("Restarted the plugin `{namespace}`"),
            None => "Restarted a plugin".to_owned(),
        },
        "plugins.logs" => match quoted("namespace", arguments) {
            Some(namespace) => format!("Read the log of plugin `{namespace}`"),
            None => "Read a plugin's log".to_owned(),
        },
        "host.facts" => "Read the machine's facts".to_owned(),
        "host.restart" => "Restarted the device's app".to_owned(),
        _ => format!("Used `{tool}`"),
    }
}

/// Whether a raw write carries text a person typed, judged before [`excerpt`] flattens it.
///
/// xterm sends the terminal's own reports through `onData` — `ESC[I` when the tab gains focus,
/// `ESC[O` when it loses it — and the cursor and function keys are escape sequences too. The
/// session must receive them (that is the terminal's job), but they are not keys a person typed. A
/// sequence's final byte is printable (`I`, `O`, `A`), so the sequence has to be removed whole
/// before asking whether the write has text at all; otherwise `ESC[I` reads as the keys `[I`. A
/// space is a keystroke; only control bytes without a printable remainder are not.
fn carries_typed_text(raw: &str) -> bool {
    let mut rest = raw;
    while let Some(character) = rest.chars().next() {
        if character == '\x1b' {
            rest = skip_escape(&rest[1..]);
        } else if character.is_control() {
            rest = &rest[character.len_utf8()..];
        } else {
            return true;
        }
    }
    false
}

/// Skips one escape sequence, given the text after its ESC.
///
/// The grammar is the input half of ANSI — CSI, SS3, OSC, the other string controls, and the
/// two-byte escapes — the same sequences the page's `typedText` removes.
fn skip_escape(rest: &str) -> &str {
    match rest.chars().next() {
        // CSI: parameter bytes 0x30–0x3f, intermediates 0x20–0x2f, then one final byte 0x40–0x7e.
        Some('[') => {
            let body = skip_bytes(&rest[1..], |byte| (0x30..=0x3f).contains(&byte));
            let body = skip_bytes(body, |byte| (0x20..=0x2f).contains(&byte));
            match body.as_bytes().first() {
                Some(&final_byte) if (0x40..=0x7e).contains(&final_byte) => &body[1..],
                // Without its final byte this is not a sequence the page would swallow whole
                // either: only the `ESC[` goes, and the bytes after it are read as text.
                _ => &rest[1..],
            }
        }
        // SS3: one final byte, used by the application cursor and function keys.
        Some('O') => match rest.as_bytes().get(1) {
            Some(&final_byte) if (0x40..=0x7e).contains(&final_byte) => &rest[2..],
            _ => &rest[1..],
        },
        // OSC ends at BEL or ST; the other string controls (DCS, SOS, PM, APC) end at ST.
        Some(']') => skip_osc(&rest[1..]),
        Some('P' | 'X' | '^' | '_') => skip_control_string(&rest[1..]),
        // Any other escape: optional intermediates, then one optional final byte. A character
        // outside both ranges (a multi-byte one, say) stays and is read as text.
        Some(_) => {
            let body = skip_bytes(rest, |byte| (0x20..=0x2f).contains(&byte));
            match body.as_bytes().first() {
                Some(&final_byte) if (0x40..=0x7e).contains(&final_byte) => &body[1..],
                _ => body,
            }
        }
        None => rest,
    }
}

/// Skips the run of ASCII bytes `allowed` accepts; the run always ends on a character boundary.
fn skip_bytes(rest: &str, allowed: impl Fn(u8) -> bool) -> &str {
    let count = rest
        .as_bytes()
        .iter()
        .take_while(|&&byte| allowed(byte))
        .count();
    &rest[count..]
}

/// An OSC payload runs to BEL or ST; an ESC that starts no ST is left for the next sequence.
fn skip_osc(rest: &str) -> &str {
    let end = rest.find(['\x07', '\x1b']).unwrap_or(rest.len());
    let tail = &rest[end..];
    if let Some(after) = tail.strip_prefix('\x07') {
        after
    } else {
        tail.strip_prefix("\x1b\\").unwrap_or(tail)
    }
}

/// A DCS/SOS/PM/APC payload runs to ST; an unterminated one takes the rest of the write.
fn skip_control_string(rest: &str) -> &str {
    match rest.find('\x1b') {
        Some(index) => {
            let tail = &rest[index..];
            tail.strip_prefix("\x1b\\").unwrap_or(tail)
        }
        None => "",
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
        "files.read" | "files.write" | "files.list" | "files.mkdir" => {
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
            // The raw write decides whether there was a keystroke; sanitizing first would turn
            // `ESC[I` into `[I` and record a focus report as typed text. The session stays.
            if let Some(raw) = arguments
                .get("data")
                .and_then(Value::as_str)
                .filter(|raw| carries_typed_text(raw))
            {
                out.insert("input".to_owned(), json!(excerpt(raw)));
            }
        }
        "terminal.read" | "terminal.resize" | "terminal.close" => {
            if let Some(id) = text("id") {
                out.insert("session".to_owned(), json!(id));
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
        // The channel's two boundaries are console facts, not calls; the audit table still draws
        // them, so they get a sentence a person reads rather than the generic operator action.
        "device" => match tool {
            Some(crate::audit::DEVICE_LEFT) => "The channel closed".to_owned(),
            Some(crate::audit::DEVICE_JOINED) => "The channel opened".to_owned(),
            _ => "The channel changed".to_owned(),
        },
        // A failed poll is a row the Activity Table draws; its sentence says who was reading and
        // that the read failed, so a reconnect window does not read as the AI's refusal.
        "poll" => match tool {
            Some(tool) if failed => format!("The console's `{tool}` poll failed"),
            Some(tool) => format!("The console polled `{tool}`"),
            None if failed => "A console poll failed".to_owned(),
            None => "The console polled the machine".to_owned(),
        },
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
        assert_eq!(
            line_sentence(&json!({
                "kind": "poll",
                "tool": "terminal.read",
                "status": 503,
                "detail": "device reconnecting",
            })),
            "The console's `terminal.read` poll failed"
        );
    }

    #[test]
    fn a_channel_boundary_is_a_sentence_not_a_tool_call() {
        // The audit table draws the boundary lines even though the timeline hides them, so their
        // sentence names the channel rather than borrowing the tool template's "Used …".
        assert_eq!(
            line_sentence(&json!({ "kind": "device", "tool": "device.left", "status": 200 })),
            "The channel closed"
        );
        assert_eq!(
            line_sentence(&json!({ "kind": "device", "tool": "device.joined", "status": 200 })),
            "The channel opened"
        );
    }

    /// The tool names `describe` has a sentence for, read from this module's own source.
    ///
    /// The registry walk proves every declared tool gets a sentence (the fallback would show). This
    /// helper tests the other direction, which a `match` cannot enumerate at run time: the block
    /// between `describe` and `detail` holds only the template table, so a lowercase literal with a
    /// dot or a slash there is a tool name.
    fn sentence_names() -> Vec<&'static str> {
        const SOURCE: &str = include_str!("activity.rs");
        let start = SOURCE
            .find("pub(crate) fn describe")
            .expect("describe is in this file");
        let rest = &SOURCE[start..];
        let end = rest
            .find("pub(crate) fn detail")
            .expect("detail follows describe");
        let table = &rest[..end];

        let mut names = Vec::new();
        let mut rest = table;
        while let Some(open) = rest.find('"') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('"') else { break };
            let literal = &after[..close];
            let namespaced = (literal.contains('.') || literal.contains('/'))
                && literal.chars().all(|character| {
                    character.is_ascii_lowercase()
                        || character.is_ascii_digit()
                        || character == '_'
                        || character == '.'
                        || character == '/'
                });
            if namespaced {
                names.push(literal);
            }
            rest = &after[close + 1..];
        }
        names.sort_unstable();
        names.dedup();
        names
    }

    #[test]
    fn every_registry_tool_has_a_sentence() {
        let missing: Vec<&str> = summrise_core::registry::CAPABILITIES
            .iter()
            .flat_map(|capability| capability.tools)
            .filter(|tool| describe(tool.name, &Value::Null) == format!("Used `{}`", tool.name))
            .map(|tool| tool.name)
            .collect();
        assert!(
            missing.is_empty(),
            "these tools are declared by the registry but describe has no sentence for them: {missing:?}"
        );
    }

    #[test]
    fn every_sentence_names_a_declared_tool_or_a_marked_alias() {
        let sentences = sentence_names();
        let extra: Vec<&str> = sentences
            .iter()
            .copied()
            .filter(|name| summrise_core::registry::tool(name).is_none())
            .filter(|name| !ALIASES.contains(name))
            .collect();
        assert!(
            extra.is_empty(),
            "describe has sentences for tools the registry does not declare; delete them or add them to ALIASES with the release that last declared them: {extra:?}"
        );
        for alias in ALIASES {
            assert!(
                summrise_core::registry::tool(alias).is_none(),
                "`{alias}` is an alias but the registry still declares it"
            );
            assert!(
                sentences.contains(alias),
                "`{alias}` is an alias but describe has no sentence for it"
            );
        }
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
    fn a_focus_report_keeps_the_session_but_records_no_input() {
        for data in ["\u{1b}[I", "\u{1b}[O"] {
            let detail = detail(
                "terminal.write",
                &json!({ "id": "session-1", "data": data }),
            )
            .expect("the session is still evidence");
            assert_eq!(detail["session"], json!("session-1"));
            assert!(
                detail.get("input").is_none(),
                "{data:?} is a focus report, not a keystroke: {detail}"
            );
        }
    }

    #[test]
    fn a_space_only_write_still_records_an_input() {
        let detail = detail("terminal.write", &json!({ "id": "session-1", "data": " " }))
            .expect("the session is still evidence");
        assert_eq!(detail["session"], json!("session-1"));
        // `excerpt` trims the write, so the recorded text is empty — the same evidence the console
        // wrote before this change; only control-only writes stop producing an `input`.
        assert_eq!(detail["input"], json!(""));
    }

    #[test]
    fn a_tool_with_nothing_to_show_has_no_evidence() {
        assert!(detail("tools/list", &json!({})).is_none());
    }
}
