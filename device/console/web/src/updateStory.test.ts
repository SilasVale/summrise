import { describe, expect, it } from "vitest";
import { startedLine } from "./updateStory";

/**
 * THE DEVICE'S OWN SENTENCE, NOT THE CONSOLE'S. Every word below comes from the section the device
 * sent: its verdict's detail is the start it recorded, and the updater's outcome is its own value.
 * The console adds the parentheses and nothing else — no "asleep", no inference about why the
 * machine was silent.
 */
describe("startedLine — the device's account of its last start", () => {
  it("says a refused update in the device's own word", () => {
    expect(
      startedLine({
        current: "0.3.275",
        verdict: { outcome: "ok", detail: "started 0.3.275" },
        last_update: { to: "0.3.276", outcome: "refused", at_ms: 1 }
      })
    ).toBe("started 0.3.275 (update to 0.3.276 refused)");
  });

  it("says an applied update as updated, with the version the device started", () => {
    expect(
      startedLine({
        current: "0.3.276",
        verdict: { outcome: "ok", detail: "started 0.3.276" },
        last_update: { to: "0.3.276", outcome: "applied", at_ms: 2 }
      })
    ).toBe("started 0.3.276 (updated)");
  });

  it("names the version a rollback returned to", () => {
    expect(
      startedLine({
        current: "0.3.275",
        verdict: { outcome: "needs_attention", detail: "started 0.3.275" },
        last_update: { to: "0.3.276", outcome: "rolled_back", at_ms: 3 },
        rolled_back_to: "0.3.275"
      })
    ).toBe("started 0.3.275 (update to 0.3.276 rolled back to 0.3.275)");
  });

  it("shows the start alone when the updater has not answered", () => {
    expect(
      startedLine({
        current: "0.3.275",
        verdict: { outcome: "ok", detail: "started 0.3.275" }
      })
    ).toBe("started 0.3.275");
  });

  it("does not repeat a verdict that is itself the updater's sentence", () => {
    // A refusal folded in by the running app replaces the start verdict; the device's sentence is
    // already the whole story, and a parenthetical would say the same thing twice.
    expect(
      startedLine({
        current: "0.3.275",
        verdict: { outcome: "needs_attention", detail: "refused: nothing was installed" },
        last_update: { to: "0.3.276", outcome: "refused", at_ms: 4 }
      })
    ).toBe("refused: nothing was installed");
  });

  it("draws no line without a verdict", () => {
    expect(startedLine(null)).toBeNull();
    expect(startedLine(undefined)).toBeNull();
    expect(startedLine({})).toBeNull();
    expect(startedLine({ current: "0.3.275" })).toBeNull();
    expect(startedLine({ verdict: null })).toBeNull();
    expect(startedLine({ verdict: { outcome: "ok", detail: "  " } })).toBeNull();
  });

  it("leaves an outcome it does not know out of the sentence", () => {
    // A newer device may add an outcome; the console does not guess a word for it.
    expect(
      startedLine({
        verdict: { outcome: "ok", detail: "started 0.3.275" },
        last_update: { to: "0.3.276", outcome: "superseded", at_ms: 5 }
      })
    ).toBe("started 0.3.275");
  });
});
