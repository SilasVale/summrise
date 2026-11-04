import { describe, expect, it } from "vitest";
import { capActivity, capSentence, mergeActivity, mergeActivityWindow } from "./activityMerge";

const audit = (ts_ms: number, tool: string, status = 200) => ({ ts_ms, kind: "mcp", tool, status });
const live = (ts_ms: number, tool: string, sentence: string, status = 200) => ({
  ts_ms, kind: "mcp", tool, status, sentence
});

describe("merging the fetched window with the live stream", () => {
  it("puts the live lines first and shows a line seen twice once", () => {
    const merged = mergeActivity(
      [audit(30, "terminal.list"), audit(20, "files.read")],
      [live(30, "terminal.list", "Listed the terminal sessions")]
    );
    expect(merged.map((line) => line.ts_ms)).toEqual([30, 20]);
  });

  it("keeps two different calls that share a millisecond", () => {
    const merged = mergeActivity([], [live(10, "files.read", "Read `a`"), live(10, "files.write", "Wrote `b`")]);
    expect(merged).toHaveLength(2);
  });

  it("is the fetched window when the stream has nothing", () => {
    expect(mergeActivity([audit(1, "files.read")], [])).toHaveLength(1);
  });

  it("keeps the newest 1000 and reports the cut when the merge exceeds the console's cap", () => {
    const fetched = Array.from({ length: 1000 }, (_, index) => audit(1_000 - index, "files.read"));
    const newer = Array.from({ length: 2 }, (_, index) => live(2_000 - index, "files.read", "Read"));
    const window = mergeActivityWindow(fetched, newer);

    expect(window.lines).toHaveLength(1000);
    expect(window.capped).toBe(true);
    // The tail dropped is the oldest end of the fetched window, not one of the live lines.
    expect(window.lines[0].ts_ms).toBe(2_000);
    expect(window.lines[999].ts_ms).toBe(3);
  });

  it("does not claim a cut when the list is exactly the cap", () => {
    const lines = Array.from({ length: 1000 }, (_, index) => audit(1_000 - index, "files.read"));
    const window = capActivity(lines);
    expect(window.lines).toHaveLength(1000);
    expect(window.capped).toBe(false);
  });

  it("describes the cut as a window, never as a day", () => {
    expect(capSentence()).toBe(
      "Showing the newest 1000 lines; older ones are not on this page."
    );
  });
});
