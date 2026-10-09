// auth-cutover.test.mjs — WHERE THE IDENTITY SURFACE IS SERVED, PINNED AS A ROUTING DECISION.
//
// `index.ts` hands `/api/auth/*` and the `/api/me*` routes this slice ported to the Rust worker when the
// `WASM_GATE` service binding is present — the production configuration — and falls through to the TypeScript
// plugin when it is not, which is the ROLLBACK. Both halves are real configurations and both are asserted here,
// because a cutover that is only described in a comment is a cutover nobody has measured: `auth.test.mjs`,
// `auth-gates.test.mjs` and `session.test.mjs` exercise the TypeScript handlers and therefore run WITHOUT the
// binding, and this file is what says the other configuration routes where it claims to.
//
// **THE SAME FOUR ROUTES AS THE DEVICE FAMILY'S PREDICATE, AND HERE THEY ARE THE FOUR THIS SLICE DOES NOT PORT:**
//   * `GET|PUT /api/me/route` — the model-route selection, which resolves through the model CATALOGUE and
//     `RouteDO` (`model-route.ts`). A different surface with its own corpus.
//   * `POST /api/me/keys/test` and `POST /api/me/keys/usage` — the key DIAGNOSTICS, which dial six providers and
//     three usage endpoints. Their decisions are the providers' wire shapes, not the caller's identity.
//
// **AND THE POINT OF THIS SLICE'S OTHER HALF IS ASSERTED TOO**: `/api/devices` WITHOUT a console cookie now
// reaches the binding (the Access arm is Rust), which is the condition slice 1 had to carry and slice 2 removed.
import test from "node:test";
import assert from "node:assert/strict";
import worker from "../src/index.ts";
import { issueSessionToken, SESSION_COOKIE } from "../src/auth.ts";
import { makeEnv } from "./helpers.mjs";

const ADMIN_PW = "cutover-admin-password";
const cookie = await issueSessionToken(ADMIN_PW, "admin", "admin");

function envWith({ wasmGate = true } = {}) {
  return makeEnv({
    wasmGate,
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

test("with the binding: the identity surface's routes are handed to it, body and all", async () => {
  for (const [method, path, session, body] of [
    ["POST", "/api/auth/register", false, { username: "x", password: "abcdef", inviteCode: "i" }],
    ["POST", "/api/auth/login", false, { username: "admin", password: ADMIN_PW }],
    ["POST", "/api/auth/reset-password", false, { adminKey: "k", newPassword: "abcdefgh" }],
    ["POST", "/api/auth/logout", true],
    ["GET", "/api/me", true],
    ["POST", "/api/me/token/regenerate", true],
    ["POST", "/api/me/token/relay", true],
    ["DELETE", "/api/me/token/relay", true],
    ["POST", "/api/me/token/relay/reveal", true],
    ["GET", "/api/me/usproxy", true],
    ["PUT", "/api/me/usproxy", true, { enabled: true }],
    ["PUT", "/api/me/keys", true, { name: "DEEPSEEK_API_KEY", value: "sk-x" }],
    ["DELETE", "/api/me/keys?name=DEEPSEEK_API_KEY", true],
    ["POST", "/api/me/keys/reveal", true, { name: "DEEPSEEK_API_KEY" }],
    // The worker answers the front door's own 404 for a shape the plugin does not register, so the path is part
    // of the family even though no handler matches it.
    ["GET", "/api/auth/nonsense", false],
    ["GET", "/api/me/anything/else", true],
  ]) {
    const env = envWith();
    const got = await call(env, method, path, { session, body });
    // The stub gate records the PATHNAME, so a route with a query string is compared without it.
    assert.deepEqual(
      env._frontDoor,
      [`${method} ${path.split("?")[0]}`],
      `${method} ${path} must reach the binding`,
    );
    assert.equal(got.status, 200, `${method} ${path} answered by the stub gate`);
  }
});

test("with the binding: a cookie-less session query goes to the worker — the Access arm is Rust", async () => {
  // **THE `requireSession` FALLBACK IS THE REASON THIS SLICE EXISTS.** A request with NO `ag_session` cookie is
  // an Access-authenticated admin in production; the worker verifies that JWT itself now, so there is no
  // condition to keep here.
  const env = envWith();
  const got = await call(env, "GET", "/api/me", { session: false });
  assert.deepEqual(env._frontDoor, ["GET /api/me"]);
  assert.equal(got.status, 200, "the stub gate answers; the worker would verify the Access JWT");
});

test("with the binding: the four routes this slice does NOT port stay on the TypeScript path", async () => {
  // Each is driven WITHOUT a session, so the answer that proves the plugin handled it is the plugin's own 401 —
  // `requireSession`, not the stub gate's 200. (A session would make `PUT /api/me/route` answer 200 from the KV
  // fallback, which is still the plugin, but a status that two implementations could both produce proves less.)
  for (const [method, path, body] of [
    ["GET", "/api/me/route"],
    ["PUT", "/api/me/route", { model: null }],
    ["POST", "/api/me/keys/test", { name: "DEEPSEEK_API_KEY" }],
    ["POST", "/api/me/keys/usage", { name: "DEEPSEEK_API_KEY" }],
  ]) {
    const env = envWith();
    const got = await call(env, method, path, { session: false, body });
    assert.deepEqual(env._frontDoor, [], `${method} ${path} must NOT reach the binding`);
    assert.equal(got.status, 401, `${method} ${path} answered by the plugin's requireSession`);
    assert.match(got.body, /Not logged in or session expired/);
  }
});

test("with the binding: the devices family no longer needs a cookie (slice 2's condition, removed)", async () => {
  const env = makeEnv({
    wasmGate: true,
    devices: [{ name: "d1", hostname: "d1.agent.summrise.test", token: "t".repeat(64) }],
    users: { admin: { id: "admin", username: "admin", role: "admin", enabled: true, token: "" } },
    kv: { "auth:admin_password": ADMIN_PW, _admin_seeded: "1" },
  });
  const got = await call(env, "GET", "/api/devices", { session: false });
  assert.deepEqual(env._frontDoor, ["GET /api/devices"], "no cookie, and it still goes to the gate");
  assert.equal(got.status, 200);
});

test("with the binding: the /v1 cutover is unchanged", async () => {
  const env = envWith();
  await call(env, "GET", "/v1/models");
  assert.deepEqual(env._frontDoor, ["GET /v1/models"]);
});

test("without the binding: the identity surface is served by the TypeScript plugin — the rollback", async () => {
  const env = envWith({ wasmGate: false });
  const got = await call(env, "GET", "/api/me", { session: true });
  assert.deepEqual(env._frontDoor, []);
  assert.equal(got.status, 200);
  const body = JSON.parse(got.body);
  assert.equal(body.id, "admin");
  assert.equal(body.username, "admin");
  assert.equal(body.role, "admin");
  assert.equal(body.keys.DEEPSEEK_API_KEY.source, "none", "the plugin's own key status is what answered");
});
