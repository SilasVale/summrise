// `useToolCatalogue` is the page's one catalogue read: the fallback is what a form offers until the
// client answers, and an empty or failed answer must leave that fallback, never a blank form. The
// served answer's destructive flags are the reason this is a catalogue read and not a name read.
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FALLBACK_CATALOGUE, useToolCatalogue } from "./useToolCatalogue";

const mocks = vi.hoisted(() => ({ tools: vi.fn() }));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return {
    ...actual,
    api: { ...actual.api, tools: mocks.tools }
  };
});

// The served catalogue: two tools and their flags, so the answer cannot be the fallback by accident.
const SERVED = [
  { name: "terminal.create", destructive: true },
  { name: "screen.capture", destructive: false }
];

/** Let a settled request reach the effect and React flush the state it leaves behind. */
async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

describe("useToolCatalogue", () => {
  beforeEach(() => {
    mocks.tools.mockResolvedValue(SERVED);
  });

  it("offers the fallback until the client answers, then the served tools with their flags", async () => {
    const { result } = renderHook(() => useToolCatalogue());

    expect(result.current).toEqual(FALLBACK_CATALOGUE);
    expect(result.current.every((tool) => tool.destructive === null)).toBe(true);

    await waitFor(() => expect(result.current).toEqual(SERVED));
  });

  it("keeps the fallback when the catalogue answer is empty", async () => {
    mocks.tools.mockResolvedValue([]);

    const { result } = renderHook(() => useToolCatalogue());
    await settle();

    expect(result.current).toEqual(FALLBACK_CATALOGUE);
  });

  it("keeps the fallback when the catalogue request fails", async () => {
    mocks.tools.mockRejectedValue(new Error("offline"));

    const { result } = renderHook(() => useToolCatalogue());
    await settle();

    expect(result.current).toEqual(FALLBACK_CATALOGUE);
  });
});
