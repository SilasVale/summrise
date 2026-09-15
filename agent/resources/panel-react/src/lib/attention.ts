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

/**
 * WHICH CHANNEL SPEAKS — the rule that keeps one event from arriving four times.
 *
 * This panel ended up with four ways to say the same thing: an in-page banner, the strip's chip,
 * a count in the tab title, and a desktop notification. A watched host going down set off all
 * four at once, which is how a signal becomes noise.
 *
 * The rule: **the page speaks when you are looking at it, the OS speaks when you are not.**
 *
 *   * tab VISIBLE  → the banner (and the chip, and the title): everything is on screen, so an OS
 *     notification would interrupt somebody who is already reading the answer.
 *   * tab HIDDEN   → the desktop notification, plus the title/badge for the moment you come back.
 *     The banner is not even rendered: nobody is there to see it.
 *
 * The chip and the title are NOT part of the choice — they are state, not interruption, and they
 * cost nothing. Only the two INTERRUPTING channels are mutually exclusive.
 */
export function shouldNotify(visibility: string | undefined | null): boolean {
    // No visibility API (a test environment, an old browser) counts as HIDDEN: a notification that
    // fires when it need not is a smaller failure than one that never fires when it must.
    if (!visibility) return true;
    return visibility !== "visible";
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
 *  THE BADGE IS ADDED TO THE ICON, NOT INSTEAD OF IT. The first version RETURNED a simplified
 *  drawing of its own (an orange square, a red disc, a count) — which looked like a different,
 *  wrong icon the moment anything needed attention (the operator noticed on the panel and asked
 *  what had happened to it). Now the badge is inserted into the EXISTING svg data URL, so the
 *  artwork is byte-for-byte the one the page already loads and only a disc is added on top.
 *
 *  Returns the base unchanged when there is nothing to badge, or when the base is not an inline
 *  SVG (a file URL, an empty href) — an icon that cannot be parsed is left alone rather than
 *  replaced by something invented. */
export function badgeIcon(count: number, urgent: boolean, baseHref?: string): string {
    const base = baseHref ?? "";
    if (count <= 0 || !base) return base;
    // THE REAL HREF IS PERCENT-ENCODED (`data:image/svg+xml,%3Csvg …%3C/svg%3E`), so the markers
    // must be looked for in BOTH spellings: the first version searched for a literal `<svg`, never
    // found one in the encoded href, and silently returned the base — the artwork was preserved and
    // the badge never appeared (found on d1 by asking the panel what its icon actually was; the
    // test's fixture was an unencoded SVG, i.e. a guess).
    const encoded = base.includes("%3Csvg") || base.includes("%3csvg");
    const close = encoded ? Math.max(base.lastIndexOf("%3C/svg%3E"), base.lastIndexOf("%3c/svg%3e")) : base.lastIndexOf("</svg>");
    const marker = encoded ? Math.max(base.indexOf("%3Csvg"), base.indexOf("%3csvg")) : base.indexOf("<svg");
    if (marker < 0 || close < marker) return base;
    const fill = urgent ? "%23d9480f" : "%23e03131";
    const label = count > 9 ? "" : String(count);
    const lt = encoded ? "%3C" : "<";
    const gt = encoded ? "%3E" : ">";
    const badge =
        `${lt}circle cx='37' cy='11' r='11' fill='${fill}'/${gt}` +
        (label
            ? `${lt}text x='37' y='43' font-family='system-ui,sans-serif' font-size='20' font-weight='700' fill='white' text-anchor='middle'${gt}${label}${lt}/text${gt}`
            : "");
    // Inserted just before the closing tag: on top of the artwork, with the artwork intact.
    return base.slice(0, close) + badge + base.slice(close);
}

/** The one-line summary the settings card and any tooltip use. */
export function attentionSummary(items: AttentionItem[]): string {
  if (items.length === 0) return "Nothing needs you right now.";
  if (items.length === 1) return items[0].text;
  return `${items.length} things need you: ${items.map((i) => i.text).join("; ")}`;
}
