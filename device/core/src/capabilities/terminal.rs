//! `terminal`: the declaration. The implementation lives in `device/src/capabilities/terminal.rs`.

use serde_json::{json, Value};

use crate::registry::{Capability, ToolDef};
/// The largest row or column count a session accepts.
pub const MAX_DIMENSION: u16 = 1000;
/// The most trailing lines `terminal.screen` returns.
pub const MAX_SCREEN_LINES: usize = 1000;
/// Serial framing defaults: 115200 8N1.
pub const DEFAULT_BAUD: u32 = 115_200;
pub const DEFAULT_DATA_BITS: u8 = 8;
pub const DEFAULT_STOP_BITS: u8 = 1;
/// Terminal defaults for a session that names no size.
pub const DEFAULT_ROWS: u16 = 24;
pub const DEFAULT_COLS: u16 = 80;
/// How many trailing lines `terminal.screen` returns when the caller names no count.
pub const DEFAULT_SCREEN_LINES: usize = 60;

/// The manual entry, rendered into the guide beside the tool list.
pub const GUIDE: &str = "The terminal capability runs a session and moves bytes through it. terminal.create starts one: a local shell (pty), an SSH shell whose host key is pinned on first use (ssh), or an OS serial port named by the caller (serial, never enumerated). The id it returns is what every other terminal tool names. terminal.write sends UTF-8 text to the session; terminal.read returns output after a cursor and the next cursor, optionally parking up to 30 s for new output and optionally paging the retained tail. terminal.resize only reshapes the window (a serial session has none), terminal.screen returns the tail as a person would see it without moving the cursor, terminal.list names the live sessions, and terminal.close ends one and never reuses its id. At most 16 sessions are live, each retains 1 MiB of output, and a session idle for 30 minutes is closed.";

/// This capability's declaration: the one place its tools and their schemas are written down.
pub const CAPABILITY: Capability = Capability {
    name: "terminal",
    guide: GUIDE,
    tools: &[
        ToolDef {
            name: "terminal.create",
            destructive: true,
            description: "Start a session of the requested kind; kind pty runs a local shell on a new pseudo-terminal, kind ssh opens a remote shell whose host key is pinned on first use and refused if it changes, and kind serial opens the OS port path named by port (for example COM3 or /dev/ttyUSB0) at 115200 8N1 unless baud, parity, data_bits and stop_bits say otherwise. A serial port is never enumerated or guessed. At most 16 sessions may be live, each retains 1 MiB of output, and a session idle for 30 minutes is closed.",
            schema: Some(create_schema),
        },
        ToolDef {
            name: "terminal.write",
            destructive: true,
            description: "Write UTF-8 text to a live session's input: a pty or ssh shell runs it as a command, and a serial session sends the bytes on the port. At most 1 MiB per call, and a write that cannot finish in 5 seconds is refused.",
            schema: Some(write_schema),
        },
        ToolDef {
            name: "terminal.read",
            destructive: false,
            description: "Read a session's output since a cursor and return the new cursor; only the newest 1 MiB is retained, so an older cursor is refused. Optional wait_ms (0-30000) parks until output arrives or the deadline passes; a terminal client should use it instead of polling. Optional limit pages the retained tail: at most that many bytes, oldest first, and the returned cursor advances by what the page carried.",
            schema: Some(read_schema),
        },
        ToolDef {
            name: "terminal.resize",
            destructive: false,
            description: "Resize a live session; a local shell receives SIGWINCH and a remote shell a window-change request. A serial session has no window, so resizing it is a successful no-op. Rows and columns are 1..=1000.",
            schema: Some(resize_schema),
        },
        ToolDef {
            name: "terminal.close",
            destructive: false,
            description: "Close a session and end its shell, SSH connection or serial handle; the id is never reused.",
            schema: Some(close_schema),
        },
        ToolDef {
            name: "terminal.list",
            destructive: false,
            description: "List the live sessions with their kind and start time.",
            schema: Some(list_schema),
        },
        ToolDef {
            name: "terminal.screen",
            destructive: false,
            description: "Read the tail of a session's output as a person would see it — escape sequences removed, the last N lines — without moving the read cursor.",
            schema: Some(screen_schema),
        },
    ],
};

