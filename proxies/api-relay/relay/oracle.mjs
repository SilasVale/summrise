// The ORACLE for the relay's routing port: the SHIPPING `server/routing.mjs` is driven over a corpus,
// and what it produced is written out as the fixture the Rust test replays.
//
//   cd proxies/api-relay/relay && node oracle.mjs routing > fixtures/routing-corpus.json
//   cd proxies/api-relay/relay && node oracle.mjs headers > fixtures/headers-corpus.json
//
// The direction matters: the expectations come from the code the VPS runs, not from the port. It is
// committed so the fixture is REGENERABLE, and it is a `.mjs` for the reason AGENTS.md's carve-outs
// name — the oracle IS the shipping module, and only a JavaScript runtime can run it.
import proxyHandler from "../api/proxy.js";
import { tokenOk, routeOf } from "../../summrise-relay/relay.mjs";
import zenHandler, { normalizeUpstreamPath } from "../api/zen.js";
import gitHandler from "../api/git.ts";
import githubHandler from "../api/github.ts";
import gformHandler from "../api/gform.ts";
import {
  rewritable,
  rewriteBody,
  setCookieValues,
} from "../api/gform.ts";
import { validPath, allowedRedirect } from "../api/git.ts";
import {
  safePath,
  parseRoute,
  redirectTarget,
  upstreamUrl,
  copyRequestHeaders,
  copyResponseHeaders,
} from "../api/github.ts";
import {
  resolveRoute,
  buildUrl,
  resolveHost,
  forwardHeaders,
  collectResponseHeaders,
} from "../server/routing.mjs";

const MODE = process.argv[2] || "routing";

const ROUTES = [
  { prefix: "/api/zen", handler: "zen" },
  { prefix: "/api/proxy", handler: "proxy" },
  { prefix: "/api/github", handler: "github", pathFromRest: true },
  { prefix: "/api/git", handler: "git", pathFromRest: true },
  { prefix: "/api/gform", handler: "gform", pathFromRest: true },
];

// ── resolveHost ───────────────────────────────────────────────────────────────────────────────────
const headerSets = [
  {},
  { host: "api.saisi.online" },
  { "x-forwarded-host": "agent.saisi.online" },
  { "x-forwarded-host": "agent.saisi.online", host: "127.0.0.1:8081" },
  { "x-forwarded-host": "", host: "127.0.0.1:8081" },
  { "x-forwarded-host": null, host: "api.saisi.online" },
  { host: "" },
  { "x-forwarded-host": ["a", "b"] },
  { host: ["only"] },
  { host: 5 },
  { "x-forwarded-host": "a:8443", host: "b" },
  { host: "localhost:8081" },
  { host: "xn--fiqs8s.example" },
];

// ── resolveRoute / buildUrl ───────────────────────────────────────────────────────────────────────
const urls = [
  "/api/zen", "/api/zen?target=og&path=%2Fv1%2Fresponses", "/api/zen/x", "/api/zen/",
  "/api/proxy", "/api/proxy/x", "/api/proxy?x=1",
  "/api/git", "/api/git/", "/api/git/info/refs?service=git-upload-pack",
  "/api/git/deepseek-ai/x.git/info/refs?service=git-receive-pack",
  "/api/gitx", "/api/gitx/y", "/api/github", "/api/github/", "/api/github/o/r",
  "/api/github/o/r?path=%2Fevil", "/api/githubx/y",
  "/api/gform", "/api/gform/v1/x", "/api/gform?path=%2Fcollide&a=1",
  "/api/gform/v1/x?a=1&path=%2Fcollide&b=2",
  "/API/GIT/x", "/api/", "/", "", "?", "?x=1", "#frag", "/api/git#frag",
  "/api/git/%2Fencoded", "/api/git/a b", "/api/git/a+b", "/api/git/%zz", "/api/git/%2",
  "/api/git/中文", "/api/git/%E4%B8%AD%E6%96%87",
  "/api/git/x?k=", "/api/git/x?=v", "/api/git/x?k", "/api/git/x?k=1&k=2", "/api/git/x?path=a&path=b",
  "/api/git/x?a=%20b", "/api/git/x?a=+b", "/api/git/x?a=%2B", "/api/git/x?a=%2F%2F",
];
const hosts = ["api.saisi.online", "localhost", "127.0.0.1:8081", "a.example:8443", "", "中文.example"];

const cases = [];
let routed = 0;
for (const rawUrl of urls) {
  const hit = resolveRoute(ROUTES, rawUrl);
  for (const host of hosts) {
    cases.push({
      rawUrl,
      host,
      hit: hit ? { prefix: hit.r.prefix, pathname: hit.pathname, search: hit.search } : null,
      url: hit ? buildUrl(hit, host) : null,
    });
    if (hit) routed += 1;
  }
}

