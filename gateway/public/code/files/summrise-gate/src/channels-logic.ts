// channels-logic.ts — GENERATED. DO NOT EDIT BY HAND.
//
// Emitted by `gateway/wasm/src/registry.rs` (`render_typescript`). The freshness test in that module is a
// byte comparison against this file:
//
//     SUMMRISE_REFRESH_CHANNELS=1 cargo test --manifest-path gateway/wasm/Cargo.toml registry::emission
//
// `gateway/src/channels.ts` re-exports this module. **THE CHANNEL CARDS LIVE HERE NOW** — they were
// `channels.ts`'s table, mirrored into the worker as a generated JSON twin with a script and a freshness
// test of its own. A twin is a second source wearing a freshness test: the prose had to be edited in
// TypeScript, copied by the script, committed, and only then could the worker serve it. The table moved
// into the Rust, the cards are rendered back here, and the twin, its generator and its test are gone.
//
// **AN EDIT TO A CARD IS NOW A RUST EDIT PLUS**:
//
//     cd gateway && SUMMRISE_REFRESH_CHANNELS=1 cargo test --manifest-path wasm/Cargo.toml registry::emission

/** The advertised models on one channel prefix, in catalogue order.  */
export const ROUTE_INFO: {
  prefix: string;
  backend: string;
  desc: string;
  models: string[];
}[] = [
  {
    prefix: "og/",
    backend: "OpenCode Go",
    desc: "opencode.ai/zen/go — all models via chat/completions (OpenAI format); gpt-5.6-luna auto-routes via OpenRouter US exit (zen region-blocks it); muse-spark-* via /v1/responses forced through the US exit (Meta region policy)",
    models: [
      "deepseek-v4.1-flash",
      "minimax-m3",
      "mimo-v2.5",
      "mimo-v2.6-flash",
      "ox-alpha-free",
      "muse-spark-1.3-contributor",
      "muse-spark-1.2-contributor",
      "gpt-5.6-luna",
      "openai/gpt-5.6-luna:floor[1m]",
    ],
  },
  {
    prefix: "or/",
    backend: "OpenRouter",
    desc: "openrouter.ai — user's own key (BYOK); dual-format passthrough, US-proxy switch decides direct vs exit",
    models: [
      "openai/gpt-5.6-luna:floor[1m]",
      "z-ai/glm-5.2:free",
      "nvidia/nemotron-3-ultra-550b-a55b:free",
      "stealth/ox-alpha",
    ],
  },
  {
    prefix: "nv/",
    backend: "NVIDIA NIM",
    desc: "integrate.api.nvidia.com — official nemotron API, dedicated key capacity (build.nvidia.com), OpenAI format",
    models: ["nvidia/nemotron-3-ultra-550b-a55b", "moonshotai/kimi-k3"],
  },
  {
    prefix: "gmi/",
    backend: "GMI Cloud",
    desc: "api.gmi-serving.com — MiniMax Week free tier (MiniMax-M3/M2.7 free 14 days, user's own GMI key), OpenAI format; any catalog model reachable as gmi/<id>",
    models: ["MiniMaxAI/MiniMax-M3", "MiniMaxAI/MiniMax-M2.7"],
  },
  {
    prefix: "qw/",
    backend: "Qwen MaaS (Aliyun)",
    desc: "token-plan.ap-southeast-1.maas.aliyuncs.com — Anthropic passthrough",
    models: ["qwen3.8-max-preview", "qwen3.8-flash", "deepseek-v4.1-flash"],
  },
  {
    prefix: "cm/",
    backend: "Command Code (GOAT)",
    desc: "api.commandcode.ai/provider — GOAT plan & up get Provider API access (Go plan excluded); Anthropic /v1/messages translated to chat/completions (the Anthropic endpoint only serves claude-*), OpenAI format passes through; any catalog model reachable as cm/<id>",
    models: [
      "meituan/LongCat-2.0:free",
      "poolside/laguna-s-2.1-free",
      "deepseek/deepseek-v4.1-flash",
    ],
  },
  {
    prefix: "r4/",
    backend: "r4.codes",
    desc: "api.r4.codes — Anthropic /v1/messages AND OpenAI /v1/chat/completions both native (no translation either way), user's own R4_API_KEY; no server-side tools — a web_search request is forwarded UNCHANGED and the tool is ignored (r4 is not on the parse gate that swaps to the og/ lane); any catalog model reachable as r4/<id>",
    models: ["deepseek-v4.1-flash"],
  },
  {
    prefix: "none",
    backend: "Command Code (default)",
    desc: "no prefix → the default channel, Command Code (GOAT) with the model name passed through as-is; `auto` resolves to cm/deepseek/deepseek-v4.1-flash (per-user selection first, see model-route.ts). Note: cm/ rides Command Code's OpenAI endpoint, which rejects claude-* ids (those exist only on their Anthropic endpoint) — use a prefixed deepseek/OSS model there",
    models: ["deepseek/deepseek-v4.1-flash"],
  },
];

/** One catalogue entry. The three DISPLAY facets (`name`, `contextWindow`, `maxTokens`) and the
 * reasoning default may be overridden by a deployment; `wire`, `usEgress`, `search` and
 * `responsesOnly` change where a request GOES and stay pinned here. */
export interface ModelSpec {
  id: string;
  ownedBy: string;
  wire?: string;
  usEgress?: boolean;
  search?: boolean;
  responsesOnly?: boolean;
  name?: string;
  contextWindow?: number;
  maxTokens?: number;
  reasoningEffort?: "low" | "medium" | "high" | "max";
  reasoningMax?: "raw" | "parsed";
  probe?: boolean;
  probeWhy?: string;
}

