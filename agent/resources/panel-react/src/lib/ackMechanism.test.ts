// ackMechanism.test.ts — ONE ACKNOWLEDGEMENT MECHANISM, AND IT STAYS ONE.
//
// WHY (round 16 of the standing goal). The panel had THIRTEEN separate `useState(false)` busy flags across nine
// components, and the flag said only that something was in flight. Three of them put the label on the wrong
// control outright:
//
//   the approval gate   `busy` disabled three buttons and said nothing about which was pressed
//   MemoryPage          SIX controls dimmed together (Search, List, Export and three more)
//   GoalBar             `busy ? "…" : "Save"` made SAVE report that it was working whenever CLEAR was pressed
//   PathView            `recipeBusy ? "Saving…" : …` — the same thing, on the recipe's three controls
//
// They all use `lib/useAck.ts` now: the key names the control that was pressed, the flag is set in the same tick
// as the click, and it clears on EVERY exit including a throw — which is the defect MonitorsCard demonstrably had
// (its flag was cleared on the line AFTER the await, so a rejected add left the button disabled until remount).
//
// A CLEAN RESULT NOTHING ENFORCES IS ONE COMMIT FROM NOT BEING TRUE, so this is the ratchet: zero ad-hoc busy
// flags, and a floor on how many components use the mechanism (a scan that read nothing is not a clean scan).
import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SRC = path.dirname(fileURLToPath(import.meta.url));
const COMPONENTS = path.join(SRC, "..", "components");

/** A busy-ish flag kept by hand. The names the panel used before the hook, plus the obvious neighbours. */
const AD_HOC = /const\s*\[\s*\w*(busy|Busy|saving|Saving|running|Running|working|Working|loading|Loading|spinning)\w*\s*,\s*set\w+\s*\]\s*=\s*useState\(false\)/;

describe("the acknowledgement mechanism", () => {
  it("is the only one: no component keeps its own busy flag", () => {
    const files = readdirSync(COMPONENTS).filter((f) => f.endsWith(".tsx"));
    const offenders: string[] = [];
    let users = 0;

    for (const file of files) {
      const src = readFileSync(path.join(COMPONENTS, file), "utf8");
      if (src.includes("useAck()")) users++;
      const m = AD_HOC.exec(src);
      if (m) offenders.push(`${file}: ${m[0]}`);
    }

    expect(
      offenders,
      `\n${offenders.length} component(s) keep a busy flag by hand. That flag cannot say WHICH control the operator` +
        ` pressed, and it is cleared on the paths somebody remembered:\n  ` +
        offenders.join("\n  ") +
        `\n\nUse \`const { busy, busyOn, ack, run } = useAck()\` from lib/useAck.ts — the key is what the label` +
        ` and \`aria-busy\` follow.`,
    ).toEqual([]);

    // AND THE MECHANISM IS ACTUALLY IN USE. Zero offenders is also what a scan of an empty directory reports.
    expect(users, `only ${users} component(s) use useAck() — this test is reading the wrong thing`).toBeGreaterThanOrEqual(8);
  });
});
