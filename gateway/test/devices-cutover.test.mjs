// devices-cutover.test.mjs — WHERE THE DEVICE FAMILY IS SERVED, PINNED AS A ROUTING DECISION.
//
// `index.ts` hands `/api/devices*` (and the three public device routes) to the Rust worker when the `WASM_GATE`
// service binding is present — the production configuration — and falls through to the TypeScript plugin when it
// is not, which is the ROLLBACK. Both halves are real configurations and both are asserted here, because a
// cutover that is only described in a comment is a cutover nobody has measured: the tests in `devices.test.mjs`
// and `panel-grant.test.mjs` exercise the TypeScript handlers and therefore run WITHOUT the binding, and this
// file is what says the other configuration routes where it claims to.
//
// **THE TWO EXCLUSIONS ARE GONE (landing 5 slice 4), AND THE CASES BELOW ARE THE SAME TWO ROWS POINTED THE OTHER
// WAY** — the shape the identity slice used when it deleted the cookie condition. `/api/devices/<name>/proxy/...`
// and `POST|PUT /api/upload` were the "not this slice" rows; both are Rust now, so the test that asserted they
// stayed on the TypeScript path asserts the handover instead. A cutover that only ever grows a list is a cutover
// whose exclusions are untested, and an exclusion deleted without moving its assertion is an exclusion that is
// merely gone.
//
// **AND THE COOKIE CONDITION IS GONE — THIS FILE IS WHERE IT WAS MEASURED AND WHERE IT IS NOW MEASURED ABSENT.**
// Until landing 5 slice 2 the eleven admin-gated routes stayed on the TypeScript path when the request carried
// no console cookie, because `requireSession`'s Cloudflare Access arm (`access.ts`) — the arm that authenticates
// an admin with NO console cookie — was not ported. It IS ported now (`gateway/wasm/src/access.rs`, proved on a
// real RS256 key pair by `gateway/wasm/verify.mjs`), so the cookie-less request goes to the worker like every
// other one, and the test below asserts THAT instead of the old carve-out.
import test from "node:test";
import assert from "node:assert/strict";
import worker from "../src/index.ts";
import { issueSessionToken, SESSION_COOKIE } from "../src/auth.ts";
import { makeEnv } from "./helpers.mjs";

const ADMIN_PW = "cutover-admin-password";
const DEVICE = { name: "d1", hostname: "d1.agent.summrise.test", token: "t".repeat(64) };

const cookie = await issueSessionToken(ADMIN_PW, "admin", "admin");

function envWith({ wasmGate = true } = {}) {
  return makeEnv({
    wasmGate,
    devices: [DEVICE],
    users: { admin: { id: "admin", username: "admin", role: "admin", enabled: true, token: "" } },
    kv: { "auth:admin_password": ADMIN_PW, _admin_seeded: "1" },
  });
}

