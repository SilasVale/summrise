// device-state pins — the console's device fact, and the state that used to have no producer.
//
// WHY. Both surfaces derived "is the agent answering" as `!!status?.agent_up`, which collapses THREE states into
// two: a device the console has never asked about (no status entry) came out `err`, so the page painted a red dot
// and the word "offline" about it. `.sig-dot.off` existed in the stylesheet for exactly that case and nothing ever
// rendered it (round 35 of the standing goal).
import test from "node:test";
import assert from "node:assert/strict";
import { agentSignal, deviceIsUp, deviceTally, tunnelSignal } from "../src/lib/deviceState.ts";

/** The dictionary, stubbed: these tests are about which KEY is chosen, not what it says. */
const t = (key) => key;

test("a device with NO status is OFF — not checked is not down", () => {
  const s = agentSignal(undefined, t);
  assert.equal(s.signal, "off");
  assert.equal(s.state, "devices.notChecked");
  // and the boolean that callers use for "is it up" is false — which is why a caller that needs to say OFFLINE
  // must read the signal instead
  assert.equal(deviceIsUp(undefined), false);
});

test("a checked device is ok or err, and never off", () => {
  assert.deepEqual(agentSignal({ agent_up: true }, t), { label: "devices.statusAgent", signal: "ok", state: "devices.online" });
  assert.deepEqual(agentSignal({ agent_up: false }, t), { label: "devices.statusAgent", signal: "err", state: "devices.offline" });
  assert.equal(deviceIsUp({ agent_up: true }), true);
  assert.equal(deviceIsUp({ agent_up: false }), false);
});

test("the tunnel reads the same three states", () => {
  assert.equal(tunnelSignal(undefined, t).signal, "off");
  assert.equal(tunnelSignal({ tunnel_up: true }, t).state, "devices.tunnelUp");
  assert.equal(tunnelSignal({ tunnel_up: false }, t).state, "devices.tunnelDown");
});

test("a device with a STATUS but no agent field is still OFF for that field", () => {
  // The gateway may answer for a device before it has probed the agent: `{}` is a real response shape, and it must
  // not read as "the agent is down" either.
  assert.equal(agentSignal({}, t).signal, "off");
  assert.equal(tunnelSignal({ agent_up: true }, t).signal, "off");
});

// ── THE COUNTS, WHICH TWO SURFACES USED TO COMPUTE SEPARATELY (round 38 of the standing goal) ────────────────
test("the tally counts what is KNOWN, and reports what is not", () => {
  const devices = [{ name: "d1" }, { name: "d2" }, { name: "d3" }];
  const statuses = {
    d1: { agent_up: true, tunnel_up: true },
    d2: { agent_up: false, tunnel_up: false },
    // d3 has no entry at all: not checked, which is not the same as down
  };
  assert.deepEqual(deviceTally(devices, statuses), { online: 1, tunnels: 1, unchecked: 1, total: 3 });
});

test("an empty or absent list tallies to zero rather than throwing", () => {
  // The Overview renders before the devices request answers, and `devices` is null until then.
  assert.deepEqual(deviceTally(null, {}), { online: 0, tunnels: 0, unchecked: 0, total: 0 });
  assert.deepEqual(deviceTally([], {}), { online: 0, tunnels: 0, unchecked: 0, total: 0 });
  // and a list where NOTHING has been asked yet is all-unchecked, not all-offline
  const t = deviceTally([{ name: "d1" }, { name: "d2" }], {});
  assert.equal(t.online, 0);
  assert.equal(t.unchecked, 2);
});
