import { describe, expect, it } from "vitest";
import type { AuditLine, JoinAnswer } from "./api";
import {
  JOIN_TRACES,
  joinCounts,
  joinKind,
  joinSentence,
  joinTraces,
  markOf,
  type JoinKind
} from "./joinMarks";

function line(over: Partial<AuditLine> = {}): AuditLine {
  return {
    ts_ms: 1,
    kind: "mcp",
    device_id: "d".repeat(64),
    status: 200,
    tool: "files.read",
    ...over
  };
}

function answer(over: Partial<JoinAnswer> = {}): JoinAnswer {
  return {
    device: "d".repeat(64),
    asked: 0,
    joined: [],
    missing: [],
    unknown: false,
    journal: true,
    bounded: false,
    oldest_seq: 1,
    last_seq: 9,
    read_at_ms: 1,
    ...over
  };
}

describe("joinTraces", () => {
  it("asks about the calls' own traces, in the order they are drawn", () => {
    const lines = [
      line({ trace: "console-1", ts_ms: 30 }),
      line({ trace: "console-2", ts_ms: 20 }),
      line({ trace: "console-1", ts_ms: 10 })
    ];
    expect(joinTraces(lines)).toEqual(["console-1", "console-2"]);
  });

  it("leaves out every row that proxied nothing", () => {
    // A grant, a token, a sign-in and a channel boundary are the operator's acts, not a machine
    // call: there is no device row for them to join to, and asking about them would draw a call
    // the machine never ran.
    const lines = [
      line({ kind: "grant", trace: "console-1" }),
      line({ kind: "token" }),
      line({ kind: "device", tool: "device.joined" }),
      line({ kind: "mcp", trace: "" }),
      line({ kind: "mcp" }),
      line({ kind: "mcp", trace: "console-9" })
    ];
    expect(joinTraces(lines)).toEqual(["console-9"]);
  });

  it("stops at the console's bound rather than sending a list it will refuse", () => {
    const lines = Array.from({ length: JOIN_TRACES + 20 }, (_, index) =>
      line({ trace: `console-${index}` })
    );
    const traces = joinTraces(lines);
    expect(traces).toHaveLength(JOIN_TRACES);
    // Newest first: the list is drawn newest first, so the kept half is the half on screen.
    expect(traces[0]).toBe("console-0");
    expect(traces[JOIN_TRACES - 1]).toBe(`console-${JOIN_TRACES - 1}`);
  });
});

describe("joinKind", () => {
  it("is one when the machine still holds the call", () => {
    expect(joinKind(answer({ asked: 1, joined: ["a"] }), "a")).toBe("one");
  });

  it("is console when the machine's journal no longer holds it", () => {
    expect(joinKind(answer({ asked: 1, missing: ["a"] }), "a")).toBe("console");
  });

  it("is unknown before any answer and after a failed one", () => {
    // The page draws the rows it has while the join read is out; marking them `console` then would
    // accuse the machine of losing calls it was never asked about.
    expect(joinKind(null, "a")).toBe("unknown");
    expect(joinKind(answer({ unknown: true, reason: "unknown_tool" }), "a")).toBe("unknown");
  });

  it("is unknown for a machine that keeps no journal", () => {
    // An older device answers `journal: false` as a fact. It has not lost the call; it never kept
    // one, and the page must not draw that as a rotation.
    expect(joinKind(answer({ journal: false, oldest_seq: 0, last_seq: 0 }), "a")).toBe("unknown");
  });

  it("is unknown for a trace a bounded scan did not find", () => {
    // The scan stopped before the row's seq could be reached. Absence past the cap is not absence.
    expect(joinKind(answer({ missing: ["a"], bounded: true }), "a")).toBe("unknown");
  });

  it("is one for a trace a bounded scan did find", () => {
    // Presence is proof wherever the scan met it; only the negative is weakened by the cap.
    expect(joinKind(answer({ joined: ["a"], bounded: true }), "a")).toBe("one");
  });

  it("is unknown for a trace the machine's answer does not mention", () => {
    // The device answers every asked trace. One that is in neither array is an answer this page
    // cannot read, so it claims nothing about the row.
    expect(joinKind(answer({ asked: 1, joined: [], missing: [] }), "a")).toBe("unknown");
  });
});

