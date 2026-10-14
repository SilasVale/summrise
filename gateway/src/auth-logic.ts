/**
 * auth-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/auth.rs` (`render_typescript`). The freshness test in that module is a byte
 * comparison against this file:
 *
 *     SUMMRISE_REFRESH_AUTH=1 cargo test --manifest-path gateway/wasm/Cargo.toml auth::emission
 *
 * `gateway/src/auth.ts` re-exports this module and keeps the HOST: `crypto.subtle`'s PBKDF2 and HMAC, the
 * base64 platform calls, and `randomHex`.
 */

/** The cookie the whole console keys on. `session-logic.ts` carries it too, from `session_gate.rs` — ONE
 *  spelling, two emissions of it, and the tests that import either get the same string. */
export const SESSION_COOKIE = "ag_session";

/** Session lifetime. **SAFELY CHANGEABLE**, unlike the password iteration count: the value is not read at
 *  VERIFY time — it is written into the signed payload as `exp` at issuance, and verification trusts that
 *  embedded expiry. */
export const SESSION_TTL_MS = 24 * 3600 * 1000;

/** The methods the CSRF gate looks at. */
export const MUTATING_METHODS = new Set(["POST", "PUT", "PATCH", "DELETE"]);

/** The timing-safe compare: **LENGTH FIRST**, then a constant-time XOR fold. A length difference is not a
 *  secret; the contents are. */
export function safeEq(a: string, b: string): boolean {
  if (typeof a !== "string" || typeof b !== "string" || a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

/**
 * `parseCookie(str)` — split on `;`, keep the pairs with an `=`, take the FIRST `=`, trim BOTH sides.
 *
 * **FOUR DETAILS A "SPLIT ON `=` AND TRIM" PORT GETS WRONG**: a pair with no `=` is SKIPPED rather than
 * stored as an empty value; the split is at the FIRST `=` so a value may contain more of them; both sides are
 * trimmed; and a duplicate name keeps the LAST one — a plain object assignment.
 */
export function parseCookie(str: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const pair of str.split(";")) {
    const i = pair.indexOf("=");
    if (i < 0) continue;
    const name = pair.slice(0, i).trim();
    out[name] = pair.slice(i + 1).trim();
  }
  return out;
}

/**
 * `csrfCookieViolation(request)` — **THE GATE IS ABOUT COOKIES, NOT ABOUT WRITES**: a mutating request is only
 * a CSRF candidate when the browser will ATTACH a credential by itself, which is why the bearer path is
 * exempt. A missing `sec-fetch-site` is exempt too (a non-browser client), and the two accepted values are
 * `same-origin` and `none` — anything else, including an absent-looking `""`, is a violation.
 */
export function csrfCookieViolation(request: Request): boolean {
  const method = request.method.toUpperCase();
  if (!MUTATING_METHODS.has(method)) return false;
  const cookie = request.headers.get("cookie") || "";
  const hasSession = cookie.includes(`${SESSION_COOKIE}=`);
  const hasDevicePair = Object.keys(parseCookie(cookie)).some((n) => n.startsWith("summrise_pt_"));
  if (!hasSession && !hasDevicePair) return false; // bearer path
  const site = request.headers.get("sec-fetch-site");
  if (site == null) return false;
  const v = site.toLowerCase();
  return v !== "same-origin" && v !== "none";
}

/** `b64urlDecodeStr(s)` — the URL-safe alphabet back to standard, PADDED to a multiple of four, then the
 *  platform's decode. */
export function b64urlDecodeStr(s: string): string {
  const pad = s.length % 4 === 0 ? "" : "=".repeat(4 - (s.length % 4));
  return atob(s.replace(/-/g, "+").replace(/_/g, "/") + pad);
}

/** The session cookie as the console sets it. `Secure` is the caller's — a loopback deployment has no TLS. */
export function sessionCookieHeader(token: string, maxAgeSec: number, secure: boolean): string {
  return `${SESSION_COOKIE}=${token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=${maxAgeSec}${secure ? "; Secure" : ""}`;
}

/** The same shape with an empty value and `Max-Age=0`. */
export function clearSessionCookieHeader(secure: boolean): string {
  return `${SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0${secure ? "; Secure" : ""}`;
}

/** `splitToken(token)` — at the FIRST dot, and `null` when there is none. */
export function splitToken(token: string): [string, string] | null {
  const dot = token.indexOf(".");
  if (dot < 0) return null;
  return [token.slice(0, dot), token.slice(dot + 1)];
}

/**
 * `verifySessionToken`'s last three steps — the claims, and the two rules that make them safe to trust:
 * **`exp` MUST BE A NUMBER** (a string `exp` is not compared as text) and must not have passed; `uid` is
 * `String(data.uid)`, and `role` falls back to `"user"` through a TRUTHINESS test, so `""` and `0` fall back
 * too.
 */
export function sessionClaims(data: any, nowMs: number): { uid: string; role: string } | null {
  if (typeof data?.exp !== "number") return null;
  if (data.exp < nowMs) return null;
  return { uid: String(data.uid), role: String(data.role || "user") };
}
