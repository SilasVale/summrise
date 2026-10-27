//! The console's page: the desktop shell — an icon rail, a top bar, a scrolling canvas and a status
//! strip — and one renderer per surface. `maud` builds the HTML from the Rust that already owns the
//! data, and every interpolated value is escaped by construction.
//!
//! The only script on any page is the terminal's glue (`assets/terminal.js`); every other page is a
//! form and a link. That is why the shell carries no toggles: the pages are server-rendered, and a
//! control that only exists in the browser would be a second, weaker copy of a rule the console
//! already enforces.

use maud::{html, Markup, PreEscaped, DOCTYPE};

use super::DeviceView;

/// The page's whole style: the console's tokens, the desktop shell, and the surfaces it frames.
///
/// Every colour is a token, and every token has a dark value: the operator reads this page on a
/// desktop that follows the system theme, and a surface that only got a light value would keep it in
/// the dark (`--panel` on `--ink` is the measured way that goes wrong).
const STYLE: &str = r#"
:root {
  color-scheme: light dark;
  --bg:#fafafa; --panel:#ffffff; --panel-2:#f4f4f5;
  --ink:#18181b; --muted:#71717a; --faint:#a1a1aa;
  --line:#e4e4e7; --line-strong:#d4d4d8;
  --accent:#bf3a0a; --accent-solid:#b03a0a; --accent-hover:#a63308; --accent-soft:#ffefe5; --accent-fg:#ffffff;
  --ok:#1e7a33; --ok-soft:#e7f6ec; --danger:#b91c1c; --danger-soft:#fdecec;
  --rail:#18181b; --rail-line:rgba(255,255,255,.08); --rail-ink:#a1a1aa; --rail-ink-hover:#f4f4f5; --rail-focus:#ffb86b;
  --shadow:0 1px 2px rgba(24,24,27,.06), 0 10px 28px -22px rgba(24,24,27,.25);
  --radius:14px; --radius-sm:10px; --radius-xs:8px;
  --fs-xs:11px; --fs-sm:12px; --fs-base:13px; --fs-md:14px; --fs-lg:17px;
  --font:-apple-system,"SF Pro Text","PingFang SC","Hiragino Sans GB","Microsoft YaHei","Segoe UI",Roboto,sans-serif;
  --mono:"SF Mono",ui-monospace,"JetBrains Mono",Consolas,"Cascadia Mono",monospace;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg:#131418; --panel:#1c1d22; --panel-2:#24252b;
    --ink:#ecedef; --muted:#a2a3ac; --faint:#6f707a;
    --line:rgba(255,255,255,.09); --line-strong:rgba(255,255,255,.16);
    --accent:#ffa94d; --accent-solid:#b03a0a; --accent-hover:#c2410c; --accent-soft:#3d2817; --accent-fg:#ffffff;
    --ok:#69db7c; --ok-soft:rgba(47,158,68,.16); --danger:#ff8787; --danger-soft:rgba(220,38,38,.16);
    --rail:#0f1014; --rail-line:rgba(255,255,255,.07); --rail-ink:#8f9099; --rail-ink-hover:#ffffff; --rail-focus:#ffb86b;
    --shadow:0 1px 2px rgba(0,0,0,.4), 0 14px 34px -24px rgba(0,0,0,.8);
  }
}
* { box-sizing:border-box; }
html { height:100%; }
body { margin:0; min-height:100%; background:var(--bg); color:var(--ink); font:var(--fs-base)/1.5 var(--font); -webkit-font-smoothing:antialiased; }
a { color:var(--accent); text-decoration:none; }
a:hover { text-decoration:underline; }
:focus-visible { outline:2px solid var(--accent); outline-offset:2px; }

