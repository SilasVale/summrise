//! The tray icon and the window, as one process.
//!
//! **What this process may do.** Read the device's loopback health document, open two URLs in a window, and
//! exit. It holds no credential and speaks to no device: every action on a device goes through the
//! console, a grant and the audit trail, exactly as it does in a browser tab. The tray is a status
//! dot and a shortcut, not a second client.
//!
//! **Why the window is a WebView2 and not a bundled browser.** The page is the console's own page.
//! Bundling Chromium would ship a second rendering engine, a second update path and a second UI to
//! keep in step, to display what `https://console.saisi.online/` already serves. WebView2 is part of
//! Windows 10 and later; this binary is a few hundred kilobytes.

use std::time::Duration;

use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::WindowBuilder;
use tray_icon::menu::accelerator::Accelerator;
use tray_icon::menu::{IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder, TrayIconEvent};
use wry::WebViewBuilder;

use crate::browser;
use crate::console;
use crate::health::{self, State};
use crate::helper;
use crate::menu;

/// Where the device's own panel lives: what works when the console does not. This is the product's
/// panel (0.3.49 lists the live terminal sessions), never the legacy panel.
///
/// Resolved at boot from the device's own port file: the port is a deployment choice, and a tray
/// that dials a constant reports "not running" for a device that is serving.
fn panel() -> &'static str {
    crate::port::panel_url()
}
/// Which page a window opens: the console if [`console::configured`] names one that answers,
/// otherwise [`panel`]. Resolved per window, because a console that was down at boot can be up at
/// the next click and the fallback is not a permanent downgrade.
fn window_target() -> String {
    console::window_url()
}
/// The loopback address the health probe asks.
fn health_addr() -> &'static str {
    crate::port::health_addr()
}
/// How often the dot is refreshed.
const POLL: Duration = Duration::from_secs(10);

const OPEN_CONSOLE: &str = "open-console";
const OPEN_PANEL: &str = "open-panel";
const UPDATE_STATUS: &str = "update-status";
const COPY_PANEL: &str = "copy-panel";
const COPY_DIAGNOSTICS: &str = "copy-diagnostics";
const ABOUT: &str = "about";
const OPEN_LOGS: &str = "open-logs";
const QUIT: &str = "quit";

/// The release version when the build names one, the crate version otherwise. `publish-canary.bash`
/// exports `SUMMRISE_BUILD_VERSION`, so the About box and the tray title agree with the installer.
const VERSION: &str = match option_env!("SUMMRISE_BUILD_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

/// The size the console opens at, and the size below which it is not a usable console.
const WINDOW_DEFAULT: (u32, u32) = (1280, 840);
const WINDOW_MIN: (u32, u32) = (960, 640);

/// How much of its natural size the console is drawn at. `wry`'s zoom is WebView2's own zoom factor,
/// so 1.0 is the page at its designed size; the menu's row range is 50 % to 200 % in ten-percent
/// steps, exactly what the legacy host's zoom roles did.
const ZOOM_MIN: f64 = 0.5;
const ZOOM_MAX: f64 = 2.0;
const ZOOM_STEP: f64 = 0.1;

/// The one-shot fragments the Session rows hand the console.
///
/// The console reads them as requests — the Connect dialog's kind, the Browser address — and its
/// address writer canonicalises them away after reading, so the same row clicked again is a new
/// fragment rather than a hash the page already has. The shell names no kind the console cannot
/// map (`local` is the dialog's `pty`), and the browser arm is the console's own prefilled view
/// until the 9444 door replaces it.
const NEW_TERMINAL_LINK: &str = "#device/terminal/new=local";
const NEW_SSH_LINK: &str = "#device/terminal/new=ssh";
const NEW_SERIAL_LINK: &str = "#device/terminal/new=serial";
const NEW_BROWSER_LINK: &str = "#device/browser/open=about%3Ablank";

/// The one window this process owns, with the page it was built for.
///
/// `page` is what a deep link reads: a view on the console takes the fragment as a hash through the
/// console's own reader (no reload, no second sign-in), while a view on the panel is given the whole
/// address, because the panel does not read a trailing fragment the console's way.
struct Ui {
    window: tao::window::Window,
    view: wry::WebView,
    page: String,
}

/// Everything that can wake the event loop: a probe's answer, the tray's own events (which arrive on
/// a channel rather than in tao's event enum), and a menu click.
enum Wake {
    Health(State),
    Tray(TrayIconEvent),
    Menu(MenuEvent),
    /// The `--check-window` self-test's stop button.
    Quit,
    /// Another instance asked this one to put its icon back (see `signal_wake`).
    Repaint,
}

/// The brand mark, rendered from `brand/icon.png` at the two sizes the shell needs. The tray is 16 px
/// because that is what Windows draws; the window is 32 px because that is what a title bar and a
/// task switcher ask for.
const TRAY_RGBA: &[u8] = include_bytes!("../assets/tray-16.rgba");
const WINDOW_RGBA: &[u8] = include_bytes!("../assets/window-32.rgba");

/// One colour for each thing the dot can say.
const OK: [u8; 4] = [0x2f, 0xbf, 0x6f, 0xff];
const BAD: [u8; 4] = [0xd7, 0x3a, 0x2a, 0xff];
const UNKNOWN: [u8; 4] = [0x9a, 0x9a, 0x9a, 0xff];

/// How long the tray may take to appear before the process gives up and says so.
///
/// A tray the user cannot see is a process the user cannot quit. On 2026-10-28 a shell started by a
/// scheduled task logged `starting` and showed nothing for at least forty seconds; the same process
/// then logged `tray built` some minutes later, so a first build in a fresh session is SLOW rather
/// than reliably hung. Three minutes is the line between slow and never: long enough not to kill a
/// working tray, short enough that a process which will never draw one exits and is retried at the
/// next logon instead of lingering invisibly.
const TRAY_WATCHDOG: Duration = Duration::from_secs(180);

/// What the command line asks this process to be.
///
/// The full tray is the default and the only mode that owns an icon. `--check-window` is the
/// diagnostic that answers through the exit code; `--panel` is the supported window-only fallback,
/// with no tray, helper, first-run, geometry or reconnect task; `--browser-session` asks the
/// existing 9444 door for one browser session, with no tray and no mutex, and answers on the exit
/// code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Tray,
    CheckWindow,
    Fallback,
    /// `--browser-session`: one `POST` to the Electron host's loopback door.
    BrowserSession,
}

