//! SSH sessions (design §4.5): a remote shell with a pinned host key.
//!
//! The connection's trust is the host key, and it is pinned: the first connection to a
//! `host:port` records the key's SHA-256 fingerprint in `<data>/known_hosts.json` and proceeds
//! (trust on first use); every later connection must present the same key. A changed key is
//! **never** accepted automatically — it is a typed refusal naming the host and both fingerprints,
//! raised during the key exchange, before any credential is sent. A caller that already knows the
//! fingerprint can pass it as `host_key`; the pin is then compared instead of recorded, and a
//! mismatch is refused before authentication.
//!
//! The store is read and written under one lock, so two first-time connections cannot lose one
//! another's entry to a read-modify-write. A store that exists but cannot be parsed fails closed:
//! a half-written file must not silently become an empty trust table, because that would re-open
//! the window a changed key was refused to close.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use russh::client::Msg;
use russh::client::{self, Handle};
use russh::keys::ssh_key::PublicKey;
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg};
use russh::{ChannelMsg, ChannelReadHalf, ChannelWriteHalf};
use serde::Deserialize;

/// How long the whole connect, authentication and shell-open sequence may take.
///
/// russh's `inactivity_timeout` is armed after the handshake, so a peer that completes the
/// handshake and then stalls (a tarpit, an sshd stuck on reverse DNS) would otherwise hang the
/// tool forever. This bound covers every phase, not just the TCP connect.
const CONNECT_TIMEOUT_MS: u64 = 30_000;

/// How much one write hands to the channel before checking its deadline.
///
/// A single `data` of a megabyte would be one unbounded await on the channel's flow-control
/// window; chunks let the write notice its deadline and keep one call from asking the transport
/// to accept more than a terminal's queue can hold at once.
const WRITE_CHUNK_BYTES: usize = 1024;

/// How long an idle SSH connection is kept before russh reports it dead.
const INACTIVITY_TIMEOUT: Duration = Duration::from_secs(30);

/// How often a keepalive is sent; it must fire before [`INACTIVITY_TIMEOUT`], or a quiet session
/// would be declared dead between probes.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);

/// The SSH credentials a create may use.
///
/// The `Debug` implementation is redacted: a password and a private-key path are configuration a
/// log line must never carry, and this type is the one place they are held.
#[derive(PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Auth {
    /// Password authentication.
    Password {
        /// The password. Never rendered by `Debug`.
        password: String,
    },
    /// Public-key authentication.
    Key {
        /// An absolute path to an OpenSSH private key. Never rendered by `Debug`.
        path: PathBuf,
    },
}

impl fmt::Debug for Auth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Auth::Password { .. } => f.write_str("Auth::Password { password: <redacted> }"),
            Auth::Key { .. } => f.write_str("Auth::Key { path: <redacted> }"),
        }
    }
}

/// The host-key pinning store at `<data>/known_hosts.json`.
///
/// One store per device: the lock is what serializes the trust table's read-modify-write. The
/// handle is cheap to clone; every clone shares the one file and the one lock.
#[derive(Clone)]
pub struct KnownHosts {
    inner: Arc<KnownHostsInner>,
}

struct KnownHostsInner {
    path: PathBuf,
    lock: Mutex<()>,
}

impl KnownHosts {
    /// The store a device writes at `<data>/known_hosts.json`.
    pub fn at(data: &Path) -> KnownHosts {
        KnownHosts {
            inner: Arc::new(KnownHostsInner {
                path: data.join("known_hosts.json"),
                lock: Mutex::new(()),
            }),
        }
    }

    /// The file the pins are kept in.
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Check `fingerprint` against the pin for `host:port`, recording it on first use.
    ///
    /// `expected` is the caller's explicit fingerprint: when it is present the store is neither
    /// read nor written, because the caller already supplied the trust decision. A mismatch is
    /// [`SshError::HostKeyMismatch`] naming both fingerprints; it never rewrites the pin.
    fn verify(
        &self,
        host: &str,
        port: u16,
        fingerprint: &str,
        expected: Option<&str>,
    ) -> Result<(), SshError> {
        if let Some(expected) = expected {
            return compare(host, port, expected, fingerprint);
        }

        let _guard = self
            .inner
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut pins = self.load()?;
        let key = trust_key(host, port);
        match pins.get(&key) {
            Some(pinned) => compare(host, port, pinned, fingerprint),
            None => {
                pins.insert(key, fingerprint.to_owned());
                self.save(&pins)
            }
        }
    }

    /// Read the trust table; a missing file is a fresh table, a malformed one is a refusal.
    fn load(&self) -> Result<BTreeMap<String, String>, SshError> {
        match std::fs::read(self.path()) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| SshError::HostKeyStore {
                message: format!(
                    "{} is not a JSON host-key table; refusing to trust it",
                    self.path().display()
                ),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(error) => Err(SshError::HostKeyStore {
                message: format!(
                    "cannot read {}: {error}; refusing to connect",
                    self.path().display()
                ),
            }),
        }
    }

