import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Device } from "./api";
import { MachineMenu } from "./MachineMenu";

// The menu is a switcher: dot, name, state word and build. It is not Home's fleet table, so the
// fleet's board facts (disk, the day's calls, the last absence) must not be drawn here. Every test
// below holds one of those facts on the fixture and asserts the switcher stays a switcher.
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
function draw(devices: Device[], chosen: Device | null = null) {
  const view = render(
    <MachineMenu
      devices={devices}
      chosen={chosen}
      onChoose={() => {}}
      onAddMachine={() => {}}
    />
  );
  const details = view.container.querySelector("details.machine-menu");
  if (details) (details as HTMLDetailsElement).open = true;
  return view;
}

describe("MachineMenu — a switcher, not a second fleet table", () => {
  it("draws each machine as a dot, a name, a state word and a build", () => {
    const { container } = draw([
      device(),
      device({ id: "dev-attic", name: "attic", version: "0.3.9" })
    ]);

    const rows = [...container.querySelectorAll(".machine-pop > button")].filter((button) =>
      button.classList.contains("machine-pop-name") === false &&
      button.querySelector(".machine-pop-name")
    );
    expect(rows).toHaveLength(2);

    const workshop = screen.getByRole("button", { name: /^workshop/ });
    expect(workshop.querySelector(".machine-pop-name")).toHaveTextContent("workshop");
    expect(workshop.querySelector(".machine-pop-meta")).toHaveTextContent("online · v0.3.42");
    expect(workshop.querySelector(".dot")).toHaveClass("dot-ok");

    const attic = screen.getByRole("button", { name: /^attic/ });
    expect(attic.querySelector(".machine-pop-meta")).toHaveTextContent("online · v0.3.9");
  });

  it("draws no disk free-space fact, even when the host reported disks", () => {
    // The measured menu read `I: 23.9 GiB free` with no label: a Machine-page fact in a picker.
    draw([
      device({
        host: {
          disks: [
            { name: "C:\\", total_bytes: 1024 * GIB, free_bytes: 312 * GIB },
            { name: "D:\\", total_bytes: 2048 * GIB, free_bytes: 1600 * GIB }
          ]
        }
      })
    ]);

    const row = screen.getByRole("button", { name: /^workshop/ });
    expect(row).not.toHaveTextContent("GiB");
    expect(row).not.toHaveTextContent("free");
    expect(row.querySelector(".fleet-attempt")).toBeNull();
  });

  it("draws no client-call counter, even when the fleet read counted the day", () => {
    // `29 client calls · 1 failed` is Activity's noun; a picker that repeats it makes the operator
    // wonder which surface owns the number.
    draw([device({ today: { client_calls: 283, failed: 38 } })]);

    const row = screen.getByRole("button", { name: /^workshop/ });
    expect(row).not.toHaveTextContent(/client calls?/);
    expect(row).not.toHaveTextContent(/failed/);
  });

  it("draws no absence line, even when the console saw the machine drop", () => {
    draw([
      device({
        online: false,
        last_frame_ms: null,
        last_left_ms: NOW - 4 * 3_600_000,
        last_outage: { away_ms: 41 * 60_000, back_at_ms: NOW - 2 * 3_600_000 }
      })
    ]);

    const row = screen.getByRole("button", { name: /^workshop/ });
    expect(row).not.toHaveTextContent(/back |away since/);
    expect(row.querySelector(".machine-pop-meta")).toHaveTextContent("offline · v0.3.42");
  });

  it("draws a machine with no build without inventing one", () => {
    draw([device({ name: "bare", version: null })]);

    const row = screen.getByRole("button", { name: /^bare/ });
    expect(row.querySelector(".machine-pop-meta")).toHaveTextContent(/^online$/);
    expect(row).not.toHaveTextContent(/v0\./);
  });

  it("states every row with the fleet's reachability word, not the app's", () => {
    // Home's fleet row says `online`; the menu is the same machines in the same shell, so it says
    // `online`/`offline` too. The measured menu said `Running` while the table below it said
    // `online` (audit-43 F2).
    draw([
      device({ name: "here", online: true }),
      device({ id: "dev-away", name: "away", online: false, last_frame_ms: null })
    ]);

    const here = screen.getByRole("button", { name: /^here/ });
    const away = screen.getByRole("button", { name: /^away/ });
    expect(here).toHaveTextContent("online · v0.3.42");
    expect(away).toHaveTextContent("offline · v0.3.42");
    expect(here).not.toHaveTextContent("Running");
    expect(away).not.toHaveTextContent("Offline");
  });

  it("leaves the console's own rows alone", () => {
    draw([
      device({
        host: { disks: [{ name: "C:", total_bytes: 1024 * GIB, free_bytes: 312 * GIB }] },
        today: { client_calls: 11, failed: 0 },
        last_outage: { away_ms: 41 * 60_000, back_at_ms: NOW - 2 * 3_600_000 }
      })
    ]);

    for (const name of ["All machines", "Add a machine"]) {
      const row = screen.getByRole("button", { name });
      expect(row).toHaveTextContent(name);
      expect(row).not.toHaveTextContent("GiB");
      expect(row).not.toHaveTextContent("calls");
      expect(row).not.toHaveTextContent("back");
    }
  });

  it("marks the chosen machine, not the others", () => {
    const chosen = device();
    draw([chosen, device({ id: "dev-attic", name: "attic" })], chosen);

    expect(screen.getByRole("button", { name: /^workshop/ })).toHaveAttribute(
      "aria-current",
      "true"
    );
    expect(screen.getByRole("button", { name: /^attic/ })).not.toHaveAttribute("aria-current");
  });

  it("says All machines in the summary when no machine is chosen", () => {
    draw([device()]);
    expect(screen.getByRole("button", { name: "All machines" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Machine:/ })).toBeNull();
  });

  it("names the chosen machine in the summary without repeating the name in the menu", () => {
    const chosen = device();
    const { container } = draw([chosen], chosen);

    const summary = container.querySelector("details.machine-menu > summary");
    expect(summary).toHaveAttribute("aria-label", "Machine: workshop");
    expect(summary?.querySelector(".machine-menu-version")).toHaveTextContent("· v0.3.42");
  });

  it("closes the menu when a machine is picked", () => {
    const onChoose = vi.fn();
    const { container } = render(
      <MachineMenu
        devices={[device()]}
        chosen={null}
        onChoose={onChoose}
        onAddMachine={() => {}}
      />
    );
    const details = container.querySelector("details.machine-menu") as HTMLDetailsElement;
    details.open = true;

    fireEvent.click(screen.getByRole("button", { name: /^workshop/ }));

    expect(onChoose).toHaveBeenCalledWith("dev-workshop");
    expect(details.open).toBe(false);
  });
});

