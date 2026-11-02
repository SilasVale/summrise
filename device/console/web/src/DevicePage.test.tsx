import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Device } from "./api";
import { DevicePage, type DevicePageProps } from "./DevicePage";
import type { Grant } from "./terminal";

// Only the grant form is stubbed: the shell's subject is which panel is shown and which grant reaches
// the child, and the real form's tool catalogue is not part of either. The stub's button is the
// "Grant access" the operator sees, and the id it prints is the device the shell handed it.
vi.mock("./GrantForm", () => ({
  GrantForm: ({ deviceId, onGranted }: { deviceId: string; onGranted: (envelope: unknown) => void }) => (
    <>
      <span>grant form for {deviceId}</span>
      <button type="button" onClick={() => onGranted({ token: "envelope" })}>
        Grant access
      </button>
    </>
  )
}));

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

const grant: Grant = { deviceId: workshop.id, envelope: { token: "envelope" } };
const otherGrant: Grant = { deviceId: attic.id, envelope: { token: "other" } };

const pickHint = "The screen belongs to the device's user session, so pick one.";
const grantHint = "A grant is signed by the console, names the tools and expires.";

function shell(overrides: Partial<DevicePageProps> = {}) {
  return (
    <DevicePage
      title="Screen"
      pickHint={pickHint}
      grantHint={grantHint}
      devices={devices}
      chosen={null}
      grant={null}
      onChoose={vi.fn()}
      onGrant={vi.fn()}
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
    expect(screen.queryByRole("button", { name: "Grant access" })).not.toBeInTheDocument();
    expect(screen.queryByText(/^child for /)).not.toBeInTheDocument();

    const use = screen.getAllByRole("button", { name: "Use" });
    expect(use).toHaveLength(2);
    fireEvent.click(use[1]);
    expect(onChoose).toHaveBeenCalledWith(attic.id);
  });

  it("asks for a grant on the chosen device and reports the envelope against that device", () => {
    const onGrant = vi.fn();
    render(shell({ chosen: workshop, onGrant }));

    expect(
      screen.getByRole("heading", { name: "Grant what the console may do on workshop" })
    ).toBeInTheDocument();
    expect(screen.getByText(grantHint)).toBeInTheDocument();
    expect(screen.getByText(`grant form for ${workshop.id}`)).toBeInTheDocument();
    expect(screen.queryByText(/^child for /)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Use" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Grant access" }));
    expect(onGrant).toHaveBeenCalledWith(workshop.id, { token: "envelope" });
  });

  it("uses the first twelve characters of the id when the device has no name", () => {
    const unnamed: Device = { ...workshop, id: "0123456789abcdef", name: null };
    render(shell({ chosen: unnamed }));

    expect(
      screen.getByRole("heading", { name: "Grant what the console may do on 0123456789ab…" })
    ).toBeInTheDocument();
  });

  it("omits the grant hint paragraph when the page has no hint to give", () => {
    const { container } = render(shell({ chosen: workshop, grantHint: undefined }));

    expect(
      screen.getByRole("heading", { name: "Grant what the console may do on workshop" })
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Grant access" })).toBeInTheDocument();
    expect(container.querySelectorAll("p")).toHaveLength(0);
  });

  it("renders the child with the matching grant and no grant heading", () => {
    render(shell({ chosen: workshop, grant }));

    expect(screen.getByText(`child for ${workshop.id}`)).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /Grant what the console may do/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Grant access" })).not.toBeInTheDocument();
  });

  it("shows the grant panel instead of the child when the grant belongs to another device", () => {
    render(shell({ chosen: workshop, grant: otherGrant }));

    expect(
      screen.getByRole("heading", { name: "Grant what the console may do on workshop" })
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Grant access" })).toBeInTheDocument();
    expect(screen.queryByText(/^child for /)).not.toBeInTheDocument();
  });
});
