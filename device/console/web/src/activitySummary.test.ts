import { describe, expect, it } from "vitest";
import type { AuditLine } from "./api";
import {
  activityCountLine,
  activityCounts,
  activityCountSentence,
  activityDaySentence,
  isClientCall,
  isConsoleRead
} from "./activitySummary";

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
      poll: 0,
      pollFailed: 0,
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
    // The console's poll is not the AI's call even if a token were ever attached to it.
    expect(isClientCall(line({ kind: "poll", caller: "console", token_id: TOKEN }))).toBe(false);
  });

  it("marks only the console's own caller as a console read", () => {
    // The hub folds exactly these rows. An absent caller is unattributed, not the console's: the
    // console now always names itself, and an older row is not proof of who made the call.
    expect(isConsoleRead(line({ caller: "console" }))).toBe(true);
    expect(isConsoleRead(line({ caller: "client" }))).toBe(false);
    expect(isConsoleRead(line({ token_id: TOKEN }))).toBe(false);
    expect(isConsoleRead(line({}))).toBe(false);
    // A failed poll is a console fact but not a read of the screen: it belongs to its own count.
    expect(isConsoleRead(line({ kind: "poll", caller: "console" }))).toBe(false);
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
      poll: 0,
      pollFailed: 0,
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
      poll: 0,
      pollFailed: 0,
      unattributed: 1,
      unattributedFailed: 1
    });
  });

  it("counts a failed poll apart from the AI's failures and the console's", () => {
    // The measured record's 41 `device reconnecting` lines were a sleeping machine's retries, not
    // the AI's refusals. They stay rows (the table draws them) but leave every failed count.
    const counts = activityCounts(
      [
        line({ caller: "client" }),
        line({ caller: "client", status: 424 }),
        line({ caller: "console", status: 500 }),
        line({
          kind: "poll",
          caller: "console",
          status: 503,
          detail: "device reconnecting",
          tool: "terminal.read"
        }),
        line({
          kind: "poll",
          caller: "console",
          status: 504,
          detail: "device did not answer",
          tool: "terminal.list"
        })
      ],
      NOW
    );
    expect(counts).toEqual({
      client: 2,
      clientFailed: 1,
      console: 1,
      consoleFailed: 1,
      poll: 2,
      pollFailed: 2,
      unattributed: 0,
      unattributedFailed: 0
    });
    expect(activityCountSentence(counts)).toBe(
      "Today: 2 client calls, 1 failed — 1 console failure; 2 poll failures."
    );
  });
});

