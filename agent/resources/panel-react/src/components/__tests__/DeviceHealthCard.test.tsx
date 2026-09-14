// The device-health surface: the card, the chip, and the wire mapping behind both.
//
// WHAT THESE PIN, and each is a way this panel has been wrong before:
//   * the card prints the NUMBERS beside the shape (a chart is a summary, never the only
//     channel), and says which window it covers;
//   * an empty series draws an EXPLANATION, not a flat line (a line along the floor is
//     indistinguishable from a device that is genuinely idle);
//   * "the device did not answer" and "the host reports no vitals" are different sentences;
//   * the chip is ABSENT unless the rule fires — the component owns nothing but rendering.
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { DeviceHealthCard } from "../DeviceHealthCard";
import { LoadChip } from "../LoadChip";
import { parseVitalsSeries, type VitalsSeries } from "../../hooks/useVitalsSeries";

const now = 1_789_000_000_000;

const series = (cpu: (number | null)[], mem: (number | null)[] = [], interval = 30): VitalsSeries => ({
  samples: cpu.map((c, i) => ({
    tsMs: now - (cpu.length - 1 - i) * interval * 1000,
    cpu: c,
    mem: mem[i] ?? null,
    memTotalMb: 16384,
  })),
  intervalSecs: interval,
  spanSecs: Math.max(0, (cpu.length - 1) * interval),
});

describe("parseVitalsSeries", () => {
  it("reads the device's own cadence and span rather than assuming them", () => {
    const s = parseVitalsSeries({
      ok: true,
      interval_secs: 30,
      span_secs: 300,
      samples: [{ ts_ms: now, cpu_pct: 12.5, mem_pct: 60.1, mem_total_mb: 16384 }],
    });
    expect(s.intervalSecs).toBe(30);
    expect(s.spanSecs).toBe(300);
    expect(s.samples[0]).toEqual({ tsMs: now, cpu: 12.5, mem: 60.1, memTotalMb: 16384 });
  });

  it("drops a sample it cannot place in time and tolerates a body it cannot use", () => {
    expect(parseVitalsSeries({ samples: [{ cpu_pct: 10 }, { ts_ms: now, cpu_pct: 20 }] }).samples).toHaveLength(1);
    expect(parseVitalsSeries(null).samples).toEqual([]);
    expect(parseVitalsSeries({ ok: false }).intervalSecs).toBe(0);
  });
});

describe("DeviceHealthCard", () => {
  it("prints the numbers beside the shape and names the window", () => {
    const { container } = render(
      <DeviceHealthCard series={series([10, 20, 30, 40], [50, 55, 60, 65])} nowMs={now} />,
    );
    const text = container.textContent!;
    expect(text).toContain("avg 25%");
    expect(text).toContain("low 10%");
    expect(text).toContain("high 40%");
    expect(text).toContain("avg 58%"); // 57.5 rounds up, and the panel never prints a fraction it did not measure
    expect(text).toContain("16.0 GB");
    expect(text).toContain("4 readings over 1m");
    expect(text).toContain("one every 30s");
    // Two charts, drawn.
    expect(container.querySelectorAll(".spark-svg")).toHaveLength(2);
    // And a verdict sentence, which for a quiet window says so in words.
    expect(container.querySelector(".health-verdict")!.textContent).toContain("No sustained load");
  });

  it("turns the verdict into the sustained-load sentence when the rule fires", () => {
    const { container } = render(
      <DeviceHealthCard series={series(Array(14).fill(95))} nowMs={now} />,
    );
    const verdict = container.querySelector(".health-verdict")!;
    expect(verdict.getAttribute("data-tone")).toBe("crit");
    expect(verdict.textContent).toContain("at or above 90%");
  });

  it("explains an EMPTY series instead of drawing a flat line", () => {
    const { container } = render(
      <DeviceHealthCard series={{ samples: [], intervalSecs: 30, spanSecs: 0 }} />,
    );
    // Not one chart element, and a sentence that says WHY — "nothing to draw" and "a flat
    // line at zero" look identical on screen and mean opposite things.
    expect(container.querySelectorAll(".spark-svg")).toHaveLength(0);
    expect(container.textContent).toContain("no vitals readings yet");
  });

  it("says which SERIES is missing when only one of them has readings", () => {
    // The device reports memory but no CPU (a host that cannot compute a delta): the card
    // must draw one chart and NAME the gap, not silently draw a shorter picture.
    const { container } = render(
      <DeviceHealthCard series={series([null, null, null], [40, 41, 42])} nowMs={now} />,
    );
    expect(container.querySelectorAll(".spark-svg")).toHaveLength(1);
    expect(container.querySelector(".spark-none")!.textContent).toContain("no readings");
    expect(container.textContent).toContain("no readings yet");
  });

  it("distinguishes a device that did not answer from a host with no vitals", () => {
    const { container } = render(
      <DeviceHealthCard series={{ samples: [], intervalSecs: 30, spanSecs: 0 }} failed />,
    );
    expect(container.textContent).toContain("did not answer");
    expect(container.textContent).not.toContain("no vitals readings yet");
  });
});

describe("LoadChip", () => {
  it("renders nothing unless the series says a load has persisted", () => {
    expect(render(<LoadChip series={series([10, 20, 30])} nowMs={now} />).container.firstChild).toBeNull();
    expect(render(<LoadChip series={null} nowMs={now} />).container.firstChild).toBeNull();
    expect(
      render(<LoadChip series={{ samples: [], intervalSecs: 0, spanSecs: 0 }} nowMs={now} />).container
        .firstChild,
    ).toBeNull();
  });

  it("shows the metric and the duration, with the numbers on the hover", () => {
    const { container } = render(<LoadChip series={series(Array(14).fill(94))} nowMs={now} />);
    const chip = container.querySelector(".load-chip.crit")!;
    expect(chip.textContent).toContain("CPU pegged");
    expect(chip.getAttribute("data-metric")).toBe("cpu");
    expect(chip.getAttribute("title")).toContain("sustained load");
    expect(container.querySelector(".load-mark.crit")).not.toBeNull();
  });
});