    /// Replace the trust table atomically: temp sibling, flush, rename.
    fn save(&self, pins: &BTreeMap<String, String>) -> Result<(), SshError> {
        let path = self.path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| SshError::HostKeyStore {
                message: format!(
                    "cannot create {} for the host-key table: {error}",
                    parent.display()
                ),
            })?;
        }
        let bytes = serde_json::to_vec(pins).map_err(|error| SshError::HostKeyStore {
            message: format!("cannot encode the host-key table: {error}"),
        })?;
        let (temporary, mut file) =
            crate::fsutil::create_new_sibling(path).map_err(|error| SshError::HostKeyStore {
                message: format!("cannot create a temporary host-key table: {error}"),
            })?;
        if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
            drop(file);
            let _ = std::fs::remove_file(&temporary);
            return Err(SshError::HostKeyStore {
                message: format!("cannot write the host-key table: {error}"),
            });
        }
        drop(file);
        if let Err(error) = std::fs::rename(&temporary, path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(SshError::HostKeyStore {
                message: format!("cannot replace the host-key table: {error}"),
            });
        }
        Ok(())
    }
}

/// The store key for a host:port. A bare IPv6 address is bracketed so the port is unambiguous.
/// Load an OpenSSH private key, tolerating the line endings a Windows editor writes.
///
/// The canary proved this matters: `ssh-keygen` on the same Windows machine the device runs on writes
/// CRLF, and `russh`'s own loader reads the file itself and refuses the trailing `\r` — so the key an
/// operator had just generated on that box was the one key the device would not accept. Normalising
/// the line endings before parsing is the whole fix; the file's bytes stay untouched on disk.
fn load_private_key(path: &Path) -> Result<PrivateKey, String> {
    let bytes = std::fs::read(path).map_err(|_| "the key file is not readable".to_owned())?;
    let text = String::from_utf8(bytes)
        .map_err(|_| "the key file is not UTF-8 text; an OpenSSH private key is".to_owned())?;
    let normalized = text.replace("\r\n", "\n");
    PrivateKey::from_openssh(&normalized)
        .map_err(|_| "the file is not a readable OpenSSH private key".to_owned())
}

fn trust_key(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// The typed refusal for a fingerprint that is not the pinned one.
fn compare(host: &str, port: u16, expected: &str, actual: &str) -> Result<(), SshError> {
    if expected == actual {
        Ok(())
    } else {
        Err(SshError::HostKeyMismatch {
            host: host.to_owned(),
            port,
            expected: expected.to_owned(),
            actual: actual.to_owned(),
        })
    }
}

/// Why an SSH operation was refused or failed.
#[derive(Debug)]
pub enum SshError {
    /// The target is not something this device will connect to.
    InvalidTarget {
        /// What is wrong, with no credential in it.
        message: String,
    },
    /// The private key could not be read. The message deliberately does not contain its path.
    Key {
        /// Why the key was refused.
        message: String,
    },
    /// The server's host key did not match the pinned or supplied fingerprint.
    HostKeyMismatch {
        /// The host the connection was for.
        host: String,
        /// The port the connection was for.
        port: u16,
        /// The fingerprint that was expected.
        expected: String,
        /// The fingerprint the server presented.
        actual: String,
    },
    /// The pinning store could not be read or written, so trust could not be established.
    HostKeyStore {
        /// What failed, with no credential in it.
        message: String,
    },
    /// The connection or its key exchange failed.
    Connect {
        /// The host the connection was for.
        host: String,
        /// The transport's report.
        reason: String,
    },
    /// The server rejected the credentials.
    Authentication {
        /// The host the connection was for.
        host: String,
        /// The server's answer.
        reason: String,
    },
    /// The shell channel could not be opened.
    Channel {
        /// The transport's report.
        message: String,
    },
    /// A write did not finish before its deadline.
    WriteTimeout,
    /// The channel could not carry an operation.
    Backend {
        /// The transport's report.
        message: String,
    },
}

impl fmt::Display for SshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SshError::InvalidTarget { message } => write!(f, "invalid SSH target: {message}"),
            SshError::Key { message } => write!(f, "the SSH private key was refused: {message}"),
            SshError::HostKeyMismatch {
                host,
                port,
                expected,
                actual,
            } => write!(
                f,
                "the host key for {host}:{port} does not match: expected {expected}, the server presented {actual}"
            ),
            SshError::HostKeyStore { message } => {
                write!(f, "the known-hosts store refused the connection: {message}")
            }
            SshError::Connect { host, reason } => {
                write!(f, "cannot connect to {host}: {reason}")
            }
            SshError::Authentication { host, reason } => {
                write!(f, "the server {host} rejected the credentials: {reason}")
            }
            SshError::Channel { message } => write!(f, "cannot open a shell channel: {message}"),
            SshError::WriteTimeout => f.write_str("the SSH write did not finish before its deadline"),
            SshError::Backend { message } => write!(f, "the SSH channel failed: {message}"),
        }
    }
}

