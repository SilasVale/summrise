import { describe, expect, it } from "vitest";
import { mergeActivity } from "./activityMerge";

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
});
