import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";
import { installRootFrom } from "../src/install-root.js";
test("the install root is above desktop\\resources, not above the asar's src", () => {
  const appDir = "C:\\Program Files\\Summrise\\desktop\\resources\\app.asar\\src";
  assert.equal(installRootFrom(appDir), "C:\\Program Files\\Summrise");
  assert.equal(path.win32.join(installRootFrom(appDir), "etc", "config.yaml"),
               "C:\\Program Files\\Summrise\\etc\\config.yaml");
  assert.equal(path.win32.join(appDir, "..", "..", ".."),
               "C:\\Program Files\\Summrise\\desktop"); // the old, wrong arithmetic
});