impl std::error::Error for SshError {}

impl From<russh::Error> for SshError {
    fn from(error: russh::Error) -> Self {
        SshError::Backend {
            message: error.to_string(),
        }
    }
}

/// What the key exchange observed, so the session can report the fingerprint that was checked.
#[derive(Clone, Default)]
struct ObservedKey(Arc<Mutex<Option<String>>>);

impl ObservedKey {
    fn set(&self, fingerprint: &str) {
        *self.0.lock().unwrap_or_else(|poison| poison.into_inner()) = Some(fingerprint.to_owned());
    }

    fn take(&self) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
    }
}

/// The client half of the host-key check: every key the server presents is verified before the
/// connection can continue.
struct ClientHandler {
    host: String,
    port: u16,
    /// The caller's explicit fingerprint; `None` means the store decides.
    expected: Option<String>,
    known_hosts: KnownHosts,
    observed: ObservedKey,
}

impl client::Handler for ClientHandler {
    type Error = SshError;

    async fn check_server_key(&mut self, key: &PublicKey) -> Result<bool, Self::Error> {
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
        self.observed.set(&fingerprint);
        self.known_hosts.verify(
            &self.host,
            self.port,
            &fingerprint,
            self.expected.as_deref(),
        )?;
        Ok(true)
    }
}

/// A remote shell, with the host key that was checked for it.
pub struct SshSession {
    host: String,
    port: u16,
    user: String,
    /// The output half, taken once by the registry's reader task.
    reader: Option<ChannelReadHalf>,
    writer: ChannelWriteHalf<Msg>,
    /// Keeps the connection's event loop alive; dropping it ends the transport.
    _handle: Handle<ClientHandler>,
}

impl fmt::Debug for SshSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SshSession")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .finish_non_exhaustive()
    }
}

impl SshSession {
    /// The connected host's name or address.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The connected port. This is the port the server bound, not the caller's `0`.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The user the shell runs as.
    pub fn user(&self) -> &str {
        &self.user
    }

    /// `user@host:port`, the session label `terminal.list` reports.
    pub fn label(&self) -> String {
        format!("{}@{}:{}", self.user(), self.host(), self.port())
    }

    /// Take the output half. It can be taken once.
    pub fn take_reader(&mut self) -> Option<SshReader> {
        self.reader.take().map(|read| SshReader { read })
    }

    /// Write bytes to the remote shell, giving up at `deadline`.
    ///
    /// The chunks keep the call's deadline meaningful under flow control: one `data` of a whole
    /// payload would only return once every byte entered the channel's window.
    pub async fn write(&self, bytes: &[u8], deadline: Instant) -> Result<usize, SshError> {
        let mut offset = 0;
        while offset < bytes.len() {
            if Instant::now() >= deadline {
                return Err(SshError::WriteTimeout);
            }
            let end = (offset + WRITE_CHUNK_BYTES).min(bytes.len());
            self.writer
                .data(&bytes[offset..end])
                .await
                .map_err(SshError::from)?;
            offset = end;
        }
        Ok(offset)
    }

    /// Resize the remote pseudo-terminal.
    pub async fn resize(&self, rows: u16, cols: u16) -> Result<(), SshError> {
        self.writer
            .window_change(cols as u32, rows as u32, 0, 0)
            .await
            .map_err(SshError::from)
    }

    /// Ask the server to close the channel.
    pub async fn close(&self) -> Result<(), SshError> {
        self.writer.close().await.map_err(SshError::from)
    }
}

/// The output half of a session: one chunk at a time, `None` when the channel ends.
pub struct SshReader {
    read: ChannelReadHalf,
}

impl SshReader {
    /// The next chunk of output, or `None` once the remote closed the channel.
    pub async fn next_chunk(&mut self) -> Option<Vec<u8>> {
        loop {
            match self.read.wait().await {
                Some(ChannelMsg::Data { data }) => return Some(Vec::from(&*data)),
                Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => return None,
                Some(_) => {}
            }
        }
    }
}