// ── the header plumbing ───────────────────────────────────────────────────────────────────────────
// What `forwardHeaders` decides, read the way a caller reads it: `[...headers.entries()]` — lowercased
// names, sorted, multi-values combined. A throw is recorded as a throw, because the `Headers` API
// VALIDATES and the port has to as well.
const rawHeaderSets = [
  { host: "127.0.0.1:8081", ":method": "GET", "x-api-key": "sk-1", cookie: ["a=1", "b=2"] },
  { ":method": "GET", ":path": "/api/git/x", ":authority": "a" },
  { host: "a", "X-Api-Key": "sk-2" },
  { "x-api-key": "sk-3", "X-API-KEY": "sk-4" },
  { "content-type": "application/json", accept: "*/*" },
  { "x-empty": "" },
  { "x-space": "  padded  " },
  { "x-tab": "\tpadded\t" },
  { "x-multi": ["1", "2", "3"] },
  { "x-empty-array": [] },
  { cookie: "single=1" },
  { "set-cookie": ["a=1", "b=2"] },
  { "x-num": 5 },
  { "x-bool": true },
  { "x-null": null },
  { "x-obj": { a: 1 } },
  { "a b": "v" },
  { "": "v" },
  { "x-bad-value": "a\nb" },
  { "x-bad-value-2": "a\u0000b" },
  { "x-unicode": "中" },
  { "x-forwarded-for": "1.2.3.4", host: "b" },
];
const forwardCases = rawHeaderSets.map((raw) => {
  try {
    return { raw, entries: [...forwardHeaders(raw).entries()] };
  } catch (e) {
    return { raw, threw: String(e && e.message ? e.message : e) };
  }
});

const responseHeaderSets = [
  [],
  [["content-type", "text/plain"]],
  [["content-type", "text/html"], ["set-cookie", "NID=1; Path=/"]],
  [["set-cookie", "NID=1; Path=/"], ["set-cookie", "AEC=2; Path=/"]],
  [["content-length", "45"], ["content-encoding", "gzip"], ["content-type", "application/json"]],
  [["Content-Length", "45"]],
  [["x-repeat", "1"], ["x-repeat", "2"]],
  [["etag", "\"abc\""], ["cache-control", "no-store"]],
];
const responseCases = responseHeaderSets.map((pairs) => {
  const h = new Headers();
  for (const [k, v] of pairs) h.append(k, v);
  const out = collectResponseHeaders({ headers: h });
  return {
    pairs,
    entries: [...h.entries()],
    setCookie: typeof h.getSetCookie === "function" ? h.getSetCookie() : [],
    out,
  };
});
// AND ONE REAL `Response`, because that is what the caller passes and the constructor adds headers of
// its own — the node suite's own first case.
const real = new Response("x", { status: 200, headers: { "content-type": "text/plain" } });
responseCases.push({
  pairs: [["(a real Response)", "content-type: text/plain"]],
  entries: [...real.headers.entries()],
  setCookie: [],
  out: collectResponseHeaders(real),
});

// ── /api/proxy's decisions ────────────────────────────────────────────────────────────────────────
// The handler is driven with a STUBBED fetch, which is how the node suite reaches these gates too: what
// it decided is read off the upstream `init` it passed and off the reply it built.
const CORS_MATRIX = [
  ["https://ai.saisi.online", "r.example"],
  ["https://api.saisi.online", "r.example"],
  ["https://evil.example", "r.example"],
  ["", "r.example"],
  ["http://localhost:3000", "127.0.0.1:3000"],
  ["http://localhost:3000", "r.example"],
  ["http://127.0.0.1:5173", "localhost:8081"],
  ["https://localhost:3000", "localhost"],
  ["http://LOCALHOST:3000", "127.0.0.1"],
  ["http://[::1]:3000", "127.0.0.1"],
  ["ftp://localhost", "localhost"],
  ["not a url", "localhost"],
  ["http://localhost.evil.example", "localhost"],
  ["https://ai.saisi.online/", "r.example"],
  ["null", "r.example"],
];
const corsCases = [];
for (const [origin, host] of CORS_MATRIX) {
  const r = await proxyHandler(
    new Request(`https://${host}/api/proxy`, { method: "OPTIONS", headers: { origin } }),
  );
  corsCases.push({ origin, host, status: r.status, headers: [...r.headers.entries()] });
}

