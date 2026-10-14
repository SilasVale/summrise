// admin-oracle.mjs — THE ADMIN ROUTE'S DIFFERENTIAL, in the shape `verify.mjs` and the retired
// `route-oracle.mjs` already use.
//
// The pure functions have had an oracle since `oracle.mjs`: drive the SHIPPING code, record what it answered,
// replay that against the Rust. The ROUTE layer needs the same thing and needs it more, because a route is
// where the pieces meet — the gate, the store reads, the masking and the response body — and every one of them
// can be right while the answer is wrong.
//
//   cd gateway/wasm && node admin-oracle.mjs > /dev/null && node verify.mjs
//
// IT DRIVES THE PLUGIN'S OWN ROUTE TABLE (`registerPlugins` + `dispatch`, the two calls `index.ts` makes), so
// the fixture also pins the MATCHER: a route that stops matching fails here rather than in production.
//
// WHY A FAKE ENV IS ENOUGH FOR THESE ROUTES: they read KV and nothing else — no upstream call, no Durable
// Object. What they need is a map, a session cookie and the admin password record.
//
// WHAT IT DOES NOT COVER, stated rather than implied: workerd's runtime glue, and every admin route that is not
// in CASES below. The family is 20 routes; this fixture is the ones the worker serves today, and it grows with
// them.
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import admin from "../src/plugins/admin.ts";
import { SESSION_COOKIE, issueSessionToken } from "../src/auth.ts";
import { jsonOk, jsonError, readJson } from "../src/http.ts";
import { createPluginContext, registerPlugins, dispatch } from "../src/plugins/registry.ts";
import { __clearCaches } from "../src/store/cache.ts";

/** A KV namespace over a plain map: the four methods the stores touch. */
function fakeKV(entries = {}) {
  const map = new Map(Object.entries(entries));
  return {
    // **THE TYPE ARGUMENT IS NOT DECORATION.** The store reads records with `env.KEYS.get(key, "json")`, which
    // PARSES in the real binding; a fake that ignores the second argument hands back a string. The retired
    // route oracle learned this the hard way ("every case with a user key answered 502").
    async get(key, type) {
      if (!map.has(key)) return null;
      const value = map.get(key);
      if (type === "json" && typeof value === "string") {
        try {
          return JSON.parse(value);
        } catch {
          return null;
        }
      }
      return value;
    },
    async put(key, value) {
      map.set(key, value);
    },
    async delete(key) {
      map.delete(key);
    },
    async list({ prefix = "" } = {}) {
      return {
        keys: [...map.keys()].filter((k) => k.startsWith(prefix)).map((name) => ({ name })),
        list_complete: true,
        cursor: undefined,
      };
    },
    __map: map,
  };
}

/** **THE CLOCK IS FROZEN, AND THE FIXTURE IS WHY.** `issueSessionToken` stamps `exp: Date.now() + 24h`, so a
 *  token minted twice is two different tokens — the fixture would churn on every run and the replay could never
 *  reproduce it. A far-future instant keeps every token VALID while making it BYTE-IDENTICAL (2100-01-01), and
 *  the replay freezes the same one. */
const FROZEN_NOW = 4102444800000;
Date.now = () => FROZEN_NOW;

const SECRET = "oracle-secret-not-a-real-one";
const ADMIN_PW = "legacy:0011223344556677:aabbccddeeff0011";

const USERS = {
  "auth:admin_password": ADMIN_PW,
  "user:admin": JSON.stringify({
    id: "admin",
    username: "admin",
    role: "admin",
    enabled: true,
    createdAt: 1,
    token: "tok-admin-secret-value",
  }),
  "user:alice": JSON.stringify({
    id: "alice",
    username: "alice",
    role: "user",
    enabled: true,
    createdAt: 2,
    token: "tok-alice-secret",
  }),
  "user:bob": JSON.stringify({
    id: "bob",
    username: "bob",
    role: "user",
    enabled: false,
    createdAt: 3,
    token: "tok-bob-secret",
  }),
  "ukeys:alice": JSON.stringify({ DEEPSEEK_API_KEY: "sk-alice-deepseek", OG_API_KEY: "" }),
  // A second ADMIN, and it exists for one reason: the revoke cache (`__notRevoked`) is keyed by the COOKIE and
  // lives in module state, so a revoked case that reused the first admin's token would read the cache the happy
  // case had just filled and answer 200. **That is what this harness did on its first run** — a case that
  // silently tested nothing, caught only because the expectation was printed.
  "user:dave": JSON.stringify({
    id: "dave",
    username: "dave",
    role: "admin",
    enabled: true,
    createdAt: 4,
    token: "tok-dave-secret",
  }),
  // A key that lives in the ENV rather than in KV — `userKeysStatus` answers from either.
  "ukeys:bob": JSON.stringify({}),
};

