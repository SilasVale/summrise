// The page has one hand-rolled dialog, and its comment claims five things: it is a modal dialog,
// Escape closes it, focus lands on the first field, Tab stays inside and focus returns to the
// trigger. These tests pin those, plus the two ways it must NOT close and the listener it must
// leave behind on unmount.
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { StrictMode, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Modal } from "./Modal";

const triggers: HTMLElement[] = [];

afterEach(() => {
  // Unmount portals before resetting body; React still owns their DOM nodes.
  cleanup();
  for (const trigger of triggers.splice(0)) trigger.remove();
  document.body.innerHTML = "";
  document.body.style.overflow = "";
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

  it("leaves forward Tab before a native disclosure to the browser", () => {
    render(<Modal title="Endpoint" onClose={() => undefined}>
      <button type="button">Copy</button>
      <details><summary>Connection details</summary><p>Loopback instructions</p></details>
    </Modal>);
    const copy = screen.getByRole("button", { name: "Copy" });
    copy.focus();
    const tab = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
    fireEvent(copy, tab);
    // jsdom does not perform native Tab traversal; only the trap's interception is asserted here.
    expect(tab.defaultPrevented).toBe(false);
    expect(copy).toHaveFocus();
  });

  it("uses the native summary as the boundary instead of hidden, disabled or inert controls", () => {
    render(<Modal title="Endpoint" onClose={() => undefined}>
      <button type="button">Copy</button>
      <details><summary>Connection details</summary><button type="button">Closed content</button></details>
      <input type="hidden" />
      <button type="button" hidden>Hidden</button>
      <div hidden><button type="button">Hidden ancestor</button></div>
      <div style={{ display: "none" }}><button type="button">Display none</button></div>
      <button type="button" style={{ visibility: "hidden" }}>Invisible</button>
      <button type="button" disabled tabIndex={0}>Disabled</button>
      <fieldset disabled><button type="button">Disabled by fieldset</button></fieldset>
      <div inert><button type="button">Inert</button></div>
      <button type="button" tabIndex={-1}>Not in tab order</button>
    </Modal>);
    const copy = screen.getByRole("button", { name: "Copy" });
    const summary = within(screen.getByRole("dialog", { name: "Endpoint" })).getByText("Connection details", { selector: "summary" });
    summary.focus();
    const tab = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
    fireEvent(summary, tab);
    expect(tab.defaultPrevented).toBe(true);
    expect(copy).toHaveFocus();
    fireEvent.keyDown(copy, { key: "Tab", shiftKey: true, cancelable: true });
    expect(summary).toHaveFocus();
  });

  it("focuses the dialog itself when it has no enabled controls", () => {
    render(<Modal title="Information" onClose={() => undefined}><p>Read this result.</p></Modal>);
    const dialog = screen.getByRole("dialog", { name: "Information" });
    expect(dialog).toHaveFocus();
    expect(fireEvent.keyDown(dialog, { key: "Tab" })).toBe(false);
    expect(dialog).toHaveFocus();
  });

  it("skips a disabled first control when choosing initial focus", () => {
    render(
      <Modal title="Confirm" onClose={() => undefined}>
        <button type="button" disabled>Unavailable</button>
        <button type="button">Cancel</button>
      </Modal>
    );
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
  });

  it("makes background branches inert and restores their prior state and body scroll", () => {
    document.body.style.overflow = "scroll";
    const { unmount } = render(
      <>
        <section data-testid="background"><button type="button">Background action</button></section>
        <section data-testid="already-inert" inert>Previously unavailable</section>
        <Modal title="Confirm" onClose={() => undefined}><button type="button">Cancel</button></Modal>
      </>
    );
    const background = screen.getByTestId("background");
    const prior = screen.getByTestId("already-inert");
    expect(background.closest("[inert]")).not.toBeNull();
    expect(prior).toHaveAttribute("inert");
    expect(screen.getByRole("dialog").closest("[inert]")).toBeNull();
    expect(document.body.style.overflow).toBe("hidden");
    unmount();
    expect(background.closest("[inert]")).toBeNull();
    expect(prior).toHaveAttribute("inert");
    expect(document.body.style.overflow).toBe("scroll");
    document.body.style.overflow = "";
  });

  it("keeps newly rendered background content inert while the modal stays open", async () => {
    function Page({ notice }: { notice: boolean }) {
      return (
        <>
          <Modal title="Result" onClose={() => undefined}><button type="button">Done</button></Modal>
          {notice && <p data-testid="late-notice">New background notification</p>}
        </>
      );
    }
    const { rerender } = render(<Page notice={false} />);
    rerender(<Page notice />);
    await waitFor(() => expect(screen.getByTestId("late-notice").closest("[inert]")).not.toBeNull());
    expect(screen.getByRole("button", { name: "Done" })).toHaveFocus();
  });

  it("restores the opener after the bootstrap's StrictMode effect replay", () => {
    function Page() {
      const [open, setOpen] = useState(false);
      return <>
        <button type="button" onClick={() => setOpen(true)}>Open modal</button>
        {open && <Modal title="Rename" onClose={() => setOpen(false)}><input aria-label="New name" /></Modal>}
      </>;
    }
    render(<StrictMode><Page /></StrictMode>);
    const trigger = screen.getByRole("button", { name: "Open modal" });
    trigger.focus();
    fireEvent.click(trigger);
    expect(screen.getByLabelText("New name")).toHaveFocus();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
    expect(document.body.style.overflow).toBe("");
  });

  it("stops listening for Escape once unmounted", () => {
    const { onClose, unmount } = renderModal();

    unmount();
    fireEvent.keyDown(window, { key: "Escape" });

    expect(onClose).not.toHaveBeenCalled();
  });
});

describe("Modal stack", () => {
  it("lets only the top dialog own Escape and aria-modal", () => {
    function Page() {
      const [inner, setInner] = useState(false);
      return (
        <Modal title="Outer" onClose={() => undefined}>
          <button type="button" onClick={() => setInner(true)}>
            Open inner
          </button>
          {inner && (
            <Modal title="Inner" onClose={() => setInner(false)}>
              <button type="button">Inner action</button>
            </Modal>
          )}
        </Modal>
      );
    }

    render(<Page />);
    const outer = screen.getByRole("dialog", { name: "Outer" });
    expect(outer).toHaveAttribute("aria-modal", "true");

    fireEvent.click(screen.getByRole("button", { name: "Open inner" }));
    const inner = screen.getByRole("dialog", { name: "Inner" });
    expect(inner).toHaveAttribute("aria-modal", "true");
    expect(outer).not.toHaveAttribute("aria-modal");
    expect(screen.getByRole("button", { name: "Inner action" })).toHaveFocus();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Inner" })).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Outer" })).toHaveAttribute("aria-modal", "true");
  });
});
