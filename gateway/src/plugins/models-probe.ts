// models-probe — ask a provider what it actually serves.
//
// WHY THIS EXISTS. The catalogue is hand-maintained: a model is added by NAME, and
// nothing tells an operator that the upstream has since added or retired one.
// `scripts/model-drift.mjs` has been able to answer that since round 57 — as an ops
// tool, run from a laptop, with its own copy of the upstream URLs. This brings the
// same question into the console, where the person editing the catalogue is: what
// does this provider offer, and which of its models am I not advertising?
//
// THE GUARD RUNS AT DIAL TIME, on the URL about to be fetched, and it is the SAME
// function the registration path uses (`deviceHostError`, the SSRF stack shared with
// the device dialler). Registration already validates a custom baseURL, but a record
// that predates a guard — or one written by a path that forgot to call it — would
// otherwise be dialled anyway. The rule this repo learned the hard way (round 95) is
// that a guard is only real at the point of use.
//
// NO REDIRECTS ARE FOLLOWED (`redirect: "manual"`), for round 90's reason: a 3xx is a
// different host, and the request carries a credential.
//
// DERIVE, DO NOT GUESS. The model-list endpoint is derived from the upstream the
// router already resolved (`…/chat/completions` → `…/models`). For a dialect where
// that derivation does not hold — the Anthropic-shaped channel — the probe says it
// cannot check rather than inventing a URL, and the ops tool keeps the verified list
// for those. A wrong endpoint would produce a confident, wrong catalogue diff.
import { deviceHostError } from "../device-fetch.ts";
import { RESERVED_PREFIXES } from "../channels.ts";
import { fetchWithTimeout, upstreamTimeoutMs } from "../reliability.ts";
import { jsonError, jsonOk } from "../http.ts";
import { requireAdmin } from "../session.ts";
import {
  barePrefix,
  providerKey,
  providerForPrefix,
  SUPPORTED_PROVIDER_APIS,
} from "../store/providers.ts";
import { getUserKeys } from "../store/users.ts";
import { resolveRoute } from "../upstream.ts";
import { advertisedIds } from "../store/models.ts";

/** The upstream's model-list endpoint, or null when this dialect has no derivable one. */
export function upstreamModelsUrl(upstream: string): string | null {
  const suffix = "/chat/completions";
  return upstream.endsWith(suffix) ? upstream.slice(0, -suffix.length) + "/models" : null;
}

/** Which of the caller's BYOK keys this channel uses. Keyed by the ROUTE KIND the
 *  router reports, which is the same vocabulary `translate.ts`'s missing-key table
 *  uses — a second naming of channels is how two tables drift apart. */
const BYOK_KEY_FOR_KIND: Record<string, string> = {
  openrouter: "OPENROUTER_API_KEY",
  commandgoat: "CMD_API_KEY",
  qwen: "QWEN_API_KEY",
  nvidia: "NVAPI_KEY",
  gmi: "GMI_API_KEY",
  amd: "AMD_API_KEY",
  opencode: "OPENCODE_GO_API_KEY",
  deepseek: "DEEPSEEK_API_KEY",
};

/** Upstream ids from either dialect we speak: an OpenAI-style `{data:[{id}]}`, an
 *  Anthropic-style `{data:[{id}]}`, or a bare array. Anything else is not a list. */
export function offeredIds(payload: any): string[] {
  const rows = Array.isArray(payload)
    ? payload
    : Array.isArray(payload?.data)
      ? payload.data
      : null;
  if (!rows) return [];
  return rows
    .map((r: any) => (typeof r === "string" ? r : String(r?.id ?? "")))
    .map((s: string) => s.trim())
    .filter(Boolean);
}

