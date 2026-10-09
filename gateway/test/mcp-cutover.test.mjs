// mcp-cutover.test.mjs — WHERE THE MCP ENDPOINT IS SERVED, PINNED AS A ROUTING DECISION.
//
// `index.ts` hands `/mcp` to the Rust worker when the `WASM_GATE` service binding is present — the production
// configuration — and falls through to `plugins/mcp.ts` when it is not, which is the ROLLBACK. Both halves are
// real configurations and both are asserted here, because a cutover that is only described in a comment is a
// cutover nobody has measured: `mcp-gateway.test.mjs` exercises the TypeScript handlers and therefore runs
// WITHOUT the binding (it says so in its own `makeEnv`), and this file is what says the other configuration
// routes where it claims to.
//
// **THE ONE EXCLUSION, ASSERTED:** `GET /api/plugins/status` is the SAME PLUGIN's other route and is
// deliberately NOT in the family. Its response carries `routes` — `routeStats(ctx)`, the console plugin
// registry's own dispatch instrumentation: a registration index and a per-isolate hit counter for every route
// of every plugin. Those counters are a property of the TypeScript plugin table and of the traffic one isolate
// has happened to see; the Rust worker dispatches by family and does not have that table, so it could only
// guess them. A cutover that only ever grows a list is a cutover whose exclusions are untested.
//
// **AND THE TOOL TABLE IS PINNED HERE TOO, FROM THE SIDE THAT SERVES IT.** `mcp_tools.rs` is the single source
// and `gateway/src/mcp-tools.ts` is its EMISSION (`gateway/wasm/src/mcp_tools.rs::render_typescript`); the Rust
// test `the_typescript_table_is_what_this_module_emits` refuses a stale copy, and the test below refuses a
// hand-written one — a file whose header stops naming its producer is a file the next reader edits by hand.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import worker from "../src/index.ts";
import { makeEnv } from "./helpers.mjs";

const ADMIN_TOKEN = "cutover-mcp-token";
const DEVICE = { name: "d1", hostname: "d1.agent.summrise.test", token: "t".repeat(64) };

function envWith({ wasmGate = true } = {}) {
  return makeEnv({
    wasmGate,
    devices: [DEVICE],
    users: {
      admin: { id: "admin", username: "admin", role: "admin", enabled: true, createdAt: 1, token: ADMIN_TOKEN },
    },
    kv: { _admin_seeded: "1", [`token:${ADMIN_TOKEN}`]: "admin" },
  });
}

async function call(env, method, path, { token = ADMIN_TOKEN, body } = {}) {
  const headers = {};
  if (token !== null) headers.authorization = `Bearer ${token}`;
  if (body !== undefined) headers["content-type"] = "application/json";
  const response = await worker.fetch(
    new Request(`https://x${path}`, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    }),
    env,
    {},
  );
  return { status: response.status, body: await response.text() };
}

const PING = { jsonrpc: "2.0", id: 1, method: "ping" };

test("with the binding: every shape of /mcp is handed to it, credential or not", async () => {
  for (const [method, token, body] of [
    ["POST", ADMIN_TOKEN, PING],
    ["POST", null, PING], // no credential: the ENDPOINT's 401 is its own answer to give
    ["POST", "not-a-token", PING],
    ["GET", ADMIN_TOKEN, undefined], // Claude Code's probe, which is the SSE stream
    ["PUT", ADMIN_TOKEN, PING], // and a verb it answers 405 to
    ["DELETE", ADMIN_TOKEN, undefined],
  ]) {
    const env = envWith();
    const got = await call(env, method, "/mcp", { token, body });
    assert.deepEqual(env._frontDoor, [`${method} /mcp`], `${method} /mcp must reach the binding`);
    assert.equal(got.status, 200, `${method} /mcp answered by the stub gate`);
  }
});

