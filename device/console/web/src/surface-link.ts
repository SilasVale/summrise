import type { DeviceTab, Page } from "./Rail";
import { parseRangeQuery, rangeQuery, type AuditRange } from "./auditTime";

/**
 * What the fragment names: a destination, the Device tab inside it, or the tool whose sheet is open
 * on the Tools tab. A run link (`#run/...`) is not a surface and is read by run-link.ts.
 */
export type Surface = {
  page: Page;
  tab?: DeviceTab;
  /** The tool whose sheet is open, on the device workspace's Tools tab. */
  tool?: string;
  /** Screen's second view. A fragment only names it when it is Browser; Desktop is the default. */
  view?: "desktop" | "browser";
  /**
   * The Table's time window, `#activity?from=&to=`. Only Activity can carry it: the range is the
   * Activity Table's request, and a link to one is a link to that view of the record.
   */
  range?: AuditRange;
};

const PAGES: readonly Page[] = ["home", "activity", "device"];
const TABS: readonly DeviceTab[] = ["overview", "terminal", "screen", "files", "runs", "tools", "machine"];

/**
 * The fragments the previous shell shipped. A bookmark, a runbook link or a browser history entry
 * must keep working: the surface moved from seven rail items into one Device workspace, and those
 * old names are aliases, not a second navigation.
 *
 * `capabilities` and `plugins` are handled apart because they were rail destinations that now live
 * inside the machine's Tools tab: they can only name that tab when a machine is selected, and a
 * destination without a machine is Home's fleet. `parseSurfaceHash` therefore takes the selection as
 * an option instead of inventing a device.
 */
const LEGACY: Record<string, Surface> = {
  devices: { page: "home" },
  terminal: { page: "device", tab: "terminal" },
  screen: { page: "device", tab: "screen" },
  browser: { page: "device", tab: "screen", view: "browser" },
  files: { page: "device", tab: "files" },
  runs: { page: "device", tab: "runs" },
  system: { page: "device", tab: "machine" },
  audit: { page: "activity" }
};

/** The fragment for a surface: `#home`, `#activity[?from=&to=]`, `#device/<tab>[/<tool>]`. */
export function surfaceHash(surface: Surface): string {
  if (surface.page === "activity" && surface.range) {
    return "#activity?" + rangeQuery(surface.range);
  }
  if (surface.page === "device") {
    const tab = surface.tab ?? "terminal";
    if (tab === "tools" && surface.tool) {
      return "#device/tools/" + encodeURIComponent(surface.tool);
    }
    if (tab === "screen" && surface.view === "browser") return "#device/screen/browser";
    return "#device/" + tab;
  }
  return "#" + surface.page;
}

/**
 * Read a surface fragment; anything that is not a destination, tab or tool is not a link.
 *
 * The retired `#capabilities` and `#plugins` destinations land on the machine's Tools tab when
 * `machineSelected` says there is a machine to put it under; without one they land on Home, where
 * the fleet (and the machine picker) is.
 */
export function parseSurfaceHash(
  hash: string,
  { machineSelected = false }: { machineSelected?: boolean } = {}
): Surface | null {
  // A range is Activity's own address: `#activity?from=<ms>&to=<ms>`. It is read before the bare
  // destination regex because the query is not a path segment.
  if (hash.startsWith("#activity?")) {
    const range = parseRangeQuery(hash.slice("#activity?".length));
    return range ? { page: "activity", range } : null;
  }
  const match = /^#([a-z]+)(?:\/(.+))?$/.exec(hash);
  if (!match) return null;
  const name = match[1];
  const rest = match[2];

  if (name === "capabilities" || name === "plugins") {
    if (!machineSelected) return { page: "home" };
    // `#plugins` has no sub-link: a plugin is a row in the list, not an address of its own.
    if (name === "plugins") {
      if (rest !== undefined) return null;
      return { page: "device", tab: "tools" };
    }
    if (rest === undefined) return { page: "device", tab: "tools" };
    try {
      const tool = decodeURIComponent(rest);
      return tool ? { page: "device", tab: "tools", tool } : null;
    } catch {
      return null;
    }
  }

  if (rest !== undefined) {
    if (name === "device") {
      // `#device/browser` is the short link the runbook uses; the canonical form is
      // `#device/screen/browser`, which keeps the tab first.
      if (rest === "browser") return { page: "device", tab: "screen", view: "browser" };
      if (rest === "screen/browser") return { page: "device", tab: "screen", view: "browser" };
      if (rest === "tools") return { page: "device", tab: "tools" };
      if (rest.startsWith("tools/")) {
        try {
          const tool = decodeURIComponent(rest.slice("tools/".length));
          return tool ? { page: "device", tab: "tools", tool } : null;
        } catch {
          return null;
        }
      }
      if ((TABS as readonly string[]).includes(rest)) {
        return { page: "device", tab: rest as DeviceTab };
      }
    }
    return null;
  }

  if ((PAGES as readonly string[]).includes(name)) return { page: name as Page };
  return LEGACY[name] ?? null;
}
