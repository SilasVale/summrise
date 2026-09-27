// sweepFill.test.tsx — DOES THE DESIGN SWEEP'S FILL REACH A CONTROLLED COMPONENT?
//
// WHY (round 52 of the standing goal). The design sweep fills the page's visible text inputs before pressing controls, so
// the acknowledgement axis measures a state a user would be in (round 48). That change moved NOTHING — the panel's ack axis
// stayed at `16r/8a` and `[Search]` still acknowledged only on its second press — and round 49's reading of the hook said
// the fill therefore did not arrive: a filled query makes `search()` await a tool call, and an awaited action DOES paint
// `data-busy` (pinned one file over in `useAck.batching.test.tsx`).
//
// THIS IS THE TEST FOR THAT, AND IT IS WRITTEN SO THE ANSWER CANNOT BE FAKED. Asserting `el.value === "probe"` would prove
// nothing: the raw setter writes the DOM property whatever React thinks. The component mirrors its own STATE into a
// separate node, so the assertion reads what the component believes rather than what the DOM was told.
import { describe, expect, it } from "vitest";
import { useState } from "react";
import { render, screen } from "@testing-library/react";

function Controlled() {
  const [value, setValue] = useState("");
  return (
    <>
      <input aria-label="q" value={value} onChange={(e) => setValue(e.target.value)} />
      <span data-testid="mirror">{value}</span>
    </>
  );
}

/** The exact technique `agent/scripts/lib/design-sweep.mjs` uses, copied rather than paraphrased. */
function sweepFill(el: HTMLInputElement) {
  const set = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")!.set!;
  set.call(el, "probe");
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

describe("the design sweep's fill", () => {
  it("REACHES a controlled component's state", () => {
    render(<Controlled />);
    const el = screen.getByLabelText("q") as HTMLInputElement;
    sweepFill(el);
    expect(screen.getByTestId("mirror").textContent).toBe("probe");
  });

  it("and the DOM property alone would NOT have proved it", () => {
    // The negative control for the assertion above: a plain assignment sets the DOM property and leaves the component's
    // state untouched, which is exactly the failure the first test is written to distinguish.
    render(<Controlled />);
    const el = screen.getByLabelText("q") as HTMLInputElement;
    el.value = "probe";
    expect(el.value).toBe("probe");
    expect(screen.getByTestId("mirror").textContent).toBe("");
  });
});
