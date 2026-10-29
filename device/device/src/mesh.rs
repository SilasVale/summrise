//! The mesh link: this device as a peer, serving the MCP surface it already serves.
//!
//! The design's target is a direct peer path (§5.2): a device joins the mesh, publishes where it
//! can be reached, and a peer that reaches it sends the **same frame the console sends** —
//! `{"id","mcp":{...},"grant":{...}}`. The grant is verified offline by [`crate::remote::admit`],
//! against the same pinned console key, the same device id and the same tool scope the console
//! channel uses; only then does the request reach the MCP handler as [`Caller::Granted`], which is
//! the same authorization path the loopback surface and the console channel take. The answer is
//! `{"id","result"|"error"}`, exactly as on the console channel.
//!
//! **The device id a grant must name is the peer id** — the hex public key in
//! [`crate::identity`]'s file. One key is this device's name on both paths, so a console that
//! registered this device's key issues grants that the mesh accepts without a second registry.
//!
//! **Absent flags mean nothing changes.** Without `--mesh-broker` this module is never started and
//! the device is exactly what it was before the mesh existed. With a broker it joins, registers its
//! candidates and serves; the relay is the floor, so `--mesh-relay` alone (a floor nothing can lead
//! to) and `--mesh-stun` alone (a reflection with nowhere to publish it) are configuration errors
//! rather than silent half-features.
//!
//! **The direct path.** Without a reflector the endpoint binds loopback, so the published candidate
//! is only reachable on this machine and a peer elsewhere uses the relay floor. With
//! `--mesh-stun` the join asks the reflector on the very socket QUIC will own; the endpoint binds
//! the wildcard, and the reflected address is the published candidate (the wildcard itself is never
//! published). A reflector that does not answer is one log line: the join falls back to the local
//! address the kernel would use to reach the reflector, or publishes nothing and leaves the relay
//! as the floor.
//!
//! **What is not here yet.** A serving session cannot also dial: `MeshSession::serve` borrows what
//! `connect` mutates, so a device that wants to open a channel of its own holds a second session
//! with the same key. Nothing in this module needs one — the device serves, and the peer dials.

use std::net::SocketAddr;
use std::time::Duration;

use ed25519_dalek::VerifyingKey;
use serde_json::{json, Value};
use tokio::task::JoinHandle;

use summrise_core::mesh::backoff;
use summrise_mesh::{Channel, Frame, Identity, Kind, MeshConfig, MeshSession, SessionError, Uri};

use crate::config::Config;
use crate::console;
use crate::identity;
use crate::mcp::{Arrival, Caller, DeviceServer};
use crate::remote::{self, RemoteFrame};

/// How long one authenticated QUIC handshake may take before the connection is closed.
///
/// It covers the whole authentication — the handshake, the challenge this side issues, the hello it
/// reads back and the challenge and signature checks — the same window the console channel uses for
/// a connection it must not let hold the accept loop.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// The one timeout `Path` uses for both the rendezvous wait and the punch.
const PUNCH_TIMEOUT_MS: u64 = 5_000;

/// The local UDP address the QUIC endpoint binds when no STUN reflector is configured.
///
/// Loopback, like the HTTP surface, and for the same reason: a device that listens on every
/// interface is one anybody on the network can probe. Without a reflector the candidate a session
/// publishes is its bound address, so this is the loopback deployment; a peer on another machine
/// is reached through the relay floor.
const MESH_BIND: &str = "127.0.0.1:0";

/// The local UDP address the QUIC endpoint binds when a STUN reflector is configured.
///
/// A loopback-bound socket's reflection is `127.0.0.1:<port>`, which no peer on another machine
/// can use. Naming a reflector is therefore the operator's opt-in to a wildcard UDP bind: the
/// exchange can then learn the address the device is seen at from outside. The QUIC handshake
/// still pins every peer by certificate and signed hello, so the wider bind changes reachability,
/// not authorization.
const MESH_STUN_BIND: &str = "0.0.0.0:0";

/// Everything the mesh link needs that the command line does not carry.
#[derive(Clone)]
pub(crate) struct MeshLink {
    /// The rendezvous broker's origin.
    broker: Uri,
    /// The relay floor, when one is configured.
    relay: Option<SocketAddr>,
    /// The STUN reflector the join asks on its own QUIC socket, when configured.
    stun_server: Option<SocketAddr>,
    /// This device's keypair: its peer id, and the id a console grant must name.
    identity: Identity,
    /// The local UDP address the QUIC endpoint binds.
    bind: SocketAddr,
    /// The punch/rendezvous budget `Path` uses, in milliseconds.
    punch_timeout_ms: u64,
    /// How long one broker exchange may take, in milliseconds.
    lookup_timeout_ms: u64,
}

