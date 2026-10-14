/**
 * auth.ts — credential PRIMITIVES leaf (zero-dependency, Web Crypto only).
 *
 * Which "auth" is which (naming map, security-round clarification):
 *   src/auth.ts          this file — pure primitives: hashing, safeEq,
 *                        HMAC session tokens, CSRF predicate. Imports NOTHING.
 *   src/session.ts       session RESOLUTION flow (requireSession/
 *                        requireAdmin/issueSessionSecret) — imports this
 *                        file + store + access.
 *   src/plugins/auth.ts  the auth PLUGIN's HTTP routes (login/register/
 *                        keys) — imports both.
 * New code: reach for these primitives via this file; resolution via
 * session.ts; never re-implement either (the devices.ts === bug was this
 * exact reachability failure).
 *
 * Passwords: PBKDF2-SHA256 with a per-user random salt and a fixed iteration count.
 * Sessions: HMAC-SHA256 signed cookie; the signing key is ADMIN_PASSWORD
 *           (the single Worker-secret trust anchor).
 */

// **THE PRIMITIVES ARE AN EMISSION NOW (2026-10-14)** — `auth-logic.ts`, GENERATED from
// `gateway/wasm/src/auth.rs`. What is left here is the HOST: `crypto.subtle`'s PBKDF2 and HMAC, the base64
// platform calls, and `randomHex`. Every decision — the compare's length rule, the cookie parser's four
// details, the CSRF gate's order, the two cookie headers' spellings, the base64url alphabet and the claims
// check — is the emission's. **This file imports NOTHING ELSE**, and that property is what the header above
// claims: the emission is a leaf, not a dependency edge.
import {
  b64urlDecodeStr,
  safeEq,
  sessionClaims,
  SESSION_TTL_MS,
  splitToken,
} from "./auth-logic.ts";

export {
  b64urlDecodeStr,
  clearSessionCookieHeader,
  csrfCookieViolation,
  MUTATING_METHODS,
  parseCookie,
  safeEq,
  sessionClaims,
  sessionCookieHeader,
  SESSION_COOKIE,
  SESSION_TTL_MS,
  splitToken,
} from "./auth-logic.ts";

/** PBKDF2 iteration count.
 *
 * ⚠️ **THIS CONSTANT IS BAKED INTO EVERY STORED HASH.** A user record holds
 * only `{ salt, passwordHash }` (store/users.ts) — no iteration count and no
 * algorithm version — and `verifyPassword` derives with the value below. So
 * RAISING this number (the normal hardening direction) makes every existing
 * hash stop matching: nobody can log in, and `verifyPassword` cannot tell
 * "wrong password" from "the constants moved", because it only compares
 * derived bits.
 *
 * CONTRAST — `SESSION_TTL_MS` below is safely changeable, and the difference
 * is worth understanding before "fixing" anything here: the session token
 * carries its own `exp` INSIDE the signed payload, so a changed TTL applies
 * to newly issued tokens and old ones keep the expiry they were minted with.
 * The password hash has no equivalent self-description.
 *
 * Migration recipe (NOT implemented — needs sign-off, see the ledger's Open
 * threads): persist the iteration count beside the hash, verify against the
 * STORED value, then re-hash on the next successful login to upgrade in place.
 * `auth.test.mjs` pins the current value and this consequence. */
export const PASSWORD_ITERATIONS = 100000;

export function randomHex(bytes: number): string {
  const b = new Uint8Array(bytes);
  crypto.getRandomValues(b);
  return [...b].map((x) => x.toString(16).padStart(2, "0")).join("");
}

export async function hashPassword(password: string, salt: string): Promise<string> {
  const enc = new TextEncoder();
  const key = await crypto.subtle.importKey("raw", enc.encode(password), "PBKDF2", false, [
    "deriveBits",
  ]);
  const bits = await crypto.subtle.deriveBits(
    { name: "PBKDF2", salt: enc.encode(salt), iterations: PASSWORD_ITERATIONS, hash: "SHA-256" },
    key,
    256,
  );
  return [...new Uint8Array(bits)].map((x) => x.toString(16).padStart(2, "0")).join("");
}

export async function verifyPassword(
  password: string,
  salt: string,
  expectedHash: string,
): Promise<boolean> {
  const h = await hashPassword(password, salt);
  return safeEq(h, expectedHash);
}

/// Auth-core audit MED-1: SameSite=Lax only blocks CROSS-SITE sends — but
/// every device panel (*.<dist-host>) is SAME-SITE with the console,
/// so rogue/XSS JS there could POST cookie-authed mutations (regenerate
/// token, toggle users, unpair…) with SameSite not saving us. Gate every
/// cookie-carrying mutation on Sec-Fetch-Site (browser-managed, unforgeable
/// via fetch): same-origin (our SPA) or none (direct/user-initiated) pass;
/// same-site/cross-site are rejected. Bearer-token clients carry no cookie
/// and are untouched.
/// Security follow-up: the gate keyed on ag_session only, but the device
/// proxy mints PER-DEVICE `summrise_pt_<name>` cookies (devices.ts) that
/// authenticate mutations on the proxy path too — every cookie-carrying
/// mutation means BOTH credential families. Same reasoning applies: a
/// same-site page holding neither cookie cannot ride them; the panel's own
/// same-origin fetches pass.

/* ---- Sessions (HMAC-signed, key = ADMIN_PASSWORD) ---- */

/** Session lifetime. SAFELY CHANGEABLE, unlike `PASSWORD_ITERATIONS` above:
 *  the value is not read at VERIFY time — it is written into the signed
 *  payload as `exp` when a token is issued, and verification trusts that
 *  embedded expiry. Changing this therefore affects only newly issued tokens. */

async function hmacKey(secret: string): Promise<CryptoKey> {
  return crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
}

function bytesToB64url(bytes: Uint8Array): string {
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function b64urlEncodeStr(str: string): string {
  return btoa(str).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** Issue a session token: `b64url(payload).hmac`, payload = { uid, role, exp } */
export async function issueSessionToken(
  secret: string,
  uid: string,
  role: string,
): Promise<string> {
  const payload = b64urlEncodeStr(JSON.stringify({ uid, role, exp: Date.now() + SESSION_TTL_MS }));
  const key = await hmacKey(secret);
  const sig = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(payload));
  return `${payload}.${bytesToB64url(new Uint8Array(sig))}`;
}

/** Verify a session token; returns { uid, role, exp } on success, or null */
export async function verifySessionToken(
  secret: string,
  token: string,
): Promise<{ uid: string; role: string } | null> {
  if (!secret || !token) return null;
  const split = splitToken(token);
  if (!split) return null;
  const [payload, sig] = split;
  const key = await hmacKey(secret);
  const sigBytes = new Uint8Array(
    await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(payload)),
  );
  // **THE SIGNATURE IS CHECKED BEFORE THE PAYLOAD IS PARSED**, and the claims rules are the emission's.
  if (!safeEq(bytesToB64url(sigBytes), sig)) return null;
  try {
    return sessionClaims(JSON.parse(b64urlDecodeStr(payload)), Date.now());
  } catch {
    return null;
  }
}
