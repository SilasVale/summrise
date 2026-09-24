// The eviction line: what the device closed, and which rule closed it.
//
// The frame is the device's (`{"ev":"session-evicted","cause","limit","sessions":[…]}`); these tests
// pin BOTH directions — a real frame becomes one readable line, and anything this build cannot
// describe becomes NOTHING rather than a notice about something that did not happen.
import { describe, it, expect } from "vitest";
import { evictedText, humanIdle, parseEvicted } from "../evicted";

const frame = {
  ev: "session-evicted",
  cause: "cap",
  limit: 16,
  sessions: [
    {
      id: "term-ab-7",
      label: "d1",
      kind: "pty",
      idle_ms: 3 * 3600_000,
      reason: "session cap (16) reached",
    },
  ],
};

describe("session evictions", () => {
  it("reads the device's frame", () => {
    const n = parseEvicted(frame)!;
    expect(n.cause).toBe("cap");
    expect(n.limit).toBe(16);
    expect(n.sessions[0].label).toBe("d1");
    // …and says what happened, naming the session and the rule.
    expect(evictedText(n)).toContain("d1");
    expect(evictedText(n)).toContain("16-session cap");
  });

  it("reads an idle reaping, with how long the session had been silent", () => {
    const n = parseEvicted({
      ev: "session-evicted",
      cause: "idle",
      limit: 900,
      sessions: [
        {
          id: "term-ab-9",
          label: "serial:COM4",
          kind: "serial",
          idle_ms: 11 * 3600_000,
          reason: "silent for 39960s (idle TTL)",
        },
      ],
    })!;
    expect(evictedText(n)).toContain("11h 00m");
    expect(evictedText(n)).toContain("serial:COM4");
    expect(evictedText(n)).toContain("15-minute limit");
  });

  it("draws NOTHING for a frame it cannot describe", () => {
    // Another event on the same stream, a missing cause, an empty list, junk.
    expect(parseEvicted({ ev: "monitor-change" })).toBeNull();
    expect(
      parseEvicted({ ev: "session-evicted", sessions: frame.sessions }),
    ).toBeNull();
    expect(
      parseEvicted({ ev: "session-evicted", cause: "cap", sessions: [] }),
    ).toBeNull();
    expect(
      parseEvicted({
        ev: "session-evicted",
        cause: "melted",
        sessions: frame.sessions,
      }),
    ).toBeNull();
    expect(
      parseEvicted({
        ev: "session-evicted",
        cause: "cap",
        sessions: [{ label: "no id" }],
      }),
    ).toBeNull();
    expect(parseEvicted(null)).toBeNull();
    expect(parseEvicted(undefined)).toBeNull();
  });

  it("names a session without a label by its id, and one eviction in the singular", () => {
    const n = parseEvicted({
      ev: "session-evicted",
      cause: "idle",
      limit: 900,
      sessions: [
        {
          id: "term-ab-3",
          label: "",
          kind: "pty",
          idle_ms: 1000,
          reason: "silent for 1s (idle TTL)",
        },
      ],
    })!;
    expect(evictedText(n)).toContain("term-ab-3");
    expect(evictedText(n)).toContain("Closed session");
  });

  // THIS TEST'S OWN NAME WAS THE BUG. "formats the way the rest of the panel does" was false: it pinned
  // `3h05m`, a shape only this file and idleSessions.ts used, while `useAttention` and `useMonitors`
  // render `3h 05m` — and the two private copies disagreed with each other below a minute too (`45s`
  // vs `0m`). The shape is now one owner, `lib/duration.ts`, and this pins the panel's spelling.
  it("humanIdle formats the way the rest of the panel does", () => {
    expect(humanIdle(5_000)).toBe("5s");
    expect(humanIdle(90_000)).toBe("1m");
    expect(humanIdle(3 * 3600_000 + 5 * 60_000)).toBe("3h 05m");
    expect(humanIdle(-1)).toBe("0s");
  });
});
