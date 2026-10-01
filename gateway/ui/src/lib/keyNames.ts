/**
 * THE PROVIDER KEYS THE CONSOLE KNOWS ABOUT — one owner, because two had already drifted.
 *
 * The Overview tile and the Keys page each carried their own copy until 2026-09-24: the page listed
 * nine names and the tile eight (no `R4_API_KEY`), so the tile rendered "N/8" while the page it links
 * to rendered "N / 9" for the same account. The tile's copy is what makes this the SECOND time this
 * vocabulary drifted: `gateway/test/byok.test.mjs` records the first, in its own words — "the page
 * still rendered EIGHT rows, because Keys.tsx renders a HARD-CODED KEY_NAMES array that nothing
 * compared to anything", found by the operator rather than by a test. That gate was added and pointed
 * at the page; nothing was pointed at the tile, and the tile was still wrong.
 *
 * WHY IT LIVES IN lib/ RATHER THAN IN A VIEW: a view is not importable by another view without making
 * the page graph a dependency graph, and a fact that two pages must agree on belongs where both can
 * read it. `byok.test.mjs` pins this array against the worker's own `USER_KEY_NAMES`, which is
 * derived from the BYOK channel table, so the console cannot render a key the server does not manage
 * (or miss one it does).
 *
 * ORDER IS PRESENTATION: the tile and the page render in this order. The array is read-only —
 * callers map and filter it, never sort it in place.
 *
 * ── WHY THIS IS STILL TYPESCRIPT, SAID RATHER THAN LEFT TO BE INFERRED (block ③, 2026-09-30) ─────
 *
 * The migration's inventory counts this file's export as LOGIC, and P0's own rule for that word is
 * "computes, parses, derives, validates, formats or decides". A CONSTANT LIST DOES NONE OF THOSE — it
 * is a VOCABULARY, and its whole value is being one declaration that a gate compares against the
 * WORKER's own `USER_KEY_NAMES`. Moving it into the crate would turn a module constant into a call
 * that allocates an array per render, and it would put the list on the far side of a boundary from the
 * test that checks it. So it stays, and this paragraph is the reason rather than an omission.
 */
export const KEY_NAMES = [
  "DEEPSEEK_API_KEY",
  "OPENCODE_GO_API_KEY",
  "QWEN_API_KEY",
  "OPENROUTER_API_KEY",
  "NVAPI_KEY",
  "GMI_API_KEY",
  "CMD_API_KEY",
  "AMD_API_KEY",
  "R4_API_KEY",
];
