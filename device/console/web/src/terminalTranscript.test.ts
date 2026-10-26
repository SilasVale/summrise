import { describe, expect, it, vi } from "vitest";
import {
  earliestCursor,
  readTranscript,
  RETAINED_BYTES,
  transcriptFile,
  type TranscriptRead
} from "./terminalTranscript";

/** The device's refusal when a tab's cursor has fallen behind the retained output. */
function tooOld(cursor: number, earliest: number): string {
  return `terminal.read: the cursor ${cursor} is older than the retained output, which starts at ${earliest}`;
}

describe("earliestCursor", () => {
  it("reads the start of the retained output from the device's sentence", () => {
    expect(earliestCursor(tooOld(0, 4096))).toBe(4096);
  });

  it("returns null for any other sentence", () => {
    expect(earliestCursor("The device is unreachable: Failed to fetch.")).toBeNull();
  });
});

describe("readTranscript", () => {
  it("reads from zero and reports no dropped bytes", async () => {
    const read = vi.fn<TranscriptRead>().mockResolvedValueOnce({ data: "hello", cursor: 5 });

    await expect(readTranscript(read)).resolves.toEqual({
      text: "hello",
      from: 0,
      to: 5,
      dropped: 0
    });
    expect(read).toHaveBeenCalledTimes(1);
    expect(read).toHaveBeenCalledWith(0);
  });

  it("retries at the earliest retained cursor the device names", async () => {
    const read = vi
      .fn<TranscriptRead>()
      .mockRejectedValueOnce(new Error(tooOld(0, 4096)))
      .mockResolvedValueOnce({ data: "tail", cursor: 5000 });

    await expect(readTranscript(read)).resolves.toEqual({
      text: "tail",
      from: 4096,
      to: 5000,
      dropped: 4096
    });
    expect(read.mock.calls).toEqual([[0], [4096]]);
  });

  it("propagates an unrelated failure without a second read", async () => {
    const read = vi.fn<TranscriptRead>().mockRejectedValueOnce(new Error("network down"));

    await expect(readTranscript(read)).rejects.toThrow("network down");
    expect(read).toHaveBeenCalledTimes(1);
  });

  it("propagates the retry's own failure", async () => {
    const read = vi
      .fn<TranscriptRead>()
      .mockRejectedValueOnce(new Error(tooOld(0, 4096)))
      .mockRejectedValueOnce(new Error("still down"));

    await expect(readTranscript(read)).rejects.toThrow("still down");
    expect(read).toHaveBeenCalledTimes(2);
  });
});

describe("transcriptFile", () => {
  const at = new Date("2026-10-26T09:00:00.000Z");

  it("states the range, the retention and the payload after the separator", () => {
    const file = transcriptFile(
      { text: "hello\n", from: 0, to: 6, dropped: 0 },
      { session: "t1", at }
    );

    expect(file).toContain("Summrise terminal transcript");
    expect(file).toContain("session: t1");
    expect(file).toContain("exported: 2026-10-26T09:00:00.000Z");
    expect(file).toContain("range: bytes 0..6 of this session's output");
    expect(file).toContain(`retained: the device keeps the newest ${RETAINED_BYTES / (1024 * 1024)} MiB of a session's output`);
    expect(file).toContain("dropped: none — the whole session is retained");
    expect(file).toContain("---\nhello\n");
  });

  it("states how many bytes the ring no longer has", () => {
    const file = transcriptFile(
      { text: "tail", from: 4096, to: 5000, dropped: 4096 },
      { session: "t1", at }
    );

    expect(file).toContain("range: bytes 4096..5000 of this session's output");
    expect(file).toContain(
      "dropped: 4096 bytes — older output is no longer retained and cannot be recovered"
    );
    expect(file).not.toContain("the whole session");
  });
});
