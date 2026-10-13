/**
 * model-route — per-user auto-model resolution.
 *
 * **ITS DECISIONS ARE EMITTED FROM RUST (2026-10-13).** The key rule, `og`'s breaker leg, the default, the ladder
 * and the fallback POLICY are `gateway/wasm/src/auto_route.rs`, and they reach this file as `model-route-logic.ts`
 * (GENERATED, freshness = a byte comparison), re-exported below so every existing import keeps working. What is left
 * here is the I/O those decisions are made FROM — `isAdvertised` (the catalogue), the user's key blob, the stored
 * route, the breaker — plus the derivation and the extension point, which are structure rather than decisions.
 *
 * WHY THE EMISSION RATHER THAN A PORT OR A DELETION: `GET|PUT /api/me/route` stays on the TypeScript path
 * (`auth_routes.rs`: the handler resolves through the CATALOGUE and the `RouteDO`, "a different surface with its own
 * corpus"), and `npm test` cannot reach `/__decision` — so a twin deleted outright would delete the only
 * implementation the tests can reach.
 *
 * **IT MOVED FROM `plugins/` TO `src/` AT THE CUTOVER (2026-10-07), BECAUSE ITS CONSUMER COUNT CHANGED.** The
 * contract in `plugins/registry.ts` says a module with two live consumers is foundation rather than a private
 * collaborator: `index.ts` re-exports `resolveAutoModel`/`isModelUsable` and `plugins/auth.ts` imports
 * `resolveAutoModel` directly for `GET /api/me/route`, where it used to read it out of the deleted translate
 * plugin's context.
 */

import { isAdvertised } from "./store/models.ts";
import { getUserKeys, getUserRoute } from "./store.ts";
import { isChannelDegraded } from "./reliability.ts";
import { BYOK_CHANNELS } from "./store/byok.ts";
import { channelUsable, pickAutoRoute, type ChannelKeyRule } from "./model-route-logic.ts";

// The emitted decisions, re-exported so `from "./model-route.ts"` keeps working for every caller and for
// `index.ts`'s public surface.
export * from "./model-route-logic.ts";

/** Per-channel BYOK rules, keyed by routing PREFIX — DERIVED from `store/byok.ts` since round 195. This was the last
 *  of FOUR independently-typed copies of the same eight channels (round 186), and the only one that could say
 *  `envKey: null` — "this channel has no deployment fallback" for nv and gmi (round 187). `byok.ts` was built to
 *  carry exactly that distinction (round 188's deletion criterion: a shared source that cannot say it is a loss of
 *  information), so the derivation is lossless in both directions.
 *
 *  **STILL A PLAIN MUTABLE OBJECT, DELIBERATELY**: `registerChannelKey` below writes into it, and
 *  `Object.fromEntries` produces an ordinary object exactly as the literal did — so the OCP extension point survives
 *  unchanged (its one caller today is the test that registers `zz-test-ocp`). **AND IT IS WHY THE EMITTED RULE TAKES
 *  THE RULE AS AN ARGUMENT**: a rule that looked itself up in Rust could not be extended from here, and deriving
 *  must not quietly remove an extension point. */
export const CHANNEL_KEY_RULES: Record<string, ChannelKeyRule> = Object.fromEntries(
  BYOK_CHANNELS.map((c) => [c.prefix, { userKey: c.userKey, envKey: c.envKey }]),
);

/** OCP extension point: new channels register here — no edit to isModelUsable. */
export function registerChannelKey(prefix: string, rule: ChannelKeyRule): void {
  CHANNEL_KEY_RULES[prefix] = rule;
}

/** Model usable for routing? In the whitelist, (og) breaker not open, AND the REQUESTING user's key for that channel
 *  is configured — a channel without a key 502s every request, so model=auto must not route to it.
 *  round-68: the old code checked ADMIN_ID's keys — a BYOK user with only an og key was told og was "unusable" (the
 *  admin lacks it) and routed to ds, which the user lacks, so every model=auto request 502'd. */
export async function isModelUsable(env: any, model: string, uid: string): Promise<boolean> {
  // Disabling a model must take effect for a route ALREADY POINTING AT IT, not only
  // for new choices — otherwise "retired" would mean "retired for new users".
  if (!(await isAdvertised(env, model))) return false;
  const userKeys: any = await getUserKeys(env, uid).catch(() => ({}));
  const prefix = model.split("/")[0] || "";
  const rule = Object.prototype.hasOwnProperty.call(CHANNEL_KEY_RULES, prefix)
    ? (CHANNEL_KEY_RULES[prefix] as ChannelKeyRule)
    : null;
  const hasUser = rule ? !!userKeys[rule.userKey] : false;
  const hasEnv = rule && rule.envKey ? !!env[rule.envKey] : false;
  // The breaker's verdict is read ONLY for an `og/` model — the source's own test, kept verbatim.
  const degraded = model.startsWith("og/") ? await isChannelDegraded(env) : false;
  return channelUsable(model, rule, hasUser, hasEnv, degraded);
}

/**
 * Resolve Claude Code's fixed `auto` model name to this user's chosen channel (per-user route selection). Falls back
 * to the default channel when unset or unusable — **AND THE POLICY IS THE EMISSION'S**: `pickAutoRoute` decides
 * (choice → ladder in order → default) and this passes it the reads.
 *
 * round-100: this plugin IS the live /v1 path (R99 made handleGateway dispatch plugins first) — the usable-fallback
 * fix landed only in the now-unreachable index.ts copy, so a BYOK user without a DeepSeek key still got a guaranteed
 * 502 on model=auto. Fall back to the first usable channel.
 */
export async function resolveAutoModel(env: any, uid: string): Promise<string> {
  return pickAutoRoute(await getUserRoute(env, uid), (model) => isModelUsable(env, model, uid));
}
