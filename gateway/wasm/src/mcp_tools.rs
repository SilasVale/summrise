//! THE MCP TOOL TABLE — the console's catalogue of device tools, and the ONE place it is written down.
//!
//! **THE DEFECT THIS MODULE EXISTS TO REMOVE.** `gateway/src/mcp-tools.ts` (823 lines) restated what each device
//! plugin's Rust `tools.rs` already defines, and `isDeviceDirectTool()` in `mcp.ts` was a SECOND copy of a
//! decision this side owns. The repository held the copies together with a gate that reads the device's
//! generated spec (`gateway/test/mcp-handler.test.mjs` against `agent/spec-tools.json`), which catches a MISSING
//! tool and a dropped parameter — but nothing could catch a description that drifted, and the file itself said
//! so: round-554's comment records that 21 of the agent's 49 tools were invisible AND uncalled with every gate
//! green, because the snapshot the comment told a reader to refresh was a hand-typed copy of this very list.
//!
//! **SO THE TABLE IS RUST NOW, AND THE TYPESCRIPT IS ITS EMISSION.** `render_typescript()` below writes
//! `gateway/src/mcp-tools.ts`; `the_typescript_table_is_what_this_module_emits` fails when the committed file
//! and this table disagree, and the file is regenerated with
//!
//! ```text
//!     cd gateway/wasm && SUMMRISE_REFRESH_MCP_TOOLS=1 cargo test mcp_tools
//! ```
//!
//! The TypeScript survives at all for ONE reason, and it is a reason with a date on it: `plugins/mcp.ts` is
//! still registered, and it is the ROLLBACK for the cutover — `index.ts` hands `/mcp` to the `WASM_GATE` service
//! binding when it is present and falls through to the TypeScript plugin when it is not. Deleting
//! `mcp-tools.ts` today would leave that fallback serving an MCP endpoint with ZERO tools. The deletion is the
//! last step of this landing, after the deploy that proves the worker serves the surface.
//!
//! **WHY THE SCHEMA IS A JSON STRING RATHER THAN A `serde_json::Value`.** Two reasons, both byte-level. A
//! `Value` in a `const` cannot be built without a macro call per field (and `serde_json::json!` does not
//! preserve key order — it is the `Map` under `preserve_order` that does), and the ORDER of `properties` is
//! what `tools/list` puts on the wire. A raw JSON string parses into an order-preserving `Map` at the one place
//! the response is built, and it reads as the wire format it is.
//!
//! **NOTES THAT WERE COMMENTS IN THE TYPESCRIPT AND ARE THE REASON A FIELD IS SHAPED AS IT IS** — carried here
//! because the emitted file can hold data but not the history of why the data is right:
//!
//!   * `terminal_execute`'s `required` is `["input"]`, NOT `["session_id"]`. The device makes `session_id`
//!     optional and has a whole non-session branch, and the relay never injects one — so requiring it here
//!     forbade a schema-validating client from making a call the device supports. The device's own `required` is
//!     `["command"]`, which is `input` on this side of the declared rename (`mcp.rs`'s `PARAM_RENAMES`).
//!   * `terminal_read`'s description was FALSE past 1 MiB of spill and was corrected on the device in round 21 —
//!     while this hand-copied string kept serving it to every console client. A single read returns AT MOST
//!     1 MiB and then the window's TAIL, so a `start` greater than the offset you asked for is the only signal
//!     that the head was withheld, and no offset can retrieve it.
//!
//! **AND THAT TOOL'S COPY WAS STILL A LOSSY PARAPHRASE — FOUND BY THIS MODULE, AND FIXED HERE RATHER THAN
//! WAIVED.** `gateway/test/mcp-handler.test.mjs` has two prose gates: one that every field the device names
//! appears in the console's text, and a STRONGER one that the device's whole description appears verbatim
//! inside it. The second was green for `terminal_read` — but only because `consoleDescriptionOf` scans the TS
//! TEXT, and the hand-written entry carried a `//` comment above its description that QUOTED the older,
//! longer text, so the reader extracted the comment instead of the string. The moment the file became
//! generated that comment was gone, the reader saw the real literal, and containment failed: the console's
//! copy had dropped `Reads work on closed sessions (retained history).` — a fact a model driving this tool
//! needs. The description below is the DEVICE's own text, which is what the gate asks for and what every
//! other tool here already carries; the corpus was re-recorded, so both implementations still agree byte for
//! byte on the one table.
//!   * `browser_screenshot` advertises `fullPage`, not `full_page`: arguments are forwarded VERBATIM to
//!     playwright-mcp, which declares `fullPage`. The snake_case spelling was silently dropped, so a full-page
//!     request returned a viewport shot and nothing said so.
//!   * `browser_wait` advertises `text`, `text_gone` and `time`, and requires NOTHING — the shipped
//!     `@playwright/mcp` 0.0.79 server (`browser_wait_for`) has exactly those three, all optional, and requires
//!     at least one of them. This side used to advertise a REQUIRED `condition` plus a `timeout_s`, neither of
//!     which the server has: a schema-validating client could not have made a working call.
//!   * `terminal_open`'s `device` description is deliberately LONGER than the shared one below ("Device name
//!     from the console Devices list."), because that is the text the console has always served for this tool.
//!     A table that "tidied" it would be a wire change.
//!
//! **AND `isDeviceDirectTool` IS THE OTHER HALF OF THE SAME DEFECT.** A name registered here and not routed is
//! registered-but-uncallable: `tools/call` looks the name up in this table and then asks `mcp.rs` where it goes,
//! and a name in only the first reaches `No route for registered tool …` at call time. `every_tool_has_a_route`
//! below is the gate for that, and it is the reason the two decisions now sit in one crate.

