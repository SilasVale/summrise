import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
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
  last_frame_ms: Date.now() - 7 * 60_000,
  host: {
    // The newest boot is when the running process started. It is the only moment that can prove
    // an updater's answer belongs to this process.
    restarts: [{ at_ms: Date.now() - 25 * 3_600_000, version: "0.3.42", pid: 42 }]
  }
};

function renderShell(tab: DeviceTab, device: Device = MACHINE, onConnect = () => undefined) {
  return render(
    <DevicePageShell
      tab={tab}
      onTab={() => undefined}
      devices={[device]}
      chosen={device}
      onChoose={() => undefined}
      onConnect={onConnect}
    >
      {() => <p>page contents</p>}
    </DevicePageShell>
  );
}

describe("DevicePageShell's Connect door", () => {
  it("names the Device's Connect door for the sheet it opens and calls it", () => {
    const onConnect = vi.fn();
    renderShell("machine", MACHINE, onConnect);

    fireEvent.click(screen.getByRole("button", { name: "Connect a client" }));

    expect(onConnect).toHaveBeenCalledTimes(1);
  });
});

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

  it("does not repeat the running version the header already shows", () => {
    renderShell("files", {
      ...MACHINE,
      update: {
        current: "0.3.42",
        verdict: { outcome: "ok", detail: "started 0.3.42" },
        last_update: { to: "0.3.41", outcome: "applied", at_ms: Date.now() - 26 * 3_600_000 }
      }
    });

    // The header chip is the surface's one version; an updater's answer from before this start is
    // not this start's story.
    expect(screen.getByRole("status")).toHaveTextContent("This machine is offline.");
    expect(screen.queryByText(/0\.3\.42/)).not.toBeInTheDocument();
    expect(screen.queryByText(/updated/)).not.toBeInTheDocument();
  });

  it("ages the update beside the silence when the answer belongs to this start", () => {
    renderShell("files", {
      ...MACHINE,
      update: {
        current: "0.3.42",
        verdict: { outcome: "ok", detail: "started 0.3.42" },
        last_update: { to: "0.3.42", outcome: "applied", at_ms: Date.now() - 12 * 60_000 }
      }
    });

    // The console knows silence, not sleep: it must never claim the machine slept. The new fact
    // beside it is the age of the answer this process folded in.
    expect(screen.getByText("updated 12m ago")).toBeInTheDocument();
    expect(screen.queryByText(/asleep|sleeping/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/started /)).not.toBeInTheDocument();
    expect(screen.queryByText(/0\.3\.42/)).not.toBeInTheDocument();
  });

  it("keeps a start that names a version the header does not show", () => {
    renderShell("files", {
      ...MACHINE,
      update: {
        current: "0.3.275",
        verdict: { outcome: "ok", detail: "started 0.3.275" }
      }
    });

    // The header says 0.3.42; the device says it started 0.3.275. That is a contradiction the
    // operator needs, not a duplicate of the chip.
    expect(screen.getByText("started 0.3.275")).toBeInTheDocument();
  });

  it("shows a refusal the device is making now, on an online machine too", () => {
    renderShell("files", {
      ...MACHINE,
      online: true,
      update: {
        current: "0.3.42",
        verdict: { outcome: "needs_attention", detail: "refused: nothing was installed" }
      }
    });

    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByText("refused: nothing was installed")).toBeInTheDocument();
  });
});
