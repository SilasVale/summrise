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
import { MonitorAlerts } from "../MonitorAlerts";
import {
  downTargets,
  fmtSince,
  parseMonitorChange,
  parseMonitors,
  unstableTargets,
  type MonitorTarget,
  type Monitors,
} from "../../hooks/useMonitors";

const now = 1_789_000_000_000;
/** Module scope: the per-describe `noop` does not reach the other blocks (learned here). */
const noop = () => {};
/** The PARSED shape the hook hands to components (`tsMs`, not the wire's `ts_ms`) — the first
 *  version of this helper built the wire shape, which vitest ran happily and `tsc --noEmit`
 *  refused, i.e. the panel's own build gate caught what the test run could not. */
const probe = (i: number, ok: boolean, ms: number | null = ok ? 5 : null) => ({
  tsMs: now - (9 - i) * 15_000,
  ok,
  ms,
});
const series = (oks: boolean[]) => oks.map((ok, i) => probe(i, ok));
/** The WIRE shape (`ts_ms`), for the parser tests — the two are deliberately not the same
 *  object, and a helper that conflated them would let a parser bug through. */
const wire = (i: number, ok: boolean, ms: number | null = ok ? 5 : null) => ({
  ts_ms: now - (9 - i) * 15_000,
  ok,
  ms,
});

