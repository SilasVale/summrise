/**
 * session.ts — session RESOLUTION flow (the middle layer of the auth trio:
 * primitives in auth.ts ← this file ← HTTP routes in plugins/auth.ts).
 * The single requireSession/sessionSecret implementation.
 *
 * Was copied verbatim into index.ts and plugins/{auth,devices,admin,mcp}.ts;
 * every copy drifted independently (the mcp plugin's round-88 gate missed the
 * sess-revoked blacklist for weeks). One module, one contract:
 *
 *   sessionSecret(env, adminPassword) — HMAC signing key for the session
 *   cookie. Prefers the dedicated high-entropy SESSION_SECRET (wrangler
 *   secret) over the admin password: using the password directly lets any
 *   invited user offline-brute-force it from their own signed cookie
 *   (HMAC-SHA256 is not memory-hard); with SESSION_SECRET set, the password
 *   is never a signing key.
 *
 *   requireSession(request, env) — resolves the console user from the
 *   session cookie, or null. Checks, in order: an admin password exists at
 *   all (no password → no sessions), the cookie is present, the cookie was
 *   not revoked by logout (server-side KV blacklist — a copied cookie dies
 *   too), the HMAC signature verifies, and the user still exists + enabled.
 */

import {
  getAdminPassword as liveGetAdminPassword,
  getUser as liveGetUser,
  type User,
} from "./store.ts";
import { parseCookie, verifySessionToken } from "./auth.ts";
// **THE SEQUENCE ITSELF IS AN EMISSION NOW (2026-10-14)** — `session-logic.ts`, GENERATED from
// `gateway/wasm/src/session_gate.rs`. What is left here is the HOST: the cookie parse, the Cloudflare Access
// fallback, the store seam, the isolate `Map` the revocation reads go through, and the two refusals. **The
// ORDER of the checks is not written here any more** — it is `cookieSession` in the emission, because the order
// is the security property and a comment cannot be reordered into a vulnerability the way code can.
import {
  ADMIN_REQUIRED_MESSAGE,
  cookieSession,
  isAdmin,
  NOT_LOGGED_IN_MESSAGE,
  SESSION_COOKIE,
} from "./session-logic.ts";

/** Production binding — the previous direct imports, named as one seam. */
export const liveSessionStore: SessionUserStore = {
  getAdminPassword: liveGetAdminPassword,
  getUser: liveGetUser,
};

export {
  ADMIN_REQUIRED_MESSAGE,
  isAdmin,
  issueSessionSecret,
  NOT_LOGGED_IN_MESSAGE,
  NOT_REVOKED_MAX,
  NOT_REVOKED_TTL_MS,
  SESSION_COOKIE,
  SESSION_TTL_MS,
  sessionSecret,
} from "./session-logic.ts";
import { jsonError } from "./http.ts";
import { requireAccessSession } from "./access.ts";

/**
 * Session user-store seam (SOLID Round-5: DIP).
 *
 * The resolution flow previously imported the KV-backed store directly, so
 * every consumer transitively depended on Cloudflare KV and the middle
 * layer was untestable without a KV stub. High-level policy now depends on
 * this narrow interface instead:
 *   - production passes nothing (defaults to `liveSessionStore`, the exact
 *     previous behavior — all ~25 existing 2-arg call sites compile and
 *     behave identically);
 *   - tests inject a fake (no KV, no cache, deterministic).
 * Keep the surface minimal: only the two reads the flow needs. Anything
 * wider would drag the abstraction back toward the concrete store (ISP).
 */
export interface SessionUserStore {
  getAdminPassword(env: any): Promise<string>;
  getUser(env: any, uid: string): Promise<User | null>;
}

// Negative-cache for the revocation check — this was the LAST raw KV read on
// every console request (the 30s status poll alone burned ~3k reads/day
// against the free tier). Tradeoff matches AUTH_CACHE_TTL elsewhere in the
// store: a logout takes <=60s to propagate to a hot isolate.
const __notRevoked = new Map<string, { revoked: boolean; exp: number }>();

/** The isolate cache, as the emission's interface — the `Map` is platform state and stays here. */
const revokedCache = {
  get: (key: string) => __notRevoked.get(key),
  set: (key: string, entry: { revoked: boolean; exp: number }) => {
    __notRevoked.set(key, entry);
  },
  size: () => __notRevoked.size,
  clear: () => __notRevoked.clear(),
};

export async function requireSession(
  request: Request,
  env: any,
  store: SessionUserStore = liveSessionStore,
): Promise<User | null> {
  const cookieUser = await requireCookieSession(request, env, store);
  if (cookieUser) return cookieUser;
  // No valid session cookie — fall back to the edge-verified Cloudflare
  // Access identity (option C). No-op unless ACCESS_AUD/TEAM_DOMAIN are set.
  return requireAccessSession(request, env);
}

async function requireCookieSession(
  request: Request,
  env: any,
  store: SessionUserStore,
): Promise<User | null> {
  // **`parseCookie(request.headers.get("Cookie") || "")[SESSION_COOKIE]`** — the host owns the header read.
  const cookie = parseCookie(request.headers.get("Cookie") || "")[SESSION_COOKIE];
  return (await cookieSession({
    adminPassword: await store.getAdminPassword(env),
    env,
    cookie,
    hasKeys: !!env.KEYS,
    nowMs: () => Date.now(),
    readRevoked: async (token: string) => (await env.KEYS.get(`sess-revoked:${token}`)) === "1",
    cache: revokedCache,
    verify: (secret: string, token: string) => verifySessionToken(secret, token),
    loadUser: (uid: string) => store.getUser(env, uid),
  })) as User | null;
}

/**
 * Admin gate used by every /api/admin/* and device-module handler: resolves
 * the session and requires role === "admin". Returns the user, or a Response
 * the handler should return directly (401 not logged in / 403 non-admin).
 */
export async function requireAdmin(
  request: Request,
  env: any,
  store: SessionUserStore = liveSessionStore,
): Promise<User | Response> {
  const user = await requireSession(request, env, store);
  if (!user) return jsonError(401, NOT_LOGGED_IN_MESSAGE, "authentication_error");
  if (!isAdmin(user)) return jsonError(403, ADMIN_REQUIRED_MESSAGE, "authorization_error");
  return user;
}
