// device-state pins — the console's device fact, and the state that used to have no producer.
//
// WHY. Both surfaces derived "is the agent answering" as `!!status?.agent_up`, which collapses THREE states into
// two: a device the console has never asked about (no status entry) came out `err`, so the page painted a red dot
// and the word "offline" about it. `.sig-dot.off` existed in the stylesheet for exactly that case and nothing ever
// rendered it (round 35 of the standing goal).
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { agentSignal, deviceIsUp, deviceTally, tunnelSignal } from "../src/lib/deviceState.ts";

const HERE = path.dirname(fileURLToPath(import.meta.url));

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
  // THE FLAGS ARE PART OF THE ANSWER NOW (round 133), so this pin says so: the view used to unpack the signal back into
  // `ok`/`err` at the call site, twice, and a fourth signal value would have been silently absent from both.
  assert.deepEqual(agentSignal({ agent_up: true }, t), {
    label: "devices.statusAgent", signal: "ok", ok: true, err: false, state: "devices.online",
  });
  assert.deepEqual(agentSignal({ agent_up: false }, t), {
    label: "devices.statusAgent", signal: "err", ok: false, err: true, state: "devices.offline",
  });
  assert.deepEqual(agentSignal(undefined, t), {
    label: "devices.statusAgent", signal: "off", ok: false, err: false, state: "devices.notChecked",
  });
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

// ── THE NOTE'S NUMBER IS CHECKED AGAINST THE SOURCE THAT DECIDES IT (round 47) ────────────────────────────────
test("the freshness claim in deviceState.ts still matches the worker's probe TTL", () => {
  // WHY THIS EXISTS. `deviceState.ts` used to carry a "WHAT THIS IS NOT" paragraph saying a row could be "checked 40
  // minutes ago" and that freshness was unanswered. The worker's own source answers it — a 30-second probe cache —
  // and a reason that is FALSE is worse than no reason, in the one file four different checks point a reader at.
  // This pins the number to the mirror so the note cannot rot: change the TTL and this fails, and the note gets
  // corrected with it.
  const mirror = readFileSync(
    path.join(HERE, "..", "..", "public", "code", "files", "vale-gate", "src", "plugins", "mcp.ts"),
    "utf8",
  );
  const ttl = /DEVICE_PROBE_TTL_MS\s*=\s*([0-9_]+)/.exec(mirror);
  assert.ok(ttl, "the probe TTL moved or was renamed — deviceState.ts cites it by name, so this check needs updating");
  const seconds = Number(ttl[1].replace(/_/g, "")) / 1000;
  const note = readFileSync(path.join(HERE, "..", "src", "lib", "deviceState.ts"), "utf8");
  assert.match(
    note,
    new RegExp(`${seconds}-second`),
    `deviceState.ts no longer states the ${seconds}-second freshness window the worker implements`,
  );
  // THE CLAIM, NOT THE WORDS. The retraction has to be able to NAME what it retracts, so forbidding the phrase
  // "second question" would forbid the correction — which is how this assertion failed its own first run. What must
  // not come back is the sentence that made the claim.
  assert.ok(
    !/distinguishing that from fresh is a second question/.test(note),
    "the retracted claim is back in deviceState.ts — the worker answers it, and a false reason is worse than none",
  );
});

test("the console's poll interval is stated in one place and matches its comment", () => {
  // THE SAME SHAPE AS THE FRESHNESS CHECK ABOVE, for the same reason: a number in a comment is a claim, and this
  // repository has now found two of them that were false. `CONSOLE_POLL_MS` was extracted in round 49 because
  // `Overview.tsx` and `DevicesPanel.tsx` each carried their own 60000 — and because the WORKER's comment about this
  // very cadence says 30s. The worker's copy is in another repository; this is the half that can be pinned here.
  const lib = readFileSync(path.join(HERE, "..", "src", "lib", "deviceState.ts"), "utf8");
  const value = /CONSOLE_POLL_MS\s*=\s*([0-9_]+)/.exec(lib);
  assert.ok(value, "CONSOLE_POLL_MS is gone — the two views would go back to their own literals");
  const seconds = Number(value[1].replace(/_/g, "")) / 1000;
  // THE BLOCK ABOVE THE CONSTANT, NOT THE WHOLE FILE: this file QUOTES the worker's wrong sentence, which contains
  // the words "polls every 30s" — so a whole-file match found the quoted claim first and reported the cadence as
  // missing. A check that reads its subject's own quotation of the thing it checks will believe the quotation.
  const block = lib.slice(Math.max(0, lib.indexOf("CONSOLE_POLL_MS") - 1400), lib.indexOf("CONSOLE_POLL_MS"));
  assert.match(block, new RegExp(`console polls every ${seconds}s\\b`), `deviceState.ts no longer states the ${seconds}s cadence it defines`);
  // and neither view may carry its own copy again
  for (const view of ["Overview.tsx", "DevicesPanel.tsx"]) {
    const src = readFileSync(path.join(HERE, "..", "src", "views", view), "utf8");
    assert.match(src, /setInterval\([^,]+,\s*CONSOLE_POLL_MS\)/, `${view} polls with something other than CONSOLE_POLL_MS`);
    assert.ok(!/setInterval\([^,]+,\s*[0-9_]+\s*\)/.test(src), `${view} has a hard-coded poll interval again`);
  }
});
