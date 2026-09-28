// device-update.test.mjs — THE CONTROL'S THREE STATES, AND THE TWO THE BRIEF'S THREE DO NOT COVER.
//
// WHY (the defect this exists to remove). The Devices page carried a badge reading "可更新到 1.2.490" and no
// button, and the operator asked for the button TWICE. Building one is the easy half; the half that needs a
// guard is WHICH INPUTS DECIDE IT, because every one of them has a plausible-looking wrong answer:
//
//   * `!agentUp` instead of `agent.signal === "err"` — "not yet asked" is also falsy, so the first frame of
//     every page load would speak about devices nobody has asked anything of yet;
//   * the console's own KV comparison instead of the device's `update_available` — two computations of one fact,
//     free to disagree, and the console's cannot see a rollback pin;
//   * a version chip when the device's RELEASE SERVER did not answer — the lie the field `error` exists to
//     prevent ("NOT AVAILABLE WHEN NOBODY ANSWERED", `agent/src/plugins/update/tools.rs`);
//   * a button for a device held by `summrise rollback` — where `force` on the device DELETES the pin.
//
// The render smoke (`devices-render-smoke.mjs`) proves the four arms RENDER. This proves the DERIVATION, and it
// is the only place the two states the smoke cannot reach are reachable at all: a device that has gone DARK
// mid-swap (no `update`, and the console's own memory is the last thing that knows), and a device that is BUSY
// while carrying a start time older than the window it trusts.
import test from "node:test";
import assert from "node:assert/strict";

import {
  UPDATE_DARK_WINDOW_MS,
  forgetUpdateAttempt,
  rememberedUpdateAttempt,
  rememberUpdateAttempt,
  updateControl,
} from "../src/lib/deviceUpdate.ts";

const NOW = 1_800_000_000_000;

/** A device answer that says nothing in particular — every test overrides what it is about. */
const answered = (over = {}) => ({
  update: { current: "1.2.490", latest: "1.2.490", update_available: false, pinned_to: null, ...over },
  agentSignal: "ok",
  remembered: null,
  now: NOW,
});

test("the device's own `update_available` is what puts a button on the row, and its `latest` is the label", () => {
  assert.deepEqual(updateControl(answered({ update_available: true, latest: "1.2.491" })), {
    kind: "action",
    to: "1.2.491",
  });
  // `update_available` TRUE WITH NO `latest` IS NOT A PRESSABLE STATE: the device's own object always carries
  // both, so a missing one means a shape this build does not understand, and a button labelled "undefined" is
  // worse than no button.
  assert.deepEqual(updateControl(answered({ update_available: true, latest: undefined })), {
    kind: "current",
    checkedAt: null,
  });
});

test("up to date says nothing and does nothing — the chip is the sentence", () => {
  assert.deepEqual(updateControl(answered()), { kind: "current", checkedAt: null });
});

test("`error` is NOT 'up to date': the release server did not answer, and the row says so", () => {
  // THE LIE, IN ONE ASSERTION. The device answers `update_available: false` in this case, so a control that
  // reads only that field renders a version chip — and a version chip reads as "you are current".
  const c = updateControl(answered({ error: "dns: no such host", checked_at: NOW - 5000 }));
  assert.equal(c.kind, "unchecked");
  assert.equal(c.error, "dns: no such host");
  assert.equal(c.checkedAt, NOW - 5000);
});

test("`error` OUTRANKS the pin, because 'we could not ask' is not 'it is held'", () => {
  // Both are set only if the device says so, and it cannot: `update_status` returns the pin without asking the
  // channel only when there is NO channel at all, in which case it sends no `error`. If a future device sends
  // both, the honest sentence is the one about the missing answer — a pin claim needs a channel to check.
  assert.equal(updateControl(answered({ error: "timed out", pinned_to: "1.2.480" })).kind, "unchecked");
});

test("a rollback pin is a state of its own, so a held device cannot look current", () => {
  const c = updateControl(answered({ pinned_to: "1.2.480", checked_at: NOW - 1000 }));
  assert.equal(c.kind, "held");
  assert.equal(c.pinnedTo, "1.2.480");
});

