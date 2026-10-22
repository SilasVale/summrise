//! The one-shot enrolment mode: register this device with the operator's console.
//!
//! The console's registration contract is `POST /devices` with
//! `{id, public_key, code, ts_ms, signature}`, where the signature is the device identity's
//! Ed25519 signature over exactly `register\n{id}\n{code}\n{ts_ms}`. The identity is the same
//! `<data>/identity.key` the mesh uses, so one key is this device's name on both remote paths.
//!
//! On a 200 the mode writes the two files the serving app already reads: the registration
//! response as `<data>/console-token.json`, and the console half of `<data>/console.json` merged
//! into whatever mesh facts the file already carries. Both are owner-only on Unix, SYSTEM/
//! Administrators-only on Windows (`summrise_winsec`, with the DACL read back), and written through
//! a random temporary sibling and a rename, so a crash leaves either the previous file or the
//! complete new one.
//!
//! The console URL follows the release path's transport rule: `https` unless the host is loopback,
//! with no userinfo, query or fragment. The registration carries the one-time code and its answer the
//! long-lived channel token, so plain `http` to a remote console would put both on the wire.
//!
//! Exit codes are the mode's contract: 0 enrolled, 2 usage (a missing argument or a URL that is not
//! an absolute https URL, or a loopback `http` one), 1 every refusal (a spent code, an unreachable
//! console, a signature the console refused, or an existing token file without `--force`).

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::config::CONSOLE_FILE;
use crate::fsutil;
use crate::identity;

/// The file the console's registration response is written to, under `<data>`.
const TOKEN_FILE: &str = "console-token.json";

/// The largest registration response this mode will read. A registration answer is a few hundred
/// bytes; a body past this is not one, and refusing it keeps a broken console from making the mode
/// hold an unbounded buffer.
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// How long one enrolment request may take, body included.
const TOTAL_TIMEOUT: Duration = Duration::from_secs(30);

/// How long establishing the connection may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// A parsed `--enroll <console-url> <code> [--data <data>] [--install <install>] [--force]` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EnrollInvocation {
    /// The console's base URL, as the operator wrote it.
    pub(crate) console_url: String,
    /// The one-time code the operator minted.
    pub(crate) code: String,
    /// The data directory holding `identity.key`, `console-token.json` and `console.json`.
    pub(crate) data: PathBuf,
    /// Replace an existing token file instead of refusing.
    pub(crate) force: bool,
    /// Accept a plaintext console base on a name the operator names explicitly.
    ///
    /// The default is https (or plain http to a loopback host) because the one-time code and the
    /// long-lived channel token cross this connection. A console that lives on a private tunnel
    /// (Tailscale, WireGuard, an SSH port-forward) is already encrypted underneath; this flag is
    /// the operator saying so, it is printed once, and it never becomes the default.
    pub(crate) allow_plaintext: bool,
}

/// What a successful enrolment wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Enrollment {
    /// The device id the console registered (the identity's public key in hex).
    pub(crate) id: String,
    /// The channel token the console issued, as 64 lowercase hex characters.
    pub(crate) token: String,
}

/// Why the console refused a registration it could read.
///
/// The console answers 403 with a closed-set `{"error":"…"}` code; these are the codes that name a
/// device-side problem, and every other code stays an [`ConsoleRefusal::Other`] so the operator sees
/// what the console actually said instead of a message this mode invented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ConsoleRefusal {
    /// The one-time code is unknown, expired, or already spent.
    SpentCode,
    /// The Ed25519 signature over `register\n{id}\n{code}\n{ts_ms}` did not verify.
    BadSignature,
    /// `ts_ms` is more than five minutes from the console's clock.
    StaleTimestamp,
    /// A refusal code this build does not know by name.
    Other(String),
}

impl fmt::Display for ConsoleRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConsoleRefusal::SpentCode => {
                f.write_str("the one-time code is unknown, expired, or already spent")
            }
            ConsoleRefusal::BadSignature => {
                f.write_str("the registration signature did not verify")
            }
            ConsoleRefusal::StaleTimestamp => {
                f.write_str("the device clock is more than five minutes from the console's")
            }
            ConsoleRefusal::Other(code) => write!(f, "the console answered {code:?}"),
        }
    }
}

