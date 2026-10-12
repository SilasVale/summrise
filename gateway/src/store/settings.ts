/**
 * store/settings.ts — global settings (console-controlled, e.g. the US_PROXY exit switch),
 * stored under `settings:<name>` in KV.
 *
 * ── THE DECISIONS ARE RUST NOW, AND THIS FILE IS A BOUNDARY ───────────────────
 *
 * `normalizeSetting` (round-95/96: normalize AT THE READ, so every consumer is fixed at once),
 * `globalSettingEnabled` (round-94: an explicit OFF is persisted as "0", which is TRUTHY as a string)
 * and the canonicalization inside `setGlobalSetting` (round-439: a real boolean `true` must not invert
 * the switch) are all emitted from `gateway/wasm/src/settings_logic.rs` into `settings-logic.ts`.
 *
 * **THE EMISSION IS THE SEAM, NOT THE DECISION ROUTE, AND THE REASON IS MEASURABLE**: `npm test` cannot
 * reach `/__decision` — `helpers.mjs`'s `WASM_GATE` stub answers a fixed model list for the `/v1` alias
 * tests — so a twin deleted in favour of a route call would delete the only implementation the tests can
 * reach. The emitted module is JavaScript, so the tests keep working, and it is GENERATED, so it is not
 * one of the files this migration is counting. **No third copy: the emission IS the Rust's output.**
 *
 * What is left here is KV, the Worker-var fallback, the cache and the reads.
 */

import { cdel, cget, cset, type Env } from "./cache.ts";
import { canonicalSetting, globalSettingEnabled, normalizeSetting } from "./settings-logic.ts";

export { globalSettingEnabled };

export async function getGlobalSetting(env: Env, name: string): Promise<string | null> {
  const key = `settings:${name}`;
  const hit = cget(key);
  if (hit !== undefined) return normalizeSetting(hit);
  let v: string | null = await env.KEYS.get(key);
  if (v === null || v === undefined) v = env[name] ? String(env[name]) : null;
  // round-96: the CACHE HIT path above once bypassed this normalization, so the isolate that just
  // wrote the "0" kept reading it as ON for the cache TTL. Both paths normalize now.
  v = normalizeSetting(v);
  cset(key, v);
  return v;
}

export async function setGlobalSetting(env: Env, name: string, value: any): Promise<void> {
  const key = `settings:${name}`;
  // round-94: `null` means DELETE (so the Worker var shows through) and `"0"` means an explicit OFF
  // that SHADOWS it — `canonicalSetting` draws that line, and it is Rust's.
  const canonical = canonicalSetting(value);
  if (canonical === null) {
    await env.KEYS.delete(key);
    cdel(key);
    return;
  }
  await env.KEYS.put(key, canonical);
  cset(key, canonical); // write-through: the switch takes effect immediately within the same isolate
}
