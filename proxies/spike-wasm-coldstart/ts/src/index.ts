// The TypeScript half of the cold-start instrument. THROWAWAY — the deployed
// worker is deleted after the measurement (see ../README.md).
//
// ISOLATE_START is captured at MODULE EVALUATION, which in a Workers isolate is
// the moment the isolate exists. So `isolateAgeMs` in a response is the whole
// life of that isolate up to that response, and `requestIndex === 1` means THIS
// request created the isolate — its latency is a cold start by definition, with
// nothing to force and nothing to guess.
//
// Route `/echo?msg=&rid=` is the trivial one the question is about; `/work?n=`
// is the same integer loop in both workers, for the CPU-per-request number.

// NOTE, measured twice by the runtime refusing things:
//  * `crypto.randomUUID()` at MODULE SCOPE is disallowed (deploy error 10021:
//    "generating random values are not allowed within global scope");
//  * `Date.now()` at MODULE SCOPE is allowed but returns **0** — measured:
//    a first response reported `isolateAgeMs: 1790601284826`, i.e. now minus
//    zero, because the Workers runtime freezes time during global scope
//    evaluation. So the isolate's birth is stamped on its FIRST REQUEST
//    instead, which is also exactly what `requestIndex === 1` means.
// The wasm half has no such rule — but it is written the same way, so the two
// instruments stay comparable.
let ISOLATE_ID: string | null = null;
let ISOLATE_BORN = 0;
let requestIndex = 0;

export default {
  async fetch(request: Request): Promise<Response> {
    const t0 = Date.now();
    const url = new URL(request.url);
    const mine = ++requestIndex;
    if (ISOLATE_ID === null) {
      ISOLATE_ID = crypto.randomUUID();
      ISOLATE_BORN = t0;
    }
    const isolateAgeMs = t0 - ISOLATE_BORN;

    const msg = url.searchParams.get("msg") ?? "hello";
    const n = Number(url.searchParams.get("n") ?? "0");
    // Identical arithmetic to the Rust half (u64 there, exact integers here).
    let acc = 0;
    for (let i = 0; i < n; i++) acc += (i * 31 + 7) % 1000003;

    const body =
      `{"ok":true,"who":"ts","isolateId":"${ISOLATE_ID}",` +
      `"requestIndex":${mine},"isolateAgeMs":${isolateAgeMs},` +
      `"handlerMs":${Date.now() - t0},"n":${n},"acc":${acc},"msg":"${msg}"}`;

    return new Response(body, {
      headers: { "content-type": "application/json", "cache-control": "no-store" },
    });
  },
};