/// Which [`Mode`] `args` asks for. Unknown arguments (the program path included) are ignored.
///
/// `--check-window` wins when it is present at all: it is the diagnostic that must always answer on
/// the exit code, so it is decided before any other flag. `--browser-session` is a valueless
/// one-shot: it asks the 9444 door for one session and exits.
pub fn mode_from_args<I, S>(args: I) -> Mode
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let arguments: Vec<String> = args
        .into_iter()
        .map(|argument| argument.as_ref().to_owned())
        .collect();
    if arguments
        .iter()
        .any(|argument| argument == "--check-window")
    {
        return Mode::CheckWindow;
    }
    let mut mode = Mode::Tray;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--panel" => {
                if matches!(mode, Mode::Tray) {
                    mode = Mode::Fallback;
                }
                index += 1;
            }
            "--browser-session" => {
                mode = Mode::BrowserSession;
                index += 1;
            }
            _ => index += 1,
        }
    }
    mode
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let first_run = first_run_pending();
    let event_loop = EventLoopBuilder::<Wake>::with_user_event().build();
    crate::note("event loop built");

    // The window is created the first time the operator asks for it, and again after they close it:
    // `tao` 0.34 destroys a window on its close request and offers no way to veto that, so the tray
    // outlives the window and the window is rebuilt on demand.
    // The WebView2 profile context outlives the view, so it is owned here rather than in the builder:
    // a context dropped after `build` takes the cookies with it.
    let mut ui: Option<Ui> = None;
    let mut profile: Option<wry::WebContext> = None;
    // The zoom the next window opens at. It outlives the window on purpose: a closed and reopened
    // console keeps the operator's size instead of asking them to choose it again.
    let mut zoom = 1.0;

    crate::note("menu built");

    // Armed before the build, disarmed after it: a build that hangs leaves a line that names the
    // stage and an exit the operator can see in the log, rather than a process with no icon.
    // A SECOND INSTANCE IS A REQUEST, NOT A MISTAKE. The first instance holds the mutex, and a
    // second launch used to exit with the icon left as it was; it now signals this event instead,
    // and the listener below re-adds the tray.
    if let Some(event) = wake_event() {
        let proxy = event_loop.create_proxy();
        std::thread::spawn(move || loop {
            // SAFETY: `event` is a valid handle for this process's lifetime and INFINITE is a constant.
            let waited = unsafe {
                windows_sys::Win32::System::Threading::WaitForSingleObject(event, 0xFFFF_FFFF)
            };
            if waited != 0 {
                return;
            }
            let _ = proxy.send_event(Wake::Repaint);
        });
    }

    let tray_ready = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let tray_ready = tray_ready.clone();
        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + TRAY_WATCHDOG;
            while std::time::Instant::now() < deadline {
                if tray_ready.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if !tray_ready.load(std::sync::atomic::Ordering::SeqCst) {
                crate::note(&format!(
                    "the tray was not built within {} s (stopped before 'tray built'); exiting",
                    TRAY_WATCHDOG.as_secs()
                ));
                std::process::exit(1);
            }
        });
    }

    let mut tray = build_tray("Summrise: checking this device…")?;
    crate::note(&format!("tray built: pid={}", std::process::id()));
    tray_ready.store(true, std::sync::atomic::Ordering::SeqCst);
    // THE SHELL DOES NOT ASK FOR A PLACE IN THE TASKBAR. Windows' default for a new tray icon is the
    // overflow, and that is where this one belongs: the operator said so plainly on 2026-10-28 ("it
    // should be inside the chevron"), after a promotion to the visible area made it sit among the
    // system icons instead. Pinning it is the operator's decision, made once, and Windows remembers.
    // The old promotion code is gone with the behaviour: writing another program's notification-icon
    // settings is a private protocol, and the "visible area" was never ours to choose.

    // The tray and its menu report through channels, not through tao's event enum. Connecting each
    // handler to the proxy is what makes `ControlFlow::Wait` wake up for a click.
    let tray_proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |event| {
        let _ = tray_proxy.send_event(Wake::Tray(event));
    }));
    let menu_proxy: EventLoopProxy<Wake> = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = menu_proxy.send_event(Wake::Menu(event));
    }));

    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || loop {
        let _ = proxy.send_event(Wake::Health(health::probe(health_addr())));
        std::thread::sleep(POLL);
    });

    register_with_the_device();

    let mut last: Option<State> = None;
    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => {
                if first_run {
                    // The tray lives in the overflow by Windows' choice, so an install can look like
                    // nothing happened. Open the local panel once per user: it is the device's own
                    // page, it needs no credential, and it is the shortest proof the product is
                    // running. The marker is written whatever the browser did, so a machine with no
                    // default browser does not try again at every logon.
                    open_url(panel());
                    mark_first_run();
                    crate::note("first run: opened the local panel");
                }
            }
            Event::UserEvent(Wake::Health(state)) => {
                if last.as_ref() == Some(&state) {
                    return;
                }
                let (colour, tooltip) = describe(&state);
                let _ = tray.set_icon(Some(tray_icon(colour).expect("a 16×16 RGBA icon")));
                let _ = tray.set_tooltip(Some(tooltip.as_str()));
                last = Some(state);
            }
            Event::UserEvent(Wake::Tray(TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                button_state: tray_icon::MouseButtonState::Up,
                ..
            })) => open_url(panel()),
            Event::UserEvent(Wake::Tray(_)) => {}
            Event::UserEvent(Wake::Repaint) => {
                // A second launch asked for the icon to be added again: Windows re-creates the
                // notification area on reconnect, and an icon added before that is not there
                // afterwards. Rebuilding the tray is the only re-add the tray crate offers.
                let tooltip = last
                    .as_ref()
                    .map(|state| describe(state).1)
                    .unwrap_or_else(|| "Summrise: checking this device…".to_owned());
                match build_tray(&tooltip) {
                    Ok(fresh) => {
                        tray = fresh;
                        crate::note("tray re-added after a wake signal");
                    }
                    Err(error) => crate::note(&format!("cannot re-add the tray: {error}")),
                }
            }
            Event::UserEvent(Wake::Quit) => *control_flow = ControlFlow::Exit,
            Event::UserEvent(Wake::Menu(event)) => {
                // **THE ID IS THE ONLY KEY.** A menu row's id is its action: the model is looked up by
                // the same string the item was built with, so the tray cannot drift from the table
                // and there is no second string-to-action map. A click that names no model row falls
                // through to the product rows, whose ids and arms are unchanged.
                let id = event.id.as_ref();
                match menu::row(id) {
                    Some(row) => perform(row.action, target, &mut ui, &mut profile, &mut zoom),
                    None => match id {
                        OPEN_CONSOLE => open_console(target, &mut ui, &mut profile, zoom, None),
                        OPEN_PANEL => open_url(panel()),
                        UPDATE_STATUS => open_url(&format!("{}#updates", panel())),
                        COPY_PANEL => copy_panel_address(),
                        COPY_DIAGNOSTICS => copy_diagnostics(),
                        ABOUT => show_about(),
                        OPEN_LOGS => open_log_folder(),
                        QUIT => {
                            if let Some(ui) = ui.as_ref() {
                                save_window_geometry(&ui.window);
                            }
                            *control_flow = ControlFlow::Exit
                        }
                        _ => {}
                    },
                }
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                // The tray stays; the window is rebuilt by the next click. Quitting is a menu item,
                // so a closed window cannot leave a device with no way back to its icon.
                drop_window(&mut ui);
            }
            _ => {}
        }
    });
}

