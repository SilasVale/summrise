import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HttpError, type Created, type Device, type LiveTool, type TokenMeta } from "./api";
import { ConnectSheet } from "./ConnectSheet";

const mocks = vi.hoisted(() => ({
  tools: vi.fn(),
  liveTools: vi.fn(),
  tokens: vi.fn(),
  createToken: vi.fn(),
  rotateToken: vi.fn(),
  revokeToken: vi.fn()
}));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return {
    ...actual,
    api: {
      ...actual.api,
      tools: mocks.tools,
      tokens: mocks.tokens,
      createToken: mocks.createToken,
      rotateToken: mocks.rotateToken,
      revokeToken: mocks.revokeToken
    }
  };
});

vi.mock("./useLiveTools", () => ({
  useLiveTools: () => ({
    tools: mocks.liveTools(),
    stale: false,
    error: null,
    offline: false,
    loading: false,
    retry: vi.fn()
  })
}));

const FALLBACK_TOOL_NAMES = ["files.read", "terminal.write"];
vi.mock("./useToolNames", () => ({
  // A stable array: the real hook returns state, and an effect that depends on it must not re-run
  // every render.
  useToolNames: () => FALLBACK_TOOL_NAMES
}));

const WORKSHOP: Device = {
  id: "dev-workshop",
  name: "workshop",
  online: true,
  version: "0.3.42",
  last_seen_ms: 1
};

function tool(name: string, capability: string, destructive: boolean): LiveTool {
  return {
    name,
    description: "",
    inputSchema: { type: "object", properties: {} },
    source: "builtin",
    capability,
    destructive
  };
}

const TOOLS = [tool("files.read", "files", false), tool("terminal.write", "terminal", true)];

const ATTIC: Device = {
  id: "dev-attic",
  name: "attic",
  online: true,
  version: "0.3.42",
  last_seen_ms: 2
};

function tokenMeta(overrides: Partial<TokenMeta> = {}): TokenMeta {
  return {
    id: "tok-device",
    name: "laptop",
    device_id: WORKSHOP.id,
    tools: ["files.read"],
    allow_destructive: false,
    created_at_ms: Date.UTC(2026, 10, 1),
    ...overrides
  };
}

const CREATED: Created = {
  id: "tok-1",
  name: "laptop",
  device_id: WORKSHOP.id,
  tools: ["files.read", "terminal.write"],
  allow_destructive: false,
  created_at_ms: Date.UTC(2026, 10, 2),
  token: "srm_" + "a".repeat(64)
};

beforeEach(() => {
  mocks.tools.mockResolvedValue([{ name: "files.read" }, { name: "terminal.write" }]);
  mocks.liveTools.mockReturnValue(TOOLS);
  mocks.tokens.mockResolvedValue([]);
  mocks.createToken.mockReset();
  mocks.rotateToken.mockReset();
  mocks.revokeToken.mockReset();
  mocks.createToken.mockResolvedValue(CREATED);
});

function tokenForm(): HTMLElement {
  return document.querySelector(".token-create") as HTMLElement;
}

function draw(overrides: Record<string, unknown> = {}) {
  const onClose = vi.fn();
  const view = render(
    <ConnectSheet
      device={WORKSHOP}
      devices={[WORKSHOP]}
      initialSection="endpoint"
      onClose={onClose}
      {...overrides}
    />
  );
  return { ...view, onClose };
}