const PROXY_REQUESTS = [
  { method: "POST", headers: { authorization: "Bearer sk-or-test" } },
  { method: "POST", headers: {} },
  { method: "POST", headers: { authorization: "" } },
  { method: "GET", headers: { authorization: "Bearer sk-or-test" } },
  { method: "HEAD", headers: { authorization: "Bearer sk-or-test" } },
  { method: "PUT", headers: { authorization: "Bearer sk-or-test" } },
  { method: "PATCH", headers: { authorization: "Bearer sk-or-test" } },
  { method: "DELETE", headers: { authorization: "Bearer sk-or-test" } },
  { method: "OPTIONS", headers: { authorization: "Bearer sk-or-test" } },
  {
    method: "POST",
    headers: {
      authorization: "Bearer sk-or-test",
      "content-type": "text/plain",
      "anthropic-version": "2024-01-01",
      accept: "application/json",
      "accept-encoding": "gzip",
      "x-api-key": "sk-should-not-ride",
      cookie: "a=1",
      origin: "https://ai.saisi.online",
    },
  },
  { method: "POST", headers: { Authorization: "Bearer sk-or-test", "User-Agent": "probe/1" } },
  { method: "POST", headers: { authorization: "Bearer sk-or-test", accept: "", "user-agent": "" } },
];
const proxyCases = [];
for (const { method, headers } of PROXY_REQUESTS) {
  const seen = {};
  const real = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    seen.url = String(url);
    seen.init = init;
    return new Response("{}", { status: 200, headers: { "content-type": "application/json" } });
  };
  try {
    const body = method === "GET" || method === "HEAD" ? undefined : "{}";
    const r = await proxyHandler(new Request("https://r.example/api/proxy", { method, headers, body }));
    proxyCases.push({
      method,
      headers,
      status: r.status,
      upstream: seen.init ? [...seen.init.headers.entries()] : null,
      sendBody: seen.init ? seen.init.body !== undefined : null,
      url: seen.url ?? null,
      reply: [...r.headers.entries()],
    });
  } catch (e) {
    proxyCases.push({ method, headers, threw: String(e && e.message ? e.message : e) });
  } finally {
    globalThis.fetch = real;
  }
}

// ── the api/git gate's two decisions ─────────────────────────────────────────────────────────────
// `allowedRedirect` takes a Response; only its `location` header is read, so the oracle builds the
// smallest thing that has one — which is what the node suite does too (`new Response(null, {headers})`).
const PATHS = [
  "/o/r.git/info/refs",
  "/o/r.git/git-upload-pack",
  "/",
  "",
  "o/r",
  "/a\\b",
  "/a\u0000b",
  "/a/../b",
  "..",
  "%zz",
  "/%2e%2e/x",
  "/a\r\nb",
  "/a\u001fb",
  "/a\u007fb",
  "//evil.example/x",
  "/%2F%2Fevil.example/x",
  "/%252e%252e/x",
  "/%",
  "/%2",
  "/%GG",
  "/%C3%A9",
  "/%FF",
  "/%E4%B8%AD",
  "/a+b",
  "/a%20b",
  "/deep/nested/path.git/info/refs?service=git-upload-pack",
  "/x%5Cy",
  "/.",
  "/...",
  "/a..b",
  "/🚀",
  "/%F0%9F%9A%80",
];
const pathCases = PATHS.map((value) => ({ value, ok: validPath(value) }));

const CURRENTS = [
  "https://github.com/o/r",
  "https://api.saisi.online/api/git/o/r",
  "not a url",
];
const LOCATIONS = [
  null, "", "/o/r2", "https://github.com/a", "https://www.github.com/a",
  "https://objects.githubusercontent.com/a", "https://evil.example/", "http://github.com/a",
  "//github.com/a", "https://github.com:8443/a", "https://GITHUB.com/a", "https://github.com.evil.example/a",
  "https://github.com/a?b=c#d", "https://github.com", "  https://github.com/a  ", "not a url",
  "/%2F%2Fevil.example", "https://github.com/a/../b",
];
const redirectCases = [];
for (const current of CURRENTS) {
  for (const location of LOCATIONS) {
    let out = null;
    let threw = null;
    try {
      const r = allowedRedirect(
        { headers: new Headers(location ? { location } : {}) },
        new URL(current),
      );
      out = r ? r.href : null;
    } catch (e) {
      threw = String(e && e.message ? e.message : e);
    }
    redirectCases.push({ current, location, out, threw });
  }
}