/// Keep this process registered with the device.
///
/// The device mints a token each boot and writes it to `ProgramData`; this thread reads it, serves the
/// launch verb on loopback, and tells the device where. It repeats the registration every thirty
/// seconds because that is all a device restart costs: the old port is forgotten, the next tick says
/// where the new one is, and no reconnect protocol has to exist.
fn register_with_the_device() {
    std::thread::spawn(|| {
        let mut current: Option<(String, helper::Server)> = None;
        // The last failure, so a permanent 401 is said ONCE instead of every thirty seconds or not at
        // all: the registration is what lets the device launch Chrome for the operator, and a silent
        // failure there is a feature that quietly stops working.
        let mut last_error: Option<String> = None;
        loop {
            if let Some(token) = helper::read_token() {
                let unchanged = current.as_ref().is_some_and(|(known, _)| known == &token);
                if !unchanged {
                    current = helper::Server::start(token.clone())
                        .ok()
                        .map(|server| (token.clone(), server));
                }
                if let Some((token, server)) = current.as_ref() {
                    let outcome = helper::register(
                        helper::device(),
                        token,
                        server.port(),
                        std::process::id(),
                    );
                    let failure = outcome.err().map(|error| error.to_string());
                    if failure != last_error {
                        match &failure {
                            Some(reason) => crate::note(&format!(
                                "the device did not accept the helper registration: {reason}"
                            )),
                            None => crate::note("the device accepted the helper registration"),
                        }
                        last_error = failure;
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(30));
        }
    });
}

/// Build the console window once, then exit — the menu's own code path without a menu.
///
/// Answers through the log (which a headless machine can be asked for) and through the exit code: 0
/// means a window and a WebView2 view were created for the same target the menu would open — the
/// console when it answers, the local panel otherwise — 1 means one of the two failed and the log
/// names which. The event loop runs for a moment because a tao window is only real once it is
/// pumping messages.
#[cfg(windows)]
pub fn check_window() -> i32 {
    // SESSION 0 HAS NO WINDOW STATION A GUI CAN USE. Run from a service context, the event loop never
    // arrives at its first `NewEvents(Init)` and the process hangs; the answer is to refuse by name,
    // not to wait. A caller that wants the real answer runs this in the operator's session.
    match current_session_id() {
        Some(0) => {
            crate::note("check-window: this is session 0, which has no interactive desktop");
            return 2;
        }
        None => {
            crate::note("check-window: cannot read the session id");
            return 2;
        }
        Some(_) => {}
    }
    // The same decision the menu makes, recorded before the event loop so a headless caller can
    // read which page this run proves. A console probe can wait up to its connect timeout; that is
    // the price of not opening an error page.
    let page = window_target();
    crate::note(&format!("check-window: opening {page}"));
    // `build()` is not fallible in tao 0.34; the call can panic on a thread that already owns an
    // event loop, which is a programming error this diagnostic cannot see from inside.
    let event_loop = EventLoopBuilder::<Wake>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    // `run` never returns: every exit this diagnostic has is a `process::exit` in an arm, so there is
    // no success value to carry out of it.
    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => {
                let Ok(window) = WindowBuilder::new()
                    .with_title("Summrise")
                    .with_window_icon(window_icon())
                    .with_visible(true)
                    .build(target)
                else {
                    crate::note("check-window: the window could not be built");
                    std::process::exit(1);
                };
                // A PERSISTENT PROFILE, so the operator signs in once. The default is a per-process temporary
                // directory: every "Open console" was a new, logged-out browser, and the console's token page was
                // what the window showed every time.
                let mut context = profile_dir().map(|dir| wry::WebContext::new(Some(dir)));
                let built = match context.as_mut() {
                    Some(context) => WebViewBuilder::new_with_web_context(context)
                        .with_url(page.as_str())
                        .build(&window),
                    None => WebViewBuilder::new().with_url(page.as_str()).build(&window),
                };
                match built {
                    Ok(_view) => {
                        crate::note(&format!("check-window: window and view built for {page}"))
                    }
                    Err(error) => {
                        crate::note(&format!("check-window: no WebView2 view: {error}"));
                        std::process::exit(1);
                    }
                }
                let proxy = proxy.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(3));
                    let _ = proxy.send_event(Wake::Quit);
                    // A DIAGNOSTIC MUST NOT BE ABLE TO HANG. On a session-0 desktop this process
                    // blocked in the event loop after the window was created and never came back;
                    // the hard stop is what turns "no answer" into an exit code.
                    std::thread::sleep(Duration::from_secs(2));
                    std::process::exit(0);
                });
            }
            Event::UserEvent(Wake::Quit) => *control_flow = ControlFlow::Exit,
            _ => {}
        }
    })
}

#[cfg(not(windows))]
pub fn check_window() -> i32 {
    1
}

/// Open the device's own panel in one window, and exit when the operator closes it.
///
/// This is the supported fallback: one visible window on the Rust `/panel`, with no tray, no helper,
/// no first-run marker, no saved geometry and no reconnect task. It answers on the exit code: 2 for
/// a session that cannot show a window, 1 when the window or its WebView2 view cannot be built, and
/// 0 when the window closes (tao exits the process on [`ControlFlow::Exit`]).
#[cfg(windows)]
pub fn show_panel() -> i32 {
    // SESSION 0 HAS NO WINDOW STATION A GUI CAN USE, exactly as for `--check-window`: the event
    // loop would never arrive at its first `NewEvents(Init)`, so waiting there is a process with no
    // window and no answer. The caller gets a named refusal instead.
    match current_session_id() {
        Some(0) => {
            crate::note("fallback: this is session 0, which has no interactive desktop");
            return 2;
        }
        None => {
            crate::note("fallback: cannot read the session id");
            return 2;
        }
        Some(_) => {}
    }
    // THE SUPPORTED FALLBACK IS THE DEVICE'S OWN PANEL, never the console: it is the Rust page the
    // installed device already serves, and it needs no console, credential or helper.
    let page = panel();
    crate::note(&format!("fallback: opening {page}"));
    // `build()` is not fallible in tao 0.34; the call can panic on a thread that already owns an
    // event loop, which is a programming error this window cannot see from inside.
    let event_loop = EventLoopBuilder::<Wake>::with_user_event().build();
    // Held for the life of the loop, exactly as `run` holds its window: dropping either the view or
    // the window closes it, and the WebContext must outlive the view or the profile goes with it.
    let mut ui: Option<(tao::window::Window, wry::WebView)> = None;
    let mut profile: Option<wry::WebContext> = None;
    // `run` never returns: every exit here is a `process::exit` in an arm or tao's own exit for
    // `ControlFlow::Exit`, so there is no success value to carry out of it.
    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => {
                let window = match WindowBuilder::new()
                    .with_title("Summrise local panel")
                    .with_window_icon(window_icon())
                    .with_visible(true)
                    .build(target)
                {
                    Ok(window) => window,
                    Err(error) => {
                        crate::note(&format!("fallback: the window could not be built: {error}"));
                        show_error_box(
                            "Summrise local panel",
                            "The local panel window could not be opened.\n\nShell log:\n%LOCALAPPDATA%\\Summrise\\shell.log",
                        );
                        std::process::exit(1);
                    }
                };
                // A PERSISTENT PROFILE, so the device's own pages keep their state between opens;
                // this is the profile the console window uses too.
                let built = match profile_dir() {
                    Some(dir) => {
                        let context = profile.get_or_insert_with(|| wry::WebContext::new(Some(dir)));
                        WebViewBuilder::new_with_web_context(context)
                            .with_url(page)
                            .build(&window)
                    }
                    None => WebViewBuilder::new().with_url(page).build(&window),
                };
                match built {
                    Ok(view) => {
                        ui = Some((window, view));
                        crate::note(&format!("fallback: window and view built for {page}"));
                    }
                    Err(error) => {
                        crate::note(&format!("fallback: no WebView2 view: {error}"));
                        show_error_box(
                            "Summrise local panel",
                            "The local panel could not be displayed because its WebView2 view could not be created.\n\nWebView2 may be missing or damaged.\nShell log:\n%LOCALAPPDATA%\\Summrise\\shell.log",
                        );
                        std::process::exit(1);
                    }
                }
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                // THE WINDOW IS THE WHOLE PROCESS. Closing it is quitting, so the view is dropped
                // before its window and the loop exits with code 0.
                if let Some((window, view)) = ui.take() {
                    drop(view);
                    drop(window);
                }
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    })
}