use serde_json::{Map, Value};

/// One tool as the console advertises it. FIELD ORDER IS THE WIRE FORMAT, which is why this is a struct and not
/// a `Value`: `name`, `description`, `inputSchema` — the order `tools/list` serializes.
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    /// The `inputSchema`, as JSON. Parsed at response time so the property ORDER is the parsed order (see the
    /// module header for why it is a string and not a `Value`).
    pub schema: &'static str,
}

/// THE TABLE. `gateway/src/mcp-tools.ts` is emitted from it, and `agent/spec-tools.json` is the device's own
/// catalogue that `gateway/test/mcp-handler.test.mjs` holds this list against.
pub const TOOLS: &[Tool] = &[
    Tool {
        name: "terminal_open",
        description: "Open a terminal connection. Kind: 'pty' (local shell; target optional — blank = default shell), 'ssh' (target=user@host:port), or 'serial' (target=port_name, optional ?baud=N&parity=E&data=8&stop=1 e.g. /dev/ttyUSB0?baud=9600&parity=even&data=8&stop=1, default 115200 8N1). THE DEVICE HAS A SESSION CAP: opening one when it is reached EVICTS the idle-longest session, so a session you opened earlier can disappear — expect a later call on it to fail, and check terminal_list before assuming it is still there. Returns session ID.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name from the console Devices list. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "kind": {
      "type": "string",
      "enum": [
        "pty",
        "ssh",
        "serial"
      ]
    },
    "target": {
      "type": "string",
      "description": "pty: optional (blank = default shell); ssh: user@host:port; serial: port_name (?baud=N&parity=E&data=8&stop=1 optional)"
    },
    "password": {
      "type": "string",
      "description": "SSH password (optional — keychain/file store fallback)"
    },
    "rows": {
      "type": "integer",
      "description": "Initial terminal rows. Default 0 (backend default)."
    },
    "cols": {
      "type": "integer",
      "description": "Initial terminal columns. Default 0 (backend default)."
    },
    "data_bits": {
      "type": "integer",
      "description": "(serial) Data bits 5-8. Overrides the target string."
    },
    "parity": {
      "type": "string",
      "description": "(serial) Parity: none|odd|even. Overrides the target string."
    },
    "stop_bits": {
      "type": "integer",
      "description": "(serial) Stop bits 1 or 2. Overrides the target string."
    },
    "key_path": {
      "type": "string",
      "description": "(ssh) Path to a private key file. When set, public-key auth is used; password (if any) is the key passphrase."
    },
    "auto_reconnect": {
      "type": "boolean",
      "description": "(serial) Auto-reconnect when the port disappears (unplug / device reboot): the session stays open and re-opens the SAME port with the SAME framing when it reappears. Default false."
    }
  },
  "required": [
    "kind"
  ]
}"#,
    },
    Tool {
        name: "terminal_screen",
        description: "Get the current on-screen text of a terminal session — the tail of the output buffer (ANSI-stripped), for AI readability. Returns up to `lines` lines (default 60).",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "session_id": {
      "type": "string"
    },
    "lines": {
      "type": "integer",
      "description": "Number of lines from the tail. Default 60."
    }
  },
  "required": [
    "session_id"
  ]
}"#,
    },
    Tool {
        name: "terminal_execute",
        description: "Run a command. If `session_id` is given, writes the command to that session and waits for output (prompt-marker detection on PTY shells, quiet-period fallback otherwise). Otherwise spawns a local shell with enforced timeout. Session mode returns {kind, state, text, read_from, wait_reason, exit_code, truncated, still_running}: state=done means text is COMPLETE; partial/timeout means text is a PREFIX and `still_running=true` — the command is STILL RUNNING, continue with terminal_read(offset=read_from) until you see the prompt/exit. NEVER re-run a command or open a new session just because a partial was returned: the output arrives in the SAME session's buffer; opening new sessions (terminal_open) while old commands run is what causes output to look interleaved/queued. Long silent SSH commands: prefer run_in_background:true or bigger timeout_secs (idle window scales: ssh 3s, serial 4s, pty 1s). Local mode returns {kind, text, truncated}. `run_in_background: true` (session mode) writes the command and returns immediately with a read_from cursor — collect output via terminal_read; do NOT busy-poll, the wait loop is the foreground path. Note: a quiet timeout or truncation does not prove the foreground command exited.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "session_id": {
      "type": "string"
    },
    "input": {
      "type": "string",
      "description": "The command to run in the session"
    },
    "timeout_secs": {
      "type": "integer",
      "description": "Max wait time in seconds. Default 30."
    },
    "quiet_ms": {
      "type": "integer",
      "description": "(fallback) Quiet period in ms before considering output complete. Default 200."
    },
    "run_in_background": {
      "type": "boolean",
      "description": "(Session mode) Write the command and return immediately with a read_from cursor; collect via terminal_read. Default false."
    },
    "intent": {
      "type": "string",
      "description": "Optional: WHY you are running this, in one sentence. Recorded with the command and shown to the operator on the session's path — it is what turns a list of commands into a readable account of what you were doing and why. Send it whenever the reason is not obvious from the command itself."
    },
    "considered": {
      "type": "array",
      "items": {
        "type": "string"
      },
      "description": "Optional: the alternatives you passed over for this step (short labels, max 8). Recorded and shown as the branches NOT taken, which is the part a command log can never reconstruct. Send it when you made a real choice — not for the only way to do something."
    },
    "plan_step": {
      "type": "integer",
      "description": "Optional: which step of your declared terminal_plan this command advances (1-based). Lets the operator see the plan being followed — or quietly abandoned — instead of having to guess which command served which step."
    },
    "run_id": {
      "type": "string",
      "description": "Optional: the id returned by run_begin, naming the execution this command belongs to. One run spans many commands AND browser actions, so this is what lets an operator see a coherent piece of work instead of the day's traffic. Pass back the id verbatim."
    },
    "approval_id": {
      "type": "string",
      "description": "Optional: the approval id from a result whose state was `awaiting_approval`. If the operator has since approved, the command runs without asking again; the permit covers exactly this command text, once. Omit it for a normal execute."
    }
  },
  "required": [
    "input"
  ]
}"#,
    },
    Tool {
        name: "terminal_write",
        description: "Write data to a terminal session, or assert a line BREAK on a serial one. `data` is UTF-8 text (JSON strings cannot carry arbitrary bytes); use `data_base64` for binary frames (control bytes, non-UTF-8 serial protocols) — it is decoded and written exactly as given. For shell commands on Unix devices (serial/ssh to Linux), the command must end with a newline (\\n) — otherwise the shell joins it with whatever is typed next, mangling both. For Windows PowerShell use \\r\\n. Control characters (e.g. \\u0003 for Ctrl+C) are sent verbatim and need no newline. `break_ms` (serial sessions only) asserts a BREAK on the line for that many milliseconds — the signal that interrupts a bootloader's autoboot or drops into a ROM monitor, and the one thing a browser terminal cannot send.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "session_id": {
      "type": "string"
    },
    "data": {
      "type": "string",
      "description": "UTF-8 text to write. Required unless data_base64 or break_ms is given."
    },
    "data_base64": {
      "type": "string",
      "description": "Base64-encoded bytes to write (for binary frames). Takes precedence over data."
    },
    "break_ms": {
      "type": "integer",
      "description": "Assert a BREAK on a serial line for this many ms (default 250, max 5000) — the signal that interrupts a bootloader's autoboot or drops into a ROM monitor. Serial sessions only; a PTY/SSH session refuses by name. Takes precedence over data."
    }
  },
  "required": [
    "session_id"
  ]
}"#,
    },
    Tool {
        name: "terminal_read",
        description: "Read buffered output from a terminal session. Non-destructive: uses a cursor so repeating the call without `offset` returns only new output since last read. `offset` is an ABSOLUTE byte offset into the session's byte stream; the response's `start`/`end` are the absolute span actually returned. A single read returns AT MOST 1 MiB: for a session that has produced more, the oldest bytes in the requested window are not returned, and `start` will be GREATER than the `offset` you asked for — that gap is the only signal, and it cannot be retrieved by any offset, so treat a `start` above your `offset` as the head being unavailable. Reads work on closed sessions (retained history). ANSI escapes are stripped and line endings normalized by default (AI-readable); pass `clean: false` for raw bytes.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "session_id": {
      "type": "string"
    },
    "offset": {
      "type": "integer",
      "description": "ABSOLUTE byte offset to start reading from. 0 = beginning. Default = last cursor position."
    },
    "clean": {
      "type": "boolean",
      "description": "Strip ANSI escapes and normalize \\r\\n → \\n. Default true."
    }
  },
  "required": [
    "session_id"
  ]
}"#,
    },
    Tool {
        name: "terminal_resize",
        description: "Resize a terminal session (PTY or SSH). `rows`/`cols` are OPTIONAL and default to 24x80 — the handler has always defaulted them, so declaring them required was a schema claim the code contradicted, and it forbade a call the device answers.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "session_id": {
      "type": "string"
    },
    "rows": {
      "type": "integer"
    },
    "cols": {
      "type": "integer"
    }
  },
  "required": [
    "session_id"
  ]
}"#,
    },
    Tool {
        name: "terminal_select",
        description: "Set the active terminal session. This is a client-liveness heartbeat: it keeps the idle sweeper from reaping a quiet-but-watched session.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "session_id": {
      "type": "string"
    }
  },
  "required": [
    "session_id"
  ]
}"#,
    },
    Tool {
        name: "terminal_plan",
        description: "Declare, revise, clear or read this session's PLAN — the steps you intend to take, in order. Call it before starting a multi-step task so the operator can see what you are about to do and judge it; call it again with a revised list when the plan changes. Pass an empty array to clear it. With `plan` omitted it just returns the current plan. Steps are short labels, not explanations — put the reasoning for a specific command in terminal_execute's `intent`, and name the step a command advances with terminal_execute's `plan_step`.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "session_id": {
      "type": "string",
      "description": "The session this plan is for."
    },
    "plan": {
      "type": "array",
      "items": {
        "type": "string"
      },
      "description": "The steps, in order (max 24, each a short line). An empty array CLEARS the plan. Omit the key entirely to read the current plan without changing it."
    },
    "run_id": {
      "type": "string",
      "description": "Optional: the id returned by run_begin, naming the execution this plan belongs to. A declared plan belongs to the run that declared it, so passing the id lets an operator see what a run said it would do next to what it actually did."
    }
  },
  "required": [
    "session_id"
  ]
}"#,
    },
    Tool {
        name: "terminal_history",
        description: "List ALL terminal sessions, including closed ones retained in history. Each entry: {id, kind, label, status: 'live'|'closed', bytes, closed_at? (unix seconds), exit_code? (natural shell exit code)}. Closed entries sorted newest-first. (This said \"closed sessions\" only, contradicting its own `limit` parameter below and the device, which always includes live sessions.)",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "limit": {
      "type": "integer",
      "description": "Max entries to return (default 20; live sessions are always included)."
    }
  },
  "required": []
}"#,
    },
    Tool {
        name: "terminal_list",
        description: "List all active terminal sessions (PTY, SSH, and serial).",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    }
  },
  "required": []
}"#,
    },
    Tool {
        name: "terminal_list_ports",
        description: "List available serial ports on this machine.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    }
  },
  "required": []
}"#,
    },
    Tool {
        name: "terminal_close",
        description: "Close a terminal session.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "session_id": {
      "type": "string"
    }
  },
  "required": [
    "session_id"
  ]
}"#,
    },
    Tool {
        name: "terminal_diag_write",
        description: "POST a diagnostic line from the terminal panel (poll results, adopt events, SSE status, errors). Stored in a process-lifetime ring buffer (cap 200), read via terminal_diag_read.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "line": {
      "type": "string"
    }
  },
  "required": [
    "line"
  ]
}"#,
    },
    Tool {
        name: "terminal_diag_read",
        description: "Read the panel diagnostic ring buffer (newest last). Returns {entries: [...]}.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    }
  },
  "required": []
}"#,
    },
    Tool {
        name: "secret_set",
        description: "Store a secret (an SSH password) for a target host, so later sessions to that host do not need it inline. The device is a SERVICE, not a desktop app: it tries the OS keychain first and falls back to a file store, so this works headless. PREFER THIS over putting a password in a command — the audit trail records full command text, and a password in it is a password in the record. Lives on the device agent — the browser extension is not involved.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "target": {
      "type": "string",
      "description": "SSH target (user@host:port)"
    },
    "password": {
      "type": "string"
    }
  },
  "required": [
    "target",
    "password"
  ]
}"#,
    },
    Tool {
        name: "secret_get",
        description: "Retrieve a stored secret for a target host. Returns the password or null. The store is the DEVICE agent's own — OS keychain first, then a file.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "target": {
      "type": "string"
    }
  },
  "required": [
    "target"
  ]
}"#,
    },
    Tool {
        name: "secret_delete",
        description: "Delete a stored secret for a target host. The store is the DEVICE agent's own — OS keychain first, then a file.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "target": {
      "type": "string"
    }
  },
  "required": [
    "target"
  ]
}"#,
    },
    Tool {
        name: "terminal_saved_connections",
        description: "List saved terminal connections (successfully-opened sessions). Each entry has id (kind:target), kind, target, label and the original open params — reconnect with terminal_connect_saved. Connect-failures are not saved; a reconnect updates the entry.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    }
  },
  "required": []
}"#,
    },
    Tool {
        name: "terminal_jobs",
        description: "Background-job registry. With no params: list recent run_in_background jobs {job_id, command, done, exit_code}. With {job_id, wait_secs}: block until that job finishes or the timeout elapses, then return its final state. THIS IS HOW A run_in_background EXECUTION IS COLLECTED — terminal_execute advertises run_in_background, and the job record exists precisely so callers poll here instead of blind-reading the session.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "job_id": {
      "type": "string",
      "description": "Job id returned by terminal_execute(run_in_background:true)."
    },
    "wait_secs": {
      "type": "integer",
      "description": "Max seconds to wait for completion when job_id is given. Default 0 (instant snapshot)."
    }
  },
  "required": []
}"#,
    },
    Tool {
        name: "terminal_forget_saved",
        description: "Remove a saved terminal connection by id (from terminal_saved_connections) and delete its stored password (ssh targets). The live session, if any, is untouched.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "id": {
      "type": "string",
      "description": "The id (kind:target) from terminal_saved_connections."
    }
  },
  "required": [
    "id"
  ]
}"#,
    },
    Tool {
        name: "terminal_connect_saved",
        description: "Reconnect to a saved terminal connection (from terminal_saved_connections) by id. Replays the saved params through terminal_open; returns the new session id. Optional params override the saved ones.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "id": {
      "type": "string"
    },
    "rows": {
      "type": "integer",
      "description": "Override the saved row count."
    },
    "cols": {
      "type": "integer",
      "description": "Override the saved column count."
    }
  },
  "required": [
    "id"
  ]
}"#,
    },
    Tool {
        name: "terminal_env",
        description: "Environment info for the AI when driving this device's terminal: default shell, install dir, bundled node.exe (for one-off node scripts run via terminal_execute), and usage guidance. Run BEFORE opening sessions/executing commands.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    }
  },
  "required": []
}"#,
    },
    Tool {
        name: "browser_pw_info",
        description: "Info about the BUNDLED Playwright runtime on this device (no install needed — AI agents must reuse it instead of installing their own): returns pw_dir, playwright-core version, node.exe path, chromium availability, screenshot output dir, and a ready-to-use script template. Combined with browser_run_script this is the canonical way to drive this device's browser.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    }
  },
  "required": []
}"#,
    },
    Tool {
        name: "browser_run_script",
        description: "Run a self-contained Node/Playwright script with the device's BUNDLED node + playwright-core (never install your own). Scripts run with SUMMRISE_BROWSER_HELPER set (acquireBrowser(): attaches to the visible embedded view when present so actions show live, else private headless) — prefer it over launching your own browser; headless only for batch jobs that must not disturb the watched screen. Concurrency: calls run as independent processes with NO runner lock — headless runs are fully parallel, but attached runs SHARE the single visible tab (one view shows one page; parallel visible drivers interleave, so keep interactive work serial). Screenshot namespacing: pass shots as \"<SUMMRISE_RUN_ID>-*.png\" (env, unique per call) for exact attribution under concurrency; the returned list is otherwise a best-effort before/after diff. Params: script (JS source, CommonJS; follow the browser_pw_info template), timeout_secs (default 120, max 600). Screenshots saved to the pwout dir are listed in the result. Returns exit_code, stdout, stderr (each capped), screenshots, timed_out.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "script": {
      "type": "string"
    },
    "timeout_secs": {
      "type": "integer"
    },
    "run_id": {
      "type": "string",
      "description": "Optional: the id returned by run_begin, naming the execution this browser action belongs to. One run spans browser actions AND terminal commands. This is NOT SUMMRISE_RUN_ID (the per-call env stem used for screenshot namespacing) — pass back the id run_begin gave you."
    }
  },
  "required": [
    "script"
  ]
}"#,
    },
    Tool {
        name: "system_file_upload",
        description: "Send a local file to the Summrise relay and return its one-time download URL (the other half of the file-transfer pair: hand that URL to system_file_download on the receiving device, or fetch it here on Linux). THE BYTES NEVER PASS THROUGH THE AI CONTEXT, so a 100 MB image is fine. The agent DOES read the file into memory before relaying it, so the cost is bounded by the 100 MiB transfer cap and the upload is NOT streamed from disk — this sentence claimed streaming, which the code has never done (`fs::read` + a buffered body); the DOWNLOAD direction really does stream, which is what made the claim look verified (system_file_write is the ≤4 MiB inline path only). The relay holds it until first download or 24 h. Returns {ok, url, bytes}.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "path": {
      "type": "string",
      "description": "Absolute path of the file on the device to send."
    }
  },
  "required": [
    "path"
  ]
}"#,
    },
    Tool {
        name: "system_file_download",
        description: "Receive a file onto THIS device (the agent host) from a URL — the device fetches it directly, so the bytes NEVER pass through the AI context (this is how a 100 MB firmware image moves; system_file_write is only for ≤4 MiB inline text). Pair with system_file_upload: the sender uploads to the Summrise relay and hands back the one-time URL, this tool lands it. Returns {ok, path, bytes}. Destination is any absolute path (parents are created; relative = <data dir>/downloads). The write is staged as <path>.part and renamed, so a truncated transfer never appears complete. IP-literal hosts are refused (SSRF guard) — use a hostname.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "url": {
      "type": "string",
      "description": "HTTP/HTTPS URL to fetch (a relay URL from system_file_upload)."
    },
    "path": {
      "type": "string",
      "description": "Destination on the device (absolute recommended, e.g. D:\\Summrise\\downloads\\fw.bin). Parent dirs are created; a relative name lands under <data dir>/downloads."
    }
  },
  "required": [
    "url",
    "path"
  ]
}"#,
    },
    Tool {
        name: "browser_open",
        description: "Open/navigate the controlled tab for a device to a URL. Returns a snapshot.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "run_id": {
      "type": "string",
      "description": "Optional: the id returned by run_begin, naming the execution this browser action belongs to. One run spans browser actions AND terminal commands, so this is what lets an operator see a coherent piece of work instead of the day's traffic. Pass back the id run_begin gave you."
    },
    "url": {
      "type": "string"
    }
  },
  "required": [
    "url"
  ]
}"#,
    },
    Tool {
        name: "browser_snapshot",
        description: "Get the interactive element tree of the controlled tab.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "run_id": {
      "type": "string",
      "description": "Optional: the id returned by run_begin, naming the execution this browser action belongs to. One run spans browser actions AND terminal commands, so this is what lets an operator see a coherent piece of work instead of the day's traffic. Pass back the id run_begin gave you."
    }
  },
  "required": []
}"#,
    },
    Tool {
        name: "browser_screenshot",
        description: "Capture a PNG screenshot of the controlled tab (image).",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "run_id": {
      "type": "string",
      "description": "Optional: the id returned by run_begin, naming the execution this browser action belongs to. One run spans browser actions AND terminal commands, so this is what lets an operator see a coherent piece of work instead of the day's traffic. Pass back the id run_begin gave you."
    },
    "fullPage": {
      "type": "boolean"
    }
  },
  "required": []
}"#,
    },
    Tool {
        name: "browser_click",
        description: "Click an element (by ref from a snapshot, e.g. 6 for e6) in the controlled tab. Returns a snapshot.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "run_id": {
      "type": "string",
      "description": "Optional: the id returned by run_begin, naming the execution this browser action belongs to. One run spans browser actions AND terminal commands, so this is what lets an operator see a coherent piece of work instead of the day's traffic. Pass back the id run_begin gave you."
    },
    "element_ref": {
      "type": "integer",
      "description": "snapshot ref number (rendered as e<N> target)"
    }
  },
  "required": [
    "element_ref"
  ]
}"#,
    },
    Tool {
        name: "browser_type",
        description: "Focus an element and type text into it (real input events). Returns a snapshot.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "run_id": {
      "type": "string",
      "description": "Optional: the id returned by run_begin, naming the execution this browser action belongs to. One run spans browser actions AND terminal commands, so this is what lets an operator see a coherent piece of work instead of the day's traffic. Pass back the id run_begin gave you."
    },
    "element_ref": {
      "type": "integer"
    },
    "text": {
      "type": "string"
    }
  },
  "required": [
    "element_ref",
    "text"
  ]
}"#,
    },
    Tool {
        name: "browser_wait",
        description: "Wait in the controlled tab: for `text` to appear, for `text_gone` to disappear, or for `time` seconds to pass. The server requires at least one; none is marked required here so its rule is the one that applies. Returns a snapshot.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "run_id": {
      "type": "string",
      "description": "Optional: the id returned by run_begin, naming the execution this browser action belongs to. One run spans browser actions AND terminal commands, so this is what lets an operator see a coherent piece of work instead of the day's traffic. Pass back the id run_begin gave you."
    },
    "text": {
      "type": "string",
      "description": "Wait for this text to appear."
    },
    "text_gone": {
      "type": "string",
      "description": "Wait for this text to disappear."
    },
    "time": {
      "type": "number",
      "description": "Seconds to wait."
    }
  }
}"#,
    },
    Tool {
        name: "browser_close",
        description: "Close the controlled tab for a device.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "run_id": {
      "type": "string",
      "description": "Optional: the id returned by run_begin, naming the execution this browser action belongs to. One run spans browser actions AND terminal commands, so this is what lets an operator see a coherent piece of work instead of the day's traffic. Pass back the id run_begin gave you."
    }
  },
  "required": []
}"#,
    },
    Tool {
        name: "run_begin",
        description: "Declare the start of ONE run — one execution of your work on this device — and get back the `run_id` that names it. Call it when you begin a piece of work that spans more than a single command, then pass the id to run_end when you stop. The device cannot tell two AIs apart (the token identifies the device, not the caller), so this declared boundary is what lets an operator see that a set of commands and browser actions belonged to one execution rather than to the day's whole traffic. The id is minted here and embeds its start time; store it and pass it back verbatim.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "label": {
      "type": "string",
      "description": "Optional: a short human-readable name for this run, e.g. \"provision the ONU on VLAN 100\". Shown to the operator, so keep it to a phrase. A blank label is recorded as absent, not as an empty string."
    },
    "goal": {
      "type": "string",
      "description": "Optional: the objective this run is pursuing, when you know it. Distinct from the session goal the OPERATOR sets — one goal can span several runs (a retry after a failure), and a run can have no goal at all."
    }
  },
  "required": []
}"#,
    },
    Tool {
        name: "run_end",
        description: "Declare that a run started with run_begin is finished, so an operator sees a closed interval instead of work that never stopped. Pass back the `run_id` run_begin gave you. A run left unclosed is NOT an error — the device renders it as open with the extent of the events it actually carries, because a client may still be working, may have stopped, or the agent may have restarted. `known` in the reply says whether this id was ever minted here; it is information for you, never a permission.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "run_id": {
      "type": "string",
      "description": "The id returned by run_begin."
    },
    "outcome": {
      "type": "string",
      "description": "Optional: how it ended, in a word or a short phrase (\"done\", \"failed: ONU did not register\"). Omit it rather than guessing — an absent outcome is rendered as nothing, never as a failure."
    }
  },
  "required": [
    "run_id"
  ]
}"#,
    },
    Tool {
        name: "monitor_list",
        description: "List the host:port targets this device watches over TCP, with each one's summary and its most recent probes. The summary carries what an operator asks first: `up_now`, `since_ms` (when the CURRENT state began — the number that turns a state into a story), `up_pct`, the latency range, `drops` (how many times it fell from up to down inside the window — a target that is down now contributes the drop that started it), `last_status` (the HTTP status code, for a target watched with a path) and `last_expect_ok` (whether the body contained the expected text, when one was given). The series is oldest-first; a probe with `ok: false` carries NO latency (nothing was measured) and a GAP in time is a probe that failed. `transitions` is the LOG of state changes — when it went down or came back, and how long the state it ended had lasted (for a recovery, the OUTAGE), which is the form a person writes into a report. The device probes every 15 s on its own timer, so this is what happened while you were doing something else — including whether something you did took a host down.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    }
  },
  "required": []
}"#,
    },
    Tool {
        name: "monitor_add",
        description: "Start watching a host:port on this device and leave the watch in place. The list is PERSISTED, so a watch you add survives an agent restart and is still there for the operator afterwards — add one when something you are about to touch must be seen coming back. The probe is a TCP connect: a REFUSED connection counts as down (the service is not there), which is the question this instrument answers. Adding the same host:port (and path) twice is idempotent — it is the same watch, not a second one. A port is required: name the SERVICE (22 for SSH, 80 for a web UI), because guessing it would probe the wrong thing and report it as fact.          `path` turns the check into a real HTTP GET of that path (\"/\" for a UI's front page, \"/api/health\" for a health endpoint): the probe then records the STATUS CODE, and `ok` means a response arrived with a status below 500 — so a UI answering 500 is DOWN while one answering 401 is UP (it wants credentials, and it is serving). Without a path the probe is a bare TCP connect, which cannot tell those apart. HTTP only: a TLS check needs a certificate story this instrument does not have. When `expect` is given, `ok` also means the body contained it — so a UI answering 200 with a login page is DOWN.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "host": {
      "type": "string",
      "description": "IP address or name, e.g. \"192.168.1.1\"."
    },
    "port": {
      "type": "integer",
      "description": "TCP port to connect to, 1-65535."
    },
    "path": {
      "type": "string",
      "description": "Optional HTTP path to GET, e.g. \"/\" or \"/api/health\". Omit for a plain TCP connect check."
    },
    "expect": {
      "type": "string",
      "description": "Optional text the response body MUST contain (needs a path). A page that answers 200 without it counts as down — the difference between a working UI and a login page or a starting-up stub."
    }
  },
  "required": [
    "host",
    "port"
  ]
}"#,
    },
    Tool {
        name: "monitor_remove",
        description: "Stop watching a target. `removed` says whether anything was being watched under that id — removing one that is not there is reported as a no-op, never as a success. Removing a watch DISCARDS its series: the record is gone, not hidden.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "id": {
      "type": "string",
      "description": "The target id from monitor_list (\"host:port\")."
    }
  },
  "required": [
    "id"
  ]
}"#,
    },
    Tool {
        name: "monitor_probe",
        description: "Probe one watched target RIGHT NOW and return the result plus the refreshed summary — the synchronous half of the instrument, against the 15 s timer that runs on its own. Use it as a BEFORE and AFTER around anything that could take a host down or bring it back: probe, act, probe. It is recorded in the series like any other probe, so the pair also becomes part of what the operator sees.",
        schema: r#"{
  "type": "object",
  "properties": {
    "device": {
      "type": "string",
      "description": "Device name. OPTIONAL — omit when only one device is registered (it is used automatically)."
    },
    "id": {
      "type": "string",
      "description": "The target id from monitor_list (\"host:port\")."
    }
  },
  "required": [
    "id"
  ]
}"#,
    },
];

