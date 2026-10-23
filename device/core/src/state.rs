//! The state document — the one file that answers "what is this device doing?".
//!
//! The schema is `docs/superpowers/specs/2026-10-20-the-two-runtime-architecture-design.md` §4.2.
//! A document is written only by `summrise-device` (atomically) and read only through the API; this
//! module owns its shape, not its storage.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The version of the state document this crate writes and reads.
///
/// A document declaring any other number is refused whole instead of parsed optimistically: the
/// fields below mean what this version says they mean, and a reader that guesses at a future schema
/// is how the console comes to show a confident wrong answer.
pub const SCHEMA_VERSION: u32 = 1;

/// The device's whole state: the document at `<data>/state.json` (design §4.2).
///
/// One writer (`summrise-device`, atomically), one reader path (the API); this type is the contract
/// between them, so every field is required and every unknown field is a mistake rather than an
/// extension.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    /// The schema this document is written against; [`SCHEMA_VERSION`] for a document we accept.
    pub schema: u32,
    /// The install the device runs from.
    pub install: Install,
    /// The last known-good copy the launcher restores from.
    pub baseline: Baseline,
    /// The service's liveness record.
    pub service: Service,
    /// What the device concluded about its own last start-up.
    pub last_verdict: LastVerdict,
    /// One row per session the device knows about.
    pub sessions: Vec<SessionRow>,
    /// Where an update has reached.
    pub update: Update,
    /// Artifacts the supervisor rolled back on this device, oldest first.
    ///
    /// A rollback is evidence about an *artifact*, not a version: the same version may be rebuilt
    /// and fixed, and that new digest is allowed to install. The app records one entry when it
    /// folds in a `rolled_back` swap result; the updater refuses a document whose digest is here
    /// until an operator overrides it by digest. `#[serde(default)]` keeps a document written
    /// before the field existed readable.
    #[serde(default)]
    pub rolled_back: Vec<RolledBackArtifact>,
    /// How many bytes of `<install>/repair.log` this document has already folded in.
    ///
    /// The launcher appends a record when it restores the app binary; the app consumes complete
    /// lines at start and records how far it got, so a restart does not record the same repair
    /// twice and a repair that happened while the app was down cannot be lost. `#[serde(default)]`
    /// keeps a document written before the field existed readable: zero means "nothing consumed".
    #[serde(default)]
    pub repair_offset: u64,
}

/// Where the device is installed and which release it runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Install {
    /// `<install>`, the root the launcher owns (design §4.1).
    ///
    /// A `String`, not a `Path`: the document has to mean the same thing when it is read on any
    /// host, and this crate never touches a filesystem, so it has no opinion to encode in a
    /// platform-specific path type.
    pub dir: String,
    /// The running app's release version, as the launcher passed it on the child's argv.
    pub version: String,
    /// The release channel this install follows.
    ///
    /// Left as its wire string: the update decision (design §4.4) compares it with the manifest's
    /// channel, and that comparison is where a label is accepted or refused.
    pub channel: String,
}

/// The copy of the app binary the launcher falls back to (design §4.4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Baseline {
    /// The version of the app binary in the generation the launcher boots.
    pub version: String,
    /// Hex-encoded SHA-256 of that binary; the launcher's verification is a comparison with this.
    pub sha256: String,
    /// The baseline binary's path in the generation the launcher boots.
    pub exe: String,
}

/// The service's liveness record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Service {
    /// Where the service is in its own lifecycle.
    pub state: ServiceState,
    /// The process id at start-up.
    ///
    /// Diagnostic, not identity: Windows reuses pids, so a reader must not address the service by
    /// this number.
    pub pid: u32,
    /// RFC 3339 UTC timestamp of the current start; `core` owns no clock, so it is a string.
    pub started_at: String,
}

/// The service's lifecycle, as the state file records it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceState {
    /// The process exists but is not serving yet.
    Starting,
    /// The service is serving.
    Running,
    /// The service is shutting down and no longer accepting work.
    Stopping,
    /// The service has shut down.
    Stopped,
}

