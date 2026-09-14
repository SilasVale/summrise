// NotificationsCard — the switch, the browser's answer, and what still works when the answer is no.
//
// WHY THE STATE IS SPELLED OUT: `Notification.permission` has four values and only one of them is
// "on". A toggle that says "enabled" while the browser is refusing is the kind of control that
// teaches an operator not to trust the panel — so this card renders the browser's actual answer,
// explains what it means, and names the channel that needs no permission at all (the tab title).
//
// THE TEST BUTTON IS NOT DECORATION: a notification permission can be granted and the OS can still
// be in Do Not Disturb, and the only way to find that out before you rely on it is to send one. It
// is also the user gesture the browser requires, so the same click can be what asks for permission.
import { useState } from "react";
import { permissionHint, type NotifyPermission } from "../lib/notify";
import { attentionSummary, type AttentionItem } from "../lib/attention";

export function NotificationsCard({
  permission,
  onRequest,
  onTest,
  attention,
}: {
  permission: NotifyPermission;
  onRequest: () => Promise<NotifyPermission>;
  /** Send one notification now, whatever the current permission (it will ask first if needed). */
  onTest: () => void;
  attention: AttentionItem[];
}) {
  const [busy, setBusy] = useState(false);
  const on = permission === "granted";
  const blocked = permission === "denied" || permission === "unsupported";

  return (
    <div className="settings-section">
      <h3>Getting your attention</h3>
      <p className="muted">
        This page keeps a count in its <strong>tab title</strong> and a badge on its icon whenever
        something needs you — no permission required, and it works in a background tab. Desktop
        notifications go further and reach you in another application.
      </p>
      <div className="notify-row">
        <button
          type="button"
          className={`btn ${on ? "" : "btn-ghost"}`}
          aria-pressed={on}
          disabled={busy || blocked}
          onClick={async () => {
            setBusy(true);
            if (permission === "default") await onRequest();
            setBusy(false);
          }}
        >
          {on ? "Notifications on" : blocked ? "Notifications unavailable" : "Turn on notifications"}
        </button>
        <button type="button" className="btn btn-ghost" onClick={onTest}>
          Send a test
        </button>
        <span className={`notify-state is-${permission}`} data-permission={permission}>
          {permission}
        </span>
      </div>
      <p className="muted notify-hint">{permissionHint(permission)}</p>
      <p className="muted notify-now">{attentionSummary(attention)}</p>
    </div>
  );
}