test("with the binding: the plugin's OTHER route is NOT handed over", async () => {
  // The exclusion, in the configuration where it matters: with the binding present, a route that WAS swept up
  // would answer the stub gate's model list (200) instead of the plugin's own session gate. The request carries
  // no cookie, so the TypeScript plugin's answer is its 401 — which is the proof that it, and not the gate,
  // served the request.
  const env = envWith();
  const got = await call(env, "GET", "/api/plugins/status", { token: null });
  assert.deepEqual(env._frontDoor, [], "the plugin-status probe is not this slice");
  assert.equal(got.status, 401, "and the TypeScript plugin really answered it");
  assert.match(got.body, /Not logged in/);
});

test("with the binding: a DEVICE's mcp config is the device family's, not this one's", async () => {
  // `/api/devices/<name>/mcp` shares the word and nothing else: it is the device registry's route, and it is
  // handed over by the OTHER predicate. Asserted here so a future `/mcp` prefix rule cannot swallow it.
  const env = envWith();
  await call(env, "GET", "/api/devices/d1/mcp", { token: null });
  assert.deepEqual(env._frontDoor, ["GET /api/devices/d1/mcp"]);
});

test("with the binding: the public tooling routes and /v1 are unchanged", async () => {
  const health = envWith();
  await call(health, "GET", "/api/health", { token: null });
  assert.deepEqual(health._frontDoor, [], "/api/health is the front door's own route");

  const v1 = envWith();
  await call(v1, "GET", "/v1/models", { token: null });
  assert.deepEqual(v1._frontDoor, ["GET /v1/models"]);
});

test("without the binding: /mcp is served by the TypeScript plugin — the rollback", async () => {
  const env = envWith({ wasmGate: false });
  const got = await call(env, "POST", "/mcp", { body: PING });
  assert.deepEqual(env._frontDoor, []);
  assert.equal(got.status, 200);
  assert.deepEqual(JSON.parse(got.body), { jsonrpc: "2.0", result: {}, id: 1 });
});

test("without the binding: the rollback still advertises the whole table", async () => {
  // **THE REASON THE GENERATED FILE EXISTS AT ALL.** Deleting `mcp-tools.ts` at the cutover would leave this
  // configuration — the one a deployment without the binding runs, and the one a rollback returns to — serving
  // an MCP endpoint with ZERO tools. The count is asserted here rather than the contents, because the contents
  // are pinned byte for byte by `gateway/wasm/verify.mjs`'s `tools/list` case.
  const env = envWith({ wasmGate: false });
  const got = await call(env, "POST", "/mcp", { body: { jsonrpc: "2.0", id: 1, method: "tools/list" } });
  const tools = JSON.parse(got.body).result.tools;
  assert.equal(tools.length, 39, "the table the rollback serves");
  assert.equal(new Set(tools.map((t) => t.name)).size, 39, "every name is unique");
  assert.ok(
    tools.some((t) => t.name === "terminal_execute" && t.inputSchema.required.join() === "input"),
    "and the shapes are the table's, not a stub's",
  );
});

test("the generated table says it is generated, and names its producer", async () => {
  // A GENERATED file whose header stops naming its producer is a file the next reader edits by hand — and the
  // manifest rule that classifies it (`agent/tests/fixtures/js-boundaries.txt`) requires the producer to exist.
  const text = readFileSync(new URL("../src/mcp-tools.ts", import.meta.url), "utf8");
  assert.match(text, /── GENERATED, AND THIS FILE IS NOT WHERE THE TABLE LIVES ──/);
  assert.match(text, /gateway\/wasm\/src\/mcp_tools\.rs/, "the producer is named");
  assert.match(
    text,
    /SUMMRISE_REFRESH_MCP_TOOLS=1 cargo test mcp_tools/,
    "and so is the command that regenerates it",
  );
  assert.ok(
    !/^\s*const (TERMINAL|SYSTEM|BROWSER|RUNS|MONITOR)_TOOLS/m.test(text),
    "the five hand-written group arrays are gone: there is ONE table now",
  );
});