/// A native, visible failure. A GUI-subsystem process has no console, so a fallback that cannot
/// build its view must not exit silently and leave a dead click behind.
#[cfg(windows)]
fn show_error_box(title: &str, text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let wide = |value: &str| -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(Some(0))
            .collect()
    };
    let text = wide(text);
    let title = wide(title);
    // SAFETY: both strings are NUL-terminated and outlive the call; no owner window is needed.
    unsafe {
        MessageBoxW(0, text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

/// Whether this user has seen the first-run panel.
#[cfg(windows)]
fn first_run_pending() -> bool {
    let Some(base) = std::env::var_os("LOCALAPPDATA") else {
        return false;
    };
    !std::path::PathBuf::from(base)
        .join("Summrise")
        .join("first-run")
        .exists()
}

#[cfg(not(windows))]
fn first_run_pending() -> bool {
    false
}

/// Record that the first-run panel was attempted, so it is a one-time event rather than a browser
/// tab at every logon.
#[cfg(windows)]
fn mark_first_run() {
    let Some(base) = std::env::var_os("LOCALAPPDATA") else {
        return;
    };
    let directory = std::path::PathBuf::from(base).join("Summrise");
    let _ = std::fs::create_dir_all(&directory);
    if let Err(error) = std::fs::write(directory.join("first-run"), b"opened\n") {
        crate::note(&format!("cannot record the first-run marker: {error}"));
    }
}

#[cfg(not(windows))]
fn mark_first_run() {}

/// Where the console window keeps its profile — cookies, storage, the signed-in session.
#[cfg(windows)]
fn profile_dir() -> Option<std::path::PathBuf> {
    // `%LOCALAPPDATA%\Summrise` is the per-user directory this process owns; when the environment does
    // not name one, a signed-in browser profile in `%TEMP%` is a cookie jar other local users can read,
    // so the answer is "no persistent profile" rather than "somewhere else".
    let base = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    let dir = base.join("Summrise").join("webview");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// The session this process runs in, for the one diagnostic that can only run in a visible one.
#[cfg(windows)]
fn current_session_id() -> Option<u32> {
    let mut session = 0u32;
    let ok = unsafe {
        windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId(
            std::process::id(),
            &mut session,
        )
    };
    (ok != 0).then_some(session)
}

/// `--browser-session`: one request to the 9444 door, answering on the exit code.
///
/// The same call the Browser row makes, without a tray or a mutex. There is no fallback here: the
/// diagnostic exists to prove whether the door answered, and opening a console window instead would
/// hide the very failure it was invoked to report.
pub fn browser_session() -> i32 {
    match browser::open() {
        Ok(()) => {
            crate::note(&format!(
                "browser-session: the door at {} opened one session",
                browser::CONTROL
            ));
            0
        }
        Err(error) => {
            crate::note(&format!("browser-session: {error}"));
            2
        }
    }
}

/// The eight product rows, with the ids the event handler matches.
///
/// **THE IDS ARE EXPLICIT, AND THAT IS THE FIX FOR "THE MENU DOES NOTHING".** `MenuItem::new` with
/// `None` mints an id of its own, so the handler compared `open-console` against a generated string
/// and no item — Quit included — ever ran. A menu item without the id the handler matches is a
/// button wired to nothing, so the wiring is a function that a test can read.
fn menu_items() -> (
    MenuItem,
    MenuItem,
    MenuItem,
    MenuItem,
    MenuItem,
    MenuItem,
    MenuItem,
    MenuItem,
) {
    (
        MenuItem::with_id(OPEN_CONSOLE, "Open Console window", true, None),
        MenuItem::with_id(OPEN_PANEL, "Open local panel in browser", true, None),
        MenuItem::with_id(UPDATE_STATUS, "View updates in local panel", true, None),
        MenuItem::with_id(COPY_PANEL, "Copy local panel address", true, None),
        MenuItem::with_id(COPY_DIAGNOSTICS, "Copy shell diagnostics", true, None),
        MenuItem::with_id(ABOUT, format!("About Summrise {VERSION}"), true, None),
        MenuItem::with_id(OPEN_LOGS, "Open shell log folder", true, None),
        MenuItem::with_id(QUIT, "Quit tray and window", true, None),
    )
}

/// The model's rows as tray items, in display order.
///
/// One item per row, built from the same [`menu::Row`] the click is looked up by. A chord that does
/// not parse leaves the row enabled and unaccelerated and says so in the log: the tray must not
/// panic in release over one string, and a row that lost its chord is still a working row.
fn model_menu_items() -> Vec<MenuItem> {
    menu::sections()
        .iter()
        .flat_map(|section| section.rows.iter())
        .map(menu_item)
        .collect()
}

/// One model row as a tray item; the accelerator is parsed once, here.
fn menu_item(row: &menu::Row) -> MenuItem {
    let accelerator = row
        .accelerator
        .and_then(|text| match text.parse::<Accelerator>() {
            Ok(accelerator) => Some(accelerator),
            Err(error) => {
                crate::note(&format!(
                    "the accelerator {text:?} for the tray row {} does not parse: {error}",
                    row.id
                ));
                None
            }
        });
    MenuItem::with_id(row.id, row.label, true, accelerator)
}

/// Bring a window that already exists back in front.
///
/// **A MINIMIZED WINDOW IGNORES `set_visible(true)` AND `set_focus()`.** tao's `apply_diff` returns
/// early when no flag changed, and its `set_focus` skips activating a minimized window, so a click on
/// "Open console" with the window minimized looked exactly like a dead menu item.
fn bring_forward(window: &tao::window::Window) {
    window.set_minimized(false);
    window.set_visible(true);
    window.set_focus();
}

/// Show the console window, building it if this is the first time (or the last one was closed).
///
/// `fragment` is a one-shot deep link the NEW window must open with. An existing window is only
/// brought forward here: replacing its page would drop the console's open sessions, so a deep link
/// against a live window goes through [`open_fragment`] and the page's own hash reader.
fn open_console(
    target: &tao::event_loop::EventLoopWindowTarget<Wake>,
    ui: &mut Option<Ui>,
    profile: &mut Option<wry::WebContext>,
    zoom: f64,
    fragment: Option<&str>,
) {
    if let Some(ui) = ui.as_ref() {
        bring_forward(&ui.window);
        return;
    }
    // **WHICH PAGE, DECIDED BEFORE THE WINDOW EXISTS.** The console is probed once per open; an
    // unreachable (or absent) console opens the device's own panel instead of a WebView2 error page.
    // Resolving here rather than in `run` means a console that came back is used on the next click.
    let page = window_target();
    if page == panel() {
        crate::note("console unreachable or not configured; opening the local panel");
    } else {
        crate::note(&format!("opening the console at {page}"));
    }
    let url = match fragment {
        Some(fragment) => format!("{page}{fragment}"),
        None => page.clone(),
    };
    let (width, height, position) = saved_window_geometry();
    let mut builder = WindowBuilder::new()
        .with_title("Summrise")
        .with_window_icon(window_icon())
        .with_inner_size(tao::dpi::PhysicalSize::new(width, height))
        .with_min_inner_size(tao::dpi::PhysicalSize::new(WINDOW_MIN.0, WINDOW_MIN.1))
        .with_visible(true);
    if let Some(position) = position {
        builder = builder.with_position(position);
    }
    let window = match builder.build(target) {
        Ok(window) => window,
        Err(error) => {
            // A menu click that does nothing is a dead button; the log is where it says why.
            crate::note(&format!("cannot build the console window: {error}"));
            return;
        }
    };
    // THE PROFILE IS THE POINT: without it every "Open console" is a new, logged-out browser and the
    // console's token page is what the operator sees each time.
    let built = match profile_dir() {
        Some(dir) => {
            let context = profile.get_or_insert_with(|| wry::WebContext::new(Some(dir)));
            WebViewBuilder::new_with_web_context(context)
                .with_url(url.as_str())
                .build(&window)
        }
        None => WebViewBuilder::new().with_url(url.as_str()).build(&window),
    };
    match built {
        Ok(view) => {
            // **THE ZOOM IS RE-APPLIED BY THE REBUILD.** A closed window takes its zoom with it, and
            // a window action can be the click that rebuilds it; a fresh view that ignored the stored
            // factor would silently return the operator to 100 %.
            if let Err(error) = view.zoom(zoom) {
                crate::note(&format!("cannot apply the stored zoom {zoom:.1}: {error}"));
            }
            // A click that worked leaves a line too: without it, "the menu does nothing" and "the
            // menu opened a window I closed" look the same from this side of the machine.
            crate::note(&format!("window built for {url}"));
            *ui = Some(Ui { window, view, page });
        }
        Err(error) => {
            // WebView2 is part of Windows 10 and later, but a machine can have the runtime removed
            // or broken; without this line the window closes itself and the menu looks dead.
            crate::note(&format!(
                "cannot build the WebView2 view for {url}: {error}"
            ));
            window.set_visible(false);
        }
    }
}

/// Do what a menu row says.
///
/// The match over [`menu::Action`] is exhaustive: a new action in the model is a compile error until
/// this function says what it does, and the event handler looks the row up by id instead of keeping a
/// second string-to-action table.
fn perform(
    action: menu::Action,
    target: &tao::event_loop::EventLoopWindowTarget<Wake>,
    ui: &mut Option<Ui>,
    profile: &mut Option<wry::WebContext>,
    zoom: &mut f64,
) {
    match action {
        menu::Action::NewTerminal => open_fragment(NEW_TERMINAL_LINK, target, ui, profile, *zoom),
        menu::Action::NewSsh => open_fragment(NEW_SSH_LINK, target, ui, profile, *zoom),
        menu::Action::NewSerial => open_fragment(NEW_SERIAL_LINK, target, ui, profile, *zoom),
        // **THE 9444 DOOR FIRST, THE CONSOLE VIEW SECOND.** The Electron host's route opens a real
        // OS browser window through CDP; the console's Browser view is the documented fallback when
        // that host is not running. The failure is logged as its own sentence before the fallback,
        // so "the console opened instead" never hides why the door did not.
        menu::Action::NewBrowser => match browser::open() {
            Ok(()) => crate::note(&format!(
                "browser-session: the door at {} opened one session",
                browser::CONTROL
            )),
            Err(error) => {
                crate::note(&format!(
                    "browser-session: {error}; falling back to the console browser view"
                ));
                open_fragment(NEW_BROWSER_LINK, target, ui, profile, *zoom);
            }
        },
        // The window closes and the tray stays: quitting is its own row, so closing the console
        // never leaves a machine with a running process and no visible way back.
        menu::Action::CloseWindow => close_window(ui),
        menu::Action::Reload => {
            // A window that does not exist is built by the same path as Open console, and its first
            // load is already a fresh page; only a live view has something to reload.
            if ui.is_none() {
                open_console(target, ui, profile, *zoom, None);
            } else if let Some(ui) = ui.as_ref() {
                match ui.view.reload() {
                    Ok(()) => crate::note("reloaded the window"),
                    Err(error) => crate::note(&format!("cannot reload the window: {error}")),
                }
            }
        }
        menu::Action::ZoomIn => {
            *zoom = stepped_zoom(*zoom, ZOOM_STEP);
            zoom_window(target, ui, profile, *zoom);
        }
        menu::Action::ZoomOut => {
            *zoom = stepped_zoom(*zoom, -ZOOM_STEP);
            zoom_window(target, ui, profile, *zoom);
        }
        menu::Action::ZoomReset => {
            *zoom = 1.0;
            zoom_window(target, ui, profile, *zoom);
        }
        menu::Action::ToggleDevTools => {
            open_console(target, ui, profile, *zoom, None);
            if let Some(ui) = ui.as_ref() {
                // WebView2 has no close verb and its `is_devtools_open` cannot see the window, so
                // the only honest action on this platform is open-and-focus: a second click brings
                // the inspector forward rather than claiming a close that cannot happen.
                ui.view.open_devtools();
                crate::note("opened the developer tools");
            }
        }
    }
}

/// Apply the zoom to the window, building it first with that zoom when there is none.
fn zoom_window(
    target: &tao::event_loop::EventLoopWindowTarget<Wake>,
    ui: &mut Option<Ui>,
    profile: &mut Option<wry::WebContext>,
    zoom: f64,
) {
    if ui.is_none() {
        // The build path applies the stored zoom, so the new window opens at the operator's size.
        open_console(target, ui, profile, zoom, None);
        return;
    }
    if let Some(ui) = ui.as_ref() {
        match ui.view.zoom(zoom) {
            Ok(()) => crate::note(&format!("zoom {zoom:.1}")),
            Err(error) => crate::note(&format!("cannot set the zoom to {zoom:.1}: {error}")),
        }
    }
}

/// The next zoom factor: whole ten-percent steps, clamped to the range the rows offer.
fn stepped_zoom(current: f64, delta: f64) -> f64 {
    (((current + delta) * 10.0).round() / 10.0).clamp(ZOOM_MIN, ZOOM_MAX)
}

/// Close and drop the window; the tray keeps running, so the next click rebuilds it.
fn close_window(ui: &mut Option<Ui>) {
    if ui.is_none() {
        crate::note("close-window: no window is open");
        return;
    }
    drop_window(ui);
    crate::note("closed the window; the tray stays");
}

/// Take the window down, dropping the view before its window (the view borrows the window's handle).
fn drop_window(ui: &mut Option<Ui>) {
    let Some(ui) = ui.take() else {
        return;
    };
    // Remember the geometry before the window goes; the next open restores it.
    save_window_geometry(&ui.window);
    let Ui { window, view, .. } = ui;
    drop(view);
    drop(window);
}

/// Open a console deep link: build the window on it, or hand it to the page already running.
///
/// A view on the console takes the fragment as its hash, so the console's own reader consumes the
/// request without a reload (the open sessions and the sign-in survive). A view on the panel gets
/// the whole address, because the panel has no reader for the console's fragment.
fn open_fragment(
    fragment: &str,
    target: &tao::event_loop::EventLoopWindowTarget<Wake>,
    ui: &mut Option<Ui>,
    profile: &mut Option<wry::WebContext>,
    zoom: f64,
) {
    let page = window_target();
    if page == panel() {
        // The deep link is the console's: the panel has no Connect dialog and no Browser view to
        // receive it, and a page that ignored the request would look like a dead row. Say why and
        // show what this machine does have.
        crate::note("console unreachable; deep link needs the console");
        open_url(&page);
        return;
    }
    let url = format!("{page}{fragment}");
    crate::note(&format!("deep link {url}"));
    let Some(mut current) = ui.take() else {
        // No window: build it ON the fragment, so the page's first render already knows the request
        // instead of racing a hash written after the load.
        open_console(target, ui, profile, zoom, Some(fragment));
        return;
    };
    if current.page == panel() {
        match current.view.load_url(&url) {
            Ok(()) => {
                current.page = page;
                crate::note(&format!("window loaded the deep link at {url}"));
            }
            Err(error) => crate::note(&format!("cannot load the deep link at {url}: {error}")),
        }
    } else {
        // A JSON string, not string interpolation: the fragment is data, and a quote in it must not
        // become script.
        let script = format!(
            "window.location.hash = {};",
            serde_json::to_string(fragment).unwrap_or_else(|_| "\"\"".to_owned())
        );
        match current.view.evaluate_script(&script) {
            Ok(()) => crate::note(&format!("window took the deep link {fragment}")),
            Err(error) => crate::note(&format!("cannot send the deep link {fragment}: {error}")),
        }
    }
    // **A DEEP LINK IS A CLICK, AND A CLICK MUST BE SEEN.** The request can arrive while the console
    // is minimized or behind another window; handing it to the page would open the dialog out of
    // sight. Bringing the window forward is the same step Open console takes, and it is why the
    // existing-window path is still a visible response to the row.
    bring_forward(&current.window);
    *ui = Some(current);
}

/// The console this process is configured to use, in words, without probing. A machine with no
/// console is a supported deployment (the panel is the whole surface), so it says that rather than
/// showing an empty field.
fn console_label() -> String {
    console::configured().unwrap_or_else(|| "Not configured; opens the local panel".to_owned())
}

/// A small, native About box: the product, the build, and the two addresses this shell opens.
#[cfg(windows)]
fn show_about() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

    let text = format!(
        "Summrise {VERSION}\n\nConsole: {}\nLocal panel on this device: {}\nShell log: %LOCALAPPDATA%\\Summrise\\shell.log\n\nQuitting closes this tray and window. The device service runs separately; this process holds no credential.",
        console_label(),
        panel()
    );
    let wide = |value: &str| -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(Some(0))
            .collect()
    };
    let text = wide(&text);
    let title = wide("About Summrise");
    // SAFETY: both strings are NUL-terminated and outlive the call; no owner window is needed.
    unsafe {
        MessageBoxW(0, text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONINFORMATION);
    }
}

#[cfg(not(windows))]
fn show_about() {}

/// Where the window's last usable size is kept, beside the WebView2 profile.
#[cfg(windows)]
fn window_state_path() -> Option<std::path::PathBuf> {
    let base = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    Some(base.join("Summrise").join("window.json"))
}

/// The last size and position, clamped to the window's usable range. A corrupt or absent file
/// means the default: a remembered number must never make the window disappear. A position is only
/// restored when it still overlaps the virtual desktop, so unplugging a monitor cannot strand the
/// window off-screen.
#[cfg(windows)]
fn saved_window_geometry() -> (u32, u32, Option<tao::dpi::PhysicalPosition<i32>>) {
    let Some(path) = window_state_path() else {
        return (WINDOW_DEFAULT.0, WINDOW_DEFAULT.1, None);
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return (WINDOW_DEFAULT.0, WINDOW_DEFAULT.1, None);
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return (WINDOW_DEFAULT.0, WINDOW_DEFAULT.1, None);
    };
    let width = value
        .get("width")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(WINDOW_DEFAULT.0 as u64) as u32;
    let height = value
        .get("height")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(WINDOW_DEFAULT.1 as u64) as u32;
    let (width, height) = clamp_window_size(width, height);
    let position = value
        .get("x")
        .and_then(serde_json::Value::as_i64)
        .zip(value.get("y").and_then(serde_json::Value::as_i64))
        .and_then(|(x, y)| visible_position(x as i32, y as i32, width, height));
    (width, height, position)
}

/// Keep a remembered size inside the range a console can actually use.
fn clamp_window_size(width: u32, height: u32) -> (u32, u32) {
    (
        width.clamp(WINDOW_MIN.0, 3840),
        height.clamp(WINDOW_MIN.1, 2160),
    )
}

/// Whether a window at `x,y` still has a useful part on the virtual desktop. A window entirely off
/// every monitor is a window the operator cannot reach.
fn within_virtual(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    virtual_x: i32,
    virtual_y: i32,
    virtual_width: i32,
    virtual_height: i32,
) -> bool {
    let right = x.saturating_add(width as i32);
    let bottom = y.saturating_add(height as i32);
    right > virtual_x + 50
        && x < virtual_x + virtual_width - 50
        && bottom > virtual_y + 50
        && y < virtual_y + virtual_height - 50
}

#[cfg(windows)]
fn visible_position(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Option<tao::dpi::PhysicalPosition<i32>> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };
    // SAFETY: GetSystemMetrics reads a process-wide value and has no failure mode for these indexes.
    let (virtual_x, virtual_y, virtual_width, virtual_height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    within_virtual(
        x,
        y,
        width,
        height,
        virtual_x,
        virtual_y,
        virtual_width,
        virtual_height,
    )
    .then_some(tao::dpi::PhysicalPosition::new(x, y))
}

#[cfg(not(windows))]
fn visible_position(
    _x: i32,
    _y: i32,
    _width: u32,
    _height: u32,
) -> Option<tao::dpi::PhysicalPosition<i32>> {
    None
}

/// Remember the window's size and position for the next open. Best effort: a read-only profile is
/// not a reason to fail a close.
#[cfg(windows)]
fn save_window_geometry(window: &tao::window::Window) {
    let Some(path) = window_state_path() else {
        return;
    };
    let size = window.inner_size();
    if size.width < WINDOW_MIN.0 || size.height < WINDOW_MIN.1 {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut value = serde_json::json!({
        "width": size.width,
        "height": size.height,
    });
    if let Ok(position) = window.outer_position() {
        value["x"] = serde_json::json!(position.x);
        value["y"] = serde_json::json!(position.y);
    }
    if let Err(error) = std::fs::write(&path, value.to_string()) {
        crate::note(&format!("cannot remember the window geometry: {error}"));
    }
}

/// Put the local panel's address on the clipboard. It is the one address an operator can paste into
/// a browser on this machine without knowing the port or the install layout.
#[cfg(windows)]
fn copy_panel_address() {
    copy_text(panel(), "local panel address");
}

#[cfg(not(windows))]
fn copy_panel_address() {}

/// Copy the facts support asks for first: the shell build, the health answer, the two URLs and the
/// log path. No token, no grant, no device identity.
#[cfg(windows)]
fn copy_diagnostics() {
    let state = health::probe(health_addr());
    let log = std::env::var_os("LOCALAPPDATA")
        .map(|base| {
            std::path::PathBuf::from(base)
                .join("Summrise")
                .join("shell.log")
                .display()
                .to_string()
        })
        .unwrap_or_else(|| "(no LOCALAPPDATA)".to_owned());
    let text = format!(
        "Summrise shell {VERSION}\nHealth: {state:?}\nConsole: {}\nLocal panel: {}\nShell log: {log}\n",
        console_label(),
        panel()
    );
    copy_text(&text, "diagnostics");
}

#[cfg(not(windows))]
fn copy_diagnostics() {}

#[cfg(windows)]
fn copy_text(text: &str, what: &str) {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
    };
    use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

    let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    // SAFETY: the strings live through the calls; the clipboard owns the allocation only after
    // SetClipboardData succeeds, and every failure path frees what was allocated.
    unsafe {
        if OpenClipboard(0) == 0 {
            crate::note(&format!("cannot open the clipboard to copy the {what}"));
            return;
        }
        let _ = EmptyClipboard();
        let bytes = text.len() * std::mem::size_of::<u16>();
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if handle.is_null() {
            let _ = CloseClipboard();
            crate::note("cannot allocate the clipboard buffer");
            return;
        }
        let target = GlobalLock(handle);
        if target.is_null() {
            let _ = GlobalFree(handle);
            let _ = CloseClipboard();
            crate::note("cannot lock the clipboard buffer");
            return;
        }
        std::ptr::copy_nonoverlapping(text.as_ptr(), target as *mut u16, text.len());
        let _ = GlobalUnlock(handle);
        if SetClipboardData(CF_UNICODETEXT as u32, handle as isize) == 0 {
            let _ = CloseClipboard();
            crate::note(&format!("cannot put the {what} on the clipboard"));
            return;
        }
        let _ = CloseClipboard();
        crate::note(&format!("copied the {what}"));
    }
}

/// Open the per-user directory that holds `shell.log`; the first thing support asks for.
#[cfg(windows)]
fn open_log_folder() {
    let Some(base) = std::env::var_os("LOCALAPPDATA") else {
        return;
    };
    let directory = std::path::PathBuf::from(base).join("Summrise");
    let _ = std::fs::create_dir_all(&directory);
    open_url(&directory.to_string_lossy());
}

#[cfg(not(windows))]
fn open_log_folder() {}

/// The dot's colour and the sentence under the cursor.
fn describe(state: &State) -> ([u8; 4], String) {
    match state {
        State::Up(health) => {
            let version = health.version.as_deref().unwrap_or("unknown version");
            let channel = health.channel.as_deref().unwrap_or("no channel");
            if health.status == "ok" {
                (
                    OK,
                    format!("Summrise {version} · {channel} · this device is serving"),
                )
            } else {
                (
                    BAD,
                    format!("Summrise {version} · the device reports {}", health.status),
                )
            }
        }
        State::Down(reason) => (BAD, format!("Summrise: {reason}")),
    }
}

/// The brand mark, with a status badge when the device is not serving.
///
/// The icon is the product's face, not a coloured dot: a dot says "some program is running" and the
/// mark says which program. Green is the mark alone; anything else gets a five-pixel badge in the
/// corner, ringed so it reads against the orange.
fn tray_icon(colour: [u8; 4]) -> Result<Icon, tray_icon::BadIcon> {
    const SIZE: usize = 16;
    let mut rgba = TRAY_RGBA.to_vec();
    if colour != OK {
        for y in 9..15 {
            for x in 9..15 {
                let edge = x == 9 || y == 9 || x == 14 || y == 14;
                let at = (y * SIZE + x) * 4;
                let pixel = if edge {
                    [0x1f, 0x1f, 0x1f, 0xff]
                } else {
                    colour
                };
                rgba[at..at + 4].copy_from_slice(&pixel);
            }
        }
    }
    Icon::from_rgba(rgba, SIZE as u32, SIZE as u32)
}

/// The same mark for the window's title bar and the task switcher.
fn window_icon() -> Option<tao::window::Icon> {
    tao::window::Icon::from_rgba(WINDOW_RGBA.to_vec(), 32, 32).ok()
}

/// Ask the shell to open `url` with whatever the operator's default browser is.
fn open_url(url: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let wide = |text: &str| -> Vec<u16> {
        OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    // Kept as text for the failure line: the UTF-16 buffer below is what goes to the shell, and
    // printing that buffer wrote code units into the log instead of the address that failed.
    let shown = url.to_owned();
    let url = wide(url);
    let verb = wide("open");
    // SAFETY: both strings are NUL-terminated and outlive the call.
    let answer = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            0,
            verb.as_ptr(),
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    };
    // The return is an HINSTANCE whose value above 32 means "started". A failure here used to be
    // invisible — the menu item simply did nothing, which is exactly the report it produced — so the
    // log now carries the code, and the tray boot is unaffected either way.
    if answer as usize <= 32 {
        crate::note(&format!(
            "cannot open {shown} in the browser (ShellExecuteW answered {})",
            answer as usize
        ));
    }
}

/// Whether another instance is already running; the first one keeps the tray.
pub fn already_running() -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let name: Vec<u16> = OsStr::new("Local\\SummriseShell")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: the name is NUL-terminated; the handle is intentionally kept for the process's whole
    // life, which is what makes the mutex hold.
    // The handle is deliberately never closed: the kernel keeps the object while the process holds
    // it, which is what keeps the name taken for this process's whole life. Dropping the value does
    // not release it (only `CloseHandle` would), and the process exiting does.
    let _handle = unsafe {
        windows_sys::Win32::System::Threading::CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr())
    };
    // SAFETY: no arguments.
    let last = unsafe { windows_sys::Win32::Foundation::GetLastError() };
    last == windows_sys::Win32::Foundation::ERROR_ALREADY_EXISTS
}

