// The origin-escape guard: a caller-supplied path must never choose the upstream HOST.
//
// WHY THIS FILE EXISTS (round 120). `git.ts`'s `validPath` accepted `//evil.example/x`, and
// `new URL("//evil.example/x", "https://github.com")` resolves to `https://evil.example/x` —
// protocol-relative paths REPLACE the origin. Measured on the live relay:
//
//   curl https://v.saisi.online/api/git//example.com/   →  Example Domain's HTML
//
// with the caller's `authorization` header riding upstream (`git.ts` forwards it deliberately,
// as the git smart-HTTP protocol requires). The 7-entry hostile table never tried `//host`, so
// seven rounds of gate work on these exact files passed over it — and CI only `node --check`s
// the vrelay handlers, so nothing ran them there either.
//
// THE THREE COPIES ARE THE POINT. `git.ts`, `github.ts` and `gform.ts` each compile to a
// standalone module (the bundle is flat), so the guard cannot be imported — it is duplicated by
// necessity. Three copies of a check is precisely how this codebase loses one of them, so the
// parity assertion below is what makes the duplication safe: change one, and the suite fails.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { validPath } from "../git.ts";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SRC = (f) => readFileSync(path.join(HERE, "..", f), "utf8");

/** The guard's source text, from `function upstreamUrl` to its closing brace at column 0. */
function guardOf(file) {
  const src = SRC(file);
  const start = src.indexOf("function upstreamUrl(");
  assert.ok(start > 0, `${file} has no upstreamUrl guard`);
  const end = src.indexOf("\n}\n", start);
  assert.ok(end > start, `${file}: could not find the guard's end`);
  return src.slice(start, end + 3);
}

test("the origin guard is byte-identical in all three handlers", () => {
  const git = guardOf("git.ts");
  assert.equal(guardOf("github.ts"), git, "github.ts's guard drifted from git.ts's");
  assert.equal(guardOf("gform.ts"), git, "gform.ts's guard drifted from git.ts's");
  // A parse that finds nothing must not report parity (round 54's lesson).
  assert.ok(git.includes("url.origin !== want.origin"), git.slice(0, 120));
});

test("validPath refuses the protocol-relative shape, raw and encoded", () => {
  for (const evil of [
    "//evil.example/x",
    "//evil.example",
    "/%2Fevil.example/x",
    "/%2f%2fevil.example/x",
  ]) {
    assert.equal(validPath(evil), false, `accepted: ${evil}`);
  }
  // ...while the real shapes still pass.
  assert.equal(validPath("/deepseek-ai/x.git/info/refs"), true);
  assert.equal(validPath("/"), true);
});
