// The ORACLE for the relay's routing port: the SHIPPING `server/routing.mjs` is driven over a corpus,
// and what it produced is written out as the fixture the Rust test replays.
//
//   cd proxies/api-relay/relay && node oracle.mjs routing > fixtures/routing-corpus.json
//   cd proxies/api-relay/relay && node oracle.mjs headers > fixtures/headers-corpus.json
//
// The direction matters: the expectations come from the code the VPS runs, not from the port. It is
// committed so the fixture is REGENERABLE, and it is a `.mjs` for the reason AGENTS.md's carve-outs
// name — the oracle IS the shipping module, and only a JavaScript runtime can run it.
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

if (MODE === "headers") {
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
