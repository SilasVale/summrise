// sync-catalogue-json.mjs — the CATALOGUE FILE, in the one other language that needs it.
//
// **WHY A TWIN EXISTS AT ALL.** `config/models.ts` is the FILE LAYER of the model catalogue: an operator (or the
// console's "Open configuration document" button) pastes a document into it, commits, and deploys. The TypeScript
// reads it with a build-time `import`. Rust cannot: `include_str!` takes bytes, and a `.ts` module is not JSON —
// its keys are unquoted and it ends in `satisfies CatalogueFile`.
//
// So the document is mirrored as JSON for the worker, and **THE TYPE-SCRIPT FILE STAYS THE SOURCE OF TRUTH**: it
// is the operator's edit surface, it is what `tsc` checks against `CatalogueFile`, and it is what the console
// hands out ready to paste. This script only copies it.
//
//   cd gateway && node scripts/sync-catalogue-json.mjs           # write the twin
//   cd gateway && node scripts/sync-catalogue-json.mjs --check   # fail if it is stale
//
// **AND `--check` IS THE HALF THAT MATTERS**, because a twin that drifts is worse than no twin: the worker would
// serve a catalogue the console and the deployed TypeScript do not have. `test/catalogue-twins.test.mjs` runs
// this same comparison inside `npm test`, so the drift fails a suite rather than a deployment.
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import catalogue from "../config/models.ts";

const OUT = fileURLToPath(new URL("../config/models.json", import.meta.url));
/** `JSON.stringify(document, null, 2)` and a trailing newline: byte-stable, so `--check` is a comparison. */
const rendered = `${JSON.stringify(catalogue, null, 2)}\n`;

if (process.argv.includes("--check")) {
  let committed = null;
  try {
    committed = readFileSync(OUT, "utf8");
  } catch {
    console.error(
      "config/models.json is MISSING — run `node scripts/sync-catalogue-json.mjs` and commit it",
    );
    process.exit(1);
  }
  if (committed !== rendered) {
    console.error(
      "config/models.json is STALE against config/models.ts — the worker reads the JSON, so the two must agree.\n" +
        "  run: cd gateway && node scripts/sync-catalogue-json.mjs",
    );
    process.exit(1);
  }
  console.error("config/models.json matches config/models.ts");
  process.exit(0);
}

writeFileSync(OUT, rendered);
console.error(`wrote ${OUT} (${rendered.length} bytes)`);