// ── the api/github gate's decisions ──────────────────────────────────────────────────────────────
const GH_PATHS = [
  "/web/o/r", "/raw/o/r/main/x", "/api/repos/o/r", "/release/o/r/v1",
  "/", "/web", "/web/", "/WEB/o/r", "/nope/x", "/constructor/x", "/__proto__/x", "/toString/x",
  "", "web/o/r", "/web//evil.example/x", "/web/%2F%2Fevil.example/x", "/web/a/../b",
  "/web/a\\b", "/web/a\u0000b", "/web/a\u001fb", "/web/a\u007fb", "/web/%zz", "/web/%FF",
  "/web/a+b", "/web/%2e%2e/x", "/web/%252e%252e/x", "/api/%E4%B8%AD", "/release/o/r/a b",
];
const ghPathCases = GH_PATHS.map((value) => {
  const route = parseRoute(value);
  return {
    value,
    safe: safePath(value),
    route: route ? { base: typeof route.base === "string" ? route.base : null, path: route.path } : null,
    baseIsString: route ? typeof route.base === "string" : null,
  };
});

const UPSTREAM_CASES = [
  ["https://github.com", "/o/r"],
  ["https://github.com", "//evil.example/x"],
  ["https://github.com", "https://evil.example/x"],
  ["https://github.com", "/a/../b"],
  ["https://raw.githubusercontent.com", "/o/r/main/x"],
  ["https://api.github.com", "/repos/o/r"],
  ["https://github.com", ""],
  ["https://github.com", "?a=1"],
  ["https://github.com", "#frag"],
  ["https://github.com", "/a b"],
  ["https://github.com", "/a%20b"],
  ["https://github.com", "/%2F%2Fevil.example"],
  ["https://github.com", "/o/r?x=1&y=2"],
  ["https://github.com:8443", "/o/r"],
  ["not a url", "/o/r"],
  ["https://github.com", "not a path"],
  ["https://github.com", "/\\evil.example"],
  ["https://api.github.com", "//api.github.com/x"],
];
const upstreamCases = UPSTREAM_CASES.map(([base, path]) => {
  let out;
  try {
    const r = upstreamUrl(base, path);
    out = r.url ? { url: r.url.href } : { error: r.error };
  } catch (e) {
    out = { threw: String(e && e.message ? e.message : e) };
  }
  return { base, path, ...out };
});

const GH_REDIRECT_CURRENTS = ["https://github.com/o/r", "https://api.github.com/repos/o/r"];
const GH_LOCATIONS = [
  null, "", "/o/r2", "https://github.com/a", "https://www.github.com/a",
  "https://objects.githubusercontent.com/a", "https://release-assets.githubusercontent.com/a",
  "https://github-releases.githubusercontent.com/a", "https://raw.githubusercontent.com/a",
  "https://api.github.com/a", "https://evil.example/", "http://github.com/a",
  "https://codeload.github.com/a", "https://github.com.evil.example/a", "https://github.com:8443/a",
];
const ghRedirectCases = [];
for (const current of GH_REDIRECT_CURRENTS) {
  for (const location of GH_LOCATIONS) {
    const r = redirectTarget({ headers: new Headers(location ? { location } : {}) }, new URL(current));
    ghRedirectCases.push({ current, location, out: r ? r.href : null });
  }
}

const HEADER_SETS = [
  {},
  { accept: "application/json", range: "bytes=0-99", "if-none-match": "W/\"x\"" },
  { "user-agent": "probe/1", "x-not-allowed": "nope", cookie: "a=1", authorization: "Bearer x" },
  { accept: "" },
  { accept: null },
  { Accept: "application/json" },
  { "content-type": "application/json", "cache-control": "no-store", etag: "\"abc\"", vary: "Accept" },
  { "content-length": "45", "content-encoding": "gzip", "content-range": "bytes 0-9/10" },
  { "x-extra": "1" },
];
const ghHeaderCases = HEADER_SETS.map((headers) => ({
  headers,
  request: [...copyRequestHeaders({ headers: new Headers(headers) }).entries()],
  response: [...copyResponseHeaders({ headers: new Headers(headers) }).entries()],
}));

// ── the api/zen gate ─────────────────────────────────────────────────────────────────────────────
const ZEN_PATHS = [
  "/v1/messages", "/v1/messages/x", "/v1/chat/completions", "/v1/responses", "/v1/models",
  "/v1/chat/completions/extra", "/messages", "/",
  "", "v1/messages", "//evil.example/x", "/%2F%2Fevil.example/x", "/a/../b", "/..", "/a/..",
  "/%2e%2e/b", "/a%2F..%2Fb", "/a\\b", "/a\u0000b", "/a\r\nb", "/a\u001fb",
  "/https:/evil.example", "/v1:2/x", "/a:b", "/%zz", "/%FF", "/a+b", "/a%20b",
  "/v1/messages?x=1", "/%E4%B8%AD", "/a..b", "/a/..b",
];
const zenPathCases = ZEN_PATHS.map((value) => {
  const out = normalizeUpstreamPath(value);
  return { value, out };
});

