#!/usr/bin/env node
// The console page's Rust/WASM logic must be the artifact its sources describe.
//
// The wasm bytes are committed and inlined into the page; a checkout has no Rust toolchain and CI's
// page job has no Rust either. This check needs only Node: it recomputes the source stamp the build
// wrote, and it decodes the base64 the page imports back to the committed bytes. A source edit with
// no rebuild, or a hand-edited base64, fails here instead of shipping a page whose logic is older
// than its own repository.
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const LOGIC = path.join(ROOT, "device", "console-logic");
const WEB = path.join(ROOT, "device", "console", "web", "src", "wasm");
const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");

function fail(message) {
  console.error(`console-logic: ${message}`);
  console.error("  fix: ./device/console-logic/build.sh && git add device/console-logic device/console/web/src/wasm");
  process.exit(1);
}

// The stamp is what `sha256sum Cargo.toml src/lib.rs | sha256sum` wrote, reproduced exactly.
const stampLines = [
  `${sha(readFileSync(path.join(LOGIC, "Cargo.toml")))}  Cargo.toml`,
  `${sha(readFileSync(path.join(LOGIC, "src", "lib.rs")))}  src/lib.rs`,
  ""
].join("\n");
const expected = readFileSync(path.join(LOGIC, "LOGIC_SOURCE.sha256"), "utf8").trim();
if (sha(Buffer.from(stampLines)) !== expected) {
  fail("the crate sources changed after the last build (LOGIC_SOURCE.sha256 is stale)");
}

const glue = readFileSync(path.join(WEB, "console_logic.js"), "utf8");
if (glue.includes("new URL('console_logic_bg.wasm'")) {
  fail("the glue still resolves a .wasm URL; the single-file page would fetch a file it does not serve");
}
if (!glue.includes("pass the wasm bytes to init()")) {
  fail("the glue's default branch is not the built one (build.sh patches it)");
}

const wasm = readFileSync(path.join(WEB, "console_logic_bg.wasm"));
const generated = readFileSync(path.join(WEB, "console_logic_bg.ts"), "utf8");
const match = /CONSOLE_LOGIC_WASM_BASE64 =\s*'([A-Za-z0-9+/=]+)'/.exec(generated);
if (!match) fail("console_logic_bg.ts has no base64 constant");
const inlined = Buffer.from(match[1], "base64");
if (!inlined.equals(wasm)) {
  fail(`the inlined bytes (${inlined.length}) differ from the committed wasm (${wasm.length})`);
}

console.log(
  `console-logic: ok — source ${expected.slice(0, 12)}…, wasm ${wasm.length} B, ` +
    `inlined ${inlined.length} B, glue patched`
);
