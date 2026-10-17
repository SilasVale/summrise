/**
 * Summrise gateway plugin: auth — /api/auth/* (register / login / logout) and
 * /api/me/* (account info, route selection, US-proxy switch, user keys).
 *
 * round-73 plugin extraction; the migration is COMPLETE — this plugin is
 * the only implementation of these routes (index.ts's inline copies are gone).
 *
 * Dispatch contract: handlers receive (request, env, url) — the same triple
 * handleConsole(request, env, url) got. The auth handlers' `secure` flag is
 * derived here the same way handleConsole did: url.protocol === "https:".
 */

import { isAdvertised } from "../store/models.ts";
import {
  createUser,
  getUser,
  findUserByUsername,
  regenerateToken,
  rotateRelayToken,
  revokeRelayToken,
  getUserKeys,
  setUserKey,
  deleteUserKey,
  getAdminPassword,
  hasAdminPassword,
  verifyAdminPassword,
  setAdminPassword,
  maskKey,
  userKeysStatus,
  ADMIN_ID,
  USER_KEY_NAMES,
  getUserRoute,
  setUserRoute,
  getGlobalSetting,
  setGlobalSetting,
  globalSettingEnabled,
} from "../store.ts";
import {
  safeEq,
  verifyPassword,
  issueSessionToken,
  b64urlDecodeStr,
  parseCookie,
  sessionCookieHeader,
  clearSessionCookieHeader,
  SESSION_COOKIE,
} from "../auth.ts";
import { createIpRateLimiter } from "../lib/ratelimit.ts";
import { jsonOk, jsonError, readJson } from "../http.ts";
import type { PluginContext } from "./registry.ts";
import { resolveAutoModel } from "../model-route.ts";

const AUTH_BASE = "/api/auth";
const ME_BASE = "/api/me";

// round-104: per-IP gate for registration (2-3 KV writes per attempt — an
// attacker can exhaust the Free-plan daily KV write quota). In-memory like
// the devices plugin's public gate; no per-request KV writes.
const authRateLimited = createIpRateLimiter({ name: "auth-rate", limit: 30, windowMs: 60_000 });

// Session HMAC key + session resolution live in the shared session module
// (one requireSession contract across index + all plugins).
import { issueSessionSecret, requireSession } from "../session.ts";

/* ---- Register / Login ---- */

async function authRegister(request: Request, env: any, secure: boolean): Promise<Response> {
  // round-104: registration costs 2-3 KV writes per attempt — an attacker
  // can exhaust the Free-plan daily KV write quota (console-wide DoS).
  // Per-IP in-memory gate, same pattern as the devices plugin.
  if (await authRateLimited(request)) {
    return jsonError(429, "rate limit exceeded", "rate_limit_error");
  }
  const ap = await getAdminPassword(env);
  if (!ap) return jsonError(500, "Admin password not configured", "config_error");
  const body = await readJson(request);
  try {
    const created = await createUser(env, {
      username: body.username,
      password: body.password,
      inviteCode: body.inviteCode,
      role: "user", // always a normal user; the admin can only come from seeding
    });
    // Fail-closed issuance: without SESSION_SECRET there is no safe HMAC key
    // (the admin-password fallback would be offline-brute-forceable).
    const sk = issueSessionSecret(env);
    if (!sk) return jsonError(500, "Session signing not configured", "config_error");
    const token = await issueSessionToken(sk, created.id, created.role);
    return jsonOk(
      { ok: true, username: created.username, role: created.role, token: created.token },
      { "Set-Cookie": sessionCookieHeader(token, 86400, secure) },
    );
  } catch (e: any) {
    return jsonError(400, e.message, "invalid_request");
  }
}

