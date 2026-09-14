// The Restarts card — the one surface where an operator can see the PATTERN rather than the
// last event.
//
// The properties worth pinning are the honest ones: the count is drawn as words (never only as
// a colour), a crash row is distinguishable without resolving a hue, a device with no history
// says exactly that instead of drawing an empty list, and a device that did not answer says
// something DIFFERENT from that — the failure mode this panel keeps finding in its own past.
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { RestartHistoryCard } from "../RestartHistoryCard";
import type { BootHistory } from "../../hooks/useBootHistory";

const now = Date.now();
const rec = (over: Partial<BootHistory["boots"][number]> = {}) => ({
  tsMs: now - 60_000,
  kind: "crashed" as const,
  detail: "run journal: previous run DID NOT EXIT CLEANLY — CRASHED or was killed",
  uptimeSecs: 121,
  gapSecs: 125,
  release: "1.2.368",
  ...over,
});

const history = (over: Partial<BootHistory> = {}): BootHistory => ({
  boots: [rec(), rec({ tsMs: now - 7_200_000, kind: "replaced", uptimeSecs: 3842, gapSecs: 4 })],
  summary: { windowSecs: 86_400, boots: 2, crashes: 1 },
  ...over,
});

describe("RestartHistoryCard", () => {
  it("draws the pattern as words, and marks a crash by more than colour", () => {
    const { container } = render(<RestartHistoryCard history={history()} />);
    const text = container.textContent!;
    expect(text).toContain("2 restarts in the last 24h");
    expect(text).toContain("1 of them a crash");
    // The device's own sentence is on the hover of every row — the count is never a summary
    // the reader cannot check.
    const rows = container.querySelectorAll(".restart-row");
    expect(rows).toHaveLength(2);
    expect(rows[0].getAttribute("title")).toContain("CRASHED or was killed");
    // Kind words come from the shared vocabulary…
    expect(rows[0].textContent).toContain("previous run crashed");
    expect(rows[1].textContent).toContain("replaced by a restart");
    // …and the crash row carries data-kind, which the stylesheet turns into the bar.
    expect(rows[0].getAttribute("data-kind")).toBe("crashed");
    expect(rows[1].getAttribute("data-kind")).toBe("replaced");
    // What the run before it did with its life, both shapes.
    expect(text).toContain("ran 2m 1s");
    expect(text).toContain("ran 1h 04m");
    expect(text).toContain("1.2.368");
  });

  it("says a device has NO HISTORY rather than drawing an empty list", () => {
    const { container } = render(<RestartHistoryCard history={{ boots: [], summary: { windowSecs: 86_400, boots: 0, crashes: 0 } }} />);
    expect(container.querySelector(".restart-list")).toBeNull();
    expect(container.textContent).toContain("No restarts recorded yet");
  });

  it("distinguishes 'the device did not answer' from 'nothing recorded'", () => {
    const { container } = render(
      <RestartHistoryCard
        history={{ boots: [], summary: { windowSecs: 86_400, boots: 0, crashes: 0 } }}
        failed
      />,
    );
    const text = container.textContent!;
    expect(text).toContain("did not answer");
    expect(text).not.toContain("No restarts recorded yet");
  });

  it("folds a long history and offers the rest", () => {
    const many = Array.from({ length: 9 }, (_, i) =>
      rec({ tsMs: now - i * 3_600_000, kind: i === 0 ? "crashed" : "replaced", uptimeSecs: 60 * i }),
    );
    const { container } = render(
      <RestartHistoryCard history={{ boots: many, summary: { windowSecs: 86_400, boots: 9, crashes: 1 } }} />,
    );
    expect(container.querySelectorAll(".restart-row")).toHaveLength(6);
    const btn = container.querySelector("button")!;
    expect(btn.textContent).toContain("show all 9");
  });

  it("never writes a run length the device did not record", () => {
    // A first start has no previous run to measure: the row draws no "ran …" at all, rather
    // than "ran 0s".
    const { container } = render(
      <RestartHistoryCard
        history={{
          boots: [rec({ kind: "first-run", uptimeSecs: null, gapSecs: null, release: null })],
          summary: { windowSecs: 86_400, boots: 1, crashes: 0 },
        }}
      />,
    );
    expect(container.textContent).toContain("first start");
    expect(container.textContent).not.toContain("ran ");
    // No crash ⇒ the summary says so in words instead of leaving the reader to infer it.
    expect(container.textContent).toContain("none of them a crash");
  });
});
