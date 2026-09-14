// The instrument line's contract: it ADDS to the strip, never replaces it, and every
// value it shows has a textual channel.
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { StatusBar } from "../StatusBar";
import type { AgentVitals } from "../../hooks/useAgentVitals";

const V: AgentVitals = { release: "1.2.361", uptime: "2h 14m", cpu: 23.4, mem: 61.2 };

const strip = (props: Partial<Parameters<typeof StatusBar>[0]> = {}) =>
  render(<StatusBar sessions={[]} status="ready" sseState="connected" {...props} />);

describe("StatusBar instrument line", () => {
  it("renders EXACTLY as before when no vitals are known", () => {
    // The instrument is an addition to this line, not a precondition for it: a
    // caller without vitals must get the strip it always got, and this test is what
    // keeps the optional prop optional.
    const { container } = strip();
    expect(container.querySelector(".instrument")).toBeNull();
    expect(container.querySelector("#status")!.textContent).toBe("ready");
    expect(container.querySelector("#session-count")).not.toBeNull();
  });

  it("prints the readings as TEXT beside the arcs — never colour alone", () => {
    const { container } = strip({ vitals: V });
    const text = container.querySelector("#statusbar")!.textContent!;
    expect(text).toContain("23%");
    expect(text).toContain("61%");
    expect(text).toContain("2h 14m");
    expect(text).toContain("1.2.361");
    // The dial is a summary of the same numbers, so it must agree with them.
    const dial = container.querySelector(".vitals-dial")!;
    expect(dial.getAttribute("aria-label")).toBe("CPU 23%, memory 61%");
  });

  it("shows an em dash, not a zero, for a reading the agent has not reported", () => {
    // cpu_pct is a server-side delta: absent on the first sample. 0% there would be
    // a claim the operator cannot check.
    const { container } = strip({ vitals: { ...V, cpu: null } });
    const text = container.querySelector("#statusbar")!.textContent!;
    expect(text).toContain("—");
    expect(text).not.toContain("0%");
  });

  it("shows the device's identity when the caller knows it", () => {
    const { container } = strip({ identity: "d1.agent.saisi.online" });
    expect(container.querySelector(".instrument-identity")!.textContent).toBe(
      "d1.agent.saisi.online",
    );
    // No vitals yet, and the identity still renders: they are independent.
    const bare = strip({ identity: "d1.agent.saisi.online" });
    expect(bare.container.querySelector(".instrument")).toBeNull();
  });

  it("does not render an identity slot when none is known", () => {
    const { container } = strip({ vitals: V });
    expect(container.querySelector(".instrument-identity")).toBeNull();
  });

  it("carries the previous run's verdict on the strip, beside the other chips", () => {
    // The strip is where the operator already looks for "what does this device want
    // from me?" — the waiting count and the boot verdict are the two answers, and this
    // asserts the verdict reaches THAT line rather than a page nobody has open.
    const crashed = strip({
      vitals: {
        ...V,
        lastBoot: { kind: "crashed", detail: "run journal: CRASHED or was killed" },
      },
    });
    expect(crashed.container.querySelector(".boot-chip.warn")!.textContent).toContain(
      "last run crashed",
    );

    // And a strip whose device has no verdict renders exactly as it did before.
    const clean = strip({ vitals: { ...V, lastBoot: null } });
    expect(clean.container.querySelector(".boot-chip")).toBeNull();
  });

  it("passes the 24h crash count to the chip's hover, without inventing one", () => {
    const crashed = { ...V, lastBoot: { kind: "crashed" as const, detail: "CRASHED or was killed" } };
    // With the count: the hover says how often.
    const counted = strip({ vitals: crashed, recentCrashes: 3 });
    expect(counted.container.querySelector(".boot-chip")!.getAttribute("title")).toContain(
      "3 crashes in the last 24 hours",
    );
    // Without it (the shell has not polled /api/boots): the same chip, no count claimed.
    const bare = strip({ vitals: crashed });
    const title = bare.container.querySelector(".boot-chip")!.getAttribute("title")!;
    expect(title).toContain("CRASHED or was killed");
    expect(title).not.toContain("24 hours");
  });

  it("carries the sustained-load chip, and only when the series says so", () => {
    const now = Date.now();
    const series = (cpu: number[]) => ({
      samples: cpu.map((c, i) => ({
        tsMs: now - (cpu.length - 1 - i) * 30_000,
        cpu: c,
        mem: 20,
        memTotalMb: 8192,
      })),
      intervalSecs: 30,
      spanSecs: (cpu.length - 1) * 30,
    });
    // A quiet device: the strip is exactly what it was before this chip existed.
    const quiet = strip({ vitals: V, vitalsSeries: series(Array(14).fill(15)) });
    expect(quiet.container.querySelector(".load-chip")).toBeNull();
    // A pegged one: the chip appears ON THE STRIP, beside the other chips.
    const pegged = strip({ vitals: V, vitalsSeries: series(Array(14).fill(96)) });
    expect(pegged.container.querySelector(".load-chip")!.textContent).toContain("CPU pegged");
    // And with no series at all (an older shell): no chip, no crash.
    expect(strip({ vitals: V }).container.querySelector(".load-chip")).toBeNull();
  });
});
