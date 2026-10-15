/**
 * routing-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/routing.rs` (`render_typescript`). The freshness test there is a byte comparison:
 *
 *     SUMMRISE_REFRESH_ROUTING=1 cargo test --manifest-path gateway/wasm/Cargo.toml routing::emission
 *
 * `gateway/src/channels.ts` re-exports this module. **AND IT IMPORTS THE REGISTRY FROM THE OTHER EMISSION**:
 * `OG_FORCE_US_PROXY` is derived from `MODEL_REGISTRY`, which is `channels-logic.ts`'s — one generated module
 * reading another's TABLE is a dependency, not a cycle, and the direction is one way.
 */
import { MODEL_REGISTRY } from "./channels-logic.ts";

/** The path an Anthropic-format upstream serves. */
export const VERIFY_PATH: string = "/v1/messages";

/** zen's Anthropic endpoint — the route `/v1/messages` takes for `og/`. */
export const OG_ZEN_ANTHROPIC: string = "https://opencode.ai/zen/go/v1/messages";

/** zen's OpenAI-compatible endpoint. */
export const OG_ZEN_CHAT: string = "https://opencode.ai/zen/go/v1/chat/completions";

/** Command Code's OpenAI-compatible endpoint (the default channel's dialect). */
export const CMD_CHAT: string = "https://api.commandcode.ai/provider/v1/chat/completions";

/**
 * Qwen MaaS (Aliyun Token Plan), OpenAI-compatible mode — **NOT** the Anthropic
 * `/apps/anthropic/v1/messages` endpoint, which rejects OpenAI-format bodies with 400 "Request body format
 * invalid", so chat/completions requests (DSH & co.) must ride the compatible-mode endpoint instead.
 */
export const QWEN_COMPAT_CHAT: string =
  "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1/chat/completions";

/**
 * AMD Radeon Cloud (`developer.amd.com.cn/radeon`) — a self-deploy inference pool ("Dynamic sglang/vllm-router
 * service managed by Model Ops"), free tier with a per-model global concurrency cap. **BOTH formats are
 * native**: `/api/v1/messages` really speaks Anthropic (thinking blocks + tool_use + SSE, verified against the
 * live API 2026-09-02) and `/api/v1/chat/completions` speaks OpenAI — no translation on either path.
 */
export const AMD_ANTHROPIC: string = "https://developer.amd.com.cn/radeon/api/v1/messages";

/** AMD's OpenAI-compatible endpoint. */
export const AMD_CHAT: string = "https://developer.amd.com.cn/radeon/api/v1/chat/completions";

/**
 * r4.codes — a coding-agent gateway serving its whole catalog on ONE host under BOTH protocols natively, like
 * AMD (verified against the live API 2026-09-20): `/v1/messages` returns a real Anthropic message (thinking
 * blocks included) and accepts `x-api-key` OR `Bearer`, and `/v1/chat/completions` returns a normal
 * chat.completion. No translation on either path, so the route is picked by `requestPath`.
 */
export const R4_ANTHROPIC: string = "https://api.r4.codes/v1/messages";

/** r4's OpenAI-compatible endpoint. */
export const R4_CHAT: string = "https://api.r4.codes/v1/chat/completions";

/** Reserved for future use — **CURRENTLY EMPTY**, and that is a measurement rather than a placeholder: models
 *  listed here would bypass the OpenAI translate path and take native Anthropic `/v1/messages` passthrough. */
export const OG_NATIVE_ANTHROPIC: Set<string> = new Set();

/** The models that MUST ride the US egress pool — `usEgress` on the registry, by advertised id. */
export const OG_FORCE_US_PROXY: Set<string> = new Set(
  MODEL_REGISTRY.filter((m) => m.usEgress).map((m) => m.id),
);

/** `model id → the id to suggest instead`, for the ids this deployment has retired. */
export const RETIRED_MODELS: Record<string, string> = {
  "ds/deepseek-v4-flash": "cm/deepseek/deepseek-v4.1-flash",
  "og/deepseek-v4-flash": "og/deepseek-v4.1-flash",
  "cm/deepseek/deepseek-v4-flash": "cm/deepseek/deepseek-v4.1-flash",
  "cm/deepseek/deepseek-v4-flash-vision-exp": "cm/deepseek/deepseek-v4.1-flash",
  "or/deepseek/deepseek-v4-flash-0731": "cm/deepseek/deepseek-v4.1-flash",
  "amd/deepseek-v4-flash": "cm/deepseek/deepseek-v4.1-flash",
  "amd/deepseek-v4-flash-vision-exp": "cm/deepseek/deepseek-v4.1-flash",
  "deepseek-v4-flash": "cm/deepseek/deepseek-v4.1-flash",
  "deepseek-v4-flash-vision-exp": "cm/deepseek/deepseek-v4.1-flash",
  "deepseek-v4-flash-0731": "cm/deepseek/deepseek-v4.1-flash",
  "deepseek/deepseek-v4-flash": "cm/deepseek/deepseek-v4.1-flash",
  "deepseek/deepseek-v4-flash-vision-exp": "cm/deepseek/deepseek-v4.1-flash",
  "deepseek/deepseek-v4-flash-0731": "cm/deepseek/deepseek-v4.1-flash",
};

/** The replacement to suggest for a retired id, or null. */
export function retiredModelHint(model: string): string | null {
  const key = String(model || "")
    .trim()
    .toLowerCase();
  return Object.prototype.hasOwnProperty.call(RETIRED_MODELS, key)
    ? (RETIRED_MODELS[key] as string)
    : null;
}