/// Open an SSH connection and an interactive shell on it.
///
/// `host_key` is the caller's explicit SHA-256 fingerprint; when it is given, the pinning store is
/// not consulted or updated. Otherwise `known_hosts` records the key on first use and insists on
/// it thereafter. `rows`/`cols` size the remote pseudo-terminal.
#[allow(clippy::too_many_arguments)] // every one is part of the wire contract
pub async fn connect(
    host: &str,
    port: u16,
    user: &str,
    auth: &Auth,
    rows: u16,
    cols: u16,
    host_key: Option<&str>,
    known_hosts: &KnownHosts,
) -> Result<SshSession, SshError> {
    if host.is_empty() {
        return Err(SshError::InvalidTarget {
            message: "the host is empty".to_owned(),
        });
    }
    if user.is_empty() {
        return Err(SshError::InvalidTarget {
            message: "the user is empty".to_owned(),
        });
    }
    if port == 0 {
        return Err(SshError::InvalidTarget {
            message: "the port is 0; an SSH server does not listen there".to_owned(),
        });
    }

    // Load the key before a socket exists: an unreadable path is a configuration error, not a
    // connection attempt. The error deliberately has no path in it.
    let private_key = match auth {
        Auth::Password { .. } => None,
        Auth::Key { path } => {
            if !path.is_absolute() {
                return Err(SshError::Key {
                    message: "the key path is not absolute".to_owned(),
                });
            }
            let key = load_private_key(path).map_err(|message| SshError::Key { message })?;
            Some(PrivateKeyWithHashAlg::new(Arc::new(key), None))
        }
    };

    let config = Arc::new(client::Config {
        inactivity_timeout: Some(INACTIVITY_TIMEOUT),
        keepalive_interval: Some(KEEPALIVE_INTERVAL),
        ..Default::default()
    });
    let observed = ObservedKey::default();
    let handler = ClientHandler {
        host: host.to_owned(),
        port,
        expected: host_key.map(str::to_owned),
        known_hosts: known_hosts.clone(),
        observed: observed.clone(),
    };

    let mut handle: Handle<ClientHandler> = tokio::time::timeout(
        Duration::from_millis(CONNECT_TIMEOUT_MS),
        client::connect(config, (host, port), handler),
    )
    .await
    .map_err(|_| SshError::Connect {
        host: host.to_owned(),
        reason: format!("the connection did not complete within {CONNECT_TIMEOUT_MS} ms"),
    })?
    .map_err(|error| match error {
        // The key check's own refusals are the answer; wrapping them in `Connect` would hide the
        // fingerprints the caller needs to act on.
        SshError::HostKeyMismatch { .. } | SshError::HostKeyStore { .. } => error,
        other => SshError::Connect {
            host: host.to_owned(),
            reason: other.to_string(),
        },
    })?;

    match auth {
        Auth::Password { password } => {
            let answer = handle
                .authenticate_password(user, password)
                .await
                .map_err(|error| SshError::Authentication {
                    host: host.to_owned(),
                    reason: error.to_string(),
                })?;
            if !answer.success() {
                return Err(SshError::Authentication {
                    host: host.to_owned(),
                    reason: "the password was rejected".to_owned(),
                });
            }
        }
        Auth::Key { .. } => {
            let key = private_key.expect("the key was loaded before the connection");
            let answer = handle
                .authenticate_publickey(user, key)
                .await
                .map_err(|error| SshError::Authentication {
                    host: host.to_owned(),
                    reason: error.to_string(),
                })?;
            if !answer.success() {
                return Err(SshError::Authentication {
                    host: host.to_owned(),
                    reason: "the public key was rejected".to_owned(),
                });
            }
        }
    }

    let channel = handle
        .channel_open_session()
        .await
        .map_err(|error| SshError::Channel {
            message: error.to_string(),
        })?;
    channel
        .request_pty(
            true,
            "xterm-256color",
            u32::from(cols),
            u32::from(rows),
            0,
            0,
            &[],
        )
        .await
        .map_err(|error| SshError::Channel {
            message: error.to_string(),
        })?;
    channel
        .request_shell(true)
        .await
        .map_err(|error| SshError::Channel {
            message: error.to_string(),
        })?;

    // A completed handshake must have presented a key; without one the key check never ran, and
    // the connection is not something this module can vouch for.
    observed.take().ok_or_else(|| SshError::Channel {
        message: "the server presented no host key".to_owned(),
    })?;
    let (reader, writer) = channel.split();

    Ok(SshSession {
        host: host.to_owned(),
        port,
        user: user.to_owned(),
        reader: Some(reader),
        writer,
        _handle: handle,
    })
}

#[cfg(test)]
pub(crate) mod test_support {
    //! An in-process russh server the crate's tests can talk to.
    //!
    //! It is a real SSH server — key exchange, password or public-key auth, a PTY shell channel that
    //! echoes — not a mock of the client's transport: the code under test is the same code that
    //! talks to a device. The listener binds port 0 so a test never guesses a free port, and
    //! `SO_REUSEADDR` lets the host-key-change test restart a server on the same port.

    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use russh::keys::ssh_key::{rand_core::OsRng, HashAlg};
    use russh::keys::{Algorithm, PrivateKey};
    use russh::server::{Auth as ServerAuth, Msg, Server, Session as ServerSession};
    use russh::{Channel, ChannelId};

    pub(crate) fn host_key() -> PrivateKey {
        PrivateKey::random(&mut OsRng, Algorithm::Ed25519).expect("a host key is generated")
    }

    pub(crate) fn fingerprint_of(key: &PrivateKey) -> String {
        key.public_key().fingerprint(HashAlg::Sha256).to_string()
    }

    /// One server connection: password auth and an echoing shell.
    struct EchoHandler {
        password: String,
        accept_public_keys: bool,
        auth_attempts: Arc<AtomicUsize>,
    }

    impl russh::server::Handler for EchoHandler {
        type Error = russh::Error;

