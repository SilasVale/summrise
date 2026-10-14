/**
 * admin-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/user_store.rs` (`render_admin_typescript`), which is the single source for the
 * rules below. The freshness test in that module is a byte comparison against this file:
 *
 *     SUMMRISE_REFRESH_ADMIN=1 cargo test --manifest-path gateway/wasm/Cargo.toml user_store::emission
 *
 * `gateway/src/store/admin.ts` re-exports this module and keeps the HOST: the `seeded` process-once flag, the
 * `__resetSeedForTests` hook, the Worker's own `crypto.subtle` PBKDF2, and the KV calls the seed is handed.
 *
 * **THREE MIGRATIONS LIVE IN THE SEED, AND EACH ONE WAS ONCE A LOCKOUT** — the v1 -> v2 schema, the empty-token
 * backfill, and round-394's "mint only when no record exists". Their ORDER is why this file exists.
 */

import { ADMIN_ID, ADMIN_USERNAME, USER_KEY_NAMES } from "./users-logic.ts";

/** `auth:admin_password` — where the admin's `salt:hash` lives. **KV IS AUTHORITATIVE.** */
export const ADMIN_PASSWORD_KEY = "auth:admin_password";

/** `_admin_seeded` — the marker a completed seed writes, and the early return for every later boot. */
export const ADMIN_SEEDED_KEY = "_admin_seeded";

/** `CLIENT_KEY` — the legacy gateway token a deployer may have set as a Worker secret. The seed uses it instead
 *  of minting, so an existing deployment's scripts keep working. */
export const CLIENT_KEY = "CLIENT_KEY";

/** The v1 -> v2 schema's three addresses: the random-id admin record, its key blob, and the old username index. */
export const V1_ADMIN_USER_KEY = "user:u-admin";
export const V1_ADMIN_UKEYS_KEY = "ukeys:u-admin";
export const V1_USERNAME_KEY = "username:admin";

/** `u-admin` — the id `token:<t>` held before the migration to "username as KV key". */
export const V1_ADMIN_ID = "u-admin";

/** **`legacy` IS THE SALT EVERY MIGRATED HASH USES**, and it is why a bare hash was a lockout: a stored hash
 *  without its salt made verification fail forever. */
export const LEGACY_SALT = "legacy";

/** `setAdminPassword`'s fresh salt — the source's `randomHex(8)`. */
export const ADMIN_PASSWORD_SALT_BYTES = 8;

/** `${salt}:${hash}` — the one stored format, used by the migration, the seed's writes and every verification. */
export function adminPasswordRecord(salt: string, hash: string): string {
  return `${salt}:${hash}`;
}

/** The two halves of a stored password, **SPLIT ON THE FIRST COLON EXACTLY AS `String.prototype.split(":")`
 *  DESTRUCTURING TAKES THE FIRST TWO PARTS** — a hash containing a colon keeps the rest in the second half, and
 *  a record with no colon at all answers `(whole, "")`. */
export function adminPasswordParts(stored: string): { salt: string; hash: string } {
  const [salt, hash] = stored.split(":");
  return { salt: salt ?? "", hash: hash ?? "" };
}

/** The record the FRESH seed writes. **FIELD ORDER IS THE STORED BYTES**:
 *  `id, username, role, enabled, createdAt, token` — and `role` is `"admin"`, the only difference from an
 *  Access-provisioned account. */
export function adminRecord(name: string, token: string, nowMs: number): unknown {
  return { id: name, username: name, role: "admin", enabled: true, createdAt: nowMs, token };
}

/** The empty-token backfill's test: **A RECORD THAT EXISTS WITH A FALSY `token`**. A deployment seeded before
 *  the mint fix holds `token: ""` with `_admin_seeded` set, so the early returns would lock it out forever. */
export function backfillNeeded(existing: unknown): boolean {
  return !!existing && !(existing as { token?: unknown }).token;
}

/** The platform the seed runs on. Four KV calls, the cache, the mint, the clock, and the Worker's secrets. */
export interface AdminSeedDeps {
  get(key: string): Promise<string | null>;
  put(key: string, value: string): Promise<void>;
  delete(key: string): Promise<void>;
  /** `cset` — the per-isolate cache. It is handed the PARSED object for a key blob, which is what the source
   *  cached (`cset(key, parsed || {})`). */
  cache(key: string, value: unknown): void;
  /** `generateGatewayToken()`. */
  mint(): string;
  /** `Date.now()`. */
  now(): number;
  /** `env[name]` — the Worker secret a channel key may come from. */
  secret(name: string): unknown;
}

