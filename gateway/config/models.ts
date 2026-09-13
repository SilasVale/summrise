// THE FILE LAYER of the model catalogue — read ../config/README.md first.
//
// WHY A .ts FILE AND NOT .json. A JSON import needs different syntax in Node
// (`with { type: "json" }`) than in the bundler, so a config document that the TESTS
// import would not be the same document the WORKER imports. A module has no such split —
// and it buys something better than portability: `satisfies` makes the COMPILER check the
// document, so `contextWindows` or an effort the router cannot speak fails `tsc` before
// anything validates it at runtime. A config file that cannot be misspelled is worth more
// than one that is merely parsed.
//
// PRECEDENCE: what this file declares WINS over the console (KV) for the same id or
// prefix. That is what "configured from the file" means, and it is why the console marks
// file-declared entries: an edit made there would be reverted by the next deploy, so the
// panel has to say so rather than let it happen silently.
//
// WHAT MAY BE DECLARED
//   providers[]  a whole custom provider (prefix, baseURL, api, apiKeyEnv, models[]).
//                An INLINE apiKey is refused: a credential must not be committed.
//   models[]     a model on a built-in channel, with its display facets.
//   overrides[]  display facets for a BUILT-IN id. Routing fields (wire, usEgress,
//                search, responsesOnly) are refused BY NAME, exactly as in the console:
//                a file must not become a second, contradictory answer to "where does
//                this request go".
//
// The console hands you this document ready to paste: Models -> Open configuration
// document.
import type { CatalogueFile } from "../src/store/file-config.ts";

export default {
  providers: [],
  models: [],
  overrides: [],
} satisfies CatalogueFile;
