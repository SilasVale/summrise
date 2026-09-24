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
    // A SUCCESS HAS NO REASON — the invariant that lets a caller print `reason` unconditionally:
    // a good value is never shown beside a sentence about a failure that is over.
    expect(result.current.reason).toBe("");
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
    expect(result.current.reason).toBe("nope");

    // AND A CALL THAT THREW IS THE SAME FACT. `await refresh()` also pins the interface's
    // promise that `refresh` NEVER rejects: a caller awaiting it cannot be thrown at.
    mockCallApi.mockRejectedValueOnce(new Error("HTTP 502"));
    await act(async () => {
      await result.current.refresh();
    });
    expect(reduce).toHaveBeenCalledTimes(1);
    expect(result.current.data).toBe("first");
    expect(result.current.read).toBe("unreadable");
    expect(result.current.reason).toBe("HTTP 502");

    // A later success is still a success — the failed reads did not poison the loop.
    mockCallApi.mockResolvedValueOnce({ ok: true, v: "second" });
    await act(async () => {
      await result.current.refresh();
    });
    expect(result.current.data).toBe("second");
    expect(result.current.read).toBe("ok");
    expect(
      result.current.reason,
      "and the success cleared the failed reads' words on the way in",
    ).toBe("");
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

  // (i) A CHANGED SUBJECT IS A DIFFERENT VALUE, AND THE SWITCH IS SETTLED IN THE FRAME IT HAPPENS.
  // Two facts share the one mechanism: the value must not describe the OLD subject for even one
  // frame (a session switch that draws the previous session's trail as the new one's — the defect
  // `trailRead.ts` records as reachable on EVERY switch), and a reply that belongs to the old
  // subject must never land under the new one. Added with `useCommandEvents`, the caller that needs
  // it; the option is `resetKey`.
  it("resets to `initial`/`reading` on a resetKey change, and the old subject's reply never lands", async () => {
    const reduce = fold();
    const staleRead = deferred();
    const newSubject = deferred();
    mockCallApi
      // The mount read of subject `a` settles at once, so the value ON SCREEN is `a`'s — which is
      // what makes the switch below observable at all.
      .mockResolvedValueOnce({ ok: true, v: "old" })
      .mockReturnValueOnce(staleRead.promise)
      .mockReturnValueOnce(newSubject.promise);
    const { result, rerender } = renderHook(
      ({ subject }: { subject: string }) =>
        useDeviceRead<string>({
          // A FUNCTION path, because this is how the caller that needs `resetKey` builds its route:
          // `/api/sessions/${sid}`, resolved at read time.
          path: () => `/api/${subject}`,
          reduce,
          initial: "none",
          resetKey: subject,
        }),
      { initialProps: { subject: "a" } },
    );
    await flush();
    expect(result.current.data).toBe("old");
    expect(result.current.read).toBe("ok");

    // A SECOND READ OF SUBJECT `a` IS IN FLIGHT WHEN THE SUBJECT CHANGES — the shape a session
    // switch has (a poll that has not answered yet).
    let stale!: Promise<void>;
    await act(async () => {
      stale = result.current.refresh();
    });
    expect(String(mockCallApi.mock.calls[1][0])).toBe("/api/a");

    rerender({ subject: "b" });
    expect(
      result.current.data,
      "back to `initial` in the frame of the switch, not one frame later",
    ).toBe("none");
    expect(result.current.read).toBe("reading");
    // AND THE NEW SUBJECT IS READ AT ONCE, not at the next tick: a caller that switched would
    // otherwise show an empty surface for up to a full cadence.
    expect(String(mockCallApi.mock.calls[2][0])).toBe("/api/b");

    // THE OLD SUBJECT ANSWERS LAST. It is stale — not merely late — and stale must not write: this
    // value belongs to a subject nobody is looking at any more.
    await settle(staleRead, { ok: true, v: "stale" });
    await act(async () => {
      await stale;
    });
    expect(result.current.data).toBe("none");
    expect(reduce, "a reply for the abandoned subject is not a read").toHaveBeenCalledTimes(1);

    // The new subject's own reply is the one that writes.
    await settle(newSubject, { ok: true, v: "new" });
    expect(result.current.data).toBe("new");
    expect(result.current.read).toBe("ok");
    expect(reduce).toHaveBeenCalledTimes(2);
  });

  // (j) A READ WITH NO SUBJECT YET IS NOT A SLOW READ. `enabled:false` is how a caller says "there is
  // nothing to read", so it must do NOTHING — and `refresh()` must be a no-op that still resolves,
  // because a caller's listener calls it without checking first. Added with the same caller.
  it("reads nothing while disabled — no mount read, no timer, a no-op refresh — and reads once enabled", async () => {
    mockCallApi.mockResolvedValue({ ok: true, v: "on" });
    const { result, rerender } = renderHook(
      ({ live }: { live: boolean }) =>
        useDeviceRead<string>({
          path: "/api/x",
          reduce: (_previous, body) => String((body as { v?: unknown }).v ?? ""),
          initial: "start",
          everyMs: 5_000,
          enabled: live,
        }),
      { initialProps: { live: false } },
    );
    await flush();
    expect(mockCallApi, "a disabled read does not read at mount").not.toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(mockCallApi, "and starts no timer either").not.toHaveBeenCalled();

    // `refresh()` IS NOT A BACK DOOR: it resolves, and it reads nothing.
    await act(async () => {
      await result.current.refresh();
    });
    expect(mockCallApi).not.toHaveBeenCalled();
    expect(result.current.data).toBe("start");
    expect(result.current.read).toBe("reading");

    // TURNING IT ON IS THE READ THE CALLER ASKED FOR — at once, and then on the cadence.
    rerender({ live: true });
    await flush();
    expect(mockCallApi).toHaveBeenCalledTimes(1);
    expect(result.current.data).toBe("on");
    expect(result.current.read).toBe("ok");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });
    expect(mockCallApi).toHaveBeenCalledTimes(2);
  });
});

