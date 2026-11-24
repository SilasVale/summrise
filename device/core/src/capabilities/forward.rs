//! `forward`: the declaration. The implementation lives in `device/src/capabilities/forward.rs`.

use serde_json::{json, Value};

use crate::registry::{Capability, ToolDef};

/// The manual entry, rendered into the guide beside the tool list.
pub const GUIDE: &str = "The forward capability turns this machine into a relay. forward.add binds listen on this machine, dials target THROUGH via (user@host:port over SSH), so the far host gains no listening port; auth is stored with the rule because the executor runs with nobody present, and an optional host_key pins the fingerprint via must present. A rule that cannot bind stays visible with its last error rather than retrying silently. forward.remove closes the listener with the rule; forward.list shows each rule's state, attempts, connections and last error.";

/// This capability's declaration: the one place its tools and their schemas are written down.
pub const CAPABILITY: Capability = Capability {
    name: "forward",
    guide: GUIDE,
    tools: &[
        ToolDef {
            name: "forward.add",
            destructive: true,
            description: "Add or replace a forward rule: `listen` is bound on THIS machine, `via` makes the SSH connection, and that host's sshd dials `target`. The far host therefore gains NO listening port. `host_key` optionally pins the fingerprint `via`'s key must match — a rule runs with nobody present, so it is the one object here that should not decide a host's identity by itself. Nothing is created until the rule is persisted; a rule that cannot bind stays visible with its error rather than retrying forever.",
            schema: Some(add_schema),
        },
        ToolDef {
            name: "forward.remove",
            destructive: true,
            description: "Remove a rule and stop its executor; `removed` says whether one was configured. The listener is closed with it, so the address becomes bindable again.",
            schema: Some(remove_schema),
        },
        ToolDef {
            name: "forward.list",
            destructive: false,
            description: "List every rule with its live state: whether the listener is bound, since when, how many connections it has carried, how many bind attempts it has made, and what its last failure said. An unbound rule reports its error rather than a plausible-looking zero.",
            schema: Some(list_schema),
        },
    ],
};

fn add_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "minLength": 1,
                    "description": "The rule's name: how a panel and the log identify it, and how an edit replaces the existing rule with that name."
                },
                "listen": {
                    "type": "string",
                    "description": "The host:port to bind on THIS machine. A wildcard (0.0.0.0, ::, *) is refused: a rule that widens its own audience is the failure the refusal prevents."
                },
                "target": {
                    "type": "string",
                    "description": "The host:port that `via` dials. IPv6 literals need brackets, because the split is on the last colon."
                },
                "via": {
                    "type": "string",
                    "description": "user@host:port of the SSH connection that MAKES the connection, e.g. zss@100.64.0.9:22. The far host therefore gains no listening port."
                },
                "auth": {
                    "type": "object",
                    "description": "How the `via` connection authenticates: {kind: \"password\", password} or {kind: \"key\", path} with an ABSOLUTE key path. It is stored with the rule, because the rule outlives the caller and its executor must reach `via` at boot.",
                    "properties": {
                        "kind": { "type": "string", "enum": ["password", "key"] },
                        "password": { "type": "string" },
                        "path": { "type": "string" }
                    },
                    "required": ["kind"],
                    "additionalProperties": false
                },
                "host_key": {
                    "type": "string",
                    "description": "Optional SHA-256 fingerprint `via`'s key must match, e.g. SHA256:abc…. A rule is the one thing that connects with NOBODY PRESENT, so it is the one that least wants to decide a host's identity by itself; without this the device's known_hosts decides (first trust on use, refusal on a change)."
                }
            },
            "required": ["name", "listen", "target", "via", "auth"],
            "additionalProperties": false
    })
}

fn remove_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "The rule to remove; its listener is closed with it, so the address becomes bindable again."
                }
            },
            "required": ["name"],
            "additionalProperties": false
    })
}

fn list_schema() -> Value {
    json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
    })
}
