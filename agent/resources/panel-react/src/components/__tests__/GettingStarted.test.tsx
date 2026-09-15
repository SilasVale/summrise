// The getting-started card: what a first-run guide must DO, beyond looking right.
//
// It has to open once, close in every way a reader will try (button, Esc, backdrop), never close
// from a click INSIDE it, take the reader somewhere when they click a step's chip, and carry the
// accessibility basics (a labelled modal dialog with focus on its primary action). The card's
// colours are measured separately, against the built stylesheet, in `lib/themeContrast.test.ts`.
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { GettingStarted } from "../GettingStarted";
import { STEPS } from "../../lib/gettingStarted";

const noop = () => {};

describe("GettingStarted", () => {
  it("shows what the product is for, and every step with its way in", () => {
    const { container } = render(<GettingStarted onClose={noop} onGoTo={noop} />);
    expect(screen.getByRole("heading", { name: "Getting started" })).toBeTruthy();
    expect(container.querySelector(".gs-lead")!.textContent!.length).toBeGreaterThan(60);
    expect(container.querySelectorAll(".gs-step")).toHaveLength(STEPS.length);
    // Each step names the rail chip the reader should look for.
    for (const step of STEPS) {
      expect(screen.getByText(step.where)).toBeTruthy();
    }
  });

  it("is a labelled modal dialog, and the keyboard lands on its primary action", () => {
    const { container } = render(<GettingStarted onClose={noop} />);
    const card = container.querySelector(".gs-card")!;
    expect(card.getAttribute("role")).toBe("dialog");
    expect(card.getAttribute("aria-modal")).toBe("true");
    // aria-labelledby must point at the element that actually holds the title.
    const labelledBy = card.getAttribute("aria-labelledby");
    expect(labelledBy).toBeTruthy();
    expect(card.querySelector(`#${labelledBy}`)!.textContent).toBe("Getting started");
    // Focus starts on "Got it": the reader can dismiss with one key, not a tab hunt.
    expect(document.activeElement).toBe(container.querySelector(".gs-foot .btn.primary"));
  });

  it("closes from the button, from Esc, and from the backdrop — but not from inside the card", () => {
    const onClose = vi.fn();
    const { container, unmount } = render(<GettingStarted onClose={onClose} />);
    fireEvent.click(container.querySelector(".gs-foot .btn.primary")!);
    expect(onClose).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(2);

    // A click INSIDE the card is not a dismissal: the target is a child, not the backdrop.
    fireEvent.click(container.querySelector(".gs-lead")!);
    expect(onClose).toHaveBeenCalledTimes(2);

    const backdrop = container.querySelector(".gs-backdrop")!;
    fireEvent.click(backdrop, { target: backdrop });
    expect(onClose).toHaveBeenCalledTimes(3);

    // The close X is the fourth way, and it is labelled for a screen reader.
    fireEvent.click(screen.getByLabelText("Close the guide"));
    expect(onClose).toHaveBeenCalledTimes(4);

    // The Esc listener must not outlive the card: a stale listener would close the NEXT thing.
    unmount();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(4);
  });

  it("the chip takes the reader to the page it names", () => {
    const onGoTo = vi.fn();
    render(<GettingStarted onClose={noop} onGoTo={onGoTo} />);
    fireEvent.click(screen.getByText(STEPS[1].where));
    expect(onGoTo).toHaveBeenCalledWith(STEPS[1].page);
  });

  it("a command is monospace and a click path is prose", () => {
    // The distinction the first screenshot review caught: monospace reads as "type this", so it
    // may only carry something that can be typed.
    const { container } = render(<GettingStarted onClose={noop} />);
    for (const step of STEPS) {
      const card = container.querySelector(".gs-card")!;
      const mono = [...card.querySelectorAll(".gs-action")].map((e) => e.textContent);
      const prose = [...card.querySelectorAll(".gs-click")].map((e) => e.textContent);
      if (step.action) expect(mono).toContain(step.action);
      if (step.click) expect(prose).toContain(step.click);
    }
  });

  it("without a navigation callback the chips are labels, not dead buttons", () => {
    const { container } = render(<GettingStarted onClose={noop} />);
    expect(container.querySelectorAll("button.gs-where")).toHaveLength(0);
    expect(container.querySelectorAll("span.gs-where.static")).toHaveLength(STEPS.length);
  });
});