const ZEN_TARGETS = [null, "og", "ds", "qw", "or", "cm", "nope", "constructor", "toString", "", "OG", " og"];
const ZEN_CREDS = [
  { "x-api-key": "sk-zen" },
  { authorization: "Bearer sk-or" },
  { "x-api-key": "sk-zen", authorization: "Bearer sk-or" },
  { authorization: "Bearer Bearer sk-x" },
  { authorization: "bearer sk-lower" },
  { "x-api-key": "  sk-padded  " },
  { "x-api-key": "" },
  {},
];
const ZEN_SESSIONS = [
  {},
  { "x-opencode-session": "s1" },
  { "x-client-request-id": "c1" },
  { session_id: "s2" },
  { "x-session-id": "s3" },
  { "x-client-request-id": "c1", session_id: "s2" },
  { "x-opencode-session": "  spaced  " },
];
const ZEN_METHODS = ["POST", "GET", "OPTIONS", "PUT", "HEAD", "DELETE"];
const ZEN_URL_PATHS = ["/v1/messages", "/v1/chat/completions", "/v1/responses", "/v1/models", "/a/../b", "/%zz", "/x"];

// THE MATRIX IS TARGETED RATHER THAN A CROSS PRODUCT. The first version was every method × target ×
// credential × session spelling — 4,032 cases and a 1.6 MB fixture, sixteen times every other corpus in
// this directory, for combinations that differ in nothing (the session header only exists on `og`, and
// a credential only matters where the auth split reads it). What each SWEEP below is for is named where
// it is built, and every arm still has its floor in the Rust test.
const zenRequests = [];
const push = (method, target, cred, session, zenPath) => {
  const q = [];
  if (target !== null) q.push(`target=${encodeURIComponent(target)}`);
  q.push(`path=${encodeURIComponent(zenPath)}`);
  zenRequests.push({
    method,
    target,
    requestUrl: `https://r.example/api/zen?${q.join("&")}`,
    headers: { ...cred, ...session },
  });
};

// 1. every method against a representative target and credential — the preflight, the body rule and
//    the 401.
for (const method of ZEN_METHODS) {
  for (const target of [null, "og", "ds", "nope", "constructor"]) {
    for (const cred of [{ "x-api-key": "sk-zen" }, { authorization: "Bearer sk-or" }, {}]) {
      push(method, target, cred, {}, "/v1/messages");
    }
  }
}
// 2. every target, with a credential, against every upstream path — the auth split and the URL.
for (const target of ZEN_TARGETS) {
  for (const zenPath of ZEN_URL_PATHS) {
    push("POST", target, { "x-api-key": "sk-zen" }, {}, zenPath);
  }
}
// 3. every credential spelling on each of og's three auth shapes — the strip-once rule.
for (const cred of ZEN_CREDS) {
  for (const zenPath of ["/v1/messages", "/v1/chat/completions", "/v1/responses", "/v1/models"]) {
    push("POST", "og", cred, {}, zenPath);
  }
}
// 4. every session spelling on og AND on a non-og target — the header is og-only, and the first
//    spelling with a value wins.
for (const session of ZEN_SESSIONS) {
  for (const target of ["og", "ds"]) {
    push("POST", target, { "x-api-key": "sk-zen" }, session, "/v1/messages");
  }
}

const zenCases = [];
for (const { method, target, requestUrl, headers } of zenRequests) {
  const seen = {};
  const real = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    seen.url = String(url);
    seen.init = init;
    return new Response("{}", { status: 200, headers: { "content-type": "application/json" } });
  };
  try {
    const body = method === "GET" || method === "HEAD" ? undefined : "{}";
    const r = await zenHandler(new Request(requestUrl, { method, headers, body }));
    zenCases.push({
      method,
      target,
      requestUrl,
      headers,
      status: r.status,
      // THE BODY TOO: a refusal's text reaches the caller, so it is part of the decision.
      body: r.status >= 400 ? await r.clone().text() : null,
      upstream: seen.init ? [...seen.init.headers.entries()] : null,
      upstreamUrl: seen.url ?? null,
      sendBody: seen.init ? seen.init.body !== undefined : null,
    });
  } catch (e) {
    zenCases.push({ method, target, requestUrl, headers, threw: String(e && e.message ? e.message : e) });
  } finally {
    globalThis.fetch = real;
  }
}

// ── the api/gform gate ───────────────────────────────────────────────────────────────────────────
const CONTENT_TYPES = [
  null, "", "text/html", "TEXT/HTML", "text/plain; charset=utf-8", "text/", "application/javascript",
  "application/x-javascript", "application/ecmascript", "application/json", "application/ld+json",
  "text/event-stream", "image/png", "image/svg+xml", "application/octet-stream", "font/woff2",
  "multipart/form-data; boundary=x", "application/json ; charset=utf-8",
];
const rewritableCases = CONTENT_TYPES.map((value) => ({ value, ok: rewritable(value) }));

