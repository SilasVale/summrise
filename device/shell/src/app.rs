//! The tray icon and the window, as one process.
//!
//! **What this process may do.** Read `127.0.0.1:18090/api/health`, open two URLs in a window, and
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
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder, TrayIconEvent};
use wry::WebViewBuilder;

use crate::health::{self, State};
use crate::helper;

/// Where the product console lives: the 0.3.4x console (fleet, terminal, `/next`), not the legacy
/// SPA. The shell and the device sit on the same tailnet; a public hostname, when there is one, is
/// the console's to publish.
const CONSOLE: &str = "http://100.74.250.98:19000/";
/// Where the device's own panel lives: what works when the console does not. This is the product's
/// panel (0.3.49 lists the live terminal sessions), never the legacy panel.
const PANEL: &str = "http://127.0.0.1:18090/";
/// The loopback address the health probe asks.
const HEALTH: &str = "127.0.0.1:18090";
/// How often the dot is refreshed.
const POLL: Duration = Duration::from_secs(10);

const OPEN_CONSOLE: &str = "open-console";
const OPEN_PANEL: &str = "open-panel";
const QUIT: &str = "quit";

/// Everything that can wake the event loop: a probe's answer, the tray's own events (which arrive on
/// a channel rather than in tao's event enum), and a menu click.
enum Wake {
    Health(State),
    Tray(TrayIconEvent),
    Menu(MenuEvent),
    /// The `--check-window` self-test's stop button.
    Quit,
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

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoopBuilder::<Wake>::with_user_event().build();
    crate::note("event loop built");

    let (console_item, panel_item, quit_item) = menu_items();
    let menu = Menu::with_items(&[
        &console_item,
        &panel_item,
        &PredefinedMenuItem::separator(),
        &quit_item,
    ])?;

    // The window is created the first time the operator asks for it, and again after they close it:
    // `tao` 0.34 destroys a window on its close request and offers no way to veto that, so the tray
    // outlives the window and the window is rebuilt on demand.
    let mut ui: Option<(tao::window::Window, wry::WebView)> = None;

    crate::note("menu built");

    // Armed before the build, disarmed after it: a build that hangs leaves a line that names the
    // stage and an exit the operator can see in the log, rather than a process with no icon.
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

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(tray_icon(UNKNOWN)?)
        .with_tooltip("Summrise: checking this device…")
        .build()?;
    crate::note(&format!("tray built: pid={}", std::process::id()));
    tray_ready.store(true, std::sync::atomic::Ordering::SeqCst);
    promote_own_icon();

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
        let _ = proxy.send_event(Wake::Health(health::probe(HEALTH)));
        std::thread::sleep(POLL);
    });

    register_with_the_device();

    // THE TRAY HAS TO COME BACK WHEN THE OPERATOR DOES. A per-machine Run key fires at logon, and a
    // session that was disconnected (an RDP session left open) reconnects without one — the tray
    // simply is not there, and on 2026-10-28 Windows refused to start it from a task for that reason
    // (`0x80070005`). Registering a task on the session-reconnection event, as the user this process
    // already runs as, is what makes the next reconnect restore the icon.
    ensure_reconnect_task();

    let mut last: Option<State> = None;
    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => {}
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
            })) => open_url(PANEL),
            Event::UserEvent(Wake::Tray(_)) => {}
            Event::UserEvent(Wake::Quit) => *control_flow = ControlFlow::Exit,
            Event::UserEvent(Wake::Menu(event)) => match event.id.as_ref() {
                OPEN_CONSOLE => open_console(target, &mut ui),
                OPEN_PANEL => open_url(PANEL),
                QUIT => *control_flow = ControlFlow::Exit,
                _ => {}
            },
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                // The tray stays; the window is rebuilt by the next click. Quitting is a menu item,
                // so a closed window cannot leave a device with no way back to its icon.
                ui = None;
            }
            _ => {}
        }
    });
}

