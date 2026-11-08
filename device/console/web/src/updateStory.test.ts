import { describe, expect, it } from "vitest";
import { exactMoment } from "./format";
import { startedLine, startedStory, updateChannel } from "./updateStory";

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
    // it went silent. The chip states the version; the start states when, and the title carries the
    // exact instant.
    expect(story).toEqual({
      text: "started 24m ago",
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

    expect(line).toBe("started 2h ago");
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
    ).toBe("started 2h ago (updated 12m ago)");
  });

  it("says the start once when the update is the start", () => {
    // The measured 9VH and 84T shape: `last_update.at_ms == restarts[0].at_ms`. The process began
    // when the update landed, so `(updated 2h ago)` beside `started 2h ago` is one moment written
    // twice. The start alone is the fact.
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "started 0.3.277" },
          last_update: { to: "0.3.277", outcome: "applied", at_ms: START }
        },
        context
      )
    ).toBe("started 2h ago");
  });

  it("keeps drawing the start when its version is the one the surface already shows", () => {
    // The version repeats the chip, so it is left out; the moment does not repeat anything, and
    // the start's age is exactly what the chip cannot state. The line is drawn, not dropped.
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "started 0.3.277" }
        },
        context
      )
    ).toBe("started 2h ago");
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
    ).toBe("started 2h ago");
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
    ).toBe("started 2h ago (update to 0.3.278 refused)");
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
    ).toBe("started 2h ago (update to 0.3.278 rolled back to 0.3.277)");
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
    ).toBe("started 2h ago");
  });
});

/**
 * THE CHANNEL IS THE RESTART JOURNAL'S, NOT ONLY THE LAUNCHER'S.
 *
 * The measured 84T: `host.update.current` was null, so the update line read "channel not reported"
 * while `host.restarts[0].channel` said `canary` and the device's own panel said `Channel canary`.
 * The journal's newest entry is the release the running process actually started from — the one the
 * "running v…" half of the line describes — so it wins over the launcher's baseline sidecar, which
 * is the fallback for a build whose journal predates the channel field.
 */
describe("updateChannel — the channel the running process started from", () => {
  it("reads the newest restart's channel and prefers it to the launcher's generation", () => {
    expect(
      updateChannel({
        host: {
          restarts: [{ at_ms: START, version: "0.3.280", channel: "canary" }],
          update: { current: { version: "0.3.280", channel: "stable" } }
        }
      })
    ).toBe("canary");
  });

  it("falls back to the launcher's current generation when the journal names no channel", () => {
    expect(
      updateChannel({
        host: {
          restarts: [{ at_ms: START, version: "0.3.280" }],
          update: { current: { version: "0.3.280", channel: "beta" } }
        }
      })
    ).toBe("beta");
  });

  it("is null when neither fact names a channel, and a blank field is not one", () => {
    expect(updateChannel({})).toBeNull();
    expect(updateChannel({ host: null })).toBeNull();
    expect(updateChannel({ host: { restarts: [] } })).toBeNull();
    expect(
      updateChannel({
        host: {
          restarts: [{ at_ms: START, channel: "   " }],
          update: { current: { channel: "" } }
        }
      })
    ).toBeNull();
  });
});

/**
 * THE PAGE THAT DRAWS THE UPDATE CARD SAYS IT ONCE.
 *
 * The Machine tab now carries the panel's `Updates` grid beside the start line. The card states the
 * applied version first when it is the running one, and it dates the updater's last answer; a start
 * line that repeated the version or the `(updated …)` clause would say the same thing twice. What
 * the line keeps is the start's own moment, and a start version the chip does not show — that
 * contradiction is a fact the card does not carry.
 */
describe("startedLine — a page that draws the update card", () => {
  it("states only when the start was, leaving the version to the chip and the card", () => {
    // **THE PAGE'S ONE VERSION IS THE CHIP, AND THE CARD'S FIRST ROW IS THE ANSWER.** The start line
    // above the card is the one sentence that places the running process in time; repeating the
    // version there made the same build read as three facts on one screen.
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "started 0.3.277" },
          last_update: { to: "0.3.277", outcome: "applied", at_ms: NOW - 12 * 60_000 }
        },
        { ...context, updateShown: true }
      )
    ).toBe("started 2h ago");
  });

  it("keeps a start version the chip does not show, because the card cannot state it", () => {
    expect(
      startedLine(
        {
          current: "0.3.275",
          verdict: { outcome: "ok", detail: "started 0.3.275" }
        },
        { ...context, updateShown: true, shownVersion: "0.3.42" }
      )
    ).toBe("started 0.3.275 · 2h ago");
  });

  it("draws no second copy of an applied verdict the card states", () => {
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "updated to 0.3.277" },
          last_update: { to: "0.3.277", outcome: "applied", at_ms: NOW - 12 * 60_000 }
        },
        { ...context, updateShown: true }
      )
    ).toBeNull();
  });

  it("keeps the device's own current sentence, which the card does not carry", () => {
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "needs_attention", detail: "refused: nothing was installed" }
        },
        { ...context, updateShown: true }
      )
    ).toBe("refused: nothing was installed");
  });
});
/**
 * A SURFACE WHOSE OWN COLUMN OR BADGE STATES THE VERSION.
 *
 * Home's fleet row has a Version cell; the start sentence sits in Started beside it. Repeating the
 * version there made the same build appear twice on one row, so the row's start states only when.
 * A start naming a different version is the contradiction the row must keep.
 */
describe("startedLine — a surface whose own column states the version", () => {
  it("leaves the version the surface already shows out of the start", () => {
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "started 0.3.277" }
        },
        { ...context, versionElsewhere: true }
      )
    ).toBe("2h ago");
  });

  it("keeps the update's age beside the start", () => {
    expect(
      startedLine(
        {
          current: "0.3.277",
          verdict: { outcome: "ok", detail: "started 0.3.277" },
          last_update: { to: "0.3.277", outcome: "applied", at_ms: NOW - 12 * 60_000 }
        },
        { ...context, versionElsewhere: true }
      )
    ).toBe("2h ago (updated 12m ago)");
  });

  it("keeps a start that names a version the column does not show", () => {
    expect(
      startedLine(
        {
          current: "0.3.275",
          verdict: { outcome: "ok", detail: "started 0.3.275" }
        },
        { ...context, versionElsewhere: true, shownVersion: "0.3.42" }
      )
    ).toBe("started 0.3.275 · 2h ago");
  });
});
