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
import {
  attentionFrom,
  attentionSummary,
  badgeIcon,
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

describe("the tab title and the badge", () => {
  it("leads with the count, because a tab truncates from the right", () => {
    expect(titleFor([])).toBe(BASE_TITLE);
    const items: AttentionItem[] = [{ key: "down:a:22", kind: "down", text: "a:22 is down" }];
    expect(titleFor(items)).toBe("(1) Vale Agent");
    const both: AttentionItem[] = [...items, { key: "approval:1", kind: "approval", text: "waiting" }];
    expect(titleFor(both)).toBe("(2) ⚠ Vale Agent");
  });

  it("badges the icon with the count, and restores the base icon when nothing is wrong", () => {
    const base = "data:image/svg+xml,BASE";
    expect(badgeIcon(0, false, base)).toBe(base);
    const one = badgeIcon(1, false, base);
    expect(one).toContain("data:image/svg+xml");
    expect(one).toContain(">1</text>");
    // Past nine the number is illegible at 16 px, so the badge is a plain disc.
    expect(badgeIcon(12, false, base)).not.toContain("</text>");
    // An urgent item changes the disc, so the shape carries it as well as the title's ⚠.
    expect(badgeIcon(1, true, base)).not.toBe(one);
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
