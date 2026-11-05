import { describe, expect, it } from "vitest";
import type { AuditLine } from "./api";
import { activityCountLine, activityCounts, activityCountSentence, isClientCall } from "./activitySummary";

// The Activity header's count is the same fact the hub's day line states, from a window that can
// also hold yesterday's rows. The three buckets are disjoint: a console read is neither the AI's
// call nor an unattributed one, and a failure is a failure whichever bucket the row fell into.

const NOW = new Date(2026, 10, 5, 12, 0, 0).getTime();
const DAY = 86_400_000;
const TOKEN = "a".repeat(64);

function line(overrides: Partial<AuditLine> = {}): AuditLine {
  return { ts_ms: NOW - 60_000, kind: "mcp", status: 200, ...overrides };
}

describe("activityCounts", () => {
  it("counts a call as the client's when the caller says so, or when a token proves it", () => {
    // The live window's 908 caller-less rows split on exactly this: 314 carried a token, 594 did not.
    const counts = activityCounts(
      [
        line({ caller: "client" }),
        line({ token_id: TOKEN }),
        line({ caller: "console" }),
        line({})
      ],
      NOW
    );
    expect(counts).toEqual({ client: 2, console: 1, failed: 0, unattributed: 1 });
  });

  it("keeps the console's own rows out of unattributed", () => {
    // Thirty console reads were being read as thirty unattributed calls; the page's own polling is
    // neither the AI working nor a call nobody can explain.
    const counts = activityCounts(
      [line({ caller: "console" }), line({ caller: "console" }), line({})],
      NOW
    );
    expect(counts.console).toBe(2);
    expect(counts.unattributed).toBe(1);
  });

  it("counts the token before an absent caller, never the console's", () => {
    expect(isClientCall(line({ token_id: TOKEN }))).toBe(true);
    expect(isClientCall(line({ caller: "client" }))).toBe(true);
    // A token on a row the console marked is still the console's; the marker is the stronger fact.
    expect(isClientCall(line({ caller: "console", token_id: TOKEN }))).toBe(false);
  });

  it("never counts a channel boundary as a call or an activity row", () => {
    // `device.left` / `device.joined` are the console's own record of a channel, not a tool call.
    // They carry `caller: "console"` so the client rule already refuses them; the day's counters
    // must not turn them into console rows either.
    const left = line({ caller: "console", kind: "device", tool: "device.left" });
    const joined = line({ caller: "console", kind: "device", tool: "device.joined" });
    expect(isClientCall(left)).toBe(false);
    expect(isClientCall(joined)).toBe(false);
    expect(activityCounts([left, joined, line({ caller: "client" })], NOW)).toEqual({
      client: 1,
      console: 0,
      failed: 0,
      unattributed: 0
    });
  });

  it("counts today's rows only, and a failure in any bucket", () => {
    const counts = activityCounts(
      [
        line({ caller: "client" }),
        line({ token_id: "b".repeat(64), status: 500 }),
        line({ ts_ms: NOW - DAY, caller: "console" }),
        line({ ts_ms: NOW - 2 * DAY })
      ],
      NOW
    );
    expect(counts).toEqual({ client: 2, console: 0, failed: 1, unattributed: 0 });
  });
});

describe("activityCountSentence", () => {
  it("states the day's client calls, failures and unattributed rows", () => {
    expect(
      activityCountSentence({ client: 363, console: 30, failed: 12, unattributed: 587 })
    ).toBe("Today 363 client calls · 12 failed · 587 unattributed");
  });

  it("says one client call, not one client calls", () => {
    expect(
      activityCountSentence({ client: 1, console: 0, failed: 0, unattributed: 0 })
    ).toBe("Today 1 client call · 0 failed · 0 unattributed");
  });

  it("reads its sentence from the window and the clock", () => {
    expect(activityCountLine([line({ caller: "client" })], NOW)).toBe(
      "Today 1 client call · 0 failed · 0 unattributed"
    );
  });
});