/// The tray, built from scratch: the menu, the icon and the tooltip.
///
/// It is a function rather than three lines in `run` because a re-add after a reconnect needs a
/// SECOND tray object, and a tray without its menu is a tray whose items do nothing.
fn build_tray(tooltip: &str) -> Result<tray_icon::TrayIcon, Box<dyn std::error::Error>> {
    // **THE MODEL COMES FIRST, THE PRODUCT ROWS AFTER.** The Session and Window sections are the
    // tray's copy of the desktop chrome; a separator closes each section, so the last one divides
    // them from the product rows. Every item is held in `items` until `Menu::with_items` has taken
    // its clone, exactly as the previous eight-item build did.
    let model_rows = model_menu_items();
    // The labels are the model's names for its groups; the tray renders the sections flat, so the
    // startup line is where a reader sees which rows came from where.
    let labels: Vec<&str> = menu::sections()
        .iter()
        .map(|section| section.label)
        .collect();
    crate::note(&format!("tray sections: {}", labels.join(", ")));
    let mut items: Vec<&dyn IsMenuItem> = Vec::with_capacity(model_rows.len() + 12);
    let mut next = 0;
    let section_separators: Vec<PredefinedMenuItem> = menu::sections()
        .iter()
        .map(|_| PredefinedMenuItem::separator())
        .collect();
    for (index, section) in menu::sections().iter().enumerate() {
        for _ in section.rows {
            items.push(&model_rows[next]);
            next += 1;
        }
        items.push(&section_separators[index]);
    }
    let (
        console_item,
        panel_item,
        update_item,
        copy_item,
        diagnostics_item,
        about_item,
        logs_item,
        quit_item,
    ) = menu_items();
    // The two dividers the product rows already had, bound here because the menu borrows every
    // item until it has taken its own copy.
    let product_separator = PredefinedMenuItem::separator();
    let quit_separator = PredefinedMenuItem::separator();
    items.push(&console_item);
    items.push(&panel_item);
    items.push(&update_item);
    items.push(&copy_item);
    items.push(&diagnostics_item);
    items.push(&product_separator);
    items.push(&about_item);
    items.push(&logs_item);
    items.push(&quit_separator);
    items.push(&quit_item);
    let menu = Menu::with_items(&items)?;
    // The menu has taken its own copies of the items, so the borrows can go.
    drop(items);
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(tray_icon(UNKNOWN)?)
        .with_tooltip(tooltip)
        .build()?;
    Ok(tray)
}

