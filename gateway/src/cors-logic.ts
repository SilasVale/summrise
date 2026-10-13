/**
 * cors-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/cors.rs` (`render_typescript`), which is now the single source. The freshness test
 * in that module is a byte comparison against this file, so a hand edit fails there with the text and one command
 * to fix it:
 *
 *     SUMMRISE_REFRESH_CORS=1 cargo test --manifest-path gateway/wasm/Cargo.toml cors::emission
 *
 * `gateway/src/http.ts` re-exports this module and keeps only the Request/Response plumbing — pulling an Origin
 * and a host out of a request, and rebuilding a response — because those are platform calls that decide nothing.
 *
 * The comment below is the file's own, carried verbatim: it is the record of why this list is what it is, and none
 * of it is recoverable from the two rows.
 *
 * CORS allowlist: the console origins used in this repo —
 *   https://ai.saisi.online + https://api.saisi.online (CONSOLE_HOST in wrangler.jsonc),
 * plus http(s) loopback for local `wrangler dev`. Any other Origin gets NO
 *
 * https://dsh.saisi.online WAS HERE UNTIL ROUND 243, and it was here for exactly one reason: the Summrise Code
 * Links browser extension ran a content script on that host and called this API from it. The extension was removed
 * (it shipped nowhere, its feature was off by default, and the half it existed for had already been deleted), so
 * the grant went with it — an allowed origin with no consumer is a permission nobody is using.
 * Access-Control-Allow-Origin header (default-closed). Non-browser clients
 * (Claude Code, curl, gateway server-side) are unaffected by CORS.
 * (Mirrors the satellites' ALLOWED_ORIGINS — proxies/zen-us-proxy/worker/src/lib.rs, which zen-go
 * re-exports. The two JavaScript copies this line used to name were deleted on 2026-10-08.)
 */

/** The DEFAULT allowlist, verbatim and in order (`DEFAULT_ALLOWED_ORIGINS` in `cors.rs`).
 *
 *  **THE LITERAL SPELLING IS LOAD-BEARING, AND THAT IS MEASURED RATHER THAN ASSUMED**: two gates read this file as
 *  TEXT — `gateway/test/cors-allowlist.test.mjs` and `agent/tests/proxy_cors_parity.rs` both anchor on the
 *  DECLARATION (`const ALLOWED_ORIGINS`), then `new Set([`, then `])` — so the list is written as a `new Set([ … ])`
 *  OF QUOTED ORIGINS rather than built from a constant, and `cors.rs` carries a test that pins those anchors.
 *  **AND THAT SENTENCE IS WHY THE ANCHOR IS THE DECLARATION RATHER THAN THE NAME**: the first version of this comment
 *  spelled the bare `ALLOWED_ORIGINS` in prose ABOVE the declaration, and `proxy_cors_parity.rs` — whose reader
 *  anchored on the first MENTION of the name — read the COMMENT: its `new Set([ … ])` slice holds no quoted string,
 *  the list read as EMPTY, and the gate failed this file with *"could not read an allowlist — this proves nothing"*.
 *  The reader anchors on the declaration now, and its own test pins that a mention in prose cannot stand in. */
export const ALLOWED_ORIGINS: ReadonlySet<string> = new Set([
  "https://ai.saisi.online",
  "https://api.saisi.online",
]);

/** THE ORIGINS THIS DEPLOYMENT ALLOWS, from configuration when it is given (round 88). The set above is the DEFAULT
 *  and is unchanged; `CONSOLE_ORIGINS` (comma-separated) overrides it. The parameter is optional on purpose, and
 *  that is the staging: every existing caller keeps the exact behaviour it had, and the callers that HAVE an `env`
 *  adopt it as they are touched — because this list is a security surface (it is what a browser is allowed to read
 *  answers from), and threading it through every response-stamping path in one commit is how a CORS grant gets
 *  widened by accident.
 *
 *  WHY IT EXISTS AT ALL: the gateway's tests must be able to run against a test domain instead of a production one,
 *  and today they cannot — 76 of 916 fail the moment the fixtures stop spelling the real host (round 48's
 *  measurement). */
export function allowedOrigins(configured?: string | null): ReadonlySet<string> {
  if (!configured) return ALLOWED_ORIGINS;
  const list = configured
    .split(",")
    .map((o) => o.trim())
    .filter(Boolean);
  return list.length ? new Set(list) : ALLOWED_ORIGINS;
}

export function isLoopbackOrigin(origin: string): boolean {
  try {
    const u = new URL(origin);
    return (
      (u.protocol === "http:" || u.protocol === "https:") &&
      (u.hostname === "localhost" || u.hostname === "127.0.0.1")
    );
  } catch {
    return false;
  }
}

export function isLoopbackHost(hostname: string): boolean {
  return hostname === "localhost" || hostname === "127.0.0.1";
}

export function isAllowedOrigin(
  origin: string,
  requestHost?: string | null,
  configured?: string | null,
): boolean {
  if (!origin) return false;
  if (allowedOrigins(configured).has(origin)) return true;
  // Loopback origins are a `wrangler dev` affordance, not a production grant:
  // a request to http://localhost:<port> whose Origin is also loopback is
  // local dev; the SAME Origin arriving at the deployed console host is just
  // a foreign local page and gets NO ACAO. (Pre-fix, production reflected
  // any localhost origin — audit P2.)
  return isLoopbackOrigin(origin) && !!requestHost && isLoopbackHost(requestHost);
}

// Base CORS headers shared by every response. Deliberately NO
// Access-Control-Allow-Origin here — the origin is reflected per request
// (corsHeadersFor / stampCors / withCors), otherwise a static `*`
// would ride along on every merge of this constant.
export const CORS_HEADERS: Record<string, string> = {
  "Access-Control-Allow-Methods": "GET,POST,OPTIONS,DELETE,PUT",
  "Access-Control-Allow-Headers": "*",
};

/** Per-request CORS headers: reflect-if-allowlisted + Vary, else no ACAO.
 *
 *  **IT TAKES THE ORIGIN AND THE HOST, NOT A `Request`** — this is the Rust signature. Extracting them from a
 *  request is a platform call and stays in `http.ts`; deciding with them is this module's. */
export function corsHeadersFor(
  origin: string,
  requestHost?: string | null,
  configured?: string | null,
): Record<string, string> {
  const headers: Record<string, string> = { ...CORS_HEADERS };
  if (isAllowedOrigin(origin, requestHost, configured)) {
    headers["Access-Control-Allow-Origin"] = origin;
    headers["Vary"] = "Origin";
  }
  return headers;
}

/** Stamp (or strip) ACAO on a mutable Headers object, per request origin.
 *
 *  **THE `else` DELETES, AND THAT IS NOT SYMMETRIC WITH `corsHeadersFor`.** That one starts from a fresh set which
 *  never had an ACAO, so "not allowed" means "do not add one"; this one is handed headers that may already carry
 *  the upstream's, so refusing has to take it away. `Vary` is touched only in the ALLOWED branch, where the
 *  source's `set` REPLACES whatever the upstream sent. */
export function stampCors(
  headers: Headers,
  origin: string,
  requestHost?: string | null,
  configured?: string | null,
): void {
  if (isAllowedOrigin(origin, requestHost, configured)) {
    headers.set("Access-Control-Allow-Origin", origin);
    headers.set("Vary", "Origin");
  } else {
    headers.delete("Access-Control-Allow-Origin");
  }
}
