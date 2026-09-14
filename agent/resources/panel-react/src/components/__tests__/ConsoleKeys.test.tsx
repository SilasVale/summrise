// The console key BAR: what it sends, and what it says when a send fails.
//
// The bar is small, and every way it can be wrong is invisible to the operator: a key that
// calls the wrong tool, a break offered on a session without a serial line, or a failure that
// is swallowed (an operator waiting for a bootloader that was never interrupted).
import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { ConsoleKeys } from "../ConsoleKeys";
import { callTool } from "../../lib/api";

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  callTool: vi.fn(),
}));
const mockCallTool = callTool as unknown as ReturnType<typeof vi.fn>;

describe("ConsoleKeys", () => {
  beforeEach(() => mockCallTool.mockReset().mockResolvedValue("OK"));

  it("sends Ctrl+C as the byte it is, through terminal_write", async () => {
    render(<ConsoleKeys sessionId="sess-1" kind="pty" />);
    fireEvent.click(screen.getByTitle(/Ctrl\+C/));
    await waitFor(() => expect(mockCallTool).toHaveBeenCalledTimes(1));
    expect(mockCallTool).toHaveBeenCalledWith("terminal_write", {
      session_id: "sess-1",
      data_base64: "Aw==",
    });
  });

  it("offers a BREAK on a serial session and asks for it as a break, not a write", async () => {
    render(<ConsoleKeys sessionId="sess-2" kind="serial" />);
    const brk = screen.getByTitle(/Serial BREAK/);
    fireEvent.click(brk);
    await waitFor(() => expect(mockCallTool).toHaveBeenCalledTimes(1));
    expect(mockCallTool).toHaveBeenCalledWith("terminal_write", {
      session_id: "sess-2",
      break_ms: 250,
    });
  });

  it("does NOT offer a break on a session that has no serial line", () => {
    render(<ConsoleKeys sessionId="sess-3" kind="ssh" />);
    expect(screen.queryByTitle(/Serial BREAK/)).toBeNull();
    // The interrupts are still there — that is the bar's reason to exist.
    expect(screen.getByTitle(/Ctrl\+C/)).toBeTruthy();
  });

  it("announces a failed send instead of swallowing it", async () => {
    mockCallTool.mockRejectedValueOnce(new Error("session lost"));
    const heard: string[] = [];
    const onFail = (e: Event) => heard.push((e as CustomEvent).detail.key);
    window.addEventListener("vale-write-failed", onFail);
    render(<ConsoleKeys sessionId="sess-4" kind="serial" />);
    fireEvent.click(screen.getByTitle(/Serial BREAK/));
    await waitFor(() => expect(heard).toEqual(["break"]));
    window.removeEventListener("vale-write-failed", onFail);
  });

  it("is a toolbar an operator can find by name", () => {
    render(<ConsoleKeys sessionId="sess-5" kind="serial" />);
    expect(screen.getByRole("toolbar", { name: "Console keys" })).toBeTruthy();
  });
});
