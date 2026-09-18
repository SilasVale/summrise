// THE MONITORS PAYLOAD, read against the shapes the device actually sends.
//
// WHY THIS FILE EXISTS, and what it is not. Round 98's coverage list named `/api/monitors` as the next
// boundary to pin; round 100 went to do it and was stopped by the payload itself. My hand-written
// fixture described a transition as `{ts_ms, ok, reason}` — plausible, and WRONG: `parseMonitors` reads
// `{at_ms, up, lasted_ms}`, and it does not complain. A transition it cannot place in time is dropped,
// exactly as the hook's header says ("a probe with no usable stamp is dropped"), so a fixture written
// from memory produced a card with no transitions and no error. That is the boundary class in its
// purest form: the shape is the contract, nothing validates it, and a wrong guess looks like data.
//
// SO THESE TESTS USE THE REAL SHAPES — read off `parseMonitors` — and pin the two directions that
// matter: the real payload parses whole, and a plausible-but-wrong one is visibly empty rather than
// silently half-rendered.
//
// STILL OPEN (recorded rather than guessed): with this payload served, the Settings page rendered no
// `.monitor-row` at all in the harness — 0 elements, though the stub returned both targets. Either the
// card is fed by props the panel density does not pass, or something else gates it. That is a measured
// observation, not a diagnosis, and it is the first thing to check next time this surface is measured.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

// The OTHER HALF of the monitors contract (round 65): `agent/tests/fixtures/monitor-row.json` is asserted by the
// device's own test in `agent/src/monitor.rs` (it must SEND every promised key) and by this one (the panel must READ
// it). Read through the filesystem rather than imported, because the fixture lives outside this package.
const fixture = JSON.parse(
  readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), "../../../../../tests/fixtures/monitor-row.json"), "utf8"),
);
import { parseMonitorChange, parseMonitors } from "../useMonitors";

/** One target in the shape the hook reads — `summary`, `transitions` and `series` as it parses them. */
const target = (over: Record<string, unknown> = {}) => ({
  id: "mon-router",
  host: "192.168.1.1",
  port: 22,
  path: null,
  expect: null,
  summary: {
    probes: 240,
    up: 239,
    down: 1,
    up_pct: 99.6,
    up_now: true,
    since_ms: 1_789_000_000_000,
    drops: 1,
    latency: { min: 3, avg: 11, max: 88 },
    last_status: null,
    last_expect_ok: null,
  },
  transitions: [{ at_ms: 1_788_990_000_000, up: false, lasted_ms: 300_000 }],
  series: [
    { ts_ms: 1_789_000_000_000, ok: true, ms: 9 },
    { ts_ms: 1_789_000_015_000, ok: false, ms: null },
  ],
  ...over,
});

describe("parseMonitors", () => {
  it("reads a target whole, including the transitions it actually ships", () => {
    const { targets } = parseMonitors({ targets: [target()] });
    expect(targets.length).toBe(1);
    const t = targets[0];
    expect(t.id).toBe("mon-router");
    expect(t.host).toBe("192.168.1.1");
    expect(t.port).toBe(22);
    expect(t.summary.upPct).toBe(99.6);
    expect(t.summary.upNow).toBe(true);
    // THE SHAPE THAT COST ME THIS ROUND: at_ms/up/lasted_ms, not ts_ms/ok/reason.
    expect(t.transitions.length, "a transition with no `at_ms` is dropped silently").toBe(1);
    expect(t.transitions[0].atMs).toBe(1_788_990_000_000);
    expect(t.transitions[0].up).toBe(false);
    expect(t.transitions[0].lastedMs).toBe(300_000);
    // A probe that did not answer has NO latency: null, never a fabricated number.
    expect(t.series.length).toBe(2);
    expect(t.series[1].ms).toBeNull();
  });

  it("drops what it cannot place in time, in both arrays, rather than guessing", () => {
    const { targets } = parseMonitors({
      targets: [
        target({
          transitions: [{ up: true, lasted_ms: 1000 }],
          series: [{ ok: true, ms: 5 }],
        }),
      ],
    });
    expect(targets[0].transitions).toEqual([]);
    expect(targets[0].series).toEqual([]);
  });

  it("keeps a latency that was never measured NULL, not zero", () => {
    const { targets } = parseMonitors({
      targets: [target({ summary: { probes: 0, up: 0, down: 0, up_pct: null, up_now: null, since_ms: null, drops: null, latency: null } })],
    });
    expect(targets[0].summary.latency).toBeNull();
    expect(targets[0].summary.upPct, "'no probes yet' is unknown, never 0%").toBeNull();
  });

  it("answers a body it cannot use with an empty list, never a throw", () => {
    for (const junk of [null, undefined, 42, "text", {}, { targets: "nope" }]) {
      expect(parseMonitors(junk).targets).toEqual([]);
    }
  });
});

