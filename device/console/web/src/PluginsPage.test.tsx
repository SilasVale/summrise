import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Device, LiveTool } from "./api";
import type { PluginRecord } from "./pluginApi";
import { PluginsPage } from "./PluginsPage";

const live = vi.hoisted(() => ({ value: {} as Record<string, unknown> }));
vi.mock("./useLiveTools", () => ({ useLiveTools: () => live.value }));

const mocks = vi.hoisted(() => ({
  list: vi.fn(),
  inspect: vi.fn(),
  add: vi.fn(),
  remove: vi.fn(),
  restart: vi.fn(),
  logs: vi.fn()
}));
vi.mock("./pluginApi", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./pluginApi")>();
  return {
    ...actual,
    listPlugins: mocks.list,
    inspectPlugin: mocks.inspect,
    addPlugin: mocks.add,
    removePlugin: mocks.remove,
    restartPlugin: mocks.restart,
    pluginLogs: mocks.logs
  };
});

const workshop: Device = {
  id: "dev-workshop",
  name: "workshop",
  online: true,
  version: "0.3.246",
  last_seen_ms: 1
};

function supportTool(name = "plugins.list"): LiveTool {
  return {
    name,
    description: "",
    inputSchema: { type: "object", properties: {} },
    source: "builtin",
    capability: "plugins",
    destructive: false
  };
}

function plugin(overrides: Partial<PluginRecord> = {}): PluginRecord {
  return {
    id: "com.example.hello",
    namespace: "hello",
    name: "Hello",
    version: "0.1.0",
    description: "Two friendly tools",
    state: "running",
    tools: 2,
    path: "C:\\dev\\plugins\\hello",
    last_error: null,
    ...overrides
  };
}

function draw(overrides: Record<string, unknown> = {}) {
  const onChoose = vi.fn();
  const onMachine = vi.fn();
  const view = render(
    <PluginsPage
      devices={[workshop]}
      device={workshop}
      onChoose={onChoose}
      onMachine={onMachine}
      {...overrides}
    />
  );
  return { ...view, onChoose, onMachine };
}

function openActions(name: string) {
  const row = screen.getByText(name).closest("article") as HTMLElement;
  fireEvent.click(within(row).getByTitle(`Actions for ${name}`));
}

function supported() {
  live.value = {
    tools: [supportTool()],
    stale: false,
    error: null,
    offline: false,
    loading: false,
    retry: vi.fn()
  };
}

beforeEach(() => {
  supported();
  mocks.list.mockReset();
  mocks.inspect.mockReset();
  mocks.add.mockReset();
  mocks.remove.mockReset();
  mocks.restart.mockReset();
  mocks.logs.mockReset();
  mocks.list.mockResolvedValue([]);
  mocks.logs.mockResolvedValue([]);
});

