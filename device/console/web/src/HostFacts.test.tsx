import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { HostFactsBlock } from "./HostFacts";

const GIB = 1024 ** 3;
const NOW = 1_700_000_000_000;

const host = {
  os: { name: "Windows", version: "11" },
  uptime_s: 90_061,
  memory: { total_bytes: 8 * GIB, available_bytes: 4 * GIB },
  disks: [{ name: "/", total_bytes: 100 * GIB, free_bytes: 25 * GIB }],
  sessions: [{ id: 2, user: "operator", state: "active" }],
  helper: { registered: true, pid: 4812, session: 2 },
  update: {
    current: { version: "0.3.218" },
    previous: { version: "0.3.217" },
    repairs: []
  }
};

const sample = (ts_ms: number) => ({
  ts_ms,
  memory_available_bytes: 4 * GIB,
  memory_total_bytes: 8 * GIB,
  disk_name: "/",
  disk_free_bytes: 25 * GIB,
  disk_total_bytes: 100 * GIB
});

describe("HostFactsBlock — the Machine page's subject", () => {
  it("draws the machine's live facts and the sampled history", () => {
    const { container } = render(
      <HostFactsBlock host={host} vitals={[sample(NOW - 60_000), sample(NOW)]} />
    );

    expect(screen.getByText("4.0 GiB free of 8.0 GiB")).toBeInTheDocument();
    expect(screen.getByText("25.0 GiB of 100.0 GiB")).toBeInTheDocument();
    expect(screen.getByText("operator@2 active")).toBeInTheDocument();
    expect(screen.getByText(/current 0\.3\.218 · previous 0\.3\.217/)).toBeInTheDocument();
    expect(screen.getByText("session 2")).toBeInTheDocument();
    expect(screen.queryByText(/not registered/)).not.toBeInTheDocument();
    expect(screen.queryByText(/not running/i)).not.toBeInTheDocument();
    expect(screen.getByText("Memory available 50.0%")).toBeInTheDocument();
    expect(screen.getByText("Tightest disk / 25.0% free")).toBeInTheDocument();
    expect(container.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
  });

  it("says the history is still collecting before the second sample", () => {
    const { container } = render(<HostFactsBlock host={host} vitals={[]} />);

    expect(
      screen.getByText(/Collecting memory and disk history while this page is open/)
    ).toBeInTheDocument();
    expect(container.querySelector("svg")).toBeNull();
  });

  it("draws no row for an absent record instead of a zero", () => {
    render(<HostFactsBlock host={{}} vitals={[]} />);

    // No memory, no disks, no sessions, no update: the block is the chart's collecting sentence,
    // not four rows claiming the machine has 0 bytes.
    expect(screen.queryByText("Memory")).not.toBeInTheDocument();
    expect(screen.queryByText(/free of/)).not.toBeInTheDocument();
    expect(screen.queryByText("Helper")).toBeInTheDocument();
    // No helper fact at all is "not reported": the row claims neither a registration nor a process
    // state the console never read.
    expect(screen.getByText("not reported")).toBeInTheDocument();
    expect(screen.queryByText(/not running/i)).not.toBeInTheDocument();
  });

  it("says no helper registered this boot instead of claiming a process state", () => {
    // The fleet's fact for a machine whose per-user helper never registered: registered is false
    // and both identifying fields are null.
    render(
      <HostFactsBlock
        host={{ helper: { registered: false, pid: null, session: null } }}
        vitals={[]}
      />
    );

    expect(screen.getByText("Helper")).toBeInTheDocument();
    expect(screen.getByText("not registered this boot")).toBeInTheDocument();
    expect(screen.queryByText(/not running/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/session/)).not.toBeInTheDocument();
  });

  it("shows the pid and session only when the fleet reported them", () => {
    // A helper process that exists (the device reported its pid and Windows session) but has not
    // registered a port: the row says the registration fact and names what is known beside it.
    render(
      <HostFactsBlock
        host={{ helper: { registered: false, pid: 4812, session: 2 } }}
        vitals={[]}
      />
    );

    expect(
      screen.getByText("not registered this boot · pid 4812 · session 2")
    ).toBeInTheDocument();
  });

  it("keeps the registered helper's session line", () => {
    render(<HostFactsBlock host={host} vitals={[]} />);

    expect(screen.getByText("session 2")).toBeInTheDocument();
    expect(screen.queryByText(/not registered/)).not.toBeInTheDocument();
  });

  it("states the repair task's registration, last result and next run", () => {
    render(
      <HostFactsBlock
        host={{
          repair_task: {
            registered: true,
            last_run_ms: NOW - 300_000,
            last_result: 0,
            next_run_ms: NOW + 60_000
          }
        }}
        vitals={[]}
      />
    );

    expect(screen.getByText("Repair task")).toBeInTheDocument();
    // The day and clock are the console's own timezone, so the shape is the assertion; the values
    // are the machine's facts and are not rounded into relative ages.
    expect(
      screen.getByText(/^registered · last run .+ · last result 0 · next .+$/)
    ).toBeInTheDocument();
  });

  it("writes a failed repair result as the audit writes it", () => {
    render(
      <HostFactsBlock
        host={{
          repair_task: {
            registered: true,
            last_run_ms: NOW - 60_000,
            last_result: 2_147_942_402,
            next_run_ms: NOW
          }
        }}
        vitals={[]}
      />
    );

    expect(screen.getByText(/last result 0x80070002/)).toBeInTheDocument();
  });

  it("says a missing repair task is not registered", () => {
    render(<HostFactsBlock host={{ repair_task: { registered: false } }} vitals={[]} />);

    expect(screen.getByText("not registered")).toBeInTheDocument();
    expect(screen.queryByText(/last result/)).not.toBeInTheDocument();
  });

  it("draws no repair task row when the machine did not report one", () => {
    render(<HostFactsBlock host={host} vitals={[]} />);

    expect(screen.queryByText("Repair task")).not.toBeInTheDocument();
  });

  it("does not invent a last run for a task that has never run", () => {
    render(
      <HostFactsBlock
        host={{ repair_task: { registered: true, last_result: 0, next_run_ms: NOW } }}
        vitals={[]}
      />
    );

    const value = screen.getByText(/^registered · last result 0 · next .+$/);
    expect(value.textContent).not.toContain("last run");
  });
});
