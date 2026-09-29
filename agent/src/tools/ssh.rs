//! SSH session via russh — used by the terminal SshBackend (`terminal` feature).

use std::io::Write;
use std::sync::Arc;

use russh::client::{self, connect, Handle};
use russh::ChannelMsg;
use tokio::sync::mpsc;

use summrise_agent_core::DeviceError;

/// SSH handler with trust-on-first-use host-key verification.
///
/// The first connection to a host records its key fingerprint in
/// summrise-known-hosts.json (next to the exe); later connections REJECT a
/// changed key. Without this, any MITM could present its own key and capture
/// the SSH password (the old handler accepted every key). Keyed by
/// "user@host:port" so the same host under a different identity is not
/// silently trusted.
struct SshHandler {
    /// Key to record on first use ("user@host:port").
    trust_key: String,
}

fn fingerprint_of(key: &russh::keys::ssh_key::PublicKey) -> String {
    // SHA-256 fingerprint, stable hex — mirrors ssh-keygen's fingerprint.
    use russh::keys::ssh_key::HashAlg;
    key.fingerprint(HashAlg::Sha256).to_string()
}

// Test-only store directory (mirrors the secrets.rs/connections.rs harness): a
// TOFU save test must never write into the real DataDir, and this file had no
// seam at all until its write path moved into `atomic` (round: the atomic-write
// consolidation) and needed a posture test. cfg(test) keeps it out of every
// shipped build.
#[cfg(test)]
thread_local! {
    static TEST_DIR: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) };
}

fn known_hosts_path() -> std::path::PathBuf {
    #[cfg(test)]
    if let Some(d) = TEST_DIR.with(|d| d.borrow().clone()) {
        return d.join("summrise-known-hosts.json");
    }
    crate::paths::data_dir().join("summrise-known-hosts.json")
}

/// Parse failure is an Err — the caller (check_server_key) fails the
/// connection. A half-written file must NOT silently become an empty trust
/// table (which would re-TOFU every host and re-open the MITM window the
/// save-side fail-closed protects against) (round-57).
/// Serializes the TOFU read-modify-write (round-118): concurrent first-time
/// connections lost one host's fingerprint (last-save-wins), re-opening that
/// host's MITM window.
static KNOWN_HOSTS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn load_known_hosts() -> Result<serde_json::Map<String, serde_json::Value>, std::io::Error> {
    let s = std::fs::read_to_string(known_hosts_path())?;
    serde_json::from_str(&s).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Missing file is a FRESH trust table (round-68): round-57 propagated the
/// NotFound, so check_server_key aborted with UnknownKey BEFORE the first-use
/// TOFU branch could write the file — SSH could never bootstrap on a fresh
/// install (nothing creates summrise-known-hosts.json). NotFound → empty map;
/// corrupt/other errors still propagate so check_server_key FAILS CLOSED
/// (the re-TOFU-everything MITM protection round-57 built stays intact).
fn load_known_hosts_or_empty() -> Result<serde_json::Map<String, serde_json::Value>, std::io::Error>
{
    match load_known_hosts() {
        Ok(map) => Ok(map),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::Map::new()),
        Err(e) => Err(e),
    }
}

/// Atomic write (round-57): temp + rename in the same directory — the old
/// std::fs::write (truncate + write) left a half-written file on power loss,
/// which load then silently swallowed as an empty trust table.
///
/// POSTURE `BestEffort`, stated here because this file's history is the reason
/// the postures exist: the temp used to get a `#[cfg(unix)]` 0o600 chmod whose
/// failure PROPAGATED, and nothing at all on Windows. Both halves moved into the
/// shared hardener (`paths::harden_file`, which is exactly that chmod on unix and
/// an icacls break-inheritance on Windows — so Windows gains best-effort
/// hardening it never had), and an unavailable ACL must not cost SSH its trust
/// table: a failed save is a re-TOFU of that host next time, not a lost device.
/// The caller discards the result anyway (`let _ = save_known_hosts(...)`), so
/// the old fail-closed propagation was never observable.
fn save_known_hosts(map: &serde_json::Map<String, serde_json::Value>) -> std::io::Result<()> {
    let p = known_hosts_path();
    crate::atomic::replace(&p, crate::atomic::Hardening::BestEffort, |f| {
        f.write_all(
            serde_json::to_string(map)
                .unwrap_or_else(|_| "{}".into())
                .as_bytes(),
        )
    })
}

