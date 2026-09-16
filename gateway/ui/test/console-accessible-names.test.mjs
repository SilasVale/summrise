// THE PASSWORD FIELD MUST CARRY A NAME, NOT ONLY A PLACEHOLDER.
//
// The console's design sweep (round 56) found exactly one control in the whole console with NO
// accessible name at all: the admin password input on the Users page — a password field has no
// text and no label to be named by, so its placeholder was all it had, and a placeholder is not a
// name (it is a hint, and it disappears as soon as anyone types).
//
// This is a source-level pin because the console's `node --test` suites have no DOM: the authority
// for rendering is the design sweep on a device. Both are needed — the sweep finds, this keeps it
// fixed.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import test from "node:test";
import assert from "node:assert/strict";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const src = (rel) => readFileSync(path.join(HERE, "..", "src", rel), "utf8");

test("every password input declares an aria-label", () => {
  for (const file of ["views/Users.tsx", "views/Keys.tsx"]) {
    const source = src(file);
    const inputs = [...source.matchAll(/<input\b[\s\S]{0,500}?\/>/g)].map((m) => m[0]);
    const passwords = inputs.filter((i) => /type="password"/.test(i));
    for (const input of passwords) {
      assert.ok(
        /aria-label=/.test(input),
        `${file}: a password input has no aria-label — a placeholder is not an accessible name`,
      );
    }
  }
});
