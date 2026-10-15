/**
 * Upstream route table — the single pickRoute/passthroughHeaders/stripBracket
 * implementation for every channel consumer.
 *
 * Was duplicated between plugins/translate.ts (the live /v1/* path) and
 * index.ts (summriseProbe) — the copies had already drifted: translate's or/
 * route stays direct under US_PROXY while index's probe copy still proxied
 * it, so `summrise use or` probed a different upstream than /v1/messages used.
 * One module, one contract. Semantics follow the translate plugin (the
 * battle-tested copy).
 *
 * Boundary (architecture review 2026-09-06): the routing DECISION layer —
 * per-prefix upstream selection, US-egress wrapping, header shaping. The
 * DATA registry (endpoints/whitelists/health cards) lives in channels.ts;
 * the dependency is one-way upstream -> channels. Reviewed as correctly
 * layered; no split warranted.
 */

// **`stripBracket` AND `wireModelName` ARE THE EMISSION'S NOW (2026-10-14)** — `routing-logic.ts`, from
// `routing.rs` and `registry.rs`. Both are one-line rules with a long history (the bracket marker clients
// append; the `og`-only wire remap whose own comment says a wire on any other prefix would be silently
// ignored), and both are re-exported below so every call site keeps its import.
export { stripBracket, wireModelName } from "./routing-logic.ts";

import {
  VERIFY_PATH,
  usProxyBase,
  CMD_CHAT,
  QWEN_COMPAT_CHAT,
  AMD_ANTHROPIC,
  AMD_CHAT,
  R4_ANTHROPIC,
  R4_CHAT,
} from "./channels.ts";
import {
  SUPPORTED_PROVIDER_APIS,
  providerForPrefix,
  type ProviderSpec,
} from "./store/providers.ts";

// **THE REQUEST-SHAPING HELPERS ARE AN EMISSION NOW (2026-10-14)** — `routing-logic.ts`, GENERATED from
// `gateway/wasm/src/routing.rs` and `session.rs`. They were hand-mirrored here (the passthrough header set,
// the session-id extraction and its synthetic fallback, the FNV that makes the fallback stable per user), and
// the worker dials by them. The router's own decisions — `pickRoute`, `providerRoute`, `resolveRoute`,
// `wireModelName`, `stripBracket` — are the next slice; `resolveRoute` is a SEQUENCE (the provider lookup, the
// KV setting, then the table), so it takes the `seedAdminSequence` treatment rather than a straight render.

export {
  clientSessionId,
  fnvHex,
  opencodeSessionHeader,
  passthroughHeaders,
  SESSION_SALT,
  syntheticSessionId,
} from "./routing-logic.ts";

export interface RouteInfo {
  /** How to talk to the upstream: "passthrough" (forward the body as-is),
   *  "translate" (reshape Anthropic <-> OpenAI), or "error" (the route exists
   *  but cannot be dialled — see `reason`). */
  type: string;
  kind: string;
  stripPrefix: boolean;
  upstream: string;
  /** The custom provider this route came from (store/providers.ts). Carried on
   *  the route so the request path can reach the record's key and its
   *  per-model facets without a second registry read. */
  provider?: ProviderSpec;
  /** Why a `type: "error"` route must not be dialled. */
  reason?: string;
}

// SOLID Round-1 (OCP): route builders are DATA — adding a channel registers
// one entry in ROUTE_TABLE instead of editing pickRoute's switch. pickRoute
// itself is now closed for modification (lookup + default fallback only).
export type ViaFn = (direct: string, path: string) => string;
export interface RouteCtx {
  env: any;
  prefix: string;
  usProxy: string | null;
  requestPath: string;
  via: ViaFn;
}
export type RouteBuilder = (ctx: RouteCtx) => RouteInfo;

function orRoute({ requestPath, via }: RouteCtx): RouteInfo {
  // (2026-08-22): off = direct to openrouter.ai; on = via the US egress.
  // requestPath distinguishes the two formats: /v1/messages (Claude Code) and
  // /v1/chat/completions (DSH). The egress is measured to be a pure pipe — it passes Authorization
  // through, so BYOK is unaffected. The openrouter-proxy worker was retired 2026-09-07 (zero callers, dead URL).
  const upstreamPath = requestPath || VERIFY_PATH;
  return {
    type: "passthrough",
    kind: "openrouter", // passes through the user's own OPENROUTER_API_KEY
    stripPrefix: true,
    upstream: via("https://openrouter.ai/api" + upstreamPath, upstreamPath),
  };
}

