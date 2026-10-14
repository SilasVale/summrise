// sync-route-info-json.mjs — ROUTE_INFO, in the one other language that needs it.
//
// **WHY A TWIN AGAIN.** `channels.ts`'s `ROUTE_INFO` is the console's channel table: a prefix, a backend NAME, a
// description, and the BARE model names that channel serves (derived from `MODEL_REGISTRY` by `routeModelsFor`).
// `/api/admin/public` answers with it — the console's "model routing" section is built from those cards — and the
// worker cannot import a `.ts` module.
//
// The catalogue document already travels this way (`sync-catalogue-json.mjs`): the TypeScript stays the source,
// a JSON twin is generated, the worker compiles it in with `include_str!`, and the drift fails `npm test`.
// **ONE PATTERN, TWO TABLES** — and the alternative (moving the prose into Rust and emitting it back) would have
// moved the operator's editing surface for a display string, which is a bigger decision than this route is worth.
//
//   cd gateway && node scripts/sync-route-info-json.mjs           # write the twin
//   cd gateway && node scripts/sync-route-info-json.mjs --check   # fail if it is stale
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { ROUTE_INFO } from "../src/channels.ts";

// Beside the worker that reads it: `gateway/wasm/src/model_store.rs` includes `../route-info.json`.
const OUT = fileURLToPath(new URL("../wasm/route-info.json", import.meta.url));
const rendered = `${JSON.stringify(ROUTE_INFO, null, 2)}\n`;

if (process.argv.includes("--check")) {
  let committed = null;
  try {
    committed = readFileSync(OUT, "utf8");
  } catch {
    console.error(
      "wasm/route-info.json is MISSING — run `node scripts/sync-route-info-json.mjs` and commit it",
    );
    process.exit(1);
  }
  if (committed !== rendered) {
    console.error(
      "wasm/route-info.json is STALE against channels.ts's ROUTE_INFO — the worker serves those cards.\n" +
        "  run: cd gateway && node scripts/sync-route-info-json.mjs",
    );
    process.exit(1);
  }
  console.error("wasm/route-info.json matches channels.ts's ROUTE_INFO");
  process.exit(0);
}

writeFileSync(OUT, rendered);
console.error(`wrote ${OUT} (${rendered.length} bytes, ${ROUTE_INFO.length} cards)`);
