// THREE DESTINATIONS, NOT SEVEN. The hub is the product, and the console is its window: Home says
// what the hub is, Activity what was called, and Device is the one workspace where the operator puts
// their hands on the machine — its terminal, screen, files, runs, tools and machine facts. Fleet is
// Home's zero state and Connect is a sheet, so neither is a place in this rail.
//
// A destination earns its seat with a daily job. Adding one means deleting one in the same change.
export type Page = "home" | "activity" | "device";

/** The one workspace's tabs. The fragment names the tab: `#device/terminal`. */
export type DeviceTab =
  | "overview"
  | "terminal"
  | "screen"
  | "files"
  | "runs"
  | "tools"
  | "memory"
  | "machine";

/// What each destination is called in the rail, the document title and each page's one `h1`.
export const PAGE_LABELS: Record<Page, string> = {
  home: "Home",
  activity: "Activity",
  device: "Device"
};

export const DEVICE_TAB_LABELS: Record<DeviceTab, string> = {
  overview: "Overview",
  terminal: "Terminal",
  screen: "Screen",
  files: "Files",
  runs: "Runs",
  tools: "Tools",
  memory: "Memory",
  machine: "Machine"
};

/// The tabs in the order the workspace offers them. Overview is first because it is the
/// machine at a glance; Terminal follows because it is the most direct way to take the keyboard
/// back; Tools follows Runs because it answers what the machine can do, and Machine is the last
/// word on what it is running.
export const DEVICE_TABS: readonly DeviceTab[] = [
  "overview",
  "terminal",
  "screen",
  "files",
  "runs",
  "tools",
  "memory",
  "machine"
];

/// The rail's order is this map's insertion order; `Record<Page, string>` keeps the map complete, so
/// there is no second list to update when a destination changes.
const SECTIONS = Object.keys(PAGE_LABELS) as Page[];

const ICONS: Record<Page, string> = {
  // a house
  home: "M4 10.5 12 4l8 6.5V20H4v-9.5ZM9.5 20v-5.5h5V20",
  // a pulse: the record of calls
  activity: "M3 12h4l2.5-6 4 12 2.5-6h5",
  // a monitor: the machine workspace
  device: "M3 4.5h18v12H3zM9 20.5h6M12 16.5v4"
};

type RailProps = {
  page: Page;
  onPage: (page: Page) => void;
  /// The account menu at the rail's foot on desktop and tablet. On a phone it lives in the header.
  footer?: React.ReactNode;
};

/**
 * The one navigation: 216 px labelled sidebar at ≥1024, 56 px icon rail at 640–1023, bottom tab bar
 * below 640. The labels are always in the DOM (and are the buttons' accessible names); CSS decides
 * whether a given width draws them. An icon-only column is a memory test, so only the tablet band
 * hides the text.
 */
export function Rail({ page, onPage, footer }: RailProps) {
  return (
    <nav className="rail" aria-label="Destinations">
      <div className="rail-items">
        {SECTIONS.map((entry) => (
          <button
            key={entry}
            type="button"
            className="rail-btn"
            aria-current={entry === page ? "page" : undefined}
            title={PAGE_LABELS[entry]}
            onClick={() => onPage(entry)}
          >
            <svg
              width="20"
              height="20"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.6"
              strokeLinecap="round"
              strokeLinejoin="round"
              aria-hidden="true"
            >
              <path d={ICONS[entry]} />
            </svg>
            <span className="rail-label">{PAGE_LABELS[entry]}</span>
          </button>
        ))}
      </div>
      {footer && <div className="rail-foot">{footer}</div>}
    </nav>
  );
}
