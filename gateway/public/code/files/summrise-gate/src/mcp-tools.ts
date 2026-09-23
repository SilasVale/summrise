/**
 * MCP tool registry for the gateway (summrise-gate /mcp endpoint).
 * All tools take a `device` name; terminal/secret tools proxy the device's
 * existing /api/tools endpoints; browser tools route through the device's
 * playwright-mcp bridge (mcp_client), which drives the embedded Electron
 * view over CDP 9333 (round-262 removed the browser-extension path).
 *
 * The terminal tools mirror the agent's /api/spec (single source of truth).
 * If the agent gains/loses a tool, update BOTH this list and the toolPath map
 * in mcp.ts, and refresh the spec snapshot in test/mcp-handler.test.mjs
 * (round-54: 11 tools were missing here — terminal_read/write/resize/select/
 * history, list_ports, diag_*, secret_* — invisible to console MCP clients).
 *
 * ROUND-554 — that comment's "refresh the snapshot" step is now FORCED. The
 * snapshot it referred to was a hand-typed copy of THIS list, so it could
 * never notice a device tool missing here: 21 of the agent's 49 tools
 * (the entire system_, memory_, mcp_client_ families, agent_update,
 * page_view, terminal_sftp/jobs/forget_saved) were invisible AND uncalled
 * — tools/call looks the name up in this registry before routing — with
 * every gate green. test/mcp-handler.test.mjs now reads the agent-generated
 * ../agent/spec-tools.json (dumped from the live PluginRegistry by
 * web::tests::spec_snapshot…) and fails on any device tool that is neither
 * registered here nor explicitly listed as not-exposed in that test. Adding
 * a device tool therefore requires an explicit exposure decision, not an
 * optional copy-paste.
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

/**
 * The device-selector field shared by every MCP tool schema (a tool runs
 * against ONE registered device; omit when only one is registered). Used to
 * be copy-pasted into every inputSchema — one source now.
 */
const DEVICE_PARAM: Record<string, unknown> = {
  device: {
    type: "string",
    description:
      "Device name. OPTIONAL — omit when only one device is registered (it is used automatically).",
  },
};

/**
 * The `run_id` parameter, shared by every browser control tool.
 *
 * One definition rather than seven copies: these tools all reach the device
 * through the SAME bridge (`mcp-browser.ts`), and the bridge lifts `run_id` out
 * of the playwright arguments to the device call's top level. A tool whose
 * schema omitted it could not be attributed to a run — and a per-tool copy is
 * how such an omission survives review.
 */
const RUN_PARAM: Record<string, unknown> = {
  run_id: {
    type: "string",
    description:
      "Optional: the id returned by run_begin, naming the execution this browser action belongs to. One run spans browser actions AND terminal commands, so this is what lets an operator see a coherent piece of work instead of the day's traffic. Pass back the id run_begin gave you.",
  },
};

