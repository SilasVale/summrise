import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Device } from "./api";
import type { LiveLine } from "./liveActivity";
import { LiveNow } from "./LiveNow";

// The band exists to answer one question: is the AI working right now? The console's own reads — the
// terminal page's ten-second `terminal.list`, the stream's parked `terminal.read` — are lines in the
// same audit, and counting them made an idle machine read "Active now · 6 calls in the last minute".
// So the band counts a **client's** calls only; a line with no caller is not one.
function line(overrides: Partial<LiveLine> = {}): LiveLine {
  return {
    ts_ms: Date.now(),
    kind: "mcp",
    status: 200,
    sentence: "Read a file",
    ...overrides
  };
}

const devices: Device[] = [
  { id: "device-a-0123456789ab", name: "Alpha", online: true, version: "1.0.0", last_seen_ms: 1 }
];
const base = { devices, state: "live" as const, onAll: () => {} };

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
    expect(screen.getByText("Read a file")).toBeInTheDocument();
  });

  it("treats a line without a caller as unknown, not as a client's", () => {
    // An old console's line, or a snapshot from before the field existed: it is not evidence of the
    // AI working, and guessing the other way is exactly the bug this band was fixed for.
    render(<LiveNow {...base} lines={[line({})]} />);

    expect(screen.getByText("Quiet")).toBeInTheDocument();
    expect(screen.getByText("no calls in the last minute")).toBeInTheDocument();
  });
});
