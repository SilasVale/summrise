import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AuditLine, Device, LiveTool } from "./api";
import { HomePage } from "./HomePage";
import type { LiveLine } from "./liveActivity";

// The hub asks `/audit?device=` for its own machine's newest lines; the fleet window belongs to
// Activity. These tests drive the fetch the way the console answers it.
const auditApi = vi.hoisted(() => ({ audit: vi.fn() }));
vi.mock("./api", () => ({ api: { audit: auditApi.audit } }));

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

function liveCall(overrides: Partial<LiveLine> = {}): LiveLine {
  return {
    ts_ms: Date.now(),
    kind: "mcp",
    device_id: workshop.id,
    status: 200,
    sentence: "Did something",
    ...overrides
  };
}

function draw(overrides: Record<string, unknown> = {}) {
  const {
    audit,
    auditError = false,
    ...props
  } = overrides;
  const now = Date.now();
  const calls = [
    call({ ts_ms: now }),
    call({ tool: "files.read", ts_ms: now }),
    call({ tool: "screen.capture", ts_ms: now }),
    call({ tool: "runs.exec", ts_ms: now })
  ];
  if (auditError) auditApi.audit.mockRejectedValue(new Error("the audit could not be read"));
  else auditApi.audit.mockResolvedValue(audit ?? calls);
  const onConnect = vi.fn();
  const onCapabilities = vi.fn();
  const onActivity = vi.fn();
  const view = render(
    <HomePage
      device={workshop}
      devices={[workshop]}
      selectedId={workshop.id}
      code={null}
      enrollCommand=""
      busy={false}
      onOpen={vi.fn()}
      onRename={vi.fn()}
      onRemove={vi.fn()}
      onDiagnostics={vi.fn()}
      onAddMachine={vi.fn()}
      onDismissCode={vi.fn()}
      onNotice={vi.fn()}
      onConnect={onConnect}
      onCapabilities={onCapabilities}
      onActivity={onActivity}
      consoleVersion="0.3.999"
      {...props}
    />
  );
  return { ...view, onConnect, onCapabilities, onActivity };
}

beforeEach(() => {
  auditApi.audit.mockReset();
  live.value = { tools, stale: false, error: null, offline: false, loading: false, retry: vi.fn() };
  plugins.list.mockResolvedValue([
    { namespace: "hello" },
    { namespace: "image" }
  ]);
});

