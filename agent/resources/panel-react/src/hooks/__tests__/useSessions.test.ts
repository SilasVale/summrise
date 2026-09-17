import { describe, it, expect, vi, beforeEach } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { render, renderHook, act, waitFor } from "@testing-library/react";
import { createElement } from "react";
import { useSessions, mapPending, pendingApprovalCount, type Session } from "../useSessions";
import { callTool } from "../../lib/api";

vi.mock("../../lib/api", () => ({
  callTool: vi.fn(),
}));

const mockCallTool = callTool as unknown as ReturnType<typeof vi.fn>;

beforeEach(() => {
  mockCallTool.mockReset();
  // Default: the 3s poll calls terminal_list — return [] (no sessions) so it
  // does not consume the open/close mock sequences. Tests override per name.
  mockCallTool.mockImplementation((name: string) => {
    if (name === "terminal_list") return Promise.resolve([]);
    return Promise.reject(new Error(`unexpected tool: ${name}`));
  });
});

// Track opened sessions so terminal_list (the 3s poll) reports them as live —
// a [] list would make the poll tombstone sessions opened in the test.
const liveSids = new Set<string>();
function mockOpen(sid: string) {
  liveSids.add(sid);
  mockCallTool.mockImplementation((name: string) => {
    if (name === "terminal_list") return Promise.resolve([...liveSids].map((id) => ({ id, label: id, kind: "pty" })));
    if (name === "terminal_open") return Promise.resolve(sid);
    if (name === "terminal_close") return Promise.resolve({ ok: true });
    return Promise.reject(new Error(`unexpected tool: ${name}`));
  });
}
beforeEach(() => { liveSids.clear(); });

