// THE FILE LAYER of the model catalogue.
//
// The precedence rule and the validator are the two halves that can lose an operator's
// work — a merge that lets the file silently drop a console edit, or a validator that
// accepts a document the API would refuse — so they are pure functions and are tested
// directly. `mergeLayers` and `parseCatalogueFile` take both sides as arguments precisely
// because a build-time document cannot be varied per test.
import test from "node:test";
import assert from "node:assert/strict";
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
