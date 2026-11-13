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
    Capability {
        name: "host",
        tools: &[
            ToolDef {
                name: "host.facts",
                destructive: false,
                // Read-only, and the console's device page is the caller: an operator should not need a
                // grant to learn what machine a device is.
                description:
                    "Read what this machine is running on: operating system and build, host name, uptime, CPU cores, memory and disk totals, and the Windows sessions with their state (active, connected, disconnected, idle). Also whether the per-user helper is registered, its pid and session. Facts that cannot be read are zero or empty, never an error.",
                schema: None,
            },
            ToolDef {
                name: "host.restart",
                destructive: true,
                // The app ends and the launcher starts it again: every live session ends with it.
                // The budget is the launcher's, so it is stated where the operator chooses, not
                // discovered by repeating the call until the device stays down.
                description:
                    "Restart this machine's app through its launcher. Terminals, browser tabs and runs on this machine end, and the machine goes offline briefly. The launcher allows three consecutive restarts, so repeated restarts can leave the device down; a standalone process refuses.",
                schema: None,
            },
        ],
        guide: "",
    },
    Capability {
        name: "screen",
        tools: &[ToolDef {
            name: "screen.capture",
            destructive: false,
            // The only capability that runs in the logged-in user's session: the helper captures a
            // desktop the service cannot see, and refuses rather than answering black.
            description:
                "Capture the logged-in user's screen as a PNG through the per-user helper. A session that is locked, disconnected, or has no helper running is refused with a sentence; a desktop nobody is looking at is not a picture.",
            schema: None,
        }],
        guide: "",
    },
    crate::capabilities::forward::CAPABILITY,
    Capability {
        name: "state",
        tools: &[ToolDef {
            name: "state.get",
            destructive: false,
            // This is the console's device page. There is no console route for it: an operator
            // mints a grant naming `state.get` and reads the device through `/mcp`, which also puts
            // the call in the console's audit index.
            description:
                "Read this machine's state document plus its health: status (ok or degraded), release version, pid, whether the state document is poisoned (unreadable), and the last ten events the device recorded. A poisoned document is returned as null state with degraded health rather than a failed call.",
            schema: None,
        }],
        guide: "",
    },
    Capability {
        name: "journal",
        tools: &[
            ToolDef {
                name: "journal.get",
                destructive: false,
                // Read-only: it answers presence and coverage, never an event's payload, and a later
                // read cannot be changed by it.
                description:
                    "Read this machine's journal: which of the traces you name are still retained, and the journal's retained bounds. Presence and coverage only, never an event's payload.",
                schema: None,
            },
            ToolDef {
                name: "journal.read",
                destructive: false,
                // Read-only, and summarising: a row is drawn from its own frame plus its tool and
                // outcome, and only the payload's SIZE is reported. A read that relayed payloads
                // would let one call carry a machine's file contents, which is an export.
                description:
                    "Read the rows of this machine's journal, newest first and bounded: each row's sequence, time and type, and on a call its tool, outcome and the door it arrived by. Summaries only; payloads are sized, not sent.",
                schema: None,
            },
            ToolDef {
                name: "journal.day",
                destructive: false,
                // Read-only and aggregated: counts of a day's calls by the door they arrived by,
                // never a row and never a payload. The console asks it for the day its header
                // counts, because a page of rows is not a day.
                description:
                    "Count this machine's journal for a time range you name: the calls and failures of each arrival door (console, panel, mesh, loopback, unknown) that finished inside the range, the journal's retained bounds, and whether the scan was cut. Aggregates only; no rows and no payloads.",
                schema: None,
            },
        ],
        guide: "",
    },
    Capability {
        name: "plugins",
        tools: &[
            // A plugin is an MCP server installed by path. Adding, removing and restarting it are
            // destructive in the registry's sense (they start or stop code, and a later read cannot
            // undo that); listing, inspecting and reading a log only observe. The tools are ordinary
            // registry tools, so the token scope, the audit record and the console need no special
            // case: they are the operator's surface for the plugin host itself.
            ToolDef {
                name: "plugins.list",
                destructive: false,
                description:
                    "List the installed plugins with their state and tool count; a plugin's tools are advertised only while it is running.",
                schema: None,
            },
            ToolDef {
                name: "plugins.inspect",
                destructive: false,
                description:
                    "Read a plugin directory's plugin.json without starting it or recording it, so a path can be checked before plugins.add.",
                schema: None,
            },
            ToolDef {
                name: "plugins.add",
                destructive: true,
                description:
                    "Install the MCP server in a directory: validate plugin.json, start it, and advertise its tools as <namespace>.<tool>; installing registers the path and runs its code with this machine's permissions.",
                schema: None,
            },
            ToolDef {
                name: "plugins.remove",
                destructive: true,
                description:
                    "Remove a plugin by namespace: forget its record and stop its process, never deleting its source directory.",
                schema: None,
            },
            ToolDef {
                name: "plugins.restart",
                destructive: true,
                description:
                    "Stop and start a plugin now, resetting its restart budget after a failure.",
                schema: None,
            },
            ToolDef {
                name: "plugins.logs",
                destructive: false,
                description:
                    "Read the last lines of a plugin's log (default 200, at most 2000).",
                schema: None,
            },
        ],
        guide: "",
    },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_declares_the_whole_surface_and_thirteen_capabilities() {
        let tools: Vec<&str> = CAPABILITIES
            .iter()
            .flat_map(|capability| capability.tools)
            .map(|tool| tool.name)
            .collect();

        assert_eq!(
            CAPABILITIES.len(),
            13,
            "thirteen capabilities declare the surface"
        );
        assert_eq!(tools.len(), 52, "the surface is 52 tools");
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
        let expected: [(&str, bool); 52] = [
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
