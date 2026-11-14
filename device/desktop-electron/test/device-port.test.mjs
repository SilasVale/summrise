import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { readDevicePort } from "../src/device-port.js";

function roots() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "summrise-port-"));
  const helper = path.join(dir, "SummriseHelper");
  const data = path.join(dir, "Summrise");
  fs.mkdirSync(helper, { recursive: true });
  fs.mkdirSync(data, { recursive: true });
  return { dir, helper, data, env: { SUMMRISE_HELPER_DATA: helper, SUMMRISE_DATA: data } };
}

test("the user-readable copy wins over the data directory", () => {
  const { helper, data, env } = roots();
  fs.writeFileSync(path.join(data, "device.port"), "7737");
  fs.writeFileSync(path.join(helper, "device.port"), "18090");
  assert.equal(readDevicePort(env), 18090);
});

test("a missing helper copy falls through to the data directory", () => {
  const { data, env } = roots();
  fs.writeFileSync(path.join(data, "device.port"), " 18090\n");
  assert.equal(readDevicePort(env), 18090);
});

test("garbage, zero and out-of-range ports are ignored", () => {
  const { helper, data, env } = roots();
  for (const text of ["not a port", "0", "70000", "-1"]) {
    fs.writeFileSync(path.join(helper, "device.port"), text);
    fs.writeFileSync(path.join(data, "device.port"), "18090");
    assert.equal(readDevicePort(env), 18090, `helper copy ${JSON.stringify(text)}`);
  }
});

test("no device file anywhere answers undefined", () => {
  const { env } = roots();
  assert.equal(readDevicePort(env), undefined);
});
