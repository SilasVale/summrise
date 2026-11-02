import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Device } from "./api";
import { DevicePage, type DevicePageProps } from "./DevicePage";

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
    <DevicePage title="Screen" pickHint={pickHint} devices={devices} chosen={null} onChoose={vi.fn()} {...overrides}>
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

    const use = screen.getAllByRole("button", { name: /^Use / });
    expect(use).toHaveLength(2);
    fireEvent.click(use[1]);
    expect(onChoose).toHaveBeenCalledWith(attic.id);
  });

  it("renders the page with a session grant and never asks for a second consent", () => {
    render(shell({ chosen: workshop }));

    expect(screen.getByText(`child for ${workshop.id}`)).toBeInTheDocument();
    // The deleted grant form was the one thing this shell could show between picking and the page.
    // Its absence is the contract: a workspace page must open, not stop on a permission screen.
    expect(screen.queryByRole("button", { name: /Grant|Sign/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /Grant what the console may do/ })).not.toBeInTheDocument();
  });
});
