// /api/git → GitHub smart HTTP reverse proxy.
// Configure Git with url."https://v.saisi.online/api/git/".insteadOf
// "https://github.com/"; repository URLs remain unchanged.

export const config = { runtime: "edge" };

const UPSTREAM = "https://github.com";
const REQUEST_HEADERS = [
  "accept",
  "accept-encoding",
  "content-type",
  "content-length",
  "if-none-match",
  "if-modified-since",
  "user-agent",
  "authorization",
];
const RESPONSE_HEADERS = [
  "cache-control",
  "clear-site-data",
  "content-encoding",
  "content-length",
  "content-type",
  "etag",
  "expires",
  "last-modified",
  "vary",
];
// ── the origin guard, IDENTICAL in git/github/gform ────────────────────────────────
//
// `new URL("//evil.example/x", "https://github.com")` is `https://evil.example/x` — a
// protocol-relative path REPLACES the origin. The per-handler shape checks (`validPath`,
// `safePath`) test the path as a STRING; this tests the RESOLVED origin, which is what decides
// where the request actually goes and what encoding tricks cannot fool. It matters most here:
// `git.ts` forwards the caller's `authorization` upstream, and all three act as egress proxies.
//
// LIVE DEFECT THIS CLOSES (round 120): `GET https://v.saisi.online/api/git//example.com/`
// returned Example Domain's HTML, with the caller's GitHub token attached. The same hole was
// reachable through `/api/github/web//host/` and `/api/gform/...`.
//
// The three copies are byte-identical and PINNED as such by `test/host-escape.test.mjs`: the
// handlers compile to standalone modules (the bundle is flat), so one cannot import another —
// and a guard that exists in three places is exactly how this codebase loses a check on one of
// them. The parity test is what makes "three copies" safe here.
function upstreamUrl(base: string, path: string): { url?: URL; error?: string } {
  let url: URL;
  try {
    url = new URL(path, base);
  } catch {
    return { error: "uncomposable upstream path" };
  }
  const want = new URL(base);
  if (url.origin !== want.origin) {
    return { error: `path escapes the upstream origin (${url.origin} != ${want.origin})` };
  }
  return { url };
}

const MAX_REDIRECTS = 3;

// Upstream fetch budget: fail fast instead of hanging a client.
// The HEADER budget (see proxies/README.md:13): it covers waiting for response
// headers ONLY, and the body then streams untimed. Configurable so a test can
// distinguish that from a whole-fetch budget without sleeping 30 s (round 129).
const UPSTREAM_TIMEOUT_MS = Number(
  (globalThis as { process?: { env?: Record<string, string | undefined> } }).process?.env
    ?.VALE_RELAY_HEADER_TIMEOUT_MS ?? 30000,
);

function errorResponse(message: string, status = 400): Response {
  return new Response(JSON.stringify({ error: message }), {
    status,
    // Errors (404/401/413...) must not be edge-cached either — a cached
    // transient failure would outlive its cause.
    headers: {
      "content-type": "application/json; charset=utf-8",
      "cache-control": "no-store, max-age=0, must-revalidate",
    },
  });
}

// Exported for direct pins (SOLID Round-22; additive — handler untouched).
export function validPath(value: string): boolean {
  if (!value || !value.startsWith("/") || value.includes("\\") || value.includes("\0") || value.includes("..")) return false;
  // `//host` is PROTOCOL-RELATIVE: it passes a "starts with /" test and then replaces the origin
  // in `new URL(path, base)`. The origin assert below is the real guard; this is the shape that
  // should never have been called a rooted path.
  if (value.startsWith("//")) return false;
  try {
    const decoded = decodeURIComponent(value);
    return decoded.startsWith("/") &&
      !decoded.startsWith("//") &&
      !decoded.includes("\\") &&
      !decoded.includes("..") &&
      !/[\0-\x1f]/.test(decoded);
  } catch {
    return false;
  }
}

function requestHeaders(request: Request): Headers {
  const headers = new Headers();
  for (const name of REQUEST_HEADERS) {
    const value = request.headers.get(name);
    if (value) headers.set(name, value);
  }
  return headers;
}

function responseHeaders(response: Response): Headers {
  const headers = new Headers();
  for (const name of RESPONSE_HEADERS) {
    const value = response.headers.get(name);
    if (value) headers.set(name, value);
  }
  return headers;
}

// Exported for direct pins (SOLID Round-22; additive — handler untouched).
export function allowedRedirect(response: Response, current: URL): URL | null {
  const location = response.headers.get("location");
  if (!location) return null;
  try {
    const target = new URL(location, current);
    return target.protocol === "https:" && target.hostname === "github.com" ? target : null;
  } catch {
    return null;
  }
}

export default async function handler(request: Request): Promise<Response> {
  if (request.method !== "GET" && request.method !== "HEAD" && request.method !== "POST") {
    return errorResponse("method not allowed", 405);
  }

  const incoming = new URL(request.url);
  const path = incoming.searchParams.get("path");
  if (!path || !validPath(path)) return errorResponse("invalid GitHub path");

  const composed = upstreamUrl(UPSTREAM, path);
  if (composed.error) return errorResponse(composed.error);
  let upstream = composed.url;
  upstream.search = incoming.search;
  upstream.searchParams.delete("path");
  const headers = requestHeaders(request);

  try {
    for (let redirects = 0; ; redirects += 1) {
      // AbortSignal.timeout starts a clock that keeps RUNNING while the body
      // streams, so a git clone/push (README:108 documents a 725 MB push path) or a
      // release-asset download was cut mid-body at 30 s — while the file's own
      // contract says the budget covers headers only. zen.js/proxy.js have had the
      // correct shape since ff5ad05a; this is that shape.
      const ac = new AbortController();
      const headerTimer = setTimeout(() => ac.abort(), UPSTREAM_TIMEOUT_MS);
      let response: Response;
      try {
        response = await fetch(upstream, {
          method: request.method,
          headers,
          body: request.method === "POST" ? request.body : undefined,
          redirect: "manual",
          signal: ac.signal,
        });
      } finally {
        clearTimeout(headerTimer);
      }
      const target = allowedRedirect(response, upstream);
      if (!target || response.status < 300 || response.status >= 400) {
        const out = responseHeaders(response);
        // Git metadata MUST NOT be edge-cached: a cached info/refs made
        // pushes appear to fail (stale 404s / stale refs for minutes).
        out.set("cache-control", "no-store, max-age=0, must-revalidate");
        return new Response(response.body, {
          status: response.status,
          headers: out,
        });
      }
      if (redirects >= MAX_REDIRECTS) return errorResponse("too many GitHub redirects", 502);
      upstream = target;
    }
  } catch (error) {
    // Never leak internal detail (DNS, TLS, timeout internals) — generic
    // client text, full detail in the function log.
    console.error(`[vercel-git] upstream error: ${error instanceof Error ? error.stack || error.message : error}`);
    return errorResponse("GitHub upstream unavailable", 502);
  }
}
