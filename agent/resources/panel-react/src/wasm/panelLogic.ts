// panelLogic — WHERE THE PANEL'S RUST IS, AND WHEN IT IS LOADED.
//
// THIS MODULE IS THE SEAM, and P0 classifies it as BOUNDARY rather than LOGIC for the reason it
// named: it computes nothing, it talks to the platform (a `fetch` in the browser, a file read under
// vitest). `docs/superpowers/p0/README.md`: an export that only talks to a browser or network API
// "can never move to Rust; it is the seam the wasm sits behind".
//
// ── WHAT IT LOADS, AND WHY IT IS A SEPARATE FILE ─────────────────────────────
//
// The wasm is `agent/resources/panel/panel_logic_bg.wasm` — the panel's own served directory,
// beside panel.js — and it is fetched at the FIRST CALL, not at page load. That is criterion ③ of
// the migration plan: the wasm is not in the first-load payload and it does not block the page.
// The price of that is stated plainly here because it is the constraint the whole migration turns
// on: **a migrated function cannot be called synchronously during render** until something is
// loaded, so a move is only free where the call site is already asynchronous — the data boundary
// (`useDeviceRead`'s fold, an SSE handler, an event handler). `lib/archive.ts`'s header records
// what that leaves behind and why.
//
// ── THE TWO ENVIRONMENTS, ONE ARTIFACT ───────────────────────────────────────
//
// vitest and the browser load THE SAME BYTES from THE SAME FILE, by the two doors each one has:
//
//   browser  `fetch(<panel.js's own directory>/panel_logic_bg.wasm)` — the URL is derived from the
//            bundle's script element, so `/panel/` and `/desktop/` both work, and the agent's
//            content hash (`?v=…`, which `web/panel.rs` stamps on panel.js) rides along so a panel
//            rebuild cannot leave a stale wasm in a browser cache.
//   vitest   `readFileSync` of the same path, and `initSync` — vitest has no server to fetch from,
//            and a test that could not load the artifact would be a test of a DIFFERENT program
//            than the one the operator runs. That is the failure this branch exists to avoid.
//
// `import.meta.url` is what tells the two apart: Vite rewrites it in the browser build to the
// bundle's own URL (`http://host/panel/panel.js`), and leaves it as the module's `file:` URL under
// vitest. The test is on the SCHEME, so neither environment can be mistaken for the other.
import * as glue from "./panel_logic.js";

/** The glue's own surface — the wasm module's exports, plus its two initializers. NOT exported: no
 *  other module needs the type, and `exports-check` counts an export nothing imports as a promise
 *  nobody asked for. */
type PanelLogic = typeof glue;

let loading: Promise<PanelLogic> | null = null;

/**
 * The panel's Rust, loading it if this is the first call.
 *
 * IDEMPOTENT AND MEMOIZED: the promise is kept, so a second caller joins the first load instead of
 * fetching again, and a FAILED load is not retried into a second fetch — the promise rejects once
 * and every caller sees that rejection.
 */
export function panelLogic(): Promise<PanelLogic> {
  if (!loading) loading = load();
  return loading;
}

async function load(): Promise<PanelLogic> {
  const bytes = await fileBytes();
  if (bytes) {
    // `initSync` rather than the async initializer: the bytes are already in hand, and a test that
    // had to await a load would make every migrated call site asynchronous in the tests too.
    glue.initSync({ module: bytes });
    return glue;
  }
  await glue.default(wasmHref());
  return glue;
}

/**
 * The artifact, read off disk — ONLY when this module is running under Node (vitest, vite-node).
 *
 * THE PATH IS BUILT FROM `import.meta.url` BY STRING, deliberately, and not with
 * `new URL("../../../panel/…", import.meta.url)`: Vite recognizes that literal pattern as an ASSET
 * REFERENCE and would emit a second copy of the wasm into the panel's build output. A string it
 * cannot see is a file it will not copy.
 */
async function fileBytes(): Promise<Uint8Array | null> {
  const here = import.meta.url;
  if (!here.startsWith("file:")) return null;
  const dir = here.slice(0, here.lastIndexOf("/") + 1);
  const { readFileSync } = await import(/* @vite-ignore */ "node:fs");
  return readFileSync(new URL(`${dir}../../../panel/panel_logic_bg.wasm`));
}

/** The served artifact's URL: panel.js's own directory, and panel.js's own cache key. */
function wasmHref(): string {
  const el = document.querySelector('script[type="module"][src*="panel.js"]');
  const src = (el as HTMLScriptElement | null)?.src ?? new URL("panel.js", document.baseURI).href;
  const stamp = src.includes("?") ? src.slice(src.indexOf("?")) : "";
  return `${src.replace(/[?#].*$/, "").replace(/panel\.js$/, "panel_logic_bg.wasm")}${stamp}`;
}
