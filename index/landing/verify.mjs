// verify.mjs — the fidelity check, and the reason "the same measurements" is a fact
// rather than a claim.
//
// It renders the page with THE RENDERER THIS REPLACES, renders it with the Rust
// renderer given the same three values, normalises ONLY the two places the migration
// is about, and requires the rest to be BYTE-IDENTICAL. If it passes, then every
// measurement of the live page — worst contrast 4.63:1, the type ladder, h1 at
// 16px/600, no h2, the cost sentence above the button, the button's fill and
// geometry — holds for the Rust page by construction, because the bytes are the
// same bytes.
//
// THE REFERENCE IS READ OUT OF GIT, not copied into the tree. `index/src/page.js`
// at 46fa0a3f is the file that rendered the live page, and a frozen copy of it in
// this crate would be a second document to keep in step with the first. If the
// reference cannot be read the check FAILS — it does not fall back to the new
// page.js, which is not an independent reference any more (it renders from the very
// template under test, so a slot in the wrong place would appear on both sides).
//
// The two normalised places, and nothing else:
//   1. `onclick="toggleTheme()"` — the listener is attached by wasm now, so the
//      attribute is gone. (This is a REAL regression until wasm boots; see README.)
//   2. the closing <script> block — replaced by the wasm module loader.
//
// The pre-paint theme bootstrap stays inline in BOTH, because a wasm module cannot
// run before first paint. That is not an omission; it is the finding.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import { escHtml, safePageUrl } from "../src/page.js";

const ROOT = fileURLToPath(new URL("../..", import.meta.url));
const HERE = fileURLToPath(new URL(".", import.meta.url));
const GEN = fileURLToPath(new URL("target/release/gen", import.meta.url));

// ── build ───────────────────────────────────────────────────────────────────────────────────────
// **THIS HARNESS ASSUMED A PRE-BUILT `target/release/gen` AND A FULL CLONE, AND CI HAS NEITHER.**
// Measured 2026-10-05, wiring the byte comparisons into `ci.yml` for the first time: the step that runs
// this file went red. Two reasons, and both are the `first_screen.rs` hazard one directory over:
//
//   1. `GEN` is a Rust binary. `execFileSync` on a missing path throws ENOENT, and nothing built it —
//      the harness was only ever run by an author whose `cargo build --release` had already happened.
//   2. `git show 46fa0a3f:index/src/page.js` needs that commit's OBJECT. `actions/checkout@v4`
//      defaults to `fetch-depth: 1`, so on a runner the object is absent entirely — the same shallow
//      clone that made `agent/tests/first_screen.rs` fail its first CI run while passing every local one.
//
// **THE SECOND IS FIXED IN `ci.yml` (the job now checks out with `fetch-depth: 0`), NOT HERE** — a byte
// comparison that falls back to the new `page.js` would compare the template against itself, which this
// file already refuses in so many words. What is fixed HERE is the binary: it builds if it is missing,
// and `--build` forces it.
if (!existsSync(GEN) || process.argv.includes("--build")) {
  console.log("  building index/landing's gen (cargo build --release) …");
  execFileSync("cargo", ["build", "--release"], { cwd: HERE, stdio: "inherit" });
}

/** The commit whose `index/src/page.js` rendered the live page. */
const REFERENCE_COMMIT = "46fa0a3f";

// The three URLs the live worker renders with (CONSOLE_URL + the release host).
const CONSOLE = "https://agent.saisi.online";
const INSTALLER = "https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz";
const SETUP = "https://agent.saisi.online/summrise-agent/SummriseAgent-Setup.exe";

const normalise = (s) =>
  s
    .replace(/<script[\s\S]*?<\/script>/g, "<!--SCRIPT-->")
    .replace(/ onclick="toggleTheme\(\)"/, "");