/// Why the enrolment mode could not complete.
#[derive(Debug)]
pub(crate) enum EnrollError {
    /// The command line is missing an argument or names a URL that is not absolute http(s).
    Usage,
    /// The console URL is not an absolute https URL, a loopback `http` URL, or has userinfo/query.
    BadUrl(String),
    /// The data directory could not be created.
    DataDir {
        /// The configured data directory.
        path: PathBuf,
        /// The OS error.
        source: io::Error,
    },
    /// The identity file could not be read or minted.
    Identity(identity::IdentityError),
    /// The console could not be reached, or its answer could not be read.
    Console {
        /// The console base URL, or the request URL when it differs.
        url: String,
        /// What went wrong.
        detail: String,
    },
    /// The console answered a status other than 200.
    Status {
        /// The request URL.
        url: String,
        /// The status code.
        status: u16,
    },
    /// The console read the registration and refused it.
    Refused {
        /// The request URL.
        url: String,
        /// The status code (the console's refusals are 403).
        status: u16,
        /// The console's closed-set reason.
        reason: ConsoleRefusal,
    },
    /// The console answered a body that is not a registration response.
    BadResponse {
        /// What about the body was not a registration response.
        detail: String,
    },
    /// An existing `<data>/console.json` could not be read or is not an object.
    ConsoleFile {
        /// The file's path.
        path: PathBuf,
        /// The refusal.
        detail: String,
    },
    /// A token file already exists and `--force` was not given.
    TokenConflict {
        /// The existing file.
        path: PathBuf,
    },
    /// A file could not be written.
    Write {
        /// The destination.
        path: PathBuf,
        /// The OS error.
        source: io::Error,
    },
    /// Windows could not install or read back the DACL of a file this mode writes.
    ///
    /// A token or key that anybody but SYSTEM and Administrators can read is not written by this
    /// mode; the Win32 refusal already names the path.
    #[cfg(windows)]
    Protect {
        /// The destination.
        path: PathBuf,
        /// The Win32 refusal.
        detail: String,
    },
}

impl EnrollError {
    /// The process exit code the mode's contract assigns this refusal.
    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            EnrollError::Usage | EnrollError::BadUrl(_) => 2,
            EnrollError::DataDir { .. }
            | EnrollError::Identity(_)
            | EnrollError::Console { .. }
            | EnrollError::Status { .. }
            | EnrollError::Refused { .. }
            | EnrollError::BadResponse { .. }
            | EnrollError::ConsoleFile { .. }
            | EnrollError::TokenConflict { .. }
            | EnrollError::Write { .. } => 1,
            #[cfg(windows)]
            EnrollError::Protect { .. } => 1,
        }
    }
}

impl fmt::Display for EnrollError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnrollError::Usage => f.write_str(
                "--enroll needs <console-url> <code> [--data <data>] [--install <install>] \
                 [--force] [--allow-plaintext]",
            ),
            EnrollError::BadUrl(url) => write!(
                f,
                "the console URL '{url}' must be an absolute https URL with a host and no \
                 userinfo, query or fragment (plain http is allowed only for a loopback host)"
            ),
            EnrollError::DataDir { path, source } => {
                write!(
                    f,
                    "cannot create the data directory {}: {source}",
                    path.display()
                )
            }
            EnrollError::Identity(error) => write!(f, "{error}"),
            EnrollError::Console { url, detail } => {
                write!(f, "cannot reach the console at {url}: {detail}")
            }
            EnrollError::Status { url, status } => {
                write!(f, "the console at {url} answered HTTP {status}")
            }
            EnrollError::Refused {
                url,
                status,
                reason,
            } => {
                write!(
                    f,
                    "the console at {url} refused the registration (HTTP {status}): {reason}"
                )
            }
            EnrollError::BadResponse { detail } => {
                write!(
                    f,
                    "the console did not answer a registration response: {detail}"
                )
            }
            EnrollError::ConsoleFile { path, detail } => {
                write!(
                    f,
                    "cannot use the console link {}: {detail}",
                    path.display()
                )
            }
            EnrollError::TokenConflict { path } => write!(
                f,
                "the console token file {} already exists; pass --force to replace it \
                 (registering again rotates the token)",
                path.display()
            ),
            EnrollError::Write { path, source } => {
                write!(f, "cannot write {}: {source}", path.display())
            }
            #[cfg(windows)]
            EnrollError::Protect { path, detail } => write!(
                f,
                "refusing to write {} unprotected: {detail}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for EnrollError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EnrollError::DataDir { source, .. } | EnrollError::Write { source, .. } => Some(source),
            EnrollError::Identity(error) => Some(error),
            _ => None,
        }
    }
}

/// The registration request the console's `POST /devices` accepts.
#[derive(Serialize)]
struct RegisterBody<'a> {
    id: &'a str,
    public_key: &'a str,
    code: &'a str,
    ts_ms: u64,
    signature: &'a str,
}

/// The registration response, of which this mode keeps the id and token.
#[derive(Deserialize)]
struct RegisterResponse {
    id: String,
    token: String,
}

/// The console's refusal body: `{"error":"<closed-set code>"}`.
#[derive(Deserialize)]
struct ErrorBody {
    error: String,
}

