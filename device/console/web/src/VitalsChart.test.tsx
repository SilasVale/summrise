import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { VitalsChart } from "./VitalsChart";
import type { VitalsSample } from "./vitals";

const GIB = 1024 ** 3;
const NOW = 1_700_000_000_000;

const CAPTION =
  "Memory available and the tightest disk's free space over the last 2 h, sampled every 30 s while this console page was open in this browser. Gaps are not filled.";

function sample(ts_ms: number, overrides: Partial<VitalsSample> = {}): VitalsSample {
  return {
    ts_ms,
    memory_available_bytes: 4 * GIB,
    memory_total_bytes: 8 * GIB,
    disk_name: "/data",
    disk_free_bytes: 20 * GIB,
    disk_total_bytes: 100 * GIB,
    ...overrides
  };
}

describe("VitalsChart", () => {
  it("shows the collecting sentence until the second sample", () => {
    const { container, rerender } = render(<VitalsChart samples={[]} />);
    expect(
      screen.getByText(/Collecting memory and disk history while this page is open/)
    ).toBeInTheDocument();
    expect(container.querySelector("svg")).toBeNull();

    rerender(<VitalsChart samples={[sample(NOW)]} />);
    expect(
      screen.getByText(/Collecting memory and disk history while this page is open/)
    ).toBeInTheDocument();
    expect(container.querySelector("svg")).toBeNull();
  });

  it("draws two aria-hidden polylines and names the latest values in text", () => {
    const samples = [
      sample(NOW - 60_000, { memory_available_bytes: 2 * GIB, disk_free_bytes: 40 * GIB }),
      sample(NOW)
    ];
    const { container } = render(<VitalsChart samples={samples} />);

    const svg = container.querySelector("svg");
    expect(svg).not.toBeNull();
    expect(svg).toHaveAttribute("aria-hidden", "true");
    expect(svg?.querySelectorAll("polyline")).toHaveLength(2);

    expect(screen.getByText("Memory available 50.0%")).toBeInTheDocument();
    expect(screen.getByText("Tightest disk /data 20.0% free")).toBeInTheDocument();
  });

  it("labels the latest sample's disk when the tightest disk changes", () => {
    const samples = [
      sample(NOW - 60_000, { disk_name: "/data", disk_free_bytes: 40 * GIB }),
      sample(NOW, { disk_name: "/home", disk_free_bytes: 90 * GIB })
    ];
    render(<VitalsChart samples={samples} />);

    expect(screen.getByText("Tightest disk /home 90.0% free")).toBeInTheDocument();
    expect(screen.queryByText(/\/data/)).not.toBeInTheDocument();
  });

  it("states its coverage: two hours, thirty seconds, and only while the page is open", () => {
    render(<VitalsChart samples={[sample(NOW - 30_000), sample(NOW)]} />);

    expect(screen.getByText(CAPTION)).toBeInTheDocument();
    expect(screen.queryByText(/Collecting memory and disk history/)).not.toBeInTheDocument();
  });
});