/// The last start-up verdict: the device's own answer to "did boot go as expected?".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LastVerdict {
    /// RFC 3339 UTC timestamp of the verdict.
    pub ts: String,
    /// What happened, as a value the console can render without matching English.
    pub outcome: VerdictOutcome,
    /// One line a human can act on.
    pub detail: String,
}

/// What the last start-up concluded.
//
// `snake_case`, not `lowercase`: the schema's third value is `needs_attention`, and a single-token
// lower-casing would have written `needsattention`, which is not the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerdictOutcome {
    /// The device started with the binary it expected.
    Ok,
    /// The launcher had to restore the app binary from the baseline.
    Repaired,
    /// The device is up but something needs a human.
    NeedsAttention,
}

/// One session, as the telemetry document carries it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionRow {
    /// The session's id.
    pub id: String,
    /// Which backend the session is attached to.
    pub kind: SessionKind,
    /// Where the session is in its state machine.
    pub state: SessionState,
}

/// What a session is attached to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    /// A pseudo-terminal on this machine.
    Pty,
    /// An SSH connection.
    Ssh,
    /// A serial port.
    Serial,
}

/// The five states a session may be in (design §4.3), and no others.
///
/// The schema is the wire half of the state machine: which transitions are legal is a rule the
/// session module decides, but a document that names a sixth state is not a state document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionState {
    /// Being created.
    Creating,
    /// Live and taking work.
    Attached,
    /// Live but with no client.
    Detached,
    /// Finished and reclaimed.
    Reaped,
    /// Closed before being reaped.
    Closed,
}

/// Where an update has reached.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Update {
    /// The phase the update is in.
    pub phase: UpdatePhase,
    /// The version the update is heading to, or `None` when nothing is in flight.
    ///
    /// The three `Option` fields (`to`, `outcome`, `lastResultAtMs`) tolerate absence on the wire:
    /// serde reads a missing `Option` as `None` (the latter two also say `#[serde(default)]` so the
    /// rule is explicit), which is what keeps a document written by an older build readable. Every
    /// non-`Option` field is refused when missing.
    pub to: Option<String>,
    /// What the supervisor's last swap result said, once this app has recorded it.
    ///
    /// This is the terminal half of the update story: `phase` says what is in flight, `outcome`
    /// says how the last swap ended. `#[serde(default)]` keeps a document written before the field
    /// existed readable instead of turning an upgrade into a schema error (I1).
    #[serde(default)]
    pub outcome: Option<UpdateOutcome>,
    /// The `at_ms` of the last `swap.result.json` this app has folded into the document.
    ///
    /// The result file is the supervisor's, not the app's; this is how the app records one result
    /// exactly once across restarts and the one-second reconciliation. `#[serde(default)]` makes it
    /// optional on the wire (I1).
    #[serde(default)]
    pub last_result_at_ms: Option<u64>,
}

/// How the last swap ended, as the supervisor reported it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateOutcome {
    /// The supervisor installed the new release and proved it healthy.
    Applied,
    /// The new release could not serve; the previous generation is current again.
    RolledBack,
    /// The request or its staged pair was unusable; nothing was stopped.
    Refused,
}

/// The phases an update passes through; the launcher's swap is the last one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdatePhase {
    /// No update in flight.
    Idle,
    /// The new artifact is being fetched.
    Downloading,
    /// The artifact is verified and on disk at `<install>\summrise-device.new.exe`.
    Staged,
    /// The launcher has been asked to swap.
    Applying,
}

/// How many rolled-back artifacts the document remembers.
///
/// The cap is an operator's memory, not a release history: sixteen recent failures are enough to
/// protect a device from a loop, and a bounded document stays a document. The oldest entry is
/// evicted first.
pub const MAX_ROLLED_BACK: usize = 16;

