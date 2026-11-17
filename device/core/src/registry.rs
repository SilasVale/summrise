//! The capability registry (design §4.5): a tool exists because a capability declares it.
//!
//! The MCP tool list, the panel's buttons, the console's rendering and the `destructive` question
//! `authorize` asks are all readings of this one table. A tool that is not here is not a tool this
//! device offers, and every caller treats `None` as a denial rather than a default.

use serde_json::Value;

/// One tool a capability exposes, and the facts the decision and the catalogue read.
///
/// No `PartialEq`: the schema field is a function pointer, and comparing two functions is not a
/// question with a meaningful answer.
#[derive(Clone, Copy, Debug)]
pub struct ToolDef {
    /// The wire name a client calls, e.g. `files.read`.
    pub name: &'static str,
    /// Whether calling it does something a later read cannot undo.
    pub destructive: bool,
    /// One sentence a client shows a human; the MCP tool description is this string.
    pub description: &'static str,
    /// The input schema, declared beside the tool. `None` is a capability that has not migrated to
    /// the declaration module yet; the surface falls back to its own table, and a test counts them.
    pub schema: Option<fn() -> Value>,
}

/// One capability: a name, the tools it declares, and its manual entry.
#[derive(Clone, Copy, Debug)]
pub struct Capability {
    /// The capability's own name, e.g. `files`. Every tool name is namespaced by it.
    pub name: &'static str,
    /// The tools the capability declares. The registry is the union of these lists.
    pub tools: &'static [ToolDef],
    /// The manual entry, rendered by [`guide`]; empty until the capability migrates.
    pub guide: &'static str,
}

/// Every capability this device offers.
///
/// The order is the order a client sees in `tools/list`; it is data, so reordering it is a wire
/// change rather than a refactor.
pub const CAPABILITIES: &[Capability] = &[
    crate::capabilities::files::CAPABILITY,
    crate::capabilities::memory::CAPABILITY,
    crate::capabilities::terminal::CAPABILITY,
    crate::capabilities::browser::CAPABILITY,
    crate::capabilities::monitor::CAPABILITY,
    crate::capabilities::runs::CAPABILITY,
    crate::capabilities::system::CAPABILITY,
    crate::capabilities::host::CAPABILITY,
    crate::capabilities::screen::CAPABILITY,
    crate::capabilities::forward::CAPABILITY,
    crate::capabilities::state::CAPABILITY,
    crate::capabilities::journal::CAPABILITY,
    crate::capabilities::plugins::CAPABILITY,
    crate::capabilities::txn::CAPABILITY,
];

/// The manual: every migrated capability's entry, then each of its tools with its sentence.
///
/// A capability with an empty guide has not migrated yet; it renders as nothing rather than as a
/// heading with no text. The guide is generated from the table, never hand-maintained beside it.
pub fn guide() -> String {
    let mut text = String::new();
    for capability in CAPABILITIES {
        if capability.guide.is_empty() {
            continue;
        }
        text.push_str("# ");
        text.push_str(capability.name);
        text.push('\n');
        text.push_str(capability.guide);
        text.push_str("\n\n");
        for tool in capability.tools {
            text.push_str("## ");
            text.push_str(tool.name);
            text.push('\n');
            text.push_str(tool.description);
            text.push_str("\n\n");
        }
    }
    text
}

/// The declaration for `name`, or `None` when no capability declares it.
pub fn tool(name: &str) -> Option<&'static ToolDef> {
    CAPABILITIES
        .iter()
        .flat_map(|capability| capability.tools)
        .find(|tool| tool.name == name)
}

/// Whether `name` is destructive: `Some(true)` for a write, `Some(false)` for a read, and `None`
/// for a name that is not in the registry at all. A caller that gets `None` denies.
pub fn is_destructive(name: &str) -> Option<bool> {
    tool(name).map(|tool| tool.destructive)
}