/* ---- the shell: dark icon rail, one column, a 26px strip at the foot ---- */
body.app { margin:0; height:100vh; display:flex; overflow:hidden; }
.rail { flex:0 0 52px; display:flex; flex-direction:column; align-items:center; gap:4px; padding:10px 0 12px; background:var(--rail); border-right:1px solid var(--rail-line); overflow-y:auto; scrollbar-width:none; }
.rail::-webkit-scrollbar { display:none; }
.rail a { display:grid; place-items:center; flex:none; width:40px; height:40px; border-radius:10px; color:var(--rail-ink); }
.rail a:hover { background:rgba(255,255,255,.08); color:var(--rail-ink-hover); text-decoration:none; }
.rail a:focus-visible { outline-color:var(--rail-focus); outline-offset:2px; }
.rail a.active, .rail a.active:hover { background:var(--accent-solid); color:#fff; }
.rail .brand { margin-bottom:10px; color:var(--rail-focus); }
.rail .brand:hover { background:transparent; color:#fff; }
.shell { flex:1; min-width:0; min-height:0; display:flex; flex-direction:column; height:100vh; }
.topbar { flex:none; display:flex; align-items:center; gap:12px; padding:12px 20px; background:var(--panel); border-bottom:1px solid var(--line); }
.topbar h1 { margin:0; font-size:var(--fs-md); font-weight:600; letter-spacing:-.01em; white-space:nowrap; }
.topbar .device { min-width:0; max-width:42ch; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:var(--muted); font-size:var(--fs-sm); }
.topbar .spacer { flex:1; }
.topbar .version { color:var(--muted); font-size:var(--fs-xs); font-family:var(--mono); }
.topbar form { margin:0; }
.content { flex:1; min-height:0; overflow:auto; padding:24px 24px 40px; }
.content h2 { margin:24px 0 10px; font-size:var(--fs-md); font-weight:600; }
.content h2:first-child { margin-top:0; }
.content h3 { margin:28px 0 8px; color:var(--muted); font-size:var(--fs-xs); font-weight:600; text-transform:uppercase; letter-spacing:.08em; }
.statusbar { flex:none; display:flex; align-items:center; gap:8px; height:26px; padding:0 14px; background:var(--panel); border-top:1px solid var(--line-strong); color:var(--muted); font-size:var(--fs-xs); white-space:nowrap; }
.statusbar .spacer { flex:1; }
.statusbar .middle { min-width:0; overflow:hidden; text-overflow:ellipsis; }
.statusbar .sep { color:var(--faint); }
.statusbar .side { color:var(--muted); text-transform:uppercase; letter-spacing:.1em; }

/* ---- shared surfaces ---- */
.hint { margin:0 0 16px; max-width:78ch; color:var(--muted); font-size:var(--fs-sm); }
.error { margin:0 0 16px; padding:8px 12px; border:1px solid var(--danger); border-radius:var(--radius-xs); background:var(--danger-soft); color:var(--danger); font-size:var(--fs-sm); }
.code { margin:0 0 16px; padding:10px 14px; border:1px solid var(--line); border-radius:var(--radius-sm); background:var(--panel); font-size:var(--fs-sm); }
.code strong { font-family:var(--mono); letter-spacing:.04em; }
code { padding:1px 5px; border-radius:6px; background:var(--panel-2); color:var(--muted); font-family:var(--mono); font-size:var(--fs-sm); }
pre.file { margin:0 0 16px; padding:14px 16px; border:1px solid var(--line); border-radius:var(--radius-sm); background:var(--panel); box-shadow:var(--shadow); overflow:auto; font:var(--fs-sm)/1.6 var(--mono); white-space:pre-wrap; }
table { width:100%; border-collapse:separate; border-spacing:0; overflow:hidden; background:var(--panel); border:1px solid var(--line); border-radius:var(--radius-sm); }
th, td { padding:10px 14px; border-bottom:1px solid var(--line); text-align:left; vertical-align:top; }
thead th { background:var(--panel-2); color:var(--muted); font-size:var(--fs-xs); font-weight:600; text-transform:uppercase; letter-spacing:.06em; }
tbody tr:last-child td { border-bottom:0; }
tbody tr:hover td, tbody tr:hover th { background:var(--panel-2); }
.ok, .off { display:inline-flex; align-items:center; gap:5px; padding:2px 8px; border-radius:999px; font-size:var(--fs-xs); font-weight:600; }
.ok { background:var(--ok-soft); color:var(--ok); }
.off { background:var(--bg); color:var(--muted); }
.ok::before, .off::before { content:""; width:6px; height:6px; border-radius:50%; background:currentColor; }
.section-head { display:flex; align-items:center; gap:12px; margin:0 0 12px; }
.section-head h2 { margin:0; }
.section-head form { margin:0 0 0 auto; }
.section-head button { margin:0; width:auto; }
.links { display:flex; gap:16px; margin-top:24px; font-size:var(--fs-sm); }
.bar { height:8px; min-width:120px; border-radius:999px; background:var(--line); overflow:hidden; }
.bar-fill { height:100%; border-radius:999px; background:var(--accent-solid); }
img.capture { max-width:100%; border:1px solid var(--line); border-radius:var(--radius-sm); background:var(--panel); box-shadow:var(--shadow); }

/* ---- forms ---- */
label { display:block; margin:0 0 4px; color:var(--muted); font-size:var(--fs-sm); }
input, select, textarea { width:100%; padding:8px 10px; border:1px solid var(--line-strong); border-radius:var(--radius-xs); background:var(--panel); color:var(--ink); font:inherit; }
input:focus-visible, select:focus-visible, textarea:focus-visible { outline:2px solid var(--accent); outline-offset:1px; }
textarea { font:var(--fs-sm)/1.5 var(--mono); }
form { margin:0; }
button { font:inherit; cursor:pointer; }
button[type=submit] { margin-top:12px; padding:8px 14px; border:0; border-radius:var(--radius-xs); background:var(--accent-solid); color:var(--accent-fg); font-size:var(--fs-sm); font-weight:600; }
button[type=submit]:hover { background:var(--accent-hover); }
button.inline-button, button.ghost { background:transparent; color:var(--muted); border:1px solid var(--line-strong); font-weight:600; }
button.inline-button:hover, button.ghost:hover { background:var(--panel-2); color:var(--ink); border-color:var(--faint); }
.inline-button, .ghost { margin:0; width:auto; }
.row-form { display:inline-flex; gap:6px; margin:8px 6px 0 0; }
.row-form input[type=text], .row-form input[type=number] { width:122px; padding:4px 8px; font-size:var(--fs-sm); }
.row-form button { margin:0; padding:5px 10px; font-size:var(--fs-sm); width:auto; }
.format-form { display:flex; flex-wrap:wrap; align-items:end; gap:8px; margin-top:8px; }
.format-form input, .format-form select { width:auto; min-width:180px; }
.format-form button { margin:0; width:auto; }
label.check { display:flex; align-items:center; gap:8px; margin:12px 0; color:var(--ink); font-size:var(--fs-sm); }
label.check input { width:auto; }

/* ---- sign-in card (no shell behind it: there is no session yet) ---- */
body.login { display:grid; place-items:center; min-height:100vh; padding:24px; }
.card { width:min(100%,380px); padding:28px; border:1px solid var(--line); border-radius:var(--radius); background:var(--panel); box-shadow:var(--shadow); }
.card h1 { margin:0 0 6px; font-size:var(--fs-lg); font-weight:600; letter-spacing:-.01em; }
.card h1 span { color:var(--muted); font-weight:400; }
.card .hint { margin-bottom:20px; }
.card form { margin:0; }
.card button { width:100%; margin-top:14px; padding:9px 12px; }

/* ---- the terminal page: the one canvas that fills instead of scrolling ---- */
body.terminal .content { display:flex; flex-direction:column; gap:8px; min-height:0; overflow:hidden; padding:16px 20px; }
.terminal-form { display:flex; flex-wrap:wrap; gap:10px 14px; align-items:end; }
.terminal-form label { margin:0; font-size:var(--fs-xs); text-transform:uppercase; letter-spacing:.06em; }
.terminal-form select, .terminal-form input { width:auto; min-width:130px; padding:6px 8px; font-size:var(--fs-sm); }
.kind-fields { display:flex; flex-wrap:wrap; gap:10px 14px; align-items:end; }
.kind-fields[hidden] { display:none; }
.tabs { display:flex; flex-wrap:wrap; gap:4px; }
.tab { display:flex; align-items:center; gap:6px; width:auto; margin:0; padding:5px 9px; border:1px solid var(--line); border-radius:var(--radius-xs); background:var(--panel); color:var(--muted); font-size:var(--fs-sm); }
.tab:hover { border-color:var(--line-strong); color:var(--ink); }
.tab.active { border-color:var(--accent-solid); background:var(--accent-soft); color:var(--ink); }
.tab-close { width:auto; margin:0; padding:0 2px; border:0; background:transparent; color:var(--muted); font-size:14px; line-height:1; }
.tab-close:hover { color:var(--danger); }
.panes { flex:1; min-height:0; display:flex; gap:6px; padding:6px; border:1px solid var(--line); border-radius:var(--radius-sm); background:#0b0c0f; }
.pane { flex:1; min-height:0; display:flex; overflow:hidden; }
.pane[hidden] { display:none; }
#terminal-status { margin:0; color:var(--muted); font-size:var(--fs-sm); }

@media (max-width: 720px) {
  .topbar { padding:8px 12px; }
  .topbar .device { display:none; }
  .content { padding:16px 12px 32px; }
  .statusbar .side { display:none; }
}
"#;

/// The rail's pages, in reading order: the console first, then the device surfaces a row links to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Fleet,
    Terminal,
    Files,
    Runs,
    System,
    Memory,
    Screen,
    Monitors,
    Browser,
    Tokens,
    Vitals,
}

impl Page {
    /// Every page the rail lists — one 40×40 anchor each, in this order.
    const ALL: [Page; 11] = [
        Page::Fleet,
        Page::Terminal,
        Page::Files,
        Page::Runs,
        Page::System,
        Page::Memory,
        Page::Screen,
        Page::Monitors,
        Page::Browser,
        Page::Tokens,
        Page::Vitals,
    ];

    fn label(self) -> &'static str {
        match self {
            Page::Fleet => "Fleet",
            Page::Terminal => "Terminal",
            Page::Files => "Files",
            Page::Runs => "Runs",
            Page::System => "System",
            Page::Memory => "Memory",
            Page::Screen => "Screen",
            Page::Monitors => "Monitors",
            Page::Browser => "Browser",
            Page::Tokens => "Tokens",
            Page::Vitals => "Vitals",
        }
    }

    /// Where the rail sends an operator. A device surface needs a device, so on a page without one
    /// the rail goes back to the fleet — the table there is the list of devices that can be named.
    fn href(self, device: Option<&DeviceView>) -> String {
        let Some(device) = device else {
            return "/next".to_owned();
        };
        let id = &device.id;
        match self {
            Page::Fleet => "/next".to_owned(),
            Page::Terminal => format!("/next/terminal/{id}"),
            Page::Files => format!("/next/files/{id}"),
            Page::Runs => format!("/next/runs/{id}"),
            Page::System => format!("/next/system/{id}"),
            Page::Memory => format!("/next/memory/{id}"),
            Page::Screen => format!("/next/screen/{id}"),
            Page::Monitors => format!("/next/monitors/{id}"),
            Page::Browser => format!("/next/browser/{id}"),
            Page::Tokens => format!("/next/tokens/{id}"),
            Page::Vitals => format!("/next/vitals/{id}"),
        }
    }

    /// The page's 18px stroke glyph, drawn inline: no icon font and no external asset, so the rail
    /// cannot render as a column of empty squares on a console with no network.
    fn icon(self) -> Markup {
        html! {
            svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" {
                @match self {
                    Page::Fleet => {
                        rect x="3" y="3" width="7" height="7" rx="1.5" {}
                        rect x="14" y="3" width="7" height="7" rx="1.5" {}
                        rect x="3" y="14" width="7" height="7" rx="1.5" {}
                        rect x="14" y="14" width="7" height="7" rx="1.5" {}
                    },
                    Page::Terminal => {
                        polyline points="4 17 10 11 4 5" {}
                        line x1="12" y1="19" x2="20" y2="19" {}
                    },
                    Page::Files => {
                        path d="M3 6.5A1.5 1.5 0 0 1 4.5 5h4.5l2 2.6h7A1.5 1.5 0 0 1 19.5 9v9a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 3 18V6.5Z" {}
                    },
                    Page::Runs => {
                        rect x="3" y="3" width="18" height="18" rx="4" {}
                        path d="M10 8.5 16 12l-6 3.5Z" {}
                    },
                    Page::System => {
                        rect x="6" y="6" width="12" height="12" rx="2" {}
                        rect x="9.5" y="9.5" width="5" height="5" rx="1" {}
                        path d="M9 3v3M15 3v3M9 18v3M15 18v3M3 9h3M3 15h3M18 9h3M18 15h3" {}
                    },
                    Page::Memory => {
                        ellipse cx="12" cy="5.5" rx="8" ry="3" {}
                        path d="M4 5.5v13c0 1.7 3.6 3 8 3s8-1.3 8-3v-13" {}
                        path d="M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3" {}
                    },
                    Page::Screen => {
                        rect x="2.5" y="4" width="19" height="13" rx="2" {}
                        path d="M8 21h8M12 17v4" {}
                    },
                    Page::Monitors => {
                        path d="M2.5 12h4l2.5-6 4 12 2.5-6h6" {}
                    },
                    Page::Browser => {
                        circle cx="12" cy="12" r="9" {}
                        path d="M3 12h18" {}
                        path d="M12 3a14 14 0 0 1 3.6 9 14 14 0 0 1-3.6 9 14 14 0 0 1-3.6-9A14 14 0 0 1 12 3Z" {}
                    },
                    Page::Tokens => {
                        circle cx="7.5" cy="12" r="3.5" {}
                        path d="M11 12h10M17 12v3.5M20 12v2.5" {}
                    },
                    Page::Vitals => {
                        path d="M20.8 5.6a5 5 0 0 0-7.1 0L12 7.3l-1.7-1.7a5 5 0 1 0-7.1 7.1L12 21l8.8-8.3a5 5 0 0 0 0-7.1Z" {}
                    },
                }
            }
        }
    }
}

