import assert from "node:assert/strict";
import test from "node:test";

import { isDevicePage } from "../src/device-page.js";

const base = "http://127.0.0.1:18090";

test("the device page predicate admits only /activity and /panel on the agent's own origin", () => {
  assert.equal(isDevicePage(`${base}/activity`, base), true);
  assert.equal(isDevicePage(`${base}/activity?theme=dark`, base), true);
  assert.equal(isDevicePage(`${base}/panel`, base), true);

  for (const url of [
    "http://127.0.0.1:50696/activity",
    "http://user@127.0.0.1:18090/activity",
    "http://127.0.0.1:18090@evil.com/activity",
    "http://127.0.0.1:18090.evil.com/activity",
    `${base}/desktop/`,
    `${base}/activity/`,
    `${base}/api/activity`,
    "not a url",
  ]) {
    assert.equal(isDevicePage(url, base), false, url);
  }

  assert.equal(isDevicePage(`${base}/activity`, "not a url"), false, "a malformed base is refused");
});
