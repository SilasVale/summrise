import { describe, expect, it } from "vitest";
import { lastHeardMs, message, seenAgo, shortId } from "./format";

describe("shortId", () => {
  it("leaves an id of twelve characters alone", () => {
    expect(shortId("0123456789ab")).toBe("0123456789ab");
  });

  it("cuts a longer id to twelve characters and says it cut", () => {
    expect(shortId("0123456789abc")).toBe("0123456789ab…");
  });

  it("leaves a short id alone", () => {
    expect(shortId("dev")).toBe("dev");
  });
});

describe("message", () => {
  it("is an Error's message", () => {
    expect(message(new Error("x"))).toBe("x");
  });

  it("is a string unchanged", () => {
    expect(message("x")).toBe("x");
  });

  it("is anything else as a string", () => {
    expect(message(42)).toBe("42");
  });
});

describe("seenAgo", () => {
  const NOW = new Date(2026, 10, 5, 12, 0, 0).getTime();

  it("states the minutes of a machine's silence, so 1h 49m cannot read as 2h", () => {
    // The measured DESKTOP-14RJCR8 row said nothing; the shared age would have said "2h ago" for
    // the 1 h 49 m the close proves. This formatter is the row's and the notice's one vocabulary.
    expect(seenAgo(NOW - (1 * 60 + 49) * 60_000, NOW)).toBe("1h 49m ago");
    expect(seenAgo(NOW - 7 * 60_000, NOW)).toBe("7m ago");
    expect(seenAgo(NOW - 15_000, NOW)).toBe("15s ago");
  });

  it("keeps a whole hour and a whole day short", () => {
    expect(seenAgo(NOW - 3 * 3600_000, NOW)).toBe("3h ago");
    expect(seenAgo(NOW - (26 + 3) * 3600_000, NOW)).toBe("1d 5h ago");
  });

  it("does not grow an age from a clock that is behind", () => {
    expect(seenAgo(NOW + 5_000, NOW)).toBe("0s ago");
  });
});

describe("lastHeardMs", () => {
  it("reads the frame, then the open close, then the registry", () => {
    expect(lastHeardMs({ last_frame_ms: 300, last_left_ms: 200, last_seen_ms: 100 })).toBe(300);
    expect(lastHeardMs({ last_frame_ms: null, last_left_ms: 200, last_seen_ms: 100 })).toBe(200);
    expect(lastHeardMs({ last_frame_ms: null, last_left_ms: null, last_seen_ms: 100 })).toBe(100);
    expect(lastHeardMs({})).toBeNull();
  });
});
