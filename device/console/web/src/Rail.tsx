// The rail is the legacy panel's grammar, kept: destinations on the left, a connection dot at the
// foot. Icons are inline SVG paths from the same 24×24 grid the old Icon set used.
//
// **THE FIRST 断舍离: EIGHT DESTINATIONS, NOT TWELVE.** Memory, Monitors, Events and Security were
// separate pages for capabilities the AI already reaches through MCP. The operator's job is the
// machine, the terminal, the screen, the files, the runs and the record; those four pages never
// earned a permanent seat. Their components and APIs stay for the later fold (Memory → Files,
// Events → Activity), but they are no longer navigation.
export type Page =
  | "devices"
  | "terminal"
  | "screen"
  | "files"
  | "runs"
  | "system"
  | "audit";

/// What each page is called in the rail and in the top bar. The rail's buttons are icon-first, so the
/// label is the difference between "a column of glyphs" and a navigable set of sections.
export const PAGE_LABELS: Record<Page, string> = {
  devices: "Devices",
  terminal: "Terminal",
  screen: "Screen",
  files: "Files",
  runs: "Runs",
  system: "Machine",
  audit: "Activity"
};

/// The rail's order is this map's insertion order; `Record<Page, string>` keeps the map complete, so
/// there is no second list to update when a section is added.
const SECTIONS = Object.keys(PAGE_LABELS) as Page[];

const ICONS: Record<Page, string> = {
  devices:
    "M4 5.5h16M4 12h16M4 18.5h10M3 3.5h18a1 1 0 0 1 1 1v15a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1v-15a1 1 0 0 1 1-1Z",
  files: "M3 5.5h6l2 2h10v11H3z",
  terminal: "M6 8l3.5 4L6 16M12 16h6M3 4.5h18v15H3z",
  screen: "M3 4.5h18v12H3zM9 20.5h6M12 16.5v4",
  runs: "M8 5v14l11-7z",
  system: "M8 8h8v8H8zM4 9h4M4 15h4M16 9h4M16 15h4M9 4v4M15 4v4M9 16v4M15 16v4",
  audit: "M5 4.5h14v15H5zM8 8.5h8M8 12h8M8 15.5h5"
};

export function Rail({
  page,
  onPage,
  devices,
  online
}: {
  page: Page;
  onPage: (page: Page) => void;
  devices: number;
  online: number;
}) {
  return (
    <nav className="rail" aria-label="Sections">
      {SECTIONS.map((entry) => (
        <button
          key={entry}
          type="button"
          className={"rail-btn" + (entry === page ? " active" : "")}
          aria-current={entry === page ? "page" : undefined}
          title={PAGE_LABELS[entry]}
          aria-label={PAGE_LABELS[entry]}
          onClick={() => onPage(entry)}
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
            <path d={ICONS[entry]} />
          </svg>
          <span className="rail-label">{PAGE_LABELS[entry]}</span>
        </button>
      ))}
      <span className="rail-spacer" />
      <span
        className={"dot rail-dot " + (online > 0 ? "dot-ok" : "dot-off")}
        title={`${online} of ${devices} online`}
      />
    </nav>
  );
}