async function call(env, method, path, { session = false, body } = {}) {
  const headers = {};
  if (session) headers.cookie = `${SESSION_COOKIE}=${cookie}`;
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

test("with the binding: the family's routes are handed to it, body and all", async () => {
  for (const [method, path, session, body] of [
    ["GET", "/api/devices", true],
    ["POST", "/api/devices", true, { name: "d2", hostname: "d2.agent.summrise.test", token: "t".repeat(64) }],
    ["GET", "/api/devices/d1/mcp", true],
    ["DELETE", "/api/devices/d1", true],
    ["POST", "/api/devices/d1/rename", true, { name: "d9" }],
    ["POST", "/api/devices/d1/panel-grant", true],
    ["GET", "/api/devices/register-keys", true],
    ["POST", "/api/devices/register-key", true],
    ["GET", "/api/devices/install-cmd", true],
    ["DELETE", "/api/devices/register-keys/abc", true],
    ["GET", "/api/devices/anything/else", true], // the worker's own 404, which is the front door's
  ]) {
    const env = envWith();
    const got = await call(env, method, path, { session, body });
    assert.deepEqual(env._frontDoor, [`${method} ${path}`], `${method} ${path} must reach the binding`);
    assert.equal(got.status, 200, `${method} ${path} answered by the stub gate`);
  }
});

test("with the binding: the routes that never asked for a session are handed over WITHOUT a cookie", async () => {
  for (const [method, path, body] of [
    ["POST", "/api/register", { key: "k", name: "d2", hostname: "d2.agent.summrise.test", token: "t".repeat(64) }],
    ["POST", "/api/devices/self-register", { name: "d2", hostname: "d2.agent.summrise.test", token: "t".repeat(64) }],
    ["POST", "/api/install/tunnel-token", { key: "k" }],
    ["POST", "/api/devices/panel-grant/redeem", { grant: "a".repeat(32) }],
  ]) {
    const env = envWith();
    await call(env, method, path, { session: false, body });
    assert.deepEqual(
      env._frontDoor,
      [`${method} ${path}`],
      `${method} ${path} needs no session and must reach the binding`,
    );
  }
});

test("with the binding: an ADMIN route with no cookie IS handed over — the Access arm is Rust now", async () => {
  // **THIS IS THE CONDITION SLICE 2 REMOVED, ASSERTED IN THE DIRECTION IT NOW POINTS.** With no `ag_session`
  // cookie at all these four used to stay on the TypeScript path; they now reach the binding, because
  // `requireSession`'s Cloudflare Access arm — which is how a cookie-less admin authenticates — is served by the
  // worker. The stub gate answers 200, so a request that was NOT handed over is visible as the plugin's 401.
  for (const [method, path] of [
    ["GET", "/api/devices"],
    ["GET", "/api/devices/d1/mcp"],
    ["DELETE", "/api/devices/d1"],
    ["GET", "/api/devices/install-cmd"],
  ]) {
    const env = envWith();
    const got = await call(env, method, path, { session: false });
    assert.deepEqual(env._frontDoor, [`${method} ${path}`], `${method} ${path} must reach the binding`);
    assert.equal(got.status, 200, `${method} ${path} answered by the stub gate`);
  }
});

test("with the binding: the reverse proxy and the file relay ARE handed over now", async () => {
  // The two rows that were asserted as "kept on the TypeScript path" until slice 4, asserted in the direction
  // the code now takes. The proxy accepts ANY method and authenticates with a plugin token as well as a cookie,
  // so it is exercised with both a GET and a POST; the upload is POST and PUT.
  for (const [method, path] of [
    ["GET", "/api/devices/d1/proxy/panel/"],
    ["POST", "/api/devices/d1/proxy/api/tools/terminal_execute"],
    ["POST", "/api/devices/d1/proxyfoo"],
  ]) {
    const env = envWith();
    const got = await call(env, method, path, { session: true });
    assert.deepEqual(env._frontDoor, [`${method} ${path}`], `${method} ${path} is the worker's now`);
    assert.equal(got.status, 200, `${method} ${path} answered by the stub gate`);
  }
  // The upload's PUT carries the filename in the query — the stub records the PATHNAME, so the row names the
  // path the gate saw, and the query is the request's own (`upload.rs` forwards it; `devices.test.mjs` pins
  // that for the TypeScript).
  for (const [method, path, seen] of [
    ["POST", "/api/upload", "POST /api/upload"],
    ["PUT", "/api/upload?name=firmware.bin", "PUT /api/upload"],
  ]) {
    const env = envWith();
    const got = await call(env, method, path, { session: true });
    assert.deepEqual(env._frontDoor, [seen], `${method} ${path} is the worker's now`);
    assert.equal(got.status, 200, `${method} ${path} answered by the stub gate`);
  }
  // …and the shapes that are still NOT the family, so the new clauses cannot swallow a neighbour: a GET of the
  // upload path (the route is POST|PUT only) and a device name that is not there.
  const crossed = envWith();
  await call(crossed, "GET", "/api/upload", { session: true });
  assert.deepEqual(crossed._frontDoor, [], "GET /api/upload is the plugin's 404, not the relay's");
});

test("with the binding: the /v1 cutover is unchanged", async () => {
  const env = envWith();
  await call(env, "GET", "/v1/models");
  assert.deepEqual(env._frontDoor, ["GET /v1/models"]);
});

test("without the binding: the family is served by the TypeScript plugin — the rollback", async () => {
  const env = envWith({ wasmGate: false });
  const got = await call(env, "GET", "/api/devices", { session: true });
  assert.deepEqual(env._frontDoor, []);
  assert.equal(got.status, 200);
  const body = JSON.parse(got.body);
  assert.equal(body.devices.length, 1);
  assert.equal(body.devices[0].name, "d1");
  assert.equal(body.devices[0].token, "ttt…tttt", "the row shape is the plugin's, masked token included");
});
