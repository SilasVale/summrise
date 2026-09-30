// DshPane pins — the harness controller with a mocked main-process bridge.
//
// WHAT THIS CAN AND CANNOT SEE: it pins the SPA side of the contract (that the pane asks the MAIN
// process to open the view, reports the slot, hides the view on the way out, and turns a crashed
// renderer into a recovery banner). It cannot see the view itself — there is no Electron here, and
// jsdom has no layout, so `getBoundingClientRect()` is all zeros and the pane correctly reports
// "no bounds" instead of a rectangle. The real geometry is the desktop app's to get right.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, act } from "@testing-library/react";
import { DshPane } from "../DshPane";

interface GoneHandler {
  (d: { reason: string; exitCode: number }): void;
}

const bridge = () => {
  const handlers = { gone: [] as GoneHandler[] };
  return {
    handlers,
    mock: {
      // `open` takes NO argument — that is the point of the surface: the port lives in the main
      // process, so the SPA never learns an address.
      open: vi.fn(() => Promise.resolve({ ok: true, url: "http://127.0.0.1:18081/" })),
      place: vi.fn(() => Promise.resolve()),
      state: vi.fn(() => Promise.resolve({ ok: true, url: "http://127.0.0.1:18081/", visible: true })),
      reload: vi.fn(() => Promise.resolve()),
      recover: vi.fn(() => Promise.resolve()),
      onGone: vi.fn((h: GoneHandler) => {
        handlers.gone.push(h);
        return () => {};
      }),
    },
  };
};

describe("DshPane", () => {
  let b: ReturnType<typeof bridge>;

  beforeEach(() => {
    b = bridge();
    (window as unknown as { summriseDsh?: unknown }).summriseDsh = b.mock;
  });
  afterEach(() => {
    delete (window as unknown as { summriseDsh?: unknown }).summriseDsh;
  });

  it("asks the MAIN process to open the harness, and passes it no address", async () => {
    render(<DshPane />);
    await waitFor(() => expect(b.mock.open).toHaveBeenCalledTimes(1));
    expect(b.mock.open.mock.calls[0]).toEqual([]);
    // The slot is reported (null in jsdom: no layout, so the pane says "no bounds" rather than
    // inventing a rectangle) and it records that it asked for the view.
    await waitFor(() => expect(b.mock.place).toHaveBeenCalled());
    expect(screen.getByRole("button", { name: "Reload" })).toBeTruthy();
  });

  it("hides the native view when the page is left, so it cannot linger over other pages", async () => {
    const { unmount } = render(<DshPane />);
    await waitFor(() => expect(b.mock.place).toHaveBeenCalled());
    unmount();
    await waitFor(() => expect(b.mock.place).toHaveBeenLastCalledWith(null));
  });

  it("a crashed renderer becomes a banner, and Recover asks the main process", async () => {
    render(<DshPane />);
    await waitFor(() => expect(b.handlers.gone.length).toBe(1));
    act(() => b.handlers.gone[0]({ reason: "crashed", exitCode: 133 }));
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("crashed"));
    fireEvent.click(screen.getByRole("button", { name: "Recover" }));
    await waitFor(() => expect(b.mock.recover).toHaveBeenCalledTimes(1));
  });
});
