// byok — THE single source for the BYOK channel vocabulary (round 190).
//
// WHY THIS EXISTS. Until this file, the same eight channels and their env-var names lived
// in FOUR independently-typed tables: `model-route.ts`'s CHANNEL_KEY_RULES (keyed by routing
// PREFIX), `models-probe.ts`'s BYOK_KEY_FOR_KIND (keyed by route KIND), `translate-vision.ts`'s
// per-key `{key, shape}` table, and `store/users.ts`'s bare USER_KEY_NAMES list. Nothing
// compared them; the values agreed only because whoever typed them was careful. The author of
// the second one wrote the risk down — "a second naming of channels is how two tables drift
// apart" — and then named the wrong file in the pointer, so a reader who followed it found a
// CONSUMER (translate.ts) and no table at all.
//
// WHAT IT IS NOT. It is not a merge of those four tables. `model-route.ts` keys by prefix and
// the other two key by kind, and NOTHING in the repository mapped one vocabulary to the other —
// so the shared thing is the table all four can DERIVE from, and the prefix<->kind pairing is
// the part that did not exist anywhere until this file.
//
// THREE FACETS, EACH ONE EARNED: `userKey`/`envKey` (round 187's measurement) and `shape`
// (found by reading the FOURTH table rather than assuming it from the three that agreed — a
// shared source with only the first two would have forced `translate-vision.ts` to keep its own
// table and the drift class would have survived the fix). `envKey: null` is load-bearing: it
// means "this channel has no environment fallback", which was expressible in exactly one of the
// four tables, so a merge that flattened to `kind -> keyName` would have DELETED a distinction.
//
// PURE DATA, ZERO IMPORTS, deliberately: every one of the four consumers can import it without
// creating a cycle, which is the property that makes the derivation possible at all.
//
// The four derivations (not yet wired — this round only creates the source):
//   model-route.ts      -> CHANNEL_KEY_RULES   : by prefix, {userKey, envKey}
//   models-probe.ts     -> BYOK_KEY_FOR_KIND   : by kind,   userKey
//   translate-vision.ts -> its {key, shape}    : by kind,   {userKey, shape}
//   store/users.ts      -> USER_KEY_NAMES      : the non-null userKey values

/** One BYOK-capable channel, carrying EVERY facet the four consumers need. */
export interface ByokChannel {
  /** Routing prefix used by the router and `model-route.ts` (e.g. "og", "cm"). */
  prefix: string;
  /** Route kind reported by `resolveRoute` and used by the probe/vision tables. */
  kind: string;
  /** The env var holding a USER's key for this channel. Always present. */
  userKey: string;
  /** The env var holding the DEPLOYMENT's key, or null when there is no fallback. */
  envKey: string | null;
  /** Upstream API dialect, for the vision preprocessor's encoder choice. */
  shape: "anthropic" | "openai";
}

export const BYOK_CHANNELS: ByokChannel[] = [
  {
    prefix: "og",
    kind: "opencode",
    userKey: "OPENCODE_GO_API_KEY",
    envKey: "OPENCODE_GO_API_KEY",
    shape: "openai",
  },
  {
    prefix: "ds",
    kind: "deepseek",
    userKey: "DEEPSEEK_API_KEY",
    envKey: "DEEPSEEK_API_KEY",
    shape: "anthropic",
  },
  {
    prefix: "qw",
    kind: "qwen",
    userKey: "QWEN_API_KEY",
    envKey: "QWEN_API_KEY",
    shape: "anthropic",
  },
  {
    prefix: "or",
    kind: "openrouter",
    userKey: "OPENROUTER_API_KEY",
    envKey: "OPENROUTER_API_KEY",
    shape: "anthropic",
  },
  // nv and gmi have NO deployment-level fallback: envKey null is the fact the other three
  // tables could not express, and the reason a naive flatten would lose information.
  { prefix: "nv", kind: "nvidia", userKey: "NVAPI_KEY", envKey: null, shape: "openai" },
  { prefix: "gmi", kind: "gmi", userKey: "GMI_API_KEY", envKey: null, shape: "openai" },
  {
    prefix: "cm",
    kind: "commandgoat",
    userKey: "CMD_API_KEY",
    envKey: "CMD_API_KEY",
    shape: "openai",
  },
  { prefix: "amd", kind: "amd", userKey: "AMD_API_KEY", envKey: "AMD_API_KEY", shape: "anthropic" },
  // r4.codes (2026-09-20). shape "anthropic": its /v1/messages is NATIVE — the
  // route passes the body through untranslated — so the vision preprocessor
  // must encode image blocks the Anthropic way when it does run. It normally
  // does not: r4/deepseek-v4.1-flash is on VISION_CAPABLE_MODELS, which is what
  // keeps the picture out of the preprocessor entirely.
  { prefix: "r4", kind: "r4", userKey: "R4_API_KEY", envKey: "R4_API_KEY", shape: "anthropic" },
];

/** Look up by routing prefix (`model-route.ts`'s vocabulary). */
export function byPrefix(prefix: string): ByokChannel | undefined {
  return BYOK_CHANNELS.find((c) => c.prefix === prefix);
}

/** Look up by route kind (`models-probe.ts` and `translate-vision.ts`'s vocabulary). */
export function byKind(kind: string): ByokChannel | undefined {
  return BYOK_CHANNELS.find((c) => c.kind === kind);
}