function dsRoute({ via }: RouteCtx): RouteInfo {
  return {
    type: "passthrough",
    kind: "deepseek",
    stripPrefix: true,
    upstream: via("https://api.deepseek.com/anthropic" + VERIFY_PATH, "/anthropic/v1/messages"),
  };
}

function qwRoute({ requestPath, via }: RouteCtx): RouteInfo {
  // Anthropic endpoint by default (/v1/messages — Claude Code & Anthropic
  // clients). OpenAI-format requests (/v1/chat/completions — DSH & co.)
  // must ride the compatible-mode endpoint: the /apps/anthropic endpoint
  // rejects OpenAI bodies (400 "Request body format invalid").
  return requestPath === "/v1/chat/completions"
    ? {
        type: "passthrough",
        kind: "qwen",
        stripPrefix: true,
        upstream: via(QWEN_COMPAT_CHAT, "/compatible-mode/v1/chat/completions"),
      }
    : {
        type: "passthrough",
        kind: "qwen",
        stripPrefix: true,
        upstream: via(
          "https://token-plan.ap-southeast-1.maas.aliyuncs.com/apps/anthropic" + VERIFY_PATH,
          "/apps/anthropic/v1/messages",
        ),
      };
}

function ogRoute({ via }: RouteCtx): RouteInfo {
  return {
    type: "translate",
    kind: "opencode",
    stripPrefix: true,
    upstream: via("https://opencode.ai/zen/go/v1/chat/completions", "/v1/chat/completions"),
  };
}

function nvRoute({ via }: RouteCtx): RouteInfo {
  // NVIDIA NIM official API — OpenAI format only, dedicated per-key
  // capacity (no shared free pool). Registered models: nemotron family.
  const upstreamPath = "/v1/chat/completions";
  return {
    type: "passthrough",
    kind: "nvidia",
    stripPrefix: true,
    upstream: via("https://integrate.api.nvidia.com" + upstreamPath, upstreamPath),
  };
}

function gmiRoute({ via }: RouteCtx): RouteInfo {
  // GMI Cloud Inference Engine (api.gmi-serving.com) — OpenAI-compatible
  // serverless endpoint; MiniMax Week free tier serves MiniMaxAI/MiniMax-M3
  // and MiniMaxAI/MiniMax-M2.7 free for 14 days (2026-08-24 → 09-06), then
  // standard pricing. Anthropic-format /v1/messages requests are translated
  // by the translate plugin (Anthropic → OpenAI → back), same as nv/.
  const upstreamPath = "/v1/chat/completions";
  return {
    type: "passthrough",
    kind: "gmi",
    stripPrefix: true,
    upstream: via("https://api.gmi-serving.com" + upstreamPath, upstreamPath),
  };
}

function cmRoute({ via }: RouteCtx): RouteInfo {
  // Command Code Provider API (api.commandcode.ai/provider) — Command
  // Code GOAT plan and above have API access (every plan except Go). The
  // Anthropic /v1/messages endpoint serves claude-* models ONLY (verified
  // against the live API: deepseek → 400 "Use /provider/v1/chat/completions
  // for OpenAI and OSS models"), so cm/ rides the OpenAI endpoint: the
  // translate plugin reshapes Anthropic /v1/messages → chat/completions
  // (the og pattern), while OpenAI-format /v1/chat/completions passes
  // through directly. Auth: the user's own CMD_API_KEY as Bearer.
  return {
    type: "translate",
    kind: "commandgoat",
    stripPrefix: true,
    upstream: via(CMD_CHAT, "/v1/chat/completions"),
  };
}

function amdRoute({ requestPath }: RouteCtx): RouteInfo {
  // AMD Radeon Cloud (developer.amd.com.cn/radeon) — a free BYOK pool that
  // speaks BOTH formats natively: Anthropic /v1/messages (thinking blocks,
  // tool_use and SSE verified against the live API 2026-09-02; accepts
  // x-api-key or Bearer) and OpenAI /v1/chat/completions (Bearer). So the
  // route is picked by requestPath, like qw/ — but no translation anywhere.
  //
  // Always DIRECT, never the US exit: developer.amd.com.cn is a CN-served
  // host (a US egress only adds a round the world), and the proxy's TARGETS
  // map has no amd entry — an unknown target silently falls back to zen,
  // which would answer with the wrong model AND the wrong key.
  return requestPath === "/v1/chat/completions"
    ? {
        type: "passthrough",
        kind: "amd",
        stripPrefix: true,
        upstream: AMD_CHAT,
      }
    : {
        type: "passthrough",
        kind: "amd",
        stripPrefix: true,
        upstream: AMD_ANTHROPIC,
      };
}

