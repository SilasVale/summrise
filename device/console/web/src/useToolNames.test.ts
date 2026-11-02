// `useToolNames` is the page's one catalogue read: the fallback is what a form offers until the
// client answers, and an empty or failed answer must leave that fallback, never a blank form.
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TOOL_NAMES, useToolNames } from "./useToolNames";

const mocks = vi.hoisted(() => ({ tools: vi.fn() }));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return {
    ...actual,
    api: { ...actual.api, tools: mocks.tools }
  };
});

// The served catalogue: two names, so the answer cannot be the 38-name fallback by accident.
const SERVED = [{ name: "terminal.create" }, { name: "screen.capture" }];

/** Let a settled request reach the effect and React flush the state it leaves behind. */
async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

describe("useToolNames", () => {
  beforeEach(() => {
    mocks.tools.mockResolvedValue(SERVED);
  });

  it("offers the fallback until the client answers, then the served names", async () => {
    const { result } = renderHook(() => useToolNames());

    expect(result.current).toEqual(TOOL_NAMES);

    await waitFor(() => expect(result.current).toEqual(["terminal.create", "screen.capture"]));
  });

  it("keeps the fallback when the catalogue answer is empty", async () => {
    mocks.tools.mockResolvedValue([]);

    const { result } = renderHook(() => useToolNames());
    await settle();

    expect(result.current).toEqual(TOOL_NAMES);
  });

  it("keeps the fallback when the catalogue request fails", async () => {
    mocks.tools.mockRejectedValue(new Error("offline"));

    const { result } = renderHook(() => useToolNames());
    await settle();

    expect(result.current).toEqual(TOOL_NAMES);
  });
});
