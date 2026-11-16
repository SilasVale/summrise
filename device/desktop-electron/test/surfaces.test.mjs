import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const root = new URL("../", import.meta.url);
const read = (p) => readFileSync(new URL(p, root), "utf8");

test("the device panel can choose light or dark, and the shared sheet carries the choice", () => {
  const panel = read("../device/src/panel.rs");
  assert.match(panel, /id="theme"/, "the device panel has the toggle");
  assert.match(panel, /chip ok/, "the panel's status rows are chips");

  const tokens = read("../ui/src/lib.rs");
  assert.match(tokens, /:root\[data-theme="dark"\]/, "the shared sheet carries the chosen dark");
});

test("the legacy panel is pulled onto the same values by its theme layer", () => {
  const index = read("panel/desktop/index.html");
  assert.match(index, /theme\.css/, "the legacy page loads the theme layer");
  const theme = read("panel/desktop/theme.css");
  assert.match(theme, /--accent:#c2410c/, "the accent is the transcript's");
  assert.match(theme, /--bg:#f5f6f8/, "the ground is the transcript's");
  assert.match(theme, /body\[data-theme="dark"\]/, "the legacy page keeps its dark choice");
});
