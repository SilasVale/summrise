import { describe, it, expect } from "vitest";
import { humanIdle } from "./duration";
import { humanIdle as reExportedByEvicted } from "./evicted";
import { humanIdle as importedByIdleSessions } from "./idleSessions";
import { fmtUptime } from "../hooks/useAgentVitals";

// THE PANEL'S DURATION VOCABULARY, pinned where it had drifted (found 2026-09-24 by the panel
// exploration's duration-family sweep). Two modules each kept a private `humanIdle`: both emitted
// `1h04m` where the rest of the panel writes `1h 04m`, and they disagreed with each other below a
// minute (`45s` vs `0m` — a false statement about a 45-second silence, reached by branch order).
describe("durations read the same everywhere", () => {
  it("spells a silence the way the panel does", () => {
    expect(humanIdle(45_000)).toBe("45s");
    expect(humanIdle(4 * 60_000)).toBe("4m");
    expect(humanIdle(3_600_000 + 4 * 60_000)).toBe("1h 04m");
    expect(humanIdle(11 * 3_600_000)).toBe("11h 00m");
    expect(humanIdle(-1)).toBe("0s");
  });

  it("is ONE function, re-exported — identity is the assertion that stops the drift returning", () => {
    expect(reExportedByEvicted).toBe(humanIdle);
    expect(importedByIdleSessions).toBe(humanIdle);
  });

  it("pads the minutes in an uptime too, which is the shape its own doc already claimed", () => {
    // `fmtUptime`'s doc: "`45s`, `12m 30s`, `3h 05m`, `2d 4h`" — the code emitted `3h 5m` until the
    // same sweep, so the doc was right and the function was not.
    expect(fmtUptime(3 * 3600 + 5 * 60)).toBe("3h 05m");
    expect(fmtUptime(45)).toBe("45s");
    expect(fmtUptime(2 * 86400 + 4 * 3600)).toBe("2d 4h");
  });
});
