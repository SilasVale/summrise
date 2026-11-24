import type { ReactNode } from "react";
import logoUrl from "./logo.svg";

export type Page = "home" | "machines" | "activity" | "access" | "device";

/** The fragment names the machine's section: `#device/terminal`. */
export type DeviceTab = "overview" | "terminal" | "screen" | "files" | "runs" | "tools" | "memory" | "machine";

export const PAGE_LABELS: Record<Page, string> = {
  home: "Overview",
  machines: "Machines",
  activity: "Activity",
  access: "Access",
  device: "Device"
};

export const DEVICE_TAB_LABELS: Record<DeviceTab, string> = {
  overview: "Overview", terminal: "Terminal", screen: "Screen", files: "Files",
  runs: "Runs", tools: "Tools", memory: "Memory", machine: "Machine"
};

export const DEVICE_TABS: readonly DeviceTab[] = ["overview", "terminal", "screen", "files", "runs", "tools", "memory", "machine"];

const GLOBAL_PAGES: readonly Page[] = ["home", "machines", "activity", "access"];
const ICONS: Record<Page, string> = {
  home: "M4 10.5 12 4l8 6.5V20H4v-9.5ZM9.5 20v-5.5h5V20",
  machines: "M3 4.5h13v10H3zM7 19h5M9.5 14.5V19M19 8h2v12h-6v-2",
  activity: "M3 12h4l2.5-6 4 12 2.5-6h5",
  access: "M9 10V7a3 3 0 0 1 6 0v3M6 10h12v10H6zM12 14v2",
  device: "M3 4.5h18v12H3zM9 20.5h6M12 16.5v4"
};

/** Global destinations and the current machine are separate navigation groups. */
export function Rail({ page, onPage, machineName, footer }: {
  page: Page;
  onPage: (page: Page) => void;
  machineName?: string;
  footer?: ReactNode;
}) {
  const destination = (entry: Page) => (
    <button key={entry} type="button" className="rail-btn" aria-current={entry === page ? "page" : undefined}
      title={entry === "device" && machineName ? machineName : PAGE_LABELS[entry]} onClick={() => onPage(entry)}>
      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d={ICONS[entry]} />
      </svg>
      <span className="rail-label">{PAGE_LABELS[entry]}</span>
    </button>
  );
  return (
    <nav className="rail" aria-label="Destinations">
      <button type="button" className="brand workspace-brand" aria-label="Summrise home" onClick={() => onPage("home")}>
        <img src={logoUrl} alt="" width={28} height={28} />
        <span className="brand-name">Summrise</span>
      </button>
      <div className="rail-items">
        <p className="rail-group-label">Workspace</p>
        {GLOBAL_PAGES.map(destination)}
      </div>
      <div className="rail-context">
        <p className="rail-group-label">{machineName ? "Selected machine" : "Machine workspace"}</p>
        {destination("device")}
        {machineName && <p className="rail-machine-name" title={machineName}>{machineName}</p>}
      </div>
      {footer && <div className="rail-foot">{footer}</div>}
    </nav>
  );
}
