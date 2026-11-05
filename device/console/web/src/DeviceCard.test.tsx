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

  it("says the attempt count and the reason the device gave, in its own words", () => {
    const { container } = draw({
      last_attempt: { attempts: 2, reason: "HTTP error: 502 Bad Gateway" }
    });

    // One line, the count first and the device's sentence after it, not paraphrased.
    const line = container.querySelector(".fleet-attempt");
    expect(line).not.toBeNull();
    expect(line).toHaveTextContent("Reconnected after 2 attempts — HTTP error: 502 Bad Gateway");
    // Cut to one line at a narrow width; the title is the whole sentence, reason included.
    expect(line).toHaveAttribute(
      "title",
      "Reconnected after 2 attempts — HTTP error: 502 Bad Gateway"
    );
    expect(screen.getByTitle("HTTP error: 502 Bad Gateway")).toBeInTheDocument();
  });

  it("keeps the count alone when the fleet carried no reason", () => {
    draw({ last_attempt: { attempts: 2, reason: "" } });

    const line = screen.getByText("Reconnected after 2 attempts");
    expect(line).not.toHaveAttribute("title");
    expect(line.querySelector(".attempt-reason")).toBeNull();
    expect(line).not.toHaveTextContent("—");
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

  it("opens the machine from the whole main row", () => {
    const onSelect = vi.fn();
    render(
      <DeviceCard device={device} selected={false} {...callbacks} onSelect={onSelect} />
    );

    fireEvent.click(screen.getByRole("button", { name: "Open workshop" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
  });
});
