import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import { groupEvents, useCommandEvents } from "../useCommandEvents";
import type { CommandEvent } from "../useCommandEvents";
import { callApi } from "../../lib/api";

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  callApi: vi.fn(),
}));

const mockCallApi = callApi as unknown as ReturnType<typeof vi.fn>;

beforeEach(() => {
  mockCallApi.mockReset();
});

function start(seq: number, command: string, ts = 100): CommandEvent {
  return { seq, ts, kind: "command/start", command };
}
function output(seq: number, text: string, ts = 100): CommandEvent {
  return { seq, ts, kind: "output", text };
}
function end(
  seq: number,
  exitCode: number | null,
  reason: string,
  durationMs?: number,
  ts = 105,
): CommandEvent {
  const ev: CommandEvent = {
    seq,
    ts,
    kind: "command/end",
    exit_code: exitCode,
    reason,
  };
  if (durationMs !== undefined) ev.duration_ms = durationMs;
  return ev;
}
function status(seq: number, st: string, ts = 105): CommandEvent {
  return { seq, ts, kind: "status", status: st };
}

describe("groupEvents", () => {
  it("groups command/start → output → command/end into one card", () => {
    const cards = groupEvents([
      start(1, "ls -la"),
      output(2, "total 64\n"),
      output(3, "drwxr-xr-x\n"),
      end(4, 0, "marker", 2100),
    ]);
    expect(cards).toHaveLength(1);
    expect(cards[0]).toMatchObject({
      id: "c-1",
      command: "ls -la",
      output: "total 64\ndrwxr-xr-x\n",
      ended: true,
      exitCode: 0,
      reason: "marker",
      durationMs: 2100,
      startedAt: 100,
    });
  });

  it("splits multiple commands into cards in seq order", () => {
    const cards = groupEvents([
      start(1, "echo one"),
      end(2, 0, "marker", 10),
      start(3, "echo two"),
      output(4, "two\n"),
      end(5, 1, "marker", 20),
    ]);
    expect(cards.map((c) => c.command)).toEqual(["echo one", "echo two"]);
    expect(cards[1].exitCode).toBe(1);
    expect(cards[1].output).toBe("two\n");
  });

  it("status backgrounded ends the command (round-99/100 semantics)", () => {
    const cards = groupEvents([
      start(1, "sleep 999"),
      output(2, "started\n"),
      status(3, "backgrounded", 105),
    ]);
    expect(cards).toHaveLength(1);
    expect(cards[0].ended).toBe(true);
    expect(cards[0].exitCode).toBeNull();
    expect(cards[0].reason).toBe("backgrounded");
    // duration derived from the status ts - start ts (seconds → ms)
    expect(cards[0].durationMs).toBe(5000);
  });

  it("status exited:N carries the exit code", () => {
    const cards = groupEvents([
      start(1, "npm test"),
      status(2, "exited:2", 110),
    ]);
    expect(cards[0].ended).toBe(true);
    expect(cards[0].exitCode).toBe(2);
    expect(cards[0].reason).toBe("exited:2");
  });

  it("status closed ends the command without an exit code", () => {
    const cards = groupEvents([start(1, "ssh host"), status(2, "closed", 108)]);
    expect(cards[0].ended).toBe(true);
    expect(cards[0].exitCode).toBeNull();
    expect(cards[0].reason).toBe("closed");
  });

  it("session-level status (opened) does NOT end a command", () => {
    const cards = groupEvents([
      status(1, "opened"),
      start(2, "echo hi"),
      status(3, "opened"),
      output(4, "hi\n"),
    ]);
    expect(cards).toHaveLength(1);
    expect(cards[0].ended).toBe(false);
  });

  it("a start with no end stays live (running card)", () => {
    const cards = groupEvents([
      start(1, "tail -f /var/log/syslog"),
      output(2, "line1\n"),
      output(3, "line2\n"),
    ]);
    expect(cards).toHaveLength(1);
    expect(cards[0].ended).toBe(false);
    expect(cards[0].exitCode).toBeNull();
    expect(cards[0].output).toBe("line1\nline2\n");
  });

  it("a new start while the previous never ended closes it as interrupted", () => {
    const cards = groupEvents([
      start(1, "hang"),
      output(2, "stuck\n"),
      start(3, "next"),
      end(4, 0, "marker", 5),
    ]);
    expect(cards).toHaveLength(2);
    expect(cards[0].reason).toBe("interrupted");
    expect(cards[0].ended).toBe(true);
    expect(cards[1].command).toBe("next");
  });

  it("orphan output before any start is dropped", () => {
    const cards = groupEvents([output(1, "junk\n")]);
    expect(cards).toHaveLength(0);
  });

  it("caps the accumulated output, keeping the tail", () => {
    const big = "x".repeat(1_200_000);
    const cards = groupEvents([start(1, "dd if=/dev/zero"), output(2, big)]);
    expect(cards[0].output.length).toBeLessThan(1_100_000);
    expect(cards[0].output).toContain("truncated");
    expect(cards[0].output.endsWith("x".repeat(10))).toBe(true);
  });
});

