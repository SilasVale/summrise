import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Device, LiveTool } from "./api";
import type { PluginRecord } from "./pluginApi";
import { ToolsTab } from "./ToolsTab";

const live = vi.hoisted(() => ({ value: {} as Record<string, unknown> }));
vi.mock("./useLiveTools", () => ({ useLiveTools: () => live.value }));

const mocks = vi.hoisted(() => ({ list: vi.fn() }));
vi.mock("./pluginApi", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./pluginApi")>();
  return { ...actual, listPlugins: mocks.list };
});

const workshop: Device = {
  id: "dev-workshop",
  name: "workshop",
  online: true,
  version: "0.3.246",
  last_seen_ms: 1
};

function tool(overrides: Partial<LiveTool> & { name: string }): LiveTool {
  return {
    description: "",
    inputSchema: { type: "object", properties: {} },
    source: "builtin",
    capability: overrides.name.split(".")[0],
    destructive: false,
    ...overrides
  };
}

const plugin: PluginRecord = {
  id: "com.example.hello",
  namespace: "hello",
  name: "Hello",
  version: "0.1.0",
  description: "Two friendly tools",
  state: "running",
  tools: 2,
  path: "C:\\dev\\plugins\\hello",
  last_error: null
};

beforeEach(() => {
  live.value = {
    tools: [tool({ name: "files.read", description: "read a file" }), tool({ name: "plugins.list" })],
    stale: false,
    error: null,
    offline: false,
    loading: false,
    retry: vi.fn()
  };
  mocks.list.mockReset();
  mocks.list.mockResolvedValue([plugin]);
});

describe("ToolsTab", () => {
  it("answers with the catalogue first and the plugins second, one h2 each", async () => {
    render(
      <ToolsTab
        device={workshop}
        deviceOnline
        tool={null}
        onTool={vi.fn()}
        onMachine={vi.fn()}
        onBrowser={vi.fn()}
      />
    );

    expect(
      screen.getAllByRole("heading", { level: 2 }).map((heading) => heading.textContent)
    ).toEqual(["Tools on this machine", "Plugins"]);
    expect(screen.getByRole("button", { name: /files\.read/ })).toBeInTheDocument();
    expect(await screen.findByText("Hello")).toBeInTheDocument();
  });

  it("opens a tool's sheet inside the catalogue section", () => {
    render(
      <ToolsTab
        device={workshop}
        deviceOnline
        tool="files.read"
        onTool={vi.fn()}
        onMachine={vi.fn()}
        onBrowser={vi.fn()}
      />
    );
    expect(screen.getByRole("dialog", { name: "files.read" })).toBeInTheDocument();
  });
});