export const MODEL_REGISTRY: ModelSpec[] = [
  {
    id: "og/deepseek-v4.1-flash",
    ownedBy: "opencode",
    wire: "deepseek-flash",
    search: true,
    probe: true,
  },
  {
    id: "og/minimax-m3",
    ownedBy: "opencode",
    probe: false,
    probeWhy: "translate-only model; the og/ health card covers the channel",
  },
  {
    id: "og/mimo-v2.5",
    ownedBy: "opencode",
    probe: true,
  },
  {
    id: "og/mimo-v2.6-flash",
    ownedBy: "opencode",
    probe: true,
  },
  {
    id: "og/ox-alpha-free",
    ownedBy: "opencode",
    reasoningMax: "parsed",
    probe: true,
  },
  {
    id: "og/muse-spark-1.3-contributor",
    ownedBy: "opencode",
    usEgress: true,
    responsesOnly: true,
    probe: true,
  },
  {
    id: "og/muse-spark-1.2-contributor",
    ownedBy: "opencode",
    usEgress: true,
    responsesOnly: true,
    probe: false,
    probeWhy: "superseded by 1.3; one probe per model family is enough",
  },
  {
    id: "og/gpt-5.6-luna",
    ownedBy: "opencode",
    usEgress: true,
    probe: true,
  },
  {
    id: "og/openai/gpt-5.6-luna:floor[1m]",
    ownedBy: "opencode",
    usEgress: true,
    probe: false,
    probeWhy: "the or/ spelling below is the probed luna card",
  },
  {
    id: "or/openai/gpt-5.6-luna:floor[1m]",
    ownedBy: "openrouter",
    probe: true,
  },
  {
    id: "or/z-ai/glm-5.2:free",
    ownedBy: "openrouter",
    probe: true,
  },
  {
    id: "or/nvidia/nemotron-3-ultra-550b-a55b:free",
    ownedBy: "openrouter",
    probe: true,
  },
  {
    id: "nv/nvidia/nemotron-3-ultra-550b-a55b",
    ownedBy: "nvidia",
    probe: true,
  },
  {
    id: "nv/moonshotai/kimi-k3",
    ownedBy: "nvidia",
    probe: false,
    probeWhy: "the nv/ health card already probes nemotron",
  },
  {
    id: "gmi/MiniMaxAI/MiniMax-M3",
    ownedBy: "gmi",
    probe: true,
  },
  {
    id: "gmi/MiniMaxAI/MiniMax-M2.7",
    ownedBy: "gmi",
    probe: true,
  },
  {
    id: "or/stealth/ox-alpha",
    ownedBy: "openrouter",
    reasoningMax: "raw",
    probe: true,
  },
  {
    id: "qw/qwen3.8-max-preview",
    ownedBy: "qwen",
    probe: true,
  },
  {
    id: "qw/qwen3.8-flash",
    ownedBy: "qwen",
    probe: true,
  },
  {
    id: "qw/deepseek-v4.1-flash",
    ownedBy: "qwen",
    probe: true,
  },
  {
    id: "cm/meituan/LongCat-2.0:free",
    ownedBy: "command-code",
    probe: true,
  },
  {
    id: "cm/poolside/laguna-s-2.1-free",
    ownedBy: "command-code",
    probe: true,
  },
  {
    id: "cm/deepseek/deepseek-v4.1-flash",
    ownedBy: "command-code",
    probe: true,
  },
  {
    id: "r4/deepseek-v4.1-flash",
    ownedBy: "r4",
    probe: true,
  },
];

/** The advertised ids and their owners, in catalogue order — what `/v1/models` reports. */
export const MODELS: { id: string; owned_by: string }[] = MODEL_REGISTRY.map((m) => ({
  id: m.id,
  owned_by: m.ownedBy,
}));

/** The `/api/health` cards, IN ORDER — the order is what the response bytes are. The duplicate ids are
 * deliberate: `buildHealth` asks the og circuit once per og card, and `recommended` takes the FIRST
 * card of a priority id. */
export const HEALTH_CHANNELS: { id: string; model: string }[] = [
  { id: "qw", model: "qw/qwen3.8-max-preview" },
  { id: "qw", model: "qw/qwen3.8-flash" },
  { id: "qw", model: "qw/deepseek-v4.1-flash" },
  { id: "og", model: "og/deepseek-v4.1-flash" },
  { id: "og", model: "og/gpt-5.6-luna" },
  { id: "og", model: "og/mimo-v2.5" },
  { id: "og", model: "og/mimo-v2.6-flash" },
  { id: "og", model: "og/ox-alpha-free" },
  { id: "og", model: "og/muse-spark-1.3-contributor" },
  { id: "or", model: "or/openai/gpt-5.6-luna:floor[1m]" },
  { id: "or", model: "or/z-ai/glm-5.2:free" },
  { id: "or", model: "or/nvidia/nemotron-3-ultra-550b-a55b:free" },
  { id: "or", model: "or/stealth/ox-alpha" },
  { id: "nv", model: "nv/nvidia/nemotron-3-ultra-550b-a55b" },
  { id: "gmi", model: "gmi/MiniMaxAI/MiniMax-M3" },
  { id: "gmi", model: "gmi/MiniMaxAI/MiniMax-M2.7" },
  { id: "cm", model: "cm/deepseek/deepseek-v4.1-flash" },
  { id: "cm", model: "cm/meituan/LongCat-2.0:free" },
  { id: "cm", model: "cm/poolside/laguna-s-2.1-free" },
  { id: "r4", model: "r4/deepseek-v4.1-flash" },
];

/** The default channel leads, so the "recommended" badge points where `auto` resolves. */
export const HEALTH_PRIORITY: string[] = ["cm", "qw", "og", "or"];