const TERMINAL_TOOLS: McpTool[] = [
  {
    name: "terminal_open",
    description:
      "Open a terminal connection on a device. Kind: 'pty' (local shell; target optional — blank = default shell), 'ssh' (target=user@host:port), or 'serial' (target=port_name, optional ?baud=N&parity=E&data=8&stop=1). Returns session ID.",
    inputSchema: {
      type: "object",
      properties: {
        device: {
          type: "string",
          description:
            "Device name from the console Devices list. OPTIONAL — omit when only one device is registered (it is used automatically).",
        },
        kind: { type: "string", enum: ["pty", "ssh", "serial"] },
        target: {
          type: "string",
          description:
            "pty: optional (blank = default shell); ssh: user@host:port; serial: port_name (?baud=N&parity=E&data=8&stop=1 optional)",
        },
        password: {
          type: "string",
          description: "SSH password (optional — keychain/file store fallback)",
        },
        rows: {
          type: "integer",
          description: "Initial terminal rows. Default 0 (backend default).",
        },
        cols: {
          type: "integer",
          description: "Initial terminal columns. Default 0 (backend default).",
        },
        data_bits: {
          type: "integer",
          description: "(serial) Data bits 5-8. Overrides the target string.",
        },
        parity: {
          type: "string",
          description: "(serial) Parity: none|odd|even. Overrides the target string.",
        },
        stop_bits: {
          type: "integer",
          description: "(serial) Stop bits 1 or 2. Overrides the target string.",
        },
        key_path: {
          type: "string",
          description:
            "(ssh) Path to a private key file. When set, public-key auth is used; password (if any) is the key passphrase.",
        },
        auto_reconnect: {
          type: "boolean",
          description:
            "(serial) Auto-reconnect when the port disappears (unplug / device reboot): the session stays open and re-opens the SAME port with the SAME framing when it reappears. Default false.",
        },
      },
      required: ["kind"],
    },
  },
  {
    name: "terminal_screen",
    description:
      "Get the current on-screen text of a terminal session — the tail of the output buffer (ANSI-stripped), for AI readability. Returns up to `lines` lines (default 60).",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        session_id: { type: "string" },
        lines: { type: "integer", description: "Number of lines from the tail. Default 60." },
      },
      required: ["session_id"],
    },
  },
  {
    name: "terminal_execute",
    description:
      "Run a command. If `session_id` is given, writes the command to that session and waits for output (prompt-marker detection on PTY shells, quiet-period fallback otherwise). Otherwise spawns a local shell with enforced timeout. Session mode returns {kind, state, text, read_from, wait_reason, exit_code, truncated, still_running}: state=done means text is COMPLETE; partial/timeout means text is a PREFIX and `still_running=true` — the command is STILL RUNNING, continue with terminal_read(offset=read_from) until you see the prompt/exit. NEVER re-run a command or open a new session just because a partial was returned: the output arrives in the SAME session's buffer; opening new sessions (terminal_open) while old commands run is what causes output to look interleaved/queued. Long silent SSH commands: prefer run_in_background:true or bigger timeout_secs (idle window scales: ssh 3s, serial 4s, pty 1s). Local mode returns {kind, text, truncated}. `run_in_background: true` (session mode) writes the command and returns immediately with a read_from cursor — collect output via terminal_read; do NOT busy-poll, the wait loop is the foreground path. Note: a quiet timeout or truncation does not prove the foreground command exited.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        session_id: { type: "string" },
        input: { type: "string", description: "The command to run in the session" },
        timeout_secs: { type: "integer", description: "Max wait time in seconds. Default 30." },
        quiet_ms: {
          type: "integer",
          description:
            "(fallback) Quiet period in ms before considering output complete. Default 200.",
        },
        run_in_background: {
          type: "boolean",
          description:
            "(Session mode) Write the command and return immediately with a read_from cursor; collect via terminal_read. Default false.",
        },
        intent: {
          type: "string",
          description:
            "Optional: WHY you are running this, in one sentence. Recorded with the command and shown to the operator on the session's path — it is what turns a list of commands into a readable account of what you were doing and why. Send it whenever the reason is not obvious from the command itself.",
        },
        considered: {
          type: "array",
          items: { type: "string" },
          description:
            "Optional: the alternatives you passed over for this step (short labels, max 8). Recorded and shown as the branches NOT taken, which is the part a command log can never reconstruct. Send it when you made a real choice — not for the only way to do something.",
        },
        plan_step: {
          type: "integer",
          description:
            "Optional: which step of your declared terminal_plan this command advances (1-based). Lets the operator see the plan being followed — or quietly abandoned — instead of having to guess which command served which step.",
        },
        run_id: {
          type: "string",
          description:
            "Optional: the id returned by run_begin, naming the execution this command belongs to. One run spans many commands AND browser actions, so this is what lets an operator see a coherent piece of work instead of the day's traffic. Pass back the id verbatim.",
        },
        approval_id: {
          type: "string",
          description:
            "Optional: the approval id from a result whose state was `awaiting_approval`. If the operator has since approved, the command runs without asking again; the permit covers exactly this command text, once. Omit it for a normal execute.",
        },
      },
      // `session_id` is NOT required: the device makes it optional and has a
      // whole non-session branch, and the relay never injects one — so requiring
      // it here forbade a schema-validating client from making a call the device
      // supports. The device's own `required` is `["command"]`, which is `input`
      // on this side of the declared rename.
      required: ["input"],
    },
  },
  {
    name: "terminal_write",
    description:
      "Write data to a terminal session, or assert a line BREAK on a serial one. `data` is UTF-8 text (JSON strings cannot carry arbitrary bytes); use `data_base64` for binary frames (control bytes, non-UTF-8 serial protocols) — it is decoded and written exactly as given. For shell commands on Unix devices (serial/ssh to Linux), the command must end with a newline (\\n) — otherwise the shell joins it with whatever is typed next, mangling both. For Windows PowerShell use \\r\\n. Control characters (e.g. \\u0003 for Ctrl+C) are sent verbatim and need no newline. `break_ms` (serial sessions only) asserts a BREAK on the line for that many milliseconds — the signal that interrupts a bootloader's autoboot or drops into a ROM monitor, and the one thing a browser terminal cannot send.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        session_id: { type: "string" },
        data: {
          type: "string",
          description: "UTF-8 text to write. Required unless data_base64 or break_ms is given.",
        },
        data_base64: {
          type: "string",
          description:
            "Base64-encoded bytes to write (for binary frames). Takes precedence over data.",
        },
        break_ms: {
          type: "integer",
          description:
            "Assert a BREAK on a serial line for this many ms (default 250, max 5000) — the signal that interrupts a bootloader's autoboot or drops into a ROM monitor. Serial sessions only; a PTY/SSH session refuses by name. Takes precedence over data.",
        },
      },
      required: ["session_id"],
    },
  },
  {
    name: "terminal_read",
    description:
      // The claim that `offset: 0` "Read buffered output from a terminal session. Non-destructive: uses a cursor so repeating the call without `offset` returns only new output since last read. `offset` is an ABSOLUTE byte offset into the session's byte stream; the response's `start`/`end` are the absolute span actually returned. A single read returns AT MOST 1 MiB: for a session that has produced more, the oldest bytes in the requested window are not returned, and `start` will be GREATER than the `offset` you asked for — that gap is the only signal, and it cannot be retrieved by any offset, so treat a `start` above your `offset` as the head being unavailable. Reads work on closed sessions (retained history). ANSI escapes are stripped and line endings normalized by default (AI-readable); pass `clean: false` for raw bytes." was FALSE past
      // 1 MiB of spill and was corrected on the device in round 21 — while this
      // hand-copied string kept serving it to every console client. A single
      // read returns AT MOST 1 MiB and then the window's TAIL, so a `start`
      // greater than the offset you asked for is the only signal that the head
      // was withheld, and no offset can retrieve it.
      "Read buffered output from a terminal session. Non-destructive cursor; `offset` is an ABSOLUTE byte offset and `start`/`end` are the absolute span actually returned. A single read returns AT MOST 1 MiB: for a longer stream the OLDEST bytes in the window are withheld, so a `start` GREATER than your `offset` means the head is unavailable and cannot be fetched by any offset. ANSI escapes stripped by default; pass clean:false for raw bytes.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        session_id: { type: "string" },
        offset: {
          type: "integer",
          description:
            "ABSOLUTE byte offset to start reading from. 0 = beginning. Default = last cursor position.",
        },
        clean: {
          type: "boolean",
          description: "Strip ANSI escapes and normalize \\r\\n → \\n. Default true.",
        },
      },
      required: ["session_id"],
    },
  },
  {
    name: "terminal_resize",
    description:
      "Resize a terminal session (PTY or SSH). `rows`/`cols` are OPTIONAL and default to 24x80 — the handler has always defaulted them, so declaring them required was a schema claim the code contradicted, and it forbade a call the device answers.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        session_id: { type: "string" },
        rows: { type: "integer" },
        cols: { type: "integer" },
      },
      required: ["session_id"],
    },
  },
  {
    name: "terminal_select",
    description:
      "Mark a session as actively watched (client-liveness heartbeat — keeps the idle sweeper from reaping a quiet-but-watched session).",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        session_id: { type: "string" },
      },
      required: ["session_id"],
    },
  },
  {
    name: "terminal_plan",
    description:
      "Declare, revise, clear or read this session's PLAN — the steps you intend to take, in order. Call it before starting a multi-step task so the operator can see what you are about to do and judge it; call it again with a revised list when the plan changes. Pass an empty array to clear it. With `plan` omitted it just returns the current plan. Steps are short labels, not explanations — put the reasoning for a specific command in terminal_execute's `intent`, and name the step a command advances with terminal_execute's `plan_step`.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        session_id: { type: "string", description: "The session this plan is for." },
        plan: {
          type: "array",
          items: { type: "string" },
          description:
            "The steps, in order (max 24, each a short line). An empty array CLEARS the plan. Omit the key entirely to read the current plan without changing it.",
        },
        run_id: {
          type: "string",
          description:
            "Optional: the id returned by run_begin, naming the execution this plan belongs to. A declared plan belongs to the run that declared it, so passing the id lets an operator see what a run said it would do next to what it actually did.",
        },
      },
      required: ["session_id"],
    },
  },
  {
    name: "terminal_history",
    description:
      'List terminal sessions with their byte ranges: LIVE sessions AND closed ones retained in history. (This said "closed sessions" only, contradicting its own `limit` parameter below and the device, which always includes live sessions.)',
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        limit: {
          type: "integer",
          description: "Max entries to return (default 20; live sessions are always included).",
        },
      },
      required: [],
    },
  },
  {
    name: "terminal_list",
    description: "List open terminal sessions on a device.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
      },
      required: [],
    },
  },
  {
    name: "terminal_list_ports",
    description: "List available serial ports on a device.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
      },
      required: [],
    },
  },
  {
    name: "terminal_close",
    description: "Close a terminal session.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        session_id: { type: "string" },
      },
      required: ["session_id"],
    },
  },
  {
    name: "terminal_diag_write",
    description:
      "POST a diagnostic line from the calling client (poll results, SSE status, errors). Stored in a process-lifetime ring buffer.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        line: { type: "string" },
      },
      required: ["line"],
    },
  },
  {
    name: "terminal_diag_read",
    description: "Read the panel diagnostic ring buffer (newest last). Returns {entries: [...]}.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
      },
      required: [],
    },
  },
  {
    name: "secret_set",
    description:
      "Store a secret (an SSH password) for a target host, so later sessions to that host do not need it inline. The device is a SERVICE, not a desktop app: it tries the OS keychain first and falls back to a file store, so this works headless. PREFER THIS over putting a password in a command — the audit trail records full command text, and a password in it is a password in the record. Lives on the device agent — the browser extension is not involved.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        target: { type: "string", description: "SSH target (user@host:port)" },
        password: { type: "string" },
      },
      required: ["target", "password"],
    },
  },
  {
    name: "secret_get",
    description:
      "Retrieve a stored secret from the DEVICE agent's secret store (OS keychain / file). Returns the password or null.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        target: { type: "string" },
      },
      required: ["target"],
    },
  },
  {
    name: "secret_delete",
    description:
      "Delete a stored secret from the DEVICE agent's secret store (OS keychain / file).",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        target: { type: "string" },
      },
      required: ["target"],
    },
  },
  {
    name: "terminal_saved_connections",
    description:
      "List saved terminal connections (successfully-opened sessions). Each entry has id (kind:target), kind, target, label and the original open params — reconnect with terminal_connect_saved. Connect-failures are not saved; a reconnect updates the entry.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
      },
      required: [],
    },
  },
  {
    name: "terminal_jobs",
    description:
      "Background-job registry. With no params: list recent run_in_background jobs {job_id, command, done, exit_code}. With {job_id, wait_secs}: block until that job finishes or the timeout elapses, then return its final state. THIS IS HOW A run_in_background EXECUTION IS COLLECTED — terminal_execute advertises run_in_background, and the job record exists precisely so callers poll here instead of blind-reading the session.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        job_id: {
          type: "string",
          description: "Job id returned by terminal_execute(run_in_background:true).",
        },
        wait_secs: {
          type: "integer",
          description:
            "Max seconds to wait for completion when job_id is given. Default 0 (instant snapshot).",
        },
      },
      required: [],
    },
  },
  {
    name: "terminal_forget_saved",
    description:
      "Remove a saved terminal connection by id (from terminal_saved_connections) and delete its stored password (ssh targets). The live session, if any, is untouched.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        id: {
          type: "string",
          description: "The id (kind:target) from terminal_saved_connections.",
        },
      },
      required: ["id"],
    },
  },
  {
    name: "terminal_connect_saved",
    description:
      "Reconnect to a saved terminal connection (from terminal_saved_connections) by id. Replays the saved params through terminal_open; returns the new session id. Optional params override the saved ones.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        id: { type: "string" },
        rows: { type: "integer", description: "Override the saved row count." },
        cols: { type: "integer", description: "Override the saved column count." },
      },
      required: ["id"],
    },
  },
  {
    name: "terminal_env",
    description:
      "Environment info for the AI when driving this device's terminal: default shell, install dir, bundled node.exe (for one-off node scripts run via terminal_execute), and usage guidance. Run BEFORE opening sessions/executing commands.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
      },
      required: [],
    },
  },
  {
    name: "browser_pw_info",
    description:
      "Info about the BUNDLED Playwright runtime on this device (no install needed — AI agents must reuse it instead of installing their own): returns pw_dir, playwright-core version, node.exe path, chromium availability, screenshot output dir, and a ready-to-use script template. Combined with browser_run_script this is the canonical way to drive this device's browser.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
      },
      required: [],
    },
  },
  {
    name: "browser_run_script",
    description:
      'Run a self-contained Node/Playwright script with the device\'s BUNDLED node + playwright-core (never install your own). Scripts run with SUMMRISE_BROWSER_HELPER set (acquireBrowser(): attaches to the visible embedded view when present so actions show live, else private headless) — prefer it over launching your own browser; headless only for batch jobs that must not disturb the watched screen. Concurrency: calls run as independent processes with NO runner lock — headless runs are fully parallel, but attached runs SHARE the single visible tab (one view shows one page; parallel visible drivers interleave, so keep interactive work serial). Screenshot namespacing: pass shots as "<SUMMRISE_RUN_ID>-*.png" (env, unique per call) for exact attribution under concurrency; the returned list is otherwise a best-effort before/after diff. Params: script (JS source, CommonJS; follow the browser_pw_info template), timeout_secs (default 120, max 600). Screenshots saved to the pwout dir are listed in the result. Returns exit_code, stdout, stderr (each capped), screenshots, timed_out.',
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        script: { type: "string" },
        timeout_secs: { type: "integer" },
        run_id: {
          type: "string",
          description:
            "Optional: the id returned by run_begin, naming the execution this browser action belongs to. One run spans browser actions AND terminal commands. This is NOT SUMMRISE_RUN_ID (the per-call env stem used for screenshot namespacing) — pass back the id run_begin gave you.",
        },
      },
      required: ["script"],
    },
  },
];