async function authLogin(request: Request, env: any, secure: boolean): Promise<Response> {
  if (!(await hasAdminPassword(env)))
    return jsonError(500, "Admin password not configured", "config_error");
  const body = await readJson(request);
  // Brute-force throttle: 5 consecutive failures lock the username for 30s
  // (exponential backoff would need the failure count; a flat lock is simple
  // and stops online guessing of the 6-8 char passwords).
  // Trim + lowercase: findUserByUsername trims, so an untrimmed key let
  // "admin " variants bypass the lock while still reaching the real admin
  // password check (brute-force bypass).
  // Bind the lock to the CALLER (IP+username): a per-username-only key let
  // any unauthenticated attacker POST 5 wrong passwords and hold the admin
  // console login at 429 forever (permanent login-layer DoS).
  const callerIp = request.headers?.get?.("cf-connecting-ip") || "unknown";
  // Per-IP in-memory burst gate BEFORE the KV lock path: a brute-force loop
  // otherwise burns 2-3 KV WRITES per failed login — ~400 attempts exhaust
  // the Free-plan daily write quota (1000). >10 failures/min from one IP
  // short-circuits with no KV traffic; the KV lock below still handles the
  // sustained case.
  const gk = `login:${callerIp}:${Math.floor(Date.now() / 60000)}`;
  const gate = (__loginGate.get(gk) || 0) + 1;
  __loginGate.set(gk, gate);
  if (__loginGate.size > 4096) __loginGate.delete(__loginGate.keys().next().value);
  if (gate > 10) {
    return jsonError(429, "Too many attempts — try again in a moment", "rate_limit_error");
  }
  const lockKey = `login-lock:${callerIp}:${String(body.username || "")
    .trim()
    .toLowerCase()}`;
  const locked = await env.KEYS.get(lockKey);
  if (locked) {
    return jsonError(429, "Too many attempts — try again in ~30s", "rate_limit_error");
  }
  const user = await findUserByUsername(env, body.username);
  if (!user || !user.enabled) {
    // Auth-core audit LOW: burn the SAME PBKDF2 work as a real attempt — the
    // instant return was a username-enumeration timing oracle.
    await verifyPassword(
      body.password || "",
      "00000000000000000000000000000000",
      "00000000000000000000000000000000",
    );
    return jsonError(401, "Incorrect username or password", "authentication_error");
  }
  // eslint-disable-next-line no-useless-assignment
  let ok = false;
  if (user.id === ADMIN_ID) {
    // The admin account logs in with the admin password (stored HASHED —
    // compare via verifyAdminPassword, never read the plaintext).
    ok = await verifyAdminPassword(env, body.password || "");
  } else {
    ok =
      !!user.salt &&
      !!user.passwordHash &&
      (await verifyPassword(body.password || "", user.salt, user.passwordHash));
  }
  if (!ok) {
    // round-115: count failures in memory — the old KV counter (lockKey:":n")
    // burned one KV WRITE per failed attempt, the same quota-exhaustion path
    // as the register/pair endpoints (round-102/104 gated the rate but a
    // parallel burst across isolates still landed hundreds of writes). Only
    // arming the 30s lock touches KV. The lock itself is still KV, so a lock
    // armed on one isolate throttles every isolate.
    const fk = `login-fails:${callerIp}:${user.id}`;
    const fails = (__loginFails.get(fk) || 0) + 1;
    if (fails >= 5) {
      await env.KEYS.put(lockKey, "1", { expirationTtl: 60 });
      __loginFails.delete(fk);
    } else {
      __loginFails.set(fk, fails);
      if (__loginFails.size > 4096) __loginFails.delete(__loginFails.keys().next().value);
    }
    return jsonError(401, "Incorrect username or password", "authentication_error");
  }
  // Success clears the failure counter.
  __loginFails.delete(`login-fails:${callerIp}:${user.id}`);
  // Fail-closed issuance (see authRegister): no SESSION_SECRET → no session.
  const sk = issueSessionSecret(env);
  if (!sk) return jsonError(500, "Session signing not configured", "config_error");
  const token = await issueSessionToken(sk, user.id, user.role);
  return jsonOk(
    { ok: true, username: user.username, role: user.role },
    { "Set-Cookie": sessionCookieHeader(token, 86400, secure) },
  );
}

/* ---- Reset admin password (by admin gateway token) ----
 *
 * The admin password is stored HASHED (never plaintext) so there is no way
 * to "look it up" — the console shows only `set`/`not set`. If the admin
 * forgets the password, the console is locked out entirely (the admin is
 * the only account that can change it). Recovery: prove possession of the
 * admin gateway token (the `x-api-key` value in the console Overview —
 * also the user's Claude Code key) and set a new password. Rate-limited
 * like register so an attacker cannot brute-force the token.
 */