/// Enrol this device with the console at `invocation.console_url`.
pub(crate) async fn enroll(invocation: &EnrollInvocation) -> Result<Enrollment, EnrollError> {
    let url = parse_console_url(&invocation.console_url, invocation.allow_plaintext)?;
    fs::create_dir_all(&invocation.data).map_err(|source| EnrollError::DataDir {
        path: invocation.data.clone(),
        source,
    })?;
    // Windows: the two credentials below are written under `<data>`. Protect the directory before
    // the identity is minted and before the token's temporary sibling exists, so both inherit a DACL
    // that grants SYSTEM and Administrators only. The read-back inside `protect_dir` is the proof.
    #[cfg(windows)]
    summrise_winsec::protect_dir(&invocation.data).map_err(|detail| EnrollError::Protect {
        path: invocation.data.clone(),
        detail,
    })?;
    // Decide the token-file conflict before the request, not after: a registration rotates the
    // console's token and spends the one-time code, so the safe moment to demand `--force` is while
    // both are still untouched. Any existing entry is a conflict — a file, a directory or a broken
    // symlink — because this mode can only ever replace it with a different token.
    let token_path = invocation.data.join(TOKEN_FILE);
    let token_path = std::path::absolute(&token_path).map_err(|source| EnrollError::Write {
        path: token_path.clone(),
        source,
    })?;
    match fs::symlink_metadata(&token_path) {
        Ok(_) if !invocation.force => return Err(EnrollError::TokenConflict { path: token_path }),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(EnrollError::Write {
                path: token_path,
                source,
            })
        }
    }
    // Read and parse the existing link before the request too: a malformed `console.json` is a
    // configuration error the operator must fix, and discovering it after the code was spent would
    // make them mint a second one to learn the same thing.
    let console_path = invocation.data.join(CONSOLE_FILE);
    let link = read_console_link(&console_path)?;
    let identity = identity::load_or_mint(&invocation.data).map_err(EnrollError::Identity)?;

    let id = identity.public_hex();
    let ts_ms = summrise_release::now_ms();
    let message = format!("register\n{}\n{}\n{}", id, invocation.code, ts_ms);
    let signature = summrise_mesh::keys::hex_encode(&identity.sign(message.as_bytes()));
    let body = RegisterBody {
        id: &id,
        public_key: &id,
        code: &invocation.code,
        ts_ms,
        signature: &signature,
    };
    let body = serde_json::to_vec(&body).map_err(|error| EnrollError::BadResponse {
        detail: format!("cannot encode the registration: {error}"),
    })?;

    let request_url = devices_url(&url);
    let response = request(&request_url, &body).await?;
    let status = response.status();
    let bytes = read_body(response, &request_url).await?;
    if status == reqwest::StatusCode::FORBIDDEN {
        let code = serde_json::from_slice::<ErrorBody>(&bytes)
            .map(|body| body.error)
            .unwrap_or_else(|_| "HTTP 403".to_owned());
        let reason = match code.as_str() {
            "unknown_code" => ConsoleRefusal::SpentCode,
            "bad_signature" => ConsoleRefusal::BadSignature,
            "stale_timestamp" => ConsoleRefusal::StaleTimestamp,
            _ => ConsoleRefusal::Other(code),
        };
        return Err(EnrollError::Refused {
            url: request_url.to_string(),
            status: status.as_u16(),
            reason,
        });
    }
    if status != reqwest::StatusCode::OK {
        return Err(EnrollError::Status {
            url: request_url.to_string(),
            status: status.as_u16(),
        });
    }
    let response: RegisterResponse =
        serde_json::from_slice(&bytes).map_err(|error| EnrollError::BadResponse {
            detail: error.to_string(),
        })?;
    if response.id != id {
        return Err(EnrollError::BadResponse {
            detail: format!(
                "the console registered id '{}', not this device's id '{id}'",
                response.id
            ),
        });
    }
    if !is_hex_64(&response.token) {
        return Err(EnrollError::BadResponse {
            detail: "the token is not 64 hex characters".to_owned(),
        });
    }

    let token_bytes = serde_json::to_vec(&TokenFile {
        id: &response.id,
        token: &response.token,
    })
    .map_err(|error| EnrollError::BadResponse {
        detail: format!("cannot encode the registration response: {error}"),
    })?;
    let link_bytes = merge_console_link(link, &invocation.console_url, &token_path, &console_path)?;
    write_private(&token_path, &token_bytes)?;
    write_private(&console_path, &link_bytes)?;

    Ok(Enrollment {
        id: response.id,
        token: response.token,
    })
}

/// The shape `<data>/console-token.json` holds: exactly the two values the channel needs.
#[derive(Serialize)]
struct TokenFile<'a> {
    id: &'a str,
    token: &'a str,
}

/// Parse and validate the console base URL.
///
/// This is the release path's transport rule plus the shape this route needs: `summrise_release`
/// owns "https unless loopback" (`http` is allowed only for `127.0.0.1`, `::1` and `localhost`), so
/// enrollment cannot post the one-time code and receive the long-lived channel token in cleartext to
/// a remote console (review I1). A query or fragment is refused because the route appended below is
/// `/devices`; userinfo is refused because the base is persisted in `console.json` and printed.
fn parse_console_url(text: &str, allow_plaintext: bool) -> Result<Url, EnrollError> {
    let text = text.trim();
    let url = match summrise_release::parse_release_url(text) {
        Ok(url) => url,
        // `--allow-plaintext` widens the scheme, nothing else: an absolute http URL with a host,
        // no userinfo, query or fragment. Everything the strict rule checks about the shape still
        // applies, so the flag cannot smuggle in a redirect target or a credential.
        Err(_) if allow_plaintext => {
            let url = Url::parse(text).map_err(|_| EnrollError::BadUrl(text.to_owned()))?;
            if url.scheme() != "http" {
                return Err(EnrollError::BadUrl(text.to_owned()));
            }
            url
        }
        Err(_) => return Err(EnrollError::BadUrl(text.to_owned())),
    };
    if url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(EnrollError::BadUrl(text.to_owned()));
    }
    Ok(url)
}

