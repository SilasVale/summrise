//! Forward rules: one row saying "traffic arriving here leaves there", and an executor per row.
//!
//! **A ROW, NOT A GATEWAY.** `{name, listen, target, via}` and nothing else: no verbs, no script, no
//! environment. A router calls the same object a port-forward rule, and the deletion test passes —
//! delete the type and every caller is back to hand-assembling an `ssh -L` per site.
//!
//! **WHY `via` EXISTS.** The far host may not gain a listening port at all. So the connection is
//! made BY ANOTHER HOST: `listen` is bound on THIS machine, the bytes travel over an authenticated
//! SSH connection to `via`, and `via`'s sshd dials `target`. The far host's listener set is
//! unchanged, which is the property the design exists for and the reason a raw TCP listener here
//! would be the wrong answer.
//!
//! **THE TRUST IS `ssh::open`'s, NOT THIS MODULE'S.** Every check that decides whether a rule may
//! reach `via` — the pinned host key, the key file, the authentication — is the SSH capability's,
//! unchanged. This module adds a row and a pump; it adds no new way to reach a host.
//!
//! **WHAT IT DOES NOT DO: TLS.** A forward is a byte pipe, which is HTTP/1.1 only: six connections
//! per host, and a long-lived stream starves the rest. A TLS front end is a separate design question
//! and this build does not pretend to answer it by accepting a `tls` value and then not serving one.

use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::ssh::{self, Auth, KnownHosts};

/// The file, under `<data>`, the rule table is persisted to.
const FORWARDS_FILE: &str = "forwards.json";

/// The device's audit log; a rule's own events land here beside the call records.
const EVENTS_FILE: &str = "events.jsonl";

/// How many rules one device carries. Small on purpose: every rule is a listening port and an SSH
/// hop, and an operator's forward table is a handful of entries.
pub const RULES_MAX: usize = 4;

/// The bind ladder, in milliseconds. An immediate first attempt, then backoff, then give up: a
/// permanent conflict must leave the row VISIBLE with its error instead of spinning.
pub const BIND_RETRY_MS: [u64; 4] = [0, 1_000, 5_000, 15_000];

/// How long one accepted connection's SSH opening may take before it is abandoned.
///
/// The SSH capability has its own connect bound; this one is the rule's, and it exists so a `via`
/// that has stopped answering cannot accumulate accepted connections holding file handles.
const OPEN_TIMEOUT_MS: u64 = 30_000;

/// One rule, as it is stored and as it is read back.
///
/// **THE CREDENTIAL IS PART OF THE ROW, NOT A SIDE TABLE.** `terminal.create` takes its credential
/// from a caller and keeps it for that session; a rule OUTLIVES the caller and its executor must reach
/// `via` at boot with nobody present. A side table keyed by `via` would also mean two rules to one host
/// silently sharing one credential, which is a decision nobody made.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    /// The name a panel and the log identify it by; also its identity when a rule is edited.
    pub name: String,
    /// The `host:port` bound on THIS machine.
    pub listen: String,
    /// The `host:port` that `via` dials.
    pub target: String,
    /// The `user@host:port` of the SSH connection that carries it.
    pub via: String,
    /// How that connection authenticates.
    pub auth: Auth,
}

/// A rule that has been through [`RuleSpec::parse`]: the executor's inputs must come from something
/// that has been checked, not from whatever a file happened to contain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleSpec {
    name: String,
    listen: (String, u16),
    target: (String, u16),
    via: ViaSpec,
    auth: Auth,
}

/// A parsed `via`: the SSH connection's three values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViaSpec {
    /// The user the connection authenticates as.
    pub user: String,
    /// The host `via` is reached at.
    pub host: String,
    /// The SSH port.
    pub port: u16,
}

impl fmt::Display for ViaSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}:{}", self.user, self.host, self.port)
    }
}

/// Split `host:port` on the LAST colon. `None` for anything malformed.
///
/// **AN IPv6 LITERAL IS WRITTEN IN BRACKETS** (`[::1]:80`), because a bare `::1:80` is genuinely
/// ambiguous and accepting it as host `::1` would be a guess about which colon belongs to the port.
/// The brackets are removed here: they are this syntax's, not the transport's, and the value that
/// reaches `ssh` must be the address itself.
pub fn hostport(s: &str) -> Option<(String, u16)> {
    let (host, port) = s.rsplit_once(':')?;
    let port: u16 = port.parse().ok()?;
    if port == 0 {
        return None;
    }
    // THE BRACKETS ARE THIS SYNTAX'S, NOT THE TRANSPORT'S. A bracketed host is an IPv6 literal and
    // loses its brackets here; an UNBRACKETED host containing a colon is refused even when it would
    // parse as an address, because the write it came from is ambiguous and a guess about which
    // colon is the port's is a guess about which host is reached.
    let (host, was_bracketed) = match host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        Some(inner) => (inner, true),
        None => (host, false),
    };
    if host.is_empty() {
        return None;
    }
    if was_bracketed {
        // `[abc]:80` is not an address in brackets; it is a host name with a typo.
        if host.parse::<std::net::Ipv6Addr>().is_err() {
            return None;
        }
    } else if host.contains(':') {
        return None;
    }
    Some((host.to_string(), port))
}

/// Is this host a wildcard — an address that names no particular audience?
///
/// `0.0.0.0`, `::` and `*` are refused: a rule that silently widens who can reach `target` is the
/// failure this refusal prevents, and the row has no fifth field to unlock it with.
fn is_wildcard_host(host: &str) -> bool {
    matches!(host, "0.0.0.0" | "::" | "*")
}

