/**
 * store/admin.ts — admin seeding (process-once) + the hashed admin password.
 *
 * The `seeded` module flag and __resetSeedForTests live HERE and only here:
 * seedAdmin must seed once per process (test/security-fixes.test.mjs's header
 * relies on that semantics), so this module must never be duplicated.
 *
 * **THE DECISIONS ARE AN EMISSION NOW (2026-10-14)** — `admin-logic.ts`, GENERATED from
 * `gateway/wasm/src/user_store.rs`. What moved: the v1 schema's three addresses and its id, the `salt:hash`
 * record's format and its first-colon split, the fresh admin record's shape, the empty-token backfill's test,
 * **the SEED'S ORDER** (the v1 migration, then the backfill, then round-394's two early returns), and the
 * KV-authoritative password resolution with its one-time `legacy:` migration.
 *
 * What is left is the HOST: the process-once flag, the test hook, this Worker's KV and cache, the mint, and
 * `crypto.subtle`'s PBKDF2 through `auth.ts`.
 */

import { hashPassword, randomHex, verifyPassword } from "../auth.ts";
import { cget, cset, type Env } from "./cache.ts";
import { generateGatewayToken } from "./users.ts";
import {
  ADMIN_PASSWORD_KEY,
  ADMIN_PASSWORD_SALT_BYTES,
  LEGACY_SALT,
  adminPasswordParts,
  adminPasswordRecord,
  resolveAdminPassword,
  seedAdminSequence,
} from "./admin-logic.ts";

// The rules are re-exported for the callers and tests that want them by name — the admin routes read the key
// names, and `auth.ts`'s password tests want `adminPasswordParts`.
export {
  ADMIN_PASSWORD_KEY,
  ADMIN_PASSWORD_SALT_BYTES,
  ADMIN_SEEDED_KEY,
  CLIENT_KEY,
  LEGACY_SALT,
  V1_ADMIN_ID,
  V1_ADMIN_USER_KEY,
  V1_ADMIN_UKEYS_KEY,
  V1_USERNAME_KEY,
  adminPasswordParts,
  adminPasswordRecord,
  adminRecord,
  backfillNeeded,
} from "./admin-logic.ts";

/* ---- Admin seeding ---- */
let seeded = false;
/** Test hook: reset the process-once seed flag so a second seedAdmin call in
 *  the same process really runs (the backfill test heals one env, then must
 *  prove a healthy env is untouched). Never called in production. */
export function __resetSeedForTests(): void {
  seeded = false;
}
export async function seedAdmin(env: Env): Promise<void> {
  if (!env.KEYS || seeded) return;
  seeded = true;
  // **THE ORDER IS THE EMISSION'S** — three migrations, each one a lockout that was fixed by hand once. The host
  // hands over the four calls it alone can make.
  await seedAdminSequence({
    get: (key) => env.KEYS.get(key),
    put: (key, value) => env.KEYS.put(key, value),
    delete: (key) => env.KEYS.delete(key),
    cache: (key, value) => cset(key, value),
    mint: () => generateGatewayToken(),
    now: () => Date.now(),
    secret: (name) => env[name],
  });
}

/* ---- Admin password (stored hashed — never plaintext) ----
 *
 * Previously stored plaintext in KV and returned verbatim by
 * GET /api/admin/password. Now: PBKDF2 (same scheme as user passwords via
 * auth.js); getAdminPassword returns the stored hash, hasAdminPassword
 * tells the console whether one is set, verifyAdminPassword compares a
 * candidate. No raw value is ever exposed.
 */

/** Read the admin password hash: KV is authoritative; migrate from the Worker secret once if absent */
export async function getAdminPassword(env: Env): Promise<string> {
  return resolveAdminPassword({
    cached: (key) => cget(key),
    hasKv: !!env.KEYS,
    secretValue: env.ADMIN_PASSWORD,
    get: (key) => env.KEYS.get(key),
    put: (key, value) => env.KEYS.put(key, value),
    cache: (key, value) => cset(key, value),
    hash: (password) => hashPassword(password, LEGACY_SALT),
  });
}

export async function hasAdminPassword(env: Env): Promise<boolean> {
  return !!(await getAdminPassword(env));
}

/** Compare a candidate password against the stored hash. */
export async function verifyAdminPassword(env: Env, candidate: string): Promise<boolean> {
  const stored = await getAdminPassword(env);
  if (!stored) return false;
  // Stored as `salt:hash` — reuse the same PBKDF2 verify as user accounts.
  const { salt, hash } = adminPasswordParts(stored);
  return verifyPassword(candidate || "", salt, hash);
}

export async function setAdminPassword(env: Env, value: string): Promise<void> {
  if (!env.KEYS) throw new Error("KV not bound");
  const salt = randomHex(ADMIN_PASSWORD_SALT_BYTES);
  const v = adminPasswordRecord(salt, await hashPassword(String(value), salt));
  await env.KEYS.put(ADMIN_PASSWORD_KEY, v);
  cset(ADMIN_PASSWORD_KEY, v);
}
