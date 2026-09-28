// loader.mjs — the two things `worker-build`'s output imports that only exist in a Worker runtime,
// supplied so the BUILT ARTIFACT can be executed on this box.
//
// WHY THIS EXISTS AT ALL. `wrangler dev` cannot start here (workerd needs GLIBC >= 2.32; this box
// has 2.31 — measured, P3.0), and `wrangler tail` cannot either (TLS-reset). So the choice is
// between proving byte-equality against a REIMPLEMENTATION of the route and proving it against the
// module `worker-build` actually produced. This runs the module: `build/index.js` is loaded as-is,
// and only its two runtime imports are replaced.
//
//   1. `cloudflare:workers` — the WorkerEntrypoint base class. Stubbed with an empty class, which is
//      all the generated glue needs (it extends it and assigns `fetch`/`start` onto the prototype).
//   2. `./index_bg.wasm` — the module import. Cloudflare's bundler hands the glue a compiled
//      `WebAssembly.Module`; `verify.mjs` writes a sibling `.mjs` that reads the same bytes and
//      compiles them, so the artifact under test is the built one, byte for byte.
export async function resolve(specifier, context, nextResolve) {
  if (specifier === "cloudflare:workers") {
    return {
      url: "data:text/javascript," + encodeURIComponent("export class WorkerEntrypoint {}"),
      format: "module",
      shortCircuit: true,
    };
  }
  if (specifier.endsWith(".wasm")) {
    // `verify.mjs` writes `<name>.wasm.mjs` beside it before importing the worker.
    return {
      url: new URL(specifier, context.parentURL).href + ".mjs",
      format: "module",
      shortCircuit: true,
    };
  }
  return nextResolve(specifier, context);
}
