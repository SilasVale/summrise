import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LiveLine, StartedCall } from "./liveActivity";
import { LiveNow } from "./LiveNow";

// The band exists to answer one question: is the AI working right now? The console's own reads — the
// terminal page's ten-second `terminal.list`, the stream's parked `terminal.read` — are lines in the
// same audit, and counting them made an idle machine read "Active now · 6 calls in the last minute".
// So the band counts a **client's** calls only; a line with no caller is not one. It reads the state
// from that window and draws none of its rows: the day's calls are the timeline's, once
// (audit-41 F1).
function line(overrides: Partial<LiveLine> = {}): LiveLine {
  return {
    ts_ms: Date.now(),
    kind: "mcp",
    status: 200,
    sentence: "Read a file",
    ...overrides
  };
}

const base = { state: "live" as const };

function started(overrides: Partial<StartedCall> = {}): StartedCall {
  return { id: "call-1", tool: "runs.exec", at_ms: Date.now(), caller: "client", ...overrides };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("LiveNow", () => {
  it("does not read Active now for the console's own read", () => {
    render(
      <LiveNow
        {...base}
        lines={[line({ caller: "console", sentence: "Listed the terminal sessions", tool: "terminal.list" })]}
      />
    );

    expect(screen.getByText("Quiet")).toBeInTheDocument();
    expect(screen.queryByText("Active now")).not.toBeInTheDocument();
    // The rate counts the AI's calls, so a console read is not one of them either.
    expect(screen.getByText("no calls in the last minute")).toBeInTheDocument();
    // And it is not shown as one of the AI's last steps.
    expect(screen.queryByText("Listed the terminal sessions")).not.toBeInTheDocument();
  });

  it("reads Active now and counts a client's call", () => {
    render(<LiveNow {...base} lines={[line({ caller: "client" })]} />);

    expect(screen.getByText("Active now")).toBeInTheDocument();
    expect(screen.getByText("1 call in the last minute")).toBeInTheDocument();
    // The call itself is the timeline's. The band is the state, not a second list of rows.
    expect(screen.queryByText("Read a file")).not.toBeInTheDocument();
    expect(document.querySelectorAll(".live-now .live-entry")).toHaveLength(0);
  });

  it("keeps the Now facts, with no call rows and no door of its own", () => {
    render(<LiveNow lines={[line({ caller: "client" })]} state="live" />);

    // The state band's own shape: its heading, its presence word, its rate and the sparkline. The
    // record's one door lives with the day line below; the band is the state, not a second list.
    expect(screen.getByRole("heading", { name: "Now", level: 2 })).toBeInTheDocument();
    expect(screen.getByText("Active now")).toBeInTheDocument();
    expect(screen.getByText("1 call in the last minute")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /Client calls per ten seconds/ })).toBeInTheDocument();
    expect(document.querySelectorAll(".live-now .live-entry")).toHaveLength(0);
    expect(screen.queryByRole("button", { name: /All activity/ })).toBeNull();
  });

  it("treats a line without a caller as unknown, not as a client's", () => {
    // An old console's line, or a snapshot from before the field existed: it is not evidence of the
    // AI working, and guessing the other way is exactly the bug this band was fixed for.
    render(<LiveNow {...base} lines={[line({})]} />);

    expect(screen.getByText("Quiet")).toBeInTheDocument();
    expect(screen.getByText("no calls in the last minute")).toBeInTheDocument();
  });
});

describe("LiveNow running calls", () => {
  it("shows a client call that is running, and ticks its elapsed time", () => {
    // The band must say it is running now, from the announcement alone, because no activity line
    // exists until the device answers. The proxy stops waiting at ninety seconds, so the line is
    // measured inside that bound; a lost clear cannot keep it past it.
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-05T12:00:00Z"));
    render(
      <LiveNow {...base} lines={[]} running={[started({ at_ms: Date.now() - 72_000 })]} />
    );

    expect(screen.getByText("running runs.exec · 1m 12s")).toBeInTheDocument();
    // The same second clock the age column uses: a frozen elapsed time would be a screenshot.
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.getByText("running runs.exec · 1m 13s")).toBeInTheDocument();
  });

  it("removes the line when the finished frame clears it", () => {
    const { rerender } = render(<LiveNow {...base} lines={[]} running={[started()]} />);
    expect(screen.getByText(/running runs\.exec/)).toBeInTheDocument();

    // The hook removed the entry when `finished` arrived; the band must not keep a phantom.
    rerender(<LiveNow {...base} lines={[]} running={[]} />);
    expect(screen.queryByText(/running runs\.exec/)).not.toBeInTheDocument();
    expect(screen.getByText("Quiet")).toBeInTheDocument();
  });

  it("does not show the console's own started call", () => {
    // The band counts the AI. A console read in flight is the same proxy, but it is not the AI's
    // work; showing it would put the page's own polling back into the one place that answers
    // "is the machine working?".
    render(
      <LiveNow
        {...base}
        lines={[]}
        running={[started({ caller: "console", tool: "terminal.list" })]}
      />
    );

    expect(screen.queryByText(/running terminal\.list/)).not.toBeInTheDocument();
    expect(screen.getByText("Quiet")).toBeInTheDocument();
  });

  it("drops a started call older than the proxy's ninety-second cut", () => {
    // A finished frame that never arrives must not leave "running" on screen forever: the page
    // applies the proxy's own cut, so the longest a phantom can live is the longest a call can.
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-05T12:00:00Z"));
    render(<LiveNow {...base} lines={[]} running={[started()]} />);
    expect(screen.getByText(/running runs\.exec/)).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(91_000);
    });
    expect(screen.queryByText(/running runs\.exec/)).not.toBeInTheDocument();
  });
});