test("NO ANSWER IS NOT 'UP TO DATE' — the control does not exist", () => {
  // Structural: the gateway sets `st.update` only inside `if (res.ok)`, so a device that did not answer has no
  // verdict. Nothing may then be claimed about it.
  assert.deepEqual(updateControl({ update: null, agentSignal: "off", remembered: null, now: NOW }), {
    kind: "none",
  });
});

test("`agent.signal === \"err\"` on its own is NOT a sentence — the console must not narrate every row", () => {
  // THE TRI-STATE RULE. Offline with no attempt of our own means exactly nothing: no chip, no button, no
  // refusal. A `!agentUp` test would ALSO be true here for a device nobody has asked about yet, which is the
  // first frame of every page load.
  assert.deepEqual(updateControl({ update: null, agentSignal: "err", remembered: null, now: NOW }), {
    kind: "none",
  });
  // And the device that answered is not affected by the signal being unknown.
  assert.equal(updateControl({ ...answered({ update_available: true }), agentSignal: "off" }).kind, "action");
});

test("a DARK device with this console's own attempt record is in flight, dated by US", () => {
  // The half the render smoke cannot reach: the swap has killed the tunnel, so there is no `update` at all and
  // every fact about the device is gone. The console pressed the button, so the console is what still knows.
  const at = NOW - 12 * 60_000;
  const c = updateControl({ update: null, agentSignal: "err", remembered: { at, to: "1.2.491" }, now: NOW });
  assert.deepEqual(c, { kind: "inflight", since: at, source: "console" });
});

test("...and the record expires with the window, so a day-old press is not resurrected", () => {
  const old = NOW - UPDATE_DARK_WINDOW_MS - 1;
  assert.deepEqual(
    updateControl({ update: null, agentSignal: "err", remembered: { at: old, to: "1.2.491" }, now: NOW }),
    { kind: "none" },
  );
});

test("`busy` is in flight — the presence of the device's own marker, which is what refuses a second press", () => {
  const at = NOW - 20 * 60_000;
  assert.deepEqual(
    updateControl(answered({ busy: true, last_attempt: { at_ms: at, to: "1.2.491" } })),
    { kind: "inflight", since: at, source: "device" },
  );
  // AND IT WINS OVER AN AVAILABLE UPDATE: a device that says a newer build exists AND that it is already
  // installing one must not be offered a second button — the device's own marker would refuse it.
  assert.equal(updateControl(answered({ busy: true, update_available: true, latest: "1.2.491" })).kind, "inflight");
});

test("a start time OLDER than the window is not used — `busy` alone is undated, not misdated", () => {
  // `last_attempt` is NOT cleared when a new attempt begins (the marker is acquired before the download, the
  // record is written at the WMI handoff after it), so a device busy right now can be carrying the record of an
  // update that finished hours ago. Dated with that, the sentence would claim an outage far longer than the one
  // being observed — `null` makes the view say "no start time shown" instead of inventing one.
  const stale = NOW - UPDATE_DARK_WINDOW_MS - 60_000;
  assert.deepEqual(updateControl(answered({ busy: true, last_attempt: { at_ms: stale } })), {
    kind: "inflight",
    since: null,
    source: "device",
  });
});

test("the console's record and the device's agree to within a round trip — the NEWER is used", () => {
  const device = NOW - 20 * 60_000;
  const ours = NOW - 19 * 60_000;
  assert.equal(
    updateControl({
      ...answered({ busy: true, last_attempt: { at_ms: device } }),
      remembered: { at: ours, to: "1.2.491" },
    }).since,
    ours,
  );
});

test("the console's attempt record survives a reload and is cleared only when asked", () => {
  // A MODULE-LEVEL STORE WITH A PROCESS-LIFETIME CACHE, so this is also the test that the cache is not a
  // write-only hole: a record written and read back in the same process must be the one that was written.
  rememberUpdateAttempt("d-test", "1.2.491", NOW);
  assert.deepEqual(rememberedUpdateAttempt("d-test", NOW), { at: NOW, to: "1.2.491" });
  assert.equal(rememberedUpdateAttempt("d-never-pressed", NOW), null);
  forgetUpdateAttempt("d-test");
  assert.equal(rememberedUpdateAttempt("d-test", NOW), null);
});
