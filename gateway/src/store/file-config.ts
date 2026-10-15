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

// **THE FILE LAYER IS AN EMISSION NOW (2026-10-14)** — `file-config-logic.ts`, GENERATED from
// `gateway/wasm/src/catalogue_file.rs`, whose twin the admin family's 82-case corpus already proves end to end:
// the validator with its eight refusal arms, the merge that gives the file precedence, the declared-key sets.
// What stays here is the HOST half — the `config/models.ts` import and the module-load cache — because the
// document's SOURCE differs by necessity: the console reads its TypeScript edit surface, the worker reads the
// JSON twin the sync script writes from it.
export {
  fileDeclaredKeys,
  mergeLayers,
  parseCatalogueFile,
  type CatalogueFileDeps,
  type CatalogueLayers,
} from "./file-config-logic.ts";
import {
  parseCatalogueFile,
  type CatalogueFileDeps,
  type CatalogueLayers,
} from "./file-config-logic.ts";

/** The file's shape. `config/models.ts` uses `satisfies CatalogueFile`, so the compiler
 *  checks the committed document; the runtime validator then covers what a type cannot —
 *  a routing field smuggled into an override, an inline credential, a bad prefix. */
export interface CatalogueFile {
  providers?: unknown[];
  models?: unknown[];
  overrides?: unknown[];
}

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
    if (
      cachedDeps.parseProvider !== deps.parseProvider ||
      cachedDeps.knownPrefixes !== deps.knownPrefixes
    ) {
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
