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
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import test from "node:test";
import assert from "node:assert/strict";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const src = (rel) => readFileSync(path.join(HERE, "..", "src", rel), "utf8");

test("every password input in the console has an accessible name", () => {
  // WHY THIS IS NOT "declares an aria-label" ANY MORE. That was the first version, and it was blind
  // three ways at once:
  //   * it scanned TWO hard-coded files (views/Users.tsx, views/Keys.tsx) — neither of which is where
  //     the console's password fields actually live, so it had never protected anything;
  //   * its pattern `<input\b[\s\S]{0,500}?\/>` required a self-closing tag inside 500 characters,
  //     and the one password input it did reach is 658 characters long;
  //   * it accepted only aria-label, while the console mostly names fields by WRAPPING them in a
  //     <label> — so a correct field could fail and, worse, an incorrect one could pass.
  //
  // What it found when it finally looked: DevicesPanel's modal had a <label> BESIDE each input with
  // no htmlFor — a caption, not a name. Both fields are associated now.
  const views = path.join(HERE, "..", "src");
  const files = [];
  const walk = (dir) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, e.name);
      if (e.isDirectory()) walk(full);
      else if (e.name.endsWith(".tsx")) files.push(full);
    }
  };
  walk(views);
  assert.ok(files.length >= 10, `expected the console's components, walked ${files.length} files`);

  let checked = 0;
  for (const file of files) {
    const source = readFileSync(file, "utf8");
    const rel = path.relative(views, file);
    // Every <input …>, self-closing or not, however long its attribute list.
    const inputs = [...source.matchAll(/<input\b(?:(?!\/>)[\s\S])*?(?:\/>|>)/g)].map((m) => m[0]);
    for (const tag of inputs) {
      if (!/type=\{?["']password["']\}?/.test(tag)) continue;
      checked++;
      const id = /id="([^"]+)"/.exec(tag)?.[1];
      const named =
        /aria-label=/.test(tag) ||
        /aria-labelledby=/.test(tag) ||
        (id !== undefined && source.includes(`htmlFor="${id}"`)) ||
        // wrapped in a <label>…</label> — the form the console uses most
        isWrappedInLabel(source, tag);
      assert.ok(
        named,
        `${rel}: a password input has no accessible name — a placeholder is a hint, not a name, ` +
          `and a <label> BESIDE an input names nothing without htmlFor`,
      );
    }
  }
  // A SCAN THAT MATCHES NOTHING MUST NOT REPORT SUCCESS (this repo's oldest lesson). The previous
  // version of this test would have passed on a console with no password inputs at all, and did.
  assert.ok(checked >= 5, `expected the console's password fields, matched ${checked}`);
});

/** `true` when the input's opening tag sits between a `<label>` and its `</label>`. */
function isWrappedInLabel(source, tag) {
  const at = source.indexOf(tag);
  if (at < 0) return false;
  const open = source.lastIndexOf("<label", at);
  if (open < 0) return false;
  const close = source.indexOf("</label>", open);
  return close > at;
}
