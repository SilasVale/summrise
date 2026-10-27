import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Device } from "./api";
import { DevicePage, type DevicePageProps } from "./DevicePage";

// The shell has exactly two states and no third: before a device is chosen it offers the picker, and
// after it hands the child the device id. The grant form that used to stand between them is gone —
// the console session is the permission — so these tests are the whole contract.
const workshop: Device = {
  id: "dev-workshop-0001",
  name: "workshop",
  online: true,
  version: "0.3.42",
  last_seen_ms: 1
};
const attic: Device = {
  id: "dev-attic-0002",
  name: "attic",
  online: false,
  version: null,
  last_seen_ms: null
};
const devices = [workshop, attic];

const pickHint = "The screen belongs to the device's user session, so pick one.";

function shell(overrides: Partial<DevicePageProps> = {}) {
  return (
    <DevicePage
      title="Screen"
      pickHint={pickHint}
      devices={devices}
      chosen={null}
      onChoose={vi.fn()}
      {...overrides}
    >
      {(active) => <div>child for {active.deviceId}</div>}
    </DevicePage>
  );
}

describe("DevicePage", () => {
  it("shows the title, the pick hint and one Use per device before a device is chosen", () => {
    const onChoose = vi.fn();
    render(shell({ onChoose }));

    expect(screen.getByRole("heading", { name: "Screen" })).toBeInTheDocument();
    expect(screen.getByText(pickHint)).toBeInTheDocument();
    expect(screen.queryByText(/^child for /)).not.toBeInTheDocument();

    const use = screen.getAllByRole("button", { name: "Use" });
    expect(use).toHaveLength(2);
    fireEvent.click(use[1]);
    expect(onChoose).toHaveBeenCalledWith(attic.id);
  });

  it("renders the child with the chosen device id and hides the picker", () => {
    render(shell({ chosen: workshop }));

    expect(screen.getByText(`child for ${workshop.id}`)).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Screen" })).not.toBeInTheDocument();
    expect(screen.queryByText(pickHint)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Use" })).not.toBeInTheDocument();
  });

  it("shows the picker again when the selection is cleared", () => {
    const { rerender } = render(shell({ chosen: workshop }));
    rerender(shell({ chosen: null }));

    expect(screen.getByRole("heading", { name: "Screen" })).toBeInTheDocument();
    expect(screen.queryByText(`child for ${workshop.id}`)).not.toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Use" })).toHaveLength(2);
  });
});
