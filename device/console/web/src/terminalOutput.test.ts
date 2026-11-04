import { describe, expect, it } from "vitest";
import { createOutputPump, splitWriteSlices, WRITE_SLICE_CHARS } from "./terminalOutput";

describe("splitWriteSlices", () => {
  it("cuts at the budget and keeps the remainder", () => {
    const text = "a".repeat(100);
    const slices = splitWriteSlices(text, 30);
    expect(slices.map((slice) => slice.length)).toEqual([30, 30, 30, 10]);
    expect(slices.join("")).toBe(text);
  });

  it("never leaves half of a surrogate pair in a slice", () => {
    // Nine letters, then an emoji (two UTF-16 units), then more text: a cut at index 10 would split
    // the pair, which renders as a replacement character.
    const text = "a".repeat(9) + "😀" + "b".repeat(4);
    const slices = splitWriteSlices(text, 10);

    expect(slices.join("")).toBe(text);
    for (const slice of slices) {
      const first = slice.charCodeAt(0);
      const last = slice.charCodeAt(slice.length - 1);
      expect(first >= 0xdc00 && first <= 0xdfff).toBe(false);
      expect(last >= 0xd800 && last <= 0xdbff).toBe(false);
    }
  });

  it("leaves text shorter than the budget alone", () => {
    expect(splitWriteSlices("hello", 10)).toEqual(["hello"]);
  });
});

describe("createOutputPump", () => {
  const frames: (() => void)[] = [];
  const schedule = (callback: () => void) => {
    frames.push(callback);
    return frames.length;
  };
  const unschedule = () => {};

  const runFrame = () => {
    const frame = frames.shift();
    expect(frame).toBeDefined();
    frame!();
  };

  it("hands xterm one budget per frame and keeps the rest", () => {
    frames.length = 0;
    const written: string[] = [];
    const pump = createOutputPump((text) => written.push(text), schedule, unschedule);

    pump.push("a".repeat(WRITE_SLICE_CHARS + 6000));
    expect(written).toEqual([]);
    expect(frames).toHaveLength(1);

    runFrame();
    expect(written).toEqual(["a".repeat(WRITE_SLICE_CHARS)]);
    expect(frames).toHaveLength(1);

    runFrame();
    expect(written[1]).toHaveLength(6000);
    expect(frames).toHaveLength(0);
  });

  it("batches answers that arrive in the same frame into one write", () => {
    frames.length = 0;
    const written: string[] = [];
    const pump = createOutputPump((text) => written.push(text), schedule, unschedule);

    pump.push("one");
    pump.push("two");
    expect(frames).toHaveLength(1);

    runFrame();
    expect(written).toEqual(["onetwo"]);
  });

  it("drops what was queued when the tab goes away", () => {
    frames.length = 0;
    const written: string[] = [];
    const pump = createOutputPump((text) => written.push(text), schedule, unschedule);

    pump.push("never shown");
    pump.cancel();
    frames.length = 0;

    expect(written).toEqual([]);
  });
});