const HOSTS = [
  "forms.gle", "docs.google.com", "www.google.com", "www.gstatic.com", "ssl.gstatic.com",
  "fonts.googleapis.com", "fonts.gstatic.com", "lh3.googleusercontent.com",
];
const BODIES = [];
for (const host of HOSTS) {
  BODIES.push(`a https://${host}/x b`);
  BODIES.push(`a https:\\/\\/${host}/x b`);
  BODIES.push(`a https:\\u002F\\u002F${host}/x b`);
  BODIES.push(`a https:\\u002f\\u002f${host}/x b`);
  BODIES.push(`a https:\\x2f\\x2f${host}/x b`);
  BODIES.push(`a //${host}/x b`);
}
BODIES.push(
  "",
  "no hosts here",
  "https://docs.google.com/a and https://www.gstatic.com/b and //forms.gle/c",
  "https://NOTdocs.google.com/x",
  "https://docs.google.com.evil.example/x",
  "https:\\\\docs.google.com/x",
  "中 https://www.google.com/中",
  "<html><head><script integrity=\"sha384-x\" src=\"https://www.gstatic.com/a.js\"></script></head></html>",
  "<html><head>\n  <script  integrity=\"\" src=x></script>\n</head></html>",
  "<html><header>no head element</header></html>",
  "<html><head></head></html>",
  "<html><HEAD><script integrity='single-quoted'></script></HEAD></html>",
  "<html><head><script integrity=unquoted></script></head></html>",
  "<html><head><script integrity=\"a\" integrity=\"b\"></script></head></html>",
);
const rewriteCases = BODIES.map((body) => {
  const out = {};
  for (const isHtml of [false, true]) {
    try {
      out[isHtml ? "html" : "text"] = rewriteBody(body, "https://v.saisi.online", isHtml);
    } catch (e) {
      out[isHtml ? "html" : "text"] = null;
      out.threw = String(e && e.message ? e.message : e);
    }
  }
  return { body, ...out };
});

const COOKIE_SETS = [
  [],
  [["set-cookie", "NID=1; Path=/"]],
  [["set-cookie", "NID=1; Path=/"], ["set-cookie", "AEC=2; Path=/"]],
  [["content-type", "text/html"]],
  [["Set-Cookie", "A=1"], ["set-cookie", "B=2"]],
];
const cookieCases = COOKIE_SETS.map((pairs) => {
  const h = new Headers();
  for (const [k, v] of pairs) h.append(k, v);
  // BOTH PATHS: the real `getSetCookie()` arm, and the `forEach` FALLBACK that a runtime without it
  // would take — which cannot be reached through a real `Headers` here, so the fake carries forEach.
  const fake = {
    forEach(cb) {
      for (const [k, v] of pairs) cb(v, k);
    },
  };
  return {
    pairs,
    entries: [...h.entries()],
    setCookie: typeof h.getSetCookie === "function" ? h.getSetCookie() : [],
    viaGetSetCookie: setCookieValues(h),
    viaForEach: setCookieValues(fake),
  };
});

// ── what the relay answers when things go wrong ──────────────────────────────────────────────────
const HANDLER_CALLS = {
  proxy: { fn: proxyHandler, url: "https://r.example/api/proxy" },
  zen: { fn: zenHandler, url: "https://r.example/api/zen?target=og&path=%2Fv1%2Fmessages" },
  git: { fn: gitHandler, url: "https://r.example/api/git?path=%2Fo%2Fr.git%2Finfo%2Frefs" },
  github: { fn: githubHandler, url: "https://r.example/api/github?path=%2Fweb%2Fo%2Fr" },
  gform: { fn: gformHandler, url: "https://r.example/api/gform?path=%2Fdocs%2Fspreadsheets%2Fd%2Fx" },
};
const METHODS = ["GET", "HEAD", "POST", "PUT", "DELETE", "PATCH", "OPTIONS", "TRACE"];
const AUTH = { authorization: "Bearer sk-test" };

async function drive(handler, url, method, stub) {
  const real = globalThis.fetch;
  globalThis.fetch = stub;
  try {
    const body = ["POST", "PUT", "PATCH"].includes(method) ? "{}" : undefined;
    const r = await handler(new Request(url, { method, headers: AUTH, body }));
    return {
      status: r.status,
      text: r.status >= 400 ? await r.clone().text() : null,
      cacheControl: r.headers.get("cache-control"),
    };
  } catch (e) {
    return { threw: String(e && e.message ? e.message : e) };
  } finally {
    globalThis.fetch = real;
  }
}

