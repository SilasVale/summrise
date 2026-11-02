import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HttpError, type LiveTool } from "./api";

const mocks = vi.hoisted(() => ({ liveTools: vi.fn() }));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return {
    ...actual,
    api: { ...actual.api, liveTools: mocks.liveTools }
  };
});

import { resetLiveToolsCache, useLiveTools } from "./useLiveTools";

function tool(name: string, capability: string): LiveTool {
  return {
    name,
    description: "",
    inputSchema: { type: "object", properties: {} },
    source: "builtin",
    capability,
    destructive: false
  };
}

const TOOLS = [tool("terminal.create", "terminal"), tool("files.list", "files")];

beforeEach(() => {
  resetLiveToolsCache();
  mocks.liveTools.mockReset();
});

describe("useLiveTools", () => {
  it("reads the live catalogue and serves it from the 30 s cache", async () => {
    mocks.liveTools.mockResolvedValue({ tools: TOOLS });

    const first = renderHook(() => useLiveTools("dev-1"));
    await waitFor(() => expect(first.result.current.tools).toHaveLength(2));
    expect(first.result.current.loading).toBe(false);
    expect(mocks.liveTools).toHaveBeenCalledTimes(1);
    expect(mocks.liveTools).toHaveBeenCalledWith("dev-1");

    first.unmount();
    const second = renderHook(() => useLiveTools("dev-1"));
    // The cache answers synchronously: no skeleton on a remount inside the window.
    expect(second.result.current.tools).toHaveLength(2);
    expect(mocks.liveTools).toHaveBeenCalledTimes(1);
  });

  it("keeps the last good catalogue and marks it stale when the read fails", async () => {
    mocks.liveTools.mockResolvedValue({ tools: TOOLS });
    const view = renderHook(() => useLiveTools("dev-1"));
    await waitFor(() => expect(view.result.current.tools).toHaveLength(2));

    mocks.liveTools.mockRejectedValue(
      new HttpError(503, "device_offline", "this machine is not connected; connect it, then try again")
    );
    act(() => view.result.current.retry());

    await waitFor(() => expect(view.result.current.error).not.toBeNull());
    expect(view.result.current.tools).toHaveLength(2);
    expect(view.result.current.stale).toBe(true);
    expect(view.result.current.offline).toBe(true);
    expect(view.result.current.error).toBe(
      "this machine is not connected; connect it, then try again"
    );
  });

  it("reports an offline read with no cache as an error, never an empty catalogue", async () => {
    mocks.liveTools.mockRejectedValue(new HttpError(503, "device_offline"));

    const view = renderHook(() => useLiveTools("dev-1"));
    await waitFor(() => expect(view.result.current.loading).toBe(false));
    expect(view.result.current.tools).toEqual([]);
    expect(view.result.current.offline).toBe(true);
    expect(view.result.current.stale).toBe(false);
    expect(view.result.current.error).toMatch(/not connected/);
  });

  it("does not read anything until a machine is chosen", () => {
    const view = renderHook(() => useLiveTools(null));
    expect(mocks.liveTools).not.toHaveBeenCalled();
    expect(view.result.current.tools).toEqual([]);
    expect(view.result.current.loading).toBe(false);
  });

  it("refetches once the cached copy is older than 30 s", async () => {
    const now = vi.spyOn(Date, "now").mockReturnValue(1_000);
    mocks.liveTools.mockResolvedValue({ tools: TOOLS });
    const first = renderHook(() => useLiveTools("dev-1"));
    await waitFor(() => expect(first.result.current.tools).toHaveLength(2));
    first.unmount();
    expect(mocks.liveTools).toHaveBeenCalledTimes(1);

    now.mockReturnValue(1_000 + 30_001);
    const second = renderHook(() => useLiveTools("dev-1"));
    await waitFor(() => expect(mocks.liveTools).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(second.result.current.stale).toBe(false));
  });
});
