// /api/github → controlled GitHub HTTP reverse proxy.
// Route format: /api/github/{web|raw|api|release}/... .
// Only public GitHub hosts are reachable; credentials are never forwarded.

export const config = { runtime: "edge" };

const UPSTREAMS: Record<string, string> = {
  web: "https://github.com",
  raw: "https://raw.githubusercontent.com",
  api: "https://api.github.com",
  release: "https://github.com",
};
const ALLOWED_REDIRECT_HOSTS = new Set([
  "github.com",
  "www.github.com",
  "api.github.com",
  "raw.githubusercontent.com",
  "objects.githubusercontent.com",
  "github-releases.githubusercontent.com",
  "release-assets.githubusercontent.com",
]);
const REQUEST_HEADERS = [
  "accept",
  "accept-encoding",
  "accept-language",
  "if-none-match",
  "if-modified-since",
  "range",
  "user-agent",
];
const RESPONSE_HEADERS = [
  "accept-ranges",
  "cache-control",
  "content-disposition",
  "content-encoding",
  "content-length",
  "content-range",
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

const MAX_REDIRECTS = 5;

// Upstream fetch budget: fail fast instead of hanging a client.
// The HEADER budget (see proxies/README.md:13): it covers waiting for response
// headers ONLY, and the body then streams untimed. Configurable so a test can
// distinguish that from a whole-fetch budget without sleeping 30 s (round 129).
const UPSTREAM_TIMEOUT_MS = Number(
  (globalThis as { process?: { env?: Record<string, string | undefined> } }).process?.env
    ?.SUMMRISE_RELAY_HEADER_TIMEOUT_MS ?? 30000,
);

type Route = { base: string; path: string };

function bad(message: string, status = 400): Response {
  return new Response(JSON.stringify({ error: message }), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

// Exported for direct pins (SOLID Round-21; additive — handler untouched).
export function safePath(value: string): boolean {
  if (!value || !value.startsWith("/")) return false;
  if (value.includes("\\") || value.includes("\0") || value.includes("..")) return false;
  try {
    const decoded = decodeURIComponent(value);
    return decoded.startsWith("/") &&
      !decoded.includes("\\") &&
      !decoded.includes("..") &&
      !/[\0-\x1f]/.test(decoded);
  } catch {
    return false;
  }
}

// Exported for direct pins (SOLID Round-21; additive — handler untouched).
export function parseRoute(value: string | null): Route | null {
  if (!value || !safePath(value)) return null;
  const slash = value.indexOf("/", 1);
  if (slash < 0) return null;
  const type = value.slice(1, slash);
  const path = value.slice(slash);
  const base = UPSTREAMS[type];
  return base && safePath(path) ? { base, path } : null;
}

function copyRequestHeaders(request: Request): Headers {
  const headers = new Headers();
  for (const name of REQUEST_HEADERS) {
    const value = request.headers.get(name);
    if (value) headers.set(name, value);
  }
  return headers;
}

function copyResponseHeaders(response: Response): Headers {
  const headers = new Headers();
  for (const name of RESPONSE_HEADERS) {
    const value = response.headers.get(name);
    if (value) headers.set(name, value);
  }
  return headers;
}

// Exported for direct pins (SOLID Round-21; additive — handler untouched).
export function redirectTarget(response: Response, currentUrl: URL): URL | null {
  const location = response.headers.get("location");
  if (!location) return null;
  try {
    const target = new URL(location, currentUrl);
    return target.protocol === "https:" && ALLOWED_REDIRECT_HOSTS.has(target.hostname)
      ? target
      : null;
  } catch {
    return null;
  }
}

export default async function handler(request: Request): Promise<Response> {
  if (request.method === "OPTIONS") {
    return new Response(null, {
      headers: {
        "access-control-allow-origin": "*",
        "access-control-allow-methods": "GET, HEAD, OPTIONS",
        "access-control-allow-headers": "Accept, Range, If-None-Match, If-Modified-Since",
      },
    });
  }
  if (request.method !== "GET" && request.method !== "HEAD") return bad("method not allowed", 405);

  const requestUrl = new URL(request.url);
  const route = parseRoute(requestUrl.searchParams.get("path"));
  if (!route) return bad("unsupported GitHub route");

  const composed = upstreamUrl(route.base, route.path);
  if (composed.error) return bad(composed.error, 400);
  let upstream = composed.url;
  upstream.search = requestUrl.search;
  upstream.searchParams.delete("path");
  const headers = copyRequestHeaders(request);

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
          redirect: "manual",
          signal: ac.signal,
        });
      } finally {
        clearTimeout(headerTimer);
      }
      const target = redirectTarget(response, upstream);
      if (!target || response.status < 300 || response.status >= 400) {
        // 5xx: GENERIC client text, detail stays in the log (proxies/README.md:13).
        // This branch used to hand the upstream body straight back, so a GitHub
        // 5xx page reached the caller verbatim — the same contract violation the
        // two .js handlers had (round 132) and the same fix.
        if (response.status >= 500) {
          let detail = `upstream ${response.status}`;
          try {
            detail = (await response.text()).slice(0, 500);
          } catch {}
          console.error(`[vercel-github] upstream ${response.status}: ${detail}`);
          return bad("GitHub upstream unavailable", response.status);
        }
        const responseHeaders = copyResponseHeaders(response);
        responseHeaders.set("access-control-allow-origin", "*");
        return new Response(response.body, { status: response.status, headers: responseHeaders });
      }
      if (redirects >= MAX_REDIRECTS) return bad("too many GitHub redirects", 502);
      upstream = target;
    }
  } catch (error) {
    // Never leak internal detail — generic client text, full detail in log.
    console.error(`[vercel-github] upstream error: ${error instanceof Error ? error.stack || error.message : error}`);
    return bad("GitHub upstream unavailable", 502);
  }
}
