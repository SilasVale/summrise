import type { Device } from "./api";
import { DEVICE_TABS, DEVICE_TAB_LABELS, type DeviceTab } from "./Rail";
import { DevicePicker } from "./DevicePicker";
import { OfflineNotice } from "./OfflineNotice";
import type { Grant } from "./terminal";

/**
 * Device is ONE destination with five tabs, not five rail items. The shell owns the destination's one
 * `h1` (the tab's name) and the tab bar in the same head; the machine identity lives in the header
 * control above, so no page inside states it again. The fragment names the tab, which is why a reload
 * and an old `#files` bookmark both land on the same view.
 */
export function DevicePageShell({
  tab,
  onTab,
  devices,
  chosen,
  onChoose,
  onConnect,
  children
}: {
  tab: DeviceTab;
  onTab: (tab: DeviceTab) => void;
  devices: Device[];
  chosen: Device | null;
  onChoose: (id: string) => void;
  onConnect: () => void;
  children: (grant: Grant) => React.ReactNode;
}) {
  const grant = grantFor(chosen);
  return (
    <div className="page device-page">
      <header className="page-head device-head">
        <h1>{DEVICE_TAB_LABELS[tab]}</h1>
        <nav className="device-tabs" aria-label="Device views">
          {DEVICE_TABS.map((entry) => (
            <button
              key={entry}
              type="button"
              className="device-tab"
              aria-current={entry === tab ? "page" : undefined}
              onClick={() => onTab(entry)}
            >
              {DEVICE_TAB_LABELS[entry]}
            </button>
          ))}
        </nav>
        <div className="page-actions device-connect">
          <button type="button" className="mini" onClick={onConnect}>
            Connect
          </button>
        </div>
      </header>

      {/* THE MACHINE'S STATE BELONGS TO EVERY TAB, not only the one that was written first.
          Terminal lives outside this shell (it stays mounted across destinations), so it carries its
          own notice; rendering this one there too would stack two copies of the same sentence. */}
      {chosen && !chosen.online && tab !== "terminal" && (
        <OfflineNotice
          lastSeenMs={chosen.last_frame_ms}
          detail="Nothing on this page can reach it until it reconnects; the console keeps trying on its own."
        />
      )}

      {chosen && grant ? (
        children(grant)
      ) : (
        <section className="empty-state">
          <h2>Choose a machine</h2>
          <DevicePicker devices={devices} onChoose={onChoose} />
        </section>
      )}
    </div>
  );
}

function grantFor(chosen: Device | null): Grant | null {
  return chosen ? { deviceId: chosen.id, envelope: null } : null;
}
