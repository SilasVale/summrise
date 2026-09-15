// GETTING YOUR ATTENTION — the rules, not the plumbing.
//
// Three channels, one rule set, and every way each can lie:
//   * a title that says the page is fine while something needs a person;
//   * a count that survives the condition that produced it (a stale "(2)" in a tab);
//   * a notification that fires on EVERY poll for a host that is still down (the cannon);
//   * a notification that fires once and never again when the link goes down a second time
//     (dedupe that forgot to forget);
//   * a denied permission rendered as "on" — the one failure that teaches an operator to
//     distrust the whole panel.
import { describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { renderHook } from "@testing-library/react";
import { useAttentionNotifications } from "../hooks/useAttention";
import {
  attentionFrom,
  attentionSummary,
  badgeIcon,
  shouldNotify,
  stateKey,
  titleFor,
  BASE_TITLE,
  type AttentionItem,
} from "../lib/attention";
import {
  DeviceNotifier,
  permissionHint,
  readPermission,
  type NotificationLike,
} from "../lib/notify";
import { humanMs } from "../hooks/useAttention";
import { NotificationsCard } from "../components/NotificationsCard";
import type { Monitors } from "../hooks/useMonitors";

const target = (id: string, upNow: boolean | null) => ({
  id,
  host: id.split(":")[0],
  port: Number(id.split(":")[1]) || 22,
  path: null,
  expect: null,
  series: [],
  transitions: [],
  summary: {
    probes: 3,
    up: upNow === false ? 0 : 3,
    down: upNow === false ? 3 : 0,
    upPct: upNow === false ? 0 : 100,
    upNow,
    sinceMs: 1,
    latency: null,
    drops: 0,
    lastStatus: null,
    lastExpectOk: null,
  },
});
const monitors = (targets: ReturnType<typeof target>[]): Monitors => ({
  targets,
  intervalSecs: 15,
  seriesMax: 240,
});

describe("what needs a person", () => {
  it("puts a blocked AI ahead of a host that is down, and ignores what is fine", () => {
    const items = attentionFrom(monitors([target("a:22", false), target("b:22", true), target("c:22", null)]), 1);
    expect(items.map((i) => i.kind)).toEqual(["approval", "down"]);
    expect(items[1].text).toBe("a:22 is down");
    expect(attentionSummary(items)).toContain("2 things need you");
    // Nothing wrong: no items, and the summary says so rather than being empty.
    expect(attentionFrom(monitors([target("b:22", true)]), 0)).toEqual([]);
    expect(attentionSummary([])).toBe("Nothing needs you right now.");
    // No monitors at all is not an error (the poll may not have answered yet).
    expect(attentionFrom(null, 0)).toEqual([]);
  });

  it("counts an AI waiting, pluralised, and keyed so it can fire again", () => {
    expect(attentionFrom(monitors([]), 1)[0].text).toBe("An AI is waiting for your approval");
    expect(attentionFrom(monitors([]), 3)[0].text).toBe("3 AIs are waiting for your approval");
    expect(attentionFrom(monitors([]), 2)[0].key).toBe("approval:2");
  });
});

describe("one event, one channel", () => {
  it("the page speaks while you look at it, the OS speaks when you do not", () => {
    // Visible: the banner says it; an OS notification would interrupt somebody already reading it.
    expect(shouldNotify("visible")).toBe(false);
    // Hidden: the notification is the only channel that can reach them.
    expect(shouldNotify("hidden")).toBe(true);
    expect(shouldNotify("prerender")).toBe(true);
    // No visibility API at all counts as hidden: a notification that fires when it need not is a
    // smaller failure than one that never fires when it must.
    expect(shouldNotify(undefined)).toBe(true);
    expect(shouldNotify(null)).toBe(true);
    expect(shouldNotify("")).toBe(true);
  });
});

describe("the tab title and the badge", () => {
  it("leads with the count, because a tab truncates from the right", () => {
    expect(titleFor([])).toBe(BASE_TITLE);
    const items: AttentionItem[] = [{ key: "down:a:22", kind: "down", text: "a:22 is down" }];
    expect(titleFor(items)).toBe("(1) Vale Agent");
    const both: AttentionItem[] = [...items, { key: "approval:1", kind: "approval", text: "waiting" }];
    expect(titleFor(both)).toBe("(2) ⚠ Vale Agent");
  });

  it("ADDS the badge to the existing icon instead of drawing a new one", () => {
    // The first version returned a simplified drawing of its own, which read as a different, wrong
    // icon the moment anything needed attention (the operator noticed immediately). The artwork must
    // survive byte-for-byte; only a disc is added.
    // THE REAL ENCODING, copied from the device's own panel (`data:image/svg+xml,%3Csvg …%3C/svg%3E`).
    // The first fixture was an unencoded SVG — a guess — and the code searched for a literal `<svg`
    // in a href that never contains one, so the badge silently never appeared while this test passed.
    const artwork = `%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 48 48'%3E%3Cdefs%3E%3ClinearGradient id='s'%3E%3Cstop stop-color='%23f59f00'/%3E%3C/linearGradient%3E%3C/defs%3E%3Crect width='48' height='48' rx='11' fill='url(%23s)'/%3E%3Cpath fill='%23fff' d='M14 41Q26 16 44 41Z'/%3E%3C/svg%3E`;
    const base = `data:image/svg+xml,${artwork}`;
    // Nothing to badge: the base comes back untouched (and an empty href stays empty).
    expect(badgeIcon(0, false, base)).toBe(base);
    expect(badgeIcon(0, false, undefined)).toBe("");
    const one = badgeIcon(1, false, base);
    // THE ARTWORK IS INTACT — the gradient, the mountain, the rounding are all still there.
    expect(one).toContain("%3ClinearGradient id='s'%3E");
    expect(one).toContain("M14 41Q26 16 44 41Z");
    expect(one).toContain("rx='11'");
    // …with the disc (and the count) inserted before the closing tag, in the same encoding.
    expect(one.indexOf("circle cx='37'")).toBeGreaterThan(one.indexOf("M14 41Q26 16 44 41Z"));
    expect(one.indexOf("circle cx='37'")).toBeLessThan(one.indexOf("%3C/svg%3E"));
    expect(one).toContain("1%3C/text%3E");
    expect(decodeURIComponent(one)).toContain("</text>");
    // Past nine the number is illegible at 16 px, so the badge is a plain disc.
    expect(badgeIcon(12, false, base)).not.toContain("text");
    // An urgent item changes the disc, so the shape carries it as well as the title's ⚠.
    expect(badgeIcon(1, true, base)).not.toBe(one);
    // An icon that is not an inline SVG is left alone rather than replaced by something invented.
    expect(badgeIcon(3, false, "/favicon.ico")).toBe("/favicon.ico");
  });
});

describe("the notifier's rules", () => {
  const ok = "granted" as const;
  function ctor() {
    const sent: { title: string; body?: string; tag?: string }[] = [];
    const C = function (this: unknown, title: string, opts?: { body?: string; tag?: string }) {
      sent.push({ title, ...opts });
      return {} as NotificationLike;
    } as unknown as new (t: string, o?: { body?: string; tag?: string }) => NotificationLike;
    return { sent, C };
  }

  it("fires once per key, and again after the condition clears", () => {
    const { sent, C } = ctor();
    let now = 1_000_000;
    const n = new DeviceNotifier(C, () => now);
    const payload = { key: "down:a:22", title: "Vale: host down", body: "a:22 is down" };
    expect(n.notify(payload, ok)).toBe(true);
    now += 60_000;
    // Still down on the next poll: NO second notification.
    expect(n.notify(payload, ok)).toBe(false);
    expect(sent).toHaveLength(1);
    // The link recovers; the key is retired…
    n.retain([]);
    now += 60_000;
    // …so the NEXT outage notifies again. This is the half a naive dedupe gets wrong.
    expect(n.notify(payload, ok)).toBe(true);
    expect(sent).toHaveLength(2);
    expect(sent[0].tag).toBe("down:a:22");
  });

  it("rate-limits a flapping link instead of firing a cannon", () => {
    const { sent, C } = ctor();
    let now = 1_000_000;
    const n = new DeviceNotifier(C, () => now);
    const fire = (i: number) => n.notify({ key: `k${i}`, title: "t", body: "b" }, ok);
    expect(fire(1)).toBe(true); // t=0
    now += 1_000;
    expect(fire(2)).toBe(false); // inside the minimum gap
    now += 5_000;
    expect(fire(3)).toBe(true);
    now += 5_000;
    expect(fire(4)).toBe(true);
    now += 5_000;
    expect(fire(5)).toBe(false); // burst of three inside the window
    expect(n.suppressed).toBe(2);
    now += 61_000;
    expect(fire(6)).toBe(true); // the window rolled over
    expect(sent.length).toBe(4);
  });

  it("never sends without permission, and survives a constructor that throws", () => {
    const { sent, C } = ctor();
    const n = new DeviceNotifier(C);
    expect(n.notify({ key: "k", title: "t", body: "b" }, "default")).toBe(false);
    expect(n.notify({ key: "k", title: "t", body: "b" }, "denied")).toBe(false);
    expect(n.notify({ key: "k", title: "t", body: "b" }, "unsupported")).toBe(false);
    expect(sent).toHaveLength(0);
    const throwing = new DeviceNotifier(
      function () {
        throw new Error("no");
      } as unknown as new () => NotificationLike,
    );
    expect(throwing.notify({ key: "k", title: "t", body: "b" }, "granted")).toBe(false);
    // A page with no Notification at all is a STATE, not a crash.
    expect(new DeviceNotifier(null).notify({ key: "k", title: "t", body: "b" }, "granted")).toBe(false);
  });

  it("reads the browser's answer as one of four states, and explains each", () => {
    expect(readPermission(null)).toBe("unsupported");
    expect(readPermission((() => {}) as never, "granted")).toBe("granted");
    expect(readPermission((() => {}) as never, "denied")).toBe("denied");
    expect(readPermission((() => {}) as never, "default")).toBe("default");
    // The hint for each state must say what to DO, and the denied one must name the channel that
    // still works rather than leaving the operator stuck.
    expect(permissionHint("denied")).toContain("tab title");
    expect(permissionHint("unsupported")).toContain("tab title");
    expect(permissionHint("granted")).toContain("background");
    expect(permissionHint("default")).toContain("asks your browser once");
  });
});

describe("one outage is ONE notification (the bug the live test found)", () => {
  it("gives the poll and the device's push the same key for the same state", async () => {
    const { sent, C } = (() => {
      const sent: { title: string; body?: string; tag?: string }[] = [];
      const C = function (this: unknown, title: string, opts?: { body?: string; tag?: string }) {
        sent.push({ title, ...opts });
        return {};
      } as unknown as new (t: string, o?: { body?: string; tag?: string }) => NotificationLike;
      return { sent, C };
    })();
    // A CLOCK THE TEST OWNS: the rate limit is part of the design, so a test that fires three
    // notifications "at once" is testing the limiter, not the keys.
    let now = 1_000_000;
    const n = new DeviceNotifier(C, () => now);

    // The PUSH arrives first (it is emitted from the same lock that appends the probe)…
    const sinceMs = 1_789_392_840_540;
    n.notify(
      { key: stateKey("a:22", sinceMs), title: "Vale: host down", body: "a:22 is DOWN — it had been up 40s" },
      "granted",
    );
    // …and the next POLL sees the same down state, deriving its key from `summary.sinceMs`.
    n.notify({ key: stateKey("a:22", sinceMs), title: "Vale: host down", body: "a:22 is down" }, "granted");
    expect(sent).toHaveLength(1);
    expect(sent[0].body).toContain("40s");

    // The link recovers: the state key changes, and the recovery is announced once.
    now += 60_000;
    n.retain([]);
    n.notify({ key: stateKey("a:22", sinceMs + 60_000), title: "Vale: host back up", body: "back up" }, "granted");
    expect(sent).toHaveLength(2);

    // A SECOND outage (new state) notifies again — the dedupe must not become permanent.
    now += 60_000;
    n.retain([]);
    n.notify({ key: stateKey("a:22", sinceMs + 300_000), title: "Vale: host down", body: "again" }, "granted");
    expect(sent).toHaveLength(3);
  });
});

describe("the settings card", () => {
  it("shows the browser's actual answer and never renders a refusal as 'on'", () => {
    const { rerender } = render(
      <NotificationsCard permission="denied" onRequest={async () => "denied" as const} onTest={() => {}} attention={[]} />,
    );
    expect(screen.getByText("Notifications unavailable")).toBeTruthy();
    expect(screen.getByText(/blocking notifications/).textContent).toContain("tab title");
    expect(screen.getByText("denied")).toBeTruthy();
    rerender(
      <NotificationsCard permission="granted" onRequest={async () => "granted" as const} onTest={() => {}} attention={[]} />,
    );
    expect(screen.getByText("Notifications on")).toBeTruthy();
    expect(screen.getByText("granted")).toBeTruthy();
  });

  it("asks the browser from the button, which is the gesture it requires", async () => {
    const onRequest = vi.fn(async () => "granted" as const);
    render(<NotificationsCard permission="default" onRequest={onRequest} onTest={() => {}} attention={[]} />);
    fireEvent.click(screen.getByRole("button", { name: "Turn on notifications" }));
    await waitFor(() => expect(onRequest).toHaveBeenCalledTimes(1));
  });

  it("sends a test only when asked, and says what currently needs the operator", () => {
    const onTest = vi.fn();
    render(
      <NotificationsCard
        permission="granted"
        onRequest={async () => "granted" as const}
        onTest={onTest}
        attention={[{ key: "down:a:22", kind: "down", text: "a:22 is down" }]}
      />,
    );
    expect(screen.getByText("a:22 is down")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Send a test" }));
    expect(onTest).toHaveBeenCalledTimes(1);
  });
});

describe("humanMs", () => {
  it("reads like the rest of the panel", () => {
    expect(humanMs(45_000)).toBe("45s");
    expect(humanMs(60_000)).toBe("1m");
    expect(humanMs(135_000)).toBe("2m 15s");
    expect(humanMs(3_900_000)).toBe("1h 05m");
  });
});


describe("useAttentionNotifications and the tab's visibility", () => {
  const notifierCtor = () => {
    const sent: { title: string; body?: string }[] = [];
    const C = function (this: unknown, title: string, opts?: { body?: string }) {
      sent.push({ title, ...opts });
      return { close() {} };
    } as unknown as new (t: string, o?: { body?: string }) => NotificationLike;
    return { sent, C };
  };

  it("does not notify while the tab is visible, and does when it is hidden", async () => {
    const { sent, C } = notifierCtor();
    const original = Object.getOwnPropertyDescriptor(Document.prototype, "visibilityState");
    const setVisibility = (v: string) =>
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => v });
    const items = [{ key: "down:a:22", kind: "down" as const, text: "a:22 is down" }];

    // A REAL notifier instance, driven through the hook's own path: install the spy ctor so
    // `DeviceNotifier` picks it up.
    const RealNotification = (globalThis as any).Notification;
    (globalThis as any).Notification = C;
    try {
      setVisibility("visible");
      const { unmount } = renderHook(() => useAttentionNotifications(items, "granted", true));
      expect(sent, "nothing interrupts a tab you are looking at").toHaveLength(0);
      unmount();

      setVisibility("hidden");
      renderHook(() => useAttentionNotifications(items, "granted", true));
      await waitFor(() => expect(sent.length).toBe(1));
      expect(sent[0].body).toBe("a:22 is down");
    } finally {
      (globalThis as any).Notification = RealNotification;
      if (original) Object.defineProperty(Document.prototype, "visibilityState", original);
      else delete (document as any).visibilityState;
    }
  });
});
