// The chip, as the operator sees it: a shape, a sentence, and the device's own words
// on the hover. The rule itself is pinned in lib/bootNotice.test.ts — this file only
// pins that the component RENDERS that answer and renders NOTHING otherwise (a chip
// that always exists is chrome; see WaitingChip's own note on its zero state).
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { BootChip } from "../BootChip";

const CRASH = {
  kind: "crashed" as const,
  detail: "run journal: previous run DID NOT EXIT CLEANLY — CRASHED or was killed; survived 12s",
};

describe("BootChip", () => {
  it("renders nothing when the rule says there is nothing to say", () => {
    expect(render(<BootChip lastBoot={null} uptimeSecs={10} />).container.firstChild).toBeNull();
    expect(
      render(
        <BootChip lastBoot={{ kind: "clean-exit", detail: "exited cleanly" }} uptimeSecs={10} />,
      ).container.firstChild,
    ).toBeNull();
    // No vitals prop at all (an older caller): still nothing, never a crash on undefined.
    expect(render(<BootChip />).container.firstChild).toBeNull();
  });

  it("shows a crash with a SHAPE and the full sentence on the hover", () => {
    const { container } = render(<BootChip lastBoot={CRASH} uptimeSecs={7200} />);
    const chip = container.querySelector(".boot-chip.warn")!;
    expect(chip).not.toBeNull();
    expect(chip.textContent).toContain("last run crashed");
    // Text AND shape: this repo has an incident where two states differed by colour
    // alone, so the fault carries a mark that survives a greyscale screenshot.
    expect(chip.querySelector(".boot-mark.warn")).not.toBeNull();
    expect(chip.getAttribute("title")).toContain("CRASHED or was killed");
  });

  it("shows a fresh restart as news, with its own mark", () => {
    const { container } = render(
      <BootChip
        lastBoot={{ kind: "replaced", detail: "run journal: REPLACED by a restart" }}
        uptimeSecs={20}
      />,
    );
    const chip = container.querySelector(".boot-chip.info")!;
    expect(chip.textContent).toContain("just restarted");
    expect(chip.querySelector(".boot-mark.info")).not.toBeNull();
    expect(chip.getAttribute("title")).toContain("REPLACED by a restart");
  });
});
