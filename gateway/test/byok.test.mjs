// The single BYOK source (round 190). Pure data, so the test is direct — and the point of
// the file is that these facets USED TO live in four independently-typed tables with nothing
// comparing them. These assertions are what "compared" means from now on.
import test from "node:test";
import assert from "node:assert/strict";
import { BYOK_CHANNELS, byPrefix, byKind } from "../src/store/byok.ts";

test("byok: every channel carries all four facets, and both lookups resolve them", () => {
  assert.equal(BYOK_CHANNELS.length, 9, "nine BYOK-capable channels");
  for (const c of BYOK_CHANNELS) {
    assert.match(c.userKey, /^[A-Z0-9_]+_API_KEY$|^NVAPI_KEY$/, `userKey shape: ${c.userKey}`);
    assert.ok(c.prefix && c.kind, "both vocabularies are present on every row");
    assert.ok(c.shape === "anthropic" || c.shape === "openai", `shape: ${c.shape}`);
    assert.equal(byPrefix(c.prefix), c, "the prefix vocabulary resolves to the same row");
    assert.equal(byKind(c.kind), c, "the kind vocabulary resolves to the same row");
  }
  // The two vocabularies must not collide or a lookup would be ambiguous.
  assert.equal(new Set(BYOK_CHANNELS.map((c) => c.prefix)).size, 9);
  assert.equal(new Set(BYOK_CHANNELS.map((c) => c.kind)).size, 9);
});

