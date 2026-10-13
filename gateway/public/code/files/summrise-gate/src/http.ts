/**
 * http.ts — the REQUEST/RESPONSE half of the console worker's HTTP helpers.
 *
 * **THE CORS DECISIONS ARE RUST NOW, AND THIS FILE DOES NOT CARRY THEM (2026-10-13).** `cors-logic.ts` is GENERATED
 * from `gateway/wasm/src/cors.rs` (`render_typescript`), the freshness test there is a byte comparison, and this
 * module re-exports it so every existing `from "./http.ts"` import keeps working. What is left here is extraction
 * and assembly — an `Origin` and a host out of a `Request`, and a `Response` rebuilt with the headers the emitted
 * rules chose — which is a platform call that decides nothing.
 *
 * WHY AN EMISSION RATHER THAN A DELETION OR A ROUTE: the status page reads these predicates and MUST NOT ASK THE
 * WORKER (`plugins.test.mjs`: *"the worker must never be asked"*), and `npm test` cannot reach `/__decision` at all
 * (`helpers.mjs`'s `WASM_GATE` stub answers a fixed model list) — so a twin deleted outright would delete the only
 * implementation the tests can reach, and an `async` route cannot serve a predicate the response path calls
 * synchronously. The emitted module is JavaScript, so the suite keeps running it, and there is no third copy: **the
 * emission IS the Rust's output.**
 *
 * **AND `errorTypeForStatus` WAS DELETED IN THE SAME LANDING, BECAUSE IT WAS DEAD.** Grepped across every tracked
 * source (`.ts`/`.tsx`/`.js`/`.mjs`/`.cjs`/`.rs`), its only occurrence was its own definition — nothing under
 * `gateway/src`, `gateway/test` or anywhere else called it. Its subject is live in the Rust
 * (`responses.rs`'s `error_type_for_status`, called from three places in that module), so the TypeScript was a twin
 * nothing reached: the same measurement that found `grants.ts` and `store/regkeys.ts`.
 */

import {
  ALLOWED_ORIGINS,
  CORS_HEADERS,
  allowedOrigins as allowedOriginsOf,
  corsHeadersFor as corsHeadersForOrigin,
  isAllowedOrigin as isAllowedOriginOf,
  isLoopbackHost,
  isLoopbackOrigin,
  stampCors as stampCorsOn,
} from "./cors-logic.ts";

// The emitted names every existing importer reaches for, re-exported so `from "./http.ts"` keeps working.
export { ALLOWED_ORIGINS, CORS_HEADERS, isLoopbackHost, isLoopbackOrigin };

/** The one variable the origin rules read off an `env`. */
interface CorsEnv {
  CONSOLE_ORIGINS?: string;
}

/** THE ORIGINS THIS DEPLOYMENT ALLOWS — the `env`-shaped spelling of the emitted predicate. The decision (the
 *  default set, the split/trim/filter, and the empty-configuration fallback) is `cors-logic.ts`'s. */
export function allowedOrigins(env?: CorsEnv | null): ReadonlySet<string> {
  return allowedOriginsOf(env?.CONSOLE_ORIGINS);
}

/** Allowlist first, then the loopback PAIR (an Origin is not enough — the request's own host must be loopback too,
 *  audit P2). Same decision as above; this is only the shape the console's callers already pass. */
export function isAllowedOrigin(
  origin: string,
  requestHost?: string,
  env?: CorsEnv | null,
): boolean {
  return isAllowedOriginOf(origin, requestHost, env?.CONSOLE_ORIGINS);
}

function requestOrigin(request?: Request | null): string {
  try {
    return request?.headers?.get?.("origin") || "";
  } catch {
    return "";
  }
}

function requestHost(request?: Request | null): string {
  try {
    return new URL(request?.url || "").hostname;
  } catch {
    return "";
  }
}

/** Per-request CORS headers: reflect-if-allowlisted + Vary, else no ACAO. */
export function corsHeadersFor(
  request?: Request | null,
  env?: CorsEnv | null,
): Record<string, string> {
  return corsHeadersForOrigin(requestOrigin(request), requestHost(request), env?.CONSOLE_ORIGINS);
}

/** Stamp (or strip) ACAO on a mutable Headers object, per request origin. */
export function stampCors(
  request: Request | null | undefined,
  headers: Headers,
  env?: CorsEnv | null,
): void {
  stampCorsOn(
    headers,
    request ? requestOrigin(request) : "",
    requestHost(request),
    env?.CONSOLE_ORIGINS,
  );
}

/**
 * Rebuild an already-built response with per-request CORS (reflect-if-allowlisted, Vary: Origin, no ACAO
 * otherwise). **THE ONE CONDITION LEFT IN THIS FILE, AND IT IS THE RUNTIME'S RATHER THAN THE PRODUCT'S**:
 * WebSocket upgrades (101 / webSocket) carry no mutable headers and a `Response` cannot be rebuilt around them —
 * the Rust needs no such branch because its upgrade path returns before `with_cors` (`lib.rs`, which has no 101
 * case either).
 */
export function withCors(
  request: Request | null | undefined,
  response: Response,
  env?: CorsEnv | null,
): Response {
  const r = response as Response & { webSocket?: unknown };
  if (r.status === 101 || r.webSocket) return response;
  const headers = new Headers(response.headers);
  stampCors(request, headers, env);
  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers,
  });
}

export function jsonOk(data: any, extraHeaders: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(data), {
    headers: { "Content-Type": "application/json", ...CORS_HEADERS, ...extraHeaders },
  });
}

export function jsonError(
  status: number,
  message: string,
  type: string,
  extraHeaders: Record<string, string> = {},
): Response {
  return new Response(JSON.stringify({ type: "error", error: { type, message } }), {
    status,
    headers: { "Content-Type": "application/json", ...CORS_HEADERS, ...extraHeaders },
  });
}

/** Parse a request body as JSON, tolerating empty/invalid input. */
export async function readJson(request: Request): Promise<any> {
  let body = {};
  try {
    body = await request.json();
  } catch {
    /* empty body */
  }
  return body;
}
