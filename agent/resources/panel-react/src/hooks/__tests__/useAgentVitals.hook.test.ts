// THE HOOK'S SIDE OF THE CONTRACT, which the boot-verdict tests next door cannot reach: those pin
// `parseLastBoot` as a pure function, so what the HOOK does with a response was untested.
//
// The property (2026-09-24, the panel exploration): a FAILED read must not update the strip. Every
// sibling poller gates on `ok !== true` — `useMonitors`, `useVitalsSeries`, `useBootHistory`,
// `UpdateCard` — and this one guarded on `!j`, which an empty object passes because `{}` is truthy. A
// refusal that carried fields would therefore have been read as a sample.
import { describe, expect, it, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { callApi } from "../../lib/api";
import { useAgentVitals } from "../useAgentVitals";

vi.mock("../../lib/api", () => ({ callApi: vi.fn() }));

const mockCallApi = callApi as unknown as ReturnType<typeof vi.fn>;

beforeEach(() => {
  mockCallApi.mockReset();
});

describe("useAgentVitals", () => {
  it("takes a good sample", async () => {
    mockCallApi.mockResolvedValue({
      ok: true,
      uptime_secs: 3661,
      host: "10.0.0.5",
      port: 18080,
    });
    const { result } = renderHook(() => useAgentVitals(10));
    await waitFor(() => expect(result.current.uptime).toBe("1h 01m"));
    expect(result.current.host).toBe("10.0.0.5");
  });

  it("IGNORES a refusal that carries fields, keeping the last known values", async () => {
    mockCallApi.mockResolvedValue({
      ok: true,
      uptime_secs: 3661,
      host: "10.0.0.5",
    });
    const { result } = renderHook(() => useAgentVitals(10));
    await waitFor(() => expect(result.current.uptime).toBe("1h 01m"));

    // The device refuses the next read — and the body carries fields anyway, which is the case the old
    // `!j` guard let through. Nothing on the strip may move.
    mockCallApi.mockResolvedValue({
      ok: false,
      error: "nope",
      uptime_secs: 0,
      host: "somewhere-else",
    });
    await new Promise((r) => setTimeout(r, 30));
    expect(result.current.uptime).toBe("1h 01m");
    expect(result.current.host).toBe("10.0.0.5");
  });

  it("ignores an EMPTY object too, which is what the design harness answers unknown routes with", async () => {
    mockCallApi.mockResolvedValue({ ok: true, uptime_secs: 60 });
    const { result } = renderHook(() => useAgentVitals(10));
    await waitFor(() => expect(result.current.uptime).toBe("1m 0s"));

    mockCallApi.mockResolvedValue({});
    await new Promise((r) => setTimeout(r, 30));
    expect(result.current.uptime).toBe("1m 0s");
  });
});