/// The argument a call names its transaction with, when it overrides inheritance.
///
/// A call that names no session inherits the caller's working device-scope transaction, and a call
/// that names a session inherits that session's; an explicit `txn` argument wins over both. The
/// spelling is one constant so the gate and the tool declarations cannot drift apart.
pub const WORKING_TXN_ARG: &str = "txn";

/// What a tool needs before the device will run it (decision §3.3).
///
/// Exactly one policy per tool, and the policy is a value rather than a rendering rule: the gate
/// reads [`txn_policy`] and nothing else decides whether a call needs a transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxnPolicy {
    /// No transaction is required, because the call is what makes one possible (`terminal.create`).
    /// Audited `txn: none` with a `bootstrap` marker.
    Bootstrap,
    /// No transaction needed: the device's own transaction tools, the goal claim and the read-only
    /// catalogue. Audited `txn: none`. **Not "read-only":** `txn.begin`, `txn.commit`,
    /// `txn.abort` and `terminal.goal` write records or claims; they are exempt because a
    /// transaction cannot require a transaction, and because the claim belongs to the session.
    ExemptRead,
    /// A working transaction of the right scope is required; the gate refuses `txn_required`.
    Required,
}

/// The tools that make a transaction possible: one tool, and it is why the gate can exist.
pub const BOOTSTRAP_TXNS: &[&str] = &["terminal.create"];

/// The tools that need no transaction: the device's own transaction tools, the goal claim and the
/// read-only catalogue, sixteen over the 56-tool surface.
///
/// **The exemption is not "read-only".** `txn.begin` opens the record, `txn.commit` and
/// `txn.abort` close it, and `terminal.goal` writes a claim; they are exempt because a transaction
/// cannot be required to open a transaction, and because a goal belongs to its session rather than
/// to a unit of work. Everything else — including `browser.open`, `runs.exec` and `plugins.logs` —
/// needs a working transaction.
pub const EXEMPT_READ_TXNS: &[&str] = &[
    "txn.begin",
    "txn.commit",
    "txn.abort",
    "terminal.list",
    "terminal.goal",
    "state.get",
    "host.facts",
    "browser.list",
    "monitor.list",
    "monitor.status",
    "forward.list",
    "plugins.list",
    "plugins.inspect",
    "runs.list",
    "runs.get",
    "system.process_list",
];

/// The transaction policy of `name`, or `None` when the registry does not declare it.
///
/// The default is [`TxnPolicy::Required`], and that direction is deliberate: a tool added without a
/// row in either list is gated, so forgetting a row can never open the gate. A name that is not a
/// tool is `None`, and every caller treats `None` as a denial rather than as exempt.
pub fn txn_policy(name: &str) -> Option<TxnPolicy> {
    if tool(name).is_none() {
        return None;
    }
    if BOOTSTRAP_TXNS.contains(&name) {
        Some(TxnPolicy::Bootstrap)
    } else if EXEMPT_READ_TXNS.contains(&name) {
        Some(TxnPolicy::ExemptRead)
    } else {
        Some(TxnPolicy::Required)
    }
}

/// The capability whose tools act on a session: a PTY, SSH or serial session is a session, and no
/// other capability's `id` is one (`monitor.probe`'s id is a target, `system.process_kill`'s a pid).
const SESSION_CAPABILITY: &str = "terminal";

/// The argument a session tool names its session with.
const SESSION_ARG: &str = "id";

