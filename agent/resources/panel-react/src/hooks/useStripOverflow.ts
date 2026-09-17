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
 * THE COUNT, NOT JUST THE FACT. Round 168 measured the live device again: 16 sessions, ten of them
 * labelled `pwsh`, and the strip says "there is more" without ever saying HOW MUCH more — so the reader
 * knows they are missing something and not what. The count was measured here all along and thrown away.
 *
 * A TAB IS HIDDEN WHEN IT IS NOT FULLY VISIBLE, which with a scrolling strip means off either edge, not
 * only the right one. `scrollWidth > clientWidth` tells you the strip overflows; only the children's rects
 * tell you by how many.
 *
 * @param ref      the scrolling strip
 * @param revision any value that changes when the CONTENT does (a session count, a list length)
 * @returns        { overflowing, hidden } — whether it hides anything, and how many children it hides
 */
export function useStripOverflow(
  ref: React.RefObject<HTMLElement | null>,
  revision: unknown,
): { overflowing: boolean; hidden: number } {
  const [state, setState] = useState({ overflowing: false, hidden: 0 });

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    // A tolerance of one pixel: sub-pixel layout makes `scrollWidth` exceed `clientWidth` by a
    // fraction on strips that fit exactly, and a fade that appears on a strip with nothing hidden
    // is worse than no fade.
    const measure = () => {
      const overflowing = el.scrollWidth > el.clientWidth + 1;
      // jsdom has no layout: every rect is 0x0, so this counts nothing there and the hook's tests drive
      // the boolean. On a real engine the children carry real boxes and the count is real.
      const box = el.getBoundingClientRect();
      let hidden = 0;
      for (const child of Array.from(el.children)) {
        // STRIP CHROME IS NOT A TAB. The `+N` chip lives inside the strip (it has to — it sits where the
        // hidden tabs begin), and counting it reported "+11 hidden" on a strip of TEN: the measurement was
        // including the thing that displays the measurement. Round 170 caught it on a rendered page.
        if (child instanceof HTMLElement && child.dataset.stripChrome) continue;
        const r = child.getBoundingClientRect();
        if (r.width === 0 && r.height === 0) continue; // not laid out — say nothing rather than guess
        if (r.left < box.left - 1 || r.right > box.right + 1) hidden += 1;
      }
      setState((prev) => (prev.overflowing === overflowing && prev.hidden === hidden ? prev : { overflowing, hidden }));
    };
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

  return state;
}
