import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useLayoutEffect, useRef } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Device, LiveTool } from "./api";
import type { PluginRecord } from "./pluginApi";
import { PluginList } from "./PluginList";
import type { LiveToolsState } from "./useLiveTools";

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

function state(tools: LiveTool[] = [supportTool()], overrides: Partial<LiveToolsState> = {}): LiveToolsState {
  return {
    tools,
    stale: false,
    error: null,
    offline: false,
    loading: false,
    retrying: false,
    retried: null,
    retry: vi.fn(),
    ...overrides
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

function draw(
  {
    live = state(),
    offline = false,
    device = workshop,
    onMachine = vi.fn()
  }: {
    live?: LiveToolsState;
    offline?: boolean;
    device?: Device;
    onMachine?: () => void;
  } = {}
) {
  const view = render(
    <PluginList device={device} live={live} offline={offline} onMachine={onMachine} />
  );
  return { ...view, onMachine };
}

function openActions(name: string) {
  const row = screen.getByText(name).closest("article") as HTMLElement;
  fireEvent.click(within(row).getByTitle(`Actions for ${name}`));
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

const studio: Device = { ...workshop, id: "dev-studio", name: "studio" };
const workshopPlugin = plugin({ name: "Workshop plugin", namespace: "workshop", state: "failed" });
const studioPlugin = plugin({ name: "Studio plugin", namespace: "studio", state: "failed" });

function listEachMachine() {
  mocks.list.mockImplementation(async (deviceId: string) =>
    deviceId === workshop.id ? [workshopPlugin] : [studioPlugin]
  );
}

function selectMachine(view: ReturnType<typeof draw>, device: Device) {
  view.rerender(<PluginList device={device} live={state()} offline={false} onMachine={vi.fn()} />);
}

function restartRow(name: string) {
  const row = screen.getByText(name).closest("article") as HTMLElement;
  fireEvent.click(within(row).getByRole("button", { name: "Restart" }));
}

function openRemove(name: string) {
  openActions(name);
  const row = screen.getByText(name).closest("article") as HTMLElement;
  fireEvent.click(within(row).getByRole("button", { name: "Remove", hidden: true }));
}

function CommitObserver({ device, observe }: { device: Device; observe: (host: HTMLDivElement) => void }) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (device.id === studio.id && host.current) observe(host.current);
  }, [device.id, observe]);
  return (
    <div ref={host}>
      <PluginList device={device} live={state()} offline={false} onMachine={vi.fn()} />
    </div>
  );
}

beforeEach(() => {
  mocks.list.mockReset();
  mocks.inspect.mockReset();
  mocks.add.mockReset();
  mocks.remove.mockReset();
  mocks.restart.mockReset();
  mocks.logs.mockReset();
  mocks.list.mockResolvedValue([]);
  mocks.logs.mockResolvedValue([]);
});

