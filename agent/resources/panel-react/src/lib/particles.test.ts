import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { startParticleField } from "./particles";

/**
 * The particle field is decorative, which makes it exactly the kind of thing that can ship
 * broken and unnoticed: nothing fails, the page just has no motes. These tests pin the three
 * properties that matter, and the DRAW one is the important one — a canvas element that
 * exists but is never drawn to is a field that silently does nothing.
 *
 * jsdom has no 2D context (`getContext("2d")` is null), so it is stubbed. That is not a
 * workaround: with the stub in place the assertions are about THIS module's draw loop, and
 * the null-context path is asserted separately below as the graceful degradation it is.
 */

function stubContext() {
  const calls = { arc: 0, fill: 0, clearRect: 0, fills: [] as string[] };
  const ctx = {
    setTransform: () => {},
    clearRect: () => {
      calls.clearRect++;
    },
    beginPath: () => {},
    arc: () => {
      calls.arc++;
    },
    fill: () => {
      calls.fill++;
      // THE COLOUR MATTERS AS MUCH AS THE DRAW. A field that fills in the wrong palette is the defect this
      // harness was extended for (round 14 of the standing goal).
      calls.fills.push(String(ctx.fillStyle));
    },
    fillStyle: "",
  };
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  HTMLCanvasElement.prototype.getContext = vi.fn(() => ctx) as any;
  return calls;
}

/** Let the rAF loop advance. The module throttles to ~30fps, so timestamps must move. */
function pumpFrames(n: number) {
  let t = 0;
  for (let i = 0; i < n; i++) {
    t += 100;
    vi.advanceTimersByTime(40);
    const cb = rafCallbacks.shift();
    if (cb) cb(t);
  }
}

let rafCallbacks: FrameRequestCallback[] = [];

beforeEach(() => {
  rafCallbacks = [];
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
    rafCallbacks.push(cb);
    return rafCallbacks.length;
  });
  vi.stubGlobal("cancelAnimationFrame", () => {});
  vi.stubGlobal("matchMedia", (q: string) => ({
    matches: false,
    media: q,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  document.querySelectorAll("canvas").forEach((c) => c.remove());
});

describe("startParticleField", () => {
  it("adds a canvas that cannot intercept input or enter the layout", () => {
    stubContext();
    const stop = startParticleField();

    const canvas = document.querySelector<HTMLCanvasElement>("canvas[data-summrise-particles]");
    expect(canvas).not.toBeNull();
    expect(canvas!.getAttribute("aria-hidden")).toBe("true");
    // Decorative AND inert: a field that swallowed clicks would break every control under it,
    // and one that took part in layout would move the page.
    expect(canvas!.style.position).toBe("fixed");
    expect(canvas!.style.pointerEvents).toBe("none");
    expect(canvas!.style.zIndex).toBe("0");

    stop();
    expect(document.querySelector("canvas[data-summrise-particles]")).toBeNull();
  });

  it("ACTUALLY DRAWS — the failure mode nothing else would catch", () => {
    const calls = stubContext();
    const stop = startParticleField();
    pumpFrames(3);

    expect(calls.arc, "motes must be drawn; a canvas nobody paints is not a field").toBeGreaterThan(0);
    expect(calls.fill).toBeGreaterThan(0);
    // And the previous frame is erased, or they would smear into a trail.
    expect(calls.clearRect).toBeGreaterThan(0);
    stop();
  });

  it("does NOTHING under prefers-reduced-motion", () => {
    stubContext();
    vi.stubGlobal("matchMedia", (q: string) => ({
      matches: q.includes("prefers-reduced-motion"),
      media: q,
      addEventListener: () => {},
      removeEventListener: () => {},
    }));

    const stop = startParticleField();

    // Not merely "slower": the honest fallback is no animation at all, leaving the static
    // wash the page already has.
    expect(document.querySelector("canvas[data-summrise-particles]")).toBeNull();
    expect(typeof stop).toBe("function");
    stop();
  });

  it("degrades to nothing when there is no 2D context instead of throwing", () => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    HTMLCanvasElement.prototype.getContext = vi.fn(() => null) as any;
    expect(() => startParticleField()).not.toThrow();
    expect(document.querySelector("canvas[data-summrise-particles]")).toBeNull();
  });
});

describe("the palette", () => {
  it("paints the BRAND's colours, never the palette the rebrand retired", () => {
    // The field read `--aura-1/3/4` — RETIRED — and fell back to 190 cyan, 280 violet and 330 pink, so the
    // ambient layer drawn behind every surface wore the colours the brand had abandoned. It reads
    // `--brand-grad-a/b` and `--accent` now, and as COLOURS: the old code turned a token into a hue and drew
    // `hsla(h 90% 62%)`, so even a correct token would not have been the colour on screen.
    const calls = stubContext();
    document.body.style.setProperty("--brand-grad-a", "#c2410c");
    document.body.style.setProperty("--brand-grad-b", "#9a3412");
    document.body.style.setProperty("--accent", "#bf3a0a");
    startParticleField();
    pumpFrames(4);

    expect(calls.fills.length, "the field must actually paint").toBeGreaterThan(0);
    const brand = /^rgba\((194, 65, 12|154, 52, 18|191, 58, 10), /;
    for (const fill of calls.fills) {
      expect(fill, `"${fill}" is not one of the brand's colours`).toMatch(brand);
    }
    // AND NOT ONE HUE-DERIVED COLOUR, which is the shape the retired palette took.
    expect(calls.fills.some((f) => f.startsWith("hsla(")), "the field is drawing by hue again").toBe(false);
  });
});
