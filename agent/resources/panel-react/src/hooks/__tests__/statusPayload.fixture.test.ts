// THE STATUS PAYLOAD, read from the fixture the device is checked against.
//
// `agent/tests/fixtures/status.json` is read by `api_status`'s own test in agent/src/web/mod.rs (which
// asserts against the REAL response, not a reconstruction) and by this one. `/api/status` is the first
// call the panel makes, and it reads the fields silently — a renamed `uptime_secs` is a blank strip, a
// renamed `last_boot_kind` is an "unrecorded" verdict. Round 98's coverage list named this route as the
// next boundary to pin; this is that pin, and its two halves are the two failure directions: the panel
// must read keys the device sends, and must survive a payload that omits the conditional ones.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseLastBoot } from "../useAgentVitals";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE = path.resolve(HERE, "..", "..", "..", "..", "..", "tests", "fixtures", "status.json");

describe("the /api/status payload", () => {
  const fixture = JSON.parse(readFileSync(FIXTURE, "utf8"));

  it("names a stable key set and a conditional one, with a reason for each", () => {
    expect(fixture.stable_keys.length).toBeGreaterThanOrEqual(5);
    const conditional = Object.keys(fixture.conditional_keys);
    expect(conditional.length).toBeGreaterThanOrEqual(3);
    for (const [k, why] of Object.entries(fixture.conditional_keys)) {
      expect(String(why).length, `${k} has no reason recorded`).toBeGreaterThan(20);
    }
    // The panel's required reads must all be declared somewhere in the fixture.
    const declared = new Set([...fixture.stable_keys, ...conditional]);
    for (const k of fixture.required_by_panel) {
      expect(declared.has(k), `the panel requires ${k}, which the fixture never declares`).toBe(true);
    }
  });

  it("reads the boot verdict from the pair the device sends together", () => {
    // The device's own comment (round 256): `last_boot` is the sentence a human reads, `last_boot_kind`
    // the same verdict as data, and they ride the same response. A kind with no sentence is DROPPED by
    // this parser rather than rendered as a bare word.
    expect(parseLastBoot(fixture.example)).toEqual({
      kind: "crashed",
      detail: "2026-09-13 04:12:03 +08:00 — unexpected exit",
    });
    // The steady state: no verdict on record is a fact, not an error.
    expect(parseLastBoot({ ok: true, version: "1.2.433" })).toBeNull();
    // Half a pair is not a verdict.
    expect(parseLastBoot({ last_boot_kind: "crashed" }), "a kind with no sentence").toBeNull();
    // And a vocabulary this build does not know is not borrowed from another kind.
    const unknown = parseLastBoot({ last_boot: "something happened", last_boot_kind: "melted" });
    expect(unknown).toEqual({ kind: null, detail: "something happened" });
  });

  it("survives the payload a HEALTHY device sends, which omits every conditional field", () => {
    // This is the shape that broke a field once before (the device's comment records it): the steady
    // state has nothing pending, no vitals sample on a non-Windows host, and no boot verdict on a fresh
    // install. The parser must not need any of them.
    const steady = { ok: true, version: "1.2.433", port: 18080, uptime_secs: 5412, live_sessions: 0, serial_ports: [] };
    expect(parseLastBoot(steady)).toBeNull();
    for (const k of Object.keys(fixture.conditional_keys)) {
      expect(k in steady, `${k} is conditional — a steady payload should not carry it`).toBe(false);
    }
  });
});
