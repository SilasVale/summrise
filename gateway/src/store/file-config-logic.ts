/**
 * file-config-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/catalogue_file.rs` (`render_typescript`). The freshness test there is a byte
 * comparison:
 *
 *     SUMMRISE_REFRESH_FILE_CONFIG=1 cargo test --manifest-path gateway/wasm/Cargo.toml catalogue_file::emission
 *
 * `gateway/src/store/file-config.ts` re-exports this module and keeps the host: the document validator's four
 * refusal arms, the `config/models.ts` import, and the module-load cache.
 */

/** The document's shape before validation — every field optional, because the whole point is that it is
 *  validated rather than trusted. */
export interface CatalogueFile {
  providers?: unknown[];
  models?: unknown[];
  overrides?: unknown[];
}

/** The layers a validated document declares.
 *
 *  **GENERIC IN THE ENTRY TYPES, AND THAT IS THE HOST'S SHAPE, NOT A CONVENIENCE**: the console's layers are
 *  `ProviderSpec[]` / `ModelSpec[]` / `FacetOverride[]` — three types this generated leaf may not import (it
 *  would be an edge back into its own host). The defaults make a bare `CatalogueLayers` accept those concrete
 *  types, and the two functions below only ever read `prefix` and `id`. */
export interface CatalogueLayers<P = any, M = any, O = any> {
  providers: P[];
  models: M[];
  overrides: O[];
}

/**
 * **THE FILE WINS, AND THE ORDER OF THE RESULT IS PART OF THE ANSWER**: the KV entries that survive come FIRST
 * and the file's follow, so a list rendered from this shows the console's own entries and then the committed
 * ones. The comparison is by [`FileDeclaredKeys`]-style key — a bare prefix for providers, an id for models and
 * overrides — because `"acme/"` and `"acme"` are the same provider.
 */
export function mergeLayers<T>(file: T[], kv: T[], keyOf: (t: T) => string): T[] {
  const declared = new Set(file.map(keyOf));
  return [...kv.filter((k) => !declared.has(keyOf(k))), ...file];
}

/** Ids/prefixes THIS FILE declares — what the console must mark as not-locally-editable.
 *
 *  **A PROVIDER'S KEY IS ITS BARE PREFIX**: `"acme/"` becomes `"acme"`, which is how the merge compares them. */
export function fileDeclaredKeys(layers: CatalogueLayers): {
  providers: Set<string>;
  models: Set<string>;
  overrides: Set<string>;
} {
  return {
    providers: new Set(layers.providers.map((p) => String(p.prefix ?? "").replace(/\/$/, ""))),
    models: new Set(layers.models.map((m) => m.id)),
    overrides: new Set(layers.overrides.map((o) => o.id)),
  };
}

const ID_SHAPE = /^[a-z0-9]+\/\S+$/;
const EFFORTS = ["low", "medium", "high", "max"];
/** Routing facets a file — like the console — may NOT set on a built-in id. */
const PINNED = ["wire", "usEgress", "search", "responsesOnly"] as const;

/** What `parseCatalogueFile` needs from the stores it validates against.
 *
 *  INJECTED, NOT IMPORTED, and that is a real constraint rather than a style choice: this
 *  module is read BY `providers.ts` and `models.ts` (they merge the file layer into their
 *  own reads), so importing their parser back would be a cycle. Type-only imports are
 *  erased at build time and cost nothing; a value import would be a runtime loop. */
export interface CatalogueFileDeps {
  /** Prefixes a model may belong to — the built-in channels. */
  knownPrefixes: string[];
  /** The SAME validator the console's POST goes through: one validator, two writers. */
  parseProvider: (raw: unknown) => { spec?: any; error?: string };
}

/**
 * Parse and validate a catalogue document.
 *
 * Exported separately from the import above so it can be tested with synthetic input: a
 * build-time import cannot be varied per test, and THIS is the risky half — a validator
 * that accepts a bad document puts a broken model in the catalogue.
 *
 * @param rawDoc - the parsed JSON document (any shape; it is validated here, not trusted).
 * @param deps - the store-owned validators and the known prefixes.
 * @returns the three normalized layers.
 * @throws when the document is not usable, naming the offending entry.
 */
export function parseCatalogueFile<P = any, M = any, O = any>(
  rawDoc: unknown,
  deps: CatalogueFileDeps,
): CatalogueLayers<P, M, O> {
  const { knownPrefixes } = deps;
  if (!rawDoc || typeof rawDoc !== "object" || Array.isArray(rawDoc)) {
    throw new Error("config/models.ts must export an object with providers/models/overrides");
  }
  const doc = rawDoc as CatalogueFile;
  const out: CatalogueLayers = { providers: [], models: [], overrides: [] };

  for (const [i, entry] of (doc.providers ?? []).entries()) {
    // The SAME parser the console's POST goes through, so the file cannot express
    // something the API would refuse — one validator, two writers.
    const { spec, error } = deps.parseProvider(entry);
    if (error || !spec)
      throw new Error(`config/models.ts providers[${i}]: ${error ?? "not a usable provider"}`);
    if (spec.apiKey) {
      throw new Error(
        `config/models.ts providers[${i}]: an inline apiKey cannot be committed — use apiKeyEnv and set the secret with wrangler`,
      );
    }
    out.providers.push(spec);
  }

  for (const [i, entry] of (doc.models ?? []).entries()) {
    const where = `config/models.ts models[${i}]`;
    if (!entry || typeof entry !== "object") throw new Error(`${where} must be an object`);
    const m = entry as Record<string, unknown>;
    const id = String(m.id ?? "").trim();
    if (!id) throw new Error(`${where}: id is required`);
    // A built-in belongs in `overrides`, not here: `models[]` creates a record this
    // deployment owns, and creating one for a built-in is the shadowing the console
    // refuses with a 200-but-override — the file must not have a third answer.
    if (!knownPrefixes.includes(id.slice(0, id.indexOf("/") + 1)) && !ID_SHAPE.test(id)) {
      throw new Error(`${where}.id must look like "prefix/name" — got ${JSON.stringify(id)}`);
    }
    out.models.push(m as unknown as M);
  }

  for (const [i, entry] of (doc.overrides ?? []).entries()) {
    const where = `config/models.ts overrides[${i}]`;
    if (!entry || typeof entry !== "object") throw new Error(`${where} must be an object`);
    const o = entry as Record<string, unknown>;
    const id = String(o.id ?? "").trim();
    if (!id) throw new Error(`${where}: id is required`);
    const pinned = PINNED.filter((f) => o[f] !== undefined);
    if (pinned.length) {
      throw new Error(
        `${where}: ${pinned.join(", ")} is a ROUTING facet pinned in the channel registry — a file may set name, contextWindow, maxTokens and reasoningEffort only`,
      );
    }
    if (o.reasoningEffort !== undefined && !EFFORTS.includes(String(o.reasoningEffort))) {
      throw new Error(`${where}.reasoningEffort must be one of ${EFFORTS.join(", ")}`);
    }
    out.overrides.push({
      id,
      ...(o.name ? { name: String(o.name) } : {}),
      ...(o.contextWindow ? { contextWindow: Number(o.contextWindow) } : {}),
      ...(o.maxTokens ? { maxTokens: Number(o.maxTokens) } : {}),
      ...(o.reasoningEffort ? { reasoningEffort: String(o.reasoningEffort) as any } : {}),
    });
  }

  return out;
}