export async function adminProbeModels(request: Request, env: any): Promise<Response> {
  const gate = await requireAdmin(request, env);
  if (gate instanceof Response) return gate;

  let body: any;
  try {
    body = await request.json();
  } catch {
    return jsonError(400, "body must be JSON", "invalid_request");
  }
  const bare = barePrefix(String(body?.prefix ?? ""));
  if (!bare) return jsonError(400, "a provider prefix is required", "invalid_request");

  // A PREFIX THAT NOBODY CLAIMS IS NOT A ROUTE TO TEST. `resolveRoute` falls through
  // to the default channel for an unknown prefix — that is the correct routing rule
  // ("no prefix -> Command Code") and the WRONG answer here: the probe would dial,
  // and report on, a channel the caller never named. So the prefix must belong to
  // somebody first: a custom provider's record, or a built-in channel.
  const provider = await providerForPrefix(env, bare);
  const builtIn = RESERVED_PREFIXES.includes(bare) || bare === "none";
  if (!provider && !builtIn) {
    return jsonOk({
      checked: false,
      reason: `${bare}/ is not a provider — requests for it would fall through to the default channel`,
    });
  }
  // The router's own answer after that, so the probe cannot test a DIFFERENT route
  // than the one requests take.
  const route = await resolveRoute(env, bare, null);
  if (route.type === "error") {
    return jsonOk({ checked: false, reason: route.reason || `${bare} is not a routable prefix` });
  }
  const endpoint = upstreamModelsUrl(route.upstream);
  if (!endpoint) {
    return jsonOk({
      checked: false,
      reason: `${bare} speaks a dialect whose model list this build cannot derive — the checked list lives in scripts/model-drift.mjs`,
    });
  }

  // THE GUARD, ON THE URL ABOUT TO BE FETCHED.
  let host: string;
  try {
    host = new URL(endpoint).hostname;
  } catch {
    return jsonOk({ checked: false, reason: `${bare}: the resolved upstream is not a URL` });
  }
  const refused = deviceHostError(host);
  if (refused) return jsonError(502, `${bare}: ${refused}`, "config_error");

  // The credential the REQUEST would use: a custom provider's own record, otherwise
  // the caller's BYOK key. A channel that needs one and has none is reported as NOT
  // CHECKED — never as "offers nothing", which is the same distinction the ops tool
  // makes and the reason it prints NOT CHECKED rather than an empty list.
  let key = "";
  if (route.kind === "custom") {
    key = providerKey(env, provider ?? route.provider);
  } else {
    const name = BYOK_KEY_FOR_KIND[route.kind];
    if (name) key = String((await getUserKeys(env, gate.id))?.[name] ?? "");
  }

  let offered: string[];
  try {
    const res = await fetchWithTimeout(
      endpoint,
      {
        method: "GET",
        headers: {
          accept: "application/json",
          ...(key ? { authorization: `Bearer ${key}` } : {}),
        },
        // NOT followed: a 3xx is another host and this request carries a credential.
        redirect: "manual",
      },
      upstreamTimeoutMs(env),
    );
    if (res.status === 401 || res.status === 403) {
      return jsonOk({
        checked: false,
        endpoint,
        reason: "the upstream rejected this credential (401/403) — nothing was compared",
      });
    }
    if (!res.ok) {
      return jsonOk({ checked: false, endpoint, reason: `upstream answered ${res.status}` });
    }
    offered = offeredIds(await res.json());
  } catch (e: any) {
    return jsonOk({
      checked: false,
      endpoint,
      reason: `could not reach the upstream: ${e?.message ?? e}`,
    });
  }

  // What WE advertise for this prefix, from the authoritative advertised set (the
  // same list `/v1/models` serves), minus the prefix so the two are comparable.
  const advertised = (await advertisedIds(env)).filter((id) => id.startsWith(bare + "/"));
  const offeredSet = new Set(offered);
  const advertisedSet = new Set(advertised);
  return jsonOk({
    checked: true,
    endpoint,
    offered,
    advertised,
    // OFFERED BUT NOT ADVERTISED — the adopt candidates, returned PREFIXED so that
    // adopting one is a direct call to the existing add-model route. A bare id here
    // would leave the panel to re-derive the prefix, which is the exact join that
    // once filed a model under the wrong channel.
    notAdvertised: offered
      .filter((id) => !advertisedSet.has(bare + "/" + id))
      .map((id) => bare + "/" + id),
    // ADVERTISED BUT NOT LISTED — printed as a CHECK, never a verdict: the router
    // normalises names (wire remaps, `[1m]` markers), so a raw diff reports false
    // drift. `scripts/model-drift.mjs` records the first live run where exactly one
    // of these was genuine.
    notOffered: advertised.filter((id) => !offeredSet.has(id.slice(bare.length + 1))),
  });
}

/** The dialects this build serves — exported so the panel's form can offer exactly
 *  these instead of guessing (it once offered `anthropic-messages`). */
export const PROBE_DIALECTS = SUPPORTED_PROVIDER_APIS;
