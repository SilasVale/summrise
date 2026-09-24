// useDeviceRead — the invariants the module OWNS, one test each.
//
// WHY THESE AND NOT "THE IMPLEMENTATION". Thirteen modules hand-rolled this loop and
// drifted in four places, so each invariant below is a defect that was LIVE somewhere in
// the panel: a refusal folded into the value (a device rendered as empty), an unfloored
// cadence, a reply that wrote after unmount, and an older reply landing on top of a newer
// one. What looks like detail here is the drift itself.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { callApi } from "../../lib/api";
import {
  useDeviceRead,
  type DeviceRead,
  type DeviceReadOptions,
} from "../useDeviceRead";

// SPREAD THE REAL MODULE, MOCK ONLY THE TRANSPORT — the idiom every hook test here uses,
// and the one `panel-mock-spread-check` enforces: a wholesale factory would replace
// `deviceRefused` too, and the module under test would then be folding `undefined`.
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  callApi: vi.fn(),
}));

const mockCallApi = callApi as unknown as ReturnType<typeof vi.fn>;

/** A read whose settle time the TEST owns, so the order two replies land in is a choice
 *  rather than a race. */
interface Pending {
  promise: Promise<unknown>;
  resolve: (body: unknown) => void;
}
function deferred(): Pending {
  let resolve!: (body: unknown) => void;
  const promise = new Promise<unknown>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

/** Flush the microtasks a settled read needs, and let React commit what it wrote. */
const flush = () =>
  act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });

/** Settle a pending read with `body`. Test-owned, so a slow read can be held back while a
 *  second one lands — which is the only way to test the ordering guard. */
const settle = (d: Pending, body: unknown) =>
  act(async () => {
    d.resolve(body);
    await d.promise;
  });

/** Render the hook, typed by the module's own contract. */
const renderRead = <T,>(opts: DeviceReadOptions<T>) =>
  renderHook((): DeviceRead<T> => useDeviceRead<T>(opts));

/** The fold as the tests write it: it returns the body's `v` and records every call, so a
 *  test can assert that a refusal never reached it. */
const fold = () =>
  vi.fn((_previous: string, body: unknown) =>
    String((body as { v?: unknown }).v ?? ""),
  );

