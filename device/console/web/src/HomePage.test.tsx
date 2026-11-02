import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AuditLine, Device, LiveTool } from "./api";
import { HomePage } from "./HomePage";

const live = vi.hoisted(() => ({ value: {} as Record<string, unknown> }));
vi.mock("./useLiveTools", () => ({ useLiveTools: () => live.value }));

const plugins = vi.hoisted(() => ({ list: vi.fn() }));
vi.mock("./pluginApi", () => ({
  listPlugins: plugins.list
}));

const workshop: Device = {
  id: "dev-workshop",
  name: "workshop",
  online: true,
  version: "0.3.246",
  last_seen_ms: 1
};

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

const tools = [
  tool("terminal.create", "terminal"),
  tool("terminal.write", "terminal"),
  tool("files.list", "files"),
  tool("files.read", "files"),
  tool("screen.capture", "screen"),
  tool("plugins.list", "plugins")
];

function call(overrides: Partial<AuditLine> = {}): AuditLine {
  return {
    ts_ms: Date.now() - 12_000,
    kind: "mcp",
    device_id: workshop.id,
    tool: "terminal.create",
    detail: null,
    status: 200,
    token_id: null,
    ...overrides
  };
}

function draw(overrides: Record<string, unknown> = {}) {
  const onConnect = vi.fn();
  const onCapabilities = vi.fn();
  const view = render(
    <HomePage
      device={workshop}
      devices={[workshop]}
      selectedId={workshop.id}
      code={null}
      enrollCommand=""
      busy={false}
      audit={[call(), call({ tool: "files.read", ts_ms: Date.now() - 60_000 }), call({ tool: "screen.capture" }), call({ tool: "runs.exec" })]}
      onOpen={vi.fn()}
      onRename={vi.fn()}
      onRemove={vi.fn()}
      onDiagnostics={vi.fn()}
      onAddMachine={vi.fn()}
      onDismissCode={vi.fn()}
      onNotice={vi.fn()}
      onConnect={onConnect}
      onCapabilities={onCapabilities}
      onPlugins={vi.fn()}
      onActivity={vi.fn()}
      {...overrides}
    />
  );
  return { ...view, onConnect, onCapabilities };
}

beforeEach(() => {
  live.value = { tools, stale: false, error: null, offline: false, loading: false, retry: vi.fn() };
  plugins.list.mockResolvedValue([
    { namespace: "hello" },
    { namespace: "image" }
  ]);
});

describe("Home — the hub", () => {
  it("states the machine in one status line, with counts and the last three calls", async () => {
    draw();

    expect(screen.getByRole("heading", { name: "Home", level: 1 })).toBeInTheDocument();
    expect(await screen.findByText("Running · v0.3.246")).toBeInTheDocument();
    // The counts line, not the status line, carries the counts: six tools across four capabilities
    // (terminal, files, screen, plugins).
    const counts = await screen.findByRole("button", { name: /6 tools · 4 capabilities · 2 plugins/ });
    expect(counts).toHaveTextContent("6 tools · 4 capabilities · 2 plugins");
    expect(screen.getByText(`http://localhost:3000/mcp/${workshop.id}`)).toBeInTheDocument();

    const rows = screen.getAllByRole("listitem");
    expect(rows).toHaveLength(3);
    expect(rows[0]).toHaveTextContent("terminal.create");
  });

  it("has one primary action and no second Connect button when no call exists", async () => {
    draw({ audit: [] });

    expect(await screen.findByText("No calls yet.")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Connect a client" })).toHaveLength(1);
    expect(screen.getAllByRole("button", { name: /Connect/ })).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Connect a client" })).toHaveClass("primary");
  });

  it("opens the catalogue from the counts line", async () => {
    const { onCapabilities } = draw();
    fireEvent.click(await screen.findByRole("button", { name: /6 tools · 4 capabilities/ }));
    expect(onCapabilities).toHaveBeenCalledTimes(1);
  });

  it("renders an offline machine without fake counts, dims the door and says why", () => {
    live.value = {
      tools: [],
      stale: false,
      error: "this machine is not connected; connect it, then try again",
      offline: true,
      loading: false,
      retry: vi.fn()
    };
    draw({ device: { ...workshop, online: false } });

    expect(screen.getByText("Offline · v0.3.246")).toBeInTheDocument();
    expect(
      screen.getByText(
        "workshop is not connected. The console keeps trying; open the machine's panel if it stays offline."
      )
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy" })).toBeDisabled();
    expect(screen.getByText("Tool counts are unavailable while the machine is offline.")).toBeInTheDocument();
    expect(screen.queryByText(/0 tools/)).not.toBeInTheDocument();
  });

  it("labels a stale catalogue and retries it", async () => {
    const retry = vi.fn();
    live.value = {
      tools,
      stale: true,
      error: "the console could not read the machine's tools",
      offline: false,
      loading: false,
      retry
    };
    draw();

    expect(
      screen.getByText(
        "The machine is running. The console could not read its tool list, so these counts may be stale."
      )
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("button", { name: /6 tools · 4 capabilities · 2 plugins · stale/ })
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(retry).toHaveBeenCalledTimes(1);
  });

  it("counts zero plugins on an older build without plugins.list", async () => {
    live.value = {
      tools: [tool("terminal.create", "terminal")],
      stale: false,
      error: null,
      offline: false,
      loading: false,
      retry: vi.fn()
    };
    draw();

    expect(await screen.findByText("Running · v0.3.246")).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: /1 tool · 1 capability · 0 plugins/ })).toBeInTheDocument();
    expect(plugins.list).not.toHaveBeenCalled();
  });
});