describe("activityCountSentence", () => {
  const counts = {
    client: 301,
    clientFailed: 37,
    console: 627,
    consoleFailed: 15,
    poll: 0,
    pollFailed: 0,
    unattributed: 21,
    unattributedFailed: 2
  };

  it("states the AI's calls and failures first, then the console's failures and the unknown", () => {
    // The measured header read "301 client calls · 55 failed · 21 unattributed"; 18 of those 55 were
    // not the AI's (15 console, 2 unattributed). The number after "client calls" is now the AI's
    // failures only, and the console's are named for what they are.
    expect(activityCountSentence(counts)).toBe(
      "Today: 301 client calls, 37 failed — 15 console failures; 21 unattributed (2 failed)."
    );
  });

  it("names the console's failures apart, so the primary count cannot be read as every caller's", () => {
    // The hub's own sentence is client-only ("301 client calls, 37 failed"); the extra clause is
    // the console's, and it is not folded into the number beside the client calls.
    expect(activityCountSentence(counts)).not.toContain("55 failed");
    expect(activityCountSentence({ ...counts, consoleFailed: 0 })).toBe(
      "Today: 301 client calls, 37 failed — 21 unattributed (2 failed)."
    );
  });

  it("gives the console's failures their noun instead of a bare number after the dash", () => {
    // The measured header read `Today: 1 client call — 2 from the console.` while the day held 59
    // console reads: the 2 are the console's failures, and the sentence never said so (audit-40
    // F9). The number and its noun are the same fact, singular or plural.
    const one = {
      client: 1,
      clientFailed: 0,
      console: 59,
      consoleFailed: 2,
      poll: 0,
      pollFailed: 0,
      unattributed: 0,
      unattributedFailed: 0
    };
    expect(activityCountSentence(one)).toBe("Today: 1 client call — 2 console failures.");
    expect(activityCountSentence(one)).not.toContain("from the console");
    expect(activityCountSentence({ ...one, consoleFailed: 1 })).toBe(
      "Today: 1 client call — 1 console failure."
    );
  });

  it("reads the operator's measured sentence when the other buckets are zero", () => {
    // The brief's example: client calls, failed, console failures, and nothing else. A zero poll
    // and zero unattributed rows must not become clauses; the sentence ends after the console.
    expect(
      activityCountSentence({
        client: 495,
        clientFailed: 29,
        console: 627,
        consoleFailed: 27,
        poll: 20,
        pollFailed: 0,
        unattributed: 0,
        unattributedFailed: 0
      })
    ).toBe("Today: 495 client calls, 29 failed — 27 console failures.");
  });

  it("reads the day header for the measured shape: three client failures, two console, 41 poll", () => {
    // The measured window's shape at the moment this work started: 41 reconnect-window poll
    // failures were inside the day's `failed` count and read as the AI's. The header now names
    // them in their own clause and never adds them to the number beside "client calls".
    expect(
      activityCountSentence({
        client: 3,
        clientFailed: 3,
        console: 2,
        consoleFailed: 2,
        poll: 41,
        pollFailed: 41,
        unattributed: 0,
        unattributedFailed: 0
      })
    ).toBe(
      "Today: 3 client calls, 3 failed — 2 console failures; 41 poll failures."
    );
  });

  it("drops the zero clauses instead of printing a counter for them", () => {
    // The old line always ended `0 unattributed` and always printed `0 failed`. A zero is not a
    // fact the operator needs; the sentence keeps the day's work and the failures that exist.
    expect(
      activityCountSentence({
        client: 10,
        clientFailed: 0,
        console: 0,
        consoleFailed: 0,
        poll: 0,
        pollFailed: 0,
        unattributed: 0,
        unattributedFailed: 0
      })
    ).toBe("Today: 10 client calls.");
  });

  it("omits the poll clause when no poll failed", () => {
    const day = {
      client: 495,
      clientFailed: 29,
      console: 627,
      consoleFailed: 27,
      poll: 20,
      pollFailed: 0,
      unattributed: 34,
      unattributedFailed: 0
    };
    expect(activityCountSentence(day)).toBe(
      "Today: 495 client calls, 29 failed — 27 console failures; 34 unattributed."
    );
    expect(activityCountSentence(day)).not.toMatch(/poll/i);
  });

  it("keeps every nonzero bucket in the measured day, and one `client calls` noun", () => {
    // The measured ux97 day. The example sentence's core is the first three clauses; the failed
    // poll and the unattributed rows are still numbers the page reads, so they stay in the same
    // sentence. The noun phrase appears exactly once, with today as its only window.
    const measured = {
      client: 495,
      clientFailed: 29,
      console: 627,
      consoleFailed: 27,
      poll: 20,
      pollFailed: 1,
      unattributed: 34,
      unattributedFailed: 0
    };
    const sentence = activityCountSentence(measured);
    expect(sentence).toBe(
      "Today: 495 client calls, 29 failed — 27 console failures; 1 poll failure; 34 unattributed."
    );
    expect(sentence.match(/client calls?/g)).toHaveLength(1);
    expect(sentence.startsWith("Today: 495 client calls, 29 failed — 27 console failures")).toBe(
      true
    );
  });

  it("says one client call and one poll failure, not the plurals", () => {
    expect(
      activityCountSentence({
        client: 1,
        clientFailed: 0,
        console: 0,
        consoleFailed: 0,
        poll: 0,
        pollFailed: 0,
        unattributed: 0,
        unattributedFailed: 0
      })
    ).toBe("Today: 1 client call.");
    expect(
      activityCountSentence({
        client: 2,
        clientFailed: 1,
        console: 0,
        consoleFailed: 0,
        poll: 0,
        pollFailed: 1,
        unattributed: 0,
        unattributedFailed: 0
      })
    ).toBe("Today: 2 client calls, 1 failed — 1 poll failure.");
  });

  it("reads its sentence from the window and the clock", () => {
    expect(activityCountLine([line({ caller: "client" })], NOW)).toBe("Today: 1 client call.");
  });

  it("states the day's route counts, even when its window holds fewer", () => {
    // The measured disagreement: the header said 269 while the file's day held 649. The day route
    // is the sentence's source now; the window is only the cap sentence's subject.
    const day = {
      client_calls: 649,
      failed: 74,
      console: 15,
      console_failed: 2,
      poll: 41,
      poll_failed: 41,
      unattributed: 3,
      unattributed_failed: 1
    };
    expect(activityDaySentence(day)).toBe(
      "Today: 649 client calls, 74 failed — 2 console failures; 41 poll failures; " +
        "3 unattributed (1 failed)."
    );
    // The filter no longer changes this sentence: it is the day's, and the filter's own count is
    // labelled with its own window. A day with no console failure is the same shape minus that
    // clause.
    expect(activityDaySentence({ ...day, console_failed: 0 })).toBe(
      "Today: 649 client calls, 74 failed — 41 poll failures; 3 unattributed (1 failed)."
    );
  });

  it("adds the machine's direct calls to the day's number and names their share", () => {
    const day = {
      client_calls: 10,
      failed: 1,
      console: 0,
      console_failed: 0,
      poll: 0,
      poll_failed: 0,
      unattributed: 0,
      unattributed_failed: 0
    };
    expect(activityDaySentence(day, { calls: 2, failed: 1, more: false })).toBe(
      "Today: 12 client calls, 2 failed — 2 straight to this machine (1 failed)."
    );
    // A single direct call, and a failure clause only when there is one.
    expect(activityDaySentence(day, { calls: 1, failed: 0, more: false })).toBe(
      "Today: 11 client calls, 1 failed — 1 straight to this machine."
    );
    // The machine's scan was cut: the number is a floor and says so.
    expect(activityDaySentence(day, { calls: 3, failed: 0, more: true })).toBe(
      "Today: 13 client calls, 1 failed — at least 3 straight to this machine."
    );
    // A machine that counted nothing adds nothing: a zero is not a clause.
    expect(activityDaySentence(day, { calls: 0, failed: 0, more: false })).toBe(
      "Today: 10 client calls, 1 failed."
    );
    expect(activityDaySentence(day, null)).toBe("Today: 10 client calls, 1 failed.");
  });

  it("says since when the day's file starts after midnight", () => {
    const since = new Date(2026, 10, 5, 3, 12, 0).getTime();
    const day = {
      client_calls: 10,
      failed: 1,
      console: 0,
      console_failed: 0,
      poll: 0,
      poll_failed: 0,
      unattributed: 0,
      unattributed_failed: 0,
      since_ms: since
    };
    expect(activityDaySentence(day)).toBe(
      "Today: 10 client calls, 1 failed · since 03:12."
    );
    // The bound is a second way a day is partial, and it is stated with its name.
    expect(activityDaySentence({ ...day, bounded: true, scan_limit: 50_000 })).toBe(
      "Today: 10 client calls, 1 failed · since 03:12 · first 50,000 lines."
    );
  });
});
