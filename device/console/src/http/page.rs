//! The console's page: the desktop shell — an icon rail, a top bar, a scrolling canvas and a status
//! strip — and one renderer per surface. `maud` builds the HTML from the Rust that already owns the
//! data, and every interpolated value is escaped by construction.
//!
//! Two scripts exist: the terminal's glue (`assets/terminal.js`), and the approvals badge
//! (`assets/approvals.js`), the one poll, because a held call waits only ninety seconds and a person on
//! another page should see it without refreshing. Every other page is a
//! form and a link. That is why the shell carries no toggles: the pages are server-rendered, and a
//! control that only exists in the browser would be a second, weaker copy of a rule the console
//! already enforces.

use maud::{html, Markup, PreEscaped, DOCTYPE};
use summrise_ui::card_head;

use crate::connections::Connection;

use super::DeviceView;

/// The page's whole style: the console's tokens, the desktop shell, and the surfaces it frames.
///
/// Every colour is a token, and every token has a dark value: the operator reads this page on a
/// desktop that follows the system theme, and a surface that only got a light value would keep it in
/// the dark (`--panel` on `--ink` is the measured way that goes wrong).
/// The console's own component rules; the tokens they read from are [`summrise_ui::TOKENS`].
///
/// Both surfaces render `summrise_ui::style(...)`, so a colour can only be defined once.
const COMPONENTS: &str = r#"* { box-sizing:border-box; }
html { height:100%; }
body { margin:0; min-height:100%; background:var(--bg); color:var(--ink); font-family:var(--font); font-size:14px; line-height:1.5; -webkit-font-smoothing:antialiased; }
a { color:var(--accent); text-decoration:none; }
a:hover { text-decoration:underline; }
:focus-visible { outline:2px solid var(--accent); outline-offset:2px; }
::selection { background:var(--accent-soft); color:var(--ink); }
h1, h2, h3, p { margin:0; }
.mono { font-family:var(--mono); }
.muted { color:var(--muted); }
.small { font-size:12px; }
.skip-link { position:absolute; left:-9999px; top:0; z-index:10; padding:8px 12px; border-radius:0 0 var(--radius-xs) 0; background:var(--accent-solid); color:var(--accent-fg); font-weight:600; }
.skip-link:focus { left:0; }
.visually-hidden { position:absolute; width:1px; height:1px; margin:-1px; padding:0; overflow:hidden; clip:rect(0 0 0 0); white-space:nowrap; border:0; }