test("byok: NO ENV FALLBACK is expressible — the fact a flattened merge would have deleted", () => {
  const noFallback = BYOK_CHANNELS.filter((c) => c.envKey === null).map((c) => c.kind).sort();
  assert.deepEqual(noFallback, ["gmi", "nvidia"], "exactly nv and gmi lack a deployment key");
  // ...and the other seven DO have one, stated positively so a typo cannot pass as a distinction.
  assert.equal(BYOK_CHANNELS.filter((c) => c.envKey !== null).length, 7);
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

test("byok: translate-vision derives the NINE and keeps the TENTH", async () => {
  // This is the assertion the round exists for. `byok.ts` covers nine BYOK channels;
  // VISION_BACKENDS needs TEN kinds, because `custom` is a route kind with no BYOK channel
  // behind it. A naive full-table derivation drops it, and the failure is silent —
  // VISION_BACKENDS[kind] would be undefined and every describe against a custom provider
  // would answer "视觉模型后端不支持".
  const { readFile } = await import("node:fs/promises");
  const src = await readFile(new URL("../src/plugins/translate-vision.ts", import.meta.url), "utf8");
  assert.match(src, /BYOK_CHANNELS\.map/, "the nine must be derived");
  assert.match(src, /custom:\s*\{\s*key:\s*""/, "and `custom` must survive, explicitly");
  const leaked = BYOK_CHANNELS.map((c) => c.userKey).filter((k) => src.includes(`"${k}"`));
  assert.deepEqual(leaked, [], `translate-vision must not re-type these: ${leaked.join(", ")}`);
});

test("byok: the console's key status names WHICH credential is in force", async () => {
  // The incident this pins: an operator rotated their opencode key in the console, the request path
  // preferred that (wrong) value, and the page could only say "configured" — there was no way to see
  // that the user key was the one being spent, or that the deployment's key was the thing serving a
  // channel the page called "not configured". `source` is that fact, derived from the same authority
  // the request path uses (BYOK_CHANNELS.envKey), so the console cannot invent a fourth answer.
  const { userKeysStatus } = await import("../src/store/users.ts");
  const { BYOK_CHANNELS } = await import("../src/store/byok.ts");
  const withEnvKey = BYOK_CHANNELS.filter((c) => c.envKey);
  const noEnvKey = BYOK_CHANNELS.filter((c) => !c.envKey);
  const env = Object.fromEntries(withEnvKey.map((c) => [c.envKey, "deployment-secret"]));
  // and a stray secret under the name of a channel that declares NO envKey (nv/gmi)
  for (const c of noEnvKey) env[c.userKey] = "not-a-fallback";

  const userKey = userKeysStatus({ [withEnvKey[0].userKey]: "sk-user" }, env);
  assert.equal(userKey[withEnvKey[0].userKey].source, "user", "the user's own key is in force");
  assert.equal(userKey[withEnvKey[0].userKey].configured, true);

  const deployment = userKeysStatus({}, env);
  for (const c of withEnvKey) {
    assert.equal(
      deployment[c.userKey].source,
      "deployment",
      `${c.kind}: no user key + envKey set → the deployment serves it`,
    );
    assert.equal(deployment[c.userKey].configured, false, "…and it is still not the USER's key");
  }
  for (const c of noEnvKey) {
    assert.equal(
      deployment[c.userKey].source,
      "none",
      `${c.kind}: envKey is null BY DESIGN — a secret of the same name is not a fallback`,
    );
  }
  const neither = userKeysStatus({}, {});
  for (const c of BYOK_CHANNELS) {
    assert.equal(neither[c.userKey].source, "none", `${c.kind}: nothing configured, nothing claimed`);
  }
});

test("byok: CHANNEL_KEY_RULES derives — and stays MUTABLE for registerChannelKey", async () => {

  // The last of the four consumers, and the only one on the per-request path. Two things
  // must hold at once: the nine rows come from the source (with nv/gmi still saying
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
  assert.deepEqual(sameName, ["gmi", "amd", "r4"], "only three channels use the same word for both");
});

test("byok: the console key page renders EVERY managed key — the FIFTH consumer", async () => {
  // THE GATE THE r4/ ROUND ADDED, and it exists because the miss was found by
  // the USER, not by a test. Adding r4/ to BYOK_CHANNELS updated the server
  // (USER_KEY_NAMES is derived from the source) and both i18n dictionaries —
  // and the page still rendered EIGHT rows, because `Keys.tsx` renders a
  // HARD-CODED `KEY_NAMES` array that nothing compared to anything. The console
  // is a fifth consumer of this vocabulary, and the only one that is a UI.
  //
  // Its twin is health.test.mjs's "probe coverage spans every USER_KEY_NAMES
  // entry": a key the console cannot RENDER is a key the operator cannot fix,
  // and a key the probe cannot SPEND leaves `summrise check` blind. Both are the
  // same question asked of a different consumer.
  const { readFile } = await import("node:fs/promises");
  const { USER_KEY_NAMES } = await import("../src/store/users.ts");
  const src = await readFile(new URL("../ui/src/views/Keys.tsx", import.meta.url), "utf8");

  const block = src.match(/const KEY_NAMES = \[([\s\S]*?)\];/);
  assert.ok(block, "Keys.tsx must still declare KEY_NAMES as an array literal");
  const rendered = [...block[1].matchAll(/"([A-Z0-9_]+)"/g)].map((m) => m[1]);
  assert.deepEqual(
    [...rendered].sort(),
    [...USER_KEY_NAMES].sort(),
    "the console's key page must render exactly the managed keys — a key missing " +
      "here is a key the operator has no way to enter",
  );

  // ...and every rendered row must have BOTH a prefix mapping and a dictionary
  // entry in BOTH languages, or the page prints a raw token like
  // "key.R4_API_KEY.backend" where a provider name belongs (the bug the
  // KEY_I18N_PREFIX comment records).
  const i18n = await readFile(new URL("../ui/src/i18n.ts", import.meta.url), "utf8");
  for (const name of rendered) {
    const prefix = src.match(
      new RegExp(`\\b${name}:\\s*"([a-z0-9]+)"`),
    )?.[1];
    assert.ok(prefix, `${name} has no KEY_I18N_PREFIX mapping`);
    for (const field of ["backend", "hint"]) {
      const hits = i18n.split(`"key.${prefix}.${field}"`).length - 1;
      assert.equal(hits, 2, `key.${prefix}.${field} must be declared in BOTH dictionaries`);
    }
  }
});
