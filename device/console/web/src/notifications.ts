/**
 * Monitor notifications: the browser's permission, the operator's preference, and the one place a
 * `Notification` is constructed.
 *
 * The chip polls `monitor.list` for the console; this module is only the announcement. The
 * preference is the operator's, not the device's: it lives in `localStorage` under one key, is off by
 * default, and is only ever turned on from the Monitors page's button. Permission is asked there
 * too — never on import, mount or a timer — because a prompt without a gesture is a dark pattern.
 *
 * Every browser API is guarded: no `Notification` (jsdom, a locked-down browser), a storage property
 * that throws, a refused prompt. Each is a quiet state, not an exception the page would have to
 * catch. `notifyMonitors` is the only constructor, so a notification's title, body and click are one
 * grammar.
 */
export type Permission = "unsupported" | "default" | "granted" | "denied";

/// The preference's one key. A stored "on" without granted permission is still off.
const STORAGE_KEY = "summrise.notify.monitors";

/// The browser's constructor, or null when this browser has none (including a property that throws).
function browserNotification(): typeof Notification | null {
  if (typeof window === "undefined") return null;
  try {
    return window.Notification ?? null;
  } catch {
    return null;
  }
}

/// The storage, or null when the browser refuses the property itself (site data blocked).
function browserStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/**
 * What the browser says, as one of four states. A value the spec does not define reads as
 * `"default"`: the page may still ask, and the browser will answer.
 */
export function permission(): Permission {
  const api = browserNotification();
  if (!api) return "unsupported";
  if (api.permission === "granted") return "granted";
  if (api.permission === "denied") return "denied";
  return "default";
}

/**
 * Ask the browser. The card's button is the only caller, so this is the click gesture.
 *
 * Never rejects: unsupported resolves `"unsupported"`, a refused prompt resolves the browser's
 * current answer. The result is the browser's, not a guess about what the operator chose.
 */
export function requestPermission(): Promise<Permission> {
  const api = browserNotification();
  if (!api) return Promise.resolve("unsupported");
  try {
    return Promise.resolve(api.requestPermission()).then(
      (answer) => (answer === "granted" || answer === "denied" ? answer : permission()),
      () => permission()
    );
  } catch {
    return Promise.resolve(permission());
  }
}

/**
 * Whether this page should announce an outage: the operator turned it on, and the browser granted
 * permission. Permission alone is not consent; the preference alone cannot deliver.
 *
 * A write the browser refused is remembered for this visit (below), so the session the operator just
 * turned on works even when it cannot be persisted.
 */
let sessionPreference: boolean | null = null;

export function enabled(): boolean {
  if (permission() !== "granted") return false;
  if (sessionPreference !== null) return sessionPreference;
  const storage = browserStorage();
  if (!storage) return false;
  try {
    return storage.getItem(STORAGE_KEY) === "on";
  } catch {
    return false;
  }
}

/**
 * Remember the operator's choice. A read-only or full storage is not an error: the choice applies to
 * this visit, and the page renders the same either way.
 */
export function setEnabled(on: boolean): void {
  const storage = browserStorage();
  if (storage) {
    try {
      storage.setItem(STORAGE_KEY, on ? "on" : "off");
      sessionPreference = null;
      return;
    } catch {
      /* fall through to the in-memory preference */
    }
  }
  sessionPreference = on;
}

/**
 * Announce a new outage, once, if the operator asked for it.
 *
 * The caller decides when a count has risen; this only obeys the preference and permission. The
 * click focuses the window and runs the same opener the chip uses, so the notification is a second
 * way to the page, not a second route. A browser that refuses to construct it is not an error the
 * poll should see.
 */
export function notifyMonitors(down: number, total: number, onOpen: () => void): void {
  try {
    if (permission() !== "granted" || !enabled()) return;
    const api = browserNotification();
    if (!api) return;
    const noun = down === 1 ? "monitor" : "monitors";
    const note = new api(`Summrise — ${down} ${noun} down`, {
      body: `${down} of ${total} monitors down. Click to open Monitors.`
    });
    note.onclick = () => {
      window.focus();
      onOpen();
    };
  } catch {
    /* a browser that refuses the constructor is a browser that shows no notification */
  }
}
