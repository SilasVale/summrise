// The model catalogue as DATA — `store/models.ts`.
//
// Until this existed the catalogue was `MODEL_REGISTRY` in `src/channels.ts`, so
// adding a model meant editing source, rebuilding and redeploying. These tests pin
// the behaviour that makes it a runtime thing instead — and the two ways it could go
// wrong quietly:
//
//   * BARE vs PREFIXED. `ROUTE_INFO[].models` holds bare names ("mimo-v2.5") while
//     the advertised/disabled sets hold full ids ("og/mimo-v2.5"). Comparing them
//     directly matches NOTHING — the same mismatch that made the console set the
//     wrong channel (round 58) and left its default card permanently empty (r65).
//   * ROUTING vs LISTING. A disabled model must stop being USABLE, not just stop
//     being listed, or "retired" would mean "retired for new users only".
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

function makeKV(seed = {}) {
  const kv = new Map(Object.entries(seed));
  const KEYS = {
    async get(k) {
      return kv.has(k) ? kv.get(k) : null;
    },
    async put(k, v) {
      kv.set(k, String(v));
    },
    async delete(k) {
      kv.delete(k);
    },
  };
  return { env: { KEYS }, _kv: kv };
}

const {
  dropModelCache,
  advertisedIds,
  catalogue,
  isAdvertised,
  putCustomModel,
  deleteCustomModel,
  setModelDisabled,
  isBuiltIn,
  customModels,
} = await import("../src/store/models.ts");
const { MODEL_REGISTRY, MODELS } = await import("../src/channels.ts");

/** A fresh isolate per test — the module caches reads in a process-global map. */
function freshEnv(seed = {}) {
  dropModelCache();
  return makeKV(seed).env;
}

const BUILT_IN = MODELS[0].id;
const PREFIX = BUILT_IN.slice(0, BUILT_IN.indexOf("/") + 1);
const BARE = BUILT_IN.slice(BUILT_IN.indexOf("/") + 1);

test("with nothing stored, the catalogue IS the compiled registry", async () => {
  const env = freshEnv();
  assert.deepEqual(
    await advertisedIds(env),
    MODELS.map((m) => m.id),
  );
});

test("a DISABLED built-in leaves the advertised set AND its channel's list", async () => {
  const env = freshEnv();
  await setModelDisabled(env, BUILT_IN, true);

  const ids = await advertisedIds(env);
  assert.ok(!ids.includes(BUILT_IN), "still advertised after being disabled");
  assert.ok(
    !(await isAdvertised(env, BUILT_IN)),
    "isAdvertised still true — routing would not stop",
  );

  // THE BARE-vs-PREFIXED CASE, AND IT NEEDS BOTH HALVES.
  //
  // Asserting only that the DISABLED model is gone is not enough: without the
  // bare->full conversion NOTHING matches, so EVERY model is filtered out and that
  // assertion still passes. (I wrote it that way first and the mutation proved it —
  // the test had the same blind spot as the bug.) Worse, "every model vanishes from
  // its channel list" is exactly the round-65 defect that left the console's default
  // card permanently empty.
  //
  // So: the disabled one is gone AND its siblings are still there.
  const cat = await catalogue(env);
  const route = cat.routes.find((r) => r.prefix === PREFIX);
  assert.ok(route, `no route for ${PREFIX}`);
  assert.ok(!route.models.includes(BARE), `the bare name ${BARE} is still listed under ${PREFIX}`);

  const siblings = MODEL_REGISTRY.filter((m) => m.id.startsWith(PREFIX) && m.id !== BUILT_IN).map(
    (m) => m.id.slice(PREFIX.length),
  );
  assert.ok(
    siblings.length > 0,
    `no sibling model on ${PREFIX} to check against — pick another prefix`,
  );
  for (const sib of siblings) {
    assert.ok(
      route.models.includes(sib),
      `disabling one model also removed ${sib} — the bare->full conversion is wrong, ` +
        `so the filter matches nothing and the channel list empties`,
    );
  }

  // And it can come back.
  await setModelDisabled(env, BUILT_IN, false);
  assert.ok((await advertisedIds(env)).includes(BUILT_IN));
});

