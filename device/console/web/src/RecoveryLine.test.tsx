import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Device } from "./api";
import { exactMoment } from "./format";
import { RecoveryLine } from "./RecoveryLine";

// The recovery line is the start line's companion: one muted sentence saying whether this machine
// was away, for how long, and which record proves it. It must be absent when no record does.

const MINUTE = 60_000;

function device(overrides: Partial<Device> = {}): Device {
  return {
    id: "device-1",
    name: "workshop",
    online: true,
    version: "0.3.42",
    last_seen_ms: 1,
    ...overrides
  };
}

describe("RecoveryLine", () => {
  it("states the console's proven gap and keeps the exact return in the title", () => {
    const backAt = new Date(2026, 10, 8, 10, 20, 0).getTime();
    render(
      <RecoveryLine
        device={device({ last_outage: { away_ms: 41 * MINUTE, back_at_ms: backAt } })}
        consoleStartedAt={backAt + 3_600_000}
      />
    );

    const line = screen.getByText("back after 41 m");
    expect(line).toHaveClass("recovery-line");
    expect(line).toHaveAttribute("title", exactMoment(backAt));
    expect(line).toHaveAttribute("data-recovery-source", "console");
  });

  it("attributes a journal gap to the device, and says it is not a frame the console saw", () => {
    const backBoot = new Date(2026, 10, 8, 8, 51, 43).getTime();
    const previousBoot = backBoot - (14 * 3_600_000 + 57 * 60_000 + 59_000);
    render(
      <RecoveryLine
        device={device({
          last_frame_ms: backBoot + 1_000,
          host: {
            uptime_s: 5,
            restarts: [
              { at_ms: backBoot + 44 * MINUTE, version: "0.3.292" },
              { at_ms: backBoot, version: "0.3.289" },
              { at_ms: previousBoot, version: "0.3.289" }
            ]
          }
        })}
        consoleStartedAt={backBoot + 3_600_000}
      />
    );

    const line = screen.getByText(/^back after 14 h 57 m \(the device's journal recorded its boot at /);
    expect(line).toHaveAttribute("data-recovery-source", "journal");
    expect(line).toHaveAttribute("title", exactMoment(backBoot));
  });

  it("says the console cannot vouch when it restarted inside the gap", () => {
    const backAt = new Date(2026, 10, 8, 10, 20, 0).getTime();
    render(
      <RecoveryLine
        device={device({ last_outage: { away_ms: 41 * MINUTE, back_at_ms: backAt } })}
        consoleStartedAt={backAt - 20 * MINUTE}
      />
    );

    expect(screen.getByText(/^The console restarted while this machine was away/)).toHaveTextContent(
      "cannot vouch for how long it was gone"
    );
  });

  it("draws no line when neither record proves a gap", () => {
    render(<RecoveryLine device={device()} />);
    expect(document.querySelector(".recovery-line")).toBeNull();

    render(<RecoveryLine device={device({ last_outage: { away_ms: 2_000, back_at_ms: 1 } })} />);
    expect(document.querySelector(".recovery-line")).toBeNull();

    render(<RecoveryLine device={null} />);
    expect(document.querySelector(".recovery-line")).toBeNull();
  });
});
