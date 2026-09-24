// useSessionArchive — the durable list, and the two facts that must stay apart:
// "this device recorded no sessions" and "the archive could not be read".
//
// WHY THESE. The loop moved onto `useDeviceRead` in this change, and the rule that had to survive
// the move is the THROW: `archiveEntries` refuses a body it cannot read, so a malformed reply must
// leave the last good list on screen and report the read as `"unreadable"` — never fold to `[]`,
// which renders as an empty archive, a claim about the DEVICE drawn from a response the panel
// failed to understand. The other pins are the wiring the hook still owns: the two listeners, and
// the absence of a timer (answering this route folds EVERY session file on disk).
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { callApi } from "../../lib/api";
import { useSessionArchive } from "../useSessionArchive";

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  callApi: vi.fn(),
}));

const mockCallApi = callApi as unknown as ReturnType<typeof vi.fn>;

/** Let a settled read land and React commit it. */
const flush = () =>
  act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });

/** The route's manifest, as the device sends it (`ok` plus a `sessions` array). */
const manifest = (ids: string[]) => ({
  ok: true,
  sessions: ids.map((id) => ({ id })),
});

/** jsdom reports `visibilityState: "prerender"`; these tests choose the tab's state explicitly. */
const asVisibility = (state: "visible" | "hidden") =>
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => state,
  });

beforeEach(() => {
  vi.useFakeTimers();
  mockCallApi.mockReset();
});
afterEach(() => {
  vi.useRealTimers();
  delete (document as { visibilityState?: unknown }).visibilityState;
});

describe("useSessionArchive", () => {
  it("reads /api/sessions at mount, and `reading` is a state of its own", async () => {
    let resolve!: (body: unknown) => void;
    mockCallApi.mockReturnValue(
      new Promise<unknown>((res) => {
        resolve = res;
      }),
    );
    const { result } = renderHook(() => useSessionArchive());
    expect(result.current.state).toBe("reading");
    expect(result.current.entries).toEqual([]);

    await act(async () => {
      resolve(manifest(["s1", "s2"]));
      await Promise.resolve();
    });
    expect(result.current.state).toBe("ok");
    expect(result.current.entries.map((e) => e.sid)).toEqual(["s1", "s2"]);
    expect(String(mockCallApi.mock.calls[0][0])).toBe("/api/sessions");
  });

  it("an EMPTY archive is `ok` — the device answered, and it recorded nothing", async () => {
    mockCallApi.mockResolvedValue(manifest([]));
    const { result } = renderHook(() => useSessionArchive());
    await flush();
    expect(result.current.state).toBe("ok");
    expect(result.current.entries).toEqual([]);
  });

  it("a body the panel cannot read is UNREADABLE, and the last good list stays", async () => {
    // `archiveEntries` throws on this body. The throw is the point: folding it into `[]` would
    // render "this device recorded no sessions" for a response nobody understood.
    mockCallApi
      .mockResolvedValueOnce(manifest(["s1"]))
      .mockResolvedValue({ ok: true });
    const { result } = renderHook(() => useSessionArchive());
    await flush();
    expect(result.current.entries.map((e) => e.sid)).toEqual(["s1"]);

    await act(async () => {
      window.dispatchEvent(new Event("summrise-sessions-changed"));
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(mockCallApi).toHaveBeenCalledTimes(2);
    expect(result.current.state).toBe("unreadable");
    expect(
      result.current.entries.map((e) => e.sid),
      "a failed re-read must not blank the durable list it already had",
    ).toEqual(["s1"]);
  });

  it("a refusal is unreadable too, and never reaches the parser", async () => {
    mockCallApi
      .mockResolvedValueOnce(manifest(["s1"]))
      .mockResolvedValue({ ok: false, error: "sessions unavailable" });
    const { result } = renderHook(() => useSessionArchive());
    await flush();
    await act(async () => {
      window.dispatchEvent(new Event("summrise-sessions-changed"));
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(result.current.state).toBe("unreadable");
    expect(result.current.entries.map((e) => e.sid)).toEqual(["s1"]);
  });

  it("re-reads on the agent's `sessions-changed` push", async () => {
    mockCallApi
      .mockResolvedValueOnce(manifest(["s1"]))
      .mockResolvedValue(manifest(["s1", "s2"]));
    const { result } = renderHook(() => useSessionArchive());
    await flush();
    expect(result.current.entries).toHaveLength(1);

    await act(async () => {
      window.dispatchEvent(new Event("summrise-sessions-changed"));
      await Promise.resolve();
    });
    expect(mockCallApi).toHaveBeenCalledTimes(2);
    expect(result.current.entries.map((e) => e.sid)).toEqual(["s1", "s2"]);
  });

  it("re-reads when a VISIBLE tab comes back, and not while it is hidden", async () => {
    // Focus only: going hidden is not a reason to re-read every session file on the device.
    mockCallApi.mockResolvedValue(manifest(["s1"]));
    renderHook(() => useSessionArchive());
    await flush();
    expect(mockCallApi).toHaveBeenCalledTimes(1);

    asVisibility("hidden");
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
      await Promise.resolve();
    });
    expect(mockCallApi, "hidden is not a reason to re-read").toHaveBeenCalledTimes(1);

    asVisibility("visible");
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
      await Promise.resolve();
    });
    expect(mockCallApi).toHaveBeenCalledTimes(2);
  });

  it("starts NO timer: this route folds every session file on disk", async () => {
    mockCallApi.mockResolvedValue(manifest(["s1"]));
    renderHook(() => useSessionArchive());
    await flush();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(600_000);
    });
    expect(mockCallApi).toHaveBeenCalledTimes(1);
  });
});