test("a disabled model is NOT usable — listing and routing are separate", async () => {
  const env = freshEnv();
  await setModelDisabled(env, BUILT_IN, true);
  // `isAdvertised` is what BOTH gates call (setRoute and isModelUsable), so this is
  // the single assertion that makes "disabled" mean disabled.
  assert.equal(await isAdvertised(env, BUILT_IN), false);
  await setModelDisabled(env, BUILT_IN, false);
  assert.equal(await isAdvertised(env, BUILT_IN), true);
});

test("a custom model appears in the advertised set and under its channel", async () => {
  const env = freshEnv();
  const id = `${PREFIX}console-added-test`;
  await putCustomModel(env, { id, ownedBy: PREFIX.replace("/", "") });

  assert.ok((await advertisedIds(env)).includes(id), "custom model not advertised");
  const route = (await catalogue(env)).routes.find((r) => r.prefix === PREFIX);
  assert.ok(
    route.models.includes("console-added-test"),
    "custom model missing from its channel list",
  );
  assert.equal(isBuiltIn(id), false);
});

test("deleting removes a CUSTOM model and refuses a built-in", async () => {
  const env = freshEnv();
  const id = `${PREFIX}console-added-test`;
  await putCustomModel(env, { id, ownedBy: "x" });

  assert.equal(await deleteCustomModel(env, id), true, "delete reported no removal");
  assert.ok(!(await advertisedIds(env)).includes(id));
  assert.deepEqual(await customModels(env), []);

  // A built-in is never in the custom set, so deleting it is a no-op here — the
  // ROUTE handler turns that case into "disable" instead.
  assert.equal(await deleteCustomModel(env, BUILT_IN), false);
  assert.equal(isBuiltIn(BUILT_IN), true);
});

test("a malformed KV value degrades to an empty catalogue, never a throw", async () => {
  // A catalogue that cannot be parsed must not take the gateway down.
  const env = freshEnv({ "models:custom": "{not json", "models:disabled": "also not json" });
  assert.deepEqual(await customModels(env), []);
  assert.deepEqual(
    await advertisedIds(env),
    MODELS.map((m) => m.id),
  );
});

test("the compiled registry is still the source of truth for built-ins", async () => {
  // The change must not have made the static list advisory: every record keeps its
  // facets, and disabling is the only thing the console can do to one.
  const env = freshEnv();
  for (const m of MODEL_REGISTRY) {
    assert.ok((await advertisedIds(env)).includes(m.id), `${m.id} vanished with no stored state`);
  }
});

// ── THE RUST REGISTRY AND THIS ONE ARE THE SAME TABLE, AND NOTHING COMPARED THEM ─────────────────
//
// `gateway/wasm/src/registry.rs` opens with "THE MODEL REGISTRY, MOVED FROM gateway/src/channels.ts",
// and its 24 entries were — in its own words — "generated once from gateway/src/channels.ts and then
// checked by the corpus". **A CORPUS IS A SNAPSHOT, NOT A COMPARISON**: the fixture pins what the table
// said the day it was recorded, so a model added HERE (the console's table, the one the operator edits)
// would leave the worker advertising a registry the console does not have — the same silent divergence
// the prefix table already has a gate for (`the_known_prefixes_are_the_console_route_table`).
//
// So this compares them FIELD BY FIELD, in the suite that already imports MODEL_REGISTRY, with the
// count guard that keeps it from passing vacuously (a parser that matched nothing would otherwise agree
// with everything).
const RUST_REGISTRY = fileURLToPath(new URL("../wasm/src/registry.rs", import.meta.url));

/** The Rust table's entries, read out of the `ModelSpec { … }` literals. Deliberately a TEXT parse:
 *  the point is to read what the file SAYS, not to compile it. */
