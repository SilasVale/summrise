import { DEVICE_TABS, type DeviceTab, type Page } from "./Rail";
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
  /**
   * A one-shot request from the desktop shell's tray: open the Connect dialog with this kind. It is
   * read from the fragment and consumed into page state; [`surfaceHash`] never writes it back, so
   * the address stays the destination and a second click is a new fragment.
   */
  newSession?: { kind: "local" | "ssh" | "serial" };
  /**
   * A one-shot request from the tray: prefill the Browser address with this URL. View only — the
   * page opens nothing; the operator still presses Open new tab. Never written back, like
   * [`Surface.newSession`].
   */
  openBrowser?: { url: string };
};

const PAGES: readonly Page[] = ["home", "activity", "device"];
// **THE LIST IS THE TYPE'S LIST (the hundred-and-seventy-first list).** This was a second copy of `DeviceTab`, so a tab added to
// `Rail.tsx` was routable nowhere: `#device/memory` fell through to the overview and the page it names looked broken. The rail's
// own list is the authority now — one list, one route table.
const TABS: readonly DeviceTab[] = DEVICE_TABS;

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
  // **A REQUEST IS NOT AN ADDRESS.** `newSession`/`openBrowser` are one-shot asks read by
  // `parseSurfaceHash`; writing one back would make a repeat click a no-op and leave a one-shot in
  // the bar. The destination is the whole fragment.
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
 * Two forms also name a one-shot request for a page that already exists: the tray's
 * `#device/terminal/new=local|ssh|serial` and `#device/browser/open=<percent-encoded url>`. They are
 * view only — the caller turns them into dialog/address state and performs no device call for the
 * link itself — and [`surfaceHash`] never emits them.
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
      // **THE TRAY'S TWO LINKS ARE READ BEFORE THE TAB CHECKS, AND THEY ARE ONE-SHOT.** The shell
      // cannot address the console's tabs or its dialog, so it names a request: a terminal kind to
      // prefill, or a URL to put in the Browser address. Each is consumed into page state and the
      // address writer canonicalises it away (`#device/terminal`, `#device/screen/browser`); a
      // repeated click therefore needs a new fragment. The kind is the LINK's vocabulary (`local`),
      // not the dialog's (`pty`): the dialog maps it, and nobody else does.
      const session = /^terminal\/new=(local|ssh|serial)$/.exec(rest);
      if (session) {
        return {
          page: "device",
          tab: "terminal",
          newSession: { kind: session[1] as "local" | "ssh" | "serial" }
        };
      }
      const browser = /^browser\/open=(.+)$/.exec(rest);
      if (browser) {
        let url: string;
        try {
          // Exactly one decode: `%23`, `%26` and a space in the URL are the URL's own characters.
          url = decodeURIComponent(browser[1]);
        } catch {
          // `%E0%A4%A` is not an address; a broken escape is not a link.
          return null;
        }
        // **A FRAGMENT MAY NAME ANY HTTP PAGE, AND NOTHING ELSE.** The WebView must never be handed
        // `javascript:` or `file:` from a link, and a double-encoded scheme is not http(s) after the
        // one decode the contract allows.
        if (url === "about:blank" || /^https?:\/\//i.test(url)) {
          return { page: "device", tab: "screen", view: "browser", openBrowser: { url } };
        }
        return null;
      }
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