/* ---- the shell: dark icon rail, one column, a 28px strip at the foot ---- */
body.app { display:flex; height:100vh; overflow:hidden; }
.rail { flex:0 0 52px; display:flex; flex-direction:column; align-items:center; gap:4px; padding:10px 0 12px; background:var(--rail); border-right:1px solid var(--rail-line); overflow-y:auto; scrollbar-width:none; }
.rail::-webkit-scrollbar { display:none; }
.rail a { display:grid; place-items:center; flex:none; width:40px; height:40px; border-radius:var(--radius-sm); color:var(--rail-ink); }
.rail a:hover { background:rgba(255,255,255,.08); color:var(--rail-ink-hover); text-decoration:none; }
.rail a:focus-visible { outline-color:var(--rail-focus); }
.rail a.active, .rail a.active:hover { background:var(--accent-solid); color:#fff; }
.rail .brand { margin-bottom:10px; color:var(--rail-focus); }
.rail .brand:hover { background:transparent; color:#fff; }
.shell { flex:1; min-width:0; min-height:0; display:flex; flex-direction:column; height:100vh; }
.topbar .context .name { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.topbar .spacer { flex:1; }
.topbar form { flex:none; }
.content { flex:1; min-height:0; overflow:auto; padding:24px max(24px, calc((100% - 1180px) / 2)) 48px; }
.content > :first-child { margin-top:0; }
.content > :last-child { margin-bottom:0; }

/* ---- type and page furniture ---- */
.lead { max-width:82ch; margin:0 0 20px; color:var(--muted); font-size:13px; line-height:1.6; }
.lead code { color:var(--ink); }
.breadcrumb { display:flex; flex-wrap:wrap; align-items:center; gap:8px; margin:0 0 16px; color:var(--faint); font-size:12px; }
.breadcrumb a { color:var(--muted); }
.breadcrumb a:hover { color:var(--accent); }
.breadcrumb .sep { color:var(--faint); }
.breadcrumb .current { color:var(--ink); font-family:var(--mono); font-size:12px; }
.footnote { margin:24px 0 0; color:var(--faint); font-size:12px; line-height:1.7; }
.footnote code { color:var(--muted); word-break:break-all; }
.alert { display:flex; align-items:flex-start; gap:8px; margin:0 0 16px; padding:12px 16px; border:1px solid transparent; border-radius:var(--radius-sm); background:var(--panel); box-shadow:var(--shadow); color:var(--ink); font-size:13px; }
.alert.error { background:var(--danger-soft); color:var(--danger); }
.alert.notice { border-color:var(--line); background:var(--panel-2); color:var(--muted); }
.callout { display:flex; flex-wrap:wrap; align-items:baseline; gap:6px 10px; margin:0 0 16px; padding:12px 16px; border:1px solid color-mix(in srgb, var(--accent) 28%, var(--line)); border-radius:var(--radius-sm); background:var(--accent-soft); color:var(--ink); font-size:13px; }
.callout .label { color:var(--accent); font-size:11px; font-weight:600; letter-spacing:.08em; text-transform:uppercase; }
.callout strong { font-family:var(--mono); font-size:14px; letter-spacing:.03em; word-break:break-all; }
.callout .note { color:var(--muted); font-size:12px; }

/* ---- cards ---- */
.card-head .spacer { flex:1; }
.card-body .lead { margin-bottom:14px; }
.card-body .alert { box-shadow:none; }
.card > .table { border-radius:0 0 var(--radius) var(--radius); }
.card > .table tbody tr:last-child td:first-child { border-bottom-left-radius:calc(var(--radius) - 1px); }
.card > .table tbody tr:last-child td:last-child { border-bottom-right-radius:calc(var(--radius) - 1px); }
.card > pre:last-child { border-radius:0 0 var(--radius) var(--radius); }
.card > .empty:last-child { border-radius:0 0 var(--radius) var(--radius); }

/* ---- stats ---- */
.stats { display:grid; grid-template-columns:repeat(auto-fit, minmax(150px, 1fr)); gap:12px; margin:0 0 20px; }
.stat { min-width:0; padding:12px 16px; border:1px solid var(--line); border-radius:var(--radius-sm); background:var(--panel); box-shadow:var(--shadow); }
.stat .label { margin:0 0 6px; color:var(--muted); font-size:11px; font-weight:600; letter-spacing:.08em; text-transform:uppercase; }
.stat .value { overflow:hidden; color:var(--ink); font-size:15px; font-weight:600; line-height:1.35; text-overflow:ellipsis; white-space:nowrap; }
.stat .value.mono { font-size:13px; font-weight:500; }
.stat .sub { margin-top:2px; color:var(--muted); font-size:12px; }
.stat .sub .up { color:var(--ok); font-weight:600; }

/* ---- chips ---- */

/* ---- buttons ---- */
.btn { display:inline-flex; align-items:center; justify-content:center; gap:6px; min-height:30px; padding:0 12px; border:1px solid var(--line-strong); border-radius:var(--radius-xs); background:var(--panel); color:var(--ink); font:inherit; font-size:13px; font-weight:600; line-height:1; text-decoration:none; white-space:nowrap; cursor:pointer; }
.btn:hover { background:var(--panel-2); text-decoration:none; }
.btn:focus-visible { outline-offset:2px; }
/* A pressed button and a disabled one were the two states the sheet never drew: every control
   looked identical under the finger and while a request was in flight. */
.btn:active { filter:brightness(.96); }
.btn:disabled, .btn[aria-disabled="true"] { border-color:var(--line); background:var(--panel-2); color:var(--faint); cursor:not-allowed; box-shadow:none; }
.btn-danger:disabled, .btn-danger[aria-disabled="true"] { border-color:var(--line); background:var(--panel-2); color:var(--faint); }
.btn-primary { border-color:var(--accent-solid); background:var(--accent-solid); color:var(--accent-fg); }
.btn-primary:hover { border-color:var(--accent-hover); background:var(--accent-hover); }
.btn-ghost { border-color:transparent; background:transparent; color:var(--muted); }
.btn-ghost:hover { background:var(--panel-2); color:var(--ink); }
.btn-danger { border-color:var(--line-strong); background:var(--panel); color:var(--danger); }
.btn-danger:hover { border-color:var(--danger); background:var(--danger-soft); }
.btn-sm { min-height:26px; padding:0 8px; font-size:12px; }
.btn-block { width:100%; }
button { font:inherit; cursor:pointer; }
button.btn { font-family:inherit; }

/* ---- forms ---- */
form { margin:0; }
label { color:var(--muted); font-size:12px; font-weight:600; }
input, select, textarea { width:100%; min-height:34px; padding:6px 10px; border:1px solid var(--line-strong); border-radius:var(--radius-xs); background:var(--panel); color:var(--ink); font:inherit; font-size:13px; }
select { appearance:none; padding-right:30px; background-image:url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%238b8c96' stroke-width='2.2' stroke-linecap='round'%3E%3Cpath d='m6 9 6 6 6-6'/%3E%3C/svg%3E"); background-repeat:no-repeat; background-position:right 10px center; }
textarea { min-height:82px; font-family:var(--mono); font-size:13px; line-height:1.6; resize:vertical; }
input[type=checkbox] { width:auto; min-height:0; accent-color:var(--accent-solid); }
input:focus-visible, select:focus-visible, textarea:focus-visible { border-color:var(--accent); outline:2px solid var(--accent); outline-offset:1px; }
input::placeholder, textarea::placeholder { color:var(--faint); }
.field { margin:0 0 16px; }
.field > label, .field-label { display:block; margin:0 0 5px; }
.form-grid { display:grid; grid-template-columns:repeat(2, minmax(0, 1fr)); gap:0 16px; }
.form-grid .span-2 { grid-column:1 / -1; }
.form-grid .field { margin-bottom:16px; }
.form-actions { display:flex; flex-wrap:wrap; align-items:center; gap:10px; }
label.check { display:flex; align-items:center; gap:8px; min-height:24px; margin:0 0 14px; color:var(--ink); font-size:13px; font-weight:400; }
.form-inline { display:flex; flex-wrap:wrap; align-items:center; gap:8px; }
.form-inline .field { margin:0; }
.input-sm { min-height:28px; padding:3px 8px; font-size:12px; }
.filter-form { display:flex; flex-wrap:wrap; align-items:center; gap:8px; }
.filter-form input { min-height:28px; padding:3px 8px; font-size:12px; width:180px; }

/* ---- spacing: 4, 8, 12, 16, 24, 32. A new value that is not one of those is a mistake the
   review will find, not a taste a rule may invent. ---- */

/* ---- tables ---- */
.table { width:100%; border-collapse:collapse; background:var(--panel); font-size:13px; }
.table th { padding:8px 16px; border-bottom:1px solid var(--line); color:var(--muted); font-size:11px; font-weight:600; letter-spacing:.08em; text-align:left; text-transform:uppercase; white-space:nowrap; }
.table td { padding:12px 16px; border-bottom:1px solid var(--line); vertical-align:middle; }
.table tbody tr:last-child td { border-bottom:0; }
.table tbody tr:hover td { background:var(--panel-2); }
.table .num { text-align:right; font-variant-numeric:tabular-nums; white-space:nowrap; }
.table .nowrap { white-space:nowrap; }
.table .wrap { word-break:break-word; }
.table td .sub { margin-top:3px; color:var(--faint); font-size:12px; }
.table .row-actions { display:flex; justify-content:flex-end; }
.table .pid { width:84px; }

/* ---- bars ---- */
.bar { height:6px; overflow:hidden; border-radius:999px; background:var(--panel-3); }
.bar-fill { height:100%; border-radius:999px; background:var(--accent); }
.usage { display:grid; gap:8px; }
.usage-line { display:flex; align-items:baseline; justify-content:space-between; gap:12px; color:var(--muted); font-size:13px; }
.usage-line strong { color:var(--ink); font-size:13px; font-variant-numeric:tabular-nums; }
.usage-sub { margin-top:6px; color:var(--faint); font-size:12px; }
.usage-row { display:grid; grid-template-columns:minmax(120px, 1fr) minmax(140px, 2.2fr) minmax(120px, auto); align-items:center; gap:16px; padding:12px 0; border-bottom:1px solid var(--line); }
.usage-row:first-child { padding-top:0; }
.usage-row:last-child { padding-bottom:0; border-bottom:0; }
.usage-name { min-width:0; font-family:var(--mono); font-size:13px; }
.usage-name .usage-sub { display:block; margin-top:2px; font-family:var(--font); }
.usage-value { color:var(--muted); font-size:12px; text-align:right; font-variant-numeric:tabular-nums; white-space:nowrap; }
.usage-value strong { display:block; color:var(--ink); font-size:13px; }

/* ---- empty states and pre ---- */
pre, code { font-family:var(--mono); }
code { padding:1px 5px; border-radius:var(--radius-xs); background:var(--panel-2); color:var(--muted); font-size:13px; }
pre.file { margin:0; padding:16px 18px; border:0; background:var(--panel); color:var(--ink); font-family:var(--mono); font-size:13px; line-height:1.65; white-space:pre-wrap; word-break:break-word; overflow:auto; }
pre.out { margin:0; padding:14px 16px; background:#0f1014; color:#d9d5ce; font-family:var(--mono); font-size:12px; line-height:1.6; white-space:pre-wrap; word-break:break-word; overflow:auto; max-height:520px; }
pre.out.stderr { color:#ffb4ad; }
pre.secret { margin:0; padding:16px 18px; background:#0f1014; color:#ffd8c4; font-family:var(--mono); font-size:13px; line-height:1.6; letter-spacing:.02em; white-space:pre-wrap; word-break:break-all; }
.cmd { display:block; padding:10px 14px; border:1px solid var(--line); border-radius:var(--radius-xs); background:var(--panel-2); color:var(--ink); font-family:var(--mono); font-size:13px; line-height:1.6; white-space:pre-wrap; word-break:break-all; }

/* ---- toolbars ---- */
.toolbar { display:flex; flex-wrap:wrap; align-items:center; gap:10px; margin:0 0 16px; }
.toolbar .spacer { flex:1; }
.pathbar { display:flex; align-items:center; gap:8px; margin:0 0 12px; color:var(--muted); font-size:12px; }
.pathbar code { color:var(--ink); }
.pathbar .spacer { flex:1; }
.img-card { padding:16px; }
img.capture { display:block; max-width:100%; max-height:560px; margin:0 auto; border:1px solid var(--line); border-radius:var(--radius-xs); background:var(--panel-2); }
img.capture.thumb { max-height:360px; }

/* ---- terminal: the one canvas that fills instead of scrolling ---- */
body.terminal .content { display:flex; flex-direction:column; min-height:0; overflow:hidden; padding:16px max(20px, calc((100% - 1180px) / 2)) 18px; }
.body-row { flex:1; display:flex; min-height:0; }
.context-rail { flex:0 0 244px; display:flex; flex-direction:column; gap:10px; padding:16px; background:var(--panel-2); border-right:1px solid var(--line); overflow:auto; }
.rail-list { list-style:none; margin:0; padding:0; display:flex; flex-direction:column; gap:2px; }
.rail-list li { display:flex; flex-direction:column; gap:3px; padding:7px 8px; border-radius:var(--radius-xs); }
.status { font-variant-numeric: tabular-nums; font-weight:600; }
.status.ok { color:var(--ok); }
.status.warn { color:var(--warn); }
.status.error { color:var(--danger, var(--accent)); }
.rail-actions { display:flex; flex-direction:column; gap:6px; margin-top:auto; }
.rail-actions .btn { justify-content:center; }
.rail-meta { display:flex; align-items:center; gap:6px; font-size:12px; color:var(--faint); }
.rail-list li:hover { background:var(--hover); }
.rail-when { font-size:12px; color:var(--faint); }
.rail-line { font-size:12px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.rail-head { display:flex; align-items:center; justify-content:space-between; }
.rail-head h2 { margin:0; }
.rail-note { margin:0; color:var(--muted); font-size:13px; }
.session-list { list-style:none; margin:0; padding:0; display:flex; flex-direction:column; gap:4px; }
.session-list a { display:flex; align-items:center; gap:10px; padding:8px 9px; border-radius:var(--radius-xs); text-decoration:none; color:var(--ink); }
.session-list a:hover { background:var(--hover); }
.session-row { display:flex; align-items:center; gap:2px; }
.session-row > a { flex:1; min-width:0; }
.rail-close { display:grid; place-items:center; flex:none; width:24px; height:24px; padding:0; border:0; border-radius:var(--radius-xs); background:transparent; color:var(--faint); font-size:15px; line-height:1; cursor:pointer; }
.rail-close:hover { background:var(--danger-soft); color:var(--danger); }
.rail-close:focus-visible { outline:2px solid var(--accent); outline-offset:1px; }
.session-body { display:flex; flex-direction:column; min-width:0; }
.session-kind { flex:none; font-size:11px; text-transform:uppercase; letter-spacing:0.07em; color:var(--muted); background:var(--surface-chip); border-radius:var(--radius-xs); padding:2px 5px; }
.session-body code { font-size:13px; background:none; padding:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.session-age { font-size:12px; color:var(--faint); }
.rail-action { margin-top:auto; }
.terminal-card { flex:1; min-height:0; display:flex; flex-direction:column; overflow:hidden; border:1px solid var(--line); border-radius:var(--radius); background:var(--panel); box-shadow:var(--shadow); }
.terminal-toolbar { flex:none; display:flex; flex-wrap:wrap; align-items:center; gap:8px; padding:8px 10px; border-bottom:1px solid var(--line); background:var(--panel); }
.terminal-form { display:flex; flex-wrap:wrap; align-items:center; gap:8px; }
.terminal-form .toolbar-label { color:var(--muted); font-size:11px; font-weight:600; letter-spacing:.07em; text-transform:uppercase; }
.terminal-form select { width:auto; min-width:150px; min-height:28px; padding:3px 30px 3px 8px; font-size:12px; }
.terminal-form button { min-height:28px; }
.kind-fields { display:flex; flex-wrap:wrap; align-items:center; gap:8px 10px; }
.kind-fields[hidden] { display:none; }
.kind-fields .field { margin:0; }
.kind-fields label { display:block; margin:0 0 2px; font-size:11px; font-weight:600; letter-spacing:.06em; text-transform:uppercase; }
.kind-fields input { width:130px; min-height:28px; padding:3px 8px; font-size:12px; }
.kind-fields input#serial_port { width:170px; }
.preset-count { margin-left:2px; color:var(--faint); font-variant-numeric:tabular-nums; }
.preset-panel { width:min(340px, calc(100vw - 96px)); padding:12px; }
.approval-say { margin:0 0 6px; font-size:13px; }
.preset-list { list-style:none; margin:0 0 10px; padding:0; display:flex; flex-direction:column; }
.preset-row { display:flex; align-items:center; gap:8px; padding:6px 0; border-bottom:1px solid var(--line); }
.preset-row:last-child { border-bottom:0; }
.preset-ident { flex:1; min-width:0; display:flex; flex-direction:column; gap:1px; }
.preset-name { overflow:hidden; font-size:13px; font-weight:600; text-overflow:ellipsis; white-space:nowrap; }
.preset-target { overflow:hidden; color:var(--muted); font-family:var(--mono); font-size:11px; text-overflow:ellipsis; white-space:nowrap; }
.preset-actions { flex:none; display:flex; align-items:center; gap:4px; }
.preset-add { display:grid; gap:6px; margin-top:8px; padding-top:8px; border-top:1px solid var(--line); }
.preset-add .field { margin:0; }
.preset-add label { font-size:11px; }
.preset-add input, .preset-add select { min-height:28px; padding:3px 8px; font-size:12px; }
.preset-add .btn { justify-content:center; }
.terminal-toolbar .spacer { flex:1; }
.terminal-tabbar { flex:none; display:flex; align-items:center; gap:10px; min-height:0; padding:0 10px; border-bottom:1px solid var(--line); background:var(--panel-2); }
.terminal-tabbar:has(#terminal-tabs:empty):has(#terminal-status:empty) { display:none; }
.tabs { display:flex; flex:1; flex-wrap:wrap; align-items:center; gap:4px; }
.tabs:empty { display:none; }
#terminal-status { min-width:0; margin:0; padding:5px 0; overflow:hidden; color:var(--faint); font-family:var(--mono); font-size:11px; text-overflow:ellipsis; white-space:nowrap; }
.tab { display:inline-flex; align-items:center; gap:6px; width:auto; min-height:28px; margin:4px 0; padding:3px 8px; border:1px solid transparent; border-radius:var(--radius-xs); background:transparent; color:var(--muted); font-size:12px; }
.tab:hover { background:var(--panel); color:var(--ink); }
.tab.active { border-color:var(--line); background:var(--panel); color:var(--ink); box-shadow:0 1px 2px rgba(24,24,27,.05); }
.tab-item { display:inline-flex; align-items:center; gap:2px; margin:4px 0; }
.tab-item .tab { margin:0; }
/* A 24px target: the glyph is 14px, the hit area is not. */
.tab-close { display:inline-flex; align-items:center; justify-content:center; width:24px; height:24px; min-width:24px; min-height:24px; padding:0; border:0; border-radius:var(--radius-xs); background:transparent; color:var(--faint); font-size:14px; line-height:1; }
.tab-close:hover { color:var(--danger); background:var(--panel-2); }
.panes { position:relative; flex:1; min-height:0; display:flex; gap:0; padding:0; background:var(--term-canvas); }
.pane { flex:1; min-height:0; display:flex; overflow:hidden; }
.pane .xterm { padding:10px 12px; background:var(--term-canvas); }
.xterm .xterm-viewport { background-color:var(--term-canvas); }
.pane[hidden] { display:none; }
.terminal-empty { position:absolute; inset:0; display:flex; flex-direction:column; align-items:center; justify-content:center; gap:12px; color:var(--muted); font-size:13px; text-align:center; }
.terminal-empty .empty-icon { width:48px; height:48px; border-radius:var(--radius); background:var(--panel-2); color:var(--faint); }
.terminal-empty .empty-icon svg { width:22px; height:22px; }
.terminal-empty p { max-width:44ch; }
#terminal-panes:has(.pane) .terminal-empty { display:none; }

/* ---- sign-in ---- */
body.login { margin:0; background:var(--bg); }
/* A fixed shell, so the card is centred in the viewport whatever else ends up in <body>: a
   password manager's badge, a translation bar, or a browser that treats the body differently. */
.login-shell { position:fixed; inset:0; overflow:auto; display:grid; place-items:center; padding:24px; background:radial-gradient(760px 420px at 50% -8%, var(--accent-soft), transparent 72%), var(--bg); }
.login-card { margin:auto; width:min(100%, 400px); padding:32px; border:1px solid var(--line); border-radius:var(--radius-lg); background:var(--panel); box-shadow:0 1px 2px rgba(24,24,27,.04), 0 24px 60px -32px rgba(24,24,27,.4); }
.login-brand { display:grid; place-items:center; width:44px; height:44px; margin-bottom:20px; border-radius:var(--radius); background:var(--accent-solid); color:#fff; box-shadow:0 8px 20px -12px rgba(176,58,10,.8); }
.login-card h1 { margin:0 0 6px; font-size:22px; font-weight:600; letter-spacing:-.02em; }
.login-card h1 span { color:var(--muted); font-weight:500; }
.login-card p.small { margin:10px 0 16px; }
.login-card .lead { margin:0 0 22px; font-size:13px; }
.login-card .field { margin-bottom:16px; }
.login-card .btn { width:100%; min-height:36px; }
.login-card .alert { margin:16px 0 0; padding:8px 12px; box-shadow:none; }

/* ---- the fleet row: identity, page links, and one disclosure ---- */
.device-cell { min-width:250px; }
.device-identity { display:flex; flex-wrap:wrap; align-items:center; gap:8px; }
.device-name { font-size:15px; font-weight:600; letter-spacing:-.01em; }
.device-version { color:var(--faint); font-family:var(--mono); font-size:12px; }
.device-meta { display:flex; flex-wrap:wrap; align-items:center; gap:7px; margin-top:4px; color:var(--muted); font-size:12px; }
.device-id { padding:0; background:none; color:var(--muted); font-size:12px; }
.meta-sep { color:var(--faint); }
.pages-cell { width:100%; }
.page-links { display:flex; flex-wrap:wrap; align-items:center; gap:2px 0; }
.page-links a { display:inline-flex; align-items:center; min-height:24px; padding:2px 0; color:var(--muted); font-size:12px; font-weight:500; white-space:nowrap; }
.page-links a:hover { color:var(--accent); }
.page-links a + a::before { content:"·"; margin:0 7px; color:var(--faint); font-weight:400; }
/* The row's seventh-to-eleventh destinations, behind one disclosure: a menu that says "More" is
   quieter than five more words in a table cell, and `details` keeps it working without JavaScript. */
.page-links .more-pages { position:relative; display:inline-block; }
.page-links .page-more { display:inline-flex; align-items:center; min-height:24px; cursor:pointer; list-style:none; padding:2px 0; color:var(--muted); font-size:12px; font-weight:500; white-space:nowrap; }
.page-links .page-more::before { content:"·"; margin:0 7px; color:var(--faint); font-weight:400; }
.page-links .page-more::-webkit-details-marker { display:none; }
.page-links .page-more:hover, .page-links .more-pages[open] .page-more { color:var(--accent); }
.page-menu { left:0; right:auto; width:168px; display:flex; flex-direction:column; gap:1px; padding:6px; }
.page-menu a { padding:6px 9px; border-radius:var(--radius-xs); color:var(--muted); font-size:13px; font-weight:500; }
.page-menu a:hover { background:var(--panel-2); color:var(--ink); }
.actions-cell { width:1%; text-align:right; white-space:nowrap; }
.row-menu { position:relative; display:inline-block; }
.row-menu summary { cursor:pointer; list-style:none; }
.row-menu summary::-webkit-details-marker { display:none; }
.row-menu summary .caret { margin-left:2px; color:var(--faint); font-size:9px; }
.row-menu[open] summary { background:var(--panel-2); color:var(--ink); }
/* An open "Restart" disclosure keeps its danger colour: the open state must not repaint a
   destructive control neutral while it is armed. */
.row-menu[open] summary.btn-danger { border-color:var(--danger); background:var(--danger-soft); color:var(--danger); }
.menu-panel { position:absolute; right:0; top:calc(100% + 8px); z-index:20; width:236px; padding:12px; border:1px solid var(--line); border-radius:var(--radius-xs); background:var(--panel); box-shadow:0 1px 2px rgba(24,24,27,.06), 0 18px 40px -20px rgba(24,24,27,.5); text-align:left; white-space:normal; }
.menu-panel form + form { margin-top:8px; padding-top:8px; border-top:1px solid var(--line); }
.menu-panel label { display:block; margin:0 0 4px; color:var(--muted); font-size:11px; font-weight:600; letter-spacing:.06em; text-transform:uppercase; }
.menu-panel input { min-height:28px; padding:3px 8px; font-size:12px; }
.menu-panel .field { margin:0 0 7px; }
.restart-panel { width:300px; padding:12px; }
.trend-row { display:grid; grid-template-columns:120px 1fr 210px; gap:16px; align-items:center; margin:0 0 16px; }
.trend-row:last-of-type { margin-bottom:6px; }
.trend-label { display:flex; flex-direction:column; gap:2px; font-family:var(--font); font-size:12px; font-weight:600; line-height:1.4; }
.trend-values { color:var(--muted); font-size:12px; text-align:right; }
.badge-approvals { display:inline-flex; align-items:center; height:24px; padding:0 9px; border-radius:999px; background:var(--danger-soft); color:var(--danger); font-family:var(--font); font-size:12px; font-weight:600; line-height:1; text-decoration:none; white-space:nowrap; }
.badge-approvals:hover { background:var(--danger); color:var(--panel); }
/* The hidden attribute loses to a display rule; without this the empty pill is visible on every
   page until the poll answers. */
.badge-approvals[hidden] { display:none; }
.spark { display:block; width:100%; height:28px; }
.spark polyline { fill:none; stroke:var(--accent); stroke-width:1.5; vector-effect:non-scaling-stroke; stroke-linejoin:round; }
.facts { display:grid; grid-template-columns:max-content 1fr; gap:6px 18px; margin:0 0 12px; }
.facts dt { color:var(--muted); font-family:var(--font); font-size:11px; line-height:1.6; text-transform:uppercase; letter-spacing:.04em; padding-top:2px; }
.facts dd { margin:0; font-family:var(--mono); font-size:12px; line-height:1.6; overflow-wrap:anywhere; }
.code-block { margin:0 0 12px; padding:12px 16px; border:1px solid var(--line); border-radius:var(--radius-xs); background:var(--panel-2); color:var(--ink); font-family:var(--mono); font-size:12px; line-height:1.5; white-space:pre-wrap; overflow-wrap:anywhere; max-height:280px; overflow:auto; }
.restart-panel p { margin:0 0 10px; }

/* ---- small additions to the component set ---- */
.btn-quiet-danger { border-color:transparent; background:transparent; color:var(--muted); }
.btn-quiet-danger:hover { border-color:transparent; background:var(--danger-soft); color:var(--danger); }
.form-inline .grow { flex:1 1 260px; min-width:0; }
.file-name { display:inline-flex; align-items:center; min-height:24px; font-weight:500; }
.cmd-line { white-space:pre-wrap; word-break:break-all; }
.url { word-break:break-all; }
.mini-usage { display:flex; align-items:center; gap:8px; min-width:104px; font-size:12px; }
.mini-bar { display:block; flex:0 0 72px; height:4px; overflow:hidden; border-radius:999px; background:var(--panel-3); }
.mini-bar .bar-fill { display:block; }
.endpoint { display:flex; flex-wrap:wrap; align-items:baseline; gap:8px 10px; margin:0 0 16px; padding:12px 16px; border:1px solid var(--line); border-radius:var(--radius-xs); background:var(--panel-2); font-size:12px; }
.endpoint .label { color:var(--muted); font-size:11px; font-weight:600; letter-spacing:.07em; text-transform:uppercase; }
.endpoint code { background:var(--panel); word-break:break-all; }

@media (max-width: 1180px) {
  /* Below the width where the row of page links fits on one line, the separators would lead a
     wrapped line: drop them and let the spacing do the separating. */
  .page-links { gap:4px 12px; }
  .page-links a + a::before { content:none; }
  .page-links .page-more::before { content:none; }
}
@media (max-width: 1000px) {
  /* The canvas keeps its width first: the context rail is context, not navigation, so it is the
     first thing to go when the window cannot hold all three columns. */
  .context-rail { display:none; }
}
@media (max-width: 860px) {
  .rail { flex:0 0 46px; }
  .rail a { width:34px; height:34px; border-radius:var(--radius-xs); }
  .content { padding:16px 16px 32px; }
  /* A table wider than the phone scrolls inside its own card rather than pushing the page. */
  .card { overflow-x:auto; }
  .table th, .table td { padding:8px 10px; }
}
@media (min-width: 721px) and (max-width: 1000px) {
  .stats { grid-template-columns:repeat(3, minmax(0, 1fr)); }
}
@media (max-width: 720px) {
  .topbar { height:52px; padding:0 12px; gap:8px; }
  .topbar .context { display:none; }
  .content { padding:16px 14px 32px; }
  .stats { grid-template-columns:repeat(2, minmax(0, 1fr)); }
  .form-grid { grid-template-columns:1fr; }
  .usage-row { grid-template-columns:1fr; gap:8px; }
  .card-head { flex-wrap:wrap; }
  /* The filter row was a flex row of five controls; at 390px every select collapsed to its
     caret, because the global `select { width:100% }` meets a shrinking flex item. Two columns
     of full-width fields is what a person can actually use. */
  .filter-form { display:grid; grid-template-columns:1fr 1fr; gap:10px 12px; align-items:end; }
  .filter-form .field { min-width:0; margin:0; }
  .filter-form input, .filter-form select { width:100%; }
  .filter-form .form-actions { grid-column:1 / -1; display:flex; align-items:center; gap:10px; }
  /* The trend row needs 362px before its first break; a 400px phone has 292. It stacks. */
  .trend-row { grid-template-columns:1fr; gap:6px; }
  .trend-row .trend-scale { justify-self:start; }
  .statusbar .side { display:none; }
  /* **A DENSE TABLE SCROLLS BEFORE IT CLIPS.** The fleet, approvals and activity tables become row
     cards; the others (tokens, monitors, memory, runs) keep their columns and scroll inside the card
     rather than running past the phone's edge. */
  .table { display:block; overflow-x:auto; }
  .activity-table thead { position:absolute; width:1px; height:1px; margin:-1px; padding:0; overflow:hidden; clip:rect(0 0 0 0); white-space:nowrap; border:0; }
  .activity-table, .activity-table tbody, .activity-table tr, .activity-table td { display:block; width:auto; }
  .activity-table tr { padding:8px 0; border-bottom:1px solid var(--line); }
  .activity-table tr:last-child { border-bottom:0; }
  .activity-table td { display:inline-block; border:0; padding:2px 8px 2px 0; }
  /* A token row is a name, two facts and the one action that matters; at 390 the five columns ran
     past the card and the Revoke button was the part you could not see. */
  .tokens-table thead { position:absolute; width:1px; height:1px; margin:-1px; padding:0; overflow:hidden; clip:rect(0 0 0 0); white-space:nowrap; border:0; }
  .tokens-table, .tokens-table tbody, .tokens-table tr { display:block; width:auto; }
  .tokens-table tr { padding:10px 0; border-bottom:1px solid var(--line); }
  .tokens-table tr:last-child { border-bottom:0; }
  .tokens-table td { display:inline-block; border:0; padding:2px 10px 2px 0; }
  .tokens-table td:first-child { display:block; font-weight:600; }
  .tokens-table td:last-child { padding-right:0; }
  /* Six columns cannot fit 390; the row becomes one wrapped line per record, which keeps the tool
     and the detail readable instead of scrolling for them. */
  .logs-table thead { position:absolute; width:1px; height:1px; margin:-1px; padding:0; overflow:hidden; clip:rect(0 0 0 0); white-space:nowrap; border:0; }
  .logs-table, .logs-table tbody, .logs-table tr { display:block; width:auto; }
  .logs-table tr { padding:8px 0; border-bottom:1px solid var(--line); }
  .logs-table tr:last-child { border-bottom:0; }
  .logs-table td { display:inline-block; border:0; padding:2px 8px 2px 0; }
  .logs-table td.wrap { display:block; width:100%; color:var(--muted); }
  /* One row-card rule for the dense tables that have no bespoke layout: files, runs, processes,
     memory, monitors and the browser's targets. The first cell is the row's subject and gets its
     own line; the rest flow and wrap. */
  .reflow thead { position:absolute; width:1px; height:1px; margin:-1px; padding:0; overflow:hidden; clip:rect(0 0 0 0); white-space:nowrap; border:0; }
  .reflow, .reflow tbody, .reflow tr { display:block; width:auto; }
  .reflow tr { padding:8px 0; border-bottom:1px solid var(--line); }
  .reflow tr:last-child { border-bottom:0; }
  .reflow td { display:inline-block; max-width:100%; border:0; padding:2px 8px 2px 0; }
  .reflow td:first-child { display:block; }
  .reflow td.wrap { display:block; width:100%; color:var(--muted); }
  /* Two tables end in bare numbers; with the header hidden, the number needs its word. */
  .runs-table td:nth-child(3)::before { content:"took "; color:var(--faint); }
  .runs-table td:nth-child(4)::before { content:"exit "; color:var(--faint); }
  .monitors-table td:nth-child(4)::before { content:"drops "; color:var(--faint); }
  .monitors-table td:nth-child(5)::before { content:"last "; color:var(--faint); }
  /* **A ROW IS NOT A TABLE ON A PHONE.** At 390px the pages column was squeezed to forty pixels —
     four links stacked vertically — and the row menu was cut off past the right edge. The row
     becomes a card: identity, then the pages on one wrapping line, then the menu. */
  /* Visually hidden, not removed: a screen reader still hears the column names. */
  .fleet-table thead { position:absolute; width:1px; height:1px; margin:-1px; padding:0; overflow:hidden; clip:rect(0 0 0 0); white-space:nowrap; border:0; }
  .fleet-table, .fleet-table tbody, .fleet-table tr, .fleet-table td { display:block; width:auto; }
  .fleet-table tr { padding:12px 0; border-bottom:1px solid var(--line); }
  .fleet-table tr:last-child { border-bottom:0; }
  .fleet-table td { border:0; padding:2px 0; }
  .fleet-table td.pages-cell { padding-top:8px; }
  .fleet-table td.actions-cell { padding-top:4px; }
  .fleet-note { margin:12px 0 4px; }
  /* The same reflow for the approval queue: the arguments block is wider than a phone, and clipped
     JSON is the one thing an approver must be able to read in full. */
  .approvals-table thead { position:absolute; width:1px; height:1px; margin:-1px; padding:0; overflow:hidden; clip:rect(0 0 0 0); white-space:nowrap; border:0; }
  .approvals-table, .approvals-table tbody, .approvals-table tr, .approvals-table td { display:block; width:auto; }
  .approvals-table tr { padding:12px 0; border-bottom:1px solid var(--line); }
  .approvals-table tr:last-child { border-bottom:0; }
  .approvals-table td { border:0; padding:2px 0; }
  .approvals-table td + td { padding-top:6px; }
  .approvals-table pre.code-block { max-height:none; }
  body.terminal .content { padding:10px; }
  .kind-fields input { width:120px; }
}
"#;

/// The console's whole stylesheet: the shared tokens, then its own component rules.
fn style_sheet() -> String {
    summrise_ui::style(COMPONENTS)
}

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
            Page::Tokens => "Connect",
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
///
/// **THE DEVICE PAGES APPEAR WITH A DEVICE.** Their hrefs need a device id, so on the fleet every one
/// of them would fall back to `/next` — ten icons that all navigate back to the page the reader is
/// already on. The fleet therefore lists its own entry, and choosing a device turns the rail into the
/// device's pages. This is progressive disclosure with a dead link removed, not a shorter menu.
fn rail(active: Page, device: Option<&DeviceView>) -> Markup {
    html! {
        nav class="rail" aria-label="Pages" {
            a class="brand" href="/next" title="Summrise console" aria-label="Summrise console" {
                (brand_icon())
            }
            @if device.is_some() {
                @for page in Page::ALL {
                    (rail_item(page, active, device))
                }
            } @else {
                (rail_item(Page::Fleet, active, device))
            }
        }
    }
}

/// A file length a person reads: bytes, KiB, MiB, GiB.
fn human_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let size = bytes as f64;
    if size >= KB * KB * KB {
        format!("{:.1} GiB", size / (KB * KB * KB))
    } else if size >= KB * KB {
        format!("{:.1} MiB", size / (KB * KB))
    } else if size >= KB {
        format!("{:.1} KiB", size / KB)
    } else {
        format!("{bytes} B")
    }
}

/// A memory figure a person reads: kilobytes under 1 MiB, MiB under 1 GiB, GiB above it.
fn human_kb(kb: u64) -> String {
    let bytes = kb.saturating_mul(1024);
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{kb} KB")
    }
}

/// How an uptime share reads: a watch that never misses is green, one that misses now and then is
/// amber, and anything under 95% is a target that is not being watched so much as hoped for.
fn uptime_tone(pct: u64) -> &'static str {
    if pct >= 99 {
        "reachable"
    } else if pct >= 95 {
        "degraded"
    } else {
        "unstable"
    }
}

fn uptime_class(pct: u64) -> &'static str {
    if pct >= 99 {
        "ok"
    } else if pct >= 95 {
        "warn"
    } else {
        "error"
    }
}

/// How long ago, in the unit a person would say: "just now", "6m ago", "3h ago", "2d ago".
///
/// `then` of 0 is a device that has never been seen, which is not the same as "56 years ago".
fn ago(now_ms: u64, then_ms: u64) -> String {
    if then_ms == 0 {
        return "never seen".to_owned();
    }
    if then_ms >= now_ms {
        return "just now".to_owned();
    }
    let seconds = (now_ms - then_ms) / 1000;
    match seconds {
        0..=59 => "just now".to_owned(),
        60..=3599 => format!("{}m ago", seconds / 60),
        3600..=86_399 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

/// The short id the fleet shows: the first eight characters, never a wall of hex.
fn short_id(id: &str) -> String {
    if let Some((index, _)) = id.char_indices().nth(8) {
        format!("{}…", &id[..index])
    } else {
        id.to_owned()
    }
}

/// The device the page acts on, as a person reads it: the name when the operator gave one, and the
/// short id otherwise (the whole id stays in the links and the top bar's tooltip).
fn device_label(device: &DeviceView) -> String {
    device.name.clone().unwrap_or_else(|| short_id(&device.id))
}

/// The name a fleet row leads with. A device nobody named is not a hex string to every reader.
fn device_name(device: &DeviceView) -> String {
    device
        .name
        .clone()
        .unwrap_or_else(|| "Unnamed device".to_owned())
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

/// Every kind the console writes, newest subsystems included. The list is what the audit filter
/// offers, and it was written before approvals, installs and the log's own events existed: a line
/// with a kind missing here could be found by text but never selected.
pub(super) const AUDIT_KINDS: [&str; 12] = [
    "approval",
    "auth",
    "denied",
    "device",
    "device_remove",
    "enroll",
    "grant",
    "mcp",
    "register",
    "token",
    "token_revoke",
    "tool",
];

/// The `head` every page shares. `extra` is where the terminal puts its stylesheet and its scripts.
fn head(title: &str, extra: Markup) -> Markup {
    html! {
        head {
            meta charset="utf-8";
            meta name="viewport" content="width=device-width, initial-scale=1";
            title { (title) " — Summrise console" }
            (extra)
            script src="/assets/approvals.js" defer {}
            style { (PreEscaped(style_sheet())) }
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

/// One status chip: the dot is decoration, the word is what a reader (and a screen reader) gets.
fn status_chip(online: bool) -> Markup {
    if online {
        html! { span class="chip online" { span class="dot" aria-hidden="true" {} "online" } }
    } else {
        html! { span class="chip offline" { span class="dot" aria-hidden="true" {} "offline" } }
    }
}

/// The top bar: where this page is, which device it acts on, and the way out.
fn topbar(title: &str, device: Option<&DeviceView>, csrf: &str) -> Markup {
    html! {
        header class="topbar" {
            h1 id="page-title" { (title) }
            @if let Some(device) = device {
                span class=(if device.online { "context" } else { "context offline" }) title=(device.id) {
                    span class="dot" aria-hidden="true" {}
                    span class="name" { (device_label(device)) }
                }
            }
            span class="spacer" {}
            a class="badge-approvals" id="approvals-badge" href="/next" hidden {}
            // **AN ANCHOR IS NOT A LIVE REGION.** `role="status"` on the link removed its link role,
            // and a region that starts hidden may not announce its first change. The announcement
            // lives in an always-present hidden span instead.
            span id="approvals-announce" class="visually-hidden" role="status" aria-live="polite" {}
            // The console's build is in the status strip. Printing it here as well was the same
            // fact twice on one screen; the strip is where chrome facts belong.
            form method="post" action="/next/logout" {
                input type="hidden" name="csrf" value=(csrf);
                button type="submit" class="btn btn-ghost btn-sm" { "Sign out" }
            }
        }
    }
}

/// The 28px strip at the foot: the build, the page's one fact, and which surface this is.
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
    page_with_rail(chrome, extra, Markup::default(), content)
}

/// The page, with a context rail between the icon rail and the canvas.
///
/// The icon rail is navigation and the context rail is context — the sessions on a machine, the
/// runs that already happened, the processes that are using the memory. A page with nothing worth
/// showing there passes an empty rail and the canvas takes the whole width.
fn page_with_rail(chrome: Chrome<'_>, extra: Markup, context: Markup, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            (head(chrome.title, extra))
            body class=(body_class(chrome.active)) {
                // The first Tab lands here: three rails and a table are a lot to walk past.
                a class="skip-link" href="#content" { "Skip to content" }
                (rail(chrome.active, chrome.device))
                div class="shell" {
                    (topbar(chrome.title, chrome.device, chrome.csrf))
                    div class="body-row" {
                        (context)
                        main id="content" class="content" tabindex="-1" aria-labelledby="page-title" { (content) }
                    }
                    (statusbar(chrome.version, chrome.status))
                }
            }
        }
    }
}

/// The context rail: one heading and what this page has to show beside its canvas.
fn context_rail(heading: &str, meta: Markup, body: Markup, action: Markup) -> Markup {
    html! {
        aside class="context-rail" aria-label=(heading) {
            div class="rail-head" {
                h2 { (heading) }
                (meta)
            }
            (body)
            (action)
        }
    }
}

/// The one empty state: a muted icon, a sentence, and at most one action.
fn empty_state(icon: Page, message: &str, action: Markup) -> Markup {
    summrise_ui::empty_state(icon.icon(), message, action)
}

/// A breadcrumb: a short trail of links and the name of the thing being read.
fn breadcrumb(trail: &[(String, String)], current: &str) -> Markup {
    html! {
        nav class="breadcrumb" aria-label="Breadcrumb" {
            @for (label, href) in trail {
                a href=(href) { (label) }
                span class="sep" aria-hidden="true" { "/" }
            }
            span class="current" { (current) }
        }
    }
}

/// The sign-in card. `note` explains why the card appeared (the edge's answer); `error` is the one
/// line a refused token or a spent link gets.
pub(super) fn login(note: Option<&str>, error: Option<&str>) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Summrise console" }
                style { (PreEscaped(style_sheet())) }
            }
            body class="login" {
                // The shell owns the centring, so a node another script adds to <body> cannot move
                // the card out from under the operator.
                div class="login-shell" {
                    main class="login-card" {
                    div class="login-brand" aria-hidden="true" { (brand_icon()) }
                    h1 { "Summrise " span { "console" } }
                    p class="lead" {
                        "The operator token opens a session for this browser. It is not stored by the page."
                    }
                    p class="small muted" {
                        "The token is the 64-hex secret in the file the console's "
                        code { "--operator-token-file" } " names, on the console host. With access to "
                        "that host, " code { "summrise-console --login-link <url> --data <dir>" }
                        " prints a two-minute link instead."
                    }
                        @if let Some(note) = note {
                            p class="alert notice" { (note) }
                        }
                    form method="post" action="/next/auth" {
                        div class="field" {
                            label for="token" { "Operator token" }
                            input id="token" name="token" type="password" autocomplete="off" required;
                        }
                        button type="submit" class="btn btn-primary" { "Sign in" }
                    }
                        @if let Some(error) = error {
                            p class="alert error" role="alert" { (error) }
                        }
                    }
                }
            }
        }
    }
}

/// A wrong address, answered in the product's own clothes.
///
/// Every unmatched path used to be an empty `404` with no link, no brand and no way back — the one
/// screen in the console that did not look like the console. A mistyped URL and a link to a device
/// that was removed both land here.
pub(super) fn not_found(version: &str) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Not here — Summrise console" }
                style { (PreEscaped(style_sheet())) }
            }
            body class="login" {
                div class="login-shell" {
                    main class="login-card" {
                        div class="login-brand" aria-hidden="true" { (brand_icon()) }
                        h1 { "Not here" }
                        p class="lead" {
                            "That address is not part of this console. A link from an older build "
                            "and a device that was removed both land here."
                        }
                        p { a class="btn btn-primary" href="/next" { "Back to the fleet" } }
                        p class="small muted" { "Summrise console " (version) }
                    }
                }
            }
        }
    }
}

/// The signed-in page: the fleet, its summary, and the phase-2 mutations. Each device is one row:
/// who it is, the pages that act on it, and the two mutations folded into a disclosure.
/// The optional lines the dashboard can carry, grouped so the renderer stays inside the argument
/// limit clippy enforces: who signed the operator in, the enrollment code `POST /next/enroll` just
/// minted, and the error a refused mutation left in the query.
#[derive(Default)]
pub(super) struct Notes<'a> {
    pub signer: Option<&'a str>,
    pub enroll: Option<&'a str>,
    pub error: Option<&'a str>,
    /// What the last approve/refuse did, announced rather than inferred.
    pub notice: Option<&'a str>,
    /// The address the operator reached this console on. A new device must be able to reach the same
    /// one, so the enrollment card prints it instead of making the reader guess.
    pub origin: Option<&'a str>,
}

/// A destructive call in one sentence, for the person deciding it.
///
/// The approval card showed only raw JSON. Ninety seconds is not enough time to parse a nested
/// object on a phone, and the operator is being asked to accept a consequence, not to review a
/// payload — the JSON stays underneath as the evidence. `None` means "this tool has no summary",
/// which is honest; the JSON is still there.
fn approval_sentence(tool: &str, arguments: &str) -> Option<String> {
    fn cut(text: &str, limit: usize) -> String {
        if text.chars().count() <= limit {
            return text.to_owned();
        }
        let mut short: String = text.chars().take(limit.saturating_sub(1)).collect();
        short.push('…');
        short
    }
    let value: serde_json::Value = serde_json::from_str(arguments).ok()?;
    let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_owned);
    let number = |key: &str| value.get(key).and_then(|v| v.as_u64());
    Some(match tool {
        "terminal.create" => match text("kind").unwrap_or_default().as_str() {
            "ssh" => {
                let host = text("host").unwrap_or_else(|| "(no host)".to_owned());
                let port = number("port").unwrap_or(22);
                match text("user") {
                    Some(user) => format!("Open an ssh session as {user}@{host}:{port}."),
                    None => format!("Open an ssh session to {host}:{port}."),
                }
            }
            "serial" => {
                let port = text("port").unwrap_or_else(|| "(no port)".to_owned());
                let baud = number("baud").unwrap_or(115200);
                format!("Open the serial port {port} at {baud} baud.")
            }
            _ => "Start a pty session: a shell on the device.".to_owned(),
        },
        "terminal.write" => {
            let id = text("id").unwrap_or_else(|| "?".to_owned());
            let bytes = text("data")
                .map(|data| data.len())
                .or_else(|| text("data_base64").map(|data| data.len()))
                .unwrap_or(0);
            format!("Type {bytes} bytes into session {id}.")
        }
        "files.write" => {
            let path = text("path").unwrap_or_else(|| "?".to_owned());
            let bytes = text("content").map(|content| content.len()).unwrap_or(0);
            format!("Write {bytes} bytes to {path}.")
        }
        "files.delete" => format!("Delete {}.", text("path").unwrap_or_else(|| "?".to_owned())),
        "memory.append" => format!(
            "Remember \u{201c}{}\u{201d}.",
            cut(&text("text").unwrap_or_default(), 60)
        ),
        "memory.update" => format!(
            "Rewrite memory entry {}.",
            text("id").unwrap_or_else(|| "?".to_owned())
        ),
        "memory.delete" => format!(
            "Forget memory entry {}.",
            text("id").unwrap_or_else(|| "?".to_owned())
        ),
        "browser.open" => format!(
            "Open {} in the device's browser.",
            text("url").unwrap_or_else(|| "?".to_owned())
        ),
        "browser.navigate" => format!(
            "Navigate the device's browser to {}.",
            text("url").unwrap_or_else(|| "?".to_owned())
        ),
        "browser.eval" => format!(
            "Run JavaScript in the device's browser: {}",
            cut(&text("expression").unwrap_or_default(), 60)
        ),
        "browser.close" => format!(
            "Close {}.",
            text("target")
                .or_else(|| text("id"))
                .unwrap_or_else(|| "the page".to_owned())
        ),
        "runs.exec" => {
            let command = text("command").unwrap_or_else(|| "?".to_owned());
            let args = value
                .get("args")
                .and_then(|v| v.as_array())
                .map(|args| {
                    args.iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            let line = format!("{command} {args}").trim().to_owned();
            match text("cwd") {
                Some(cwd) => format!("Run {line} in {cwd}."),
                None => format!("Run {line}."),
            }
        }
        "system.process_kill" => {
            if let Some(name) = text("name") {
                format!("End every process named {name}.")
            } else if let Some(pid) = number("pid") {
                format!("End process {pid}.")
            } else {
                "End a process.".to_owned()
            }
        }
        "monitor.remove" => format!(
            "Stop watching {}.",
            text("id").unwrap_or_else(|| "?".to_owned())
        ),
        _ => return None,
    })
}

/// One call a device refused for lack of destructive consent, waiting for the operator.
#[derive(Clone)]
pub(super) struct ApprovalRow {
    pub(super) id: String,
    pub(super) device_id: String,
    pub(super) tool: String,
    pub(super) arguments: String,
    pub(super) created_ms: u64,
}

pub(super) fn dashboard(
    devices: &[DeviceView],
    csrf: &str,
    version: &str,
    notes: Notes<'_>,
    recent: &[serde_json::Value],
    approvals: &[ApprovalRow],
    now_ms: u64,
) -> Markup {
    let total = devices.len();
    let online = devices.iter().filter(|device| device.online).count();
    let enroll = html! {
        form method="post" action="/next/enroll" {
            input type="hidden" name="csrf" value=(csrf);
            button type="submit" class="btn btn-primary" { "New enrollment code" }
        }
    };
    let content = html! {
        @if !approvals.is_empty() {
            section class="card" {
                (card_head(
                    "Waiting for your approval",
                    html! { span class="chip failed" { (approvals.len()) } },
                    Markup::default(),
                ))
                div class="card-body" {
                    p class="lead" {
                        "A client asked for a destructive call its credential did not allow. The call "
                        "is held here — the request is still open — and an unanswered question is "
                        "refused after ninety seconds, never run."
                    }
                    table class="table approvals-table" {
                        caption class="visually-hidden" { "Calls waiting for an answer." }
                        thead {
                            tr {
                                th scope="col" { "Device" }
                                th scope="col" { "Tool" }
                                th scope="col" { "Arguments" }
                                th scope="col" { "Waiting since" }
                                th class="num" scope="col" { span class="visually-hidden" { "Actions" } }
                            }
                        }
                        tbody {
                            @for row in approvals {
                                tr {
                                    td {
                                        @match devices.iter().find(|device| device.id == row.device_id) {
                                            Some(device) => {
                                                strong class="device-name" { (device_label(device)) }
                                                " "
                                                code class="device-id" title=(row.device_id) { (row.device_id.chars().take(12).collect::<String>()) }
                                            }
                                            None => {
                                                code title=(row.device_id) { (row.device_id.chars().take(12).collect::<String>()) }
                                            }
                                        }
                                    }
                                    td { code { (row.tool) } }
                                    td class="wrap" {
                                        @if let Some(sentence) = approval_sentence(&row.tool, &row.arguments) {
                                            p class="approval-say" { (sentence) }
                                        }
                                        pre class="code-block" { (row.arguments) }
                                    }
                                    td class="nowrap" { (stamp(row.created_ms)) }
                                    td class="num" {
                                        div class="form-inline" {
                                            // **"APPROVE" WITH NO CONTEXT IS THE WRONG BUTTON.** A screen
                                            // reader hears "Approve, button" once per row; the label
                                            // names the tool and the device.
                                            form method="post" action=(format!("/next/approvals/{}/approve", encode(&row.id))) {
                                                input type="hidden" name="csrf" value=(csrf);
                                                button type="submit" class="btn btn-primary btn-sm" aria-label=(format!("Approve {} for {}", row.tool, short_id(&row.device_id))) { "Approve" }
                                            }
                                            form method="post" action=(format!("/next/approvals/{}/refuse", encode(&row.id))) {
                                                input type="hidden" name="csrf" value=(csrf);
                                                button type="submit" class="btn btn-quiet-danger btn-sm" aria-label=(format!("Refuse {} for {}", row.tool, short_id(&row.device_id))) { "Refuse" }
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
        p class="lead" {
            "Every device this console has enrolled, and the pages that act on it."
        }
        // Who opened this page. An operator who was shown a token box for a secret they never had
        // deserves to see that Cloudflare Access, not a token, signed them in.
        @match notes.signer {
            Some(email) => p class="small muted" {
                "Signed in through Cloudflare Access as " strong { (email) } "."
            },
            None => p class="small muted" { "Signed in with this browser's session." },
        }
        @if let Some(code) = notes.enroll {
            div class="callout" role="status" {
                p {
                    span class="label" { "Enrollment code:" }
                    strong { (code) }
                    span class="note" { " · single use, valid for 15 minutes" }
                }
                p class="small muted" {
                    "Spend it at the machine: open " code { "http://127.0.0.1:18080/" } ", choose "
                    "Console link → Enrol this machine, and paste the code. Or, from an elevated "
                    "prompt on that machine:"
                }
                pre class="code-block" { "summrise-device.exe --enroll " (notes.origin.unwrap_or("<console-url>")) " " (code) " --data C:\\ProgramData\\Summrise" }
                @if let Some(origin) = notes.origin {
                    @if origin.contains("127.0.0.1") || origin.contains("localhost") {
                        p class="small muted" {
                            "That address only reaches this machine. A device on another machine "
                            "needs the console's own hostname instead — open the console on that "
                            "name and mint the code again."
                        }
                    }
                }
                p class="small muted" {
                    "The first enrollment spends the code; if the window closes, mint another."
                }
            }
        }
        @if let Some(error) = notes.error {
            p class="alert error" { "That change was not applied: " (error) }
        }
        @if let Some(notice) = notes.notice {
            p class="alert notice" role="status" { (notice) }
        }
        @if !devices.is_empty() {
            div class="stats" {
                div class="stat" {
                    p class="label" { "Devices" }
                    p class="value" { (total) }
                }
                div class="stat" {
                    p class="label" { "Online" }
                    p class="value" { (online) }
                }
                div class="stat" {
                    p class="label" { "Offline" }
                    p class="value" { (total - online) }
                }
            }
        }
        section class="card" {
            (card_head(
                "Devices",
                html! { span class="chip muted" { (total) } },
                if devices.is_empty() { Markup::default() } else { enroll.clone() },
            ))
            @if devices.is_empty() {
                (empty_state(
                    Page::Fleet,
                    "No device has enrolled yet. Mint an enrollment code and spend it at the machine — \
                     the card shows the exact command.",
                    enroll,
                ))
            } @else {
                @if devices.iter().any(|device| !device.online && device.version.is_none() && device.last_attempt.is_none()) {
                    // The CLI enrollment writes the link, but the running service read it at boot.
                    // Without this line the first row an operator ever sees says "offline" and
                    // nothing tells them the one command that fixes it.
                    p class="fleet-note small muted" {
                        "A device that just enrolled reads offline until its service restarts: "
                        code { "sc stop SummriseLaunch && sc start SummriseLaunch" }
                        ", then reload this page."
                    }
                }
                table class="table fleet-table" {
                    caption class="visually-hidden" { "Every device this console has enrolled, and the pages that act on it." }
                    thead {
                        tr {
                            th scope="col" { "Device" }
                            th scope="col" { "Pages" }
                            th class="num" scope="col" { span class="visually-hidden" { "Actions" } }
                        }
                    }
                    tbody {
                        @for device in devices {
                            tr {
                                td class="device-cell" {
                                    div class="device-identity" {
                                        strong class="device-name" { (device_name(device)) }
                                        (status_chip(device.online))
                                    }
                                    div class="device-meta" {
                                        code class="device-id" title=(device.id) { (short_id(&device.id)) }
                                        span class="meta-sep" aria-hidden="true" { "·" }
                                        span class="device-version" {
                                            @match device.version.as_deref() {
                                                Some(announced) => { "v" (announced) },
                                                None => { "no version" },
                                            }
                                        }
                                        span class="meta-sep" aria-hidden="true" { "·" }
                                        span class="device-seen" { (ago(now_ms, device.last_seen_ms)) }
                                        @if !device.online {
                                            @if let Some(retry) = &device.last_attempt {
                                                // WHY IT IS NOT CONNECTED, in the place the operator looks: "offline"
                                                // with no reason sends them to the logs, and the reason is already
                                                // on the wire (the device reports how its own attempt ended).
                                                span class="meta-sep" aria-hidden="true" { "·" }
                                                span class="device-retry" title=(retry.reason) {
                                                    "last try " (retry.attempts) "× failed: "
                                                    (if retry.reason.chars().count() > 60 {
                                                        let mut cut: String = retry.reason.chars().take(59).collect();
                                                        cut.push('…');
                                                        cut
                                                    } else {
                                                        retry.reason.clone()
                                                    })
                                                }
                                            }
                                        }
                                    }
                                }
                                td class="pages-cell" {
                                    // **FOUR PLACES A PERSON GOES, AND THE REST ONE CLICK AWAY.** Eleven
                                    // links in a row read as a menu bar rather than a device: the row
                                    // carried every page the console has, so none of them looked like
                                    // the one to press. The four that answer the daily jobs stay; the
                                    // other seven live behind `More`, still in the DOM and still
                                    // reachable without JavaScript.
                                    nav class="page-links" aria-label=(format!("Pages for {}", device_label(device))) {
                                        a href=(format!("/next/terminal/{}", device.id)) { "Terminal" }
                                        a href=(format!("/next/screen/{}", device.id)) { "Screen" }
                                        a href=(format!("/next/files/{}", device.id)) { "Files" }
                                        a href=(format!("/next/tokens/{}", device.id)) { "Connect" }
                                        details class="row-menu more-pages" {
                                            summary class="page-more" aria-label=(format!("More pages for {}", device_label(device))) {
                                                "More"
                                                span class="caret" aria-hidden="true" { "▾" }
                                            }
                                            div class="menu-panel page-menu" {
                                                a href=(format!("/next/runs/{}", device.id)) { "Runs" }
                                                a href=(format!("/next/system/{}", device.id)) { "System" }
                                                a href=(format!("/next/state/{}", device.id)) { "State" }
                                                a href=(format!("/next/memory/{}", device.id)) { "Memory" }
                                                a href=(format!("/next/monitors/{}", device.id)) { "Monitors" }
                                                a href=(format!("/next/browser/{}", device.id)) { "Browser" }
                                                a href=(format!("/next/vitals/{}", device.id)) { "Vitals" }
                                            }
                                        }
                                    }
                                }
                                td class="actions-cell" {
                                    details class="row-menu" {
                                        summary class="btn btn-ghost btn-sm" aria-label=(format!("Manage {}", device_label(device))) {
                                            "Manage"
                                            span class="caret" aria-hidden="true" { "▾" }
                                        }
                                        div class="menu-panel" {
                                            form method="post" action=(format!("/next/fleet/{}/name", device.id)) {
                                                input type="hidden" name="csrf" value=(csrf);
                                                div class="field" {
                                                    label for=(format!("rename-{}", short_id(&device.id))) { "Rename device" }
                                                    input id=(format!("rename-{}", short_id(&device.id))) name="name" type="text" maxlength="64" placeholder="New name";
                                                }
                                                button type="submit" class="btn btn-sm" aria-label=(format!("Rename {}", device_label(device))) { "Rename" }
                                            }
                                            form method="post" action=(format!("/next/fleet/{}/remove", device.id)) {
                                                input type="hidden" name="csrf" value=(csrf);
                                                button type="submit" class="btn btn-sm btn-danger" aria-label=(format!("Remove {}", device_label(device))) { "Remove" }
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
        @if !recent.is_empty() {
            section class="card" {
                // The footer below already links Audit and Denials; a second link here was noise.
                (card_head(
                    "Recent activity",
                    html! { span class="chip muted" { (recent.len()) } },
                    Markup::default(),
                ))
                table class="table activity-table" {
                    caption class="visually-hidden" { "What the console did most recently." }
                    thead {
                        tr {
                            th scope="col" { "When" }
                            th scope="col" { "Kind" }
                            th scope="col" { "Tool" }
                            th class="num" scope="col" { "Status" }
                        }
                    }
                    tbody {
                        @for row in recent {
                            tr {
                                td class="nowrap" { (stamp(row["ts_ms"].as_u64().unwrap_or(0))) }
                                td {
                                    @if let Some(kind) = row["kind"].as_str() {
                                        span class="chip muted" { (kind) }
                                    } @else { span class="muted" { "—" } }
                                }
                                td {
                                    // A tool when the line has one (a device action), otherwise
                                    // what the line says it was (a sign-in, a channel event).
                                    @if let Some(tool) = row["tool"].as_str().filter(|tool| !tool.is_empty()) {
                                        code { (tool) }
                                    } @else if let Some(detail) = row["detail"].as_str().filter(|detail| !detail.is_empty()) {
                                        span class="muted" { (detail) }
                                    } @else { span class="muted" { "—" } }
                                }
                                td class="num" {
                                    @match row["status"].as_u64().unwrap_or(0) {
                                        0 => { span class="muted" { "—" } },
                                        status if status >= 500 => { span class="status error" { (status) } },
                                        status if status >= 400 => { span class="status warn" { (status) } },
                                        status => { span class="status ok" { (status) } },
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        nav class="toolbar" aria-label="Records" {
            a class="btn btn-ghost btn-sm" href="/next/audit" { "Audit" }
            a class="btn btn-ghost btn-sm" href="/next/denials" { "Denials" }
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
/// What an operator narrowed the audit log to. Empty means "no filter".
#[derive(Debug, Default)]
pub(super) struct AuditFilter {
    pub(super) device: String,
    pub(super) kind: String,
    /// `2`, `4` or `5`: the status class, not an exact code.
    pub(super) status: String,
    pub(super) tool: String,
}

impl AuditFilter {
    /// Whether anything is narrowed, so the page can say "filtered" rather than imply the log is small.
    pub(super) fn is_active(&self) -> bool {
        !self.device.is_empty()
            || !self.kind.is_empty()
            || !self.status.is_empty()
            || !self.tool.is_empty()
    }
}

/// The audit page's filter bar: a GET form, so a narrowed view is a URL an operator can keep.
pub(super) fn audit_filter(
    action: &str,
    devices: &[(String, String)],
    filter: &AuditFilter,
    show_device: bool,
    kinds: &[&str],
) -> Markup {
    html! {
        section class="card" {
            (card_head("Filter", Markup::default(), Markup::default()))
            div class="card-body" {
                form method="get" action=(action) class="filter-form" {
                    @if show_device {
                        div class="field" {
                            label for="f-device" { "Device" }
                            select id="f-device" name="device" {
                                option value="" { "All devices" }
                                @for (id, label) in devices {
                                    option value=(id) selected[filter.device == *id] { (label) }
                                }
                            }
                        }
                    }
                    /* **NO KIND SELECT WHEN THERE IS ONE KIND.** Denials holds exactly `denied`
                       rows, so the audit's twelve-option list offered eleven choices that could
                       never match and made a filter look broken. An empty slice means "this log
                       has no other kind", not "show every kind". */
                    @if !kinds.is_empty() {
                        div class="field" {
                            label for="f-kind" { "Kind" }
                            select id="f-kind" name="kind" {
                                option value="" { "Every kind" }
                                @for kind in kinds {
                                    option value=(kind) selected[filter.kind == *kind] { (kind) }
                                }
                            }
                        }
                    }
                    div class="field" {
                        label for="f-status" { "Status" }
                        select id="f-status" name="status" {
                            option value="" { "Every status" }
                            @for (value, label) in [("2", "2xx"), ("4", "4xx"), ("5", "5xx")] {
                                option value=(value) selected[filter.status == value] { (label) }
                            }
                        }
                    }
                    div class="field" {
                        label for="f-tool" { "Tool or detail" }
                        input id="f-tool" name="tool" type="search" value=(filter.tool) placeholder="runs.exec, unknown_code";
                    }
                    div class="form-actions" {
                        /* **A FILTER IS NOT THE PAGE'S ACTION.** System's Filter and Memory's
                           Search are plain buttons; this one was filled, so the same job looked
                           like the primary action on one page and a secondary one on two
                           others. Filled is for the action that changes the product: start,
                           mint, approve, write. */
                        button type="submit" class="btn btn-sm" { "Apply" }
                        @if filter.is_active() {
                            a class="btn btn-ghost btn-sm" href=(action) { "Clear" }
                        }
                    }
                }
            }
        }
    }
}

pub(super) fn logs(
    rows: &[serde_json::Value],
    title: &str,
    total: usize,
    filter: Option<Markup>,
    csrf: &str,
    version: &str,
    lead: &str,
    caption: &str,
    filtered: bool,
) -> Markup {
    let count = rows.len();
    let content = html! {
        p class="lead" { (lead) }
        @if let Some(filter) = filter {
            (filter)
        }
        section class="card" {
            (card_head(
                title,
                html! {
                    @if total > count {
                        span class="chip muted" role="status" { (count) " of " (total) }
                    } @else {
                        span class="chip muted" { (count) }
                    }
                },
                html! { a class="btn btn-ghost btn-sm" href="/next" { "Fleet" } },
            ))
            @if rows.is_empty() {
                @if filtered {
                    (empty_state(Page::Fleet, "No line matches this filter.", Markup::default()))
                } @else {
                    (empty_state(Page::Fleet, "Nothing has been recorded yet.", Markup::default()))
                }
            } @else {
                table class="table logs-table" {
                    caption class="visually-hidden" { (caption) }
                    thead {
                        tr {
                            th scope="col" { "When" }
                            th scope="col" { "Kind" }
                            th class="num" scope="col" { "Status" }
                            th scope="col" { "Device" }
                            th scope="col" { "Tool" }
                            th scope="col" { "Detail" }
                        }
                    }
                    tbody {
                        @for row in rows {
                            tr {
                                td class="nowrap" { (stamp(row["ts_ms"].as_u64().unwrap_or(0))) }
                                td {
                                    @if let Some(kind) = row["kind"].as_str().filter(|kind| !kind.is_empty()) {
                                        span class="chip muted" { (kind) }
                                    } @else {
                                        span class="muted" { "—" }
                                    }
                                }
                                td class="num" {
                                    @match row["status"].as_u64().unwrap_or(0) {
                                        0 => { span class="muted" { "—" } },
                                        status if status >= 500 => { span class="status error" { (status) } },
                                        status if status >= 400 => { span class="status warn" { (status) } },
                                        status => { span class="status ok" { (status) } },
                                    }
                                }
                                td {
                                    @if let Some(id) = row["device_id"].as_str() {
                                        code class="device-id" title=(id) { (short_id(id)) }
                                    } @else {
                                        "—"
                                    }
                                }
                                td {
                                    @if let Some(tool) = row["tool"].as_str().filter(|tool| !tool.is_empty()) {
                                        code { (tool) }
                                    } @else {
                                        span class="muted" { "—" }
                                    }
                                }
                                td class="wrap" {
                                    @if let Some(detail) = row["detail"].as_str().filter(|detail| !detail.is_empty()) {
                                        (detail)
                                    } @else {
                                        span class="muted" { "—" }
                                    }
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
            active: Page::Fleet,
            device: None,
            title,
            version,
            csrf,
            status: html! { (count) " lines" },
        },
        content,
    )
}

/// The terminal page: a toolbar, a tab strip and the xterm host, all inside one card that fills the
/// canvas. The device id and this session's CSRF value ride in data attributes, not in the URL or a
/// cookie the script reads: the session cookie is `HttpOnly`, so the page cannot leak what it
/// cannot see.
/// One live terminal session on the device, as the rail lists it.
pub(super) struct TerminalSession {
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) started_at_ms: u64,
}

/// What the terminal page shows besides the canvas: the device's live sessions, its saved presets,
/// the preset the operator asked to prefill (`?preset=`), and the sentence a refused preset left.
pub(super) struct TerminalView<'a> {
    pub sessions: &'a [TerminalSession],
    pub presets: &'a [Connection],
    pub preset: Option<&'a Connection>,
    /// The console's last terminal operations on this device, from the audit log.
    pub activity: &'a [TerminalActivity],
    pub error: Option<&'a str>,
}

/// One audited terminal operation: what the console (or an AI through it) did, and when.
pub(super) struct TerminalActivity {
    pub at_ms: u64,
    pub tool: String,
    pub status: u64,
    /// How it arrived: `console`, `panel`, `loopback`, `mesh`.
    pub arrival: String,
}

pub(super) fn terminal(
    device: &DeviceView,
    csrf: &str,
    version: &str,
    view: TerminalView<'_>,
) -> Markup {
    let sessions = view.sessions;
    // The one-line form's prefill: a preset the operator clicked "Use" on fills the ssh or serial
    // fields, so the password is the only thing left to type (and the only thing never stored).
    let preset = view.preset;
    let preset_kind = preset.map(|preset| preset.kind.as_str()).unwrap_or("pty");
    let preset_host = preset
        .and_then(|preset| preset.host.as_deref())
        .unwrap_or_default();
    let preset_user = preset
        .and_then(|preset| preset.user.as_deref())
        .unwrap_or_default();
    let preset_port = preset
        .and_then(|preset| preset.port)
        .map(|port| port.to_string())
        .unwrap_or_else(|| "22".to_owned());
    let preset_serial = preset
        .and_then(|preset| preset.port_path.as_deref())
        .unwrap_or_default();
    let preset_baud = preset
        .and_then(|preset| preset.baud)
        .map(|baud| baud.to_string())
        .unwrap_or_else(|| "115200".to_owned());
    let meta = if sessions.is_empty() {
        Markup::default()
    } else {
        html! { span class="chip" { (sessions.len()) } }
    };
    let recent = if view.activity.is_empty() {
        Markup::default()
    } else {
        html! {
            div class="rail-history" {
                p class="rail-note" {
                    "Sessions do not survive a device restart. These are the console's last terminal "
                    "operations on this device, from the audit log."
                }
                ul class="session-list" {
                    @for line in view.activity {
                        li class="session-row" {
                            span class="session-kind" { (&line.arrival) }
                            span class="session-body" {
                                code { (&line.tool) }
                                span class="session-age" { (stamp(line.at_ms)) " · " (line.status) }
                            }
                        }
                    }
                }

            }
        }
    };
    let body = if sessions.is_empty() {
        html! {
            (recent)
            @if view.error.is_some() {
                p class="rail-note" { "The device did not answer." }
            } @else {
                p class="rail-note" { "No live session on this device." }
            }
        }
    } else {
        html! {
            ul class="session-list" {
                @for session in sessions {
                    li class="session-row" {
                        a href=(format!("?attach={}", encode(&session.id))) title=(format!("open {} · started {}", session.id, stamp(session.started_at_ms))) {
                            span class="session-kind" { (&session.kind) }
                            span class="session-body" {
                                code { (&session.id) }
                                span class="session-age" { "since " (clock(session.started_at_ms)) }
                            }
                        }
                        // A session another browser left behind can be ended here, without opening
                        // it: the rail is the only list of them the operator has.
                        form method="post" action=(format!("/next/terminal/{}/{}/end", device.id, encode_segment(&session.id))) {
                            input type="hidden" name="csrf" value=(csrf);
                            button type="submit" class="rail-close" title="End this session" aria-label=(format!("End session {}", session.id)) { "×" }
                        }
                    }
                }
            }
            (recent)
        }
    };
    let context = context_rail(
        "Sessions",
        meta,
        body,
        // No actions here: the rail is context (the sessions), not navigation, and a "Refresh"
        // anchor pointed at this same page is what the browser's own refresh already does.
        Markup::default(),
    );
    let content = html! {
        div class="terminal-card" {
            div class="terminal-toolbar" {
                form id="terminal-form" class="terminal-form" {
                    label for="kind" class="toolbar-label" { "Kind" }
                    select id="kind" name="kind" {
                        option value="pty" selected[preset_kind == "pty"] { "pty — a shell on this machine" }
                        option value="ssh" selected[preset_kind == "ssh"] { "ssh — a host this device can reach" }
                        option value="serial" selected[preset_kind == "serial"] { "serial — an OS port" }
                    }
                    div class="kind-fields" data-kind="ssh" hidden[preset_kind != "ssh"] {
                        div class="field" {
                            label for="host" { "Host" }
                            input id="host" name="host" type="text" placeholder="192.168.1.1" value=(preset_host);
                        }
                        div class="field" {
                            label for="ssh_port" { "Port" }
                            input id="ssh_port" name="ssh_port" type="number" value=(preset_port) min="1" max="65535";
                        }
                        div class="field" {
                            label for="user" { "User" }
                            input id="user" name="user" type="text" placeholder="root" value=(preset_user);
                        }
                        div class="field" {
                            label for="password" { "Password" }
                            input id="password" name="password" type="password" autocomplete="off";
                        }
                        div class="field" {
                            label for="key_path" { "Key file" }
                            input id="key_path" name="key_path" type="text" placeholder="C:\\Users\\you\\.ssh\\id_ed25519";
                        }
                    }
                    div class="kind-fields" data-kind="serial" hidden[preset_kind != "serial"] {
                        div class="field" {
                            label for="serial_port" { "Port" }
                            input id="serial_port" name="serial_port" type="text" placeholder="COM3 or /dev/ttyUSB0" value=(preset_serial);
                        }
                        div class="field" {
                            label for="baud" { "Baud" }
                            input id="baud" name="baud" type="number" value=(preset_baud) min="1";
                        }
                    }
                    button type="submit" class="btn btn-primary btn-sm" { "Start session" }
                }
                // A refused preset opens the panel: the sentence is the point of the redirect, and a
                // closed dropdown would hide it.
                details class="row-menu presets" open[view.error.is_some()] {
                    summary class="btn btn-ghost btn-sm" title="Saved connections for this device" {
                        "Presets " span class="preset-count" { (view.presets.len()) }
                        span class="caret" { "▾" }
                    }
                    div class="menu-panel preset-panel" {
                        @if let Some(error) = view.error {
                            p class="alert error" { (error) }
                        }
                        @if view.presets.is_empty() {
                            p class="rail-note" { "No saved connections on this device yet." }
                        } @else {
                            ul class="preset-list" {
                                @for preset in view.presets {
                                    li class="preset-row" {
                                        div class="preset-ident" {
                                            span class="preset-name" { (preset.name) }
                                            span class="preset-target" { (preset_summary(preset)) }
                                        }
                                        div class="preset-actions" {
                                            @if preset.kind == "ssh" && preset.key_path.is_none() {
                                                // An SSH preset with no key deliberately carries no
                                                // password either, so it fills the form instead of
                                                // opening a session.
                                                a class="btn btn-sm" href=(format!("/next/terminal/{}?preset={}", device.id, encode(&preset.id))) { "Use" }
                                            } @else {
                                                form method="post" action=(format!("/next/terminal/{}/connections/{}/connect", device.id, encode(&preset.id))) {
                                                    input type="hidden" name="csrf" value=(csrf);
                                                    button type="submit" class="btn btn-sm" aria-label=(format!("Connect {}", preset.name)) { "Connect" }
                                                }
                                            }
                                            form method="post" action=(format!("/next/terminal/{}/connections/{}/remove", device.id, encode(&preset.id))) {
                                                input type="hidden" name="csrf" value=(csrf);
                                                button type="submit" class="btn btn-quiet-danger btn-sm" aria-label=(format!("Forget {}", preset.name)) { "Forget" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        form class="preset-add" method="post" action=(format!("/next/terminal/{}/connections", device.id)) {
                            input type="hidden" name="csrf" value=(csrf);
                            div class="field" {
                                label for="preset-name" { "Name" }
                                input id="preset-name" name="name" maxlength="64" required;
                            }
                            div class="field" {
                                label for="preset-kind" { "Kind" }
                                select id="preset-kind" name="kind" {
                                    option value="pty" { "pty" }
                                    option value="ssh" { "ssh" }
                                    option value="serial" { "serial" }
                                }
                            }
                            div class="field" {
                                label for="preset-target" { "Target" }
                                input id="preset-target" name="target" placeholder="root@host:22 or COM3@115200";
                            }
                            div class="field" {
                                label for="preset-key" { "Key file (ssh)" }
                                input id="preset-key" name="key_path" placeholder="C:\\Users\\you\\.ssh\\id_ed25519";
                            }
                            button type="submit" class="btn btn-sm" { "Save preset" }
                        }
                    }
                }
                span class="spacer" {}
                // **THE WAY OUT OF THE TERMINAL IS IN THE TOOLBAR, NOT THE CONTEXT RAIL.** The rail
                // is hidden below 1000px, so a navigation link that lives only there disappears on
                // a tablet; and a "Refresh" anchor pointed at this same page, which is what the
                // browser's own refresh already does.
                a class="btn btn-ghost btn-sm" href="/next/audit" { "Audit" }
            }
            div class="terminal-tabbar" {
                div id="terminal-tabs" class="tabs" {}
                p id="terminal-status" aria-live="polite" {}
            }
            div id="terminal-panes" class="panes" {
                div class="terminal-empty" {
                    div class="empty-icon" aria-hidden="true" { (Page::Terminal.icon()) }
                    @if sessions.is_empty() {
                        p { "No session yet — choose a kind and start one." }
                    } @else {
                        // The rail is hidden below 1000px, so this must not point at it.
                        p { "No session open in this pane — start one, or attach an existing session." }
                    }
                }
                div id="terminal" data-device=(device.id) data-csrf=(csrf) hidden {}
            }
        }
    };
    let extra = html! {
        link rel="stylesheet" href="/assets/xterm.css";
        script src="/assets/xterm.js" defer {}
        script src="/assets/addon-fit.js" defer {}
        script src="/assets/terminal.js" defer {}
    };
    page_with_rail(
        Chrome {
            active: Page::Terminal,
            device: Some(device),
            title: "Terminal",
            version,
            csrf,
            status: device_status(device),
        },
        extra,
        context,
        content,
    )
}

/// One entry the files page draws: the name, what it is, and its length.
pub(super) struct FileEntry {
    pub(super) name: String,
    pub(super) kind: String,
    pub(super) len: u64,
}

/// Percent-encode one path segment. Unlike [`encode`], `/` is escaped: a device-supplied session id
/// with a slash must not become two path segments in a form's action.
pub(super) fn encode_segment(value: &str) -> String {
    encode(value).replace('/', "%2F")
}

/// Percent-encode a query value. `/` stays readable because a path is the value most of the time.
/// The time of day a rail row shows: UTC, no date, because the rail is about "since when" rather
/// than "which day". The full stamp is in the row's tooltip.
fn clock(ms: u64) -> String {
    let stamp = stamp(ms);
    match stamp.split_once(' ') {
        Some((_, rest)) => rest.to_owned(),
        None => stamp,
    }
}

/// One preset, in the line an operator recognises it by.
fn preset_summary(preset: &Connection) -> String {
    match preset.kind.as_str() {
        "ssh" => format!(
            "{}@{}:{}",
            preset.user.as_deref().unwrap_or("?"),
            preset.host.as_deref().unwrap_or("?"),
            preset.port.unwrap_or(22)
        ),
        "serial" => match preset.baud {
            Some(baud) => format!("{}@{}", preset.port_path.as_deref().unwrap_or("?"), baud),
            None => preset.port_path.clone().unwrap_or_default(),
        },
        _ => "a shell on this device".to_owned(),
    }
}

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

/// A count with thousands separators: an operator reads `512,900` without counting digits.
fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out.chars().rev().collect()
}

/// The ancestors of a path, as breadcrumb links: `Files`, then one link per directory above the
/// current one. The last segment is left for the caller to render as the page's current name.
fn trail_to(base: &str, path: &str) -> Vec<(String, String)> {
    let segments: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    let mut trail = vec![("Files".to_owned(), base.to_owned())];
    let mut cumulative = String::new();
    for segment in segments.iter().take(segments.len().saturating_sub(1)) {
        if !cumulative.is_empty() {
            cumulative.push('/');
        }
        cumulative.push_str(segment);
        trail.push((
            (*segment).to_owned(),
            format!("{}?path={}", base, encode(&cumulative)),
        ));
    }
    trail
}

/// The final segment of a device path, which is the name the page is reading.
fn last_segment(path: &str) -> &str {
    path.rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or("root")
}

/// The device's files, server-rendered: a directory card, then the read and write forms.
pub(super) fn files(
    device: &DeviceView,
    path: &str,
    entries: &[FileEntry],
    csrf: &str,
    version: &str,
    error: Option<&str>,
) -> Markup {
    let base = format!("/next/files/{}", device.id);
    let count = entries.len();
    let content = html! {
        p class="lead" {
            "The device's files. Paths are relative to its root; the device enforces the root."
        }
        @if let Some(error) = error {
            p class="alert error" { (error) }
        }
        @if !path.is_empty() {
            (breadcrumb(&trail_to(&base, path), last_segment(path)))
        }
        section class="card" {
            (card_head(
                "Directory",
                html! { span class="chip muted" { (count) " item" (if count == 1 { "" } else { "s" }) } },
                if path.is_empty() {
                    Markup::default()
                } else {
                    html! { a class="btn btn-sm" href=(format!("{}?path={}", base, encode(parent(path)))) { "Up one level" } }
                },
            ))
            @if entries.is_empty() {
                (empty_state(
                    Page::Files,
                    "This directory is empty.",
                    html! { a class="btn btn-sm" href="#write" { "Write the first file" } },
                ))
            } @else {
                table class="table reflow" {
                    caption class="visually-hidden" { "The directory's entries." }
                    thead {
                        tr {
                            th scope="col" { "Name" }
                            th scope="col" { "Kind" }
                            th class="num" scope="col" { "Bytes" }
                        }
                    }
                    tbody {
                        @for entry in entries {
                            tr {
                                td {
                                    @if entry.kind == "directory" {
                                        a class="file-name" href=(format!("{}?path={}", base, encode(&join(path, &entry.name)))) { (entry.name) }
                                    } @else {
                                        a class="file-name" href=(format!("{}/read?path={}", base, encode(&join(path, &entry.name)))) { (entry.name) }
                                    }
                                }
                                td {
                                    span class="chip muted" { (entry.kind) }
                                }
                                td class="num mono" {
                                    @if entry.kind == "directory" { "—" } @else { (grouped(entry.len)) }
                                }
                            }
                        }
                    }
                }
            }
        }
        section class="card" {
            (card_head("Read a file", Markup::default(), Markup::default()))
            div class="card-body" {
                form method="get" action=(format!("{}/read", base)) class="form-inline" {
                    div class="field grow" {
                        label class="visually-hidden" for="read-path" { "Path to read" }
                        input id="read-path" type="text" name="path" placeholder="notes.txt" required;
                    }
                    button type="submit" class="btn" { "Read" }
                }
            }
        }
        section class="card" id="write" {
            (card_head("Write a file", Markup::default(), Markup::default()))
            div class="card-body" {
                form method="post" action=(format!("{}/write", base)) {
                    input type="hidden" name="csrf" value=(csrf);
                    div class="field" {
                        label for="write-path" { "Path" }
                        input id="write-path" type="text" name="path" placeholder="notes.txt" required;
                    }
                    div class="field" {
                        label for="write-content" { "Contents" }
                        textarea id="write-content" name="content" rows="8" placeholder="text, written as UTF-8" {}
                    }
                    div class="form-actions" {
                        button type="submit" class="btn btn-primary" { "Write" }
                        span class="muted" { "Replaces the file as UTF-8." }
                    }
                }
            }
        }
        section class="card" {
            (card_head("New folder", Markup::default(), Markup::default()))
            div class="card-body" {
                form method="post" action=(format!("{}/mkdir", base)) class="form-inline" {
                    input type="hidden" name="csrf" value=(csrf);
                    input type="hidden" name="current" value=(path);
                    div class="field grow" {
                        label class="visually-hidden" for="mkdir-path" { "Folder path" }
                        input id="mkdir-path" type="text" name="path" placeholder="logs/2026" required;
                    }
                    button type="submit" class="btn" { "Create folder" }
                }
            }
        }
    };
    // The rail is the directory itself: how much is in it, and the way back to the root.
    let dirs = entries
        .iter()
        .filter(|entry| entry.kind == "directory")
        .count();
    let bytes: u64 = entries.iter().map(|entry| entry.len).sum();
    // The root is "" in a URL and "/" to a reader; the rail always speaks to the reader.
    let display_path = if path.is_empty() { "/" } else { path };
    let context = context_rail(
        "This directory",
        html! { span class="chip muted" { (entries.len()) } },
        html! {
            ul class="rail-list" {
                li {
                    span class="rail-line" { "Entries" }
                    span class="rail-meta" { (dirs) " directories · " (entries.len() - dirs) " files" }
                }
                li {
                    span class="rail-line" { "Size" }
                    span class="rail-meta" { (human_bytes(bytes)) " in this listing" }
                }
                li {
                    span class="rail-line" { "Path" }
                    span class="rail-meta mono" { (display_path) }
                }
            }
        },
        html! {
            div class="rail-actions" {
                // "Up one level" lives in the card head, where it builds `?path=` correctly. The copy
                // that used to be here concatenated the parent onto the device id and 404ed on every
                // nested directory; a second control for one job is not redundancy, it is a bug.
                a class="btn btn-ghost btn-sm" href=(format!("/next/files/{}", device.id)) { "Root" }
            }
        },
    );
    page_with_rail(
        Chrome {
            active: Page::Files,
            device: Some(device),
            title: "Files",
            version,
            csrf,
            status: device_status(device),
        },
        Markup::default(),
        context,
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
        (breadcrumb(&trail_to(&base, path), last_segment(path)))
        section class="card" {
            (card_head(
                "Contents",
                html! { span class="chip muted" { "text" } },
                html! {
                    a class="btn btn-sm" href=(format!("{}?path={}", base, encode(parent(path)))) { "Back to the directory" }
                },
            ))
            pre class="file" tabindex="0" { (text) }
        }
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
    pub(super) id: String,
    pub(super) command_line: String,
    pub(super) started_at_ms: u64,
    pub(super) duration_ms: u64,
    pub(super) exit_code: Option<i64>,
    pub(super) timed_out: bool,
}

/// One history row, as `runs.get` answers it: what ran, where, and how much it wrote. The device
/// keeps rows, not output.
pub(super) struct RunDetail {
    pub(super) id: String,
    pub(super) command: String,
    pub(super) args: String,
    pub(super) cwd: String,
    pub(super) started_at_ms: u64,
    pub(super) duration_ms: u64,
    pub(super) exit_code: Option<i64>,
    pub(super) timed_out: bool,
    pub(super) stdout_bytes: u64,
    pub(super) stderr_bytes: u64,
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

/// One exit code, as a chip: a finished zero, a refusal, or an end the device could not name.
fn exit_chip(code: Option<i64>, timed_out: bool) -> Markup {
    if timed_out {
        html! { span class="chip failed" { "timed out" } }
    } else {
        match code {
            Some(0) => html! { span class="chip online" { "0" } },
            Some(code) => html! { span class="chip failed" { (code) } },
            None => html! { span class="chip muted" { "killed" } },
        }
    }
}

/// The runs page: a form to run one command, and the device's own history under it.
pub(super) fn runs(
    device: &DeviceView,
    rows: &[RunRow],
    detail: Option<&RunDetail>,
    csrf: &str,
    version: &str,
    error: Option<&str>,
) -> Markup {
    let base = format!("/next/runs/{}", device.id);
    let content = html! {
        p class="lead" {
            "A run is a process the device starts and reaps before it answers. The request holds "
            "the page open for the run's length: keep the timeout short, or start the long work "
            "with Start-Process and watch it through a later run. runs.exec is audited."
        }
        @if let Some(error) = error {
            p class="alert error" { (error) }
        }
        @if let Some(detail) = detail {
            section class="card" {
                (card_head(
                    &format!("Run {}", detail.id),
                    html! {
                        @if detail.timed_out {
                            span class="chip failed" { "timed out" }
                        } @else if detail.exit_code == Some(0) {
                            span class="chip online" { "exit 0" }
                        } @else {
                            span class="chip failed" { (format!("exit {}", detail.exit_code.map_or("—".to_owned(), |code| code.to_string()))) }
                        }
                    },
                    html! { a class="btn btn-ghost btn-sm" href=(base.clone()) { "Close" } },
                ))
                div class="card-body" {
                    dl class="facts" {
                        dt { "Command" }
                        dd { code { (detail.command) } }
                        @if !detail.args.is_empty() {
                            dt { "Arguments" }
                            dd { code { (detail.args) } }
                        }
                        dt { "Directory" }
                        dd { code { (detail.cwd) } }
                        dt { "Started" }
                        dd { (stamp(detail.started_at_ms)) }
                        dt { "Took" }
                        dd { (detail.duration_ms) " ms" }
                        dt { "Output" }
                        dd { (detail.stdout_bytes) " bytes stdout, " (detail.stderr_bytes) " bytes stderr" }
                    }
                    p class="footnote" {
                        "The device keeps history rows, not output: a run's stdout appears only on "
                        "the page that ran it."
                    }
                }
            }
        }
        section class="card" {
            (card_head("Run a command", Markup::default(), Markup::default()))
            div class="card-body" {
                form method="post" action=(format!("{base}/exec")) {
                    input type="hidden" name="csrf" value=(csrf);
                    div class="form-grid" {
                        div class="field span-2" {
                            label for="command" { "Command" }
                            input id="command" name="command" type="text" placeholder="pwsh" required;
                        }
                        div class="field span-2" {
                            label for="args" { "Arguments, one per line" }
                            textarea id="args" name="args" rows="3" placeholder="-NoProfile -Command Get-Date" {}
                        }
                        div class="field" {
                            label for="cwd" { "Working directory (optional)" }
                            input id="cwd" name="cwd" type="text" placeholder="relative to the files root";
                        }
                        div class="field" {
                            label for="timeout_ms" { "Timeout (ms)" }
                            input id="timeout_ms" name="timeout_ms" type="number" value="30000" min="1000" max="600000";
                        }
                    }
                    label class="check" {
                        input type="checkbox" name="shell" value="1";
                        " Run through the platform shell"
                    }
                    div class="form-actions" {
                        button type="submit" class="btn btn-primary" { "Run" }
                    }
                }
            }
        }
        section class="card" {
            (card_head(
                "History",
                html! { span class="chip muted" { (rows.len()) } },
                Markup::default(),
            ))
            @if rows.is_empty() && error.is_none() {
                (empty_state(Page::Runs, "No runs recorded yet.", Markup::default()))
            } @else {
                table class="table reflow runs-table" {
                    caption class="visually-hidden" { "Runs the device has completed, newest first." }
                    thead {
                        tr {
                            th scope="col" { "When" }
                            th scope="col" { "Command" }
                            th class="num" scope="col" { "Took" }
                            th scope="col" { "Exit" }
                        }
                    }
                    tbody {
                        @for row in rows {
                            tr {
                                td class="nowrap" { (stamp(row.started_at_ms)) }
                                td {
                                    a class="mono cmd-line" href=(format!("{base}?run={}", encode(&row.id))) { (row.command_line) }
                                }
                                td class="num" { (row.duration_ms) " ms" }
                                td { (exit_chip(row.exit_code, row.timed_out)) }
                            }
                        }
                    }
                }
            }
        }
    };
    let meta = if rows.is_empty() {
        Markup::default()
    } else {
        html! { span class="chip muted" { (rows.len()) } }
    };
    let body = if rows.is_empty() {
        html! { p class="rail-note" { "Nothing has run on this device yet." } }
    } else {
        html! {
            ul class="rail-list" {
                @for row in rows.iter().take(8) {
                    li {
                        span class="rail-line mono" { (row.command_line) }
                        span class="rail-meta" {
                            (clock(row.started_at_ms))
                            " · "
                            (exit_chip(row.exit_code, row.timed_out))
                        }
                    }
                }
            }
        }
    };
    let rail = context_rail("Recent runs", meta, body, Markup::default());
    page_with_rail(
        Chrome {
            active: Page::Runs,
            device: Some(device),
            title: "Runs",
            version,
            csrf,
            status: device_status(device),
        },
        Markup::default(),
        rail,
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
    let base = format!("/next/runs/{}", device.id);
    let status = if result.timed_out {
        "timed out".to_owned()
    } else if let Some(code) = result.exit_code {
        format!("exit {code}")
    } else {
        "killed".to_owned()
    };
    let content = html! {
        (breadcrumb(&[("Runs".to_owned(), base)], &result.id))
        div class="toolbar" {
            span class="chip online" { (status) }
            span class="muted small" { (result.duration_ms) " ms" }
            @if result.truncated {
                span class="chip muted" { "output truncated" }
            }
        }
        section class="card" {
            (card_head("Command", Markup::default(), Markup::default()))
            div class="card-body" {
                code class="cmd" { (command_line) }
            }
        }
        section class="card" {
            (card_head("stdout", Markup::default(), Markup::default()))
            pre class="out" tabindex="0" { (result.stdout) }
        }
        @if !result.stderr.is_empty() {
            section class="card" {
                (card_head("stderr", Markup::default(), Markup::default()))
                pre class="out stderr" tabindex="0" { (result.stderr) }
            }
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
        p class="lead" {
            "The document the device keeps: its health, and the last ten events it recorded."
        }
        section class="card" {
            (card_head(
                "State document",
                html! { span class="chip muted" { "json" } },
                html! { a class="btn btn-ghost btn-sm" href=(format!("/next/system/{}", device.id)) { "Processes" } },
            ))
            pre class="file" tabindex="0" { (pretty) }
        }
    };
    let health = pretty
        .lines()
        .find_map(|line| line.trim().strip_prefix("\"health\": \""))
        .and_then(|rest| rest.split('"').next())
        .unwrap_or("unknown")
        .to_owned();
    let ready = pretty.contains("\"ready\": true");
    let events = pretty.matches("\"kind\":").count();
    let context = context_rail(
        "Health",
        html! { span class="chip" { (&health) } },
        html! {
            ul class="rail-list" {
                li {
                    span class="rail-line" { "Ready" }
                    span class="rail-meta" {
                        @if ready { span class="chip online" { "yes" } } @else { span class="chip offline" { "no" } }
                    }
                }
                li {
                    span class="rail-line" { "Events" }
                    span class="rail-meta" { (events) " recorded in this document" }
                }
            }
        },
        Markup::default(),
    );
    page_with_rail(
        Chrome {
            active: Page::System,
            device: Some(device),
            title: "State",
            version,
            csrf,
            status: device_status(device),
        },
        Markup::default(),
        context,
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
        p class="lead" {
            "The processes this device is running now. It is one sample, not a history; the filter "
            "matches an executable name and asks the device for the list again."
        }
        @if let Some(notice) = notice {
            p class="alert notice" { (notice) }
        }
        section class="card" {
            (card_head(
                "Processes",
                html! { span class="chip muted" { (processes.len()) } },
                html! {
                    form method="get" action=(base) class="filter-form" {
                        label class="visually-hidden" for="process-name" { "Filter by name" }
                        input id="process-name" type="text" name="name" value=(name_filter) placeholder="filter by name";
                        button type="submit" class="btn btn-sm" { "Filter" }
                        @if !name_filter.is_empty() {
                            a class="btn btn-ghost btn-sm" href=(base) { "Clear" }
                        }
                    }
                },
            ))
            @if processes.is_empty() && notice.is_none() {
                (empty_state(Page::System, "The device listed no processes.", Markup::default()))
            } @else {
                table class="table reflow" {
                    caption class="visually-hidden" { "Processes running on the device." }
                    thead {
                        tr {
                            th class="num pid" scope="col" { "PID" }
                            th scope="col" { "Name" }
                            th class="num" scope="col" { "Memory" }
                        }
                    }
                    tbody {
                        @for process in processes {
                            tr {
                                td class="num mono pid" { (process.pid) }
                                td { span class="mono" { (process.name) } }
                                td class="num" { (human_kb(process.memory_kb)) }
                            }
                        }
                    }
                }
            }
        }
        section class="card" {
            (card_head("Reachability", Markup::default(), Markup::default()))
            div class="card-body" {
                p class="lead" {
                    "Ask this device whether it can reach a host and port — the way to tell a "
                    "broken target from a broken path to it."
                }
                form method="post" action=(format!("{base}/net")) class="form-inline" {
                    input type="hidden" name="csrf" value=(csrf);
                    div class="field grow" {
                        label class="visually-hidden" for="net-host" { "Host" }
                        input id="net-host" type="text" name="host" placeholder="10.0.0.5 or example.com" required;
                    }
                    div class="field" {
                        label class="visually-hidden" for="net-port" { "Port" }
                        input id="net-port" type="number" name="port" min="1" max="65535" placeholder="443" required;
                    }
                    button type="submit" class="btn" { "Test reachability" }
                }
            }
        }
        section class="card" {
            (card_head("Lifecycle", Markup::default(), Markup::default()))
            div class="card-body" {
                p class="lead" {
                    "Restarting restarts the app the launcher supervises. Every session on this "
                    "device ends with it; a device the launcher did not start refuses."
                }
                details class="row-menu restart-menu" {
                    summary class="btn btn-danger btn-sm" { "Restart the device" }
                    div class="menu-panel restart-panel" {
                        p class="small muted" {
                            "Every terminal session ends, watches pause, and this page will show the "
                            "device offline while the launcher brings it back — but after three "
                            "consecutive restarts the launcher gives up, and the device stays down."
                        }
                        form method="post" action=(format!("{base}/restart")) {
                            input type="hidden" name="csrf" value=(csrf);
                            button type="submit" class="btn btn-danger btn-sm btn-block" { "Restart now" }
                        }
                    }
                }
            }
        }
        section class="card" {
            (card_head("End a process", Markup::default(), Markup::default()))
            div class="card-body" {
                p class="lead" {
                    "Name a pid or an exact executable name, not both. This is destructive and audited."
                }
                form method="post" action=(format!("{base}/kill")) class="form-inline" {
                    input type="hidden" name="csrf" value=(csrf);
                    div class="field" {
                        label for="kill-pid" { "PID" }
                        input id="kill-pid" type="number" name="pid" min="1" placeholder="pid";
                    }
                    div class="field" {
                        label for="kill-name" { "Name" }
                        input id="kill-name" type="text" name="name" placeholder="name";
                    }
                    button type="submit" class="btn btn-danger" { "Kill" }
                }
            }
        }
    };
    // The rail answers the question the page is about: what is using the machine.
    let mut top: Vec<&ProcessRow> = processes.iter().collect();
    top.sort_by_key(|row| std::cmp::Reverse(row.memory_kb));
    let meta = if processes.is_empty() {
        Markup::default()
    } else {
        html! { span class="chip muted" { (processes.len()) } }
    };
    let body = if processes.is_empty() {
        html! { p class="rail-note" { "The device reported no processes." } }
    } else {
        html! {
            ul class="rail-list" {
                @for row in top.iter().take(8) {
                    li {
                        span class="rail-line mono" { (row.name) }
                        span class="rail-meta" {
                            "pid " (row.pid)
                            " · "
                            (human_kb(row.memory_kb))
                        }
                    }
                }
            }
        }
    };
    let context = context_rail("Top by memory", meta, body, Markup::default());
    page_with_rail(
        Chrome {
            active: Page::System,
            device: Some(device),
            title: "System",
            version,
            csrf,
            status: device_status(device),
        },
        Markup::default(),
        context,
        content,
    )
}

/// One memory entry, as the page draws it.
#[derive(Clone)]
pub(super) struct MemoryRow {
    pub(super) id: String,
    pub(super) text: String,
    pub(super) tags: String,
}

/// The device-local memory: the entries, an append form, and a delete button per row.
/// What the memory page is showing: which filter or search produced the list, and which entry (if
/// any) the edit form is filled from.
pub(super) struct MemoryLook<'a> {
    pub tag: &'a str,
    pub query: &'a str,
    pub editing: Option<&'a MemoryRow>,
}

pub(super) fn memory(
    device: &DeviceView,
    entries: &[MemoryRow],
    look: MemoryLook<'_>,
    csrf: &str,
    version: &str,
    notice: Option<&str>,
) -> Markup {
    let MemoryLook {
        tag,
        query,
        editing,
    } = look;
    let base = format!("/next/memory/{}", device.id);
    let content = html! {
        p class="lead" {
            "What this device remembers: notes an operator or a client appended, newest first. "
            "It is the device's own store, not the console's."
        }
        @if let Some(notice) = notice {
            p class="alert notice" { (notice) }
        }
        section class="card" {
            (card_head(
                "Entries",
                html! { span class="chip muted" { (entries.len()) } },
                html! {
                    div class="form-inline" {
                        form method="get" action=(base) class="filter-form" {
                            label class="visually-hidden" for="memory-q" { "Search" }
                            input id="memory-q" type="text" name="q" value=(query) placeholder="search text or tags";
                            label class="visually-hidden" for="memory-tag" { "Filter by tag" }
                            input id="memory-tag" type="text" name="tag" value=(tag) placeholder="tag";
                            button type="submit" class="btn btn-sm" { "Search" }
                            @if !tag.is_empty() || !query.is_empty() {
                                a class="btn btn-ghost btn-sm" href=(base) { "Clear" }
                            }
                        }
                        a class="btn btn-ghost btn-sm" href=(format!("{base}/export")) { "Export" }
                    }
                },
            ))
            @if entries.is_empty() && notice.is_none() {
                (empty_state(
                    Page::Memory,
                    "Nothing is remembered yet.",
                    html! { a class="btn btn-sm" href="#remember" { "Remember the first fact" } },
                ))
            } @else {
                table class="table reflow" {
                    caption class="visually-hidden" { "What this device remembers." }
                    thead {
                        tr {
                            th scope="col" { "Text" }
                            th scope="col" { "Tags" }
                            th class="num" scope="col" { span class="visually-hidden" { "Actions" } }
                        }
                    }
                    tbody {
                        @for entry in entries {
                            tr {
                                td class="wrap" { (entry.text) }
                                td { code { (entry.tags) } }
                                td class="num" {
                                    div class="form-inline" {
                                        a class="btn btn-sm" href=(format!(
                                            "{base}?edit={}{}{}",
                                            encode(&entry.id),
                                            if query.is_empty() { String::new() } else { format!("&q={}", encode(query)) },
                                            if tag.is_empty() { String::new() } else { format!("&tag={}", encode(tag)) }
                                        )) { "Edit" }
                                        form method="post" action=(format!("{base}/delete")) {
                                            input type="hidden" name="csrf" value=(csrf);
                                            input type="hidden" name="id" value=(entry.id);
                                            button type="submit" class="btn btn-sm btn-quiet-danger" aria-label=(format!("Delete the memory entry \"{}\"", entry.text.chars().take(60).collect::<String>())) { "Delete" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        @if let Some(entry) = editing {
            section class="card" id="edit" {
                (card_head(
                    "Edit an entry",
                    html! { span class="chip muted" { (entry.id) } },
                    Markup::default(),
                ))
                div class="card-body" {
                    form method="post" action=(format!("{base}/update")) {
                        input type="hidden" name="csrf" value=(csrf);
                        input type="hidden" name="id" value=(entry.id);
                        input type="hidden" name="q" value=(query);
                        input type="hidden" name="tag" value=(tag);
                        div class="field" {
                            label for="edit-text" { "Fact" }
                            textarea id="edit-text" name="text" rows="3" required { (entry.text) }
                        }
                        div class="field" {
                            label for="edit-tags" { "Tags" }
                            input id="edit-tags" type="text" name="tags" value=(entry.tags) placeholder="tags, comma-separated";
                        }
                        div class="form-actions" {
                            button type="submit" class="btn btn-primary" { "Save" }
                            a class="btn btn-ghost" href=(base) { "Cancel" }
                            span class="muted" { "Tags are the complete new list, not a patch." }
                        }
                    }
                }
            }
        }
        section class="card" id="remember" {
            (card_head("Remember something", Markup::default(), Markup::default()))
            div class="card-body" {
                form method="post" action=(format!("{base}/append")) {
                    input type="hidden" name="csrf" value=(csrf);
                    div class="field" {
                        label for="memory-text" { "Fact" }
                        textarea id="memory-text" name="text" rows="3" placeholder="the fact to keep" required {}
                    }
                    div class="field" {
                        label for="memory-tags" { "Tags" }
                        input id="memory-tags" type="text" name="tags" placeholder="tags, comma-separated";
                    }
                    div class="form-actions" {
                        button type="submit" class="btn btn-primary" { "Append" }
                    }
                }
            }
        }
        p class="footnote" {
            "Device " code title=(device.id) { (&device.id) }
        }
    };
    let meta = if entries.is_empty() {
        Markup::default()
    } else {
        html! { span class="chip muted" { (entries.len()) } }
    };
    let body = if entries.is_empty() {
        html! { p class="rail-note" { "Nothing is remembered on this device yet." } }
    } else {
        html! {
            ul class="rail-list" {
                @for entry in entries.iter().take(6) {
                    li {
                        span class="rail-line" { (entry.text.chars().take(60).collect::<String>()) }
                        @if !entry.tags.is_empty() {
                            span class="rail-meta" { (&entry.tags) }
                        }
                    }
                }
            }
        }
    };
    let context = context_rail("Memory", meta, body, Markup::default());
    page_with_rail(
        Chrome {
            active: Page::Memory,
            device: Some(device),
            title: "Memory",
            version,
            csrf,
            status: device_status(device),
        },
        Markup::default(),
        context,
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
    let capture_again = format!("/next/screen/{}", device.id);
    let meta = if image.is_some() {
        html! { span class="chip muted" { "capture" } }
    } else {
        Markup::default()
    };
    let action = if image.is_some() {
        html! { a class="btn btn-sm" href=(capture_again) { "Capture again" } }
    } else {
        Markup::default()
    };
    let content = html! {
        p class="lead" { (detail) }
        section class="card" {
            (card_head("Screen", meta, action))
            @if let Some(image) = image {
                div class="card-body" {
                    img class="capture" src=(image) alt="the device screen";
                }
            } @else {
                (empty_state(
                    Page::Screen,
                    "No capture yet. The helper that takes one must be running in the user's session.",
                    html! { a class="btn btn-sm" href=(capture_again) { "Capture again" } },
                ))
            }
        }
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
    /// When the current run of identical probe results began, from the device's summary.
    pub(super) since_ms: Option<u64>,
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
    now_ms: u64,
    error: Option<&str>,
) -> Markup {
    let base = format!("/next/monitors/{}", device.id);
    let content = html! {
        p class="lead" {
            "The device probes each target on its own interval and keeps the last 60 probes. "
            "A down target is a fact, not a failure of this page."
        }
        @if let Some(error) = error {
            p class="alert error" { (error) }
        }
        section class="card" {
            (card_head(
                "Watched targets",
                html! { span class="chip muted" { (rows.len()) } },
                Markup::default(),
            ))
            @if rows.is_empty() {
                (empty_state(
                    Page::Monitors,
                    "No targets are watched yet.",
                    html! { a class="btn btn-sm" href="#watch" { "Watch a target" } },
                ))
            } @else {
                table class="table reflow monitors-table" {
                    caption class="visually-hidden" { "The targets this device watches." }
                    thead {
                        tr {
                            th scope="col" { "Target" }
                            th scope="col" { "Now" }
                            th scope="col" { "Last 60" }
                            th class="num" scope="col" { "Drops" }
                            th class="num" scope="col" { "Status" }
                            th class="num" scope="col" { span class="visually-hidden" { "Actions" } }
                        }
                    }
                    tbody {
                        @for row in rows {
                            tr {
                                td {
                                    code { (row.target) }
                                    div class="sub" {
                                        span class="chip muted" { (row.kind) }
                                        " every " (row.interval_ms / 1000) " s"
                                    }
                                }
                                td {
                                    @match row.up_now {
                                        Some(true) => { span class="chip online" { "up" } },
                                        Some(false) => { span class="chip failed" { "down" } },
                                        None => { span class="muted" { "—" } },
                                    }
                                    // "down" without "since when" is a state, not a story: the device's
                                    // summary carries the moment the current run began.
                                    @if let Some(since) = row.since_ms.filter(|since| *since > 0) {
                                        // `ago` reads "6h ago"; a duration after "for" drops the "ago".
                                        @let age = ago(now_ms, since);
                                        @let lasted = age.strip_suffix(" ago").unwrap_or(&age).to_owned();
                                        @let state = if row.up_now == Some(true) { "up" } else { "down" };
                                        div class="sub mono" title=(format!("{state} since {}", stamp(since))) {
                                            (state) " for " (lasted)
                                        }
                                    }
                                }
                                td {
                                    @if let Some(pct) = row.up_pct {
                                        div class="mini-usage" {
                                            span class="mini-bar" aria-hidden="true" {
                                                span class="bar-fill" style=(format!("width:{pct}%")) {}
                                            }
                                            span class=(format!("mono status {}", uptime_class(pct))) { (pct) "%" span class="visually-hidden" { " " (uptime_tone(pct)) } }
                                        }
                                    } @else {
                                        span class="muted" { "—" }
                                    }
                                }
                                td class="num" { (row.drops.map(|drops| drops.to_string()).unwrap_or_else(|| "—".to_owned())) }
                                td class="num mono" { (row.last_status.map(|status| status.to_string()).unwrap_or_else(|| "—".to_owned())) }
                                td class="num" {
                                    div class="form-inline" {
                                        form method="post" action=(format!("{base}/probe")) {
                                            input type="hidden" name="csrf" value=(csrf);
                                            input type="hidden" name="id" value=(row.id);
                                            button type="submit" class="btn btn-sm" { "Probe now" }
                                        }
                                        form method="post" action=(format!("{base}/remove")) {
                                            input type="hidden" name="csrf" value=(csrf);
                                            input type="hidden" name="id" value=(row.id);
                                            button type="submit" class="btn btn-sm btn-quiet-danger" aria-label=(format!("Stop watching {}", row.id)) { "Remove" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        section class="card" id="watch" {
            (card_head("Watch a target", Markup::default(), Markup::default()))
            div class="card-body" {
                form method="post" action=(format!("{base}/add")) {
                    input type="hidden" name="csrf" value=(csrf);
                    div class="form-grid" {
                        div class="field" {
                            label for="kind" { "Kind" }
                            select id="kind" name="kind" {
                                option value="tcp" { "tcp" }
                                option value="http" { "http" }
                            }
                        }
                        div class="field" {
                            label for="host" { "Host" }
                            input id="host" name="host" type="text" placeholder="192.168.1.1" required;
                        }
                        div class="field" {
                            label for="port" { "Port" }
                            input id="port" name="port" type="number" min="1" max="65535" required;
                        }
                        div class="field" {
                            label for="interval_ms" { "Interval (ms)" }
                            input id="interval_ms" name="interval_ms" type="number" value="15000" min="5000";
                        }
                        div class="field" {
                            label for="path" { "HTTP path (http only)" }
                            input id="path" name="path" type="text" placeholder="/";
                        }
                        div class="field" {
                            label for="expect" { "Body must contain (http only)" }
                            input id="expect" name="expect" type="text";
                        }
                    }
                    div class="form-actions" {
                        button type="submit" class="btn btn-primary" { "Add target" }
                    }
                }
            }
        }
    };
    let meta = if rows.is_empty() {
        Markup::default()
    } else {
        html! { span class="chip muted" { (rows.len()) } }
    };
    let body = if rows.is_empty() {
        html! { p class="rail-note" { "No target is watched yet." } }
    } else {
        html! {
            ul class="rail-list" {
                @for row in rows {
                    li {
                        span class="rail-line mono" { (&row.target) }
                        span class="rail-meta" {
                            (&row.kind)
                            " · "
                            @if let Some(up) = row.up_now {
                                @if up { span class="chip online" { "up" } } @else { span class="chip offline" { "down" } }
                            } @else { span class="chip muted" { "unknown" } }
                        }
                    }
                }
            }
        }
    };
    let context = context_rail("Targets", meta, body, Markup::default());
    page_with_rail(
        Chrome {
            active: Page::Monitors,
            device: Some(device),
            title: "Monitors",
            version,
            csrf,
            status: device_status(device),
        },
        Markup::default(),
        context,
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

/// The last expression the operator evaluated in a device's browser, and its answer. It lives in
/// the console's memory (not on a device, not in a file) so a page load right after the form can
/// show it without putting a megabyte into a URL.
#[derive(Clone)]
pub(super) struct LastEval {
    pub target: String,
    pub expression: String,
    /// The device's answer, pretty-printed and truncated for display.
    pub value: String,
}

/// The browser page: the targets, a capture of the first one, the forms, and the last evaluation.
pub(super) fn browser(
    device: &DeviceView,
    rows: &[BrowserRow],
    shot: Option<&str>,
    csrf: &str,
    version: &str,
    error: Option<&str>,
    last_eval: Option<&LastEval>,
) -> Markup {
    let base = format!("/next/browser/{}", device.id);
    let content = html! {
        p class="lead" {
            "The browser the device runs for you — headless by default, in the user's session only "
            "when SUMMRISE_BROWSER_VISIBLE is set and the helper is there. The capture is the first "
            "target in the device's list."
        }
        @if let Some(error) = error {
            p class="alert error" { (error) }
        }
        @if let Some(shot) = shot {
            section class="card" {
                (card_head(
                    "Capture",
                    html! { span class="chip muted" { "first target" } },
                    Markup::default(),
                ))
                div class="card-body" {
                    @if let Some(first) = rows.first() {
                        img id="cast" class="capture thumb" src=(shot) alt="the device's browser"
                            data-device=(device.id) data-target=(first.id);
                    } @else {
                        img class="capture thumb" src=(shot) alt="the device's browser";
                    }
                    p class="small muted" {
                        a class="btn btn-ghost btn-sm" href=(format!("/next/browser/{}", device.id)) { "Refresh the capture" }
                        " One frame a second while this tab is visible. With reduced motion on, the "
                        "capture stays still and this link loads a new one."
                    }
                }
            }
        }
        section class="card" {
            (card_head(
                "Targets",
                html! { span class="chip muted" { (rows.len()) } },
                html! {
                    details class="row-menu" {
                        summary class="btn btn-ghost btn-sm" { "Shut down" }
                        div class="menu-panel restart-panel" {
                            p class="small muted" {
                                "Ends the browser process and every tab in it."
                            }
                            form method="post" action=(format!("{base}/shutdown")) {
                                input type="hidden" name="csrf" value=(csrf);
                                button type="submit" class="btn btn-quiet-danger btn-sm btn-block" { "Shut down the browser" }
                            }
                        }
                    }
                },
            ))
            @if rows.is_empty() {
                (empty_state(
                    Page::Browser,
                    "The device has no browser targets.",
                    html! { a class="btn btn-sm" href="#open" { "Open a page" } },
                ))
            } @else {
                table class="table reflow" {
                    caption class="visually-hidden" { "The browser's open targets." }
                    thead {
                        tr {
                            th scope="col" { "Title" }
                            th scope="col" { "URL" }
                            th scope="col" { "Type" }
                            th class="num" scope="col" { span class="visually-hidden" { "Actions" } }
                        }
                    }
                    tbody {
                        @for row in rows {
                            tr {
                                td { (row.title) }
                                td { code class="url" { (row.url) } }
                                td { span class="chip muted" { (row.kind) } }
                                td class="num" {
                                    form method="post" action=(format!("{base}/close")) {
                                        input type="hidden" name="csrf" value=(csrf);
                                        input type="hidden" name="target" value=(row.id);
                                        button type="submit" class="btn btn-sm btn-quiet-danger" aria-label=(format!("Close {}", row.title)) { "Close" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        @if let Some(eval) = last_eval {
            section class="card" {
                (card_head(
                    "Last evaluation",
                    html! { span class="chip muted" { (eval.target) } },
                    Markup::default(),
                ))
                div class="card-body" {
                    p class="small muted" { "Expression" }
                    pre class="code-block" { (eval.expression) }
                    p class="small muted" { "Answer" }
                    pre class="code-block" { (eval.value) }
                }
            }
        }
        @if !rows.is_empty() {
            section class="card" {
                (card_head("Navigate a target", Markup::default(), Markup::default()))
                div class="card-body" {
                    form method="post" action=(format!("{base}/navigate")) class="form-inline" {
                        input type="hidden" name="csrf" value=(csrf);
                        div class="field" {
                            label class="visually-hidden" for="navigate-target" { "Target" }
                            select id="navigate-target" name="target" {
                                @for row in rows {
                                    option value=(row.id) { (row.title) }
                                }
                            }
                        }
                        div class="field grow" {
                            label class="visually-hidden" for="navigate-url" { "URL" }
                            input id="navigate-url" type="text" name="url" placeholder="https://example.com/" required;
                        }
                        button type="submit" class="btn" { "Go" }
                    }
                }
            }
        }
        section class="card" {
            (card_head("Evaluate in a page", Markup::default(), Markup::default()))
            div class="card-body" {
                @if rows.is_empty() {
                    p class="lead" { "Open a page first; an expression runs in a target's document." }
                } @else {
                    form method="post" action=(format!("{base}/eval")) {
                        input type="hidden" name="csrf" value=(csrf);
                        div class="field" {
                            label for="eval-target" { "Target" }
                            select id="eval-target" name="target" {
                                @for row in rows {
                                    option value=(row.id) { (row.title) " — " (row.url) }
                                }
                            }
                        }
                        div class="field" {
                            label for="eval-expression" { "JavaScript" }
                            textarea id="eval-expression" name="expression" rows="4" placeholder="document.title" {}
                        }
                        div class="form-actions" {
                            button type="submit" class="btn" { "Evaluate" }
                            span class="muted" { "Runs in the device's browser, not in this page." }
                        }
                    }
                }
            }
        }
        section class="card" id="open" {
            (card_head("Open a page", Markup::default(), Markup::default()))
            div class="card-body" {
                form method="post" action=(format!("{base}/open")) class="form-inline" {
                    input type="hidden" name="csrf" value=(csrf);
                    div class="field grow" {
                        label class="visually-hidden" for="browser-url" { "URL" }
                        input id="browser-url" type="text" name="url" placeholder="https://example.com" required;
                    }
                    button type="submit" class="btn btn-primary" { "Open" }
                }
            }
        }
    };
    let meta = if rows.is_empty() {
        Markup::default()
    } else {
        html! { span class="chip muted" { (rows.len()) } }
    };
    let body = if rows.is_empty() {
        html! { p class="rail-note" { "The browser has no open target." } }
    } else {
        html! {
            ul class="rail-list" {
                @for row in rows {
                    li {
                        span class="rail-line" { (&row.title) }
                        span class="rail-meta" { (&row.kind) " · " (&row.url) }
                    }
                }
            }
        }
    };
    let context = context_rail("Targets", meta, body, Markup::default());
    page_with_rail(
        Chrome {
            active: Page::Browser,
            device: Some(device),
            title: "Browser",
            version,
            csrf,
            status: device_status(device),
        },
        html! { script src="/assets/browser.js" defer {} },
        context,
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
        section class="card" {
            (card_head(
                "Connect an AI client",
                html! { span class="chip muted" { "MCP" } },
                Markup::default(),
            ))
            div class="card-body" {
                p class="lead" {
                    "A token minted here is one credential for this device and names every tool the "
                    "console knows. A destructive call without its consent is refused by the device; "
                    "this page holds the question for ninety seconds and then refuses it, never run. "
                    "The secret is shown once; the console keeps only its hash. The client connects "
                    "to this endpoint with an Authorization: Bearer header."
                }
                div class="endpoint" {
                    span class="label" { "Endpoint" }
                    code { "/mcp/" (device.id) }
                }
                @if let Some(error) = error {
                    p class="alert error" { (error) }
                }
                form method="post" action=(format!("{base}/mint")) class="form-inline" {
                    input type="hidden" name="csrf" value=(csrf);
                    div class="field grow" {
                        label class="visually-hidden" for="token-name" { "Client name" }
                        input id="token-name" type="text" name="name" placeholder="what this client is, e.g. dsh" required;
                    }
                    label class="check" {
                        input type="checkbox" name="allow_destructive" value="on";
                        "Allow destructive tools"
                    }
                    button type="submit" class="btn btn-primary" { "Mint a token" }
                }
            }
        }
        section class="card" {
            (card_head(
                "Tokens for this device",
                html! { span class="chip muted" { (rows.len()) } },
                Markup::default(),
            ))
            @if rows.is_empty() {
                (empty_state(
                    Page::Tokens,
                    "No token names this device yet.",
                    Markup::default(),
                ))
            } @else {
                table class="table tokens-table" {
                    caption class="visually-hidden" { "The tokens that name this device." }
                    thead {
                        tr {
                            th scope="col" { "Name" }
                            th class="num" scope="col" { "Tools" }
                            th scope="col" { "Access" }
                            th scope="col" { "Created" }
                            th class="num" scope="col" { span class="visually-hidden" { "Actions" } }
                        }
                    }
                    tbody {
                        @for row in rows {
                            tr {
                                td { (row.name) }
                                td class="num" { (row.tools) }
                                td {
                                    @if row.destructive {
                                        span class="chip brand" { "destructive allowed" }
                                    } @else {
                                        span class="chip muted" { "no destructive tools" }
                                    }
                                }
                                td class="nowrap" { (stamp(row.created_ms)) }
                                td class="num" {
                                    form method="post" action=(format!("{base}/revoke")) {
                                        input type="hidden" name="csrf" value=(csrf);
                                        input type="hidden" name="id" value=(row.id);
                                        button type="submit" class="btn btn-sm btn-quiet-danger" aria-label=(format!("Revoke the token {}", row.name)) { "Revoke" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        p class="footnote" {
            "Device " code title=(device.id) { (&device.id) }
        }
    };
    page(
        Chrome {
            active: Page::Tokens,
            device: Some(device),
            title: "Connect",
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
    let base = format!("/next/tokens/{}", device.id);
    let content = html! {
        (breadcrumb(&[("Connect".to_owned(), base)], "new token"))
        section class="card" {
            (card_head(
                "Token",
                html! { span class="chip brand" { (name) } },
                Markup::default(),
            ))
            div class="card-body" {
                p class="alert notice" {
                    "This is the only time the token is shown. Copy it into the client's configuration "
                    "now; a lost value means minting another."
                }
                pre class="secret" tabindex="0" { (secret) }
            }
        }
        section class="card" {
            (card_head("Client configuration", Markup::default(), Markup::default()))
            pre class="file" tabindex="0" { (config) }
        }
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

/// One point on the vitals trend: taken when this page is opened, kept in the console's memory, and
/// drawn only where it can be honest about what it is.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct VitalPoint {
    pub(super) memory_pct: u64,
    pub(super) disk_pct: u64,
}

/// One install generation the device reports: its version and channel, side by side.
#[derive(Clone)]
pub(super) struct Generation {
    pub(super) version: String,
    pub(super) channel: String,
}

/// The launcher's last swap, as the device reports it: what happened, to what, and why.
#[derive(Clone)]
pub(super) struct SwapView {
    /// `applied`, `rolled_back` or `refused`.
    pub(super) outcome: String,
    pub(super) version: Option<String>,
    pub(super) at_ms: u64,
    pub(super) reason: Option<String>,
}

/// One repair the launcher made, newest first.
#[derive(Clone)]
pub(super) struct RepairView {
    pub(super) from: String,
    pub(super) at_ms: u64,
}

/// The tray binary this install has: present, its digest, and its size.
#[derive(Clone)]
pub(super) struct ShellView {
    pub(super) sha256: String,
    pub(super) bytes: u64,
}

/// One boot the device journaled: when, and from which release.
#[derive(Clone)]
pub(super) struct RestartView {
    pub(super) at_ms: u64,
    pub(super) version: String,
    pub(super) channel: String,
}

/// The install's generations and the launcher's last answers. The console reads them from
/// `host.facts`; it keeps no copy of its own.
pub(super) struct InstallView {
    pub(super) current: Option<Generation>,
    pub(super) previous: Option<Generation>,
    pub(super) pending: Option<Generation>,
    pub(super) swap: Option<SwapView>,
    pub(super) repairs: Vec<RepairView>,
    /// This app's own recent boots, newest first. The launcher says why a generation changed; this
    /// says when the device last came up, which is how a restart loop becomes visible.
    pub(super) restarts: Vec<RestartView>,
    /// The installed tray binary, when the machine has one. Its digest is what tells an operator
    /// whether the icon is the build they shipped: the canary update does not carry the shell.
    pub(super) shell: Option<ShellView>,
}

/// What the device reports about itself right now, plus the points this console has collected by
/// opening this page. The series is the page's own: one point per visit, console memory only, and
/// the trend card says so rather than implying a sampler runs in the background.
pub(super) struct Vitals {
    pub(super) machine: String,
    pub(super) os: String,
    pub(super) cores: u64,
    /// The device's CPU busy share when it reported one; the page omits the row rather than guessing.
    pub(super) cpu_percent: Option<u64>,
    pub(super) uptime_s: u64,
    pub(super) memory_total: u64,
    pub(super) memory_available: u64,
    pub(super) disks: Vec<VitalsDisk>,
    pub(super) version: String,
    pub(super) channel: String,
    pub(super) trend: Vec<VitalPoint>,
    pub(super) install: InstallView,
}

/// A generation as one line: `0.3.113 (canary)`, or the word for an absent one.
fn generation_line(generation: Option<&Generation>) -> Markup {
    match generation {
        Some(generation) => html! {
            span class="mono" { (generation.version) }
            span class="muted" { " (" (generation.channel) ")" }
        },
        None => html! { span class="muted" { "no generation" } },
    }
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

/// Uptime at the resolution a reader needs: days when there are days, minutes when there are not.
fn uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{days} d {hours} h")
    } else if hours > 0 {
        format!("{hours} h {minutes} m")
    } else {
        format!("{minutes} m")
    }
}

/// A 0–100 percentage series as an SVG polyline. Fixed scale on purpose: a self-scaling line makes a
/// 2% wobble look like a cliff, and this chart is read at a glance.
fn sparkline(points: &[VitalPoint], pick: impl Fn(&VitalPoint) -> u64) -> Markup {
    let values: Vec<u64> = points.iter().map(|point| pick(point).min(100)).collect();
    let last = (values.len().saturating_sub(1)) as f64;
    let coordinates: Vec<String> = values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let x = if values.len() <= 1 {
                0.0
            } else {
                index as f64 * 100.0 / last
            };
            let y = 28.0 - (*value as f64) * 28.0 / 100.0;
            format!("{x:.1},{y:.1}")
        })
        .collect();
    html! {
        svg class="spark" viewBox="0 0 100 28" preserveAspectRatio="none" aria-hidden="true" {
            polyline points=(coordinates.join(" ")) {}
        }
    }
}

fn trend_row(label: &str, points: &[VitalPoint], pick: impl Fn(&VitalPoint) -> u64) -> Markup {
    let values: Vec<u64> = points.iter().map(&pick).collect();
    let now = values.last().copied().unwrap_or(0);
    let min = values.iter().copied().min().unwrap_or(0);
    let max = values.iter().copied().max().unwrap_or(0);
    html! {
        div class="trend-row" {
            div class="trend-label" {
                span { (label) }
                span class="usage-sub" { (points.len()) " samples" }
            }
            (sparkline(points, pick))
            div class="trend-values mono" {
                "now " (now) "% · min " (min) "% · max " (max) "%"
            }
        }
    }
}

fn bar(used_pct: u64) -> Markup {
    html! {
        div class="bar" aria-hidden="true" {
            div class="bar-fill" style=(format!("width:{used_pct}%")) {}
        }
    }
}

/// The vitals page: one sample of what the device reports, as stat cards and usage sections.
pub(super) fn vitals(device: &DeviceView, sample: &Vitals, csrf: &str, version: &str) -> Markup {
    let memory_used = sample.memory_total.saturating_sub(sample.memory_available);
    let memory_pct = percent(memory_used, sample.memory_total);
    let content = html! {
        p class="lead" {
            "One sample, read from the device now. The trend below is built from this page's own "
            "visits and says so: the console does not sample in the background."
        }
        div class="stats" {
            div class="stat" {
                p class="label" { "Machine" }
                p class="value" title=(sample.machine) { (sample.machine) }
            }
            div class="stat" {
                p class="label" { "Operating system" }
                p class="value" title=(sample.os) { (sample.os) }
            }
            div class="stat" {
                p class="label" { "Cores" }
                p class="value" { (sample.cores) }
            }
            @if let Some(cpu) = sample.cpu_percent {
                div class="stat" {
                    p class="label" { "CPU busy" }
                    // A share of a short sample, said as one: "37%" is the answer to "is this machine
                    // working", and the title says what it is measured over.
                    p class="value" title="busy share over the device's last short sample" { (cpu) "%" }
                }
            }
            div class="stat" {
                p class="label" { "Uptime" }
                p class="value" { (uptime(sample.uptime_s)) }
            }
            div class="stat" {
                p class="label" { "Build" }
                p class="value mono" { (sample.version) " (" (sample.channel) ")" }
            }
        }
        section class="card" {
            (card_head(
                "Trend",
                html! { span class="chip muted" { (sample.trend.len()) } },
                Markup::default(),
            ))
            div class="card-body" {
                @if sample.trend.len() < 2 {
                    p class="lead" {
                        "One point so far. The console takes a point each time this page is opened, "
                        "keeps them in its own memory, and loses them when it restarts — there is no "
                        "background sampler behind this line."
                    }
                } @else {
                    (trend_row("Memory", &sample.trend, |point| point.memory_pct))
                    (trend_row("Disks", &sample.trend, |point| point.disk_pct))
                    p class="footnote" {
                        "One point per time this page was opened; the console's memory only, lost on "
                        "restart. Not a sampler."
                    }
                }
            }
        }
        section class="card" {
            (card_head(
                "Install",
                html! {
                    @if let Some(generation) = &sample.install.current {
                        span class="chip muted" { (generation.version) }
                    }
                },
                Markup::default(),
            ))
            div class="card-body" {
                dl class="facts" {
                    dt { "Current" }
                    dd { (generation_line(sample.install.current.as_ref())) }
                    dt { "Previous" }
                    dd { (generation_line(sample.install.previous.as_ref())) }
                    dt { "Pending" }
                    dd { (generation_line(sample.install.pending.as_ref())) }
                }
                @if let Some(swap) = &sample.install.swap {
                    p class="small muted" { "Last swap" }
                    div class="form-inline" {
                        @match swap.outcome.as_str() {
                            "applied" => { span class="chip online" { "applied" } },
                            "rolled_back" => { span class="chip warn" { "rolled back" } },
                            _ => { span class="chip failed" { (swap.outcome) } },
                        }
                        @if let Some(version) = &swap.version {
                            span class="mono small" { (version) }
                        }
                        span class="muted small" { (stamp(swap.at_ms)) }
                    }
                    @if let Some(reason) = &swap.reason {
                        p class="small muted" { (reason) }
                    }
                }
                @if !sample.install.repairs.is_empty() {
                    p class="small muted" { "Repairs" }
                    ul class="rail-list" {
                        @for repair in &sample.install.repairs {
                            li {
                                span class="rail-line mono" { "from " (repair.from) }
                                span class="rail-meta" { (stamp(repair.at_ms)) }
                            }
                        }
                    }
                }
                @if let Some(shell) = &sample.install.shell {
                    dt { "Tray" }
                    dd {
                        span class="mono small" { (shell.sha256) }
                        span class="muted small" { " · " (shell.bytes) " bytes" }
                    }
                }
                @if !sample.install.restarts.is_empty() {
                    p class="small muted" { "Restarts" }
                    ul class="rail-list" {
                        @for restart in &sample.install.restarts {
                            li {
                                span class="rail-line mono" {
                                    (restart.version) " (" (restart.channel) ")"
                                }
                                span class="rail-meta" { (stamp(restart.at_ms)) }
                            }
                        }
                    }
                }
                p class="footnote" {
                    "Read from the device's install sidecars and its own boot journal: what it has, "
                    "what it last tried, what the launcher repaired, and when it last came up. The "
                    "console keeps no copy."
                }
            }
        }
        section class="card" {
            (card_head(
                "Memory",
                html! { span class="chip muted" { (memory_pct) "% used" } },
                Markup::default(),
            ))
            div class="card-body" {
                div class="usage" {
                    div class="usage-line" {
                        span { (gib(memory_used)) " used of " (gib(sample.memory_total)) }
                        strong { (memory_pct) "%" }
                    }
                    (bar(memory_pct))
                }
                p class="usage-sub" { (gib(sample.memory_available)) " available" }
            }
        }
        section class="card" {
            (card_head(
                "Disks",
                html! { span class="chip muted" { (sample.disks.len()) } },
                Markup::default(),
            ))
            @if sample.disks.is_empty() {
                (empty_state(Page::Vitals, "The device reported no disks.", Markup::default()))
            } @else {
                div class="card-body" {
                    @for disk in &sample.disks {
                        @let used = disk.total.saturating_sub(disk.free);
                        @let pct = percent(used, disk.total);
                        div class="usage-row" {
                            div class="usage-name" {
                                code { (&disk.name) }
                                span class="usage-sub" { (gib(disk.free)) " free" }
                            }
                            (bar(pct))
                            div class="usage-value" {
                                strong { (pct) "%" }
                                span { (gib(used)) " of " (gib(disk.total)) }
                            }
                        }
                    }
                }
            }
        }
    };
    let mut disks: Vec<&VitalsDisk> = sample.disks.iter().collect();
    disks.sort_by_key(|disk| std::cmp::Reverse(disk.total.saturating_sub(disk.free)));
    let meta = if sample.disks.is_empty() {
        Markup::default()
    } else {
        html! { span class="chip muted" { (sample.disks.len()) } }
    };
    let body = if sample.disks.is_empty() {
        html! { p class="rail-note" { "The device reported no disks." } }
    } else {
        html! {
            ul class="rail-list" {
                @for disk in disks {
                    li {
                        span class="rail-line mono" { (&disk.name) }
                        span class="rail-meta" { (percent(disk.total.saturating_sub(disk.free), disk.total)) "% used · " (gib(disk.free)) " free" }
                    }
                }
            }
        }
    };
    let context = context_rail("Disks", meta, body, Markup::default());
    page_with_rail(
        Chrome {
            active: Page::Vitals,
            device: Some(device),
            title: "Vitals",
            version,
            csrf,
            status: device_status(device),
        },
        Markup::default(),
        context,
        content,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rail's labels, in order, as a reader sees them.
    const LABELS: [&str; 11] = [
        "Fleet", "Terminal", "Files", "Runs", "System", "Memory", "Screen", "Monitors", "Browser",
        "Connect", "Vitals",
    ];

    /// A stand-in capture for the review renderer, so the screen and browser cards have pixels.
    const CAPTURE: &str = concat!(
        "data:image/svg+xml;base64,",
        "PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSI5NjAiIGhlaWdodD0iNjAwIiB2aWV3Qm94",
        "PSIwIDAgOTYwIDYwMCI+CiAgPGRlZnM+CiAgICA8bGluZWFyR3JhZGllbnQgaWQ9ImJnIiB4MT0iMCIgeTE9IjAiIHgyPSIx",
        "IiB5Mj0iMSI+CiAgICAgIDxzdG9wIG9mZnNldD0iMCIgc3RvcC1jb2xvcj0iIzFjMjczMyIvPgogICAgICA8c3RvcCBvZmZz",
        "ZXQ9IjEiIHN0b3AtY29sb3I9IiMyYTFjMTgiLz4KICAgIDwvbGluZWFyR3JhZGllbnQ+CiAgPC9kZWZzPgogIDxyZWN0IHdp",
        "ZHRoPSI5NjAiIGhlaWdodD0iNjAwIiBmaWxsPSJ1cmwoI2JnKSIvPgogIDxyZWN0IHg9IjQ4IiB5PSI0MCIgd2lkdGg9Ijg2",
        "NCIgaGVpZ2h0PSI1MjAiIHJ4PSIxMCIgZmlsbD0iIzBkMTExNyIgc3Ryb2tlPSIjMzAzNjNkIi8+CiAgPGNpcmNsZSBjeD0i",
        "NzYiIGN5PSI2OCIgcj0iNSIgZmlsbD0iI2ZmNWY1NiIvPgogIDxjaXJjbGUgY3g9Ijk0IiBjeT0iNjgiIHI9IjUiIGZpbGw9",
        "IiNmZmJkMmUiLz4KICA8Y2lyY2xlIGN4PSIxMTIiIGN5PSI2OCIgcj0iNSIgZmlsbD0iIzI3YzkzZiIvPgogIDx0ZXh0IHg9",
        "IjQ4IiB5PSI5OCIgZmlsbD0iIzhiOTQ5ZSIgZm9udC1mYW1pbHk9Im1vbm9zcGFjZSIgZm9udC1zaXplPSIxMSIgbGV0dGVy",
        "LXNwYWNpbmc9IjIiPkRFU0tUT1AtOVZIOTJCSiDCtyBQT1dFUlNIRUxMPC90ZXh0PgogIDx0ZXh0IHg9Ijc2IiB5PSIxNTAi",
        "IGZpbGw9IiNjOWQxZDkiIGZvbnQtZmFtaWx5PSJtb25vc3BhY2UiIGZvbnQtc2l6ZT0iMTciPlBTIEM6XFVzZXJzXG9wZXJh",
        "dG9yJmd0OyBHZXQtRGF0ZTwvdGV4dD4KICA8dGV4dCB4PSI3NiIgeT0iMTgyIiBmaWxsPSIjOGI5NDllIiBmb250LWZhbWls",
        "eT0ibW9ub3NwYWNlIiBmb250LXNpemU9IjE3Ij5Nb25kYXksIDI3IE9jdG9iZXIgMjAyNiAxMjowMDowMDwvdGV4dD4KICA8",
        "dGV4dCB4PSI3NiIgeT0iMjMwIiBmaWxsPSIjYzlkMWQ5IiBmb250LWZhbWlseT0ibW9ub3NwYWNlIiBmb250LXNpemU9IjE3",
        "Ij5QUyBDOlxVc2Vyc1xvcGVyYXRvciZndDsgR2V0LVNlcnZpY2UgU3Bvb2xlcjwvdGV4dD4KICA8dGV4dCB4PSI3NiIgeT0i",
        "MjYyIiBmaWxsPSIjOGI5NDllIiBmb250LWZhbWlseT0ibW9ub3NwYWNlIiBmb250LXNpemU9IjE3Ij5SdW5uaW5nICAgU3Bv",
        "b2xlciAgIFByaW50IFNwb29sZXI8L3RleHQ+CiAgPHRleHQgeD0iNzYiIHk9IjMxMCIgZmlsbD0iI2M5ZDFkOSIgZm9udC1m",
        "YW1pbHk9Im1vbm9zcGFjZSIgZm9udC1zaXplPSIxNyI+UFMgQzpcVXNlcnNcb3BlcmF0b3ImZ3Q7IF88L3RleHQ+Cjwvc3Zn",
        "Pgo=",
    );

    fn view(tag: char, name: Option<&str>, online: bool) -> DeviceView {
        DeviceView {
            id: tag.to_string().repeat(64),
            public_key: "00".repeat(32),
            name: name.map(str::to_owned),
            registered_at_ms: 0,
            last_seen_ms: 0,
            online,
            version: Some("0.3.72".to_owned()),
            host: None,
            last_attempt: None,
        }
    }

    fn sample_vitals() -> Vitals {
        Vitals {
            machine: "STUDIO-PC".to_owned(),
            os: "Windows 11 10.0.22631".to_owned(),
            cores: 16,
            cpu_percent: Some(37),
            uptime_s: 3 * 86_400 + 14 * 3_600,
            memory_total: 8 * 1024 * 1024 * 1024,
            memory_available: 4 * 1024 * 1024 * 1024,
            disks: vec![
                VitalsDisk {
                    name: "C:\\".to_owned(),
                    total: 512 * 1024 * 1024 * 1024,
                    free: 128 * 1024 * 1024 * 1024,
                },
                VitalsDisk {
                    name: "D:\\".to_owned(),
                    total: 2048 * 1024 * 1024 * 1024,
                    free: 1434 * 1024 * 1024 * 1024,
                },
            ],
            version: "0.3.72".to_owned(),
            channel: "canary".to_owned(),
            install: InstallView {
                current: Some(Generation {
                    version: "0.3.72".to_owned(),
                    channel: "stable".to_owned(),
                }),
                previous: Some(Generation {
                    version: "0.3.71".to_owned(),
                    channel: "stable".to_owned(),
                }),
                pending: None,
                swap: Some(SwapView {
                    outcome: "rolled_back".to_owned(),
                    version: Some("0.3.73".to_owned()),
                    at_ms: 1_790_990_000_000,
                    reason: Some("the new build did not answer /health in 30 s".to_owned()),
                }),
                repairs: vec![RepairView {
                    from: "previous".to_owned(),
                    at_ms: 1_790_990_300_000,
                }],
                shell: Some(ShellView {
                    sha256: "ab".repeat(32),
                    bytes: 1_411_072,
                }),
                restarts: vec![
                    RestartView {
                        at_ms: 1_790_991_000_000,
                        version: "0.3.72".to_owned(),
                        channel: "stable".to_owned(),
                    },
                    RestartView {
                        at_ms: 1_790_990_000_000,
                        version: "0.3.71".to_owned(),
                        channel: "stable".to_owned(),
                    },
                ],
            },
            trend: vec![
                VitalPoint {
                    memory_pct: 42,
                    disk_pct: 61,
                },
                VitalPoint {
                    memory_pct: 46,
                    disk_pct: 61,
                },
                VitalPoint {
                    memory_pct: 51,
                    disk_pct: 62,
                },
                VitalPoint {
                    memory_pct: 58,
                    disk_pct: 62,
                },
                VitalPoint {
                    memory_pct: 53,
                    disk_pct: 63,
                },
                VitalPoint {
                    memory_pct: 50,
                    disk_pct: 63,
                },
            ],
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
                dashboard(
                    std::slice::from_ref(device),
                    "00",
                    "0.1.0",
                    Notes::default(),
                    &[],
                    &[],
                    1_791_000_000_000,
                ),
            ),
            (
                "Fleet",
                logs(
                    &[],
                    "Audit",
                    0,
                    None,
                    "00",
                    "0.1.0",
                    "The console's own record, newest first. Times are UTC; a device id is shortened here but named in full by the line's tooltip.",
                    "The console's own audit lines.",
                    true,
                ),
            ),
            (
                "Fleet",
                logs(
                    &[],
                    "Denials",
                    0,
                    None,
                    "00",
                    "0.1.0",
                    "Every request the console refused before authentication: a missing session, an unknown token, an unauthenticated probe. A refusal is not a device call, and the arguments it carried are not stored.",
                    "Refusals recorded before authentication.",
                    false,
                ),
            ),
            (
                "Terminal",
                terminal(
                    device,
                    "00",
                    "0.1.0",
                    TerminalView {
                        sessions: &[],
                        presets: &[],
                        preset: None,
                        activity: &[],
                        error: None,
                    },
                ),
            ),
            ("Files", files(device, "", &[], "00", "0.1.0", None)),
            (
                "Files",
                file_text(device, "notes.txt", "text", "00", "0.1.0"),
            ),
            ("Runs", runs(device, &[], None, "00", "0.1.0", None)),
            ("Runs", run_result(device, "cmd", &run, "00", "0.1.0")),
            ("System", state_document(device, "{}", "00", "0.1.0")),
            ("System", system(device, &[], "", "00", "0.1.0", None)),
            (
                "Memory",
                memory(
                    device,
                    &[],
                    MemoryLook {
                        tag: "",
                        query: "",
                        editing: None,
                    },
                    "00",
                    "0.1.0",
                    None,
                ),
            ),
            ("Screen", screen(device, None, "no screen", "00", "0.1.0")),
            (
                "Monitors",
                monitors(device, &[], "00", "0.1.0", 1_791_000_000_000, None),
            ),
            (
                "Browser",
                browser(device, &[], None, "00", "0.1.0", None, None),
            ),
            ("Connect", tokens(device, &[], "00", "0.1.0", None)),
            (
                "Connect",
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
    fn one_path_segment_stays_one_segment() {
        assert_eq!(encode_segment("session-3"), "session-3");
        assert_eq!(encode_segment("a/b"), "a%2Fb");
        assert_eq!(encode_segment("a b"), "a%20b");
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
            // Fleet, Audit and Denials are the device-less pages: their rail is the fleet alone,
            // because a device page's href has no device to name. Every other page lists them all.
            let device_less = expected == "Fleet";
            let want = if device_less { 1 } else { LABELS.len() };
            assert!(
                page.matches("<a class=\"rail-item").count() == want,
                "{expected}: {want} anchor(s) in the rail: {page}"
            );
            let labels: &[&str] = if device_less {
                &["Fleet"]
            } else {
                LABELS.as_slice()
            };
            for label in labels {
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

    /// An offline device says WHY in the row, because the reason is already on the wire and the logs
    /// are where "offline" alone sends the operator.
    #[test]
    fn an_offline_device_shows_how_its_last_attempt_ended() {
        let mut offline = view('3', Some("workshop"), false);
        offline.last_attempt = Some(super::super::RetryView {
            attempts: 3,
            reason: "the console accepted the channel and then dropped it".to_owned(),
        });
        let mut online = view('4', Some("studio"), true);
        online.last_attempt = Some(super::super::RetryView {
            attempts: 7,
            reason: "a stale note from before it connected".to_owned(),
        });
        let devices = [offline, online];
        let page = dashboard(
            &devices,
            "00",
            "0.1.0",
            Notes::default(),
            &[],
            &[],
            1_791_000_000_000,
        )
        .into_string();

        assert!(
            page.contains("last try 3×"),
            "the row says how many attempts ended badly: {page}"
        );
        assert!(
            page.contains("accepted the channel and then dropped it"),
            "the row says how the attempt ended: {page}"
        );
        assert!(
            !page.contains("stale note"),
            "a device that is online does not report an old failure: {page}"
        );
    }

    #[test]
    fn the_fleet_renders_one_row_per_device_with_every_page_link() {
        let online = view('1', Some("studio"), true);
        let offline = view('2', None, false);
        let ids = [online.id.clone(), offline.id.clone()];
        let devices = [online, offline];
        let page = dashboard(
            &devices,
            "00",
            "0.1.0",
            Notes::default(),
            &[],
            &[],
            1_791_000_000_000,
        )
        .into_string();

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

    /// **A HELD CALL IS DECIDED IN NINETY SECONDS.** The card used to show raw JSON and nothing
    /// else; the sentence says what the call does, and the JSON stays under it as the evidence.
    #[test]
    fn a_held_call_says_what_it_does_before_the_json() {
        let cases = [
            (
                "terminal.create",
                r#"{"kind":"pty"}"#,
                Some("Start a pty session: a shell on the device."),
            ),
            (
                "terminal.create",
                r#"{"kind":"ssh","host":"10.0.0.5","port":2222,"user":"root"}"#,
                Some("Open an ssh session as root@10.0.0.5:2222."),
            ),
            (
                "terminal.create",
                r#"{"kind":"serial","port":"COM3","baud":9600}"#,
                Some("Open the serial port COM3 at 9600 baud."),
            ),
            (
                "files.write",
                r#"{"path":"notes.txt","content":"hi"}"#,
                Some("Write 2 bytes to notes.txt."),
            ),
            (
                "memory.delete",
                r#"{"id":"m1"}"#,
                Some("Forget memory entry m1."),
            ),
            (
                "system.process_kill",
                r#"{"pid":4242}"#,
                Some("End process 4242."),
            ),
            (
                "runs.exec",
                r#"{"command":"cmd","args":["/c","dir"],"cwd":"work"}"#,
                Some("Run cmd /c dir in work."),
            ),
            ("audit.export", "{}", None),
        ];
        for (tool, arguments, expected) in cases {
            assert_eq!(
                approval_sentence(tool, arguments).as_deref(),
                expected,
                "{tool} {arguments}"
            );
        }
    }

    /// **A DEVICE THAT DID NOT ANSWER IS NOT A DEVICE WITH NOTHING ON IT.** Four empty states said
    /// "no sessions", "no runs", "nothing remembered", "no processes" when the call had failed —
    /// the copy audit's remaining finding. The error suppresses the empty sentence, and the
    /// operator's own message (a refusal, a kill result) still wins over the list failure.
    #[test]
    fn an_unanswered_device_is_not_an_empty_one() {
        let device = view('1', Some("studio"), true);
        let failure = Some("The device did not answer.");

        let runs_page = runs(&device, &[], None, "00", "0.1.0", failure).into_string();
        assert!(
            runs_page.contains("The device did not answer."),
            "{runs_page}"
        );
        assert!(!runs_page.contains("No runs recorded yet."), "{runs_page}");

        let memory_page = memory(
            &device,
            &[],
            MemoryLook {
                tag: "",
                query: "",
                editing: None,
            },
            "00",
            "0.1.0",
            failure,
        )
        .into_string();
        assert!(
            !memory_page.contains("Nothing is remembered yet."),
            "{memory_page}"
        );

        let system_page = system(&device, &[], "", "00", "0.1.0", failure).into_string();
        assert!(
            !system_page.contains("The device listed no processes."),
            "{system_page}"
        );

        let terminal_page = terminal(
            &device,
            "00",
            "0.1.0",
            TerminalView {
                sessions: &[],
                presets: &[],
                preset: None,
                activity: &[],
                error: failure,
            },
        )
        .into_string();
        assert!(
            terminal_page.contains("The device did not answer."),
            "{terminal_page}"
        );
        assert!(
            !terminal_page.contains("No live session on this device."),
            "{terminal_page}"
        );
    }

    #[test]
    fn the_terminal_rail_lists_the_devices_live_sessions() {
        let device = view('1', Some("studio"), true);
        let sessions = [
            TerminalSession {
                id: "session-3".to_owned(),
                kind: "pty".to_owned(),
                started_at_ms: 1_791_000_000_000,
            },
            TerminalSession {
                id: "session-7".to_owned(),
                kind: "ssh".to_owned(),
                started_at_ms: 1_791_000_500_000,
            },
        ];
        let page = terminal(
            &device,
            "00",
            "0.1.0",
            TerminalView {
                sessions: &sessions,
                presets: &[],
                preset: None,
                activity: &[],
                error: None,
            },
        )
        .into_string();
        assert!(page.contains("class=\"context-rail\""), "{page}");
        assert!(
            page.contains("?attach=session-3"),
            "a rail row attaches: {page}"
        );
        assert!(page.contains("?attach=session-7"), "{page}");
        assert!(page.contains(">pty<") && page.contains(">ssh<"), "{page}");

        // An empty device says so rather than showing an empty rail.
        let empty = terminal(
            &device,
            "00",
            "0.1.0",
            TerminalView {
                sessions: &[],
                presets: &[],
                preset: None,
                activity: &[],
                error: None,
            },
        )
        .into_string();
        assert!(empty.contains("No live session on this device"), "{empty}");
        assert!(!empty.contains("?attach="), "{empty}");
    }

    #[test]
    fn the_terminal_page_keeps_its_form_and_tabs() {
        let device = view('1', Some("studio"), true);
        let page = terminal(
            &device,
            "ab".repeat(32).as_str(),
            "0.1.0",
            TerminalView {
                sessions: &[],
                presets: &[],
                preset: None,
                activity: &[],
                error: None,
            },
        )
        .into_string();

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

    /// A review harness: write each page to `/tmp/pages` so a browser can be pointed at the real
    /// markup. Ignored, because it is a tool for the eyes rather than a gate.
    #[test]
    #[ignore = "writes pages for a browser to look at"]
    fn render_pages_for_review() {
        // `SUMMRISE_PAGE_DIR` lets the screenshot script render somewhere it owns; the default is
        // the directory everyone already looks in.
        let out = std::env::var("SUMMRISE_PAGE_DIR").unwrap_or_else(|_| "/tmp/pages".to_owned());
        let dir = std::path::Path::new(&out);
        std::fs::create_dir_all(dir).expect("the review directory");
        let mut devices = vec![
            view('a', Some("STUDIO-PC"), true),
            view('b', Some("LAB-PC"), true),
            view('c', None, false),
        ];
        devices[0].last_seen_ms = 1_791_000_000_000 - 6 * 60 * 1000;
        devices[1].last_seen_ms = 1_791_000_000_000 - 3 * 3600 * 1000;
        devices[2].last_seen_ms = 1_791_000_000_000 - 2 * 86_400 * 1000;
        let id = devices[0].id.clone();
        let entries = vec![
            FileEntry {
                name: "config".to_owned(),
                kind: "directory".to_owned(),
                len: 0,
            },
            FileEntry {
                name: "logs".to_owned(),
                kind: "directory".to_owned(),
                len: 0,
            },
            FileEntry {
                name: "notes.txt".to_owned(),
                kind: "file".to_owned(),
                len: 1_842,
            },
            FileEntry {
                name: "report-2026-10-27.txt".to_owned(),
                kind: "file".to_owned(),
                len: 65_210,
            },
        ];
        let log_entries = vec![
            FileEntry {
                name: "agent.log".to_owned(),
                kind: "file".to_owned(),
                len: 148_211,
            },
            FileEntry {
                name: "updater.log".to_owned(),
                kind: "file".to_owned(),
                len: 9_120,
            },
        ];
        let runs_rows = vec![
            RunRow {
                id: format!("run-{}", 0),
                command_line: "pwsh -NoProfile Get-Date".to_owned(),
                started_at_ms: 1_793_059_200_000,
                duration_ms: 412,
                exit_code: Some(0),
                timed_out: false,
            },
            RunRow {
                id: format!("run-{}", 0),
                command_line: "Get-Service Spooler".to_owned(),
                started_at_ms: 1_793_058_100_000,
                duration_ms: 286,
                exit_code: Some(0),
                timed_out: false,
            },
            RunRow {
                id: format!("run-{}", 0),
                command_line: "ping 10.0.0.1 -n 4".to_owned(),
                started_at_ms: 1_793_057_000_000,
                duration_ms: 30_000,
                exit_code: None,
                timed_out: true,
            },
            RunRow {
                id: format!("run-{}", 0),
                command_line: "Get-ChildItem C:\\Missing".to_owned(),
                started_at_ms: 1_793_056_000_000,
                duration_ms: 190,
                exit_code: Some(1),
                timed_out: false,
            },
        ];
        let run = RunResult {
            id: "run-2".to_owned(),
            exit_code: Some(0),
            timed_out: false,
            duration_ms: 412,
            stdout: "Monday, 27 October 2026 12:00:00\n\nPS C:\\Users\\operator>\n".to_owned(),
            stderr: "WARNING: the spooler is not running.\n".to_owned(),
            truncated: false,
        };
        let processes = vec![
            ProcessRow {
                pid: 1204,
                name: "pwsh.exe".to_owned(),
                memory_kb: 96_432,
            },
            ProcessRow {
                pid: 880,
                name: "explorer.exe".to_owned(),
                memory_kb: 240_112,
            },
            ProcessRow {
                pid: 4422,
                name: "msedge.exe".to_owned(),
                memory_kb: 512_900,
            },
            ProcessRow {
                pid: 1712,
                name: "svchost.exe".to_owned(),
                memory_kb: 38_400,
            },
            ProcessRow {
                pid: 2330,
                name: "Summrise.Agent.exe".to_owned(),
                memory_kb: 74_100,
            },
        ];
        let memory_rows = vec![
            MemoryRow {
                id: "mem-1".to_owned(),
                text: "The antenna is on COM3; 9600 8N1.".to_owned(),
                tags: "hardware, serial".to_owned(),
            },
            MemoryRow {
                id: "mem-2".to_owned(),
                text: "Backups live on D:\\backups; keep 14 days.".to_owned(),
                tags: "ops, backup".to_owned(),
            },
        ];
        let monitors_rows = vec![
            MonitorRow {
                id: "mon-1".to_owned(),
                kind: "tcp".to_owned(),
                target: "10.0.0.1:443".to_owned(),
                interval_ms: 15_000,
                up_now: Some(true),
                since_ms: Some(1_791_000_000_000 - 6 * 3600 * 1000),
                up_pct: Some(100),
                drops: Some(0),
                last_status: None,
            },
            MonitorRow {
                id: "mon-2".to_owned(),
                kind: "http".to_owned(),
                target: "192.168.1.1:80".to_owned(),
                interval_ms: 30_000,
                up_now: Some(true),
                since_ms: Some(1_791_000_000_000 - 40 * 60 * 1000),
                up_pct: Some(95),
                drops: Some(3),
                last_status: Some(200),
            },
            MonitorRow {
                id: "mon-3".to_owned(),
                kind: "tcp".to_owned(),
                target: "10.0.0.9:22".to_owned(),
                interval_ms: 10_000,
                up_now: Some(false),
                since_ms: Some(1_791_000_000_000 - 12 * 60 * 1000),
                up_pct: Some(41),
                drops: Some(12),
                last_status: None,
            },
        ];
        let browser_rows = vec![
            BrowserRow {
                id: "target-1".to_owned(),
                title: "Summrise console".to_owned(),
                url: "http://127.0.0.1:19000/next".to_owned(),
                kind: "page".to_owned(),
            },
            BrowserRow {
                id: "target-2".to_owned(),
                title: "Example Domain".to_owned(),
                url: "https://example.com/".to_owned(),
                kind: "page".to_owned(),
            },
        ];
        let token_rows = vec![
            TokenRow {
                id: "token-1".to_owned(),
                name: "dsh".to_owned(),
                tools: 27,
                destructive: true,
                created_ms: 1_793_000_000_000,
            },
            TokenRow {
                id: "token-2".to_owned(),
                name: "readonly-dashboard".to_owned(),
                tools: 12,
                destructive: false,
                created_ms: 1_792_900_000_000,
            },
        ];
        let secret = "srm_2f9a8c7b6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d1e0f9a";
        let config = format!(
            "{{\n  \"mcpServers\": {{\n    \"desk\": {{\n      \"url\": \"http://127.0.0.1:19000/mcp/{}\",\n      \"headers\": {{ \"Authorization\": \"Bearer {}\" }}\n    }}\n  }}\n}}",
            id, secret
        );
        let log_rows = vec![
            serde_json::json!({"ts_ms": 1_793_059_200_000u64, "kind": "tool", "status": 200, "device_id": id, "tool": "runs.exec", "detail": "pwsh -NoProfile Get-Date"}),
            serde_json::json!({"ts_ms": 1_793_058_700_000u64, "kind": "tool", "status": 200, "device_id": id, "tool": "files.list", "detail": "logs"}),
            serde_json::json!({"ts_ms": 1_793_058_300_000u64, "kind": "auth", "status": 200, "device_id": id, "tool": "", "detail": "operator session opened"}),
            serde_json::json!({"ts_ms": 1_793_057_900_000u64, "kind": "tool", "status": 403, "device_id": id, "tool": "system.kill", "detail": "not permitted for this grant"}),
            serde_json::json!({"ts_ms": 1_793_057_500_000u64, "kind": "device", "status": 0, "device_id": id, "tool": "", "detail": "channel open"}),
            serde_json::json!({"ts_ms": 1_793_056_100_000u64, "kind": "tool", "status": 500, "device_id": id, "tool": "browser.open", "detail": "the device did not answer"}),
        ];
        let preset = Connection {
            id: "a".repeat(32),
            name: "Edge box".to_owned(),
            kind: "ssh".to_owned(),
            host: Some("10.0.0.5".to_owned()),
            port: Some(2222),
            user: Some("root".to_owned()),
            port_path: None,
            baud: None,
            key_path: Some("C:\\Users\\operator\\.ssh\\id_ed25519".to_owned()),
        };
        let pages: Vec<(&str, Markup)> = vec![
            (
                "dashboard",
                dashboard(
                    &devices,
                    "csrf",
                    "0.3.72",
                    Notes {
                        signer: Some("operator@example.test"),
                        ..Notes::default()
                    },
                    &[
                        serde_json::json!({ "ts_ms": 1_791_000_000_000u64, "kind": "tool", "tool": "runs.exec", "status": 200 }),
                        serde_json::json!({ "ts_ms": 1_790_999_500_000u64, "kind": "tool", "tool": "system.kill", "status": 403 }),
                        serde_json::json!({ "ts_ms": 1_790_999_000_000u64, "kind": "device", "tool": "", "status": 0 }),
                    ],
                    &[ApprovalRow {
                        id: "a1b2c3d4e5f60718".to_owned(),
                        device_id: devices[0].id.clone(),
                        tool: "terminal.create".to_owned(),
                        arguments: "{\n  \"kind\": \"pty\"\n}".to_owned(),
                        created_ms: 1_790_999_900_000,
                    }],
                    1_791_000_000_000,
                ),
            ),
            (
                "dashboard-code",
                dashboard(
                    &devices,
                    "csrf",
                    "0.3.72",
                    Notes {
                        enroll: Some(&"9f".repeat(32)),
                        origin: Some("http://127.0.0.1:19000"),
                        ..Notes::default()
                    },
                    &[],
                    &[],
                    1_791_000_000_000,
                ),
            ),
            (
                "dashboard-empty",
                dashboard(&[], "csrf", "0.3.72", Notes::default(), &[], &[], 1_791_000_000_000),
            ),
            (
                "terminal",
                terminal(
                    &devices[0],
                    "csrf",
                    "0.3.72",
                    TerminalView {
                        sessions: &[
                        TerminalSession {
                            id: "session-3".to_owned(),
                            kind: "pty".to_owned(),
                            started_at_ms: 1_791_000_000_000,
                        },
                        TerminalSession {
                            id: "session-7".to_owned(),
                            kind: "ssh".to_owned(),
                            started_at_ms: 1_791_000_500_000,
                        },
                        ],
                        presets: &[],
                        preset: None, activity: &[],
                        error: None,
                    },
                ),
            ),
            (
                "terminal-prefilled",
                terminal(
                    &devices[0],
                    "csrf",
                    "0.3.72",
                    TerminalView {
                        sessions: &[],
                        presets: std::slice::from_ref(&preset),
                        preset: Some(&preset),
                        activity: &[],
                        error: None,
                    },
                ),
            ),
            (
                "terminal-preset-error",
                terminal(
                    &devices[0],
                    "csrf",
                    "0.3.72",
                    TerminalView {
                        sessions: &[],
                        presets: std::slice::from_ref(&preset),
                        preset: None, activity: &[],
                        error: Some("That preset is not on this device."),
                    },
                ),
            ),
            (
                "files",
                files(&devices[0], "", &entries, "csrf", "0.3.72", None),
            ),
            (
                "files-empty",
                files(&devices[0], "empty", &[], "csrf", "0.3.72", None),
            ),
            (
                "files-sub",
                files(&devices[0], "logs", &log_entries, "csrf", "0.3.72", None),
            ),
            (
                "file",
                file_text(
                    &devices[0],
                    "logs/agent.log",
                    "2026-10-27T12:00:01Z  INFO  channel opened from STUDIO-PC\n2026-10-27T12:00:02Z  INFO  host.facts answered in 12 ms\n2026-10-27T12:00:04Z  WARN  update channel canary is one build behind\n",
                    "csrf",
                    "0.3.72",
                ),
            ),
            (
                "runs",
                runs(&devices[0], &runs_rows, None, "csrf", "0.3.72", None),
            ),
            (
                "runs-empty",
                runs(&devices[0], &[], None, "csrf", "0.3.72", None),
            ),
            (
                "run",
                run_result(&devices[0], "pwsh -NoProfile Get-Date", &run, "csrf", "0.3.72"),
            ),
            (
                "system",
                system(
                    &devices[0],
                    &processes,
                    "",
                    "csrf",
                    "0.3.72",
                    Some("Ended pwsh.exe (pid 1188)."),
                ),
            ),
            (
                "state",
                state_document(
                    &devices[0],
                    "{\n  \"health\": \"ok\",\n  \"ready\": true,\n  \"events\": [\n    { \"at\": \"12:00:01\", \"kind\": \"boot\" },\n    { \"at\": \"12:00:02\", \"kind\": \"channel_open\" }\n  ]\n}",
                    "csrf",
                    "0.3.72",
                ),
            ),
            (
                "memory",
                memory(
                    &devices[0],
                    &memory_rows,
                    MemoryLook { tag: "", query: "", editing: None },
                    "csrf",
                    "0.3.72",
                    None,
                ),
            ),
            ("screen", screen(&devices[0], Some(CAPTURE), "A 1280×720 capture, taken just now.", "csrf", "0.3.72")),
            (
                "screen-empty",
                screen(
                    &devices[0],
                    None,
                    "No capture: the helper is not running in the user's session.",
                    "csrf",
                    "0.3.72",
                ),
            ),
            (
                "monitors",
                monitors(&devices[0], &monitors_rows, "csrf", "0.3.72", 1_791_000_000_000, None),
            ),
            (
                "monitors-empty",
                monitors(&devices[0], &[], "csrf", "0.3.72", 1_791_000_000_000, None),
            ),
            (
                "browser",
                browser(
                    &devices[0],
                    &browser_rows,
                    Some(CAPTURE),
                    "csrf",
                    "0.3.72",
                    None,
                    None,
                ),
            ),
            (
                "tokens",
                tokens(&devices[0], &token_rows, "csrf", "0.3.72", None),
            ),
            (
                "tokens-empty",
                tokens(&devices[0], &[], "csrf", "0.3.72", None),
            ),
            (
                "token-created",
                token_created(
                    &devices[0],
                    "dsh",
                    secret,
                    &config,
                    "csrf",
                    "0.3.72",
                ),
            ),
            ("vitals", vitals(&devices[0], &sample_vitals(), "csrf", "0.3.72")),
            (
                "audit",
                logs(
                    &log_rows,
                    "Audit",
                    log_rows.len() + 3,
                    Some(audit_filter(
                        "/next/audit",
                        &[
                            ("a".repeat(64), "STUDIO-PC".to_owned()),
                            ("b".repeat(64), "LAB-PC".to_owned()),
                        ],
                        &AuditFilter {
                            kind: "tool".to_owned(),
                            ..AuditFilter::default()
                        },
                        true,
                        AUDIT_KINDS.as_slice(),
                    )),
                    "csrf",
                    "0.3.72",
                    "The console's own record, newest first. Times are UTC; a device id is shortened here but named in full by the line's tooltip.",
                    "The console's own audit lines.",
                    true,
                ),
            ),
            ("audit-filtered", logs(&log_rows, "Audit", log_rows.len(), Some(audit_filter("/next/audit", &[], &AuditFilter::default(), true, AUDIT_KINDS.as_slice())), "csrf", "0.3.72", "The console's own record, newest first. Times are UTC; a device id is shortened here but named in full by the line's tooltip.", "The console's own audit lines.", false)),
            (
                "denials",
                logs(
                    &[],
                    "Denials",
                    0,
                    Some(audit_filter("/next/denials", &[], &AuditFilter::default(), false, &[])),
                    "csrf",
                    "0.3.72",
                    "Every request the console refused before authentication: a missing session, an unknown token, an unauthenticated probe. A refusal is not a device call, and the arguments it carried are not stored.",
                    "Refusals recorded before authentication.",
                    false,
                ),
            ),
            ("login", login(None, None)),
            ("not-found", not_found("0.3.72")),
        ];
        // Two fixtures can share a page name (a page and its post-submit variant); the file name
        // must not be one of them, or the second silently overwrites the first in the review set.
        let mut seen: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for (name, markup) in pages {
            let count = seen.entry(name.to_owned()).or_insert(0);
            *count += 1;
            let file = if *count == 1 {
                name.to_owned()
            } else {
                format!("{name}-{count}")
            };
            std::fs::write(dir.join(format!("{file}.html")), markup.into_string())
                .expect("the page is written");
        }
    }

    #[test]
    fn a_short_id_is_eight_characters_and_an_ellipsis() {
        assert_eq!(short_id(&"a".repeat(64)), "aaaaaaaa…");
        assert_eq!(short_id("abc"), "abc");
    }

    #[test]
    fn the_fleet_row_is_identity_pages_and_one_disclosure() {
        let device = view('a', Some("studio"), true);
        let id = device.id.clone();
        let page = dashboard(
            &[device],
            "00",
            "0.1.0",
            Notes::default(),
            &[],
            &[],
            1_791_000_000_000,
        )
        .into_string();

        assert!(
            page.contains("aaaaaaaa…"),
            "the row shows a short id: {page}"
        );
        assert!(
            !page.contains(&format!(
                "<code class=\"device-id\" title=\"{id}\">{id}</code>"
            )),
            "the whole id is not the row's text: {page}"
        );
        assert!(page.contains("class=\"row-menu\""), "{page}");
        assert!(
            page.contains(">Manage"),
            "the mutations are folded away: {page}"
        );
        assert!(
            page.contains("name=\"name\"") && page.contains(">Rename</button>"),
            "the rename form is still on the page: {page}"
        );
        assert!(
            page.contains(&format!("action=\"/next/fleet/{id}/remove\"")),
            "the remove form is still on the page: {page}"
        );
        assert_eq!(
            page.matches("class=\"page-links\"").count(),
            1,
            "one tidy row of page links: {page}"
        );
    }

    #[test]
    fn the_terminal_has_an_empty_state_and_a_close_control() {
        let device = view('1', Some("studio"), true);
        let page = terminal(
            &device,
            "00",
            "0.1.0",
            TerminalView {
                sessions: &[],
                presets: &[],
                preset: None,
                activity: &[],
                error: None,
            },
        )
        .into_string();
        assert!(
            page.contains("No session yet — choose a kind and start one"),
            "the empty state explains the canvas: {page}"
        );
        assert!(
            page.contains("class=\"terminal-empty\""),
            "the empty state sits inside the terminal area: {page}"
        );
        assert!(
            !page.contains("Close all"),
            "the toolbar closes every session: {page}"
        );
        let form = between(&page, "<form id=\"terminal-form\"", "</form>");
        assert!(
            form.contains(">Start session</button>"),
            "the toolbar starts a session: {page}"
        );
    }

    #[test]
    fn the_vitals_page_is_stat_cards_and_two_usage_sections() {
        let device = view('1', Some("studio"), true);
        let page = vitals(&device, &sample_vitals(), "00", "0.1.0").into_string();
        assert_eq!(
            page.matches("class=\"stat\"").count(),
            6,
            "six facts, six cards — the CPU share joined them: {page}"
        );
        assert!(
            page.contains("CPU busy") && page.contains("37%"),
            "the device's CPU share is shown: {page}"
        );
        assert!(page.contains("4.0 GiB used of 8.0 GiB"), "{page}");
        assert!(page.contains("width:50%"), "the memory bar is half: {page}");
        assert_eq!(
            page.matches("class=\"usage-row\"").count(),
            sample_vitals().disks.len(),
            "one row per disk: {page}"
        );
        assert!(page.contains("Build"), "{page}");
    }

    /// Parse a theme's `--name:#rrggbb` declarations out of the stylesheet.
    fn theme_tokens(css: &str, block_start: usize) -> std::collections::HashMap<String, [u8; 3]> {
        let rest = &css[block_start..];
        // A colour declaration never contains a brace, so the first one closes the block.
        let end = rest.find('}').expect("the theme block ends");
        let block = &rest[..end];
        let mut tokens = std::collections::HashMap::new();
        // Split on the prefix rather than on `;`: the block's first chunk carries the selector and
        // its indentation, and that is not worth parsing.
        for token in block.split("--").skip(1) {
            let Some((name, value)) = token.split_once(':') else {
                continue;
            };
            let name = name.trim();
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                continue;
            }
            // The chunk runs to the declaration's `;`, which is not part of the colour.
            let value = value.split(';').next().unwrap_or(value).trim();
            let Some(hex) = value.strip_prefix('#') else {
                continue;
            };
            let (r, g, b) = match hex.len() {
                6 => (
                    u8::from_str_radix(&hex[0..2], 16),
                    u8::from_str_radix(&hex[2..4], 16),
                    u8::from_str_radix(&hex[4..6], 16),
                ),
                3 => {
                    let expand = |c: &str| u8::from_str_radix(&format!("{c}{c}"), 16);
                    (expand(&hex[0..1]), expand(&hex[1..2]), expand(&hex[2..3]))
                }
                _ => continue,
            };
            if let (Ok(r), Ok(g), Ok(b)) = (r, g, b) {
                tokens.insert(name.to_owned(), [r, g, b]);
            }
        }
        tokens
    }

    /// WCAG relative luminance and the contrast ratio between two opaque colours.
    fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
        fn luminance([r, g, b]: [u8; 3]) -> f64 {
            let channel = |value: u8| {
                let value = value as f64 / 255.0;
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
        }
        let (a, b) = (luminance(a), luminance(b));
        let (lighter, darker) = if a > b { (a, b) } else { (b, a) };
        (lighter + 0.05) / (darker + 0.05)
    }

    #[test]
    fn every_page_can_be_skipped_into_and_every_table_is_captioned() {
        // A keyboard reaches the canvas in one Tab instead of walking three rails, and a screen
        // reader announces what a table holds before it reads the columns.
        let device = view('1', Some("studio"), true);
        let fleet = [view('2', Some("other"), false)];
        let pages = [
            dashboard(
                &fleet,
                "00",
                "0.1.0",
                Notes::default(),
                &[],
                &[],
                1_791_000_000_000,
            ),
            terminal(
                &device,
                "00",
                "0.1.0",
                TerminalView {
                    sessions: &[],
                    presets: &[],
                    preset: None,
                    activity: &[],
                    error: None,
                },
            ),
            files(&device, "", &[], "00", "0.1.0", None),
            runs(&device, &[], None, "00", "0.1.0", None),
            system(&device, &[], "", "00", "0.1.0", None),
            memory(
                &device,
                &[],
                MemoryLook {
                    tag: "",
                    query: "",
                    editing: None,
                },
                "00",
                "0.1.0",
                None,
            ),
            monitors(&device, &[], "00", "0.1.0", 1_791_000_000_000, None),
            browser(&device, &[], None, "00", "0.1.0", None, None),
            tokens(&device, &[], "00", "0.1.0", None),
            vitals(&device, &sample_vitals(), "00", "0.1.0"),
        ];
        for page in pages {
            let html = page.into_string();
            assert!(
                html.contains("class=\"skip-link\""),
                "the page has a skip link"
            );
            assert!(
                html.contains("id=\"content\""),
                "the skip link has a target"
            );
            let tables = html.matches("<table").count();
            let captions = html.matches("<caption").count();
            assert_eq!(
                tables, captions,
                "every table is captioned: {tables} vs {captions}"
            );
        }
    }

    /// A device that restarted has no live sessions, and the rail must still say what the console did
    /// there: the audit log outlives the sessions.
    #[test]
    fn the_terminal_rail_shows_the_last_audited_operations() {
        let devices = [view('1', Some("studio"), true)];
        let activity = [TerminalActivity {
            at_ms: 1_791_000_000_000,
            tool: "terminal.write".to_owned(),
            status: 200,
            arrival: "console".to_owned(),
        }];
        let page = terminal(
            &devices[0],
            "csrf",
            "0.3.72",
            TerminalView {
                sessions: &[],
                presets: &[],
                preset: None,
                activity: &activity,
                error: None,
            },
        )
        .into_string();

        assert!(
            page.contains("Sessions do not survive a device restart"),
            "the rail explains why it is empty: {page}"
        );
        assert!(
            page.contains("terminal.write"),
            "the operation is named: {page}"
        );
        // The audit link is in the toolbar, which survives the 1000px breakpoint that hides the
        // context rail; the rail no longer carries navigation.
        assert_eq!(
            page.matches("href=\"/next/audit\"").count(),
            1,
            "exactly one audit link, and it is in the toolbar: {page}"
        );

        // Without activity the rail says only that there is no session.
        let bare = terminal(
            &devices[0],
            "csrf",
            "0.3.72",
            TerminalView {
                sessions: &[],
                presets: &[],
                preset: None,
                activity: &[],
                error: None,
            },
        )
        .into_string();
        assert_eq!(
            bare.matches("href=\"/next/audit\"").count(),
            1,
            "the audit link does not depend on activity: {bare}"
        );
    }

    /// **ONE RADIUS SCALE, ENFORCED.** Five literals (5, 6, 7, 8px) drew small controls that
    /// should have been one token. The sheet may name `var(--radius*)` or a pill/round shape
    /// (999px, 50%); any other pixel radius is a new scale being invented one rule at a time.
    #[test]
    fn radii_come_from_the_scale() {
        let css = style_sheet();
        let mut rest = css.as_str();
        while let Some(at) = rest.find("border-radius:") {
            // **ADVANCE PAST THE DECLARATION, NOT PAST THE TRIMMED SLICE.** The first draft
            // shadowed `value` with `value[..end].trim()` and then sliced the trimmed copy at
            // `end`; that produced an empty tail, so the loop checked exactly one declaration and
            // every later radius was never read. A mutation (`border-radius:7px` on `.btn-sm`)
            // is what proved the gate vacuous — the test passed with the literal in the sheet.
            let after = &rest[at + "border-radius:".len()..];
            let end = after.find([';', '}']).unwrap_or(after.len());
            let value = after[..end].trim();
            // A corner can be a shorthand (`0 0 var(--radius) var(--radius)`), so every part is
            // checked; `0` is a corner that is square on purpose.
            for part in value.split([' ', '/']) {
                assert!(
                    part.is_empty()
                        || part == "0"
                        || part.starts_with("var(--radius")
                        || part == "999px"
                        || part == "50%",
                    "border-radius:{value} is not on the radius scale"
                );
            }
            rest = &after[end..];
        }
    }

    /// **ONE TYPE SCALE, ENFORCED.** Fourteen sizes became eight (9, 11, 12, 13, 14, 15, 18, 22) in
    /// this round. A new `font-size:13.5px` is exactly how the fourteenth came back, so the sheet is
    /// parsed and every declared size must be one of the eight.
    #[test]
    fn the_type_scale_has_eight_sizes() {
        const ALLOWED: [&str; 8] = ["9", "11", "12", "13", "14", "15", "18", "22"];
        let css = style_sheet();
        let mut seen = std::collections::BTreeSet::new();
        let mut rest = css.as_str();
        while let Some(at) = rest.find("font-size:") {
            let value = &rest[at + "font-size:".len()..];
            let end = value
                .find("px")
                .unwrap_or_else(|| panic!("a font-size without px: {value}"));
            seen.insert(value[..end].to_owned());
            rest = &value[end..];
        }
        for size in &seen {
            assert!(
                ALLOWED.contains(&size.as_str()),
                "font-size:{size}px is not on the scale {ALLOWED:?}"
            );
        }

        // **THE SHORTHAND IS THE SAME SCALE.** The first version of this gate read only
        // `font-size:`; thirteen `font:<n>px/… var(--mono)` shorthands kept 11.5px and 12.5px
        // alive through four rounds of "the scale is eight sizes". Every sized shorthand is a
        // longhand now, and this half refuses to let one back in.
        let mut rest = css.as_str();
        while let Some(at) = rest.find("font:") {
            let after = &rest[at + "font:".len()..];
            let end = after.find([';', '}']).unwrap_or(after.len());
            let value = after[..end].trim();
            if let Some(px) = value.find("px") {
                let size: String = value[..px]
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                assert!(
                    !size.is_empty(),
                    "font:{value} has a pixel size with no number"
                );
                assert!(
                    ALLOWED.contains(&size.as_str()),
                    "font:{value} carries {size}px, off the scale {ALLOWED:?}"
                );
            }
            rest = &after[end..];
        }
    }

    /// **A TOKEN THAT REFERENCES ITSELF IS NOT A TOKEN.** `--ok-soft:var(--ok-soft)` computed to the
    /// guaranteed-invalid value, so the light "online" chip silently lost its fill, and no contrast
    /// pair could see it because the pair it would check was transparent. The class is what this pins.
    /// The second half is the focus ring: an accent-SOFT outline measured 1.11:1 on the panel, which
    /// is an outline nobody can see.
    #[test]
    fn no_token_references_itself_and_the_focus_ring_is_the_accent() {
        for block in [summrise_ui::TOKENS, summrise_ui::COMPONENTS] {
            for line in block.lines() {
                let line = line.trim();
                let Some(rest) = line.strip_prefix("--") else {
                    continue;
                };
                let Some((name, value)) = rest.split_once(':') else {
                    continue;
                };
                let name = name.trim();
                assert!(
                    !value.trim().starts_with(&format!("var(--{name})")),
                    "--{name} references itself, which computes to nothing"
                );
            }
        }
        let sheet = style_sheet();
        assert!(
            sheet.contains("outline:2px solid var(--accent)")
                && !sheet.contains("outline:2px solid var(--accent-soft)"),
            "the input focus ring must be the accent, not accent-soft (1.11:1 on the panel)"
        );
    }

    #[test]
    fn every_text_colour_reads_on_the_surface_it_sits_on() {
        // The design system's promise, measured the way a browser would: WCAG 2.1 AA for normal
        // text is 4.5:1. A token that fails this test is a token nobody can read in one of the two
        // themes, and the page's look is not a matter of taste once that is true.
        let light = theme_tokens(
            summrise_ui::TOKENS,
            summrise_ui::TOKENS
                .find(":root {")
                .expect("the light block")
                + 6,
        );
        let dark_block = summrise_ui::TOKENS
            .find("@media (prefers-color-scheme: dark)")
            .expect("the dark block");
        let dark = theme_tokens(
            summrise_ui::TOKENS,
            summrise_ui::TOKENS[dark_block..]
                .find(":root {")
                .expect("its root")
                + dark_block
                + 6,
        );

        for (theme, tokens) in [("light", &light), ("dark", &dark)] {
            for (foreground, background) in [
                ("ink", "bg"),
                ("ink", "panel"),
                ("muted", "bg"),
                ("muted", "panel"),
                ("faint", "panel"),
                // The two surfaces `--faint` was measured against and failed: the page itself
                // (footnotes) and the context rail (4.16:1 before it moved to #696a74).
                ("faint", "bg"),
                ("faint", "panel-2"),
                ("accent", "panel"),
                ("ok", "panel"),
                ("warn", "panel"),
                // The soft surface the warn chip is actually drawn on — dark `--warn-soft` did not
                // exist, so the dark chip inherited the light cream and measured 2.02:1.
                ("warn", "warn-soft"),
                ("danger", "panel"),
                // The icon rail is dark in both themes, so its ink is checked against its own
                // background rather than the page's.
                ("rail-ink", "rail"),
            ] {
                let fg = tokens
                    .get(foreground)
                    .unwrap_or_else(|| panic!("{theme}: --{foreground} is a plain colour"));
                let bg = tokens
                    .get(background)
                    .unwrap_or_else(|| panic!("{theme}: --{background} is a plain colour"));
                let ratio = contrast(*fg, *bg);
                assert!(
                    ratio >= 4.5,
                    "{theme}: --{foreground} on --{background} is {ratio:.2}:1, under AA's 4.5:1"
                );
            }
            // A primary button's label sits on the solid accent, not on the page.
            let on_solid = contrast(
                *tokens.get("accent-fg").expect("the button's label colour"),
                *tokens.get("accent-solid").expect("the button's fill"),
            );
            assert!(
                on_solid >= 4.5,
                "{theme}: the primary button's label is {on_solid:.2}:1 on its fill"
            );
        }
    }

    #[test]
    fn an_uptime_share_is_coloured_by_its_own_number() {
        // A watch that never misses is green; one that misses now and then is amber; anything under
        // 95% is a target that is not being watched so much as hoped for.
        assert_eq!(uptime_class(100), "ok");
        assert_eq!(uptime_class(99), "ok");
        assert_eq!(uptime_class(98), "warn");
        assert_eq!(uptime_class(95), "warn");
        assert_eq!(uptime_class(94), "error");
        assert_eq!(uptime_class(0), "error");
    }

    #[test]
    fn a_device_says_how_long_ago_it_was_seen() {
        let now = 1_791_000_000_000u64;
        assert_eq!(ago(now, now - 5_000), "just now");
        assert_eq!(ago(now, now - 6 * 60 * 1000), "6m ago");
        assert_eq!(ago(now, now - 3 * 3600 * 1000), "3h ago");
        assert_eq!(ago(now, now - 2 * 86_400 * 1000), "2d ago");
        // A device that has never connected is not "56 years ago", and a clock that went backwards
        // is not "in the future".
        assert_eq!(ago(now, 0), "never seen");
        assert_eq!(ago(now, now + 60_000), "just now");
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
        let page = dashboard(
            &[device],
            "00",
            "0.3.48",
            Notes::default(),
            &[],
            &[],
            1_791_000_000_000,
        )
        .into_string();
        assert!(
            page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "{page}"
        );
        assert!(!page.contains("<script>alert(1)</script>"), "{page}");
    }
}
