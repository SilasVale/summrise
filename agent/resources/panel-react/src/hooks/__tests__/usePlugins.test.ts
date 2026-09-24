// Coverage audit row 18: usePlugins had no test file. Exercises the plugin
// status poll contract: inactive → no fetch + empty rows; active → fetches
// /api/spec + /api/plugins/status.
//
// WHY THESE, AFTER THE READS MOVED ONTO `useDeviceRead`. Both routes are the module's now (the mount
// read, the refusal guard, keep-last-on-failure), so what this file pins is every rule the hook kept
// at its own end: the `active` gate (the read must not dial a device this panel is not connected to),
// the `specLoaded` gate (the registry is asked for until it loads, then never again), and the two
// sentences — byte for byte, INCLUDING the words the failure carried, which the module hands back as
// `reason`. Round 4 could not: every failed read was one word, so a refused status read had to print the
// message-less sentence, and the hook kept a ref whose only job was to re-print the fold's own message
// one layer up. The ref is gone and those words are the module's; the refusal assertions below are the
// ones this hook produced before round 4 (`status: <the device's own words>`).
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { callApi } from "../../lib/api";
import { usePlugins } from "../usePlugins";

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

/** The routes dialled so far, in order — the two reads are two facts, so the pin is WHICH was asked. */
const routes = () => mockCallApi.mock.calls.map((c) => String(c[0]));
const callsTo = (route: string) => routes().filter((r) => r === route).length;

/** `/api/plugins/status`, as the device sends it (`ok` plus the live state the card draws). */
const status = (running: boolean) => ({
  ok: true,
  playwright: { running, port: 1 },
});

/** `/api/spec`, as the device sends it. */
const spec = () => ({
  ok: true,
  plugins: [{ name: "playwright", displayName: "Playwright", description: "" }],
});

