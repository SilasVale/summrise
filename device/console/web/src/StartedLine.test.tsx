import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { exactMoment } from "./format";
import { StartedLine } from "./StartedLine";

// The four start-line cases the shell used to own, now the component's own: the sentence and its
// title are `StartedLine`'s contract, and Overview and Machine place the component where the start
// is the answer (audit-44 F1).
const BOOT_AT = Date.now() - 25 * 3_600_000;

const STARTED = {
  version: "0.3.42",
  host: { restarts: [{ at_ms: BOOT_AT, version: "0.3.42", pid: 42 }] },
  update: {
    current: "0.3.42",
    verdict: { outcome: "ok", detail: "started 0.3.42" }
  }
};

describe("StartedLine", () => {
  it("draws the running version in the Machine tab's start line, and leaves the update answer to the card", () => {
    render(
      <StartedLine
        device={{
          ...STARTED,
          update: {
            ...STARTED.update,
            last_update: { to: "0.3.41", outcome: "applied", at_ms: Date.now() - 26 * 3_600_000 }
          }
        }}
        updateShown
      />
    );

    // **THE TOPBAR STATES THE BUILD; THE START LINE STATES WHEN.** The update card carries the
    // version and the release comparison; the line above it keeps only the moment a chip cannot
    // state. The card's `(updated …)` stays on the card.
    const line = screen.getByText("started 1d 1h ago");
    expect(line).toHaveAttribute("title", exactMoment(BOOT_AT));
    expect(screen.queryByText(/updated/)).not.toBeInTheDocument();
  });

  it("ages the update beside the start when the answer belongs to this start", () => {
    render(
      <StartedLine
        device={{
          ...STARTED,
          update: {
            ...STARTED.update,
            last_update: { to: "0.3.42", outcome: "applied", at_ms: Date.now() - 12 * 60_000 }
          }
        }}
      />
    );

    // The console knows silence, not sleep: it must never claim the machine slept. The new facts
    // beside it are when this process started and the age of the answer it folded in.
    const line = screen.getByText("started 1d 1h ago (updated 12m ago)");
    expect(line).toHaveAttribute("title", exactMoment(BOOT_AT));
    expect(screen.queryByText(/asleep|sleeping/i)).not.toBeInTheDocument();
  });

  it("keeps a start that names a version the header does not show", () => {
    render(
      <StartedLine
        device={{
          ...STARTED,
          update: {
            current: "0.3.275",
            verdict: { outcome: "ok", detail: "started 0.3.275" }
          }
        }}
      />
    );

    // The header says 0.3.42; the device says it started 0.3.275. That is a contradiction the
    // operator needs, not a duplicate of the chip.
    expect(screen.getByText("started 0.3.275 · 1d 1h ago")).toHaveAttribute(
      "title",
      exactMoment(BOOT_AT)
    );
  });

  it("shows a refusal the device is making now, on an online machine too", () => {
    render(
      <StartedLine
        device={{
          ...STARTED,
          update: {
            current: "0.3.42",
            verdict: { outcome: "needs_attention", detail: "refused: nothing was installed" }
          }
        }}
      />
    );

    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByText("refused: nothing was installed")).toBeInTheDocument();
  });
});