describe("useCommandEvents", () => {
  it("polls the session audit log and groups events", async () => {
    mockCallApi.mockResolvedValue({
      ok: true,
      id: "s1",
      events: [start(1, "echo hi"), output(2, "hi\n"), end(3, 0, "marker", 5)],
    });
    const { result } = renderHook(() => useCommandEvents("s1"));
    await waitFor(() => expect(result.current.cards).toHaveLength(1));
    expect(result.current.cards[0].command).toBe("echo hi");
    expect(mockCallApi).toHaveBeenCalledWith("/api/sessions/s1");
  });

  // `found:false` IS AN ANSWER, NOT AN EMPTY ONE.
  //
  // The device used to answer `200 {events:[]}` both for "this session recorded
  // nothing" and for "there is no readable record for this id" — it collapsed
  // the two. The server now says which (SessionLogger::events_of returns the
  // flag), and the hook must carry that through rather than drawing an empty
  // trail for a session whose file is gone.
  it("an unreadable record reports 'unreadable', not an empty trail", async () => {
    mockCallApi.mockResolvedValue({
      ok: true,
      id: "gone",
      found: false,
      events: [],
    });
    const { result } = renderHook(() => useCommandEvents("gone"));
    await waitFor(() => expect(result.current.readState).toBe("unreadable"));
    expect(result.current.cards).toHaveLength(0);
  });

  it("found:true with no events is 'ok' — a quiet session is not a missing one", async () => {
    mockCallApi.mockResolvedValue({
      ok: true,
      id: "quiet",
      found: true,
      events: [],
    });
    const { result } = renderHook(() => useCommandEvents("quiet"));
    await waitFor(() => expect(result.current.readState).toBe("ok"));
  });

  it("an older agent that omits `found` is treated as readable", async () => {
    // Backward compatibility matters here: the field is new, and an agent that
    // predates it answers a perfectly good empty trail. Treating a MISSING
    // field as "unreadable" would make every older device look broken.
    mockCallApi.mockResolvedValue({ ok: true, id: "old", events: [] });
    const { result } = renderHook(() => useCommandEvents("old"));
    await waitFor(() => expect(result.current.readState).toBe("ok"));
  });

  it("switching sessions resets cards + the seq watermark", async () => {
    // sA's max seq (10) is HIGHER than sB's (2) — without the per-session
    // watermark reset, sB's first poll would be skipped as "nothing new".
    mockCallApi.mockImplementation((path: string) => {
      if (path === "/api/sessions/sA") {
        return Promise.resolve({
          ok: true,
          id: "sA",
          events: [start(9, "big seq"), end(10, 0, "marker", 5)],
        });
      }
      if (path === "/api/sessions/sB") {
        return Promise.resolve({
          ok: true,
          id: "sB",
          events: [start(1, "echo hi"), end(2, 0, "marker", 5)],
        });
      }
      return Promise.reject(new Error(`unexpected ${path}`));
    });
    const { result, rerender } = renderHook(
      ({ sid }: { sid: string }) => useCommandEvents(sid, 30),
      { initialProps: { sid: "sA" } },
    );
    await waitFor(() => expect(result.current.cards[0]?.id).toBe("c-9"));
    rerender({ sid: "sB" });
    await waitFor(() => {
      expect(result.current.cards[0]?.command).toBe("echo hi");
    });
  });

  it("does not poll while sid is null", async () => {
    const { result } = renderHook(() => useCommandEvents(null));
    await new Promise((r) => setTimeout(r, 50));
    expect(result.current.cards).toEqual([]);
    expect(mockCallApi).not.toHaveBeenCalled();
  });

  it("a failed read keeps the last good cards (no blanking)", async () => {
    // THIS TEST USED TO PROVE NOTHING. It queued `mockRejectedValueOnce` and
    // waited 80 ms for "the fast 30ms poll" to ride through it — but THERE IS NO
    // POLL (round 163 removed the timer), so the rejection was NEVER CONSUMED and
    // the assertions passed because nothing happened at all. It would have stayed
    // green with the failure path deleted.
    //
    // The re-read triggers the hook really has are the agent's `summrise-term-output`
    // event and a `visibilitychange` back to visible. This drives the SECOND of
    // those — the same path production uses — so the rejection is actually taken.
    mockCallApi.mockResolvedValue({
      ok: true,
      id: "s1",
      events: [start(1, "echo hi"), end(2, 0, "marker", 5)],
    });
    const { result } = renderHook(() => useCommandEvents("s1"));
    await waitFor(() => expect(result.current.cards).toHaveLength(1));

    mockCallApi.mockRejectedValueOnce(new Error("HTTP 502"));
    document.dispatchEvent(new Event("visibilitychange"));
    await waitFor(() => expect(mockCallApi).toHaveBeenCalledTimes(2));

    expect(result.current.cards).toHaveLength(1);
    expect(result.current.cards[0].command).toBe("echo hi");
    expect(
      result.current.readState,
      "a failure after a good read keeps the last state",
    ).toBe("ok");
  });

  // THE OTHER HALF OF THE SAME RULE, AND THE ONE THE MIGRATION HAD TO RECONSTRUCT.
  //
  // `useDeviceRead` reports `"unreadable"` for ANY failed settle, so its word cannot tell "a failure
  // with nothing in hand" from "a failure after a good read" — the two facts this reader's views are
  // built on. What the module can say is which TRANSITION it made, and this reader mirrors that: a
  // failure while nothing has settled for this subject is "unreadable", and the trail must not be
  // drawn as an empty one.
  it("a read that has NEVER succeeded is 'unreadable', not an empty trail", async () => {
    mockCallApi.mockRejectedValueOnce(new Error("HTTP 502"));
    const { result } = renderHook(() => useCommandEvents("s1"));
    await waitFor(() => expect(result.current.readState).toBe("unreadable"));
    expect(result.current.cards).toHaveLength(0);

    // AND IT RECOVERS: the next successful read is "ok" — a quiet session, not a missing one. (The
    // rejection above was consumed by the mount read; this one is the reader's own refresh path, the
    // same `visibilitychange` production uses.)
    mockCallApi.mockResolvedValue({ ok: true, id: "s1", found: true, events: [] });
    document.dispatchEvent(new Event("visibilitychange"));
    await waitFor(() => expect(result.current.readState).toBe("ok"));
  });

  // THE FACT round-138 FIXED, RE-PINNED ON THE NEW SEAM. A slow read for the OLD session used to
  // resolve after the switch and land the old session's events under the new tab — and, because the
  // reply carried a HIGHER seq (sA's log is longer), poison the new subject's watermark so every
  // later poll of it looked like "nothing new". The check is no longer a re-check this hook makes
  // after its `await`: it is the module's ordering guard plus the `resetKey` reset that abandons an
  // in-flight read on a switch. This asserts the OUTCOME, which is what the bug was.
  it("drops a reply that belongs to the session the operator switched away from", async () => {
    let resolveOld!: (body: unknown) => void;
    const oldRead = new Promise((r) => {
      resolveOld = r;
    });
    mockCallApi.mockImplementation((path: string) => {
      if (path === "/api/sessions/sA") return oldRead;
      if (path === "/api/sessions/sB") {
        return Promise.resolve({
          ok: true,
          id: "sB",
          events: [start(1, "new session"), end(2, 0, "marker", 5)],
        });
      }
      return Promise.reject(new Error(`unexpected ${path}`));
    });
    const { result, rerender } = renderHook(
      ({ sid }: { sid: string }) => useCommandEvents(sid),
      { initialProps: { sid: "sA" } },
    );

    // sA's read is still in flight when the operator switches to sB.
    rerender({ sid: "sB" });
    await waitFor(() => expect(result.current.cards[0]?.command).toBe("new session"));

    // THE OLD SESSION ANSWERS NOW. Its events belong to a subject nobody is looking at any more.
    await act(async () => {
      resolveOld({
        ok: true,
        id: "sA",
        events: [start(90, "old session"), end(91, 0, "marker", 5)],
      });
      await oldRead;
    });
    expect(result.current.cards.map((c) => c.command)).toEqual(["new session"]);
  });

  // AND THE FLAG THAT CARRIES THAT RULE IS PER SUBJECT. sA reads fine; the operator switches to sB,
  // whose first read fails. If "a read of this session has succeeded" survived the switch, sB would
  // be reported "ok" with no events — an empty trail drawn for a session whose file could not be
  // read at all, which is the defect `lib/trailRead.ts` exists to prevent, on the switch where it is
  // most likely. This is the rule the module's own `"reading"` transition carries (it is the state a
  // `resetKey` change puts the read back to).
  it("does not inherit the previous session's read state across a switch", async () => {
    mockCallApi.mockImplementation((path: string) => {
      if (path === "/api/sessions/sA") {
        return Promise.resolve({
          ok: true,
          id: "sA",
          found: true,
          events: [start(1, "ls"), end(2, 0, "marker", 5)],
        });
      }
      if (path === "/api/sessions/sB") return Promise.reject(new Error("HTTP 502"));
      return Promise.reject(new Error(`unexpected ${path}`));
    });
    const { result, rerender } = renderHook(
      ({ sid }: { sid: string }) => useCommandEvents(sid),
      { initialProps: { sid: "sA" } },
    );
    await waitFor(() => expect(result.current.readState).toBe("ok"));

    rerender({ sid: "sB" });
    await waitFor(() => expect(result.current.readState).toBe("unreadable"));
    expect(result.current.cards).toHaveLength(0);
  });
});