/// The tool, by name — `allMcpTools().find(t => t.name === name)`.
pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

/// `{ tools: allMcpTools() }` — the `tools/list` result, with each tool's key order preserved.
pub fn tools_result() -> Value {
    let tools: Vec<Value> = TOOLS
        .iter()
        .map(|t| {
            let mut tool = Map::new();
            tool.insert("name".to_string(), Value::String(t.name.to_string()));
            tool.insert(
                "description".to_string(),
                Value::String(t.description.to_string()),
            );
            // The schema string is this module's own constant, so a parse failure is a bug in the table rather
            // than anything a request can cause — `Null` would put `"inputSchema":null` on the wire and the
            // corpus would fail on it, which is the honest outcome for a table that does not parse.
            tool.insert(
                "inputSchema".to_string(),
                serde_json::from_str(t.schema).unwrap_or(Value::Null),
            );
            Value::Object(tool)
        })
        .collect();
    let mut result = Map::new();
    result.insert("tools".to_string(), Value::Array(tools));
    Value::Object(result)
}

/* ─────────────────────────── the emission ─────────────────────────── */

/// The header of the emitted TypeScript, and the one place the file says it is not hand-written.
const TS_HEADER: &str = r#"/**
 * MCP tool registry for the gateway (summrise-gate /mcp endpoint).
 * All tools take a `device` name; terminal/secret tools proxy the device's
 * existing /api/tools endpoints; browser tools route through the device's
 * playwright-mcp bridge (mcp_client), which drives the embedded Electron
 * view over CDP 9333 (round-262 removed the browser-extension path).
 *
 * ── GENERATED, AND THIS FILE IS NOT WHERE THE TABLE LIVES ────────────────────────────────────────
 * The table is Rust: `gateway/wasm/src/mcp_tools.rs`. This file is its EMISSION, and
 * `cargo test -p summrise-gate-wasm` refuses a committed copy that disagrees with the table:
 *
 *     cd gateway/wasm && SUMMRISE_REFRESH_MCP_TOOLS=1 cargo test mcp_tools
 *
 * WHY IT STILL EXISTS AT ALL: `plugins/mcp.ts` is registered and is the ROLLBACK for the cutover —
 * `index.ts` hands `/mcp` to the `WASM_GATE` binding when it is present and falls through to the
 * TypeScript plugin when it is not. Deleting this file before that deploy would leave the fallback
 * serving an MCP endpoint with ZERO tools. It is deleted with the plugin, after the deploy.
 *
 * THE DEFECT IT CLOSES: this file used to be a HAND-COPY of what each device plugin's Rust
 * `tools.rs` defines. Round-554's note here records what that cost — 21 of the agent's 49 tools
 * were invisible AND uncalled with every gate green, because the snapshot a reader was told to
 * refresh was itself a hand-typed copy of this list. The contract test that reads
 * `../agent/spec-tools.json` still holds the table against the DEVICE's registry; what it could
 * never see — a description that drifted — is now impossible, because there is one table.
 */

