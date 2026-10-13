/**
 * model-route-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/auto_route.rs` (`render_typescript`), which is now the single source. The freshness
 * test in that module is a byte comparison against this file, so a hand edit fails there with the text and one
 * command to fix it:
 *
 *     SUMMRISE_REFRESH_AUTO_ROUTE=1 cargo test --manifest-path gateway/wasm/Cargo.toml auto_route::emission
 *
 * `gateway/src/model-route.ts` re-exports this module and keeps the I/O these decisions are made FROM — the
 * catalogue (`isAdvertised`), the user's key blob, the stored route and the breaker. **`CHANNEL_KEY_RULES` stays
 * over there, mutable, because `registerChannelKey` is an extension point**, which is why `channelKeyOk` takes the
 * rule as an argument instead of looking one up.
 *
 * The comments below are the source file's own, carried verbatim: they are the record of why the default and the
 * ladder are what they are, and none of it is recoverable from the rows.
 */

/** Model usable for routing? In the whitelist, (og) breaker not open, AND the REQUESTING user's key for that channel
 *  is configured — a channel without a key 502s every request, so model=auto must not route to it.
 *  round-68: the old code checked ADMIN_ID's keys — a BYOK user with only an og key was told og was "unusable" (the
 *  admin lacks it) and routed to ds, which the user lacks, so every model=auto request 502'd. */

// SOLID Round-26 (OCP): per-channel key rules are DATA — adding a channel registers one entry instead of appending
// another if-line (nv/gmi/cm/amd each arrived as a new line). userKey is always required; envKey null means pure
// BYOK (the /v1 handler reads ONLY the user's key blob there).
export interface ChannelKeyRule {
  userKey: string;
  envKey: string | null;
}

/** `isModelUsable`'s KEY half. **`envKey` FALSY IS NOT A FALLBACK**: nv and gmi carry `null` (pure BYOK) and a
 *  record carrying an EMPTY name behaves the same way, because the source tests the name for truthiness. */
export function channelKeyOk(
  rule: ChannelKeyRule | null,
  hasUser: boolean,
  hasEnv: boolean,
): boolean {
  if (!rule) return true;
  return hasUser || (rule.envKey ? hasEnv : false);
}

/** `isModelUsable`'s channel half: the key rule first, then **`og`'s breaker and NOTHING else**. The order is the
 *  meaning — a keyless `og` is refused for the KEY, so the breaker is never read for it. */
export function channelUsable(
  model: string,
  rule: ChannelKeyRule | null,
  hasUser: boolean,
  hasEnv: boolean,
  degraded: boolean,
): boolean {
  if (!channelKeyOk(rule, hasUser, hasEnv)) return false;
  if (model.startsWith("og/")) return !degraded;
  return true;
}

// Default channel when the user hasn't made a selection. 2026-09-10 (V4 retirement): this used to be
// ds/deepseek-v4-flash — the official key is now out of balance (402 on every request) and its V4 names are
// retired, so the default moved to Command Code's V4.1 Flash, the model the whole catalog standardised on (and the
// one the console's health badge recommends).
export const DEFAULT_ROUTE_MODEL = "cm/deepseek/deepseek-v4.1-flash";

/** The fallback ladder for `auto`, in PRIORITY ORDER.
 *
 *  The ORDER here is the meaning — the default channel first, then alternatives — so this is NOT derived from the
 *  catalogue the way ROUTE_INFO's per-route lists are. What it must be is a SUBSET of the catalogue: a ladder entry
 *  that is not advertised (a typo, or a model removed from the catalogue) would be a fallback that can never
 *  satisfy `isModelUsable`, and the ladder would skip it in silence. `cors.rs`'s neighbour in this crate,
 *  `auto_route.rs`, is where that sentence is a TEST (`the_ladder_is_a_subset_of_the_catalogue`). */
export const AUTO_FALLBACK_LADDER: string[] = [
  DEFAULT_ROUTE_MODEL,
  "qw/qwen3.8-max-preview",
  "qw/qwen3.8-flash",
  "og/deepseek-v4.1-flash",
  "or/openai/gpt-5.6-luna:floor[1m]",
];

/** `resolveAutoModel`'s POLICY, with the I/O handed in: the caller's stored choice when it is usable, else the first
 *  usable rung of the ladder IN ORDER, else the default. `isUsable` is the caller's async predicate — the reads
 *  (the catalogue, the key blob, the breaker) are `model-route.ts`'s, and this is what decides with them. */
export async function pickAutoRoute(
  chosen: string | null | undefined,
  isUsable: (model: string) => Promise<boolean>,
): Promise<string> {
  if (chosen && (await isUsable(chosen))) return chosen;
  for (const model of AUTO_FALLBACK_LADDER) {
    if (await isUsable(model)) return model;
  }
  return DEFAULT_ROUTE_MODEL;
}
