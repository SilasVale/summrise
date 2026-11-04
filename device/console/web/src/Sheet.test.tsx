import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Sheet } from "./Sheet";
import { Modal } from "./Modal";

function open(onClose = vi.fn(), busy = false) {
  return render(
    <Sheet title="Try a tool" onClose={onClose} busy={busy}>
      <label htmlFor="path">Path</label>
      <input id="path" defaultValue="" />
      <button type="button">Run</button>
    </Sheet>
  );
}

describe("Sheet", () => {
  it("is a labelled modal, focuses its first control and locks the page behind it", () => {
    const { unmount } = open();

    const dialog = screen.getByRole("dialog", { name: "Try a tool" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(screen.getByLabelText("Path")).toHaveFocus();
    expect(screen.getByTestId("sheet-scrim")).toBeInTheDocument();
    expect(document.body.style.overflow).toBe("hidden");

    unmount();
    expect(document.body.style.overflow).toBe("");
  });

  it("focuses the first field even when a mode toggle comes before it", () => {
    render(
      <Sheet title="Try a tool" onClose={vi.fn()}>
        <button type="button">Raw JSON</button>
        <label htmlFor="path">Path</label>
        <input id="path" defaultValue="" />
      </Sheet>
    );

    // "Raw JSON" is a way of not filling the form; the form is the job.
    expect(screen.getByLabelText("Path")).toHaveFocus();
    expect(screen.getByRole("button", { name: "Raw JSON" })).not.toHaveFocus();
  });

  it("traps Tab and Shift+Tab inside the sheet", () => {
    open();
    const close = screen.getByRole("button", { name: "Close" });
    const run = screen.getByRole("button", { name: "Run" });

    run.focus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(close).toHaveFocus();

    close.focus();
    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(run).toHaveFocus();
  });

  it("closes on Escape and on a backdrop press, and returns focus to the trigger", async () => {
    const trigger = document.createElement("button");
    trigger.textContent = "Open";
    document.body.append(trigger);
    trigger.focus();

    const onClose = vi.fn();
    const { unmount } = open(onClose);

    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);

    fireEvent.mouseDown(screen.getByTestId("sheet-scrim"));
    expect(onClose).toHaveBeenCalledTimes(2);

    fireEvent.mouseDown(screen.getByRole("dialog"));
    expect(onClose).toHaveBeenCalledTimes(2);

    unmount();
    expect(trigger).toHaveFocus();
    trigger.remove();
  });

  it("refuses Escape, the close button and the backdrop while a call is running", () => {
    const onClose = vi.fn();
    open(onClose, true);

    fireEvent.keyDown(document, { key: "Escape" });
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    fireEvent.mouseDown(screen.getByTestId("sheet-scrim"));

    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Close" })).toBeDisabled();
  });

  it("lets only the top dialog own Escape when a Modal opens over it", () => {
    const outer = vi.fn();
    const inner = vi.fn();
    render(
      <>
        <Sheet title="Outer" onClose={outer}>
          <button type="button">Outer action</button>
        </Sheet>
        <Modal title="Inner" onClose={inner}>
          <button type="button">Inner action</button>
        </Modal>
      </>
    );

    expect(screen.getByRole("dialog", { name: "Inner" })).toHaveAttribute("aria-modal", "true");
    expect(screen.getByRole("dialog", { name: "Outer" })).not.toHaveAttribute("aria-modal");

    fireEvent.keyDown(document, { key: "Escape" });
    expect(inner).toHaveBeenCalledTimes(1);
    expect(outer).not.toHaveBeenCalled();
  });
});