const okStub = async () =>
  new Response("{}", { status: 200, headers: { "content-type": "application/json" } });
const failStub = (status) => async () =>
  new Response("UPSTREAM SECRET DETAIL", { status, headers: { "content-type": "text/plain" } });
const throwStub = async () => {
  throw new Error("ECONNREFUSED 127.0.0.1:443");
};
// PER-HANDLER, because each follows only its own allowlisted hosts — a redirect to a FOREIGN host is
// refused before the cap is ever reached, which is a different rule and a different case.
const redirectStubFor = (handler) => async () =>
  new Response(null, {
    status: 302,
    headers: {
      location:
        handler === "gform" ? "https://docs.google.com/loop" : "https://github.com/loop",
    },
  });

const methodCases = [];
for (const [name, { fn, url }] of Object.entries(HANDLER_CALLS)) {
  for (const method of METHODS) {
    methodCases.push({ handler: name, method, ...(await drive(fn, url, method, okStub)) });
  }
}
const failureCases = [];
for (const [name, { fn, url }] of Object.entries(HANDLER_CALLS)) {
  for (const status of [500, 502, 503, 504]) {
    failureCases.push({
      handler: name,
      kind: `upstream-${status}`,
      status,
      ...(await drive(fn, url, "GET", failStub(status))),
    });
  }
  failureCases.push({ handler: name, kind: "throw", status: null, ...(await drive(fn, url, "GET", throwStub)) });
  failureCases.push({
    handler: name,
    kind: "redirect-loop",
    status: null,
    ...(await drive(fn, url, "GET", redirectStubFor(name))),
  });
}

// ── summrise-relay's two pure decisions ──────────────────────────────────────────────────────────
const TOKEN_PAIRS = [
  ["s3cret", "s3cret"],
  ["s3cret", "s3cres"],
  ["s3cret", "s3cre"],
  ["s3cret", "s3crett"],
  ["", "s3cret"],
  ["s3cret", ""],
  ["", ""],
  ["S3CRET", "s3cret"],
  ["s3cret ", "s3cret"],
  ["中", "中"],
  ["中", "a"],
  ["Bearer x", "Bearer x"],
  [null, "s3cret"],
  [undefined, "s3cret"],
  [5, "5"],
  [5, "s3cret"],
];
const tokenCases = TOKEN_PAIRS.map(([given, want]) => ({ given: given === undefined ? null : given, want, ok: tokenOk(given, want) }));

const ROUTE_CASES = [];
for (const pathname of [
  "/agent/pull", "/agent/answer", "/healthz", "/", "", "/mcp", "/panel/x", "/api/vitals",
  "/agent/pull/", "/AGENT/PULL", "/agent/answer/", "/healthz/", "/health", "/agent/answer?x=1",
]) {
  for (const method of ["GET", "POST", "HEAD", "PUT", "DELETE", "get"]) {
    ROUTE_CASES.push({ pathname, method, route: routeOf(pathname, method) });
  }
}

