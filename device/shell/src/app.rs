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

/// Where the operator console lives.
const CONSOLE: &str = "https://console.saisi.online/";
/// Where the device's own panel lives: what works when the console does not.
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
}

/// One colour for each thing the dot can say.
const OK: [u8; 4] = [0x2f, 0xbf, 0x6f, 0xff];
const BAD: [u8; 4] = [0xd7, 0x3a, 0x2a, 0xff];
const UNKNOWN: [u8; 4] = [0x9a, 0x9a, 0x9a, 0xff];

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoopBuilder::<Wake>::with_user_event().build();

    let console_item = MenuItem::new("Open console", true, None);
    let panel_item = MenuItem::new("Open local panel", true, None);
    let quit_item = MenuItem::new("Quit", true, None);
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

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(dot(UNKNOWN)?)
        .with_tooltip("Summrise: checking this device…")
        .build()?;

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
                let _ = tray.set_icon(Some(dot(colour).expect("a 16×16 RGBA icon")));
                let _ = tray.set_tooltip(Some(tooltip.as_str()));
                last = Some(state);
            }
            Event::UserEvent(Wake::Tray(TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                button_state: tray_icon::MouseButtonState::Up,
                ..
            })) => open_console(target, &mut ui),
            Event::UserEvent(Wake::Tray(_)) => {}
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
    let Ok(window) = WindowBuilder::new()
        .with_title("Summrise console")
        .with_visible(true)
        .build(target)
    else {
        return;
    };
    match WebViewBuilder::new().with_url(CONSOLE).build(&window) {
        Ok(view) => *ui = Some((window, view)),
        Err(_) => window.set_visible(false),
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

/// A solid disc in one colour, drawn rather than shipped: the tray needs 16×16 RGBA and the state is
/// one byte of colour, so an asset per state would be three files to keep in step with this constant.
fn dot(colour: [u8; 4]) -> Result<Icon, tray_icon::BadIcon> {
    const SIZE: u32 = 16;
    const RADIUS: f32 = 6.0;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - (SIZE as f32 - 1.0) / 2.0;
            let dy = y as f32 - (SIZE as f32 - 1.0) / 2.0;
            if dx * dx + dy * dy <= RADIUS * RADIUS {
                let at = ((y * SIZE + x) * 4) as usize;
                rgba[at..at + 4].copy_from_slice(&colour);
            }
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE)
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
    // SAFETY: both strings are NUL-terminated and outlive the call; the return value is only a
    // success hint and is deliberately ignored (a failure to open a browser must not stop the tray).
    unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            0,
            wide("open").as_ptr(),
            wide(url).as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        );
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