async function authResetPassword(request: Request, env: any): Promise<Response> {
  if (await authRateLimited(request)) {
    return jsonError(429, "rate limit exceeded", "rate_limit_error");
  }
  const admin = await getUser(env, ADMIN_ID);
  if (!admin) return jsonError(500, "Admin not seeded", "config_error");
  const body = await readJson(request);
  const adminKey = String(body?.adminKey || "").trim();
  const newPassword = String(body?.newPassword || "");
  if (!adminKey || !newPassword) {
    return jsonError(400, "adminKey and newPassword are required", "invalid_request");
  }
  if (newPassword.length < 8) {
    return jsonError(400, "New password must be at least 8 chars", "invalid_request");
  }
  // Prove possession of the admin gateway token (the console's Overview
  // shows this value; it is the same key Claude Code uses as x-api-key).
  if (!admin.token || !safeEq(adminKey, admin.token)) {
    return jsonError(403, "Invalid admin key", "authentication_error");
  }
  await setAdminPassword(env, newPassword);
  return jsonOk({ ok: true });
}

const __loginGate = new Map(); // `login:${ip}:${minute}` → failed-login burst count
const __loginFails = new Map(); // `login-fails:${ip}:${userId}` → consecutive failures (in-memory, round-115)

/* ---- Logout ---- */

async function authLogout(request: Request, env: any, secure: boolean): Promise<Response> {
  // round-110: logout is an unauthenticated KV-write endpoint (any crafted
  // cookie with a truthy exp triggers a write) — an attacker could exhaust
  // the daily KV write quota. Per-IP gate like register.
  // round-111: a 429 must STILL clear the client cookie — skipping the
  // whole handler left the browser cookie alive AND skipped the KV
  // blacklist (the endpoint's entire security purpose).
  if (await authRateLimited(request)) {
    return jsonError(429, "rate limit exceeded", "rate_limit_error", {
      "set-cookie": clearSessionCookieHeader(secure),
    });
  }
  // Revoke the session server-side: the HMAC cookie alone was cleared
  // client-side, but a copied cookie stayed valid for the full 24h. Blacklist
  // the token hash in KV until its exp so it dies everywhere.
  const cookie = parseCookie(request.headers.get("Cookie") || "")[SESSION_COOKIE];
  if (cookie && env.KEYS) {
    try {
      // Token format is payload.sig (see issueSessionToken) — decode
      // segment [0]. [1] is the binary HMAC signature: parsing it as JSON
      // always throws, which silently disabled this whole server-side
      // blacklist (logout cleared only the client cookie).
      // b64urlDecodeStr restores the stripped '=' padding (workerd's
      // atob is strict, unlike Node's) — same helper as verifySessionToken.
      const payload = JSON.parse(b64urlDecodeStr(cookie.split(".")[0] || ""));
      if (payload.exp) {
        // round-122: exp is stored in MS (issueSessionToken uses Date.now() + SESSION_TTL_MS) — the old *1000 treated it as seconds, overstating the remaining life ~1000x (every logout wrote a 2-day entry).
        // round-124: floor 60 — KV's minimum expirationTtl; the old floor of
        // 1 let a <60s-life session skip the blacklist write entirely.
        const ttl = Math.max(60, Math.min(86400, Math.ceil((payload.exp - Date.now()) / 1000)));
        await env.KEYS.put(`sess-revoked:${cookie}`, "1", { expirationTtl: ttl });
      }
    } catch {
      /* malformed cookie — ignore */
    }
  }
  return jsonOk({ ok: true }, { "Set-Cookie": clearSessionCookieHeader(secure) });
}

/* ---- /api/me (session-gated; gate copied from handleConsole) ---- */

async function meGet(request: Request, env: any): Promise<Response> {
  const user = await requireSession(request, env);
  if (!user) return jsonError(401, "Not logged in or session expired", "authentication_error");
  const ukeys = await getUserKeys(env, user.id);
  return jsonOk({
    id: user.id,
    username: user.username,
    role: user.role,
    enabled: user.enabled,
    token: user.token,
    relayTokenSet: !!user.relayToken,
    keys: userKeysStatus(ukeys, env),
  });
}

