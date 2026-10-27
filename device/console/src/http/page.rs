//! The Rust page's first slice: the sign-in card and the fleet shell. The React page at `/` is
//! untouched; this module is where it moves to, one surface at a time.

use maud::{html, Markup, DOCTYPE};

use super::DeviceView;

/// The page's whole style: the console's tokens, a centered card, a table. No script.
const STYLE: &str = r#"
:root { color-scheme: light dark; --bg:#f6f5f2; --panel:#fff; --ink:#1d1c1a; --muted:#6c6a65; --line:#dedbd4; --accent:#b3400f; }
@media (prefers-color-scheme: dark) { :root { --bg:#141311; --panel:#1d1b18; --ink:#f2efe9; --muted:#a6a29a; --line:#37332d; --accent:#e07a4a; } }
* { box-sizing: border-box; }
body { margin:0; min-height:100vh; background:var(--bg); color:var(--ink); font:15px/1.5 system-ui, sans-serif; }
body.login { display:grid; place-items:center; padding:24px; }
.card { width:min(100%,380px); background:var(--panel); border:1px solid var(--line); border-radius:12px; padding:28px; }
h1 { margin:0 0 6px; font-size:20px; letter-spacing:-0.01em; }
h1 span { color:var(--muted); font-weight:400; }
.hint { color:var(--muted); font-size:13px; margin:0 0 20px; }
label { display:block; font-size:13px; margin:0 0 6px; }
input { width:100%; padding:9px 10px; border:1px solid var(--line); border-radius:7px; background:var(--bg); color:var(--ink); font:inherit; }
input:focus { outline:2px solid var(--accent); outline-offset:1px; }
button { margin-top:14px; width:100%; padding:9px 12px; border:0; border-radius:7px; background:var(--accent); color:#fff; font:inherit; cursor:pointer; }
button:hover { filter:brightness(1.08); }
.error { color:var(--accent); font-size:13px; margin:12px 0 0; }
header { display:flex; align-items:center; gap:12px; padding:14px 20px; border-bottom:1px solid var(--line); }
header form { margin:0 0 0 auto; width:auto; }
header button { width:auto; margin:0; background:transparent; color:var(--muted); border:1px solid var(--line); }
main { max-width:920px; margin:0 auto; padding:28px 20px 48px; }
h2 { font-size:15px; text-transform:uppercase; letter-spacing:0.08em; color:var(--muted); }
table { width:100%; border-collapse:collapse; background:var(--panel); border:1px solid var(--line); border-radius:10px; overflow:hidden; }
th, td { text-align:left; padding:11px 14px; border-bottom:1px solid var(--line); vertical-align:top; }
tr:last-child td { border-bottom:0; }
th { font-size:12px; text-transform:uppercase; letter-spacing:0.06em; color:var(--muted); }
code { color:var(--muted); font-size:12px; }
.ok { color:#1f8f4e; }
.off { color:var(--muted); }
.version { color:var(--muted); font-size:12px; }
nav { display:flex; gap:14px; margin-top:18px; font-size:13px; }
nav a { color:var(--accent); text-decoration:none; }
.format-form { display:flex; gap:6px; margin-top:6px; }
.row-form input { padding:4px 6px; font-size:12px; width:120px; }
.row-form button, .inline-button { margin:0; width:auto; padding:4px 8px; font-size:12px; }
.inline-button { background:transparent; color:var(--muted); border:1px solid var(--line); }
.code { background:var(--panel); border:1px solid var(--line); border-radius:8px; padding:10px 14px; font-size:13px; }
.code strong { font-family:ui-monospace, monospace; letter-spacing:0.04em; }
textarea { width:100%; font:13px/1.4 ui-monospace, monospace; padding:8px 10px; border:1px solid var(--line); border-radius:7px; background:var(--bg); color:var(--ink); }
pre.file { background:var(--panel); border:1px solid var(--line); border-radius:8px; padding:12px 14px; overflow:auto; font:13px/1.5 ui-monospace, monospace; white-space:pre-wrap; }
h3 { font-size:13px; text-transform:uppercase; letter-spacing:0.06em; color:var(--muted); margin:22px 0 8px; }
"#;

/// The terminal page's own few rules: the xterm host must have a height, and the page is a column.
const TERMINAL_STYLE: &str = r#"
body.terminal { display:flex; flex-direction:column; height:100vh; overflow:hidden; }
body.terminal main { flex:1; display:flex; flex-direction:column; min-height:0; }
#terminal { flex:1; min-height:0; background:#000; border-radius:8px; padding:8px; }
#terminal-status { margin:6px 2px 0; }
header a { margin-left:auto; color:var(--accent); text-decoration:none; font-size:13px; }
"#;

/// The sign-in card. `error` is the one line a refused token gets.
pub(super) fn login(error: Option<&str>) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Summrise console" }
                style { (STYLE) }
            }
            body class="login" {
                main class="card" {
                    h1 { "Summrise " span { "console" } }
                    p class="hint" {
                        "The operator token opens a session for this browser. It is not stored by the page."
                    }
                    form method="post" action="/next/auth" {
                        label for="token" { "Operator token" }
                        input id="token" name="token" type="password" autocomplete="off" required;
                        button type="submit" { "Sign in" }
                    }
                    @if let Some(error) = error {
                        p class="error" { (error) }
                    }
                }
            }
        }
    }
}

/// The signed-in page: the fleet table, the phase-2 mutations, and the sign-out form carrying the
/// session's CSRF value.
pub(super) fn dashboard(
    devices: &[DeviceView],
    csrf: &str,
    version: &str,
    code: Option<&str>,
    error: Option<&str>,
) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Summrise console" }
                style { (STYLE) }
            }
            body {
                header {
                    h1 { "Summrise " span { "console" } }
                    span class="version" { "console " (version) }
                    form method="post" action="/next/logout" {
                        input type="hidden" name="csrf" value=(csrf);
                        button type="submit" { "Sign out" }
                    }
                }
                main {
                    @if let Some(code) = code {
                        p class="code" {
                            "Enrollment code: " strong { (code) } " — single use, valid for 15 minutes."
                        }
                    }
                    @if let Some(error) = error {
                        p class="error" { "That change was not applied: " (error) }
                    }
                    h2 { "Devices" }
                    form method="post" action="/next/enroll" {
                        input type="hidden" name="csrf" value=(csrf);
                        button type="submit" class="inline-button" { "New enrollment code" }
                    }
                    @if devices.is_empty() {
                        p class="hint" { "No devices have registered yet." }
                    } @else {
                        table {
                            thead { tr { th { "Device" } th { "State" } th { "Version" } th { "" } } }
                            tbody {
                                @for device in devices {
                                    tr {
                                        td {
                                            strong { (device.name.as_deref().unwrap_or(&device.id)) }
                                            br;
                                            code { (&device.id) }
                                            form method="post" action=(format!("/next/fleet/{}/name", device.id)) class="row-form" {
                                                input type="hidden" name="csrf" value=(csrf);
                                                input type="text" name="name" maxlength="64" placeholder="rename";
                                                button type="submit" { "Rename" }
                                            }
                                            form method="post" action=(format!("/next/fleet/{}/remove", device.id)) class="row-form" {
                                                input type="hidden" name="csrf" value=(csrf);
                                                button type="submit" class="inline-button" { "Remove" }
                                            }
                                        }
                                        td {
                                            @if device.online { span class="ok" { "online" } } @else { span class="off" { "offline" } }
                                        }
                                        td { (device.version.as_deref().unwrap_or("—")) }
                                        td {
                                            a href=(format!("/next/terminal/{}", device.id)) { "Terminal" }
                                            " · "
                                            a href=(format!("/next/files/{}", device.id)) { "Files" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    nav {
                        a href="/next/audit" { "Audit" }
                        a href="/next/denials" { "Denials" }
                    }
                }
            }
        }
    }
}

/// A read-only log page: the audit or denial lines, newest first, bounded by the caller.
pub(super) fn logs(rows: &[serde_json::Value], title: &str, version: &str) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Summrise console — " (title) }
                style { (STYLE) }
            }
            body {
                header {
                    h1 { "Summrise " span { "console" } }
                    span class="version" { "console " (version) }
                    a href="/next" { "Back to the fleet" }
                }
                main {
                    h2 { (title) }
                    @if rows.is_empty() {
                        p class="hint" { "Nothing has been recorded yet." }
                    } @else {
                        table {
                            thead { tr { th { "When (ms)" } th { "Kind" } th { "Status" } th { "Device" } th { "Tool" } th { "Detail" } } }
                            tbody {
                                @for row in rows {
                                    tr {
                                        td { (row["ts_ms"].as_u64().unwrap_or(0)) }
                                        td { (row["kind"].as_str().unwrap_or("—")) }
                                        td { (row["status"].as_u64().unwrap_or(0)) }
                                        td { (row["device_id"].as_str().unwrap_or("—")) }
                                        td { (row["tool"].as_str().unwrap_or("—")) }
                                        td { (row["detail"].as_str().unwrap_or("")) }
                                    }
                                }
                            }
                        }
                    }
                    nav { a href="/next" { "Fleet" } }
                }
            }
        }
    }
}