describe("PluginsPage", () => {
  it("puts Add plugin inside the empty state, as the one action, and no manifest example", async () => {
    draw();
    expect(screen.getAllByRole("heading", { level: 1 })).toHaveLength(1);
    expect(screen.getByRole("heading", { name: "Plugins", level: 1 })).toBeInTheDocument();
    expect(await screen.findByText("No plugins yet. Add one when a built-in cannot do the job.")).toBeInTheDocument();
    expect(screen.queryByText(/plugin\.json/)).not.toBeInTheDocument();
    expect(screen.queryByText(/manifest/i)).not.toBeInTheDocument();

    const add = screen.getByRole("button", { name: "Add plugin" });
    expect(add.closest(".empty-state")).not.toBeNull();
    fireEvent.click(add);
    expect(screen.getByRole("dialog", { name: "Add plugin" })).toBeInTheDocument();
  });

  it("renders an older build as a sentence with Open Machine, and shows no Add button", () => {
    live.value = {
      tools: [supportTool("terminal.list")],
      stale: false,
      error: null,
      offline: false,
      loading: false,
      retry: vi.fn()
    };
    const { onMachine } = draw();

    expect(screen.getByText("This machine needs a newer Summrise to host plugins.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add plugin" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open Machine" }));
    expect(onMachine).toHaveBeenCalledTimes(1);
    expect(mocks.list).not.toHaveBeenCalled();
  });

  it("renders an offline machine as a banner with Retry, without pretending the list is empty", () => {
    live.value = {
      tools: [],
      stale: false,
      error: "this machine is not connected; connect it, then try again",
      offline: true,
      loading: false,
      retry: vi.fn()
    };
    draw({ device: { ...workshop, online: false } });

    expect(
      screen.getByText(
        "workshop is not connected. The console keeps trying; open the machine's panel if it stays offline."
      )
    ).toBeInTheDocument();
    expect(screen.queryByText("No plugins yet.")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add plugin" })).not.toBeInTheDocument();
  });

  it("lists each plugin with its state, tools and source path, and a failed row's reason and Restart", async () => {
    mocks.list.mockResolvedValue([
      plugin(),
      plugin({
        id: "com.example.image",
        namespace: "image",
        name: "Image",
        version: "0.2.1",
        state: "failed",
        tools: 0,
        path: "C:\\work\\image-tools",
        last_error: "initialize timed out after 10s"
      })
    ]);
    draw();

    expect(await screen.findByText("Hello")).toBeInTheDocument();
    expect(screen.getByText("2 plugins")).toBeInTheDocument();
    expect(screen.getByText("Running")).toBeInTheDocument();
    expect(screen.getByText("Failed")).toBeInTheDocument();
    expect(screen.getByText(/2 tools/)).toBeInTheDocument();
    const path = screen.getByText("C:\\dev\\plugins\\hello");
    expect(path).toHaveAttribute("title", "C:\\dev\\plugins\\hello");
    expect(screen.getByText("initialize timed out after 10s")).toBeInTheDocument();

    // A failed row carries a visible Restart and keeps Logs in its overflow.
    const image = screen.getByText("Image").closest("article") as HTMLElement;
    expect(within(image).getByRole("button", { name: "Restart" })).toBeInTheDocument();
    fireEvent.click(within(image).getByRole("button", { name: "Restart" }));
    await waitFor(() => expect(mocks.restart).toHaveBeenCalledWith(workshop.id, "image"));
  });

  it("validates the path on blur, shows the manifest, then Add and run creates it", async () => {
    mocks.list.mockResolvedValue([]);
    mocks.inspect.mockResolvedValue({
      id: "com.example.hello",
      namespace: "hello",
      name: "Hello",
      version: "0.1.0",
      description: ""
    });
    mocks.add.mockResolvedValue({ plugin: plugin(), tools: 2 });
    draw();

    fireEvent.click(screen.getByRole("button", { name: "Add plugin" }));
    const dialog = await screen.findByRole("dialog", { name: "Add plugin" });
    expect(
      within(dialog).getByText(
        "This runs the plugin's code with this machine's privileges. Install only code you trust."
      )
    ).toBeInTheDocument();
    const add = within(dialog).getByRole("button", { name: "Add and run" });
    expect(add).toBeDisabled();

    const field = within(dialog).getByLabelText("Plugin directory");
    fireEvent.change(field, { target: { value: "C:\\plugins\\hello" } });
    fireEvent.blur(field);

    expect(await within(dialog).findByText("hello · v0.1.0 · runs with this machine's privileges")).toBeInTheDocument();
    expect(mocks.inspect).toHaveBeenCalledWith(workshop.id, "C:\\plugins\\hello");
    expect(add).toBeEnabled();
    fireEvent.click(add);

    await waitFor(() => expect(mocks.add).toHaveBeenCalledWith(workshop.id, "C:\\plugins\\hello"));
    expect(await screen.findByText("Hello added with 2 tools.")).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "Add plugin" })).not.toBeInTheDocument();
    await waitFor(() => expect(mocks.list).toHaveBeenCalledTimes(2));
  });

  it("reads a pasted path without waiting for blur", async () => {
    mocks.list.mockResolvedValue([]);
    mocks.inspect.mockResolvedValue({
      id: "com.example.hello",
      namespace: "hello",
      name: "Hello",
      version: "0.1.0",
      description: ""
    });
    draw();

    fireEvent.click(screen.getByRole("button", { name: "Add plugin" }));
    const dialog = await screen.findByRole("dialog", { name: "Add plugin" });
    const field = within(dialog).getByLabelText("Plugin directory");
    fireEvent.change(field, { target: { value: "C:\\plugins\\hello" } });

    // No blur: the disabled Add button was the old dead end.
    expect(await within(dialog).findByText("hello · v0.1.0 · runs with this machine's privileges")).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Add and run" })).toBeEnabled();
  });

  it("shows a validation failure and keeps Add and run disabled", async () => {
    mocks.inspect.mockRejectedValue(
      new Error(
        JSON.stringify({
          code: "NO_MANIFEST",
          message: "No plugin.json in that folder.",
          action: "Open the guide"
        })
      )
    );
    draw();
    fireEvent.click(screen.getByRole("button", { name: "Add plugin" }));
    const dialog = await screen.findByRole("dialog", { name: "Add plugin" });
    const field = within(dialog).getByLabelText("Plugin directory");
    fireEvent.change(field, { target: { value: "C:\\plugins\\empty" } });
    fireEvent.blur(field);

    expect(await within(dialog).findByText("No plugin.json in that folder.")).toBeInTheDocument();
    expect(within(dialog).getByRole("link", { name: "Open the guide" })).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Add and run" })).toBeDisabled();
  });

  it("keeps the sheet open with the device's refusal when Add and run fails", async () => {
    mocks.inspect.mockResolvedValue({
      id: "com.example.hello",
      namespace: "hello",
      name: "Hello",
      version: "0.1.0",
      description: ""
    });
    mocks.add.mockRejectedValue(
      new Error(
        JSON.stringify({
          code: "NAMESPACE_TAKEN",
          message: "namespace hello is taken by a built-in capability.",
          action: "plugins.list"
        })
      )
    );
    draw();

    fireEvent.click(screen.getByRole("button", { name: "Add plugin" }));
    const dialog = await screen.findByRole("dialog", { name: "Add plugin" });
    const field = within(dialog).getByLabelText("Plugin directory");
    fireEvent.change(field, { target: { value: "C:\\plugins\\hello" } });
    fireEvent.blur(field);
    await within(dialog).findByText("hello · v0.1.0 · runs with this machine's privileges");
    fireEvent.click(within(dialog).getByRole("button", { name: "Add and run" }));

    expect(
      await within(dialog).findByText("namespace hello is taken by a built-in capability.")
    ).toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Add plugin" })).toBeInTheDocument();
  });

  it("confirms a remove by naming the plugin, its tool calls and the source directory", async () => {
    mocks.list.mockResolvedValue([plugin()]);
    mocks.remove.mockResolvedValue(undefined);
    draw();
    await screen.findByText("Hello");

    openActions("Hello");
    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    const dialog = await screen.findByRole("dialog", { name: "Remove Hello?" });
    expect(within(dialog).getByText(/Calls to Hello's tools will fail/)).toBeInTheDocument();
    expect(within(dialog).getByText(/source directory stays where it is/)).toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole("button", { name: "Remove" }));
    await waitFor(() => expect(mocks.remove).toHaveBeenCalledWith(workshop.id, "hello"));
    await waitFor(() => expect(mocks.list).toHaveBeenCalledTimes(2));
  });

  it("loads the last log lines on open and offers Copy, without a Refresh for a running plugin", async () => {
    mocks.list.mockResolvedValue([plugin()]);
    mocks.logs.mockResolvedValue(["started hello", "listening"]);
    draw();
    await screen.findByText("Hello");

    openActions("Hello");
    fireEvent.click(screen.getByRole("button", { name: "Logs" }));
    const dialog = await screen.findByRole("dialog", { name: "Logs · Hello" });
    expect(await within(dialog).findByText(/started hello/)).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Copy" })).toBeInTheDocument();
    expect(within(dialog).queryByRole("button", { name: "Refresh" })).not.toBeInTheDocument();
    expect(mocks.logs).toHaveBeenCalledWith(workshop.id, "hello", 200);
  });

  it("says No output yet for an empty log, and offers Refresh only while Starting", async () => {
    mocks.list.mockResolvedValue([plugin({ state: "starting" })]);
    draw();
    await screen.findByText("Hello");

    openActions("Hello");
    fireEvent.click(screen.getByRole("button", { name: "Logs" }));
    const dialog = await screen.findByRole("dialog", { name: "Logs · Hello" });

    expect(await within(dialog).findByText("No output yet.")).toBeInTheDocument();
    expect(
      within(dialog).getByText("The plugin has not written anything since it started.")
    ).toBeInTheDocument();
    const refresh = within(dialog).getByRole("button", { name: "Refresh" });
    fireEvent.click(refresh);
    await waitFor(() => expect(mocks.logs).toHaveBeenCalledTimes(2));
  });
});