/// The name of the event a second instance signals.
fn wake_name() -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    OsStr::new("Local\\SummriseShell.Wake")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Create (or open) this session's wake event and return its handle for the listener thread.
fn wake_event() -> Option<windows_sys::Win32::Foundation::HANDLE> {
    let name = wake_name();
    // SAFETY: a NUL-terminated name and a null security descriptor; the handle is a real handle or 0.
    let handle = unsafe {
        windows_sys::Win32::System::Threading::CreateEventW(
            std::ptr::null_mut(),
            0,
            0,
            name.as_ptr(),
        )
    };
    (handle != 0).then_some(handle)
}

/// Tell the running instance to put its icon back; returns false when there is none to tell.
pub fn signal_wake() -> bool {
    let name = wake_name();
    // SAFETY: `EVENT_MODIFY_STATE` is the minimal access for `SetEvent`; the handle is closed here.
    let handle = unsafe {
        windows_sys::Win32::System::Threading::OpenEventW(
            windows_sys::Win32::System::Threading::EVENT_MODIFY_STATE,
            0,
            name.as_ptr(),
        )
    };
    if handle == 0 {
        return false;
    }
    let signalled = unsafe { windows_sys::Win32::System::Threading::SetEvent(handle) } != 0;
    unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
    signalled
}

