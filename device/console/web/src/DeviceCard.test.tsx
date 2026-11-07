import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DeviceCard, type Device } from "./DeviceCard";
import { exactMoment } from "./format";

const GIB = 1024 ** 3;
const BOOT_AT = Date.now() - 25 * 3_600_000;

const device: Device = {
  id: "dev-1",
  name: "workshop",
  online: true,
  version: "0.3.42",
  // The row's age is the last FRAME: `last_frame_ms` is the one moment it reads.
  last_frame_ms: Date.now() - 15_000,
  host: {
    os: { name: "Windows", version: "11" },
    uptime_s: 90_061,
    // This app's own boot journal: its newest entry is when the RUNNING process started, which is
    // the moment an updater's answer must be after to belong to this process.
    restarts: [{ at_ms: BOOT_AT, version: "0.3.42", channel: "stable", pid: 42 }],
    memory: { total_bytes: 8 * GIB, available_bytes: 2 * GIB },
    disks: [{ name: "/", total_bytes: 100 * GIB, free_bytes: 10 * GIB }]
  }
};

const callbacks = {
  onSelect: vi.fn(),
  onRename: vi.fn(),
  onRemove: vi.fn(),
  onDiagnostics: vi.fn()
};

/**
 * The row is a real table row now: it is rendered inside the table Home draws, so a test sees the
 * same cells a browser does (a `<tr>` outside `<tbody>` is not a table).
 */
function draw(overrides: Partial<Device> = {}) {
  return render(
    <table>
      <tbody>
        <DeviceCard device={{ ...device, ...overrides }} selected={false} {...callbacks} />
      </tbody>
    </table>
  );
}

