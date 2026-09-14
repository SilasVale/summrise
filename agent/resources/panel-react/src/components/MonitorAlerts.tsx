// MonitorAlerts — the device SPEAKING, rather than waiting to be looked at.
//
// Everything else about the reachability monitors is pull: a card in Settings and a chip on the
// strip, both of which an operator has to be looking at. A host you asked the device to watch is
// the one case where the answer is only useful NOW — "192.168.1.1:22 went down at 18:41" is a
// fact that decays, and by the time somebody opens Settings the interesting part is over.
//
// WHAT IT SAYS. The state, the host, and the duration of what just ENDED: a recovery names the
// OUTAGE ("back up after 2m 15s"), a fall names the uptime that ended. Both come from the
// device's own transition record, so the banner and the card's log cannot disagree.
//
// IT DOES NOT STEAL THE SCREEN: at most three, each expiring on its own (see `useMonitorAlerts`),
// `role="status"` rather than `role="alert"` so a screen reader is not interrupted mid-sentence,
// and it sits above the workspace instead of over it — a console session underneath keeps its
// keystrokes.
import { fmtSince, type MonitorAlert } from "../hooks/useMonitors";
import { shouldNotify } from "../lib/attention";

export function MonitorAlerts({ alerts }: { alerts: MonitorAlert[] }) {
  // THE VISIBLE-TAB CHANNEL (see `shouldNotify`): while the tab is hidden the desktop
  // notification carries the same event, and this renders nothing — nobody is here to read it, and
  // the chip still holds the state for when they come back.
  if (typeof document !== "undefined" && !shouldNotify(document.visibilityState)) return null;
  if (alerts.length === 0) return null;
  return (
    <div className="monitor-alerts" role="status" aria-live="polite">
      {alerts.map((a) => (
        <div key={a.key} className={`monitor-alert ${a.up ? "is-up" : "is-down"}`}>
          <span className={`monitor-mark ${a.up ? "is-up" : ""}`} aria-hidden="true" />
          <span className="monitor-alert-text">
            <strong>
              {a.host}:{a.port}
            </strong>
            {a.up ? (
              <>
                {" "}
                is back up after {fmtSince(a.lastedMs)} down
                {a.status !== null ? ` (HTTP ${a.status})` : ""}
              </>
            ) : (
              <>
                {" "}
                is DOWN — it had been up {fmtSince(a.lastedMs)}
                {/* "is DOWN" and "is DOWN, answering 500" are different sentences, and the
                    second is the one that tells an operator where to look. */}
                {a.status !== null ? ` (HTTP ${a.status})` : ""}
              </>
            )}
          </span>
        </div>
      ))}
    </div>
  );
}
