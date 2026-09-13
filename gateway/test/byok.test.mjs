// The single BYOK source (round 190). Pure data, so the test is direct — and the point of
// the file is that these facets USED TO live in four independently-typed tables with nothing
// comparing them. These assertions are what "compared" means from now on.
import test from "node:test";
import assert from "node:assert/strict";
import { BYOK_CHANNELS, byPrefix, byKind } from "../src/store/byok.ts";

test("byok: every channel carries all four facets, and both lookups resolve them", () => {
  assert.equal(BYOK_CHANNELS.length, 8, "eight BYOK-capable channels");
  for (const c of BYOK_CHANNELS) {
    assert.match(c.userKey, /^[A-Z0-9_]+_API_KEY$|^NVAPI_KEY$/, `userKey shape: ${c.userKey}`);
    assert.ok(c.prefix && c.kind, "both vocabularies are present on every row");
    assert.ok(c.shape === "anthropic" || c.shape === "openai", `shape: ${c.shape}`);
    assert.equal(byPrefix(c.prefix), c, "the prefix vocabulary resolves to the same row");
    assert.equal(byKind(c.kind), c, "the kind vocabulary resolves to the same row");
  }
  // The two vocabularies must not collide or a lookup would be ambiguous.
  assert.equal(new Set(BYOK_CHANNELS.map((c) => c.prefix)).size, 8);
  assert.equal(new Set(BYOK_CHANNELS.map((c) => c.kind)).size, 8);
});

test("byok: NO ENV FALLBACK is expressible — the fact a flattened merge would have deleted", () => {
  const noFallback = BYOK_CHANNELS.filter((c) => c.envKey === null).map((c) => c.kind).sort();
  assert.deepEqual(noFallback, ["gmi", "nvidia"], "exactly nv and gmi lack a deployment key");
  // ...and the other six DO have one, stated positively so a typo cannot pass as a distinction.
  assert.equal(BYOK_CHANNELS.filter((c) => c.envKey !== null).length, 6);
});

test("byok: models-probe DERIVES its table — a re-typed copy fails this", async () => {
  // The acceptance test round 189 named, in the only form that is mutation-proof: the
  // values must not appear in the consumer at all. If someone re-types the table (the exact
  // way the four-copy drift started), this goes red — and it does so without needing the
  // private BYOK_KEY_FOR_KIND exported, which is what round 188 measured as the cost of the
  // alternative. Rewired round 191; the other three consumers are still their own copies.
  const { readFile } = await import("node:fs/promises");
  const src = await readFile(new URL("../src/plugins/models-probe.ts", import.meta.url), "utf8");
  const leaked = BYOK_CHANNELS.map((c) => c.userKey).filter((k) => src.includes(`"${k}"`));
  assert.deepEqual(leaked, [], `models-probe must not re-type these: ${leaked.join(", ")}`);
  assert.match(src, /BYOK_CHANNELS\.map/, "and it must derive them from the shared source");
});

test("byok: USER_KEY_NAMES IS the source — set-equal, and absent from the consumer", async () => {
  // The strongest form, available because this consumer is EXPORTED (round 188 measured that
  // the other two are not). Two assertions, because they catch different things: set-equality
  // fails if the values drift, and the text check fails if someone re-types the list next to
  // the derivation — which is how the four-copy problem started.
  const { USER_KEY_NAMES } = await import("../src/store/users.ts");
  assert.deepEqual(
    [...USER_KEY_NAMES].sort(),
    BYOK_CHANNELS.map((c) => c.userKey).sort(),
    "USER_KEY_NAMES must be exactly the source's userKey values",
  );
  const { readFile } = await import("node:fs/promises");
  const src = await readFile(new URL("../src/store/users.ts", import.meta.url), "utf8");
  const leaked = BYOK_CHANNELS.map((c) => c.userKey).filter((k) => src.includes(`"${k}"`));
  assert.deepEqual(leaked, [], `users.ts must not re-type these: ${leaked.join(", ")}`);
});

test("byok: translate-vision derives the EIGHT and keeps the NINTH", async () => {
  // This is the assertion the round exists for. `byok.ts` covers eight BYOK channels;
  // VISION_BACKENDS needs NINE kinds, because `custom` is a route kind with no BYOK channel
  // behind it. A naive full-table derivation drops it, and the failure is silent —
  // VISION_BACKENDS[kind] would be undefined and every describe against a custom provider
  // would answer "视觉模型后端不支持".
  const { readFile } = await import("node:fs/promises");
  const src = await readFile(new URL("../src/plugins/translate-vision.ts", import.meta.url), "utf8");
  assert.match(src, /BYOK_CHANNELS\.map/, "the eight must be derived");
  assert.match(src, /custom:\s*\{\s*key:\s*""/, "and `custom` must survive, explicitly");
  const leaked = BYOK_CHANNELS.map((c) => c.userKey).filter((k) => src.includes(`"${k}"`));
  assert.deepEqual(leaked, [], `translate-vision must not re-type these: ${leaked.join(", ")}`);
});

test("byok: CHANNEL_KEY_RULES derives — and stays MUTABLE for registerChannelKey", async () => {
  // The last of the four consumers, and the only one on the per-request path. Two things
  // must hold at once: the eight rows come from the source (with nv/gmi still saying
  // envKey null — the distinction round 188 called the deletion criterion), and the object
  // stays a plain mutable one, because registerChannelKey writes into it and the test suite
  // registers `zz-test-ocp` through it.
  const { CHANNEL_KEY_RULES, registerChannelKey } = await import("../src/plugins/model-route.ts");
  for (const c of BYOK_CHANNELS) {
    assert.deepEqual(
      CHANNEL_KEY_RULES[c.prefix],
      { userKey: c.userKey, envKey: c.envKey },
      `prefix ${c.prefix} must come from the source`,
    );
  }
  assert.equal(CHANNEL_KEY_RULES.nv.envKey, null, "and the null survives the derivation");
  assert.equal(CHANNEL_KEY_RULES.gmi.envKey, null);
  const probe = "zz-byok-probe";
  registerChannelKey(probe, { userKey: "ZZ_PROBE_KEY", envKey: null });
  assert.equal(CHANNEL_KEY_RULES[probe].userKey, "ZZ_PROBE_KEY", "still extensible");
  delete CHANNEL_KEY_RULES[probe];
  const { readFile } = await import("node:fs/promises");
  const src = await readFile(new URL("../src/plugins/model-route.ts", import.meta.url), "utf8");
  const leaked = BYOK_CHANNELS.map((c) => c.userKey).filter((k) => src.includes(`"${k}"`));
  assert.deepEqual(leaked, [], `model-route must not re-type these: ${leaked.join(", ")}`);
});

test("byok: the two vocabularies genuinely differ (a merge of one name would be a coincidence)", () => {
  const sameName = BYOK_CHANNELS.filter((c) => c.prefix === c.kind).map((c) => c.kind);
  assert.deepEqual(sameName, ["gmi", "amd"], "only two channels use the same word for both");
});