// Per-user route selection (Claude Code model=auto)
// resolveAutoModel is imported directly from model-route.ts (module-level
// so the route handler can call it — handlers don't receive plugin context).
let resolveRouteModel: ((env: any, uid: string) => Promise<string>) | null = null;

async function meGetRoute(request: Request, env: any): Promise<Response> {
  const user = await requireSession(request, env);
  if (!user) return jsonError(401, "Not logged in or session expired", "authentication_error");
  // effective: the model a model=auto request actually resolves to
  // (stored route if usable, else the usable fallback). The panel shows
  // this as "current" so a user without a manual choice still sees what
  // they're actually using.
  const model = await getUserRoute(env, user.id);
  const effective = resolveRouteModel ? await resolveRouteModel(env, user.id) : model;
  return jsonOk({ model, effective });
}

async function mePutRoute(request: Request, env: any): Promise<Response> {
  const user = await requireSession(request, env);
  if (!user) return jsonError(401, "Not logged in or session expired", "authentication_error");
  const body = await readJson(request);
  const model = body?.model ?? null;
  // ADVERTISED, not merely compiled: a model disabled from the console must not be
  // settable, or a user could pin a route the gateway will not serve.
  if (model !== null && !(await isAdvertised(env, model))) {
    return jsonError(400, `Unknown model: ${model}`, "invalid_request");
  }
  await setUserRoute(env, user.id, model);
  return jsonOk({ ok: true, model });
}

async function meRegenerateToken(request: Request, env: any): Promise<Response> {
  const user = await requireSession(request, env);
  if (!user) return jsonError(401, "Not logged in or session expired", "authentication_error");
  const token = await regenerateToken(env, user.id);
  return jsonOk({ ok: true, token });
}

// F3 scoped relay token (ADR-0007 step 1): issue/rotate the caller's relay
// credential for settings.json. The relay token resolves to its owner with
// role "relay" — translate/models dual-accept it, /mcp + recovery reject
// it. Returned in the clear exactly once, like the admin token rotation
// above; afterwards only presence (meGet relayTokenSet) is visible.
async function meRotateRelayToken(request: Request, env: any): Promise<Response> {
  const user = await requireSession(request, env);
  if (!user) return jsonError(401, "Not logged in or session expired", "authentication_error");
  const token = await rotateRelayToken(env, user.id);
  return jsonOk({ ok: true, token });
}

// Revoke the caller's relay token (a leaked settings.json without touching
// the admin token — the rotation coupling F3 was created to break).
async function meRevokeRelayToken(request: Request, env: any): Promise<Response> {
  const user = await requireSession(request, env);
  if (!user) return jsonError(401, "Not logged in or session expired", "authentication_error");
  const revoked = await revokeRelayToken(env, user.id);
  return jsonOk({ ok: true, revoked });
}

// Session-gated full-value read for the console Relay section ("forgot to
// save after issuing" — same trust as keys/reveal: a session holder can
// already rotate the credential, so reading it adds no privilege).
async function meRevealRelayToken(request: Request, env: any): Promise<Response> {
  const user = await requireSession(request, env);
  if (!user) return jsonError(401, "Not logged in or session expired", "authentication_error");
  if (!user.relayToken) return jsonError(404, "Relay token not configured", "not_found_error");
  return jsonOk({ ok: true, value: user.relayToken });
}

// US egress switch (global setting): GET reads the current value; PUT changes it (admin only).
// The gateway reads KV on every request route, so the switch takes effect immediately without a restart.
async function meGetUsproxy(request: Request, env: any): Promise<Response> {
  const user = await requireSession(request, env);
  if (!user) return jsonError(401, "Not logged in or session expired", "authentication_error");
  const v = await getGlobalSetting(env, "US_PROXY");
  return jsonOk({ enabled: !!v });
}

async function mePutUsproxy(request: Request, env: any): Promise<Response> {
  const user = await requireSession(request, env);
  if (!user) return jsonError(401, "Not logged in or session expired", "authentication_error");
  if (user.role !== "admin") {
    return jsonError(403, "Admin only", "forbidden");
  }
  const body = await readJson(request);
  // round-94: an explicit OFF is persisted as "0" (not deleted) so it
  // shadows an env/US_PROXY Worker var — deleting let the env fallback
  // bounce the toggle straight back ON.
  await setGlobalSetting(env, "US_PROXY", body?.enabled ? "1" : "0");
  const v = await getGlobalSetting(env, "US_PROXY");
  return jsonOk({ ok: true, enabled: globalSettingEnabled(v) });
}