/// The terminal page: one xterm host, wired to the console's `/next/terminal` endpoints.
///
/// The device id and this session's CSRF value ride in data attributes, not in the URL or a cookie
/// the script reads: the session cookie is `HttpOnly`, so the page cannot leak what it cannot see.
pub(super) fn terminal(device: &DeviceView, csrf: &str, version: &str) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Summrise console — terminal" }
                link rel="stylesheet" href="/assets/xterm.css";
                style { (STYLE) (TERMINAL_STYLE) }
                script src="/assets/xterm.js" defer {}
                script src="/assets/addon-fit.js" defer {}
                script src="/assets/terminal.js" defer {}
            }
            body class="terminal" {
                header {
                    h1 { "Summrise " span { "console" } }
                    span class="version" { "console " (version) }
                    span class="version" { (device.name.as_deref().unwrap_or(&device.id)) }
                    a href="/next" { "Back to the fleet" }
                }
                main {
                    div id="terminal" data-device=(device.id) data-csrf=(csrf) {}
                    p id="terminal-status" class="hint" { "connecting…" }
                }
            }
        }
    }
}

/// One entry the files page draws: the name, what it is, and its length.
pub(super) struct FileEntry {
    pub(super) name: String,
    pub(super) kind: String,
    pub(super) len: u64,
}

