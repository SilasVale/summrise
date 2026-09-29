// consoleLogic — WHERE THE CONSOLE'S RUST IS, AND WHEN IT IS LOADED.
//
// THIS MODULE IS THE SEAM, and P0 classifies it as BOUNDARY rather than LOGIC for the reason it
// named: it computes nothing, it talks to the platform (a `fetch` in the browser, a file read under
// `node --test`). `docs/superpowers/p0/README.md`: an export that only talks to a browser or network
// API "can never move to Rust; it is the seam the wasm sits behind".
//
// ── IT IS THE PANEL'S SEAM, DELIBERATELY (`agent/resources/panel-react/src/wasm/panelLogic.ts`) ──
//
// The panel learned two things the hard way and this file is both of them, so the console does not
// have to learn them again:
//
//  1. **THE MODULE IS FETCHED AND COMPILED WHILE THE BUNDLE IS STILL DOWNLOADING, AND AWAITED
//     BEFORE THE FIRST RENDER.** `index.html` carries the inline module that does it; `main.tsx`
//     awaits `consoleLogic()` before it mounts anything. That is what makes a migrated function a
//     PLAIN SYNCHRONOUS CALL from inside a component, which is the difference between "the logic
//     can move" and "the logic can move if you first reshape its call site".
//  2. **THE COMPILE IS THE EXPENSIVE HALF, NOT THE FETCH.** A `<link rel="preload">` starts the
//     request at the same moment and still costs +11 ms, because it leaves
//     `WebAssembly.compileStreaming` to the glue — i.e. to the first render, where it competes with
//     React. Started in the inline module, it overlaps the bundle's own download. The alternating
//     A/B behind that number (10 pairs, one browser, median −12.6 ms for the eager load) is in the
//     panel's seam header.
//
// `logic()` is the door a render-path call site uses, and it THROWS rather than falling back to a
// JavaScript copy: a silent second implementation is the thing this migration exists to delete.
//
// ── THE TWO ENVIRONMENTS, ONE ARTIFACT ───────────────────────────────────────────────────────────
//
//   browser      the inline module in `index.html` fetches `/ui_logic_bg.wasm` and compiles it; the
//                glue's own `fetch` of the same path is the fallback for a page that does not carry
//                the inline module. THE URL IS ABSOLUTE because the console is served from the root
//                of `ai.saisi.online` and its routes are client-side (`/models`, `/devices`), so a
//                relative specifier would resolve differently per route.
//   node --test  `readFileSync` of `gateway/ui/public/ui_logic_bg.wasm` and `initSync` — the console
//                suite imports the `.ts` modules directly (Node strips the types), and a test that
//                loaded a different artifact than the operator's browser would be a test of a
//                different program. A test file that calls a migrated function awaits
//                `consoleLogic()` first; `test/lane.test.mjs` is the example.
//
// `import.meta.url` is what tells the two apart: Vite rewrites it in the browser build to the
// bundle's own URL, and leaves it as the module's `file:` URL under `node --test`. The test is on the
// SCHEME, so neither environment can be mistaken for the other.
import * as glue from "./ui_logic.js";

/** The glue's own surface — the wasm module's exports, plus its two initializers. NOT exported: no
 *  other module needs the type, and the console's `exports-check` counts an export nothing imports
 *  as a promise nobody asked for. */
type ConsoleLogic = typeof glue;

let loading: Promise<ConsoleLogic> | null = null;
let ready: ConsoleLogic | null = null;

/**
 * The console's Rust, loading it if this is the first call.
 *
 * IDEMPOTENT AND MEMOIZED: the promise is kept, so a second caller joins the first load instead of
 * fetching again, and a FAILED load is not retried into a second fetch — the promise rejects once
 * and every caller sees that rejection. `main.tsx` catches it so a failed load cannot blank the
 * console; what it does NOT do is fall back to TypeScript.
 */
export function consoleLogic(): Promise<ConsoleLogic> {
  if (!loading) loading = load();
  return loading;
}

/**
 * The console's Rust, SYNCHRONOUSLY — for a call site that runs during render.
 *
 * LEGAL BECAUSE THE MODULE IS ALREADY LOADED: `index.html`'s inline module fetches and compiles it
 * while the bundle downloads and `main.tsx` awaits `consoleLogic()` before it mounts anything. A
 * caller that renders outside that path (a `node --test` file) awaits `consoleLogic()` itself; one
 * that does neither gets a THROW.
 */
export function logic(): ConsoleLogic {
  if (ready) return ready;
  throw new Error(
    "console logic was called before it loaded — main.tsx (and a test file) await consoleLogic() first",
  );
}

async function load(): Promise<ConsoleLogic> {
  const bytes = await fileBytes();
  if (bytes) {
    // `initSync` rather than the async initializer: the bytes are already in hand, and a test that
    // had to await a load would make every migrated call site asynchronous in the tests too.
    glue.initSync({ module: bytes });
    ready = glue;
    return glue;
  }
  // THE SERVED PAGE HANDS US A MODULE THAT IS ALREADY COMPILED — see the header. `initSync` accepts
  // a compiled `WebAssembly.Module` and skips the compile, which is the whole point: the compile is
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
  const holder = globalThis as { __consoleLogicModule?: Promise<WebAssembly.Module> };
  return holder.__consoleLogicModule ?? null;
}

/**
 * The artifact, read off disk — ONLY when this module is running under Node.
 *
 * THE PATH IS BUILT FROM `import.meta.url` BY STRING, deliberately, and not with
 * `new URL("../../public/…", import.meta.url)`: Vite recognizes that literal pattern as an ASSET
 * REFERENCE and would emit a second copy of the wasm into the console's build output. A string it
 * cannot see is a file it will not copy. (The panel's seam carries the same note for the same
 * reason; both were found by reading the build output rather than by guessing.)
 */
async function fileBytes(): Promise<Uint8Array | null> {
  const here = import.meta.url;
  if (!here.startsWith("file:")) return null;
  const dir = here.slice(0, here.lastIndexOf("/") + 1);
  // THE SPECIFIER IS BUILT AT RUNTIME, and both halves of that are deliberate. Written as the
  // literal `"node:fs"` it is (a) a module the BUNDLER tries to resolve for the browser, which it
  // externalizes with a warning, and (b) a module the COMPILER tries to resolve for types, which it
  // cannot — this app's `tsconfig.app.json` pins `types: ["vite/client"]`, so Node's are not in
  // scope, on purpose. A string neither tool can see is a module neither of them touches, and at
  // runtime Node resolves it normally.
  const spec = ["node", "fs"].join(":");
  const { readFileSync } = (await import(/* @vite-ignore */ spec)) as {
    readFileSync: (path: URL) => Uint8Array;
  };
  return readFileSync(new URL(`${dir}../../public/ui_logic_bg.wasm`));
}

/** The served artifact's URL. ABSOLUTE, because the console's routes are client-side. */
function wasmHref(): string {
  return "/ui_logic_bg.wasm";
}
