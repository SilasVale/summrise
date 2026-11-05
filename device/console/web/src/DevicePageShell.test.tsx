import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Device } from "./api";
import { DevicePageShell } from "./DevicePageShell";
import type { DeviceTab } from "./Rail";

const MACHINE: Device = {
  id: "d1",
  name: "workshop",
  online: false,
  version: "0.3.42",
  // The channel opened three hours ago; the last FRAME was seven minutes ago. The notice must say
  // the frame's moment, not the socket's.
  last_seen_ms: Date.now() - 3 * 3600_000,
  last_frame_ms: Date.now() - 7 * 60_000
};

function renderShell(tab: DeviceTab, device: Device = MACHINE) {
  return render(
    <DevicePageShell
      tab={tab}
      onTab={() => undefined}
      devices={[device]}
      chosen={device}
      onChoose={() => undefined}
      onConnect={() => undefined}
    >
      {() => <p>page contents</p>}
    </DevicePageShell>
  );
}

describe("DevicePageShell offline notice", () => {
  it("says the machine is down on the tabs that ask it for something", () => {
    renderShell("files");
    expect(screen.getByRole("status")).toHaveTextContent("This machine is offline.");
    expect(screen.getByRole("status")).toHaveTextContent(/nothing on this page can reach it/i);
  });

  it("says when the machine was last heard, from its last frame", () => {
    renderShell("files");
    expect(screen.getByRole("status")).toHaveTextContent(/last seen 7m ago/i);
  });

  it("carries the one notice on Tools too, above both of its sections", () => {
    renderShell("tools");
    expect(screen.getAllByRole("status")).toHaveLength(1);
    expect(screen.getByRole("status")).toHaveTextContent("This machine is offline.");
  });

  it("leaves the Terminal tab alone, because that page carries its own notice", () => {
    // The terminal workspace is mounted outside this shell and stays mounted across destinations;
    // a second copy here would stack the same sentence twice on one screen.
    renderShell("terminal");
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("says nothing on a machine that is online", () => {
    renderShell("files", { ...MACHINE, online: true });
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });
});
