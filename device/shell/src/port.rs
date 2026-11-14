//! Where the device listens, resolved once at boot.
//!
//! `summrise-shell` used to name `127.0.0.1:18090` in three constants. That was right while the port
//! was a constant of the product; it is a deployment choice now (`--port` on the launcher's service
//! command line, canonical `7737` since 2026-11-04), and a tray that dials a hardcoded port is a tray
//! that reports "not running" for a device that is serving.
//!
//! The device writes the port it actually bound to `<data>/device.port`, beside `state.json`. This
//! reads the environment a test or an operator may set first, then that file — the only source that
//! cannot disagree with the running process — then the canonical default.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The port a device serves on when nothing else names one. The same number `summrise-device` and
/// `summrise-launch` default to.
pub const DEFAULT_PORT: u16 = 7_737;

/// The file the device writes its bound port to, in its data directory.
pub const PORT_FILE: &str = "device.port";

/// Where the device also leaves the port for the logged-in user, beside the helper token.
///
/// The data directory's ACL is SYSTEM and Administrators, so a shell running as the user cannot read
/// `<data>/device.port`; before this fallback it fell back to [`DEFAULT_PORT`] while the device served
/// 18090, and every panel and helper registration went to a dead port. `SummriseHelper` inherits
/// ProgramData's user-read ACL, which is why the device writes the same number there.
#[cfg(windows)]
const PUBLIC_PORT_FILE: &str = r"C:\ProgramData\SummriseHelper\device.port";

/// The `<data>` directory the device was installed with: `SUMMRISE_DATA` for a test or a
/// non-default install, `C:\ProgramData\Summrise` otherwise. The shell reads `device.port` and
/// `console.json` from it; both are written by the device or the launcher, not by the shell.
pub(crate) fn data_dir() -> PathBuf {
    std::env::var_os("SUMMRISE_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData\Summrise"))
}

/// The device's loopback port: the environment, its own port file, then [`DEFAULT_PORT`].
pub fn device_port() -> u16 {
    static PORT: OnceLock<u16> = OnceLock::new();
    *PORT.get_or_init(|| {
        if let Some(port) = std::env::var_os("SUMMRISE_PORT")
            .and_then(|value| {
                value
                    .to_str()
                    .and_then(|text| text.trim().parse::<u16>().ok())
            })
            .filter(|port| *port != 0)
        {
            return port;
        }
        read_port_at(&data_dir().join(PORT_FILE))
            .or_else(public_port)
            .unwrap_or(DEFAULT_PORT)
    })
}

/// Read a port file: the number, trimmed, with zero treated as "no port".
fn read_port_at(path: &Path) -> Option<u16> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| text.trim().parse::<u16>().ok())
        .filter(|port| *port != 0)
}

/// The user-readable copy, when this platform has one.
#[cfg(windows)]
fn public_port() -> Option<u16> {
    read_port_at(std::path::Path::new(PUBLIC_PORT_FILE))
}

#[cfg(not(windows))]
fn public_port() -> Option<u16> {
    None
}

/// `http://127.0.0.1:<port>/panel` — the device's own panel, the surface the shell opens when the
/// console is unreachable (§11, journey 6). `/` renders the same page for older links; `/panel` is
/// the path the design names and the one the page's own enrollment redirects return to.
pub fn panel_url() -> &'static str {
    static URL: OnceLock<String> = OnceLock::new();
    URL.get_or_init(|| format!("http://127.0.0.1:{}/panel", device_port()))
}

/// `127.0.0.1:<port>` — where the health document and the helper registration go.
pub fn health_addr() -> &'static str {
    static ADDR: OnceLock<String> = OnceLock::new();
    ADDR.get_or_init(|| format!("127.0.0.1:{}", device_port()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_the_canonical_port() {
        assert_eq!(DEFAULT_PORT, 7_737);
    }

    #[test]
    fn the_two_addresses_agree_on_the_port() {
        let port = device_port();
        assert!(
            panel_url().ends_with(&format!(":{port}/panel")),
            "{}",
            panel_url()
        );
        assert_eq!(health_addr(), format!("127.0.0.1:{port}"));
    }
}

#[cfg(test)]
mod port_file_tests {
    use super::read_port_at;

    #[test]
    fn a_port_file_is_read_trimmed_and_zero_is_no_port() {
        let dir = std::env::temp_dir().join(format!("summrise-port-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("device.port");
        std::fs::write(&path, " 18090\n").unwrap();
        assert_eq!(read_port_at(&path), Some(18090));
        std::fs::write(&path, "0").unwrap();
        assert_eq!(read_port_at(&path), None);
        std::fs::write(&path, "not a port").unwrap();
        assert_eq!(read_port_at(&path), None);
        assert_eq!(read_port_at(&dir.join("missing")), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
