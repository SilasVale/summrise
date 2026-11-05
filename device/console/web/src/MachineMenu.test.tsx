import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Device } from "./api";
import { MachineMenu } from "./MachineMenu";

// The menu's line is two facts from the fleet read: the tightest disk the host reported, and the
// last absence the console observed. Both are optional, and an unknown fact is left out rather than
// drawn as a zero.
const NOW = Date.UTC(2026, 10, 5, 12, 0, 0);
const GIB = 1024 ** 3;

function device(overrides: Partial<Device> = {}): Device {
  return {
    id: "dev-workshop",
    name: "workshop",
    online: true,
    version: "0.3.42",
    last_seen_ms: NOW - 12_000,
    ...overrides
  };
}

/** Render the menu with its popover open, the way an operator opens it. */
function draw(devices: Device[]) {
  const view = render(
    <MachineMenu devices={devices} chosen={null} onChoose={() => {}} onAddMachine={() => {}} />
  );
  const details = view.container.querySelector("details.machine-menu");
  if (details) (details as HTMLDetailsElement).open = true;
  return view;
}

describe("MachineMenu", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("compares disk and last absence in one muted line under the name", () => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
    const { container } = draw([
      device({
        host: {
          disks: [
            { name: "C:\\", total_bytes: 1024 * GIB, free_bytes: 312 * GIB },
            { name: "D:\\", total_bytes: 2048 * GIB, free_bytes: 1600 * GIB }
          ]
        },
        last_outage: { away_ms: 41 * 60_000, back_at_ms: NOW - 2 * 3_600_000 }
      })
    ]);

    expect(screen.getByText("C: 312 GiB free · back 2h ago after 41m")).toBeInTheDocument();
    // The row is the button, so the line is part of its accessible name rather than a hover secret.
    expect(screen.getByRole("button", { name: /^workshop/ })).toHaveTextContent(
      "C: 312 GiB free · back 2h ago after 41m"
    );
    expect(container.querySelector(".machine-pop")).not.toBeNull();
  });

  it("draws the disk alone when the machine has not dropped", () => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
    draw([
      device({
        name: "attic",
        host: { disks: [{ name: "/", total_bytes: 512 * GIB, free_bytes: 241 * GIB }] }
      })
    ]);

    expect(screen.getByText("/ 241 GiB free")).toBeInTheDocument();
    expect(screen.queryByText(/back .* ago after/)).toBeNull();
  });

  it("draws nothing when neither the disk nor an absence is known", () => {
    draw([device({ name: "bare", host: null })]);

    const row = screen.getByRole("button", { name: /^bare/ });
    expect(row).not.toHaveTextContent("GiB");
    expect(row).not.toHaveTextContent("back");
  });

  it("leaves the console's own rows alone", () => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
    draw([
      device({
        host: { disks: [{ name: "C:", total_bytes: 1024 * GIB, free_bytes: 312 * GIB }] },
        last_outage: { away_ms: 41 * 60_000, back_at_ms: NOW - 2 * 3_600_000 }
      })
    ]);

    for (const name of ["All machines", "Add a machine"]) {
      const row = screen.getByRole("button", { name });
      expect(row).toHaveTextContent(name);
      expect(row).not.toHaveTextContent("GiB");
      expect(row).not.toHaveTextContent("back");
    }
  });
});
