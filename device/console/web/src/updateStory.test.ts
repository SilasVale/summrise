import { describe, expect, it } from "vitest";
import { startedLine } from "./updateStory";

/**
 * THE DEVICE'S OWN SENTENCE, NOT THE CONSOLE'S, AND ONLY THE FACT THAT IS NEW.
 *
 * Every word below comes from the section the device sent: its verdict's detail is what it says
 * about its last start (or the updater's last answer), and the updater's outcome is its own value.
 * What the console adds is the one fact the raw field cannot state: how long ago an answer that
 * belongs to THIS process arrived. An answer from before this process started is history — the
 * running version proves it took — so the line says nothing about it rather than stitching it to a
 * start it predates.
 */
const HOUR = 3_600_000;
const NOW = 1_800_000_000_000;
const START = NOW - 24 * HOUR;

const context = { startedAtMs: START, shownVersion: "0.3.277", nowMs: NOW };

describe("startedLine — the device's last start, and an update that belongs to it", () => {
  it("shows nothing when the updater's answer predates this start", () => {
    // The measured 9VH92BJ case: the applied answer is to 0.3.273 at 19:11, and the process
    // running 0.3.277 started at 09:41. "(updated)" attached to that start was a sentence about a
    // different process.
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "started 0.3.277" },
          last_update: { to: "0.3.273", outcome: "applied", at_ms: START - 30 * 60_000 }
        },
        context
      )
    ).toBeNull();
  });

  it("states the update's age when the answer belongs to this start", () => {
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "started 0.3.277" },
          last_update: { to: "0.3.277", outcome: "applied", at_ms: NOW - 12 * 60_000 }
        },
        context
      )
    ).toBe("updated 12m ago");
  });

  it("never repeats the running version the surface already shows", () => {
    // No update: the only content the start sentence has is the version the row's chip and the
    // header's chip carry. The line is not drawn at all.
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "started 0.3.277" }
        },
        context
      )
    ).toBeNull();
  });

  it("keeps the start version when the surface shows no version to repeat", () => {
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "started 0.3.277" }
        },
        { ...context, shownVersion: null }
      )
    ).toBe("started 0.3.277");
  });

  it("keeps a start that names a version the surface does not show", () => {
    expect(
      startedLine(
        {
          current: "0.3.275",
          verdict: { outcome: "ok", detail: "started 0.3.275" }
        },
        { ...context, shownVersion: "0.3.42" }
      )
    ).toBe("started 0.3.275");
  });

  it("says a refusal the device is making now", () => {
    // The running app folded the updater's refusal in without restarting; the verdict itself is
    // the updater's sentence, so it is shown alone rather than reworded into a parenthetical.
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "needs_attention", detail: "refused: nothing was installed" },
          last_update: { to: "0.3.278", outcome: "refused", at_ms: NOW - 2 * 60_000 }
        },
        context
      )
    ).toBe("refused: nothing was installed");
  });

  it("does not attach a refusal that predates this start", () => {
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "started 0.3.277" },
          last_update: { to: "0.3.278", outcome: "refused", at_ms: START - 1 }
        },
        context
      )
    ).toBeNull();
  });

  it("says an updater's refusal inside this start as its own sentence", () => {
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "started 0.3.277" },
          last_update: { to: "0.3.278", outcome: "refused", at_ms: START + HOUR }
        },
        context
      )
    ).toBe("update to 0.3.278 refused");
  });

  it("names the version a rollback inside this start returned to", () => {
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "needs_attention", detail: "started 0.3.277" },
          last_update: { to: "0.3.278", outcome: "rolled_back", at_ms: START + HOUR },
          rolled_back_to: "0.3.277"
        },
        context
      )
    ).toBe("update to 0.3.278 rolled back to 0.3.277");
  });

  it("ages a verdict that is itself an applied-update sentence", () => {
    // The app writes "updated to X" when it folds an applied result in. That names the running
    // version the surface already shows; the new fact is when it answered.
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "updated to 0.3.277" },
          last_update: { to: "0.3.277", outcome: "applied", at_ms: NOW - 12 * 60_000 }
        },
        context
      )
    ).toBe("updated 12m ago");
  });

  it("says nothing about an update when this start's moment is unknown", () => {
    // Without the moment the process started, the console cannot prove the answer is not older
    // than the start; claiming it belongs to this process would be the measured mistake again.
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "started 0.3.277" },
          last_update: { to: "0.3.277", outcome: "applied", at_ms: NOW - 12 * 60_000 }
        },
        { ...context, startedAtMs: null }
      )
    ).toBeNull();
  });

  it("draws no line without a verdict", () => {
    expect(startedLine(null, context)).toBeNull();
    expect(startedLine(undefined, context)).toBeNull();
    expect(startedLine({}, context)).toBeNull();
    expect(startedLine({ current: "0.3.275" }, context)).toBeNull();
    expect(startedLine({ verdict: null }, context)).toBeNull();
    expect(startedLine({ verdict: { outcome: "ok", detail: "  " } }, context)).toBeNull();
  });

  it("leaves an outcome it does not know out of the sentence", () => {
    // A newer device may add an outcome; the console does not guess a word for it.
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "started 0.3.277" },
          last_update: { to: "0.3.278", outcome: "superseded", at_ms: START + HOUR }
        },
        context
      )
    ).toBeNull();
  });
});