beforeEach(() => {
  localStorage.clear();
  vi.useFakeTimers();
  mockCallApi.mockReset();
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("usePlugins", () => {
  it("does not read while not active and returns empty rows", async () => {
    const { result } = renderHook(() => usePlugins(false));
    expect(result.current.rows).toEqual([]);
    // `useDeviceRead` reads at mount — so the hook hands it `enabled: active`, and this is the
    // assertion that notices if that gate goes: no call, and no failure claimed for a read nobody
    // made (the read states are `"reading"`, which is not a sentence).
    await flush();
    expect(mockCallApi).not.toHaveBeenCalled();
    expect(result.current.loadError).toBe("");
  });

  it("reads both routes when the panel connects, having read nothing while it was off", async () => {
    mockCallApi.mockImplementation(async (path: string) =>
      String(path).includes("/api/spec") ? spec() : status(true),
    );
    const { result, rerender } = renderHook(({ on }) => usePlugins(on), {
      initialProps: { on: false },
    });
    await flush();
    expect(mockCallApi, "an inactive hook dials nothing").not.toHaveBeenCalled();

    // A RECONNECT IS NOT A MOUNT, and the module's `enabled` is what reads for it: turning the read
    // back on re-arms the module's read effect, and this hook's effect only wires the events. Without
    // that read the page would keep whatever the previous connection last said — there is no poll to
    // correct it (round 163).
    rerender({ on: true });
    await flush();
    expect(routes().sort()).toEqual(["/api/plugins/status", "/api/spec"]);
    expect(result.current.specLoaded).toBe(true);
    expect(result.current.rows.map((r) => r.name)).toEqual(["playwright"]);
  });
});

// A FAILED SPEC READ USED TO LEAVE THE INVENTORY SAYING "Loading inventory…"
// FOR EVER. The catch was `catch { /* transient — retry next tick */ }` and set
// nothing, while the SAME hook's status fetch sets `loadError` — the twin rule
// applied to one branch and not the other, inside one function. There is no tick
// to retry on: the 5 s poll was removed in round 163 (the file says so itself,
// four lines below the comment that promised the retry), and `specLoaded` is the
// only thing that re-arms the fetch. So `specLoaded` stayed false, no error was
// reported, and the page's `!specLoaded` branch rendered a claim of progress
// that had stopped.
describe("usePlugins — a failed inventory read", () => {
  it("reports the failure instead of loading for ever", async () => {
    // A 200 the panel cannot use — the shape that reached the silent catch. `callApi` hands a
    // non-JSON body back as the raw TEXT (`lib/api.ts`), and the module refuses that before any fold
    // runs: a body that is not an object with `ok: true` is a refusal, and a refusal is not "no
    // plugins" either. The hook's own throw is the `{ok: true}` case below.
    mockCallApi.mockImplementation(async (path: string) =>
      String(path).includes("/api/spec") ? "not json" : status(true),
    );

    const { result } = renderHook(() => usePlugins(true));
    await flush();
    expect(mockCallApi, "the hook must actually dial").toHaveBeenCalled();
    expect(result.current.loadError, "the failure must be REPORTED").not.toBe("");
    expect(result.current.specLoaded, "and the inventory must not claim it loaded").toBe(false);
    expect(result.current.loadError).toMatch(/inventory/i);
  });

  it("reports a body the device ANSWERED with, in the fold's own words", async () => {
    // The other half of the rule, and the one the module cannot reach: a body the device SENT (so it
    // is folded — a refusal never reaches `reduce`) that carries no `plugins` array is the failure the
    // hook raises itself. It is reported as `"unreadable"` by the module, and the sentence is the one
    // the hand-written `throw` produced, byte for byte.
    mockCallApi.mockImplementation(async (path: string) =>
      String(path).includes("/api/spec") ? { ok: true } : status(true),
    );

    const { result } = renderHook(() => usePlugins(true));
    await flush();
    expect(result.current.specLoaded).toBe(false);
    expect(result.current.loadError).toBe(
      "inventory: the spec route answered without a plugins list",
    );
    // AND THE SECOND READ IS STILL ITS OWN: the status answered, so the card has its live state while
    // the inventory reports its failure — the single-error-cell shape this hook was fixed out of.
    expect(result.current.playwright?.running).toBe(true);
  });

  it("shows the DEVICE's own words for a refused inventory read, in the same sentence shape", async () => {
    // The same arm on the spec side: a refusal's `error` is the device's own text, and it stops at the
    // module — the fold never sees it, so this sentence can only come from `reason`. The `inventory: `
    // prefix is the shape the hand-written throw produced for the fold's own message.
    mockCallApi.mockImplementation(async (path: string) =>
      String(path).includes("/api/spec")
        ? { ok: false, error: "the registry is still starting" }
        : status(true),
    );

    const { result } = renderHook(() => usePlugins(true));
    await flush();
    expect(result.current.specLoaded).toBe(false);
    expect(result.current.loadError).toBe(
      "inventory: the registry is still starting",
    );
  });

  it("retries the inventory while it has never loaded, and stops once it has", async () => {
    let specCalls = 0;
    mockCallApi.mockImplementation(async (path: string) => {
      if (String(path).includes("/api/spec")) {
        specCalls += 1;
        // The first body is one the fold refuses; the second is the registry.
        return specCalls === 1 ? { ok: true } : spec();
      }
      return status(true);
    });

    const { result } = renderHook(() => usePlugins(true));
    await flush();
    expect(result.current.specLoaded).toBe(false);
    expect(specCalls).toBe(1);

    // A refresh asks AGAIN: `"unreadable"` is not `"ok"`, which is the whole of "read until it
    // succeeds" now that there is no poll to retry on. The `playwright-changed` push is one of the
    // refreshes round 163 left in place.
    await act(async () => {
      window.dispatchEvent(new Event("summrise-playwright-changed"));
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(specCalls).toBe(2);
    expect(result.current.specLoaded).toBe(true);
    expect(result.current.rows.map((r) => r.name)).toEqual(["playwright"]);

    // AND NOW IT STOPS — the registry is static per agent process, so no refresh re-reads it. The
    // STATUS still does: that is the other read, and it is deliberately not gated.
    const statusBefore = callsTo("/api/plugins/status");
    await act(async () => {
      window.dispatchEvent(new Event("summrise-playwright-changed"));
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(callsTo("/api/plugins/status")).toBe(statusBefore + 1);
    expect(specCalls, "a loaded inventory is never re-read").toBe(2);
  });

  it("shows the DEVICE's own words for a refused status read, keeping the last good one", async () => {
    // The hook used to ask `deviceRefused(res)` and throw `status: <the device's own words>`. The module
    // asks that question now (`lib/api.ts` owns the predicate) and hands the device's `error` back as
    // `reason`, so the sentence is the one this hook produced before round 4 — and the refusal never
    // reaches the fold, which is why the last good status stays on the card.
    let statusCalls = 0;
    mockCallApi.mockImplementation(async (path: string) => {
      if (String(path).includes("/api/spec")) return spec();
      statusCalls += 1;
      return statusCalls === 1
        ? status(true)
        : { ok: false, error: "playwright is not installed" };
    });

    const { result } = renderHook(() => usePlugins(true));
    await flush();
    expect(result.current.playwright?.running).toBe(true);

    await act(async () => {
      window.dispatchEvent(new Event("summrise-playwright-changed"));
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(result.current.loadError).toBe("status: playwright is not installed");
    expect(
      result.current.playwright?.running,
      "a refusal is not a body — the last good status stays",
    ).toBe(true);
  });

  it("shows a transport failure's message for the status read", async () => {
    // The other source of words: `callApi` throws (`HTTP 502`, `unauthorized`, a timeout — see
    // `lib/api.ts`) and the module carries the Error's message out. Round 4 flattened this to the
    // message-less sentence with the rest; the operator gets the transport's own diagnosis again.
    let statusCalls = 0;
    mockCallApi.mockImplementation(async (path: string) => {
      if (String(path).includes("/api/spec")) return spec();
      statusCalls += 1;
      if (statusCalls === 1) return status(true);
      throw new Error("HTTP 502");
    });

    const { result } = renderHook(() => usePlugins(true));
    await flush();
    expect(result.current.loadError).toBe("");

    await act(async () => {
      window.dispatchEvent(new Event("summrise-playwright-changed"));
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(result.current.loadError).toBe("status: HTTP 502");
    expect(result.current.playwright?.running, "keep-last still holds").toBe(
      true,
    );
  });

  it("still says `status poll failed` when the failure carried no words", async () => {
    // The fallback is not gone, it is the OTHER arm: a refusal with no `error` (or a body that is not an
    // object at all) has no sentence to show, and the module reports `reason: ""` rather than inventing
    // one. It is unreachable for a failure that DOES carry words — which is the half round 4's review
    // found missing.
    //
    // ONE CASE DID NOT COME BACK BYTE FOR BYTE, and this comment said it had. Before round 26 this
    // hook's own throw invented a sentence for a refusal (`res?.error || "status failed"`), so a refusal
    // carrying no error rendered `status: status failed`; a transport failure with no message rendered
    // `status poll failed`. Now there is ONE fallback for both, because the module reports `""` and
    // cannot tell the two apart — and telling them apart would mean a fourth word on its interface for a
    // sentence the hook used to make up. The invented sentence is the one that was dropped; `reason`
    // cannot hand back what nobody said.
    let statusCalls = 0;
    mockCallApi.mockImplementation(async (path: string) => {
      if (String(path).includes("/api/spec")) return spec();
      statusCalls += 1;
      return statusCalls === 1 ? status(true) : { ok: false };
    });

    const { result } = renderHook(() => usePlugins(true));
    await flush();

    await act(async () => {
      window.dispatchEvent(new Event("summrise-playwright-changed"));
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(result.current.loadError).toBe("status poll failed");
  });
});