/// The registration route under a console base that may carry a path prefix.
fn devices_url(base: &Url) -> Url {
    let mut url = base.clone();
    let path = format!("{}/devices", url.path().trim_end_matches('/'));
    url.set_path(&path);
    url
}

/// POST the registration body and return the raw response.
async fn request(url: &Url, body: &[u8]) -> Result<reqwest::Response, EnrollError> {
    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(TOTAL_TIMEOUT)
        // A redirect is a status this mode reports, not a second host it follows: the body carries
        // a one-time code, and an honest console has no reason to move the request elsewhere.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| EnrollError::Console {
            url: url.to_string(),
            detail: error.to_string(),
        })?;
    client
        .post(url.clone())
        .header("content-type", "application/json")
        .body(body.to_vec())
        .send()
        .await
        .map_err(|error| EnrollError::Console {
            url: url.to_string(),
            detail: error.to_string(),
        })
}

/// Read a response body, refusing one larger than the cap while it is being read.
async fn read_body(mut response: reqwest::Response, url: &Url) -> Result<Vec<u8>, EnrollError> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| EnrollError::Console {
            url: url.to_string(),
            detail: error.to_string(),
        })?
    {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(EnrollError::BadResponse {
                detail: format!("the body is larger than the {MAX_RESPONSE_BYTES}-byte cap"),
            });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Read `<data>/console.json`, or an empty map when the file does not exist.
fn read_console_link(path: &Path) -> Result<Map<String, Value>, EnrollError> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| EnrollError::ConsoleFile {
            path: path.to_path_buf(),
            detail: error.to_string(),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Map::new()),
        Err(error) => Err(EnrollError::ConsoleFile {
            path: path.to_path_buf(),
            detail: error.to_string(),
        }),
    }
}

/// Merge the console half into an already-read link, keeping every other field (the mesh's).
fn merge_console_link(
    mut link: Map<String, Value>,
    console_url: &str,
    token_path: &Path,
    path: &Path,
) -> Result<Vec<u8>, EnrollError> {
    link.insert(
        "url".to_owned(),
        Value::String(console_url.trim().to_owned()),
    );
    link.insert(
        "token_file".to_owned(),
        Value::String(token_path.to_string_lossy().into_owned()),
    );
    serde_json::to_vec(&Value::Object(link)).map_err(|error| EnrollError::ConsoleFile {
        path: path.to_path_buf(),
        detail: format!("cannot encode the link: {error}"),
    })
}

/// Write `bytes` to `path` through a random temporary sibling and a rename, owner-only on Unix.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), EnrollError> {
    // The temporary is created 0600 (Unix), not chmodded after: the token is a credential even
    // under its temporary name, and the rename carries the mode to the destination.
    let (temporary, mut file) =
        fsutil::create_new_sibling_private(path).map_err(|source| EnrollError::Write {
            path: path.to_path_buf(),
            source,
        })?;
    // Windows: protect the temporary before the secret is written, so the token is never readable
    // under the inherited DACL even for the instant it carries the temporary name. The rename
    // carries the descriptor to the destination; the explicit protect below proves the final path.
    #[cfg(windows)]
    if let Err(detail) = summrise_winsec::protect_file(&temporary) {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(EnrollError::Protect {
            path: path.to_path_buf(),
            detail,
        });
    }
    if let Err(source) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(EnrollError::Write {
            path: path.to_path_buf(),
            source,
        });
    }
    drop(file);
    if let Err(source) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(EnrollError::Write {
            path: path.to_path_buf(),
            source,
        });
    }
    #[cfg(windows)]
    summrise_winsec::protect_file(path).map_err(|detail| EnrollError::Protect {
        path: path.to_path_buf(),
        detail,
    })?;
    Ok(())
}