        async fn auth_password(
            &mut self,
            _user: &str,
            password: &str,
        ) -> Result<ServerAuth, Self::Error> {
            self.auth_attempts.fetch_add(1, Ordering::SeqCst);
            if password == self.password {
                Ok(ServerAuth::Accept)
            } else {
                Ok(ServerAuth::reject())
            }
        }

        async fn auth_publickey(
            &mut self,
            _user: &str,
            _public_key: &russh::keys::ssh_key::PublicKey,
        ) -> Result<ServerAuth, Self::Error> {
            self.auth_attempts.fetch_add(1, Ordering::SeqCst);
            if self.accept_public_keys {
                Ok(ServerAuth::Accept)
            } else {
                Ok(ServerAuth::reject())
            }
        }

        async fn channel_open_session(
            &mut self,
            _channel: Channel<Msg>,
            _session: &mut ServerSession,
        ) -> Result<bool, Self::Error> {
            Ok(true)
        }

        async fn pty_request(
            &mut self,
            channel: ChannelId,
            _term: &str,
            _col_width: u32,
            _row_height: u32,
            _pix_width: u32,
            _pix_height: u32,
            _modes: &[(russh::Pty, u32)],
            session: &mut ServerSession,
        ) -> Result<(), Self::Error> {
            session.channel_success(channel)?;
            Ok(())
        }

        async fn shell_request(
            &mut self,
            channel: ChannelId,
            session: &mut ServerSession,
        ) -> Result<(), Self::Error> {
            session.channel_success(channel)?;
            Ok(())
        }

        async fn data(
            &mut self,
            channel: ChannelId,
            data: &[u8],
            session: &mut ServerSession,
        ) -> Result<(), Self::Error> {
            // A shell channel that echoes: what the client reads is what it wrote.
            session.data(channel, russh::CryptoVec::from_slice(data))?;
            Ok(())
        }
    }

    struct EchoServer {
        password: String,
        accept_public_keys: bool,
        auth_attempts: Arc<AtomicUsize>,
    }

    impl Server for EchoServer {
        type Handler = EchoHandler;

        fn new_client(&mut self, _peer: Option<SocketAddr>) -> EchoHandler {
            EchoHandler {
                password: self.password.clone(),
                accept_public_keys: self.accept_public_keys,
                auth_attempts: Arc::clone(&self.auth_attempts),
            }
        }
    }

    /// An in-process SSH server on a loopback port.
    pub(crate) struct TestServer {
        addr: SocketAddr,
        fingerprint: String,
        auth_attempts: Arc<AtomicUsize>,
        task: tokio::task::JoinHandle<()>,
    }

    impl TestServer {
        pub(crate) async fn start(key: PrivateKey, password: &str, port: u16) -> TestServer {
            Self::start_with(key, password, port, false).await
        }