// `reason` — THE WORDS THE MODULE CAUGHT AND USED TO THROW AWAY. Three different failures landed in
// `setRead("unreadable")` and the sentence each one carried was dropped, which cost a caller: `usePlugins`
// rendered the DEVICE's own words for a failed status read and, after the migration, could only print
// "status poll failed" — so it kept a ref whose only job was to re-print the fold's message one layer up.
// These cases are that sentence, one per source, plus the two ways it is cleared.
describe("useDeviceRead — the reason a settle failed", () => {
  it("reports the DEVICE's own error for a refusal, which is the text written for a person", async () => {
    const reduce = fold();
    mockCallApi.mockResolvedValueOnce({ ok: true, v: "kept" });
    const { result } = renderRead<string>({
      path: "/api/x",
      reduce,
      initial: "start",
    });
    await flush();
    expect(result.current.read).toBe("ok");
    expect(result.current.reason, "no failure, no words").toBe("");

    // The device's words are handed on UNCHANGED — not summarised, not prefixed: this is the half a
    // caller could never recover, because a refusal stops at the module and never reaches its fold.
    mockCallApi.mockResolvedValueOnce({
      ok: false,
      error: "playwright is not installed",
      code: "E_PLUGIN",
    });
    await act(async () => {
      await result.current.refresh();
    });
    expect(result.current.read).toBe("unreadable");
    expect(result.current.reason).toBe("playwright is not installed");
    expect(
      result.current.data,
      "keep-last-on-failure is untouched by the reason riding along",
    ).toBe("kept");
    expect(reduce).toHaveBeenCalledTimes(1);
  });

  it("reports NOTHING for a refusal that carries no error, rather than inventing a sentence", async () => {
    // THE MODULE DOES NOT KNOW WHAT THE CALLER WANTS TO SAY, so a refusal with no `error` — and a body
    // that is not even an object (the proxy's error page `callApi` hands back as raw text) — gets `""`,
    // which is the caller's cue to use its own sentence. An invented reason beside a real failure is
    // worse than none: it would be the module's words dressed as the device's.
    mockCallApi.mockResolvedValueOnce({ ok: false, code: "E_REFUSED" });
    const { result } = renderRead<string>({
      path: "/api/x",
      reduce: fold(),
      initial: "start",
    });
    await flush();
    expect(result.current.read).toBe("unreadable");
    expect(result.current.reason).toBe("");

    mockCallApi.mockResolvedValueOnce("not json");
    await act(async () => {
      await result.current.refresh();
    });
    expect(result.current.read).toBe("unreadable");
    expect(result.current.reason).toBe("");
  });

  it("reports a thrown Error's message, which is the transport's own words", async () => {
    // `callApi` throws `Error("HTTP 502")`, `Error("unauthorized")`, a timeout — the message IS the
    // diagnosis, and it used to stop at the module's catch.
    mockCallApi.mockRejectedValueOnce(new Error("HTTP 502"));
    const { result } = renderRead<string>({
      path: "/api/x",
      reduce: fold(),
      initial: "start",
    });
    await flush();
    expect(result.current.read).toBe("unreadable");
    expect(result.current.reason).toBe("HTTP 502");
  });

  it("stringifies a thrown non-Error, and reports nothing when the throw carried no words", async () => {
    mockCallApi.mockRejectedValueOnce("the socket went away");
    const { result } = renderRead<string>({
      path: "/api/x",
      reduce: fold(),
      initial: "start",
    });
    await flush();
    expect(result.current.reason).toBe("the socket went away");

    // A REJECTION WITH NO VALUE HAS NO MESSAGE, and this is deliberately not `String(e)`: that would
    // print the literal words "undefined"/"null" as though the failure had sent them.
    mockCallApi.mockRejectedValueOnce(undefined);
    await act(async () => {
      await result.current.refresh();
    });
    expect(result.current.read).toBe("unreadable");
    expect(result.current.reason).toBe("");
  });

  it("carries the FOLD's own message out, so the caller need not re-print it one layer up", async () => {
    // The third source, and the one `usePlugins` kept a ref for: a body the device SENT that the caller's
    // own fold cannot use. The message is already the caller's diagnosis — the module only has to keep it.
    const reduce = vi.fn((): string => {
      throw new Error("the spec route answered without a plugins list");
    });
    mockCallApi.mockResolvedValueOnce({ ok: true });
    const { result } = renderRead<string>({
      path: "/api/x",
      reduce,
      initial: "start",
    });
    await flush();
    expect(result.current.read).toBe("unreadable");
    expect(result.current.reason).toBe(
      "the spec route answered without a plugins list",
    );
    expect(result.current.data, "a fold that refused writes no value").toBe(
      "start",
    );
  });

  it("clears the reason on the next success, so a good value never sits beside a stale sentence", async () => {
    // THE INVARIANT THAT MAKES `reason` SAFE TO READ UNCONDITIONALLY, and the reason a caller may print
    // it without asking `read` first: whenever the read is `"ok"`, the reason is `""`.
    mockCallApi.mockResolvedValueOnce({ ok: false, error: "nope" });
    const { result } = renderRead<string>({
      path: "/api/x",
      reduce: fold(),
      initial: "start",
    });
    await flush();
    expect(result.current.reason).toBe("nope");

    mockCallApi.mockResolvedValueOnce({ ok: true, v: "good" });
    await act(async () => {
      await result.current.refresh();
    });
    expect(result.current.read).toBe("ok");
    expect(result.current.reason).toBe("");
    expect(result.current.data).toBe("good");
  });

  it("clears the reason when the SUBJECT changes, because the new subject has no failure yet", async () => {
    mockCallApi
      // Subject `a` fails, so the sentence is ON SCREEN when the switch happens.
      .mockResolvedValueOnce({ ok: false, error: "session a blew up" })
      // The new subject's own read never settles: the frame of the switch is what this case is about.
      .mockReturnValueOnce(new Promise(() => {}));
    const { result, rerender } = renderHook(
      ({ sid }: { sid: string }) =>
        useDeviceRead<string>({
          path: () => `/api/${sid}`,
          reduce: fold(),
          initial: "none",
          resetKey: sid,
        }),
      { initialProps: { sid: "a" } },
    );
    await flush();
    expect(result.current.read).toBe("unreadable");
    expect(result.current.reason).toBe("session a blew up");

    rerender({ sid: "b" });
    expect(result.current.read).toBe("reading");
    expect(
      result.current.reason,
      "the old subject's sentence is not about the new subject",
    ).toBe("");
  });
});