impl std::fmt::Debug for MeshLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeshLink")
            .field("peer_id", &self.identity.public_hex())
            .field("broker", &self.broker.to_string())
            .field("relay", &self.relay)
            .field("stun_server", &self.stun_server)
            .field("bind", &self.bind)
            .finish_non_exhaustive()
    }
}

/// Why the mesh link could not be built, joined or served.
#[derive(Debug)]
pub(crate) enum MeshError {
    /// `--mesh-broker` is not the `http://host:port` origin the rendezvous speaks.
    BadBroker(String),
    /// `--mesh-relay` is not a socket address.
    BadRelay(String),
    /// `--mesh-stun` is not a socket address.
    BadStun(String),
    /// The identity file could not be read or minted.
    Identity(identity::IdentityError),
    /// The session could not be joined, or the endpoint closed underneath it.
    Session(SessionError),
}

impl std::fmt::Display for MeshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MeshError::BadBroker(broker) => write!(
                f,
                "the mesh broker '{broker}' is not an http://host:port origin (the broker is plain \
                 HTTP; TLS is phase 2b)"
            ),
            MeshError::BadRelay(relay) => write!(
                f,
                "the mesh relay '{relay}' is not an address such as 127.0.0.1:9000"
            ),
            MeshError::BadStun(stun) => write!(
                f,
                "the mesh STUN server '{stun}' is not an address such as 127.0.0.1:3478"
            ),
            MeshError::Identity(error) => write!(f, "{error}"),
            MeshError::Session(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for MeshError {}

impl From<identity::IdentityError> for MeshError {
    fn from(error: identity::IdentityError) -> Self {
        MeshError::Identity(error)
    }
}

impl MeshLink {
    /// Build the link from the configuration, or `None` when no broker flag was given.
    ///
    /// Everything that can be wrong with the configured facts is refused here, at startup, rather
    /// than on the first frame: a broker URL the rendezvous cannot use and an identity file that is
    /// not this device's key are both facts an operator must fix, not conditions to retry.
    pub(crate) fn from_config(config: &Config) -> Result<Option<MeshLink>, MeshError> {
        let Some(broker) = config.mesh_broker.as_deref() else {
            return Ok(None);
        };
        let broker = parse_broker(broker)?;
        let relay = match config.mesh_relay.as_deref() {
            Some(relay) => Some(
                relay
                    .trim()
                    .parse::<SocketAddr>()
                    .map_err(|_| MeshError::BadRelay(relay.trim().to_owned()))?,
            ),
            None => None,
        };
        let stun_server = match config.mesh_stun.as_deref() {
            Some(stun) => Some(
                stun.trim()
                    .parse::<SocketAddr>()
                    .map_err(|_| MeshError::BadStun(stun.trim().to_owned()))?,
            ),
            None => None,
        };
        // The identity is minted or read here, at startup. A device whose key file is unusable must
        // fail loudly: serving a mesh under a name nobody registered is the silent failure this
        // whole module exists to avoid.
        let identity = identity::load_or_mint(&config.data)?;
        let bind: SocketAddr = if stun_server.is_some() {
            MESH_STUN_BIND
        } else {
            MESH_BIND
        }
        .parse()
        .expect("the mesh bind address parses");
        Ok(Some(MeshLink {
            broker,
            relay,
            stun_server,
            identity,
            bind,
            punch_timeout_ms: PUNCH_TIMEOUT_MS,
            lookup_timeout_ms: summrise_mesh::LOOKUP_TIMEOUT_MS,
        }))
    }

    /// This device's peer id: the identity's public key in hex.
    pub(crate) fn peer_id(&self) -> String {
        self.identity.public_hex()
    }

    /// The session configuration this link joins with.
    fn mesh_config(&self) -> MeshConfig {
        MeshConfig {
            broker: self.broker.clone(),
            relay: self.relay,
            bind: self.bind,
            stun_server: self.stun_server,
            punch_timeout_ms: self.punch_timeout_ms,
            lookup_timeout_ms: self.lookup_timeout_ms,
        }
    }

    /// Join the mesh as this device: bind the endpoint and publish the candidates.
    pub(crate) async fn join(&self) -> Result<MeshSession, MeshError> {
        MeshSession::join(self.identity.clone(), self.mesh_config())
            .await
            .map_err(MeshError::Session)
    }
}

/// Parse and check the broker origin the rendezvous client will speak to.
///
/// The client refuses TLS and any path — phase 2's broker is a plain-HTTP process — so a URL it
/// would refuse is a startup error naming the flag, not a link that retries a join it can never
/// complete.
fn parse_broker(broker: &str) -> Result<Uri, MeshError> {
    let text = broker.trim();
    let uri: Uri = text
        .parse()
        .map_err(|_| MeshError::BadBroker(text.to_owned()))?;
    let origin = uri.scheme_str() == Some("http")
        && uri.authority().is_some()
        && uri.query().is_none()
        && matches!(uri.path(), "" | "/");
    if !origin {
        return Err(MeshError::BadBroker(text.to_owned()));
    }
    Ok(uri)
}

/// Serve every inbound channel of `session` through `server`, until the endpoint closes.
///
/// `device_id` is the id a frame's grant must name: this device's peer id.
pub(crate) async fn serve(
    session: MeshSession,
    server: DeviceServer,
    key: VerifyingKey,
    device_id: String,
) -> Result<(), MeshError> {
    session
        .serve(HANDSHAKE_TIMEOUT, move |peer, channel| {
            let server = server.clone();
            let device_id = device_id.clone();
            let key = key;
            async move { handle_channel(&server, &key, &device_id, peer, channel).await }
        })
        .await
        .map_err(MeshError::Session)
}

/// Answer every request frame on one channel until the peer disconnects.
///
/// The channel is authenticated and pinned by the session before this is called: the peer's key is
/// the one the hello proved, so what is left to check is the grant the frame carries.
async fn handle_channel(
    server: &DeviceServer,
    key: &VerifyingKey,
    device_id: &str,
    peer: String,
    mut channel: Channel,
) {
    while let Ok(frame) = channel.recv_frame().await {
        // Only a request is answered. A frame that is not one is ignored rather than refused: the
        // session's own frames (a challenge, a hello, a response) are the transport's business, and
        // a response is not a question.
        if frame.kind != Kind::Req {
            continue;
        }
        let Some(reply) = answer(server, key, device_id, &peer, &frame.payload).await else {
            continue;
        };
        let response = Frame {
            kind: Kind::Res,
            id: frame.id,
            payload: reply,
        };
        if channel.send_frame(&response).await.is_err() {
            return;
        }
    }
}

/// Admit one request frame's grant and run its request, or answer the refusal.
///
/// `None` means the payload was not a remote frame at all, so there is nothing to correlate an
/// answer with; the connection stays open for the next frame.
async fn answer(
    server: &DeviceServer,
    key: &VerifyingKey,
    device_id: &str,
    peer: &str,
    payload: &Value,
) -> Option<Value> {
    let frame: RemoteFrame = match serde_json::from_value(payload.clone()) {
        Ok(frame) => frame,
        Err(error) => {
            eprintln!("summrise-device: a mesh peer sent a malformed frame: {error}");
            return None;
        }
    };

    let request_id = frame.mcp.get("id").cloned().unwrap_or(Value::Null);
    let tool = remote::requested_tool(&frame.mcp).map(str::to_owned);

    let admitted = match remote::admit(
        &frame,
        device_id,
        key,
        crate::http::now_ms(),
        remote::Transport::Mesh,
        server.nonces(),
    ) {
        Ok(envelope) => envelope,
        Err(refusal) => {
            server
                .audit_refused(
                    tool.as_deref().unwrap_or("(unknown)"),
                    refusal.error_kind(),
                    Arrival::Mesh,
                    Some(peer),
                )
                .await;
            return Some(json!({
                "id": frame.id,
                "error": remote::jsonrpc_error(
                    request_id,
                    remote::GRANT_REFUSAL_CODE,
                    refusal.message(),
                ),
            }));
        }
    };

    let caller = match &admitted {
        remote::Admitted::Grant(envelope) => Caller::Granted {
            grant: &envelope.grant,
            arrival: Arrival::Mesh,
            peer: Some(peer),
        },
        remote::Admitted::Token(scope) => Caller::Console {
            token_id: &scope.token_id,
            scopes: &scope.scopes,
            allow_destructive: scope.allow_destructive,
            arrival: Arrival::Mesh,
        },
    };
    let response = server.handle(&frame.mcp, caller).await;
    if response.get("error").is_some() {
        Some(json!({ "id": frame.id, "error": response }))
    } else {
        Some(json!({ "id": frame.id, "result": response }))
    }
}

/// Start the mesh link: join, serve, and re-join on failure until the task is aborted.
///
/// A build that pins no usable console key refuses to arm the link at all: every frame would be
/// unverifiable, and serving them would be a worse failure than not serving.
pub(crate) fn spawn(link: MeshLink, server: DeviceServer) -> JoinHandle<()> {
    match console::pinned_console_key() {
        Ok(key) => tokio::spawn(run(link, server, key)),
        Err(error) => {
            eprintln!("summrise-device: the mesh link is disabled: {error}");
            tokio::spawn(async {})
        }
    }
}

/// Join and serve forever, with the console link's backoff between attempts.
async fn run(link: MeshLink, server: DeviceServer, key: VerifyingKey) {
    let mut attempt: u8 = 1;
    loop {
        match link.join().await {
            Ok(session) => {
                // A join that worked is a link that was up: a later failure starts the backoff over
                // rather than resuming where an earlier outage left it.
                attempt = 1;
                let device_id = link.peer_id();
                if let Err(error) = serve(session, server.clone(), key, device_id).await {
                    eprintln!("summrise-device: the mesh link is down: {error}");
                }
            }
            Err(error) => eprintln!(
                "summrise-device: cannot join the mesh at {}: {error}",
                link.broker
            ),
        }
        let delay = backoff(attempt);
        tokio::time::sleep(Duration::from_millis(delay)).await;
        attempt = attempt.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;
    use summrise_core::grant::{canonical_message, Grant, GrantEnvelope};
    use summrise_core::mesh::PathState;
    use summrise_mesh::broker::Broker;
    use summrise_mesh::relay::RelayServer;
    use summrise_mesh::rendezvous::{now_ms, Candidate, CandidateKind, Rendezvous};
    use summrise_mesh::REGISTRATION_TTL_MS;
    use tokio::net::{TcpListener, UdpSocket};
    use tokio::time::timeout;

    use super::*;
    use crate::capabilities::terminal;
    use crate::test_support::TempDir;

    /// How long a test waits for something that must happen.
    const PATIENCE: Duration = Duration::from_secs(10);

    /// The punch budget the direct tests use: long enough for a loopback QUIC handshake.
    const DIRECT_PUNCH_TIMEOUT_MS: u64 = 2_000;

    /// The punch budget the forced-relay test uses: long enough to fail, short enough to be quick.
    const DEAD_PUNCH_TIMEOUT_MS: u64 = 300;

    const SIGNING_SEED: [u8; 32] = [0x51; 32];

    fn signing() -> SigningKey {
        SigningKey::from_bytes(&SIGNING_SEED)
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// A real broker on an ephemeral loopback port, serving for the test's lifetime.
    async fn start_broker() -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let address = listener.local_addr().expect("the bound address");
        tokio::spawn(async move {
            axum::serve(listener, Broker::new().router())
                .await
                .expect("the test broker serves");
        });
        format!("http://{address}")
    }

    /// A real relay on an ephemeral loopback port, serving for the test's lifetime.
    async fn start_relay() -> SocketAddr {
        let bind: SocketAddr = "127.0.0.1:0".parse().expect("a loopback address");
        let server = RelayServer::bind(bind, Duration::from_secs(5))
            .await
            .expect("a relay");
        let address = server.local_addr();
        tokio::spawn(async move { server.run().await });
        address
    }

    /// One device instance: its own data directory, identity file, MCP handler and mesh link.
    struct Device {
        config: Config,
        server: DeviceServer,
        link: MeshLink,
        _dir: TempDir,
    }

    impl Device {
        fn peer_id(&self) -> String {
            self.link.peer_id()
        }
    }

    fn device_config(label: &str, broker: Option<&str>, relay: Option<&str>) -> (TempDir, Config) {
        let dir = TempDir::new(label);
        let install = dir.join("install");
        let data = dir.join("data");
        let files_root = dir.join("workspace");
        for path in [&install, &data, &files_root] {
            fs::create_dir_all(path).expect("the fixture directory is creatable");
        }
        let config = Config {
            install,
            data,
            port: 0,
            token_file: dir.join("token"),
            files_root,
            release_version: None,
            release_sha: None,
            release_channel: None,
            supervised: false,
            console_url: None,
            console_token_file: None,
            console_allow_plaintext: false,
            mesh_broker: broker.map(str::to_owned),
            mesh_relay: relay.map(str::to_owned),
            mesh_stun: None,
        };
        (dir, config)
    }

    fn device(label: &str, broker: &str, relay: Option<SocketAddr>) -> Device {
        let relay = relay.map(|address| address.to_string());
        let (dir, config) = device_config(label, Some(broker), relay.as_deref());
        let server = DeviceServer::new(&config, terminal::Limits::default());
        let link = MeshLink::from_config(&config)
            .expect("the fixture's facts build a link")
            .expect("the fixture names a broker");
        Device {
            config,
            server,
            link,
            _dir: dir,
        }
    }

    /// Two devices, the higher-keyed one dialing the other.
    ///
    /// The direct role is a function of the two keys — the higher key is the TLS client and dials,
    /// the lower is the server and accepts — so the fixture discovers which device dials instead of
    /// assuming, and asserts the rule it relies on.
    fn ordered(first: Device, second: Device) -> (Device, Device) {
        let (dialer, target) = if first.peer_id() > second.peer_id() {
            (first, second)
        } else {
            (second, first)
        };
        assert!(
            dialer.peer_id() > target.peer_id(),
            "the dialer is the higher key; the session computes the role from the two keys"
        );
        (dialer, target)
    }

    /// A joined pair: the target serving through the shipped [`serve`], the dialer connected to it.
    struct Pair {
        target: Device,
        /// Held for the lifetime of the pair: dropping the session closes the channel.
        _session: MeshSession,
        channel: Channel,
        _dialer: Device,
        _serving: JoinHandle<Result<(), MeshError>>,
    }

    impl Pair {
        /// Send one frame and read its answer.
        async fn ask(&mut self, mcp: Value, grant: Option<GrantEnvelope>) -> Value {
            let payload = match grant {
                Some(grant) => json!({ "id": 7, "mcp": mcp, "grant": grant }),
                None => json!({ "id": 7, "mcp": mcp }),
            };
            self.exchange(payload).await
        }

        /// Send a payload exactly as given — for a frame the harness cannot build from a grant.
        async fn exchange(&mut self, payload: Value) -> Value {
            let request = Frame {
                kind: Kind::Req,
                id: 7,
                payload,
            };
            self.channel
                .send_frame(&request)
                .await
                .expect("the request goes out");
            let reply = timeout(PATIENCE, self.channel.recv_frame())
                .await
                .expect("the answer timed out")
                .expect("the answer arrives");
            assert_eq!(reply.kind, Kind::Res, "the target answers with a response");
            assert_eq!(reply.id, 7, "the mesh id correlates the answer");
            reply.payload
        }
    }

    /// Start the target's serving task and dial it from the dialer.
    async fn connect(
        dialer: &Device,
        target: &Device,
    ) -> (MeshSession, Channel, JoinHandle<Result<(), MeshError>>) {
        let target_session = target.link.join().await.expect("the target joins the mesh");
        let serving = tokio::spawn(serve(
            target_session,
            target.server.clone(),
            signing().verifying_key(),
            target.peer_id(),
        ));
        let mut session = dialer.link.join().await.expect("the dialer joins the mesh");
        let channel = timeout(PATIENCE, session.connect(&target.peer_id(), now_ms()))
            .await
            .expect("the connect timed out")
            .expect("the channel opens");
        (session, channel, serving)
    }

    /// A joined pair on a fresh broker and relay, dialing directly.
    async fn pair(label: &str) -> Pair {
        let broker = start_broker().await;
        let relay = start_relay().await;
        let (mut dialer, target) = ordered(
            device(&format!("{label}-a"), &broker, Some(relay)),
            device(&format!("{label}-b"), &broker, Some(relay)),
        );
        dialer.link.punch_timeout_ms = DIRECT_PUNCH_TIMEOUT_MS;

        let (session, channel, serving) = connect(&dialer, &target).await;
        assert!(
            channel.is_direct(),
            "the direct path is the one this fixture must exercise"
        );
        Pair {
            target,
            _session: session,
            channel,
            _dialer: dialer,
            _serving: serving,
        }
    }

    fn state_call(id: u64) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": "state.get", "arguments": {} },
        })
    }

    fn write_call(id: u64, path: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": "files.write",
                "arguments": { "path": path, "content": "must not land" }
            },
        })
    }

    fn signed_by(key: &SigningKey, grant: Grant) -> GrantEnvelope {
        let signature = key.sign(canonical_message(&grant).as_bytes());
        GrantEnvelope {
            grant,
            signature: hex(&signature.to_bytes()),
        }
    }

    fn grant_for(device_id: &str, tools: &[&str], expires_at_ms: u64) -> GrantEnvelope {
        signed_by(
            &signing(),
            Grant {
                device_id: device_id.to_owned(),
                tools: tools.iter().map(|tool| (*tool).to_owned()).collect(),
                expires_at_ms,
                allow_destructive: false,
                nonce: String::new(),
            },
        )
    }

    fn audit_lines(config: &Config) -> Vec<Value> {
        match fs::read_to_string(config.data.join("events.jsonl")) {
            Ok(contents) => contents
                .lines()
                .map(|line| serde_json::from_str(line).expect("an audit line is JSON"))
                .collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("the audit log is readable: {error}"),
        }
    }

    #[test]
    fn no_broker_flag_is_no_mesh_link() {
        let (_dir, config) = device_config("mesh-absent", None, None);
        assert!(
            MeshLink::from_config(&config)
                .expect("an absent mesh is not an error")
                .is_none(),
            "absent flags mean nothing changes"
        );
    }

    #[test]
    fn the_broker_url_must_be_the_plain_http_origin_the_rendezvous_speaks() {
        for broker in [
            "",
            "127.0.0.1:8080",
            "https://broker.example.test",
            "ftp://broker.example.test",
            "http://",
            "http://broker.example.test/path",
        ] {
            let (_dir, config) = device_config("mesh-bad-broker", Some(broker), None);
            let error = MeshLink::from_config(&config)
                .expect_err("a URL the rendezvous cannot use is refused at startup");
            assert!(
                matches!(error, MeshError::BadBroker(_)),
                "{broker:?} must be a typed refusal: {error}"
            );
        }

        let (_dir, config) = device_config("mesh-good-broker", Some("http://127.0.0.1:8080"), None);
        let link = MeshLink::from_config(&config)
            .expect("a loopback origin is the broker's own shape")
            .expect("the broker flag is present");
        assert_eq!(
            link.broker.to_string().trim_end_matches('/'),
            "http://127.0.0.1:8080",
            "the origin is what the rendezvous client dials; `Uri` spells its empty path as /"
        );
    }

    #[test]
    fn the_relay_address_is_a_socket_address_or_a_typed_refusal() {
        let (_dir, config) = device_config(
            "mesh-bad-relay",
            Some("http://127.0.0.1:8080"),
            Some("not-an-address"),
        );
        let error = MeshLink::from_config(&config).expect_err("a relay address must parse");
        assert!(
            matches!(error, MeshError::BadRelay(_)),
            "the refusal names the flag's fact: {error}"
        );

        let (_dir, config) = device_config(
            "mesh-good-relay",
            Some("http://127.0.0.1:8080"),
            Some("127.0.0.1:9000"),
        );
        let link = MeshLink::from_config(&config)
            .expect("a socket address is the relay's own shape")
            .expect("the broker flag is present");
        assert_eq!(
            link.relay,
            Some("127.0.0.1:9000".parse().expect("a socket address"))
        );
    }

    #[test]
    fn the_stun_address_is_a_socket_address_or_a_typed_refusal() {
        let (_dir, mut config) =
            device_config("mesh-bad-stun", Some("http://127.0.0.1:8080"), None);
        config.mesh_stun = Some("not-an-address".to_owned());
        let error = MeshLink::from_config(&config).expect_err("a STUN address must parse");
        assert!(
            matches!(error, MeshError::BadStun(_)),
            "the refusal names the flag's fact: {error}"
        );

        config.mesh_stun = Some("203.0.113.10:3478".to_owned());
        let link = MeshLink::from_config(&config)
            .expect("a socket address is the reflector's own shape")
            .expect("the broker flag is present");
        assert_eq!(
            link.stun_server,
            Some("203.0.113.10:3478".parse().expect("a socket address"))
        );
        assert_eq!(
            link.mesh_config().stun_server,
            Some("203.0.113.10:3478".parse().expect("a socket address")),
            "the link hands the reflector to the session"
        );
    }

    #[test]
    fn a_stun_reflector_moves_the_mesh_socket_off_loopback() {
        let (_dir, mut config) =
            device_config("mesh-stun-bind", Some("http://127.0.0.1:8080"), None);
        let loopback_link = MeshLink::from_config(&config)
            .expect("the link builds")
            .expect("the broker flag is present");
        assert_eq!(
            loopback_link.bind,
            "127.0.0.1:0".parse().expect("a loopback address"),
            "without a reflector the mesh keeps the loopback-only posture"
        );

        config.mesh_stun = Some("203.0.113.10:3478".to_owned());
        let stun_link = MeshLink::from_config(&config)
            .expect("the link builds")
            .expect("the broker flag is present");
        assert_eq!(
            stun_link.bind,
            "0.0.0.0:0".parse().expect("a wildcard address"),
            "a reflection is only useful off-host when the socket is not bound to loopback"
        );
    }

    #[test]
    fn the_peer_id_is_the_identity_files_public_key() {
        let (_dir, config) = device_config("mesh-peer-id", Some("http://127.0.0.1:8080"), None);
        let link = MeshLink::from_config(&config)
            .expect("the link builds")
            .expect("the broker flag is present");

        let text = fs::read_to_string(config.data.join(identity::IDENTITY_FILE))
            .expect("joining mints the identity file");
        let seed: [u8; 32] = summrise_mesh::keys::hex_decode(text.trim())
            .expect("the seed decodes")
            .try_into()
            .expect("the seed is 32 bytes");
        assert_eq!(
            link.peer_id(),
            Identity::from_private_bytes(seed).public_hex(),
            "the peer id is the key on disk, which is the name the mesh knows this device by"
        );
    }

    #[tokio::test]
    async fn a_build_with_no_pinned_console_key_serves_no_mesh() {
        // The placeholder build is the one CI compiles. It must refuse to arm a link whose frames
        // it could not verify; a build that pins a key skips this test's premise.
        if console::pinned_console_key().is_ok() {
            return;
        }
        let broker = start_broker().await;
        let target = device("mesh-no-key", &broker, None);

        let handle = spawn(target.link.clone(), target.server.clone());
        let _ = timeout(Duration::from_secs(1), handle)
            .await
            .expect("a placeholder build starts no link and never retries one");
    }

    #[tokio::test]
    async fn a_granted_state_get_over_the_mesh_answers_the_device_document() {
        let mut pair = pair("mesh-granted").await;
        let caller = pair._dialer.peer_id();
        let grant = grant_for(&pair.target.peer_id(), &["state.get"], now_ms() + 60_000);

        let reply = pair.ask(state_call(101), Some(grant)).await;

        assert_eq!(reply["id"], 7, "the payload id is echoed: {reply}");
        assert_eq!(reply["result"]["id"], 101, "{reply}");
        assert!(
            reply["result"]["result"]["isError"] != true,
            "the granted read succeeds: {reply}"
        );
        let document: Value = serde_json::from_str(
            reply["result"]["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or_else(|| panic!("state.get answers text: {reply}")),
        )
        .expect("state.get answers one JSON document");
        assert_eq!(
            document["state"]["install"]["dir"],
            pair.target.config.install.display().to_string(),
            "the peer reads the same document /api/state answers: {document}"
        );
        assert_eq!(document["health"]["status"], "ok", "{document}");

        let lines = audit_lines(&pair.target.config);
        assert_eq!(lines.len(), 2, "one call is a begin and an end: {lines:?}");
        assert_eq!(lines[0]["event"], "begin");
        assert_eq!(lines[0]["tool"], "state.get");
        assert_eq!(lines[0]["decision"], "allow");
        assert_eq!(
            lines[0]["transport"], "mesh",
            "a mesh call is not a loopback one: {lines:?}"
        );
        assert_eq!(
            lines[0]["peer"], caller,
            "the begin line names the channel's pinned public key: {lines:?}"
        );
        assert_eq!(lines[1]["event"], "end");
        assert_eq!(lines[1]["ok"], true);
        assert_eq!(lines[1]["transport"], "mesh");
        assert_eq!(
            lines[1]["peer"], caller,
            "the end line names the same caller as its begin: {lines:?}"
        );
    }

    #[tokio::test]
    async fn a_mesh_frame_with_no_grant_is_refused_and_audited_deny() {
        let mut pair = pair("mesh-no-grant").await;
        let caller = pair._dialer.peer_id();

        let reply = pair.ask(state_call(102), None).await;

        assert_eq!(reply["id"], 7);
        assert_eq!(
            reply["error"]["error"]["code"], -32001,
            "a frame with no grant is refused before it runs: {reply}"
        );
        assert!(
            reply["error"]["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("no console grant")),
            "{reply}"
        );
        let lines = audit_lines(&pair.target.config);
        assert_eq!(lines[0]["decision"], "deny");
        assert_eq!(lines[0]["tool"], "state.get");
        assert_eq!(lines[0]["transport"], "mesh");
        assert_eq!(
            lines[0]["peer"], caller,
            "a refused mesh call still names the peer that sent it: {lines:?}"
        );
        assert_eq!(lines[1]["ok"], false);
        assert_eq!(lines[1]["error_kind"], "no_grant");
        assert_eq!(
            lines[1]["peer"], caller,
            "the refusal's end line names the same peer: {lines:?}"
        );
    }

    /// A token scope has no signature: the console's authenticated channel is the only thing that
    /// can stand behind one. A mesh peer is authenticated as a peer, not as the console, so a scope
    /// it writes for itself must never be admitted — that would let any peer run any tool here,
    /// destructive ones included, with `scopes: ["*"]` and `allow_destructive: true`.
    #[tokio::test]
    async fn a_mesh_peer_cannot_authorize_itself_with_a_token_scope() {
        let mut pair = pair("mesh-token-scope").await;
        let caller = pair._dialer.peer_id();

        let payload = json!({
            "id": 7,
            "token": {
                "token_id": "made-up",
                "device_id": pair.target.peer_id(),
                "scopes": ["*"],
                "allow_destructive": true,
                "expires_at_ms": null,
            },
            "mcp": state_call(104),
        });
        let reply = pair.exchange(payload).await;

        assert_eq!(
            reply["error"]["error"]["code"], -32001,
            "a self-written scope is a refusal, not a call: {reply}"
        );
        let lines = audit_lines(&pair.target.config);
        assert_eq!(lines[0]["decision"], "deny", "{lines:?}");
        assert_eq!(lines[0]["transport"], "mesh");
        assert_eq!(lines[0]["peer"], caller);
        assert_eq!(lines[1]["ok"], false);
        assert_eq!(lines[1]["error_kind"], "no_grant");
    }

    #[tokio::test]
    async fn a_mesh_grant_signed_by_another_key_is_refused_and_audited_deny() {
        let mut pair = pair("mesh-foreign-key").await;
        let other = SigningKey::from_bytes(&[0x77; 32]);
        let grant = signed_by(
            &other,
            Grant {
                device_id: pair.target.peer_id(),
                tools: vec!["state.get".to_owned()],
                expires_at_ms: now_ms() + 60_000,
                allow_destructive: false,
                nonce: String::new(),
            },
        );

        let reply = pair.ask(state_call(103), Some(grant)).await;

        assert_eq!(reply["error"]["error"]["code"], -32001, "{reply}");
        assert!(
            reply["error"]["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("signature")),
            "the refusal names the signature: {reply}"
        );
        let lines = audit_lines(&pair.target.config);
        assert_eq!(lines[0]["decision"], "deny");
        assert_eq!(lines[0]["transport"], "mesh");
        assert_eq!(lines[1]["error_kind"], "bad_grant");
    }

    #[tokio::test]
    async fn a_mesh_expired_grant_is_refused_and_audited_deny() {
        let mut pair = pair("mesh-expired").await;
        let grant = grant_for(
            &pair.target.peer_id(),
            &["state.get"],
            (now_ms() - 600_000) as u64,
        );

        let reply = pair.ask(state_call(104), Some(grant)).await;

        assert_eq!(reply["error"]["error"]["code"], -32001, "{reply}");
        assert!(
            reply["error"]["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("expired")),
            "{reply}"
        );
        let lines = audit_lines(&pair.target.config);
        assert_eq!(lines[0]["decision"], "deny");
        assert_eq!(lines[0]["transport"], "mesh");
        assert_eq!(lines[1]["error_kind"], "grant_expired");
    }

    #[tokio::test]
    async fn a_mesh_tool_outside_the_grant_never_reaches_the_capability() {
        let mut pair = pair("mesh-outside").await;
        let grant = grant_for(&pair.target.peer_id(), &["files.read"], now_ms() + 60_000);

        let reply = pair.ask(write_call(105, "denied.txt"), Some(grant)).await;

        assert_eq!(reply["error"]["error"]["code"], -32001, "{reply}");
        assert!(
            !pair.target.config.files_root.join("denied.txt").exists(),
            "a tool outside the grant must not reach the capability"
        );
        let lines = audit_lines(&pair.target.config);
        assert_eq!(lines[0]["decision"], "deny");
        assert_eq!(lines[0]["tool"], "files.write");
        assert_eq!(lines[0]["transport"], "mesh");
        assert_eq!(lines[1]["error_kind"], "tool_not_covered");
    }

    #[tokio::test]
    async fn a_dead_direct_candidate_falls_back_to_the_relay_and_still_answers() {
        let broker = start_broker().await;
        let relay = start_relay().await;
        let (mut dialer, target) = ordered(
            device("mesh-relay-a", &broker, Some(relay)),
            device("mesh-relay-b", &broker, Some(relay)),
        );
        dialer.link.punch_timeout_ms = DEAD_PUNCH_TIMEOUT_MS;

        let target_session = target.link.join().await.expect("the target joins the mesh");
        let _serving = tokio::spawn(serve(
            target_session,
            target.server.clone(),
            signing().verifying_key(),
            target.peer_id(),
        ));

        // The target still answers on its bound address, but the candidate it publishes is a UDP
        // socket that reads nothing: the dialer's QUIC handshake can never complete, so the punch
        // must time out and the path must fall to the relay floor.
        let dead = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("a dead UDP port");
        Rendezvous::new(
            target.link.identity.clone(),
            vec![Candidate::new(
                CandidateKind::Udp,
                dead.local_addr().expect("the dead address").to_string(),
                now_ms(),
            )],
            REGISTRATION_TTL_MS,
        )
        .register(&broker)
        .await
        .expect("the dead candidate is registered");

        let mut session = dialer.link.join().await.expect("the dialer joins the mesh");
        let mut channel = timeout(PATIENCE, session.connect(&target.peer_id(), now_ms()))
            .await
            .expect("the connect timed out")
            .expect("the relay channel opens");
        assert!(
            !channel.is_direct(),
            "the dead candidate must force the relay, not a lucky direct handshake"
        );
        assert_eq!(session.path(), PathState::Relayed);

        let grant = grant_for(&target.peer_id(), &["state.get"], now_ms() + 60_000);
        let request = Frame {
            kind: Kind::Req,
            id: 11,
            payload: json!({ "id": 106, "mcp": state_call(106), "grant": grant }),
        };
        channel
            .send_frame(&request)
            .await
            .expect("the request goes out over the relay");
        let reply = timeout(PATIENCE, channel.recv_frame())
            .await
            .expect("the relayed answer timed out")
            .expect("the relayed answer arrives");
        assert_eq!(reply.id, 11);
        assert_eq!(reply.payload["id"], 106);
        assert_eq!(
            reply.payload["result"]["id"], 106,
            "the call runs over the relay floor too: {}",
            reply.payload
        );

        let lines = audit_lines(&target.config);
        assert_eq!(lines[0]["decision"], "allow");
        assert_eq!(lines[0]["transport"], "mesh");
    }
}