/**
 * The device-direct file-transfer pair — the ONE sanctioned way to move a
 * file between a device and anything else (Linux workstation, another
 * device, a CDN URL). Bytes never touch the AI context in either direction,
 * so the 100 MB firmware image that `system_file_write` (≤4 MiB, inline)
 * cannot carry works here.
 *
 * Registration is a POLICY decision, not a capability one: the device serves
 * 52 tools, this file used to mirror 28 of them by hand, and everything
 * unmirrored was uncalled (`tools/call` looks the name up here before
 * routing). test/mcp-handler.test.mjs now reads the agent's generated
 * spec-tools.json and fails on any name that is neither registered here nor
 * explicitly listed as not-exposed.
 */
const SYSTEM_TOOLS: McpTool[] = [
  {
    name: "system_file_upload",
    description:
      "Send a local file to the Summrise relay and return its one-time download URL (the other half of the file-transfer pair: hand that URL to system_file_download on the receiving device, or fetch it here on Linux). THE BYTES NEVER PASS THROUGH THE AI CONTEXT, so a 100 MB image is fine. The agent DOES read the file into memory before relaying it, so the cost is bounded by the 100 MiB transfer cap and the upload is NOT streamed from disk — this sentence claimed streaming, which the code has never done (`fs::read` + a buffered body); the DOWNLOAD direction really does stream, which is what made the claim look verified (system_file_write is the ≤4 MiB inline path only). The relay holds it until first download or 24 h. Returns {ok, url, bytes}.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        path: {
          type: "string",
          description: "Absolute path of the file on the device to send.",
        },
      },
      required: ["path"],
    },
  },
  {
    name: "system_file_download",
    description:
      "Receive a file onto THIS device (the agent host) from a URL — the device fetches it directly, so the bytes NEVER pass through the AI context (this is how a 100 MB firmware image moves; system_file_write is only for ≤4 MiB inline text). Pair with system_file_upload: the sender uploads to the Summrise relay and hands back the one-time URL, this tool lands it. Returns {ok, path, bytes}. Destination is any absolute path (parents are created; relative = <data dir>/downloads). The write is staged as <path>.part and renamed, so a truncated transfer never appears complete. IP-literal hosts are refused (SSRF guard) — use a hostname.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        url: {
          type: "string",
          description: "HTTP/HTTPS URL to fetch (a relay URL from system_file_upload).",
        },
        path: {
          type: "string",
          description:
            "Destination on the device (absolute recommended, e.g. D:\\Summrise\\downloads\\fw.bin). Parent dirs are created; a relative name lands under <data dir>/downloads.",
        },
      },
      required: ["url", "path"],
    },
  },
];

