import { describe, expect, it } from "vitest";
import { exactMoment } from "./format";
import { startedLine, startedStory } from "./updateStory";

/**
 * THE DEVICE'S OWN SENTENCE, AND THE START IT KEEPS.
 *
 * Every word below comes from the section the device sent: its verdict's detail is what it says
 * about its last start (or the updater's last answer), the updater's outcome is its own value, and
 * the newest restart entry is when the running process started. What the console adds is the one
 * fact the raw fields cannot state: how long ago that start was, and — for an answer inside it —
 * how long ago the answer arrived.
 *
 * THE START IS DRAWN EVEN WHEN ITS VERSION REPEATS THE CHIP. The chips say what runs; they cannot
 * say when it started, and on a machine that restarted without changing version that moment is what
 * places the last boot against the last check and the silence. An answer from before this process
 * started is history — the running version proves it took — so the start line does not stitch it to
 * a start it predates.
 */
const HOUR = 3_600_000;
const NOW = 1_800_000_000_000;
const START = NOW - 2 * HOUR;

const context = { startedAtMs: START, shownVersion: "0.3.277", nowMs: NOW };

describe("startedLine — the device's last start, and an update that belongs to it", () => {
  it("draws a fresh start of the version the surface already shows, with its time", () => {
    const story = startedStory(
      {
        current: "0.3.286",
        verdict: { outcome: "ok", detail: "started 0.3.286" }
      },
      { startedAtMs: NOW - 24 * 60_000, shownVersion: "0.3.286", nowMs: NOW }
    );

    // The measured 9VH shape: started 0.3.286, the same version the header shows, 24 minutes before
    // it went silent. The sentence is the device's; the age and the exact title are the console's.
    expect(story).toEqual({
      text: "started 0.3.286 · 24m ago",
      title: exactMoment(NOW - 24 * 60_000)
    });
  });

  it("draws the start while leaving out the updater's answer that predates it", () => {
    // The measured 9VH shape: the applied answer is days old, the process started 2 h ago. The start
    // is the fact that places the last boot against the silence; the old answer is not this start's
    // story.
    const line = startedLine(
      {
        current: "0.3.277",
        verdict: { outcome: "ok", detail: "started 0.3.277" },
        last_update: { to: "0.3.273", outcome: "applied", at_ms: START - 30 * 60_000 }
      },
      context
    );

    expect(line).toBe("started 0.3.277 · 2h ago");
    expect(line).not.toContain("updated");
  });

  it("states the update's age beside the start when the answer belongs to this start", () => {
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "started 0.3.277" },
          last_update: { to: "0.3.277", outcome: "applied", at_ms: NOW - 12 * 60_000 }
        },
        context
      )
    ).toBe("started 0.3.277 · 2h ago (updated 12m ago)");
  });

  it("keeps drawing the start when its version is the one the surface already shows", () => {
    // The version repeats the chip, but the moment does not: the start's age is exactly what the
    // chip cannot state, so the line is drawn instead of dropped as a duplicate.
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "started 0.3.277" }
        },
        context
      )
    ).toBe("started 0.3.277 · 2h ago");
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
    ).toBe("started 0.3.277 · 2h ago");
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
    ).toBe("started 0.3.275 · 2h ago");
  });

  it("keeps a differing start version as the device's sentence when no start moment was reported", () => {
    // No journal: the console cannot age the start, and must not borrow another moment. The
    // contradiction between the header and the start version is still shown as written.
    expect(
      startedLine(
        {
          current: "0.3.275",
          verdict: { outcome: "ok", detail: "started 0.3.275" }
        },
        { ...context, startedAtMs: null, shownVersion: "0.3.42" }
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
    ).toBe("started 0.3.277 · 2h ago");
  });

  it("says an updater's refusal inside this start after the start", () => {
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "started 0.3.277" },
          last_update: { to: "0.3.278", outcome: "refused", at_ms: START + HOUR }
        },
        context
      )
    ).toBe("started 0.3.277 · 2h ago (update to 0.3.278 refused)");
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
    ).toBe("started 0.3.277 · 2h ago (update to 0.3.278 rolled back to 0.3.277)");
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

  it("has no exact start moment to title an update-only sentence", () => {
    expect(
      startedStory(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "updated to 0.3.277" },
          last_update: { to: "0.3.277", outcome: "applied", at_ms: NOW - 12 * 60_000 }
        },
        context
      )
    ).toEqual({ text: "updated 12m ago", title: null });
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
    // A newer device may add an outcome; the console does not guess a word for it. The start itself
    // is still the device's fact and stays.
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "started 0.3.277" },
          last_update: { to: "0.3.278", outcome: "superseded", at_ms: START + HOUR }
        },
        context
      )
    ).toBe("started 0.3.277 · 2h ago");
  });
});