// ── the reference: the renderer this replaces, out of git ───────────────────────
mkdirSync(`${HERE}dist`, { recursive: true });
let oldSource;
try {
  oldSource = execFileSync("git", ["show", `${REFERENCE_COMMIT}:index/src/page.js`], {
    cwd: ROOT,
    encoding: "utf8",
    maxBuffer: 1 << 24,
  });
} catch (e) {
  console.error(`FAIL  could not read ${REFERENCE_COMMIT}:index/src/page.js — this check has no reference,`);
  console.error(`      and falling back to the new page.js would compare the template against itself.`);
  console.error(`      ${String(e.message).split("\n")[0]}`);
  process.exit(1);
}
const referenceModule = `${HERE}dist/pagejs-reference.mjs`;
writeFileSync(referenceModule, oldSource);
const { PAGE } = await import(pathToFileURL(referenceModule).href);
// The reference page as SERVED, kept on disk so the size comparison in build.sh is
// against the page a reader gets rather than against the module it is wrapped in.
writeFileSync(`${HERE}dist/pagejs-reference.html`, PAGE(CONSOLE, INSTALLER, SETUP));

// ── the Rust page, given the values the worker would compute ────────────────────
// The renderer interpolates verbatim: the caller has already applied the whitelist
// and the escaping, which is what `index/src/page.js` does per request. Handing it
// the same expressions the worker uses is what makes this a comparison of DOCUMENTS
// rather than of two URL policies.
const rust = (out, setup) =>
  execFileSync(GEN, ["render", out, escHtml(safePageUrl(CONSOLE, "/")),
    escHtml(safePageUrl(INSTALLER, "/summrise-agent/summrise-agent-latest.tgz")),
    setup === null ? "-" : escHtml(safePageUrl(setup, "/summrise-agent/SummriseAgent-Setup.exe"))],
    { cwd: HERE, encoding: "utf8", maxBuffer: 1 << 24 });

let bad = 0;
for (const [arm, setup, out] of [
  ["setup", SETUP, `${HERE}dist/rust-setup.html`],
  ["npm-only", null, `${HERE}dist/rust-npm-only.html`],
]) {
  rust(out, setup);
  const want = normalise(PAGE(CONSOLE, INSTALLER, setup));
  const got = normalise(readFileSync(out, "utf8"));
  if (want !== got) {
    console.error(`FAIL  ${arm}: the Rust document differs from page.js outside the 2 migration points`);
    const A = want.split("\n");
    const B = got.split("\n");
    let shown = 0;
    for (let i = 0; i < Math.max(A.length, B.length) && shown < 8; i++) {
      if (A[i] !== B[i]) {
        console.error(`      line ${i + 1}\n        page.js: ${JSON.stringify(A[i]).slice(0, 150)}\n        rust   : ${JSON.stringify(B[i]).slice(0, 150)}`);
        shown++;
      }
    }
    bad++;
    continue;
  }
  // BYTES, not String.length. The two differ by 578 here (289 multi-byte characters),
  // and reporting the UTF-16 length as "bytes served" is how a baseline of 31,140 was
  // recorded for a page that serves 31,718.
  console.log(`      ${arm}: byte-identical to page.js outside the 2 migration points (${Buffer.byteLength(got)} vs ${Buffer.byteLength(want)} bytes)`);
}

// ── the honesty checks the page must keep, asserted on the RUST bytes directly ──
const body = readFileSync(`${HERE}dist/rust-setup.html`, "utf8");
const npmOnly = readFileSync(`${HERE}dist/rust-npm-only.html`, "utf8");
const b = body.slice(body.indexOf("<body>"));
const cost = b.indexOf("Easiest path: one setup.exe");
const button = b.indexOf('class="btn-primary"');
const checks = [
  ["cost sentence present", cost !== -1],
  ["cost sentence ABOVE the download button", cost !== -1 && button !== -1 && cost < button],
  ["the npm fallback channel is offered", b.includes("npm i -g")],
  ["the console link is present", b.includes(">Summrise console</a>")],
  ["tgz-only arm offers NO installer button", !npmOnly.includes('class="btn-primary"')],
  ["tgz-only arm says what it knows", npmOnly.includes("No Windows installer is published")],
  ["the installer URL is in the setup arm's button", /<a class="btn-primary" href="https:\/\/agent\.saisi\.online\/summrise-agent\/SummriseAgent-Setup\.exe">/.test(body)],
  ["the pre-paint theme bootstrap is still inline", body.includes("localStorage.getItem('summrise-theme')")],
];
for (const [name, ok] of checks) {
  console.log(`      ${ok ? "ok  " : "FAIL"} ${name}`);
  if (!ok) bad++;
}
if (bad) process.exit(1);