/// Whether `text` is exactly 64 hex characters.
fn is_hex_64(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Parse the arguments after `--enroll`.
///
/// `<data>` comes from `--data`, then `SUMMRISE_DATA`, then `<install>/data`, where `<install>` is
/// `--install`, `SUMMRISE_INSTALL`, or the directory this binary lives in. That is the launcher's
/// own default, so an installed `summrise-device --enroll <url> <code>` needs no path flags.
pub(crate) fn parse_enroll_args(args: &[OsString]) -> Result<EnrollInvocation, EnrollError> {
    let mut url = None;
    let mut code = None;
    let mut data = None;
    let mut install = None;
    let mut force = false;
    let mut allow_plaintext = false;

    let mut arguments = args.iter();
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--data") => {
                let value = arguments.next().ok_or(EnrollError::Usage)?;
                data = Some(PathBuf::from(value));
            }
            Some("--install") => {
                let value = arguments.next().ok_or(EnrollError::Usage)?;
                install = Some(PathBuf::from(value));
            }
            Some("--force") => force = true,
            Some("--allow-plaintext") => allow_plaintext = true,
            Some(flag) if flag.starts_with("--") => return Err(EnrollError::Usage),
            _ => {
                let text = argument.to_str().ok_or(EnrollError::Usage)?;
                if url.is_none() {
                    url = Some(text.to_owned());
                } else if code.is_none() {
                    code = Some(text.to_owned());
                } else {
                    return Err(EnrollError::Usage);
                }
            }
        }
    }

    let console_url = url.ok_or(EnrollError::Usage)?.trim().to_owned();
    let code = code.ok_or(EnrollError::Usage)?;
    if code.is_empty() {
        return Err(EnrollError::Usage);
    }
    parse_console_url(&console_url, allow_plaintext)?;

    let data = data
        .or_else(|| std::env::var_os("SUMMRISE_DATA").map(PathBuf::from))
        .or_else(|| {
            install
                .or_else(|| std::env::var_os("SUMMRISE_INSTALL").map(PathBuf::from))
                .or_else(|| {
                    std::env::current_exe()
                        .ok()
                        .and_then(|exe| exe.parent().map(Path::to_path_buf))
                })
                .map(|install| install.join("data"))
        })
        .ok_or(EnrollError::Usage)?;

    Ok(EnrollInvocation {
        console_url,
        code,
        data,
        force,
        allow_plaintext,
    })
}

