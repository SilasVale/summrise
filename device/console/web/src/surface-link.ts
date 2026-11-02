import type { DeviceTab, Page } from "./Rail";

/**
 * What the fragment names: a destination, the Device tab inside it, or the tool whose sheet is open
 * on Capabilities. A run link (`#run/...`) is not a surface and is read by run-link.ts.
 */
export type Surface = { page: Page; tab?: DeviceTab; tool?: string };

const PAGES: readonly Page[] = ["home", "capabilities", "plugins", "activity", "device"];
const TABS: readonly DeviceTab[] = ["terminal", "screen", "files", "runs", "machine"];

/**
 * The fragments the previous shell shipped. A bookmark, a runbook link or a browser history entry
 * must keep working: the surface moved from seven rail items into one Device workspace, and those
 * old names are aliases, not a second navigation.
 */
const LEGACY: Record<string, Surface> = {
  devices: { page: "home" },
  terminal: { page: "device", tab: "terminal" },
  screen: { page: "device", tab: "screen" },
  files: { page: "device", tab: "files" },
  runs: { page: "device", tab: "runs" },
  system: { page: "device", tab: "machine" },
  audit: { page: "activity" }
};

/** The fragment for a surface: `#home`, `#capabilities/<tool>`, `#device/<tab>`. */
export function surfaceHash(surface: Surface): string {
  if (surface.page === "device") return "#device/" + (surface.tab ?? "terminal");
  if (surface.page === "capabilities" && surface.tool) {
    return "#capabilities/" + encodeURIComponent(surface.tool);
  }
  return "#" + surface.page;
}

/** Read a surface fragment; anything that is not a destination, tab or tool is not a link. */
export function parseSurfaceHash(hash: string): Surface | null {
  const match = /^#([a-z]+)(?:\/(.+))?$/.exec(hash);
  if (!match) return null;
  const name = match[1];
  const rest = match[2];
  if (rest !== undefined) {
    if (name === "device" && (TABS as readonly string[]).includes(rest)) {
      return { page: "device", tab: rest as DeviceTab };
    }
    if (name === "capabilities") {
      try {
        const tool = decodeURIComponent(rest);
        return tool ? { page: "capabilities", tool } : null;
      } catch {
        return null;
      }
    }
    return null;
  }
  if ((PAGES as readonly string[]).includes(name)) return { page: name as Page };
  return LEGACY[name] ?? null;
}
