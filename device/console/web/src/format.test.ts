import { describe, expect, it } from "vitest";
import {
  callsLine,
  exactMoment,
  isoDate,
  lastHeardMs,
  message,
  seenAgo,
  shortDeviceName,
  shortId
} from "./format";

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

describe("callsLine", () => {
  it("names the count as the client's calls, the noun the hub's day line uses", () => {
    // The 9VH's row read `141 calls · 4 failed` beside the hub's `Today 141 client calls` and
    // `Console reads · 238`. The row's number is `device.today.client_calls`; it never held the
    // console's reads, so a bare `calls` made the row read as the machine's whole work and made
    // the hub's count look contradictory (audit-37 F5).
    expect(callsLine({ client_calls: 141, failed: 4 })).toBe("141 client calls · 4 failed");
  });

  it("keeps the singular for one call", () => {
    expect(callsLine({ client_calls: 1, failed: 0 })).toBe("1 client call");
  });
});

describe("the console's absolute date", () => {
  // Local instants, so the assertions do not depend on the machine's timezone.
  const at = new Date(2026, 10, 4, 9, 41, 5).getTime();

  it("states the local day in ISO, not the browser's locale", () => {
    // The measured token sheet read `11/4/2026` in en-US: the locale decided the order, and a CN
    // operator had no way to tell day from month. The audit's own dates are ISO.
    expect(isoDate(at)).toBe("2026-11-04");
  });

  it("keeps the fixed width for a single-digit month and day", () => {
    expect(isoDate(new Date(2026, 0, 5, 7, 3, 2).getTime())).toBe("2026-01-05");
  });

  it("puts the exact local clock and the audit's UTC instant in the title", () => {
    expect(exactMoment(at)).toBe(`2026-11-04 09:41:05 (${new Date(at).toISOString()})`);
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

describe("shortDeviceName", () => {
  // The fleet the phone audit measured: three names whose only shared prefix is the vendor's.
  const desktops = ["DESKTOP-9VH92BJ", "DESKTOP-14RJCR8", "DESKTOP-CPH4KB2"];

  it("drops the prefix every machine shares and keeps what tells them apart", () => {
    expect(shortDeviceName("DESKTOP-9VH92BJ", desktops)).toBe("9VH92BJ");
    expect(shortDeviceName("DESKTOP-14RJCR8", desktops)).toBe("14RJCR8");
    expect(shortDeviceName("DESKTOP-CPH4KB2", desktops)).toBe("CPH4KB2");
  });

  it("keeps a name whole when the fleet shares no prefix", () => {
    expect(shortDeviceName("workshop", ["workshop", "attic", "studio"])).toBe("workshop");
  });

  it("keeps a name whole rather than cutting a shared prefix inside a word", () => {
    // "alpha" is not a separator: shortening these to "1" and "2" would be a different name, not
    // the same machine's distinguishing tail.
    expect(shortDeviceName("alpha1", ["alpha1", "alpha2"])).toBe("alpha1");
    expect(shortDeviceName("alpha2", ["alpha1", "alpha2"])).toBe("alpha2");
  });

  it("keeps a lone machine's name whole", () => {
    expect(shortDeviceName("DESKTOP-9VH92BJ", ["DESKTOP-9VH92BJ"])).toBe("DESKTOP-9VH92BJ");
  });
});
