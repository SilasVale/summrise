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
                        "The Rust page. The operator token opens a cookie session; nothing leaves this machine."
                    }
                    form method="post" action="/next/auth" {
                        label for="token" { "Operator token" }
                        input id="token" name="token" type="password" autocomplete="current-password" required;
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

/// The signed-in page: the fleet table and the sign-out form carrying the session's CSRF value.
pub(super) fn dashboard(devices: &[DeviceView], csrf: &str, version: &str) -> Markup {
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
                    h2 { "Devices" }
                    @if devices.is_empty() {
                        p class="hint" { "No devices have registered yet." }
                    } @else {
                        table {
                            thead { tr { th { "Device" } th { "State" } th { "Version" } } }
                            tbody {
                                @for device in devices {
                                    tr {
                                        td {
                                            strong { (device.name.as_deref().unwrap_or(&device.id)) }
                                            br;
                                            code { (&device.id) }
                                        }
                                        td {
                                            @if device.online { span class="ok" { "online" } } @else { span class="off" { "offline" } }
                                        }
                                        td { (device.version.as_deref().unwrap_or("—")) }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
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
        let page = dashboard(&[device], "00", "0.3.48").into_string();
        assert!(
            page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "{page}"
        );
        assert!(!page.contains("<script>alert(1)</script>"), "{page}");
    }
}
