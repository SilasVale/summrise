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
// MUTATION: change one byte the JavaScript sends in a path this corpus covers — the BYOK refusal's
// sentence, `caller Authorization required (BYOK)` -> `... (BYOK MUTATED)`, in `src/lib.rs` — rebuild the
// binary with `cargo build --bin vrelay`, and run this.
// RESULT: `DIFF  proxy: no key`, `differential: 11/14 byte-identical, 2 route-not-wired, 1 unexplained`,
// exit 1. Restoring the sentence returns it to 12/14 and 0 unexplained. Measured 2026-10-02.
//
// A DIFFERENCE IS NOT AUTOMATICALLY A DEFECT. `/api/github` and `/api/gform` are marked `unwired`: they
// are routes of the shipping relay that the binary does not serve yet, and whether they are needed at all
// is a question for the operator. An unexplained difference fails the run; an explained one is printed.
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
  { name: "github: route not wired", method: "GET", path: "/api/github?path=%2Fweb%2Fo%2Fr", unwired: true },
  { name: "gform: route not wired", method: "GET", path: "/api/gform?path=%2Fdocs%2Fx", unwired: true },
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
    const explained = [];
    for (const item of CORPUS) {
      const [a, b] = await Promise.all([send(JS_PORT, item), send(RS_PORT, item)]);
      const equal = a.status === b.status && a.type === b.type && a.body === b.body && a.acao === b.acao;
      if (equal) {
        same += 1;
        console.log(`  same  ${item.name}  ${a.status} ${a.type || "-"} ${JSON.stringify(a.body.slice(0, 60))}`);
        continue;
      }
      const detail = `${item.name}\n      node: ${a.status} ${a.type || "-"} acao=${a.acao || "-"} ${JSON.stringify(a.body.slice(0, 120))}\n      rust: ${b.status} ${b.type || "-"} acao=${b.acao || "-"} ${JSON.stringify(b.body.slice(0, 120))}`;
      if (item.unwired) {
        explained.push(detail);
        console.log(`  DIFF (route not wired)  ${item.name}`);
      } else {
        unexplained.push(detail);
        console.log(`  DIFF  ${detail}`);
      }
    }
    console.log("");
    console.log(
      `differential: ${same}/${CORPUS.length} byte-identical, ${explained.length} route-not-wired, ${unexplained.length} unexplained`,
    );
    exitCode = unexplained.length ? 1 : 0;
  }
} finally {
  js.kill("SIGKILL");
  rs.kill("SIGKILL");
}
process.exit(exitCode);