/// The shell's brand mark: the sunrise over a horizon, drawn from the same 24-grid as the rail.
fn brand_icon() -> Markup {
    html! {
        svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true" {
            circle cx="12" cy="10" r="3.6" {}
            path d="M3 17h18M6 21h12" {}
        }
    }
}

/// One rail anchor. The label rides in `title` and `aria-label`, never in the text: the rail is
/// icon-only, and an anchor whose text is "Files" would be a second "Files" link on a page whose
/// device row already has one.
fn rail_item(item: Page, active: Page, device: Option<&DeviceView>) -> Markup {
    let label = item.label();
    let href = item.href(device);
    if item == active {
        html! {
            a class="rail-item active" href=(href) title=(label) aria-label=(label) aria-current="page" {
                (item.icon())
            }
        }
    } else {
        html! {
            a class="rail-item" href=(href) title=(label) aria-label=(label) {
                (item.icon())
            }
        }
    }
}

/// The 52px rail: the brand, then one anchor per page, the current one marked for a screen reader.
fn rail(active: Page, device: Option<&DeviceView>) -> Markup {
    html! {
        nav class="rail" aria-label="Pages" {
            a class="brand" href="/next" title="Summrise console" aria-label="Summrise console" {
                (brand_icon())
            }
            @for page in Page::ALL {
                (rail_item(page, active, device))
            }
        }
    }
}

/// The device the page acts on, as a person reads it: the name when the operator gave one.
fn device_label(device: &DeviceView) -> String {
    device.name.clone().unwrap_or_else(|| {
        if device.id.len() > 12 {
            format!("{}…", &device.id[..12])
        } else {
            device.id.clone()
        }
    })
}

/// The status strip's middle cell on a device page.
fn device_status(device: &DeviceView) -> Markup {
    html! { (device_label(device)) }
}

/// The status strip's middle cell on the fleet: how many devices there are, and how many are live.
fn fleet_status(devices: &[DeviceView]) -> Markup {
    let total = devices.len();
    let online = devices.iter().filter(|device| device.online).count();
    html! {
        (total) " device" (if total == 1 { "" } else { "s" }) " · " (online) " online"
    }
}

/// The `head` every page shares. `extra` is where the terminal puts its stylesheet and its scripts.
fn head(title: &str, extra: Markup) -> Markup {
    html! {
        head {
            meta charset="utf-8";
            meta name="viewport" content="width=device-width, initial-scale=1";
            title { (title) " — Summrise console" }
            (extra)
            style { (PreEscaped(STYLE)) }
        }
    }
}

/// The chrome a signed-in page shares. Constructed per page; `page` writes it once.
struct Chrome<'a> {
    /// The rail item that marks this page. Sub-pages without their own item name their section.
    active: Page,
    /// The device the page acts on; `None` on the fleet and the logs.
    device: Option<&'a DeviceView>,
    /// The top bar's heading and the document's `<title>`.
    title: &'a str,
    version: &'a str,
    /// The session's CSRF value: the sign-out form and every mutation on the page carry it.
    csrf: &'a str,
    /// The status strip's middle cell.
    status: Markup,
}

/// A page's body class: the terminal is the one canvas that fills instead of scrolling.
fn body_class(active: Page) -> &'static str {
    if active == Page::Terminal {
        "app terminal"
    } else {
        "app"
    }
}

