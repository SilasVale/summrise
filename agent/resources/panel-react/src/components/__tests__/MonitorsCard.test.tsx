// The reachability surfaces: the wire mapping, the card, and the strip chip.
//
// Each case here is a way this instrument can lie to an operator:
//   * a failed probe drawn as a POINT rather than a gap (a latency nobody measured);
//   * "no probes yet" rendered as 0% up (which reads as a dead host);
//   * a chip that appears for a healthy watch, or that says "1 monitor down" without naming
//     which one and for how long — the two facts the operator needs.
import { describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { MonitorsCard } from "../MonitorsCard";
import { MonitorChip } from "../MonitorChip";
import { downTargets, fmtSince, parseMonitors, type Monitors } from "../../hooks/useMonitors";

const now = 1_789_000_000_000;
const probe = (i: number, ok: boolean, ms: number | null = ok ? 5 : null) => ({
  ts_ms: now - (9 - i) * 15_000,
  ok,
  ms,
});
const series = (oks: boolean[]) => oks.map((ok, i) => probe(i, ok));

const target = (over: Partial<{ id: string; upNow: boolean | null; oks: boolean[] }> = {}) => {
  const oks = over.oks ?? [true, true, true];
  const up = oks.filter(Boolean).length;
  const upNow = over.upNow === undefined ? (oks.length ? oks[oks.length - 1] : null) : over.upNow;
  const first = oks[0] === undefined ? null : oks[0];
  void first;
  const id = over.id ?? "192.168.1.1:22";
  // host/port DERIVED from the id, so two targets in one test are two different hosts — the
  // helper's first version hardcoded them and the chip's two rows silently read as one host.
  const [host, portStr] = id.split(":");
  return {
    id,
    host,
    port: Number(portStr) || 22,
    series: series(oks),
    summary: {
      probes: oks.length,
      up,
      down: oks.length - up,
      upPct: oks.length ? Math.round((up / oks.length) * 100) : null,
      upNow,
      sinceMs: oks.length ? now - 30_000 : null,
      latency: up ? { min: 3, avg: 5, max: 9 } : null,
    },
  };
};

const monitors = (targets: ReturnType<typeof target>[]): Monitors => ({
  targets,
  intervalSecs: 15,
  seriesMax: 240,
});

describe("parseMonitors", () => {
  it("reads targets, summaries and series, and tolerates a body it cannot use", () => {
    const m = parseMonitors({
      ok: true,
      interval_secs: 15,
      series_max: 240,
      targets: [
        {
          id: "a:22",
          host: "a",
          port: 22,
          summary: { probes: 2, up: 1, down: 1, up_pct: 50, up_now: false, since_ms: now - 15_000, latency: { min: 3, avg: 4, max: 5 } },
          series: [probe(0, true, 4), probe(1, false)],
        },
      ],
    });
    expect(m.intervalSecs).toBe(15);
    expect(m.targets[0].summary.upNow).toBe(false);
    expect(m.targets[0].summary.upPct).toBe(50);
    // The failed probe keeps its stamp and carries NO latency.
    expect(m.targets[0].series[1].ok).toBe(false);
    expect(m.targets[0].series[1].ms).toBeNull();
    expect(m.targets[0].series[1].tsMs).toBe(probe(1, false).ts_ms);
    // An unusable body is an EMPTY list, not a throw, and a probe with no stamp is dropped.
    expect(parseMonitors(null).targets).toEqual([]);
    expect(parseMonitors({ targets: [{ id: "x", series: [{ ok: true }] }] }).targets[0].series).toEqual([]);
  });

  it("keeps 'no probes' as null rather than inventing a share", () => {
    const m = parseMonitors({ targets: [{ id: "x:22", host: "x", port: 22, summary: { probes: 0, up: 0, down: 0 }, series: [] }] });
    expect(m.targets[0].summary.upPct).toBeNull();
    expect(m.targets[0].summary.upNow).toBeNull();
  });
});

describe("MonitorsCard", () => {
  const noop = () => {};

  it("names the state and the duration, and prints the numbers beside the shape", () => {
    const { container } = render(
      <MonitorsCard
        monitors={monitors([target({ oks: [true, true, false] })])}
        onAdd={async () => ({ ok: true })}
        onRemove={noop}
        onProbe={noop}
        nowMs={now}
      />,
    );
    const row = container.querySelector(".monitor-row")!;
    expect(row.getAttribute("data-state")).toBe("down");
    expect(row.textContent).toContain("192.168.1.1:22");
    expect(row.textContent).toContain("down 30s");
    expect(row.textContent).toContain("67% up");
    expect(row.textContent).toContain("1 failed probe");
    // One gap in the line, because a failed probe is not a latency.
    expect(container.querySelectorAll(".spark-svg").length).toBe(1);
  });

  it("says 'no readings' for a target that has none, never 0%", () => {
    const t = target({ oks: [], upNow: null });
    const { container } = render(
      <MonitorsCard monitors={monitors([t])} onAdd={async () => ({ ok: true })} onRemove={noop} onProbe={noop} nowMs={now} />,
    );
    expect(container.querySelector(".monitor-row")!.textContent).toContain("no readings");
    expect(container.textContent).not.toContain("0% up");
  });

  it("sends the form's values and shows the device's reason VERBATIM on refusal", async () => {
    const onAdd = vi.fn(async () => ({ ok: false, error: "a port is required (22 for SSH, 80 for a web UI, …)" }));
    const { container } = render(
      <MonitorsCard monitors={monitors([])} onAdd={onAdd} onRemove={noop} onProbe={noop} nowMs={now} />,
    );
    fireEvent.change(screen.getByLabelText("host"), { target: { value: "192.168.1.1" } });
    fireEvent.change(screen.getByLabelText("port"), { target: { value: "22" } });
    fireEvent.click(screen.getByRole("button", { name: "watch" }));
    await waitFor(() => expect(onAdd).toHaveBeenCalledWith("192.168.1.1", 22));
    await waitFor(() =>
      expect(container.querySelector(".monitor-error")!.textContent).toContain("a port is required"),
    );
  });

  it("clears the host field after a successful add, and keeps it after a failure", async () => {
    let ok = true;
    const onAdd = vi.fn(async () => (ok ? { ok: true } : { ok: false, error: "nope" }));
    render(<MonitorsCard monitors={monitors([])} onAdd={onAdd} onRemove={noop} onProbe={noop} nowMs={now} />);
    const host = screen.getByLabelText("host") as HTMLInputElement;
    fireEvent.change(host, { target: { value: "10.0.0.1" } });
    fireEvent.click(screen.getByRole("button", { name: "watch" }));
    await waitFor(() => expect(host.value).toBe(""));
    ok = false;
    fireEvent.change(host, { target: { value: "10.0.0.2" } });
    fireEvent.click(screen.getByRole("button", { name: "watch" }));
    await waitFor(() => expect(host.value).toBe("10.0.0.2"));
  });

  it("offers check-now and remove per target", async () => {
    const onProbe = vi.fn();
    const onRemove = vi.fn();
    render(
      <MonitorsCard monitors={monitors([target()])} onAdd={async () => ({ ok: true })} onRemove={onRemove} onProbe={onProbe} nowMs={now} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "check now" }));
    fireEvent.click(screen.getByRole("button", { name: "remove" }));
    expect(onProbe).toHaveBeenCalledWith("192.168.1.1:22");
    expect(onRemove).toHaveBeenCalledWith("192.168.1.1:22");
  });
});

