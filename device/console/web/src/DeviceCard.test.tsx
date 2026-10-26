import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DeviceCard, type Device } from "./DeviceCard";
import type { VitalsSample } from "./vitals";

const GIB = 1024 ** 3;
const NOW = 1_700_000_000_000;

const device: Device = {
  id: "dev-1",
  name: "workshop",
  online: true,
  version: "0.3.42",
  last_seen_ms: NOW - 15_000,
  host: {
    memory: { total_bytes: 8 * GIB, available_bytes: 2 * GIB },
    disks: [{ name: "/", total_bytes: 100 * GIB, free_bytes: 10 * GIB }]
  }
};

function sample(ts_ms: number): VitalsSample {
  return {
    ts_ms,
    memory_available_bytes: 4 * GIB,
    memory_total_bytes: 8 * GIB,
    disk_name: "/data",
    disk_free_bytes: 20 * GIB,
    disk_total_bytes: 100 * GIB
  };
}

const callbacks = {
  onSelect: vi.fn(),
  onRename: vi.fn(),
  onRemove: vi.fn(),
  onDiagnostics: vi.fn(),
  onConnect: vi.fn()
};

function draw(vitals?: VitalsSample[]) {
  return render(
    <DeviceCard device={device} newest={null} selected={false} vitals={vitals} {...callbacks} />
  );
}

describe("DeviceCard vitals", () => {
  it("keeps the live host facts and adds the history chart when samples exist", () => {
    const { container } = draw([sample(NOW - 30_000), sample(NOW)]);

    // The live rows are still the device's own facts.
    expect(screen.getByText("2.0 GiB free of 8.0 GiB")).toBeInTheDocument();
    expect(screen.getByText("10.0 GiB of 100.0 GiB")).toBeInTheDocument();

    expect(screen.getByText("Memory available 50.0%")).toBeInTheDocument();
    expect(screen.getByText("Tightest disk /data 20.0% free")).toBeInTheDocument();
    expect(
      screen.getByText(/over the last 2 h, sampled every 30 s while this console page was open/)
    ).toBeInTheDocument();
    expect(container.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
  });

  it("shows the collecting sentence when the device has no history yet", () => {
    const { container } = draw();

    expect(
      screen.getByText(/Collecting memory and disk history while this page is open/)
    ).toBeInTheDocument();
    expect(container.querySelector("svg")).toBeNull();
  });
});