/** The plugin context `index.ts` builds, with the four helpers the registry hands every plugin. */
function context() {
  const ctx = createPluginContext(null, { jsonOk, jsonError, readJson, CORS_HEADERS: {} });
  registerPlugins(ctx, [admin]);
  return ctx;
}

const cookie = (token) => `${SESSION_COOKIE}=${token}`;

const cases = [];
const record = async (name, { method = "GET", path, headers = {}, env }) => {
  __clearCaches();
  const request = new Request(`https://console.test${path}`, { method, headers });
  const res = await dispatch(context(), method, path, request, env, new URL(request.url));
  const body = await res.text();
  cases.push({
    name,
    method,
    path,
    headers,
    env: {
      // Only the KEYS the case needs, so the fixture says what the route reads.
      // **`Object.fromEntries(MAP)`, NOT `Object.entries(MAP)`** — a Map's entries are not own enumerable
      // properties, so `Object.entries` answers `[]` and the fixture records an EMPTY namespace. That is what
      // this line did on its first run: every recorded case carried no KV at all, and the replay's gated cases
      // answered 401 for a reason that had nothing to do with the route.
      KEYS: Object.fromEntries(env.KEYS.__map ?? new Map()),
      SESSION_SECRET: env.SESSION_SECRET ?? null,
    },
    expected: { status: res.status, body },
  });
  // STDERR: stdout IS the fixture (`node admin-oracle.mjs > fixtures/admin-corpus.json`).
  console.error(`  ${name} — ${res.status}, ${body.length} bytes`);
};

const envFor = (entries) => ({ KEYS: fakeKV(entries), SESSION_SECRET: SECRET });

// ── THE CASES ────────────────────────────────────────────────────────────────────────────────────
await record("no cookie is a 401", { path: "/api/admin/users", env: envFor(USERS) });

const userToken = await issueSessionToken(SECRET, "alice", "user");
await record("a non-admin session is a 403", {
  path: "/api/admin/users",
  headers: { Cookie: cookie(userToken) },
  env: envFor(USERS),
});

const adminToken = await issueSessionToken(SECRET, "admin", "admin");
await record("the admin session lists every user, tokens masked, keys reported", {
  path: "/api/admin/users",
  headers: { Cookie: cookie(adminToken) },
  env: envFor(USERS),
});

const disabledToken = await issueSessionToken(SECRET, "bob", "user");
await record("a DISABLED user's own valid cookie is a 401", {
  path: "/api/admin/users",
  headers: { Cookie: cookie(disabledToken) },
  env: envFor(USERS),
});

// A revoked cookie — the store's blacklist is read before the signature is checked. `dave`'s token, because the
// cookie-keyed cache above must not answer for this case.
const daveToken = await issueSessionToken(SECRET, "dave", "admin");
await record("a revoked cookie is a 401", {
  path: "/api/admin/users",
  headers: { Cookie: cookie(daveToken) },
  env: envFor({ ...USERS, [`sess-revoked:${daveToken}`]: "1" }),
});

// An empty deployment: the route must answer the same shape with nothing in it.
await record("an empty deployment answers an empty list", {
  path: "/api/admin/users",
  headers: { Cookie: cookie(adminToken) },
  env: envFor({ "auth:admin_password": ADMIN_PW, "user:admin": USERS["user:admin"] }),
});

process.stdout.write(
  JSON.stringify(
    {
      note: "Generated by gateway/wasm/admin-oracle.mjs from the SHIPPING admin plugin. Do not edit by hand.",
      cases,
    },
    null,
    1,
  ) + "\n",
);