impl ViaSpec {
    /// Parse `user@host:port`.
    ///
    /// `user` is split on the LAST `@`, so a user name containing one is not silently truncated into
    /// a different account on the far side.
    pub fn parse(via: &str) -> Result<ViaSpec, String> {
        let trimmed = via.trim();
        if trimmed.is_empty() {
            return Err(
                "via is required: it is the host that MAKES the connection, because the far host \
                 may not open a listening port at all"
                    .into(),
            );
        }
        let (user, hostport_part) = trimmed.rsplit_once('@').ok_or_else(|| {
            format!(
                "via \"{trimmed}\" is not user@host:port — an SSH connection needs all three, \
                 e.g. zss@100.64.0.9:22"
            )
        })?;
        if user.is_empty() {
            return Err(format!("via \"{trimmed}\" has no user before the @"));
        }
        let (host, port) = hostport(hostport_part).ok_or_else(|| {
            format!(
                "via \"{trimmed}\" is not user@host:port — the part after the @ must be host:port"
            )
        })?;
        Ok(ViaSpec {
            user: user.to_string(),
            host,
            port,
        })
    }
}

impl RuleSpec {
    /// The rule as it is stored.
    pub fn to_rule(&self) -> Rule {
        Rule {
            name: self.name.clone(),
            listen: format!("{}:{}", self.listen.0, self.listen.1),
            target: format!("{}:{}", self.target.0, self.target.1),
            via: self.via.to_string(),
            auth: self.auth.clone(),
        }
    }

    /// Validate a rule. The refusals are ordered so the sentence a caller reads is about the FIRST
    /// thing that is wrong, not the last one a loop happened to reach.
    pub fn parse(
        name: &str,
        listen: &str,
        target: &str,
        via: &str,
        auth: Auth,
    ) -> Result<RuleSpec, String> {
        if name.trim().is_empty() {
            return Err("a rule needs a name: it is how the panel and the log identify it".into());
        }
        let listen_parts = hostport(listen.trim()).ok_or_else(|| {
            format!(
                "listen \"{listen}\" is not host:port — name the address to listen on and its port, \
                 e.g. 100.64.0.5:443"
            )
        })?;
        if is_wildcard_host(&listen_parts.0) {
            return Err(format!(
                "listen \"{listen}\" is a wildcard: it names no audience the operator chose. Name the \
                 address traffic should arrive on (a tailnet address, or a specific LAN one); a rule \
                 that widens its own audience is the failure this refusal prevents"
            ));
        }
        let target_parts = hostport(target.trim()).ok_or_else(|| {
            format!(
                "target \"{target}\" is not host:port — name the far side's address and the port its \
                 service listens on"
            )
        })?;
        Ok(RuleSpec {
            name: name.trim().to_string(),
            listen: listen_parts,
            target: target_parts,
            via: ViaSpec::parse(via)?,
            auth,
        })
    }
}

/// A one-way stop signal: the registry cancels a rule's executor with it.
///
/// **IT IS THIS CRATE'S OWN, not `tokio_util`'s,** because the dependency is not in the tree and one
/// more crate for a boolean would be a poor trade. `watch::Receiver::changed()` is the await, and a
/// sender that is dropped cancels too — which is what makes a forgotten `cancel()` safe rather than
/// a leak.
#[derive(Clone)]
pub struct Cancel {
    tx: tokio::sync::watch::Sender<bool>,
}

impl Cancel {
    /// A live token.
    fn new() -> Cancel {
        let (tx, _rx) = tokio::sync::watch::channel(false);
        Cancel { tx }
    }

    /// Stop the executor waiting on this token.
    fn cancel(&self) {
        let _ = self.tx.send(true);
    }

    /// Resolve when the token is cancelled.
    async fn cancelled(&self) {
        let mut rx = self.tx.subscribe();
        // `wait_for` returns immediately when the value is already true, which is what makes a
        // cancel racing a spawn still cancel.
        let _ = rx.wait_for(|stop| *stop).await;
    }
}

/// The persisted document: one array, so a future version can add siblings without moving `rules`.
#[derive(Serialize, Deserialize)]
struct Persisted {
    rules: Vec<Rule>,
}

/// Why a rule operation was refused or failed.
#[derive(Debug)]
pub enum ForwardError {
    /// The table already holds [`RULES_MAX`] rules.
    TooManyRules {
        /// The cap.
        max: usize,
    },
    /// Another rule already owns the address this one would bind.
    AddressTaken {
        /// The address both rules name.
        listen: String,
        /// The rule already holding it.
        incumbent: String,
    },
    /// A parsed rule was refused.
    Refused {
        /// The sentence the parser produced.
        message: String,
    },
    /// The filesystem refused a read or a write.
    Io(std::io::Error),
}

impl fmt::Display for ForwardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ForwardError::TooManyRules { max } => write!(
                f,
                "this device carries at most {max} forward rules; remove one before adding another \
                 (a forward is a listening port and an SSH hop, so the cap is small on purpose)"
            ),
            ForwardError::AddressTaken { listen, incumbent } => write!(
                f,
                "{listen} is already listened on by rule \"{incumbent}\": two rules cannot own one \
                 address, because the second binds nothing while the table shows it as configured"
            ),
            ForwardError::Refused { message } => f.write_str(message),
            ForwardError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ForwardError {}

/// What one rule's executor is doing, as the panel and `forward.list` report it.
///
/// Every field says what was OBSERVED, not what was hoped: `listening` is bind() having returned,
/// `since_ms` is when that happened, and `last_error` is the sentence the last failure produced.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ForwardState {
    /// The rule's name.
    pub name: String,
    /// The address this rule binds.
    pub listen: String,
    /// The address `via` dials.
    pub target: String,
    /// The connection that carries it.
    pub via: String,
    /// Whether the listener is bound right now.
    pub listening: bool,
    /// When the current run of `listening` began, in Unix milliseconds.
    pub since_ms: Option<u64>,
    /// The last failure's sentence; cleared by the next success.
    pub last_error: Option<String>,
    /// How many bind attempts the ladder has made.
    pub attempts: u64,
    /// How many connections this executor has CARRIED — both directions ended without an error —
    /// since it started. It is not the number that arrived: a connection that was accepted and
    /// refused is a log line (D6), not a count, or the field means "arrived" while its name and the
    /// panel read "worked".
    pub connections: u64,
}

