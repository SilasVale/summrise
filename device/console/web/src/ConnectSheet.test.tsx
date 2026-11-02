import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HttpError, type Created, type Device, type LiveTool } from "./api";
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
    expect(screen.getByText("Fleet tokens")).toBeInTheDocument();
    expect(screen.getByText(/2 tools · 1 can change the machine/)).toBeInTheDocument();
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

  it("keeps the fleet endpoint available when no machine is chosen", () => {
    draw({ device: null });
    expect(screen.getByText("http://localhost:3000/mcp/all")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Tokens" }));
    expect(screen.getByText(/spans the fleet/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create fleet token" })).toBeInTheDocument();
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
