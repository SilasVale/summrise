import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Device } from "./api";
import { DevicePageShell, deviceJournal } from "./DevicePageShell";
import { clockTime } from "./format";
import type { LiveLine } from "./liveActivity";
import type { DeviceTab } from "./Rail";

const BOOT_AT = Date.now() - 25 * 3_600_000;

const MACHINE: Device = {
  id: "d1",
  name: "workshop",
  online: false,
  version: "0.3.42",
  // The channel opened three hours ago; the last FRAME was seven minutes ago. The notice must say
  // the frame's moment, not the socket's.
  last_seen_ms: Date.now() - 3 * 3600_000,
  last_frame_ms: Date.now() - 7 * 60_000,
  host: {
    // The newest boot is when the running process started. It is the only moment that can prove
    // an updater's answer belongs to this process.
    restarts: [{ at_ms: BOOT_AT, version: "0.3.42", pid: 42 }]
  }
};

function renderShell(
  tab: DeviceTab,
  device: Device = MACHINE,
  onConnect = () => undefined,
  journalLines: readonly LiveLine[] = []
) {
  return render(
    <DevicePageShell
      tab={tab}
      onTab={() => undefined}
      devices={[device]}
      chosen={device}
      onChoose={() => undefined}
      onConnect={onConnect}
      journalLines={journalLines}
    >
      {() => <p>page contents</p>}
    </DevicePageShell>
  );
}

/** One synthetic session row, shaped exactly as `useLiveActivity` publishes one. */
function journalLine(ts_ms: number, sentence: string, overrides: Partial<LiveLine> = {}): LiveLine {
  return {
    ts_ms,
    kind: "session",
    status: 200,
    ok: true,
    device_id: MACHINE.id,
    live: true,
    sentence,
    ...overrides
  };
}

