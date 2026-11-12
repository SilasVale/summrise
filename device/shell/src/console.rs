//! Which page the window opens: the console when it answers, the device's own panel otherwise.
//!
//! The design's recovery guarantee (§11, journey 6) is concrete: *when the console is unreachable —
//! or no console is configured — the window opens the device's loopback panel*, `…/panel`. The tray
//! used to build every window for one hardcoded console URL, so an unreachable console was a
//! WebView2 error page and the operator had to find "Open local panel" in the menu to recover.
//!
//! The decision is deliberately portable and testable on Linux: the shell's Windows event loop
//! cannot run in CI, but the choice between two addresses can. The probe is a TCP connect to the
//! console's authority — enough to tell "nothing is listening" from "something is" without pulling
//! an HTTP client into a binary whose only other network call is the loopback health probe.
//!
//! The console is **configured**, not guessed. `SUMMRISE_CONSOLE_URL` names it for a test or
//! an operator; otherwise the shell reads `<data>/console.json` — the file the launcher writes from
//! `--console-url` and enrollment merges — for its `url`; a link with no `url` (a mesh-only install)
//! means this machine has no console. Only when no such source exists does the build's product
//! default apply. Setting `SUMMRISE_CONSOLE_URL` to an empty string is how a deployment says "no console
//! here"; a value that is not an `http://` or `https://` URL is treated the same way rather than
//! letting the window open a `file:` or `javascript:` target.

use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

/// The product console: the 0.3.4x console (fleet, terminal), not the legacy SPA. The shell and the
/// device sit on the same tailnet; a public hostname, when there is one, is the console's to publish.
pub const DEFAULT_CONSOLE: &str = "http://100.74.250.98:7736/";

/// The environment variable that names (or removes) the console.
pub const CONSOLE_ENV: &str = "SUMMRISE_CONSOLE_URL";

/// How long the window may wait for the console before it opens the panel instead. One short
/// connect, on the click that asks for a window, not a poll: a console on the same tailnet answers
/// in milliseconds, and a down one should cost the operator less than a second.
const PROBE_TIMEOUT: Duration = Duration::from_millis(900);

/// The console this process is configured to use, or `None` when it has none.
///
/// Read at each window open rather than cached: an operator can install the shell before the
/// console's address is known, and a stale decision is exactly the failure this fallback exists to
/// remove.
pub fn configured() -> Option<String> {
    if let Some(value) = std::env::var_os(CONSOLE_ENV) {
        return value
            .to_str()
            .and_then(|value| configured_from(Some(value)));
    }
    match read_console_file(&crate::port::data_dir().join(CONSOLE_FILE)) {
        // The product's console address is what a machine gets when no install step named one. It
        // is still probed below, so a stale default ends at the panel rather than an error page.
        FileConsole::Absent => Some(DEFAULT_CONSOLE.to_owned()),
        FileConsole::Named(url) => configured_from(Some(&url)),
        // A link that exists but names no console is an instruction, not a missing fact: the mesh
        // can be configured alone. An unreadable link is treated the same way — the panel still
        // works when the device's own configuration does not.
        FileConsole::Unnamed | FileConsole::Unreadable => None,
    }
}

/// The name of the file the launcher writes the remote link to, beside `state.json` and
/// `device.port`.
pub const CONSOLE_FILE: &str = "console.json";

/// What `<data>/console.json` says about the console, for the shell's three distinct cases.
#[derive(Debug, PartialEq, Eq)]
enum FileConsole {
    /// No file: a standalone install that never named a console.
    Absent,
    /// A URL named by the link.
    Named(String),
    /// The link exists for the mesh (or other facts) but names no console URL.
    Unnamed,
    /// The file exists and cannot be read or parsed; never guessed at.
    Unreadable,
}

