// THE CUTOVER'S CRITERION, RUNNABLE: the same request, answered byte for byte by both relays.
//
// The plan states the bar for cutting a surface over in as many words — 同请求新旧响应字节可比 — and this is
// that comparison for block ④'s api-relay. It starts BOTH implementations (the shipping Node entry from
// `dist/`, and the Rust binary), sends one corpus to each, compares status, content-type, CORS origin and
// body byte for byte, and exits non-zero on any difference it cannot explain.
//
// WHY THE CORPUS IS THE REFUSAL SET. The dialling paths are covered twice already: the crate's corpora
// prove the DECISIONS against the JavaScript, and each binary's own tests prove the WIRING against a stub
// upstream. What neither can see is whether the two IMPLEMENTATIONS answer the same bytes — and the paths
// where a difference is silent rather than loud are the refusals: a BYOK gate, a method gate, an origin
// guard. Those are deterministic, they never touch the network, and they are where the security lives.
//
//   node proxies/api-relay/relay/differential.mjs
//
// It needs `bash proxies/api-relay/build-relay.sh` to have run (the Node side) and `cargo build --bin
// vrelay` (the Rust side), and it pins `SUMMRISE_RELAY_HEADER_TIMEOUT_MS=300` for BOTH so the two cases
// that DO dial fail fast and deterministically instead of waiting thirty seconds for a real upstream.
//
// MUTATION (2), THE ONE THAT PROVES THE EXEMPTION WAS DANGEROUS, and it was run with the OLD file beside the
// new one rather than argued: change the RUST side's sentence for a route that used to be exempt —
// `"unsupported GitHub route", 400` -> `"unsupported GitHub route MUTATED", 400` in `src/main.rs` — rebuild,
// and run BOTH files against the same binary pair.
// RESULT (2): the OLD file printed `DIFF (route not wired)  github: route not wired` and
// `differential: 13/14 byte-identical, 1 route-not-wired, 0 unexplained`, **exit 0** — a real divergence
// between the two relays, reported as success. The NEW file printed `DIFF  github: unsupported route` and
// `13/14 byte-identical, 1 unexplained`, **exit 1**. Measured 2026-10-09. So the exemption is not a
// theoretical hazard: it excused a divergence this corpus can now catch.
//
// MUTATION: change one byte the JavaScript sends in a path this corpus covers — the BYOK refusal's
// sentence, `caller Authorization required (BYOK)` -> `... (BYOK MUTATED)`, in `src/lib.rs` — rebuild the
// binary with `cargo build --bin vrelay`, and run this.
// RESULT: `DIFF  proxy: no key`, `differential: 13/14 byte-identical, 1 unexplained`, exit 1. Restoring the
// sentence returns it to `14/14 byte-identical, 0 unexplained`, exit 0. **RE-MEASURED 2026-10-09, when the
// `unwired` exemption was deleted** — the old line read `11/14 ... 2 route-not-wired, 1 unexplained` and
// `12/14`, and those numbers were only ever true while two routes were excused from the comparison.
//
// **AND THE EXEMPTION THIS FILE USED TO CARRY IS GONE, BECAUSE ITS REASON EXPIRED AND NOBODY NOTICED.**
// `/api/github` and `/api/gform` were marked `unwired` — "routes of the shipping relay that the binary does
// not serve yet, and whether they are needed at all is a question for the operator". That was true when it
// was written and FALSE FROM 2026-10-07, when both were wired: `relay/src/main.rs` serves them and answers
// the shipping relay's own **400 `{"error":"unsupported GitHub route"}`** for an unsupported path. So for a
// week the file compared two routes it believed were absent, and a REAL divergence between them would have
// been filed as "route not wired" and the run would still have exited 0 — a silent exemption, on the paths
// where the header above says a difference is quiet rather than loud. Measured 2026-10-09: with the flag
// removed the two are BYTE-IDENTICAL (`14/14 byte-identical, 0 unexplained`), so the exemption was not
// hiding a defect; it was hiding the QUESTION. A difference now fails the run, and the mechanism that could
// excuse one has been deleted rather than left standing with nothing to excuse.
import { spawn } from "node:child_process";
import { request } from "node:http";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const RELAY_DIR = join(HERE, "..");
const JS_DIR = join(RELAY_DIR, "dist");
const RUST_BIN = join(HERE, "target", "debug", "vrelay");
const JS_PORT = 18091;
const RS_PORT = 18092;
const ENV = { ...process.env, SUMMRISE_RELAY_HEADER_TIMEOUT_MS: "300" };

for (const [what, path] of [
  ["the Node bundle", join(JS_DIR, "entry.mjs")],
  ["the Rust binary", RUST_BIN],
]) {
  if (!existsSync(path)) {
    console.error(`${what} is not built: ${path}`);
    console.error("  bash proxies/api-relay/build-relay.sh && (cd proxies/api-relay/relay && cargo build --bin vrelay)");
    process.exit(2);
  }
}