/// The mode the entry point calls: parse the arguments and enrol.
pub(crate) async fn enroll_from_args(args: &[OsString]) -> Result<Enrollment, EnrollError> {
    let invocation = parse_enroll_args(args)?;
    enroll(&invocation).await
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::post;
    use axum::{Json, Router};
    use ed25519_dalek::{Signature, VerifyingKey};
    use serde_json::{json, Value};
    use tokio::net::TcpListener;

    use super::*;
    use crate::test_support::TempDir;

    /// What one fake console answers.
    struct FakeState {
        /// When set, every registration is refused with this console error code.
        refusal: Option<&'static str>,
        /// The next token's counter; the token is its 64-hex form.
        tokens: AtomicU64,
        /// Every body the console received, in order.
        seen: Mutex<Vec<Value>>,
    }

    /// An in-process console that verifies the registration proof the way the real one does.
    struct FakeConsole {
        base_url: String,
        state: Arc<FakeState>,
        _handle: tokio::task::JoinHandle<()>,
    }

    impl FakeConsole {
        async fn start(refusal: Option<&'static str>) -> FakeConsole {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("a loopback listener");
            let base_url = format!("http://{}", listener.local_addr().expect("an address"));
            let state = Arc::new(FakeState {
                refusal,
                tokens: AtomicU64::new(0),
                seen: Mutex::new(Vec::new()),
            });
            let app = Router::new()
                .route("/devices", post(register))
                .with_state(Arc::clone(&state));
            let handle = tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });
            FakeConsole {
                base_url,
                state,
                _handle: handle,
            }
        }

        /// Every registration body the console received, in order.
        fn requests(&self) -> Vec<Value> {
            self.state.seen.lock().expect("the body list").clone()
        }

        /// The token the `index`-th successful registration issued (zero-based).
        fn token(&self, index: u64) -> String {
            format!("{:064x}", index + 1)
        }
    }

    async fn register(State(state): State<Arc<FakeState>>, Json(body): Json<Value>) -> Response {
        state.seen.lock().expect("the body list").push(body.clone());
        if let Some(refusal) = state.refusal {
            return (StatusCode::FORBIDDEN, Json(json!({ "error": refusal }))).into_response();
        }

        let Some(id) = body.get("id").and_then(Value::as_str) else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_id" })),
            )
                .into_response();
        };
        let Some(public_key) = body.get("public_key").and_then(Value::as_str) else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_public_key" })),
            )
                .into_response();
        };
        let Some(code) = body.get("code").and_then(Value::as_str) else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "missing_code" })),
            )
                .into_response();
        };
        let Some(ts_ms) = body.get("ts_ms").and_then(Value::as_u64) else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "missing_ts" })),
            )
                .into_response();
        };
        let Some(signature) = body.get("signature").and_then(Value::as_str) else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "missing_signature" })),
            )
                .into_response();
        };

        let Ok(id_bytes) = summrise_mesh::keys::hex_decode(id) else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_id" })),
            )
                .into_response();
        };
        let Ok(key_bytes) = summrise_mesh::keys::hex_decode(public_key) else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_public_key" })),
            )
                .into_response();
        };
        let (Ok(id_bytes), Ok(key_bytes)) = (
            <[u8; 32]>::try_from(id_bytes),
            <[u8; 32]>::try_from(key_bytes),
        ) else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_key" })),
            )
                .into_response();
        };
        if id_bytes != key_bytes {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "id_mismatch" })),
            )
                .into_response();
        }
        let Ok(key) = VerifyingKey::from_bytes(&key_bytes) else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_public_key" })),
            )
                .into_response();
        };
        let Ok(signature) = summrise_mesh::keys::hex_decode(signature).and_then(|bytes| {
            <[u8; 64]>::try_from(bytes).map_err(|_| summrise_mesh::KeyError::BadHex)
        }) else {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "bad_signature" })),
            )
                .into_response();
        };
        let message = format!("register\n{id}\n{code}\n{ts_ms}");
        if key
            .verify_strict(message.as_bytes(), &Signature::from_bytes(&signature))
            .is_err()
        {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "bad_signature" })),
            )
                .into_response();
        }

        let token = format!("{:064x}", state.tokens.fetch_add(1, Ordering::SeqCst) + 1);
        (
            StatusCode::OK,
            Json(json!({
                "id": id,
                "public_key": public_key,
                "registered_at_ms": 0,
                "last_seen_ms": 0,
                "token": token,
            })),
        )
            .into_response()
    }

    #[cfg(unix)]
    fn mode(path: &std::path::Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .expect("the file exists")
            .permissions()
            .mode()
            & 0o777
    }

    #[tokio::test]
    async fn a_valid_enrolment_writes_the_token_and_the_console_link_owner_only() {
        let console = FakeConsole::start(None).await;
        let dir = TempDir::new("enroll-valid");
        let data = dir.join("data");
        let invocation = EnrollInvocation {
            console_url: console.base_url.clone(),
            code: "a".repeat(64),
            data: data.clone(),
            force: false,
            allow_plaintext: false,
        };

        let enrollment = enroll(&invocation)
            .await
            .expect("a valid registration response enrols the device");

        assert_eq!(enrollment.id.len(), 64, "the id is the public key in hex");
        assert_eq!(
            enrollment.token,
            console.token(0),
            "the mode returns the token the console issued"
        );

        let token_path = data.join("console-token.json");
        let token: Value = serde_json::from_slice(
            &fs::read(&token_path).expect("the registration response is written"),
        )
        .expect("the token file is JSON");
        assert_eq!(token["id"], enrollment.id);
        assert_eq!(token["token"], enrollment.token);

        let link_path = data.join("console.json");
        let link: Value =
            serde_json::from_slice(&fs::read(&link_path).expect("the console link is written"))
                .expect("the console link is JSON");
        assert_eq!(link["url"], console.base_url);
        assert_eq!(
            link["token_file"].as_str(),
            Some(token_path.to_str().expect("a UTF-8 path")),
            "the link names the token file this run wrote"
        );

        assert!(
            data.join("identity.key").is_file(),
            "the identity is minted beside the files it named"
        );
        let requests = console.requests();
        assert_eq!(requests.len(), 1, "one registration was sent");
        assert_eq!(requests[0]["id"], enrollment.id);
        assert_eq!(requests[0]["code"], invocation.code);
        #[cfg(unix)]
        {
            assert_eq!(mode(&token_path), 0o600, "the token is owner-only");
            assert_eq!(mode(&link_path), 0o600, "the console link is owner-only");
        }
    }

    #[tokio::test]
    async fn a_second_enrolment_is_refused_without_force_and_replaces_with_it() {
        let console = FakeConsole::start(None).await;
        let dir = TempDir::new("enroll-conflict");
        let data = dir.join("data");
        let first = EnrollInvocation {
            console_url: console.base_url.clone(),
            code: "a".repeat(64),
            data: data.clone(),
            force: false,
            allow_plaintext: false,
        };
        let first = enroll(&first).await.expect("the first code enrols");
        let token_path = data.join("console-token.json");
        let before = fs::read(&token_path).expect("the first response is on disk");

        let second = EnrollInvocation {
            console_url: console.base_url.clone(),
            code: "b".repeat(64),
            data: data.clone(),
            force: false,
            allow_plaintext: false,
        };
        let error = enroll(&second)
            .await
            .expect_err("a second enrolment replaces a working token");
        assert!(
            matches!(error, EnrollError::TokenConflict { .. }),
            "the refusal is typed, not a string: {error}"
        );
        assert_eq!(error.exit_code(), 1, "a token-file conflict is a refusal");
        assert_eq!(
            fs::read(&token_path).expect("the first response is still there"),
            before,
            "the refused run leaves the token alone"
        );
        assert_eq!(
            console.requests().len(),
            1,
            "the refused run does not contact the console, so it spends no code"
        );

        let forced = EnrollInvocation {
            force: true,
            ..second
        };
        let replaced = enroll(&forced)
            .await
            .expect("--force replaces the token file");
        assert_eq!(
            console.requests().len(),
            2,
            "the forced run is the one that reaches the console"
        );
        assert_ne!(
            replaced.token, first.token,
            "a new registration rotates the token"
        );
        let token: Value = serde_json::from_slice(
            &fs::read(&token_path).expect("the replaced response is on disk"),
        )
        .expect("the token file is JSON");
        assert_eq!(token["token"], replaced.token);
    }

    #[tokio::test]
    async fn a_spent_code_is_a_typed_refusal_that_writes_nothing() {
        let console = FakeConsole::start(Some("unknown_code")).await;
        let dir = TempDir::new("enroll-spent");
        let data = dir.join("data");
        let invocation = EnrollInvocation {
            console_url: console.base_url.clone(),
            code: "c".repeat(64),
            data: data.clone(),
            force: false,
            allow_plaintext: false,
        };

        let error = enroll(&invocation)
            .await
            .expect_err("a spent code cannot enrol");
        assert!(
            matches!(
                error,
                EnrollError::Refused {
                    status: 403,
                    reason: ConsoleRefusal::SpentCode,
                    ..
                }
            ),
            "the refusal is the console's own answer: {error}"
        );
        assert_eq!(error.exit_code(), 1, "a spent code is a refusal");
        assert!(
            !data.join("console-token.json").exists(),
            "a refused registration writes no token"
        );
        assert!(
            !data.join("console.json").exists(),
            "a refused registration arms no console link"
        );
    }

    #[tokio::test]
    async fn a_refused_signature_is_a_typed_refusal_that_writes_nothing() {
        let console = FakeConsole::start(Some("bad_signature")).await;
        let dir = TempDir::new("enroll-bad-signature");
        let data = dir.join("data");
        let invocation = EnrollInvocation {
            console_url: console.base_url.clone(),
            code: "d".repeat(64),
            data: data.clone(),
            force: false,
            allow_plaintext: false,
        };

        let error = enroll(&invocation)
            .await
            .expect_err("the console's signature check is the one that decides");
        assert!(
            matches!(
                error,
                EnrollError::Refused {
                    status: 403,
                    reason: ConsoleRefusal::BadSignature,
                    ..
                }
            ),
            "the refusal names the console's reason: {error}"
        );
        assert_eq!(error.exit_code(), 1);
        assert!(!data.join("console-token.json").exists());
        assert!(!data.join("console.json").exists());
    }

    #[test]
    fn a_bad_console_url_is_a_usage_error() {
        let dir = TempDir::new("enroll-bad-url");
        let data = dir.join("data");
        for url in [
            "not-a-url",
            "ftp://console.example.test",
            "http://",
            "http://console.example.test/?query=1",
            "http://console.example.test/#fragment",
        ] {
            let args = vec![
                OsString::from(url),
                OsString::from("code"),
                OsString::from("--data"),
                data.clone().into_os_string(),
            ];
            let error = parse_enroll_args(&args)
                .expect_err("a URL that is not a console base cannot enrol");
            assert!(matches!(error, EnrollError::BadUrl(_)), "{url}: {error}");
            assert_eq!(error.exit_code(), 2, "{url}: a bad URL is usage");
        }
    }

    #[test]
    fn the_console_url_is_https_unless_loopback_and_carries_no_userinfo() {
        for url in [
            "https://console.example.test",
            "https://console.example.test/console",
            "http://127.0.0.1:18082",
            "http://[::1]:18082",
            "http://localhost:18082",
        ] {
            assert_eq!(
                parse_console_url(url, false)
                    .ok()
                    .map(|parsed| parsed.to_string()),
                Url::parse(url).ok().map(|parsed| parsed.to_string()),
                "{url} is an allowed console base"
            );
        }

        for url in [
            // The long-lived channel token comes back over this URL: plain http to a remote host
            // would put the one-time code and the token on the wire in cleartext (review I1).
            "http://console.example.test",
            "http://203.0.113.10:18082",
            // The base is persisted in console.json and printed, so credentials in it leak twice.
            "https://user:pass@console.example.test",
            "https://user@console.example.test",
        ] {
            let error = parse_console_url(url, false).expect_err("this console base is refused");
            assert!(
                matches!(error, EnrollError::BadUrl(_)),
                "{url}: the refusal is a usage error: {error}"
            );
            assert_eq!(error.exit_code(), 2, "{url}: a bad URL is usage");
            assert!(
                error.to_string().contains("https"),
                "{url}: the refusal names the transport rule: {error}"
            );
        }
    }

    /// A console that answers `POST /devices` with a redirect to a *different* origin. A redirect is
    /// a status this mode reports, not a second host it follows with the one-time code.
    async fn redirect(State(target): State<String>) -> Redirect {
        Redirect::temporary(&target)
    }

    #[tokio::test]
    async fn a_redirect_to_another_origin_is_not_followed() {
        use tokio::io::AsyncWriteExt;

        // The other origin counts connections. It must see none.
        let other = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let other_address = other.local_addr().expect("an address");
        let hits = Arc::new(AtomicU64::new(0));
        let hits_in_task = Arc::clone(&hits);
        let other_task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = other.accept().await {
                hits_in_task.fetch_add(1, Ordering::SeqCst);
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n")
                    .await;
            }
        });

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let base_url = format!("http://{}", listener.local_addr().expect("an address"));
        let app = Router::new()
            .route("/devices", post(redirect))
            .with_state(format!("http://{other_address}/devices"));
        let console_task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let dir = TempDir::new("enroll-redirect");
        let data = dir.join("data");
        let invocation = EnrollInvocation {
            console_url: base_url,
            code: "9".repeat(64),
            data: data.clone(),
            force: false,
            allow_plaintext: false,
        };
        let error = enroll(&invocation)
            .await
            .expect_err("a redirect is a status to report, not a target to follow");
        assert!(
            matches!(error, EnrollError::Status { status: 307, .. }),
            "the redirect itself is the answer: {error}"
        );
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "the one-time code never reached the redirect's other origin"
        );
        assert!(!data.join("console-token.json").exists());
        assert!(!data.join("console.json").exists());

        console_task.abort();
        other_task.abort();
    }

    #[tokio::test]
    async fn an_absent_console_is_a_refusal_that_writes_nothing() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let port = listener.local_addr().expect("an address").port();
        drop(listener);

        let dir = TempDir::new("enroll-absent");
        let data = dir.join("data");
        let invocation = EnrollInvocation {
            console_url: format!("http://127.0.0.1:{port}"),
            code: "e".repeat(64),
            data: data.clone(),
            force: false,
            allow_plaintext: false,
        };

        let error = enroll(&invocation)
            .await
            .expect_err("nothing answers on the closed port");
        assert!(
            matches!(error, EnrollError::Console { .. }),
            "the refusal is a transport failure, not a registration answer: {error}"
        );
        assert_eq!(error.exit_code(), 1);
        assert!(!data.join("console-token.json").exists());
        assert!(!data.join("console.json").exists());
    }

    #[tokio::test]
    async fn the_mesh_fields_in_console_json_survive_the_merge() {
        let console = FakeConsole::start(None).await;
        let dir = TempDir::new("enroll-mesh");
        let data = dir.join("data");
        fs::create_dir_all(&data).expect("the data directory is creatable");
        let link_path = data.join("console.json");
        fs::write(
            &link_path,
            serde_json::to_vec(&json!({
                "url": "http://old.example.test",
                "token_file": "/old/console-token.json",
                "mesh_broker": "http://127.0.0.1:8080",
                "mesh_relay": "127.0.0.1:9000",
                "mesh_stun": "203.0.113.10:3478",
            }))
            .expect("the fixture link is JSON"),
        )
        .expect("the fixture link is writable");

        let invocation = EnrollInvocation {
            console_url: console.base_url.clone(),
            code: "f".repeat(64),
            data: data.clone(),
            force: false,
            allow_plaintext: false,
        };
        enroll(&invocation).await.expect("the registration enrols");

        let link: Value = serde_json::from_slice(
            &fs::read(&link_path).expect("the merged console link is on disk"),
        )
        .expect("the merged console link is JSON");
        assert_eq!(
            link["url"], console.base_url,
            "the console URL is replaced with this run's"
        );
        assert_eq!(
            link["token_file"].as_str(),
            Some(
                data.join("console-token.json")
                    .to_str()
                    .expect("a UTF-8 path")
            ),
            "the token path is this run's"
        );
        assert_eq!(
            link["mesh_broker"], "http://127.0.0.1:8080",
            "the mesh broker survives the console merge"
        );
        assert_eq!(
            link["mesh_relay"], "127.0.0.1:9000",
            "the mesh relay survives the console merge"
        );
        assert_eq!(
            link["mesh_stun"], "203.0.113.10:3478",
            "the mesh STUN reflector survives the console merge"
        );
    }

    #[test]
    fn the_data_directory_defaults_to_install_data() {
        let dir = TempDir::new("enroll-default-data");
        let install = dir.join("install");
        let args = vec![
            OsString::from("http://127.0.0.1:18082"),
            OsString::from("code"),
            OsString::from("--install"),
            install.clone().into_os_string(),
        ];

        let invocation = parse_enroll_args(&args).expect("install and the two positionals parse");
        assert_eq!(
            invocation.data,
            install.join("data"),
            "the launcher's default is <install>/data"
        );
    }

    #[test]
    fn a_missing_code_is_a_usage_error() {
        let dir = TempDir::new("enroll-missing-code");
        let data = dir.join("data");
        let args = vec![
            OsString::from("http://127.0.0.1:18082"),
            OsString::from("--data"),
            data.into_os_string(),
        ];

        let error = parse_enroll_args(&args).expect_err("a code is required");
        assert!(matches!(error, EnrollError::Usage));
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn plaintext_http_is_accepted_only_with_the_flag() {
        let strict = parse_console_url("http://console.example.test:19000", false)
            .expect_err("plain http is refused by default");
        assert!(matches!(strict, EnrollError::BadUrl(_)), "{strict:?}");
        let allowed = parse_console_url("http://console.example.test:19000", true)
            .expect("the operator can name a trusted tunnel");
        assert_eq!(allowed.scheme(), "http");
        assert_eq!(allowed.host_str(), Some("console.example.test"));
    }

    #[test]
    fn the_plaintext_flag_widens_the_scheme_and_nothing_else() {
        assert!(parse_console_url("http://user:pass@host:19000", true).is_err());
        assert!(parse_console_url("http://host:19000/?next=x", true).is_err());
        assert!(parse_console_url("http://host:19000/#frag", true).is_err());
        assert!(parse_console_url("ftp://host:19000", true).is_err());
        assert!(parse_console_url("host:19000", true).is_err());
    }
}