fn read_console_file(path: &Path) -> FileConsole {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return FileConsole::Absent,
        Err(_) => return FileConsole::Unreadable,
    };
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_) => return FileConsole::Unreadable,
    };
    let Some(object) = value.as_object() else {
        return FileConsole::Unreadable;
    };
    match object.get("url").and_then(serde_json::Value::as_str) {
        Some(url) if !url.trim().is_empty() => FileConsole::Named(url.to_owned()),
        _ => FileConsole::Unnamed,
    }
}

fn configured_from(value: Option<&str>) -> Option<String> {
    match value {
        None => Some(DEFAULT_CONSOLE.to_owned()),
        Some(value) => {
            let value = value.trim();
            if value.is_empty() || host_port(value).is_err() {
                None
            } else {
                Some(value.to_owned())
            }
        }
    }
}

/// The URL the window opens: the console only if one is configured **and** answers, else `panel`.
pub fn window_url() -> String {
    choose(configured().as_deref(), probe, crate::port::panel_url())
}

/// The decision, separated from the environment and the network so it can be tested on any host.
///
/// `panel` is used verbatim; the caller passes the device's already-resolved loopback URL.
pub fn choose(console: Option<&str>, reachable: impl FnOnce(&str) -> bool, panel: &str) -> String {
    match console {
        Some(url) if reachable(url) => url.to_owned(),
        _ => panel.to_owned(),
    }
}

/// Whether something accepts a TCP connection at the console's authority.
///
/// A malformed or non-HTTP URL is not reachable by definition. DNS itself has no timeout in
/// `std`; the connect does, and a name that never resolves is the rarer failure (the product's
/// console is an address, not a name).
pub fn probe(url: &str) -> bool {
    let Ok((host, port)) = host_port(url) else {
        return false;
    };
    let Ok(addresses) = (host.as_str(), port).to_socket_addrs() else {
        return false;
    };
    // A bounded set: a name with many addresses must not multiply the click's wait.
    addresses
        .take(4)
        .any(|address| TcpStream::connect_timeout(&address, PROBE_TIMEOUT).is_ok())
}

/// The host and effective port of an `http://`/`https://` URL, without a URL crate.
///
/// This is the smallest parse that is correct for an origin: scheme, authority (userinfo removed),
/// an optional port, and the default port from the scheme. A path, query or fragment is ignored.
fn host_port(url: &str) -> Result<(String, u16), String> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| format!("{url}: no scheme"))?;
    let default_port = match scheme.to_ascii_lowercase().as_str() {
        "http" => 80,
        "https" => 443,
        other => return Err(format!("{url}: {other} is not http or https")),
    };
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or_else(|| rest.split(['/', '?', '#']).next().unwrap_or_default());
    if authority.is_empty() {
        return Err(format!("{url}: no host"));
    }
    // Bracketed IPv6: `[::1]:7736`, and `[::1]` for the scheme's default port.
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, tail) = rest
            .split_once(']')
            .ok_or_else(|| format!("{url}: unterminated IPv6 address"))?;
        let port = match tail {
            "" => default_port,
            _ => tail
                .strip_prefix(':')
                .ok_or_else(|| format!("{url}: unexpected text after the address"))?
                .parse::<u16>()
                .map_err(|_| format!("{url}: the port is not a number"))?,
        };
        return checked_port(host, port, url);
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => {
            let port = port
                .parse::<u16>()
                .map_err(|_| format!("{url}: the port is not a number"))?;
            checked_port(host, port, url)
        }
        Some(_) => Err(format!("{url}: no host")),
        None => checked_port(authority, default_port, url),
    }
}

