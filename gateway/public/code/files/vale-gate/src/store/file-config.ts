// THE FILE LAYER of the model catalogue — see ../config/README.md.
//
// WHY A FILE AT ALL. Everything an operator could configure lived in KV, reachable only
// through the console: it could not be reviewed in a pull request, diffed, copied to
// another deployment, or restored from git. A deployment's model catalogue is exactly the
// kind of thing that should be reviewable before it takes effect, so this layer exists —
// and it is deliberately DEPLOY-TIME ONLY. Nothing here is written at runtime; the file is
// applied by `wrangler deploy`.
//
// WHY IT FAILS AT MODULE LOAD. This module parses the file ONCE, with the SAME validators
// the admin routes use. A typo therefore breaks `wrangler deploy` — the strongest available
// place to fail, because the alternative is a model that the catalogue advertises and the
// upstream rejects, discovered by a user.
//
// WHY THE PRECEDENCE IS A PURE FUNCTION. `mergeLayers` is the whole rule — the file wins
// per id/prefix, the KV layer supplies everything else — and it is separated from the
// import so it can be tested with both sides supplied. A build-time JSON import cannot be
// varied per test, and the precedence is the part that can silently lose an operator's work.
import raw from "../../config/models.ts";
import type { ProviderSpec } from "./providers.ts";
import type { ModelSpec } from "../channels.ts";
import type { FacetOverride } from "./models.ts";

/** The file's shape. `config/models.ts` uses `satisfies CatalogueFile`, so the compiler
 *  checks the committed document; the runtime validator then covers what a type cannot —
 *  a routing field smuggled into an override, an inline credential, a bad prefix. */
export interface CatalogueFile {
  providers?: unknown[];
  models?: unknown[];
  overrides?: unknown[];
}

/** The three layers this file may declare, normalized and validated. */
export interface CatalogueLayers {
  providers: ProviderSpec[];
  models: ModelSpec[];
  overrides: FacetOverride[];
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
  parseProvider: (raw: unknown) => { spec?: ProviderSpec; error?: string };
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
export function parseCatalogueFile(rawDoc: unknown, deps: CatalogueFileDeps): CatalogueLayers {
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
    out.models.push(m as unknown as ModelSpec);
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
      ...(o.reasoningEffort
        ? { reasoningEffort: String(o.reasoningEffort) as FacetOverride["reasoningEffort"] }
        : {}),
    });
  }

  return out;
}

/**
 * THE PRECEDENCE RULE, as a pure function: the file wins per id/prefix, KV supplies the
 * rest. Separated from the import deliberately — this is the part that can silently
 * discard an operator's work, and it has to be testable with both sides supplied.
 *
 * @param file - entries declared in the committed document.
 * @param kv - entries the console wrote.
 * @param keyOf - how to identify an entry (prefix for providers, id for models/overrides).
 * @returns the KV entries that survive, followed by the file's.
 */
export function mergeLayers<T>(file: T[], kv: T[], keyOf: (t: T) => string): T[] {
  const declared = new Set(file.map(keyOf));
  return [...kv.filter((k) => !declared.has(keyOf(k))), ...file];
}

/** Ids/prefixes THIS FILE declares — what the console must mark as not-locally-editable. */
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

/**
 * The validated document, parsed at MODULE LOAD — so a malformed file fails the deploy.
 * `knownPrefixes` is read from the registry by the caller (`store/providers.ts` exposes
 * it) to keep this module free of a channel-table import cycle.
 */
let cached: CatalogueLayers | null = null;
let cachedDeps: CatalogueFileDeps | null = null;

export function loadCatalogueFile(deps: CatalogueFileDeps): CatalogueLayers {
  // D14 (round-178): the precondition this cache rests on is now CHECKED, not
  // documented. The cache is shared by every caller, so callers passing different
  // validators would make the parsed result depend on import order — the
  // poisoning shape the comment below describes. Comparing the two FIELDS (not the
  // object) is what makes this workable: every caller builds a fresh `{...}` literal,
  // so object identity is never equal, while the validator references are module
  // constants. `providers.ts` used to pass a fresh arrow wrapper, which is exactly the
  // shape this check exists to catch, and it now passes the bare function.
  if (cached !== null && cachedDeps !== null) {
    if (cachedDeps.parseProvider !== deps.parseProvider || cachedDeps.knownPrefixes !== deps.knownPrefixes) {
      throw new Error(
        "loadCatalogueFile: called with DIFFERENT validators than the first call — " +
          "the parsed catalogue is cached per isolate, so mixed callers would make the " +
          "result depend on import order. Pass the store-owned validators.",
      );
    }
  }
  // CACHED, AND THE CACHE IS SHARED BY EVERY CALLER — so `deps` must be equivalent
  // wherever it is passed. It is: all SEVEN callers pass the REAL validators
  // (`RESERVED_PREFIXES` + `parseProviderSpec`) — six in src/ (`admin.ts` x3,
  // `models.ts` x2, `providers.ts` x1) and the suite's own deps object. One of the six
  // wraps the validator (`providers.ts`: `(raw) => parseProviderSpec(raw)`) rather than
  // passing the bare reference; behaviourally identical, and harmless only because the
  // cache keys on nothing.
  //
  // THE PRECONDITION IS REAL AND UNCHECKED (round-175): "every caller passes equivalent
  // deps" is what makes one shared cache correct, and NOTHING ENFORCES IT — it is a
  // convention held by seven call sites. A stub here (my first version) would have been
  // a cache-poisoning bug with a nasty shape — a valid document failing, or worse, an
  // INVALID one passing, depending on which module happened to import first. The count
  // in this comment was already stale once (it said "both"), which is the argument for
  // removing the precondition rather than documenting it: if this function closed over
  // the real validators itself, no caller could get it wrong — blocked today only by the
  // import cycle the module notes above (`parseProviderSpec` lives in providers.ts,
  // which imports this file). Recorded as a design item in docs/agents/iteration-
  // coverage.md; until then, a caller passing synthetic deps is a bug.
  if (cached === null) {
    cached = parseCatalogueFile(raw as unknown, deps);
    cachedDeps = deps;
  }
  return cached;
}