const BROWSER_TOOLS: McpTool[] = [
  {
    name: "browser_open",
    description: "Open/navigate the controlled tab for a device to a URL. Returns a snapshot.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        ...RUN_PARAM,
        url: { type: "string" },
      },
      required: ["url"],
    },
  },
  {
    name: "browser_snapshot",
    description: "Get the interactive element tree of the controlled tab.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        ...RUN_PARAM,
      },
      required: [],
    },
  },
  {
    name: "browser_screenshot",
    description: "Capture a PNG screenshot of the controlled tab (image).",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        ...RUN_PARAM,
        // `fullPage`, not `full_page`: arguments are forwarded VERBATIM, and the
        // shipped server declares `fullPage`. The snake_case spelling was
        // silently dropped, so a full-page request returned a viewport shot and
        // nothing said so.
        fullPage: { type: "boolean" },
      },
      required: [],
    },
  },
  {
    name: "browser_click",
    description:
      "Click an element (by ref from a snapshot, e.g. 6 for e6) in the controlled tab. Returns a snapshot.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        ...RUN_PARAM,
        element_ref: {
          type: "integer",
          description: "snapshot ref number (rendered as e<N> target)",
        },
      },
      required: ["element_ref"],
    },
  },
  {
    name: "browser_type",
    description: "Focus an element and type text into it (real input events). Returns a snapshot.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        ...RUN_PARAM,
        element_ref: { type: "integer" },
        text: { type: "string" },
      },
      required: ["element_ref", "text"],
    },
  },
  {
    name: "browser_wait",
    // SPEAKS THE SHIPPED SERVER'S LANGUAGE. This advertised `condition` as
    // REQUIRED plus a `timeout_s` — and the playwright-mcp the agent installs
    // (`@playwright/mcp` 0.0.79, read out of summrise-playwright.zip) has NEITHER:
    // `browser_wait_for` takes `time`, `text` and `textGone`, all optional.
    // Arguments are forwarded verbatim, so the console was advertising a call
    // whose ONLY required argument the server does not have, and a
    // schema-validating client could not have made a working one.
    //
    // The description now says what the server does — wait for text to appear or
    // disappear, or for a time to pass — instead of promising a selector wait
    // that does not exist.
    description:
      "Wait in the controlled tab: for `text` to appear, for `text_gone` to disappear, or for `time` seconds to pass. The server requires at least one; none is marked required here so its rule is the one that applies. Returns a snapshot.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        ...RUN_PARAM,
        text: { type: "string", description: "Wait for this text to appear." },
        text_gone: { type: "string", description: "Wait for this text to disappear." },
        time: { type: "number", description: "Seconds to wait." },
      },
      // NOTHING IS REQUIRED: the server accepts any one of the three, so marking
      // one required would forbid the other two valid calls.
    },
  },
  {
    name: "browser_close",
    description: "Close the controlled tab for a device.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        ...RUN_PARAM,
      },
      required: [],
    },
  },
];