fn checked_port(host: &str, port: u16, url: &str) -> Result<(String, u16), String> {
    if host.is_empty() || port == 0 {
        Err(format!("{url}: no host or port"))
    } else {
        Ok((host.to_owned(), port))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn an_answering_console_keeps_the_window_on_the_console() {
        assert_eq!(
            choose(
                Some("http://console:7736/"),
                |_| true,
                "http://127.0.0.1:7737/panel"
            ),
            "http://console:7736/"
        );
    }

    #[test]
    fn an_unreachable_console_opens_the_device_panel() {
        assert_eq!(
            choose(
                Some("http://console:7736/"),
                |_| false,
                "http://127.0.0.1:7737/panel"
            ),
            "http://127.0.0.1:7737/panel"
        );
    }

    #[test]
    fn no_console_at_all_opens_the_device_panel_without_probing() {
        assert_eq!(
            choose(
                None,
                |_| panic!("there is nothing to probe"),
                "http://127.0.0.1:7737/panel"
            ),
            "http://127.0.0.1:7737/panel"
        );
    }

    #[test]
    fn the_default_is_the_product_console_and_an_empty_setting_removes_it() {
        assert_eq!(configured_from(None).as_deref(), Some(DEFAULT_CONSOLE));
        assert_eq!(configured_from(Some("")), None);
        assert_eq!(configured_from(Some("   ")), None);
        assert_eq!(
            configured_from(Some(" http://console:7736/ ")).as_deref(),
            Some("http://console:7736/")
        );
        // A target the window must never open is not a console.
        assert_eq!(
            configured_from(Some("file:///C:/Summrise/panel.html")),
            None
        );
        assert_eq!(configured_from(Some("javascript:alert(1)")), None);
        assert_eq!(configured_from(Some("http://")), None);
    }

    #[test]
    fn a_console_file_names_the_console_and_a_mesh_only_link_does_not() {
        let directory = std::env::temp_dir().join(format!(
            "summrise-shell-console-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("the temp directory is writable");
        let path = directory.join(CONSOLE_FILE);

        assert_eq!(read_console_file(&path), FileConsole::Absent);
        std::fs::write(
            &path,
            r#"{"url":"http://console:7736/","token_file":"/tmp/console-token.json"}"#,
        )
        .expect("the link is writable");
        assert_eq!(
            read_console_file(&path),
            FileConsole::Named("http://console:7736/".to_owned())
        );
        // A mesh-only link is not a console, and an unreadable one is never guessed at.
        std::fs::write(&path, r#"{"mesh_broker":"wss://broker.example"}"#)
            .expect("the link is writable");
        assert_eq!(read_console_file(&path), FileConsole::Unnamed);
        std::fs::write(&path, "not json").expect("the file is writable");
        assert_eq!(read_console_file(&path), FileConsole::Unreadable);

        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn an_origin_is_parsed_with_or_without_a_port() {
        assert_eq!(
            host_port("http://100.74.250.98:7736/").unwrap(),
            ("100.74.250.98".to_owned(), 7736)
        );
        assert_eq!(
            host_port("https://console.example/path?q=1").unwrap(),
            ("console.example".to_owned(), 443)
        );
        assert_eq!(
            host_port("http://console.example").unwrap(),
            ("console.example".to_owned(), 80)
        );
        assert_eq!(
            host_port("http://operator:secret@console.example:9000/").unwrap(),
            ("console.example".to_owned(), 9000)
        );
        assert_eq!(
            host_port("http://[::1]:7737/panel").unwrap(),
            ("::1".to_owned(), 7737)
        );
        assert_eq!(
            host_port("https://[::1]/").unwrap(),
            ("::1".to_owned(), 443)
        );
        for bad in [
            "console.example",
            "ftp://console.example/",
            "http:///panel",
            "http://console.example:0/",
            "http://console.example:notaport/",
        ] {
            assert!(host_port(bad).is_err(), "{bad} must not parse");
        }
    }

    #[test]
    fn a_loopback_listener_is_reachable_and_a_malformed_url_is_not() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let port = listener.local_addr().expect("the port").port();
        let url = format!("http://127.0.0.1:{port}/panel");
        let accepting = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 1];
                let _ = stream.read(&mut buffer);
            }
        });
        assert!(probe(&url), "{url}");
        let _ = accepting.join();
        assert!(!probe("http://127.0.0.1:notaport/"));
        assert!(!probe("file:///C:/Summrise/panel.html"));
    }
}
