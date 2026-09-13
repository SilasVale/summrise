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

test("byok: the two vocabularies genuinely differ (a merge of one name would be a coincidence)", () => {
  const sameName = BYOK_CHANNELS.filter((c) => c.prefix === c.kind).map((c) => c.kind);
  assert.deepEqual(sameName, ["gmi", "amd"], "only two channels use the same word for both");
});
