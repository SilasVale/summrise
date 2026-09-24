// TerminalPane pins — xterm mount/wiring plus the React-owned overlays:
// registerWrite registration, font zoom (persist + clamp), search bar
// open/close, inactive hiding, adopt read on mount.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { TerminalPane } from "../TerminalPane";
import { callTool } from "../../lib/api";

vi.mock("../../lib/api", () => ({
  callTool: vi.fn(async () => ({})),
}));

const session = (over: Record<string, unknown> = {}) => ({
  sid: "s1",
  label: "s1",
  kind: "pty",
  closed: false,
  savedOnly: false,
  active: true,
  idleMs: 0,
  commandRunning: false,
  firstSeenAt: 0,
  closedAt: null,
  heldByHuman: false,
  approvalRequired: false,
  pendingApproval: null,
  approvalGrants: [],
  goal: null,
  plan: [],
  ...over,
});

const registerWrite = vi.fn(() => () => {});

beforeEach(() => {
  vi.mocked(callTool).mockClear();
  registerWrite.mockClear();
  localStorage.clear();
});

describe("TerminalPane", () => {
  it("mounts xterm, registers the write callback, adopts history", async () => {
    const { container } = render(
      <TerminalPane session={session()} registerWrite={registerWrite} />,
    );
    expect(container.querySelector(".term-host")).toBeTruthy();
    expect(container.querySelector(".term-session.active")).toBeTruthy();
    expect(registerWrite).toHaveBeenCalledWith(
      "s1",
      expect.any(Function),
      expect.any(Function),
      expect.any(Function),
    );
    await waitFor(() =>
      expect(callTool).toHaveBeenCalledWith(
        "terminal_read",
        expect.objectContaining({ session_id: "s1" }),
      ),
    );
  });

  // THE ADOPT CHAIN STOPS WHEN THE PANE DOES (2026-09-24, the panel exploration). The cleanup disposes
  // the terminal, nulls the refs and cancels the pump, but it left this chain running: a read already
  // in flight resolves anyway, its bytes are written into a disposed xterm (the pump swallows them)
  // and — because the server still says more exists — the chain recurses for up to 64 more 1 MiB pages
  // with nobody left to read them. Every page below says "more exists"; the pane unmounts after the
  // first response; no further read may start.
  it("stops paging history once the pane unmounts", async () => {
    let reads = 0;
    vi.mocked(callTool).mockImplementation(
      async (name: string, params: unknown) => {
        if (name !== "terminal_read") return {};
        reads += 1;
        // A page that takes a macrotask, so the chain is still RUNNING when we unmount.
        await new Promise((r) => setTimeout(r, 5));
        const start = Number((params as { offset?: number })?.offset) || 0;
        // AND IT BEGINS WHERE WE ASKED. A stub that always answers `start: 0` makes the next read look
        // like it advanced nothing, so the chain stops on its own — which is how the first version of
        // this test passed with the guard removed. Each page here advances 100 bytes from the requested
        // offset and reports an `end` far ahead: a real server's answer, and a chain with every reason
        // to continue.
        return { start, text: "x".repeat(100), raw: "", end: 10_000_000 };
      },
    );
    const { unmount } = render(
      <TerminalPane session={session()} registerWrite={registerWrite} />,
    );
    await waitFor(() => expect(reads).toBeGreaterThanOrEqual(2));
    unmount();
    const atUnmount = reads;
    // Ten pages of room: without the liveness guard the chain fetches another ten here.
    await new Promise((r) => setTimeout(r, 50));
    expect(reads).toBe(atUnmount);
  });

  it("write callback reaches the terminal", async () => {
    const { container } = render(
      <TerminalPane session={session()} registerWrite={registerWrite} />,
    );
    const cb = (registerWrite.mock.calls[0] as unknown[])[1] as (
      bytes: Uint8Array,
    ) => void;
    cb(new TextEncoder().encode("hello-pane"));
    await waitFor(() =>
      expect(container.querySelector(".term-host")!.textContent).toContain(
        "hello-pane",
      ),
    );
  });

  it("font zoom persists and clamps to min/max", async () => {
    render(<TerminalPane session={session()} registerWrite={registerWrite} />);
    const smaller = screen.getByTitle("Smaller font");
    for (let i = 0; i < 10; i++) fireEvent.click(smaller);
    expect(localStorage.getItem("summriseFontSize")).toBe("9");
    const larger = screen.getByTitle("Larger font");
    for (let i = 0; i < 20; i++) fireEvent.click(larger);
    expect(localStorage.getItem("summriseFontSize")).toBe("22");
    fireEvent.click(screen.getByTitle("Reset font size"));
    expect(localStorage.getItem("summriseFontSize")).toBe("13");
  });

  it("search bar opens via button and closes via Esc", async () => {
    render(<TerminalPane session={session()} registerWrite={registerWrite} />);
    expect(screen.queryByPlaceholderText("Search…")).toBeNull();
    // BOTH the label and the tooltip: a title-only name is the last resort in the accessible-name
    // computation and is not exposed on touch at all (measured by the name sweep, round 49).
    expect(
      screen.getByLabelText("Search scrollback (Ctrl+F)"),
      "the button must carry a name",
    ).toBeTruthy();
    fireEvent.click(screen.getByTitle("Search scrollback (Ctrl+F)"));
    const input = await screen.findByPlaceholderText("Search…");
    fireEvent.keyDown(input, { key: "Escape" });
    expect(screen.queryByPlaceholderText("Search…")).toBeNull();
  });

  it("inactive session hides and drops the overlays", () => {
    const { container } = render(
      <TerminalPane
        session={session({ active: false })}
        registerWrite={registerWrite}
      />,
    );
    expect(
      container.querySelector(".term-session")!.getAttribute("style"),
    ).toContain("none");
    expect(screen.queryByTitle("Smaller font")).toBeNull();
  });
});
// The scrollback search button carried its name in `title` alone — the last resort in the
// accessible-name computation and not exposed on touch (measured by the name sweep, round 49).