interface McpTool {
  name: string;
  description: string;
  inputSchema: {
    type: string;
    properties: Record<string, unknown>;
    required?: string[];
  };
}

const TOOLS: McpTool[] = [
"#;

/// The footer: the ONE exported binding `mcp.ts` imports. `allMcpTools()` answered a fresh array every call in
/// the hand-written version (it was `[...TERMINAL_TOOLS, …]`); the array is shared now, which is observable only
/// to a caller that mutates the catalogue — and no caller may.
const TS_FOOTER: &str = r#"];

export function allMcpTools(): McpTool[] {
  return TOOLS;
}
"#;

/// Indent every line of `text` by `spaces`, except the first.
fn indent_after_first(text: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    let mut out = String::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
            out.push_str(&pad);
        }
        out.push_str(line);
    }
    out
}

/// **THE PRODUCER.** `gateway/src/mcp-tools.ts` is exactly this function's output, and the freshness test below
/// is a byte comparison against it.
pub fn render_typescript() -> String {
    let mut out = String::from(TS_HEADER);
    for (i, tool) in TOOLS.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str("  {\n");
        out.push_str(&format!(
            "    name: {},\n",
            serde_json::to_string(tool.name).unwrap_or_default()
        ));
        out.push_str(&format!(
            "    description: {},\n",
            serde_json::to_string(tool.description).unwrap_or_default()
        ));
        // The schema is emitted as the JSON it is, parsed and re-pretty-printed so a reader sees the wire shape
        // rather than one 2 KB line. `serde_json` under `preserve_order` keeps the property order.
        let schema: Value =
            serde_json::from_str(tool.schema).unwrap_or_else(|e| panic!("{}: {e}", tool.name));
        let pretty = serde_json::to_string_pretty(&schema).unwrap_or_default();
        out.push_str("    inputSchema: ");
        out.push_str(&indent_after_first(&pretty, 4));
        out.push_str(",\n  },\n");
    }
    out.push_str(TS_FOOTER);
    out
}