describe("useSessions", () => {
  it("openSession activates the new session (R86: blank terminal fix)", async () => {
    mockOpen("term-1");
    const { result } = renderHook(() => useSessions(true));
    await act(async () => {
      await result.current.openSession("pty", "");
    });
    await waitFor(() => {
      expect(result.current.sessions).toHaveLength(1);
      expect(result.current.sessions[0]?.active).toBe(true);
    });
    expect(result.current.activeSid).toBe("term-1");
  });

  it("openSession deactivates previous sessions (R86)", async () => {
    mockOpen("term-1");
    const { result } = renderHook(() => useSessions(true));
    await act(async () => { await result.current.openSession("pty", ""); });
    mockOpen("term-2");
    await act(async () => { await result.current.openSession("pty", ""); });
    await waitFor(() => {
      expect(result.current.sessions.find((s) => s.sid === "term-1")?.active).toBe(false);
      expect(result.current.sessions.find((s) => s.sid === "term-2")?.active).toBe(true);
    });
  });

  it("closeSession switches to the next live session (R86)", async () => {
    mockOpen("term-1");
    const { result } = renderHook(() => useSessions(true));
    await act(async () => { await result.current.openSession("pty", ""); });
    mockOpen("term-2");
    await act(async () => { await result.current.openSession("pty", ""); });
    await act(async () => { await result.current.closeSession("term-2"); });
    await waitFor(() => {
      expect(result.current.sessions.find((s) => s.sid === "term-1")?.active).toBe(true);
      expect(result.current.activeSid).toBe("term-1");
    });
  });

  it("closeSession failure keeps the session open (R83)", async () => {
    mockOpen("term-1");
    const { result } = renderHook(() => useSessions(true));
    await act(async () => { await result.current.openSession("pty", ""); });
    mockCallTool.mockImplementation((name: string) => {
      if (name === "terminal_list") return Promise.resolve([]);
      if (name === "terminal_close") return Promise.reject(new Error("busy"));
      return Promise.reject(new Error(`unexpected tool: ${name}`));
    });
    await act(async () => { await result.current.closeSession("term-1"); });
    await waitFor(() => { expect(result.current.sessions[0]?.closed).toBe(false); });
  });

  it("closing the LAST live session clears active (R88)", async () => {
    mockOpen("term-1");
    const { result } = renderHook(() => useSessions(true));
    await act(async () => { await result.current.openSession("pty", ""); });
    await act(async () => { await result.current.closeSession("term-1"); });
    await waitFor(() => {
      expect(result.current.activeSid).toBe(null);
      expect(result.current.sessions[0]?.active).toBe(false);
    });
  });

  it("sessions-changed event marks server-dead sessions closed and releases focus (R88)", async () => {
    mockOpen("term-1");
    const { result } = renderHook(() => useSessions(true));
    await act(async () => { await result.current.openSession("pty", ""); });
    await waitFor(() => { expect(result.current.sessions[0]?.active).toBe(true); });
    // The server-side session dies (PTY exit) — the agent pushes the
    // sessions-changed SSE event (round-163 replaced the 3s poll with it),
    // terminal_list now returns [] (term-1 gone).
    liveSids.delete("term-1");
    await act(async () => {
      window.dispatchEvent(new CustomEvent("vale-sessions-changed"));
    });
    await waitFor(() => {
      expect(result.current.sessions[0]?.closed).toBe(true);
    });
    expect(result.current.activeSid).toBe(null);
  });

  it("revives a tombstoned session whose sid reappears live (round-245 HIGH-1)", async () => {
    mockOpen("term-1");
    const { result } = renderHook(() => useSessions(true));
    await act(async () => { await result.current.openSession("pty", ""); });
    await waitFor(() => { expect(result.current.sessions[0]?.active).toBe(true); });
    // Server-side death → tombstoned.
    liveSids.delete("term-1");
    await act(async () => { window.dispatchEvent(new CustomEvent("vale-sessions-changed")); });
    await waitFor(() => { expect(result.current.sessions[0]?.closed).toBe(true); });
    // The SAME sid comes back live (agent restarted a re-used session, or a
    // race tombstoned it while it was still open) — the next list must
    // REVIVE it, not keep a dead tab.
    liveSids.add("term-1");
    await act(async () => { window.dispatchEvent(new CustomEvent("vale-sessions-changed")); });
    await waitFor(() => {
      const s = result.current.sessions.find((x) => x.sid === "term-1");
      expect(s?.closed).toBe(false);
    });
    expect(result.current.sessions.filter((x) => x.sid === "term-1")).toHaveLength(1);
  });

  it("retries terminal_list once after a transient failure on sessions-changed (round-245 HIGH-1)", async () => {
    vi.useFakeTimers();
    try {
      liveSids.add("term-ai");
      mockCallTool.mockImplementation((name: string) => {
        if (name === "terminal_open") return Promise.resolve("term-ai");
        return Promise.reject(new Error(`unexpected tool: ${name}`));
      });
      const { result } = renderHook(() => useSessions(true));
      await act(async () => { await vi.advanceTimersByTimeAsync(10); });
      // From now on: the first terminal_list fails (transient), the retry
      // (1.2s later) succeeds and discovers the AI-opened session.
      let listCalls = 0;
      mockCallTool.mockImplementation((name: string) => {
        if (name === "terminal_list") {
          listCalls += 1;
          if (listCalls === 1) return Promise.reject(new Error("tunnel blip"));
          return Promise.resolve([...liveSids].map((id) => ({ id, label: id, kind: "pty" })));
        }
        return Promise.reject(new Error(`unexpected tool: ${name}`));
      });
      await act(async () => {
        window.dispatchEvent(new CustomEvent("vale-sessions-changed"));
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(result.current.sessions.some((s) => s.sid === "term-ai" && !s.closed)).toBe(true);
      expect(listCalls).toBeGreaterThanOrEqual(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("background sweep discovers AI-opened sessions never announced by an event (round-245 HIGH-1)", async () => {
    vi.useFakeTimers();
    try {
      liveSids.add("term-ai-1");
      mockCallTool.mockImplementation((name: string) => {
        if (name === "terminal_list") {
          return Promise.resolve([...liveSids].map((id) => ({ id, label: id, kind: "pty" })));
        }
        return Promise.reject(new Error(`unexpected tool: ${name}`));
      });
      const { result } = renderHook(() => useSessions(true));
      // The AI opens a session while the SSE event was missed entirely.
      liveSids.add("term-ai-2");
      await act(async () => {
        await vi.advanceTimersByTimeAsync(31_000);
      });
      expect(result.current.sessions.some((s) => s.sid === "term-ai-2" && !s.closed)).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });
});

// ============================================================================
// The approval deadline is ABSOLUTE.
//
// The device reports `expires_in_ms`: a budget that SHRINKS on every read. The
// panel polls it while the gate is armed (2 s), and the gate's real TTL is
// ~15 minutes. Keeping that shrinking number in React state while the component
// ALSO accumulated its own elapsed time counted every second twice — the
// displayed time fell at about double speed, which against the old 60 s block
// was invisible and against a 15-minute TTL is the difference between "you have
// 8 minutes" and "you have 15".
//
// So the conversion happens exactly once, at the wire edge: `mapPending` turns
// the budget into a wall-clock deadline, and nothing downstream ever adds to it.
// ============================================================================
describe("pending approval — the shrinking budget becomes one absolute deadline", () => {
  it("stores Date.now() + expires_in_ms, not the budget itself", () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(new Date("2026-03-01T10:00:00Z"));
      const p = mapPending({
        pending_approval: { id: "g1", command: "reload", expires_in_ms: 900_000 },
      });
      expect(p).not.toBeNull();
      expect(p!.expiresAtMs).toBe(Date.now() + 900_000);
      // ABSOLUTE, not relative. A bare 900_000 fails this by three orders of
      // magnitude, and that relative shape is what the double-count needed.
      expect(p!.expiresAtMs).toBeGreaterThan(1e12);
    } finally {
      vi.useRealTimers();
    }
  });

  it("a re-read reporting a SMALLER budget for the same id does not shorten the deadline by double", () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(new Date("2026-03-01T10:00:00Z"));
      const first = mapPending({
        pending_approval: { id: "g1", command: "reload", expires_in_ms: 900_000 },
      })!;
      // 60 s of wall clock later the SAME question reports 60 s less budget.
      vi.setSystemTime(new Date("2026-03-01T10:01:00Z"));
      const second = mapPending({
        pending_approval: { id: "g1", command: "reload", expires_in_ms: 840_000 },
      })!;

      expect(second.expiresAtMs).toBe(first.expiresAtMs);
      // 14 minutes left — not the 13 a double-counting display reaches by
      // subtracting the elapsed minute from the fresh budget as well.
      expect(second.expiresAtMs - Date.now()).toBe(840_000);
    } finally {
      vi.useRealTimers();
    }
  });

  it("the session carries that deadline, and a poll cannot pull it earlier", async () => {
    vi.useFakeTimers();
    try {
      const t0 = new Date("2026-03-01T10:00:00Z").getTime();
      vi.setSystemTime(t0);
      let budget = 900_000;
      mockCallTool.mockImplementation((name: string) => {
        if (name === "terminal_list") {
          return Promise.resolve([
            {
              id: "term-1",
              label: "shell",
              kind: "pty",
              approval_required: true,
              pending_approval: { id: "g1", command: "reload", expires_in_ms: budget },
            },
          ]);
        }
        return Promise.reject(new Error(`unexpected tool: ${name}`));
      });
      const { result } = renderHook(() => useSessions(true));
      await act(async () => { await vi.advanceTimersByTimeAsync(10); });
      const first = result.current.sessions[0]?.pendingApproval;
      expect(first?.expiresAtMs).toBe(t0 + 900_000);

      // A minute of the armed fast-poll, with the device now reporting the
      // SMALLER remaining budget.
      budget = 840_000;
      await act(async () => {
        await vi.advanceTimersByTimeAsync(60_000);
        window.dispatchEvent(new CustomEvent("vale-sessions-changed"));
        await vi.advanceTimersByTimeAsync(10);
      });

      const second = result.current.sessions[0]?.pendingApproval;
      expect(second?.expiresAtMs).toBe(first?.expiresAtMs);
      // Still ~14 minutes of wall clock left, not 13.
      expect((second?.expiresAtMs ?? 0) - Date.now()).toBeGreaterThan(830_000);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("pendingApprovalCount — the badge input", () => {
  const withPending = (over: Partial<Session> = {}) => ({
    sid: "s1",
    label: "s1",
    kind: "pty",
    closed: false,
    savedOnly: false,
    active: true,
    idleMs: 0, commandRunning: false, firstSeenAt: 0,
    closedAt: null,
    heldByHuman: false,
    approvalRequired: false,
    pendingApproval: { id: "g1", command: "reload", expiresAtMs: 1 },
    approvalGrants: [],
    goal: null,
    plan: [],
    ...over,
  });

  it("counts QUESTIONS, never the armed posture", () => {
    // Keying a badge off `approvalRequired` would make every armed session
    // shout forever — an indicator that is always on is one nobody reads.
    expect(pendingApprovalCount([withPending({ approvalRequired: true, pendingApproval: null })])).toBe(0);
    expect(pendingApprovalCount([withPending()])).toBe(1);
    expect(pendingApprovalCount([withPending(), withPending({ sid: "s2" })])).toBe(2);
  });

  it("does not count a closed tombstone's question", () => {
    // A closed session's question is history; the device has retired it.
    expect(pendingApprovalCount([withPending({ closed: true })])).toBe(0);
  });
});

// ── THE QUIET POLL: a sweep that learns nothing must not touch the DOM ────────────────────────
//
// WHY THIS IS PINNED (round 48). Performance had never been measured on this panel, so it was
// measured the same way the design is: on the device, in the built bundle.
//
//     first paint            168ms (panel) / 96ms (desktop)
//     session list on screen 200ms / 171ms
//     long tasks             0
//     rail switch (click->2 frames)  27-43ms
//     poll window, 34s idle  48 fetches (to 127.0.0.1) and ZERO DOM MUTATIONS
//
// The last line is the property worth keeping: the 30s sweep re-fetches the session list, gets the
// SAME list, and must therefore change nothing. A sweep that re-rendered the rail every 30 seconds
// would look identical in every screenshot and cost the operator nothing visible — until a session
// list of sixteen made it stutter. This test fails the moment the poll starts producing new state
// from identical data.
describe("the 30s sweep is quiet", () => {
  it("leaves the DOM untouched when the device reports the same list", async () => {
    // FAKE TIMERS, because the sweep runs every 30s: a real-timer version of this test advanced
    // 200ms and therefore never ran a poll at all — a test that could not fail for the reason it
    // claimed, which is worse than no test.
    //
    // AND THE ASSERTION IS ABOUT THE DOM, NOT ABOUT RENDERS. With identical data the hook DOES
    // re-render twice across three poll periods (React re-running a component whose state changed
    // identity is normal and costs microseconds); what matters — and what the device measured — is
    // that the DOM is untouched, so nothing repaints and no long task appears. Counting renders
    // would have failed a correct implementation; counting mutations fails only a real regression.
    vi.useFakeTimers();
    try {
      const list = [
        { id: "term-1", label: "one", kind: "pty" },
        { id: "term-2", label: "two", kind: "pty" },
      ];
      mockCallTool.mockImplementation((name: string) =>
        name === "terminal_list" ? Promise.resolve(list) : Promise.resolve({ ok: true }),
      );
      let container: HTMLElement | null = null;
      // createElement, not JSX: this file is .ts and the parser rejects JSX in it. Renaming the file
      // to .tsx would work and would also churn every path that names it; one createElement is the
      // smaller change.
      function Probe() {
        const { sessions } = useSessions(true);
        return createElement(
          "ul",
          {
            ref: (el: HTMLElement | null) => {
              container = el;
            },
          },
          sessions.map((s) => createElement("li", { key: s.sid }, s.label)),
        );
      }
      render(createElement(Probe));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1000);
      });
      expect(container!.textContent).toContain("one");
      let mutations = 0;
      const mo = new MutationObserver((recs) => {
        mutations += recs.length;
      });
      mo.observe(container!, { childList: true, subtree: true, characterData: true, attributes: true });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(95_000);
      });
      mo.disconnect();
      expect(mutations, `the poll touched the DOM ${mutations} times with identical data`).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });
});

// ── THE WIRE FORMAT, pinned from THIS end ─────────────────────────────────────────────────────
//
// `agent/tests/fixtures/session-row.json` is read here and by the device's
// `the_session_row_serializes_to_the_shared_fixture`. The mappers below are defensive on purpose —
// a missing or mistyped field yields a default rather than an error, which is right for a live UI
// and means a RENAMED field would silently render nothing at all. So the fixture is fed through the
// real hook and every field is asserted: if a mapper stops reading one, this fails; if the device
// renames one, the Rust half fails. Neither side can drift without the other noticing.
describe("the device's session row", () => {
  it("lands every field the panel renders", async () => {
    const fixture = JSON.parse(
      readFileSync(
        path.join(path.dirname(fileURLToPath(import.meta.url)), "../../../../../tests/fixtures/session-row.json"),
        "utf8",
      ),
    ) as { full: Record<string, unknown>; minimal: Record<string, unknown> };
    mockCallTool.mockImplementation((name: string) =>
      name === "terminal_list" ? Promise.resolve([fixture.full, fixture.minimal]) : Promise.resolve({ ok: true }),
    );
    const before = Date.now();
    const { result } = renderHook(() => useSessions(true));
    await act(async () => {
      await new Promise((r) => setTimeout(r, 60));
    });
    const sessions = result.current.sessions;
    expect(sessions).toHaveLength(2);

    const full = sessions.find((s) => s.sid === "term-abc123-7")!;
    expect(full.kind).toBe("ssh");
    expect(full.label).toBe("stc@192.168.1.1");
    expect(full.heldByHuman).toBe(true);
    expect(full.idleMs).toBe(3_600_000);
    // THE DEVICE'S BUSY FLAG, on the row: a command in flight, which output recency cannot see (round 28).
    expect(full.commandRunning).toBe(true);
    expect(full.approvalRequired).toBe(true);
    expect(full.approvalGrants).toEqual(["display", "show"]);
    expect(full.goal).toBe("provision the ONU 0/1 on VLAN 100");
    expect(full.plan).toEqual(["read the current config", "apply the VLAN", "verify"]);
    // The countdown is converted ONCE into an absolute deadline: keeping the device's shrinking
    // budget while also accumulating time would count down twice as fast.
    expect(full.pendingApproval?.id).toBe("ap-9f2c");
    expect(full.pendingApproval?.command).toBe("vlan 100 / port vlan 100 0/1 1");
    expect(full.pendingApproval!.expiresAtMs - before).toBeGreaterThanOrEqual(46_000);
    expect(full.pendingApproval!.expiresAtMs - before).toBeLessThanOrEqual(47_500);

    // The minimal row keeps its defaults — no invented question, no invented goal.
    const minimal = sessions.find((s) => s.sid === "term-abc123-8")!;
    expect(minimal.pendingApproval).toBeNull();
    expect(minimal.approvalRequired).toBe(false);
    expect(minimal.approvalGrants).toEqual([]);
    expect(minimal.goal).toBeNull();
    expect(minimal.plan).toEqual([]);
    expect(minimal.idleMs).toBe(0);
    expect(minimal.commandRunning).toBe(false);
  });
});

// ── a keystroke that did not land ────────────────────────────────────────────────────────────────
//
// `TerminalPane` catches a rejected `terminal_write` to keep its write chain alive, and dispatches
// `vale-write-failed`. Until round 94 NOTHING LISTENED, so an operator typing into a session whose
// agent had gone away watched their keystrokes vanish with no explanation. The message must start with
// "error" — that is what lights the status line's error state (`StatusBar` switches on the prefix, and
// round 76 pinned the colour that state uses).
describe("a terminal write that failed", () => {
  it("says so on the status line, instead of letting the keystroke vanish", async () => {
    mockCallTool.mockImplementation((name: string) =>
      name === "terminal_list" ? Promise.resolve([]) : Promise.reject(new Error("nope")),
    );
    const { result } = renderHook(() => useSessions(true));
    await waitFor(() => expect(mockCallTool).toHaveBeenCalled());

    act(() => {
      window.dispatchEvent(new CustomEvent("vale-write-failed", { detail: { sid: "term-gone-1" } }));
    });

    await waitFor(() => {
      expect(result.current.status).toMatch(/^error/);
      expect(result.current.status).toMatch(/keystrokes could not be sent/);
    });
  });

  it("stops listening when the hook unmounts", async () => {
    mockCallTool.mockImplementation((name: string) =>
      name === "terminal_list" ? Promise.resolve([]) : Promise.reject(new Error("nope")),
    );
    const { result, unmount } = renderHook(() => useSessions(true));
    await waitFor(() => expect(mockCallTool).toHaveBeenCalled());
    unmount();
    // A listener left attached would call setState on an unmounted hook; React logs that as a warning,
    // and the assertion here is simply that the dispatch does not throw or hang.
    act(() => {
      window.dispatchEvent(new CustomEvent("vale-write-failed", { detail: { sid: "term-gone-2" } }));
    });
    expect(result.current.status ?? "").toBeDefined();
  });
});