/// One artifact the supervisor installed and rolled back on this device.
///
/// Keyed by digest, because that is what a rebuild changes: a fixed build with the same version and
/// a different digest is a different artifact and is allowed. `at_ms` is the supervisor's
/// `swap.result.json` time — when the rollback was answered, not when the app first read it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RolledBackArtifact {
    /// Lower- or upper-hex SHA-256 of the artifact that failed.
    pub sha256: String,
    /// The version the failed artifact claimed, for the operator-facing refusal.
    pub version: String,
    /// Millisecond Unix time the supervisor wrote the rollback result.
    pub at_ms: u64,
}

/// Why a document was refused.
#[derive(Debug)]
pub enum StateError {
    /// The document's `schema` is not [`SCHEMA_VERSION`].
    UnsupportedSchema,
    /// The document is not JSON or does not match the schema.
    Invalid(serde_json::Error),
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateError::UnsupportedSchema => {
                write!(
                    f,
                    "unsupported state schema; this build understands {SCHEMA_VERSION}"
                )
            }
            StateError::Invalid(error) => write!(f, "invalid state document: {error}"),
        }
    }
}

impl std::error::Error for StateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StateError::UnsupportedSchema => None,
            StateError::Invalid(error) => Some(error),
        }
    }
}

impl State {
    /// Parse a state document, refusing anything this build does not understand.
    ///
    /// The version is read first and on its own, so a future document is refused as a future document
    /// instead of as a pile of unknown fields — the caller's next move ("upgrade the device") depends
    /// on that distinction.
    pub fn parse(document: &str) -> Result<State, StateError> {
        #[derive(Deserialize)]
        struct SchemaProbe {
            schema: u32,
        }

        let probe: SchemaProbe = serde_json::from_str(document).map_err(StateError::Invalid)?;
        if probe.schema != SCHEMA_VERSION {
            return Err(StateError::UnsupportedSchema);
        }

        serde_json::from_str(document).map_err(StateError::Invalid)
    }

