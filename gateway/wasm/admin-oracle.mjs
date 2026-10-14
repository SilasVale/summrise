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
import { SESSION_COOKIE, hashPassword, issueSessionToken } from "../src/auth.ts";
import { jsonOk, jsonError, readJson } from "../src/http.ts";
import { createPluginContext, registerPlugins, dispatch } from "../src/plugins/registry.ts";
import { __clearCaches } from "../src/store/cache.ts";

/** A KV namespace over a plain map: the four methods the stores touch. */
function fakeKV(entries = {}) {
  const map = new Map(Object.entries(entries));
  /** Every write, in the shape the device corpus compares: `put key=value ttl=N` / `del key`. **A WRITE THAT
   *  IS NOT COMPARED IS A WRITE NOBODY CHECKED** — the invite's week-long TTL and the enable/disable record are
   *  invisible in a status and a body. */
  const writes = [];
  return {
    writes,
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
    async put(key, value, options = {}) {
      map.set(key, String(value));
      writes.push(
        `put ${key}=${value}${options.expirationTtl ? ` ttl=${options.expirationTtl}` : ""}`,
      );
    },
    async delete(key) {
      map.delete(key);
      writes.push(`del ${key}`);
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

/** **THE RANDOM STREAM IS FROZEN TOO, AND FOR THE SAME REASON THE CLOCK IS.** `createInvite` mints a random
 *  code, so a fixture recorded from a live CSPRNG could never be replayed. This is `verify.mjs`'s
 *  `installDeterminism` stream (bytes `cursor, cursor+1, …`) applied to node's own `crypto`, and the replay
 *  installs the identical one — with the cursor RESET PER CASE, because the draw sequence belongs to a request
 *  and each side must start a case where the other one did.
 *
 *  `Object.defineProperty`, not assignment: node's `globalThis.crypto` is a getter with no setter. */
let randomCursor = 0;
const realCrypto = globalThis.crypto;
Object.defineProperty(globalThis, "crypto", {
  configurable: true,
  value: new Proxy(realCrypto, {
    get(target, prop) {
      if (prop === "getRandomValues") {
        return (array) => {
          for (let i = 0; i < array.length; i++) array[i] = (randomCursor + i) & 0xff;
          randomCursor += array.length;
          return array;
        };
      }
      const value = target[prop];
      return typeof value === "function" ? value.bind(target) : value;
    },
  }),
});

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
const record = async (name, { method = "GET", path, headers = {}, body: rawBody, env }) => {
  __clearCaches();
  randomCursor = 0;
  // **THE NAMESPACE IS SNAPSHOTTED BEFORE THE REQUEST, AND THIS IS THE THIRD TIME THIS HARNESS HAS LIED ABOUT
  // ITS OWN INPUT.** Recording it after the handler ran meant every case whose route WRITES replayed against the
  // state the write had just produced: the password cases arrived with the NEW hash already stored, so the route
  // took its "a password exists" arm and refused the current password — 403 on both sides' fixtures and 200 from
  // the shipping route. A fixture must describe the world the REQUEST was made in. The WRITES are still read
  // afterwards, because they are the request's effect.
  const before = Object.fromEntries(env.KEYS.__map ?? new Map());
  const request = new Request(`https://console.test${path}`, {
    method,
    headers,
    ...(rawBody === undefined ? {} : { body: rawBody }),
  });
  let res;
  try {
    res = await dispatch(context(), method, path, request, env, new URL(request.url));
  } catch {
    // **A HANDLER THAT THROWS IS THE FRONT DOOR'S 500, AND THESE ARE ITS BYTES** — `index.ts`'s catch is
    // `withCors(request, jsonError(500, "Internal error", "api_error"), env)`, and the body comes from that
    // same shipping helper rather than from a string typed here. The worker's arm answers the identical bytes
    // (`RouteFailure::Threw`), which is what makes the case comparable at all.
    res = jsonError(500, "Internal error", "api_error");
  }
  const body = await res.text();
  cases.push({
    name,
    method,
    path,
    headers,
    writes: env.KEYS.writes ?? [],
    // **THE BODY IS PART OF THE CASE, AND LEAVING IT OUT WAS THIS FIXTURE'S SECOND FALSE GREEN.** The replay
    // sent every write request with an EMPTY body — which the coercion reads as `false` — so the two cases whose
    // shipping answer is `true` failed while the two that answer `false` passed on a request that was never
    // made.
    ...(rawBody === undefined ? {} : { body: rawBody }),
    env: {
      // Only the KEYS the case needs, so the fixture says what the route reads.
      // **`Object.fromEntries(MAP)`, NOT `Object.entries(MAP)`** — a Map's entries are not own enumerable
      // properties, so `Object.entries` answers `[]` and the fixture records an EMPTY namespace. That is what
      // this line did on its first run: every recorded case carried no KV at all, and the replay's gated cases
      // answered 401 for a reason that had nothing to do with the route.
      KEYS: before,
      SESSION_SECRET: env.SESSION_SECRET ?? null,
      // The Worker VARS the case set (`ADMIN_PASSWORD`, say), minus the two the harness owns.
      ...Object.fromEntries(
        Object.entries(env).filter(([k]) => k !== "KEYS" && k !== "SESSION_SECRET"),
      ),
    },
    expected: { status: res.status, body },
  });
  // STDERR: stdout IS the fixture (`node admin-oracle.mjs > fixtures/admin-corpus.json`).
  console.error(`  ${name} — ${res.status}, ${body.length} bytes`);
};

const envFor = (entries, extra = {}) => ({
  KEYS: fakeKV(entries),
  SESSION_SECRET: SECRET,
  ...extra,
});

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

// ── THE CF TOKEN: PRESENT, ABSENT, AND NEVER IN THE CLEAR ────────────────────────────────────────
await record("a configured Cloudflare token is masked, never returned", {
  path: "/api/admin/cloudflare-token",
  headers: { Cookie: cookie(adminToken) },
  env: envFor({ ...USERS, "cf:api_token": "cf-account-token-0123456789" }),
});

await record("an unconfigured Cloudflare token answers empty", {
  path: "/api/admin/cloudflare-token",
  headers: { Cookie: cookie(adminToken) },
  env: envFor(USERS),
});

// ── THE WRITE: ENABLE / DISABLE, AND ITS THREE REFUSALS ─────────────────────────────────────────
const put = (path, body) => ({
  method: "PUT",
  path,
  body: JSON.stringify(body),
  headers: { Cookie: cookie(adminToken), "content-type": "application/json" },
});

await record("a user can be disabled", {
  ...put("/api/admin/users/alice/enabled", { enabled: false }),
  env: envFor(USERS),
});
await record("a user can be re-enabled", {
  ...put("/api/admin/users/bob/enabled", { enabled: true }),
  env: envFor(USERS),
});
// The coercion: a missing field, a "false" string and a 0 all mean DISABLE.
await record("a missing enabled field means DISABLE", {
  ...put("/api/admin/users/alice/enabled", {}),
  env: envFor(USERS),
});
await record('the STRING "false" means DISABLE', {
  ...put("/api/admin/users/alice/enabled", { enabled: "false" }),
  env: envFor(USERS),
});
await record("the admin account cannot be disabled", {
  ...put("/api/admin/users/admin/enabled", { enabled: false }),
  env: envFor(USERS),
});
await record("a malformed percent-escape is a 400, not a 500", {
  ...put("/api/admin/users/%zz/enabled", { enabled: false }),
  env: envFor(USERS),
});
await record("an unknown id is the front door's 500", {
  ...put("/api/admin/users/ghost/enabled", { enabled: false }),
  env: envFor(USERS),
});
await record("the write refuses without a session", {
  method: "PUT",
  path: "/api/admin/users/alice/enabled",
  body: JSON.stringify({ enabled: false }),
  headers: { "content-type": "application/json" },
  env: envFor(USERS),
});

// ── THE PASSWORD: PRESENCE, THE BOOTSTRAP, AND THE ROTATION ──────────────────────────────────────
await record("a deployment WITH a password reports it set", {
  path: "/api/admin/password",
  headers: { Cookie: cookie(adminToken) },
  env: envFor(USERS),
});
await record("a deployment with NO password reports it unset", {
  path: "/api/admin/password",
  headers: { Cookie: cookie(adminToken) },
  env: envFor({ ...USERS, "auth:admin_password": "" }),
});

const putJson = (path, body, cookieValue = adminToken) => ({
  method: "PUT",
  path,
  body: JSON.stringify(body),
  headers: { Cookie: cookie(cookieValue), "content-type": "application/json" },
});

// THE BOOTSTRAP ARM: no password anywhere, and the admin gateway token is the credential.
await record("the FIRST password is set with the admin key, session-less", {
  ...putJson("/api/admin/password", {
    password: "a-fresh-password",
    adminKey: "tok-admin-secret-value",
  }),
  env: envFor({ ...USERS, "auth:admin_password": "" }),
});
await record("the bootstrap refuses a WRONG admin key", {
  ...putJson("/api/admin/password", { password: "a-fresh-password", adminKey: "not-the-key" }),
  env: envFor({ ...USERS, "auth:admin_password": "" }),
});
await record("the bootstrap refuses a short password BEFORE reading any key", {
  ...putJson("/api/admin/password", { password: "short", adminKey: "not-the-key" }),
  env: envFor({ ...USERS, "auth:admin_password": "" }),
});

// THE ROTATION ARM: a password exists, so the session and the CURRENT password are both required. **THE STORED
// RECORD IS DERIVED HERE RATHER THAN TYPED** — `hashPassword(pw, "legacy")` is PBKDF2 over a fixed salt, so it
// is deterministic, and a hand-written hash would have made "the right current password" a case that could only
// ever answer 403 (which is what the first version of this fixture recorded, and it looked like a refusal the
// route had decided rather than one the fixture had baked in).
const CURRENT_PW = "the-current-one";
const CURRENT_RECORD = `legacy:${await hashPassword(CURRENT_PW, "legacy")}`;
const ROTATION = { ...USERS, "auth:admin_password": CURRENT_RECORD };

await record("the password rotates with the current one", {
  ...putJson("/api/admin/password", { password: "a-new-password", currentPassword: CURRENT_PW }),
  env: envFor(ROTATION),
});
await record("a rotation with the WRONG current password is refused", {
  ...putJson("/api/admin/password", { password: "a-new-password", currentPassword: "wrong" }),
  env: envFor(ROTATION),
});
await record("a rotation with no session is refused even with the right password", {
  method: "PUT",
  path: "/api/admin/password",
  body: JSON.stringify({ password: "a-new-password", currentPassword: CURRENT_PW }),
  headers: { "content-type": "application/json" },
  env: envFor(ROTATION),
});
await record("a rotation to a short password is refused", {
  ...putJson("/api/admin/password", { password: "tiny", currentPassword: CURRENT_PW }),
  env: envFor(ROTATION),
});

// ── THE INVITE: A RANDOM CODE AND A TTL, BOTH FROZEN BY THE HARNESS ──────────────────────────────
await record("an invite is minted with a week-long TTL at the binding", {
  method: "POST",
  path: "/api/admin/invite",
  headers: { Cookie: cookie(adminToken) },
  env: envFor(USERS),
});
await record("an invite refuses without a session", {
  method: "POST",
  path: "/api/admin/invite",
  env: envFor(USERS),
});

// ── THE MODEL FAMILY'S TWO WRITES: DELETE AND RE-ENABLE ──────────────────────────────────────────
// A CONSOLE-OWNED model is REMOVED; a BUILT-IN is DISABLED. The registry decides which, and the two answers
// have different shapes (`removed` vs `disabled` + the whole list) — so both arms need a case.
const MODELS = { ...USERS, "models:custom": JSON.stringify([{ id: "my/thing", ownedBy: "my" }]) };
const del = (path) => ({ method: "DELETE", path, headers: { Cookie: cookie(adminToken) } });
const enable = (path) => ({ method: "PUT", path, headers: { Cookie: cookie(adminToken) } });

await record("deleting a console-owned model removes it", {
  ...del("/api/admin/models/my%2Fthing"),
  env: envFor(MODELS),
});
await record("deleting an id this deployment does not own is a 404", {
  ...del("/api/admin/models/my%2Fghost"),
  env: envFor(MODELS),
});
await record("deleting a BUILT-IN disables it instead of removing it", {
  ...del("/api/admin/models/og%2Fmimo-v2.5"),
  env: envFor(MODELS),
});
await record("a malformed percent-escape in a model id is the front door's 500", {
  ...del("/api/admin/models/%zz"),
  env: envFor(MODELS),
});
await record("re-enabling a built-in drops it from the disabled list, SORTED", {
  ...enable("/api/admin/models/og%2Fmimo-v2.5/enabled"),
  env: envFor({
    ...MODELS,
    "models:disabled": JSON.stringify(["zz/last", "og/mimo-v2.5", "aa/first"]),
  }),
});
await record("re-enabling an id that was not disabled writes the same list", {
  ...enable("/api/admin/models/og%2Fnever-disabled/enabled"),
  env: envFor({ ...MODELS, "models:disabled": JSON.stringify(["zz/last"]) }),
});
await record("the model writes refuse without a session", {
  method: "DELETE",
  path: "/api/admin/models/my%2Fthing",
  env: envFor(MODELS),
});

// ── ADDING A MODEL: THE VALIDATOR'S REFUSALS, THEN THE TWO ANSWERS ───────────────────────────────
const post = (body) => ({
  method: "POST",
  path: "/api/admin/models",
  body: JSON.stringify(body),
  headers: { Cookie: cookie(adminToken), "content-type": "application/json" },
});

// Every refusal below is a NAMED error the operator reads, in the source's own order.
await record("a model id is required", { ...post({}), env: envFor(MODELS) });
await record("an id must look like prefix/name", {
  ...post({ id: "noslash" }),
  env: envFor(MODELS),
});
await record("an unknown channel prefix lists the known ones", {
  ...post({ id: "zz/thing" }),
  env: envFor(MODELS),
});
await record("a name over 80 units, or with a control character, is refused", {
  ...post({ id: "qw/thing", name: "a".repeat(81) }),
  env: envFor(MODELS),
});
await record("a contextWindow of ZERO is refused (omitting it already means unset)", {
  ...post({ id: "qw/thing", contextWindow: 0 }),
  env: envFor(MODELS),
});
await record("a fractional capacity is refused", {
  ...post({ id: "qw/thing", maxTokens: 1.5 }),
  env: envFor(MODELS),
});
await record("a capacity above the ceiling is refused", {
  ...post({ id: "qw/thing", contextWindow: 10_000_001 }),
  env: envFor(MODELS),
});
await record("an unknown reasoning level is REFUSED, not dropped", {
  ...post({ id: "qw/thing", reasoningEffort: "extreme" }),
  env: envFor(MODELS),
});
await record("a BUILT-IN's pinned routing facet is refused BY NAME", {
  ...post({ id: "og/mimo-v2.5", wire: "mimo-2.5" }),
  env: envFor(MODELS),
});
await record("a console-owned model is stored with its defaults", {
  ...post({
    id: "qw/thing",
    name: "Thing",
    contextWindow: 128000,
    maxTokens: 4096,
    reasoningEffort: "high",
  }),
  env: envFor(MODELS),
});
await record("a wire is stored only on og/, and ownedBy defaults to the bare prefix", {
  ...post({ id: "og/new-model", wire: "new-wire" }),
  env: envFor(MODELS),
});
await record("a built-in's DISPLAY facets become an override", {
  ...post({ id: "og/mimo-v2.5", name: "Mimo", reasoningEffort: "low" }),
  env: envFor(MODELS),
});
await record("clearing every display facet REMOVES the override", {
  ...post({ id: "og/mimo-v2.5" }),
  env: envFor({
    ...MODELS,
    "models:overrides": JSON.stringify([{ id: "og/mimo-v2.5", name: "Old" }]),
  }),
});

// ── PROVIDERS: CREATE, EDIT (the key is kept), DELETE, AND THE CF TOKEN'S WRITE ──────────────────
const PROVIDERS = {
  ...USERS,
  "providers:custom": JSON.stringify([
    {
      prefix: "acme/",
      label: "Acme",
      baseURL: "https://acme.test/v1",
      api: "openai-completions",
      models: [{ id: "chat" }],
      apiKeyEnv: "ACME_API_KEY",
    },
  ]),
};
const postJson = (path, body) => ({
  method: "POST",
  path,
  body: JSON.stringify(body),
  headers: { Cookie: cookie(adminToken), "content-type": "application/json" },
});

// THE VALIDATOR'S REFUSALS: a built-in prefix (409), a prefix with no slash, and a create with no key.
await record("a provider on a BUILT-IN prefix is refused with the parser's own 409", {
  ...postJson("/api/admin/providers", {
    prefix: "og/",
    baseURL: "https://x.test",
    api: "openai-completions",
    apiKey: "k".repeat(24),
  }),
  env: envFor(PROVIDERS),
});
await record("a provider prefix must end in a slash", {
  ...postJson("/api/admin/providers", {
    prefix: "acme",
    baseURL: "https://x.test",
    api: "openai-completions",
    apiKey: "k".repeat(24),
  }),
  env: envFor(PROVIDERS),
});
await record("creating a provider with NO key is refused", {
  ...postJson("/api/admin/providers", {
    prefix: "new/",
    baseURL: "https://new.test/v1",
    api: "openai-completions",
  }),
  env: envFor(PROVIDERS),
});
await record("a new provider is stored, and its key is masked in the answer", {
  ...postJson("/api/admin/providers", {
    prefix: "new/",
    label: "New",
    baseURL: "https://new.test/v1",
    api: "openai-completions",
    apiKey: "sk-new-provider-key-0123456789",
    models: [{ id: "chat" }],
  }),
  env: envFor(PROVIDERS),
});
await record("EDITING an existing provider may omit the key — the stored one is kept", {
  ...postJson("/api/admin/providers", {
    prefix: "acme/",
    label: "Acme Renamed",
    baseURL: "https://acme.test/v1",
    api: "openai-completions",
    models: [{ id: "chat" }],
  }),
  env: envFor({ ...PROVIDERS, ACME_API_KEY: "sk-env-inline-key-0123456789" }),
});

const delProvider = (path) => ({ method: "DELETE", path, headers: { Cookie: cookie(adminToken) } });
await record("deleting a provider removes it and answers the slashed prefix", {
  ...delProvider("/api/admin/providers/acme"),
  env: envFor(PROVIDERS),
});
await record("deleting a provider that is not there is a 404", {
  ...delProvider("/api/admin/providers/ghost"),
  env: envFor(PROVIDERS),
});
await record("deleting an EMPTY prefix is its own 400", {
  ...delProvider("/api/admin/providers/%2F%2F"),
  env: envFor(PROVIDERS),
});
await record("a malformed escape in a provider prefix is a 400, not a 500", {
  ...delProvider("/api/admin/providers/%zz"),
  env: envFor(PROVIDERS),
});

// THE CF TOKEN'S WRITE: a shape check, and an empty value CLEARS the key.
await record("a Cloudflare token is stored when it looks like one", {
  method: "PUT",
  path: "/api/admin/cloudflare-token",
  body: JSON.stringify({ token: "cf-token-0123456789abcd" }),
  headers: { Cookie: cookie(adminToken), "content-type": "application/json" },
  env: envFor(PROVIDERS),
});
await record("a token that looks wrong is refused", {
  method: "PUT",
  path: "/api/admin/cloudflare-token",
  body: JSON.stringify({ token: "short" }),
  headers: { Cookie: cookie(adminToken), "content-type": "application/json" },
  env: envFor(PROVIDERS),
});
await record("an EMPTY token deletes the stored one", {
  method: "PUT",
  path: "/api/admin/cloudflare-token",
  body: JSON.stringify({ token: "" }),
  headers: { Cookie: cookie(adminToken), "content-type": "application/json" },
  env: envFor({ ...PROVIDERS, "cf:api_token": "cf-account-token" }),
});

// ── THE TWO READS THE CATALOGUE FILE LAYER UNLOCKS ───────────────────────────────────────────────
// The FILE WINS per id/prefix over KV, and the responses say which entries that is — so both a KV-only
// deployment and one whose console records are shadowed have cases.
const SHADOWED = {
  ...USERS,
  "models:custom": JSON.stringify([
    { id: "qw/console", ownedBy: "qw", name: "Console", contextWindow: 4096 },
    { id: "qw/gone", ownedBy: "qw" },
  ]),
  "models:overrides": JSON.stringify([{ id: "og/mimo-v2.5", name: "Override" }]),
  "models:disabled": JSON.stringify(["zz/last", "og/off"]),
  "providers:custom": JSON.stringify([
    {
      prefix: "acme/",
      label: "Acme",
      baseURL: "https://acme.test/v1",
      api: "openai-completions",
      models: [{ id: "chat" }],
      apiKeyEnv: "ACME_API_KEY",
    },
  ]),
};

await record("the model state: ids, the disabled list, the file's keys and the declared facets", {
  path: "/api/admin/models",
  headers: { Cookie: cookie(adminToken) },
  env: envFor(SHADOWED, { ACME_API_KEY: "sk-env-key-0123456789" }),
});
await record("the model state on a deployment with nothing at all", {
  path: "/api/admin/models",
  headers: { Cookie: cookie(adminToken) },
  env: envFor(USERS),
});
await record("the provider list names the protocols this build serves", {
  path: "/api/admin/providers",
  headers: { Cookie: cookie(adminToken) },
  env: envFor(SHADOWED, { ACME_API_KEY: "sk-env-key-0123456789" }),
});
await record("the provider list masks an inline key", {
  path: "/api/admin/providers",
  headers: { Cookie: cookie(adminToken) },
  env: envFor({
    ...USERS,
    "providers:custom": JSON.stringify([
      {
        prefix: "inline/",
        label: "Inline",
        baseURL: "https://inline.test/v1",
        api: "openai-completions",
        models: [{ id: "m" }],
        apiKey: "sk-inline-provider-key-0123",
      },
    ]),
  }),
});
await record("both reads refuse without a session", {
  path: "/api/admin/providers",
  env: envFor(SHADOWED),
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