describe("DeviceCard — one row on the fleet", () => {
  it("keeps only the decision facts on the row", () => {
    const { container } = draw();

    expect(screen.getByText("workshop")).toBeInTheDocument();
    expect(screen.getByText("online · seen 15s ago")).toBeInTheDocument();
    expect(screen.getByText("v0.3.42")).toBeInTheDocument();
    // The version appears once on the row: the chip is the surface's one copy, and nothing in the
    // device's story repeats it.
    expect(container.textContent?.match(/0\.3\.42/g)).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Open workshop" })).toBeInTheDocument();
    // The status is a coloured dot and a word, in its own cell.
    const row = screen.getByRole("row");
    expect(row.querySelector(".fleet-status .dot.dot-ok")).not.toBeNull();
    expect(row.querySelector(".fleet-status .fleet-state")).toHaveTextContent("online");

    // The row is the target; there is no second per-row Connect action competing with the screen's
    // one primary action, and the machine's own facts have one home: Machine.
    expect(screen.queryByRole("button", { name: /Connect/ })).not.toBeInTheDocument();
    expect(screen.queryByText(/Gib free/)).not.toBeInTheDocument();
    expect(container.querySelector(".vitals-chart")).toBeNull();
    expect(container.querySelector("svg.vitals")).toBeNull();
  });

  it("puts the management actions in the row's overflow", () => {
    const { container } = draw();

    const menu = container.querySelector("details.row-menu");
    expect(menu).not.toBeNull();
    expect(menu).not.toHaveAttribute("open");
    expect(menu).toContainElement(screen.getByRole("button", { name: "Copy diagnostics" }));
    expect(menu).toContainElement(screen.getByRole("button", { name: "Rename" }));
    expect(menu).toContainElement(screen.getByRole("button", { name: "Remove" }));
    // No paragraph explaining where the facts went: the row says everything it needs to.
    expect(container.querySelector(".card-details-body")).toBeNull();
  });

  it("is one table row with a cell per column", () => {
    const { container } = draw();

    const row = container.querySelector("tr.fleet-row");
    expect(row).not.toBeNull();
    expect(row?.querySelectorAll("td")).toHaveLength(5);
    expect(row?.querySelector(".fleet-machine .fleet-name")).toHaveTextContent("workshop");
    expect(row?.querySelector(".fleet-version .fleet-build")).toHaveTextContent("v0.3.42");
    // A device that sent no update story has no start to state; the cell says so rather than
    // borrowing a moment.
    expect(row?.querySelector(".fleet-started .fleet-empty")).toHaveTextContent("—");
  });

  it("draws no client-call counters: that is Activity's noun", () => {
    // The measured old row: `554 client calls · 53 failed · since 03:12`. The new Home states the
    // fleet and what needs attention; how many calls a machine served belongs to the record.
    const midnight = new Date();
    midnight.setHours(0, 0, 0, 0);
    const since = midnight.getTime() + (3 * 60 + 12) * 60_000;
    const { container } = draw({ today: { client_calls: 554, failed: 53, since_ms: since } });

    expect(container.querySelector(".fleet-work")).toBeNull();
    expect(screen.queryByText(/client call/)).toBeNull();
    expect(screen.queryByText(/failed/)).toBeNull();
    expect(screen.queryByText(/554/)).toBeNull();
  });

  it("draws no reconnect sentence: the attempt has no moment of its own", () => {
    // The measured row: both online machines carried "Reconnected after 1 attempt — the console
    // accepted the channel and then dropped it" with no date, so the 09:41 restart read at 10:07 as
    // if the channel had just dropped. `/fleet` serves `last_attempt` as a count and a reason, with
    // no moment; any date on the row would have to borrow the last frame's, and the reconnect is
    // before that frame, not at it. So the row shows neither the count nor the reason: the state
    // line already dates presence, and Copy diagnostics keeps the attempt as a record read on
    // demand, with the moment the console last heard the machine.
    const { container } = draw({
      last_attempt: { attempts: 1, reason: "the console accepted the channel and then dropped it" }
    });

    expect(container.querySelector(".fleet-attempt")).toBeNull();
    expect(screen.queryByText(/Reconnected after/)).not.toBeInTheDocument();
    expect(screen.queryByText(/the console accepted the channel/)).not.toBeInTheDocument();
    // What the row does say: the state with its age and the build.
    expect(screen.getByText("online · seen 15s ago")).toBeInTheDocument();
    expect(screen.getByText("v0.3.42")).toBeInTheDocument();
  });

  it("draws no line for an attempt the device reported without a reason", () => {
    const { container } = draw({ last_attempt: { attempts: 2, reason: "" } });
    expect(container.querySelector(".fleet-attempt")).toBeNull();
    expect(screen.queryByText(/Reconnected after/)).not.toBeInTheDocument();
  });

  it("draws no attempt line at all when there is no attempt to report", () => {
    const { container } = draw({ last_attempt: null });
    expect(container.querySelector(".fleet-attempt")).toBeNull();
  });

  it("says offline in words, with when it was last heard", () => {
    draw({
      online: false,
      last_frame_ms: Date.now() - 3 * 3600_000,
      version: null,
      host: null
    });

    expect(screen.getByText("offline · last seen 3h ago")).toBeInTheDocument();
    expect(screen.getByRole("row").querySelector(".fleet-status .dot.dot-off")).not.toBeNull();
  });

  it("ages a device with no frame from the close the audit recorded", () => {
    // The measured DESKTOP-14RJCR8 case: the console restart forgot the frame, and the only moment
    // it has is the newest `device.left` (08:18:06), not the registry's older 08:06:04. The row says
    // the silence's age, and it says it with the minutes — "2h ago" is this item's bug.
    draw({
      online: false,
      last_frame_ms: null,
      last_left_ms: Date.now() - (1 * 60 + 49) * 60_000,
      last_seen_ms: Date.now() - 2 * 3600_000,
      version: null,
      host: null
    });

    expect(screen.getByText("offline · last seen 1h 49m ago")).toBeInTheDocument();
  });

  it("falls back to the registry's moment only when there is no frame and no close", () => {
    draw({
      online: false,
      last_frame_ms: null,
      last_left_ms: null,
      last_seen_ms: Date.now() - 2 * 3600_000,
      version: null,
      host: null
    });

    expect(screen.getByText("offline · last seen 2h ago")).toBeInTheDocument();
  });

  it("reads the frame before the close, and the close before the registry", () => {
    draw({
      online: false,
      last_frame_ms: Date.now() - 60_000,
      last_left_ms: Date.now() - 3 * 3600_000,
      last_seen_ms: Date.now() - 9 * 3600_000,
      version: null,
      host: null
    });

    expect(screen.getByText("offline · last seen 1m ago")).toBeInTheDocument();
  });

  it("calls a silent channel offline without inventing a reconnect", () => {
    // The live finding: the socket is still open, the frames stopped seven minutes ago. The row says
    // offline because presence is a frame, and the attempt line is silent because nothing reconnected.
    const { container } = draw({
      online: false,
      last_frame_ms: Date.now() - 7 * 60_000,
      last_attempt: { attempts: 3, reason: "the console accepted the channel and then dropped it" }
    });

    expect(screen.getByText("offline · last seen 7m ago")).toBeInTheDocument();
    expect(container.querySelector(".fleet-attempt")).toBeNull();
    expect(screen.queryByText(/Reconnected after/)).not.toBeInTheDocument();
  });

  it("does not say when a device the console never heard was last seen", () => {
    draw({ online: false, last_frame_ms: null, version: null });

    expect(screen.getByText("offline")).toBeInTheDocument();
    expect(screen.queryByText(/last seen/)).not.toBeInTheDocument();
    expect(screen.queryByText(/never/)).not.toBeInTheDocument();
  });

  it("draws the last start, not an update that predates it", () => {
    // The measured 9VH92BJ row: an applied answer to 0.3.273 at 19:11, and a process that started
    // at 09:41 running 0.3.277. The old answer is history; the start is the fact that places the
    // boot against the console's silence, and the row states it even though the version repeats
    // the chip.
    const { container } = draw({
      online: false,
      last_frame_ms: Date.now() - 7 * 60_000,
      update: {
        current: "0.3.42",
        verdict: { outcome: "ok", detail: "started 0.3.42" },
        last_update: { to: "0.3.41", outcome: "applied", at_ms: Date.now() - 26 * 3_600_000 }
      }
    });

    expect(screen.getByText("offline · last seen 7m ago")).toBeInTheDocument();
    const verdict = container.querySelector(".fleet-verdict");
    expect(verdict).toHaveTextContent("started 0.3.42 · 1d 1h ago");
    expect(verdict).toHaveAttribute("title", exactMoment(BOOT_AT));
    expect(screen.queryByText(/updated/)).not.toBeInTheDocument();
  });

  it("states the update's age beside the start when it belongs to this start", () => {
    const { container } = draw({
      online: true,
      update: {
        current: "0.3.42",
        verdict: { outcome: "ok", detail: "started 0.3.42" },
        last_update: { to: "0.3.42", outcome: "applied", at_ms: Date.now() - 12 * 60_000 }
      }
    });

    const verdict = container.querySelector(".fleet-verdict");
    expect(verdict).toHaveTextContent("started 0.3.42 · 1d 1h ago (updated 12m ago)");
    expect(verdict).toHaveAttribute("title", exactMoment(BOOT_AT));
    // The version appears twice: the chip's v0.3.42 and the device's own start sentence. The
    // update clause does not repeat it.
    expect(container.textContent?.match(/0\.3\.42/g)).toHaveLength(2);
  });

  it("shows a refusal the device is making now, without a start to attach it to", () => {
    draw({
      online: true,
      update: {
        current: "0.3.42",
        verdict: { outcome: "needs_attention", detail: "refused: nothing was installed" }
      }
    });

    expect(screen.getByText("refused: nothing was installed")).toBeInTheDocument();
    expect(screen.queryByText(/started /)).not.toBeInTheDocument();
  });

  it("draws no verdict line for a device that sent no update section", () => {
    const { container } = draw({ online: false, last_frame_ms: Date.now() - 7 * 60_000 });

    // A silent older device shows exactly the silence line and nothing new.
    expect(screen.getByText("offline · last seen 7m ago")).toBeInTheDocument();
    expect(container.querySelector(".fleet-verdict")).toBeNull();
    expect(screen.queryByText(/started /)).not.toBeInTheDocument();
    expect(screen.queryByText(/asleep|sleeping/i)).not.toBeInTheDocument();
  });

  it("draws no channel line: the channel is the Machine page's fact, not a Home column", () => {
    // The measured 84T: `host.update.current` was null, so the old Machine line read "channel not
    // reported" while the restart journal named canary. Home now names what runs, when it started
    // and whether anything needs a person; the channel has no column and is not repeated.
    const { container } = draw({
      host: {
        restarts: [{ at_ms: BOOT_AT, version: "0.3.42", channel: "canary", pid: 42 }],
        update: { current: null, previous: null, pending: null, last_swap: null }
      }
    });

    expect(container.querySelector(".fleet-channel")).toBeNull();
    expect(screen.queryByText(/channel canary/)).toBeNull();
  });

  it("opens the machine from the row's link", () => {
    const onSelect = vi.fn();
    render(
      <table>
        <tbody>
          <DeviceCard device={device} selected={false} {...callbacks} onSelect={onSelect} />
        </tbody>
      </table>
    );

    fireEvent.click(screen.getByRole("button", { name: "Open workshop" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("never marks behind on the row: that sentence belongs to Needs attention", () => {
    // The comparison is still proven in versions.test.ts; Home draws it once, as an attention row
    // with the action beside it. A second cue in the version chip would say it twice.
    const { container } = draw({ version: "0.3.280" });

    expect(screen.getByText("v0.3.280")).toBeInTheDocument();
    expect(container.querySelector(".fleet-build")).toHaveTextContent(/^v0\.3\.280$/);
    expect(screen.queryByText(/behind/)).toBeNull();
    expect(within(screen.getByRole("row")).queryByText(/0\.3\.284/)).toBeNull();
  });

  it("draws no day counts, even when the file's coverage starts after midnight", () => {
    // The old row's `554 client calls · 53 failed · since 03:12` is exactly the shape Home no
    // longer draws; the `since` clause was useful only because the counts were there.
    const midnight = new Date();
    midnight.setHours(0, 0, 0, 0);
    draw({
      today: {
        client_calls: 554,
        failed: 53,
        since_ms: midnight.getTime() + (3 * 60 + 12) * 60_000
      }
    });

    expect(screen.queryByText(/client call/)).toBeNull();
    expect(screen.queryByText(/since 03:12/)).toBeNull();
    expect(screen.queryByText(/554/)).toBeNull();
    expect(screen.queryByText(/53 failed/)).toBeNull();
  });

  it("draws no day counts when the machine has none", () => {
    draw({ today: null });

    expect(screen.queryByText(/client calls/)).toBeNull();
    expect(screen.queryByText(/calls/)).toBeNull();
  });

  it("does not say failed on the row at all", () => {
    draw({ today: { client_calls: 11, failed: 0 } });

    expect(screen.queryByText("11 client calls")).toBeNull();
    expect(screen.queryByText(/failed/)).toBeNull();
  });

  it("keeps the machine's name in its own cell, beside the status cell", () => {
    // The name is the row's link and the status is its own column; the old structural rule (a
    // name that could not swallow the counters at 390) now says the dot and word have their home.
    const { container } = draw();
    const name = container.querySelector(".fleet-name");
    const status = container.querySelector(".fleet-status");

    expect(name).toHaveTextContent(/^workshop$/);
    expect(name?.closest("td")?.className).toBe("fleet-machine");
    expect(name?.closest(".fleet-status")).toBeNull();
    expect(status?.closest("td")?.className).toBe("fleet-status");
    // The fixture is 15 s old, but a loaded full suite can spend the half-second that rounds it to
    // 16 s between building the fixture and rendering it. Both are the same fact; the exact second
    // is not this test's subject.
    expect(status).toHaveTextContent(/online · seen 1[56]s ago/);
  });

  it("draws no channel line when the journal names none and the launcher does", () => {
    const { container } = draw({
      host: {
        restarts: [{ at_ms: BOOT_AT, version: "0.3.42", pid: 42 }],
        update: { current: { version: "0.3.42", channel: "beta" } }
      }
    });

    expect(container.querySelector(".fleet-channel")).toBeNull();
    expect(screen.queryByText(/channel beta/)).toBeNull();
  });

  it("draws no channel line when neither the journal nor the launcher names one", () => {
    const { container } = draw({ host: null });
    expect(container.querySelector(".fleet-channel")).toBeNull();
  });

  it("draws no behind cue when the versions are equal", () => {
    draw({ version: "0.3.42" });
    expect(screen.queryByText(/behind/)).toBeNull();
  });

  it("draws no behind cue when the version is unknown", () => {
    draw({ version: null });
    expect(screen.getByLabelText("No version reported")).toHaveTextContent("—");
    expect(screen.queryByText(/behind/)).toBeNull();
  });

  it("draws no behind cue for a machine newer than the staged installer", () => {
    draw({ version: "0.3.99" });
    expect(screen.queryByText(/behind/)).toBeNull();
  });
});
