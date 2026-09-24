// mcp.js handler behavior tests — pure local (mock KV + stubbed fetch), no Cloudflare calls.
//
// store.js reads via env.KEYS (`token:<t>` → userId, `user:<id>` → JSON, `devices:v1` →
// JSON array), so a Map-backed KV stub stands in. Note store.js keeps a module-level
// 24h cache: distinct tokens per distinct user and one consistent devices payload keep
// tests from stepping on each other's cached entries.
//
// terminal_execute is asserted through the real deviceFetch path by stubbing globalThis.fetch
// (ESM namespace exports are frozen, so monkey-patching deviceFetch itself is not possible;
// stubbing fetch exercises the full handleMcp → callTool → deviceFetch pipeline unchanged).
import test from "node:test";
import assert from "node:assert/strict";
import { handleMcp, PARAM_RENAMES } from "../src/mcp.ts";
import { __clearCaches } from "../src/store.ts";
import { makeEnv as makeBaseEnv } from "./helpers.mjs";

// The device hosts and the rule that accepts them are ONE fixture (round 117); the env declares it below.
const ENV_EXTRA = { DEVICE_HOST_SUFFIX: ".agent.summrise.test" };
const DEVICE = { name: "d1", hostname: "d1.agent.summrise.test", token: "devtok" };

// admin: token:admintoken → admin (role admin); bob: token:usertoken → bob (role user)
// Shared Map-KV stub (helpers.mjs) seeded with this file's MCP base.
function makeEnv() {
  return makeBaseEnv({
    extra: ENV_EXTRA,
    devices: [DEVICE],
    users: {
      admin: { id: "admin", username: "admin", role: "admin", enabled: true, token: "admintoken" },
      bob: { id: "bob", username: "bob", role: "user", enabled: true, token: "usertoken" },
    },
    kv: { "token:admintoken": "admin", "token:usertoken": "bob" },
  });
}

const post = (body, auth = "Bearer admintoken") =>
  new Request("https://x/mcp", {
    method: "POST",
    headers: { authorization: auth, "content-type": "application/json" },
    body: JSON.stringify(body),
  });

// ── Auth gate ─────────────────────────────────────────────────

test("mcp: bad token → 401 JSON-RPC error", async () => {
  const res = await handleMcp(
    post({ jsonrpc: "2.0", method: "ping", id: 1 }, "Bearer bad"),
    makeEnv(),
  );
  assert.equal(res.status, 401);
  const data = await res.json();
  assert.equal(data.jsonrpc, "2.0");
  assert.equal(data.error.code, -32001);
  assert.equal(data.id, null);
});

test("mcp: missing authorization header → 401", async () => {
  const res = await handleMcp(
    new Request("https://x/mcp", { method: "POST", body: "{}" }),
    makeEnv(),
  );
  assert.equal(res.status, 401);
});

test("mcp: valid token but non-admin role → 401", async () => {
  const res = await handleMcp(
    post({ jsonrpc: "2.0", method: "ping", id: 1 }, "Bearer usertoken"),
    makeEnv(),
  );
  assert.equal(res.status, 401);
});

test("mcp: disabled admin token → 401 (enabled check, cf. translate/session gates)", async () => {
  const env = makeBaseEnv({
    extra: ENV_EXTRA,
    devices: [DEVICE],
    users: {
      // Distinct id/token: store.ts caches token→user module-wide.
      dadmin: {
        id: "dadmin",
        username: "dadmin",
        role: "admin",
        enabled: false,
        token: "disablet-admin-tok",
      },
    },
    kv: { "token:disablet-admin-tok": "dadmin" },
  });
  const res = await handleMcp(
    post({ jsonrpc: "2.0", method: "ping", id: 1 }, "Bearer disablet-admin-tok"),
    env,
  );
  assert.equal(res.status, 401);
  const data = await res.json();
  assert.equal(data.error.code, -32001);
});

// ── initialize ─────────────────────────────────────────────────

test("mcp: initialize echoes protocolVersion + summrise-gate serverInfo", async () => {
  const res = await handleMcp(
    post({
      jsonrpc: "2.0",
      method: "initialize",
      params: { protocolVersion: "2025-06-18", capabilities: {} },
      id: 1,
    }),
    makeEnv(),
  );
  assert.equal(res.status, 200);
  assert.equal(res.headers.get("content-type"), "application/json");
  const data = await res.json();
  assert.equal(data.jsonrpc, "2.0");
  assert.equal(data.id, 1);
  assert.equal(data.result.protocolVersion, "2025-06-18");
  assert.equal(data.result.serverInfo.name, "summrise-gate");
  assert.deepEqual(data.result.capabilities, { tools: { listChanged: false } });
});

// ── notifications → 202 (JSON-RPC notifications are not answered) ──

test("mcp: notifications/initialized + notifications/cancelled → 202 empty body", async () => {
  for (const method of ["notifications/initialized", "notifications/cancelled"]) {
    const res = await handleMcp(post({ jsonrpc: "2.0", method }), makeEnv());
    assert.equal(res.status, 202, `${method} must be 202`);
    assert.equal(res.body, null, `${method} must have an empty body`);
  }
});

// ── tools/call body mapping (through the real deviceFetch) ─────