#[cfg(test)]
mod tests {

    /// **THERE IS ONE SUPPORTED FALLBACK, AND IT IS A WINDOW.** `--panel` selects the window-only
    /// mode and never the tray: a second tray would be a second automatic surface.
    #[test]
    fn the_fallback_mode_is_a_window_only_panel() {
        assert_eq!(
            mode_from_args(["x", "--panel"]),
            Mode::Fallback,
            "the fallback flag is the window-only mode"
        );
        assert_eq!(
            mode_from_args(["x", "--check-window"]),
            Mode::CheckWindow,
            "the diagnostic keeps its own mode"
        );
        assert_eq!(
            mode_from_args(["x"]),
            Mode::Tray,
            "no flag is the full tray, as before"
        );
        assert_eq!(
            mode_from_args(["--show"]),
            Mode::Tray,
            "an unknown flag is not a mode"
        );
    }

    /// **`--browser-session` IS A VALUELESS ONE-SHOT.** It asks the 9444 door for one session and
    /// must not fall through to a tray or take the next argument as a value; `--check-window` keeps
    /// its exit-code precedence when the diagnostic is combined with it.
    #[test]
    fn the_browser_session_mode_is_one_shot() {
        assert_eq!(
            mode_from_args(["x", "--browser-session"]),
            Mode::BrowserSession,
            "the browser-session flag is its own mode"
        );
        assert_eq!(
            mode_from_args(["--browser-session", "--panel"]),
            Mode::BrowserSession,
            "the door is not downgraded to the window-only fallback"
        );
        assert_eq!(
            mode_from_args(["--check-window", "--browser-session"]),
            Mode::CheckWindow,
            "check-window still wins the exit-code answer"
        );
        assert_eq!(
            mode_from_args(["x"]),
            Mode::Tray,
            "a plain launch is still the tray"
        );
    }

