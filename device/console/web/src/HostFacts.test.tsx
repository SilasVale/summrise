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

  it("states the 9VH's registered repair task and never the absent-build sentence", () => {
    // The 9VH on 0.3.292: the fleet's host facts carry `{registered: true, last_result: 0}` (plus
    // the run moments). The present fact keeps the row's shapes, whatever the build reports.
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
        online
        runningVersion="0.3.292"
      />
    );

    expect(screen.getByText("Repair task")).toBeInTheDocument();
    // The day and clock are the console's own timezone, so the shape is the assertion; the values
    // are the machine's facts and are not rounded into relative ages. A zero result is the Task
    // Scheduler's success, and the row says so instead of leaving a code to be decoded (audit-44
    // F11).
    expect(
      screen.getByText(/^registered · last run .+ · last result 0 \(success\) · next .+$/)
    ).toBeInTheDocument();
    expect(screen.queryByText(/not reported by this build/)).not.toBeInTheDocument();
  });

  it("keeps the raw repair result code in the row's title", () => {
    // The visible verdict is `0 (success)`; the raw code is what an operator matches to a log, so
    // it stays one hover away rather than being replaced by the word.
    render(
      <HostFactsBlock
        host={{ repair_task: { registered: true, last_result: 0 } }}
        vitals={[]}
        online
        runningVersion="0.3.292"
      />
    );

    const value = screen.getByText(/last result 0 \(success\)/);
    expect(value).toHaveAttribute("title", expect.stringContaining("0"));
    expect(value).toHaveAttribute("title", expect.stringMatching(/success/i));
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

  it("says a missing repair task is not registered, not that the build withheld it", () => {
    // `registered: false` is the machine's Task Scheduler answer: the task is absent. The row must
    // keep that word even when the build reports the fact, and never borrow the absent-build
    // sentence for a fact the machine did report.
    render(
      <HostFactsBlock
        host={{ repair_task: { registered: false } }}
        vitals={[]}
        online
        runningVersion="0.3.292"
      />
    );

    expect(screen.getByText("not registered")).toBeInTheDocument();
    expect(screen.queryByText(/not reported by this build/)).not.toBeInTheDocument();
    expect(screen.queryByText(/last result/)).not.toBeInTheDocument();
  });

  it("names the 84T's running build when its facts carry no repair task", () => {
    // The measured 84T on 0.3.291: online, and its `/fleet` host object has no `repair_task` key.
    // The row used to vanish, which read as a machine with no repair path; it now says what the
    // frame shows and names the build whose frame it is, so the absence cannot pass for health.
    render(
      <HostFactsBlock
        host={{
          memory: { total_bytes: 16 * GIB, available_bytes: 5 * GIB },
          sessions: [{ id: 5, state: "connected" }],
          helper: { registered: false }
        }}
        vitals={[]}
        online
        runningVersion="0.3.291"
      />
    );

    expect(screen.getByText("Repair task")).toBeInTheDocument();
    expect(screen.getByText("not reported by this build (v0.3.291)")).toBeInTheDocument();
    // The silence is not the machine's `registered: false`: the console never read a Task Scheduler
    // answer, so it must not claim the task is absent.
    expect(screen.queryByText("not registered")).not.toBeInTheDocument();
  });

  it("draws no repair task row for an offline machine's dated facts", () => {
    // The row is for a live machine: an offline block says when its frame was, and its absent
    // facts stay silent rather than naming a running build the console cannot read them from.
    render(
      <HostFactsBlock host={host} vitals={[]} readAtMs={NOW - 3_600_000} online={false} />
    );

    expect(screen.queryByText("Repair task")).not.toBeInTheDocument();
  });

  it("does not invent a version for an online build that announced none", () => {
    render(
      <HostFactsBlock host={{}} vitals={[]} online runningVersion={null} />
    );

    expect(screen.getByText("not reported by this build")).toBeInTheDocument();
    expect(screen.queryByText(/\(v/)).not.toBeInTheDocument();
  });

  it("names what the machine is — OS, architecture and cores — above its memory", () => {
    // **THE MACHINE PAGE ANSWERS WHAT THE MACHINE IS (audit-44 F8).** The fleet frame carries the
    // platform (`Windows 11`, `x86_64`, 12 cores) and no surface printed it, so support had to open
    // a shell to learn what the machine runs. The line sits above Memory, where the facts begin.
    render(
      <HostFactsBlock
        host={{
          os: { name: "Windows", release: "Windows 11", version: "10.0.26100", arch: "x86_64" },
          cpu: { cores: 12 },
          memory: { total_bytes: 8 * GIB, available_bytes: 4 * GIB }
        }}
        vitals={[]}
      />
    );

    const platform = screen.getByText("Windows 11 · x86_64 · 12 cores");
    const memory = screen.getByText("Memory");
    expect(
      platform.compareDocumentPosition(memory) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
  });

  it("omits a platform fact the frame did not carry instead of inventing a zero", () => {
    // A frame with no architecture and no core count still names the OS; the row must not print
    // `undefined` or `0 cores`. A frame with no OS at all draws no Platform row.
    const { rerender } = render(
      <HostFactsBlock
        host={{ os: { name: "linux", version: "6.8.0" }, cpu: { cores: 0 } }}
        vitals={[]}
      />
    );

    expect(screen.getByText("linux 6.8.0")).toBeInTheDocument();
    expect(screen.queryByText(/0 cores/)).toBeNull();
    expect(screen.queryByText(/undefined/)).toBeNull();

    rerender(<HostFactsBlock host={{ memory: { total_bytes: 8 * GIB, available_bytes: 4 * GIB } }} vitals={[]} />);
    expect(screen.queryByText("Platform")).toBeNull();
    expect(screen.getByText("Memory")).toBeInTheDocument();
  });

  it("does not invent a last run for a task that has never run", () => {
    render(
      <HostFactsBlock
        host={{ repair_task: { registered: true, last_result: 0, next_run_ms: NOW } }}
        vitals={[]}
      />
    );

    const value = screen.getByText(/^registered · last result 0 \(success\) · next .+$/);
    expect(value.textContent).not.toContain("last run");
  });
});
