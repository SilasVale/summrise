import { describe, expect, it } from "vitest";
import { CONSOLE_RESTART_SENTENCE, recoveryStory } from "./recovery";
import { exactMoment } from "./format";

// The recovery line is a fact about a gap, not a mood: it is drawn only when the console's own
// channel pair or the device's own boot journal proves one, and a console that restarted inside the
// gap says it cannot vouch rather than charging its own downtime to the machine.

const HOUR = 3_600_000;
const MINUTE = 60_000;

/** The measured CPH shape: the host booted on return, after a 14 h 57 m boot gap. */
const backBoot = new Date(2026, 10, 8, 8, 51, 43).getTime();
const previousBoot = backBoot - (14 * 3_600_000 + 57 * 60_000 + 59_000);

function cph() {
  return {
    // The host booted four seconds before the app started; the newest frame is one second after the
    // boot, which is the clock the frame's `uptime_s` is read against.
    last_frame_ms: backBoot + 1_000,
    host: {
      uptime_s: 5,
      restarts: [
        { at_ms: backBoot + 44 * MINUTE, version: "0.3.292" },
        { at_ms: backBoot, version: "0.3.289" },
        { at_ms: previousBoot, version: "0.3.289" }
      ]
    }
  };
}

describe("recoveryStory from the console's channel pair", () => {
  it("states the pair's duration and keeps the exact return in the title", () => {
    const backAt = new Date(2026, 10, 8, 10, 20, 0).getTime();
    const story = recoveryStory(
      { last_outage: { away_ms: 41 * MINUTE, back_at_ms: backAt } },
      // The console started after the gap: it read both ends from the audit, so it can state them.
      backAt + HOUR
    );
    expect(story).toEqual({
      text: "back after 41 m",
      title: exactMoment(backAt),
      source: "console"
    });
  });

  it("says the console cannot vouch when its own process began inside the gap", () => {
    const backAt = new Date(2026, 10, 8, 10, 20, 0).getTime();
    const away = 41 * MINUTE;
    const story = recoveryStory(
      { last_outage: { away_ms: away, back_at_ms: backAt } },
      // The console started half-way through the silence: part of that gap is the console's own
      // downtime, so the machine's absence cannot be the whole of it.
      backAt - away / 2
    );
    expect(story?.text).toBe(CONSOLE_RESTART_SENTENCE);
    expect(story?.source).toBe("console");
    // A console that started exactly at the return missed the silence too.
    expect(
      recoveryStory({ last_outage: { away_ms: away, back_at_ms: backAt } }, backAt)?.text
    ).toBe(CONSOLE_RESTART_SENTENCE);
  });

  it("draws nothing for a blip, and nothing for a pair without a usable moment", () => {
    const backAt = new Date(2026, 10, 8, 10, 20, 0).getTime();
    // An apply closes and reopens the channel in seconds; that is not a recovery.
    expect(
      recoveryStory({ last_outage: { away_ms: 2_000, back_at_ms: backAt } })
    ).toBeNull();
    expect(
      recoveryStory({ last_outage: { away_ms: 41 * MINUTE, back_at_ms: Number.NaN } })
    ).toBeNull();
    expect(recoveryStory({})).toBeNull();
    expect(recoveryStory(null)).toBeNull();
  });

  it("prefers the console's pair to the device's journal", () => {
    const backAt = new Date(2026, 10, 8, 10, 20, 0).getTime();
    const story = recoveryStory(
      { ...cph(), last_outage: { away_ms: 41 * MINUTE, back_at_ms: backAt } },
      backAt + HOUR
    );
    expect(story?.text).toBe("back after 41 m");
    expect(story?.source).toBe("console");
  });
});

describe("recoveryStory from the device's boot journal", () => {
  it("states the measured CPH boot gap, and says which record it is", () => {
    const story = recoveryStory(cph(), backBoot + HOUR);
    expect(story).toEqual({
      // 14 h 57 m 59 s, at the span's minute precision.
      text: "back after 14 h 57 m (the device's journal recorded its boot at 08:51)",
      title: exactMoment(backBoot),
      source: "journal"
    });
  });

  it("withholds the journal's gap when the console restarted inside it", () => {
    const story = recoveryStory(cph(), new Date(2026, 10, 8, 3, 0, 0).getTime());
    expect(story?.text).toBe(CONSOLE_RESTART_SENTENCE);
    expect(story?.source).toBe("journal");
    expect(story?.title).toBe(exactMoment(backBoot));
  });

  it("does not read an ordinary app restart as the machine's boot", () => {
    // The current host has been up for 44 minutes and the newest boot is the app's update restart
    // 44 minutes after the host boot: that is not the boot the frame's uptime names, and the gap
    // before it is the previous app's uptime, not an absence.
    const hostBoot = backBoot; // the host booted at backBoot
    const story = recoveryStory(
      {
        last_frame_ms: hostBoot + 44 * MINUTE + 60_000,
        host: {
          uptime_s: (44 * MINUTE + 60_000) / 1000,
          restarts: [
            { at_ms: hostBoot + 44 * MINUTE, version: "0.3.292" },
            { at_ms: hostBoot - 3 * HOUR, version: "0.3.289" }
          ]
        }
      },
      hostBoot + HOUR
    );
    // No journal entry is the host's boot, so there is no proven gap to state.
    expect(story).toBeNull();
  });

  it("does not draw a gap under the floor, or one with no earlier boot to close it", () => {
    const hostBoot = new Date(2026, 10, 8, 8, 51, 43).getTime();
    expect(
      recoveryStory(
        {
          last_frame_ms: hostBoot + 1_000,
          host: {
            uptime_s: 1,
            restarts: [
              { at_ms: hostBoot, version: "0.3.42" },
              { at_ms: hostBoot - 2 * MINUTE, version: "0.3.42" }
            ]
          }
        },
        hostBoot + HOUR
      )
    ).toBeNull();
    expect(
      recoveryStory(
        {
          last_frame_ms: hostBoot + 1_000,
          host: { uptime_s: 1, restarts: [{ at_ms: hostBoot, version: "0.3.42" }] }
        },
        hostBoot + HOUR
      )
    ).toBeNull();
  });

  it("says nothing when the frame carries no uptime or no journal", () => {
    expect(recoveryStory({ host: { restarts: cph().host.restarts } })).toBeNull();
    expect(recoveryStory({ host: { uptime_s: 5 } })).toBeNull();
    expect(recoveryStory({ host: null })).toBeNull();
  });
});