async function mePutKeys(request: Request, env: any): Promise<Response> {
  const r = await sessionAndKeyName(request, env, USER_KEY_NAMES);
  if (r instanceof Response) return r;
  const { user, name, body } = r;
  const { value } = body || {};
  if (typeof value !== "string" || !value.trim())
    return jsonError(400, "value must not be empty", "invalid_request");
  const v = value.trim();
  await setUserKey(env, user.id, name, v);
  return jsonOk({ ok: true, name, masked: maskKey(v) });
}

// POST /api/me/keys/reveal {name} — the session-gated full-key read for the
// Keys page's copy button. The /api/me list deliberately returns only
// maskKey() output (display hygiene — masks survive screenshots and DOM
// scrapes), which made the old copy button copy the MASK instead of the
// credential. This route is the explicit-intent counterpart: one key per
// click, same session trust as save/delete (a session holder can already
// rotate or clear the key; the value is the caller's OWN BYOK credential).
// Kept POST-with-body like its sibling routes (no key names in URLs/logs).

/**
 * Shared prologue of the /api/me/keys/{save,reveal,test,usage} handlers:
 * resolve the session, read the JSON body's `name` and validate it against
 * `allowed`. Returns the {user, name, body} triple, or a Response the caller
 * should return directly (401 / 400). The body rides along because the
 * request stream is consumed by the read — callers needing more fields
 * (save's `value`) must use this copy, never re-read. The per-handler
 * prologues used to be copy-pasted; the usage route passed its narrower
 * 3-name set until it moved to Rust (2026-10-17), and `meTestKeys`,
 * `meRevealKey` and `mePutKeys` still share this one. (SOLID Round-91:
 * mePutKeys was the last inline copy; the claim above is true for all three.)
 */
async function sessionAndKeyName(
  request: Request,
  env: any,
  allowed: readonly string[],
): Promise<{ user: any; name: string; body: any } | Response> {
  const user = await requireSession(request, env);
  if (!user) return jsonError(401, "Not logged in or session expired", "authentication_error");
  const body = await readJson(request);
  const name = body?.name;
  if (!allowed.includes(name))
    return jsonError(400, `Unknown key name: ${name}`, "invalid_request");
  return { user, name, body };
}

async function meRevealKey(request: Request, env: any): Promise<Response> {
  const r = await sessionAndKeyName(request, env, USER_KEY_NAMES);
  if (r instanceof Response) return r;
  const { user, name } = r;
  const ukeys = await getUserKeys(env, user.id);
  const key = ukeys[name];
  if (!key) return jsonError(404, "Key not configured", "not_found_error");
  return jsonOk({ ok: true, name, value: key });
}

async function meDeleteKeys(request: Request, env: any, url: URL): Promise<Response> {
  const user = await requireSession(request, env);
  if (!user) return jsonError(401, "Not logged in or session expired", "authentication_error");
  const name = url.searchParams.get("name") || "";
  if (!USER_KEY_NAMES.includes(name))
    return jsonError(400, `Unknown key name: ${name}`, "invalid_request");
  await deleteUserKey(env, user.id, name);
  return jsonOk({ ok: true, name });
}

// ── THE USAGE ROUTE IS `gateway/wasm/src/key_probe.rs`'S NOW (2026-10-17) ────────────────────────
//
// `POST /api/me/keys/usage`, its `usageQuery` skeleton, its three mappings (`mapOpenRouterUsage`,
// `mapAmdUsage`, `mapOgUsage`), its `USAGE_QUERIES` table and `usageQueryFor` all left this file. **THE MAPPINGS
// WERE ALREADY RUST'S** — `decisions.rs` had served them over `POST /__decision` since the decision route
// existed — so what moved was the half around them: the session gate, the key lookup, the fetch and the
// envelope. With the route on the Rust side the decision hop is gone for these three (the table shrank to the
// one row `store/providers.ts` still asks for), and the console's four pins on the envelope live in
// `key_probe.rs`'s own tests.
//
// **WHAT IS *NOT* HERE ANY MORE IS THE FALLBACK, AND THAT IS DELIBERATE**: a deployment without `WASM_GATE`
// answers the front door's 404 for this path, the same end state `/mcp` and `/api/admin/models/probe` reached.
// `sessionAndKeyName` stays — `meRevealKey` and `mePutKeys` still share it.

