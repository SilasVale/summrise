/**
 * access.ts — Cloudflare Access IDENTITY layer (zero-password console login,
 * spec option C). Sits beside session.ts in the resolution flow:
 * requireSession tries the cookie session first, then falls back to this
 * module's edge-verified identity. Reviewed 2026-09-06: correctly layered
 * (one-way deps store/auth; JWT verify self-contained with L5 hardening).
 *
 * After a visitor passes the zone's Access email-OTP challenge, Cloudflare
 * injects a signed JWT into every origin-bound request:
 *
 *     Cf-Access-Jwt-Assertion: <RS256 JWT, claims include email + aud + exp>
 *
 * This module verifies that JWT against the team's published certs and maps
 * the verified email onto a console user:
 *
 *   - ACCESS_ADMIN_EMAIL match  → the seeded admin account
 *   - previously-seen email      → its bound account (KV access-email:<email>)
 *   - first time seen            → auto-provision a passwordless "user"
 *
 * Password accounts keep working unchanged: requireSession tries the cookie
 * session FIRST (local/dev fallback), and only falls back to this path when
 * there is no valid session cookie. Multi-user is preserved — identity is
 * the verified email instead of a shared password.
 *
 * **THE DECISIONS ARE AN EMISSION NOW (2026-10-14)** — `access-logic.ts`, GENERATED from
 * `gateway/wasm/src/access.rs`. What is left here is the HOST: the JWKS fetch and its cache, the
 * `crypto.subtle` verify, the KV reads and writes, and the provisioning lock.
 */

interface Env {
  [key: string]: any;
}
import type { User } from "./store.ts";
import { withKeyLock } from "./store.ts";
import { randomHex } from "./auth.ts";
import {
  ACCESS_NAME_ATTEMPTS,
  ACCESS_SUFFIX_BYTES,
  ACCESS_TOKEN_BYTES,
  accessEmailKeyFor,
  accessUserRecord,
  b64urlToJson,
  claimsOk,
  emailOf,
  isAdminEmail,
  isDisabledAccount,
  normalizeEmail,
  splitJwt,
  tokenKeyFor,
  userKeyFor,
  usernameFromEmail,
} from "./access-logic.ts";

const CERTS_TTL_MS = 60 * 60 * 1000;
let jwksCache: { keys: any[]; exp: number } | null = null;

async function fetchJwks(env: Env): Promise<any[]> {
  if (env.ACCESS_JWKS_JSON) return JSON.parse(env.ACCESS_JWKS_JSON).keys;
  if (jwksCache && Date.now() < jwksCache.exp) return jwksCache.keys;
  const res = await fetch(`https://${env.ACCESS_TEAM_DOMAIN}/cdn-cgi/access/certs`, {
    cf: { cacheTtl: 3600 },
  } as any);
  if (!res.ok) throw new Error(`certs ${res.status}`);
  const body: any = await res.json();
  jwksCache = { keys: body.keys, exp: Date.now() + CERTS_TTL_MS };
  return body.keys;
}