/// The argument a call names its session with, or `None` when it names no session.
///
/// The gate's only way to find the session a call acts on: inheritance is resolved from this map,
/// never by a renderer parsing arguments. A tool names a session only when **both** facts hold —
/// it belongs to the capability that owns sessions ([`SESSION_CAPABILITY`]) and its own declared
/// schema has an `id` argument. Both are read from the registry, so nothing is guessed from a name:
/// `terminal.create` makes the session and declares no `id`; `terminal.list` takes no arguments at
/// all; a `terminal.<something>` that is not declared is not a tool and names nothing. `txn.begin`
/// names the session it binds to as `session`. Everything else is device-scope or must pass `txn`
/// explicitly.
pub fn session_arg(name: &str) -> Option<&'static str> {
    if name == "txn.begin" {
        return Some("session");
    }
    let (capability, declaration) = CAPABILITIES.iter().find_map(|capability| {
        let declaration = capability
            .tools
            .iter()
            .find(|declared| declared.name == name)?;
        Some((capability, declaration))
    })?;
    if capability.name != SESSION_CAPABILITY {
        return None;
    }
    let schema = declaration.schema?;
    let schema = schema();
    let properties = schema.get("properties")?.as_object()?;
    properties.contains_key(SESSION_ARG).then_some(SESSION_ARG)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_declares_the_whole_surface_and_fourteen_capabilities() {
        let tools: Vec<&str> = CAPABILITIES
            .iter()
            .flat_map(|capability| capability.tools)
            .map(|tool| tool.name)
            .collect();

        assert_eq!(
            CAPABILITIES.len(),
            14,
            "fourteen capabilities declare the surface"
        );
        assert_eq!(tools.len(), 56, "the surface is 56 tools");
        assert_eq!(
            tools,
            [
                "files.read",
                "files.write",
                "files.mkdir",
                "files.list",
                "memory.append",
                "memory.update",
                "memory.delete",
                "memory.search",
                "memory.list",
                "memory.export",
                "terminal.create",
                "terminal.write",
                "terminal.read",
                "terminal.resize",
                "terminal.close",
                "terminal.list",
                "terminal.goal",
                "terminal.screen",
                "browser.list",
                "browser.open",
                "browser.navigate",
                "browser.eval",
                "browser.screenshot",
                "browser.cast",
                "browser.close",
                "browser.shutdown",
                "monitor.add",
                "monitor.remove",
                "monitor.list",
                "monitor.probe",
                "monitor.status",
                "runs.exec",
                "runs.list",
                "runs.get",
                "system.process_list",
                "system.process_kill",
                "system.net_test",
                "host.facts",
                "host.restart",
                "screen.capture",
                "forward.add",
                "forward.remove",
                "forward.list",
                "state.get",
                "journal.get",
                "journal.read",
                "journal.day",
                "plugins.list",
                "plugins.inspect",
                "plugins.add",
                "plugins.remove",
                "plugins.restart",
                "plugins.logs",
                "txn.begin",
                "txn.commit",
                "txn.abort",
            ],
            "the registry is the whole surface: no tool exists outside it"
        );
    }

    #[test]
    fn the_destructive_flag_is_the_declared_value() {
        // **ONE TABLE, AND IT IS CHECKED AGAINST THE REGISTRY IN BOTH DIRECTIONS.** The list below is the
        // EXPECTATION;  reads the DECLARATION. Two lists that agree only for the tools
        // both of them name is a sample, not a check — measured: the table used to stop at
        // `("plugins.logs", false)` and six real tools (`memory.export`, `memory.list`,
        // `system.net_test`, `system.process_kill`, `system.process_list`, `terminal.screen`) had no
        // row at all. **One of those, `system.process_kill`, is destructive** — a kill signal that
        // anyone could send through a token that never consented, with the flag that authorizes it
        // unasserted.
        let expected: [(&str, bool); 56] = [
            ("files.read", false),
            ("files.write", true),
            ("files.mkdir", false),
            ("files.list", false),
            ("memory.append", true),
            ("memory.update", true),
            ("memory.delete", true),
            ("memory.search", false),
            ("memory.list", false),
            ("memory.export", false),
            ("terminal.create", true),
            ("terminal.write", true),
            ("terminal.read", false),
            ("terminal.resize", false),
            ("terminal.close", false),
            ("terminal.list", false),
            ("terminal.goal", false),
            ("terminal.screen", false),
            ("browser.list", false),
            ("browser.open", true),
            ("browser.navigate", true),
            ("browser.eval", true),
            ("browser.screenshot", false),
            ("browser.cast", false),
            ("browser.close", false),
            ("browser.shutdown", false),
            ("forward.add", true),
            ("forward.remove", true),
            ("forward.list", false),
            ("monitor.add", false),
            ("monitor.remove", false),
            ("monitor.list", false),
            ("monitor.probe", false),
            ("monitor.status", false),
            ("runs.exec", true),
            ("runs.list", false),
            ("runs.get", false),
            ("host.facts", false),
            ("host.restart", true),
            ("screen.capture", false),
            ("state.get", false),
            ("journal.get", false),
            ("journal.read", false),
            ("journal.day", false),
            ("system.net_test", false),
            ("system.process_list", false),
            ("system.process_kill", true),
            ("plugins.list", false),
            ("plugins.inspect", false),
            ("plugins.add", true),
            ("plugins.remove", true),
            ("plugins.restart", true),
            ("plugins.logs", false),
            ("txn.begin", false),
            ("txn.commit", false),
            ("txn.abort", false),
        ];

        for (name, destructive) in expected {
            assert_eq!(
                is_destructive(name),
                Some(destructive),
                "{name}'s destructive flag is the fact authorize's Ask rule consults"
            );
        }

        // **EVERY REGISTRY TOOL HAS A ROW, OR THE TABLE ABOVE IS A LIST OF SAMPLES.** A tool added to
        // `CAPABILITIES` without a row here was never compared; deleting `("forward.list", false)`
        // from the old table left this suite green.
        let declared: Vec<&str> = CAPABILITIES
            .iter()
            .flat_map(|capability| capability.tools)
            .map(|tool| tool.name)
            .collect();
        let mut missing: Vec<&str> = declared
            .iter()
            .copied()
            .filter(|name| !expected.iter().any(|(row, _)| row == name))
            .collect();
        missing.sort_unstable();
        assert!(
        missing.is_empty(),
        "a tool in CAPABILITIES has no row in the table above, so its destructive flag is          unasserted: {missing:?}"
    );
        // And the other direction: a row for a tool the registry no longer declares is a stale line that
        // would keep passing after the tool was removed.
        let mut stale: Vec<&str> = expected
            .iter()
            .map(|(row, _)| *row)
            .filter(|name| !declared.contains(name))
            .collect();
        stale.sort_unstable();
        assert!(
            stale.is_empty(),
            "a row names a tool the registry does not declare: {stale:?}"
        );
    }

    #[test]
    fn the_memory_update_description_says_its_arguments_are_required() {
        // A client that reads only this sentence must not assume append's optional tags: update
        // replaces the whole list, and the schema refuses an update that omits it.
        let description = tool("memory.update")
            .expect("the tool is declared")
            .description;
        for fact in [
            "both are required",
            "complete new list",
            "[] to clear",
            "exactly memory.append's",
            "no_such_entry",
        ] {
            assert!(
                description.contains(fact),
                "memory.update's description states {fact:?}: {description}"
            );
        }
    }

    #[test]
    fn the_terminal_descriptions_name_its_caps() {
        for (name, caps) in [
            (
                "terminal.create",
                &[
                    "16",
                    "1 MiB",
                    "30 minutes",
                    "pty",
                    "ssh",
                    "serial",
                    "115200 8N1",
                ][..],
            ),
            ("terminal.write", &["1 MiB", "5 seconds"][..]),
            ("terminal.read", &["1 MiB"][..]),
            ("terminal.resize", &["1..=1000"][..]),
        ] {
            let description = tool(name).expect("the tool is declared").description;
            for cap in caps {
                assert!(
                    description.contains(cap),
                    "{name}'s description names its cap {cap:?}: {description}"
                );
            }
        }
    }

    #[test]
    fn the_browser_descriptions_name_its_caps() {
        for (name, caps) in [
            ("browser.open", &["8 tabs"][..]),
            ("browser.eval", &["1 MiB"][..]),
            ("browser.screenshot", &["8 MiB"][..]),
            ("browser.cast", &["8 MiB", "60 s"][..]),
        ] {
            let description = tool(name).expect("the tool is declared").description;
            for cap in caps {
                assert!(
                    description.contains(cap),
                    "{name}'s description names its cap {cap:?}: {description}"
                );
            }
        }
    }

    #[test]
    fn the_runs_descriptions_name_its_caps() {
        for (name, caps) in [
            (
                "runs.exec",
                &["1 MiB", "60", "10 minutes", "32", "4 KiB"][..],
            ),
            ("runs.list", &["10 000", "100", "20"][..]),
        ] {
            let description = tool(name).expect("the tool is declared").description;
            for cap in caps {
                assert!(
                    description.contains(cap),
                    "{name}'s description names its cap {cap:?}: {description}"
                );
            }
        }
    }

    #[test]
    fn the_monitor_descriptions_name_its_caps() {
        for (name, caps) in [
            ("monitor.add", &["16", "5000", "60"][..]),
            ("monitor.list", &["60"][..]),
            ("monitor.status", &["60"][..]),
        ] {
            let description = tool(name).expect("the tool is declared").description;
            for cap in caps {
                assert!(
                    description.contains(cap),
                    "{name}'s description names its cap {cap:?}: {description}"
                );
            }
        }
    }

    #[test]
    fn a_name_no_capability_declares_is_none() {
        for name in [
            "",
            "files",
            "files.delete",
            "files.read ",
            "Files.read",
            "memory.read",
            "terminal.pty",
            "browser",
            "browser.delete",
            "browser.open ",
            "runs",
            "runs.delete",
            "runs.exec ",
            "*",
        ] {
            assert!(tool(name).is_none(), "{name:?}");
            assert_eq!(is_destructive(name), None, "{name:?}");
        }
    }

    #[test]
    fn every_tool_name_is_unique() {
        let mut names: Vec<&str> = CAPABILITIES
            .iter()
            .flat_map(|capability| capability.tools)
            .map(|tool| tool.name)
            .collect();
        let declared = names.len();
        names.sort_unstable();
        names.dedup();

        assert_eq!(names.len(), declared, "a tool name must be declared once");
    }

    #[test]
    fn every_tool_declares_its_transaction_policy() {
        // **ONE TABLE, AND IT IS CHECKED AGAINST THE REGISTRY IN BOTH DIRECTIONS.** The gate refuses
        // a machine call with no working transaction, so a tool with no policy row is a hole: the
        // census iterates the registry, never a hand-kept list of names.
        assert_eq!(
            BOOTSTRAP_TXNS,
            ["terminal.create"],
            "creating the shell is what makes a transaction possible, and it is the only Bootstrap tool"
        );

        let mut exempt: Vec<&str> = EXEMPT_READ_TXNS.to_vec();
        exempt.sort_unstable();
        let mut expected: Vec<&str> = vec![
            "txn.begin",
            "txn.commit",
            "txn.abort",
            "terminal.list",
            "terminal.goal",
            "state.get",
            "host.facts",
            "browser.list",
            "monitor.list",
            "monitor.status",
            "forward.list",
            "plugins.list",
            "plugins.inspect",
            "runs.list",
            "runs.get",
            "system.process_list",
        ];
        expected.sort_unstable();
        assert_eq!(
            exempt, expected,
            "the exemption is the device's own record tools, the goal claim and the read-only \
             catalogue; it is not a claim that every exempt tool is a read"
        );

        let declared: Vec<&str> = CAPABILITIES
            .iter()
            .flat_map(|capability| capability.tools)
            .map(|tool| tool.name)
            .collect();
        assert_eq!(declared.len(), 56, "the surface is 56 tools");

        let mut required = 0;
        for name in &declared {
            let policy =
                txn_policy(name).unwrap_or_else(|| panic!("{name} declares no transaction policy"));
            if BOOTSTRAP_TXNS.contains(name) {
                assert_eq!(policy, TxnPolicy::Bootstrap, "{name}");
            } else if EXEMPT_READ_TXNS.contains(name) {
                assert_eq!(policy, TxnPolicy::ExemptRead, "{name}");
            } else {
                assert_eq!(policy, TxnPolicy::Required, "{name}");
                required += 1;
            }
        }
        assert_eq!(
            BOOTSTRAP_TXNS.len() + EXEMPT_READ_TXNS.len() + required,
            declared.len(),
            "the three policies partition the surface"
        );
        assert_eq!(
            required, 39,
            "1 Bootstrap + 16 ExemptRead + 39 Required = 56"
        );

        // The other direction: a policy row for a name the registry does not declare is a stale row
        // that would keep passing after the tool was removed.
        for name in BOOTSTRAP_TXNS.iter().chain(EXEMPT_READ_TXNS) {
            assert!(
                tool(name).is_some(),
                "{name} has a policy row but the registry does not declare it"
            );
        }
        for name in EXEMPT_READ_TXNS {
            assert!(
                !BOOTSTRAP_TXNS.contains(name),
                "{name} is both Bootstrap and ExemptRead"
            );
        }

        // **AN EXEMPT TOOL MAY WRITE A RECORD, BUT IT IS NEVER DESTRUCTIVE.** The exemption covers
        // the transaction tools and the goal claim as well as the catalogue, so "read-only" is the
        // wrong name for it; what still holds is that no exempt call changes the machine, and a
        // destructive tool drifting into this list would run with no unit of work to answer for it.
        for name in EXEMPT_READ_TXNS {
            assert_eq!(
                is_destructive(name),
                Some(false),
                "{name} is exempt from the transaction gate, so it must not change the machine"
            );
        }

        // A name that is not a tool of this surface has no policy: the gate denies rather than
        // defaulting a stranger to "exempt".
        for name in ["", "files", "txn", "txn.cancel", "Files.read"] {
            assert_eq!(
                txn_policy(name),
                None,
                "{name:?} is not a tool of this surface"
            );
        }
    }

    #[test]
    fn a_bootstrap_tool_creates_the_session_and_never_runs_a_command() {
        // A Bootstrap call is admitted with no transaction at all, so it must be the act that makes
        // a transaction possible and nothing that runs a command.
        //
        // `terminal.create` **is** `destructive` in the shipped table, and the two facts do not
        // conflict: `destructive` is what `authorize` asks the token's consent for — creating a
        // process cannot be undone by a later read (`capabilities/terminal.rs`) — while Bootstrap
        // exempts the call from needing a *transaction*, because there is none yet to carry it.
        // The rule that would be a hole is the other one: no command runner may ever be Bootstrap.
        assert_eq!(
            BOOTSTRAP_TXNS,
            ["terminal.create"],
            "one tool makes a transaction possible, and this test names it"
        );
        for name in BOOTSTRAP_TXNS {
            assert_ne!(
                *name, "runs.exec",
                "a command execution cannot be admitted with no transaction"
            );
            assert_ne!(
                *name, "terminal.write",
                "typing into a shell cannot be admitted with no transaction"
            );
            assert!(
                !name.starts_with("runs."),
                "{name}: the runs capability executes commands"
            );
        }
        assert_eq!(
            is_destructive("terminal.create"),
            Some(true),
            "creating a process is not undone by a read; the token's consent still covers it"
        );
    }

    #[test]
    fn the_session_argument_map_names_every_tool_that_has_one() {
        // The gate's only way to find the session a call acts on: it resolves inheritance from this
        // map, never from a renderer's parse of the arguments.
        //
        // The expectation is written out here **independently of the implementation**: the six
        // operations on an existing session. `terminal.create` makes the session and `terminal.list`
        // takes no arguments at all, so neither names one — a map that said `terminal.* → id` would
        // send the gate looking for an `id` argument that does not exist.
        let expected: [(&str, &str); 6] = [
            ("terminal.goal", "id"),
            ("terminal.write", "id"),
            ("terminal.read", "id"),
            ("terminal.resize", "id"),
            ("terminal.close", "id"),
            ("terminal.screen", "id"),
        ];

        let declared: Vec<&str> = CAPABILITIES
            .iter()
            .flat_map(|capability| capability.tools)
            .map(|tool| tool.name)
            .collect();
        for name in &declared {
            let want = if *name == "txn.begin" {
                Some("session")
            } else {
                expected
                    .iter()
                    .find(|(tool, _)| tool == name)
                    .map(|(_, arg)| *arg)
            };
            assert_eq!(session_arg(name), want, "{name}");
        }

        // The oracle agrees with the declarations: every tool it names really declares `id`, and the
        // two terminal tools it leaves out really do not.
        let declares_id = |name: &str| {
            tool(name)
                .and_then(|declaration| declaration.schema)
                .map(|schema| {
                    schema()
                        .get("properties")
                        .and_then(|properties| properties.as_object())
                        .is_some_and(|properties| properties.contains_key("id"))
                })
                .unwrap_or(false)
        };
        for (name, arg) in expected {
            assert_eq!(arg, "id");
            assert!(declares_id(name), "{name} declares its {arg} argument");
        }
        for name in ["terminal.create", "terminal.list"] {
            assert!(
                !declares_id(name),
                "{name} declares no id argument, so it names no session"
            );
        }

        assert_eq!(
            session_arg("txn.begin"),
            Some("session"),
            "the begin names the session it binds to"
        );
        assert_eq!(
            WORKING_TXN_ARG, "txn",
            "a call overrides inheritance by naming the transaction argument"
        );

        // No prefix guessing: a name the registry does not declare names no session, whatever it
        // looks like, and every entry is one of the two argument names the map can answer with.
        for name in [
            "",
            "files",
            "txn",
            "txn.commit",
            "terminal",
            "terminal.future",
            "terminal.create",
            "terminal.list",
            "Terminal.list",
        ] {
            assert_eq!(session_arg(name), None, "{name:?}");
        }
        for name in ["terminal.write", "terminal.screen", "txn.begin"] {
            assert!(
                matches!(session_arg(name), Some("id") | Some("session")),
                "{name}"
            );
        }
        assert_eq!(
            session_arg("terminal.list"),
            None,
            "the list takes no arguments"
        );
    }

    /// **A SESSION TOOL WITH NO SCHEMA WOULD RIDE A DEVICE-SCOPE TRANSACTION.**
    ///
    /// [`session_arg`] reads a session tool's own schema to find its `id` argument. A tool of the
    /// session capability whose `schema` is `None` therefore answers `None` from that map, and a
    /// `Required` call on a live session would be gated as if it named no session — the exact hole
    /// the map exists to close. Every migrated capability declares a schema today; this test is the
    /// guard that keeps a future `terminal.*` tool from being the first exception.
    ///
    /// MUTATION (run, not asserted): deleting `schema: Some(screen_schema)` from `terminal.screen`
    /// fails here with "terminal.screen: a session-capability tool must declare its schema, or
    /// `session_arg` cannot find the session it names". Restored, green.
    #[test]
    fn every_session_capability_tool_declares_a_schema() {
        let mut without = Vec::new();
        for capability in CAPABILITIES
            .iter()
            .filter(|capability| capability.name == SESSION_CAPABILITY)
        {
            for declaration in capability.tools {
                if declaration.schema.is_none() {
                    without.push(declaration.name);
                }
            }
        }
        assert!(
            without.is_empty(),
            "these session-capability tools declare no schema, so `session_arg` cannot find the \
             session they name and a Required call would ride a device-scope transaction: {without:?}"
        );
    }

    #[test]
    fn every_tool_is_namespaced_by_its_capability_and_described() {
        for capability in CAPABILITIES {
            assert!(
                !capability.tools.is_empty(),
                "{} declares no tools",
                capability.name
            );
            for tool in capability.tools {
                assert!(
                    tool.name.starts_with(capability.name)
                        && tool.name[capability.name.len()..].starts_with('.'),
                    "{} is not namespaced under {}",
                    tool.name,
                    capability.name
                );
                assert!(!tool.name.is_empty());
                assert!(
                    !tool.description.is_empty(),
                    "{} has no description for a client to show",
                    tool.name
                );
            }
        }
    }
}
