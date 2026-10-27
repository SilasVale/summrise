import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Created, Device, TokenMeta } from "./api";
import { FleetTokens } from "./FleetTokens";

const mocks = vi.hoisted(() => ({
  tools: vi.fn(),
  createToken: vi.fn(),
  revokeToken: vi.fn(),
}));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return {
    ...actual,
    api: {
      ...actual.api,
      tools: mocks.tools,
      createToken: mocks.createToken,
      revokeToken: mocks.revokeToken,
    },
  };
});

const alpha: Device = {
  id: "a".repeat(64),
  name: "alpha",
  online: true,
  version: "0.3.43",
  last_seen_ms: 1,
};
const beta: Device = {
  id: "b".repeat(64),
  name: "beta",
  online: true,
  version: "0.3.43",
  last_seen_ms: 2,
};

function aggregate(overrides: Partial<TokenMeta> = {}): TokenMeta {
  return {
    id: "t1",
    name: "dsh",
    device_id: "",
    device_ids: [alpha.id, beta.id],
    tools: ["files.read"],
    allow_destructive: false,
    created_at_ms: Date.UTC(2026, 9, 27),
    ...overrides,
  };
}

function created(token: string): Created {
  return { ...aggregate(), token };
}

beforeEach(() => {
  mocks.tools.mockResolvedValue([
    { name: "files.read", destructive: false, description: "read" },
    { name: "files.list", destructive: false, description: "list" },
  ]);
  mocks.createToken.mockReset();
  mocks.revokeToken.mockReset();
});

describe("FleetTokens", () => {
  it("creates a token with the selected devices and no single device", async () => {
    const onChanged = vi.fn();
    mocks.createToken.mockResolvedValue(created("srm_" + "ab".repeat(32)));
    render(
      <FleetTokens devices={[alpha, beta]} tokens={[]} onChanged={onChanged} />,
    );

    await screen.findByText("Tools (2 of 2)");
    fireEvent.change(screen.getByLabelText("Name"), {
      target: { value: "dsh" },
    });
    // Full trust is the default: every device is already checked and destructive tools are already
    // allowed, so the operator types a name and presses the button.
    fireEvent.click(screen.getByRole("button", { name: "Create fleet token" }));

    await waitFor(() =>
      expect(mocks.createToken).toHaveBeenCalledWith({
        name: "dsh",
        device_ids: [alpha.id, beta.id],
        tools: ["files.read", "files.list"],
        allow_destructive: true,
      }),
    );
    expect(
      await screen.findByText("srm_" + "ab".repeat(32)),
    ).toBeInTheDocument();
    expect(screen.getByText(/url: .*\/mcp\/all/)).toBeInTheDocument();
    expect(onChanged).toHaveBeenCalled();
  });

  it("lists only aggregate tokens, with their device names, and revokes one", async () => {
    const onChanged = vi.fn();
    mocks.revokeToken.mockResolvedValue(undefined);
    const deviceToken: TokenMeta = {
      ...aggregate({ id: "t2", name: "one" }),
      device_id: alpha.id,
      device_ids: undefined,
    };
    render(
      <FleetTokens
        devices={[alpha, beta]}
        tokens={[aggregate(), deviceToken]}
        onChanged={onChanged}
      />,
    );

    expect(screen.getByText("alpha, beta")).toBeInTheDocument();
    expect(screen.queryByText("one")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Revoke" }));
    fireEvent.click(screen.getByRole("button", { name: "Revoke token" }));

    await waitFor(() => expect(mocks.revokeToken).toHaveBeenCalledWith("t1"));
    expect(onChanged).toHaveBeenCalled();
  });
});
