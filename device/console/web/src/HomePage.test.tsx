import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AuditLine, Device, LiveTool } from "./api";
import { HomePage } from "./HomePage";
import type { LiveLine } from "./liveActivity";

// The hub asks `/audit?device=` for its own machine's newest lines; the fleet window belongs to
// Activity. These tests drive the fetch the way the console answers it.
const auditApi = vi.hoisted(() => ({ audit: vi.fn() }));
const installerApi = vi.hoisted(() => ({ installer: vi.fn() }));
vi.mock("./api", () => ({
  api: { audit: auditApi.audit, installer: installerApi.installer }
}));

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
    // The hub now draws the AI's work by default, so a test's live line is a client's call unless
    // it says otherwise; a console line is written where a test needs one.
    caller: "client",
    sentence: "Did something",
    ...overrides
  };
}

/**
 * `count` calls today, newest first, in the part of the local day that has already happened: the
 * newest sits just before `now`, and the block is compressed toward midnight so the oldest stays on
 * this side of it. The times therefore never sit on a boundary — a fixed 01:00 block is in the
 * future until 01:00, and on the first hour after midnight a live row is older than it and is
 * pushed out of the newest 60.
 * They are the AI's calls: the hub draws client-proven rows, so a fixture without a caller would
 * be hidden by the very rule these tests exercise.
 */
function todayCalls(count: number): AuditLine[] {
  const now = Date.now();
  const start = new Date(now);
  start.setHours(0, 0, 0, 0);
  const gap = Math.min(1_000, (now - start.getTime()) / (count + 1));
  return Array.from({ length: count }, (_, index) =>
    call({
      ts_ms: Math.floor(now - (index + 1) * gap),
      caller: "client",
      tool: `tool.${index}`,
      sentence: `Call ${index}`
    })
  );
}

/**
 * A hub day shaped like the measured DESKTOP-9VH92BJ page: `clientCalls` AI calls and
 * `consoleReads` console reads, newest first, one second apart, all inside today's first hour.
 * The 60-row case (56 console, 4 client) is the measurement this rule was written from.
 */
function hubDay(consoleReads: number, clientCalls: number): AuditLine[] {
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  const base = start.getTime() + 3_600_000;
  return [
    ...Array.from({ length: clientCalls }, (_, index) =>
      call({
        ts_ms: base - index * 1000,
        caller: "client",
        tool: `client.${index}`,
        sentence: `Client call ${index}`
      })
    ),
    ...Array.from({ length: consoleReads }, (_, index) =>
      call({
        ts_ms: base - (clientCalls + index) * 1000,
        caller: "console",
        tool: `console.${index}`,
        sentence: `Console read ${index}`
      })
    )
  ];
}

/**
 * The measured hub window (2026-11-05): 1004 fetched lines become 974 the hub can draw — 283 AI
 * calls, 580 console reads and 111 rows with no recorded caller — after 9 channel boundaries and 21
 * repeated keys are withheld. The newest 60 rows are the fold.
 */
function measuredHubWindow(): { audit: AuditLine[]; liveLines: LiveLine[] } {
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  const base = start.getTime() + 6 * 3_600_000;
  let tick = 0;
  const next = () => base - tick++ * 1000;
  const lines: AuditLine[] = [
    ...Array.from({ length: 9 }, (_, index) =>
      call({
        ts_ms: next(),
        caller: "console",
        tool: index % 2 === 0 ? "device.left" : "device.joined",
        sentence: "The console's channel boundary"
      })
    ),
    ...Array.from({ length: 576 }, (_, index) =>
      call({
        ts_ms: next(),
        caller: "console",
        tool: `console.${index}`,
        sentence: `Console read ${index}`
      })
    ),
    ...Array.from({ length: 283 }, (_, index) =>
      call({
        ts_ms: next(),
        caller: "client",
        tool: `client.${index}`,
        sentence: `Client call ${index}`
      })
    ),
    ...Array.from({ length: 111 }, (_, index) =>
      call({
        ts_ms: next(),
        tool: `unknown.${index}`,
        sentence: `Unattributed ${index}`
      })
    )
  ];
  // Lines the file wrote twice: the same instant, tool and status, so the merge draws each once.
  const repeats = lines.slice(7, 28).map((line) => ({ ...line }));
  // The console's newest reads, on the live stream rather than in the fetched window.
  const liveLines: LiveLine[] = Array.from({ length: 4 }, (_, index) =>
    liveCall({
      ts_ms: base + 4_000 - index * 1000,
      caller: "console",
      tool: `live.${index}`,
      sentence: `Console read live ${index}`
    })
  );
  return { audit: [...lines, ...repeats], liveLines };
}