        pub(crate) async fn start_with(
            key: PrivateKey,
            password: &str,
            port: u16,
            accept_public_keys: bool,
        ) -> TestServer {
            let fingerprint = fingerprint_of(&key);
            // `SO_REUSEADDR` is what lets the restarted-server test bind the same port again while
            // the first server's sockets are still in the kernel's bookkeeping.
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let listener = loop {
                let socket = tokio::net::TcpSocket::new_v4().expect("a TCP socket");
                socket.set_reuseaddr(true).expect("reuseaddr is settable");
                match socket.bind(SocketAddr::from(([127, 0, 0, 1], port))) {
                    Ok(()) => break socket.listen(128).expect("the listener listens"),
                    Err(error)
                        if error.kind() == std::io::ErrorKind::AddrInUse
                            && std::time::Instant::now() < deadline =>
                    {
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                    Err(error) => panic!("cannot bind loopback port {port}: {error}"),
                }
            };
            let addr = listener.local_addr().expect("the listener has an address");
            let auth_attempts = Arc::new(AtomicUsize::new(0));
            let config = Arc::new(russh::server::Config {
                inactivity_timeout: Some(Duration::from_secs(30)),
                keys: vec![key],
                ..Default::default()
            });
            let mut server = EchoServer {
                password: password.to_owned(),
                accept_public_keys,
                auth_attempts: Arc::clone(&auth_attempts),
            };
            let task = tokio::spawn(async move {
                let running = server.run_on_socket(config, &listener);
                let _ = running.await;
            });
            TestServer {
                addr,
                fingerprint,
                auth_attempts,
                task,
            }
        }

        pub(crate) fn port(&self) -> u16 {
            self.addr.port()
        }

        /// The host key the server presents, for a test that supplies it explicitly.
        pub(crate) fn fingerprint(&self) -> &str {
            &self.fingerprint
        }

        /// A password server on a fresh loopback port, with a freshly generated host key.
        pub(crate) async fn start_password(password: &str) -> TestServer {
            TestServer::start(host_key(), password, 0).await
        }

        pub(crate) fn auth_attempts(&self) -> usize {
            self.auth_attempts.load(Ordering::SeqCst)
        }

        pub(crate) async fn stop(self) {
            self.task.abort();
            let _ = self.task.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use russh::keys::ssh_key::LineEnding;

    use crate::test_support::TempDir;

    use super::test_support::{fingerprint_of, host_key, TestServer};
    use super::*;

    /// How long a test waits for the echoed marker.
    const MARKER_TIMEOUT: Duration = Duration::from_secs(10);

    fn password(text: &str) -> Auth {
        Auth::Password {
            password: text.to_owned(),
        }
    }

    async fn read_until(reader: &mut SshReader, marker: &str) -> String {
        let deadline = std::time::Instant::now() + MARKER_TIMEOUT;
        let mut text = String::new();
        while std::time::Instant::now() < deadline {
            let Some(chunk) = reader.next_chunk().await else {
                break;
            };
            text.push_str(&String::from_utf8_lossy(&chunk));
            if text.contains(marker) {
                return text;
            }
        }
        panic!("{marker:?} did not arrive within {MARKER_TIMEOUT:?}; output so far: {text:?}");
    }

    fn store(label: &str) -> (TempDir, KnownHosts) {
        let dir = TempDir::new(label);
        let data = dir.join("data");
        std::fs::create_dir_all(&data).expect("the data directory is creatable");
        let known = KnownHosts::at(&data);
        (dir, known)
    }

    #[tokio::test]
    async fn a_first_connection_pins_the_key_and_the_next_one_uses_the_pin() {
        let (_dir, known) = store("ssh-tofu");
        let server = TestServer::start(host_key(), "correct horse", 0).await;
        let port = server.port();
        assert!(server.fingerprint().starts_with("SHA256:"));

        let mut session = connect(
            "127.0.0.1",
            port,
            "tester",
            &password("correct horse"),
            24,
            80,
            None,
            &known,
        )
        .await
        .expect("the first connection is trusted on first use");
        assert_eq!(session.port(), port, "the session reports the bound port");
        assert_eq!(session.host(), "127.0.0.1");
        assert_eq!(session.user(), "tester");
        assert_eq!(session.label(), format!("tester@127.0.0.1:{port}"));

        let mut reader = session.take_reader().expect("a session has one reader");
        let command = b"echo SUMMRISE-SSH-MARK\n";
        let written = session
            .write(command, std::time::Instant::now() + Duration::from_secs(10))
            .await
            .expect("the write reaches the shell");
        assert_eq!(written, command.len());
        let text = read_until(&mut reader, "SUMMRISE-SSH-MARK").await;
        assert!(
            text.contains("SUMMRISE-SSH-MARK"),
            "the echoed command is the session's output: {text:?}"
        );

        let saved = std::fs::read_to_string(known.path()).expect("first use records the key");
        assert!(
            saved.contains(server.fingerprint()),
            "the store pins the fingerprint the session checked: {saved}"
        );
        session.close().await.expect("the channel closes");

        // The second connection to the same host:port verifies against the stored pin.
        connect(
            "127.0.0.1",
            port,
            "tester",
            &password("correct horse"),
            24,
            80,
            None,
            &known,
        )
        .await
        .expect("the pinned key is accepted");
    }

    #[tokio::test]
    async fn a_wrong_password_is_a_typed_authentication_error() {
        let (_dir, known) = store("ssh-wrong-password");
        let server = TestServer::start(host_key(), "right", 0).await;

        let error = connect(
            "127.0.0.1",
            server.port(),
            "tester",
            &password("wrong"),
            24,
            80,
            Some(server.fingerprint()),
            &known,
        )
        .await
        .expect_err("the server rejects the password");

        assert!(
            matches!(error, SshError::Authentication { .. }),
            "a rejected password is a typed authentication error, not {error:?}"
        );
        assert!(
            server.auth_attempts() >= 1,
            "the server saw the rejected attempt"
        );
        assert!(
            !known.path().exists(),
            "an explicit fingerprint needs no trust-on-first-use entry"
        );
    }

    #[tokio::test]
    async fn a_restarted_server_with_a_new_key_is_refused_and_both_fingerprints_are_named() {
        let (_dir, known) = store("ssh-changed-key");
        let first_key = host_key();
        let first_fingerprint = fingerprint_of(&first_key);
        let first = TestServer::start(first_key, "pw", 0).await;
        let port = first.port();

        let session = connect(
            "127.0.0.1",
            port,
            "tester",
            &password("pw"),
            24,
            80,
            None,
            &known,
        )
        .await
        .expect("the first key is trusted");
        session.close().await.expect("the channel closes");
        drop(session);
        first.stop().await;
        // Let the server's connection task observe the dropped client before the port is reused.
        tokio::time::sleep(Duration::from_millis(50)).await;

        let second_key = host_key();
        let second_fingerprint = fingerprint_of(&second_key);
        let second = TestServer::start(second_key, "pw", port).await;

        let error = connect(
            "127.0.0.1",
            port,
            "tester",
            &password("pw"),
            24,
            80,
            None,
            &known,
        )
        .await
        .expect_err("a changed host key is never accepted automatically");

        match &error {
            SshError::HostKeyMismatch {
                host,
                port: mismatch_port,
                expected,
                actual,
            } => {
                assert_eq!(host, "127.0.0.1");
                assert_eq!(*mismatch_port, port);
                assert_eq!(expected, &first_fingerprint, "the pinned key is named");
                assert_eq!(actual, &second_fingerprint, "the presented key is named");
            }
            other => panic!("a changed host key is a typed mismatch, not {other:?}"),
        }
        let text = error.to_string();
        assert!(
            text.contains(&first_fingerprint)
                && text.contains(&second_fingerprint)
                && text.contains(&format!("127.0.0.1:{port}")),
            "the refusal names the host and both fingerprints: {text}"
        );
        assert_eq!(
            second.auth_attempts(),
            0,
            "the key is checked during the exchange, so no credential was sent"
        );

        let saved = std::fs::read_to_string(known.path()).expect("the store still reads");
        assert!(
            saved.contains(&first_fingerprint) && !saved.contains(&second_fingerprint),
            "a refused key does not replace the pin: {saved}"
        );
    }

    #[tokio::test]
    async fn an_explicit_host_key_is_checked_before_authentication_and_skips_tofu() {
        let (_dir, known) = store("ssh-explicit-key");
        let server = TestServer::start(host_key(), "pw", 0).await;

        let session = connect(
            "127.0.0.1",
            server.port(),
            "tester",
            &password("pw"),
            24,
            80,
            Some(server.fingerprint()),
            &known,
        )
        .await
        .expect("the supplied fingerprint matches");
        drop(session);
        assert!(
            !known.path().exists(),
            "an explicit fingerprint writes no trust-on-first-use entry"
        );

        let attempts_before = server.auth_attempts();
        let error = connect(
            "127.0.0.1",
            server.port(),
            "tester",
            &password("pw"),
            24,
            80,
            Some("SHA256:not-the-server-key"),
            &known,
        )
        .await
        .expect_err("a supplied fingerprint that does not match is refused");

        match &error {
            SshError::HostKeyMismatch {
                expected, actual, ..
            } => {
                assert_eq!(expected, "SHA256:not-the-server-key");
                assert_eq!(actual, &server.fingerprint());
            }
            other => panic!("a wrong explicit fingerprint is a mismatch, not {other:?}"),
        }
        assert_eq!(
            server.auth_attempts(),
            attempts_before,
            "a fingerprint mismatch is refused before the password is sent"
        );
    }

    #[tokio::test]
    async fn a_private_key_authenticates_and_an_unreadable_key_is_refused_without_the_path() {
        let (dir, known) = store("ssh-key-auth");
        let client_key = host_key();
        let key_path = dir.join("id_ed25519");
        client_key
            .write_openssh_file(&key_path, LineEnding::LF)
            .expect("the test key is written");

        let server = TestServer::start_with(host_key(), "unused", 0, true).await;
        let session = connect(
            "127.0.0.1",
            server.port(),
            "tester",
            &Auth::Key {
                path: key_path.clone(),
            },
            24,
            80,
            Some(server.fingerprint()),
            &known,
        )
        .await
        .expect("public-key authentication succeeds");
        assert_eq!(session.user(), "tester");

        let missing = dir.join("not-a-key");
        let error = connect(
            "127.0.0.1",
            // Nothing listens here; a key that is loaded before the TCP connect reports `Key`,
            // which is the order this test exists to pin.
            9,
            "tester",
            &Auth::Key {
                path: missing.clone(),
            },
            24,
            80,
            None,
            &known,
        )
        .await
        .expect_err("an unreadable key is refused");
        assert!(
            matches!(error, SshError::Key { .. }),
            "an unreadable key is a typed key error, not {error:?}"
        );
        let text = error.to_string();
        assert!(
            !text.contains(&missing.to_string_lossy().to_string()),
            "the refusal must not log the key's path: {text}"
        );

        let relative = connect(
            "127.0.0.1",
            9,
            "tester",
            &Auth::Key {
                path: std::path::PathBuf::from("relative/id_ed25519"),
            },
            24,
            80,
            None,
            &known,
        )
        .await
        .expect_err("a relative key path is refused");
        assert!(matches!(relative, SshError::Key { .. }), "{relative:?}");
        assert!(
            !relative.to_string().contains("relative/id_ed25519"),
            "the refusal must not log the key's path: {relative}"
        );
    }

    #[tokio::test]
    async fn an_ssh_session_round_trips_through_the_registry() {
        use std::sync::Arc;

        use summrise_core::SessionKind;

        use crate::capabilities::terminal::{
            CreateTarget, Kind, Limits, PtyError, PtyRegistry, SshTarget,
        };
        use crate::config::Release;
        use crate::store::Store;

        let dir = TempDir::new("ssh-registry");
        let data = dir.join("data");
        std::fs::create_dir_all(&data).expect("the data directory is creatable");
        let store = Store::new(dir.join("install"), &data, Release::standalone());
        store
            .save(&store.initial())
            .expect("the fixture state saves");
        let registry = PtyRegistry::with_clock(
            store.clone(),
            Arc::new(tokio::sync::Mutex::new(())),
            Arc::new(|| 1_000),
            Limits::default(),
        );

        let server = TestServer::start(host_key(), "pw", 0).await;
        let auth = password("pw");
        let id = registry
            .create(
                CreateTarget::Ssh(SshTarget {
                    host: "127.0.0.1",
                    port: server.port(),
                    user: "tester",
                    auth: &auth,
                    host_key: None,
                }),
                24,
                80,
            )
            .await
            .expect("the ssh session starts");

        let info = &registry.list()[0];
        assert_eq!(info.id, id);
        assert_eq!(info.kind, Kind::Ssh, "the list names the session's kind");
        assert_eq!(
            info.shell,
            format!("tester@127.0.0.1:{}", server.port()),
            "the list names the remote target"
        );
        assert_eq!((info.rows, info.cols), (24, 80));
        assert_eq!(info.pid, None, "a remote shell has no local pid");

        let rows = store.load().expect("the state document loads").sessions;
        let row = rows
            .iter()
            .find(|row| row.id == id.as_str())
            .unwrap_or_else(|| panic!("the live session is in the state document: {rows:?}"));
        assert_eq!(row.kind, SessionKind::Ssh);
        assert_eq!(row.state, summrise_core::SessionState::Attached);

        registry
            .write(&id, b"echo REGISTRY-MARK\n")
            .await
            .expect("the command is written");
        let deadline = std::time::Instant::now() + MARKER_TIMEOUT;
        let mut cursor = 0;
        let mut text = String::new();
        while std::time::Instant::now() < deadline {
            let (bytes, next) = registry.read(&id, cursor).expect("a live session reads");
            text.push_str(&String::from_utf8_lossy(&bytes));
            cursor = next;
            if text.contains("REGISTRY-MARK") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            text.contains("REGISTRY-MARK"),
            "the remote output arrives through the shared ring: {text:?}"
        );

        registry
            .resize(&id, 40, 120)
            .await
            .expect("a live session resizes");
        assert_eq!(
            (registry.list()[0].rows, registry.list()[0].cols),
            (40, 120)
        );

        registry.close(&id).await.expect("the session closes");
        assert!(registry.list().is_empty(), "a closed session is not listed");
        assert!(
            store
                .load()
                .expect("the state document loads")
                .sessions
                .iter()
                .all(|row| row.id != id.as_str()),
            "a closed session's state row is gone"
        );
        assert!(matches!(
            registry.read(&id, 0),
            Err(PtyError::UnknownSession { .. })
        ));
    }

    #[tokio::test]
    async fn a_corrupt_pin_store_fails_closed() {
        let (_dir, known) = store("ssh-corrupt-store");
        std::fs::write(known.path(), b"this is not JSON").expect("the corrupt store is written");
        let server = TestServer::start(host_key(), "pw", 0).await;

        let error = connect(
            "127.0.0.1",
            server.port(),
            "tester",
            &password("pw"),
            24,
            80,
            None,
            &known,
        )
        .await
        .expect_err("a store that cannot be parsed must not become an empty trust table");

        assert!(
            matches!(error, SshError::HostKeyStore { .. }),
            "a corrupt store is a typed store error, not {error:?}"
        );
        assert_eq!(
            std::fs::read_to_string(known.path()).expect("the corrupt store still reads"),
            "this is not JSON",
            "a failed connection does not rewrite the store"
        );
    }

    /// A key written with CRLF — which is what `ssh-keygen` on Windows produces — is accepted. The
    /// canary refused exactly such a key with "not a readable OpenSSH private key".
    #[test]
    fn a_key_written_with_windows_line_endings_is_accepted() {
        let dir = std::env::temp_dir().join(format!("summrise-ssh-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("key");
        let fixture = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\nQyNTUxOQAAACBt73PrmwidvDpkFRqERBt32N10DpRHCdVrn+3JSnTDQgAAAJANTEDWDUxA\n1gAAAAtzc2gtZWQyNTUxOQAAACBt73PrmwidvDpkFRqERBt32N10DpRHCdVrn+3JSnTDQg\nAAAEDgQZV+5PjjydkZGmJv72AZxNt0ZfdW3Yt8XrTZFWwJqG3vc+ubCJ28OmQVGoREG3fY\n3XQOlEcJ1Wuf7clKdMNCAAAAB2ZpeHR1cmUBAgMEBQY=\n-----END OPENSSH PRIVATE KEY-----\n";
        let crlf = fixture.replace("\n", "\r\n");
        std::fs::write(&path, crlf.as_bytes()).expect("write the CRLF key");
        let loaded = load_private_key(&path).expect("a CRLF OpenSSH key loads");
        assert_eq!(loaded.algorithm(), russh::keys::Algorithm::Ed25519);
        std::fs::remove_dir_all(&dir).ok();
    }
}