// ── THE PUSH THAT FEEDS THE ALERT STRIP ──────────────────────────────────────────────────────────
//
// `useMonitorAlerts` renders nothing until the DEVICE pushes a state change over the SSE stream — the
// `vale-monitor-change` event round 94's vocabulary contract pins as one of the four the device emits.
// That is why the strip measured empty in this round's first pass with a down target on screen: no
// push, no strip. Not a defect, and worth stating because it looks exactly like one.
//
// Measured once the frame is dispatched (round 101): the strip says
// "192.168.1.1:8000 is DOWN — it had been up 15m (HTTP 502)", carries role="status" and
// aria-live="polite" (announced without interrupting), and measures 15.31 light / 11.42 dark.
describe("parseMonitorChange — the frame the device pushes", () => {
  const frame = {
    ev: "monitor-change",
    id: "mon-ont",
    host: "192.168.1.1",
    port: 8000,
    up: false,
    lasted_ms: 900_000,
    at_ms: 1_789_002_000_000,
    status: 502,
  };

  it("reads a real change, keyed so a repeat replaces rather than repeats", () => {
    const alert = parseMonitorChange(frame);
    expect(alert).not.toBeNull();
    expect(alert!.host).toBe("192.168.1.1");
    expect(alert!.port).toBe(8000);
    expect(alert!.up).toBe(false);
    expect(alert!.lastedMs).toBe(900_000);
    expect(alert!.status).toBe(502);
    // The key is id + timestamp, so the same transition arriving twice does not stack in the strip.
    expect(alert!.key).toBe("mon-ont:1789002000000");
    expect(parseMonitorChange(frame)!.key).toBe(alert!.key);
  });

  it("refuses a frame that is not this event, or that cannot be placed in time", () => {
    // The `ev` guard is what keeps one window event from being handled by the wrong listener — and a
    // frame with no stamp cannot be keyed, so it is dropped rather than shown twice.
    expect(parseMonitorChange({ ...frame, ev: "sessions-changed" })).toBeNull();
    expect(parseMonitorChange({ ...frame, at_ms: undefined })).toBeNull();
    expect(parseMonitorChange({ ...frame, id: "" })).toBeNull();
    expect(parseMonitorChange(null)).toBeNull();
  });

  it("keeps an unknown port or status absent rather than inventing one", () => {
    const bare = parseMonitorChange({ ev: "monitor-change", id: "m1", at_ms: 1_789_000_000_000 });
    expect(bare!.port, "a missing port is 0 only because the alert renders host:port").toBe(0);
    expect(bare!.status, "no HTTP status is null, not 200").toBeNull();
    // AND AN ABSENT HOST IS THE EMPTY STRING, not null: this file's `str` helper returns "" where the
    // boot-history hook's returns null. I asserted null and was wrong — the alert then renders as
    // ":8000", which the DEVICE never produces (it always sends host from the monitor row), so the
    // default is defensive rather than reachable. Recorded because the two helpers disagree and the
    // difference is invisible until a test makes you look.
    expect(bare!.host).toBe("");
  });
});

describe("the monitors fixture, end to end", () => {
  it("parses the example the DEVICE promises, value for value", () => {
    const parsed = parseMonitors(fixture.example);
    // the envelope
    expect(parsed.intervalSecs).toBe(15);
    expect(parsed.seriesMax).toBe(240);
    const t = parsed.targets[0];
    expect(t.id).toBe("mon-router");
    expect(t.host).toBe("192.168.1.1");
    expect(t.port).toBe(22);
    // EVERY KEY the fixture promises for the panel, with the value the fixture carries: a renamed field on the
    // device falls back to a default here, so this is what turns a silently empty card into a failure.
    expect(t.summary.probes).toBe(240);
    expect(t.summary.up).toBe(237);
    expect(t.summary.down).toBe(3);
    expect(t.summary.upPct).toBe(99);
    expect(t.summary.upNow).toBe(true);
    expect(t.summary.sinceMs).toBe(1789700000000);
    expect(t.summary.drops).toBe(2);
    expect(t.summary.latency).toEqual({ min: 4, avg: 9, max: 41 });
    expect(t.summary.lastStatus).toBeNull();
    expect(t.summary.lastExpectOk).toBeNull();
    // the two arrays, whose entry shapes are their own contract
    expect(t.transitions).toEqual([{ atMs: 1789700000000, up: true, lastedMs: 1800000 }]);
    expect(t.series).toEqual([
      { tsMs: 1789699990000, ok: true, ms: 9 },
      { tsMs: 1789700000000, ok: true, ms: 7 },
    ]);
  });

  it("reads the HTTP target's two extra facts, which a TCP target leaves null", () => {
    const parsed = parseMonitors({ targets: [{ ...fixture.example.targets[0], ...fixture.example_http }], interval_secs: 15, series_max: 240 });
    const s = parsed.targets[0].summary;
    expect(s.lastStatus).toBe(503);
    expect(s.lastExpectOk).toBe(false);
    expect(parsed.targets[0].path).toBe("/health");
    expect(parsed.targets[0].expect).toBe("ok");
  });
});