/** Verify the Cf-Access JWT; returns the verified email, or null. */
export async function verifyAccessJwt(
  request: Request,
  env: Env,
): Promise<{ email: string } | null> {
  const token = request.headers.get("cf-access-jwt-assertion");
  if (!token || !env.ACCESS_AUD || !env.ACCESS_TEAM_DOMAIN) return null;
  const parts = splitJwt(token);
  if (!parts) return null;

  let header: any;
  let payload: any;
  try {
    header = b64urlToJson(parts[0]!);
    payload = b64urlToJson(parts[1]!);
  } catch {
    return null;
  }
  // The four checks — `kid`, the audience, the PINNED issuer (audit L5) and a numeric `exp` — are the
  // emission's, in the source's order.
  if (
    !claimsOk(header, payload, String(env.ACCESS_AUD), String(env.ACCESS_TEAM_DOMAIN), Date.now())
  ) {
    return null;
  }

  let keyJwk: any;
  try {
    keyJwk = (await fetchJwks(env)).find((k) => k.kid === header.kid);
  } catch {
    return null;
  }
  if (!keyJwk || keyJwk.alg !== "RS256") return null;

  const key = await crypto.subtle.importKey(
    "jwk",
    keyJwk,
    { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
    false,
    ["verify"],
  );
  // base64url → base64 with the right padding (length may be %4 = 2 or 3;
  // hardcoding "==" mis-decodes signatures whose encoded length is %4 = 3).
  // audit L5: atob THROWS on garbage signature characters and subtle.verify
  // rejects malformed key/sig input — every cookie-less request carrying a
  // junk cf-access-jwt 500'd instead of 401. Any throw = invalid token.
  let ok: boolean;
  try {
    const sigB64 = parts[2]!.replace(/-/g, "+").replace(/_/g, "/");
    const sigPad = sigB64.length % 4 === 0 ? "" : "=".repeat(4 - (sigB64.length % 4));
    const sig = Uint8Array.from(atob(sigB64 + sigPad), (c) => c.charCodeAt(0));
    const data = new TextEncoder().encode(`${parts[0]}.${parts[1]}`);
    ok = await crypto.subtle.verify("RSASSA-PKCS1-v1_5", key, sig, data);
  } catch {
    return null;
  }
  if (!ok) return null;

  const email = emailOf(payload);
  return email ? { email } : null;
}

/**
 * Map a verified Access email to a console user — bind, reuse or provision.
 * Returns null only when KV is unavailable.
 */
export async function ensureUserByEmail(env: Env, email: string): Promise<User | null> {
  if (!env.KEYS) return null;
  email = normalizeEmail(email);

  // Owner shortcut: the configured admin email drives the seeded admin account.
  if (isAdminEmail(email, env.ACCESS_ADMIN_EMAIL)) {
    const admin = await env.KEYS.get(userKeyFor("admin"));
    if (admin) {
      const u = typeof admin === "string" ? JSON.parse(admin) : admin;
      if (!isDisabledAccount(u)) return u;
    }
    return null;
  }

  // Previously bound?
  const bound = await env.KEYS.get(accessEmailKeyFor(email));
  if (bound) {
    const u = await env.KEYS.get(userKeyFor(bound));
    if (u) {
      const user = typeof u === "string" ? JSON.parse(u) : u;
      // round-395: a DISABLED bound account must stay logged out — the old
      // code fell through to the provision block below, whose re-check
      // returned the record WITHOUT an enabled check (the cookie path in
      // session.ts rejects disabled users, so Access users could never be
      // suspended). Return null WITHOUT provisioning: minting a fresh
      // suffixed account for a suspended user would defeat the suspension.
      if (isDisabledAccount(user)) return null;
      return user;
    }
  }

  // Provision a passwordless user. Serialized like createUser (round-107).
  return withKeyLock(`access-provision:${email}`, async () => {
    const again = await env.KEYS.get(accessEmailKeyFor(email));
    if (again) {
      const u = await env.KEYS.get(userKeyFor(again));
      if (u) {
        const user = typeof u === "string" ? JSON.parse(u) : u;
        if (isDisabledAccount(user)) return null;
        return user;
      }
    }

    const base = usernameFromEmail(email);
    let name = base;
    // Uniqueness loop: a single 4-hex suffix attempt could still collide and
    // silently OVERWRITE the colliding account's record (the old code suffixed
    // once, then put unconditionally). Retry with a fresh random suffix until
    // the name is free; bounded so a pathological KV state can't loop forever.
    if (await env.KEYS.get(userKeyFor(name))) {
      let unique = false;
      for (let i = 0; i < ACCESS_NAME_ATTEMPTS; i++) {
        const candidate = `${base}-${randomHex(ACCESS_SUFFIX_BYTES)}`; // 4 hex chars — collision suffix
        if (!(await env.KEYS.get(userKeyFor(candidate)))) {
          name = candidate;
          unique = true;
          break;
        }
      }
      if (!unique) throw new Error("could not allocate a unique username — try again");
    }

    const token = randomHex(ACCESS_TOKEN_BYTES);
    const user = accessUserRecord(name, email, token, Date.now()) as User & {
      accessEmail?: string;
    };
    await env.KEYS.put(userKeyFor(name), JSON.stringify(user));
    await env.KEYS.put(tokenKeyFor(token), name);
    await env.KEYS.put(accessEmailKeyFor(email), name);
    return user;
  });
}

/**
 * Fallback identity resolution for requireSession: cookie session failed,
 * so trust the edge-verified Access identity when this deployment opts in.
 */
export async function requireAccessSession(request: Request, env: Env): Promise<User | null> {
  if (!env.ACCESS_AUD || !env.ACCESS_TEAM_DOMAIN) return null;
  const claims = await verifyAccessJwt(request, env);
  if (!claims) return null;
  return ensureUserByEmail(env, claims.email);
}
