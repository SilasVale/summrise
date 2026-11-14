import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { trayMenuRows } = require("../src/tray-menu.js");

test("the tray menu carries the version, the transcript, the terminal and the update door", () => {
  const rows = trayMenuRows({ version: "2.0.0", health: "running" });
  const labels = rows.map((row) => row.label).filter(Boolean);
  assert.ok(labels.includes("Open transcript"), `labels: ${labels.join(", ")}`);
  assert.ok(labels.includes("Open terminal"), `labels: ${labels.join(", ")}`);
  assert.ok(labels.includes("Check for updates…"), `labels: ${labels.join(", ")}`);
  const version = rows.find((row) => row.action === undefined && /^Version/.test(row.label));
  assert.equal(version.label, "Version 2.0.0");
  assert.equal(version.enabled, false, "the version row is a fact, not a button");
});

test("a tray with no version says so instead of showing a bare 'Version'", () => {
  const rows = trayMenuRows({ version: "", health: "" });
  const version = rows.find((row) => /^Version/.test(row.label));
  assert.equal(version.label, "Version (not reported)");
  assert.equal(
    rows.filter((row) => row.type === "separator").length,
    3,
    "an empty health line must not leave two separators together"
  );
});

test("the actions the shell must implement are exactly these", () => {
  const actions = trayMenuRows({ version: "2.0.0", health: "running" })
    .map((row) => row.action)
    .filter(Boolean);
  assert.deepEqual(actions, ["open", "transcript", "terminal", "update", "quit"]);
});
