import { render, screen } from "@testing-library/react";
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
  onDiagnostics: vi.fn(),
  onConnect: vi.fn()
};

function draw(overrides: Partial<Device> = {}) {
  return render(
    <DeviceCard device={{ ...device, ...overrides }} newest={null} selected={false} {...callbacks} />
  );
}

describe("DeviceCard — the picker's card, not the machine page", () => {
  it("keeps only the decision facts on the card", () => {
    const { container } = draw();

    expect(screen.getByText("workshop")).toBeInTheDocument();
    expect(screen.getByText("online")).toBeInTheDocument();
    expect(screen.getByText(/seen 15s ago/)).toBeInTheDocument();
    expect(screen.getByText(/v0\.3\.42/)).toBeInTheDocument();
    expect(screen.getByText(/Windows 11 · up 1d 1h/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Work on workshop" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Connect AI client to workshop" })).toBeInTheDocument();

    // The machine's own facts are NOT on the picker any more. They have one home: Machine.
    expect(screen.queryByText("2.0 GiB free of 8.0 GiB")).not.toBeInTheDocument();
    expect(container.querySelector(".vitals-chart")).toBeNull();
    expect(container.querySelector("svg")).toBeNull();
  });

  it("keeps the management actions behind Details", () => {
    const { container } = draw();

    const details = container.querySelector("details.card-details");
    expect(details).not.toBeNull();
    expect(details).not.toHaveAttribute("open");
    expect(details).toContainElement(screen.getByRole("button", { name: "Copy diagnostics for workshop" }));
    expect(details).toContainElement(screen.getByRole("button", { name: "Rename workshop" }));
    expect(details).toContainElement(screen.getByRole("button", { name: "Remove workshop" }));
    // The raw id is not furniture on the picker.
    expect(screen.queryByText("dev-1")).not.toBeInTheDocument();
  });

  it("shows a warning chip only when the build is behind the fleet", () => {
    const { rerender } = draw();
    // Current build: the version is a quiet fact on the status line, not an always-on badge.
    expect(document.querySelector(".card header .chip")).toBeNull();

    rerender(
      <DeviceCard device={device} newest="0.3.99" selected={false} {...callbacks} />
    );
    const chip = document.querySelector(".card header .chip");
    expect(chip).not.toBeNull();
    expect(chip).toHaveTextContent("v0.3.42");
  });

  it("says offline in words, with when it was last seen", () => {
    draw({ online: false, last_seen_ms: Date.now() - 3 * 3600_000, version: null, host: null });

    expect(screen.getByText("offline")).toBeInTheDocument();
    expect(screen.getByText(/seen 3h ago/)).toBeInTheDocument();
  });
});
