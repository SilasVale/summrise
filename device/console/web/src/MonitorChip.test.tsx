import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MonitorChip } from "./MonitorChip";
import { notifyMonitors } from "./notifications";
import { callTool, ToolError, type Grant } from "./terminal";

// The chip's whole device surface is `callTool`; the real `ToolError` stays so a refusal is the class
// the chip actually receives from `./terminal`.
vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: vi.fn() };
});

// The chip's whole notification surface is `notifyMonitors`: the transition rule is what this file
// asserts, and the module's own tests cover what a notification says.
vi.mock("./notifications", () => ({ notifyMonitors: vi.fn() }));

const mocked = vi.mocked(callTool);
const notified = vi.mocked(notifyMonitors);

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

/** Let an awaited read finish while fake timers are installed, without running the interval. */
async function settle() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
}

describe("MonitorChip", () => {
  beforeEach(() => {
    mocked.mockReset();
    notified.mockClear();
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
    try {
      render(<MonitorChip grant={GRANT} onOpen={vi.fn()} />);

      await waitFor(() => expect(mocked).toHaveBeenCalledTimes(1));
      // Let the refusal settle; a chip that reported it would by now.
      await act(async () => {
        await Promise.resolve();
      });

      expect(screen.queryByRole("button")).not.toBeInTheDocument();
      expect(consoleError).not.toHaveBeenCalled();
    } finally {
      consoleError.mockRestore();
    }
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

  it("keeps the last down report when a refresh fails, and retries on the next tick", async () => {
    let failing = false;
    mocked.mockImplementation(async (_grant: Grant, name: string) => {
      if (name !== "monitor.list") throw new Error(`the test did not expect a ${name} call`);
      if (failing) throw new ToolError("the console never reached the device", 0);
      return JSON.stringify([{ id: "a:22", summary: DOWN }]);
    });
    vi.useFakeTimers();
    try {
      render(<MonitorChip grant={GRANT} onOpen={vi.fn()} />);
      await settle();
      expect(screen.getByRole("button", { name: "1 monitor down" })).toBeInTheDocument();

      failing = true;
      await act(async () => {
        await vi.advanceTimersByTimeAsync(20_000);
      });
      expect(mocked).toHaveBeenCalledTimes(2);
      // Unknown is not an all-clear: the last fact stays until the device answers again.
      expect(screen.getByRole("button", { name: "1 monitor down" })).toBeInTheDocument();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(20_000);
      });
      // A transport failure is retried, unlike a refused grant.
      expect(mocked).toHaveBeenCalledTimes(3);
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not overlap a slow list, and asks again on the next tick after it resolves", async () => {
    let release: ((value: string) => void) | null = null;
    mocked.mockImplementationOnce(
      () =>
        new Promise<string>((resolve) => {
          release = resolve;
        })
    );
    lists([{ id: "a:22", summary: DOWN }]);
    vi.useFakeTimers();
    try {
      render(<MonitorChip grant={GRANT} onOpen={vi.fn()} />);
      expect(mocked).toHaveBeenCalledTimes(1);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(20_000);
      });
      // The list is still on the wire: the tick is skipped rather than queued behind it.
      expect(mocked).toHaveBeenCalledTimes(1);

      await act(async () => {
        release?.(JSON.stringify([{ id: "a:22", summary: DOWN }]));
        await Promise.resolve();
      });
      await settle();
      expect(screen.getByRole("button", { name: "1 monitor down" })).toBeInTheDocument();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(20_000);
      });
      expect(mocked).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("restarts the poll on a grant change, and a late answer from the old grant never paints", async () => {
    const OTHER: Grant = { deviceId: "device-2", envelope: { token: "other" } };
    let releaseOld: ((value: string) => void) | null = null;
    mocked.mockImplementationOnce(
      () =>
        new Promise<string>((resolve) => {
          releaseOld = resolve;
        })
    );
    lists([{ id: "b:22", summary: DOWN }]);
    vi.useFakeTimers();
    try {
      const { rerender } = render(<MonitorChip grant={GRANT} onOpen={vi.fn()} />);
      expect(mocked).toHaveBeenCalledTimes(1);

      rerender(<MonitorChip grant={OTHER} onOpen={vi.fn()} />);
      // The new grant reads immediately, without waiting for the old list or the next tick.
      expect(mocked).toHaveBeenCalledTimes(2);
      expect(mocked).toHaveBeenLastCalledWith(OTHER, "monitor.list", {});
      await settle();
      expect(screen.getByRole("button", { name: "1 monitor down" })).toHaveAttribute(
        "title",
        "1 of 1 monitors down — open Monitors"
      );

      await act(async () => {
        releaseOld?.(JSON.stringify([DOWN, DOWN, DOWN]));
        await Promise.resolve();
      });
      await settle();
      // Three downs in the old envelope belong to the old grant, not this one.
      expect(screen.getByRole("button", { name: "1 monitor down" })).toHaveAttribute(
        "title",
        "1 of 1 monitors down — open Monitors"
      );

      await act(async () => {
        await vi.advanceTimersByTimeAsync(20_000);
      });
      // The new grant's timer is the only one left: one call, with the new grant.
      expect(mocked).toHaveBeenCalledTimes(3);
      expect(mocked).toHaveBeenLastCalledWith(OTHER, "monitor.list", {});
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not announce the first down read, or a count that does not rise", async () => {
    lists([{ id: "a:22", summary: DOWN }]);
    vi.useFakeTimers();
    try {
      render(<MonitorChip grant={GRANT} onOpen={vi.fn()} />);
      await settle();
      expect(screen.getByRole("button", { name: "1 monitor down" })).toBeInTheDocument();
      // The first fact is not a transition: there is no earlier count to have risen from.
      expect(notified).not.toHaveBeenCalled();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(20_000);
      });
      await settle();
      // The same count is the same fact, however many times it is read.
      expect(notified).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("announces a rise after a successful read, with the count, the total and the opener", async () => {
    let targets: Target[] = [{ id: "a:22", summary: UP }, { id: "b:443", summary: UP }];
    mocked.mockImplementation(async (_grant: Grant, name: string) => {
      if (name !== "monitor.list") throw new Error(`the test did not expect a ${name} call`);
      return JSON.stringify(targets);
    });
    const onOpen = vi.fn();
    vi.useFakeTimers();
    try {
      render(<MonitorChip grant={GRANT} onOpen={onOpen} />);
      await settle();
      expect(notified).not.toHaveBeenCalled();

      targets = [{ id: "a:22", summary: DOWN }, { id: "b:443", summary: UP }];
      await act(async () => {
        await vi.advanceTimersByTimeAsync(20_000);
      });
      await settle();
      expect(notified).toHaveBeenCalledTimes(1);
      expect(notified).toHaveBeenCalledWith(1, 2, onOpen);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(20_000);
      });
      await settle();
      // Once per rise: reading the same down target again is not a second outage.
      expect(notified).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("announces a new rise after a fall, and nothing when the count falls", async () => {
    let targets: Target[] = [{ id: "a:22", summary: DOWN }];
    mocked.mockImplementation(async (_grant: Grant, name: string) => {
      if (name !== "monitor.list") throw new Error(`the test did not expect a ${name} call`);
      return JSON.stringify(targets);
    });
    const onOpen = vi.fn();
    vi.useFakeTimers();
    try {
      render(<MonitorChip grant={GRANT} onOpen={onOpen} />);
      await settle();
      expect(notified).not.toHaveBeenCalled();

      targets = [{ id: "a:22", summary: UP }];
      await act(async () => {
        await vi.advanceTimersByTimeAsync(20_000);
      });
      await settle();
      // A fall is good news; it is not an announcement.
      expect(notified).not.toHaveBeenCalled();

      targets = [{ id: "a:22", summary: DOWN }];
      await act(async () => {
        await vi.advanceTimersByTimeAsync(20_000);
      });
      await settle();
      expect(notified).toHaveBeenCalledTimes(1);
      expect(notified).toHaveBeenLastCalledWith(1, 1, onOpen);

      targets = [{ id: "a:22", summary: UP }];
      await act(async () => {
        await vi.advanceTimersByTimeAsync(20_000);
      });
      await settle();
      expect(notified).toHaveBeenCalledTimes(1);

      targets = [{ id: "a:22", summary: DOWN }];
      await act(async () => {
        await vi.advanceTimersByTimeAsync(20_000);
      });
      await settle();
      // The second outage is a second rise, and it is announced once.
      expect(notified).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("stops polling when the grant is refused", async () => {
    mocked.mockRejectedValue(new ToolError("the grant expired", 403, -32001));
    vi.useFakeTimers();
    try {
      render(<MonitorChip grant={GRANT} onOpen={vi.fn()} />);
      await settle();
      expect(mocked).toHaveBeenCalledTimes(1);
      // A refused read is not a fall to zero, and it is not a rise: it says nothing at all.
      expect(notified).not.toHaveBeenCalled();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(20_000);
      });
      // A grant that does not allow monitor.list cannot be fixed by asking again every 20 seconds.
      expect(mocked).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });
});