/**
 * RUN IDENTITY — declaring the boundaries of one AI execution on the device.
 *
 * A run crosses the terminal/browser boundary, so these belong to no existing
 * group; they are device-direct (relayed to `/api/tools/<name>`) and must also
 * match `isDeviceDirectTool()` in mcp.ts, which is a SEPARATE gate: registering
 * here without that predicate reaches `throw ToolErr(TOOL_ERROR, "No route for
 * registered tool …")` at call time.
 *
 * `run_id` is a LABEL the device mints, never a credential — see
 * `agent/src/runs.rs`. The console advertises these so a model can group its own
 * work; the gateway stores nothing.
 */
/**
 * REACHABILITY MONITORING — the device watches host:port targets over TCP
 * (`agent/src/monitor.rs`) and these four tools are how a model reads and
 * extends that watch. Registering them here is HALF the job: the names must also
 * match `isDeviceDirectTool()` in mcp.ts, which is a SEPARATE gate — a name in
 * only the first is registered-but-uncallable and fails at call time.
 *
 * The value to a model is ground truth about the network it is working on: it can
 * reboot a device and then SAY whether it came back, from the device's own
 * minute-by-minute record rather than from a guess between two commands.
 */
const MONITOR_TOOLS: McpTool[] = [
  {
    name: "monitor_list",
    description:
      "List the host:port targets this device watches over TCP, with each one's summary and its most recent probes. The summary carries what an operator asks first: `up_now`, `since_ms` (when the CURRENT state began — the number that turns a state into a story), `up_pct`, the latency range, `drops` (how many times it fell from up to down inside the window — a target that is down now contributes the drop that started it), `last_status` (the HTTP status code, for a target watched with a path) and `last_expect_ok` (whether the body contained the expected text, when one was given). The series is oldest-first; a probe with `ok: false` carries NO latency (nothing was measured) and a GAP in time is a probe that failed. `transitions` is the LOG of state changes — when it went down or came back, and how long the state it ended had lasted (for a recovery, the OUTAGE), which is the form a person writes into a report. The device probes every 15 s on its own timer, so this is what happened while you were doing something else — including whether something you did took a host down.",
    inputSchema: { type: "object", properties: { ...DEVICE_PARAM }, required: [] },
  },
  {
    name: "monitor_add",
    description:
      'Start watching a host:port on this device and leave the watch in place. The list is PERSISTED, so a watch you add survives an agent restart and is still there for the operator afterwards — add one when something you are about to touch must be seen coming back. The probe is a TCP connect: a REFUSED connection counts as down (the service is not there), which is the question this instrument answers. Adding the same host:port (and path) twice is idempotent — it is the same watch, not a second one. A port is required: name the SERVICE (22 for SSH, 80 for a web UI), because guessing it would probe the wrong thing and report it as fact.          `path` turns the check into a real HTTP GET of that path ("/" for a UI\'s front page, "/api/health" for a health endpoint): the probe then records the STATUS CODE, and `ok` means a response arrived with a status below 500 — so a UI answering 500 is DOWN while one answering 401 is UP (it wants credentials, and it is serving). Without a path the probe is a bare TCP connect, which cannot tell those apart. HTTP only: a TLS check needs a certificate story this instrument does not have. When `expect` is given, `ok` also means the body contained it — so a UI answering 200 with a login page is DOWN.',
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        host: { type: "string", description: 'IP address or name, e.g. "192.168.1.1".' },
        port: { type: "integer", description: "TCP port to connect to, 1-65535." },
        path: {
          type: "string",
          description:
            'Optional HTTP path to GET, e.g. "/" or "/api/health". Omit for a plain TCP connect check.',
        },
        expect: {
          type: "string",
          description:
            "Optional text the response body MUST contain (needs a path). A page that answers 200 without it counts as down — the difference between a working UI and a login page or a starting-up stub.",
        },
      },
      required: ["host", "port"],
    },
  },
  {
    name: "monitor_remove",
    description:
      "Stop watching a target. `removed` says whether anything was being watched under that id — removing one that is not there is reported as a no-op, never as a success. Removing a watch DISCARDS its series: the record is gone, not hidden.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        id: { type: "string", description: 'The target id from monitor_list ("host:port").' },
      },
      required: ["id"],
    },
  },
  {
    name: "monitor_probe",
    description:
      "Probe one watched target RIGHT NOW and return the result plus the refreshed summary — the synchronous half of the instrument, against the 15 s timer that runs on its own. Use it as a BEFORE and AFTER around anything that could take a host down or bring it back: probe, act, probe. The probe is recorded in the series like any other.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        id: { type: "string", description: 'The target id from monitor_list ("host:port").' },
      },
      required: ["id"],
    },
  },
];

