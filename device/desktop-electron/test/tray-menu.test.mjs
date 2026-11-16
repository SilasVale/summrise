import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { trayMenuRows } = require("../src/tray-menu.js");

test("the tray menu carries the two device pages, the two panes, the version and the update door", () => {
  const rows = trayMenuRows({ version: "2.0.3", health: "running" });
  const labels = rows.map((row) => row.label).filter(Boolean);
  for (const label of ["Open transcript", "Open panel", "Browser", "Harness", "Check for updates…"]) {
    assert.ok(labels.includes(label), `${label} missing — labels: ${labels.join(", ")}`);
  }
  // The rows the deleted surfaces used to own are GONE, not merely unused.
  assert.ok(!labels.includes("Open"), "there is no generic 'Open' row — the tray icon's click focuses the main window");
  assert.ok(!labels.includes("Open terminal"), "the terminal died with the SPA");
  const version = rows.find((row) => row.action === undefined && /^Version/.test(row.label));
  assert.equal(version.label, "Version 2.0.3");
  assert.equal(version.enabled, false, "the version row is a fact, not a button");
});

test("a tray with no version says so, and an empty health line leaves two separators", () => {
  const rows = trayMenuRows({ version: "", health: "" });
  const version = rows.find((row) => /^Version/.test(row.label));
  assert.equal(version.label, "Version (not reported)");
  assert.equal(
    rows.filter((row) => row.type === "separator").length,
    2,
    "an empty health line and its neighbours must collapse to two separators (never two in a row, never one at an end)",
  );
});

test("the actions the shell must implement are exactly these six", () => {
  const actions = trayMenuRows({ version: "2.0.3", health: "running" })
    .map((row) => row.action)
    .filter(Boolean);
  assert.deepEqual(actions, ["transcript", "panel", "browser", "harness", "update", "quit"]);
});
