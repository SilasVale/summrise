import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MonitorChip } from "./MonitorChip";
import { callTool, ToolError, type Grant } from "./terminal";

// The chip's whole device surface is `callTool`; the real `ToolError` stays so a refusal is the class
// the chip actually receives from `./terminal`.
vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: vi.fn() };
});

const mocked = vi.mocked(callTool);

const GRANT: Grant = { deviceId: "device-1", envelope: { token: "envelope" } };

// The Monitors page's `Target`/`Status` are private; these are their wire shapes, not a second
// definition. Only `summary.up_now` is the fact the chip counts.
type Status = { up_now: boolean | null };

type Target = { id: string; summary?: Status | null };

const UP: Status = { up_now: true };
const DOWN: Status = { up_now: false };

// Answer `monitor.list`; an unasked-for call fails the test instead of returning a plausible value.
function lists(targets: Target[]) {
  mocked.mockImplementation(async (_grant: Grant, name: string) => {
    if (name === "monitor.list") return JSON.stringify(targets);
    throw new Error(`the test did not expect a ${name} call`);
  });
}

describe("MonitorChip", () => {
  beforeEach(() => {
    mocked.mockReset();
  });

  it("says how many monitors are down, names the total, and opens Monitors", async () => {
    lists([{ id: "a:22", summary: DOWN }, { id: "b:443", summary: UP }]);
    const onOpen = vi.fn();
    render(<MonitorChip grant={GRANT} onOpen={onOpen} />);

    const chip = await screen.findByRole("button", { name: "1 monitor down" });
    expect(chip).toHaveAttribute("title", "1 of 2 monitors down — open Monitors");

    fireEvent.click(chip);
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("renders nothing when every target is up or has no summary", async () => {
    lists([{ id: "a:22", summary: UP }, { id: "b:443", summary: null }, { id: "c:80" }]);
    render(<MonitorChip grant={GRANT} onOpen={vi.fn()} />);

    await waitFor(() => expect(mocked).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("renders nothing, and complains nowhere, when the device refuses the list", async () => {
    mocked.mockRejectedValue(new ToolError("the grant expired", 403, -32001));
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    render(<MonitorChip grant={GRANT} onOpen={vi.fn()} />);

    await waitFor(() => expect(mocked).toHaveBeenCalledTimes(1));
    // Let the refusal settle; a chip that reported it would by now.
    await act(async () => {
      await Promise.resolve();
    });

    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(consoleError).not.toHaveBeenCalled();
  });

  it("renders nothing for an empty list, and still asked the device once", async () => {
    lists([]);
    render(<MonitorChip grant={GRANT} onOpen={vi.fn()} />);

    await waitFor(() => expect(mocked).toHaveBeenCalledTimes(1));
    expect(mocked).toHaveBeenCalledWith(GRANT, "monitor.list", {});
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("polls every 20 seconds, and unmounting clears the timer", async () => {
    lists([]);
    vi.useFakeTimers();
    try {
      const start = vi.spyOn(window, "setInterval");
      const stop = vi.spyOn(window, "clearInterval");
      const { unmount } = render(<MonitorChip grant={GRANT} onOpen={vi.fn()} />);

      const poll = start.mock.calls.findIndex(([, delay]) => delay === 20_000);
      expect(poll).toBeGreaterThanOrEqual(0);
      const timer = start.mock.results[poll].value;
      expect(mocked).toHaveBeenCalledTimes(1);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(20_000);
      });
      expect(mocked).toHaveBeenCalledTimes(2);

      unmount();

      expect(stop).toHaveBeenCalledWith(timer);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(20_000);
      });
      // The unmounted chip made no third call.
      expect(mocked).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });
});
