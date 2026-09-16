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
import { parseMonitors } from "../useMonitors";

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