/// Percent-encode a query value. `/` stays readable because a path is the value most of the time.
pub(super) fn encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The device's files, server-rendered: the list, a read form and a write form.
pub(super) fn files(
    device: &DeviceView,
    path: &str,
    entries: &[FileEntry],
    csrf: &str,
    version: &str,
    error: Option<&str>,
) -> Markup {
    let base = format!("/next/files/{}", device.id);
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Summrise console — files" }
                style { (STYLE) }
            }
            body {
                header {
                    h1 { "Summrise " span { "console" } }
                    span class="version" { "console " (version) }
                    span class="version" { (device.name.as_deref().unwrap_or(&device.id)) }
                    a href="/next" { "Back to the fleet" }
                }
                main {
                    h2 { "Files" }
                    p class="hint" {
                        "The device's files root. Paths are relative to it; the device enforces the root."
                    }
                    @if let Some(error) = error {
                        p class="error" { (error) }
                    }
                    p {
                        "at /" (path)
                        @if !path.is_empty() {
                            " · "
                            a href=(format!("{}?path={}", base, encode(parent(path)))) { "up" }
                        }
                    }
                    @if entries.is_empty() {
                        p class="hint" { "This directory is empty." }
                    } @else {
                        table {
                            thead { tr { th { "Name" } th { "Kind" } th { "Bytes" } } }
                            tbody {
                                @for entry in entries {
                                    tr {
                                        td {
                                            @if entry.kind == "directory" {
                                                a href=(format!("{}?path={}", base, encode(&join(path, &entry.name)))) { (entry.name) }
                                            } @else {
                                                a href=(format!("{}/read?path={}", base, encode(&join(path, &entry.name)))) { (entry.name) }
                                            }
                                        }
                                        td { (entry.kind) }
                                        td { (entry.len) }
                                    }
                                }
                            }
                        }
                    }
                    h3 { "Read a file" }
                    form method="get" action=(format!("{}/read", base)) class="format-form" {
                        input type="text" name="path" placeholder="notes.txt" required;
                        button type="submit" { "Read" }
                    }
                    h3 { "Write a file" }
                    form method="post" action=(format!("{}/write", base)) {
                        input type="hidden" name="csrf" value=(csrf);
                        input type="text" name="path" placeholder="notes.txt" required;
                        textarea name="content" rows="8" placeholder="text, written as UTF-8";
                        button type="submit" { "Write" }
                    }
                }
            }
        }
    }
}

/// One file, as text.
pub(super) fn file_text(device: &DeviceView, path: &str, text: &str, version: &str) -> Markup {
    let base = format!("/next/files/{}", device.id);
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Summrise console — " (path) }
                style { (STYLE) }
            }
            body {
                header {
                    h1 { "Summrise " span { "console" } }
                    span class="version" { "console " (version) }
                    span class="version" { (device.name.as_deref().unwrap_or(&device.id)) }
                    a href=(format!("{}?path={}", base, encode(parent(path)))) { "Back to the directory" }
                }
                main {
                    h2 { (path) }
                    pre class="file" { (text) }
                }
            }
        }
    }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map(|(head, _)| head).unwrap_or("")
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_owned()
    } else {
        format!("{path}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_name_is_escaped() {
        let device = DeviceView {
            id: "00".repeat(32),
            public_key: "00".repeat(32),
            name: Some("<script>alert(1)</script>".to_owned()),
            registered_at_ms: 0,
            last_seen_ms: 0,
            online: false,
            version: None,
            host: None,
            last_attempt: None,
        };
        let page = dashboard(&[device], "00", "0.3.48", None, None).into_string();
        assert!(
            page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "{page}"
        );
        assert!(!page.contains("<script>alert(1)</script>"), "{page}");
    }
}