const RUNS_TOOLS: McpTool[] = [
  {
    name: "run_begin",
    description:
      "Declare the start of ONE run — one execution of your work on this device — and get back the `run_id` that names it. Call it when you begin a piece of work that spans more than a single command, then pass the id to run_end when you stop. The device cannot tell two AIs apart (the token identifies the device, not the caller), so this declared boundary is what lets an operator see that a set of commands and browser actions belonged to one execution rather than to the day's whole traffic. The id is minted by the device and embeds its start time; store it and pass it back verbatim.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        label: {
          type: "string",
          description:
            'Optional: a short human-readable name for this run, e.g. "provision the ONU on VLAN 100". Shown to the operator, so keep it to a phrase. A blank label is recorded as absent, not as an empty string.',
        },
        goal: {
          type: "string",
          description:
            "Optional: the objective this run is pursuing, when you know it. Distinct from the session goal the OPERATOR sets — one goal can span several runs (a retry after a failure), and a run can have no goal at all.",
        },
      },
      required: [],
    },
  },
  {
    name: "run_end",
    description:
      "Declare that a run started with run_begin is finished, so an operator sees a closed interval instead of work that never stopped. Pass back the `run_id` run_begin gave you. A run left unclosed is NOT an error — the device renders it as open with the extent of the events it actually carries, because a client may still be working, may have stopped, or the agent may have restarted. `known` in the reply says whether this id was ever minted here; it is information for you, never a permission.",
    inputSchema: {
      type: "object",
      properties: {
        ...DEVICE_PARAM,
        run_id: { type: "string", description: "The id returned by run_begin." },
        outcome: {
          type: "string",
          description:
            'Optional: how it ended, in a word or a short phrase ("done", "failed: ONU did not register"). Omit it rather than guessing — an absent outcome is rendered as nothing, never as a failure.',
        },
      },
      required: ["run_id"],
    },
  },
];

export function allMcpTools(): McpTool[] {
  return [...TERMINAL_TOOLS, ...SYSTEM_TOOLS, ...BROWSER_TOOLS, ...RUNS_TOOLS, ...MONITOR_TOOLS];
}
