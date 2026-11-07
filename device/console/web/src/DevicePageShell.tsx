import type { Device } from "./api";
import { lastHeardMs } from "./format";
import { DEVICE_TABS, DEVICE_TAB_LABELS, type DeviceTab } from "./Rail";
import { DevicePicker } from "./DevicePicker";
import { OfflineNotice } from "./OfflineNotice";
import type { Grant } from "./terminal";
import { processStartMs, startedStory } from "./updateStory";

/**
 * Device is ONE destination with five tabs, not five rail items. The shell owns the destination's one
 * `h1` (the tab's name) and the tab bar in the same head; the machine identity lives in the header
 * control above, so no page inside states it again. The fragment names the tab, which is why a reload
 * and an old `#files` bookmark both land on the same view.
 *
 * WITH NO MACHINE THE PAGE IS THE PICKER. The tabs describe surfaces that cannot run until a machine
 * is chosen, so drawing them made the page claim to be `Terminal` while its real question — which
 * machine? — sat below the strip (audit-40 F7). The `h1` names the question and the strip and Connect
 * wait for an answer.
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
  // The device's own account of its last start, beside — never instead of — the console's account
  // of its silence. It is shown online too: a machine that woke is exactly the one whose start the
  // operator needs to place without a shell on it. The start keeps being drawn when its version
  // repeats the header's chip: the chip states what runs, and only the journal says when it began.
  const started = chosen
    ? startedStory(chosen.update, {
        startedAtMs: processStartMs(chosen),
        shownVersion: chosen.version
      })
    : null;
  return (
    <div className="page device-page">
      <header className="page-head device-head">
        <h1>{chosen ? DEVICE_TAB_LABELS[tab] : "Choose a machine"}</h1>
        {chosen && (
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
        )}
        {/* The machine's one local door. The sheet is the same one Home and the account menu open;
            below 640 px this same button stays in the head's first row, with the short visible
            label and the sheet's full name for assistive technology. With no machine there is no
            client to connect, so the door waits for the choice. */}
        {chosen && (
          <div className="page-actions device-connect">
            <button
              type="button"
              className="mini"
              aria-label="Connect a client"
              onClick={onConnect}
            >
              Connect
            </button>
          </div>
        )}
      </header>

      {/* THE MACHINE'S STATE BELONGS TO EVERY TAB, not only the one that was written first.
          Terminal lives outside this shell (it stays mounted across destinations), so it carries its
          own notice; rendering this one there too would stack two copies of the same sentence. */}
      {chosen && !chosen.online && tab !== "terminal" && (
        <OfflineNotice
          lastSeenMs={lastHeardMs(chosen)}
          detail="Nothing on this page can reach it until it reconnects; the console keeps trying on its own."
        />
      )}

      {/* The device's sentence, not the console's: "started 0.3.275 · 2h ago (update to 0.3.276
          refused)". It sits beside the silence notice because the two answer different questions —
          was it heard, and what did it start — and neither is used to explain the other. Its title,
          when it is a start, is the exact instant the device reported. */}
      {started && (
        <p className="started-line" title={started.title ?? undefined}>
          {started.text}
        </p>
      )}

      {chosen && grant ? (
        children(grant)
      ) : (
        <section className="empty-state">
          <DevicePicker devices={devices} onChoose={onChoose} />
        </section>
      )}
    </div>
  );
}

function grantFor(chosen: Device | null): Grant | null {
  return chosen ? { deviceId: chosen.id, envelope: null } : null;
}
