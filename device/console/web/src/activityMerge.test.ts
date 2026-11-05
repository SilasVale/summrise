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

  it("names the machine whose window was cut, and only when the window is one machine's", () => {
    expect(capSentence(1000, "workshop")).toBe(
      "Showing the newest 1000 lines from workshop; older ones are not on this page."
    );
    // The fleet's window has no machine to name; the sentence stays the one it has always used.
    expect(capSentence(1000, null)).toBe(
      "Showing the newest 1000 lines; older ones are not on this page."
    );
  });

  it("says the window's limit without claiming a cut", () => {
    // The server answered with exactly the requested 1000. It cannot prove an older line exists, so
    // the sentence names the window's size and claims nothing about what is behind it. Nothing was
    // dropped from the list either, so the window has no machine to blame for a cut.
    expect(capSentence(1000, null, "at-limit")).toBe(
      "Showing the newest 1000 lines — the window's limit."
    );
    expect(capSentence(1000, "workshop", "at-limit")).toBe(
      "Showing the newest 1000 lines — the window's limit."
    );
  });

  it("reports a fetched window that filled the request", () => {
    const fetched = Array.from({ length: 1000 }, (_, index) => audit(1_000 - index, "files.read"));
    const window = mergeActivityWindow(fetched, []);
    expect(window.capped).toBe(false);
    expect(window.atLimit).toBe(true);

    const short = Array.from({ length: 999 }, (_, index) => audit(1_000 - index, "files.read"));
    expect(mergeActivityWindow(short, []).atLimit).toBe(false);
  });

  it("keeps the full-window fact when the merge dropped duplicate lines", () => {
    // 997 rows drawn from a full 1000-line answer: three lines the file wrote twice (same instant,
    // tool and status) collapse in the merge. Nothing was cut by the page's cap, but the request
    // still filled the server's window.
    const fetched = Array.from({ length: 1000 }, (_, index) => audit(2_000 - index, "files.read"));
    fetched[1] = { ...fetched[0] };
    fetched[2] = { ...fetched[0] };
    fetched[3] = { ...fetched[0] };
    const window = mergeActivityWindow(fetched, []);

    expect(window.lines).toHaveLength(997);
    expect(window.capped).toBe(false);
    expect(window.atLimit).toBe(true);
  });
});
