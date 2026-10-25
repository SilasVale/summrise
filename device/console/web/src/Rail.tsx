// The rail is the legacy panel's grammar, kept: destinations on the left, a connection dot at the
// foot. Icons are inline SVG paths from the same 24×24 grid the old Icon set used.
export type Page =
  | "devices"
  | "files"
  | "terminal"
  | "browser"
  | "memory"
  | "monitors"
  | "runs"
  | "audit";

const ICONS: Record<Page, string> = {
  devices:
    "M4 5.5h16M4 12h16M4 18.5h10M3 3.5h18a1 1 0 0 1 1 1v15a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1v-15a1 1 0 0 1 1-1Z",
  files: "M3 5.5h6l2 2h10v11H3z",
  terminal: "M6 8l3.5 4L6 16M12 16h6M3 4.5h18v15H3z",
  browser: "M3 5.5h18v13H3zM3 9.5h18M6.5 7.5h.01M9 7.5h.01",
  memory:
    "M4 6c0-1.7 3.6-3 8-3s8 1.3 8 3-3.6 3-8 3-8-1.3-8-3M4 6v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3",
  monitors: "M3 12h3.5l2.5-6 4 12 2.5-6H21",
  runs: "M8 5v14l11-7z",
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
      {(["devices", "files", "terminal", "browser", "memory", "monitors", "runs", "audit"] as Page[]).map((entry) => (
        <button
          key={entry}
          type="button"
          className={"rail-btn" + (entry === page ? " active" : "")}
          aria-current={entry === page ? "page" : undefined}
          title={entry}
          onClick={() => onPage(entry)}
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
            <path d={ICONS[entry]} />
          </svg>
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