describe("ConnectSheet", () => {
  it("switches between the endpoint, the four clients and the tokens", () => {
    draw();

    expect(screen.getByRole("dialog", { name: "Connect a client" })).toBeInTheDocument();
    expect(
      screen.getByText(`http://localhost:3000/mcp/${WORKSHOP.id}`)
    ).toBeInTheDocument();
    expect(screen.getByText(/A bearer token is the credential/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("tab", { name: "Clients" }));
    const config = screen.getByText(/deepseek-ai\/dsh-mcp-client/);
    expect(config).toHaveTextContent("Bearer <token>");
    fireEvent.click(screen.getByRole("tab", { name: "Claude" }));
    expect(screen.getByText(/claude mcp add --transport http summrise/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("tab", { name: "Tokens" }));
    expect(screen.getByRole("heading", { name: "Create a token" })).toBeInTheDocument();
    expect(screen.getByText(/2 tools · 1 can change the machine/)).toBeInTheDocument();
    // One credential list, not a device list with a fleet list folded into a disclosure.
    expect(screen.queryByText("Fleet tokens")).not.toBeInTheDocument();
    expect(document.querySelector("details.fleet-token-disclosure")).toBeNull();
  });

  it("opens on Tokens when the account menu asked for it, with Tokens as the title", () => {
    draw({ initialSection: "tokens" });
    expect(screen.getByRole("dialog", { name: "Tokens" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Create a token" })).toBeInTheDocument();
  });

  it("creates a token, shows it once with the warning, and forgets it when the sheet closes", async () => {
    const first = draw();
    fireEvent.click(screen.getByRole("tab", { name: "Tokens" }));
    fireEvent.change(within(tokenForm()).getByLabelText("Name"), { target: { value: "laptop" } });
    fireEvent.click(screen.getByRole("button", { name: "Create token" }));

    await waitFor(() =>
      expect(mocks.createToken).toHaveBeenCalledWith({
        name: "laptop",
        device_id: WORKSHOP.id,
        tools: ["files.read", "terminal.write"],
        allow_destructive: false
      })
    );
    expect(await screen.findByText("Copy this token now. It is not shown again.")).toBeInTheDocument();
    expect(screen.getByText(CREATED.token)).toBeInTheDocument();

    first.unmount();
    draw();
    expect(document.body.textContent).not.toContain(CREATED.token);
    expect(screen.queryByText("Copy this token now. It is not shown again.")).not.toBeInTheDocument();
  });

  it("offers Read-only and Choose tools presets, and scopes the token to what is chosen", async () => {
    draw({ initialSection: "tokens" });
    fireEvent.change(within(tokenForm()).getByLabelText("Name"), { target: { value: "read" } });

    fireEvent.click(within(tokenForm()).getByRole("button", { name: "Read-only" }));
    expect(within(tokenForm()).getByText(/1 tool · 0 can change the machine/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Create token" }));
    await waitFor(() =>
      expect(mocks.createToken).toHaveBeenCalledWith(
        expect.objectContaining({ name: "read", tools: ["files.read"] })
      )
    );
  });

  it("opens the full scope grid only behind Choose tools", () => {
    draw({ initialSection: "tokens" });
    expect(within(tokenForm()).queryByRole("checkbox", { name: "terminal.write" })).not.toBeInTheDocument();
    fireEvent.click(within(tokenForm()).getByRole("button", { name: "Choose tools…" }));
    const grid = within(tokenForm()).getByRole("checkbox", { name: "terminal.write" });
    expect(grid).toBeChecked();
    fireEvent.click(grid);
    expect(grid).not.toBeChecked();
    expect(within(tokenForm()).getByText(/1 tool · 0 can change the machine/)).toBeInTheDocument();
  });

  it("shows the console's recovery sentence when the token is refused", async () => {
    mocks.createToken.mockRejectedValue(
      new HttpError(
        400,
        "unknown_tool",
        "the device does not advertise hello.greet; connect it and try again"
      )
    );
    draw({ initialSection: "tokens" });
    fireEvent.change(within(tokenForm()).getByLabelText("Name"), { target: { value: "plugin" } });
    fireEvent.click(screen.getByRole("button", { name: "Create token" }));

    expect(
      await screen.findByText("the device does not advertise hello.greet; connect it and try again")
    ).toBeInTheDocument();
  });

  it("rotates a token after naming the cost, then shows the new value once", async () => {
    mocks.tokens.mockResolvedValue([
      {
        id: "tok-1",
        name: "laptop",
        device_id: WORKSHOP.id,
        tools: ["files.read"],
        allow_destructive: false,
        created_at_ms: Date.UTC(2026, 10, 2)
      }
    ]);
    mocks.rotateToken.mockResolvedValue({ ...CREATED, token: "srm_" + "b".repeat(64) });
    draw({ initialSection: "tokens" });

    fireEvent.click(await screen.findByRole("button", { name: "Rotate" }));
    const dialog = await screen.findByRole("dialog", { name: 'Rotate "laptop"?' });
    expect(within(dialog).getByText(/stops working/)).toBeInTheDocument();
    expect(within(dialog).getByText(/shown once/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Rotate token" }));

    await waitFor(() => expect(mocks.rotateToken).toHaveBeenCalledWith("tok-1"));
    expect(await screen.findByText("Copy this token now. It is not shown again.")).toBeInTheDocument();
    expect(screen.getByText("srm_" + "b".repeat(64))).toBeInTheDocument();
  });

  it("revokes a token only after the confirmation names the consequence", async () => {
    mocks.tokens.mockResolvedValue([
      {
        id: "tok-1",
        name: "laptop",
        device_id: WORKSHOP.id,
        tools: ["files.read"],
        allow_destructive: false,
        created_at_ms: Date.UTC(2026, 10, 2)
      }
    ]);
    mocks.revokeToken.mockResolvedValue(undefined);
    draw({ initialSection: "tokens" });

    fireEvent.click(await screen.findByRole("button", { name: "Revoke" }));
    const dialog = await screen.findByRole("dialog", { name: 'Revoke "laptop"?' });
    expect(within(dialog).getByText(/cannot be recovered/)).toBeInTheDocument();
    expect(mocks.revokeToken).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Revoke token" }));

    await waitFor(() => expect(mocks.revokeToken).toHaveBeenCalledWith("tok-1"));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: 'Revoke "laptop"?' })).not.toBeInTheDocument());
  });

  it("keeps the fleet endpoint available when no machine is chosen", () => {
    draw({ device: null });
    expect(screen.getByText("http://localhost:3000/mcp/all")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Tokens" }));
    expect(screen.getByText(/spans the fleet/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create token" })).toBeInTheDocument();
    // With no current machine the only scope the form can mint is the fleet one.
    expect(screen.getByRole("button", { name: "all machines" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "this machine" })).toBeDisabled();
  });

  it("dims the endpoint and disables Copy while the machine is offline", () => {
    draw({ device: { ...WORKSHOP, online: false } });
    const copy = screen.getByRole("button", { name: "Copy" });
    expect(copy).toBeDisabled();
    expect(screen.getByText(`http://localhost:3000/mcp/${WORKSHOP.id}`)).toHaveClass("dim");
  });

  it("drops the client block to a Copied sentence", async () => {
    const clipboard = { writeText: vi.fn().mockResolvedValue(undefined) };
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: clipboard });
    draw();
    fireEvent.click(screen.getByRole("tab", { name: "Clients" }));
    fireEvent.click(screen.getByRole("button", { name: "Copy configuration" }));
    expect(await screen.findByText("Copied. It is not shown again.")).toBeInTheDocument();
    expect(screen.queryByText(/deepseek-ai\/dsh-mcp-client/)).not.toBeInTheDocument();
    delete (navigator as { clipboard?: unknown }).clipboard;
  });
});

describe("ConnectSheet's one credential list", () => {
  it("shows a device token and a fleet token together, each with its scope, newest first", async () => {
    const deviceToken = tokenMeta();
    const fleetToken = tokenMeta({
      id: "tok-fleet",
      name: "dsh-fleet",
      device_id: "",
      device_ids: [WORKSHOP.id, ATTIC.id],
      tools: ["files.read", "terminal.write"],
      allow_destructive: true,
      created_at_ms: Date.UTC(2026, 10, 2)
    });
    // One registry carries both scopes; the list must not draw the same record twice and must
    // label each row by the scope the registry says it has.
    mocks.tokens.mockResolvedValue([fleetToken, deviceToken]);

    draw({ initialSection: "tokens" });

    const deviceRow = await screen.findByRole("row", { name: /laptop/ });
    const fleetRow = screen.getByRole("row", { name: /dsh-fleet/ });
    expect(deviceRow).toHaveTextContent("this machine");
    expect(fleetRow).toHaveTextContent("all machines");
    expect(screen.getAllByRole("row", { name: /laptop|dsh-fleet/ })).toHaveLength(2);

    const dataRows = within(screen.getByRole("table")).getAllByRole("row").slice(1);
    expect(dataRows[0]).toHaveTextContent("dsh-fleet");
    expect(dataRows[1]).toHaveTextContent("laptop");
  });

  it("mints against all machines with the fleet payload and shows the new row with that scope", async () => {
    mocks.createToken.mockResolvedValue({
      ...CREATED,
      id: "tok-fleet",
      name: "fleet",
      device_id: "",
      device_ids: [WORKSHOP.id, ATTIC.id],
      token: "srm_" + "c".repeat(64)
    });
    draw({ initialSection: "tokens", devices: [WORKSHOP, ATTIC] });

    fireEvent.click(within(tokenForm()).getByRole("button", { name: "all machines" }));
    fireEvent.change(within(tokenForm()).getByLabelText("Name"), { target: { value: "fleet" } });
    fireEvent.click(screen.getByRole("button", { name: "Create token" }));

    await waitFor(() =>
      expect(mocks.createToken).toHaveBeenCalledWith({
        name: "fleet",
        device_ids: [WORKSHOP.id, ATTIC.id],
        tools: ["files.read", "terminal.write"],
        allow_destructive: false
      })
    );
    const row = await screen.findByRole("row", { name: /fleet/ });
    expect(row).toHaveTextContent("all machines");
    // The one-time value is still the reveal; the row is metadata and never the secret.
    expect(screen.getByText("srm_" + "c".repeat(64))).toBeInTheDocument();
    expect(row).not.toHaveTextContent("srm_");
  });

  it("revokes a fleet row through the token endpoint after naming the consequence", async () => {
    mocks.tokens.mockResolvedValue([
      tokenMeta({
        id: "tok-fleet",
        name: "dsh-fleet",
        device_id: "",
        device_ids: [WORKSHOP.id, ATTIC.id]
      })
    ]);
    mocks.revokeToken.mockResolvedValue(undefined);
    draw({ initialSection: "tokens" });

    const row = await screen.findByRole("row", { name: /dsh-fleet/ });
    fireEvent.click(within(row).getByRole("button", { name: "Revoke" }));
    const dialog = await screen.findByRole("dialog", { name: 'Revoke "dsh-fleet"?' });
    expect(within(dialog).getByText(/cannot be recovered/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Revoke token" }));

    await waitFor(() => expect(mocks.revokeToken).toHaveBeenCalledWith("tok-fleet"));
    expect(mocks.revokeToken).toHaveBeenCalledTimes(1);
  });

  it("says what a token is for when none exists, and keeps the mint form visible", async () => {
    draw({ initialSection: "tokens" });

    expect(await screen.findByText(/none has been minted yet/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Create a token" })).toBeInTheDocument();
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
  });

  it("defaults the mint scope to this machine and can be switched before minting", async () => {
    mocks.createToken.mockResolvedValue({ ...CREATED, name: "here" });
    draw({ initialSection: "tokens", devices: [WORKSHOP, ATTIC] });

    expect(within(tokenForm()).getByRole("button", { name: "this machine" })).toHaveAttribute(
      "aria-pressed",
      "true"
    );
    fireEvent.click(within(tokenForm()).getByRole("button", { name: "all machines" }));
    expect(within(tokenForm()).getByRole("button", { name: "all machines" })).toHaveAttribute(
      "aria-pressed",
      "true"
    );
    fireEvent.change(within(tokenForm()).getByLabelText("Name"), { target: { value: "here" } });
    fireEvent.click(screen.getByRole("button", { name: "Create token" }));

    await waitFor(() =>
      expect(mocks.createToken).toHaveBeenCalledWith(
        expect.objectContaining({ device_ids: [WORKSHOP.id, ATTIC.id] })
      )
    );
  });
});
