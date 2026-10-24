//! The capability registry (design §4.5): a tool exists because a capability declares it.
//!
//! The MCP tool list, the panel's buttons, the console's rendering and the `destructive` question
//! `authorize` asks are all readings of this one table. A tool that is not here is not a tool this
//! device offers, and every caller treats `None` as a denial rather than a default.

/// One tool a capability exposes, and the fact the decision's `Ask` rule consults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolDef {
    /// The wire name a client calls, e.g. `files.read`.
    pub name: &'static str,
    /// Whether calling it does something a later read cannot undo.
    pub destructive: bool,
    /// One sentence a client shows a human; the MCP tool description is this string.
    pub description: &'static str,
}

/// One capability: a name and the tools it declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capability {
    /// The capability's own name, e.g. `files`. Every tool name is namespaced by it.
    pub name: &'static str,
    /// The tools the capability declares. The registry is the union of these lists.
    pub tools: &'static [ToolDef],
}

/// Every capability this device offers.
///
/// The order is the order a client sees in `tools/list`; it is data, so reordering it is a wire
/// change rather than a refactor.
pub const CAPABILITIES: &[Capability] = &[
    Capability {
        name: "files",
        tools: &[
            ToolDef {
                name: "files.read",
                destructive: false,
                description: "Read a file inside the device's files root.",
            },
            ToolDef {
                name: "files.write",
                destructive: true,
                description: "Write a file inside the device's files root; its parent must exist.",
            },
            ToolDef {
                name: "files.mkdir",
                destructive: false,
                description:
                    "Create one directory inside the device's files root; never overwrites.",
            },
            ToolDef {
                name: "files.list",
                destructive: false,
                description:
                    "List a directory inside the device's files root, sorted, at most 1000 entries.",
            },
        ],
    },
    Capability {
        name: "memory",
        tools: &[
            ToolDef {
                name: "memory.append",
                destructive: true,
                description: "Append one entry to the device's append-only memory.",
            },
            ToolDef {
                name: "memory.search",
                destructive: false,
                description: "Search the device's memory, newest entries first.",
            },
            ToolDef {
                name: "memory.list",
                destructive: false,
                description:
                    "Enumerate remembered entries, newest first, optionally only those carrying one tag; a search matches text, this lists what is there.",
            },
            ToolDef {
                name: "memory.export",
                destructive: false,
                description:
                    "Export the whole store as JSONL text, up to 2 MiB; the result names the entry count and whether the cap cut it.",
            },
        ],
    },
    Capability {
        name: "terminal",
        tools: &[
            // Creating a PTY starts a process, creating a serial session opens a device handle,
            // and writing to either feeds the far end; all are destructive in the registry's sense
            // (a later read cannot undo them). Reading, resizing and listing only observe or
            // reshape a session that already exists. Closing creates no new effect either: it ends
            // one, and it is the action a client needs to be able to take when a session is stuck,
            // so it does not pass through the destructive question.
            ToolDef {
                name: "terminal.create",
                destructive: true,
                description:
                    "Start a session of the requested kind; kind pty runs a local shell on a new pseudo-terminal, kind ssh opens a remote shell whose host key is pinned on first use and refused if it changes, and kind serial opens the OS port path named by port (for example COM3 or /dev/ttyUSB0) at 115200 8N1 unless baud, parity, data_bits and stop_bits say otherwise. A serial port is never enumerated or guessed. At most 16 sessions may be live, each retains 1 MiB of output, and a session idle for 30 minutes is closed.",
            },
            ToolDef {
                name: "terminal.write",
                destructive: true,
                description:
                    "Write UTF-8 text to a live session's input: a pty or ssh shell runs it as a command, and a serial session sends the bytes on the port. At most 1 MiB per call, and a write that cannot finish in 5 seconds is refused.",
            },
            ToolDef {
                name: "terminal.read",
                destructive: false,
                description:
                    "Read a session's output since a cursor and return the new cursor; only the newest 1 MiB is retained, so an older cursor is refused.",
            },
            ToolDef {
                name: "terminal.resize",
                destructive: false,
                description:
                    "Resize a live session; a local shell receives SIGWINCH and a remote shell a window-change request. A serial session has no window, so resizing it is a successful no-op. Rows and columns are 1..=1000.",
            },
            ToolDef {
                name: "terminal.close",
                destructive: false,
                description:
                    "Close a session and end its shell, SSH connection or serial handle; the id is never reused.",
            },
            ToolDef {
                name: "terminal.list",
                destructive: false,
                description:
                    "List the live sessions with their kind and start time.",
            },
            ToolDef {
                name: "terminal.screen",
                destructive: false,
                description:
                    "Read the tail of a session's output as a person would see it — escape sequences removed, the last N lines — without moving the read cursor.",
            },
        ],
    },
    Capability {
        name: "browser",
        tools: &[
            // Opening, navigating and evaluating all change what a page holds — a page's state is
            // not something a later read undoes — so they are destructive in the registry's sense.
            // Listing and screenshotting only observe. Closing a tab and shutting the browser down
            // destroy only the session, exactly as `terminal.close` does: they create no new
            // effect, and they are what a client must be able to do to stop a wedged browser
            // without asking a human first.
            ToolDef {
                name: "browser.list",
                destructive: false,
                description:
                    "List the browser's targets (id, title, url, type); the first browser tool call starts headless Chrome.",
            },
            ToolDef {
                name: "browser.open",
                destructive: true,
                description:
                    "Open a tab at a URL and wait for its document to be parsed; at most 8 tabs, and an open beyond that is refused.",
            },
            ToolDef {
                name: "browser.navigate",
                destructive: true,
                description:
                    "Navigate one tab to a URL and wait for its document to be parsed.",
            },
            ToolDef {
                name: "browser.eval",
                destructive: true,
                description:
                    "Evaluate a JavaScript expression in one tab and return its value; a result over 1 MiB is refused.",
            },
            ToolDef {
                name: "browser.screenshot",
                destructive: false,
                description:
                    "Capture one tab as a base64 PNG; a capture over 8 MiB is refused.",
            },
            ToolDef {
                name: "browser.close",
                destructive: false,
                description:
                    "Close one tab; the browser stays running and the tab id is not reused.",
            },
            ToolDef {
                name: "browser.shutdown",
                destructive: false,
                description:
                    "Stop the browser the device started; a later browser tool call starts it again.",
            },
        ],
    },
    Capability {
        name: "monitor",
        tools: &[
            // Monitoring only observes and configures the probe itself; none of these tools writes
            // a file outside the capability's own target list, starts a process, or changes
            // anything a later read cannot see. `monitor.add` and `monitor.remove` configure a
            // watch, which is why they are not destructive in the registry's sense.
            ToolDef {
                name: "monitor.add",
                destructive: false,
                description:
                    "Watch a host:port and persist the watch; tcp connects, http GETs path and is up on a status below 500 whose body contains expect when set. At most 16 targets, an interval of at least 5000 ms, and the last 60 samples are summarized.",
            },
            ToolDef {
                name: "monitor.remove",
                destructive: false,
                description:
                    "Stop watching a target by id and discard its in-memory series; removed says whether a watch existed.",
            },
            ToolDef {
                name: "monitor.list",
                destructive: false,
                description:
                    "List the watched targets with their settings and summary: up_now, since_ms, up_pct, drops and last_status from the last 60 probes.",
            },
            ToolDef {
                name: "monitor.probe",
                destructive: false,
                description:
                    "Probe one watched target now and return the sample {ts, ok, latency_ms}; it is recorded like a scheduled probe.",
            },
            ToolDef {
                name: "monitor.status",
                destructive: false,
                description:
                    "Return one target's summary over its last 60 probes: up_now, since_ms, up_pct, drops and last_status.",
            },
        ],
    },
    Capability {
        name: "runs",
        tools: &[
            // Running a command is arbitrary code, so `runs.exec` is destructive in the registry's
            // sense: no later read can undo what the process did. Listing and getting only read the
            // history file the capability maintains.
            ToolDef {
                name: "runs.exec",
                destructive: true,
                description:
                    "Run a command once and return its captured output; command and args are an argv array with no shell unless shell is true. cwd must be inside the files root (where the process starts, not a jail), stdout and stderr keep 1 MiB each, timeout_ms defaults to 60 seconds and is at most 10 minutes, and env is at most 32 variables of 4 KiB each.",
            },
            ToolDef {
                name: "runs.list",
                destructive: false,
                description:
                    "List the recent runs newest first; the history keeps 10 000 rows per file with the overflow in runs.1.jsonl, stores no output bodies, and limit defaults to 20 and is at most 100.",
            },
            ToolDef {
                name: "runs.get",
                destructive: false,
                description:
                    "Return one run's history row by id; output bodies are not stored.",
            },
        ],
    },
    Capability {
        name: "system",
        tools: &[
            // Killing ends a process the device may not own, so it is destructive. Listing and a
            // network test only observe, and neither changes what a later read sees.
            ToolDef {
                name: "system.process_list",
                destructive: false,
                description:
                    "List running processes on this device: pid, executable name and resident memory, optionally filtered by name.",
            },
            ToolDef {
                name: "system.process_kill",
                destructive: true,
                description:
                    "Terminate processes by pid or by exact name; the result names the pids that were signalled and the pids that could not be.",
            },
            ToolDef {
                name: "system.net_test",
                destructive: false,
                description:
                    "Resolve a host name and connect to one TCP port from this device, returning the addresses resolved and the connect latency; ICMP needs raw sockets, so it is not offered.",
            },
        ],
    },
    Capability {
        name: "state",
        tools: &[ToolDef {
            name: "state.get",
            destructive: false,
            // This is the console's device page. There is no console route for it: an operator
            // mints a grant naming `state.get` and reads the device through `/mcp`, which also puts
            // the call in the console's audit index.
            description:
                "Read the device's state document plus its health: status (ok or degraded), release version, pid, and whether the state document is poisoned (unreadable). A poisoned document is returned as null state with degraded health rather than a failed call.",
        }],
    },
];

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
    fn the_registry_declares_the_phase_four_tools_and_the_first_session_capability() {
        let tools: Vec<&str> = CAPABILITIES
            .iter()
            .flat_map(|capability| capability.tools)
            .map(|tool| tool.name)
            .collect();

        assert_eq!(
            tools,
            [
                "files.read",
                "files.write",
                "files.mkdir",
                "files.list",
                "memory.append",
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
                "state.get",
            ],
            "the registry is the whole surface: no tool exists outside it"
        );
    }

    #[test]
    fn the_destructive_flag_is_the_declared_value() {
        for (name, destructive) in [
            ("files.read", false),
            ("files.write", true),
            ("files.mkdir", false),
            ("files.list", false),
            ("memory.append", true),
            ("memory.search", false),
            ("terminal.create", true),
            ("terminal.write", true),
            ("terminal.read", false),
            ("terminal.resize", false),
            ("terminal.close", false),
            ("terminal.list", false),
            ("browser.list", false),
            ("browser.open", true),
            ("browser.navigate", true),
            ("browser.eval", true),
            ("browser.screenshot", false),
            ("browser.close", false),
            ("browser.shutdown", false),
            ("monitor.add", false),
            ("monitor.remove", false),
            ("monitor.list", false),
            ("monitor.probe", false),
            ("monitor.status", false),
            ("runs.exec", true),
            ("runs.list", false),
            ("runs.get", false),
            ("state.get", false),
        ] {
            assert_eq!(
                is_destructive(name),
                Some(destructive),
                "{name}'s destructive flag is the fact authorize's Ask rule consults"
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
            assert_eq!(tool(name), None, "{name:?}");
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