/** An answer the test decides when to give. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((give) => {
    resolve = give;
  });
  return { promise, resolve };
}

function draw(overrides: Record<string, unknown> = {}) {
  const {
    audit,
    auditError = false,
    ...props
  } = overrides;
  const now = Date.now();
  const calls = [
    call({ ts_ms: now, caller: "client" }),
    call({ tool: "files.read", ts_ms: now, caller: "client" }),
    call({ tool: "screen.capture", ts_ms: now, caller: "client" }),
    call({ tool: "runs.exec", ts_ms: now, caller: "client" })
  ];
  if (auditError) auditApi.audit.mockRejectedValue(new Error("the audit could not be read"));
  else auditApi.audit.mockResolvedValue(audit ?? calls);
  const onConnect = vi.fn();
  const onTools = vi.fn();
  const onActivity = vi.fn();
  const page = (extra: Record<string, unknown> = {}) => (
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
      onTokens={vi.fn()}
      onDismissCode={vi.fn()}
      onNotice={vi.fn()}
      onConnect={onConnect}
      onTools={onTools}
      onActivity={onActivity}
      {...props}
      {...extra}
    />
  );
  const view = render(page());
  return {
    ...view,
    /** Redraw the same page with new props: a tick, a recovery, or another snapshot. */
    rerenderHub: (extra: Record<string, unknown>) => view.rerender(page(extra)),
    onConnect,
    onTools,
    onActivity
  };
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
    expect(await screen.findByText("Running")).toBeInTheDocument();
    // The console's build belongs to the Account menu; the subtitle says what the machine is.
    expect(screen.queryByText(/console v/)).toBeNull();
    // The counts line, not the status line, carries the counts: six tools across four capabilities
    // (terminal, files, screen, plugins).
    const counts = await screen.findByRole("button", { name: /6 tools · 4 capabilities · 2 plugins/ });
    expect(counts).toHaveTextContent("6 tools · 4 capabilities · 2 plugins");

    // The four fetched calls for this machine are the timeline, newest first. The old "Recent calls"
    // list is gone: it was the same rows without the sentence.
    await waitFor(() => expect(document.querySelectorAll(".timeline-row")).toHaveLength(4));
    const rows = document.querySelectorAll(".timeline-row");
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

  it("feeds the Now band from this machine's window, not the fleet's live slice", async () => {
    // A fixed local afternoon, not the wall clock: at 00:00 `now - 10s` is yesterday, and the
    // hub's day list correctly drops it before the band can count it. The fixture moves; the rule
    // that the band is fed from this machine's window is what the test proves.
    const now = new Date(2026, 10, 5, 14, 45, 0).getTime();
    const clock = vi.spyOn(Date, "now").mockReturnValue(now);
    try {
      draw({
        audit: [
          call({ ts_ms: now - 10_000, caller: "client", tool: "terminal.create" }),
          call({ ts_ms: now - 20_000, caller: "client", tool: "files.read" })
        ],
        // The fleet's newest 200 live lines held nothing for this machine. The machine's own window
        // does, and the band must describe the machine the timeline is showing.
        liveLines: [],
        liveState: "live"
      });

      expect(await screen.findByText("Active now")).toBeInTheDocument();
      expect(screen.getByText("2 calls in the last minute")).toBeInTheDocument();
    } finally {
      clock.mockRestore();
    }
  });

  it("shows this machine's running client call in the band, and no other machine's", async () => {
    const now = Date.now();
    draw({
      audit: [],
      liveRunning: [
        {
          id: "call-mine",
          tool: "runs.exec",
          at_ms: now - 72_000,
          device_id: workshop.id,
          caller: "client"
        },
        {
          id: "call-other",
          tool: "files.read",
          at_ms: now,
          device_id: "dev-other",
          caller: "client"
        }
      ]
    });

    // The band is this machine's, like its timeline: another machine's in-flight call must not name
    // itself here, and the line is the tool plus how long it has run.
    expect(await screen.findByText(/running runs\.exec · /)).toBeInTheDocument();
    expect(screen.queryByText(/running files\.read/)).not.toBeInTheDocument();
  });

  it("rereads the machine's window on a reconnect, and the fresh window feeds the band", async () => {
    // A fixed local afternoon: three hours back and five seconds back are both today at any wall
    // clock, where just after midnight either one would be yesterday's line.
    const now = new Date(2026, 10, 5, 14, 45, 0).getTime();
    const clock = vi.spyOn(Date, "now").mockReturnValue(now);
    try {
      // The page loaded three hours ago: the day below is a memory with a hole after it.
      auditApi.audit.mockResolvedValueOnce([call({ ts_ms: now - 3 * 3_600_000, caller: "client" })]);
      const { rerenderHub } = draw({ liveState: "offline" });
      await waitFor(() => expect(auditApi.audit).toHaveBeenCalledTimes(1));

      const reread = deferred<AuditLine[]>();
      auditApi.audit.mockReturnValueOnce(reread.promise);
      // The snapshot and a tick land in the same moment: the recovery's reread is the request, and
      // the tick must not stack another one on top of it.
      rerenderHub({ liveState: "live", liveRecoveries: 1, tick: 1 });
      await waitFor(() => expect(auditApi.audit).toHaveBeenCalledTimes(2));
      rerenderHub({ liveState: "live", liveRecoveries: 1, tick: 2 });
      await act(async () => {});
      expect(auditApi.audit).toHaveBeenCalledTimes(2);

      await act(async () => {
        reread.resolve([call({ ts_ms: Date.now() - 5_000, caller: "client", tool: "runs.exec" })]);
      });
      expect(auditApi.audit).toHaveBeenLastCalledWith(1000, workshop.id);
      expect(await screen.findByText("Active now")).toBeInTheDocument();
      expect(screen.getByText("1 call in the last minute")).toBeInTheDocument();
    } finally {
      clock.mockRestore();
    }
  });

  it("rereads once per tick, never twice for the same tick or while a read is open", async () => {
    const { rerenderHub } = draw({ liveState: "live" });
    await waitFor(() => expect(auditApi.audit).toHaveBeenCalledTimes(1));

    rerenderHub({ tick: 1 });
    await waitFor(() => expect(auditApi.audit).toHaveBeenCalledTimes(2));

    // The same tick rendered again is not a second request.
    rerenderHub({ tick: 1 });
    await act(async () => {});
    expect(auditApi.audit).toHaveBeenCalledTimes(2);

    // A tick that lands while the previous read is still open is a tick with no request; the next
    // tick after the answer reads again.
    const answer = deferred<AuditLine[]>();
    auditApi.audit.mockReturnValueOnce(answer.promise);
    rerenderHub({ tick: 2 });
    await waitFor(() => expect(auditApi.audit).toHaveBeenCalledTimes(3));
    rerenderHub({ tick: 3 });
    await act(async () => {});
    expect(auditApi.audit).toHaveBeenCalledTimes(3);

    await act(async () => {
      answer.resolve([call({ caller: "client" })]);
    });
    rerenderHub({ tick: 4 });
    await waitFor(() => expect(auditApi.audit).toHaveBeenCalledTimes(4));
  });

  it("says when the window was read only when it is older than the live lines", async () => {
    const midday = new Date();
    midday.setHours(12, 0, 0, 0);
    const now = midday.getTime();
    const clock = vi.spyOn(Date, "now").mockReturnValue(now);
    try {
      const { rerenderHub } = draw({
        audit: [call({ ts_ms: now - 3 * 3_600_000, caller: "client" })],
        liveLines: [],
        liveState: "live"
      });
      await screen.findByText("terminal.create");
      expect(screen.queryByText(/^Read \d/)).toBeNull();

      // A live line newer than the window, one second after the read: the tick's reread is still on
      // its way, so the page does not yet call the window stale.
      clock.mockReturnValue(now + 1_000);
      rerenderHub({ liveLines: [liveCall({ ts_ms: now + 500, caller: "client" })] });
      expect(screen.queryByText(/^Read \d/)).toBeNull();

      // Two minutes without a successful reread: the window is older than the lines above it, and
      // the page says when it was read instead of letting it stand for live data.
      clock.mockReturnValue(now + 2 * 60_000);
      rerenderHub({ liveLines: [liveCall({ ts_ms: now + 2 * 60_000 - 1_000, caller: "client" })] });
      expect(screen.getByText("Read 2 min ago.")).toBeInTheDocument();

      // The next read answers with a window that includes the live line: the sentence goes away,
      // because the fetched part is no longer behind the stream.
      auditApi.audit.mockResolvedValueOnce([
        call({ ts_ms: now + 2 * 60_000 - 500, caller: "client" })
      ]);
      rerenderHub({
        tick: 1,
        liveLines: [liveCall({ ts_ms: now + 2 * 60_000 - 1_000, caller: "client" })]
      });
      await waitFor(() => expect(screen.queryByText("Read 2 min ago.")).toBeNull());
    } finally {
      clock.mockRestore();
    }
  });

  it("counts a call whose caller was never recorded but whose token proves it", async () => {
    // The live window has 908 lines with no caller, and 376 of them carry a token (no console line
    // does). Counting only caller === "client" read 62 where the record proved 363.
    const now = new Date(2026, 10, 5, 14, 45, 0).getTime();
    const clock = vi.spyOn(Date, "now").mockReturnValue(now);
    try {
      draw({
        audit: [
          call({ ts_ms: new Date(2026, 10, 5, 14, 32).getTime(), caller: "client" }),
          call({ ts_ms: new Date(2026, 10, 5, 14, 20).getTime(), token_id: "1".repeat(64) }),
          call({ ts_ms: new Date(2026, 10, 5, 14, 10).getTime(), token_id: "2".repeat(64) }),
          call({ ts_ms: new Date(2026, 10, 5, 14, 5).getTime(), caller: "console" })
        ],
        liveLines: []
      });

      expect(
        await screen.findByText("Today 3 client calls · last 14:32")
      ).toBeInTheDocument();
    } finally {
      clock.mockRestore();
    }
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
      audit: [call({ ts_ms: yesterday.getTime(), caller: "client", tool: "old.tool", sentence: "Ran yesterday" })],
      liveLines: []
    });

    expect(screen.queryByText("Ran yesterday")).not.toBeInTheDocument();
    expect(await screen.findByText(/No calls from workshop today/)).toBeInTheDocument();
  });

  it("counts today's client calls, the failures among them, and when the last one was", async () => {
    const now = new Date(2026, 10, 5, 14, 45, 0).getTime();
    const clock = vi.spyOn(Date, "now").mockReturnValue(now);
    try {
      draw({
        audit: [
          call({ ts_ms: new Date(2026, 10, 5, 14, 32).getTime(), caller: "client" }),
          call({ ts_ms: new Date(2026, 10, 5, 14, 20).getTime(), caller: "client", status: 503 }),
          call({ ts_ms: new Date(2026, 10, 5, 14, 10).getTime(), caller: "client" }),
          // The console's own failed poll is the machine's silence, not the AI's failure: it is
          // newer than every client call, so a count or a clock that folded it in would move both.
          {
            ts_ms: new Date(2026, 10, 5, 14, 40).getTime(),
            kind: "poll",
            caller: "console",
            tool: "terminal.read",
            detail: "device reconnecting",
            status: 503
          }
        ],
        liveLines: []
      });

      // Two succeeded and one failed, all after local midnight: the line counts the calls and the
      // failures, and the last is the newest client call's clock — not a number the page invented.
      expect(
        await screen.findByText("Today 3 client calls · 1 failed · last 14:32")
      ).toBeInTheDocument();
    } finally {
      clock.mockRestore();
    }
  });

  it("says today is none and when yesterday's last call was", async () => {
    const now = new Date(2026, 10, 5, 8, 0, 0).getTime();
    const clock = vi.spyOn(Date, "now").mockReturnValue(now);
    try {
      draw({
        // The newest line is 20 h old: last night's work ended before the browser's midnight, so the
        // timeline above is empty and the summary still says when the machine last worked.
        audit: [call({ ts_ms: now - 20 * 3_600_000, caller: "client", tool: "runs.exec" })],
        liveLines: []
      });

      expect(
        await screen.findByText("Today no client calls · last 12:00 yesterday")
      ).toBeInTheDocument();
    } finally {
      clock.mockRestore();
    }
  });

  it("says today is none when the window holds no lines at all", async () => {
    draw({ audit: [], liveLines: [] });

    expect(await screen.findByText("Today no client calls")).toBeInTheDocument();
  });

  it("does not count the console's own calls in the day's line", async () => {
    const now = new Date(2026, 10, 5, 14, 45, 0).getTime();
    const clock = vi.spyOn(Date, "now").mockReturnValue(now);
    try {
      draw({
        audit: [
          call({ ts_ms: new Date(2026, 10, 5, 14, 32).getTime(), caller: "client" }),
          call({ ts_ms: new Date(2026, 10, 5, 14, 20).getTime(), caller: "client" }),
          // The console's own polling is in the record and in the timeline, marked "console"; the
          // day's count is the AI's work, and a console line must not become a client call.
          call({
            ts_ms: new Date(2026, 10, 5, 14, 44).getTime(),
            caller: "console",
            tool: "terminal.list"
          }),
          call({
            ts_ms: new Date(2026, 10, 5, 14, 43).getTime(),
            caller: "console",
            tool: "terminal.read"
          })
        ],
        liveLines: []
      });

      expect(
        await screen.findByText("Today 2 client calls · last 14:32")
      ).toBeInTheDocument();
    } finally {
      clock.mockRestore();
    }
  });

  it("draws the AI's calls and folds the console's own reads behind one counted chip", async () => {
    // The measured hub: 60 rows, 56 of them the console's polling. The list is the AI's work.
    draw({ audit: hubDay(56, 4) });

    await waitFor(() => expect(document.querySelectorAll(".timeline-row")).toHaveLength(4));
    expect(screen.queryByText("Console read 0")).toBeNull();
    const chip = screen.getByRole("button", { name: "Console reads · 56" });
    expect(chip).toHaveAttribute("aria-pressed", "false");

    // One click adds them: the rows were never dropped, only folded.
    fireEvent.click(chip);
    await waitFor(() => expect(document.querySelectorAll(".timeline-row")).toHaveLength(60));
    expect(screen.getByText("Console read 0")).toBeInTheDocument();
    expect(chip).toHaveAttribute("aria-pressed", "true");

    // And one click takes them back out; the toggle is a view, not a persisted preference.
    fireEvent.click(chip);
    await waitFor(() => expect(document.querySelectorAll(".timeline-row")).toHaveLength(4));
  });

  it("names the hidden console reads when the AI made no calls, with the chip still offered", async () => {
    draw({ audit: hubDay(56, 0) });

    expect(
      await screen.findByText("No client calls yet; the console's own reads are hidden.")
    ).toBeInTheDocument();
    const chip = screen.getByRole("button", { name: "Console reads · 56" });
    expect(chip).toHaveAttribute("aria-pressed", "false");

    fireEvent.click(chip);
    await waitFor(() => expect(document.querySelectorAll(".timeline-row")).toHaveLength(56));
    expect(
      screen.queryByText("No client calls yet; the console's own reads are hidden.")
    ).toBeNull();
  });

  it("does not invent client calls when the window holds only unattributed rows", async () => {
    // A caller-less, token-less row is not the AI's proven work. The hub must not draw it, and the
    // empty state must not claim calls were made or that console reads are hidden when there are
    // none. Activity still has the row: the hub is the AI's page, not the audit.
    // Fixed local afternoon: ten seconds before a real midnight is yesterday, which the day list
    // (correctly) drops before the empty state can be read.
    const now = new Date(2026, 10, 5, 14, 45, 0).getTime();
    const clock = vi.spyOn(Date, "now").mockReturnValue(now);
    try {
      draw({ audit: [call({ ts_ms: now - 10_000 })] });

      expect(await screen.findByText("No client calls from workshop today.")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Console reads · 0" })).toBeInTheDocument();
      expect(screen.queryByRole("listitem")).toBeNull();
    } finally {
      clock.mockRestore();
    }
  });

  it("opens the machine's Tools tab from the counts line", async () => {
    const { onTools } = draw();
    fireEvent.click(await screen.findByRole("button", { name: /6 tools · 4 capabilities/ }));
    expect(onTools).toHaveBeenCalledTimes(1);
  });

  it("renders an offline machine without fake counts, an endpoint door or a Copy control", () => {
    live.value = {
      tools: [],
      stale: false,
      error: "this machine is not connected; connect it, then try again",
      offline: true,
      loading: false,
      retry: vi.fn()
    };
    draw({ device: { ...workshop, online: false } });

    expect(screen.getByText("Offline")).toBeInTheDocument();
    const notice = screen.getByRole("status");
    expect(notice).toHaveTextContent(
      "workshop is not connected. The console keeps trying; Retry works from here."
    );
    expect(notice).toHaveTextContent(
      "If it stays offline, open Device → Machine in the console for the machine's last facts."
    );
    expect(notice).toHaveTextContent(
      "The machine's local panel is http://127.0.0.1:18090/ — it opens only at the machine."
    );
    expect(screen.queryByRole("button", { name: "Copy" })).not.toBeInTheDocument();
    expect(screen.queryByText(/\/mcp\//)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Connect a client" })).toBeEnabled();
    expect(screen.getByText("Tool counts are unavailable while the machine is offline.")).toBeInTheDocument();
    expect(screen.queryByText(/0 tools/)).not.toBeInTheDocument();
  });

  it("links the console's Device → Machine view from the offline sentence", () => {
    live.value = {
      tools: [],
      stale: false,
      error: "this machine is not connected; connect it, then try again",
      offline: true,
      loading: false,
      retry: vi.fn()
    };
    draw({ device: { ...workshop, online: false } });

    // The console's own view of the machine is the door that works from here; its fragment is the
    // address the app already parses, so the link survives a reload like every other surface link.
    const link = screen.getByRole("link", { name: "Device → Machine" });
    expect(link).toHaveAttribute("href", "#device/machine");
    expect(link.closest('[role="status"]')).not.toBeNull();
  });

  it("names the machine's local panel as local, not as a door this console opens", () => {
    live.value = {
      tools: [],
      stale: false,
      error: "this machine is not connected; connect it, then try again",
      offline: true,
      loading: false,
      retry: vi.fn()
    };
    draw({ device: { ...workshop, online: false } });

    // The panel is loopback-only, so its address is a fact about the machine, not a link the
    // console can promise to open. It must be named, and named as local.
    const notice = screen.getByRole("status");
    expect(notice).toHaveTextContent(
      "The machine's local panel is http://127.0.0.1:18090/ — it opens only at the machine."
    );
    expect(notice.querySelector("a[href*='127.0.0.1']")).toBeNull();
    expect(screen.queryByRole("link", { name: /127\.0\.0\.1/ })).toBeNull();
  });

  it("says the count and the reason when the console's last attempt failed", () => {
    live.value = {
      tools: [],
      stale: false,
      error: "this machine is not connected; connect it, then try again",
      offline: true,
      loading: false,
      retry: vi.fn()
    };
    draw({
      device: {
        ...workshop,
        online: false,
        last_attempt: { attempts: 5, reason: "timed out waiting for the channel hello" }
      }
    });

    // The count is the fleet's own, and the reason is the device's sentence exactly as it arrived.
    const sentence = screen.getByText(/is not connected after 5 attempts/);
    expect(sentence).toHaveTextContent("timed out waiting for the channel hello");
    expect(screen.getByTitle("timed out waiting for the channel hello")).toBeInTheDocument();
    // The advice follows the reason rather than replacing it: both doors are still named.
    expect(screen.getByRole("link", { name: "Device → Machine" })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("http://127.0.0.1:18090/");
  });

  it("leaves the offline sentence alone when the fleet carried no reason", () => {
    live.value = {
      tools: [],
      stale: false,
      error: "this machine is not connected; connect it, then try again",
      offline: true,
      loading: false,
      retry: vi.fn()
    };
    draw({
      device: { ...workshop, online: false, last_attempt: { attempts: 5, reason: "" } }
    });

    const notice = screen.getByRole("status");
    expect(notice).toHaveTextContent(
      "workshop is not connected. The console keeps trying; Retry works from here."
    );
    expect(screen.queryByText(/after 5 attempts/)).not.toBeInTheDocument();
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

    expect(screen.getByText("Running")).toBeInTheDocument();
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

    expect(await screen.findByText("Running")).toBeInTheDocument();
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
        caller: "client",
        tool: index % 2 === 0 ? "terminal.create" : "files.read"
      })
    );
    draw({ audit: thousand });

    expect(
      await screen.findByText(
        "Showing the newest 60 of 1000 lines (1000 client calls); older ones are not on this page."
      )
    ).toBeInTheDocument();
  });

  it("names the drawable window, the rows it draws and what the merge withheld", async () => {
    // The measured DESKTOP-14RJCR8 hub before the console and unattributed chips existed: 1000
    // fetched, 6 channel boundaries and 17 repeated keys, so 977 rows are in the day list behind
    // the 60-row fold (60 + 917 more). All 977 are client calls.
    const start = new Date();
    start.setHours(0, 0, 0, 0);
    const base = start.getTime() + 3_600_000;
    const calls = Array.from({ length: 977 }, (_, index) =>
      call({
        ts_ms: base - index * 1000,
        caller: "client",
        tool: `tool.${index}`,
        sentence: `Call ${index}`
      })
    );
    const boundaries = Array.from({ length: 6 }, (_, index) =>
      call({
        ts_ms: base - (977 + index) * 1000,
        caller: "console",
        tool: index % 2 === 0 ? "device.left" : "device.joined",
        sentence: "The console's channel boundary"
      })
    );
    const repeats = calls.slice(0, 17).map((line) => ({ ...line }));
    draw({ audit: [...calls, ...boundaries, ...repeats] });

    expect(
      await screen.findByText(
        "Showing the newest 60 of 977 lines (977 client calls; 6 channel events, 17 repeats not drawn); older ones are not on this page."
      )
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Show the rest of today (917 more)" })
    ).toBeInTheDocument();
  });

  it("states a window whose every number its controls can reach, and reaches all of it", async () => {
    const { audit, liveLines } = measuredHubWindow();
    draw({ audit, liveLines });

    const sentence = await screen.findByText(/^Showing the newest 60 of 974 lines/);
    expect(sentence.textContent).toBe(
      "Showing the newest 60 of 974 lines (283 client calls, 580 console reads, 111 unattributed; 9 channel events, 21 repeats not drawn); older ones are not on this page."
    );
    // THE RULE THIS TEST PINS. The fetched window was 1004 lines, but the sentence may not state a
    // line the fold and the two chips cannot draw: every number it prints is at most the 974 rows
    // the controls reach.
    for (const digits of sentence.textContent?.match(/\d+/g) ?? []) {
      expect(Number(digits)).toBeLessThanOrEqual(974);
    }
    expect(sentence.textContent).not.toContain("1004");
    // One rule: the sentence's client bucket is the number the day line above the list counts.
    expect(screen.getByText(/^Today 283 client calls/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Console reads · 580" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Unattributed · 111" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Show the rest of today (223 more)" })
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Console reads · 580" }));
    fireEvent.click(screen.getByRole("button", { name: "Unattributed · 111" }));
    fireEvent.click(screen.getByRole("button", { name: "Show the rest of today (914 more)" }));

    // The claim is the list's length: the fold draws the last row it says it holds.
    await waitFor(() => expect(document.querySelectorAll(".timeline-row")).toHaveLength(974));
    expect(await screen.findByText(/^Showing all 974 lines/)).toHaveTextContent(
      "Showing all 974 lines (283 client calls, 580 console reads, 111 unattributed; 9 channel events, 21 repeats not drawn); older ones are not on this page."
    );
    // The oldest row of the window — the last one the fold claims — is on screen.
    expect(screen.getByText("Unattributed 110")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Show the rest of today/ })).toBeNull();
  });

  it("names and draws the unattributed rows even when no window sentence is due", async () => {
    // The measured DESKTOP-9VH92BJ shape: its window is not cut, so there is no cap sentence, but
    // 111 of its 311 lines are unattributed. The chip is where those rows are counted and reached.
    const start = new Date();
    start.setHours(0, 0, 0, 0);
    const base = start.getTime() + 3_600_000;
    let tick = 0;
    const next = () => base - tick++ * 1000;
    const audit: AuditLine[] = [
      ...Array.from({ length: 36 }, (_, index) =>
        call({
          ts_ms: next(),
          caller: "console",
          tool: index % 2 === 0 ? "device.left" : "device.joined",
          sentence: "The console's channel boundary"
        })
      ),
      ...Array.from({ length: 145 }, (_, index) =>
        call({
          ts_ms: next(),
          caller: "console",
          tool: `console.${index}`,
          sentence: `Console read ${index}`
        })
      ),
      ...Array.from({ length: 19 }, (_, index) =>
        call({
          ts_ms: next(),
          caller: "client",
          tool: `client.${index}`,
          sentence: `Client call ${index}`
        })
      ),
      ...Array.from({ length: 111 }, (_, index) =>
        call({
          ts_ms: next(),
          tool: `unknown.${index}`,
          sentence: `Unattributed ${index}`
        })
      )
    ];
    draw({ audit });

    await waitFor(() => expect(document.querySelectorAll(".timeline-row")).toHaveLength(19));
    expect(screen.queryByText(/Showing the newest/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Unattributed · 111" }));
    // 19 client calls and 111 unattributed rows: the fold now holds 70 of them.
    fireEvent.click(screen.getByRole("button", { name: "Show the rest of today (70 more)" }));
    await waitFor(() => expect(document.querySelectorAll(".timeline-row")).toHaveLength(130));
    fireEvent.click(screen.getByRole("button", { name: "Console reads · 145" }));
    await waitFor(() => expect(document.querySelectorAll(".timeline-row")).toHaveLength(275));
    expect(screen.getByText("Unattributed 110")).toBeInTheDocument();
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

  it("puts the capability line above this machine's timeline", async () => {
    draw();

    const counts = await screen.findByRole("button", {
      name: /6 tools · 4 capabilities · 2 plugins/
    });
    const timeline = screen.getByRole("region", { name: "This machine's activity" });
    // The line answers "what can this machine do now"; at the bottom it was 56 screens down.
    expect(counts.compareDocumentPosition(timeline) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("draws the newest 60 rows and offers the rest of today", async () => {
    draw({ audit: todayCalls(75) });

    // 60 rows, not 1000: the day is scannable, and the record behind it is one control away.
    await waitFor(() => expect(document.querySelectorAll(".timeline-row")).toHaveLength(60));
    const more = screen.getByRole("button", { name: "Show the rest of today (15 more)" });
    expect(screen.getByRole("button", { name: /All activity/ })).toBeInTheDocument();
    fireEvent.click(more);

    await waitFor(() => expect(document.querySelectorAll(".timeline-row")).toHaveLength(75));
    expect(screen.queryByRole("button", { name: /Show the rest of today/ })).toBeNull();
    // The expansion is in place: the same region still holds the rows and its summary.
    expect(
      within(screen.getByRole("region", { name: "This machine's activity" })).getByText("Call 74")
    ).toBeInTheDocument();
  });

  it("names the four repeats the day line counts but the window drops", async () => {
    // The audit's uncut shape: 60 drawable rows plus four repeated keys. `/fleet` counts all 64
    // client lines, the merge draws 60, and there is no fold because all 60 drawable rows are
    // drawn. The day line and the fold cannot meet without the sentence Activity's cap sentence
    // already prints: 60 drawn + 4 repeats not drawn = the 64 the day line counts.
    const start = new Date();
    start.setHours(0, 0, 0, 0);
    const base = start.getTime() + 3_600_000;
    const calls = Array.from({ length: 60 }, (_, index) =>
      call({
        ts_ms: base - index * 1000,
        caller: "client",
        tool: `tool.${index}`,
        sentence: `Call ${index}`,
        // The four failures are drawable; the repeats below are successful, so the day read's
        // `failed` and the failures the list draws are the same four.
        status: index < 4 ? 503 : 200
      })
    );
    const repeats = calls.slice(4, 8).map((line) => ({ ...line }));
    draw({
      audit: [...calls, ...repeats],
      device: { ...workshop, today: { client_calls: 64, failed: 4 } }
    });

    expect(
      await screen.findByText("Today 64 client calls · 4 failed · last 01:00")
    ).toBeInTheDocument();
    await waitFor(() => expect(document.querySelectorAll(".timeline-row")).toHaveLength(60));
    expect(screen.queryByRole("button", { name: /Show the rest of today/ })).toBeNull();
    expect(screen.getByText("4 repeats not drawn.")).toBeInTheDocument();
  });

  it("names the four repeats between the measured day line and its fold", async () => {
    // The 2026-11-05 hub: `/fleet` counted 183 client calls and 8 failed, the window held four
    // repeated keys, and the fold drew 60 + 119 = 179 rows. The sentence is the missing term:
    // 60 + 119 + 4 = 183, and the failed count is the same eight the list draws.
    const start = new Date();
    start.setHours(0, 0, 0, 0);
    const base = start.getTime() + 3_600_000;
    const calls = Array.from({ length: 179 }, (_, index) =>
      call({
        ts_ms: base - index * 1000,
        caller: "client",
        tool: `tool.${index}`,
        sentence: `Call ${index}`,
        status: index < 8 ? 503 : 200
      })
    );
    const repeats = calls.slice(8, 12).map((line) => ({ ...line }));
    draw({
      audit: [...calls, ...repeats],
      device: { ...workshop, today: { client_calls: 183, failed: 8 } }
    });

    expect(
      await screen.findByText("Today 183 client calls · 8 failed · last 01:00")
    ).toBeInTheDocument();
    const more = screen.getByRole("button", { name: "Show the rest of today (119 more)" });
    expect(screen.getByText("4 repeats not drawn.")).toBeInTheDocument();
    fireEvent.click(more);

    await waitFor(() => expect(document.querySelectorAll(".timeline-row")).toHaveLength(179));
    expect(screen.queryByRole("button", { name: /Show the rest of today/ })).toBeNull();
    // The sentence stays after the fold is gone: the window still dropped the four rows.
    expect(screen.getByText("4 repeats not drawn.")).toBeInTheDocument();
  });

  it("recounts the rest when a live row arrives while collapsed", async () => {
    const { rerenderHub } = draw({ audit: todayCalls(75) });
    await screen.findByRole("button", { name: "Show the rest of today (15 more)" });

    // A new row at the top pushes the oldest visible row out of the window; N grows by one and the
    // page says so, rather than keeping the count it happened to draw first.
    rerenderHub({ liveLines: [liveCall({ sentence: "A brand new call" })] });

    await screen.findByRole("button", { name: "Show the rest of today (16 more)" });
    expect(document.querySelectorAll(".timeline-row")).toHaveLength(60);
    expect(
      within(screen.getByRole("region", { name: "This machine's activity" })).getByText(
        "A brand new call"
      )
    ).toBeInTheDocument();
  });

  it("keeps the whole day drawn when a live row arrives after expanding", async () => {
    const { rerenderHub } = draw({ audit: todayCalls(75) });
    fireEvent.click(await screen.findByRole("button", { name: "Show the rest of today (15 more)" }));
    await waitFor(() => expect(document.querySelectorAll(".timeline-row")).toHaveLength(75));

    rerenderHub({ liveLines: [liveCall({ sentence: "A brand new call" })] });

    await waitFor(() => expect(document.querySelectorAll(".timeline-row")).toHaveLength(76));
    expect(
      within(screen.getByRole("region", { name: "This machine's activity" })).getByText(
        "A brand new call"
      )
    ).toBeInTheDocument();
    // Expanded stays expanded; the hidden-rows control has nothing left to offer.
    expect(screen.queryByRole("button", { name: /Show the rest of today/ })).toBeNull();
  });
});

/** The installer facts the console answers with, as a fleet test's `/installer` would. */
const installerFacts = {
  available: true as const,
  filename: "SummriseSetup-0.3.9.exe",
  version: "0.3.9",
  size_bytes: 6_638_339,
  sha256: "26d39c12" + "a".repeat(56)
};

/**
 * Render Home with no machine chosen: the fleet, where the Add-a-machine flow lives. This is the
 * state the gate signs in to and the item is about.
 */
function drawFleet(extra: Record<string, unknown> = {}) {
  const onTokens = vi.fn();
  const fleetProps = {
    device: null,
    devices: [],
    selectedId: null,
    code: null,
    enrollCommand: "",
    busy: false,
    onOpen: vi.fn(),
    onRename: vi.fn(),
    onRemove: vi.fn(),
    onDiagnostics: vi.fn(),
    onAddMachine: vi.fn(),
    onTokens,
    onDismissCode: vi.fn(),
    onNotice: vi.fn(),
    onConnect: vi.fn(),
    onTools: vi.fn(),
    onActivity: vi.fn()
  };
  const page = (overrides: Record<string, unknown> = {}) => (
    <HomePage {...fleetProps} {...overrides} />
  );
  const view = render(page(extra));
  return {
    ...view,
    onTokens,
    rerenderFleet: (overrides: Record<string, unknown>) =>
      view.rerender(page({ ...extra, ...overrides }))
  };
}

describe("Home — the fleet's installer", () => {
  beforeEach(() => {
    installerApi.installer.mockReset();
  });

  it("offers the installer the console was given, with its own filename, size and hash", async () => {
    installerApi.installer.mockResolvedValue(installerFacts);
    drawFleet();

    const heading = await screen.findByRole("heading", {
      name: "Install the device runtime"
    });
    const link = screen.getByRole("link", { name: "Download Windows installer" });
    expect(link).toHaveAttribute("href", "/installer/download");
    expect(link).toHaveAttribute("download");
    // Every value on the muted line is the /installer answer: the filename, the size at the scale
    // runFormat states it, and the first eight hex characters of the hash it computed.
    expect(
      screen.getByText("SummriseSetup-0.3.9.exe · 6.3 MB · sha256 26d39c12…")
    ).toBeInTheDocument();
    expect(
      heading.compareDocumentPosition(link) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
  });

  it("marks a fleet row whose version is behind the staged installer", async () => {
    // The measured fleet's 84T3LIN row: the machine ran v0.3.280 while this same page offered the
    // 0.3.284 installer, and the row said nothing. The comparison uses the one `/installer` read
    // the Add-a-machine block already makes.
    installerApi.installer.mockResolvedValue(installerFacts);
    drawFleet({ devices: [{ ...workshop, version: "0.3.8" }] });

    expect(await screen.findByText("behind 0.3.9")).toBeInTheDocument();
  });

  it("draws no fleet cue when the console staged no installer", async () => {
    installerApi.installer.mockResolvedValue({ available: false });
    drawFleet({ devices: [{ ...workshop, version: "0.3.8" }] });

    await screen.findByText(/No installer is published/);
    expect(screen.queryByText(/behind/)).not.toBeInTheDocument();
  });

  it("says plainly when the release carries no installer, and draws no door", async () => {
    installerApi.installer.mockResolvedValue({ available: false });
    drawFleet();

    expect(
      await screen.findByText(
        "No installer is published for this console yet. Install it from the release, then add the machine for its enrollment command."
      )
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("link", { name: "Download Windows installer" })
    ).toBeNull();
    expect(
      screen.queryByRole("heading", { name: "Install the device runtime" })
    ).toBeNull();
  });

  it("hides the block when the read fails rather than showing a door that cannot open", async () => {
    installerApi.installer.mockRejectedValue(new Error("the installer could not be read"));
    drawFleet();

    await waitFor(() => expect(installerApi.installer).toHaveBeenCalledTimes(1));
    expect(
      screen.queryByRole("heading", { name: "Install the device runtime" })
    ).toBeNull();
    expect(screen.queryByText(/No installer is published/)).toBeNull();
  });

  it("reads the installer once, not on every redraw", async () => {
    installerApi.installer.mockResolvedValue(installerFacts);
    const { rerenderFleet } = drawFleet();
    await screen.findByRole("heading", { name: "Install the device runtime" });

    rerenderFleet({ busy: true });
    await act(async () => {});

    expect(installerApi.installer).toHaveBeenCalledTimes(1);
  });

  it("puts the block before the enrollment command in the Add-a-machine panel", async () => {
    installerApi.installer.mockResolvedValue(installerFacts);
    drawFleet({
      code: { code: "code-1", expires_at_ms: Date.now() + 15 * 60_000 },
      enrollCommand:
        "summrise-device.exe --enroll https://console.test code-1 --data C:\\ProgramData\\Summrise"
    });

    const heading = await screen.findByRole("heading", {
      name: "Install the device runtime"
    });
    const command = screen.getByText(
      "summrise-device.exe --enroll https://console.test code-1 --data C:\\ProgramData\\Summrise"
    );
    // The command assumes the binary is installed; the door that installs it comes first.
    expect(
      heading.compareDocumentPosition(command) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    expect(
      screen.getAllByRole("heading", { name: "Install the device runtime" })
    ).toHaveLength(1);
  });
});

/**
 * The enrollment command names only `summrise-device.exe`, and the console cannot know a machine's
 * install root, so the sentence must carry what the command cannot: the folder chosen on the
 * installer's directory page (its default is C:\Program Files\Summrise). An elevated prompt opened
 * anywhere else fails with "not recognized".
 */
describe("Home — the enrollment sentence", () => {
  beforeEach(() => {
    installerApi.installer.mockResolvedValue(installerFacts);
  });

  it("sends the elevated prompt to the folder where Summrise was installed", () => {
    drawFleet({
      code: { code: "code-1", expires_at_ms: Date.now() + 15 * 60_000 },
      enrollCommand:
        "summrise-device.exe --enroll https://console.test code-1 --data C:\\ProgramData\\Summrise"
    });

    const panel = screen.getByRole("region", { name: "Enrollment command" });
    expect(panel).toHaveTextContent(
      "On the Windows machine, open an elevated Command Prompt in the folder you chose on the installer's directory page (the default is C:\\Program Files\\Summrise), run this command, then restart the SummriseLaunch service."
    );
    // The old sentence sent the person to a prompt whose cwd was not the install folder.
    expect(panel).not.toHaveTextContent("run this in an elevated Command Prompt");
    expect(within(panel).getByText("SummriseLaunch").tagName).toBe("CODE");
    expect(within(panel).getByText("C:\\Program Files\\Summrise").tagName).toBe("CODE");
  });
});

/**
 * The credential's door. Tokens used to live only under Account → Tokens, so a person adding a
 * machine never met the thing a client needs; the fleet header and the Add-a-machine panel each
 * open the same Connect sheet on its Tokens section. There is no second token surface to test.
 */
describe("Home — the fleet's Tokens door", () => {
  beforeEach(() => {
    installerApi.installer.mockResolvedValue(installerFacts);
  });

  it("opens the Connect sheet's Tokens section from the fleet header", async () => {
    const { onTokens } = drawFleet({ devices: [workshop] });

    const door = screen.getByRole("button", { name: "Tokens" });
    // The accent belongs to Add a machine; the credential is the secondary control beside it.
    expect(door).not.toHaveClass("primary");
    fireEvent.click(door);
    expect(onTokens).toHaveBeenCalledTimes(1);
  });

  it("keeps the door in the fleet's zero state", () => {
    drawFleet();

    expect(screen.getByRole("heading", { name: "Machines", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Tokens" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add a machine" })).toBeInTheDocument();
  });

  it("puts a Tokens control in the Add-a-machine panel", () => {
    const { onTokens } = drawFleet({
      code: { code: "code-1", expires_at_ms: Date.now() + 15 * 60_000 },
      enrollCommand:
        "summrise-device.exe --enroll https://console.test code-1 --data C:\\ProgramData\\Summrise"
    });

    const panel = screen.getByRole("region", { name: "Enrollment command" });
    fireEvent.click(within(panel).getByRole("button", { name: "Tokens" }));
    expect(onTokens).toHaveBeenCalledTimes(1);
  });
});