    /// Write the document this crate accepts.
    ///
    /// One line: the file is a wire document written atomically and read by the API, not a file
    /// humans edit. Serialization cannot fail for this type, so the invariant is an `expect` rather
    /// than an error a caller would have nothing to do with.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a State is always serializable")
    }

    /// Remember one artifact the supervisor rolled back, newest last.
    ///
    /// The digest is the identity: recording the same artifact again is a no-op (an operator may
    /// retry it, and a restart may fold the same result in twice). The list is capped at
    /// [`MAX_ROLLED_BACK`] and the oldest entries are evicted, so a document that has survived many
    /// failures is still a bounded document.
    pub fn record_rolled_back(&mut self, artifact: RolledBackArtifact) {
        if self
            .rolled_back
            .iter()
            .any(|known| known.sha256.eq_ignore_ascii_case(&artifact.sha256))
        {
            return;
        }
        if self.rolled_back.len() >= MAX_ROLLED_BACK {
            let excess = self.rolled_back.len() + 1 - MAX_ROLLED_BACK;
            self.rolled_back.drain(..excess);
        }
        self.rolled_back.push(artifact);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The design's example document (§4.2) with its placeholders filled in.
    ///
    /// Kept as the literal wire document rather than as constructed values: this is the fixture that
    /// proves the schema in the design and the schema in the Rust types are the same schema.
    const EXAMPLE: &str = r#"{
      "schema": 1,
      "install": { "dir": "D:\\Summrise", "version": "2.0.3", "channel": "stable" },
      "baseline": { "version": "2.0.3", "sha256": "b1946ac92492d2347c6235b4d2611184", "exe": "D:\\Summrise\\baseline\\current\\summrise-device.exe" },
      "service": { "state": "running", "pid": 1234, "startedAt": "2026-10-20T12:00:00Z" },
      "lastVerdict": { "ts": "2026-10-20T12:00:00Z", "outcome": "ok", "detail": "started" },
      "sessions": [ { "id": "session-1", "kind": "pty", "state": "attached" } ],
      "update": { "phase": "idle", "to": null, "outcome": null, "lastResultAtMs": null }
    }"#;

    #[test]
    fn the_designs_example_document_round_trips() {
        let state = State::parse(EXAMPLE).expect("the design's example is a valid document");

        assert_eq!(state.schema, SCHEMA_VERSION);
        assert_eq!(state.install.dir, r"D:\Summrise");
        assert_eq!(state.install.version, "2.0.3");
        assert_eq!(state.install.channel, "stable");
        assert_eq!(state.baseline.version, "2.0.3");
        assert_eq!(
            state.baseline.exe,
            r"D:\Summrise\baseline\current\summrise-device.exe"
        );
        assert_eq!(state.service.state, ServiceState::Running);
        assert_eq!(state.service.pid, 1234);
        assert_eq!(state.service.started_at, "2026-10-20T12:00:00Z");
        assert_eq!(state.last_verdict.outcome, VerdictOutcome::Ok);
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].id, "session-1");
        assert_eq!(state.sessions[0].kind, SessionKind::Pty);
        assert_eq!(state.sessions[0].state, SessionState::Attached);
        assert_eq!(state.update.phase, UpdatePhase::Idle);
        assert_eq!(state.update.to, None);
        assert_eq!(state.update.outcome, None);
        assert_eq!(state.update.last_result_at_ms, None);

        let written = state.to_json();
        let reread = State::parse(&written).expect("what to_json writes parses");
        assert_eq!(reread, state);
    }

    /// A document written before the update outcome fields existed is still a document: they are
    /// optional and default, not a schema change that turns an upgrade into an error.
    #[test]
    fn a_document_without_the_optional_update_fields_still_parses() {
        let document = EXAMPLE.replace(r#", "outcome": null, "lastResultAtMs": null"#, "");
        let state = State::parse(&document).expect("the optional update fields default");
        assert_eq!(state.update.outcome, None);
        assert_eq!(state.update.last_result_at_ms, None);
    }

    /// I3: the repair-log offset is a fact in the one document, so it has one writer and survives a
    /// restart. A document written before the field existed reads as "nothing consumed yet" rather
    /// than becoming a schema error (`#[serde(default)]`), and the value round-trips.
    #[test]
    fn the_repair_offset_defaults_for_an_older_document_and_round_trips() {
        let state = State::parse(EXAMPLE).expect("the design's example is a valid document");
        assert_eq!(
            state.repair_offset, 0,
            "an older document has consumed no repair log"
        );

        let mut consumed = state.clone();
        consumed.repair_offset = 4_096;
        let written = consumed.to_json();
        let document: serde_json::Value =
            serde_json::from_str(&written).expect("to_json writes JSON");
        assert_eq!(
            document["repairOffset"], 4_096,
            "the offset is a camelCase document field"
        );
        let reread = State::parse(&written).expect("the offset round-trips");
        assert_eq!(reread.repair_offset, 4_096);
    }

    /// A rollback is remembered by artifact, not by version: the digest is the key, the version is
    /// what the operator reads, and the time is when the supervisor answered. The list is part of
    /// the one document and defaults for a document written before it existed.
    #[test]
    fn the_rolled_back_list_defaults_for_an_older_document_and_round_trips() {
        let state = State::parse(EXAMPLE).expect("the design's example is a valid document");
        assert!(
            state.rolled_back.is_empty(),
            "an older document remembers no rollback"
        );

        let mut state = state;
        state.record_rolled_back(RolledBackArtifact {
            sha256: "aa".repeat(32),
            version: "2.0.3".to_owned(),
            at_ms: 1_700_000_000_000,
        });
        let written = state.to_json();
        let document: serde_json::Value =
            serde_json::from_str(&written).expect("to_json writes JSON");
        assert_eq!(
            document["rolledBack"][0]["sha256"],
            serde_json::json!("aa".repeat(32)),
            "the digest is a camelCase document field"
        );
        assert_eq!(document["rolledBack"][0]["version"], "2.0.3");
        assert_eq!(document["rolledBack"][0]["atMs"], 1_700_000_000_000u64);
        let reread = State::parse(&written).expect("the rolled-back list round-trips");
        assert_eq!(reread, state);
    }

    /// The same artifact can roll back twice (an operator retried it); the document must remember it
    /// once, by digest. Hex case is not a second artifact.
    #[test]
    fn a_duplicate_sha_is_recorded_once() {
        let mut state = State::parse(EXAMPLE).expect("the design's example is a valid document");
        let first = RolledBackArtifact {
            sha256: "AB".repeat(32),
            version: "2.0.3".to_owned(),
            at_ms: 1_000,
        };

        state.record_rolled_back(first.clone());
        state.record_rolled_back(RolledBackArtifact {
            sha256: "ab".repeat(32),
            version: "2.0.4".to_owned(),
            at_ms: 2_000,
        });

        assert_eq!(state.rolled_back.len(), 1, "one digest is one artifact");
        assert_eq!(
            state.rolled_back[0], first,
            "the first record keeps its version and time"
        );
    }

    /// Sixteen is the cap: an operator needs recent failures, not an unbounded document. The oldest
    /// entry is evicted first, so the list is the newest sixteen in the order they were recorded.
    #[test]
    fn the_rolled_back_list_keeps_the_newest_sixteen() {
        let mut state = State::parse(EXAMPLE).expect("the design's example is a valid document");

        for index in 0..=MAX_ROLLED_BACK {
            state.record_rolled_back(RolledBackArtifact {
                sha256: format!("{index:064x}"),
                version: format!("2.0.{index}"),
                at_ms: index as u64,
            });
        }

        assert_eq!(state.rolled_back.len(), MAX_ROLLED_BACK);
        assert_eq!(
            state.rolled_back[0].at_ms, 1,
            "the oldest artifact is the one evicted"
        );
        assert_eq!(
            state.rolled_back[MAX_ROLLED_BACK - 1].at_ms,
            MAX_ROLLED_BACK as u64,
            "the newest artifact is kept"
        );
    }

    #[test]
    fn the_written_document_uses_the_designs_key_names() {
        let state = State::parse(EXAMPLE).expect("the design's example is a valid document");
        let document: serde_json::Value =
            serde_json::from_str(&state.to_json()).expect("to_json writes JSON");
        let object = document.as_object().expect("a state document is an object");

        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "baseline",
                "install",
                "lastVerdict",
                "repairOffset",
                "rolledBack",
                "schema",
                "service",
                "sessions",
                "update"
            ]
        );
        assert!(
            !object.contains_key("last_verdict"),
            "the document is camelCase on the wire"
        );

        let service = object["service"].as_object().expect("service is an object");
        assert!(service.contains_key("startedAt"));
        assert!(!service.contains_key("started_at"));

        let update = object["update"].as_object().expect("update is an object");
        assert_eq!(update.get("to"), Some(&serde_json::Value::Null));
        assert!(update.contains_key("outcome"));
        assert!(update.contains_key("lastResultAtMs"));
    }

    #[test]
    fn an_unknown_schema_version_is_rejected() {
        assert_eq!(SCHEMA_VERSION, 1);

        for version in ["0", "2", "99"] {
            let document = EXAMPLE.replace("\"schema\": 1", &format!("\"schema\": {version}"));
            assert!(
                matches!(State::parse(&document), Err(StateError::UnsupportedSchema)),
                "schema {version} must be rejected as unsupported"
            );
        }
    }

    #[test]
    fn a_missing_required_field_is_rejected() {
        let base: serde_json::Value = serde_json::from_str(EXAMPLE).expect("the fixture is JSON");
        let object = base.as_object().expect("the fixture is an object");

        for field in [
            "schema",
            "install",
            "baseline",
            "service",
            "lastVerdict",
            "sessions",
            "update",
        ] {
            let mut without = object.clone();
            without.remove(field);
            let document = serde_json::to_string(&without).expect("serializes");
            assert!(
                matches!(State::parse(&document), Err(StateError::Invalid(_))),
                "a document without `{field}` must be rejected, not defaulted"
            );
        }

        // Nested required fields are required too — `deny_unknown_fields` does not imply this, so the
        // fixture proves it at the second level.
        for (parent, field) in [
            ("install", "version"),
            ("baseline", "sha256"),
            ("service", "pid"),
            ("lastVerdict", "outcome"),
            ("update", "phase"),
        ] {
            let mut without = base.clone();
            without[parent]
                .as_object_mut()
                .expect("parent is an object")
                .remove(field);
            let document = serde_json::to_string(&without).expect("serializes");
            assert!(
                matches!(State::parse(&document), Err(StateError::Invalid(_))),
                "a document without `{parent}.{field}` must be rejected"
            );
        }
    }

    #[test]
    fn an_unknown_field_is_rejected() {
        let document = EXAMPLE.replace("\"schema\": 1,", "\"schema\": 1, \"build\": \"abc\",");
        assert!(
            matches!(State::parse(&document), Err(StateError::Invalid(_))),
            "an unknown top-level field must be rejected"
        );

        let document = EXAMPLE.replace("\"pid\": 1234,", "\"pid\": 1234, \"threads\": 8,");
        assert!(
            matches!(State::parse(&document), Err(StateError::Invalid(_))),
            "an unknown nested field must be rejected"
        );
    }

    #[test]
    fn every_enum_writes_its_wire_name() {
        let cases = [
            (serde_json::to_string(&UpdatePhase::Idle), "\"idle\""),
            (
                serde_json::to_string(&UpdatePhase::Downloading),
                "\"downloading\"",
            ),
            (serde_json::to_string(&UpdatePhase::Staged), "\"staged\""),
            (
                serde_json::to_string(&UpdatePhase::Applying),
                "\"applying\"",
            ),
            (serde_json::to_string(&VerdictOutcome::Ok), "\"ok\""),
            (
                serde_json::to_string(&VerdictOutcome::Repaired),
                "\"repaired\"",
            ),
            (
                serde_json::to_string(&VerdictOutcome::NeedsAttention),
                "\"needs_attention\"",
            ),
            (serde_json::to_string(&SessionKind::Pty), "\"pty\""),
            (serde_json::to_string(&SessionKind::Ssh), "\"ssh\""),
            (serde_json::to_string(&SessionKind::Serial), "\"serial\""),
            (
                serde_json::to_string(&SessionState::Creating),
                "\"creating\"",
            ),
            (
                serde_json::to_string(&SessionState::Attached),
                "\"attached\"",
            ),
            (
                serde_json::to_string(&SessionState::Detached),
                "\"detached\"",
            ),
            (serde_json::to_string(&SessionState::Reaped), "\"reaped\""),
            (serde_json::to_string(&SessionState::Closed), "\"closed\""),
            (
                serde_json::to_string(&ServiceState::Starting),
                "\"starting\"",
            ),
            (serde_json::to_string(&ServiceState::Running), "\"running\""),
            (
                serde_json::to_string(&ServiceState::Stopping),
                "\"stopping\"",
            ),
            (serde_json::to_string(&ServiceState::Stopped), "\"stopped\""),
            (
                serde_json::to_string(&UpdateOutcome::Applied),
                "\"applied\"",
            ),
            (
                serde_json::to_string(&UpdateOutcome::RolledBack),
                "\"rolled_back\"",
            ),
            (
                serde_json::to_string(&UpdateOutcome::Refused),
                "\"refused\"",
            ),
        ];

        for (actual, expected) in cases {
            assert_eq!(actual.expect("an enum variant serializes"), expected);
        }
    }

    #[test]
    fn session_kind_and_state_are_lower_case_on_the_wire() {
        let row = SessionRow {
            id: "session-1".to_owned(),
            kind: SessionKind::Pty,
            state: SessionState::Attached,
        };
        let json = serde_json::to_string(&row).expect("a session row serializes");

        assert!(
            json.contains("\"kind\":\"pty\""),
            "kind is lower-case: {json}"
        );
        assert!(
            json.contains("\"state\":\"attached\""),
            "state is lower-case: {json}"
        );

        // The closed wire vocabulary is enforced in both directions.
        let document = EXAMPLE.replace("\"kind\": \"pty\"", "\"kind\": \"Pty\"");
        assert!(
            matches!(State::parse(&document), Err(StateError::Invalid(_))),
            "a capitalised kind is not a wire value"
        );
    }
}