/// The committed emission, and the path the refresh command writes.
pub fn typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/mcp-tools.ts")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **THE FRESHNESS CHECK — AND IT IS THE REASON THE TYPESCRIPT CANNOT DRIFT.** A hand edit to
    /// `gateway/src/mcp-tools.ts` (or a table change that was not re-emitted) fails here with both texts, so the
    /// fix is one command rather than a reading exercise.
    #[test]
    fn the_typescript_table_is_what_this_module_emits() {
        let path = typescript_path();
        let rendered = render_typescript();
        if std::env::var("SUMMRISE_REFRESH_MCP_TOOLS").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write mcp-tools.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_MCP_TOOLS=1 cargo test mcp_tools",
                path.display()
            )
        });
        assert_eq!(
            committed.trim_end(),
            rendered.trim_end(),
            "{} is stale vs the Rust table — run SUMMRISE_REFRESH_MCP_TOOLS=1 cargo test mcp_tools and commit it",
            path.display()
        );
    }

    /// The table's own invariants: a duplicate name would make `find` answer for one tool and `tools/list`
    /// advertise two, which is exactly the class of drift this module removes.
    #[test]
    fn every_name_is_unique_and_the_schemas_parse() {
        let mut seen = std::collections::BTreeSet::new();
        for tool in TOOLS {
            assert!(seen.insert(tool.name), "duplicate tool name {}", tool.name);
            let schema: Value = serde_json::from_str(tool.schema)
                .unwrap_or_else(|e| panic!("{}'s schema does not parse: {e}", tool.name));
            assert_eq!(
                schema.get("type").and_then(Value::as_str),
                Some("object"),
                "{}'s inputSchema is not an object schema",
                tool.name
            );
            // Every `required` entry must be a declared property: a required name with no property is a schema
            // that forbids every call (a validating client cannot supply it).
            if let Some(required) = schema.get("required").and_then(Value::as_array) {
                let props = schema.get("properties").and_then(Value::as_object);
                for name in required {
                    let name = name.as_str().unwrap_or_default();
                    assert!(
                        props.is_some_and(|p| p.contains_key(name)),
                        "{} requires `{name}`, which it does not declare",
                        tool.name
                    );
                }
            }
        }
        assert_eq!(TOOLS.len(), 39, "the table changed size — see the module header");
    }

    /// **A NAME IN THIS TABLE WITH NO ROUTE IS REGISTERED-BUT-UNCALLABLE** — `callTool`'s own programming-error
    /// arm (`No route for registered tool …`). The two decisions live in one crate now, so the partition is
    /// checkable rather than a comment: every tool is device-direct (`mcp.rs::is_device_direct_tool`) or goes
    /// through the playwright bridge (`mcp_browser.rs`), and there is no third answer.
    #[test]
    fn every_tool_has_a_route() {
        for tool in TOOLS {
            let direct = crate::mcp::is_device_direct_tool(tool.name);
            let bridged = tool.name.starts_with("browser_");
            assert!(
                direct || bridged,
                "{} is registered in the table and routed by neither arm — `tools/call` would answer \
                 `No route for registered tool {}`",
                tool.name,
                tool.name
            );
        }
    }

    /// The device-direct prefixes are a POLICY, and the two bundled-playwright runner tools are named
    /// individually because they are the two `browser_*` names the device serves itself (`mcp.ts`'s own comment:
    /// the bridge rejects them). Pinned here so a prefix cannot quietly grow.
    #[test]
    fn the_device_direct_set_is_the_sources() {
        assert!(crate::mcp::is_device_direct_tool("terminal_anything"));
        assert!(crate::mcp::is_device_direct_tool("secret_get"));
        assert!(crate::mcp::is_device_direct_tool("system_file_upload"));
        assert!(crate::mcp::is_device_direct_tool("run_begin"));
        assert!(crate::mcp::is_device_direct_tool("monitor_add"));
        assert!(crate::mcp::is_device_direct_tool("browser_pw_info"));
        assert!(crate::mcp::is_device_direct_tool("browser_run_script"));
        assert!(!crate::mcp::is_device_direct_tool("browser_open"));
        assert!(!crate::mcp::is_device_direct_tool("browser_close"));
        // Every browser_* name that is NOT one of the two runner tools goes through the bridge.
        for tool in TOOLS.iter().filter(|t| t.name.starts_with("browser_")) {
            let expected = tool.name == "browser_pw_info" || tool.name == "browser_run_script";
            assert_eq!(
                crate::mcp::is_device_direct_tool(tool.name),
                expected,
                "{} is on the wrong side of the routing partition",
                tool.name
            );
        }
    }

    /// `tools_result()` is what `tools/list` puts on the wire, so its shape is pinned here rather than only in
    /// the differential: key order, and the `required` array that IS present versus absent.
    #[test]
    fn the_tools_result_keeps_the_wire_order() {
        let result = tools_result();
        let first = &result["tools"][0];
        let keys: Vec<&str> = first.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["name", "description", "inputSchema"]);
        let schema_keys: Vec<&str> = first["inputSchema"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(schema_keys, ["type", "properties", "required"]);
        let properties: Vec<&str> = first["inputSchema"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(properties[0], "device", "the device selector leads every schema");
        // `browser_wait` declares no `required` at all (the shipped server accepts any one of the three), while
        // `terminal_history` declares an EMPTY one. Absent and empty are different bytes and both must survive.
        let by_name = |n: &str| {
            result["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == n)
                .unwrap()
                .clone()
        };
        assert!(by_name("browser_wait")["inputSchema"].get("required").is_none());
        assert_eq!(by_name("terminal_history")["inputSchema"]["required"], serde_json::json!([]));
    }
}
