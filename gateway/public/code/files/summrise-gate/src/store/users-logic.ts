/**
 * users-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/user_store.rs` (`render_typescript`), which is now the single source for the
 * decisions below. The freshness test in that module is a byte comparison against this file, so a hand edit
 * fails there with the text and one command to fix it:
 *
 *     SUMMRISE_REFRESH_USERS=1 cargo test --manifest-path gateway/wasm/Cargo.toml user_store::emission
 *
 * `gateway/src/store/users.ts` re-exports this module and keeps the HOST: the KV reads and writes, the keyed
 * locks, the record reads, the invites write and the RouteDO calls. **THE DECISIONS ARE HERE** — who the
 * seeded admin is, which key names a user may hold, how long a token is, how many bytes a salt and an invite
 * code take, and the mask and three-state "which credential is in force" answers the console renders.
 */

import { BYOK_CHANNELS } from "./byok.ts";

/** The seeded account's name. `ADMIN_ID` is the same string: **user ID = username**, which is what makes the
 *  KV keys readable (`user:admin`). */
export const ADMIN_USERNAME = "admin";

/** The seeded account's id, and the id whose login goes through the ADMIN password rather than the record's own
 *  hash. */
export const ADMIN_ID = "admin";

/** Every env-var name a USER may hold a key for, DERIVED from the emitted BYOK table since round 193 — this was
 *  the third of four independently-typed copies of the same names. */
export const USER_KEY_NAMES: string[] = BYOK_CHANNELS.map((c) => c.userKey);

/** `generateGatewayToken()`'s `randomHex(24)` — bytes, so 48 hex characters. */
export const GATEWAY_TOKEN_BYTES = 24;

/** `createUser`'s per-user salt — the source's `randomHex(16)`. */
export const PASSWORD_SALT_BYTES = 16;

/** `createInvite`'s code — `randomHex(5).toUpperCase()`, so ten hex characters. */
export const INVITE_CODE_BYTES = 5;

/** `createInvite`'s TTL: *"7-day TTL: an invite shared in chat must not stay valid forever."* */
export const INVITE_TTL_SECS = 7 * 24 * 60 * 60;

/** `maskKey(v)` — the console's only view of a device token or a user's key. **CODE-UNIT-WISE, because that is
 *  what `v.length` and `v.slice` count**: an astral character is two units, so `"abcd😀"` takes the SHORT arm
 *  on both sides. (`v[0]` on an empty string is `undefined`, which is why the `!v` arm comes first.) */
export function maskKey(v: string): string {
  if (!v) return "not configured";
  if (v.length <= 6) return v[0] + "…" + v.slice(-2);
  return v.slice(0, 3) + "…" + v.slice(-4);
}

/** Per-key configured/masked status for the console (shared by the auth plugin's /api/me and the admin plugin's
 *  user list — was copy-pasted in both with subtly different types).
 *
 *  AND WHICH CREDENTIAL IS IN FORCE: `configured` answers "has this user stored this key"; it does NOT answer
 *  "will a request on this channel be served, and on whose credential" — and since the request path falls back
 *  to the channel's Worker secret (`bearerKeyFor`), those are different questions.
 *
 *      user        this user's own key is set — it wins, the deployment is not charged
 *      deployment  no user key, but the channel declares an envKey and the worker has it
 *      none        neither — including nv/gmi, whose envKey is null BY DESIGN, so a stray secret of the same
 *                  name is not a fallback here
 *
 *  Pure: `env` is passed in, never read from a global, so the three states are directly testable and the
 *  console cannot invent a fourth. */
export function userKeysStatus(
  ukeys: Record<string, any>,
  env: Record<string, any> = {},
): Record<string, { configured: boolean; masked: string; source: "user" | "deployment" | "none" }> {
  const out: Record<
    string,
    { configured: boolean; masked: string; source: "user" | "deployment" | "none" }
  > = {};
  for (const n of USER_KEY_NAMES) {
    const v = ukeys?.[n];
    const channel = BYOK_CHANNELS.find((c) => c.userKey === n);
    const envKey = channel?.envKey ?? null;
    const source: "user" | "deployment" | "none" = v
      ? "user"
      : envKey && env[envKey]
        ? "deployment"
        : "none";
    out[n] = { configured: !!v, masked: maskKey(v || ""), source };
  }
  return out;
}
