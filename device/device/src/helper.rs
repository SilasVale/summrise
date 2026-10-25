//! The per-user helper's half of the contract: a token this boot mints, and the registration that
//! says where the helper listens.
//!
//! The device runs as SYSTEM in session 0 and cannot show a tray, open a folder dialog, or launch a
//! browser in the logged-in user's session. `summrise-shell` does those, in the user's session. This
//! module is how the two find each other without the device trusting anything but a secret it wrote
//! itself.
//!
//! The file is the only thing shared, and it flows one way: the device writes it, the logged-in user
//! reads it. A user who can read it can hand the token back; a user who cannot write it cannot forge
//! one. The token is minted per boot, so a stolen one dies with the next restart.

use std::io;
use std::path::PathBuf;
use std::sync::Mutex;

/// Where the token is left for the logged-in user. `ProgramData`'s inherited ACL gives users read and
/// administrators write — exactly the direction this needs, and the reason the file is not in the
/// device's own data directory (which is protected to SYSTEM and Administrators).
#[cfg_attr(not(windows), allow(dead_code))]
pub const TOKEN_FILE: &str = r"C:\ProgramData\SummriseHelper\helper.token";

/// The process's helper identity, set once at startup. The browser capability is deep inside the MCP
/// service, which has no reason to know about a user-session helper; a per-boot singleton is the
/// smallest thing that reaches it without threading a parameter through five layers.
static CURRENT: std::sync::OnceLock<std::sync::Arc<Helper>> = std::sync::OnceLock::new();

/// Publish this boot's helper identity to the rest of the process.
pub fn install(helper: std::sync::Arc<Helper>) {
    let _ = CURRENT.set(helper);
}

/// This boot's helper identity, if startup installed one.
pub fn current() -> Option<std::sync::Arc<Helper>> {
    CURRENT.get().cloned()
}

/// This boot's helper facts.
pub struct Helper {
    token: String,
    port: Mutex<Option<u16>>,
    pid: Mutex<Option<u32>>,
}

impl Helper {
    /// Mint this boot's token and leave it where the helper can read it.
    ///
    /// A failure to write is returned rather than swallowed: without the file the helper can never
    /// register, and a silent failure would look like "the helper is not installed".
    pub fn start() -> io::Result<Helper> {
        let helper = Helper::with_token(mint());
        let path = token_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, &helper.token)?;
        Ok(helper)
    }

    /// A helper with a known token: the tests' door, and the reason the token is not minted inline.
    pub fn with_token(token: String) -> Helper {
        Helper {
            token,
            port: Mutex::new(None),
            pid: Mutex::new(None),
        }
    }

    /// The token as minted: read by the file test, and by a human debugging a registration.
    #[allow(dead_code)]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Whether a request carries this boot's token, compared in constant time over its bytes.
    pub fn authorized(&self, headers: &axum::http::HeaderMap) -> bool {
        let Some(value) = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
        else {
            return false;
        };
        let Some(presented) = value.strip_prefix("Bearer ") else {
            return false;
        };
        same(presented.as_bytes(), self.token.as_bytes())
    }

    /// Record where the helper said it listens, and which process said it.
    pub fn registered(&self, port: u16, pid: u32) {
        *self.port.lock().expect("the helper port is never poisoned") = Some(port);
        *self.pid.lock().expect("the helper pid is never poisoned") = Some(pid);
    }

    /// The helper process, once it has registered this boot.
    pub fn pid(&self) -> Option<u32> {
        *self.pid.lock().expect("the helper pid is never poisoned")
    }

    /// Where the helper listens, if it has registered this boot. The browser preference (next
    /// commit) is the caller that needs it; today only the registration test reads it.
    #[allow(dead_code)]
    pub fn port(&self) -> Option<u16> {
        *self.port.lock().expect("the helper port is never poisoned")
    }
}

/// Where the token goes: `ProgramData` on Windows, a temp directory anywhere else (the device only
/// runs on Windows in production; the tests run here).
pub fn token_path() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(TOKEN_FILE)
    }
    #[cfg(not(windows))]
    {
        std::env::temp_dir().join("summrise-helper.token")
    }
}

/// One capture from the helper: PNG bytes as base64, and the size it was taken at.
pub struct Capture {
    pub png: String,
    pub width: u32,
    pub height: u32,
}

/// Ask the helper for the logged-in user's screen.
///
/// The refusal is the feature as much as the picture: a locked or disconnected session answers with a
/// sentence, and this turns it into the tool's error rather than returning a black frame the operator
/// would have to interpret.
pub async fn capture(port: u16, token: &str) -> Result<Capture, String> {
    let answer = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/capture"))
        .bearer_auth(token)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|error| format!("the helper did not answer: {error}"))?;
    let status = answer.status();
    let text = answer
        .text()
        .await
        .map_err(|error| format!("the helper's answer was unreadable: {error}"))?;
    let body: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("the helper answered something else: {error}"))?;
    if !status.is_success() || body.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        let reason = body
            .get("error")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("the helper refused");
        return Err(reason.to_owned());
    }
    let png = body
        .get("png")
        .and_then(serde_json::Value::as_str)
        .ok_or("the helper's answer carries no picture")?;
    Ok(Capture {
        png: png.to_owned(),
        width: body
            .get("width")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32,
        height: body
            .get("height")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32,
    })
}

/// 32 bytes from the operating system's generator, as hex. The same shape the console uses for its
/// session tokens, and for the same reason: a token that is guessable is not a token.
fn mint() -> String {
    use rand_core::{OsRng, RngCore};

    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let mut text = String::with_capacity(64);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

/// Compare without an early exit: the time taken must not say how much of the token was right.
fn same(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{header, HeaderMap, HeaderValue};

    fn headers(bearer: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {bearer}")).expect("a header value"),
        );
        headers
    }

    #[test]
    fn only_the_minted_token_authorizes_a_registration() {
        let helper = Helper::with_token("a".repeat(64));
        assert!(helper.authorized(&headers(&"a".repeat(64))));
        assert!(!helper.authorized(&headers(&"b".repeat(64))));
        assert!(!helper.authorized(&headers("a".repeat(63).as_str())));
        assert!(!helper.authorized(&HeaderMap::new()));
    }

    #[test]
    fn the_token_is_64_hex_characters_and_different_each_time() {
        let first = mint();
        let second = mint();
        assert_eq!(first.len(), 64);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }

    #[test]
    fn a_registration_records_the_port_and_the_process() {
        let helper = Helper::with_token("a".repeat(64));
        assert_eq!(helper.port(), None);
        assert_eq!(helper.pid(), None);
        helper.registered(49812, 2260);
        assert_eq!(helper.port(), Some(49812));
        assert_eq!(helper.pid(), Some(2260));
    }

    #[test]
    fn start_leaves_the_token_where_the_helper_reads_it() {
        let helper = Helper::start().expect("the token file is written");
        let path = token_path();
        let on_disk = std::fs::read_to_string(&path).expect("the token file exists");
        assert_eq!(on_disk, helper.token());
        let _ = std::fs::remove_file(path);
    }
}