function r4Route({ requestPath }: RouteCtx): RouteInfo {
  // r4.codes — BOTH formats native on ONE host, so nothing is translated and
  // the route is picked by requestPath (the amd/ pattern). Verified against the
  // live API 2026-09-20: /v1/messages returns a real Anthropic message
  // (thinking blocks included) and accepts x-api-key OR Bearer, and
  // /v1/chat/completions returns a normal chat.completion.
  //
  // Always DIRECT, never the US exit: the egress relay's TARGETS map has no r4
  // entry, and an unknown target silently falls back to zen — which would
  // answer with the wrong model under the user's r4 key (the trap amdRoute
  // records). Adding r4 to that TARGETS map is the prerequisite if the US exit
  // is ever wanted for this channel.
  return requestPath === "/v1/chat/completions"
    ? {
        type: "passthrough",
        kind: "r4",
        stripPrefix: true,
        upstream: R4_CHAT,
      }
    : {
        type: "passthrough",
        kind: "r4",
        stripPrefix: true,
        upstream: R4_ANTHROPIC,
      };
}

function defaultRoute({ via }: RouteCtx): RouteInfo {
  // No prefix / unknown prefix → the DEFAULT channel. Since the 2026-09-10 V4
  // retirement that is Command Code (GOAT), the same channel `auto` resolves
  // to via model-route.ts's DEFAULT_ROUTE_MODEL: DeepSeek official (the old
  // default) is unpayable for this deployment (402 on every request) and its
  // V4 names are retired anyway. The model name passes through VERBATIM
  // (stripPrefix false) — Command Code's catalog covers claude-*/gpt-5.6-*/
  // gemini-*/deepseek/* spellings, so an unprefixed name has a real chance of
  // resolving there; anything retired is stopped earlier by the RETIRED_MODELS
  // gate in translate.ts.
  return {
    type: "translate",
    kind: "commandgoat",
    stripPrefix: false,
    upstream: via(CMD_CHAT, "/v1/chat/completions"),
  };
}

export const ROUTE_TABLE: Record<string, RouteBuilder> = {
  or: orRoute,
  ds: dsRoute,
  qw: qwRoute,
  og: ogRoute,
  nv: nvRoute,
  gmi: gmiRoute,
  cm: cmRoute,
  amd: amdRoute,
  r4: r4Route,
};

/** OCP extension point: new channels register here — no edit to pickRoute. */
export function registerRoute(prefix: string, builder: RouteBuilder): void {
  ROUTE_TABLE[prefix] = builder;
}

/** Does the BUILT-IN route table own this prefix? (Bare form, no slash.) */
export function isBuiltInPrefix(prefix: string): boolean {
  return Object.prototype.hasOwnProperty.call(ROUTE_TABLE, prefix);
}

/**
 * The route for a CUSTOM provider (store/providers.ts) — kind "custom",
 * upstream built from the record.
 *
 * THE URL JOIN IS DSH'S, deliberately. `baseURL` is a PREFIX and the dialect's
 * path is appended (`openai-completions` → `/chat/completions`), which is what
 * the OpenAI SDK does with `baseURL` — the reason DSH's own settings.yaml can
 * name the gateway itself (`https://api.saisi.online`, no `/v1`; the gateway
 * aliases that path). Treating it as a URL to resolve against would throw away
 * a deployment path like `https://host/openai/v1`.
 *
 * ALWAYS type "translate": /v1/chat/completions forwards the OpenAI body
 * verbatim (that arm never looks at `type`), while /v1/messages must reshape
 * Anthropic → chat/completions — with `toOpenAIRequest` +
 * `openAIUpstreamToAnthropicResponse`, the og/cm path. No new translator.
 *
 * AN UNROUTABLE RECORD IS AN ERROR ROUTE, NEVER THE DEFAULT CHANNEL. A dialect
 * this build does not serve, or a record with no baseURL (hand-edited KV), must
 * not fall through to `defaultRoute`: that would dial a built-in upstream —
 * Command Code — under a different provider's name, which is exactly the
 * "advertised but silently somewhere else" failure a provider registry exists
 * to remove.
 */
