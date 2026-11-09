import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AuditLine, Device, LiveTool } from "./api";
import { exactMoment } from "./format";
import { OverviewPage } from "./OverviewPage";
import type { LiveLine } from "./liveActivity";

// The overview asks `/audit?device=` for its own machine's newest lines; the fleet window belongs
// to Activity. These tests drive the fetch the way the console answers it.
const auditApi = vi.hoisted(() => ({ audit: vi.fn() }));
vi.mock("./api", () => ({ api: { audit: auditApi.audit } }));

const live = vi.hoisted(() => ({ value: {} as Record<string, unknown> }));
vi.mock("./useLiveTools", () => ({ useLiveTools: () => live.value }));

const plugins = vi.hoisted(() => ({ list: vi.fn() }));
vi.mock("./pluginApi", () => ({ listPlugins: plugins.list }));

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
    auditPromise,
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
  else if (auditPromise) auditApi.audit.mockReturnValue(auditPromise);
  else auditApi.audit.mockResolvedValue(audit ?? calls);
  const onTools = vi.fn();
  const onActivity = vi.fn();
  const page = (extra: Record<string, unknown> = {}) => (
    <OverviewPage
      device={workshop}
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

describe("Device overview — the machine's live page", () => {
  it("states the machine in one status line, with its counts and one day line", async () => {
    draw();

    // The Device shell owns the destination's one h1 ("Overview"); this page names the machine
    // it is about and states its status.
    expect(screen.queryByRole("heading", { level: 1 })).toBeNull();
    expect(screen.getByRole("heading", { name: "workshop", level: 2 })).toBeInTheDocument();
    expect(await screen.findByText("online")).toBeInTheDocument();
    // The console's build belongs to the Account menu; the subtitle says what the machine is.
    expect(screen.queryByText(/console v/)).toBeNull();
    // The counts line, not the status line, carries the counts: six tools across four capabilities
    // (terminal, files, screen, plugins).
    const counts = await screen.findByRole("button", { name: /6 tools · 4 capabilities · 2 plugins/ });
    expect(counts).toHaveTextContent("6 tools · 4 capabilities · 2 plugins");

    // The fetched calls are a day line, not a second copy of Activity's rows.
    expect(await screen.findByText(/^Today 4 client calls/)).toBeInTheDocument();
    expect(document.querySelectorAll(".timeline-row")).toHaveLength(0);
  });

  it("puts the machine's start in its own head, after the machine's name", async () => {
    // **THE START BELONGS TO THE MACHINE HEAD (audit-44 F1).** It used to be the shell's first line,
    // above the h1 `Overview` and 35 px above the machine's name, so the machine read as a
    // subsection of its start time. It now sits beside the name it describes, and keeps the update
    // answer that belongs to this start because Overview draws no update card.
    const startedAt = Date.now() - 25 * 3_600_000;
    draw({
      device: {
        ...workshop,
        version: "0.3.42",
        host: { restarts: [{ at_ms: startedAt, version: "0.3.42" }] },
        update: {
          current: "0.3.42",
          verdict: { outcome: "ok", detail: "started 0.3.42" },
          last_update: { to: "0.3.42", outcome: "applied", at_ms: Date.now() - 12 * 60_000 }
        }
      }
    });

    const head = document.querySelector(".overview-head");
    if (!(head instanceof HTMLElement)) throw new Error("the machine head is not on the page");
    const name = within(head).getByRole("heading", { name: "workshop", level: 2 });
    const line = within(head).getByText("started 1d 1h ago (updated 12m ago)");
    expect(line).toHaveAttribute("title", exactMoment(startedAt));
    // The name is read first: the line follows it in the document, not the other way round.
    expect(name.compareDocumentPosition(line) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(await screen.findByText("online")).toBeInTheDocument();
  });

  it("puts the recovery line under the start, from the console's channel pair", async () => {
    // **THE START'S OTHER HALF (audit-44 F12).** `started 1h ago` alone reads as a normal start; the
    // machine that was away for a day left no trace. The console's own close/reopen pair is the
    // proven gap, and it is drawn beside the start it belongs to.
    const backAt = Date.now() - 2 * 3_600_000;
    draw({
      device: {
        ...workshop,
        host: { restarts: [{ at_ms: Date.now() - 3_600_000, version: "0.3.246" }] },
        update: {
          current: "0.3.246",
          verdict: { outcome: "ok", detail: "started 0.3.246" }
        },
        last_outage: { away_ms: 41 * 60_000, back_at_ms: backAt }
      },
      // The console started after the gap: it read both ends from the audit and can state them.
      consoleStartedAt: backAt + 3_600_000
    });

    const head = document.querySelector(".overview-head");
    if (!(head instanceof HTMLElement)) throw new Error("the machine head is not on the page");
    const start = within(head).getByText(/^started /);
    const line = within(head).getByText("back after 41 m");
    expect(line).toHaveAttribute("title", exactMoment(backAt));
    expect(line).toHaveAttribute("data-recovery-source", "console");
    // The recovery follows the start, in the same right-hand block: first when, then how it got there.
    expect(start.compareDocumentPosition(line) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(await screen.findByText("online")).toBeInTheDocument();
  });

  it("draws the day once, as one line, and draws none of Activity's rows", async () => {
    const now = new Date(2026, 10, 5, 14, 45, 0).getTime();
    const clock = vi.spyOn(Date, "now").mockReturnValue(now);
    try {
      draw({
        audit: [
          call({ ts_ms: now - 10_000, caller: "client", sentence: "Read the run history" }),
          call({ ts_ms: now - 20_000, caller: "client", sentence: "Ran a command" }),
          call({ ts_ms: now - 30_000, caller: "client", sentence: "Read the machine's state" })
        ],
        liveState: "live"
      });

      // THE STATE BAND KEEPS ITS OWN SHAPE. Presence, rate and the sparkline are read from the
      // machine's window; the band draws none of the rows.
      expect(await screen.findByText("Active now")).toBeInTheDocument();
      expect(screen.getByText("3 calls in the last minute")).toBeInTheDocument();
      const band = document.querySelector(".live-now");
      expect(band).not.toBeNull();
      expect(band?.querySelectorAll(".timeline-row")).toHaveLength(0);
      expect(band?.querySelectorAll(".live-entry")).toHaveLength(0);

      // THE DAY IS ONE LINE. It states the day, the count and the span the calls cover; the
      // sentences themselves are Activity's now, so none of them is drawn here.
      const line = await screen.findByText("Today 3 client calls · spanning 20 s");
      expect(line).toHaveClass("home-today");
      for (const sentence of ["Read the run history", "Ran a command", "Read the machine's state"]) {
        expect(screen.queryByText(sentence)).toBeNull();
      }
      expect(document.querySelectorAll(".timeline-row")).toHaveLength(0);
      expect(screen.queryByRole("heading", { level: 2, name: "Today" })).toBeNull();
      expect(screen.queryByRole("button", { name: /Console reads/ })).toBeNull();
      expect(screen.queryByRole("button", { name: /Show the rest of today/ })).toBeNull();
      const region = screen.getByRole("region", { name: "This machine's activity" });
      expect(within(region).getByText(/^Today 3 client calls/)).toBeInTheDocument();
      expect(within(region).getByRole("button", { name: /All activity/ })).toBeInTheDocument();
    } finally {
      clock.mockRestore();
    }
  });

  it("has no endpoint hero, URL or Copy control", () => {
    draw();

    expect(document.querySelector(".home-hero")).toBeNull();
    expect(screen.queryByText(/\/mcp\//)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Copy" })).not.toBeInTheDocument();
    expect(screen.queryByText("Recent calls")).not.toBeInTheDocument();
    expect(document.querySelector(".home-recent")).toBeNull();
  });

  it("leaves the one Connect door to the shell and says when the machine has no calls", async () => {
    // **ONE ACTION, ONE HOME.** The page used to draw `Connect a client` in its own machine head
    // while the shell drew the same sheet on the other six tabs; the shell's header is now the one
    // place, on every tab. This page only keeps the sentence that names what the door is for.
    draw({ audit: [] });

    expect(await screen.findByText(/No calls from workshop today/)).toBeInTheDocument();
    expect(screen.getByText(/Connect a client sets one up/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Connect a client" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Connect/ })).not.toBeInTheDocument();
  });

  it("keeps one All activity link, beside the day line, that opens the Activity page", async () => {
    const { onActivity } = draw();

    const all = screen.getAllByRole("button", { name: /All activity/ });
    expect(all).toHaveLength(1);
    const line = await screen.findByText(/^Today 4 client calls/);
    expect(all[0].closest(".home-day")).toBe(line.closest(".home-day"));
    fireEvent.click(all[0]);
    expect(onActivity).toHaveBeenCalledTimes(1);
  });

  it("counts this machine's live lines in its day line and no other machine's", async () => {
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

    // The page is this machine's: another machine's live line must not enter its count, and no
    // live sentence is drawn — the rows belong to Activity.
    expect(await screen.findByText(/^Today 2 client calls/)).toBeInTheDocument();
    for (const sentence of [
      "Ran the workshop tests",
      "Read a workshop note",
      "Built another machine"
    ]) {
      expect(screen.queryByText(sentence)).toBeNull();
    }
    expect(document.querySelectorAll(".timeline-row")).toHaveLength(0);
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
        await screen.findByText("Today 3 client calls · spanning 22 m")
      ).toBeInTheDocument();
    } finally {
      clock.mockRestore();
    }
  });

  it("keeps Activity's filters off the machine's page", async () => {
    // The page is the machine's state, not a second Activity: no rows means no machine chips, no
    // callers chip and no search to narrow the record here.
    draw();
    await screen.findByText(/^Today 4 client calls/);
    const region = screen.getByRole("region", { name: "This machine's activity" });
    expect(region.querySelector("input")).toBeNull();
    expect(screen.queryByRole("searchbox")).toBeNull();
    expect(screen.queryByRole("group", { name: "Filter by machine" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Clients only" })).toBeNull();
  });

  it("keeps older days off the machine's day line", async () => {
    const yesterday = new Date();
    yesterday.setDate(yesterday.getDate() - 1);
    yesterday.setHours(12, 0, 0, 0);
    draw({
      audit: [call({ ts_ms: yesterday.getTime(), caller: "client", tool: "old.tool", sentence: "Ran yesterday" })],
      liveLines: []
    });

    expect(screen.queryByText("Ran yesterday")).not.toBeInTheDocument();
    expect(await screen.findByText(/No calls from workshop today/)).toBeInTheDocument();
    expect(document.querySelectorAll(".timeline-row")).toHaveLength(0);
  });

  it("counts today's client calls, the failures among them, and the span of the day", async () => {
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
      // failures, and its span is the calls' own first-to-last moment — not a number the page invented.
      expect(
        await screen.findByText("Today 3 client calls · 1 failed · spanning 22 m")
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
        await screen.findByText("Today 2 client calls · spanning 12 m")
      ).toBeInTheDocument();
    } finally {
      clock.mockRestore();
    }
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

      expect(await screen.findByText("Today no client calls")).toBeInTheDocument();
      expect(await screen.findByText("No client calls from workshop today.")).toBeInTheDocument();
      expect(screen.queryByRole("button", { name: /Console reads/ })).toBeNull();
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
    draw({
      device: { ...workshop, online: false, last_frame_ms: Date.now() - 7 * 60_000 }
    });

    expect(screen.getByText("offline")).toBeInTheDocument();
    const notice = screen.getByRole("status");
    expect(notice).toHaveTextContent("This machine is offline.");
    expect(notice).toHaveTextContent("Last seen 7m ago.");
    expect(notice).toHaveTextContent("The console keeps trying; Retry works from here.");
    expect(notice).toHaveTextContent(
      "If it stays offline, open Device → Machine in the console for the machine's last facts."
    );
    expect(notice).toHaveTextContent(
      "The machine's local panel is http://127.0.0.1:18090/ — it opens only at the machine."
    );
    expect(screen.queryByRole("button", { name: "Copy" })).not.toBeInTheDocument();
    expect(screen.queryByText(/\/mcp\//)).not.toBeInTheDocument();
    expect(screen.getByText("Tool counts are unavailable while the machine is offline.")).toBeInTheDocument();
    expect(screen.queryByText(/0 tools/)).not.toBeInTheDocument();
  });

  it("does not draw a Now band for a machine that is away", () => {
    // audit-43 F5: the offline Overview said `Now · Quiet · no calls in the last minute` — a
    // positive claim about a present the console cannot see — while the Machine tab dated the same
    // silence. The band's question is live, so it is not drawn; the notice above dates the machine.
    live.value = {
      tools: [],
      stale: false,
      error: "this machine is not connected; connect it, then try again",
      offline: true,
      loading: false,
      retry: vi.fn()
    };
    draw({
      device: { ...workshop, online: false, last_frame_ms: Date.now() - (14 * 60 + 31) * 60_000 }
    });

    expect(screen.getByRole("status")).toHaveTextContent("Last seen 14h 31m ago.");
    expect(screen.queryByRole("heading", { name: "Now" })).toBeNull();
    expect(screen.queryByText("Quiet")).toBeNull();
    expect(screen.queryByText("Reconnecting")).toBeNull();
    expect(screen.queryByText(/no calls in the last minute/)).toBeNull();
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
    const sentence = screen.getByText(/5 attempts so far, the last failed/);
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
    expect(notice).toHaveTextContent("This machine is offline.");
    expect(notice).toHaveTextContent("The console keeps trying; Retry works from here.");
    expect(screen.queryByText(/attempts? so far/)).not.toBeInTheDocument();
  });

  it("shows the operator's Retry while it is in flight, disabled", () => {
    live.value = {
      tools: [],
      stale: false,
      error: "this machine is not connected; connect it, then try again",
      offline: true,
      loading: false,
      retrying: true,
      retried: null,
      retry: vi.fn()
    };
    draw({ device: { ...workshop, online: false } });

    const retry = screen.getByRole("button", { name: "Retrying…" });
    expect(retry).toBeDisabled();
    // The working state is the press's, not the poll's: no outcome exists until the read settles.
    expect(screen.queryByText(/last checked/)).toBeNull();
  });

  it("says the retry's same offline result with the time it settled", () => {
    const at = new Date(2026, 10, 7, 14, 5, 0).getTime();
    live.value = {
      tools: [],
      stale: false,
      error: "this machine is not connected; connect it, then try again",
      offline: true,
      loading: false,
      retrying: false,
      retried: {
        ok: false,
        at,
        error: "this machine is not connected; connect it, then try again"
      },
      retry: vi.fn()
    };
    draw({ device: { ...workshop, online: false } });

    // The result is the same one already on screen, and that is the point: it is said again, with
    // the moment the operator's own attempt finished, so the press cannot be mistaken for a poll.
    const notice = screen.getByRole("status");
    expect(notice).toHaveTextContent("Still offline · last checked 14:05.");
    expect(screen.getByText("Still offline · last checked 14:05.")).toBeInTheDocument();
    expect(within(notice).getByRole("button", { name: "Retry" })).toBeEnabled();
  });

  it("keeps the retry note silent when only the poll has read", () => {
    live.value = {
      tools: [],
      stale: false,
      error: "this machine is not connected; connect it, then try again",
      offline: true,
      loading: false,
      retrying: false,
      retried: null,
      retry: vi.fn()
    };
    const offline = { ...workshop, online: false };
    const { rerenderHub } = draw({ device: offline, tick: 1 });
    expect(screen.getByRole("button", { name: "Retry" })).toBeEnabled();

    // The five-second tick rereads the page; it must not announce a press nobody made.
    rerenderHub({ device: offline, tick: 2 });
    expect(screen.queryByText(/last checked/)).toBeNull();
    expect(screen.getByRole("button", { name: "Retry" })).toBeEnabled();
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

  it("states a failed tools read once, with the raw error as evidence", () => {
    live.value = {
      tools: [],
      stale: false,
      error: "the console could not read this machine's tools",
      offline: false,
      loading: false,
      retry: vi.fn()
    };
    draw();

    // audit-43 F9: the notice and the line where the counts would be said the same failure twice.
    // One notice now, and the envelope's raw sentence survives as its title rather than in the page.
    const notices = screen.getAllByRole("status");
    expect(notices).toHaveLength(1);
    expect(notices[0]).toHaveTextContent(
      "The machine is running. The console could not read its tool list, so the counts are not shown."
    );
    expect(notices[0]).toHaveAttribute("title", "the console could not read this machine's tools");
    expect(screen.getByText("online")).toBeInTheDocument();
    expect(
      screen.queryByText("Tool counts are unavailable until the console can read the machine's list.")
    ).toBeNull();
    expect(screen.queryByText(/^0 tools/)).not.toBeInTheDocument();
  });

  it("draws the read instead of a zero while the tool list is on the wire", () => {
    live.value = {
      tools: [],
      stale: false,
      error: null,
      offline: false,
      loading: true,
      retry: vi.fn()
    };
    draw();

    expect(screen.getByText("Reading the tool list…")).toBeInTheDocument();
    expect(screen.queryByText(/^0 tools/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /tools ·/ })).not.toBeInTheDocument();
  });

  it("says the counts are not shown when the failed read left no list", () => {
    live.value = {
      tools: [],
      stale: false,
      error: "the console could not read this machine's tools",
      offline: false,
      loading: false,
      retry: vi.fn()
    };
    draw();

    // "may be stale" needs counts to have a subject; a failed read with no list draws none.
    expect(
      screen.getByText(
        "The machine is running. The console could not read its tool list, so the counts are not shown."
      )
    ).toBeInTheDocument();
    expect(screen.queryByText(/may be stale/)).toBeNull();
    expect(screen.queryByText(/Tool counts are unavailable/)).toBeNull();
    expect(
      within(screen.getByRole("status")).getByRole("button", { name: "Retry" })
    ).toBeInTheDocument();
  });

  it("draws a true zero once the read answered with no tools", async () => {
    live.value = {
      tools: [],
      stale: false,
      error: null,
      offline: false,
      loading: false,
      retry: vi.fn()
    };
    draw();

    // This zero is the machine's answer, not a placeholder for a read still in flight.
    expect(
      await screen.findByRole("button", { name: /0 tools · 0 capabilities/ })
    ).toBeInTheDocument();
    expect(screen.queryByText("Reading the tool list…")).toBeNull();
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

    expect(await screen.findByText("online")).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: /1 tool · 1 capability · 0 plugins/ })).toBeInTheDocument();
    expect(plugins.list).not.toHaveBeenCalled();
  });

  it("asks the console for this machine's own newest lines, not the fleet window", async () => {
    draw({ audit: [] });

    await waitFor(() => expect(auditApi.audit).toHaveBeenCalledWith(1000, workshop.id));
    expect(auditApi.audit).toHaveBeenCalledTimes(1);
  });

  it("does not claim there were no calls when this machine's audit could not be read", async () => {
    draw({ auditError: true });

    expect(
      await screen.findByText(
        "The console could not read this machine's activity, so this day line may be incomplete."
      )
    ).toBeInTheDocument();
    expect(screen.queryByText(/No calls from workshop today/)).toBeNull();
  });

  it("puts the capability line above the day line", async () => {
    draw();

    const counts = await screen.findByRole("button", {
      name: /6 tools · 4 capabilities · 2 plugins/
    });
    const day = screen.getByRole("region", { name: "This machine's activity" });
    // The line answers "what can this machine do now"; it sits with the Now band, before the day.
    expect(counts.compareDocumentPosition(day) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("states a one-call day's moment, because one call has no span", async () => {
    const now = new Date(2026, 10, 5, 14, 45, 0).getTime();
    const clock = vi.spyOn(Date, "now").mockReturnValue(now);
    try {
      draw({
        audit: [
          call({ ts_ms: new Date(2026, 10, 5, 14, 32).getTime(), caller: "client" })
        ]
      });

      expect(await screen.findByText("Today 1 client call · last 14:32")).toBeInTheDocument();
      expect(screen.queryByText(/spanning/)).toBeNull();
    } finally {
      clock.mockRestore();
    }
  });

  it("states no span when the window is cut before the day's first call", async () => {
    // Every fetched line is today and the window is full, so the oldest line on the page is not
    // the day's first call: a span counted from it would start late, and the line says none.
    const start = new Date();
    start.setHours(0, 0, 0, 0);
    const base = start.getTime() + 2 * 3_600_000;
    const thousand: AuditLine[] = Array.from({ length: 1000 }, (_, index) =>
      call({
        ts_ms: base - index,
        caller: "client",
        tool: index % 2 === 0 ? "terminal.create" : "files.read"
      })
    );
    draw({ audit: thousand, device: { ...workshop, today: { client_calls: 1000, failed: 0 } } });

    expect(await screen.findByText("Today 1000 client calls")).toBeInTheDocument();
    expect(screen.queryByText(/spanning/)).toBeNull();
    expect(document.querySelectorAll(".timeline-row")).toHaveLength(0);
  });

  it("keeps the one day line while a busy day's rows stay in Activity", async () => {
    draw({ audit: todayCalls(75) });

    // A day's 75 calls are one line here and 75 rows in Activity: no fold, no cap sentence and no
    // second copy of the record on this page.
    expect(await screen.findByText(/^Today 75 client calls · spanning/)).toBeInTheDocument();
    expect(document.querySelectorAll(".timeline-row")).toHaveLength(0);
    expect(screen.queryByRole("button", { name: /Show the rest of today/ })).toBeNull();
    expect(screen.getByRole("button", { name: /All activity/ })).toBeInTheDocument();
  });
  it("draws no row of Activity's record", async () => {
    draw();
    expect(await screen.findByText(/^Today 4 client calls/)).toBeInTheDocument();
    expect(document.querySelectorAll(".timeline-row")).toHaveLength(0);
  });

  it("draws no Today heading of its own", async () => {
    draw();
    await screen.findByText(/^Today 4 client calls/);
    expect(screen.queryByRole("heading", { name: "Today" })).toBeNull();
    expect(document.querySelector(".timeline-day-label")).toBeNull();
  });

  it("draws no Console reads chip, even when the window is mostly the console's", async () => {
    draw({ audit: [
      ...Array.from({ length: 4 }, (_, index) =>
        call({ ts_ms: Date.now() - index * 1_000, caller: "client", tool: `client.${index}` })
      ),
      ...Array.from({ length: 56 }, (_, index) =>
        call({ ts_ms: Date.now() - (4 + index) * 1_000, caller: "console", tool: `console.${index}` })
      )
    ] });

    expect(await screen.findByText(/^Today 4 client calls/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Console reads/ })).toBeNull();
    expect(document.querySelectorAll(".timeline-row")).toHaveLength(0);
  });

  it("draws no Unattributed chip", async () => {
    draw({ audit: [
      ...Array.from({ length: 3 }, (_, index) =>
        call({ ts_ms: Date.now() - index * 1_000, caller: "client", tool: `client.${index}` })
      ),
      ...Array.from({ length: 111 }, (_, index) =>
        call({ ts_ms: Date.now() - (3 + index) * 1_000, tool: `unknown.${index}` })
      )
    ] });

    expect(await screen.findByText(/^Today 3 client calls/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Unattributed/ })).toBeNull();
    expect(document.querySelectorAll(".timeline-row")).toHaveLength(0);
  });

  it("offers no rest-of-today fold", async () => {
    draw({ audit: todayCalls(75) });
    expect(await screen.findByText(/^Today 75 client calls/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Show the rest of today/ })).toBeNull();
  });

  it("draws no window sentence when the window is cut", async () => {
    const start = new Date();
    start.setHours(0, 0, 0, 0);
    const base = start.getTime() + 2 * 3_600_000;
    draw({
      audit: Array.from({ length: 1000 }, (_, index) =>
        call({ ts_ms: base - index, caller: "client", tool: `tool.${index}` })
      ),
      device: { ...workshop, today: { client_calls: 1000, failed: 0 } }
    });

    expect(await screen.findByText("Today 1000 client calls")).toBeInTheDocument();
    expect(document.querySelector(".timeline-capped")).toBeNull();
    expect(screen.queryByText(/Showing the newest/)).toBeNull();
  });

  it("draws no repeats sentence", async () => {
    const start = new Date();
    start.setHours(0, 0, 0, 0);
    const base = start.getTime() + 3_600_000;
    const calls = Array.from({ length: 60 }, (_, index) =>
      call({ ts_ms: base - index * 1_000, caller: "client", tool: `tool.${index}` })
    );
    draw({
      audit: [...calls, ...calls.slice(0, 4).map((line) => ({ ...line }))],
      device: { ...workshop, today: { client_calls: 60, failed: 0 } }
    });

    expect(await screen.findByText(/^Today 60 client calls/)).toBeInTheDocument();
    expect(screen.queryByText(/repeats? not drawn/)).toBeNull();
  });

  it("draws no window-age note", async () => {
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
      await screen.findByText(/^Today 1 client call/);
      clock.mockReturnValue(now + 2 * 60_000);
      rerenderHub({
        liveLines: [liveCall({ ts_ms: now + 2 * 60_000 - 1_000, caller: "client" })]
      });
      expect(screen.queryByText(/^Read \d/)).toBeNull();
    } finally {
      clock.mockRestore();
    }
  });

  it("does not let the window's lines become the day's count", async () => {
    const now = new Date(2026, 10, 5, 14, 45, 0).getTime();
    const clock = vi.spyOn(Date, "now").mockReturnValue(now);
    try {
      draw({
        audit: [14_10, 14_20, 14_32].map((hour) =>
          call({ ts_ms: new Date(2026, 10, 5, 14, hour % 100, 0).getTime(), caller: "client" })
        ),
        device: { ...workshop, today: { client_calls: 10, failed: 0 } }
      });
      expect(
        await screen.findByText("Today 10 client calls · spanning 22 m")
      ).toBeInTheDocument();
      expect(screen.queryByText(/3 client calls/)).toBeNull();
    } finally {
      clock.mockRestore();
    }
  });

  it("keeps the one All activity door out of the Now band", async () => {
    draw();
    await screen.findByText(/^Today 4 client calls/);
    expect(screen.getAllByRole("button", { name: /All activity/ })).toHaveLength(1);
    expect(document.querySelectorAll(".live-now button")).toHaveLength(0);
  });

  it("states no span before the window that proves it has answered", async () => {
    const now = new Date(2026, 10, 5, 14, 45, 0).getTime();
    const clock = vi.spyOn(Date, "now").mockReturnValue(now);
    try {
      const answer = deferred<AuditLine[]>();
      draw({
        auditPromise: answer.promise,
        device: { ...workshop, today: { client_calls: 2, failed: 0 } }
      });

      // `/fleet` has the day's count, but the window that proves the first and last call has not
      // answered: the line states the count alone rather than a span from half a window.
      expect(await screen.findByText("Today 2 client calls")).toBeInTheDocument();
      expect(screen.queryByText(/spanning/)).toBeNull();

      await act(async () => {
        answer.resolve([
          call({ ts_ms: now - 60_000, caller: "client" }),
          call({ ts_ms: now - 120_000, caller: "client" })
        ]);
      });
      expect(
        await screen.findByText("Today 2 client calls · spanning 1 m")
      ).toBeInTheDocument();
    } finally {
      clock.mockRestore();
    }
  });

});