test("mcp: tools/call terminal_execute → device /api/tools/terminal_execute with mapped body", async () => {
  const env = makeEnv();
  const calls = [];
  const realFetch = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    calls.push({ url: String(url), init });
    const cmd = JSON.parse(init.body).command; // echo the command like a real device terminal
    return new Response(JSON.stringify({ ok: true, output: `ran: ${cmd}` }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  };
  try {
    // explicit quiet_ms
    let res = await handleMcp(
      post({
        jsonrpc: "2.0",
        method: "tools/call",
        params: {
          name: "terminal_execute",
          arguments: { device: "d1", session_id: "s-1", input: "ls -la", quiet_ms: 400 },
        },
        id: 2,
      }),
      env,
    );
    assert.equal(res.status, 200);
    let data = await res.json();
    assert.equal(data.result.content[0].type, "text");
    // quiet_ms defaults to 200 when omitted (it said 400 here for a while, while the assertion
    // below sent 200 and both ends defaulted to 200 — a comment that contradicted its own case)
    res = await handleMcp(
      post({
        jsonrpc: "2.0",
        method: "tools/call",
        params: {
          name: "terminal_execute",
          arguments: { device: "d1", session_id: "s-1", input: "pwd" },
        },
        id: 3,
      }),
      env,
    );
    assert.equal(res.status, 200);
    data = await res.json();
    assert.ok(data.result.content[0].text.includes("pwd"));

    // No gateway heartbeat since round-54: the agent's execute wait-loop
    // pings the session itself, so each execute is exactly ONE device fetch.
    assert.equal(calls.length, 2);
    assert.equal(calls[0].url, "https://d1.agent.summrise.test/api/tools/terminal_execute");
    assert.deepEqual(JSON.parse(calls[0].init.body), {
      command: "ls -la",
      session_id: "s-1",
      quiet_ms: 400,
    }); // device + input stripped, input→command, explicit quiet_ms passed through
    assert.deepEqual(JSON.parse(calls[1].init.body), {
      command: "pwd",
      session_id: "s-1",
      quiet_ms: 200,
    }); // default quiet_ms matches the agent
    assert.equal(calls[0].init.headers.get("authorization"), "Bearer devtok"); // device token injected server-side
    assert.equal(calls[0].init.headers.get("host"), null); // host/cookie stripped
    assert.equal(calls[0].init.headers.get("cookie"), null);
  } finally {
    globalThis.fetch = realFetch;
  }
});

test("mcp: tools/call unknown device → -32602 listing registered devices (round-160)", async () => {
  // round-160: a SINGLE registered device absorbs any missing/misguessed
  // name (the dominant real-world failure — 43 "Unknown device" calls/week);
  // the listing error only fires when several devices exist.
  __clearCaches();
  const env = makeBaseEnv({
    extra: ENV_EXTRA,
    devices: [
      { name: "d1", hostname: "d1.agent.summrise.test", token: "t1" },
      { name: "d2", hostname: "d2.agent.summrise.test", token: "t2" },
    ],
    users: {
      admin: { id: "admin", username: "admin", role: "admin", enabled: true, token: "admintoken" },
    },
    kv: { "token:admintoken": "admin" },
  });
  const res = await handleMcp(
    post({
      jsonrpc: "2.0",
      method: "tools/call",
      params: { name: "terminal_list", arguments: { device: "nope" } },
      id: 4,
    }),
    env,
  );
  assert.equal(res.status, 200);
  const data = await res.json();
  assert.equal(data.error.code, -32602);
  assert.match(data.error.message, /Unknown device: nope/);
  assert.match(data.error.message, /Registered devices: d1, d2/);
});

// ── SSE GET ────────────────────────────────────────────────────

test("mcp: GET → 200 text/event-stream keepalive stream; cancel() clears the timer", async () => {
  const res = await handleMcp(
    new Request("https://x/mcp", { headers: { authorization: "Bearer admintoken" } }),
    makeEnv(),
  );
  assert.equal(res.status, 200);
  assert.equal(res.headers.get("content-type"), "text/event-stream");
  assert.equal(res.headers.get("cache-control"), "no-cache");
  assert.ok(res.body instanceof ReadableStream);
  await res.body.cancel(); // must not throw; underlying source cancel() clears the keepalive interval
});

// ── Contract: gateway tool list vs the DEVICE registry (round-54; rewritten
// round-554) ───────────────────────────────────────────────────────────────
// The round-54 version compared mcp-tools.ts with a hand-typed array of tool
// names maintained IN THIS TEST. Two copies of the same hand cannot drift
// apart, so it caught nothing: 21 of the agent's 49 tools (the whole
// system_*/memory_*/mcp_client_* families, agent_update, page_view,
// terminal_sftp/jobs/forget_saved) were invisible to console MCP clients AND
// uncalled — tools/call looks the name up in this registry BEFORE routing, so
// "not listed" was never cosmetic.
//
// The expected set now comes from ../agent/spec-tools.json, generated from
// the agent's live PluginRegistry by
// web::tests::spec_snapshot_pins_every_device_tool_for_the_gateway_contract.
// Adding a device tool now forces an explicit decision: register it, or list
// it in NOT_EXPOSED with a reason.
import { readFileSync } from "node:fs";

// THE RENAME MAP, ONCE (round 226). It stood identically at three places in this file — and a fourth
// once one caller needed it — while the rename it mirrors is two lines in the source:
//   gateway/src/mcp.ts:238   body.command = body.input; delete body.input;
// If that changes this must change with it; a copy per test is how the two drift apart unnoticed.
// MODULE SCOPE, because three tests read it.
// The ONE binding, imported from the code that applies it (round 226-227).
const RENAMES = PARAM_RENAMES;

// ONE WALKER, USED BY BOTH PROSE GATES. There were two copies of this reader, and only one of them
// unescaped a single-quoted literal — the style prettier picks when a description contains a double
// quote, which is how browser_run_script is written. The containment gate therefore failed on a text
// that was already verbatim the device's: a second copy of a reader is a second reader's bugs, and
// this is the same "one fact, two owners" the gates below exist to find.
const consoleDescriptionOf = (ts, name) => {
  const i = ts.indexOf(`name: "${name}"`);
  if (i < 0) return null;
  const j = ts.indexOf("description:", i);
  if (j < 0) return null;
  // prettier picks whichever quote needs fewer escapes, so BOTH styles must be handled
  const q = ts.slice(j).match(/description:\s*(["'])/);
  if (!q) return null;
  const quote = q[1];
  const open = ts.indexOf(quote, j);
  let k = open + 1;
  while (k < ts.length && !(ts[k] === quote && ts[k - 1] !== "\\")) k++;
  const lit = ts.slice(open + 1, k);
  // UNESCAPE BOTH STYLES. A double-quoted literal is JSON, so let JSON decode it; prettier's
  // single-quoted style needs the same treatment by hand, escapes first and backslashes last so the
  // collapse does not eat the one it just produced. A reader that does not decode escapes reports
  // violations that are not there — terminal_write's `\n` reads as `\\n` in the file and matched
  // nothing, which is the third bug this walker has had and the reason there is only one of it.
  if (quote === '"') {
    try {
      return JSON.parse(`"${lit}"`);
    } catch {
      return lit;
    }
  }
  return lit
    .replace(/\\'/g, "'")
    .replace(/\\n/g, "\n")
    .replace(/\\t/g, "\t")
    .replace(/\\\\/g, "\\");
};

/** Device tools deliberately NOT on the console MCP surface. Each needs a
 *  reason — an unexplained absence is exactly the bug this map exists to
 *  prevent, and the ghost test below keeps the map honest. */
const NOT_EXPOSED = {
  // OS surface beyond the sanctioned transfer pair: a PTY is strictly more
  // capable and the panel already renders these.
  system_file_list: "OS browsing — terminal_* covers it",
  system_file_stat: "OS metadata — terminal_* covers it",
  system_file_read: "inline ≤1 MiB read; the relay pair is the transfer path",
  system_file_write: "inline ≤4 MiB write; the relay pair is the transfer path",
  system_process_list: "tasklist is a PTY away",
  system_process_kill: "taskkill is a PTY away",
  system_net_test: "reachability probe — a PTY away",
  // Device-local knowledge base (panel + device MCP surface).
  memory_save: "device KB — panel surface",
  memory_search: "device KB — panel surface",
  memory_list: "device KB — panel surface",
  memory_update: "device KB — panel surface",
  memory_delete: "device KB — panel surface",
  memory_export: "device KB — panel surface",
  // The playwright-mcp bridge plumbing mcp-browser.ts drives internally;
  // exposing it lets a client route around the browser_* tools entirely.
  mcp_client_connect: "internal bridge plumbing (mcp-browser.ts calls it)",
  mcp_client_list:
    "session introspection is device-local; the console bridge (mcp-browser.ts) calls only mcp_client_connect and mcp_client_call",
  mcp_client_call: "internal bridge plumbing",
  mcp_client_disconnect:
    "teardown is device-local; the console bridge never disconnects a client it did not open",
  // Swaps the device binary and restarts the agent (drops every session).
  agent_update: "self-modifying — a CLI action (`summrise update`), not an MCP call",
  page_view: "legacy remote-page helper (design plugin)",
  // terminal_sftp was the pre-relay transfer path. Kept off deliberately:
  // round-554 makes the relay pair the ONE method, and sftp takes arbitrary
  // host/user/credential args an MCP client should not be offered.
  terminal_sftp: "superseded by the relay pair; takes arbitrary SSH credentials",
  sftp: "legacy alias of terminal_sftp",
  terminal_secret_set: "alias of secret_set (registered)",
  terminal_secret_get: "alias of secret_get (registered)",
  terminal_secret_delete: "alias of secret_delete (registered)",
};

/** The gateway-synthesized browser_* tools: implemented over the device's
 *  playwright-mcp bridge, with no device-side /api/tools/<name> counterpart. */
const BRIDGE_SYNTHETIC = [
  "browser_open",
  "browser_snapshot",
  "browser_screenshot",
  "browser_click",
  "browser_type",
  "browser_wait",
  "browser_close",
];

function deviceTools() {
  const raw = readFileSync(new URL("../../agent/spec-tools.json", import.meta.url), "utf8");
  // The generated file carries // header lines so it reads in-repo; JSON has
  // no comments, so drop them before parsing.
  return JSON.parse(
    raw
      .split("\n")
      .filter((l) => !l.startsWith("//"))
      .join("\n"),
  );
}

test("contract: every device tool is registered or explicitly not exposed", async () => {
  const { allMcpTools } = await import("../src/mcp-tools.ts");
  const registered = new Set(allMcpTools().map((t) => t.name));
  const tools = deviceTools();
  assert.ok(
    tools.length >= 49,
    `device registry looks truncated (${tools.length} tools) — regenerate spec-tools.json`,
  );
  for (const t of tools) {
    assert.ok(
      registered.has(t.name) || t.name in NOT_EXPOSED,
      `device tool ${t.name} (plugin ${t.plugin}) is neither registered in mcp-tools.ts nor decided against in NOT_EXPOSED`,
    );
  }
});

test("contract: NOT_EXPOSED contains no ghost and every entry is justified", () => {
  const served = new Set(deviceTools().map((t) => t.name));
  for (const [name, reason] of Object.entries(NOT_EXPOSED)) {
    assert.ok(served.has(name), `NOT_EXPOSED.${name} is stale — the device no longer serves it`);
    assert.ok(reason.length > 8, `NOT_EXPOSED.${name} needs a real reason`);
  }
});

test("contract: every registered tool is device-served or bridge-synthetic", async () => {
  const { allMcpTools } = await import("../src/mcp-tools.ts");
  const served = new Set(deviceTools().map((t) => t.name));
  for (const t of allMcpTools()) {
    assert.ok(
      served.has(t.name) || BRIDGE_SYNTHETIC.includes(t.name),
      `unexpected gateway tool: ${t.name} (the device cannot serve it)`,
    );
  }
});

// The gateway advertises its OWN inputSchema for every device-direct tool and
// relays args verbatim, so a parameter the DEVICE accepts but the gateway does
// not advertise is invisible to a console client: it cannot discover it, and a
// schema-validating client would refuse to send it. That is the same shape as
// round-54's missing-tool drift, one level down — the tool exists, the parameter
// does not.
//
// Found by reading rather than by a gate: `terminal_execute` gained `intent` and
// `considered` on the device and reached a console client as undocumented extras.
// This test is what turns that from a discovery into a failure.
test("contract: the gateway advertises every parameter the device accepts", async () => {
  const { allMcpTools } = await import("../src/mcp-tools.ts");
  const byName = new Map(allMcpTools().map((t) => [t.name, t]));

  // Parameters the GATEWAY owns rather than relaying: routing metadata the
  // device never sees (identical to the existing `delete body.device` in mcp.ts).
  const GATEWAY_ONLY = new Set(["device"]);
  // Deliberate renames applied in callTerminalToolOnce, mirrored here so the
  // comparison is about what the DEVICE receives.

  const problems = [];
  for (const t of deviceTools()) {
    if (!(t.name in Object.fromEntries(byName))) continue;
    if (NOT_EXPOSED[t.name]) continue;
    const tool = byName.get(t.name);
    const advertised = new Set(Object.keys(tool.inputSchema.properties || {}));
    for (const name of Object.keys(RENAMES[t.name] || {})) advertised.add(RENAMES[t.name][name]);
    // AND THE DECLARED TYPES (round 221). The device snapshot carries `param_types`; this side holds its own
  // inputSchema. A difference here is a schema that refuses a call the device would serve — the same class
  // of defect as the `required` array that forbade terminal_execute's `session_id` call.
  for (const [name, ty] of Object.entries(t.param_types || {})) {
      // THE RENAME APPLIES TO TYPES TOO. The device calls terminal_execute s first parameter `command`;
      // this side calls it `input` and rewrites it before forwarding (mcp.ts). The name loop above already
      // maps the rename; my first version of this check did not, and it failed on exactly that pair — the
      // check doing its job before it ever reached a type difference.
      const renames = RENAMES[t.name] || {};
      const onThisSide = Object.keys(renames).find((k) => renames[k] === name) || name;
      const prop = (tool.inputSchema?.properties || {})[onThisSide];
    assert.ok(prop, `${t.name}: the device declares ${name} but this side does not`);
    assert.equal(
      prop.type,
      ty,
      `${t.name}.${name}: this side says ${prop.type}, the device says ${ty} — a schema-validating client would be refused a call the device serves`,
    );
  }

  for (const p of t.params || []) {
      if (GATEWAY_ONLY.has(p)) continue;
      if (!advertised.has(p)) problems.push(`${t.name}.${p}`);
    }
  }
  assert.deepEqual(
    problems,
    [],
    `the device accepts parameters the console MCP schema does not advertise, so no ` +
      `console client can discover or send them: ${problems.join(", ")}`,
  );
});

test("contract: no advertised parameter is a name the device would reject", async () => {
  // The other direction. An advertised parameter the device does not accept is
  // worse than a missing one: the client is TOLD it may send something that is
  // silently dropped.
  const { allMcpTools } = await import("../src/mcp-tools.ts");
  const byName = new Map(allMcpTools().map((t) => [t.name, t]));
  const GATEWAY_ONLY = new Set(["device"]);

  const problems = [];
  for (const t of deviceTools()) {
    const tool = byName.get(t.name);
    if (!tool || NOT_EXPOSED[t.name]) continue;
    const accepted = new Set(t.params || []);
    for (const p of Object.keys(tool.inputSchema.properties || {})) {
      if (GATEWAY_ONLY.has(p)) continue;
      const deviceName = (RENAMES[t.name] || {})[p] || p;
      if (!accepted.has(deviceName)) problems.push(`${t.name}.${p}`);
    }
  }
  assert.deepEqual(
    problems,
    [],
    `the console MCP schema advertises parameters the device does not accept, so a ` +
      `client would be told it may send them and they would be dropped: ${problems.join(", ")}`,
  );
});

test("contract: the relay pair is registered, routed and self-documenting", async () => {
  const { allMcpTools } = await import("../src/mcp-tools.ts");
  const { isDeviceDirectTool } = await import("../src/mcp.ts");
  const byName = new Map(allMcpTools().map((t) => [t.name, t]));
  for (const n of ["system_file_upload", "system_file_download"]) {
    const t = byName.get(n);
    assert.ok(t, `${n} must be registered — it is the sanctioned transfer method`);
    assert.ok(isDeviceDirectTool(n), `${n} must route device-direct`);
    assert.ok(
      /context/i.test(t.description),
      `${n} must state whether bytes pass through the AI context`,
    );
    assert.equal(t.inputSchema.properties.device.type, "string", `${n} takes device`);
  }
  // THE UPLOAD IS NOT STREAMED, AND SAYING IT IS WAS FALSE IN BOTH COPIES.
  // The device (`plugins/system/tools.rs`) and this catalogue both advertised
  // "streamed from disk straight to the relay" while the handler does
  // `std::fs::read` + a buffered body. The DOWNLOAD direction really does stream
  // (`bytes_stream()`), which is what made the claim look verified — a twin that
  // behaves differently is how a shared sentence survives.
  //
  // Pin the FACT, not the prose: descriptions are deliberately re-worded for the
  // console, so a text-equality assertion would rot.
  // PIN THE FALSE PHRASE, NOT THE WORDS IN IT. My first version asserted the
  // description does not match /streamed from disk/i — and FAILED against the
  // CORRECT text, because the corrective sentence says "the upload is NOT
  // streamed from disk". A check that reads a phrase without reading its POLARITY
  // reports a problem for the sentence that fixes it. So this pins the exact
  // claim that was false, and separately requires a statement of the real cost.
  const up = byName.get("system_file_upload").description;
  assert.ok(
    !/streamed from disk straight to the relay/i.test(up),
    "the upload must not repeat the disproven claim that it streams from disk",
  );
  assert.ok(
    /(into memory|not streamed|reads the file)/i.test(up),
    "and it must say the file is read into memory, since that is the real cost",
  );
  assert.deepEqual(byName.get("system_file_upload").inputSchema.required, ["path"]);
  assert.deepEqual(byName.get("system_file_download").inputSchema.required, ["url", "path"]);
});

test("contract: terminal_execute schema/quiet default match the agent", async () => {
  const { allMcpTools } = await import("../src/mcp-tools.ts");
  const t = allMcpTools().find((t) => t.name === "terminal_execute");
  assert.ok(t, "terminal_execute registered");
  // The gateway exposes `input` (mapped to the agent's `command` in mcp.js);
  // quiet_ms default must be the agent's 200ms, not a gateway invention.
  assert.equal(t.inputSchema.properties.quiet_ms.description.includes("200"), true);
  // round-160: device is OPTIONAL (single-device default resolution).
  //
  // `session_id` is optional too — the DEVICE says so, and it has a whole
  // non-session branch. This assertion used to pin `session_id,input`, which
  // made the console forbid a call the device answers; the device's own
  // `required` is `["command"]`, which is `input` after the declared rename.
  // (The paired contract test above now compares `required` in both directions,
  // so this one only has to state what the console intends.)
  assert.equal(t.inputSchema.required.join(","), "input");
});

// round-2026-09-08: callTerminalToolOnce's device path is now the mechanical
// "/api/tools/<name>" guarded by isDeviceDirectTool — the old 21-entry path
// table duplicated mcp-tools.ts's registration list and drifted silently.
// Pin the routing partition: every registered tool must be classified as
// device-direct XOR bridge-routed, matching how callTool dispatches it.
test("contract: device-direct partition matches the bridge-vs-device dispatch", async () => {
  const { allMcpTools } = await import("../src/mcp-tools.ts");
  const { isDeviceDirectTool } = await import("../src/mcp.ts");
  const BRIDGE_ROUTED = new Set([
    "browser_open",
    "browser_snapshot",
    "browser_screenshot",
    "browser_click",
    "browser_type",
    "browser_wait",
    "browser_close",
  ]);
  for (const t of allMcpTools()) {
    const n = t.name;
    if (isDeviceDirectTool(n)) {
      // Device-direct: the terminal_/secret_/system_ families, the run_* run
      // boundaries, the monitor_* reachability watches, plus the two
      // bundled-runner tools. callTool routes on THIS predicate now (it used to
      // re-implement it inline, which is how a registered tool could still fall
      // through to the bridge and die there) — and this list is a MIRROR of it,
      // which is why adding a family to the predicate means adding it here too.
      assert.ok(
        n.startsWith("terminal_") ||
          n.startsWith("secret_") ||
          n.startsWith("system_") ||
          n.startsWith("run_") ||
          n.startsWith("monitor_") ||
          n === "browser_pw_info" ||
          n === "browser_run_script",
        `device-direct misclassification: ${n}`,
      );
    } else {
      // Bridge-routed: every other browser_* tool.
      assert.ok(BRIDGE_ROUTED.has(n), `non-device-direct tool without a bridge route: ${n}`);
    }
  }
});

// round-58: agent tool errors are HTTP 200 + {"ok":false,"error":...} — the
// gateway must map them to stable codes instead of returning them as success.
test("mcp: agent error (200 + ok:false) → SESSION_NOT_FOUND code", async () => {
  const env = makeEnv();
  const realFetch = globalThis.fetch;
  globalThis.fetch = async (url) => {
    return new Response(JSON.stringify({ ok: false, error: "Session not found: s-1" }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  };
  try {
    const res = await handleMcp(
      post({
        jsonrpc: "2.0",
        method: "tools/call",
        params: {
          name: "terminal_execute",
          arguments: { device: "d1", session_id: "s-1", input: "ls" },
        },
        id: 9,
      }),
      env,
    );
    assert.equal(res.status, 200);
    const data = await res.json();
    assert.equal(data.error.code, -32603);
    assert.equal(data.error.data.code, "SESSION_NOT_FOUND");
  } finally {
    globalThis.fetch = realFetch;
  }
});

test("mcp: agent error (200 + ok:false) → SESSION_BUSY code", async () => {
  const env = makeEnv();
  const realFetch = globalThis.fetch;
  globalThis.fetch = async (url) => {
    return new Response(
      JSON.stringify({ ok: false, error: "Session busy (another execute in progress): s-1" }),
      { status: 200, headers: { "content-type": "application/json" } },
    );
  };
  try {
    const res = await handleMcp(
      post({
        jsonrpc: "2.0",
        method: "tools/call",
        params: {
          name: "terminal_execute",
          arguments: { device: "d1", session_id: "s-1", input: "ls" },
        },
        id: 10,
      }),
      env,
    );
    const data = await res.json();
    assert.equal(data.error.data.code, "SESSION_BUSY");
  } finally {
    globalThis.fetch = realFetch;
  }
});

// round-474 (coverage-driven): the message-text TIMEOUT fallback arm had
// ZERO pins (typed-code and not-found/busy message arms were covered).
test("mcp: agent error (200 + ok:false) → TIMEOUT code on 'timed out' text", async () => {
  const env = makeEnv();
  const realFetch = globalThis.fetch;
  globalThis.fetch = async (url) => {
    return new Response(JSON.stringify({ ok: false, error: "SSH command timed out after 30s" }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  };
  try {
    const res = await handleMcp(
      post({
        jsonrpc: "2.0",
        method: "tools/call",
        params: {
          name: "terminal_execute",
          arguments: { device: "d1", session_id: "s-1", input: "ls" },
        },
        id: 12,
      }),
      env,
    );
    const data = await res.json();
    assert.equal(data.error.data.code, "TIMEOUT");
  } finally {
    globalThis.fetch = realFetch;
  }
});

test("mcp: agent typed error (200 + ok:false + code) → TOOL_ERROR not DEVICE_UNREACHABLE (round-64)", async () => {
  const env = makeEnv();
  const realFetch = globalThis.fetch;
  globalThis.fetch = async () => {
    return new Response(
      JSON.stringify({
        ok: false,
        error: "Serial port not found: COM9",
        code: "serial_port_not_found",
      }),
      { status: 200, headers: { "content-type": "application/json" } },
    );
  };
  try {
    const res = await handleMcp(
      post({
        jsonrpc: "2.0",
        method: "tools/call",
        params: {
          name: "terminal_open",
          arguments: { device: "d1", kind: "serial", target: "COM9" },
        },
        id: 11,
      }),
      env,
    );
    const data = await res.json();
    assert.equal(data.error.data.code, "TOOL_ERROR");
    assert.notEqual(data.error.data.code, "DEVICE_UNREACHABLE");
  } finally {
    globalThis.fetch = realFetch;
  }
});

// SOLID Round-48: the TYPED-code arms (session_not_found/session_busy/
// ssh_timeout) had pins only via message texts that ALSO match the
// message-guess fallbacks — a reordered ternary could silently shift arms
// with every gate green. These use guess-proof messages so ONLY the typed
// arm can produce the verdict.
test("mcp: typed codes map without message guessing", async () => {
  const env = makeEnv();
  const realFetch = globalThis.fetch;
  const cases = [
    [{ ok: false, error: "gone", code: "session_not_found" }, "SESSION_NOT_FOUND"],
    [{ ok: false, error: "occupied", code: "session_busy" }, "SESSION_BUSY"],
    [{ ok: false, error: "SSH stalled", code: "ssh_timeout" }, "TIMEOUT"],
  ];
  try {
    let id = 60;
    for (const [body, want] of cases) {
      globalThis.fetch = async () => {
        return new Response(JSON.stringify(body), {
          status: 200,
          headers: { "content-type": "application/json" },
        });
      };
      const res = await handleMcp(
        post({
          jsonrpc: "2.0",
          method: "tools/call",
          params: {
            name: "terminal_execute",
            arguments: { device: "d1", session_id: "s-1", input: "ls" },
          },
          id: id++,
        }),
        env,
      );
      const data = await res.json();
      assert.equal(data.error.data.code, want, `typed ${body.code} (guess-proof text)`);
    }
  } finally {
    globalThis.fetch = realFetch;
  }
});

test("mcp: every other typed agent code → TOOL_ERROR (round-64 backstop)", async () => {
  const env = makeEnv();
  const realFetch = globalThis.fetch;
  try {
    let id = 70;
    for (const code of [
      "keychain",
      "invalid_params",
      "internal",
      "serial_port_not_open",
      "ssh_connect_failed",
    ]) {
      globalThis.fetch = async () => {
        return new Response(JSON.stringify({ ok: false, error: "plain failure", code }), {
          status: 200,
          headers: { "content-type": "application/json" },
        });
      };
      const res = await handleMcp(
        post({
          jsonrpc: "2.0",
          method: "tools/call",
          params: { name: "terminal_open", arguments: { device: "d1" } },
          id: id++,
        }),
        env,
      );
      const data = await res.json();
      assert.equal(data.error.data.code, "TOOL_ERROR", `typed ${code} is device-UP`);
    }
  } finally {
    globalThis.fetch = realFetch;
  }
});

// ── JSON-RPC edge methods (round-367: dispatch arms with zero pins) ──

test("mcp: ping → empty result echoing the id", async () => {
  const res = await handleMcp(post({ jsonrpc: "2.0", method: "ping", id: 42 }), makeEnv());
  assert.equal(res.status, 200);
  const data = await res.json();
  assert.equal(data.id, 42);
  assert.deepEqual(data.result, {});
});

test("mcp: unknown method → -32601 with the id echoed", async () => {
  const res = await handleMcp(
    post({ jsonrpc: "2.0", method: "tools/brew-coffee", id: 7 }),
    makeEnv(),
  );
  const data = await res.json();
  assert.equal(data.error.code, -32601);
  assert.match(data.error.message, /brew-coffee/);
  assert.equal(data.id, 7);
});

test("mcp: tools/call unknown tool → -32602 without touching the network", async () => {
  const realFetch = globalThis.fetch;
  globalThis.fetch = async () => {
    throw new Error("must not be called");
  };
  try {
    const res = await handleMcp(
      post({
        jsonrpc: "2.0",
        method: "tools/call",
        params: { name: "teleport", arguments: {} },
        id: 9,
      }),
      makeEnv(),
    );
    const data = await res.json();
    assert.equal(data.error.code, -32602);
    assert.match(data.error.message, /Unknown tool: teleport/);
    assert.equal(data.id, 9);
  } finally {
    globalThis.fetch = realFetch;
  }
});

test("mcp: tools/call with no devices registered → -32602 guidance (not a dial)", async () => {
  const env = makeBaseEnv({
    extra: ENV_EXTRA,
    devices: [],
    users: {
      admin: { id: "admin", username: "admin", role: "admin", enabled: true, token: "admintoken" },
    },
    kv: { "token:admintoken": "admin" },
  });
  const realFetch = globalThis.fetch;
  globalThis.fetch = async () => {
    throw new Error("must not be called");
  };
  try {
    const res = await handleMcp(
      post({
        jsonrpc: "2.0",
        method: "tools/call",
        params: { name: "terminal_list", arguments: {} },
        id: 3,
      }),
      env,
    );
    const data = await res.json();
    assert.equal(data.error.code, -32602);
    assert.match(data.error.message, /No devices registered/);
  } finally {
    globalThis.fetch = realFetch;
  }
});

test("mcp: unparseable body → -32700 parse error", async () => {
  const res = await handleMcp(
    new Request("https://x/mcp", {
      method: "POST",
      headers: { authorization: "Bearer admintoken", "content-type": "application/json" },
      body: "{not json",
    }),
    makeEnv(),
  );
  const data = await res.json();
  assert.equal(data.error.code, -32700);
});

test("mcp: non-GET/POST method → 405", async () => {
  const res = await handleMcp(
    new Request("https://x/mcp", {
      method: "PUT",
      headers: { authorization: "Bearer admintoken" },
    }),
    makeEnv(),
  );
  assert.equal(res.status, 405);
});

// ── Device resolution matrix (round-398: single-device fallback, I6a
// typo guard, multi-device guidance, tools/list, throw mapping) ──

test("mcp: omitted device with exactly one registered executes on it (round-160 fallback)", async () => {
  const env = makeEnv(); // single DEVICE d1
  let dialed = null;
  const realFetch = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    dialed = String(url);
    return new Response(JSON.stringify({ ok: true, sessions: [] }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  };
  try {
    const res = await handleMcp(
      post({
        jsonrpc: "2.0",
        method: "tools/call",
        params: { name: "terminal_list", arguments: {} },
        id: 11,
      }),
      env,
    );
    assert.equal(res.status, 200);
    assert.equal(dialed, "https://d1.agent.summrise.test/api/tools/terminal_list");
  } finally {
    globalThis.fetch = realFetch;
  }
});

test("mcp: typo'd device with one registered → Unknown device, never executes (I6a)", async () => {
  const env = makeEnv();
  const realFetch = globalThis.fetch;
  globalThis.fetch = async () => {
    throw new Error("must not be called");
  };
  try {
    const res = await handleMcp(
      post({
        jsonrpc: "2.0",
        method: "tools/call",
        params: { name: "terminal_list", arguments: { device: "d2" } },
        id: 12,
      }),
      env,
    );
    const data = await res.json();
    assert.equal(data.error.code, -32602);
    assert.match(data.error.message, /Unknown device: d2/);
  } finally {
    globalThis.fetch = realFetch;
  }
});

test("mcp: omitted device with several registered names them (round-398)", async () => {
  const env = makeBaseEnv({
    extra: ENV_EXTRA,
    devices: [DEVICE, { name: "d2", hostname: "d2.agent.summrise.test", token: "devtok2" }],
    users: {
      admin: { id: "admin", username: "admin", role: "admin", enabled: true, token: "admintoken" },
    },
    kv: { "token:admintoken": "admin" },
  });
  const res = await handleMcp(
    post({
      jsonrpc: "2.0",
      method: "tools/call",
      params: { name: "terminal_list", arguments: {} },
      id: 13,
    }),
    env,
  );
  const data = await res.json();
  assert.equal(data.error.code, -32602);
  assert.match(data.error.message, /Multiple devices registered — specify device: d1, d2/);
});

test("mcp: tools/list returns the tool table with the id echoed", async () => {
  const res = await handleMcp(post({ jsonrpc: "2.0", method: "tools/list", id: 14 }), makeEnv());
  const data = await res.json();
  assert.equal(data.id, 14);
  assert.ok(Array.isArray(data.result.tools));
  assert.ok(data.result.tools.length >= 20);
  assert.ok(data.result.tools.every((t) => t.inputSchema.properties.device));
});

test("mcp: device dial failure → -32603 with DEVICE_UNREACHABLE in data", async () => {
  const env = makeEnv();
  const realFetch = globalThis.fetch;
  globalThis.fetch = async () => {
    throw new Error("fetch failed");
  };
  try {
    const res = await handleMcp(
      post({
        jsonrpc: "2.0",
        method: "tools/call",
        params: { name: "terminal_list", arguments: { device: "d1" } },
        id: 15,
      }),
      env,
    );
    const data = await res.json();
    assert.equal(data.error.code, -32603);
    assert.deepEqual(data.error.data, { code: "DEVICE_UNREACHABLE" });
  } finally {
    globalThis.fetch = realFetch;
  }
});

// ── contract: the DESCRIPTION must not state a fact the device disproved ────
//
// The parameter-name contract above is what this suite has always checked, and
// it is why a stale DESCRIPTION can ship indefinitely: the gateway hand-copies
// each tool's prose, and nothing compared it.
//
// That is not hypothetical. Round 21 corrected `terminal_read`'s description on
// the DEVICE — it had claimed "`offset: 0` re-reads from the beginning", which is
// false past 1 MiB of spill, where a single read returns the window's TAIL and
// the head is unreachable by ANY offset. The gateway's own copy kept serving the
// disproven sentence to every console client, and no gate could see it, because
// descriptions were not an enforced axis.
//
// Descriptions are deliberately re-worded for the console, so asserting textual
// equality would be wrong and would rot. This pins the FACT instead of the
// prose: the disproven claim must be absent, and the real limit must be present.
test("contract: terminal_read does not re-state the disproven 're-reads from the beginning'", async () => {
  const { allMcpTools } = await import("../src/mcp-tools.ts");
  const tool = allMcpTools().find((t) => t.name === "terminal_read");
  assert.ok(tool, "terminal_read must be advertised");
  const d = String(tool.description || "");

  assert.ok(
    !/re-reads from the beginning/i.test(d),
    "terminal_read's description claims `offset: 0` re-reads from the beginning. " +
      "That is FALSE past 1 MiB of spill — read_spill caps one read and returns the " +
      "window's TAIL, so the head cannot be fetched by any offset. Round 21 fixed the " +
      "device copy; this is the console copy, and it reaches the model verbatim.",
  );
  assert.ok(
    /1 MiB|1 MB/i.test(d),
    "terminal_read's description must state the per-read cap, because a client that " +
      "does not know it will read a truncated window as the whole stream: " +
      d,
  );
});

// ── contract: `required` must agree, in BOTH directions ────────────────────
//
// The two tests above compare parameter NAMES. That axis let a real lie through:
// the console advertised `terminal_execute` as requiring `session_id`, while the
// device makes it OPTIONAL and requires only `command` — so a schema-validating
// client was forbidden a call the device explicitly supports (there is a whole
// non-session branch on the device side, and the relay never injects a session
// id). Nothing compared the arrays, so nothing could see it.
//
// Both directions matter and they fail differently:
//   * a required parameter the device does not require DISCOURAGES valid calls —
//     the client refuses to make a call it would have been allowed to make;
//   * a required parameter the device DOES require but the console omits means a
//     client sends an incomplete call and gets a runtime rejection instead of a
//     schema one.
test("contract: the console's `required` matches the device's, after renames", async () => {
  const { allMcpTools } = await import("../src/mcp-tools.ts");
  const byName = new Map(allMcpTools().map((t) => [t.name, t]));
  // Same declared seams as the name-contract tests above.
  const GATEWAY_ONLY = new Set(["device"]);

  const problems = [];
  for (const t of deviceTools()) {
    const tool = byName.get(t.name);
    if (!tool || NOT_EXPOSED[t.name]) continue;
    // The console's public name for a renamed device parameter.
    const publicName = (deviceParam) => {
      const map = RENAMES[t.name] || {};
      for (const [pub, dev] of Object.entries(map)) if (dev === deviceParam) return pub;
      return deviceParam;
    };
    const expectRequired = new Set(
      (t.required || []).filter((p) => !GATEWAY_ONLY.has(p)).map(publicName),
    );
    const advertised = new Set(tool.inputSchema.required || []);
    for (const p of expectRequired) {
      if (!advertised.has(p)) problems.push(`${t.name}.${p} required by device, not advertised`);
    }
    for (const p of advertised) {
      if (!expectRequired.has(p))
        problems.push(`${t.name}.${p} advertised required, device optional`);
    }
  }
  assert.deepEqual(
    problems,
    [],
    "the console's `required` list disagrees with the device's, so a " +
      "schema-validating client is told to send a call the device would reject or " +
      `forbidden one it would accept: ${problems.join(", ")}`,
  );
});

// A 2xx WHOSE BODY CANNOT BE READ IS NOT A SUCCESSFUL TOOL RESULT.
//
// `resp.json().catch(() => ({}))` turned an empty, truncated or non-JSON body
// into `{}`, and `{}` PASSES the agent-ok check below it: `data.ok` is
// `undefined`, which is not `false`. So the model was handed a successful result
// containing nothing and would report having done something it had no evidence
// for. The round-58 comment above that check documents the SAME defect reached
// through a different door — it taught this code to read the AGENT's `ok` flag,
// and the fallback manufactured one whenever the agent's answer could not be
// read at all.
test("mcp: a 2xx with a NON-JSON body is an error, not an empty success", async () => {
  for (const [name, body] of [
    ["empty", ""],
    ["html (a proxy error page)", "<html><body>502 Bad Gateway</body></html>"],
    ["truncated json", '{"ok": tru'],
  ]) {
    const env = makeEnv();
    const realFetch = globalThis.fetch;
    globalThis.fetch = async () =>
      new Response(body, { status: 200, headers: { "content-type": "text/html" } });
    try {
      const res = await handleMcp(
        post({
          jsonrpc: "2.0",
          method: "tools/call",
          params: { name: "terminal_execute", arguments: { device: "d1", input: "ls" } },
          id: 9,
        }),
        env,
      );
      const data = await res.json();
      assert.ok(
        data.error,
        `${name}: an unreadable body must not come back as a successful tool ` +
          `result — the model would report work it has no evidence for: ${JSON.stringify(data)}`,
      );
      assert.equal(data.result, undefined, `${name}: no empty result object`);
    } finally {
      globalThis.fetch = realFetch;
    }
  }
});

// THE CONSOLE'S PROSE IS A HAND-COPY OF THE DEVICE'S, AND ONE DRIFTED (architecture round 9).
// `monitor_list`'s console copy had lost `last_expect_ok` while `drops`' explanation moved onto
// `last_status`, so a model on the console was told a different contract than the device implements
// — with every other contract test green. Nothing could compare prose that existed in only one
// machine-readable place; the device's is in agent/spec-tools.json now (the snapshot carries it),
// and this asks the question that would have caught it: does any console copy DROP a field the
// device names? Fields are spelled as `backticked_identifiers` on both sides, which is what makes
// the comparison mechanical rather than a reading exercise.
test("the console's tool descriptions name every field the device's name", () => {
  const raw = readFileSync(new URL("../../agent/spec-tools.json", import.meta.url), "utf8");
  // the file is JSONC: a generated header of // comments, then the array
  const spec = JSON.parse(raw.split("\n").filter((l) => !l.trimStart().startsWith("//")).join("\n"));
  const ts = readFileSync(new URL("../src/mcp-tools.ts", import.meta.url), "utf8");

  const fields = (s) => new Set([...String(s).matchAll(/`([a-z][a-z0-9_]{2,})`/g)].map((m) => m[1]));

  let exposed = 0;
  const dropped = [];
  for (const e of spec) {
    const desc = consoleDescriptionOf(ts, e.name);
    if (desc === null) continue; // deliberately not console-exposed (the NOT_EXPOSED decision)
    exposed++;
    const lost = [...fields(e.description)].filter((f) => !fields(desc).has(f));
    if (lost.length) dropped.push(`${e.name}: ${lost.join(", ")}`);
  }
  assert.ok(exposed >= 25, `only ${exposed} console-exposed tools were read — this proves nothing`);
  assert.deepEqual(
    dropped,
    [],
    `console copies that drop a field the device names: ${dropped.join(" | ")}`,
  );
});

// AND CONTAINMENT, THE STRONGER RULE (architecture round 11). The field-presence gate above catches
// a DROPPED field; it cannot see a MOVED explanation, which was the other half of the original drift
// (`drops`' text attaching to `last_status`). Containment catches both: the device's description
// must appear, verbatim, inside the console's.
//
// SEVENTEEN CONSOLE COPIES STILL FAIL IT, every one a lossy paraphrase measured on 2026-09-24
// (monitor_add worst: 1122 device characters against 788). They are listed as DEBT rather than
// silently tolerated — and the list is SELF-CLEANING: an entry that no longer fails containment
// fails this test, because a waiver that outlives its defect is how a list like this becomes a
// graveyard nobody trusts.
// EMPTY, AND THAT IS THE POINT (architecture round 16). Every console-exposed tool's description now
// carries the device's text, so containment holds everywhere and there is nothing left to waive. The
// set stays as the place a future exception must be declared AND justified, with the staleness check
// below still failing on any entry that is no longer owed.
const CONTAINMENT_DEBT = new Set([]);

test("the console carries the device's description, or is a listed debt that is still owed", () => {
  const raw = readFileSync(new URL("../../agent/spec-tools.json", import.meta.url), "utf8");
  const spec = JSON.parse(raw.split("\n").filter((l) => !l.trimStart().startsWith("//")).join("\n"));
  const ts = readFileSync(new URL("../src/mcp-tools.ts", import.meta.url), "utf8");

  const norm = (s) => String(s).replace(/\s+/g, " ").trim();

  let exposed = 0;
  const stillOwed = new Set();
  const unexpected = [];
  for (const e of spec) {
    const c = consoleDescriptionOf(ts, e.name);
    if (c === null) continue; // deliberately not console-exposed
    exposed++;
    if (norm(c).includes(norm(e.description))) continue;
    if (CONTAINMENT_DEBT.has(e.name)) stillOwed.add(e.name);
    else unexpected.push(e.name);
  }
  assert.ok(exposed >= 25, `only ${exposed} console-exposed tools were read — this proves nothing`);
  assert.deepEqual(
    unexpected,
    [],
    `console descriptions that neither carry the device's text nor are listed debt: ${unexpected.join(", ")}`,
  );
  const paid = [...CONTAINMENT_DEBT].filter((n) => !stillOwed.has(n));
  assert.deepEqual(
    paid,
    [],
    `these were listed as debt but now satisfy containment — delete them from CONTAINMENT_DEBT: ${paid.join(", ")}`,
  );
});

// THE CATALOGUE HAS ONE OWNER, AND THIS IS WHAT KEEPS IT THAT WAY (architecture round 20). Door A
// withholds tools by name here in NOT_EXPOSED; door B (the device proxy) now refuses the ones no
// panel component calls, from gateway/src/tool-policy.ts. Two lists of one fact drift — so every
// name the POLICY knows must exist in this catalogue: a tool cannot be withheld from one door and
// unknown to the other. The reverse is deliberately not required: NOT_EXPOSED carries names withheld
// for MCP-client reasons that the panel legitimately uses (the memory family, agent_update).
test("contract: every name the tool policy withholds is decided in NOT_EXPOSED too", () => {
  const src = readFileSync(new URL("../src/tool-policy.ts", import.meta.url), "utf8");
  const known = Object.keys(NOT_EXPOSED);
  const unknown = [];
  for (const name of src.matchAll(/^  ([a-z][a-z0-9_]*): \{ reason:/gm)) {
    if (!known.includes(name[1])) unknown.push(name[1]);
  }
  assert.ok(src.includes("WITHHELD_TOOLS"), "the policy module was not read — this proves nothing");
  assert.deepEqual(
    unknown,
    [],
    `these are withheld by tool-policy.ts but not decided in NOT_EXPOSED: ${unknown.join(", ")}`,
  );
});