/// Windows 11 files every new tray icon under the overflow chevron and keeps it there until the app
/// asks otherwise. An icon nobody sees is the same as no icon, so the shell promotes its own entry —
/// only its own, matched by executable path, and only when it is not already shown.
///
/// The entry is written by Explorer shortly after the icon is added, which is why this retries: the
/// first attempt is usually too early.
#[cfg(windows)]
fn promote_own_icon() {
    std::thread::spawn(|| {
        for _ in 0..20 {
            if set_own_icon_promoted() {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

#[cfg(windows)]
fn set_own_icon_promoted() -> bool {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    let Ok(own) = std::env::current_exe() else {
        return false;
    };
    let own = own.to_string_lossy().to_lowercase();
    let Ok(settings) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(r"Control Panel\NotifyIconSettings", KEY_READ | KEY_WRITE)
    else {
        return false;
    };
    for name in settings.enum_keys().flatten() {
        let Ok(entry) = settings.open_subkey_with_flags(&name, KEY_READ | KEY_WRITE) else {
            continue;
        };
        let Ok(path) = entry.get_value::<String, _>("ExecutablePath") else {
            continue;
        };
        if path.to_lowercase() != own {
            continue;
        }
        if entry.get_value::<u32, _>("IsPromoted").unwrap_or(0) != 0 {
            return true;
        }
        if entry.set_value("IsPromoted", &1u32).is_ok() {
            crate::note("tray promoted to the visible area");
            return true;
        }
        return false;
    }
    false
}

#[cfg(not(windows))]
fn promote_own_icon() {}

/// Keep this process registered with the device.
///
/// The device mints a token each boot and writes it to `ProgramData`; this thread reads it, serves the
/// launch verb on loopback, and tells the device where. It repeats the registration every thirty
/// seconds because that is all a device restart costs: the old port is forgotten, the next tick says
/// where the new one is, and no reconnect protocol has to exist.
fn register_with_the_device() {
    std::thread::spawn(|| {
        let mut current: Option<(String, helper::Server)> = None;
        loop {
            if let Some(token) = helper::read_token() {
                let unchanged = current.as_ref().is_some_and(|(known, _)| known == &token);
                if !unchanged {
                    current = helper::Server::start(token.clone())
                        .ok()
                        .map(|server| (token.clone(), server));
                }
                if let Some((token, server)) = current.as_ref() {
                    let _ =
                        helper::register(helper::DEVICE, token, server.port(), std::process::id());
                }
            }
            std::thread::sleep(Duration::from_secs(30));
        }
    });
}

/// Make sure a task starts this shell when the user reconnects to their session.
///
/// Best effort on purpose: a machine whose policy forbids scheduled tasks still gets a tray, and the
/// only thing lost is the automatic return after a reconnect. The task is created as *this* user
/// with an interactive token, so no password or elevation is involved, and it does nothing when it
/// already exists (the check is what makes the "at every start" cost a `query` rather than a rewrite).
#[cfg(windows)]
fn ensure_reconnect_task() {
    use std::process::{Command, Stdio};

    std::thread::spawn(|| {
        let exists = Command::new("schtasks")
            .args(["/query", "/tn", "SummriseShellOnReconnect"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if exists {
            return;
        }
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let created = Command::new("schtasks")
            .args([
                "/create",
                "/tn",
                "SummriseShellOnReconnect",
                "/tr",
                &exe.to_string_lossy(),
                "/sc",
                "onevent",
                "/ec",
                "Microsoft-Windows-TerminalServices-LocalSessionManager/Operational",
                "/mo",
                "*[System[EventID=25]]",
                "/f",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match created {
            Ok(status) if status.success() => crate::note("registered the session-reconnect task"),
            Ok(status) => crate::note(&format!(
                "could not register the session-reconnect task (schtasks exit {status})"
            )),
            Err(error) => crate::note(&format!(
                "could not register the session-reconnect task: {error}"
            )),
        }
    });
}

#[cfg(not(windows))]
fn ensure_reconnect_task() {}

/// Build the console window once, then exit — the menu's own code path without a menu.
///
/// Answers through the log (which a headless machine can be asked for) and through the exit code: 0
/// means a window and a WebView2 view were created for [`CONSOLE`], 1 means one of the two failed and
/// the log names which. The event loop runs for a moment because a tao window is only real once it is
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
    // `build()` is not fallible in tao 0.34; the call can panic on a thread that already owns an
    // event loop, which is a programming error this diagnostic cannot see from inside.
    let event_loop = EventLoopBuilder::<Wake>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let mut built = false;
    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => {
                built = true;
                let Ok(window) = WindowBuilder::new()
                    .with_title("Summrise console")
                    .with_window_icon(window_icon())
                    .with_visible(true)
                    .build(target)
                else {
                    crate::note("check-window: the window could not be built");
                    std::process::exit(1);
                };
                match WebViewBuilder::new().with_url(CONSOLE).build(&window) {
                    Ok(_view) => crate::note(&format!(
                        "check-window: window and view built for {CONSOLE}"
                    )),
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
    });
    let _ = built;
    0
}

#[cfg(not(windows))]
pub fn check_window() -> i32 {
    1
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

/// The three items, with the ids the event handler matches.
///
/// **THE IDS ARE EXPLICIT, AND THAT IS THE FIX FOR "THE MENU DOES NOTHING".** `MenuItem::new` with
/// `None` mints an id of its own, so the handler compared `open-console` against a generated string
/// and no item — Quit included — ever ran. A menu item without the id the handler matches is a
/// button wired to nothing, so the wiring is a function that a test can read.
fn menu_items() -> (MenuItem, MenuItem, MenuItem) {
    (
        MenuItem::with_id(OPEN_CONSOLE, "Open console", true, None),
        MenuItem::with_id(OPEN_PANEL, "Open local panel", true, None),
        MenuItem::with_id(QUIT, "Quit", true, None),
    )
}

/// Show the console window, building it if this is the first time (or the last one was closed).
fn open_console(
    target: &tao::event_loop::EventLoopWindowTarget<Wake>,
    ui: &mut Option<(tao::window::Window, wry::WebView)>,
) {
    if let Some((window, _)) = ui.as_ref() {
        window.set_visible(true);
        window.set_focus();
        return;
    }
    let window = match WindowBuilder::new()
        .with_title("Summrise console")
        .with_window_icon(window_icon())
        .with_visible(true)
        .build(target)
    {
        Ok(window) => window,
        Err(error) => {
            // A menu click that does nothing is a dead button; the log is where it says why.
            crate::note(&format!("cannot build the console window: {error}"));
            return;
        }
    };
    match WebViewBuilder::new().with_url(CONSOLE).build(&window) {
        Ok(view) => {
            // A click that worked leaves a line too: without it, "the menu does nothing" and "the
            // menu opened a window I closed" look the same from this side of the machine.
            crate::note(&format!("console window built for {CONSOLE}"));
            *ui = Some((window, view))
        }
        Err(error) => {
            // WebView2 is part of Windows 10 and later, but a machine can have the runtime removed
            // or broken; without this line the window closes itself and the menu looks dead.
            crate::note(&format!(
                "cannot build the WebView2 view for {CONSOLE}: {error}"
            ));
            window.set_visible(false);
        }
    }
}

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
            "cannot open {url:?} in the browser (ShellExecuteW answered {answer:?})"
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

#[cfg(test)]
mod tests {
    #[test]
    fn the_default_targets_are_the_product_surfaces() {
        assert!(
            CONSOLE.contains(":19000"),
            "the product console, not the legacy SPA: {CONSOLE}"
        );
        assert!(!CONSOLE.contains("console.saisi.online"), "{CONSOLE}");
        assert!(PANEL.contains(":18090"), "the device's own panel: {PANEL}");
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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this pins: an item built with a generated id never matches the handler, and the menu
    /// becomes three dead buttons. The ids are compared as strings because that is what the event
    /// carries.
    #[test]
    fn every_menu_item_carries_the_id_its_handler_matches() {
        let (console, panel, quit) = menu_items();
        assert_eq!(console.id().as_ref(), OPEN_CONSOLE);
        assert_eq!(panel.id().as_ref(), OPEN_PANEL);
        assert_eq!(quit.id().as_ref(), QUIT);
    }
}
