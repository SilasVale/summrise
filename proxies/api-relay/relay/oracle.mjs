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
import { validPath, allowedRedirect } from "../api/git.ts";
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

if (MODE === "git") {
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
