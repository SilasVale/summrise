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
                description: "Read a file inside one of this machine's files roots: workspace by default, or shared when the operator configured it (the optional root argument).",
            },
            ToolDef {
                name: "files.write",
                destructive: true,
                description: "Write a file inside one of this machine's files roots; its parent must exist. Pass root: \"shared\" to use the operator-configured second root.",
            },
            ToolDef {
                name: "files.mkdir",
                destructive: false,
                description:
                    "Create one directory inside one of this machine's files roots; never overwrites. Pass root: \"shared\" to use the operator-configured second root.",
            },
            ToolDef {
                name: "files.list",
                destructive: false,
                description:
                    "List a directory inside one of this machine's files roots, sorted, at most 1000 entries. Pass root: \"shared\" to use the operator-configured second root.",
            },
        ],
    },
    Capability {
        name: "memory",
        tools: &[
            ToolDef {
                name: "memory.append",
                destructive: true,
                description: "Append one entry to this machine's append-only memory and return its id; the id is what memory.update and memory.delete name.",
            },
            ToolDef {
                name: "memory.update",
                destructive: true,
                description:
                    "Replace the text and tags of the entry an id names; both are required, tags is the complete new list (send [] to clear them), validation is exactly memory.append's, and an id that is not live is refused as no_such_entry.",
            },
            ToolDef {
                name: "memory.delete",
                destructive: true,
                description:
                    "Retire one remembered entry by id; every later read omits it, and a second delete is refused as no_such_entry.",
            },
            ToolDef {
                name: "memory.search",
                destructive: false,
                description: "Search this machine's memory, newest entries first.",
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
                    "Read a session's output since a cursor and return the new cursor; only the newest 1 MiB is retained, so an older cursor is refused. Optional wait_ms (0-30000) parks until output arrives or the deadline passes; a terminal client should use it instead of polling. Optional limit pages the retained tail: at most that many bytes, oldest first, and the returned cursor advances by what the page carried.",
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
                name: "browser.cast",
                destructive: false,
                description:
                    "The newest frame of a live screencast of one tab, as a base64 JPEG; a frame over 8 MiB is refused, and the stream stops itself after 60 s unpolled.",
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
                    "Stop the browser this machine started; a later browser tool call starts it again.",
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
                    "List running processes on this machine: pid, executable name and resident memory, optionally filtered by name.",
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
                    "Resolve a host name and connect to one TCP port from this machine, returning the addresses resolved and the connect latency; ICMP needs raw sockets, so it is not offered.",
            },
        ],
    },
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
            },
            ToolDef {
                name: "host.restart",
                destructive: true,
                // The app ends and the launcher starts it again: every live session ends with it.
                // The budget is the launcher's, so it is stated where the operator chooses, not
                // discovered by repeating the call until the device stays down.
                description:
                    "Restart this machine's app through its launcher. Terminals, browser tabs and runs on this machine end, and the machine goes offline briefly. The launcher allows three consecutive restarts, so repeated restarts can leave the device down; a standalone process refuses.",
            },
        ],
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
        }],
    },
    Capability {
        name: "forward",
        tools: &[
            // A rule's values decide who can reach what and which host is trusted to carry it, so
            // `forward.add` and `forward.remove` are the operator's own act and are DESTRUCTIVE in
            // this registry's sense: they change a listening socket and persist a credential, and a
            // later read cannot undo that. `forward.list` only observes.
            ToolDef {
                name: "forward.add",
                destructive: true,
                description:
                    "Add or replace a forward rule: `listen` is bound on THIS machine, `via` makes the SSH connection, and that host's sshd dials `target`. The far host therefore gains NO listening port. `host_key` optionally pins the fingerprint `via`'s key must match — a rule runs with nobody present, so it is the one object here that should not decide a host's identity by itself. Nothing is created until the rule is persisted; a rule that cannot bind stays visible with its error rather than retrying forever.",
            },
            ToolDef {
                name: "forward.remove",
                destructive: true,
                description:
                    "Remove a rule and stop its executor; `removed` says whether one was configured. The listener is closed with it, so the address becomes bindable again.",
            },
            ToolDef {
                name: "forward.list",
                destructive: false,
                description:
                    "List every rule with its live state: whether the listener is bound, since when, how many connections it has carried, how many bind attempts it has made, and what its last failure said. An unbound rule reports its error rather than a plausible-looking zero.",
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
                "Read this machine's state document plus its health: status (ok or degraded), release version, pid, whether the state document is poisoned (unreadable), and the last ten events the device recorded. A poisoned document is returned as null state with degraded health rather than a failed call.",
        }],
    },
    Capability {
        name: "journal",
        tools: &[ToolDef {
            name: "journal.get",
            destructive: false,
            // Read-only: it answers presence and coverage, never an event's payload, and a later
            // read cannot be changed by it.
            description:
                "Read this machine's journal: which of the traces you name are still retained, and the journal's retained bounds. Presence and coverage only, never an event's payload.",
        }],
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
            },
            ToolDef {
                name: "plugins.inspect",
                destructive: false,
                description:
                    "Read a plugin directory's plugin.json without starting it or recording it, so a path can be checked before plugins.add.",
            },
            ToolDef {
                name: "plugins.add",
                destructive: true,
                description:
                    "Install the MCP server in a directory: validate plugin.json, start it, and advertise its tools as <namespace>.<tool>; installing registers the path and runs its code with this machine's permissions.",
            },
            ToolDef {
                name: "plugins.remove",
                destructive: true,
                description:
                    "Remove a plugin by namespace: forget its record and stop its process, never deleting its source directory.",
            },
            ToolDef {
                name: "plugins.restart",
                destructive: true,
                description:
                    "Stop and start a plugin now, resetting its restart budget after a failure.",
            },
            ToolDef {
                name: "plugins.logs",
                destructive: false,
                description:
                    "Read the last lines of a plugin's log (default 200, at most 2000).",
            },
        ],
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
        assert_eq!(tools.len(), 50, "the surface is 50 tools");
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
        let expected: [(&str, bool); 50] = [
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
