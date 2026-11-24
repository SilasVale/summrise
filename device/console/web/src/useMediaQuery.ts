import { useEffect, useState } from "react";

/**
 * A media query as a render-time boolean. `matchMedia` is absent in the jsdom test environment and
 * optional in an old WebView, so an unsupported window answers `false`: the desktop placement is the
 * safe default, and CSS still handles the width when JavaScript cannot.
 */
export function useMediaQuery(query: string): boolean {
  const read = () =>
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia(query).matches;

  const [matches, setMatches] = useState(read);

  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return;
    const list = window.matchMedia(query);
    const sync = () => setMatches(list.matches);
    sync();
    if (typeof list.addEventListener === "function") {
      list.addEventListener("change", sync);
      return () => list.removeEventListener("change", sync);
    }
    // Older WebView2: the deprecated listener is the only one it has.
    list.addListener(sync);
    return () => list.removeListener(sync);
  }, [query]);

  return matches;
}
