import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DeviceCard, type Device } from "./DeviceCard";

const GIB = 1024 ** 3;

const device: Device = {
  id: "dev-1",
  name: "workshop",
  online: true,
  version: "0.3.42",
  last_seen_ms: Date.now() - 15_000,
  host: {
    os: { name: "Windows", version: "11" },
    uptime_s: 90_061,
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

  it("reports a reconnection as attempts, never the transport's failure sentence", () => {
    draw({ last_attempt: { attempts: 2, reason: "connection reset by peer" } });

    expect(screen.getByText("Reconnected after 2 attempts")).toBeInTheDocument();
    expect(screen.queryByText(/connection reset by peer/)).not.toBeInTheDocument();
  });

  it("says offline in words, with when it was last seen", () => {
    draw({ online: false, last_seen_ms: Date.now() - 3 * 3600_000, version: null, host: null });

    expect(screen.getByText(/offline · last seen 3h ago/)).toBeInTheDocument();
  });

  it("opens the machine from the whole main row", () => {
    const onSelect = vi.fn();
    render(
      <DeviceCard device={device} selected={false} {...callbacks} onSelect={onSelect} />
    );

    fireEvent.click(screen.getByRole("button", { name: "Open workshop" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
  });
});