const target = (
  over: Partial<{ id: string; upNow: boolean | null; oks: boolean[] }> = {},
): MonitorTarget => {
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
    path: null,
    series: series(oks),
    // TRANSITIONS derived the way the device derives them (a flip, with the duration of the
    // state it ended), so the fixture cannot describe a log the device could not produce.
    transitions: (() => {
      const out: { atMs: number; up: boolean; lastedMs: number }[] = [];
      let runStart = series(oks)[0]?.tsMs ?? now;
      for (let i = 1; i < oks.length; i++) {
        if (oks[i] !== oks[i - 1]) {
          out.push({ atMs: series(oks)[i].tsMs, up: oks[i], lastedMs: series(oks)[i].tsMs - runStart });
          runStart = series(oks)[i].tsMs;
        }
      }
      return out;
    })(),
    summary: {
      probes: oks.length,
      up,
      down: oks.length - up,
      upPct: oks.length ? Math.round((up / oks.length) * 100) : null,
      upNow,
      sinceMs: oks.length ? now - 30_000 : null,
      latency: up ? { min: 3, avg: 5, max: 9 } : null,
      lastStatus: null,
      // DROPS: the falls in the fixture, which is the same rule the device counts.
      drops: oks.length ? oks.slice(1).filter((ok, i) => oks[i] && !ok).length : null,
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
          series: [wire(0, true, 4), wire(1, false)],
        },
      ],
    });
    expect(m.intervalSecs).toBe(15);
    expect(m.targets[0].summary.upNow).toBe(false);
    expect(m.targets[0].summary.upPct).toBe(50);
    // The failed probe keeps its stamp and carries NO latency.
    expect(m.targets[0].series[1].ok).toBe(false);
    expect(m.targets[0].series[1].ms).toBeNull();
    expect(m.targets[0].series[1].tsMs).toBe(probe(1, false).tsMs);
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
    // "No readings YET" — nothing has been probed. The other state ("never answered") is
    // asserted below, and the two must not read the same on a row that also counts failures.
    expect(container.querySelector(".monitor-row")!.textContent).toContain("no readings yet");
    expect(container.textContent).not.toContain("0% up");
  });

  it("says 'never answered' for a target whose every probe failed", () => {
    // The row also counts the failed probes; "no readings" beside "7 failed probes" reads as a
    // contradiction, and the two facts are about different things (latency vs. reachability).
    const { container } = render(
      <MonitorsCard
        monitors={monitors([target({ oks: [false, false, false] })])}
        onAdd={async () => ({ ok: true })}
        onRemove={noop}
        onProbe={noop}
        nowMs={now}
      />,
    );
    const row = container.querySelector(".monitor-row")!;
    expect(row.textContent).toContain("never answered");
    expect(row.textContent).not.toContain("no readings yet");
    expect(row.textContent).toContain("3 failed probes");
    // No chart at all: a line along the floor would read as a measurement.
    expect(container.querySelectorAll(".spark-svg")).toHaveLength(0);
  });

  it("sends the form's values and shows the device's reason VERBATIM on refusal", async () => {
    const onAdd = vi.fn(async () => ({ ok: false, error: "a port is required (22 for SSH, 80 for a web UI, …)" }));
    const { container } = render(
      <MonitorsCard monitors={monitors([])} onAdd={onAdd} onRemove={noop} onProbe={noop} nowMs={now} />,
    );
    fireEvent.change(screen.getByLabelText("host"), { target: { value: "192.168.1.1" } });
    fireEvent.change(screen.getByLabelText("port"), { target: { value: "22" } });
    fireEvent.click(screen.getByRole("button", { name: "watch" }));
    // The optional PATH rides along as an empty string when the operator does not fill it in —
    // an empty path means the plain TCP connect this instrument started as.
    await waitFor(() => expect(onAdd).toHaveBeenCalledWith("192.168.1.1", 22, ""));
    await waitFor(() =>
      expect(container.querySelector(".monitor-error")!.textContent).toContain("a port is required"),
    );
  });

  it("sends a path when one is given, and shows the status a path-bearing target answered", () => {
    const onAdd = vi.fn(async () => ({ ok: true }));
    const { container, rerender } = render(
      <MonitorsCard monitors={monitors([])} onAdd={onAdd} onRemove={noop} onProbe={noop} nowMs={now} />,
    );
    fireEvent.change(screen.getByLabelText("host"), { target: { value: "192.168.1.1" } });
    fireEvent.change(screen.getByLabelText("port"), { target: { value: "80" } });
    fireEvent.change(screen.getByLabelText("path"), { target: { value: "/status" } });
    fireEvent.click(screen.getByRole("button", { name: "watch" }));
    expect(onAdd).toHaveBeenCalledWith("192.168.1.1", 80, "/status");

    // A row watched with a path shows the URL and the CODE — including when it is up, because
    // "404 up" and "200 up" are different facts about the same service.
    const withPath = { ...target({ oks: [true, true] }), path: "/status" };
    withPath.summary = { ...withPath.summary, lastStatus: 503 };
    rerender(
      <MonitorsCard monitors={monitors([withPath])} onAdd={onAdd} onRemove={noop} onProbe={noop} nowMs={now} />,
    );
    expect(container.querySelector(".monitor-name")!.textContent).toBe("192.168.1.1:22/status");
    expect(container.querySelector(".monitor-status")!.textContent).toBe("HTTP 503");
    expect(container.querySelector(".monitor-status")!.className).toContain("is-bad");
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

describe("the flapping rule (and its chip)", () => {
  it("calls a link unstable only when it FELL more than once, and only while it is up", () => {
    // Up every time the operator looks, but it fell twice: the pattern the state cannot show.
    const flapping = monitors([target({ oks: [true, false, true, false, true] })]);
    expect(unstableTargets(flapping).map((t) => t.id)).toEqual(["192.168.1.1:22"]);

    // ONE drop is not unstable: it is often the operator's own reboot.
    expect(unstableTargets(monitors([target({ oks: [true, false, true] })]))).toEqual([]);

    // A target that is DOWN is named by downTargets, not here — two chips about one host is
    // worse than one.
    expect(unstableTargets(monitors([target({ oks: [true, false, false] })]))).toEqual([]);

    // Steady, or nothing probed: silence.
    expect(unstableTargets(monitors([target({ oks: [true, true, true] })]))).toEqual([]);
    expect(unstableTargets(monitors([target({ oks: [], upNow: null })]))).toEqual([]);
  });

  it("shows the pattern when nothing is down, and the outage when something is", () => {
    const flapping = monitors([target({ oks: [true, false, true, false, true] })]);
    const chip = render(<MonitorChip monitors={flapping} nowMs={now} />).container.querySelector(".monitor-chip")!;
    expect(chip.textContent).toContain("192.168.1.1:22 flapping (2 drops)");
    expect(chip.getAttribute("title")).toContain("up now");
    // A hollow mark: the silhouette separates "unstable" from "down".
    expect(container_mark(chip, "is-flapping")).toBe(true);

    // A DOWN target outranks the pattern: the chip names the outage (and the uptime story).
    const both = monitors([
      target({ id: "a:22", oks: [true, false, false] }),
      target({ id: "b:22", oks: [true, false, true, false, true] }),
    ]);
    const chip2 = render(<MonitorChip monitors={both} nowMs={now} />).container.querySelector(".monitor-chip")!;
    expect(chip2.textContent).toContain("a:22 down");
    expect(chip2.textContent).not.toContain("flapping");
  });
});

/** The mark's class list, as a boolean — the shape channel this chip relies on. */
function container_mark(chip: Element, cls: string): boolean {
  const mark = chip.querySelector(".monitor-mark");
  return !!mark && mark.className.includes(cls);
}

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

describe("the outage log and the device speaking", () => {
  it("lists the transitions newest-first with what each one ENDED", () => {
    const { container } = render(
      <MonitorsCard
        monitors={monitors([target({ oks: [true, true, false, false, true] })])}
        onAdd={async () => ({ ok: true })}
        onRemove={noop}
        onProbe={noop}
        nowMs={now}
      />,
    );
    const log = [...container.querySelectorAll(".monitor-log li")];
    expect(log).toHaveLength(2);
    // Newest first: the recovery, naming the OUTAGE; then the fall, naming the uptime it ended.
    expect(log[0].textContent).toContain("back up");
    expect(log[0].textContent).toContain("after 30s down");
    expect(log[1].textContent).toContain("went down");
    expect(log[1].textContent).toContain("it had been up");
    expect(log[0].querySelector(".monitor-log-time")!.textContent).toMatch(/^\d\d:\d\d:\d\d$/);
  });

  it("shows no log at all for a target that never changed state", () => {
    const { container } = render(
      <MonitorsCard monitors={monitors([target({ oks: [true, true, true] })])} onAdd={async () => ({ ok: true })} onRemove={noop} onProbe={noop} nowMs={now} />,
    );
    expect(container.querySelectorAll(".monitor-log")).toHaveLength(0);
  });

  it("reads a change frame, and refuses one it cannot use", () => {
    const a = parseMonitorChange({
      ev: "monitor-change",
      id: "192.168.1.1:22",
      host: "192.168.1.1",
      port: 22,
      up: false,
      at_ms: now,
      lasted_ms: 61_000,
    })!;
    expect(a.id).toBe("192.168.1.1:22");
    expect(a.up).toBe(false);
    expect(a.lastedMs).toBe(61_000);
    expect(a.key).toBe(`192.168.1.1:22:${now}`);
    // A different event, a frame with no id, and one with no stamp: all null, never a banner
    // about something that did not happen.
    expect(parseMonitorChange({ ev: "sessions-changed" })).toBeNull();
    expect(parseMonitorChange({ ev: "monitor-change", at_ms: now })).toBeNull();
    expect(parseMonitorChange({ ev: "monitor-change", id: "x:1" })).toBeNull();
    expect(parseMonitorChange(null)).toBeNull();
  });

  it("names the outage on the way back and the uptime on the way down", () => {
    const { container } = render(
      <MonitorAlerts
        alerts={[
          { key: "k1", id: "a:22", host: "a", port: 22, up: false, lastedMs: 3_600_000, atMs: now, status: null },
          { key: "k2", id: "b:22", host: "b", port: 22, up: true, lastedMs: 135_000, atMs: now, status: null },
        ]}
      />,
    );
    const rows = [...container.querySelectorAll(".monitor-alert")];
    expect(rows[0].textContent).toContain("a:22 is DOWN — it had been up 1h 00m");
    expect(rows[1].textContent).toContain("b:22 is back up after 2m down");
    // is-down / is-up drive the mark's silhouette, which is what separates the two without colour.
    expect(rows[0].className).toContain("is-down");
    expect(rows[1].className).toContain("is-up");
    expect(rows[1].querySelector(".monitor-mark")!.className).toContain("is-up");
  });

  it("renders nothing when the device has said nothing", () => {
    expect(render(<MonitorAlerts alerts={[]} />).container.firstChild).toBeNull();
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