// ── THE KEY PROBE IS `gateway/wasm/src/key_probe.rs`'S NOW (2026-10-17) ─────────────────────────
//
// `POST /api/me/keys/test`, `keyProbeResult` and `testKey`'s eight arms (deepseek, openrouter,
// opencode-go with its US-proxy switch and SSE first-chunk read, command code, gmi, amd with its model
// list, nvidia, qwen) all left this file. **THE COMPLETENESS CHECK LEFT WITH THEM AND GOT STRICTER**: the
// deleted `key-probe.test.mjs` asserted that every `USER_KEY_NAMES` entry "probes to a Response", which the
// backstop's `400 No probe for key name` satisfied — so it held while `R4_API_KEY` had no arm at all. The
// Rust check names that gap instead of accepting it (`key_probe.rs`'s `every_managed_key_has_an_arm`).
//
// **THE FALLBACK GOES WITH THE ROUTE**, as `/mcp`, `/api/admin/models/probe` and `/api/me/keys/usage`
// established: a deployment without `WASM_GATE` answers the front door's 404 for this path.

export default {
  name: "auth",
  setup(ctx: PluginContext) {
    // **`meGetRoute` RESOLVES THE EFFECTIVE MODEL THROUGH A DIRECT IMPORT NOW.** It used to read
    // `ctx.api.translate.resolveAutoModel` — a soft dependency on the translate plugin, with a null fallback. That
    // plugin was DELETED at the cutover (its `/v1` half is the Rust worker's), and a soft dependency that resolves
    // to null is exactly the silent degradation this repository keeps finding: `GET /api/me/route` would have gone
    // on answering, with `effective` silently equal to the stored model. `plugins/model-route.ts` is where the
    // function has lived since the structure refactor, and it is imported directly, so a missing export is a build
    // error rather than a quiet null.
    resolveRouteModel = resolveAutoModel;
    // Exact method+path match, same as the index.js if/else chain (the
    // registry's route() helper does prefix matching — exact here so
    // /api/me never swallows /api/me/route etc.). Order mirrors index.js.
    const add = (method: string, path: string, handler: (...args: any[]) => any) =>
      ctx.routes.push({ match: (m: string, p: string) => m === method && p === path, handler });
    // `secure` derived exactly as handleConsole did: url.protocol === "https:"
    const withSecure =
      (fn: (request: Request, env: any, secure: boolean) => Response | Promise<Response>) =>
      (request: Request, env: any, url: URL): Response | Promise<Response> =>
        fn(request, env, url.protocol === "https:");
    add("POST", `${AUTH_BASE}/register`, withSecure(authRegister));
    add("POST", `${AUTH_BASE}/login`, withSecure(authLogin));
    add("POST", `${AUTH_BASE}/reset-password`, authResetPassword);
    add("POST", `${AUTH_BASE}/logout`, withSecure(authLogout));
    add("GET", ME_BASE, meGet);
    add("GET", `${ME_BASE}/route`, meGetRoute);
    add("PUT", `${ME_BASE}/route`, mePutRoute);
    add("POST", `${ME_BASE}/token/regenerate`, meRegenerateToken);
    add("POST", `${ME_BASE}/token/relay`, meRotateRelayToken);
    add("DELETE", `${ME_BASE}/token/relay`, meRevokeRelayToken);
    add("POST", `${ME_BASE}/token/relay/reveal`, meRevealRelayToken);
    add("GET", `${ME_BASE}/usproxy`, meGetUsproxy);
    add("PUT", `${ME_BASE}/usproxy`, mePutUsproxy);
    add("PUT", `${ME_BASE}/keys`, mePutKeys);
    add("DELETE", `${ME_BASE}/keys`, meDeleteKeys);
    add("POST", `${ME_BASE}/keys/reveal`, meRevealKey);
  },
};