fn create_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["pty", "ssh", "serial"],
                    "description": "The kind of session to start. pty runs a local shell on a new pseudo-terminal; ssh opens a remote shell whose host key is pinned on first use; serial opens the OS port path the caller names at 115200 8N1 unless framing is given."
                },
                "shell": {
                    "type": "string",
                    "description": "For kind pty: the program to run. The default is bash on Linux/macOS and pwsh on Windows. This is one program path, not a command line with arguments."
                },
                "host": {
                    "type": "string",
                    "description": "For kind ssh: the server's DNS name or IP address."
                },
                "port": {
                    "oneOf": [
                        {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 65535,
                            "description": "For kind ssh: the server's TCP port."
                        },
                        {
                            "type": "string",
                            "minLength": 1,
                            "description": "For kind serial: the OS port path, e.g. COM3 on Windows or /dev/ttyUSB0 on Unix. The capability never enumerates or guesses a port."
                        }
                    ]
                },
                "user": {
                    "type": "string",
                    "description": "For kind ssh: the remote user whose shell runs."
                },
                "auth": {
                    "type": "object",
                    "properties": {
                        "kind": {
                            "type": "string",
                            "enum": ["password", "key"],
                            "description": "password sends a password; key presents an OpenSSH private key."
                        },
                        "password": {
                            "type": "string",
                            "description": "For kind password: the password. Never logged."
                        },
                        "path": {
                            "type": "string",
                            "description": "For kind key: an absolute path to an OpenSSH private key. Never logged; an unreadable or relative path is refused."
                        }
                    },
                    "required": ["kind"],
                    "additionalProperties": false
                },
                "host_key": {
                    "type": "string",
                    "description": "For kind ssh: the SHA-256 fingerprint the server's key must match, e.g. SHA256:... . When given, no trust-on-first-use is performed, and a mismatch is refused before authentication."
                },
                "baud": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": u32::MAX,
                    "description": format!("For kind serial: bits per second (default {}). A rate the port rejects is an error, not a fallback.", DEFAULT_BAUD)
                },
                "parity": {
                    "type": "string",
                    "enum": ["none", "odd", "even"],
                    "description": "For kind serial: the parity mode (default none)."
                },
                "data_bits": {
                    "type": "integer",
                    "enum": [5, 6, 7, 8],
                    "description": format!("For kind serial: data bits per character (default {}).", DEFAULT_DATA_BITS)
                },
                "stop_bits": {
                    "type": "integer",
                    "enum": [1, 2],
                    "description": format!("For kind serial: stop bits per character (default {}).", DEFAULT_STOP_BITS)
                },
                "rows": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_DIMENSION,
                    "description": format!("Terminal rows (default {DEFAULT_ROWS}).")
                },
                "cols": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_DIMENSION,
                    "description": format!("Terminal columns (default {DEFAULT_COLS}).")
                }
            },
            "required": ["kind"],
            "additionalProperties": false
    })
}

fn write_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The id terminal.create returned."
                },
                "data": {
                    "type": "string",
                    "description": "UTF-8 text for the session's input; write \"\\n\" to run a command. At most 1 MiB per call, and a write that cannot finish in 5 seconds is refused."
                }
            },
            "required": ["id", "data"],
            "additionalProperties": false
    })
}

fn read_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The id terminal.create returned."
                },
                "cursor": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Read output after this cursor. 0 (the default) reads all retained output; the returned cursor is the next call's value."
                },
                "wait_ms": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 30000,
                    "description": "Wait up to this long for new output when the cursor is already at the end; absent/0 answers immediately. The device caps the wait at 30 s."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Return at most this many bytes from the cursor, oldest first; the returned cursor advances by what the page carried, so the next call continues. Absent returns the whole retained tail."
                }
            },
            "required": ["id"],
            "additionalProperties": false
    })
}

fn resize_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The id terminal.create returned."
                },
                "rows": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_DIMENSION,
                    "description": "The new row count."
                },
                "cols": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_DIMENSION,
                    "description": "The new column count."
                }
            },
            "required": ["id", "rows", "cols"],
            "additionalProperties": false
    })
}

fn close_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The id terminal.create returned. Its shell is killed and the id is not reused."
                }
            },
            "required": ["id"],
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

fn screen_schema() -> Value {
    json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "A session id from terminal.create or terminal.list."
                },
                "lines": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": MAX_SCREEN_LINES,
                    "description": format!("How many trailing lines to return (default {DEFAULT_SCREEN_LINES}).")
                }
            },
            "required": ["id"],
            "additionalProperties": false
    })
}
