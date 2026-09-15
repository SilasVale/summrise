// useStripOverflow — DOES THIS STRIP HAVE MORE THAN IT SHOWS?
//
// WHY IT EXISTS (measured, round 36). The operator's panel carries 16 sessions and its tab strip
// measured 211px wide for 1777px of tabs: TWO tabs visible, fourteen behind the edge. The desktop
// strip measured 911px for 1462px — four behind the edge — and it explicitly hid its scrollbar
// (`scrollbar-width: none` + a hidden webkit bar), so nothing on screen said the rest existed.
//
// A scrollbar was the obvious answer and it is not sufficient: `scrollbar-width: thin` is honoured,
// but Chromium on Windows draws OVERLAY scrollbars that appear only while scrolling, so a strip at
// rest still looks complete. The affordance has to be drawn by the app.
//
// This reports one boolean, measured from the element itself (`scrollWidth > clientWidth`), and
// re-measures when the things that can change it do: the session count, the window size, and the
// strip's own box. The caller renders a fade — and NOTHING when the content fits, which is the half
// that matters: a permanent "there is more" hint on a strip with two tabs is a lie.
import { useEffect, useState } from "react";

/**
 * @param ref      the scrolling strip
 * @param revision any value that changes when the CONTENT does (a session count, a list length)
 * @returns        true when the strip is hiding content to its right
 */
export function useStripOverflow(ref: React.RefObject<HTMLElement | null>, revision: unknown): boolean {
  const [overflowing, setOverflowing] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    // A tolerance of one pixel: sub-pixel layout makes `scrollWidth` exceed `clientWidth` by a
    // fraction on strips that fit exactly, and a fade that appears on a strip with nothing hidden
    // is worse than no fade.
    const measure = () => setOverflowing(el.scrollWidth > el.clientWidth + 1);
    measure();
    // `scroll` matters because the fade must go away once the last tab is reached — the strip is no
    // longer hiding anything at that moment.
    el.addEventListener("scroll", measure, { passive: true });
    const ro = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    ro?.observe(el);
    window.addEventListener("resize", measure);
    return () => {
      el.removeEventListener("scroll", measure);
      ro?.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, [ref, revision]);

  return overflowing;
}