/// The top bar: where this page is, which device it acts on, the build, and the way out.
fn topbar(title: &str, device: Option<&DeviceView>, version: &str, csrf: &str) -> Markup {
    html! {
        header class="topbar" {
            h1 { (title) }
            @if let Some(device) = device {
                span class="device" { (device_label(device)) }
            }
            span class="spacer" {}
            span class="version" { "console " (version) }
            form method="post" action="/next/logout" {
                input type="hidden" name="csrf" value=(csrf);
                button type="submit" class="ghost" { "Sign out" }
            }
        }
    }
}

/// The 26px strip at the foot: the build, the page's one fact, and which surface this is.
fn statusbar(version: &str, middle: Markup) -> Markup {
    html! {
        footer class="statusbar" {
            span { "v" (version) }
            span class="sep" aria-hidden="true" { "·" }
            span class="middle" { (middle) }
            span class="spacer" {}
            span class="side" { "console" }
        }
    }
}

/// One signed-in page: rail, top bar, canvas and status strip — written once, for every surface.
fn page(chrome: Chrome<'_>, content: Markup) -> Markup {
    page_with_head(chrome, Markup::default(), content)
}

/// The same page with something extra in `<head>` (the terminal's xterm stylesheet and scripts).
fn page_with_head(chrome: Chrome<'_>, extra: Markup, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            (head(chrome.title, extra))
            body class=(body_class(chrome.active)) {
                (rail(chrome.active, chrome.device))
                div class="shell" {
                    (topbar(chrome.title, chrome.device, chrome.version, chrome.csrf))
                    main class="content" { (content) }
                    (statusbar(chrome.version, chrome.status))
                }
            }
        }
    }
}

/// The sign-in card. `error` is the one line a refused token gets.
pub(super) fn login(error: Option<&str>) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Summrise console" }
                style { (PreEscaped(STYLE)) }
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
    let content = html! {
        @if let Some(code) = code {
            p class="code" {
                "Enrollment code: " strong { (code) } " — single use, valid for 15 minutes."
            }
        }
        @if let Some(error) = error {
            p class="error" { "That change was not applied: " (error) }
        }
        div class="section-head" {
            h2 { "Devices" }
            form method="post" action="/next/enroll" {
                input type="hidden" name="csrf" value=(csrf);
                button type="submit" class="inline-button" { "New enrollment code" }
            }
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
                                " · "
                                a href=(format!("/next/runs/{}", device.id)) { "Runs" }
                                " · "
                                a href=(format!("/next/system/{}", device.id)) { "System" }
                                " · "
                                a href=(format!("/next/state/{}", device.id)) { "State" }
                                " · "
                                a href=(format!("/next/memory/{}", device.id)) { "Memory" }
                                " · "
                                a href=(format!("/next/screen/{}", device.id)) { "Screen" }
                                " · "
                                a href=(format!("/next/monitors/{}", device.id)) { "Monitors" }
                                " · "
                                a href=(format!("/next/browser/{}", device.id)) { "Browser" }
                                " · "
                                a href=(format!("/next/tokens/{}", device.id)) { "Connect" }
                                " · "
                                a href=(format!("/next/vitals/{}", device.id)) { "Vitals" }
                            }
                        }
                    }
                }
            }
        }
        nav class="links" {
            a href="/next/audit" { "Audit" }
            a href="/next/denials" { "Denials" }
        }
    };
    page(
        Chrome {
            active: Page::Fleet,
            device: None,
            title: "Fleet",
            version,
            csrf,
            status: fleet_status(devices),
        },
        content,
    )
}

/// A read-only log page: the audit or denial lines, newest first, bounded by the caller.
pub(super) fn logs(rows: &[serde_json::Value], title: &str, csrf: &str, version: &str) -> Markup {
    let content = html! {
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
        nav class="links" { a href="/next" { "Fleet" } }
    };
    page(
        Chrome {
            active: Page::Fleet,
            device: None,
            title,
            version,
            csrf,
            status: html! { (rows.len()) " lines" },
        },
        content,
    )
}

