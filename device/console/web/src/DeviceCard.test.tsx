import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DeviceCard, type Device } from "./DeviceCard";

const GIB = 1024 ** 3;

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
    restarts: [{ at_ms: Date.now() - 25 * 3_600_000, version: "0.3.42", channel: "stable", pid: 42 }],
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

function draw(overrides: Partial<Device> = {}) {
  return render(
    <DeviceCard device={{ ...device, ...overrides }} selected={false} {...callbacks} />
  );
}

describe("DeviceCard — one row on the fleet", () => {
  it("keeps only the decision facts on the row", () => {
    const { container } = draw();

    expect(screen.getByText("workshop")).toBeInTheDocument();
    expect(screen.getByText("online", { exact: false })).toBeInTheDocument();
    expect(screen.getByText(/seen 15s ago/)).toBeInTheDocument();
    expect(screen.getByText("v0.3.42")).toBeInTheDocument();
    // The version appears once on the row: the chip is the surface's one copy, and nothing in the
    // device's story repeats it.
    expect(container.textContent?.match(/0\.3\.42/g)).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Open workshop" })).toBeInTheDocument();

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

  it("names the hour its day starts at when the file rotated", () => {
    // The day read cannot prove the hours before the rotation; the row says since when it can
    // instead of letting a partial day read as the whole one.
    const midnight = new Date();
    midnight.setHours(0, 0, 0, 0);
    const since = midnight.getTime() + (3 * 60 + 12) * 60_000;
    draw({ today: { client_calls: 554, failed: 53, since_ms: since } });

    expect(screen.getByText("554 calls · 53 failed · since 03:12")).toBeInTheDocument();
  });

  it("does not draw a reconnect sentence: the attempt has no moment of its own", () => {
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
    expect(screen.getByText(/online · seen 15s ago/)).toBeInTheDocument();
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

    expect(screen.getByText(/offline · last seen 3h ago/)).toBeInTheDocument();
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

    expect(screen.getByText(/offline · last seen 1m ago/)).toBeInTheDocument();
  });

  it("calls a silent channel offline without inventing a reconnect", () => {
    // The live finding: the socket is still open, the frames stopped seven minutes ago. The row says
    // offline because presence is a frame, and the attempt line is silent because nothing reconnected.
    const { container } = draw({
      online: false,
      last_frame_ms: Date.now() - 7 * 60_000,
      last_attempt: { attempts: 3, reason: "the console accepted the channel and then dropped it" }
    });

    expect(screen.getByText(/offline · last seen 7m ago/)).toBeInTheDocument();
    expect(container.querySelector(".fleet-attempt")).toBeNull();
    expect(screen.queryByText(/Reconnected after/)).not.toBeInTheDocument();
  });

  it("does not say when a device the console never heard was last seen", () => {
    draw({ online: false, last_frame_ms: null, version: null });

    expect(screen.getByText("offline")).toBeInTheDocument();
    expect(screen.queryByText(/last seen/)).not.toBeInTheDocument();
    expect(screen.queryByText(/never/)).not.toBeInTheDocument();
  });

  it("draws no update line when the updater's answer predates this start", () => {
    // The measured 9VH92BJ row: an applied answer to 0.3.273 at 19:11, and a process that started
    // at 09:41 running 0.3.277. "(updated)" was attached to a start it predates.
    const { container } = draw({
      online: false,
      last_frame_ms: Date.now() - 7 * 60_000,
      update: {
        current: "0.3.42",
        verdict: { outcome: "ok", detail: "started 0.3.42" },
        last_update: { to: "0.3.41", outcome: "applied", at_ms: Date.now() - 26 * 3_600_000 }
      }
    });

    // The silence is the console's account; the device's story has nothing new to add, and the
    // running version is already the row's chip.
    expect(screen.getByText(/offline · last seen 7m ago/)).toBeInTheDocument();
    expect(container.querySelector(".fleet-verdict")).toBeNull();
    expect(screen.queryByText(/started /)).not.toBeInTheDocument();
    expect(screen.queryByText(/updated/)).not.toBeInTheDocument();
  });

  it("states the update's age, not the running version, when it belongs to this start", () => {
    const { container } = draw({
      online: true,
      update: {
        current: "0.3.42",
        verdict: { outcome: "ok", detail: "started 0.3.42" },
        last_update: { to: "0.3.42", outcome: "applied", at_ms: Date.now() - 12 * 60_000 }
      }
    });

    expect(screen.getByText("updated 12m ago")).toBeInTheDocument();
    expect(container.querySelector(".fleet-verdict")).not.toBeNull();
    // The chip still carries the version; the line states only the new fact.
    expect(container.textContent?.match(/0\.3\.42/g)).toHaveLength(1);
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
    expect(screen.getByText(/offline · last seen 7m ago/)).toBeInTheDocument();
    expect(container.querySelector(".fleet-verdict")).toBeNull();
    expect(screen.queryByText(/started /)).not.toBeInTheDocument();
    expect(screen.queryByText(/asleep|sleeping/i)).not.toBeInTheDocument();
  });

  it("opens the machine from the whole main row", () => {
    const onSelect = vi.fn();
    render(
      <DeviceCard device={device} selected={false} {...callbacks} onSelect={onSelect} />
    );

    fireEvent.click(screen.getByRole("button", { name: "Open workshop" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("states the day's calls and failures on the row", () => {
    const { container } = draw({ today: { client_calls: 283, failed: 38 } });

    expect(screen.getByText("283 calls · 38 failed")).toBeInTheDocument();
    expect(container.querySelector(".fleet-work")).toHaveTextContent("283 calls · 38 failed");
    // The counts share the row's one muted status cell, inside the one open button: no new
    // column, no second line.
    expect(screen.getByRole("button", { name: "Open workshop" })).toHaveTextContent(
      "283 calls · 38 failed"
    );
    expect(container.querySelectorAll(".fleet-work")).toHaveLength(1);
  });

  it("omits the failed half at zero", () => {
    draw({ today: { client_calls: 11, failed: 0 } });

    expect(screen.getByText("11 calls")).toBeInTheDocument();
    expect(screen.queryByText(/· 0 failed/)).not.toBeInTheDocument();
    expect(screen.queryByText(/failed/)).not.toBeInTheDocument();
  });

  it("draws nothing for a machine the audit window holds no line for", () => {
    const { container } = draw();

    expect(container.querySelector(".fleet-work")).toBeNull();
    expect(screen.queryByText(/calls/)).not.toBeInTheDocument();
  });

  it("keeps the name its own node so the counts can wrap below it at 390", () => {
    // The <=639 stylesheet gives the status cell a full-width row and the counts their own line.
    // jsdom has no layout, so the test pins the structure that rule acts on: the name is its own
    // node and the counts are its sibling, never a suffix inside the name.
    const { container } = draw({ today: { client_calls: 84, failed: 0 } });
    const name = container.querySelector(".fleet-name");
    const work = container.querySelector(".fleet-work");

    expect(name).toHaveTextContent(/^workshop$/);
    expect(work).not.toBeNull();
    expect(name).not.toContainElement(work as HTMLElement);
    expect(work?.closest(".fleet-name")).toBeNull();
    expect(work?.parentElement?.className).toBe("fleet-now");
    expect(name?.parentElement).toBe(work?.parentElement?.parentElement);
  });
});
