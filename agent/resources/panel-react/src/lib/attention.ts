// ATTENTION — what the device needs a HUMAN for, and how it says so when nobody is looking.
//
// WHY IT EXISTS. Everything this panel shows assumes somebody is looking at it: a card, a chip, a
// banner inside the page. But the two facts on this device that are only useful NOW are exactly the
// ones an operator is away for — **a host they asked us to watch went down**, and **an AI is
// blocked on a question a person has to answer**. Round 8 gave the first a banner inside the page;
// this module is the part that leaves the page.
//
// THREE CHANNELS, and the ordering between them is the design (the cheapest and most reliable is
// first, the most intrusive is last):
//   1. **the tab TITLE** — no permission, no API, works in every browser, and it is what a
//      background tab is judged by. `(2) ⚠ Vale Agent`, never `Vale Agent`.
//   2. **the FAVICON** — the same information as a shape, for a pinned tab where the title is
//      clipped to nothing;
//   3. **a desktop NOTIFICATION** — the only one that crosses to another application, and the only
//      one that can be REFUSED. It is offered, never assumed, and the panel states plainly what
//      happens if it is denied.
//
// THE RULE IS ONE FUNCTION (`attentionFrom`, `titleFor`, `badgeIcon`) so the three channels cannot
// disagree about how much needs attention — a title saying 1 and a badge saying 2 is worse than
// either alone.
import type { Monitors } from "../hooks/useMonitors";

/** The key for "this target is in THIS state, since this moment" — shared by the poll
 *  (`summary.sinceMs`) and the device's push (`at_ms`), which describe the same transition. */
export function stateKey(id: string, sinceMs: number | null | undefined): string {
  return `state:${id}:${sinceMs ?? 0}`;
}

export interface AttentionItem {
  /** Stable across renders: what makes one item the same item. */
  key: string;
  kind: "down" | "approval";
  /** The line a notification shows. */
  text: string;
}

export const BASE_TITLE = "Vale Agent";

/** What needs a human, in the order a human would want it: an AI BLOCKED ON A QUESTION outranks a
 *  host that is down, because the first is a person's work stopped and the second is a fact that
 *  keeps (the monitor is still watching either way). */
export function attentionFrom(
  monitors: Monitors | null | undefined,
  pendingApprovals: number,
): AttentionItem[] {
  const items: AttentionItem[] = [];
  if (pendingApprovals > 0) {
    items.push({
      key: `approval:${pendingApprovals}`,
      kind: "approval",
      text:
        pendingApprovals === 1
          ? "An AI is waiting for your approval"
          : `${pendingApprovals} AIs are waiting for your approval`,
    });
  }
  for (const t of monitors?.targets ?? []) {
    if (t.summary.upNow === false) {
      items.push({
        // ONE KEY PER STATE, NOT PER SOURCE. The device PUSHES a transition (with the outage
        // duration) and the poll also sees the same down state — and they are the same fact, so
        // they must share a key or one outage is announced twice. `sinceMs` is when the current
        // state began, which is exactly what the push calls `at_ms`; when the state changes the
        // key changes, so a link that goes down twice still notifies twice.
        key: stateKey(t.id, t.summary.sinceMs),
        kind: "down",
        text: `${t.host}:${t.port} is down`,
      });
    }
  }
  return items;
}

/** The `document.title` for this much attention. The count comes FIRST because a browser tab
 *  truncates from the right, and a title that ends in `…` before the number is a title that told
 *  nobody anything. */
export function titleFor(items: AttentionItem[], base: string = BASE_TITLE): string {
  if (items.length === 0) return base;
  const urgent = items.some((i) => i.kind === "approval");
  return `(${items.length})${urgent ? " ⚠" : ""} ${base}`;
}

/** The favicon for this much attention, as a data URL.
 *
 *  Built rather than shipped as files: one SVG, one colour decision, and the SAME icon the panel
 *  already loads (the orange mark) with a badge added — so a pinned tab stays recognisable. The
 *  badge is a red disc with the count, or a plain disc with NO count when there is nothing to
 *  count (a count of 9+ would be illegible at 16 px anyway). */
export function badgeIcon(count: number, urgent: boolean, baseHref?: string): string {
  if (count <= 0) return baseHref ?? "";
  const fill = urgent ? "%23d9480f" : "%23e03131";
  const label = count > 9 ? "" : String(count);
  const text = label
    ? `<text x='37' y='43' font-family='system-ui,sans-serif' font-size='20' font-weight='700' fill='white' text-anchor='middle'>${label}</text>`
    : "";
  return (
    "data:image/svg+xml," +
    "%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 48 48'%3E" +
    "%3Crect width='48' height='48' rx='11' fill='%23f59f00'/%3E" +
    "%3Cpath fill='%23ffffff' opacity='.85' d='M14 41Q26 16 44 41Z'/%3E" +
    `%3Ccircle cx='37' cy='11' r='11' fill='${fill}'/%3E` +
    text +
    "%3C/svg%3E"
  );
}

/** The one-line summary the settings card and any tooltip use. */
export function attentionSummary(items: AttentionItem[]): string {
  if (items.length === 0) return "Nothing needs you right now.";
  if (items.length === 1) return items[0].text;
  return `${items.length} things need you: ${items.map((i) => i.text).join("; ")}`;
}
