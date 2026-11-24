import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const root = new URL("../", import.meta.url);
const read = (p) => readFileSync(new URL(p, root), "utf8");

/// The raw `TOKENS` string, without the Rust that surrounds it: a negative assertion in the crate's
/// own test names the strings this test forbids, and a whole-file scan would read that prose as a
/// declaration.
function tokenSheet(source) {
  const opening = 'pub const TOKENS: &str = r#"';
  const start = source.indexOf(opening);
  assert.notEqual(start, -1, "the shared sheet declares TOKENS");
  const from = start + opening.length;
  const end = source.indexOf('"#;', from);
  assert.notEqual(end, -1, "the shared sheet closes its TOKENS string");
  return source.slice(from, end);
}

test("one_palette_is_declared_once", () => {
  const sheet = tokenSheet(read("../ui/src/lib.rs"));
  assert.match(sheet, /color-scheme: light;/, "the shared sheet declares one light scheme");
  assert.doesNotMatch(sheet, /prefers-color-scheme/, "the shared sheet follows no system theme");
  assert.doesNotMatch(sheet, /data-theme/, "the shared sheet answers no chosen theme");
  assert.equal(sheet.match(/:root/g)?.length, 1, "the shared sheet has one root");

  const panel = read("../device/src/panel.rs");
  assert.doesNotMatch(panel, /button\s+id="theme"/, "the device panel offers no theme toggle");
  assert.doesNotMatch(panel, /Toggle light\/dark/, "the device panel offers no theme label");
  assert.doesNotMatch(panel, /localStorage/, "the device panel stores no theme choice");
  assert.doesNotMatch(panel, /dataset\.theme/, "the device panel applies no theme");
  assert.match(panel, /chip ok/, "the panel's status rows are chips");
});

/// THE SHELL'S FIRST FRAME IS THE PAGE'S CANVAS, IN BOTH SOURCES. `main.js` is what Electron runs and
/// `main.ts` is its hand-maintained twin, so a half-edit — one file, or one of the two windows — must
/// fail here. The chrome files carry none of the five dark literals the light contract deleted.
test("the_shell_paints_paper_before_the_page", () => {
  for (const source of ["src/main.ts", "src/main.js"]) {
    const shell = read(source);
    assert.doesNotMatch(shell, /#111/, `${source} paints no ink-black first frame`);
    assert.equal(
      shell.match(/backgroundColor:\s*"#f3f0e8"/g)?.length,
      2,
      `${source} paints BOTH windows in the sheet's canvas`,
    );
  }

  for (const file of ["src/host.css", "src/offline.html"]) {
    const chrome = read(file);
    for (const gone of ["#111", "#131418", "#2a1616", "#0c0d10", "#6b2b2b"]) {
      assert.equal(chrome.includes(gone), false, `${file} carries no dark literal ${gone}`);
    }
  }
});