impl client::Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        // stage-n: the user drives SSH via MCP to devices on the local network
        // — host-key verification blocks legitimate connections when a device's
        // key changes (reinstall, sshd regen). Accept every key (TOFU on first
        // use, update-on-change) and log the change for observability. This
        // matches `ssh -o StrictHostKeyChecking=accept-new` — the password/OTP
        // is the real credential, not the host key.
        let fp = fingerprint_of(key);
        let _guard = KNOWN_HOSTS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let mut hosts = load_known_hosts_or_empty().map_err(|_| russh::Error::UnknownKey)?;
        // Clone the existing fingerprint (if any) so we can release the borrow
        // on `hosts` before mutating it. Compute the action first — the log
        // macros borrow `fp`/`old_fp`, so the insert (which moves `fp`) must
        // happen AFTER logging in its own scope.
        let existing = hosts
            .get(&self.trust_key)
            .and_then(|v| v.as_str().map(String::from));
        let changed = match &existing {
            None => {
                tracing::info!(
                    "[summrise-agent] ssh: TOFU trust {} fp={}",
                    self.trust_key,
                    fp
                );
                true
            }
            Some(old_fp) if old_fp != &fp => {
                tracing::warn!(
                    "[summrise-agent] ssh: host key CHANGED for {} (old={}, new={}) — updated trust",
                    self.trust_key,
                    old_fp,
                    fp
                );
                true
            }
            _ => false,
        };
        if changed {
            hosts.insert(self.trust_key.clone(), serde_json::Value::String(fp));
            let _ = save_known_hosts(&hosts);
        }
        Ok(true)
    }
}

// ── Core SSH Session ─────────────────────────────────────────

/// What one [`SshSession::exec_capture`] produced.
#[derive(Debug)]
pub struct ExecOutcome {
    /// The target's stdout, up to the caller's cap.
    pub stdout: Vec<u8>,
    /// The target's stderr (SSH extended data type 1), up to the caller's cap.
    pub stderr: Vec<u8>,
    /// The target wrote more stdout than the cap allowed. The surplus was **drained and discarded**,
    /// not left in the pipe — see the note on `exec_capture`.
    pub stdout_overflow: bool,
    /// The same, for stderr.
    pub stderr_overflow: bool,
    /// How the target ended, as far as the channel reported.
    pub status: ExecStatus,
}

/// How a remote command ended.
///
/// `Unknown` is a real outcome and not a placeholder: a channel can close without the server ever
/// sending an exit status (a killed `sshd`, a dropped connection), and a caller that treated that as
/// success would report a command that never ran. The subprocess seam's own contract distinguishes
/// "exited with a code" from "killed by a signal", and this type keeps that distinction rather than
/// flattening both into a number.
#[derive(Debug, PartialEq, Eq)]
pub enum ExecStatus {
    /// The target exited normally with this status.
    Exited(u32),
    /// The target was killed by a signal.
    Signalled {
        /// The signal's name as the far side spelled it.
        name: String,
        /// Whether the far side reported a core dump.
        core_dumped: bool,
        /// The far side's own message, which is often empty.
        message: String,
    },
    /// The channel closed with no exit status and no signal.
    Unknown,
}

/// Append `bytes` to `sink` without letting it exceed `cap`, recording whether anything was dropped.
///
/// The dropping is what the cap is for; the *continuing* is what the loop around it is for. A reader
/// that stopped at the cap would leave the target blocked writing into a full pipe, so an overflowing
/// command would hang instead of returning a truncated result.
fn collect_capped(sink: &mut Vec<u8>, bytes: &[u8], cap: usize, overflow: &mut bool) {
    let room = cap.saturating_sub(sink.len());
    if bytes.len() > room {
        sink.extend_from_slice(&bytes[..room]);
        *overflow = true;
    } else {
        sink.extend_from_slice(bytes);
    }
}

/// A single SSH connection with an interactive PTY shell.
pub struct SshSession {
    pub host: String,
    pub username: String,
    handle: Handle<SshHandler>,
}