/** **`seedAdmin`'s SEQUENCE, AND ITS ORDER IS THE PRODUCT.** */
export async function seedAdminSequence(deps: AdminSeedDeps): Promise<void> {
  // 1. THE v1 -> v2 MIGRATION. Inside a `try` because **a failed migration must not block startup**.
  const oldRaw = await deps.get(V1_ADMIN_USER_KEY);
  if (oldRaw) {
    try {
      const admin = JSON.parse(oldRaw);
      admin.id = ADMIN_ID;
      await deps.put(`user:${ADMIN_ID}`, JSON.stringify(admin));
      deps.cache(`user:${ADMIN_ID}`, admin);
      await deps.delete(V1_ADMIN_USER_KEY);
      // The token mapping is remapped only when it still points at the v1 id.
      if (admin.token && (await deps.get(`token:${admin.token}`)) === V1_ADMIN_ID) {
        await deps.put(`token:${admin.token}`, ADMIN_ID);
        deps.cache(`token:${admin.token}`, ADMIN_ID);
      }
      const uks = await deps.get(V1_ADMIN_UKEYS_KEY);
      if (uks) {
        await deps.put(`ukeys:${ADMIN_ID}`, uks);
        let parsed = null;
        try {
          parsed = JSON.parse(uks);
        } catch {
          /* malformed JSON */
        }
        deps.cache(`ukeys:${ADMIN_ID}`, parsed || {});
        await deps.delete(V1_ADMIN_UKEYS_KEY);
      }
      await deps.delete(V1_USERNAME_KEY);
    } catch {
      /* a failed migration must not block startup */
    }
  }

  // `String(... || "")` IS the source's `env.CLIENT_KEY || ""` with the type the deps object carries: a secret
  // that is absent, null or falsy contributes nothing, and a non-string one is rendered as itself.
  const legacyToken = (await deps.get(CLIENT_KEY)) || String(deps.secret(CLIENT_KEY) || "");

  // 2. THE EMPTY-TOKEN BACKFILL — heal exactly like fresh: mint and remap. A record with a NON-EMPTY token is
  // left completely untouched.
  try {
    const existingRaw = await deps.get(`user:${ADMIN_ID}`);
    if (existingRaw) {
      const existing = JSON.parse(existingRaw);
      if (backfillNeeded(existing)) {
        const adminToken = legacyToken || deps.mint();
        existing.token = adminToken;
        await deps.put(`user:${ADMIN_ID}`, JSON.stringify(existing));
        deps.cache(`user:${ADMIN_ID}`, existing);
        await deps.put(`token:${adminToken}`, ADMIN_ID);
        deps.cache(`token:${adminToken}`, ADMIN_ID);
        await deps.put(ADMIN_SEEDED_KEY, "1");
      }
    }
  } catch {
    /* a failed backfill must not block startup */
  }

  // 3. THE FRESH SEED, and the two early returns that make an upgrade a no-op. **round-394: mint ONLY when no
  // `user:admin` record exists** — the migration IS the seed for a v1 upgrade.
  if (await deps.get(ADMIN_SEEDED_KEY)) return;
  if (await deps.get(`user:${ADMIN_ID}`)) return;
  const adminToken = legacyToken || deps.mint();
  const admin = adminRecord(ADMIN_USERNAME, adminToken, deps.now());
  await deps.put(`user:${ADMIN_ID}`, JSON.stringify(admin));
  deps.cache(`user:${ADMIN_ID}`, admin);
  await deps.put(`token:${adminToken}`, ADMIN_ID);
  deps.cache(`token:${adminToken}`, ADMIN_ID);

  // The key blob: a Worker secret per name, whichever the deployer set.
  const ukeys: Record<string, string> = {};
  for (const n of USER_KEY_NAMES) {
    const v = (await deps.get(n)) || String(deps.secret(n) || "");
    if (v) ukeys[n] = v;
  }
  await deps.put(`ukeys:${ADMIN_ID}`, JSON.stringify(ukeys));
  deps.cache(`ukeys:${ADMIN_ID}`, ukeys);
  await deps.put(ADMIN_SEEDED_KEY, "1");
}

/** The platform `getAdminPassword` runs on. */
export interface AdminPasswordDeps {
  /** `cget(key)` — the per-isolate cache, which answers `undefined` for a miss. */
  cached(key: string): unknown;
  hasKv: boolean;
  /** `env.ADMIN_PASSWORD` — the Worker secret a fresh deployment may carry. */
  secretValue: unknown;
  get(key: string): Promise<string | null>;
  put(key: string, value: string): Promise<void>;
  cache(key: string, value: string): void;
  /** `hashPassword(password, LEGACY_SALT)`. */
  hash(password: string): Promise<string>;
}

/** `getAdminPassword(env)` — **KV IS AUTHORITATIVE AND THE WORKER SECRET IS MIGRATED ONCE**, hashed in the same
 *  `salt:hash` format verification expects. **A CACHED VALUE WINS EVEN WHEN IT IS THE EMPTY STRING**, and the
 *  no-KV arm returns WITHOUT caching, because there is no store to be consistent with. */
export async function resolveAdminPassword(deps: AdminPasswordDeps): Promise<string> {
  const hit = deps.cached(ADMIN_PASSWORD_KEY);
  if (hit !== undefined) return String(hit);
  const secret = deps.secretValue ? String(deps.secretValue) : "";
  const legacy = async () => adminPasswordRecord(LEGACY_SALT, await deps.hash(secret));
  if (!deps.hasKv) return secret ? await legacy() : "";
  let v = (await deps.get(ADMIN_PASSWORD_KEY)) || "";
  if (!v && secret) {
    // one-time migration from the Worker secret — stored HASHED, because a bare hash (no colon) made
    // verification permanently fail: admin locked out of the console.
    v = await legacy();
    await deps.put(ADMIN_PASSWORD_KEY, v);
  }
  deps.cache(ADMIN_PASSWORD_KEY, v);
  return v;
}