    #[test]
    fn the_default_targets_are_the_product_surfaces() {
        let console = console::DEFAULT_CONSOLE;
        assert!(
            console.contains(":7736"),
            "the product console, not the legacy SPA: {console}"
        );
        assert!(!console.contains("console.saisi.online"), "{console}");
        assert!(
            panel().ends_with(&format!(":{}/panel", crate::port::device_port())),
            "the device's own panel follows its port and names /panel: {}",
            panel()
        );
    }

    use super::*;
    use crate::health::Health;

    #[test]
    fn a_serving_device_is_a_green_dot_naming_the_version_and_channel() {
        let (colour, tooltip) = describe(&State::Up(Health {
            status: "ok".to_owned(),
            version: Some("0.3.29".to_owned()),
            channel: Some("canary".to_owned()),
        }));
        assert_eq!(colour, OK);
        assert!(tooltip.contains("0.3.29"), "{tooltip}");
        assert!(tooltip.contains("canary"), "{tooltip}");
    }

    #[test]
    fn a_device_that_says_it_is_not_ok_is_red_and_quotes_it() {
        let (colour, tooltip) = describe(&State::Up(Health {
            status: "degraded".to_owned(),
            version: None,
            channel: None,
        }));
        assert_eq!(colour, BAD);
        assert!(tooltip.contains("degraded"), "{tooltip}");
        assert!(tooltip.contains("unknown version"), "{tooltip}");
    }

    #[test]
    fn an_unreachable_device_is_red_and_carries_the_reason() {
        let (colour, tooltip) = describe(&State::Down("connection refused".to_owned()));
        assert_eq!(colour, BAD);
        assert!(tooltip.contains("connection refused"), "{tooltip}");
    }

    #[test]
    fn a_remembered_window_size_cannot_hide_the_console() {
        assert_eq!(clamp_window_size(0, 0), WINDOW_MIN);
        assert_eq!(clamp_window_size(10_000, 10_000), (3840, 2160));
        assert_eq!(clamp_window_size(1280, 840), WINDOW_DEFAULT);
    }

    #[test]
    fn an_offscreen_window_position_is_not_restored() {
        assert!(within_virtual(100, 100, 1280, 840, 0, 0, 2560, 1440));
        assert!(within_virtual(-100, 100, 1280, 840, 0, 0, 2560, 1440));
        assert!(!within_virtual(-2000, 100, 1280, 840, 0, 0, 2560, 1440));
        assert!(!within_virtual(100, 3000, 1280, 840, 0, 0, 2560, 1440));
    }

    #[test]
    fn every_menu_item_carries_the_id_its_handler_matches() {
        let (console, panel, update, copy, diagnostics, about, logs, quit) = menu_items();
        assert_eq!(console.id().as_ref(), OPEN_CONSOLE);
        assert_eq!(panel.id().as_ref(), OPEN_PANEL);
        assert_eq!(update.id().as_ref(), UPDATE_STATUS);
        assert_eq!(copy.id().as_ref(), COPY_PANEL);
        assert_eq!(diagnostics.id().as_ref(), COPY_DIAGNOSTICS);
        assert_eq!(about.id().as_ref(), ABOUT);
        assert_eq!(logs.id().as_ref(), OPEN_LOGS);
        assert_eq!(quit.id().as_ref(), QUIT);
    }

    /// **THE TRAY IS BUILT FROM THE MODEL, NOT FROM A SECOND LIST.** Every model row must become one
    /// item with that row's id and label; a row the build path missed would be a chord with no
    /// button. The model itself is portable and tested where no tray exists; this test is with the
    /// Windows tests because a `MenuItem` needs the Win32 menu machinery (a headless Linux job
    /// cannot build one, and this is the documented limitation rather than a fake render).
    #[test]
    fn the_tray_menu_carries_every_model_row() {
        let items = model_menu_items();
        let rows: Vec<&menu::Row> = menu::sections()
            .iter()
            .flat_map(|section| section.rows.iter())
            .collect();
        assert_eq!(items.len(), rows.len(), "one item per model row");
        for (item, row) in items.iter().zip(rows.iter()) {
            assert_eq!(item.id().as_ref(), row.id, "{} has its own id", row.id);
            assert_eq!(item.text(), row.label, "{} has its own label", row.id);
        }
        // The six chords parse in muda; a failed parse would have logged and left the row without
        // one, which is the defect the tray test exists to catch.
        for row in rows {
            if let Some(text) = row.accelerator {
                assert!(
                    text.parse::<Accelerator>().is_ok(),
                    "{} has an unparseable chord",
                    row.id
                );
            }
        }
    }

    /// **EVERY MODEL ACTION COMPILES INTO A DISPATCH.** `perform` matches `menu::Action` with no
    /// wildcard arm, so adding a variant is a compile error until it has a behaviour. This test
    /// compiles the same exhaustive shape over every action the tray can raise, so a row cannot name
    /// an action the shell does not know how to run.
    #[test]
    fn every_action_compiles() {
        let actions: Vec<menu::Action> = menu::sections()
            .iter()
            .flat_map(|section| section.rows.iter())
            .map(|row| row.action)
            .collect();
        assert_eq!(actions.len(), 10, "ten model rows, one action each");
        for action in actions {
            let label = match action {
                menu::Action::NewTerminal => "new terminal",
                menu::Action::NewSsh => "new ssh",
                menu::Action::NewSerial => "new serial",
                menu::Action::NewBrowser => "new browser",
                menu::Action::CloseWindow => "close window",
                menu::Action::Reload => "reload",
                menu::Action::ZoomIn => "zoom in",
                menu::Action::ZoomOut => "zoom out",
                menu::Action::ZoomReset => "reset zoom",
                menu::Action::ToggleDevTools => "toggle devtools",
            };
            assert!(!label.is_empty(), "every action names a behaviour");
        }
    }

    /// A tray row is not a reason to panic in release: a chord muda cannot read is dropped and the
    /// row remains, enabled and unaccelerated.
    #[test]
    fn a_row_whose_chord_does_not_parse_still_builds() {
        let row = menu::Row {
            id: "test-row",
            label: "Test row",
            accelerator: Some("not+a+chord"),
            action: menu::Action::Reload,
        };
        let item = menu_item(&row);
        assert_eq!(item.id().as_ref(), "test-row");
        assert_eq!(item.text(), "Test row");
        // The chord may be lost; the button may not be. A row disabled by its own accelerator is
        // the tray losing a feature to a typo in a string.
        assert!(item.is_enabled());
    }

    /// **THE TWO ID SPACES ARE ONE NAMESPACE AT THE EVENT HANDLER.** The model is looked up first,
    /// so a product id that also named a model row would silently perform the model's action and
    /// the product's arm would be dead. The eight product ids and the ten model ids must be
    /// disjoint.
    #[test]
    fn the_product_ids_and_the_model_ids_do_not_collide() {
        let (console, panel, update, copy, diagnostics, about, logs, quit) = menu_items();
        let product = [
            console.id().as_ref(),
            panel.id().as_ref(),
            update.id().as_ref(),
            copy.id().as_ref(),
            diagnostics.id().as_ref(),
            about.id().as_ref(),
            logs.id().as_ref(),
            quit.id().as_ref(),
        ];
        for id in product {
            assert!(
                menu::row(id).is_none(),
                "{id} is both a product id and a model id"
            );
        }
    }

    /// 50 %..200 % in whole ten-percent steps: the ends clamp instead of wrapping or drifting.
    #[test]
    fn the_zoom_rows_move_in_ten_percent_steps_and_stop_at_the_ends() {
        assert_eq!(stepped_zoom(1.0, ZOOM_STEP), 1.1);
        assert_eq!(stepped_zoom(1.1, -ZOOM_STEP), 1.0);
        assert_eq!(stepped_zoom(0.5, ZOOM_STEP), 0.6);
        assert_eq!(stepped_zoom(ZOOM_MAX, ZOOM_STEP), ZOOM_MAX);
        assert_eq!(stepped_zoom(ZOOM_MIN, -ZOOM_STEP), ZOOM_MIN);
    }
}