/// The terminal page: one xterm host, wired to the console's `/next/terminal` endpoints.
///
/// The device id and this session's CSRF value ride in data attributes, not in the URL or a cookie
/// the script reads: the session cookie is `HttpOnly`, so the page cannot leak what it cannot see.
pub(super) fn terminal(device: &DeviceView, csrf: &str, version: &str) -> Markup {
    let content = html! {
        form id="terminal-form" class="terminal-form" {
            label for="kind" { "Kind" }
            select id="kind" name="kind" {
                option value="pty" { "pty — a shell on this machine" }
                option value="ssh" { "ssh — a host this device can reach" }
                option value="serial" { "serial — an OS port" }
            }
            div class="kind-fields" data-kind="ssh" hidden {
                label for="host" { "Host" }
                input id="host" name="host" type="text" placeholder="192.168.1.1";
                label for="ssh_port" { "Port" }
                input id="ssh_port" name="ssh_port" type="number" value="22" min="1" max="65535";
                label for="user" { "User" }
                input id="user" name="user" type="text" placeholder="root";
                label for="password" { "Password" }
                input id="password" name="password" type="password" autocomplete="off";
            }
            div class="kind-fields" data-kind="serial" hidden {
                label for="serial_port" { "Port" }
                input id="serial_port" name="serial_port" type="text" placeholder="COM3 or /dev/ttyUSB0";
                label for="baud" { "Baud" }
                input id="baud" name="baud" type="number" value="115200" min="1";
            }
            button type="submit" { "Start session" }
        }
        div id="terminal-tabs" class="tabs" {}
        p id="terminal-status" { "choose a kind and start a session" }
        div id="terminal-panes" class="panes" {
            div id="terminal" data-device=(device.id) data-csrf=(csrf) hidden {}
        }
    };
    let extra = html! {
        link rel="stylesheet" href="/assets/xterm.css";
        script src="/assets/xterm.js" defer {}
        script src="/assets/addon-fit.js" defer {}
        script src="/assets/terminal.js" defer {}
    };
    page_with_head(
        Chrome {
            active: Page::Terminal,
            device: Some(device),
            title: "Terminal",
            version,
            csrf,
            status: device_status(device),
        },
        extra,
        content,
    )
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
    let content = html! {
        @if path.is_empty() {
            p class="hint" {
                "The device's files root. Paths are relative to it; the device enforces the root."
            }
        } @else {
            p {
                "at /" (path)
                " · "
                a href=(format!("{}?path={}", base, encode(parent(path)))) { "up" }
            }
        }
        @if let Some(error) = error {
            p class="error" { (error) }
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
        h2 { "Read a file" }
        form method="get" action=(format!("{}/read", base)) class="format-form" {
            input type="text" name="path" placeholder="notes.txt" required;
            button type="submit" { "Read" }
        }
        h2 { "Write a file" }
        form method="post" action=(format!("{}/write", base)) {
            input type="hidden" name="csrf" value=(csrf);
            input type="text" name="path" placeholder="notes.txt" required;
            textarea name="content" rows="8" placeholder="text, written as UTF-8" {}
            button type="submit" { "Write" }
        }
    };
    page(
        Chrome {
            active: Page::Files,
            device: Some(device),
            title: "Files",
            version,
            csrf,
            status: device_status(device),
        },
        content,
    )
}

/// One file, as text.
pub(super) fn file_text(
    device: &DeviceView,
    path: &str,
    text: &str,
    csrf: &str,
    version: &str,
) -> Markup {
    let base = format!("/next/files/{}", device.id);
    let content = html! {
        div class="section-head" {
            h2 { (path) }
            a href=(format!("{}?path={}", base, encode(parent(path)))) { "Back to the directory" }
        }
        pre class="file" { (text) }
    };
    page(
        Chrome {
            active: Page::Files,
            device: Some(device),
            title: "File",
            version,
            csrf,
            status: device_status(device),
        },
        content,
    )
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

/// One row of the run history, as the page draws it.
pub(super) struct RunRow {
    pub(super) command_line: String,
    pub(super) started_at_ms: u64,
    pub(super) duration_ms: u64,
    pub(super) exit_code: Option<i64>,
    pub(super) timed_out: bool,
}

/// One finished run, with the output the tool captured for it.
pub(super) struct RunResult {
    pub(super) id: String,
    pub(super) exit_code: Option<i64>,
    pub(super) timed_out: bool,
    pub(super) duration_ms: u64,
    pub(super) stdout: String,
    pub(super) stderr: String,
    pub(super) truncated: bool,
}

/// A UTC stamp from Unix milliseconds, without a date library: `MM-DD HH:MM:SS UTC`.
fn stamp(ms: u64) -> String {
    let seconds = ms / 1000;
    let days = seconds / 86_400;
    let day_seconds = seconds % 86_400;
    let (hour, minute, second) = (
        day_seconds / 3600,
        (day_seconds % 3600) / 60,
        day_seconds % 60,
    );
    // 1970-01-01 was a Thursday; the month/day arithmetic starts at 1970-01-01 and counts forward.
    let (month, day) = civil_from_days(days as i64);
    format!("{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC")
}

/// Days since 1970-01-01 to (month, day), by the usual 400-year cycle arithmetic.
fn civil_from_days(days: i64) -> (u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let _year = yoe + era as u64 * 400 + if month <= 2 { 1 } else { 0 };
    (month as u32, day as u32)
}

/// The runs page: a form to run one command, and the device's own history under it.
pub(super) fn runs(
    device: &DeviceView,
    rows: &[RunRow],
    csrf: &str,
    version: &str,
    error: Option<&str>,
) -> Markup {
    let base = format!("/next/runs/{}", device.id);
    let content = html! {
        p class="hint" {
            "A run is a process the device starts and reaps before it answers. The request holds "
            "the page open for the run's length: keep the timeout short, or start the long work "
            "with Start-Process and watch it through a later run. runs.exec is audited."
        }
        @if let Some(error) = error {
            p class="error" { (error) }
        }
        h2 { "Run a command" }
        form method="post" action=(format!("{base}/exec")) {
            input type="hidden" name="csrf" value=(csrf);
            label for="command" { "Command" }
            input id="command" name="command" type="text" placeholder="pwsh" required;
            label for="args" { "Arguments, one per line" }
            textarea id="args" name="args" rows="3" placeholder="-NoProfile&#10;-Command&#10;Get-Date" {}
            label for="cwd" { "Working directory (optional)" }
            input id="cwd" name="cwd" type="text" placeholder="relative to the files root";
            label for="timeout_ms" { "Timeout (ms)" }
            input id="timeout_ms" name="timeout_ms" type="number" value="30000" min="1000" max="600000";
            label class="check" {
                input type="checkbox" name="shell" value="1";
                " Run through the platform shell"
            }
            button type="submit" { "Run" }
        }
        h2 { "History" }
        @if rows.is_empty() {
            p class="hint" { "No runs recorded yet." }
        } @else {
            table {
                thead { tr { th { "When" } th { "Command" } th { "Took" } th { "Exit" } } }
                tbody {
                    @for row in rows {
                        tr {
                            td { (stamp(row.started_at_ms)) }
                            td { code { (row.command_line) } }
                            td { (row.duration_ms) " ms" }
                            td {
                                @if row.timed_out {
                                    "timed out"
                                } @else if let Some(code) = row.exit_code {
                                    (code)
                                } @else {
                                    "killed"
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    page(
        Chrome {
            active: Page::Runs,
            device: Some(device),
            title: "Runs",
            version,
            csrf,
            status: device_status(device),
        },
        content,
    )
}

/// One finished run: its status, then the output the tool captured.
pub(super) fn run_result(
    device: &DeviceView,
    command_line: &str,
    result: &RunResult,
    csrf: &str,
    version: &str,
) -> Markup {
    let status = if result.timed_out {
        "timed out".to_owned()
    } else if let Some(code) = result.exit_code {
        format!("exit {code}")
    } else {
        "killed".to_owned()
    };
    let content = html! {
        h2 { "Run " (result.id) }
        p { code { (command_line) } }
        p class="hint" {
            (status) " · " (result.duration_ms) " ms"
            @if result.truncated { " · output truncated at the device's cap" }
        }
        h3 { "stdout" }
        pre class="file" { (result.stdout) }
        @if !result.stderr.is_empty() {
            h3 { "stderr" }
            pre class="file" { (result.stderr) }
        }
    };
    page(
        Chrome {
            active: Page::Runs,
            device: Some(device),
            title: "Run",
            version,
            csrf,
            status: device_status(device),
        },
        content,
    )
}

/// One process row on the system page.
pub(super) struct ProcessRow {
    pub(super) pid: u64,
    pub(super) name: String,
    pub(super) memory_kb: u64,
}

/// The device's own state document: health, readiness and the last events, as the device wrote it.
pub(super) fn state_document(
    device: &DeviceView,
    pretty: &str,
    csrf: &str,
    version: &str,
) -> Markup {
    let content = html! {
        p class="hint" {
            "The document the device keeps: its health, and the last ten events it recorded."
        }
        pre class="file" { (pretty) }
    };
    page(
        Chrome {
            active: Page::System,
            device: Some(device),
            title: "State",
            version,
            csrf,
            status: device_status(device),
        },
        content,
    )
}

/// The system page: the process list, a kill form, and whatever the last action reported.
pub(super) fn system(
    device: &DeviceView,
    processes: &[ProcessRow],
    name_filter: &str,
    csrf: &str,
    version: &str,
    notice: Option<&str>,
) -> Markup {
    let base = format!("/next/system/{}", device.id);
    let content = html! {
        @if let Some(notice) = notice {
            p class="hint" { (notice) }
        }
        form method="get" action=(base) class="format-form" {
            input type="text" name="name" value=(name_filter) placeholder="filter by name";
            button type="submit" { "Filter" }
        }
        @if processes.is_empty() {
            p class="hint" { "The device listed no processes." }
        } @else {
            table {
                thead { tr { th { "PID" } th { "Name" } th { "Memory" } } }
                tbody {
                    @for process in processes {
                        tr {
                            td { (process.pid) }
                            td { code { (process.name) } }
                            td { (process.memory_kb) " KB" }
                        }
                    }
                }
            }
        }
        h2 { "End a process" }
        p class="hint" {
            "Name a pid or an exact executable name, not both. This is destructive and audited."
        }
        form method="post" action=(format!("{base}/kill")) class="format-form" {
            input type="hidden" name="csrf" value=(csrf);
            input type="number" name="pid" min="1" placeholder="pid";
            input type="text" name="name" placeholder="name";
            button type="submit" { "Kill" }
        }
    };
    page(
        Chrome {
            active: Page::System,
            device: Some(device),
            title: "System",
            version,
            csrf,
            status: device_status(device),
        },
        content,
    )
}

/// One memory entry, as the page draws it.
pub(super) struct MemoryRow {
    pub(super) id: String,
    pub(super) text: String,
    pub(super) tags: String,
}

/// The device-local memory: the entries, an append form, and a delete button per row.
pub(super) fn memory(
    device: &DeviceView,
    entries: &[MemoryRow],
    tag: &str,
    csrf: &str,
    version: &str,
    notice: Option<&str>,
) -> Markup {
    let base = format!("/next/memory/{}", device.id);
    let content = html! {
        p class="hint" {
            "What this device remembers: notes an operator or a client appended, newest first. "
            "It is the device's own store, not the console's."
        }
        @if let Some(notice) = notice {
            p class="hint" { (notice) }
        }
        form method="get" action=(base) class="format-form" {
            input type="text" name="tag" value=(tag) placeholder="filter by tag";
            button type="submit" { "Filter" }
        }
        @if entries.is_empty() {
            p class="hint" { "Nothing is remembered yet." }
        } @else {
            table {
                thead { tr { th { "Text" } th { "Tags" } th { "" } } }
                tbody {
                    @for entry in entries {
                        tr {
                            td { (entry.text) }
                            td { code { (entry.tags) } }
                            td {
                                form method="post" action=(format!("{base}/delete")) class="row-form" {
                                    input type="hidden" name="csrf" value=(csrf);
                                    input type="hidden" name="id" value=(entry.id);
                                    button type="submit" class="inline-button" { "Delete" }
                                }
                            }
                        }
                    }
                }
            }
        }
        h2 { "Remember something" }
        form method="post" action=(format!("{base}/append")) {
            input type="hidden" name="csrf" value=(csrf);
            textarea name="text" rows="3" placeholder="the fact to keep" required {}
            input type="text" name="tags" placeholder="tags, comma-separated";
            button type="submit" { "Append" }
        }
        p class="hint" { code { (device.id) } }
    };
    page(
        Chrome {
            active: Page::Memory,
            device: Some(device),
            title: "Memory",
            version,
            csrf,
            status: device_status(device),
        },
        content,
    )
}

/// The device's screen, as the per-user helper captured it.
pub(super) fn screen(
    device: &DeviceView,
    image: Option<&str>,
    detail: &str,
    csrf: &str,
    version: &str,
) -> Markup {
    let content = html! {
        p class="hint" { (detail) }
        @if let Some(image) = image {
            img class="capture" src=(image) alt="the device screen";
        }
        p { a href=(format!("/next/screen/{}", device.id)) { "Capture again" } }
    };
    page(
        Chrome {
            active: Page::Screen,
            device: Some(device),
            title: "Screen",
            version,
            csrf,
            status: device_status(device),
        },
        content,
    )
}

/// One monitor as the page draws it.
pub(super) struct MonitorRow {
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) target: String,
    pub(super) interval_ms: u64,
    pub(super) up_now: Option<bool>,
    pub(super) up_pct: Option<u64>,
    pub(super) drops: Option<u64>,
    pub(super) last_status: Option<u16>,
}

/// The monitors page: the watches the device runs, an add form and a remove per row.
pub(super) fn monitors(
    device: &DeviceView,
    rows: &[MonitorRow],
    csrf: &str,
    version: &str,
    error: Option<&str>,
) -> Markup {
    let base = format!("/next/monitors/{}", device.id);
    let content = html! {
        p class="hint" {
            "The device probes each target on its own interval and keeps the last 60 probes. "
            "A down target is a fact, not a failure of this page."
        }
        @if let Some(error) = error {
            p class="error" { (error) }
        }
        @if rows.is_empty() {
            p class="hint" { "No targets are watched yet." }
        } @else {
            table {
                thead { tr { th { "Target" } th { "Kind" } th { "Every" } th { "Now" } th { "Up %" } th { "Drops" } th { "Status" } th { "" } } }
                tbody {
                    @for row in rows {
                        tr {
                            td { code { (row.target) } }
                            td { (row.kind) }
                            td { (row.interval_ms / 1000) " s" }
                            td {
                                @match row.up_now {
                                    Some(true) => { span class="ok" { "up" } },
                                    Some(false) => { span class="off" { "down" } },
                                    None => { "—" },
                                }
                            }
                            td { (row.up_pct.map(|pct| pct.to_string()).unwrap_or_else(|| "—".to_owned())) }
                            td { (row.drops.map(|drops| drops.to_string()).unwrap_or_else(|| "—".to_owned())) }
                            td { (row.last_status.map(|status| status.to_string()).unwrap_or_else(|| "—".to_owned())) }
                            td {
                                form method="post" action=(format!("{base}/remove")) class="row-form" {
                                    input type="hidden" name="csrf" value=(csrf);
                                    input type="hidden" name="id" value=(row.id);
                                    button type="submit" class="inline-button" { "Remove" }
                                }
                            }
                        }
                    }
                }
            }
        }
        h2 { "Watch a target" }
        form method="post" action=(format!("{base}/add")) {
            input type="hidden" name="csrf" value=(csrf);
            label for="kind" { "Kind" }
            select id="kind" name="kind" {
                option value="tcp" { "tcp" }
                option value="http" { "http" }
            }
            label for="host" { "Host" }
            input id="host" name="host" type="text" placeholder="192.168.1.1" required;
            label for="port" { "Port" }
            input id="port" name="port" type="number" min="1" max="65535" required;
            label for="interval_ms" { "Interval (ms)" }
            input id="interval_ms" name="interval_ms" type="number" value="15000" min="5000";
            label for="path" { "HTTP path (http only)" }
            input id="path" name="path" type="text" placeholder="/";
            label for="expect" { "Body must contain (http only)" }
            input id="expect" name="expect" type="text";
            button type="submit" { "Add target" }
        }
    };
    page(
        Chrome {
            active: Page::Monitors,
            device: Some(device),
            title: "Monitors",
            version,
            csrf,
            status: device_status(device),
        },
        content,
    )
}

/// One browser target on the browser page.
pub(super) struct BrowserRow {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) url: String,
    pub(super) kind: String,
}

/// The browser page: the targets, a capture of the first one, and the open/close forms.
pub(super) fn browser(
    device: &DeviceView,
    rows: &[BrowserRow],
    shot: Option<&str>,
    csrf: &str,
    version: &str,
    error: Option<&str>,
) -> Markup {
    let base = format!("/next/browser/{}", device.id);
    let content = html! {
        p class="hint" {
            "The browser the device runs for you — in the user's session when the helper is "
            "there, headless otherwise. The capture is the first target in the device's list."
        }
        @if let Some(error) = error {
            p class="error" { (error) }
        }
        @if let Some(shot) = shot {
            img class="capture" src=(shot) alt="the device's browser";
        }
        @if rows.is_empty() {
            p class="hint" { "The device has no browser targets." }
        } @else {
            table {
                thead { tr { th { "Title" } th { "URL" } th { "Type" } th { "" } } }
                tbody {
                    @for row in rows {
                        tr {
                            td { (row.title) }
                            td { code { (row.url) } }
                            td { (row.kind) }
                            td {
                                form method="post" action=(format!("{base}/close")) class="row-form" {
                                    input type="hidden" name="csrf" value=(csrf);
                                    input type="hidden" name="target" value=(row.id);
                                    button type="submit" class="inline-button" { "Close" }
                                }
                            }
                        }
                    }
                }
            }
        }
        h2 { "Open a page" }
        form method="post" action=(format!("{base}/open")) class="format-form" {
            input type="hidden" name="csrf" value=(csrf);
            input type="text" name="url" placeholder="https://example.com" required;
            button type="submit" { "Open" }
        }
    };
    page(
        Chrome {
            active: Page::Browser,
            device: Some(device),
            title: "Browser",
            version,
            csrf,
            status: device_status(device),
        },
        content,
    )
}

/// One token as the page draws it.
pub(super) struct TokenRow {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) tools: u64,
    pub(super) destructive: bool,
    pub(super) created_ms: u64,
}

/// The tokens a device accepts, with the form that mints one and a revoke per row.
pub(super) fn tokens(
    device: &DeviceView,
    rows: &[TokenRow],
    csrf: &str,
    version: &str,
    error: Option<&str>,
) -> Markup {
    let base = format!("/next/tokens/{}", device.id);
    let content = html! {
        h2 { "Connect an AI client" }
        p class="hint" {
            "A permanent token is one credential for this device, every tool, destructive "
            "calls included. It is shown once; the console keeps only its hash. The client "
            "connects to /mcp/{device id} with `Authorization: Bearer <token>`."
        }
        @if let Some(error) = error {
            p class="error" { (error) }
        }
        form method="post" action=(format!("{base}/mint")) class="format-form" {
            input type="hidden" name="csrf" value=(csrf);
            input type="text" name="name" placeholder="what this client is, e.g. dsh" required;
            button type="submit" { "Mint a token" }
        }
        h3 { "Tokens for this device" }
        @if rows.is_empty() {
            p class="hint" { "No token names this device yet." }
        } @else {
            table {
                thead { tr { th { "Name" } th { "Tools" } th { "Destructive" } th { "Created" } th { "" } } }
                tbody {
                    @for row in rows {
                        tr {
                            td { (row.name) }
                            td { (row.tools) }
                            td { @if row.destructive { "allowed" } @else { "read-only" } }
                            td { (stamp(row.created_ms)) }
                            td {
                                form method="post" action=(format!("{base}/revoke")) class="row-form" {
                                    input type="hidden" name="csrf" value=(csrf);
                                    input type="hidden" name="id" value=(row.id);
                                    button type="submit" class="inline-button" { "Revoke" }
                                }
                            }
                        }
                    }
                }
            }
        }
        p class="hint" { code { (device.id) } }
    };
    page(
        Chrome {
            active: Page::Tokens,
            device: Some(device),
            title: "Tokens",
            version,
            csrf,
            status: device_status(device),
        },
        content,
    )
}

/// The one page a minted token is ever shown on.
pub(super) fn token_created(
    device: &DeviceView,
    name: &str,
    secret: &str,
    config: &str,
    csrf: &str,
    version: &str,
) -> Markup {
    let content = html! {
        h2 { "Token " (name) }
        p class="hint" {
            "This is the only time the token is shown. Copy it into the client's configuration "
            "now; a lost value means minting another."
        }
        pre class="file" { (secret) }
        h3 { "Client configuration" }
        pre class="file" { (config) }
    };
    page(
        Chrome {
            active: Page::Tokens,
            device: Some(device),
            title: "Token",
            version,
            csrf,
            status: device_status(device),
        },
        content,
    )
}

/// One disk on the vitals page.
pub(super) struct VitalsDisk {
    pub(super) name: String,
    pub(super) total: u64,
    pub(super) free: u64,
}

/// What the device reports about itself right now. The page is a sample, not a history: the
/// console keeps no time series of its own.
pub(super) struct Vitals {
    pub(super) machine: String,
    pub(super) os: String,
    pub(super) cores: u64,
    pub(super) uptime_s: u64,
    pub(super) memory_total: u64,
    pub(super) memory_available: u64,
    pub(super) disks: Vec<VitalsDisk>,
    pub(super) version: String,
    pub(super) channel: String,
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

fn percent(used: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    ((used as f64 / total as f64) * 100.0).round() as u64
}

fn bar(used_pct: u64) -> Markup {
    html! {
        div class="bar" {
            div class="bar-fill" style=(format!("width:{used_pct}%")) {}
        }
    }
}

/// The vitals page: one sample of what the device reports, drawn as bars and a table.
pub(super) fn vitals(device: &DeviceView, sample: &Vitals, csrf: &str, version: &str) -> Markup {
    let memory_used = sample.memory_total.saturating_sub(sample.memory_available);
    let memory_pct = percent(memory_used, sample.memory_total);
    let content = html! {
        p class="hint" {
            "One sample, read from the device now. The console keeps no time series of its own: "
            "what it cannot store it does not draw."
        }
        table {
            tbody {
                tr { th { "Machine" } td { (sample.machine) } }
                tr { th { "OS" } td { (sample.os) } }
                tr { th { "Cores" } td { (sample.cores) } }
                tr { th { "Uptime" } td { (sample.uptime_s / 3600) " h " ((sample.uptime_s % 3600) / 60) " m" } }
                tr { th { "Build" } td { (sample.version) " (" (sample.channel) ")" } }
            }
        }
        h2 { "Memory" }
        p { (gib(memory_used)) " used of " (gib(sample.memory_total)) " — " (memory_pct) "%" }
        (bar(memory_pct))
        h2 { "Disks" }
        @if sample.disks.is_empty() {
            p class="hint" { "The device reported no disks." }
        } @else {
            table {
                thead { tr { th { "Disk" } th { "Used" } th { "Free" } th { "" } } }
                tbody {
                    @for disk in &sample.disks {
                        tr {
                            td { code { (&disk.name) } }
                            td { (gib(disk.total.saturating_sub(disk.free))) " of " (gib(disk.total)) }
                            td { (gib(disk.free)) }
                            td { (bar(percent(disk.total.saturating_sub(disk.free), disk.total))) }
                        }
                    }
                }
            }
        }
    };
    page(
        Chrome {
            active: Page::Vitals,
            device: Some(device),
            title: "Vitals",
            version,
            csrf,
            status: device_status(device),
        },
        content,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rail's labels, in order, as a reader sees them.
    const LABELS: [&str; 11] = [
        "Fleet", "Terminal", "Files", "Runs", "System", "Memory", "Screen", "Monitors", "Browser",
        "Tokens", "Vitals",
    ];

    fn view(tag: char, name: Option<&str>, online: bool) -> DeviceView {
        DeviceView {
            id: tag.to_string().repeat(64),
            public_key: "00".repeat(32),
            name: name.map(str::to_owned),
            registered_at_ms: 0,
            last_seen_ms: 0,
            online,
            version: None,
            host: None,
            last_attempt: None,
        }
    }

    fn sample_vitals() -> Vitals {
        Vitals {
            machine: "DESKTOP-TEST".to_owned(),
            os: "Windows".to_owned(),
            cores: 4,
            uptime_s: 3_600,
            memory_total: 8 * 1024 * 1024 * 1024,
            memory_available: 4 * 1024 * 1024 * 1024,
            disks: vec![VitalsDisk {
                name: "C:\\".to_owned(),
                total: 100,
                free: 50,
            }],
            version: "0.3.65".to_owned(),
            channel: "canary".to_owned(),
        }
    }

    /// Every page function, with the rail item each one must mark. The terminal and the fleet are
    /// the two surfaces with their own chrome; the rest are the same shell with a different canvas.
    fn every_page(device: &DeviceView, sample: &Vitals) -> Vec<(&'static str, Markup)> {
        let run = RunResult {
            id: "run-1".to_owned(),
            exit_code: Some(0),
            timed_out: false,
            duration_ms: 12,
            stdout: "out".to_owned(),
            stderr: String::new(),
            truncated: false,
        };
        vec![
            (
                "Fleet",
                dashboard(std::slice::from_ref(device), "00", "0.1.0", None, None),
            ),
            ("Fleet", logs(&[], "Audit", "00", "0.1.0")),
            ("Fleet", logs(&[], "Denials", "00", "0.1.0")),
            ("Terminal", terminal(device, "00", "0.1.0")),
            ("Files", files(device, "", &[], "00", "0.1.0", None)),
            (
                "Files",
                file_text(device, "notes.txt", "text", "00", "0.1.0"),
            ),
            ("Runs", runs(device, &[], "00", "0.1.0", None)),
            ("Runs", run_result(device, "cmd", &run, "00", "0.1.0")),
            ("System", state_document(device, "{}", "00", "0.1.0")),
            ("System", system(device, &[], "", "00", "0.1.0", None)),
            ("Memory", memory(device, &[], "", "00", "0.1.0", None)),
            ("Screen", screen(device, None, "no screen", "00", "0.1.0")),
            ("Monitors", monitors(device, &[], "00", "0.1.0", None)),
            ("Browser", browser(device, &[], None, "00", "0.1.0", None)),
            ("Tokens", tokens(device, &[], "00", "0.1.0", None)),
            (
                "Tokens",
                token_created(device, "dsh", "srm_secret", "config", "00", "0.1.0"),
            ),
            ("Vitals", vitals(device, sample, "00", "0.1.0")),
        ]
    }

    /// The slice between two markers; the page tests count rows inside `<tbody>`.
    fn between<'a>(haystack: &'a str, start: &str, end: &str) -> &'a str {
        let from = haystack.find(start).expect("the start marker") + start.len();
        let to = haystack[from..].find(end).expect("the end marker") + from;
        &haystack[from..to]
    }

    #[test]
    fn every_page_renders_the_rail_and_marks_its_own_item() {
        let device = view('1', Some("studio"), true);
        let sample = sample_vitals();
        for (expected, markup) in every_page(&device, &sample) {
            let page = markup.into_string();
            assert!(
                page.contains("class=\"rail\""),
                "{expected}: the rail is missing: {page}"
            );
            assert!(
                page.matches("<a class=\"rail-item").count() == LABELS.len(),
                "{expected}: one anchor per rail page: {page}"
            );
            for label in LABELS {
                assert!(
                    page.contains(&format!("title=\"{label}\"")),
                    "{expected}: the {label} anchor is missing: {page}"
                );
            }
            assert_eq!(
                page.matches("aria-current=\"page\"").count(),
                1,
                "{expected}: exactly one rail item is current: {page}"
            );
            let item = Page::ALL
                .into_iter()
                .find(|page_kind| page_kind.label() == expected)
                .expect("the expected label names a rail page");
            let current = format!(
                "href=\"{}\" title=\"{expected}\" aria-label=\"{expected}\" aria-current=\"page\"",
                item.href(Some(&device))
            );
            assert!(
                page.contains(&current),
                "{expected}: its own anchor is the current one: {page}"
            );
            assert!(
                page.contains("class=\"statusbar\""),
                "{expected}: the status strip is missing: {page}"
            );
            assert!(
                page.contains("class=\"topbar\""),
                "{expected}: the top bar is missing: {page}"
            );
        }
    }

    #[test]
    fn the_fleet_renders_one_row_per_device_with_every_page_link() {
        let online = view('1', Some("studio"), true);
        let offline = view('2', None, false);
        let ids = [online.id.clone(), offline.id.clone()];
        let devices = [online, offline];
        let page = dashboard(&devices, "00", "0.1.0", None, None).into_string();

        assert!(
            page.contains("2 devices · 1 online"),
            "the status strip counts the fleet: {page}"
        );
        let rows = between(&page, "<tbody>", "</tbody>");
        assert_eq!(
            rows.matches("<tr>").count(),
            devices.len(),
            "one row per device: {rows}"
        );
        for id in &ids {
            assert!(rows.contains(id), "every device has a row: {rows}");
        }
        for label in [
            "Terminal", "Files", "Runs", "System", "State", "Memory", "Screen", "Monitors",
            "Browser", "Connect", "Vitals",
        ] {
            assert_eq!(
                rows.matches(&format!(">{label}</a>")).count(),
                devices.len(),
                "every row links {label}: {rows}"
            );
        }
        // The smoke test clicks `a:has-text("Files")`; the rail must not add a second text match.
        assert_eq!(page.matches(">Files</a>").count(), devices.len(), "{page}");
    }

    #[test]
    fn the_terminal_page_keeps_its_form_and_tabs() {
        let device = view('1', Some("studio"), true);
        let page = terminal(&device, "ab".repeat(32).as_str(), "0.1.0").into_string();

        for marker in [
            "id=\"terminal-form\"",
            "id=\"terminal-tabs\"",
            "class=\"tabs\"",
            "id=\"terminal-panes\"",
            "class=\"panes\"",
            "id=\"terminal-status\"",
            "src=\"/assets/xterm.js\"",
            "src=\"/assets/terminal.js\"",
        ] {
            assert!(
                page.contains(marker),
                "the terminal page keeps {marker}: {page}"
            );
        }
        assert!(
            page.contains(&format!(
                "id=\"terminal\" data-device=\"{}\" data-csrf=\"{}\"",
                device.id,
                "ab".repeat(32)
            )),
            "the host carries the device and the session's CSRF value: {page}"
        );
        for kind in ["pty", "ssh", "serial"] {
            assert!(
                page.contains(&format!("value=\"{kind}\"")),
                "the kind {kind} is offered: {page}"
            );
        }
        assert!(
            page.contains("data-kind=\"ssh\"") && page.contains("data-kind=\"serial\""),
            "the kind-specific fields stay hidden until chosen: {page}"
        );
    }

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
