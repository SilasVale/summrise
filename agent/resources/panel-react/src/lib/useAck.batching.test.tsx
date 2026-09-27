// useAck.batching.test.tsx — WHAT THE MECHANISM PAINTS, AND WHEN.
//
// WHY (round 51 of the standing goal). `ackMechanism.test.ts` records a fact in its header and says plainly that it cannot
// pin it, because it scans source and never renders:
//
//   WHEN THE ACTION DOES NO ASYNC WORK THE TWO STATE UPDATES BATCH INTO ONE RENDER AND `data-busy` IS NEVER PAINTED.
//
// The design-sweep's acknowledgement note has offered two readings for that since it was written — "a timing artefact" or
// "an acknowledgement that depends on state" — and reading the hook settled it in favour of the first. This is the test
// that turns that paragraph into a ratchet: the async case MUST paint, the synchronous case MUST NOT, and if a future
// change makes the synchronous case paint, the design sweep's note becomes wrong in a way nothing else would catch.
import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { useAck } from "./useAck";

function Probe({ action }: { action: () => Promise<unknown> }) {
  const { run, ack } = useAck();
  return (
    <button type="button" onClick={() => run("probe", action)} {...ack("probe")}>
      probe
    </button>
  );
}

describe("what the acknowledgement mechanism paints", () => {
  it("paints data-busy while an ASYNC action is in flight, and clears it after", async () => {
    let release: () => void = () => {};
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });
    render(<Probe action={() => pending} />);
    const btn = screen.getByRole("button");
    await act(async () => {
      fireEvent.click(btn);
    });
    expect(btn.getAttribute("data-busy")).toBe("1");
    await act(async () => {
      release();
    });
    expect(btn.getAttribute("data-busy")).toBeNull();
  });

  it("NEVER paints data-busy for a SYNCHRONOUS action, however often it is pressed", async () => {
    // This is a CHARACTERISATION test: it pins behaviour that is arguably a limitation rather than a feature. It is here
    // because the design sweep's note reads on it — a control whose handler completes without awaiting acknowledges
    // nothing, however fast the machine is — and because a change that made this paint would silently invalidate that note.
    render(<Probe action={async () => {}} />);
    const btn = screen.getByRole("button");
    for (const attempt of [1, 2, 3]) {
      await act(async () => {
        fireEvent.click(btn);
      });
      expect(btn.getAttribute("data-busy"), `press ${attempt}`).toBeNull();
    }
  });
});