describe("PluginList", () => {
  it("puts Add plugin inside the empty state, as the one action, and no manifest example", async () => {
    draw();
    expect(await screen.findByText("No plugins yet. Add one when a built-in cannot do the job.")).toBeInTheDocument();
    expect(screen.queryByText(/plugin\.json/)).not.toBeInTheDocument();
    expect(screen.queryByText(/manifest/i)).not.toBeInTheDocument();

    const add = screen.getByRole("button", { name: "Add plugin" });
    expect(add.closest(".empty-state")).not.toBeNull();
    fireEvent.click(add);
    expect(screen.getByRole("dialog", { name: "Add plugin" })).toBeInTheDocument();
  });

  it("renders an older build as a sentence with Open Machine, and shows no Add button", () => {
    const { onMachine } = draw({ live: state([supportTool("terminal.list")]) });

    expect(screen.getByText("This machine needs a newer Summrise to host plugins.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add plugin" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open Machine" }));
    expect(onMachine).toHaveBeenCalledTimes(1);
    expect(mocks.list).not.toHaveBeenCalled();
  });

  it("does not repeat an offline machine's state inside the plugin section", () => {
    draw({
      live: state([], {
        error: "this machine is not connected; connect it, then try again",
        offline: true
      }),
      offline: true,
      device: { ...workshop, online: false }
    });

    expect(screen.getByText("The console has no plugin list from this machine yet.")).toBeInTheDocument();
    expect(screen.queryByText(/not connected/)).toBeNull();
    expect(screen.queryByText("No plugins yet.")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add plugin" })).not.toBeInTheDocument();
    // The Retry belongs to the tools read, above this section; a second one would be the same read
    // asked twice.
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
  });

  it("inherits a failed tools read as one sentence with no second Retry", () => {
    draw({
      live: state([], {
        error: "the console could not read this machine's tools"
      })
    });

    // The plugin section points at the tools notice rather than restating the failure; the one
    // Retry is up there, and this section adds no second one (audit-43 F9).
    expect(screen.getByText("Plugins need the tool list; Retry above.")).toBeInTheDocument();
    expect(
      screen.queryByText("Plugins are unavailable until the console can read the tool list.")
    ).toBeNull();
    expect(screen.queryByText(/The console could not read this machine's tools/)).toBeNull();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    expect(
      screen.queryByText("No plugins yet. Add one when a built-in cannot do the job.")
    ).toBeNull();
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

  describe("restart namespace ownership", () => {
    function restartButton(name: string) {
      const row = screen.getByText(name).closest("article") as HTMLElement;
      return within(row).getByRole("button", { name: /^Restart/ });
    }

    it("keeps each parallel restart busy until its own follow-up list finishes", async () => {
      const first = deferred<PluginRecord>();
      const second = deferred<PluginRecord>();
      const firstRead = deferred<PluginRecord[]>();
      const secondRead = deferred<PluginRecord[]>();
      mocks.list.mockResolvedValueOnce([workshopPlugin, studioPlugin])
        .mockReturnValueOnce(firstRead.promise).mockReturnValueOnce(secondRead.promise);
      mocks.restart.mockImplementation((_deviceId: string, namespace: string) =>
        namespace === "workshop" ? first.promise : second.promise
      );
      draw();
      await screen.findByText("Workshop plugin");
      restartRow("Workshop plugin");
      expect(restartButton("Studio plugin")).toBeEnabled();
      restartRow("Studio plugin");

      expect({ first: restartButton("Workshop plugin").hasAttribute("disabled"), second: restartButton("Studio plugin").hasAttribute("disabled") })
        .toEqual({ first: true, second: true });
      expect(mocks.restart.mock.calls).toEqual([["dev-workshop", "workshop"], ["dev-workshop", "studio"]]);
      await act(async () => first.resolve(workshopPlugin));
      expect(restartButton("Workshop plugin")).toBeDisabled();
      expect(restartButton("Studio plugin")).toBeDisabled();
      await act(async () => firstRead.resolve([workshopPlugin, studioPlugin]));
      expect(restartButton("Workshop plugin")).toBeEnabled();
      expect(restartButton("Studio plugin")).toBeDisabled();
      await act(async () => second.resolve(studioPlugin));
      expect(restartButton("Studio plugin")).toBeDisabled();
      await act(async () => secondRead.resolve([workshopPlugin, studioPlugin]));
      expect(restartButton("Studio plugin")).toBeEnabled();
      expect(mocks.list.mock.calls).toEqual([["dev-workshop"], ["dev-workshop"], ["dev-workshop"]]);
    });

    it("does not release B when A completes while B is pending", async () => {
      const first = deferred<PluginRecord>();
      const second = deferred<PluginRecord>();
      mocks.list.mockResolvedValue([workshopPlugin, studioPlugin]);
      mocks.restart.mockImplementation((_deviceId: string, namespace: string) =>
        namespace === "workshop" ? first.promise : second.promise
      );
      draw();
      await screen.findByText("Workshop plugin");
      restartRow("Workshop plugin");
      restartRow("Studio plugin");

      await act(async () => first.resolve(workshopPlugin));

      expect(restartButton("Studio plugin")).toBeDisabled();
      expect(restartButton("Workshop plugin")).toBeEnabled();
      await act(async () => second.reject(new Error("Studio restart refused")));
      expect(restartButton("Studio plugin")).toBeEnabled();
      expect(screen.getByText("Studio restart refused")).toBeInTheDocument();
      expect(mocks.restart.mock.calls).toEqual([["dev-workshop", "workshop"], ["dev-workshop", "studio"]]);
    });

    it("admits one namespace only once from two enabled clicks in the same commit", async () => {
      const pending = deferred<PluginRecord>();
      mocks.list.mockResolvedValue([workshopPlugin]);
      mocks.restart.mockReturnValue(pending.promise);
      const enabledAtDispatch: boolean[] = [];
      function RestartBurst({ go }: { go: boolean }) {
        const host = useRef<HTMLDivElement>(null);
        useLayoutEffect(() => {
          if (!go || !host.current) return;
          const button = host.current.querySelector<HTMLButtonElement>("article button")!;
          for (let press = 0; press < 2; press += 1) {
            enabledAtDispatch.push(!button.disabled);
            if (!button.disabled) button.click();
          }
        }, [go]);
        return <div ref={host}><PluginList device={workshop} live={state()} offline={false} onMachine={vi.fn()} /></div>;
      }
      const view = render(<RestartBurst go={false} />);
      await screen.findByText("Workshop plugin");

      view.rerender(<RestartBurst go />);

      expect(enabledAtDispatch).toEqual([true, true]);
      expect(mocks.restart.mock.calls).toEqual([["dev-workshop", "workshop"]]);
      expect(restartButton("Workshop plugin")).toBeDisabled();
      await act(async () => pending.resolve(workshopPlugin));
      expect(restartButton("Workshop plugin")).toBeEnabled();
      expect(mocks.list.mock.calls).toEqual([["dev-workshop"], ["dev-workshop"]]);
    });
  });

  it("keeps cached catalogue loading and activation distinct from an empty plugin list", async () => {
    const answer = deferred<PluginRecord[]>();
    mocks.list.mockReturnValue(answer.promise);
    const commits: { ready: boolean; reading: boolean; zero: boolean; empty: boolean }[] = [];
    function CachedCatalogue({ ready }: { ready: boolean }) {
      const host = useRef<HTMLDivElement>(null);
      useLayoutEffect(() => {
        if (!host.current) return;
        commits.push({
          ready,
          reading: within(host.current).queryByRole("status", { name: "Reading the plugin list" }) !== null,
          zero: within(host.current).queryByText("0 plugins") !== null,
          empty: within(host.current).queryByText("No plugins yet. Add one when a built-in cannot do the job.") !== null
        });
      }, [ready]);
      return (
        <div ref={host}>
          <PluginList device={studio} live={state([supportTool()], { loading: !ready })} offline={false} onMachine={vi.fn()} />
        </div>
      );
    }
    const view = render(<CachedCatalogue ready={false} />);
    expect(mocks.list).not.toHaveBeenCalled();

    view.rerender(<CachedCatalogue ready />);

    expect(commits).toEqual([
      { ready: false, reading: true, zero: false, empty: false },
      { ready: true, reading: true, zero: false, empty: false }
    ]);
    expect(screen.getByRole("status", { name: "Reading the plugin list" })).toBeInTheDocument();
    await act(async () => answer.resolve([]));
    expect(screen.getByText("0 plugins")).toBeInTheDocument();
    expect(screen.getByText("No plugins yet. Add one when a built-in cannot do the job.")).toBeInTheDocument();
    expect(mocks.list.mock.calls).toEqual([["dev-studio"]]);
  });

  it("keeps the first list unknown when the same machine's catalogue becomes ready", async () => {
    const answer = deferred<PluginRecord[]>();
    mocks.list.mockReturnValue(answer.promise);
    const commits: { reading: boolean; zero: boolean; empty: boolean }[] = [];
    function CatalogueActivation({ ready }: { ready: boolean }) {
      const host = useRef<HTMLDivElement>(null);
      useLayoutEffect(() => {
        if (!ready || !host.current) return;
        commits.push({
          reading: within(host.current).queryByRole("status", { name: "Reading the plugin list" }) !== null,
          zero: within(host.current).queryByText("0 plugins") !== null,
          empty: within(host.current).queryByText("No plugins yet. Add one when a built-in cannot do the job.") !== null
        });
      }, [ready]);
      return (
        <div ref={host}>
          <PluginList device={studio} live={ready ? state() : state([], { loading: true })} offline={false} onMachine={vi.fn()} />
        </div>
      );
    }
    const view = render(<CatalogueActivation ready={false} />);
    expect(mocks.list).not.toHaveBeenCalled();

    view.rerender(<CatalogueActivation ready />);

    expect(commits).toEqual([{ reading: true, zero: false, empty: false }]);
    expect(screen.getByRole("status", { name: "Reading the plugin list" })).toBeInTheDocument();
    await act(async () => answer.resolve([]));
    expect(screen.getByText("0 plugins")).toBeInTheDocument();
    expect(screen.getByText("No plugins yet. Add one when a built-in cannot do the job.")).toBeInTheDocument();
    expect(mocks.list.mock.calls).toEqual([["dev-studio"]]);
  });

  it("does not count an unknown or refused first list as zero, then accepts a real empty Retry", async () => {
    const first = deferred<PluginRecord[]>();
    const retry = deferred<PluginRecord[]>();
    mocks.list.mockReturnValueOnce(first.promise).mockReturnValueOnce(retry.promise);
    const initial: { reading: boolean; zero: boolean }[] = [];
    const observe = (host: HTMLDivElement) => initial.push({
      reading: within(host).queryByRole("status", { name: "Reading the plugin list" }) !== null,
      zero: within(host).queryByText("0 plugins") !== null
    });
    render(<CommitObserver device={studio} observe={observe} />);
    await act(async () => first.reject(new Error("Studio list refused")));

    expect(screen.getByText("Studio list refused")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeEnabled();
    expect(screen.queryByText("0 plugins")).not.toBeInTheDocument();
    expect(screen.queryByText("No plugins yet. Add one when a built-in cannot do the job.")).not.toBeInTheDocument();
    expect(initial).toEqual([{ reading: true, zero: false }]);
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(screen.getByRole("status", { name: "Reading the plugin list" })).toBeInTheDocument();
    expect(screen.queryByText("0 plugins")).not.toBeInTheDocument();
    await act(async () => retry.resolve([]));
    expect(screen.getByText("0 plugins")).toBeInTheDocument();
    expect(screen.getByText("No plugins yet. Add one when a built-in cannot do the job.")).toBeInTheDocument();
    expect(screen.queryByText("Studio list refused")).not.toBeInTheDocument();
    expect(mocks.list.mock.calls).toEqual([["dev-studio"], ["dev-studio"]]);
  });

  describe("manifest inspection ownership", () => {
    it("does not validate a cleared path when its previous inspection finishes", async () => {
      const inspected = deferred<PluginRecord>();
      mocks.inspect.mockReturnValue(inspected.promise);
      draw();
      await screen.findByText("No plugins yet. Add one when a built-in cannot do the job.");
      vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
      try {
        fireEvent.click(screen.getByRole("button", { name: "Add plugin" }));
        const dialog = screen.getByRole("dialog", { name: "Add plugin" });
        const field = within(dialog).getByLabelText("Plugin directory");
        const add = within(dialog).getByRole("button", { name: "Add and run" }) as HTMLButtonElement;
        fireEvent.change(field, { target: { value: "C:\\plugins\\workshop" } });
        fireEvent.blur(field);
        expect(mocks.inspect.mock.calls).toEqual([["dev-workshop", "C:\\plugins\\workshop"]]);
        fireEvent.change(field, { target: { value: "" } });
        fireEvent.blur(field);

        await act(async () => inspected.resolve(workshopPlugin));

        expect({
          valid: within(dialog).queryByText("workshop · v0.1.0 · runs with this machine's privileges") !== null,
          enabled: !add.disabled
        }).toEqual({ valid: false, enabled: false });
        expect(field).toHaveValue("");
        expect(mocks.add).not.toHaveBeenCalled();
        expect(mocks.inspect.mock.calls).toEqual([["dev-workshop", "C:\\plugins\\workshop"]]);
      } finally {
        vi.useRealTimers();
      }
    });

    it("waits for the changed path's own inspection before adding that trimmed path", async () => {
      const first = deferred<PluginRecord>();
      const second = deferred<PluginRecord>();
      mocks.inspect.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
      mocks.add.mockResolvedValue({ plugin: studioPlugin, tools: 2 });
      draw();
      await screen.findByText("No plugins yet. Add one when a built-in cannot do the job.");
      vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
      try {
        fireEvent.click(screen.getByRole("button", { name: "Add plugin" }));
        const dialog = screen.getByRole("dialog", { name: "Add plugin" });
        const field = within(dialog).getByLabelText("Plugin directory");
        const add = within(dialog).getByRole("button", { name: "Add and run" }) as HTMLButtonElement;
        fireEvent.change(field, { target: { value: "C:\\plugins\\workshop" } });
        fireEvent.blur(field);
        fireEvent.change(field, { target: { value: "  C:\\plugins\\studio  " } });

        // The new path's debounce has not run: A's reply cannot stand in for B's manifest.
        await act(async () => first.resolve(workshopPlugin));

        expect({
          valid: within(dialog).queryByText("workshop · v0.1.0 · runs with this machine's privileges") !== null,
          enabled: !add.disabled
        }).toEqual({ valid: false, enabled: false });
        expect(mocks.add).not.toHaveBeenCalled();
        expect(mocks.inspect.mock.calls).toEqual([["dev-workshop", "C:\\plugins\\workshop"]]);
        await act(async () => { vi.advanceTimersByTime(300); });
        expect(mocks.inspect.mock.calls).toEqual([
          ["dev-workshop", "C:\\plugins\\workshop"], ["dev-workshop", "C:\\plugins\\studio"]
        ]);
        expect(add).toBeDisabled();
        await act(async () => second.resolve(studioPlugin));
        expect(within(dialog).getByText("studio · v0.1.0 · runs with this machine's privileges")).toBeInTheDocument();
        expect(add).toBeEnabled();
        await act(async () => { fireEvent.click(add); });
        expect(mocks.add.mock.calls).toEqual([["dev-workshop", "C:\\plugins\\studio"]]);
        expect(screen.getByText("Studio plugin added with 2 tools.")).toBeInTheDocument();
      } finally {
        vi.useRealTimers();
      }
    });
  });

  describe("device row admission before passive effects", () => {
    it("exposes no A row or B action with A's namespace in B's first commit", async () => {
      const answer = deferred<PluginRecord[]>();
      const restarted = deferred<PluginRecord>();
      mocks.list.mockImplementation((deviceId: string) =>
        deviceId === workshop.id ? Promise.resolve([workshopPlugin]) : answer.promise
      );
      mocks.restart.mockReturnValue(restarted.promise);
      const commits: { rows: string[]; reading: boolean; zero: boolean; restarts: unknown[][] }[] = [];
      const observe = (host: HTMLDivElement) => {
        const rows = Array.from(host.querySelectorAll<HTMLElement>(".plugin-name"), (row) => row.textContent ?? "");
        const oldRow = host.querySelector("article");
        const button = oldRow?.querySelector<HTMLButtonElement>("button");
        // Native click in the parent layout effect, before the child's passive list reset.
        if (button && !button.disabled) button.click();
        commits.push({
          rows,
          reading: within(host).queryByRole("status", { name: "Reading the plugin list" }) !== null,
          zero: within(host).queryByText("0 plugins") !== null,
          restarts: mocks.restart.mock.calls.map((call) => [...call])
        });
      };
      const view = render(<CommitObserver device={workshop} observe={observe} />);
      await screen.findByText("Workshop plugin");

      view.rerender(<CommitObserver device={studio} observe={observe} />);

      expect(commits).toEqual([{ rows: [], reading: true, zero: false, restarts: [] }]);
      expect(screen.queryByText("Workshop plugin")).not.toBeInTheDocument();
      expect(screen.getByRole("status", { name: "Reading the plugin list" })).toBeInTheDocument();
      await act(async () => answer.resolve([studioPlugin]));
      expect(await screen.findByText("Studio plugin")).toBeInTheDocument();
      restartRow("Studio plugin");
      expect(mocks.restart.mock.calls).toEqual([["dev-studio", "studio"]]);
      await act(async () => restarted.resolve(studioPlugin));
      expect(mocks.list.mock.calls).toEqual([["dev-workshop"], ["dev-studio"], ["dev-studio"]]);
    });

    it("shows reading rather than A's true zero until B returns its own zero", async () => {
      const answer = deferred<PluginRecord[]>();
      mocks.list.mockImplementation((deviceId: string) =>
        deviceId === workshop.id ? Promise.resolve([]) : answer.promise
      );
      const commits: { reading: boolean; empty: boolean; zero: boolean }[] = [];
      const observe = (host: HTMLDivElement) => commits.push({
        reading: within(host).queryByRole("status", { name: "Reading the plugin list" }) !== null,
        empty: within(host).queryByText("No plugins yet. Add one when a built-in cannot do the job.") !== null,
        zero: within(host).queryByText("0 plugins") !== null
      });
      const view = render(<CommitObserver device={workshop} observe={observe} />);
      await screen.findByText("No plugins yet. Add one when a built-in cannot do the job.");
      expect(screen.getByText("0 plugins")).toBeInTheDocument();

      view.rerender(<CommitObserver device={studio} observe={observe} />);

      expect(commits).toEqual([{ reading: true, empty: false, zero: false }]);
      await act(async () => answer.resolve([]));
      expect(await screen.findByText("No plugins yet. Add one when a built-in cannot do the job.")).toBeInTheDocument();
      expect(screen.getByText("0 plugins")).toBeInTheDocument();
      expect(screen.queryByRole("status", { name: "Reading the plugin list" })).not.toBeInTheDocument();
    });

    it("masks A's failure at B commit and keeps B's own refusal and Retry", async () => {
      const answer = deferred<PluginRecord[]>();
      mocks.list.mockImplementation((deviceId: string) =>
        deviceId === workshop.id ? Promise.reject(new Error("Workshop list refused")) : answer.promise
      );
      const commits: { reading: boolean; oldError: boolean; retry: boolean }[] = [];
      const observe = (host: HTMLDivElement) => commits.push({
        reading: within(host).queryByRole("status", { name: "Reading the plugin list" }) !== null,
        oldError: within(host).queryByText("Workshop list refused") !== null,
        retry: within(host).queryByRole("button", { name: "Retry" }) !== null
      });
      const view = render(<CommitObserver device={workshop} observe={observe} />);
      await screen.findByText("Workshop list refused");

      view.rerender(<CommitObserver device={studio} observe={observe} />);

      expect(commits).toEqual([{ reading: true, oldError: false, retry: false }]);
      await act(async () => answer.reject(new Error("Studio list refused")));
      expect(await screen.findByText("Studio list refused")).toBeInTheDocument();
      expect(screen.queryByText("Workshop list refused")).not.toBeInTheDocument();
      expect(screen.queryByText("No plugins yet. Add one when a built-in cannot do the job.")).not.toBeInTheDocument();
      mocks.list.mockResolvedValue([studioPlugin]);
      fireEvent.click(screen.getByRole("button", { name: "Retry" }));
      expect(await screen.findByText("Studio plugin")).toBeInTheDocument();
      expect(mocks.list.mock.calls).toEqual([["dev-workshop"], ["dev-studio"], ["dev-studio"]]);
    });

    it("keeps the current machine's rows while its restart refresh is pending", async () => {
      const refresh = deferred<PluginRecord[]>();
      mocks.list.mockResolvedValueOnce([studioPlugin]).mockReturnValueOnce(refresh.promise);
      mocks.restart.mockResolvedValue(studioPlugin);
      draw({ device: studio });
      await screen.findByText("Studio plugin");

      restartRow("Studio plugin");
      await waitFor(() => expect(mocks.list).toHaveBeenCalledTimes(2));

      expect(screen.getByText("Studio plugin")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Restarting…" })).toBeDisabled();
      expect(screen.queryByRole("status", { name: "Reading the plugin list" })).not.toBeInTheDocument();
      await act(async () => refresh.resolve([studioPlugin]));
      expect(screen.getByRole("button", { name: "Restart" })).toBeEnabled();
      expect(mocks.list.mock.calls).toEqual([["dev-studio"], ["dev-studio"]]);
    });
  });

  describe("device completion ownership", () => {
    it("does not reload A or replace B's rows when A's restart succeeds late", async () => {
      listEachMachine();
      const pending = deferred<PluginRecord>();
      mocks.restart.mockReturnValue(pending.promise);
      const view = draw();
      await screen.findByText("Workshop plugin");
      restartRow("Workshop plugin");
      expect(mocks.restart).toHaveBeenCalledWith("dev-workshop", "workshop");

      selectMachine(view, studio);
      await screen.findByText("Studio plugin");
      await act(async () => pending.resolve(workshopPlugin));

      expect(mocks.list.mock.calls).toEqual([["dev-workshop"], ["dev-studio"]]);
      expect(screen.queryByText("Workshop plugin")).not.toBeInTheDocument();
      expect(screen.getByText("Studio plugin")).toBeInTheDocument();
      openRemove("Studio plugin");
      const dialog = screen.getByRole("dialog", { name: "Remove Studio plugin?" });
      fireEvent.click(within(dialog).getByRole("button", { name: "Remove" }));
      await waitFor(() => expect(mocks.remove).toHaveBeenCalledWith("dev-studio", "studio"));
    });

    it("does not show A's late restart refusal on B", async () => {
      listEachMachine();
      const pending = deferred<PluginRecord>();
      mocks.restart.mockReturnValue(pending.promise);
      const view = draw();
      await screen.findByText("Workshop plugin");
      restartRow("Workshop plugin");
      selectMachine(view, studio);
      await screen.findByText("Studio plugin");

      await act(async () => pending.reject(new Error("Workshop restart refused")));

      expect(screen.queryByText("Workshop restart refused")).not.toBeInTheDocument();
      expect(screen.getByText("Studio plugin")).toBeInTheDocument();
      expect(mocks.list.mock.calls).toEqual([["dev-workshop"], ["dev-studio"]]);
    });

    it("does not inherit A's busy state for the same namespace on B", async () => {
      const pending = deferred<PluginRecord>();
      mocks.list.mockImplementation(async (deviceId: string) => [
        plugin({ name: deviceId === workshop.id ? "Workshop plugin" : "Studio plugin", state: "failed" })
      ]);
      mocks.restart.mockReturnValue(pending.promise);
      const view = draw();
      await screen.findByText("Workshop plugin");
      restartRow("Workshop plugin");

      selectMachine(view, studio);
      await screen.findByText("Studio plugin");

      const row = screen.getByText("Studio plugin").closest("article") as HTMLElement;
      expect(within(row).getByRole("button", { name: /^Restart/ })).toBeEnabled();
      await act(async () => pending.resolve(plugin()));
    });

    it.each(["success", "refusal"] as const)(
      "keeps B's newer restart busy when A settles with %s",
      async (outcome) => {
        const first = deferred<PluginRecord>();
        const second = deferred<PluginRecord>();
        // Both machines have these plugins, so a stale A reload cannot hide the B busy assertion.
        mocks.list.mockResolvedValue([workshopPlugin, studioPlugin]);
        mocks.restart.mockImplementation((deviceId: string) =>
          deviceId === workshop.id ? first.promise : second.promise
        );
        const view = draw();
        await screen.findByText("Workshop plugin");
        restartRow("Workshop plugin");
        selectMachine(view, studio);
        await screen.findByText("Studio plugin");
        await waitFor(() => expect(mocks.list).toHaveBeenCalledTimes(2));
        restartRow("Studio plugin");
        expect(mocks.restart.mock.calls).toEqual([["dev-workshop", "workshop"], ["dev-studio", "studio"]]);

        await act(async () => {
          if (outcome === "success") first.resolve(plugin());
          else first.reject(new Error("Workshop restart refused"));
        });

        const row = screen.getByText("Studio plugin").closest("article") as HTMLElement;
        expect(within(row).getByRole("button", { name: /^Restart/ })).toBeDisabled();
        expect(screen.queryByText("Workshop restart refused")).not.toBeInTheDocument();
        expect(mocks.list.mock.calls).toEqual([["dev-workshop"], ["dev-studio"]]);
        await act(async () => second.resolve(plugin()));
        expect(await within(row).findByRole("button", { name: "Restart" })).toBeEnabled();
        expect(mocks.list.mock.calls).toEqual([["dev-workshop"], ["dev-studio"], ["dev-studio"]]);
      }
    );

    it("invalidates an old restart even after selecting A again", async () => {
      listEachMachine();
      const pending = deferred<PluginRecord>();
      mocks.restart.mockReturnValue(pending.promise);
      const view = draw();
      await screen.findByText("Workshop plugin");
      restartRow("Workshop plugin");
      selectMachine(view, studio);
      await screen.findByText("Studio plugin");
      selectMachine(view, workshop);
      await screen.findByText("Workshop plugin");

      await act(async () => pending.resolve(workshopPlugin));

      expect(mocks.list.mock.calls).toEqual([["dev-workshop"], ["dev-studio"], ["dev-workshop"]]);
      expect(screen.getByRole("button", { name: "Restart" })).toBeEnabled();
    });

    it("does not start a follow-up read after an old PluginList unmounts", async () => {
      listEachMachine();
      const pending = deferred<PluginRecord>();
      mocks.restart.mockReturnValue(pending.promise);
      const view = draw();
      await screen.findByText("Workshop plugin");
      restartRow("Workshop plugin");
      view.unmount();

      await act(async () => pending.resolve(workshopPlugin));

      expect(mocks.list.mock.calls).toEqual([["dev-workshop"]]);
    });

    it("clears A's completed action notice when selecting B", async () => {
      listEachMachine();
      mocks.restart.mockRejectedValue(new Error("Workshop restart refused"));
      const view = draw();
      await screen.findByText("Workshop plugin");
      restartRow("Workshop plugin");
      await screen.findByText("Workshop restart refused");

      selectMachine(view, studio);
      await screen.findByText("Studio plugin");

      expect(screen.queryByText("Workshop restart refused")).not.toBeInTheDocument();
    });

    it("drops A's removal confirmation before B can use its namespace", async () => {
      listEachMachine();
      const view = draw();
      await screen.findByText("Workshop plugin");
      openRemove("Workshop plugin");
      expect(screen.getByRole("dialog", { name: "Remove Workshop plugin?" })).toBeInTheDocument();

      selectMachine(view, studio);
      await screen.findByText("Studio plugin");

      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      expect(mocks.remove).not.toHaveBeenCalled();
      openRemove("Studio plugin");
      const dialog = screen.getByRole("dialog", { name: "Remove Studio plugin?" });
      fireEvent.click(within(dialog).getByRole("button", { name: "Remove" }));
      await waitFor(() => expect(mocks.remove).toHaveBeenCalledWith("dev-studio", "studio"));
    });

    it("drops A's logs without reading A's namespace from B", async () => {
      listEachMachine();
      const pending = deferred<string[]>();
      mocks.logs.mockImplementation((deviceId: string) =>
        deviceId === workshop.id ? pending.promise : Promise.resolve(["Studio output"])
      );
      const view = draw();
      await screen.findByText("Workshop plugin");
      openActions("Workshop plugin");
      fireEvent.click(screen.getByRole("button", { name: "Logs" }));
      expect(mocks.logs).toHaveBeenCalledWith("dev-workshop", "workshop", 200);

      selectMachine(view, studio);
      await screen.findByText("Studio plugin");
      await act(async () => pending.resolve(["Workshop output"]));

      expect(mocks.logs.mock.calls).toEqual([["dev-workshop", "workshop", 200]]);
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      expect(screen.queryByText("Workshop output")).not.toBeInTheDocument();
      openActions("Studio plugin");
      fireEvent.click(screen.getByRole("button", { name: "Logs" }));
      expect(await screen.findByText("Studio output")).toBeInTheDocument();
      expect(mocks.logs).toHaveBeenLastCalledWith("dev-studio", "studio", 200);
    });

    it("drops A's checked add form when selecting B", async () => {
      listEachMachine();
      const pending = deferred<{ id: string; namespace: string; name: string; version: string; description: string }>();
      mocks.inspect.mockReturnValue(pending.promise);
      const view = draw();
      await screen.findByText("Workshop plugin");
      fireEvent.click(screen.getByRole("button", { name: "Add plugin" }));
      const dialog = screen.getByRole("dialog", { name: "Add plugin" });
      const field = within(dialog).getByLabelText("Plugin directory");
      fireEvent.change(field, { target: { value: "C:\\plugins\\workshop" } });
      fireEvent.blur(field);
      expect(mocks.inspect).toHaveBeenCalledWith("dev-workshop", "C:\\plugins\\workshop");

      selectMachine(view, studio);
      await screen.findByText("Studio plugin");
      await act(async () => pending.resolve(workshopPlugin));

      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "Add plugin" }));
      const next = screen.getByRole("dialog", { name: "Add plugin" });
      expect(within(next).getByLabelText("Plugin directory")).toHaveValue("");
      expect(within(next).getByRole("button", { name: "Add and run" })).toBeDisabled();
      expect(mocks.add).not.toHaveBeenCalled();
    });

    it.each(["success", "refusal"] as const)(
      "keeps B's add form when A's pending add settles with %s",
      async (outcome) => {
        listEachMachine();
        mocks.inspect.mockResolvedValue(workshopPlugin);
        const pending = deferred<{ plugin: PluginRecord; tools: number }>();
        mocks.add.mockReturnValue(pending.promise);
        const view = draw();
        await screen.findByText("Workshop plugin");
        fireEvent.click(screen.getByRole("button", { name: "Add plugin" }));
        const dialog = screen.getByRole("dialog", { name: "Add plugin" });
        const field = within(dialog).getByLabelText("Plugin directory");
        fireEvent.change(field, { target: { value: "C:\\plugins\\workshop" } });
        fireEvent.blur(field);
        await within(dialog).findByText("workshop · v0.1.0 · runs with this machine's privileges");
        fireEvent.click(within(dialog).getByRole("button", { name: "Add and run" }));
        expect(mocks.add).toHaveBeenCalledWith("dev-workshop", "C:\\plugins\\workshop");
        selectMachine(view, studio);
        await screen.findByText("Studio plugin");
        fireEvent.click(within(view.container).getByRole("button", { name: "Add plugin", hidden: true }));

        await act(async () => {
          if (outcome === "success") pending.resolve({ plugin: workshopPlugin, tools: 2 });
          else pending.reject(new Error("Workshop add refused"));
        });

        expect(mocks.list.mock.calls).toEqual([["dev-workshop"], ["dev-studio"]]);
        const next = screen.getByRole("dialog", { name: "Add plugin" });
        expect(within(next).getByLabelText("Plugin directory")).toHaveValue("");
        expect(within(next).getByRole("button", { name: "Add and run" })).toBeDisabled();
        expect(screen.queryByText("Workshop add refused")).not.toBeInTheDocument();
        expect(screen.queryByText("Workshop plugin added with 2 tools.")).not.toBeInTheDocument();
      }
    );

    it.each(["success", "refusal"] as const)(
      "keeps B's confirmation when A's pending remove settles with %s",
      async (outcome) => {
        listEachMachine();
        const pending = deferred<void>();
        mocks.remove.mockReturnValue(pending.promise);
        const view = draw();
        await screen.findByText("Workshop plugin");
        openRemove("Workshop plugin");
        const dialog = screen.getByRole("dialog", { name: "Remove Workshop plugin?" });
        fireEvent.click(within(dialog).getByRole("button", { name: "Remove" }));
        expect(mocks.remove).toHaveBeenCalledWith("dev-workshop", "workshop");
        selectMachine(view, studio);
        await screen.findByText("Studio plugin");
        openRemove("Studio plugin");

        await act(async () => {
          if (outcome === "success") pending.resolve(undefined);
          else pending.reject(new Error("Workshop remove refused"));
        });

        expect(mocks.list.mock.calls).toEqual([["dev-workshop"], ["dev-studio"]]);
        const next = screen.getByRole("dialog", { name: "Remove Studio plugin?" });
        expect(within(next).getByRole("button", { name: "Remove" })).toBeEnabled();
        expect(screen.queryByText("Workshop remove refused")).not.toBeInTheDocument();
        expect(screen.queryByText("Workshop plugin removed. Its tools are gone from this machine.")).not.toBeInTheDocument();
      }
    );
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