describe("MonitorChip", () => {
  it("is silent when every target is up or unknown", () => {
    const up = monitors([target({ oks: [true, true] })]);
    expect(render(<MonitorChip monitors={up} nowMs={now} />).container.firstChild).toBeNull();
    expect(render(<MonitorChip monitors={monitors([])} nowMs={now} />).container.firstChild).toBeNull();
    expect(render(<MonitorChip monitors={null} nowMs={now} />).container.firstChild).toBeNull();
    const unknown = monitors([target({ oks: [], upNow: null })]);
    expect(render(<MonitorChip monitors={unknown} nowMs={now} />).container.firstChild).toBeNull();
  });

  it("names the target and the duration, and counts the others", () => {
    const m = monitors([
      target({ id: "192.168.1.1:22", oks: [true, false] }),
      target({ id: "192.168.1.1:80", oks: [false, false] }),
    ]);
    const { container } = render(<MonitorChip monitors={m} nowMs={now} />);
    const chip = container.querySelector(".monitor-chip")!;
    expect(chip.textContent).toContain("192.168.1.1:22 down 30s (+1)");
    expect(chip.getAttribute("title")).toContain("192.168.1.1:80");
    expect(container.querySelector(".monitor-mark")).not.toBeNull();
  });
});

describe("downTargets / fmtSince", () => {
  it("counts only targets that are down NOW, with an honest duration", () => {
    const m = monitors([
      target({ id: "a:22", oks: [false, false] }),
      target({ id: "b:22", oks: [true, true] }),
      target({ id: "c:22", oks: [], upNow: null }),
    ]);
    const down = downTargets(m, now);
    expect(down.map((d) => d.target.id)).toEqual(["a:22"]);
    expect(down[0].sinceMs).toBe(30_000);
  });

  it("formats durations the way the rest of the panel does", () => {
    expect(fmtSince(45_000)).toBe("45s");
    expect(fmtSince(90_000)).toBe("1m");
    expect(fmtSince(3_900_000)).toBe("1h 05m");
    expect(fmtSince(200_000_000)).toBe("2d 7h");
  });
});
