/**
 * access-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/access.rs` (`render_typescript`), which is now the single source for the rules
 * below. The freshness test in that module is a byte comparison against this file, so a hand edit fails there
 * with the text and one command to fix it:
 *
 *     SUMMRISE_REFRESH_ACCESS=1 cargo test --manifest-path gateway/wasm/Cargo.toml access::emission
 *
 * `gateway/src/access.ts` re-exports this module and keeps the HOST: the JWKS fetch and its cache, the
 * `crypto.subtle` verify, the KV reads and writes, and the provisioning lock. **THE DECISIONS ARE HERE** — the
 * JWT's split and its four claim checks, the email and username derivations, the two key-name families, and the
 * record a provisioned account is written as.
 *
 * **AND IT IMPORTS THE GATEWAY TOKEN SIZE RATHER THAN RESTATING IT**: the account this file provisions is a user
 * like any other, and a second `24` would be a second answer to "how long is a gateway token".
 */

import { GATEWAY_TOKEN_BYTES } from "./store/users-logic.ts";

/** `access-email:<email>` — the binding from a verified Access identity to a console user. */
export const ACCESS_EMAIL_KEY = "access-email:";

/** `user:admin` — the seeded account the owner shortcut reads. */
export const ADMIN_USER_KEY = "user:admin";

/** The uniqueness suffix: `randomHex(2)`, so four hex characters. */
export const ACCESS_SUFFIX_BYTES = 2;

/** The provision loop's bound — the source's own `for (let i = 0; i < 10; i++)`. */
export const ACCESS_NAME_ATTEMPTS = 10;

/** The token a provisioned account is minted with. */
export const ACCESS_TOKEN_BYTES = GATEWAY_TOKEN_BYTES;

/** `user:<id>` — the record key. Named, because the two languages write and read it. */
export function userKeyFor(id: string): string {
  return `user:${id}`;
}

/** `token:<token>` — the lookup key. */
export function tokenKeyFor(token: string): string {
  return `token:${token}`;
}

/** `access-email:<email>` — the binding key. */
export function accessEmailKeyFor(email: string): string {
  return `${ACCESS_EMAIL_KEY}${email}`;
}

/** `String(v || "").toLowerCase().trim()` — the normalization both the claim and the record path use. */
export function normalizeEmail(value: unknown): string {
  return String(value || "")
    .toLowerCase()
    .trim();
}

/** The owner shortcut's test: a CONFIGURED admin email that matches exactly. An empty `ACCESS_ADMIN_EMAIL` is
 *  not a match for anything, which is what `adminEmail &&` means before the comparison. */
export function isAdminEmail(email: string, adminEmail: unknown): boolean {
  const configured = String(adminEmail || "").toLowerCase();
  return !!configured && email === configured;
}

/** `user.enabled === false` — **AN EXPLICIT `false` DISABLES, AND A MISSING FIELD DOES NOT** (round-395: the
 *  bound-account path must return null for a suspended user WITHOUT provisioning a fresh one, and the owner
 *  shortcut answers a record whose `enabled` is merely absent). */
export function isDisabledAccount(user: unknown): boolean {
  return (user as { enabled?: unknown } | null | undefined)?.enabled === false;
}

/** `b64urlToJson(part)` — the padding restoration, the URL alphabet, `atob`, `JSON.parse`. It THROWS where the
 *  source throws (`atob` on a bad alphabet, `JSON.parse` on bad text), which is what its caller's `try` catches. */
export function b64urlToJson(part: string): unknown {
  const pad = part.length % 4 === 0 ? "" : "=".repeat(4 - (part.length % 4));
  return JSON.parse(atob(part.replace(/-/g, "+").replace(/_/g, "/") + pad));
}

/** `token.split(".")` with the source's `parts.length !== 3` refusal — THREE parts exactly, and the third is the
 *  signature. (This is NOT `auth.ts`'s first-dot split: a session token has one dot, an Access JWT has two.) */
export function splitJwt(token: string): [string, string, string] | null {
  const parts = token.split(".");
  return parts.length === 3 ? [parts[0]!, parts[1]!, parts[2]!] : null;
}

/** `Array.isArray(payload.aud) ? payload.aud : typeof payload.aud === "string" ? [payload.aud] : []` — the two
 *  shapes Cloudflare emits. **ONLY STRING ENTRIES CAN MATCH**, which is what `includes` does with them. */
export function audList(payload: unknown): string[] {
  const aud = (payload as { aud?: unknown } | null | undefined)?.aud;
  return Array.isArray(aud)
    ? aud.filter((a): a is string => typeof a === "string")
    : typeof aud === "string"
      ? [aud]
      : [];
}

/** **THE FOUR CLAIMS CHECKS, IN THE SOURCE'S ORDER, EACH ONE A REFUSAL THAT NEVER REACHES THE NETWORK.** The
 *  `kid` lookup is by name, the audience must list this deployment, the issuer is PINNED to the team (audit L5:
 *  a token from ANOTHER Cloudflare team under the same `aud` was possible), and `exp` must be a NUMBER that has
 *  not passed. */
export function claimsOk(
  header: unknown,
  payload: unknown,
  aud: string,
  teamDomain: string,
  nowMs: number,
): boolean {
  const kid = (header as { kid?: unknown } | null | undefined)?.kid;
  if (typeof kid !== "string" || !kid) return false;
  if (!audList(payload).includes(aud)) return false;
  if ((payload as { iss?: unknown } | null | undefined)?.iss !== `https://${teamDomain}`)
    return false;
  const exp = (payload as { exp?: unknown } | null | undefined)?.exp;
  return typeof exp === "number" && exp * 1000 >= nowMs;
}

/** `String(payload.email || "").toLowerCase().trim()`, then the `@` test — null where the source answers null. */
export function emailOf(payload: unknown): string | null {
  const email = normalizeEmail((payload as { email?: unknown } | null | undefined)?.email);
  return email.includes("@") ? email : null;
}

/** `usernameFromEmail(email)`: the local part, everything outside `[A-Za-z0-9_.-]` dropped, 28 characters at
 *  most — and `"user"` when what is left is not a 2-to-32 character name. **THE ORDER IS THE SOURCE'S**: the
 *  strip is BEFORE the slice and the test is AFTER it, so a name that is legal before stripping and illegal
 *  after becomes `"user"` rather than being kept. */
export function usernameFromEmail(email: string): string {
  let base = email
    .split("@")[0]!
    .replace(/[^A-Za-z0-9_.-]/g, "")
    .slice(0, 28);
  if (!/^[A-Za-z0-9_.-]{2,32}$/.test(base)) base = "user";
  return base;
}

/** The record a provisioned account is written as. **FIELD ORDER IS THE STORED BYTES**:
 *  `id, username, role, enabled, createdAt, token, accessEmail`. */
export function accessUserRecord(
  name: string,
  email: string,
  token: string,
  nowMs: number,
): unknown {
  return {
    id: name,
    username: name,
    role: "user",
    enabled: true,
    createdAt: nowMs,
    token,
    accessEmail: email,
  };
}
