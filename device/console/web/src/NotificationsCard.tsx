import { useState } from "react";
import {
  enabled,
  permission,
  requestPermission,
  setEnabled,
  type Permission
} from "./notifications";

/**
 * The Monitors page's notification preference.
 *
 * It owns no polling and no `Notification`: the chip announces, this card asks. The browser prompt
 * happens only in the button's click handler — never on mount — and a state the browser will not
 * deliver is a plain sentence with no button that could promise otherwise. The preference is
 * per-browser and lives in `localStorage`; a page that refuses to store it still works for the
 * visit.
 */
export function NotificationsCard() {
  const [state, setState] = useState<Permission>(() => permission());
  const [on, setOn] = useState<boolean>(() => enabled());
  const [asking, setAsking] = useState(false);

  const turnOn = async () => {
    setAsking(true);
    try {
      // Permission already granted needs no prompt; only the unasked state raises the browser's.
      const answer = state === "granted" ? state : await requestPermission();
      setState(answer);
      if (answer === "granted") {
        setEnabled(true);
        setOn(true);
      } else {
        setOn(false);
      }
    } finally {
      setAsking(false);
    }
  };

  const turnOff = () => {
    setEnabled(false);
    setOn(false);
  };

  const ready = state === "default" || state === "granted";

  return (
    <section className="card" role="region" aria-label="Monitor notifications">
      <header>
        <strong>Monitor notifications</strong>
      </header>

      {state === "unsupported" && (
        <p className="hint">
          This browser does not support notifications. The down chip still shows the count.
        </p>
      )}

      {state === "denied" && (
        <p className="hint">
          Notifications are blocked for this site. Allow them in the browser&apos;s site settings, then
          reload this page.
        </p>
      )}

      {ready && on && (
        <>
          <p className="hint">
            Notifications are on. A new outage shows a browser notification; clicking it opens
            Monitors.
          </p>
          <footer>
            <button type="button" onClick={turnOff}>
              Turn off
            </button>
          </footer>
        </>
      )}

      {ready && !on && (
        <>
          <p className="hint">
            Get a browser notification when a new monitor goes down. The permission is asked only when
            you press the button.
          </p>
          <footer>
            <button
              type="button"
              className="primary"
              disabled={asking}
              onClick={() => void turnOn()}
            >
              {asking ? "Asking…" : "Enable notifications"}
            </button>
          </footer>
        </>
      )}
    </section>
  );
}
