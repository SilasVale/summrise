import { describe, expect, it } from "vitest";
import type { AuditLine } from "./api";
import { activityCountLine, activityCounts, activityCountSentence, isClientCall } from "./activitySummary";

// The Activity header's count is the same fact the hub's day line states, from a window that can
// also hold yesterday's rows. The three buckets are disjoint: a console read is neither the AI's
// call nor an unattributed one, and the failure count the line leads with is the AI's alone — the
// console's failures are named apart so "failed" cannot mean two things on two pages.

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
    expect(counts).toEqual({
      client: 2,
      clientFailed: 0,
      console: 1,
      consoleFailed: 0,
      unattributed: 1,
      unattributedFailed: 0
    });
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
      clientFailed: 0,
      console: 0,
      consoleFailed: 0,
      unattributed: 0,
      unattributedFailed: 0
    });
  });

  it("counts today's rows only, and each failure in the bucket that holds it", () => {
    const counts = activityCounts(
      [
        line({ caller: "client" }),
        line({ token_id: "b".repeat(64), status: 500 }),
        line({ caller: "console", status: 424 }),
        line({ detail: "jsonrpc error -32002" }),
        line({ ts_ms: NOW - DAY, caller: "console" }),
        line({ ts_ms: NOW - 2 * DAY })
      ],
      NOW
    );
    expect(counts).toEqual({
      client: 2,
      clientFailed: 1,
      console: 1,
      consoleFailed: 1,
      unattributed: 1,
      unattributedFailed: 1
    });
  });
});

describe("activityCountSentence", () => {
  const counts = {
    client: 301,
    clientFailed: 37,
    console: 627,
    consoleFailed: 15,
    unattributed: 21,
    unattributedFailed: 2
  };

  it("states the AI's calls and failures first, then the console's failures and the unknown", () => {
    // The measured header read "301 client calls · 55 failed · 21 unattributed"; 18 of those 55 were
    // not the AI's (15 console, 2 unattributed). The number after "client calls" is now the AI's
    // failures only, and the console's are named for what they are.
    expect(activityCountSentence(counts)).toBe(
      "Today 301 client calls · 37 failed · 15 console failures · 21 unattributed (2 failed)"
    );
  });

  it("names the console's failures apart, so the primary count cannot be read as every caller's", () => {
    // The hub's own sentence is client-only ("301 client calls · 37 failed"); the extra clause is
    // the console's, and it is not folded into the number beside the client calls.
    expect(activityCountSentence(counts)).not.toContain("55 failed");
    expect(activityCountSentence({ ...counts, consoleFailed: 0 })).toBe(
      "Today 301 client calls · 37 failed · 21 unattributed (2 failed)"
    );
  });

  it("shows the client subset, and nothing the chip hides, when the chip is on", () => {
    // "Clients only" draws the AI's rows; its header is then the hub's sentence: no console clause,
    // no unattributed count, and the same failure number the visible rows carry.
    expect(activityCountSentence(counts, true)).toBe("Today 301 client calls · 37 failed");
    expect(activityCountSentence({ ...counts, clientFailed: 0 }, true)).toBe(
      "Today 301 client calls · 0 failed"
    );
  });

  it("says one client call, not one client calls", () => {
    expect(
      activityCountSentence(
        {
          client: 1,
          clientFailed: 0,
          console: 0,
          consoleFailed: 0,
          unattributed: 0,
          unattributedFailed: 0
        }
      )
    ).toBe("Today 1 client call · 0 failed · 0 unattributed");
  });

  it("reads its sentence from the window and the clock", () => {
    expect(activityCountLine([line({ caller: "client" })], NOW)).toBe(
      "Today 1 client call · 0 failed · 0 unattributed"
    );
  });
});
