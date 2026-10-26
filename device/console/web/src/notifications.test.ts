import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  enabled,
  notifyMonitors,
  permission,
  requestPermission,
  setEnabled
} from "./notifications";
import { restoreGlobal } from "./test/dom";

/// The preference lives under one key; the test carries the literal rather than importing a
/// constant the module does not promise, so a rename cannot silently keep the test green.
const STORAGE_KEY = "summrise.notify.monitors";

/**
 * A browser `Notification`, as little of it as the module touches.
 *
 * `permission` and `requestPermission` are the browser's answer; `constructed` is every notification
 * the page asked for, with the exact arguments, because the title and body are the product here.
 */
class FakeNotification {
  static permission: NotificationPermission = "default";
  static requestPermission = vi.fn(async (): Promise<NotificationPermission> => FakeNotification.permission);
  static constructed: FakeNotification[] = [];
  static throws = false;

  onclick: (() => void) | null = null;
  close = vi.fn();
  title: string;
  options?: NotificationOptions;

  constructor(title: string, options?: NotificationOptions) {
    if (FakeNotification.throws) throw new Error("the browser refused to show it");
    this.title = title;
    this.options = options;
    FakeNotification.constructed.push(this);
  }
}

// jsdom has no `Notification`; the original descriptor is whatever the environment started with.
const originalNotification = Object.getOwnPropertyDescriptor(window, "Notification");

function installFake() {
  Object.defineProperty(window, "Notification", { configurable: true, writable: true, value: FakeNotification });
}

describe("notification permission", () => {
  beforeEach(() => {
    FakeNotification.permission = "default";
    FakeNotification.requestPermission.mockReset();
    FakeNotification.requestPermission.mockImplementation(async () => FakeNotification.permission);
    FakeNotification.constructed = [];
    FakeNotification.throws = false;
    window.localStorage.clear();
    installFake();
  });

  afterEach(() => {
    restoreGlobal(window, "Notification", originalNotification);
  });

  it("is unsupported when the browser has no Notification", () => {
    Reflect.deleteProperty(window, "Notification");

    expect(permission()).toBe("unsupported");
    expect(() => setEnabled(true)).not.toThrow();
    expect(enabled()).toBe(false);
  });

  it("maps the browser's permission", () => {
    FakeNotification.permission = "default";
    expect(permission()).toBe("default");
    FakeNotification.permission = "granted";
    expect(permission()).toBe("granted");
    FakeNotification.permission = "denied";
    expect(permission()).toBe("denied");
  });

  it("asks the browser and resolves its answer", async () => {
    FakeNotification.permission = "granted";

    await expect(requestPermission()).resolves.toBe("granted");
    expect(FakeNotification.requestPermission).toHaveBeenCalledTimes(1);
  });

  it("resolves, rather than throwing, when the browser refuses to answer", async () => {
    FakeNotification.requestPermission.mockRejectedValue(new Error("the prompt was blocked"));

    await expect(requestPermission()).resolves.toBe("default");
  });

  it("resolves unsupported without asking when there is no Notification", async () => {
    Reflect.deleteProperty(window, "Notification");

    await expect(requestPermission()).resolves.toBe("unsupported");
  });
});

describe("the stored preference", () => {
  beforeEach(() => {
    FakeNotification.permission = "granted";
    window.localStorage.clear();
    installFake();
  });

  afterEach(() => {
    restoreGlobal(window, "Notification", originalNotification);
    window.localStorage.clear();
  });

  it("is off until the operator turns it on, and remembers both answers", () => {
    expect(enabled()).toBe(false);

    setEnabled(true);
    expect(window.localStorage.getItem(STORAGE_KEY)).toBe("on");
    expect(enabled()).toBe(true);

    setEnabled(false);
    expect(window.localStorage.getItem(STORAGE_KEY)).toBe("off");
    expect(enabled()).toBe(false);
  });

  it("is off while the browser has not granted permission, even if the preference says on", () => {
    window.localStorage.setItem(STORAGE_KEY, "on");
    FakeNotification.permission = "denied";
    expect(enabled()).toBe(false);

    FakeNotification.permission = "default";
    expect(enabled()).toBe(false);
  });

  it("ignores a storage write the browser refuses, and still works for this visit", () => {
    const descriptor = Object.getOwnPropertyDescriptor(window, "localStorage");
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      get() {
        throw new Error("The browser blocked storage.");
      }
    });
    try {
      expect(() => setEnabled(true)).not.toThrow();
      // A preference the page just accepted is honoured for this visit; it simply cannot persist.
      expect(enabled()).toBe(true);
    } finally {
      restoreGlobal(window, "localStorage", descriptor);
      setEnabled(false);
    }
  });
});

describe("notifyMonitors", () => {
  beforeEach(() => {
    FakeNotification.permission = "granted";
    FakeNotification.constructed = [];
    FakeNotification.throws = false;
    window.localStorage.clear();
    installFake();
  });

  afterEach(() => {
    restoreGlobal(window, "Notification", originalNotification);
    window.localStorage.clear();
  });

  it("constructs nothing until permission is granted and the preference is on", () => {
    notifyMonitors(1, 2, vi.fn());
    expect(FakeNotification.constructed).toHaveLength(0);

    setEnabled(true);
    notifyMonitors(1, 2, vi.fn());
    expect(FakeNotification.constructed).toHaveLength(1);
  });

  it("stays quiet when permission is denied, even with the preference on", () => {
    window.localStorage.setItem(STORAGE_KEY, "on");
    FakeNotification.permission = "denied";

    notifyMonitors(1, 2, vi.fn());
    expect(FakeNotification.constructed).toHaveLength(0);
  });

  it("names the count in the title and the total in the body", () => {
    setEnabled(true);

    notifyMonitors(1, 3, vi.fn());
    notifyMonitors(2, 3, vi.fn());

    expect(FakeNotification.constructed.map((note) => [note.title, note.options?.body])).toEqual([
      ["Summrise — 1 monitor down", "1 of 3 monitors down. Click to open Monitors."],
      ["Summrise — 2 monitors down", "2 of 3 monitors down. Click to open Monitors."]
    ]);
  });

  it("focuses the window and opens Monitors when the notification is clicked", () => {
    setEnabled(true);
    const focus = vi.spyOn(window, "focus").mockImplementation(() => {});
    const onOpen = vi.fn();

    notifyMonitors(1, 2, onOpen);
    const note = FakeNotification.constructed[0];
    expect(note.onclick).not.toBeNull();
    note.onclick?.();

    expect(focus).toHaveBeenCalledTimes(1);
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("swallows a constructor the browser refuses", () => {
    setEnabled(true);
    FakeNotification.throws = true;
    const onOpen = vi.fn();

    expect(() => notifyMonitors(1, 2, onOpen)).not.toThrow();
    expect(onOpen).not.toHaveBeenCalled();
  });
});
