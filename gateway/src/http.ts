/**
 * HTTP helpers shared across the gateway: CORS headers, JSON responses,
 * and the readJson() body-parsing helper (replaces the repeated
 * `let body = {}; try { body = await request.json(); } catch {}` pattern).
 * Extracted from index.js (2026-08-12).
 */

// CORS allowlist: the console origins used in this repo —
//   https://ai.saisi.online + https://api.saisi.online (CONSOLE_HOST in wrangler.jsonc),
// plus http(s) loopback for local `wrangler dev`. Any other Origin gets NO
//
// https://dsh.saisi.online WAS HERE UNTIL ROUND 243, and it was here for exactly one reason: the Vale Code Links
// browser extension ran a content script on that host and called this API from it. The extension was removed
// (it shipped nowhere, its feature was off by default, and the half it existed for had already been deleted), so
// the grant went with it — an allowed origin with no consumer is a permission nobody is using.
// Access-Control-Allow-Origin header (default-closed). Non-browser clients
// (Claude Code, curl, gateway server-side) are unaffected by CORS.
// (Mirrors proxies/zen-go-proxy/src/index.js.)
export const ALLOWED_ORIGINS: ReadonlySet<string> = new Set([
  "https://ai.saisi.online",
  "https://api.saisi.online",
]);

/** THE ORIGINS THIS DEPLOYMENT ALLOWS, from configuration when it is given (round 88). The set above is the DEFAULT and
 *  is unchanged; `env.CONSOLE_ORIGINS` (comma-separated) overrides it. The parameter is optional on purpose, and that is
 *  the staging: every existing caller keeps the exact behaviour it had, and the callers that HAVE an `env` adopt it as
 *  they are touched — because this list is a security surface (it is what a browser is allowed to read answers from), and
 *  threading it through every response-stamping path in one commit is how a CORS grant gets widened by accident.
 *
 *  WHY IT EXISTS AT ALL: the gateway's tests must be able to run against a test domain instead of a production one, and
 *  today they cannot — 76 of 916 fail the moment the fixtures stop spelling the real host (round 48's measurement). */
export function allowedOrigins(env?: { CONSOLE_ORIGINS?: string } | null): ReadonlySet<string> {
  const configured = env?.CONSOLE_ORIGINS;
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
  requestHost?: string,
  env?: { CONSOLE_ORIGINS?: string } | null,
): boolean {
  if (!origin) return false;
  if (allowedOrigins(env).has(origin)) return true;
  // Loopback origins are a `wrangler dev` affordance, not a production grant:
  // a request to http://localhost:<port> whose Origin is also loopback is
  // local dev; the SAME Origin arriving at the deployed console host is just
  // a foreign local page and gets NO ACAO. (Pre-fix, production reflected
  // any localhost origin — audit P2.)
  return isLoopbackOrigin(origin) && !!requestHost && isLoopbackHost(requestHost);
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

// Base CORS headers shared by every response. Deliberately NO
// Access-Control-Allow-Origin here — the origin is reflected per request
// (corsHeadersFor / stampCors / withCors below), otherwise a static `*`
// would ride along on every merge of this constant.
export const CORS_HEADERS: Record<string, string> = {
  "Access-Control-Allow-Methods": "GET,POST,OPTIONS,DELETE,PUT",
  "Access-Control-Allow-Headers": "*",
};

/** Per-request CORS headers: reflect-if-allowlisted + Vary, else no ACAO. */
export function corsHeadersFor(
  request?: Request | null,
  env?: { CONSOLE_ORIGINS?: string } | null,
): Record<string, string> {
  const headers: Record<string, string> = { ...CORS_HEADERS };
  const origin = requestOrigin(request);
  if (isAllowedOrigin(origin, requestHost(request), env)) {
    headers["Access-Control-Allow-Origin"] = origin;
    headers["Vary"] = "Origin";
  }
  return headers;
}

/** Stamp (or strip) ACAO on a mutable Headers object, per request origin. */
export function stampCors(
  request: Request | null | undefined,
  headers: Headers,
  env?: { CONSOLE_ORIGINS?: string } | null,
): void {
  const origin = request ? requestOrigin(request) : "";
  if (isAllowedOrigin(origin, requestHost(request), env)) {
    headers.set("Access-Control-Allow-Origin", origin);
    headers.set("Vary", "Origin");
  } else {
    headers.delete("Access-Control-Allow-Origin");
  }
}

/**
 * Rebuild an already-built response with per-request CORS (reflect-if-
 * allowlisted, Vary: Origin, no ACAO otherwise). WebSocket upgrades (101 /
 * webSocket) carry no mutable headers — returned untouched.
 */
export function withCors(
  request: Request | null | undefined,
  response: Response,
  env?: { CONSOLE_ORIGINS?: string } | null,
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

/** The Anthropic error `type` for an upstream HTTP status.
 *
 * ONE definition of a decision that was written out FOUR times across
 * translate.ts (`failStatus === 429 ? "rate_limit_error" : "api_error"`).
 *
 * It is not a cosmetic mapping. Anthropic-protocol clients key their retry and
 * re-auth flows off `error.type`: a 429 answered as a bare `api_error` tells
 * Claude Code to GIVE UP instead of backing off, which is the incident the
 * OpenRouter comment in translate.ts records ("a bare api_error on a 429 told
 * clients to give up instead of backing off").
 *
 * Only 429 is special-cased. Every other status — including 5xx, which the
 * retry layer has already exhausted by the time this runs — is `api_error`,
 * and richer upstream types are preserved separately by the body sniffing in
 * `upstreamBodyErrorResponse`. */
export function errorTypeForStatus(status: number): string {
  return status === 429 ? "rate_limit_error" : "api_error";
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
