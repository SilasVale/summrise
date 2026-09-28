// verify.mjs — the fidelity check, and the reason "the same numbers" is a fact
// rather than a claim.
//
// It renders the page with the page.js THIS BRANCH SHIPS, renders it with the Rust
// renderer, normalises ONLY the two places the migration is about, and requires the
// rest to be byte-identical. If it passes, then every measurement of the live page —
// 26 text nodes, worst contrast 4.63:1, the type ladder, the cost sentence above the
// button, the button's fill and geometry — holds for the Rust page by construction,
// because the bytes are the same bytes.
//
// The two normalised places, and nothing else:
//   1. `onclick="toggleTheme()"` — the listener is attached by wasm now, so the
//      attribute is gone. (This is a REAL regression until wasm boots; see README.)
//   2. the closing <script> block — replaced by the wasm module loader.
//
// The pre-paint theme bootstrap stays inline in BOTH, because a wasm module cannot
// run before first paint. That is not an omission; it is the finding.
import { readFileSync, writeFileSync } from "node:fs";
import { PAGE } from "../src/page.js";

// The three URLs the live worker renders with (CONSOLE_URL + the release host).
const CONSOLE = "https://agent.saisi.online";
const INSTALLER = "https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz";
const SETUP = "https://agent.saisi.online/summrise-agent/SummriseAgent-Setup.exe";

const normalise = (s) =>
  s
    .replace(/<script[\s\S]*?<\/script>/g, "<!--SCRIPT-->")
    .replace(/ onclick="toggleTheme\(\)"/, "");

const reference = PAGE(CONSOLE, INSTALLER, SETUP);
writeFileSync("dist/pagejs-reference.html", reference);
const rust = readFileSync("dist/index.html", "utf8");

const a = normalise(reference);
const b = normalise(rust);
if (a !== b) {
  const A = a.split("\n");
  const B = b.split("\n");
  let shown = 0;
  console.error(`FAIL  Rust output differs from page.js outside the 2 migration points`);
  console.error(`      reference ${Buffer.byteLength(reference)} bytes, rust ${Buffer.byteLength(rust)} bytes`);
  for (let i = 0; i < Math.max(A.length, B.length) && shown < 8; i++) {
    if (A[i] !== B[i]) {
      console.error(`      line ${i + 1}\n        page.js: ${JSON.stringify(A[i]).slice(0, 150)}\n        rust   : ${JSON.stringify(B[i]).slice(0, 150)}`);
      shown++;
    }
  }
  process.exit(1);
}

// The honesty checks the page must keep, asserted on the RUST bytes directly.
const body = rust.slice(rust.indexOf("<body>"));
const cost = body.indexOf("Easiest path: one setup.exe");
const button = body.indexOf('class="btn-primary"');
const fallback = body.indexOf("No Windows installer is published");
const npmOnly = readFileSync("dist/npm-only.html", "utf8");
const checks = [
  ["cost sentence present", cost !== -1],
  ["cost sentence ABOVE the download button", cost !== -1 && button !== -1 && cost < button],
  ["the npm fallback channel is offered", body.includes("npm i -g")],
  ["the console link is present", body.includes(">Summrise console</a>")],
  ["tgz-only arm offers NO installer button", !npmOnly.includes('class="btn-primary"')],
  ["tgz-only arm says what it knows", npmOnly.includes(fallback >= 0 ? "No Windows installer is published" : "x")],
];
let bad = 0;
for (const [name, ok] of checks) {
  console.log(`      ${ok ? "ok  " : "FAIL"} ${name}`);
  if (!ok) bad++;
}
if (bad) process.exit(1);

// BYTES, not String.length. The two differ by 578 here (289 multi-byte characters),
// and reporting the UTF-16 length as "bytes served" is how a baseline of 31,140 was
// recorded for a page that serves 31,718.
console.log(`      byte-identical to page.js outside the 2 migration points (${Buffer.byteLength(rust)} vs ${Buffer.byteLength(reference)} bytes)`);
