// THROWAWAY probe: an isolate states its own identity, so a gap sweep can ask
// "did the SAME isolate serve the request after G seconds of silence?".
// The design is P3.0's (proxies/spike-wasm-coldstart), reused: a random UUID is
// per-isolate, `requestIndex` starts at 1, `isolateAgeMs` is this isolate's own
// age at request time.
//
// Two things measured while building it, both recorded because they bit:
//   * `crypto.randomUUID()` in GLOBAL scope is refused by the API (code 10021,
//     "generating random values are not allowed within global scope") — minted
//     lazily instead.
//   * `Date.now()` in GLOBAL scope answers 0 in Workers, so a module-scope BORN
//     makes isolateAgeMs read as the whole epoch. It is stamped on the first
//     request of the isolate instead.
//   * `colo` is reported because a request from one box can land in more than
//     one colo, and two colos are two isolate pools — a gap "failure" that is
//     really an anycast hop is not an eviction.
let ISOLATE_ID = null;
let BORN = 0;
let n = 0;

export default {
  async fetch(req) {
    const now = Date.now();
    if (ISOLATE_ID === null) { ISOLATE_ID = crypto.randomUUID(); BORN = now; }
    n++;
    return Response.json({
      isolateId: ISOLATE_ID,
      requestIndex: n,
      isolateAgeMs: now - BORN,
      colo: req.cf?.colo ?? null,
      serverNow: new Date(now).toISOString(),
    });
  },
};
