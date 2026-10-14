/**
 * session-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/session_gate.rs` (`render_typescript`). The freshness test in that module is a
 * byte comparison against this file:
 *
 *     SUMMRISE_REFRESH_SESSION=1 cargo test --manifest-path gateway/wasm/Cargo.toml session_gate::emission
 *
 * `gateway/src/session.ts` re-exports this module and keeps the HOST: the cookie parse, the Cloudflare Access
 * fallback, the store seam (`SessionUserStore`), the isolate cache the revocation reads go through, and the two
 * refusal responses.
 */

/** The session's lifetime — 24 hours, written into the signed payload as `exp` at ISSUANCE. */
export const SESSION_TTL_MS = 24 * 3600 * 1000;

/** The cookie the whole console keys on. */
export const SESSION_COOKIE = "ag_session";

/** The revocation negative cache: 60 s, bounded at 512 entries like the source's `Map`. */
export const NOT_REVOKED_TTL_MS = 60000;
export const NOT_REVOKED_MAX = 512;

/** The two refusals, spelled exactly — they are response bodies. */
export const NOT_LOGGED_IN_MESSAGE = "Not logged in or session expired";
export const ADMIN_REQUIRED_MESSAGE = "Admin permission required";

/** The line the console logs when it refuses to ISSUE a session. **FAIL CLOSED**: without a dedicated
 *  high-entropy `SESSION_SECRET` the HMAC key would fall back to the admin password, letting any invited user
 *  offline-brute-force it from their own signed cookie — HMAC-SHA256 is not memory-hard. */
export const ISSUE_SECRET_REFUSED_LOG =
  "[session] SESSION_SECRET not configured — refusing to issue new session (fail-closed)";

/** The VERIFY path's key choice (rotation compat): the dedicated `SESSION_SECRET`, or the admin password so a
 *  first-time `SESSION_SECRET` rollout does not log everyone out. **ISSUANCE MUST NOT USE THIS** — it uses
 *  `issueSessionSecret`, which fails closed. */
export function sessionSecret(env: any, adminPassword: string): string {
  return env?.SESSION_SECRET || adminPassword;
}

/** The ISSUANCE key, or `null` when unconfigured — the caller must refuse to issue a session (HTTP 500
 *  `config_error`) rather than silently signing with the admin password. */
export function issueSessionSecret(env: any): string | null {
  return env?.SESSION_SECRET ? env.SESSION_SECRET : null;
}

/** The admin gate's role test, spelled once. */
export function isAdmin(user: unknown): boolean {
  return (user as { role?: unknown } | null | undefined)?.role === "admin";
}

/** What the revocation cache holds for one cookie. */
export interface RevokedEntry {
  revoked: boolean;
  exp: number;
}

/** The isolate cache's surface — the HOST owns the `Map`, because it is platform state. */
export interface RevokedCache {
  get(key: string): RevokedEntry | undefined;
  set(key: string, entry: RevokedEntry): void;
  size(): number;
  clear(): void;
}

/** A verified session's claims. */
export interface SessionClaims {
  uid: string;
  role: string;
}

/** Everything `cookieSession` needs from the platform, named once.
 *
 *  **GENERIC IN THE USER RECORD, BECAUSE THE SEQUENCE RETURNS THE USER, NOT THE CLAIMS.** The source returns
 *  `user` — the record its caller then reads fields off — and an earlier revision of this emission returned the
 *  verified CLAIMS instead: `role` survived (so the admin gate still worked) while every other field was
 *  missing, which the console's own suite caught as 28 failures across session-gated routes. The claims are the
 *  uid lookup's input and nothing else. */
export interface CookieSessionDeps<T> {
  /** The deployment's admin password — an EMPTY one means no session can exist. */
  adminPassword: string;
  /** The env-like object the session flow reads — its ONE field is `SESSION_SECRET`, and it is an object
   *  rather than a string because the SOURCE's `sessionSecret(env, adminPassword)` takes one. */
  env: { SESSION_SECRET?: string | null };
  /** The request's session cookie, already parsed out (the host owns header parsing). */
  cookie: string | undefined;
  /** Whether the KV binding exists at all: the revocation read is skipped without one. */
  hasKeys: boolean;
  /** `Date.now()`, injected so the cache's expiry is testable. */
  nowMs(): number;
  /** The revocation read: `(await KEYS.get(`sess-revoked:${cookie}`)) === "1"`. */
  readRevoked(cookie: string): Promise<boolean>;
  /** The negative cache, bounded and cleared by the host's `Map`. */
  cache: RevokedCache;
  /** `verifySessionToken(secret, token)`. */
  verify(secret: string, token: string): Promise<SessionClaims | null>;
  /** The user record, or null when it is gone. */
  loadUser(uid: string): Promise<T | null>;
}

/**
 * **THE SESSION SEQUENCE, IN ORDER — AND THE ORDER IS THE BEHAVIOUR:**
 *
 *   1. an EMPTY admin password answers `null` before anything else is read;
 *   2. a missing cookie answers `null`;
 *   3. a REVOKED cookie dies BEFORE the signature is verified (a copied cookie is dead even when it verifies),
 *      through a 60 s negative cache that is CLEARED rather than pruned at its bound;
 *   4. the signature is tried with `sessionSecret(...)`, then — ONCE, and only when the two keys really differ —
 *      with the admin password, for the rotation window;
 *   5. the user must exist AND be enabled.
 *
 * Every arm returns `null` rather than distinguishing its reason: the caller answers 401 either way, and the
 * distinction is not one a client may learn.
 */
export async function cookieSession<T extends { enabled?: boolean }>(
  deps: CookieSessionDeps<T>,
): Promise<T | null> {
  const adminPassword = deps.adminPassword;
  if (!adminPassword) return null;
  const cookie = deps.cookie;
  if (!cookie) return null;
  if (deps.hasKeys) {
    const hit = deps.cache.get(cookie);
    if (hit && hit.exp > deps.nowMs()) {
      if (hit.revoked) return null;
    } else {
      const revoked = await deps.readRevoked(cookie);
      if (deps.cache.size() >= NOT_REVOKED_MAX) deps.cache.clear();
      deps.cache.set(cookie, { revoked, exp: deps.nowMs() + NOT_REVOKED_TTL_MS });
      if (revoked) return null;
    }
  }
  // **ROTATION COMPAT, ONCE**: `SESSION_SECRET` is set now, but this cookie may have been signed with the old
  // admin-password key — and the two keys must really differ for the second attempt to be a rotation rather
  // than the same verify twice.
  const primary = sessionSecret(deps.env, adminPassword);
  let session = await deps.verify(primary, cookie);
  if (
    !session &&
    deps.env.SESSION_SECRET &&
    adminPassword &&
    deps.env.SESSION_SECRET !== adminPassword
  ) {
    session = await deps.verify(adminPassword, cookie);
  }
  if (!session) return null;
  // **THE USER, NOT THE CLAIMS** — `role` is the only claim the caller reads, and its presence is what made a
  // claims-returning version look correct while breaking twenty-eight routes.
  const user = await deps.loadUser(session.uid);
  if (!user || !user.enabled) return null;
  return user;
}