/** `usProxyBase(env)` — the US-exit base, `US_PROXY_BASE` over it. **AN EMPTY VALUE FALLS BACK**, which is
 *  what the deployment that clears the variable wants. */
export function usProxyBase(env: any): string {
  return env?.US_PROXY_BASE || "https://v.saisi.online";
}

/** `museResponsesExit(env)` — where `muse-spark-*`'s `/v1/responses` requests leave through: `vercel` builds a
 *  path on the US-proxy base, `zen-us` is a fixed host, any absolute URL is taken as given, and everything else
 *  (including unset) is the oracle. */
export function museResponsesExit(env: any): string {
  const v = env?.MUSE_RESPONSES_EXIT;
  if (v === "vercel") {
    return `${usProxyBase(env)}/api/zen?target=og&path=${encodeURIComponent("/v1/responses")}`;
  }
  if (v === "zen-us") return "https://zen-us.saisi.online/v1/responses";
  if (typeof v === "string" && (v.startsWith("http://") || v.startsWith("https://"))) return v;
  return "https://oracle.saisi.online/v1/responses";
}

// ── THE REQUEST-SHAPING HELPERS (`session.rs`) ──────────────────────────────────────────────────
//
// **THEY ARE HERE BECAUSE THE ROUTER IS WHAT CALLS THEM**, and they were hand-mirrored in `upstream.ts`:
// the passthrough header set, the session-id extraction and its synthetic fallback, and the FNV that makes
// the fallback stable per user.

/**
 * The headers a passthrough dial sends. **THE CLIENT'S OWN AUTH IS NEVER FORWARDED** — the user's key is —
 * and `apiKeyHeader` is the one upstream that authenticates with `x-api-key` instead of `Bearer`.
 *
 * The two base headers are unconditional: every passthrough target speaks the Anthropic protocol (ds/qw
 * native, the openrouter proxy), and OpenAI-format backends ignore `anthropic-version`.
 */
export function passthroughHeaders(
  bearerKey: string | null,
  {
    apiKeyHeader = false,
    extra = {},
  }: { apiKeyHeader?: string | false; extra?: Record<string, string> } = {},
): Headers {
  const h = new Headers();
  h.set("Content-Type", "application/json");
  h.set("anthropic-version", "2023-06-01");
  // An EMPTY key is not a key: the arms below are skipped rather than sent blank.
  if (bearerKey) {
    if (apiKeyHeader) h.set(apiKeyHeader, bearerKey);
    else h.set("Authorization", `Bearer ${bearerKey}`);
  }
  for (const [name, value] of Object.entries(extra)) {
    if (value) h.set(name, value);
  }
  return h;
}

/** `SESSION_SALT` — **A CONSTANT APPLICATION SALT, NOT A SECRET**: it keeps two deployments' synthetic ids
 *  from colliding without pretending to be a credential. */
export const SESSION_SALT = "summrise-og-session-v1";

/** One header, trimmed, or `""` — **AND IT ANSWERS `""` RATHER THAN THROWING** for a `HeadersInit` the
 *  platform refuses (the source's `try`/`catch`), because a malformed header set must not fail a dial. */
function headerValue(headers: Headers | HeadersInit | undefined, name: string): string {
  if (!headers) return "";
  try {
    const h = headers instanceof Headers ? headers : new Headers(headers as HeadersInit);
    return h.get(name)?.trim() || "";
  } catch {
    return "";
  }
}

/** The client's session id, from the four header names zen/go and its clients use — the FIRST non-empty one
 *  wins, and an absent one answers `""` rather than throwing. */
export function clientSessionId(incoming: Headers | HeadersInit | undefined): string {
  return (
    headerValue(incoming, "x-opencode-session") ||
    headerValue(incoming, "x-client-request-id") ||
    headerValue(incoming, "session_id") ||
    headerValue(incoming, "x-session-id")
  );
}

/** A per-USER session id for a client that sent none: `summrise-` and the FNV of a salted uid, so the same
 *  user gets the same id across requests and two users never collide. */
export function syntheticSessionId(uid: string): string {
  return `summrise-${fnvHex(`${SESSION_SALT}:${uid}`)}`;
}

/** The header zen/go routes on: the client's id verbatim, or the synthetic per-user value. */
export function opencodeSessionHeader(
  incoming: Headers | HeadersInit | undefined,
  uid: string,
): { "x-opencode-session": string } {
  return { "x-opencode-session": clientSessionId(incoming) || syntheticSessionId(uid) };
}

/** Two 32-bit FNV-1a lanes over the string's UTF-16 code units, hex-padded — **`Math.imul` SEMANTICS, WHICH
 *  IS WHY IT IS NOT ONE 64-BIT HASH**: the source's arithmetic wraps at 32 bits per lane, and the emitted
 *  TypeScript keeps `Math.imul` so the two implementations agree digit for digit. */
export function fnvHex(s: string): string {
  let h1 = 0x811c9dc5;
  let h2 = 0x01000193;
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    h1 = Math.imul(h1 ^ c, 0x01000193) >>> 0;
    h2 = Math.imul(h2 ^ c, 0x85ebca6b) >>> 0;
  }
  return h1.toString(16).padStart(8, "0") + h2.toString(16).padStart(8, "0");
}
