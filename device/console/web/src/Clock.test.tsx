import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Clock, Moment } from "./Clock";
import { exactMoment } from "./format";

/** A local instant, so the assertions do not depend on the machine's timezone. */
const TS = new Date(2026, 10, 5, 8, 46, 18).getTime();

describe("the console's one clock", () => {
  it("draws HH:MM:SS and keeps the exact instant in the title", () => {
    // The Table, the Timeline and the Runs history all draw this element. Before audit-43 F8 the
    // Table's and the Timeline's `<time>` had no title at all, so the exact instant was nowhere
    // for the row; the Runs history had one, which is the third spelling.
    const { container } = render(<Clock ts={TS} />);
    const time = container.querySelector("time") as HTMLTimeElement;
    expect(time).not.toBeNull();
    expect(time).toHaveTextContent("08:46:18");
    expect(time).toHaveAttribute("datetime", new Date(TS).toISOString());
    expect(time).toHaveAttribute("title", exactMoment(TS));
    // The seconds are their own span (the phone used to drop it), but the element's text is the
    // whole instant and the whole instant is what the title carries too.
    expect(time.querySelector(".timeline-clock")).toHaveTextContent("08:46");
    expect(time.querySelector(".timeline-clock-seconds")).toHaveTextContent(":18");
  });

  it("writes the age beside the clock with exactly one spacing", () => {
    // The measured Table read `08:46:18· 8s ago`: the only gap was a 4 px margin, so the text
    // itself glued the separator to the seconds. `Moment` owns the join now: one space either
    // side of the dot, no margin.
    const { container } = render(<Moment ts={TS} age="8s ago" />);
    expect(container.textContent).toBe("08:46:18 · 8s ago");
    expect(container.textContent?.match(/ · /g)).toHaveLength(1);
    expect(container.textContent).not.toMatch(/\d·|·\d/);
    expect(container.querySelector("time")).toHaveAttribute("title", exactMoment(TS));
  });
});
