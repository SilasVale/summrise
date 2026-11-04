import { compactAge } from "./age";

/**
 * A terminal tab's identity.
 *
 * A tab is a device session plus a name a person can recognise. The name is console-local: renaming
 * one changes no device session and sends no call. A tab the operator has not renamed carries no
 * title at all — its label derives the kind and the session's age at render time, rather than
 * storing a generated string that would have to be rewritten as the age grows.
 */
export type Tab = {
  key: string;
  session: string;
  kind: string;
  /**
   * The name the operator typed, exactly; `null` means "no name of its own" and the label is
   * generated from `kind` and `startedAtMs`. Keeping this distinct from a generated string is what
   * makes a rename survive the next render and a generated label never overwrite it.
   */
  title: string | null;
  /**
   * When the device created the session, from `terminal.list`; a session this page opened has the
   * page's clock at the moment the device answered. The session card reads it as an age; a tab from
   * a list that did not say keeps it undefined rather than inventing a start.
   */
  startedAtMs?: number;
};

/**
 * What a tab says: the operator's name when there is one, otherwise the kind and the session's age.
 *
 * **THE PAGE DOES NOT TICK FOR THIS LABEL.** It has no interval that re-renders the strip; the age
 * is computed from the clock every time React renders the tab, so any state change (a poll fault, a
 * new tab, a refreshed session list) brings it up to date without a timer whose only job is a label.
 * A session whose start the device did not report says only its kind: a missing start is not a
 * licence to invent an age.
 */
export function tabLabel(tab: Tab, nowMs: number): string {
  if (tab.title !== null) return tab.title;
  if (typeof tab.startedAtMs !== "number") return tab.kind;
  return `${tab.kind} · ${compactAge(nowMs - tab.startedAtMs)}`;
}

/** What the rename field means: trimmed, internal runs collapsed, empty refused, at most 60 chars. */
export function titleFromInput(value: string): string | null {
  const collapsed = value.trim().replace(/\s+/g, " ");
  if (!collapsed) return null;
  // Count code points, not UTF-16 units: `slice` would cut an emoji in half at the cap.
  return Array.from(collapsed).slice(0, 60).join("");
}

/** Replace one tab's title, keeping every other tab object as it was. */
export function renameTab(tabs: readonly Tab[], key: string, title: string): Tab[] {
  if (!tabs.some((tab) => tab.key === key)) return [...tabs];
  return tabs.map((tab) => (tab.key === key ? { ...tab, title } : tab));
}
