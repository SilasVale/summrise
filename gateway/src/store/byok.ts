/**
 * byok.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/byok.rs` (`render_typescript`), which is now the single source. The freshness test
 * in that module is a byte comparison against this file, so a hand edit fails there with the text and one command
 * to fix it:
 *
 *     SUMMRISE_REFRESH_BYOK=1 cargo test --manifest-path gateway/wasm/Cargo.toml byok
 *
 * The comment below is the file's own, carried verbatim: it is the record of why this table exists, and it is not
 * recoverable from the rows.
 *
 * byok — THE single source for the BYOK channel vocabulary (round 190).
 *
 * WHY THIS EXISTS. Until this file, the same eight channels and their env-var names lived
 * in FOUR independently-typed tables: `model-route.ts`'s CHANNEL_KEY_RULES (keyed by routing
 * PREFIX), `models-probe.ts`'s BYOK_KEY_FOR_KIND (keyed by route KIND), `translate-vision.ts`'s
 * per-key `{key, shape}` table, and `store/users.ts`'s bare USER_KEY_NAMES list. Nothing
 * compared them; the values agreed only because whoever typed them was careful. The author of
 * the second one wrote the risk down — "a second naming of channels is how two tables drift
 * apart" — and then named the wrong file in the pointer, so a reader who followed it found a
 * CONSUMER (translate.ts) and no table at all.
 *
 * WHAT IT IS NOT. It is not a merge of those four tables. `model-route.ts` keys by prefix and
 * the other two key by kind, and NOTHING in the repository mapped one vocabulary to the other —
 * so the shared thing is the table all four can DERIVE from, and the prefix<->kind pairing is
 * the part that did not exist anywhere until this file.
 *
 * THREE FACETS, EACH ONE EARNED: `userKey`/`envKey` (round 187's measurement) and `shape`
 * (found by reading the FOURTH table rather than assuming it from the three that agreed — a
 * shared source with only the first two would have forced `translate-vision.ts` to keep its own
 * table and the drift class would have survived the fix). `envKey: null` is load-bearing: it
 * means "this channel has no environment fallback", which was expressible in exactly one of the
 * four tables, so a merge that flattened to `kind -> keyName` would have DELETED a distinction.
 *
 * PURE DATA, ZERO IMPORTS, deliberately: every one of the four consumers can import it without
 * creating a cycle, which is the property that makes the derivation possible at all.
 */

export interface ByokChannel {
  prefix: string;
  kind: string;
  userKey: string;
  envKey: string | null;
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
  { prefix: "r4", kind: "r4", userKey: "R4_API_KEY", envKey: "R4_API_KEY", shape: "anthropic" },
];

export function byPrefix(prefix: string): ByokChannel | undefined {
  return BYOK_CHANNELS.find((c) => c.prefix === prefix);
}

export function byKind(kind: string): ByokChannel | undefined {
  return BYOK_CHANNELS.find((c) => c.kind === kind);
}