describe("Home — the hub", () => {
  it("states the machine in one status line, with counts and the machine's timeline", async () => {
    draw();

    expect(screen.getAllByRole("heading", { level: 1 })).toHaveLength(1);
    expect(screen.getByRole("heading", { name: "Home", level: 1 })).toBeInTheDocument();
    expect(await screen.findByText("Running · console v0.3.999")).toBeInTheDocument();
    // The counts line, not the status line, carries the counts: six tools across four capabilities
    // (terminal, files, screen, plugins).
    const counts = await screen.findByRole("button", { name: /6 tools · 4 capabilities · 2 plugins/ });
    expect(counts).toHaveTextContent("6 tools · 4 capabilities · 2 plugins");

    // The four fetched calls for this machine are the timeline, newest first. The old "Recent calls"
    // list is gone: it was the same rows without the sentence.
    await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(4));
    const rows = screen.getAllByRole("listitem");
    expect(rows[0]).toHaveTextContent("terminal.create");
  });

  it("has no endpoint hero, URL or Copy control", () => {
    draw();

    expect(document.querySelector(".home-hero")).toBeNull();
    expect(screen.queryByText(/\/mcp\//)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Copy" })).not.toBeInTheDocument();
    expect(screen.queryByText("Recent calls")).not.toBeInTheDocument();
    expect(document.querySelector(".home-recent")).toBeNull();
  });

  it("offers one secondary Connect action and says when the machine has no calls", async () => {
    const { onConnect } = draw({ audit: [] });

    expect(await screen.findByText(/No calls from workshop today/)).toBeInTheDocument();
    expect(screen.getByText(/Connect a client sets one up/)).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Connect a client" })).toHaveLength(1);
    expect(screen.getAllByRole("button", { name: /Connect/ })).toHaveLength(1);
    const connect = screen.getByRole("button", { name: "Connect a client" });
    expect(connect).not.toHaveClass("primary");
    fireEvent.click(connect);
    expect(onConnect).toHaveBeenCalledTimes(1);
  });

  it("keeps one All activity link that opens the Activity page", () => {
    const { onActivity } = draw();

    const all = screen.getAllByRole("button", { name: /All activity/ });
    expect(all).toHaveLength(1);
    fireEvent.click(all[0]);
    expect(onActivity).toHaveBeenCalledTimes(1);
  });

  it("shows this machine's live lines in its timeline and no other machine's", () => {
    const now = Date.now();
    draw({
      audit: [],
      liveLines: [
        liveCall({ ts_ms: now, tool: "terminal.create", sentence: "Ran the workshop tests" }),
        liveCall({ ts_ms: now, tool: "files.read", sentence: "Read a workshop note" }),
        liveCall({
          ts_ms: now,
          tool: "runs.exec",
          device_id: "dev-other",
          sentence: "Built another machine"
        })
      ]
    });

    const timeline = screen.getByRole("region", { name: "This machine's activity" });
    expect(within(timeline).getByText("Ran the workshop tests")).toBeInTheDocument();
    expect(within(timeline).getByText("Read a workshop note")).toBeInTheDocument();
    expect(within(timeline).queryByText("Built another machine")).not.toBeInTheDocument();
    // The Now band is this machine's too; another machine's line must not leak into it.
    expect(screen.queryByText(/Built another machine/)).not.toBeInTheDocument();
  });

  it("keeps the Activity filters off the hub's one-machine timeline", () => {
    // The hub already shows this machine's day. An All machines chip over one machine's lines would
    // be a control that cannot tell the truth, so the timeline is drawn without the filter bar.
    draw();
    const timeline = screen.getByRole("region", { name: "This machine's activity" });
    expect(within(timeline).queryByRole("searchbox")).toBeNull();
    expect(within(timeline).queryByRole("group", { name: "Filter by machine" })).toBeNull();
  });

  it("keeps older days off the hub's timeline", async () => {
    const yesterday = new Date();
    yesterday.setDate(yesterday.getDate() - 1);
    yesterday.setHours(12, 0, 0, 0);
    draw({
      audit: [call({ ts_ms: yesterday.getTime(), tool: "old.tool", sentence: "Ran yesterday" })],
      liveLines: []
    });

    expect(screen.queryByText("Ran yesterday")).not.toBeInTheDocument();
    expect(await screen.findByText(/No calls from workshop today/)).toBeInTheDocument();
  });

  it("opens the catalogue from the counts line", async () => {
    const { onCapabilities } = draw();
    fireEvent.click(await screen.findByRole("button", { name: /6 tools · 4 capabilities/ }));
    expect(onCapabilities).toHaveBeenCalledTimes(1);
  });

  it("renders an offline machine without fake counts, a door or a Copy control", () => {
    live.value = {
      tools: [],
      stale: false,
      error: "this machine is not connected; connect it, then try again",
      offline: true,
      loading: false,
      retry: vi.fn()
    };
    draw({ device: { ...workshop, online: false } });

    expect(screen.getByText("Offline · console v0.3.999")).toBeInTheDocument();
    expect(
      screen.getByText(
        "workshop is not connected. The console keeps trying; open the machine's panel if it stays offline."
      )
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Copy" })).not.toBeInTheDocument();
    expect(screen.queryByText(/\/mcp\//)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Connect a client" })).toBeEnabled();
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

  it("does not print zero tools when the catalogue was never read", () => {
    live.value = {
      tools: [],
      stale: false,
      error: "the console could not read this machine's tools",
      offline: false,
      loading: false,
      retry: vi.fn()
    };
    draw();

    expect(screen.getByText("Running · console v0.3.999")).toBeInTheDocument();
    expect(
      screen.getByText("Tool counts are unavailable until the console can read the machine's list.")
    ).toBeInTheDocument();
    expect(screen.queryByText(/^0 tools/)).not.toBeInTheDocument();
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

    expect(await screen.findByText("Running · console v0.3.999")).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: /1 tool · 1 capability · 0 plugins/ })).toBeInTheDocument();
    expect(plugins.list).not.toHaveBeenCalled();
  });

  it("asks the console for this machine's own newest lines, not the fleet window", async () => {
    draw({ audit: [] });

    await waitFor(() => expect(auditApi.audit).toHaveBeenCalledWith(1000, workshop.id));
    expect(auditApi.audit).toHaveBeenCalledTimes(1);
  });

  it("says the window was cut when this machine's own 1000 were", async () => {
    // Midnight, a minute in: every row is today even in a run that starts just after midnight.
    const start = new Date();
    start.setHours(0, 0, 0, 0);
    const base = start.getTime() + 60_000;
    const thousand: AuditLine[] = Array.from({ length: 1000 }, (_, index) =>
      call({
        ts_ms: base - index,
        tool: index % 2 === 0 ? "terminal.create" : "files.read"
      })
    );
    draw({ audit: thousand });

    expect(
      await screen.findByText("Showing the newest 1000 lines; older ones are not on this page.")
    ).toBeInTheDocument();
  });

  it("does not say the window was cut for a list shorter than the cap", async () => {
    draw();

    await screen.findByText("terminal.create");
    expect(screen.queryByText(/Showing the newest/)).toBeNull();
  });

  it("does not claim there were no calls when this machine's audit could not be read", async () => {
    draw({ auditError: true });

    expect(
      await screen.findByText(
        "The console could not read this machine's activity, so this list may be incomplete."
      )
    ).toBeInTheDocument();
    expect(screen.queryByText(/No calls from workshop today/)).toBeNull();
  });
});
