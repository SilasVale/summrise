// The overflow signal: it must be TRUE when the strip hides content and FALSE when it does not.
//
// WHY THIS EXISTS. Two of sixteen tabs were visible on the operator's own panel and NOTHING said the
// other fourteen existed; the desktop strip hid its scrollbar outright. The fade that fixes it is
// only honest if it is driven by a measurement — a permanent "there is more" hint on a strip with
// two tabs is the same lie in the other direction — so the measurement is what is tested here.
//
// jsdom has no layout, so the numbers are supplied by hand: `scrollWidth`/`clientWidth` are defined
// as getters on the element and the hook re-measures on a resize event. That is the honest way to
// test a measurement in this environment (the alternative is asserting nothing).
import { describe, it, expect } from "vitest";
import { render, act } from "@testing-library/react";
import { useRef } from "react";
import { useStripOverflow } from "../useStripOverflow";

function Probe({ scrollWidth, clientWidth }: { scrollWidth: number; clientWidth: number }) {
  const ref = useRef<HTMLDivElement>(null);
  // getters, not properties: the hook reads them at measure time, exactly like a real element.
  const attach = (el: HTMLDivElement | null) => {
    (ref as { current: HTMLDivElement | null }).current = el;
    if (!el) return;
    Object.defineProperty(el, "scrollWidth", { configurable: true, get: () => scrollWidth });
    Object.defineProperty(el, "clientWidth", { configurable: true, get: () => clientWidth });
  };
  const more = useStripOverflow(ref, `${scrollWidth}:${clientWidth}`);
  return (
    <div ref={attach} data-testid="strip" data-more={more ? "1" : undefined}>
      <span>tab</span>
    </div>
  );
}

describe("useStripOverflow", () => {
  it("is true only when the strip actually hides content", () => {
    const hidden = render(<Probe scrollWidth={1462} clientWidth={911} />);
    expect(hidden.getByTestId("strip").getAttribute("data-more")).toBe("1");
    hidden.unmount();

    const fits = render(<Probe scrollWidth={400} clientWidth={911} />);
    expect(fits.getByTestId("strip").getAttribute("data-more")).toBeNull();
    fits.unmount();
  });

  it("tolerates a sub-pixel difference, which is not hidden content", () => {
    // Fractional layout makes scrollWidth exceed clientWidth by a fraction on strips that fit; a
    // fade appearing there is a lie about hidden tabs.
    const { getByTestId } = render(<Probe scrollWidth={911.4} clientWidth={911} />);
    expect(getByTestId("strip").getAttribute("data-more")).toBeNull();
  });

  it("re-measures when the window changes", () => {
    // The count is the hook's `revision`; a resize is the other thing that can change the answer,
    // and both must re-run the measurement rather than leaving a stale fade on screen.
    const el = document.createElement("div");
    let sw = 100, cw = 200;
    Object.defineProperty(el, "scrollWidth", { configurable: true, get: () => sw });
    Object.defineProperty(el, "clientWidth", { configurable: true, get: () => cw });
    const ref = { current: el } as React.RefObject<HTMLElement | null>;
    const Probe2 = () => {
      const more = useStripOverflow(ref, 0);
      return <div data-testid="s" data-more={more ? "1" : undefined} />;
    };
    const { getByTestId } = render(<Probe2 />);
    expect(getByTestId("s").getAttribute("data-more")).toBeNull();
    act(() => {
      sw = 400; // the strip now overflows
      window.dispatchEvent(new Event("resize"));
    });
    expect(getByTestId("s").getAttribute("data-more")).toBe("1");
  });
});