describe("joinCounts", () => {
  it("counts what the window asked and what each trace came to", () => {
    const kinds = new Map<string, JoinKind>([
      ["a", "one"],
      ["b", "console"],
      ["c", "unknown"]
    ]);
    expect(joinCounts(["a", "b", "c", "d"], kinds)).toEqual({
      one: 1,
      console: 1,
      unknown: 2,
      asked: 4
    });
  });
});

describe("joinSentence", () => {
  it("says nothing when every call joined", () => {
    // The proof that the join worked is that nothing needed saying.
    expect(joinSentence({ one: 12, console: 0, unknown: 0, asked: 12 })).toBeNull();
  });

  it("says nothing when there was nothing to ask", () => {
    expect(joinSentence({ one: 0, console: 0, unknown: 0, asked: 0 })).toBeNull();
  });

  it("names the calls only the console holds", () => {
    expect(joinSentence({ one: 9, console: 3, unknown: 0, asked: 12 })).toBe(
      "3 of these 12 calls are in this console's record only — the machine's journal no longer " +
        "holds them."
    );
  });

  it("names one such call in the singular", () => {
    expect(joinSentence({ one: 1, console: 1, unknown: 0, asked: 2 })).toBe(
      "1 of these 2 calls is in this console's record only — the machine's journal no longer holds it."
    );
  });

  it("names the calls it could not check, and does not count them as lost", () => {
    expect(joinSentence({ one: 4, console: 0, unknown: 2, asked: 6 })).toBe(
      "2 were not checked against the machine's journal."
    );
  });

  it("carries both deviations when both are true", () => {
    // Told only about the rotated rows, a reader would take the unchecked ones for joins that
    // worked; the sentence has to hold both facts.
    expect(joinSentence({ one: 2, console: 3, unknown: 1, asked: 6 })).toBe(
      "3 of these 6 calls are in this console's record only — the machine's journal no longer " +
        "holds them; 1 was not checked against the machine's journal."
    );
  });

  it("reads as one sentence for a window of one call", () => {
    expect(joinSentence({ one: 0, console: 1, unknown: 0, asked: 1 })).toBe(
      "This call is in this console's record only — the machine's journal no longer holds it."
    );
    expect(joinSentence({ one: 0, console: 0, unknown: 1, asked: 1 })).toBe(
      "This call was not checked against the machine's journal."
    );
  });
});

describe("markOf", () => {
  const marks = {
    kinds: new Map<string, JoinKind>([["a", "console"]]),
    counts: { one: 0, console: 1, unknown: 0, asked: 1 }
  };

  it("reads the row's own trace", () => {
    expect(markOf(marks, line({ trace: "a" }))).toBe("console");
    expect(markOf(marks, line({ trace: "b" }))).toBe("unknown");
  });

  it("is unknown for a row that names no trace", () => {
    expect(markOf(marks, line())).toBe("unknown");
    expect(markOf(marks, line({ trace: "" }))).toBe("unknown");
  });

  it("is unknown before the answer arrives", () => {
    // The rows are drawn while the join read is out; a mark then would name a check not yet made.
    expect(markOf(null, line({ trace: "a" }))).toBe("unknown");
  });
});

describe("joinSentence's reason for an unchecked call", () => {
  it("says why the console could not check, in the console's own words", () => {
    // "Not checked" alone leaves the operator guessing between an older machine, an offline one and
    // a console that never answered; the reason is the sentence every other failed read shows.
    expect(joinSentence({ one: 0, console: 0, unknown: 3, asked: 3 }, "the machine is not connected")).toBe(
      "3 were not checked against the machine's journal: the machine is not connected."
    );
  });

  it("keeps the singular's own shape", () => {
    expect(joinSentence({ one: 0, console: 0, unknown: 1, asked: 1 }, "the machine did not answer in time")).toBe(
      "This call was not checked against the machine's journal: the machine did not answer in time."
    );
  });

  it("names no cause it was not given", () => {
    expect(joinSentence({ one: 0, console: 0, unknown: 2, asked: 2 })).toBe(
      "2 were not checked against the machine's journal."
    );
  });

  it("does not attach a failure to the calls it did check", () => {
    // The rotated rows and the unchecked ones are different facts; the reason belongs to the second.
    expect(joinSentence({ one: 0, console: 2, unknown: 0, asked: 2 }, "the machine is not connected")).toBe(
      "2 of these 2 calls are in this console's record only — the machine's journal no longer holds them."
    );
  });
});