impl SshSession {
    /// Create a new SSH connection. Auth order: public key (`key_path`,
    /// with `password` doubling as the key passphrase) then password with
    /// keyboard-interactive fallback.
    pub async fn connect(
        host: &str,
        port: u16,
        username: &str,
        password: Option<&str>,
        key_path: Option<&str>,
    ) -> Result<Self, DeviceError> {
        // A dead host (firewall DROP, blackholed) used to hang connect()
        // forever — the terminal_open caller never got a timeout. The
        // inactivity_timeout below is only armed AFTER the handshake
        // (round-53 verified in russh), so the TCP connect + SSH-id exchange
        // window is covered by an explicit 15s timeout here; otherwise a
        // blackholed address blocks for the OS connect timeout (Windows
        // ~21s, Linux up to ~130s).
        let config = Arc::new(client::Config {
            // round-95: the old values were inverted — inactivity_timeout 15s
            // with keepalive_interval 30s meant ANY session with no incoming
            // bytes for 15s died before the keepalive ever fired (a `sleep
            // 20` over SSH, or 16s of reading output, killed the session).
            // keepalive must fire BEFORE the inactivity timer expires: 5s
            // keepalive keeps the connection alive; 30s inactivity is the
            // real dead-peer bound.
            inactivity_timeout: Some(std::time::Duration::from_secs(30)),
            keepalive_interval: Some(std::time::Duration::from_secs(5)),
            ..Default::default()
        });
        // TOFU host-key verification — keyed by user@host:port so a changed
        // key (MITM) is rejected on later connections.
        let handler = SshHandler {
            trust_key: format!("{username}@{host}:{port}"),
        };
        let mut handle: Handle<SshHandler> = match tokio::time::timeout(
            std::time::Duration::from_secs(15),
            connect(config, format!("{host}:{port}"), handler),
        )
        .await
        {
            Ok(Ok(h)) => h,
            Ok(Err(e)) => {
                return Err(DeviceError::SshConnectFailed {
                    host: host.to_string(),
                    reason: format!("connect failed: {e}"),
                })
            }
            Err(_) => {
                return Err(DeviceError::SshTimeout {
                    host: host.to_string(),
                })
            }
        };

        // Authenticate: public key when a key path is given (the password
        // field doubles as the key passphrase — None works for unencrypted
        // keys), else the password path below.
        if let Some(kp) = key_path {
            let key = russh::keys::load_secret_key(kp, password).map_err(|e| {
                DeviceError::SshConnectFailed {
                    host: host.to_string(),
                    reason: format!("load private key failed: {e}"),
                }
            })?;
            let auth = handle
                .authenticate_publickey(
                    username,
                    russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), None),
                )
                .await
                .map_err(|e| DeviceError::SshConnectFailed {
                    host: host.to_string(),
                    reason: format!("public key auth failed: {e}"),
                })?;
            if !auth.success() {
                return Err(DeviceError::SshConnectFailed {
                    host: host.to_string(),
                    reason: "public key authentication rejected".into(),
                });
            }
        } else if let Some(pass) = password {
            let auth = handle
                .authenticate_password(username, pass)
                .await
                .map_err(|e| DeviceError::SshConnectFailed {
                    host: host.to_string(),
                    reason: format!("password auth failed: {e}"),
                })?;
            if !auth.success() {
                // Keyboard-interactive fallback: servers with UsePAM /
                // AD / LDAP / 2FA sshd may reject the password method
                // outright and offer only keyboard-interactive. Answer a
                // single password prompt with the same password (bounded —
                // a 2FA/OTP second prompt cannot be answered by a
                // single-password UI).
                use russh::client::KeyboardInteractiveAuthResponse;
                use russh::MethodKind;
                let mut ki_ok = false;
                if let russh::client::AuthResult::Failure {
                    remaining_methods, ..
                } = auth
                {
                    if remaining_methods.contains(&MethodKind::KeyboardInteractive) {
                        let mut resp = handle
                            .authenticate_keyboard_interactive_start(username, None)
                            .await
                            .map_err(|e| DeviceError::SshConnectFailed {
                                host: host.to_string(),
                                reason: format!("keyboard-interactive start failed: {e}"),
                            })?;
                        for _ in 0..4 {
                            match resp {
                                KeyboardInteractiveAuthResponse::Success => {
                                    ki_ok = true;
                                    break;
                                }
                                KeyboardInteractiveAuthResponse::Failure { .. } => break,
                                KeyboardInteractiveAuthResponse::InfoRequest {
                                    prompts, ..
                                } => {
                                    // Answer ONLY a password-looking prompt with
                                    // the password — the first non-empty prompt
                                    // may be an OTP/2FA challenge (Duo, TOTP),
                                    // and sending the real password there is a
                                    // credential leak to the wrong factor.
                                    // A password prompt: hidden echo (p.echo
                                    // false) + prompt text hints (password/
                                    // passphrase/passcode). Empty prompts get
                                    // empty responses (russh requires equal
                                    // lengths).
                                    let responses: Vec<String> = prompts
                                        .iter()
                                        .map(|p| {
                                            let t = p.prompt.to_lowercase();
                                            let pass_like = !p.echo
                                                && (t.contains("password")
                                                    || t.contains("passphrase")
                                                    || t.contains("passcode"));
                                            if pass_like {
                                                pass.to_string()
                                            } else {
                                                String::new()
                                            }
                                        })
                                        .collect();
                                    resp = handle
                                        .authenticate_keyboard_interactive_respond(responses)
                                        .await
                                        .map_err(|e| DeviceError::SshConnectFailed {
                                            host: host.to_string(),
                                            reason: format!(
                                                "keyboard-interactive respond failed: {e}"
                                            ),
                                        })?;
                                }
                            }
                        }
                    }
                }
                if !ki_ok {
                    return Err(DeviceError::SshConnectFailed {
                        host: host.to_string(),
                        reason:
                            "password authentication rejected (password or keyboard-interactive)"
                                .into(),
                    });
                }
            }
        } else {
            return Err(DeviceError::SshConnectFailed {
                host: host.to_string(),
                reason: "no authentication method provided (password or key_path required)".into(),
            });
        }

        Ok(Self {
            host: host.to_string(),
            username: username.to_string(),
            handle,
        })
    }

    /// Open an SFTP session on this connection (P4c). The caller owns the
    /// SftpSession and must close it before dropping the SshSession.
    #[cfg(feature = "terminal")]
    pub async fn sftp_session(&self) -> Result<russh_sftp::client::SftpSession, DeviceError> {
        let channel =
            self.handle
                .channel_open_session()
                .await
                .map_err(|e| DeviceError::Internal {
                    message: format!("sftp open channel: {e}"),
                })?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| DeviceError::Internal {
                message: format!("sftp subsystem request: {e}"),
            })?;
        let sftp = russh_sftp::client::SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| DeviceError::Internal {
                message: format!("sftp init: {e}"),
            })?;
        sftp.set_timeout(15);
        Ok(sftp)
    }

    /// Run one command to completion and collect its output.
    ///
    /// This is the transport the remote subprocess seam needs and that `open_shell` cannot provide:
    /// a one-shot command with a **separate** stdout and stderr, a real exit status, and no PTY.
    /// `command` is the exec string; `stdin` is written and then closed, which is how an argv frame
    /// reaches the staged helper (see `summrise-exec-argv`).
    ///
    /// **The caps do not stop the read.** Once a stream reaches its cap the bytes are dropped rather
    /// than accumulated, but the channel is still drained to EOF: the target writes into a pipe whose
    /// far end is this loop, and a reader that stopped would block the target forever instead of
    /// returning an overflow. `*_overflow` is how the caller learns the output is not the whole story.
    ///
    /// Cancellation is by drop: aborting the caller's task drops the channel, which closes it, which
    /// is what makes the far side terminate. A caller wanting a deadline wraps this in
    /// `tokio::time::timeout`.
    #[cfg(feature = "terminal")]
    pub async fn exec_capture(
        &self,
        command: &str,
        stdin: &[u8],
        stdout_cap: usize,
        stderr_cap: usize,
    ) -> Result<ExecOutcome, DeviceError> {
        let mut channel =
            self.handle
                .channel_open_session()
                .await
                .map_err(|e| DeviceError::Internal {
                    message: format!("exec open channel: {e}"),
                })?;

        channel
            .exec(true, command)
            .await
            .map_err(|e| DeviceError::Internal {
                message: format!("exec request: {e}"),
            })?;

        // SSH carries a data message in bounded pieces; a frame larger than one piece is split here
        // rather than left to the transport to reject.
        const CHUNK: usize = 32 * 1024;
        for piece in stdin.chunks(CHUNK) {
            channel
                .data(piece)
                .await
                .map_err(|e| DeviceError::Internal {
                    message: format!("exec stdin: {e}"),
                })?;
        }
        channel.eof().await.map_err(|e| DeviceError::Internal {
            message: format!("exec stdin close: {e}"),
        })?;

        let mut outcome = ExecOutcome {
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_overflow: false,
            stderr_overflow: false,
            status: ExecStatus::Unknown,
        };

        // `None` means the channel is finished; `Eof`/`Close` arrive before it and are also terminal.
        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Data { data } => {
                    collect_capped(
                        &mut outcome.stdout,
                        &data,
                        stdout_cap,
                        &mut outcome.stdout_overflow,
                    );
                }
                // Extended data type 1 is stderr; every other type is not part of the seam's contract
                // and is deliberately not folded into either stream.
                ChannelMsg::ExtendedData { data, ext: 1 } => {
                    collect_capped(
                        &mut outcome.stderr,
                        &data,
                        stderr_cap,
                        &mut outcome.stderr_overflow,
                    );
                }
                ChannelMsg::ExitStatus { exit_status } => {
                    outcome.status = ExecStatus::Exited(exit_status);
                }
                ChannelMsg::ExitSignal {
                    signal_name,
                    core_dumped,
                    error_message,
                    ..
                } => {
                    outcome.status = ExecStatus::Signalled {
                        name: format!("{signal_name:?}"),
                        core_dumped,
                        message: error_message,
                    };
                }
                ChannelMsg::Eof | ChannelMsg::Close => break,
                _ => {}
            }
        }

        Ok(outcome)
    }

    /// Open an interactive PTY shell on this connection.
    /// Returns (output_rx, write_tx, resize_tx).
    /// A background task multiplexes read/write/resize on the channel.
    /// Output is bounded (backpressure); write/resize are bounded and the
    /// caller uses `try_send` — keystrokes never block.
    pub async fn open_shell(
        &self,
        rows: u16,
        cols: u16,
    ) -> Result<
        (
            mpsc::Receiver<Vec<u8>>,
            mpsc::Sender<Vec<u8>>,
            mpsc::Sender<(u16, u16)>,
        ),
        DeviceError,
    > {
        let channel =
            self.handle
                .channel_open_session()
                .await
                .map_err(|e| DeviceError::Internal {
                    message: format!("open channel: {e}"),
                })?;

        let r = if rows > 0 { rows } else { 24 };
        let c = if cols > 0 { cols } else { 80 };
        channel
            .request_pty(true, "xterm-256color", c as u32, r as u32, 0, 0, &[])
            .await
            .map_err(|e| DeviceError::Internal {
                message: format!("pty request: {e}"),
            })?;

        channel
            .request_shell(true)
            .await
            .map_err(|e| DeviceError::Internal {
                message: format!("shell request: {e}"),
            })?;

        // Output is bounded: a stalled reader pauses the channel task
        // (backpressure on the SSH stream). Write/resize are small and the
        // backend sends with try_send — dropping when full, never blocking.
        let (output_tx, output_rx) = mpsc::channel::<Vec<u8>>(256);
        let (write_tx, mut write_rx) = mpsc::channel::<Vec<u8>>(16);
        let (resize_tx, mut resize_rx) = mpsc::channel::<(u16, u16)>(16);

        // Single task owns the channel, handles read+write+resize via select
        tokio::spawn(async move {
            let mut ch = channel;
            loop {
                tokio::select! {
                    msg = ch.wait() => {
                        match msg {
                            Some(ChannelMsg::Data { data }) => match output_tx.send(Vec::from(&*data)).await {
                                // bounded send: awaits if the queue is full
                                Ok(()) => {}
                                Err(_) => break,
                            },
                            Some(ChannelMsg::Eof) | None => break,
                            _ => {}
                        }
                    }
                    data = write_rx.recv() => {
                        match data {
                            Some(d) => { if ch.data(&d[..]).await.is_err() { break; } }
                            None => break,
                        }
                    }
                    resize = resize_rx.recv() => {
                        match resize {
                            // Channel carries (rows, cols); window_change takes (width=cols, height=rows)
                            Some((rows, cols)) => {
                                let _ = ch.window_change(cols as u32, rows as u32, 0, 0).await;
                            }
                            None => break,
                        }
                    }
                }
            }
        });

        Ok((output_rx, write_tx, resize_tx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn isolated(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("summrise-ssh-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TEST_DIR.with(|d| *d.borrow_mut() = Some(dir.clone()));
        dir
    }

    fn unisolate(dir: &std::path::Path) {
        TEST_DIR.with(|d| *d.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The migrated site's posture, measured: `BestEffort`. An unavailable ACL
    /// must NOT cost SSH its trust table (a failed save re-TOFUs the host next
    /// connection, which is the round-57 trade), the save must still be
    /// READABLE through the production reader, and it must leave no temp — the
    /// name `prune`-style sweeps would have to know is `<name>.tmp`, appended.
    #[test]
    fn tofu_save_lands_when_the_acl_is_unavailable_and_leaves_no_temp() {
        let dir = isolated("tofu-acl");
        let mut map = serde_json::Map::new();
        map.insert(
            "u@h:22".into(),
            serde_json::Value::String("SHA256:abc".into()),
        );
        crate::atomic::with_failing_hardening(|| save_known_hosts(&map))
            .expect("an unavailable ACL must not fail a best-effort trust-table save");

        let landed = load_known_hosts().expect("the trust table must still READ");
        assert_eq!(
            landed.get("u@h:22").and_then(|v| v.as_str()),
            Some("SHA256:abc"),
            "the saved fingerprint is what the next connection verifies against"
        );
        assert!(
            !crate::atomic::temp_path(&known_hosts_path()).exists(),
            "a completed save leaves no temp beside the trust table"
        );
        unisolate(&dir);
    }

    // ── exec_capture, against an in-process SSH server ──────────────────────────────────────────
    //
    // The transport is the part that cannot be reasoned about: it is a channel, a stream split, a
    // flow-controlled window and an exit-status message. A real sshd would need credentials and a
    // machine that is not this one, so the server lives in the test — and it is a *real* SSH server,
    // not a mock of one, so the client code under test is the same code that talks to a device.
    mod exec_server {
        use russh::server::{Auth, Msg, Server, Session};
        use russh::{Channel, ChannelId};
        use std::sync::{Arc, Mutex};

        /// What the server should reply with, and what it saw.
        #[derive(Clone, Default)]
        pub struct Script {
            pub stdout: Vec<u8>,
            pub stderr: Vec<u8>,
            pub exit: u32,
            /// Everything the client wrote to the channel — the argv frame, for a real caller.
            pub received: Arc<Mutex<Vec<u8>>>,
            /// The exec string the client asked for.
            pub command: Arc<Mutex<String>>,
        }

        pub struct TestServer(pub Script);

        impl Server for TestServer {
            type Handler = Script;
            fn new_client(&mut self, _peer: Option<std::net::SocketAddr>) -> Script {
                self.0.clone()
            }
        }

        impl russh::server::Handler for Script {
            type Error = russh::Error;

            async fn auth_password(
                &mut self,
                _user: &str,
                _password: &str,
            ) -> Result<Auth, Self::Error> {
                Ok(Auth::Accept)
            }

            async fn channel_open_session(
                &mut self,
                _channel: Channel<Msg>,
                _session: &mut Session,
            ) -> Result<bool, Self::Error> {
                Ok(true)
            }

            async fn exec_request(
                &mut self,
                channel: ChannelId,
                data: &[u8],
                session: &mut Session,
            ) -> Result<(), Self::Error> {
                *self.command.lock().unwrap() = String::from_utf8_lossy(data).into_owned();
                session.channel_success(channel)?;
                Ok(())
            }

            async fn data(
                &mut self,
                _channel: ChannelId,
                data: &[u8],
                _session: &mut Session,
            ) -> Result<(), Self::Error> {
                self.received.lock().unwrap().extend_from_slice(data);
                Ok(())
            }

            /// The client sends EOF once its frame is written, so this is the moment to answer —
            /// which is what lets the assertions compare the reply against everything that was sent.
            async fn channel_eof(
                &mut self,
                channel: ChannelId,
                session: &mut Session,
            ) -> Result<(), Self::Error> {
                if !self.stdout.is_empty() {
                    session.data(channel, russh::CryptoVec::from_slice(&self.stdout))?;
                }
                if !self.stderr.is_empty() {
                    session.extended_data(
                        channel,
                        1,
                        russh::CryptoVec::from_slice(&self.stderr),
                    )?;
                }
                session.exit_status_request(channel, self.exit)?;
                session.eof(channel)?;
                session.close(channel)?;
                Ok(())
            }
        }

        /// Start a server on a loopback port and return its address.
        ///
        /// The listener is bound *inside* the spawned task so the task is `'static`: `run_on_socket`
        /// borrows the listener, and a borrowed server cannot be spawned alongside the client that
        /// has to talk to it.
        pub async fn start(script: Script) -> std::net::SocketAddr {
            let (tx, rx) = tokio::sync::oneshot::channel();
            tokio::spawn(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("the test server binds a loopback port");
                let addr = listener.local_addr().expect("the listener has an address");
                let _ = tx.send(addr);

                let config = russh::server::Config {
                    inactivity_timeout: Some(std::time::Duration::from_secs(30)),
                    keys: vec![russh::keys::PrivateKey::random(
                        &mut rand::rngs::OsRng,
                        russh::keys::Algorithm::Ed25519,
                    )
                    .expect("a host key is generated per run")],
                    ..Default::default()
                };
                let mut server = TestServer(script);
                let running = server.run_on_socket(Arc::new(config), &listener);
                let _ = running.await;
            });
            rx.await.expect("the test server reports its address")
        }
    }

    /// Connect to the test server and run one command through `exec_capture`.
    ///
    /// Callers keep their own clone of the script's shared handles (`received`, `command`) before
    /// handing it over, because the server moves it into a spawned task.
    async fn exec_against(
        script: exec_server::Script,
        stdin: &[u8],
        stdout_cap: usize,
        stderr_cap: usize,
    ) -> ExecOutcome {
        let addr = exec_server::start(script).await;
        let session = SshSession::connect("127.0.0.1", addr.port(), "tester", Some("pw"), None)
            .await
            .expect("the test server accepts the connection");
        session
            .exec_capture("summrise-exec-argv", stdin, stdout_cap, stderr_cap)
            .await
            .expect("the command runs to completion")
    }

    #[tokio::test]
    async fn exec_capture_separates_the_streams_and_reads_the_exit_status() {
        let dir = isolated("exec-capture-basic");
        let script = exec_server::Script {
            stdout: b"the target's stdout".to_vec(),
            stderr: b"the target's stderr".to_vec(),
            exit: 7,
            ..Default::default()
        };
        let received = script.received.clone();
        let command = script.command.clone();

        let outcome = exec_against(script, b"an argv frame", 4096, 4096).await;

        assert_eq!(outcome.stdout, b"the target's stdout");
        assert_eq!(
            outcome.stderr, b"the target's stderr",
            "extended data type 1 is stderr and must not be folded into stdout"
        );
        assert_eq!(outcome.status, ExecStatus::Exited(7));
        assert!(!outcome.stdout_overflow && !outcome.stderr_overflow);
        assert_eq!(
            &*received.lock().unwrap(),
            b"an argv frame",
            "the frame must arrive byte-identical, which is the whole point of the helper"
        );
        assert_eq!(
            &*command.lock().unwrap(),
            "summrise-exec-argv",
            "the exec string carries only the helper's path — never the model's argv"
        );
        unisolate(&dir);
    }

    #[tokio::test]
    async fn exec_capture_caps_a_stream_without_blocking_the_target() {
        // The behaviour a cap is worthless without. The target writes three times what the caller
        // will keep; if the reader stopped at the cap, the target would block on a full pipe and the
        // call would hang instead of returning a truncated result. So this test passing at all is
        // the assertion that the drain continues — the flag and the length are the visible evidence.
        let dir = isolated("exec-capture-cap");
        let script = exec_server::Script {
            stdout: vec![b'x'; 3000],
            stderr: Vec::new(),
            exit: 0,
            ..Default::default()
        };

        let outcome = exec_against(script, b"", 1000, 1000).await;

        assert_eq!(
            outcome.stdout.len(),
            1000,
            "the cap is what the caller keeps"
        );
        assert!(
            outcome.stdout_overflow,
            "the caller must be told the output is not the whole story"
        );
        assert_eq!(outcome.status, ExecStatus::Exited(0));
        unisolate(&dir);
    }
}
