// useAttention — the three channels, wired to the device's own facts.
//
// SOURCES, and each one is a fact the device already produces:
//   * a watched target's state (`useMonitors`, polled) → the tab title, the badge, and a
//     notification when it CHANGES;
//   * a `monitor-change` frame (round 8's push) → the notification, immediately, with the outage
//     duration in the body — the poll would catch it too, but up to 20 s later;
//   * an AI waiting for approval (`pendingApprovalCount`) → all three channels.
//
// IT DOES NOT OWN PERMISSION. The operator's answer to "may this page notify you?" is stored per
// origin by the browser and the toggle in Settings is what asks; this hook only READS it, so a page
// that has never asked shows the title/badge channels and nothing else.
import { useEffect, useMemo, useRef, useState } from "react";
import { attentionFrom, badgeIcon, titleFor, BASE_TITLE, type AttentionItem } from "../lib/attention";
import {
  DeviceNotifier,
  readPermission,
  type NotificationCtor,
  type NotifyPermission,
} from "../lib/notify";
import type { Monitors } from "./useMonitors";
import { parseMonitorChange } from "./useMonitors";

/** The live permission, re-read when the tab regains focus (the operator may have changed it in
 *  the browser's site settings while the panel was in the background). */
export function useNotifyPermission(): [NotifyPermission, () => Promise<NotifyPermission>] {
  const ctor = (typeof Notification !== "undefined" ? Notification : null) as NotificationCtor | null;
  const [state, setState] = useState<NotifyPermission>(() => readPermission(ctor));
  useEffect(() => {
    const reread = () => setState(readPermission(ctor));
    window.addEventListener("focus", reread);
    document.addEventListener("visibilitychange", reread);
    return () => {
      window.removeEventListener("focus", reread);
      document.removeEventListener("visibilitychange", reread);
    };
  }, [ctor]);
  const request = async (): Promise<NotifyPermission> => {
    if (!ctor) return "unsupported";
    try {
      const answer = await Notification.requestPermission();
      const next = readPermission(ctor, answer);
      setState(next);
      return next;
    } catch {
      const next = readPermission(ctor);
      setState(next);
      return next;
    }
  };
  return [state, request];
}

/** The attention items, memoised on the two things that produce them. */
export function useAttention(
  monitors: Monitors | null | undefined,
  pendingApprovals: number,
): AttentionItem[] {
  return useMemo(
    () => attentionFrom(monitors, pendingApprovals),
    // The monitors object is rebuilt on every poll; its MEANING is what matters, so the memo keys
    // on the derived signature rather than on identity (which would re-render on every poll).
    [monitorsSignature(monitors), pendingApprovals],
  );
}

/** A string that changes exactly when the attention-relevant part of the monitors changes. */
function monitorsSignature(m: Monitors | null | undefined): string {
  return (m?.targets ?? []).map((t) => `${t.id}:${t.summary.upNow === false ? "d" : "u"}`).join(",");
}

/**
 * Channel 1 and 2: the tab title and the favicon.
 *
 * EVERY TAB ALREADY HAS A TITLE, and that is why this is the channel that always works: it needs no
 * permission, no API and no cooperation, and it is the one piece of the page a browser shows when
 * the page is not on screen. The favicon is set by href so the base icon (served from the page) is
 * restored the moment nothing needs attention.
 */
export function useAttentionTitle(items: AttentionItem[]) {
  useEffect(() => {
    const link = document.querySelector<HTMLLinkElement>("link[rel='icon']");
    const baseHref = link?.getAttribute("href") ?? "";
    const count = items.length;
    document.title = titleFor(items);
    if (link) link.setAttribute("href", badgeIcon(count, items.some((i) => i.kind === "approval"), baseHref));
    return () => {
      // Leaving the panel restores the plain title/icon: a stale count in a tab that no longer
      // tracks anything is a lie that outlives its page.
      document.title = BASE_TITLE;
      if (link && baseHref) link.setAttribute("href", baseHref);
    };
  }, [items]);
}

/**
 * Channel 3: desktop notifications, for the two things that are worth interrupting somebody for.
 *
 * A change frame notifies IMMEDIATELY (it carries the outage duration, which the poll cannot); the
 * poll only notifies for a state it has not announced yet, so the two cannot double-fire for one
 * event — the notifier's dedupe key is the same in both paths.
 */
export function useAttentionNotifications(
  items: AttentionItem[],
  permission: NotifyPermission,
  enabled: boolean,
) {
  const notifier = useRef<DeviceNotifier | null>(null);
  if (!notifier.current) {
    const ctor = (typeof Notification !== "undefined" ? Notification : null) as NotificationCtor | null;
    notifier.current = new DeviceNotifier(ctor);
  }

  // A condition that CLEARS must be able to fire again later: the keys of live items are retained,
  // everything else is forgotten here.
  useEffect(() => {
    notifier.current?.retain(items.map((i) => i.key));
  }, [items]);

  // The poll's view of the world (a host that is down, an AI waiting).
  useEffect(() => {
    if (!enabled) return;
    for (const item of items) {
      notifier.current?.notify(
        {
          key: item.key,
          title: item.kind === "approval" ? "Vale needs you" : "Vale: host down",
          body: item.text,
        },
        permission,
      );
    }
  }, [items, permission, enabled]);

  // The device's PUSH, which arrives between polls and carries how long the last state lasted.
  useEffect(() => {
    if (!enabled) return;
    const onFrame = (e: Event) => {
      const change = parseMonitorChange((e as CustomEvent).detail);
      if (!change) return;
      const body = change.up
        ? `back up after ${humanMs(change.lastedMs)} down`
        : `DOWN — it had been up ${humanMs(change.lastedMs)}`;
      notifier.current?.notify(
        {
          key: `change:${change.id}:${change.atMs}`,
          title: change.up ? "Vale: host back up" : "Vale: host down",
          body: `${change.host}:${change.port} is ${body}`,
        },
        permission,
      );
      // The poll's `down:<id>` key has been answered by this push; retiring it stops the same
      // outage from being announced twice.
      notifier.current?.clear(`down:${change.id}`);
    };
    window.addEventListener("vale-monitor-change", onFrame);
    return () => window.removeEventListener("vale-monitor-change", onFrame);
  }, [permission, enabled]);
}

/** `2m 15s` / `45s` / `1h 04m` — the same shapes the rest of the panel uses for durations. */
export function humanMs(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) {
    const m = Math.floor(s / 60);
    const r = s % 60;
    return r === 0 ? `${m}m` : `${m}m ${r}s`;
  }
  const h = Math.floor(s / 3600);
  return `${h}h ${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}m`;
}
