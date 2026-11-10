import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Device } from "./api";
import { DevicePicker } from "./DevicePicker";

const devices: Device[] = [
  { id: "a", name: "Alpha", online: true, version: "1.2.3", last_seen_ms: 1 },
  { id: "b", name: "Beta", online: false, version: null, last_seen_ms: null }
];

describe("DevicePicker", () => {
  it("names each Open button with its machine and states the state in words", () => {
    render(<DevicePicker devices={devices} onChoose={vi.fn()} />);

    // **ONE VERB, ONE NAME (audit-44 F9).** The picker's door was `Use` while Home's rows said
    // `Open`; it is the same action, so it says `Open` too.
    const alpha = screen.getByRole("button", { name: "Open Alpha" });
    expect(alpha).toHaveTextContent(/^Open$/);
    expect(screen.getByRole("button", { name: "Open Beta" })).toBeInTheDocument();
    // **THE STATE IS A WORD, NOT ONLY A COLOUR.** The words used to be `.sr-only` under an
    // `aria-hidden` dot; a sighted operator who cannot read green from grey had no state at all.
    const online = screen.getByText("online");
    const offline = screen.getByText("offline");
    expect(online).toHaveClass("picker-state");
    expect(offline).toHaveClass("picker-state");
    expect(online).not.toHaveClass("sr-only");
    // The age stays one hover away, as Home's row spells it; a machine the console never heard
    // states the state alone rather than inventing a moment.
    expect(online).toHaveAttribute("title", expect.stringMatching(/^online · seen .+ ago$/));
    expect(offline).toHaveAttribute("title", "offline");
  });

  it("keeps the version's v, so it is the same fact as Home's column", () => {
    render(<DevicePicker devices={devices} onChoose={vi.fn()} />);
    expect(screen.getByText("v1.2.3")).toBeInTheDocument();
    expect(screen.queryByText("1.2.3")).not.toBeInTheDocument();
  });

  it("says the machine list is being read instead of calling the fleet empty", () => {
    // audit-43 F4: `/fleet` takes up to 15 s, and the Device destination said `No devices are
    // enrolled yet.` for every one of them. An empty list is a fact only when a read answered.
    render(<DevicePicker devices={[]} onChoose={vi.fn()} fleetRead={{ state: "reading" }} />);

    // **THE READING STATE IS THE SHAPE, NOT A SENTENCE (the ninety-seventh list).** Measured with `/fleet`
    // held 90 s: six of this product's seven pages drew the contract's `loading` atom and this one drew
    // `p.hint` — the same sentence, in a 52 px card where three 60 px rows were about to arrive.
    const status = screen.getByRole("status");
    expect(status).toHaveClass("skeleton-list");
    expect(status).toHaveAttribute("aria-label", "Reading the machine list");
    expect(status.querySelectorAll(".skeleton-row")).toHaveLength(3);
    expect(screen.queryByText(/No devices are enrolled yet/)).toBeNull();
  });

  it("states a failed machine-list read and offers the one Retry", () => {
    const onRetry = vi.fn();
    render(
      <DevicePicker
        devices={[]}
        onChoose={vi.fn()}
        fleetRead={{ state: "failed", reason: "Failed to fetch" }}
        onRetry={onRetry}
      />
    );

    expect(screen.getByRole("status")).toHaveTextContent("The machine list could not be read.");
    // The browser's own cause stays evidence, in the title, as Home's failed notice keeps it.
    expect(screen.getByTitle("Failed to fetch")).toBeInTheDocument();
    expect(screen.queryByText(/No devices are enrolled yet/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("states the enrollment sentence only when a successful read brought no machines", () => {
    render(<DevicePicker devices={[]} onChoose={vi.fn()} fleetRead={{ state: "ready" }} />);

    // **THE EMPTY PAGE SAYS THE FACT, THEN THE DOOR (the ninety-eighth list).** Measured with `/fleet`
    // answering `[]`: this drew ONE 18 px line inside the card's own 16 px — a box hugging its sentence like
    // a label — where the contract's `empty` atom says a card at 14/16 with `wayBack: true`. A reader who
    // arrives here needs BOTH: nothing is enrolled, and where to add one. One line gives the first and leaves
    // the second implicit.
    expect(screen.getByRole("heading", { name: "No devices are enrolled yet" })).toBeInTheDocument();
    expect(
      screen.getByText("Add one from Home, and it appears here with its own address and version."),
    ).toBeInTheDocument();
    // **AND IT IS NOT A CARD INSIDE A CARD**: the page's `.card` already paints the atom's box, so this
    // branch brings the atom's CONTENT and no second surface (the thirty-eighth list's rule).
    expect(document.querySelector(".empty-state")).toBeNull();
    // (This test renders the picker alone, so the page's card is not here to check; what the PICKER owns is
    // the atom's content and no second surface — the page's `.card` paints the box.)
    expect(document.querySelector(".picker-empty")).not.toBeNull();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("keeps a last known list on the page while the read is pending or failed", () => {
    // A stale list is still the console's knowledge; the page's question is which machine, and the
    // shell's banner owns the failure. Only a zero list may be stated by the read's state.
    for (const fleetRead of [{ state: "reading" }, { state: "failed", reason: "boom" }] as const) {
      const { unmount } = render(
        <DevicePicker devices={devices} onChoose={vi.fn()} fleetRead={fleetRead} />
      );
      expect(screen.getByRole("button", { name: "Open Alpha" })).toBeInTheDocument();
      expect(screen.queryByText(/No devices are enrolled yet/)).toBeNull();
      unmount();
    }
  });
});
