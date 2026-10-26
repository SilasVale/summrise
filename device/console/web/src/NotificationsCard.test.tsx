import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NotificationsCard } from "./NotificationsCard";
import { restoreGlobal } from "./test/dom";

/// The preference lives under one key; the test carries the literal rather than importing a
/// constant the module does not promise.
const STORAGE_KEY = "summrise.notify.monitors";

/**
 * The two static members the card asks the browser for.
 *
 * The card never constructs a notification — that is `notifyMonitors`'s job — so this fake only
 * answers the permission question and records whether the button asked it.
 */
class FakeNotification {
  static permission: NotificationPermission = "default";
  static requestPermission = vi.fn(async (): Promise<NotificationPermission> => FakeNotification.permission);
}

const originalNotification = Object.getOwnPropertyDescriptor(window, "Notification");

describe("NotificationsCard", () => {
  beforeEach(() => {
    window.localStorage.clear();
    FakeNotification.permission = "default";
    FakeNotification.requestPermission.mockReset();
    FakeNotification.requestPermission.mockImplementation(async () => FakeNotification.permission);
    Object.defineProperty(window, "Notification", { configurable: true, writable: true, value: FakeNotification });
  });

  afterEach(() => {
    restoreGlobal(window, "Notification", originalNotification);
    window.localStorage.clear();
  });

  it("is off by default, asks only when the button is pressed, and then remembers the choice", async () => {
    render(<NotificationsCard />);

    expect(screen.getByRole("region", { name: "Monitor notifications" })).toBeInTheDocument();
    expect(
      screen.getByText(
        "Get a browser notification when a new monitor goes down. The permission is asked only when you press the button."
      )
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Enable notifications" })).toBeInTheDocument();
    // Rendering the card is not a gesture: the prompt must wait for the click.
    expect(FakeNotification.requestPermission).not.toHaveBeenCalled();

    FakeNotification.permission = "granted";
    fireEvent.click(screen.getByRole("button", { name: "Enable notifications" }));

    expect(await screen.findByText(/^Notifications are on\./)).toBeInTheDocument();
    expect(FakeNotification.requestPermission).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Turn off" })).toBeInTheDocument();
    expect(window.localStorage.getItem(STORAGE_KEY)).toBe("on");
  });

  it("turns on without a prompt when the browser already granted permission", async () => {
    FakeNotification.permission = "granted";
    render(<NotificationsCard />);

    fireEvent.click(screen.getByRole("button", { name: "Enable notifications" }));

    expect(await screen.findByText(/^Notifications are on\./)).toBeInTheDocument();
    expect(FakeNotification.requestPermission).not.toHaveBeenCalled();
    expect(window.localStorage.getItem(STORAGE_KEY)).toBe("on");
  });

  it("turns off from the on state without asking permission again", async () => {
    FakeNotification.permission = "granted";
    window.localStorage.setItem(STORAGE_KEY, "on");
    render(<NotificationsCard />);

    expect(screen.getByText(/^Notifications are on\./)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Turn off" }));

    expect(await screen.findByRole("button", { name: "Enable notifications" })).toBeInTheDocument();
    expect(screen.queryByText(/^Notifications are on\./)).not.toBeInTheDocument();
    expect(window.localStorage.getItem(STORAGE_KEY)).toBe("off");
    expect(FakeNotification.requestPermission).not.toHaveBeenCalled();
  });

  it("says notifications are blocked, and promises no button that cannot work", () => {
    FakeNotification.permission = "denied";
    window.localStorage.setItem(STORAGE_KEY, "on");
    render(<NotificationsCard />);

    expect(
      screen.getByText(
        "Notifications are blocked for this site. Allow them in the browser's site settings, then reload this page."
      )
    ).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("says the browser does not support notifications instead of throwing", () => {
    Reflect.deleteProperty(window, "Notification");

    render(<NotificationsCard />);

    expect(
      screen.getByText(
        "This browser does not support notifications. The down chip still shows the count."
      )
    ).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});