const CORPUS = [
  { name: "healthz", method: "GET", path: "/healthz" },
  { name: "unknown route", method: "GET", path: "/nope" },
  { name: "proxy: no key", method: "POST", path: "/api/proxy", body: "{}" },
  { name: "proxy: preflight", method: "OPTIONS", path: "/api/proxy", headers: { origin: "https://ai.saisi.online" } },
  { name: "proxy: preflight, foreign origin", method: "OPTIONS", path: "/api/proxy", headers: { origin: "https://evil.example" } },
  { name: "zen: no key", method: "POST", path: "/api/zen?path=%2Fv1%2Fmessages", body: "{}" },
  { name: "zen: unknown target", method: "POST", path: "/api/zen?target=nope&path=%2Fv1%2Fmessages", headers: { "x-api-key": "k" }, body: "{}" },
  { name: "git: PUT refused", method: "PUT", path: "/api/git/o/r.git/info/refs" },
  { name: "git: protocol-relative tail", method: "GET", path: "/api/git/%2F%2Fevil.example%2Fx" },
  { name: "git: backslash tail", method: "GET", path: "/api/git/%2Fa%5Cb" },
  { name: "git: dotdot tail", method: "GET", path: "/api/git/%2Fa%2F..%2Fb" },
  { name: "git: dials, upstream unreachable", method: "GET", path: "/api/git/o/r.git/info/refs" },
  // The two routes that used to be exempt. They are ROUTES — 400, not 404 — on both sides.
  { name: "github: unsupported route", method: "GET", path: "/api/github?path=%2Fweb%2Fo%2Fr" },
  { name: "gform: unsupported route", method: "GET", path: "/api/gform?path=%2Fdocs%2Fx" },
];

function send(port, item) {
  return new Promise((resolve) => {
    const req = request(
      { host: "127.0.0.1", port, method: item.method, path: item.path, headers: { host: "v.saisi.online", ...(item.headers || {}) } },
      (res) => {
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () =>
          resolve({
            status: res.statusCode,
            type: (res.headers["content-type"] || "").split(";")[0],
            acao: res.headers["access-control-allow-origin"] || "",
            body: Buffer.concat(chunks).toString("utf8"),
          }),
        );
      },
    );
    req.on("error", (e) => resolve({ status: 0, type: "", acao: "", body: `TRANSPORT ${e.code}` }));
    if (item.body) req.write(item.body);
    req.end();
  });
}

async function waitForHealthz(port, tries = 60) {
  for (let i = 0; i < tries; i += 1) {
    const answer = await send(port, { method: "GET", path: "/healthz" });
    if (answer.status === 200) return true;
    await new Promise((r) => setTimeout(r, 250));
  }
  return false;
}

const js = spawn(process.execPath, ["entry.mjs"], { cwd: JS_DIR, env: { ...ENV, PORT: String(JS_PORT) }, stdio: ["ignore", "ignore", "pipe"] });
const rs = spawn(RUST_BIN, [`127.0.0.1:${RS_PORT}`], { env: ENV, stdio: ["ignore", "ignore", "pipe"] });
const stderrOf = (child) => {
  let text = "";
  child.stderr.on("data", (c) => (text += c));
  return () => text;
};
const jsErr = stderrOf(js);
const rsErr = stderrOf(rs);

let exitCode = 1;
try {
  if (!(await waitForHealthz(JS_PORT)) || !(await waitForHealthz(RS_PORT))) {
    console.error("a relay never answered /healthz");
    console.error(`  node: ${jsErr()}`);
    console.error(`  rust: ${rsErr()}`);
  } else {
    let same = 0;
    const unexplained = [];
    for (const item of CORPUS) {
      const [a, b] = await Promise.all([send(JS_PORT, item), send(RS_PORT, item)]);
      const equal = a.status === b.status && a.type === b.type && a.body === b.body && a.acao === b.acao;
      if (equal) {
        same += 1;
        console.log(`  same  ${item.name}  ${a.status} ${a.type || "-"} ${JSON.stringify(a.body.slice(0, 60))}`);
        continue;
      }
      const detail = `${item.name}\n      node: ${a.status} ${a.type || "-"} acao=${a.acao || "-"} ${JSON.stringify(a.body.slice(0, 120))}\n      rust: ${b.status} ${b.type || "-"} acao=${b.acao || "-"} ${JSON.stringify(b.body.slice(0, 120))}`;
      unexplained.push(detail);
      console.log(`  DIFF  ${detail}`);
    }
    console.log("");
    console.log(
      `differential: ${same}/${CORPUS.length} byte-identical, ${unexplained.length} unexplained`,
    );
    exitCode = unexplained.length ? 1 : 0;
  }
} finally {
  js.kill("SIGKILL");
  rs.kill("SIGKILL");
}
process.exit(exitCode);