/**
 * THE TOPBAR STATES THE BUILD AT EVERY WIDTH. The phone used to hide the whole meta span, so an
 * offline Device page at 390 named no version anywhere; the version keeps its own element now, and
 * the phone's stylesheet hides only the status word (the dot already carries it).
 */
describe("MachineMenu — the chosen machine's version", () => {
  it("keeps the version's v and its own element beside the status", () => {
    const { container } = render(
      <MachineMenu
        devices={[device()]}
        chosen={device()}
        onChoose={() => {}}
        onAddMachine={() => {}}
      />
    );

    const meta = container.querySelector(".machine-menu-meta");
    expect(meta).not.toBeNull();
    expect(meta?.querySelector(".machine-menu-state")).toHaveTextContent("online");
    expect(meta?.querySelector(".machine-menu-version")).toHaveTextContent("· v0.3.42");
    // The version is a fact, not a colour: it stays a `v`-prefixed word, as Home's column does.
    expect(meta).toHaveTextContent("online · v0.3.42");
  });

  it("names an offline machine's build too", () => {
    const offline = device({ online: false, version: "0.3.275" });
    const { container } = render(
      <MachineMenu devices={[offline]} chosen={offline} onChoose={() => {}} onAddMachine={() => {}} />
    );

    expect(container.querySelector(".machine-menu-state")).toHaveTextContent("offline");
    expect(container.querySelector(".machine-menu-version")).toHaveTextContent("v0.3.275");
  });
});