if (MODE === "relay") {
  console.log(
    JSON.stringify(
      {
        note: "Generated by proxies/api-relay/relay/oracle.mjs relay from the SHIPPING proxies/summrise-relay/relay.mjs. Do not edit by hand.",
        token_cases: tokenCases,
        route_cases: ROUTE_CASES,
      },
      null,
      1,
    ),
  );
  const ok = tokenCases.filter((c) => c.ok).length;
  const routes = new Set(ROUTE_CASES.map((c) => c.route)).size;
  if (ok < 2 || routes < 4) {
    console.error(`oracle: the summrise-relay corpus moved (${ok} accepted, ${routes} routes)`);
    process.exit(1);
  }
} else if (MODE === "responses") {
  console.log(
    JSON.stringify(
      {
        note: "Generated by proxies/api-relay/relay/oracle.mjs responses from the SHIPPING api/*.js|ts. Do not edit by hand.",
        method_cases: methodCases,
        failure_cases: failureCases,
      },
      null,
      1,
    ),
  );
  const refused = methodCases.filter((c) => c.status === 405).length;
  const upstream = failureCases.filter((c) => c.kind.startsWith("upstream")).length;
  if (methodCases.length < 30 || refused < 3 || upstream < 15) {
    console.error(`oracle: the responses corpus moved (${methodCases.length} methods, ${refused} 405s, ${upstream} upstream failures)`);
    process.exit(1);
  }
} else if (MODE === "gform") {
  console.log(
    JSON.stringify(
      {
        note: "Generated by proxies/api-relay/relay/oracle.mjs gform from the SHIPPING api/gform.ts. Do not edit by hand.",
        rewritable_cases: rewritableCases,
        rewrite_cases: rewriteCases,
        cookie_cases: cookieCases,
      },
      null,
      1,
    ),
  );
  const yes = rewritableCases.filter((c) => c.ok).length;
  const changed = rewriteCases.filter((c) => c.text && c.text !== c.body).length;
  const injected = rewriteCases.filter((c) => c.html && c.html.includes("SRI hashes do not survive")).length;
  if (yes < 5 || changed < 20 || injected < 3) {
    console.error(`oracle: the gform corpus moved (${yes} rewritable, ${changed} changed, ${injected} injected)`);
    process.exit(1);
  }
} else if (MODE === "zen") {
  console.log(
    JSON.stringify(
      {
        note: "Generated by proxies/api-relay/relay/oracle.mjs zen from the SHIPPING api/zen.js. Do not edit by hand.",
        path_cases: zenPathCases,
        zen_cases: zenCases,
      },
      null,
      1,
    ),
  );
  const sent = zenCases.filter((c) => c.upstream).length;
  const refused = zenCases.filter((c) => c.status === 401 || c.status === 400).length;
  const valid = zenPathCases.filter((c) => c.out).length;
  if (sent < 50 || refused < 20 || valid < 5) {
    console.error(`oracle: the zen corpus moved (${sent} sent, ${refused} refused, ${valid} valid paths)`);
    process.exit(1);
  }
} else if (MODE === "github") {
  console.log(
    JSON.stringify(
      {
        note: "Generated by proxies/api-relay/relay/oracle.mjs github from the SHIPPING api/github.ts. Do not edit by hand.",
        path_cases: ghPathCases,
        upstream_cases: upstreamCases,
        redirect_cases: ghRedirectCases,
        header_cases: ghHeaderCases,
      },
      null,
      1,
    ),
  );
  const routed = ghPathCases.filter((c) => c.route).length;
  const escaped = upstreamCases.filter((c) => c.error).length;
  const followed = ghRedirectCases.filter((c) => c.out).length;
  if (routed < 5 || escaped < 4 || followed < 5) {
    console.error(`oracle: the github corpus moved (${routed} routed, ${escaped} escaped, ${followed} followed)`);
    process.exit(1);
  }
} else if (MODE === "git") {
  console.log(
    JSON.stringify(
      {
        note: "Generated by proxies/api-relay/relay/oracle.mjs git from the SHIPPING api/git.ts. Do not edit by hand.",
        path_cases: pathCases,
        redirect_cases: redirectCases,
      },
      null,
      1,
    ),
  );
  const okPaths = pathCases.filter((c) => c.ok).length;
  const allowed = redirectCases.filter((c) => c.out).length;
  if (okPaths < 8 || allowed < 3 || pathCases.length < 20) {
    console.error(`oracle: the git corpus moved (${okPaths} valid of ${pathCases.length}, ${allowed} redirects)`);
    process.exit(1);
  }
} else if (MODE === "proxy") {
  console.log(
    JSON.stringify(
      {
        note: "Generated by proxies/api-relay/relay/oracle.mjs proxy from the SHIPPING api/proxy.js. Do not edit by hand.",
        cors_cases: corsCases,
        proxy_cases: proxyCases,
      },
      null,
      1,
    ),
  );
  const allowed = corsCases.filter((c) => c.headers.some(([k]) => k === "access-control-allow-origin")).length;
  const refused = proxyCases.filter((c) => c.status === 401).length;
  if (allowed < 3 || refused < 2 || proxyCases.length < 8) {
    console.error(`oracle: the proxy corpus moved (${allowed} allowed, ${refused} refused of ${proxyCases.length})`);
    process.exit(1);
  }
} else if (MODE === "headers") {
  console.log(
    JSON.stringify(
      {
        note: "Generated by proxies/api-relay/relay/oracle.mjs headers from the SHIPPING server/routing.mjs. Do not edit by hand.",
        forward_cases: forwardCases,
        response_cases: responseCases,
      },
      null,
      1,
    ),
  );
  const threw = forwardCases.filter((c) => c.threw).length;
  if (forwardCases.length < 10 || responseCases.length < 5 || threw === 0) {
    console.error(`oracle: the header corpus moved (${forwardCases.length} forward, ${responseCases.length} response, ${threw} throw)`);
    process.exit(1);
  }
} else {
  console.log(
    JSON.stringify(
      {
        note: "Generated by proxies/api-relay/relay/oracle.mjs from the SHIPPING server/routing.mjs. Do not edit by hand.",
        routes: ROUTES,
        hosts: headerSets.map((headers) => ({ headers, host: resolveHost(headers) })),
        routes_cases: cases,
      },
      null,
      1,
    ),
  );
  if (routed === 0) {
    console.error("oracle: NOTHING matched — the table or the corpus moved");
    process.exit(1);
  }
}