beforeEach(() => {
  vi.useFakeTimers();
  mockCallApi.mockReset();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("useDeviceRead", () => {
  // (a) the fold never sees a refusal, and (b) a failed read keeps the last good value
  // while reporting the read as unreadable.
  it("never folds a refusal, or a call that threw, and keeps the last good value", async () => {
    const reduce = fold();
    mockCallApi.mockResolvedValueOnce({ ok: true, v: "first" });
    const { result } = renderRead({
      path: "/api/x",
      reduce,
      initial: "start",
    });
    await flush();
    expect(result.current.data).toBe("first");
    expect(result.current.read).toBe("ok");
    expect(reduce).toHaveBeenCalledTimes(1);

    // THE DEVICE SAYS NO. `reduce` must not be handed the refusal: a device that refused
    // a read is not a device whose value is empty, and the two are only distinguishable
    // if the fold never sees the failure.
    mockCallApi.mockResolvedValueOnce({ ok: false, error: "nope" });
    await act(async () => {
      await result.current.refresh();
    });
    expect(reduce).toHaveBeenCalledTimes(1);
    expect(result.current.data).toBe("first");
    expect(result.current.read).toBe("unreadable");

    // AND A CALL THAT THREW IS THE SAME FACT. `await refresh()` also pins the interface's
    // promise that `refresh` NEVER rejects: a caller awaiting it cannot be thrown at.
    mockCallApi.mockRejectedValueOnce(new Error("HTTP 502"));
    await act(async () => {
      await result.current.refresh();
    });
    expect(reduce).toHaveBeenCalledTimes(1);
    expect(result.current.data).toBe("first");
    expect(result.current.read).toBe("unreadable");

    // A later success is still a success — the failed reads did not poison the loop.
    mockCallApi.mockResolvedValueOnce({ ok: true, v: "second" });
    await act(async () => {
      await result.current.refresh();
    });
    expect(result.current.data).toBe("second");
    expect(result.current.read).toBe("ok");
  });

  // (c) the third state, which is what stops an in-flight read rendering as "empty".
  it("is `reading` until the first read settles, then `ok`", async () => {
    const pending = deferred();
    mockCallApi.mockReturnValueOnce(pending.promise);
    const { result } = renderRead<string>({
      path: "/api/x",
      reduce: (_previous, body) => String((body as { v?: unknown }).v ?? ""),
      initial: "start",
    });
    expect(result.current.read).toBe("reading");
    expect(result.current.data).toBe("start");

    await settle(pending, { ok: true, v: "late" });
    expect(result.current.read).toBe("ok");
    expect(result.current.data).toBe("late");
  });

  // (d) one cadence rule: the floor wins, and nothing at all runs without a cadence.
  it("floors the cadence, and starts no timer when no cadence was asked for", async () => {
    mockCallApi.mockResolvedValue({ ok: true });
    const { unmount } = renderRead<number>({
      path: "/api/x",
      reduce: (previous) => previous,
      initial: 0,
      everyMs: 1_000,
      floorMs: 10_000,
    });
    await flush();
    expect(mockCallApi).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(9_999);
    });
    expect(
      mockCallApi,
      "the floor, not the caller's 1 s cadence",
    ).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(mockCallApi).toHaveBeenCalledTimes(2);
    unmount();

    // AN OMITTED CADENCE IS NOT A DEFAULT ONE: the read happens at mount, and after that
    // only when the caller asks.
    mockCallApi.mockClear();
    const noTimer = renderRead<number>({
      path: "/api/x",
      reduce: (previous) => previous,
      initial: 0,
    });
    await flush();
    expect(mockCallApi).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(600_000);
    });
    expect(mockCallApi).toHaveBeenCalledTimes(1);
    noTimer.unmount();
  });

  // (e) unmounted is not a place to write.
  it("never writes after unmount", async () => {
    const reduce = fold();
    const pending = deferred();
    mockCallApi.mockReturnValueOnce(pending.promise);
    const { result, unmount } = renderRead<string>({
      path: "/api/x",
      reduce,
      initial: "start",
    });
    unmount();

    await settle(pending, { ok: true, v: "after the card is gone" });

    // The fold is the write, and it must not have happened: a reply that arrives once the
    // surface is gone has nobody to tell, and the last frame the operator saw stands.
    expect(reduce, "a write after unmount is not a write").not.toHaveBeenCalled();
    expect(result.current.data).toBe("start");
    expect(result.current.read).toBe("reading");
  });

  // (f) only the newest read may write — the guard two of the thirteen readers had.
  it("lets only the newest read write, so a slow older reply cannot rewind the value", async () => {
    const reduce = fold();
    const slow = deferred();
    const fast = deferred();
    mockCallApi.mockReturnValueOnce(slow.promise).mockReturnValueOnce(fast.promise);
    const { result } = renderRead<string>({
      path: "/api/x",
      reduce,
      initial: "start",
    });
    // The mount read is still in flight; a refresh starts a second, newer read.
    let second!: Promise<void>;
    await act(async () => {
      second = result.current.refresh();
    });
    expect(mockCallApi).toHaveBeenCalledTimes(2);

    await settle(fast, { ok: true, v: "newest" });
    expect(result.current.data).toBe("newest");

    // The FIRST read now answers, later than the second: it is stale, and stale must not
    // land. Without the sequence guard this line is `older` — the rewind the panel's other
    // eleven readers could perform.
    await settle(slow, { ok: true, v: "older" });
    expect(result.current.data).toBe("newest");
    expect(reduce).toHaveBeenCalledTimes(1);

    await act(async () => {
      await second;
    });
  });

  // (g) refresh reads NOW, timer or no timer.
  it("reads immediately on refresh, with no interval configured", async () => {
    const reduce = fold();
    mockCallApi.mockResolvedValueOnce({ ok: true, v: "a" });
    const { result } = renderRead<string>({
      path: "/api/x",
      reduce,
      initial: "start",
    });
    await flush();
    expect(result.current.data).toBe("a");

    mockCallApi.mockResolvedValueOnce({ ok: true, v: "b" });
    await act(async () => {
      await result.current.refresh();
    });
    expect(mockCallApi).toHaveBeenCalledTimes(2);
    expect(result.current.data).toBe("b");
  });

  // (h) A FUNCTION PATH IS RESOLVED PER READ. This is the test the deleted form took with it: the
  // first version accepted `string | (() => string)` and dropped it when neither cursor reader
  // migrated, so its only consumer WAS a test. `useOperationRuns` is the production caller now, and
  // the property to pin is the one that made a string insufficient: the route must be built at
  // read time, because the cursor it carries advances between reads. A path resolved ONCE at
  // hook-call time would ask for `since_ms=0` forever.
  it("resolves a function path on every read, so a cursor advances", async () => {
    mockCallApi.mockResolvedValue({ ok: true });
    let cursor = 0;
    const { result } = renderRead<number>({
      path: () => `/api/operation?since_ms=${cursor}`,
      reduce: (previous) => previous,
      initial: 0,
      everyMs: 5_000,
    });
    await flush();
    expect(String(mockCallApi.mock.calls[0][0])).toBe(
      "/api/operation?since_ms=0",
    );

    cursor = 42;
    await act(async () => {
      await result.current.refresh();
    });
    expect(String(mockCallApi.mock.calls[1][0])).toBe(
      "/api/operation?since_ms=42",
    );

    // AND ON THE TIMER PATH TOO, which is the read the strip actually makes: the interval calls
    // the same `refresh`, so the cursor is re-read there rather than captured with the effect.
    cursor = 99;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });
    expect(String(mockCallApi.mock.calls[2][0])).toBe(
      "/api/operation?since_ms=99",
    );
  });
});