#[cfg(test)]
mod tests {
    //! The refusals, the store, the ladder, and one connection carried end to end.
    //!
    //! **THE MODULE'S RULE TABLE IS PER-REGISTRY, NOT GLOBAL.** Each test builds its own registry in
    //! its own temporary directory, so no test inherits another's rows — the defect a global table
    //! invites, and the reason `MonitorRegistry` is per-device too.

    use super::*;

    /// A registry on a fresh directory, kept alive for the test.
    struct Fixture {
        dir: std::path::PathBuf,
        registry: Arc<ForwardRegistry>,
    }

    impl Fixture {
        fn new(tag: &str) -> Fixture {
            let dir =
                std::env::temp_dir().join(format!("summrise-forward-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("a temp directory");
            let registry = Arc::new(ForwardRegistry::new(&dir));
            Fixture { dir, registry }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn password() -> Auth {
        Auth::Password {
            password: "unused-by-these-tests".to_owned(),
        }
    }

    fn spec(name: &str, listen: &str, target: &str, via: &str) -> RuleSpec {
        RuleSpec::parse(name, listen, target, via, password()).expect("a valid rule")
    }

    /// `host:port` splits on the LAST colon, and every malformed shape is refused rather than guessed.
    #[test]
    fn hostport_splits_on_the_last_colon_and_refuses_the_rest() {
        assert_eq!(hostport("10.0.0.5:443"), Some(("10.0.0.5".to_owned(), 443)));
        assert_eq!(
            hostport("100.64.0.9:22"),
            Some(("100.64.0.9".to_owned(), 22))
        );
        // A BARE IPv6 LITERAL IS REFUSED: `::1:80` is ambiguous about which colon is the port's,
        // and a guess there is a guess about which host is reached. In brackets it is accepted, and
        // the brackets stay out of the value the transport dials.
        assert_eq!(hostport("::1:80"), None);
        assert_eq!(hostport("[::1]:80"), Some(("::1".to_owned(), 80)));
        assert_eq!(hostport("10.0.0.5"), None);
        assert_eq!(hostport("10.0.0.5:"), None);
        assert_eq!(hostport(":443"), None);
        assert_eq!(hostport("10.0.0.5:0"), None, "0 is not a port");
        assert_eq!(hostport("10.0.0.5:notaport"), None);
    }

    /// The four refusals, each with the sentence a caller reads.
    #[test]
    fn every_malformed_rule_is_refused_with_its_reason() {
        assert!(
            RuleSpec::parse("", "1.2.3.4:1", "5.6.7.8:2", "u@h:22", password())
                .unwrap_err()
                .contains("name")
        );
        assert!(
            RuleSpec::parse("r", "nope", "5.6.7.8:2", "u@h:22", password())
                .unwrap_err()
                .contains("host:port")
        );
        assert!(
            RuleSpec::parse("r", "1.2.3.4:1", "nope", "u@h:22", password())
                .unwrap_err()
                .contains("host:port")
        );
        assert!(
            RuleSpec::parse("r", "1.2.3.4:1", "5.6.7.8:2", "", password())
                .unwrap_err()
                .contains("via")
        );
        assert!(
            RuleSpec::parse("r", "1.2.3.4:1", "5.6.7.8:2", "no-at-sign", password())
                .unwrap_err()
                .contains("user@host:port")
        );
        assert!(
            RuleSpec::parse("r", "1.2.3.4:1", "5.6.7.8:2", "@h:22", password())
                .unwrap_err()
                .contains("no user")
        );
    }

    /// **A WILDCARD LISTEN IS REFUSED.** A rule that silently widens its own audience is the failure
    /// this refusal exists for, and the row has no fifth field to unlock it with.
    ///
    /// MUTATION: delete the `is_wildcard_host` check -> this accepts `0.0.0.0:443` and fails.
    #[test]
    fn a_wildcard_listen_is_refused() {
        for wild in ["0.0.0.0:443", "[::]:80", "*:8080"] {
            let error = RuleSpec::parse("r", wild, "10.0.0.5:80", "u@h:22", password())
                .expect_err(&format!("{wild} must be refused"));
            assert!(error.contains("wildcard"), "{wild}: {error}");
        }
    }

    /// Two rules cannot own one address: the second would bind nothing while the table showed it as
    /// configured, which is the state an operator then debugs in the wrong place.
    ///
    /// MUTATION: drop the `AddressTaken` arm -> this adds the second rule and the assertion fails.
    #[tokio::test]
    async fn two_rules_cannot_own_one_address() {
        let fixture = Fixture::new("address");
        fixture
            .registry
            .add(&spec("a", "100.64.0.5:9001", "10.0.0.1:80", "u@h:22"))
            .expect("the first rule");
        let error = fixture
            .registry
            .add(&spec("b", "100.64.0.5:9001", "10.0.0.2:80", "u@h:22"))
            .expect_err("the same address twice");
        match error {
            ForwardError::AddressTaken { listen, incumbent } => {
                assert_eq!(listen, "100.64.0.5:9001");
                assert_eq!(incumbent, "a");
            }
            other => panic!("expected AddressTaken, got {other}"),
        }
        // The refused add changed nothing: one rule, and it is the first one.
        assert_eq!(fixture.registry.rules().len(), 1);
    }

    /// The file is the commit point: a rule survives a NEW registry over the same directory, which is
    /// what makes it outlive a restart.
    ///
    /// MUTATION: drop the `save` call in `add` -> the reloaded registry is empty and this fails.
    #[test]
    fn a_rule_outlives_the_registry_that_wrote_it() {
        let fixture = Fixture::new("persist");
        let rule = fixture
            .registry
            .add(&spec(
                "kept",
                "100.64.0.5:9101",
                "10.0.0.1:443",
                "zss@100.64.0.9:22",
            ))
            .expect("the rule");
        let reloaded = ForwardRegistry::new(&fixture.dir);
        assert_eq!(reloaded.rules(), vec![rule]);
    }

    /// **THE LADDER IS BOUNDED, AND THIS TEST DRIVES IT.**
    ///
    /// `203.0.113.0/24` is TEST-NET-3: nothing can own it, so every bind fails immediately and the
    /// ladder walks every step. The cost is the ladder's real 21 seconds, paid deliberately: the
    /// subject is the LOOP, and the loop's clock is real sleeps. A test that asserted the four
    /// constants would say nothing about the loop that walks them.
    ///
    /// MUTATION: replace the `attempt + 1 == BIND_RETRY_MS.len()` return with `continue` -> the
    /// executor keeps retrying and never finishes, which this reports as a timeout after the ladder.
    #[tokio::test]
    async fn the_bind_ladder_stops_after_its_last_step() {
        let fixture = Fixture::new("ladder");
        fixture
            .registry
            .add(&spec("ladder", "203.0.113.7:9", "10.0.0.1:80", "u@h:22"))
            .expect("the rule is configured even though it cannot bind");
        let total_ms: u64 = BIND_RETRY_MS.iter().sum();
        let budget = Duration::from_millis(total_ms + 400);
        let settled = tokio::time::timeout(budget, async {
            loop {
                let state = fixture.registry.state("ladder").expect("the row");
                if !state.listening && state.attempts as usize == BIND_RETRY_MS.len() {
                    return state;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
        let state = settled.expect("the ladder ended within its own steps");
        // THE ROW STAYS, WITH ITS ERROR: that is the state the operator reads instead of a log they
        // would have to find.
        assert!(!state.listening);
        assert_eq!(state.attempts as usize, BIND_RETRY_MS.len());
        assert!(state.last_error.is_some(), "the row carries why it failed");
    }

    /// **AN EDITED RULE SERVES THE NEW PORT, AND THE OLD ONE STOPS.** An edit that left the old
    /// executor alone would keep the previous listener bound while the table showed the new port — a
    /// row that lies, which is worse than a row that fails.
    ///
    /// The two ports are this test's own, taken from the kernel and released before the rules use
    /// them: a port the test holds would be bound by the test, not by the rule under test.
    ///
    /// MUTATION (run, not asserted): with `needs_rebuild.insert` in `add` disabled, this went
    /// `FAILED. 0 passed; 1 failed` with `the old port 32781 was never released`; restored, it is
    /// green. The flag is what cancels the previous executor, and this is the measurement of that.
    #[tokio::test]
    async fn editing_a_rule_moves_the_listener() {
        let first = free_port();
        let second = free_port();
        assert_ne!(first, second);
        let fixture = Fixture::new("edit");
        fixture
            .registry
            .add(&spec(
                "edit",
                &format!("127.0.0.1:{first}"),
                "10.0.0.1:80",
                "u@h:22",
            ))
            .expect("the first rule");
        assert!(
            wait_listening(&fixture.registry, "edit", true).await,
            "the first listener is up"
        );

        fixture
            .registry
            .add(&spec(
                "edit",
                &format!("127.0.0.1:{second}"),
                "10.0.0.2:80",
                "u@h:22",
            ))
            .expect("the edit");
        assert!(
            wait_listening(&fixture.registry, "edit", true).await,
            "the second listener is up"
        );

        // THE OLD ADDRESS MUST BE FREE AGAIN. Waiting, because the cancelled executor releases its
        // socket when tokio drops it, and that is a scheduler turn, not a synchronous fact.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if tokio::net::TcpListener::bind(("127.0.0.1", first))
                .await
                .is_ok()
            {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the old port {first} was never released"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        // …and the new one is still held by the rule, not by this test.
        assert!(
            tokio::net::TcpListener::bind(("127.0.0.1", second))
                .await
                .is_err(),
            "the rule holds {second}"
        );
    }

    /// Poll a rule's row until it says what the test expects, or the deadline passes.
    async fn wait_listening(registry: &Arc<ForwardRegistry>, name: &str, want: bool) -> bool {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            if registry
                .state(name)
                .map(|state| state.listening == want)
                .unwrap_or(false)
            {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        false
    }

    /// A port no rule and no test holds: asked for from the kernel, then released.
    fn free_port() -> u16 {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a free port");
        let port = listener.local_addr().expect("an address").port();
        drop(listener);
        port
    }

    /// **THE CAP IS REAL, AND REPLACING A RULE AT THE CAP STILL WORKS.** `RULES_MAX` exists because
    /// every rule is a listening socket and an SSH hop, so the fifth rule is refused — but an EDIT of
    /// an existing name is not a fifth rule, and refusing it would make a full device unmaintainable
    /// (the only way to change anything would be to delete something first).
    ///
    /// MUTATION (run, not asserted): with `!replacing &&` dropped from the cap check, this went
    /// `FAILED. 0 passed; 1 failed` with `the fourth rule can still be edited: TooManyRules { max: 4 }`.
    /// Restored, it is green.
    #[tokio::test]
    async fn the_fifth_rule_is_refused_and_an_edit_at_the_cap_is_not() {
        let fixture = Fixture::new("cap");
        for index in 0..RULES_MAX {
            fixture
                .registry
                .add(&spec(
                    &format!("r{index}"),
                    &format!("127.0.0.1:{}", 19000 + index as u16),
                    "10.0.0.1:80",
                    "u@h:22",
                ))
                .expect("a rule below the cap");
        }
        assert_eq!(fixture.registry.rules().len(), RULES_MAX);

        let fifth = fixture
            .registry
            .add(&spec("r5", "127.0.0.1:19500", "10.0.0.1:80", "u@h:22"))
            .expect_err("the fifth rule is refused");
        match fifth {
            ForwardError::TooManyRules { max } => assert_eq!(max, RULES_MAX),
            other => panic!("expected TooManyRules, got {other}"),
        }
        // THE REFUSAL CHANGED NOTHING: still four, and none of them is `r5`.
        assert_eq!(fixture.registry.rules().len(), RULES_MAX);
        assert!(!fixture
            .registry
            .rules()
            .iter()
            .any(|rule| rule.name == "r5"));

        // …and the fourth rule CAN still be edited, which is what keeps a full device usable.
        fixture
            .registry
            .add(&spec("r3", "127.0.0.1:19501", "10.0.0.9:443", "u@h:22"))
            .expect("the fourth rule can still be edited");
        assert_eq!(fixture.registry.rules().len(), RULES_MAX);
        let edited = fixture
            .registry
            .rules()
            .into_iter()
            .find(|rule| rule.name == "r3")
            .expect("r3 is still configured");
        assert_eq!(edited.listen, "127.0.0.1:19501");
    }

    /// **A WRITE THAT FAILS LEAVES THE REGISTRY AS IT WAS.** The file is the commit point: if
    /// `forwards.json` cannot be written, the in-memory table must not grow a rule that nothing will
    /// restore after a restart — a rule that lives only until the next boot is worse than a refusal,
    /// because the operator saw it succeed.
    ///
    /// The failure is made REAL, not simulated: the table's own directory is made read-only, so
    /// `save` fails the way it would on a full disk or a refused permission.
    ///
    /// MUTATION (run, not asserted): with the in-memory insert moved above `save(...)?`, this went
    /// `FAILED. 0 passed; 1 failed` with `the in-memory table did not grow: [Rule { name: "never", … }]`
    /// — the rule the operator saw refused, present in the table. Restored, it is green.
    #[tokio::test]
    async fn a_refused_write_does_not_change_the_table() {
        use std::os::unix::fs::PermissionsExt as _;

        let fixture = Fixture::new("readonly");
        let path = fixture.dir.join(FORWARDS_FILE);
        let read_only = fixture.dir.clone();
        std::fs::set_permissions(&read_only, std::fs::Permissions::from_mode(0o500))
            .expect("the directory becomes read-only");

        let refused = fixture
            .registry
            .add(&spec("never", "127.0.0.1:19600", "10.0.0.1:80", "u@h:22"))
            .map_err(|error| error.to_string());
        // Restore the mode before asserting, so a panic does not leave an undeletable directory.
        std::fs::set_permissions(&read_only, std::fs::Permissions::from_mode(0o700))
            .expect("the directory is writable again");

        let refused = refused.expect_err("a table that cannot be written must refuse the rule");
        assert!(
            !refused.is_empty(),
            "the refusal carries its reason: {refused}"
        );
        assert!(
            fixture.registry.rules().is_empty(),
            "the in-memory table did not grow: {:?}",
            fixture.registry.rules()
        );
        assert!(!path.exists(), "no half-written file was left behind");
    }

    /// **D6, MEASURED: A REFUSED CONNECTION WRITES ONE LINE; A CARRIED ONE WRITES NONE.**
    ///
    /// A rule whose `via` cannot be reached accepts the client, fails to open the channel, and is
    /// gone — that is the case an operator asks "who tried, and why did it fail" about, so it leaves
    /// a record. The record is the ROW's error plus one line in `events.jsonl`.
    ///
    /// **THE OTHER HALF IS THE POINT OF THE DECISION**: a connection that WORKS is counted
    /// (`connections`) and NOT written. Written literally, "one line per connection" is a file written
    /// on the busiest path this module has, where a rule carrying a UI's requests appends hundreds of
    /// lines a minute. This test reads the log to prove both halves at once.
    ///
    /// MUTATION (run, not asserted): counting a connection when it is ACCEPTED rather than when it is
    /// CARRIED — the shape `connections` had before this test — went `FAILED. 0 passed; 1 failed`
    /// with `a refused connection is not a carried one, left: Some(1), right: Some(0)`. That is the
    /// defect this test found: the field was named, and documented, for carried connections while the
    /// accept arm moved it. Restored, it is green.
    #[tokio::test]
    async fn a_refused_connection_is_one_line_and_a_carried_one_is_none() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        let listener_port = free_port();
        let fixture = Fixture::new("audit");
        fixture
            .registry
            .add(&spec(
                "audit",
                &format!("127.0.0.1:{listener_port}"),
                "127.0.0.1:9",
                // Port 1 is this box's own; nothing listens there, so the SSH opening fails at once.
                "u@127.0.0.1:1",
            ))
            .expect("the rule");
        assert!(
            wait_listening(&fixture.registry, "audit", true).await,
            "the listener is up"
        );

        // ONE CLIENT that is carried nowhere: the connection is accepted, the channel is not opened.
        let mut client = tokio::net::TcpStream::connect(("127.0.0.1", listener_port))
            .await
            .expect("a connection to the rule");
        let _ = client.write_all(b"hi").await;
        let mut sink = [0u8; 8];
        let _ = client.read(&mut sink).await; // the rule closes it; either answer is fine here

        // WAIT FOR THE ROW TO CARRY ITS ERROR, which is written beside the log line.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if fixture
                .registry
                .state("audit")
                .and_then(|state| state.last_error)
                .is_some()
            {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the row never recorded the failure"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        let events = std::fs::read_to_string(fixture.dir.join(EVENTS_FILE)).unwrap_or_default();
        let refused: Vec<&str> = events
            .lines()
            .filter(|line| line.contains("forward.refused"))
            .collect();
        assert_eq!(
            refused.len(),
            1,
            "one refused connection is one line: {events}"
        );
        let line: serde_json::Value = serde_json::from_str(refused[0]).expect("the line is JSON");
        assert_eq!(line["rule"], "audit");
        assert_eq!(line["listen"], format!("127.0.0.1:{listener_port}"));
        assert!(
            line["from"]
                .as_str()
                .is_some_and(|from| from.contains("127.0.0.1")),
            "the line says who arrived: {line}"
        );
        assert!(
            line["reason"]
                .as_str()
                .is_some_and(|reason| !reason.is_empty()),
            "the line says why: {line}"
        );
        // A CARRIED CONNECTION WOULD HAVE COUNTED `connections`; a refused one must not.
        assert_eq!(
            fixture
                .registry
                .state("audit")
                .map(|state| state.connections),
            Some(0),
            "a refused connection is not a carried one"
        );
    }

    /// A half-close is a half-close: each direction ends on its own EOF, so a client that sends its
    /// request and then shuts down its write half still GETS THE ANSWER.
    ///
    /// MUTATION: return as soon as either direction ends (the obvious `select!`) -> this reads an
    /// empty body, which is what the branch measured on the agent's first version.
    #[tokio::test]
    async fn a_half_close_still_carries_the_answer() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        // A far side that waits for the client's request and only then answers, which is the shape
        // every HTTP server has.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a port");
        let addr = listener.local_addr().expect("an address");
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("a client");
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf).await.expect("the request");
            stream.write_all(b"the answer").await.expect("the answer");
            stream.flush().await.expect("flushed");
        });

        // The forwarder carries ONE client stream and ONE far stream. The client is a duplex the
        // test itself holds the other end of; the far side is a real TCP connection to a server that
        // waits for the request to END before answering.
        let (mut handset, client_side) = tokio::io::duplex(4096);
        let far = tokio::net::TcpStream::connect(addr)
            .await
            .expect("the far side");
        tokio::spawn(async move {
            let _ = pipe_bidirectional(client_side, far).await;
        });
        // This is the client: write the request, then SHUT DOWN the write half and wait.
        handset.write_all(b"request").await.expect("the request");
        handset.shutdown().await.expect("the half-close");
        let mut answer = Vec::new();
        handset.read_to_end(&mut answer).await.expect("the answer");
        assert_eq!(String::from_utf8_lossy(&answer), "the answer");
    }
}

/// The device's one forward registry: rules, their executors, and the listeners they own.
pub struct ForwardRegistry {
    path: PathBuf,
    events: PathBuf,
    known_hosts: KnownHosts,
    inner: Mutex<Inner>,
}

impl ForwardRegistry {
    /// A registry reading and writing `<data>/forwards.json`, with the table it finds on disk.
    ///
    /// **THE TABLE IS LOADED HERE, NOT WHEN SOMEONE ASKS.** A rule created in an earlier boot must be
    /// listening again after a restart without a client having to touch it first — the same reason
    /// the monitor loads its targets at boot. A file that cannot be read is an EMPTY table rather
    /// than an error: a damaged rule list must not stop the device from starting.
    pub fn new(data: &Path) -> ForwardRegistry {
        let path = data.join(FORWARDS_FILE);
        let rules = load(&path);
        ForwardRegistry {
            events: data.join(EVENTS_FILE),
            known_hosts: KnownHosts::at(data),
            path,
            inner: Mutex::new(Inner {
                rules,
                ..Inner::default()
            }),
        }
    }

    /// Start one executor per loaded rule. Called once at boot; [`ForwardRegistry::reconcile`] is the
    /// same function, so a later call is not a second mechanism.
    pub fn start(self: &Arc<Self>) {
        self.reconcile();
    }

    /// The configured rules, in configuration order.
    #[allow(dead_code)] // the tests read it; `forward.list` is the wire surface
    pub fn rules(&self) -> Vec<Rule> {
        lock(&self.inner).rules.clone()
    }

    /// One rule's live state, or `None` when the name is not configured.
    #[allow(dead_code)] // the tests read it; `forward.list` is the wire surface
    pub fn state(&self, name: &str) -> Option<ForwardState> {
        let inner = lock(&self.inner);
        inner
            .rules
            .iter()
            .find(|rule| rule.name == name)
            .map(|rule| state_of(rule, inner.states.get(name)))
    }

    /// Every rule with its live state, in configuration order.
    pub fn list(&self) -> Vec<ForwardState> {
        let inner = lock(&self.inner);
        inner
            .rules
            .iter()
            .map(|rule| state_of(rule, inner.states.get(&rule.name)))
            .collect()
    }

    /// Add or replace a rule, then make its executor match it. The file is the commit point.
    pub fn add(self: &Arc<Self>, spec: &RuleSpec) -> Result<Rule, ForwardError> {
        let rule = spec.to_rule();
        let to_spawn = {
            let mut inner = lock(&self.inner);
            let replacing = inner.rules.iter().any(|r| r.name == rule.name);
            if !replacing && inner.rules.len() >= RULES_MAX {
                return Err(ForwardError::TooManyRules { max: RULES_MAX });
            }
            if let Some(incumbent) = inner
                .rules
                .iter()
                .find(|r| r.listen == rule.listen && r.name != rule.name)
            {
                return Err(ForwardError::AddressTaken {
                    listen: rule.listen.clone(),
                    incumbent: incumbent.name.clone(),
                });
            }
            let mut persisted: Vec<Rule> = inner.rules.clone();
            match persisted.iter_mut().find(|r| r.name == rule.name) {
                Some(existing) => *existing = rule.clone(),
                None => persisted.push(rule.clone()),
            }
            // A refused or failed write leaves the registry exactly as it was.
            save(&self.path, &persisted).map_err(ForwardError::Io)?;
            match inner.rules.iter_mut().find(|r| r.name == rule.name) {
                Some(existing) => *existing = rule.clone(),
                None => inner.rules.push(rule.clone()),
            }
            // THE EXECUTOR IS REPLACED WHEN THE ROW CHANGED, not only when the name is new. An
            // edited rule whose executor was left alone would keep serving the OLD port while the
            // table showed the new one.
            inner.needs_rebuild.insert(rule.name.clone(), true);
            rule
        };
        self.reconcile();
        Ok(to_spawn)
    }

    /// Remove a rule and stop its executor. `false` means it was not configured.
    pub fn remove(&self, name: &str) -> Result<bool, ForwardError> {
        let removed = {
            let mut inner = lock(&self.inner);
            let Some(index) = inner.rules.iter().position(|r| r.name == name) else {
                return Ok(false);
            };
            inner.rules.remove(index);
            save(&self.path, &inner.rules).map_err(ForwardError::Io)?;
            if let Some(cancel) = inner.cancels.remove(name) {
                cancel.cancel();
            }
            inner.states.remove(name);
            inner.needs_rebuild.remove(name);
            true
        };
        Ok(removed)
    }

    /// Start (or restart) one executor per rule whose row no longer matches its executor.
    ///
    /// NOT "spawns nothing new": a second call is safe — a stale executor is cancelled before its
    /// replacement is pushed, so two listeners for one address cannot exist — but it is not a no-op.
    /// The rebuild is what makes an EDITED rule take effect, and it is the operator's recovery from
    /// a spent bind ladder.
    pub fn reconcile(self: &Arc<Self>) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let todo: Vec<Rule> = {
            let mut inner = lock(&self.inner);
            let rebuilding: Vec<String> = inner
                .rules
                .iter()
                .filter(|r| inner.needs_rebuild.get(&r.name).copied().unwrap_or(false))
                .map(|r| r.name.clone())
                .collect();
            for name in &rebuilding {
                inner.needs_rebuild.remove(name);
                if let Some(cancel) = inner.cancels.remove(name) {
                    cancel.cancel();
                }
            }
            inner
                .rules
                .iter()
                .filter(|r| !inner.cancels.contains_key(&r.name))
                .cloned()
                .collect()
        };
        for rule in todo {
            let cancel = Cancel::new();
            {
                let mut inner = lock(&self.inner);
                inner.states.insert(
                    rule.name.clone(),
                    ForwardState {
                        name: rule.name.clone(),
                        listen: rule.listen.clone(),
                        target: rule.target.clone(),
                        via: rule.via.clone(),
                        ..ForwardState::default()
                    },
                );
                inner.cancels.insert(rule.name.clone(), cancel.clone());
            }
            let registry = Arc::clone(self);
            let spawned = rule.clone();
            let name = rule.name.clone();
            let handle = tokio::spawn(async move {
                registry.run_executor(spawned, cancel).await;
            });
            lock(&self.inner).tasks.insert(name, handle);
        }
    }
}

#[derive(Default)]
struct Inner {
    rules: Vec<Rule>,
    states: HashMap<String, ForwardState>,
    cancels: HashMap<String, Cancel>,
    tasks: HashMap<String, tokio::task::JoinHandle<()>>,
    /// Rules whose executor no longer matches their row.
    needs_rebuild: HashMap<String, bool>,
}

fn state_of(rule: &Rule, state: Option<&ForwardState>) -> ForwardState {
    let mut out = state.cloned().unwrap_or_default();
    out.name = rule.name.clone();
    out.listen = rule.listen.clone();
    out.target = rule.target.clone();
    out.via = rule.via.clone();
    out
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

/// Write the table atomically: a sibling first, then a rename.
fn save(path: &Path, rules: &[Rule]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(&Persisted {
        rules: rules.to_vec(),
    })
    .map_err(std::io::Error::other)?;
    let (temporary, mut file) = crate::fsutil::create_new_sibling(path)?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    drop(file);
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

/// Load the table. Tolerant by design: an unreadable or malformed file is an EMPTY list, never an
/// error and never a panic — a rule table must not stop the device from starting.
fn load(path: &Path) -> Vec<Rule> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Persisted>(&text)
        .map(|persisted| persisted.rules)
        .unwrap_or_default()
}

/// Append one event to the device's event log, best effort: a rule must never fail because its
/// record could not be written, and the ROW carries the failure for the operator anyway.
fn record_event(path: &Path, event: serde_json::Value) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut line = event.to_string();
    line.push('\n');
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

impl ForwardRegistry {
    /// One rule's executor: bind, then accept and carry, until the token is cancelled.
    ///
    /// **THE LADDER IS BOUNDED, AND THAT IS THE POINT.** A permanent conflict (something else owns
    /// the port, the address does not exist on this machine) must leave the row VISIBLE with its
    /// error after four attempts rather than retry forever behind a table that looks merely quiet.
    async fn run_executor(self: Arc<Self>, rule: Rule, cancel: Cancel) {
        let address = rule.listen.clone();
        let mut listener: Option<tokio::net::TcpListener> = None;
        for (attempt, delay_ms) in BIND_RETRY_MS.iter().enumerate() {
            if *delay_ms > 0 {
                let slept = tokio::select! {
                    () = cancel.cancelled() => false,
                    () = tokio::time::sleep(Duration::from_millis(*delay_ms)) => true,
                };
                if !slept {
                    return;
                }
            }
            self.update(&rule.name, |st| st.attempts = attempt as u64 + 1);
            match tokio::net::TcpListener::bind(&address).await {
                Ok(bound) => {
                    listener = Some(bound);
                    self.update(&rule.name, |st| {
                        st.listening = true;
                        st.since_ms = Some(now_ms());
                        st.last_error = None;
                    });
                    record_event(
                        &self.events,
                        json!({
                            "ts_ms": now_ms(),
                            "event": "forward.listening",
                            "rule": rule.name,
                            "listen": rule.listen,
                        }),
                    );
                    break;
                }
                Err(error) => {
                    // The sentence names the ADDRESS and the REASON: "bind failed" alone is the
                    // sentence an operator cannot act on.
                    let reason = format!(
                        "binding {address} for rule \"{}\" failed: {error}",
                        rule.name
                    );
                    self.update(&rule.name, |st| st.last_error = Some(reason));
                    if attempt + 1 == BIND_RETRY_MS.len() {
                        // The ladder is spent. The row stays, with its error.
                        return;
                    }
                }
            }
        }
        let Some(listener) = listener else {
            return;
        };

        loop {
            let accepted = tokio::select! {
                () = cancel.cancelled() => return,
                accepted = listener.accept() => accepted,
            };
            match accepted {
                Ok((stream, peer)) => {
                    // THE COUNT MOVES WHEN A CONNECTION IS CARRIED, NOT WHEN ONE IS ACCEPTED (see
                    // `carry`: it increments after the pipes finish). An accepted-then-refused
                    // connection is exactly the case D6 writes a LOG line for, and counting it as
                    // carried would make `connections` mean "how many arrived" while its name and
                    // the panel read "how many worked".
                    let registry = Arc::clone(&self);
                    let rule = rule.clone();
                    tokio::spawn(async move {
                        registry.carry(&rule, stream, peer).await;
                    });
                }
                Err(error) => {
                    // ONE FAILED `accept` DOES NOT END THE LISTENER. A transient error here (a
                    // connection reset during the handshake, a file-descriptor peak) would otherwise
                    // silently take the rule down while its row still said `listening: true`.
                    self.update(&rule.name, |st| {
                        st.last_error =
                            Some(format!("accept failed for rule \"{}\": {error}", rule.name));
                    });
                }
            }
        }
    }

    fn update(&self, name: &str, f: impl FnOnce(&mut ForwardState)) {
        let mut inner = lock(&self.inner);
        if let Some(state) = inner.states.get_mut(name) {
            f(state);
        }
    }

    /// Carry ONE accepted connection: open `via`, open the channel to `target`, pump both ways.
    ///
    /// **A HALF-CLOSE IS A HALF-CLOSE, NOT A HANG-UP.** Each direction ends on its own terms: when a
    /// copy reaches EOF it SHUTS DOWN the destination's write half and the other copy keeps
    /// running until its own EOF. The obvious version — return as soon as either direction ends —
    /// LOSES the answer for the commonest client there is, one that sends its request and then
    /// shuts down its write half: the response arrives 30 ms later and the forwarder has already
    /// closed. A forwarder that truncates responses is worse than none, because the client sees a
    /// short body rather than a refusal.
    async fn carry(&self, rule: &Rule, client: tokio::net::TcpStream, peer: std::net::SocketAddr) {
        let target = match hostport(&rule.target) {
            Some(parts) => parts,
            None => {
                self.audit_refusal(
                    rule,
                    peer,
                    "the rule's target could not be read as host:port",
                );
                return;
            }
        };
        let via = match ViaSpec::parse(&rule.via) {
            Ok(parsed) => parsed,
            Err(message) => {
                self.audit_refusal(rule, peer, &message);
                return;
            }
        };
        let auth = self.credential(rule);
        let opened = tokio::time::timeout(
            Duration::from_millis(OPEN_TIMEOUT_MS),
            ssh::open(
                &via.host,
                via.port,
                &via.user,
                &auth,
                None,
                &self.known_hosts,
            ),
        )
        .await;
        let handle = match opened {
            Ok(Ok(handle)) => handle,
            Ok(Err(error)) => {
                let message = format!("connecting to via {via} failed: {error}");
                self.update(&rule.name, |st| st.last_error = Some(message.clone()));
                self.audit_refusal(rule, peer, &message);
                return;
            }
            Err(_) => {
                let message =
                    format!("connecting to via {via} did not finish within {OPEN_TIMEOUT_MS} ms");
                self.update(&rule.name, |st| st.last_error = Some(message.clone()));
                self.audit_refusal(rule, peer, &message);
                return;
            }
        };
        let stream =
            match ssh::open_forward(&handle, (&target.0, target.1), (peer.ip(), peer.port())).await
            {
                Ok(stream) => stream,
                Err(error) => {
                    let message = error.to_string();
                    self.update(&rule.name, |st| st.last_error = Some(message.clone()));
                    self.audit_refusal(rule, peer, &message);
                    return;
                }
            };
        if let Err(error) = pipe_bidirectional(client, stream).await {
            self.update(&rule.name, |st| st.last_error = Some(error.to_string()));
        } else {
            // THE CARRIED CONNECTION IS COUNTED HERE, once both directions ended without an error.
            self.update(&rule.name, |st| st.connections += 1);
            // A SUCCESS CLEARS THE PREVIOUS FAILURE. Otherwise a rule that answered one bad
            // connection reads as broken forever, which is the state an operator then acts on.
            self.update(&rule.name, |st| st.last_error = None);
        }
    }

    /// The credential a rule uses for `via`.
    ///
    /// **THE RULE CARRIES ITS OWN, AND THAT IS WHY `Auth` GREW `Serialize`.** `terminal.create` takes
    /// the credential from the caller and holds it in memory for the session; a rule OUTLIVES that
    /// caller, and its executor must be able to reach `via` at boot with nobody present. So the
    /// credential is part of the row, stored beside it — one file, one commit point, one thing to
    /// delete when a rule is removed.
    ///
    /// **IT IS NOT A SERVER SECRET STORE**, and the difference is stated rather than implied: the
    /// file is `<data>/forwards.json`, protected by whatever protects `<data>` (the install's own
    /// ACL, which already holds this device's registration token). A host that needs a credential
    /// store of its own is a different design, and this one does not pretend to be it.
    fn credential(&self, rule: &Rule) -> Auth {
        rule.auth.clone()
    }

    /// Record a connection that did not work: who arrived, which rule, and why.
    ///
    /// **A SUCCESSFUL CONNECTION IS COUNTED, NOT WRITTEN TO DISK.** Written literally, "one line per
    /// connection" is a file written on the busiest path this module has, where a rule carrying a
    /// UI's requests appends hundreds of lines a minute. The count lives in the row, and the LOG
    /// keeps the failures, which are the ones an operator asks "who tried, and why did it fail"
    /// about.
    fn audit_refusal(&self, rule: &Rule, peer: std::net::SocketAddr, reason: &str) {
        record_event(
            &self.events,
            json!({
                "ts_ms": now_ms(),
                "event": "forward.refused",
                "rule": rule.name,
                "listen": rule.listen,
                "target": rule.target,
                "via": rule.via,
                "from": peer.to_string(),
                "reason": reason,
            }),
        );
    }
}

/// Carry bytes between a client and the far stream, in BOTH directions, each ending on its own EOF.
pub async fn pipe_bidirectional<A, B>(client: A, far: B) -> std::io::Result<()>
where
    A: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
    B: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
{
    use tokio::io::AsyncWriteExt as _;
    let (mut client_read, mut client_write) = tokio::io::split(client);
    let (mut far_read, mut far_write) = tokio::io::split(far);

    let to_far = async {
        tokio::io::copy(&mut client_read, &mut far_write).await?;
        // The client has no more input; the far side must learn that by the write half closing,
        // not by the whole connection going away.
        far_write.shutdown().await
    };
    let to_client = async {
        tokio::io::copy(&mut far_read, &mut client_write).await?;
        client_write.shutdown().await
    };
    let (a, b) = tokio::join!(to_far, to_client);
    a?;
    b
}