describe("DevicePageShell's Events strip", () => {
  it("draws this machine's newest journal rows first, with HH:MM and the kind it has to name", () => {
    const earlier = journalLine(
      new Date(2026, 10, 12, 9, 5).getTime(),
      "terminal 2 opened on workshop"
    );
    const newest = journalLine(
      new Date(2026, 10, 12, 9, 8).getTime(),
      "session.idle on workshop"
    );
    const middle = journalLine(
      new Date(2026, 10, 12, 9, 6).getTime(),
      "workshop started"
    );
    // A snapshot row was read back, not watched live, so it is not this strip's event.
    const snapshot = journalLine(
      new Date(2026, 10, 12, 9, 9).getTime(),
      "terminal 4 opened on workshop",
      { live: false }
    );
    const otherMachine = journalLine(
      new Date(2026, 10, 12, 9, 10).getTime(),
      "terminal 9 opened on attic",
      { device_id: "d2" }
    );

    renderShell("overview", MACHINE, () => undefined, [
      earlier,
      newest,
      middle,
      snapshot,
      otherMachine
    ]);

    expect(screen.getByText("Events")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Events" })).toBeInTheDocument();
    expect(screen.getByText("Live")).toBeInTheDocument();
    const rows = within(screen.getByRole("list")).getAllByRole("listitem");
    expect(rows).toHaveLength(3);
    expect(rows.map((row) => row.textContent)).toEqual([
      expect.stringContaining(newest.sentence),
      expect.stringContaining(middle.sentence),
      expect.stringContaining(earlier.sentence)
    ]);
    expect(rows[0].querySelector("time")).toHaveTextContent(clockTime(newest.ts_ms));
    // `session.idle on workshop` already names its kind; the two human sentences do not.
    expect(rows[0].querySelector(".device-event-kind")).toBeNull();
    expect(rows[2].querySelector(".device-event-kind")).toHaveTextContent("session");
    expect(screen.queryByText(otherMachine.sentence)).toBeNull();
    expect(screen.queryByText(snapshot.sentence)).toBeNull();
  });

  it("says the journal is empty before the machine has opened a session or run a plugin", () => {
    renderShell("overview");

    expect(screen.getByText("No sessions or plugin events yet on this machine.")).toBeInTheDocument();
    expect(screen.queryByRole("list")).toBeNull();
    expect(screen.queryByText("Live")).toBeNull();
  });

  it("still offers the empty sentence when the feed holds only call rows", () => {
    // Calls are Activity's rows, and this strip lists sessions and plugin events. A feed of calls
    // alone is an empty strip, and its sentence must not claim the machine has done nothing: it
    // names the two things this strip watches, which is what the reader is being told there are none of.
    const calls: LiveLine[] = [
      {
        ts_ms: 9_005,
        kind: "mcp",
        status: 200,
        ok: true,
        device_id: MACHINE.id,
        tool: "files.read",
        trace: "call-7",
        live: true,
        sentence: "files.read ran on workshop"
      },
      {
        ts_ms: 9_006,
        kind: "mcp",
        status: 424,
        ok: false,
        device_id: MACHINE.id,
        tool: "runs.exec",
        live: true,
        sentence: "runs.exec was refused on workshop (denied)"
      }
    ];

    renderShell("overview", MACHINE, () => undefined, calls);

    expect(screen.getByText("No sessions or plugin events yet on this machine.")).toBeInTheDocument();
    expect(screen.queryByText("files.read ran on workshop")).toBeNull();
    expect(screen.queryByText("Live")).toBeNull();
  });
});

describe("deviceJournal", () => {
  it("keeps only this machine's watched session rows, newest first, at twenty", () => {
    const rows = Array.from({ length: 25 }, (_, index) =>
      journalLine(1_000 + index, `terminal ${index} opened on workshop`)
    );
    const feed = [
      ...rows,
      journalLine(9_000, "terminal 9 opened on attic", { device_id: "d2" }),
      journalLine(9_001, "Read a file", { kind: "mcp" }),
      journalLine(9_002, "terminal 9 opened on workshop", { live: false })
    ];

    const journal = deviceJournal(feed, MACHINE.id);

    expect(journal).toHaveLength(20);
    expect(journal.map((line) => line.ts_ms)).toEqual(
      Array.from({ length: 20 }, (_, index) => 1_024 - index)
    );
    expect(journal.every((line) => line.device_id === MACHINE.id && line.kind === "session")).toBe(
      true
    );
    expect(deviceJournal(feed, null)).toEqual([]);
  });

  it("drops a call's row: the strip lists sessions and plugin events, not calls", () => {
    // A call the machine's own journal reports is Activity's row, and `useLiveActivity` draws it as
    // an `mcp` line. This strip's sentence promises sessions and plugin events, so a call must not
    // appear under it — that would read as a session that never opened.
    const call: LiveLine = {
      ts_ms: 9_003,
      kind: "mcp",
      status: 200,
      ok: true,
      device_id: MACHINE.id,
      tool: "files.read",
      trace: "call-7",
      live: true,
      sentence: "files.read ran on workshop"
    };
    const refused: LiveLine = {
      ...call,
      ts_ms: 9_004,
      status: 424,
      ok: false,
      tool: "runs.exec",
      sentence: "runs.exec was refused on workshop (denied)"
    };
    const session = journalLine(9_005, "terminal 2 opened on workshop");

    expect(deviceJournal([call, refused, session], MACHINE.id).map((line) => line.sentence)).toEqual(
      ["terminal 2 opened on workshop"]
    );
  });
});

describe("DevicePageShell's Connect door", () => {
  it("names the Device's Connect door for the sheet it opens and calls it", () => {
    const onConnect = vi.fn();
    renderShell("machine", MACHINE, onConnect);

    fireEvent.click(screen.getByRole("button", { name: "Connect a client" }));

    expect(onConnect).toHaveBeenCalledTimes(1);
  });
});

describe("DevicePageShell offline notice", () => {
  it("says the machine is down on the tabs that ask it for something", () => {
    renderShell("files");
    expect(screen.getByRole("status")).toHaveTextContent("This machine is offline.");
    expect(screen.getByRole("status")).toHaveTextContent(/nothing on this page can reach it/i);
  });

  it("says when the machine was last heard, from its last frame", () => {
    renderShell("files");
    expect(screen.getByRole("status")).toHaveTextContent(/last seen 7m ago/i);
  });

  it("ages the notice from the close when the console has no frame to read", () => {
    // The console restarted mid-outage: no frame, but the audit's newest `device.left` is the moment
    // the silence started. The registry's older connect time must not be the age the notice shows.
    renderShell("files", {
      ...MACHINE,
      last_frame_ms: null,
      last_left_ms: Date.now() - (1 * 60 + 49) * 60_000
    });

    expect(screen.getByRole("status")).toHaveTextContent(/last seen 1h 49m ago/i);
  });

  it("carries the one notice on Tools too, above both of its sections", () => {
    renderShell("tools");
    expect(screen.getAllByRole("status")).toHaveLength(1);
    expect(screen.getByRole("status")).toHaveTextContent("This machine is offline.");
  });

  it("leaves the Terminal tab alone, because that page carries its own notice", () => {
    // The terminal workspace is mounted outside this shell and stays mounted across destinations;
    // a second copy here would stack the same sentence twice on one screen.
    renderShell("terminal");
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("says nothing on a machine that is online", () => {
    renderShell("files", { ...MACHINE, online: true });
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("leaves the machine's start story to the page that answers it", () => {
    // **THE START BELONGS TO THE MACHINE HEAD, NOT TO EVERY TAB (audit-44 F1).** The shell used to
    // print `started … ago` under the h1 on all seven tabs, so Files, Terminal and Tools opened
    // with an age before their own answer, and Overview put it above the machine's name. Each page
    // now owns the sentence where it is the answer; the shell draws no part of it.
    const withStart = {
      ...MACHINE,
      update: {
        current: "0.3.42",
        verdict: { outcome: "ok", detail: "started 0.3.42" },
        last_update: { to: "0.3.42", outcome: "applied", at_ms: Date.now() - 12 * 60_000 }
      }
    };
    for (const tab of [
      "overview",
      "machine",
      "terminal",
      "screen",
      "files",
      "runs",
      "tools"
    ] as const) {
      const { unmount } = renderShell(tab, withStart);
      expect(document.querySelector(".started-line")).toBeNull();
      unmount();
    }
  });
});

describe("DevicePageShell without a machine", () => {
  it("is the picker: its title is the question, and no tab strip or Connect is drawn", () => {
    // audit-40 F7: the page was headed `Terminal` with the full tab strip above `Choose a machine`,
    // inviting five surfaces that cannot run until a machine is chosen. The picker is the page now.
    const onTab = vi.fn();
    const onChoose = vi.fn();
    render(
      <DevicePageShell
        tab="terminal"
        onTab={onTab}
        devices={[MACHINE]}
        chosen={null}
        onChoose={onChoose}
        onConnect={() => undefined}
      >
        {() => <p>page contents</p>}
      </DevicePageShell>
    );

    expect(
      screen.getByRole("heading", { name: "Choose a machine", level: 1 })
    ).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Terminal" })).toBeNull();
    expect(screen.queryByRole("navigation", { name: "Device views" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Connect a client" })).toBeNull();
    // The picker itself is still the page's action.
    fireEvent.click(screen.getByRole("button", { name: "Open workshop" }));
    expect(onChoose).toHaveBeenCalledWith(MACHINE.id);
    expect(screen.queryByText("page contents")).toBeNull();
  });

  it("hands the picker the fleet read's state, not only the list it holds", () => {
    // audit-43 F4: with `/fleet` pending or failed the destination said `No devices are enrolled
    // yet.` because the picker only knew `devices`. The shell is the one place that knows the read.
    const onRetryFleet = vi.fn();
    render(
      <DevicePageShell
        tab="terminal"
        onTab={() => undefined}
        devices={[]}
        chosen={null}
        onChoose={() => undefined}
        onConnect={() => undefined}
        fleetRead={{ state: "failed", reason: "Failed to fetch" }}
        onRetryFleet={onRetryFleet}
      >
        {() => <p>page contents</p>}
      </DevicePageShell>
    );

    expect(screen.getByRole("status")).toHaveTextContent("The machine list could not be read.");
    expect(screen.queryByText(/No devices are enrolled yet/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetryFleet).toHaveBeenCalledTimes(1);
  });
});

describe("DevicePageShell's one Connect door", () => {
  it("draws the same named door on every Device tab, Overview included", () => {
    // **ONE LABEL, ONE HOME.** The six tabs used to say `Connect` in the shell header while
    // Overview said `Connect a client` in its content; the shell's head is the one place on every
    // tab now, and its visible label is the sheet's own name.
    for (const tab of ["overview", "machine", "terminal", "screen", "files", "runs", "tools"] as const) {
      const { unmount } = renderShell(tab, MACHINE);
      const door = screen.getByRole("button", { name: "Connect a client" });
      expect(door).toHaveTextContent("Connect a client");
      unmount();
    }
  });

  it("keeps the Overview offline notice on the page, whose Retry is the live read's", () => {
    renderShell("overview", MACHINE);

    // The page inside owns the machine's name and status, and its offline notice: the notice's
    // Retry is the live read's, not the shell's tab notice.
    expect(screen.getByRole("heading", { name: "Overview", level: 1 })).toBeInTheDocument();
    expect(screen.queryByRole("status")).toBeNull();
  });
});

/**
 * jsdom has no layout, so the strip's and the tab's boxes are supplied: the 390 measurements from
 * audit-41 F2 (a 358 px strip at 16..374, and `Machine` at 389..461 while the strip is unscrolled).
 * The tab's box follows `scrollLeft`, as it would in a browser, so a second resize can prove the
 * strip does not scroll a tab that is already in the band.
 */
function tabGeometry() {
  let bandRight = 1000;
  const rect = (left: number, width: number): DOMRect => ({
    x: left,
    y: 0,
    top: 0,
    bottom: 40,
    left,
    right: left + width,
    width,
    height: 40,
    toJSON: () => ({})
  });
  const original = HTMLElement.prototype.getBoundingClientRect;
  HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement): DOMRect {
    if (this.classList.contains("device-tabs")) return rect(16, bandRight - 16);
    if (this.getAttribute("aria-current") === "page") {
      return rect(389 - (this.parentElement?.scrollLeft ?? 0), 72);
    }
    return rect(0, 0);
  };
  return {
    phone: () => {
      bandRight = 374;
    },
    restore: () => {
      HTMLElement.prototype.getBoundingClientRect = original;
    }
  };
}

describe("DevicePageShell's tab strip at 390", () => {
  it("brings a mid-strip route's tab into the strip", () => {
    const geometry = tabGeometry();
    geometry.phone();
    try {
      renderShell("machine");
      const strip = screen.getByRole("navigation", { name: "Device views" });
      expect(screen.getByRole("button", { name: "Machine" })).toHaveAttribute(
        "aria-current",
        "page"
      );
      // 389..461 against the strip's 16..374: 87 px of the tab starts past the edge, so the strip
      // travels exactly that far and the tab's right edge meets the strip's.
      expect(strip.scrollLeft).toBe(87);
    } finally {
      geometry.restore();
    }
  });

  it("brings it back when a wide window becomes the phone's, and not again once it is in band", () => {
    const geometry = tabGeometry();
    try {
      renderShell("machine");
      const strip = screen.getByRole("navigation", { name: "Device views" });
      // The 1000 px band holds every tab, so the route's selection does not move the strip.
      expect(strip.scrollLeft).toBe(0);
      geometry.phone();
      fireEvent(window, new Event("resize"));
      expect(strip.scrollLeft).toBe(87);
      // The tab's box follows the scroll: it is inside the band now, so a second resize is a no-op.
      fireEvent(window, new Event("resize"));
      expect(strip.scrollLeft).toBe(87);
    } finally {
      geometry.restore();
    }
  });
});

/**
 * jsdom reports 0 for the strip's `clientWidth` and `scrollWidth`; the browser's 390 measurements
 * (audit-44 F2) are supplied on the element. The strip is 358 px inside a 447 px row of tabs, so
 * 89 px are reachable by scrolling.
 */
function stripOverflow(strip: HTMLElement, clientWidth: number, scrollWidth: number) {
  Object.defineProperty(strip, "clientWidth", { configurable: true, value: clientWidth });
  Object.defineProperty(strip, "scrollWidth", { configurable: true, value: scrollWidth });
  fireEvent.scroll(strip);
}

describe("DevicePageShell's tab strip affordance at 390", () => {
  it("shows a way back to the tabs off the left edge, and it returns them", () => {
    const geometry = tabGeometry();
    geometry.phone();
    try {
      renderShell("machine");
      const strip = screen.getByRole("navigation", { name: "Device views" });
      // The route brought `Machine` into the band (87 px in), so `Overview` and `Screen` sit left
      // of it; the measured 390 strip has 89 px of travel and the leftover 2 px is not a hidden tab.
      expect(strip.scrollLeft).toBe(87);
      stripOverflow(strip, 358, 447);

      const earlier = screen.getByRole("button", { name: "Show earlier device views" });
      expect(screen.queryByRole("button", { name: "Show later device views" })).toBeNull();

      fireEvent.click(earlier);

      // One page of tabs back: the strip starts at `Overview`, which is now the first visible tab.
      expect(strip.scrollLeft).toBe(0);
      expect(screen.queryByRole("button", { name: "Show earlier device views" })).toBeNull();
      expect(
        screen.getByRole("button", { name: "Show later device views" })
      ).toBeInTheDocument();
    } finally {
      geometry.restore();
    }
  });

  it("shows the later-tabs affordance only while tabs are off the right edge", () => {
    const geometry = tabGeometry();
    try {
      renderShell("machine");
      const strip = screen.getByRole("navigation", { name: "Device views" });
      // The wide window wraps every tab, so there is nothing off either edge.
      stripOverflow(strip, 1000, 447);
      expect(screen.queryByRole("button", { name: /Show (earlier|later) device views/ })).toBeNull();

      // The phone's strip: at scrollLeft 0 the last tabs are the hidden ones.
      stripOverflow(strip, 358, 447);
      const later = screen.getByRole("button", { name: "Show later device views" });
      expect(screen.queryByRole("button", { name: "Show earlier device views" })).toBeNull();

      fireEvent.click(later);
      // The whole remaining travel: 447 px of tabs in a 358 px band.
      expect(strip.scrollLeft).toBe(89);
      expect(
        screen.getByRole("button", { name: "Show earlier device views" })
      ).toBeInTheDocument();
    } finally {
      geometry.restore();
    }
  });
});
