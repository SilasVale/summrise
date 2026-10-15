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