function readRustRegistry(source) {
  const body = source.slice(source.indexOf("pub const MODEL_REGISTRY"));
  const entries = [];
  for (const chunk of body.split("ModelSpec {").slice(1)) {
    const entry = chunk.slice(0, chunk.indexOf("\n    },"));
    const field = (name) => {
      const m = new RegExp(`${name}: ([^,\\n]+)`).exec(entry);
      return m ? m[1].trim() : undefined;
    };
    // **`None` IS "ABSENT", NOT `false`** — the Rust carries the optional fields as `Option`, and the
    // three-state distinction is the whole point of comparing them: a flag that is `None` here and
    // `undefined` there is agreement, while `Some(false)` and `undefined` is not.
    const opt = (v) => {
      if (v === undefined || v === "None") return undefined;
      const some = /^Some\((.*)\)$/.exec(v);
      return (some ? some[1] : v).trim().replace(/^"|"$/g, "");
    };
    const flag = (v) => {
      const inner = opt(v);
      return inner === undefined ? undefined : inner === "true";
    };
    const num = (v) => {
      const inner = opt(v);
      return inner === undefined ? undefined : Number(inner);
    };
    entries.push({
      id: opt(field("id")),
      ownedBy: opt(field("owned_by")),
      wire: opt(field("wire")),
      usEgress: flag(field("us_egress")),
      search: flag(field("search")),
      responsesOnly: flag(field("responses_only")),
      // The Rust spells the two enums as `ReasoningMax::Parsed` / `Some("parsed")`; the console's table
      // spells the VALUE. Compare the value, not the path to it.
      reasoningMax: (opt(field("reasoning_max")) ?? "").replace(/^ReasoningMax::/, "").toLowerCase() || undefined,
      reasoningEffort: opt(field("reasoning_effort")),
      contextWindow: num(field("context_window")),
      maxTokens: num(field("max_tokens")),
      probe: flag(field("probe")),
    });
  }
  return entries;
}

test("RUST: registry.rs IS builtin-models' registry, field for field", async () => {
  const source = readFileSync(RUST_REGISTRY, "utf8");
  const rust = readRustRegistry(source);
  // **THE VACUITY GUARD FIRST**: a parser that found nothing agrees with everything.
  assert.equal(rust.length, MODEL_REGISTRY.length, "the Rust table was not parsed");
  assert.ok(rust.length > 20, `parsed only ${rust.length} entries — the field regexes have drifted`);

  const shape = (m) => ({
    id: m.id,
    ownedBy: m.ownedBy,
    wire: m.wire,
    usEgress: m.usEgress,
    search: m.search,
    responsesOnly: m.responsesOnly,
    reasoningMax: m.reasoningMax,
    reasoningEffort: m.reasoningEffort,
    contextWindow: m.contextWindow,
    maxTokens: m.maxTokens,
    probe: m.probe,
  });
  assert.deepEqual(
    rust.map(shape),
    MODEL_REGISTRY.map(shape),
    "gateway/wasm/src/registry.rs and src/channels.ts's MODEL_REGISTRY have drifted — the worker and the console would advertise different tables",
  );
});

// ── THE EMITTED CARDS AND THIS FILE'S DERIVATION ────────────────────────────────────────────────
test("CARDS: the emitted ROUTE_INFO is routeModelsFor's answer, card by card", async () => {
  const { ROUTE_INFO, routeModelsFor } = await import("../src/channels.ts");
  assert.equal(ROUTE_INFO.length, 8, "the card list");
  for (const card of ROUTE_INFO) {
    // The DEFAULT card is the exception on purpose: a bare model name has no prefix to filter by, so its
    // list is explicit — and it is the model `auto` resolves to.
    const expected = card.prefix === "none" ? ["deepseek/deepseek-v4.1-flash"] : routeModelsFor(card.prefix);
    assert.deepEqual(
      card.models,
      expected,
      `${card.prefix}'s card and routeModelsFor disagree — the worker's derivation and the console's have drifted`,
    );
  }
  assert.deepEqual(
    ROUTE_INFO.map((c) => c.prefix),
    ["og/", "or/", "nv/", "gmi/", "qw/", "cm/", "r4/", "none"],
    "the order is the console's own, and `none` is last",
  );
});
