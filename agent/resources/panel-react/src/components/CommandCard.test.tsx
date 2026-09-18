// A RUNNING COMMAND'S DURATION MUST ADVANCE ON ITS OWN (round 62).
//
// The card computes `Date.now() - startedAt` during render, so the number only moves when something re-renders the
// card — and what re-renders it is an SSE event carrying new OUTPUT. A silent command (a flash, a probe, a serial
// write that prints one final line) produces none, so the elapsed time freezes at whatever the last event said. The
// ledger already names that case in `sessionActive`'s own comment: "a long command that prints nothing for a while".
//
// Measured here before it was fixed: the text was identical five seconds later.
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, act } from "@testing-library/react";
import { CommandCard } from "./CommandCard";
import type { CommandCard as CardData } from "../hooks/useCommandEvents";

const running = (over: Partial<CardData> = {}): CardData => ({
  id: "c-1",
  seq: 1,
  command: "flash --silent",
  output: "",
  startedAt: Math.floor(Date.now() / 1000),
  ended: false,
  exitCode: null,
  reason: null,
  durationMs: null,
  ...over,
});

afterEach(() => {
  vi.useRealTimers();
});

describe("a running command's duration", () => {
  it("advances while the command is still running, with no new output to trigger a render", () => {
    vi.useFakeTimers();
    render(<CommandCard card={running()} selected={false} onSelect={() => {}} />);
    const first = screen.getByText(/^\d+(\.\d+)?s$|^\d+ms$/).textContent;
    act(() => {
      vi.advanceTimersByTime(5000);
    });
    const second = screen.getByText(/^\d+(\.\d+)?s$|^\d+ms$/).textContent;
    expect(second).not.toBe(first);
  });

  it("stops moving once the command has ended, so an ended card is not repainting every second", () => {
    vi.useFakeTimers();
    render(
      <CommandCard
        card={running({ ended: true, exitCode: 0, durationMs: 2500 })}
        selected={false}
        onSelect={() => {}}
      />,
    );
    const first = screen.getByText(/^\d+(\.\d+)?s$|^\d+ms$/).textContent;
    act(() => {
      vi.advanceTimersByTime(5000);
    });
    expect(screen.getByText(/^\d+(\.\d+)?s$|^\d+ms$/).textContent).toBe(first);
  });
});