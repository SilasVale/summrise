// panelLogic — WHERE THE PANEL'S RUST IS, AND WHEN IT IS LOADED.
//
// THIS MODULE IS THE SEAM, and P0 classifies it as BOUNDARY rather than LOGIC for the reason it
// named: it computes nothing, it talks to the platform (a `fetch` in the browser, a file read under
// vitest). `docs/superpowers/p0/README.md`: an export that only talks to a browser or network API
// "can never move to Rust; it is the seam the wasm sits behind".
//
// ── WHAT IT LOADS, AND WHEN ──────────────────────────────────────────────────
//
// The wasm is `agent/resources/panel/panel_logic_bg.wasm` — the panel's own served directory,
// beside panel.js. **`index.html` FETCHES AND COMPILES IT WHILE panel.js IS STILL DOWNLOADING, AND
// `main.tsx` AWAITS IT BEFORE THE FIRST RENDER (2026-09-29)**, which replaced "fetched at the first
// call" for a measured reason.
//
// THE OLD MODEL WAS THE CONSTRAINT THE WHOLE MIGRATION TURNED ON. With the module fetched at the
// first call, a migrated function could not be called DURING RENDER: React's render is synchronous
// and the fetch is not. P2's own record is what that cost — the async seam was exhausted, the
// remaining families were classified by whether they could be re-derived at the data boundary
// instead, and `disambiguateLabels` was written off as unable to pass (`TabBar` numbers `sessions`,
// `DesktopShell` numbers `openTabs`, so no single boundary derivation can serve both).
//
// ── THE MEASUREMENT, AND IT POINTED THE OTHER WAY ────────────────────────────
//
// Three builds of the same tree, the repository's own harness, and — because the panel's first frame
// drifts ~10 ms between sessions on this box, which is the size of the effect — an ALTERNATING A/B
// inside one browser (10 pairs, `/tmp/ab/ab.mjs`). The number is the first child of `#root`, NOT
// first-contentful-paint: `main.tsx` starts the decorative particle field before React mounts, so
// FCP is the canvas and it fires whether or not a component has rendered (measured: FCP 88 ms while
// the module was still in flight).
//
//     lazy (first call), same session   median 141.4 ms   mean 142.4   n=10
//     inline + await before render      median 128.6 ms   mean 126.7   n=10
//     paired difference (inline − lazy) median −12.6 ms   mean −15.6  — negative in 10 of 10 pairs
//
// **THE EAGER LOAD IS FASTER, WHICH IS NOT THE OBVIOUS ANSWER.** A `<link rel="preload">` alone was
// measured too and cost +11 ms (11 runs each, median 133.8 → 145.0): it starts the fetch at the same
// moment but leaves `WebAssembly.compileStreaming` to the glue, i.e. to the moment `main.tsx` runs,
// where a 62 KB module's compile competes with React's first render. The inline module in
// `index.html` compiles it during the bundle's own download instead. Both variants fetch the module
// EXACTLY ONCE (the request count is 1 in every run of both).
//
// `logic()` below is the door a render-path call site uses, and it THROWS rather than falling back
// to a JavaScript copy: a silent second implementation is the thing this migration exists to delete.
// Under vitest `src/test-setup.ts` awaits the same bytes off disk and `initSync`s them, so a
// component test needs no `beforeAll` and cannot measure a different program than the operator runs.
//
// ── THE TWO ENVIRONMENTS, ONE ARTIFACT ───────────────────────────────────────
//
// vitest and the browser load THE SAME BYTES from THE SAME FILE, by the two doors each one has:
//
//   browser  `index.html`'s inline module fetches `<panel.js's own directory>/panel_logic_bg.wasm`
//            and compiles it; the glue's own `fetch` of the same URL is the fallback for a page that
//            does not carry the inline module (a harness), and it is what the `?v=…` content hash —
//            stamped by `web/panel.rs` on panel.js AND on the inline module's URL — protects against
//            a stale module in a browser cache.
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
let ready: PanelLogic | null = null;

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

/**
 * The panel's Rust, SYNCHRONOUSLY — for a call site that runs during render.
 *
 * LEGAL BECAUSE THE MODULE IS ALREADY LOADED: `index.html` preloads the artifact and `main.tsx`
 * awaits `panelLogic()` before it mounts anything, and `src/test-setup.ts` awaits it before any test
 * body runs. A caller that renders outside both paths gets a THROW — never a JavaScript fallback,
 * which would be a second implementation of the same rule and the exact drift this migration exists
 * to delete.
 */
export function logic(): PanelLogic {
  if (ready) return ready;
  throw new Error(
    "panel logic was called before it loaded — main.tsx (and test-setup.ts) await panelLogic() first",
  );
}

async function load(): Promise<PanelLogic> {
  const bytes = await fileBytes();
  if (bytes) {
    // `initSync` rather than the async initializer: the bytes are already in hand, and a test that
    // had to await a load would make every migrated call site asynchronous in the tests too.
    glue.initSync({ module: bytes });
    ready = glue;
    return glue;
  }
  // THE SERVED PAGE HANDS US A MODULE THAT IS ALREADY COMPILED — `index.html`'s inline module
  // fetches and `WebAssembly.compileStreaming`s the artifact while panel.js is still downloading, so
  // what is awaited here has been in flight since before this bundle executed. `initSync` accepts a
  // compiled `WebAssembly.Module` and skips the compile, which is the whole point: the compile is
  // main-thread work and it must not land on the first render.
  const precompiled = precompiledModule();
  if (precompiled) {
    glue.initSync({ module: await precompiled });
    ready = glue;
    return glue;
  }
  // THE OBJECT FORM, NOT THE BARE URL: wasm-bindgen 0.2.129 prints "using deprecated parameters
  // for the initialization function; pass a single object instead" for the positional one, and it
  // printed it in the console of every page that took this fallback path (measured in the console's
  // own render smoke). The deprecated form still works; a warning nobody can act on is still noise.
  await glue.default({ module_or_path: wasmHref() });
  ready = glue;
  return glue;
}

/** `index.html`'s inline module, when this is the served page rather than a test. */
function precompiledModule(): Promise<WebAssembly.Module> | null {
  const holder = globalThis as { __panelLogicModule?: Promise<WebAssembly.Module> };
  return holder.__panelLogicModule ?? null;
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
