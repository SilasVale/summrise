// The page has one hand-rolled dialog, and its comment claims three things: it is a modal dialog,
// Escape closes it, and focus lands on the first field. These tests pin those, plus the two ways it
// must NOT close (a mousedown inside the dialog) and the listener it must leave behind on unmount.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Modal } from "./Modal";

function renderModal(): {
  onClose: ReturnType<typeof vi.fn>;
  unmount: () => void;
  dialog: HTMLElement;
  field: HTMLElement;
} {
  const onClose = vi.fn();
  const { unmount } = render(
    <Modal title="Rename the tab" onClose={onClose}>
      <input aria-label="Name" defaultValue="tab-1" />
      <button type="button">Rename</button>
    </Modal>
  );
  return {
    onClose,
    unmount,
    dialog: screen.getByRole("dialog", { name: "Rename the tab" }),
    field: screen.getByLabelText("Name")
  };
}

describe("Modal", () => {
  it("calls itself a modal dialog and puts focus in its first field", () => {
    const { dialog, field } = renderModal();

    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(field).toHaveFocus();
  });

  it("closes on Escape, and not on another key", () => {
    const { onClose } = renderModal();

    fireEvent.keyDown(window, { key: "Enter" });
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes on a mousedown on the scrim", () => {
    const { onClose, dialog } = renderModal();
    const scrim = dialog.parentElement;
    expect(scrim).not.toBeNull();

    fireEvent.mouseDown(scrim!);

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does not close on a mousedown inside the dialog", () => {
    const { onClose, dialog, field } = renderModal();

    fireEvent.mouseDown(dialog);
    fireEvent.mouseDown(field);

    expect(onClose).not.toHaveBeenCalled();
  });

  it("stops listening for Escape once unmounted", () => {
    const { onClose, unmount } = renderModal();

    unmount();
    fireEvent.keyDown(window, { key: "Escape" });

    expect(onClose).not.toHaveBeenCalled();
  });
});
