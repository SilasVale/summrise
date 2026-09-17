// DESKTOP NOTIFICATIONS — the one channel that leaves the browser, and the one that can be refused.
//
// WHAT MAKES THIS WORTH ITS OWN MODULE is not `new Notification(...)`; it is everything around it:
//
//   * **PERMISSION IS A STATE, NOT A BOOLEAN.** The browser has four answers — unsupported,
//     `default` (never asked), `granted`, `denied` — and `denied` is PERMANENT for the origin: the
//     page cannot re-ask, so the only honest thing to do is SAY SO and point at the channel that
//     still works (the tab title). A toggle that silently does nothing is how a feature gets
//     distrusted.
//   * **THE REQUEST NEEDS A GESTURE.** `Notification.requestPermission()` outside a user action is
//     ignored by every modern browser, so this is called from the toggle, never from an effect.
//   * **DEDUPE, because the same fact arrives repeatedly.** A watched host that is down is down on
//     every poll; the notification for it must fire ONCE. Keys are the caller's (see
//     `attentionFrom`), and a key is retired when the condition clears, so a link that goes down,
//     recovers and goes down again DOES notify twice — which is the point.
//   * **A RATE LIMIT, because a flapping link is a notification cannon.** At most one every
//     `MIN_GAP_MS`, and at most `BURST` in a minute; the ones that are dropped are counted and
//     reported rather than silently lost.
//   * **`tag`, so the OS replaces rather than stacks.** One notification per subject, updated.
//
// The whole thing takes an injectable constructor so it can be tested without a browser: the rules
// above are the feature, and none of them need a real OS notification to verify.

export type NotifyPermission = "unsupported" | "default" | "granted" | "denied";

interface NotifyPayload {
  key: string;
  title: string;
  body: string;
}

/** What a `Notification` needs to look like for this module — a structural type, so a test double
 *  is a plain object and no DOM lib is required to exercise the rules. */
export interface NotificationLike {
  close?: () => void;
}
export type NotificationCtor = new (title: string, options?: { body?: string; tag?: string }) => NotificationLike;

const MIN_GAP_MS = 5_000;
const BURST = 3;
const BURST_WINDOW_MS = 60_000;

/** The browser's answer, read defensively: a page in an insecure context has no `Notification` at
 *  all, and that is a STATE, not an error. */
export function readPermission(ctor?: NotificationCtor | null, permission?: string): NotifyPermission {
  if (!ctor) return "unsupported";
  const p = permission ?? (typeof Notification !== "undefined" ? Notification.permission : "default");
  return p === "granted" || p === "denied" ? p : "default";
}

/** What the operator should be told about the current state, in the panel's own voice — the
 *  settings card renders this verbatim, and it is the ONLY place the difference between "you never
 *  turned it on", "the browser said no" and "this browser cannot" is explained. */
export function permissionHint(state: NotifyPermission): string {
  switch (state) {
    case "granted":
      return "Desktop notifications are on. A watched host going down reaches you even when this window is in the background.";
    case "denied":
      return "This browser is blocking notifications for this page, and it will not ask again — allow them in the site settings (the padlock in the address bar), or rely on the tab title, which always works.";
    case "unsupported":
      return "This browser (or this page's context) cannot show desktop notifications, so the tab title carries the count instead — it always works.";
    default:
      return "Notifications are off. Turning them on asks your browser once; if it says no, the tab title still carries the count.";
  }
}

/** The notifier: dedupe + rate limit + tag. Constructed with the live ctor and a clock, so both the
 *  panel and a test drive the same object. */
export class DeviceNotifier {
  private seen = new Set<string>();
  private lastAt = 0;
  private windowStart = 0;
  private sentInWindow = 0;
  /** Notifications the rate limit refused, for the settings card to be honest about. */
  suppressed = 0;

  constructor(
    private ctor: NotificationCtor | null,
    private now: () => number = () => Date.now(),
  ) {}

  /** Forget a key whose condition has cleared, so it can fire again later. */
  clear(key: string) {
    this.seen.delete(key);
  }

  /** Keep only these keys — called with the live attention keys on every render, which is what
   *  makes "down, up, down again" notify twice without the caller tracking history. */
  retain(keys: Iterable<string>) {
    const keep = new Set(keys);
    for (const k of [...this.seen]) if (!keep.has(k)) this.seen.delete(k);
  }

  /** Send one, if the rules allow. Returns whether an OS notification was actually requested. */
  notify(payload: NotifyPayload, permission: NotifyPermission): boolean {
    if (permission !== "granted" || !this.ctor) return false;
    if (this.seen.has(payload.key)) return false;
    const t = this.now();
    if (t - this.lastAt < MIN_GAP_MS) {
      this.suppressed += 1;
      return false;
    }
    if (t - this.windowStart > BURST_WINDOW_MS) {
      this.windowStart = t;
      this.sentInWindow = 0;
    }
    if (this.sentInWindow >= BURST) {
      this.suppressed += 1;
      return false;
    }
    try {
      new this.ctor(payload.title, { body: payload.body, tag: payload.key });
    } catch {
      // A browser that refuses to construct one (permission revoked between the check and the
      // call) must not take the panel down with it.
      return false;
    }
    this.seen.add(payload.key);
    this.lastAt = t;
    this.sentInWindow += 1;
    return true;
  }
}
