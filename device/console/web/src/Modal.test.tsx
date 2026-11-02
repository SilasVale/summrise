// The page has one hand-rolled dialog, and its comment claims five things: it is a modal dialog,
// Escape closes it, focus lands on the first field, Tab stays inside and focus returns to the
// trigger. These tests pin those, plus the two ways it must NOT close and the listener it must
// leave behind on unmount.
import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Modal } from "./Modal";

const triggers: HTMLElement[] = [];

afterEach(() => {
  for (const trigger of triggers.splice(0)) trigger.remove();
  document.body.innerHTML = "";
});

function renderModal(): {
  onClose: ReturnType<typeof vi.fn>;
  unmount: () => void;
  dialog: HTMLElement;
  field: HTMLElement;
  button: HTMLElement;
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
    field: screen.getByLabelText("Name"),
    button: screen.getByRole("button", { name: "Rename" })
  };
}

describe("Modal", () => {
  it("calls itself a modal dialog and puts focus in its first field", () => {
    const { dialog, field } = renderModal();

    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(field).toHaveFocus();
  });

  it("keeps focus in the field being typed in, and closes with the latest handler", () => {
    const first = vi.fn();
    const second = vi.fn();

    function Page() {
      const [switched, setSwitched] = useState(false);
      const [host, setHost] = useState("");
      const close = switched ? second : first;
      return (
        <>
          <button type="button" onClick={() => setSwitched(true)}>
            use a new handler
          </button>
          <Modal title="Save a terminal preset" onClose={() => close()}>
            <input aria-label="Name" defaultValue="the db host" />
            <input aria-label="Host" value={host} onChange={(event) => setHost(event.target.value)} />
          </Modal>
        </>
      );
    }

    render(<Page />);
    const host = screen.getByLabelText("Host");
    host.focus();

    fireEvent.change(host, { target: { value: "db.example" } });

    // The controlled value re-rendered the page; the caret must not be pulled back to Name.
    expect(host).toHaveFocus();

    fireEvent.click(screen.getByRole("button", { name: "use a new handler" }));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(second).toHaveBeenCalledTimes(1);
    expect(first).not.toHaveBeenCalled();

    fireEvent.mouseDown(screen.getByTestId("scrim"));
    expect(second).toHaveBeenCalledTimes(2);
  });

  it("closes on Escape, and not on another key", () => {
    const { onClose } = renderModal();

    fireEvent.keyDown(window, { key: "Enter" });
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes on a mousedown on the scrim", () => {
    const { onClose } = renderModal();

    fireEvent.mouseDown(screen.getByTestId("scrim"));

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does not close on a mousedown inside the dialog", () => {
    const { onClose, dialog, field } = renderModal();

    fireEvent.mouseDown(dialog);
    fireEvent.mouseDown(field);

    expect(onClose).not.toHaveBeenCalled();
  });

  it("keeps Tab inside and gives focus back to the trigger on unmount", () => {
    const trigger = document.createElement("button");
    trigger.textContent = "open";
    document.body.append(trigger);
    triggers.push(trigger);
    trigger.focus();

    const { field, button, unmount } = renderModal();
    expect(field).toHaveFocus();

    button.focus();
    fireEvent.keyDown(window, { key: "Tab" });
    expect(field).toHaveFocus();

    field.focus();
    fireEvent.keyDown(window, { key: "Tab", shiftKey: true });
    expect(button).toHaveFocus();

    unmount();
    expect(trigger).toHaveFocus();
  });

  it("stops listening for Escape once unmounted", () => {
    const { onClose, unmount } = renderModal();

    unmount();
    fireEvent.keyDown(window, { key: "Escape" });

    expect(onClose).not.toHaveBeenCalled();
  });
});
