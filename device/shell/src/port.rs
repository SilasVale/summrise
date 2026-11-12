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

use std::path::PathBuf;
use std::sync::OnceLock;

/// The port a device serves on when nothing else names one. The same number `summrise-device` and
/// `summrise-launch` default to.
pub const DEFAULT_PORT: u16 = 7_737;

/// The file the device writes its bound port to, in its data directory.
pub const PORT_FILE: &str = "device.port";

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
        std::fs::read_to_string(data_dir().join(PORT_FILE))
            .ok()
            .and_then(|text| text.trim().parse::<u16>().ok())
            .filter(|port| *port != 0)
            .unwrap_or(DEFAULT_PORT)
    })
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
