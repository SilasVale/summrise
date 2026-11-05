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

  it("ages the notice from the close when the console has no frame to read", () => {
    // The console restarted mid-outage: no frame, but the audit's newest `device.left` is the moment
    // the silence started. The registry's older connect time must not be the age the notice shows.
    renderShell("files", {
      ...MACHINE,
      last_frame_ms: null,
      last_left_ms: Date.now() - (1 * 60 + 49) * 60_000
    });

    expect(screen.getByRole("status")).toHaveTextContent(/last seen 1h 49m ago/i);
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

  it("shows the device's own start verdict beside the silence, not as an explanation of it", () => {
    renderShell("files", {
      ...MACHINE,
      update: {
        current: "0.3.275",
        verdict: { outcome: "ok", detail: "started 0.3.275" },
        last_update: { to: "0.3.276", outcome: "refused", at_ms: 1 }
      }
    });

    expect(screen.getByRole("status")).toHaveTextContent("This machine is offline.");
    expect(screen.getByText("started 0.3.275 (update to 0.3.276 refused)")).toBeInTheDocument();
    // The console knows silence, not sleep: it must never claim the machine slept.
    expect(screen.queryByText(/asleep|sleeping/i)).not.toBeInTheDocument();
  });

  it("shows the start verdict on an online machine too, because a machine that woke proves its version", () => {
    renderShell("files", {
      ...MACHINE,
      online: true,
      update: {
        current: "0.3.276",
        verdict: { outcome: "ok", detail: "started 0.3.276" },
        last_update: { to: "0.3.276", outcome: "applied", at_ms: 2 }
      }
    });

    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByText("started 0.3.276 (updated)")).toBeInTheDocument();
  });
});
