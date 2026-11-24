import { fireEvent, render, screen, within } from "@testing-library/react";
import { createContext, StrictMode, useContext, useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { Sheet } from "./Sheet";
import { Modal } from "./Modal";

function open(onClose = vi.fn(), busy = false) {
  return render(
    <Sheet title="Edit a setting" onClose={onClose} busy={busy}>
      <label htmlFor="path">Path</label>
      <input id="path" defaultValue="" />
      <button type="button">Save</button>
    </Sheet>
  );
}

describe("Sheet", () => {
  it("is a labelled modal, focuses its first control and locks the page behind it", () => {
    const { unmount } = open();

    const dialog = screen.getByRole("dialog", { name: "Edit a setting" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(screen.getByLabelText("Path")).toHaveFocus();
    expect(screen.getByTestId("sheet-scrim")).toBeInTheDocument();
    expect(document.body.style.overflow).toBe("hidden");

    unmount();
    expect(document.body.style.overflow).toBe("");
  });

  it("focuses the first field even when a mode toggle comes before it", () => {
    render(
      <Sheet title="Edit a setting" onClose={vi.fn()}>
        <button type="button">Advanced</button>
        <label htmlFor="path">Path</label>
        <input id="path" defaultValue="" />
      </Sheet>
    );

    // "Advanced" is a way of not filling the form; the form is the job.
    expect(screen.getByLabelText("Path")).toHaveFocus();
    expect(screen.getByRole("button", { name: "Advanced" })).not.toHaveFocus();
  });

  it("traps Tab and Shift+Tab inside the sheet", () => {
    open();
    const close = screen.getByRole("button", { name: "Close" });
    const save = screen.getByRole("button", { name: "Save" });

    save.focus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(close).toHaveFocus();

    close.focus();
    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(save).toHaveFocus();
  });

  it("leaves forward Tab before a native disclosure to the browser", () => {
    render(<Sheet title="Endpoint" onClose={() => undefined}>
      <button type="button">Copy</button>
      <details><summary>Connection details</summary><p>Loopback instructions</p></details>
    </Sheet>);
    const copy = screen.getByRole("button", { name: "Copy" });
    copy.focus();
    const tab = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
    fireEvent(copy, tab);
    // jsdom has no native Tab default action: this proves the trap leaves the interior step alone.
    expect(tab.defaultPrevented).toBe(false);
    expect(copy).toHaveFocus();
  });

  it("uses the native summary as the boundary instead of hidden, disabled or inert controls", () => {
    render(<Sheet title="Endpoint" onClose={() => undefined}>
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
    </Sheet>);
    const close = screen.getByRole("button", { name: "Close" });
    const summary = screen.getByText("Connection details", { selector: "summary" });
    summary.focus();
    const tab = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
    fireEvent(summary, tab);
    expect(tab.defaultPrevented).toBe(true);
    expect(close).toHaveFocus();
    fireEvent.keyDown(close, { key: "Tab", shiftKey: true, cancelable: true });
    expect(summary).toHaveFocus();
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

  it("mounts a sheet confirmation at the body without losing its React context", () => {
    const JobContext = createContext("missing job");
    function JobDetails() {
      const job = useContext(JobContext);
      return <p>{job}</p>;
    }
    function Page() {
      const [confirm, setConfirm] = useState(false);
      return <JobContext.Provider value="Revoke laptop token">
        <Sheet title="Tokens" onClose={() => undefined}>
          <button type="button" onClick={() => setConfirm(true)}>Revoke</button>
          {confirm && <Modal title="Confirmation" onClose={() => undefined}>
            <JobDetails />
            <button type="button">Cancel</button>
          </Modal>}
        </Sheet>
      </JobContext.Provider>;
    }
    const { container, unmount } = render(<Page />);
    const sheet = screen.getByRole("dialog", { name: "Tokens" });
    fireEvent.click(screen.getByRole("button", { name: "Revoke" }));
    const confirmation = screen.getByRole("dialog", { name: "Confirmation" });
    // A sheet's shadow/overflow/animation must not become ancestors of the confirmation.
    expect(confirmation.closest(".scrim")?.parentElement).toBe(document.body);
    expect(sheet.contains(confirmation)).toBe(false);
    expect(container.contains(confirmation)).toBe(false);
    expect(within(confirmation).getByText("Revoke laptop token")).toBeInTheDocument();
    expect(confirmation.closest("[inert]")).toBeNull();
    expect(sheet.closest("[inert]")).not.toBeNull();
    unmount();
    expect(screen.queryByRole("dialog", { hidden: true })).not.toBeInTheDocument();
  });

  it("keeps the outer sheet open behind a confirmation and restores focus one layer at a time", () => {
    function Page() {
      const [sheet, setSheet] = useState(false);
      const [modal, setModal] = useState(false);
      return (
        <>
          <button type="button" onClick={() => setSheet(true)}>Open settings</button>
          {sheet && (
            <Sheet title="Settings" onClose={() => setSheet(false)}>
              <button type="button" onClick={() => setModal(true)}>Open confirmation</button>
              {modal && (
                <Modal title="Confirmation" onClose={() => setModal(false)}>
                  <button type="button" onClick={() => setModal(false)}>Cancel</button>
                  <button type="button">Confirm</button>
                </Modal>
              )}
            </Sheet>
          )}
        </>
      );
    }
    render(<Page />);
    const trigger = screen.getByRole("button", { name: "Open settings" });
    trigger.focus();
    fireEvent.click(trigger);
    const outer = screen.getByRole("dialog", { name: "Settings" });
    const confirmationTrigger = screen.getByRole("button", { name: "Open confirmation" });
    confirmationTrigger.focus();
    fireEvent.click(confirmationTrigger);
    const inner = screen.getByRole("dialog", { name: "Confirmation" });
    expect(inner.closest("[inert]")).toBeNull();
    expect(confirmationTrigger.closest("[inert]")).not.toBeNull();
    const cancel = within(inner).getByRole("button", { name: "Cancel" });
    const confirm = within(inner).getByRole("button", { name: "Confirm" });
    expect(cancel).toHaveFocus();
    expect(inner).toHaveAttribute("aria-modal", "true");
    expect(outer).not.toHaveAttribute("aria-modal");
    fireEvent.keyDown(cancel, { key: "Tab", shiftKey: true });
    expect(confirm).toHaveFocus();
    fireEvent.keyDown(confirm, { key: "Tab" });
    expect(cancel).toHaveFocus();
    fireEvent.mouseDown(cancel);
    expect(inner).toBeInTheDocument();
    fireEvent.mouseDown(screen.getByTestId("sheet-scrim"));
    expect(outer).toBeInTheDocument();
    expect(inner).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Confirmation" })).not.toBeInTheDocument();
    expect(confirmationTrigger).toHaveFocus();
    expect(outer).toHaveAttribute("aria-modal", "true");
    expect(confirmationTrigger.closest("[inert]")).toBeNull();
    expect(document.body.style.overflow).toBe("hidden");
    expect(trigger.closest("[inert]")).not.toBeNull();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
    expect(trigger.closest("[inert]")).toBeNull();
    expect(document.body.style.overflow).toBe("");
  });

  it("keeps the remaining sheet locked and focused when an underlying sheet unmounts", () => {
    function Page({ outer }: { outer: boolean }) {
      return (
        <>
          {outer && <Sheet title="Outer" onClose={() => undefined}><input aria-label="Outer field" /></Sheet>}
          <Sheet title="Inner" onClose={() => undefined}><input aria-label="Inner field" /></Sheet>
        </>
      );
    }
    const { rerender, unmount } = render(<Page outer />);
    const field = screen.getByLabelText("Inner field");
    expect(field).toHaveFocus();
    rerender(<Page outer={false} />);
    expect(document.body.style.overflow).toBe("hidden");
    expect(field).toHaveFocus();
    unmount();
    expect(document.body.style.overflow).toBe("");
  });

  it("uses the current busy state for Escape and backdrop dismissal", () => {
    const onClose = vi.fn();
    const { rerender } = open(onClose);
    rerender(<Sheet title="Edit a setting" busy onClose={onClose}><button type="button">Save</button></Sheet>);
    fireEvent.keyDown(document, { key: "Escape" });
    fireEvent.mouseDown(screen.getByTestId("sheet-scrim"));
    expect(onClose).not.toHaveBeenCalled();
    rerender(<Sheet title="Edit a setting" onClose={onClose}><button type="button">Save</button></Sheet>);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("restores the opener after the bootstrap's StrictMode effect replay", () => {
    function Page() {
      const [open, setOpen] = useState(false);
      return <>
        <button type="button" onClick={() => setOpen(true)}>Open sheet</button>
        {open && <Sheet title="Settings" onClose={() => setOpen(false)}><input aria-label="Setting" /></Sheet>}
      </>;
    }
    render(<StrictMode><Page /></StrictMode>);
    const trigger = screen.getByRole("button", { name: "Open sheet" });
    trigger.focus();
    fireEvent.click(trigger);
    expect(screen.getByLabelText("Setting")).toHaveFocus();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
    expect(document.body.style.overflow).toBe("");
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

    fireEvent.mouseDown(screen.getByTestId("sheet-scrim"));
    expect(outer).not.toHaveBeenCalled();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(inner).toHaveBeenCalledTimes(1);
    expect(outer).not.toHaveBeenCalled();
  });
});