export function providerRoute(provider: ProviderSpec): RouteInfo {
  const join = SUPPORTED_PROVIDER_APIS[provider?.api];
  if (!provider?.baseURL || !join) {
    return {
      type: "error",
      kind: "custom",
      stripPrefix: true,
      upstream: "",
      provider,
      reason: `custom provider ${provider?.prefix || "?"} cannot be routed: ${JSON.stringify(
        provider?.api,
      )} is not a protocol this gateway serves (see store/providers.ts)`,
    };
  }
  return {
    type: "translate",
    kind: "custom",
    stripPrefix: true,
    upstream: provider.baseURL + join,
    provider,
  };
}

/**
 * Resolve the route for a request prefix — the ONE entry point that knows about
 * custom providers. `pickRoute` stays exactly as it was (a pure built-in table
 * lookup), so the extension point is a layer rather than a rewrite:
 *
 *   1. ROUTE_TABLE[prefix]       — a built-in channel always wins, and costs no
 *                                  registry read: the common case is untouched.
 *   2. providers:custom[prefix]  — a custom provider (see providerRoute).
 *   3. pickRoute → defaultRoute  — no prefix / genuinely unknown prefix.
 *
 * The built-in check comes FIRST on purpose (see store/providers.ts): a KV
 * record that shadowed a built-in prefix — hand-edited, or written before a
 * built-in channel existed — would re-point every existing route, and the key
 * that rides it, at a third party.
 */
export async function resolveRoute(
  env: any,
  prefix: string,
  usProxy: string | null = null,
  requestPath: string = VERIFY_PATH,
): Promise<RouteInfo> {
  if (isBuiltInPrefix(prefix)) return pickRoute(prefix, env, usProxy, requestPath);
  const provider = await providerForPrefix(env, prefix);
  if (provider) return providerRoute(provider);
  return pickRoute(prefix, env, usProxy, requestPath);
}

export function pickRoute(
  prefix: string,
  env: any,
  usProxy: string | null = null,
  requestPath: string = VERIFY_PATH,
): RouteInfo {
  // US egress switch: with US_PROXY=1 all models reach upstreams via the api relay
  // (v.saisi.online/api/zen — served by vrelay on the Oracle box since 2026-09-08)
  // from US edge nodes, avoiding regional restrictions/congestion. target=og|ds|qw|or selects the upstream,
  // the path param carries the upstream relative path (the proxy base already includes the host-level prefix). usProxy is a local
  // per-request value — never mutate the shared env object with it.
  const via: ViaFn = (direct: string, path: string): string =>
    usProxy
      ? // audit round F4: prefix is model-derived ARBITRARY text — unencoded
        // it could inject &path=… into the egress URL and re-point the proxy
        // request. Encode (the proxy decodes) so it stays one opaque value.
        `${usProxyBase(env)}/api/zen?target=${encodeURIComponent(prefix)}&path=${encodeURIComponent(path)}`
      : direct;
  // CLOSED for modification: new prefixes register in ROUTE_TABLE above.
  const builder: RouteBuilder = Object.prototype.hasOwnProperty.call(ROUTE_TABLE, prefix)
    ? (ROUTE_TABLE[prefix] as RouteBuilder)
    : defaultRoute;
  return builder({ env, prefix, usProxy, requestPath, via });
}

// Stable per-conversation session id for opencode zen/go requests.
//
// zen/go requires x-opencode-session on every request since 2026-09-05 and
// 400s without it ("Request is missing x-opencode-session and cannot be
// routed efficiently" — the muse-spark 1.3 breakage; the same gate hits the
// og chat-completions/search paths). It wants a stable per-conversation
// identifier so it can route requests and reuse prompt caches.
//
// The gateway relays the CLIENT's own per-conversation identifiers when a
// client sends one (keeping one cache namespace per real conversation):
//   - x-opencode-session — native opencode clients / future DSH builds;
//   - x-client-request-id — DSH's pi-ai adapter stamps the per-conversation
//     session uuid on every openai-responses request (summrise-muse today);
//   - session_id / x-session-id — OpenAI/OpenRouter-style conversation ids.
// Otherwise it synthesizes a stable per-user value derived WITHOUT KV (no
// extra reads/writes): a digest of the uid under a constant application
// salt. Stable across isolates and deploys (cache reuse), never a secret —
// zen treats the value as a routing hint, not a gate.
//
// Only attached to requests actually destined for zen/go (route.kind ===
// "opencode"), so ds/qw/or/nv/gmi/cm/amd wires stay untouched and no foreign
// header leaks to other upstreams.
