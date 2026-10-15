// THE FILE LAYER of the model catalogue.
//
// The precedence rule and the validator are the two halves that can lose an operator's
// work — a merge that lets the file silently drop a console edit, or a validator that
// accepts a document the API would refuse — so they are pure functions and are tested
// directly. `mergeLayers` and `parseCatalogueFile` take both sides as arguments precisely
// because a build-time document cannot be varied per test.
import test from "node:test";
import { execFileSync } from "node:child_process";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { parseCatalogueFile, mergeLayers, loadCatalogueFile } from "../src/store/file-config.ts";
import { parseProviderSpec } from "../src/store/providers.ts";
import { RESERVED_PREFIXES } from "../src/channels.ts";

const deps = { knownPrefixes: RESERVED_PREFIXES, parseProvider: parseProviderSpec };
const good = (doc) => parseCatalogueFile(doc, deps);

test("MERGE: the file wins for what it declares, and the console keeps the rest", () => {
  const file = [{ id: "og/from-file", name: "File" }];
  const kv = [
    { id: "og/from-file", name: "Console" },
    { id: "og/only-console", name: "Console" },
  ];
  const merged = mergeLayers(file, kv, (m) => m.id);
  assert.deepEqual(
    merged.map((m) => `${m.id}=${m.name}`),
    ["og/only-console=Console", "og/from-file=File"],
    "the file must win for its own id, and must not remove the console's other entries",
  );
});

test("MERGE: an empty file changes nothing at all", () => {
  // The shipped document is empty, so this is the property that makes the layer safe to
  // land: a deployment that declares nothing behaves exactly as it did before.
  const kv = [{ id: "og/a" }, { id: "og/b" }];
  assert.deepEqual(mergeLayers([], kv, (m) => m.id), kv);
});

test("FILE: the shipped document is empty and valid", () => {
  const layers = loadCatalogueFile(deps);
  assert.deepEqual(layers, { providers: [], models: [], overrides: [] });
});

test("FILE: an inline credential is refused — a key must not be committed", () => {
  // The API accepts an inline apiKey; the FILE must not, because the file's threat model
  // is a git history that lives forever. This is the one place the two writers differ,
  // and it differs on purpose.
  assert.throws(
    () =>
      good({
        providers: [
          {
            prefix: "my/",
            baseURL: "https://api.example.com",
            api: "openai-completions",
            apiKey: "sk-xxxxxxxx",
            models: [{ id: "m" }],
          },
        ],
      }),
    /inline apiKey cannot be committed/,
  );
});

test("FILE: a routing facet on an override is refused BY NAME", () => {
  for (const field of ["wire", "usEgress", "search", "responsesOnly"]) {
    assert.throws(
      () => good({ overrides: [{ id: "og/x", [field]: true }] }),
      new RegExp(field),
      `${field} was accepted into an override`,
    );
  }
});

test("FILE: an unknown reasoning level is refused, naming the field", () => {
  assert.throws(() => good({ overrides: [{ id: "og/x", reasoningEffort: "turbo" }] }), /reasoningEffort/);
});

test("FILE: a provider the API would refuse is refused here too", () => {
  // One validator, two writers: the file delegates to the SAME parser the console's POST
  // goes through, so it cannot express something the API rejects.
  assert.throws(
    () => good({ providers: [{ prefix: "BAD/", baseURL: "https://x.test", api: "openai-completions" }] }),
    /prefix/i,
  );
});

test("FILE: a document that is not an object is refused", () => {
  assert.throws(() => good([]), /must export an object/);
  assert.throws(() => good(null), /must export an object/);
});

test("FILE: an override with no id is refused rather than stored against nothing", () => {
  assert.throws(() => good({ overrides: [{ name: "Nameless" }] }), /id is required/);
});

// ── THE JSON TWIN, WHICH IS THE HALF THE WORKER READS ────────────────────────────────────────────
//
// `gateway/config/models.json` is generated from `config/models.ts` by `scripts/sync-catalogue-json.mjs`, because
// Rust cannot `include_str!` a `.ts` module. **TWO FILES WITH ONE MEANING**, so the drift has to fail a suite —
// and it does, here, by running the script's OWN `--check` rather than a second implementation of the comparison.
const syncScript = fileURLToPath(new URL("../scripts/sync-catalogue-json.mjs", import.meta.url));

test("FILE: the JSON twin is what config/models.ts declares", () => {
  let failed = null;
  try {
    execFileSync(process.execPath, [syncScript, "--check"], { stdio: "pipe" });
  } catch (error) {
    failed = String(error.stderr ?? error.message);
  }
  assert.equal(
    failed,
    null,
    `${failed}\nconfig/models.json is the document the WORKER reads — regenerate it with\n` +
      "  cd gateway && node scripts/sync-catalogue-json.mjs",
  );
});

// ── THE REFUSALS, AS A TABLE — THE OTHER HALF OF THE SAME COMPARISON ─────────────────────────────
//
// **THE SENTENCES ARE THE CONTRACT**: a document the file layer refuses is refused with a sentence the operator
// reads, and the WORKER reads the same document through `parse_catalogue_file` (Rust). Round 35 tried to compare
// the two spellings by grepping `"config/models.ts…"` out of each side and concluded NOTHING — the TypeScript
// builds every message from a `where` variable and the Rust spells it out, so the extractions do not line up.
//
// **THE INSTRUMENT THAT WORKS IS A TABLE OF (bad document -> the sentence), FED TO BOTH SIDES.** This is the
// console's half; the Rust half is
// `catalogue_file.rs::the_refusals_are_the_sentences_the_console_answers`, with the same rows. A wording that
// moves on either side turns one of the two red — which is the gate a grep could not be.
test("REFUSALS: both validators answer the same sentence", () => {
  const cases = [
    ["a document that is not an object", "not an object", /must export an object with providers\/models\/overrides/],
    [
      "a provider with an INLINE key",
      {
        providers: [
          {
            prefix: "acme/",
            baseURL: "https://acme.test",
            api: "openai-completions",
            // The model list is REQUIRED for this arm to be reached: the parser refuses an empty one first.
            models: [{ id: "chat" }],
            apiKey: "sk-inline-0123456789",
          },
        ],
      },
      /providers\[0\]: an inline apiKey cannot be committed — use apiKeyEnv and set the secret with wrangler/,
    ],
    ["a model with no id", { models: [{}] }, /models\[0\]: id is required/],
    [
      "a model id that is not prefix/name",
      { models: [{ id: "!!! nope" }] },
      /models\[0\]\.id must look like "prefix\/name" — got "!!! nope"/,
    ],
    ["an override with no id", { overrides: [{}] }, /overrides\[0\]: id is required/],
    [
      "an override that sets a PINNED routing facet",
      { overrides: [{ id: "og/mimo-v2.5", wire: "x" }] },
      /overrides\[0\]: wire is a ROUTING facet pinned in the channel registry — a file may set name, contextWindow, maxTokens and reasoningEffort only/,
    ],
    [
      "an override with an unknown reasoning level",
      { overrides: [{ id: "og/mimo-v2.5", reasoningEffort: "extreme" }] },
      /overrides\[0\]\.reasoningEffort must be one of low, medium, high, max/,
    ],
  ];
  for (const [why, doc, expected] of cases) {
    let message = null;
    try {
      parseCatalogueFile(doc, deps);
    } catch (error) {
      message = String(error.message);
    }
    assert.notEqual(message, null, `${why}: ACCEPTED, and it must be refused`);
    assert.match(message, expected, `${why}: the sentence moved`);
  }
});
