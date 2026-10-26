/**
 * A terminal tab's identity.
 *
 * A tab is a device session plus a name a person can recognise. The name is console-local: renaming
 * one changes no device session and sends no call. Generated names are `${kind} ${n}` for the smallest
 * free `n`, so closing a middle tab fills its gap and a name the operator typed is never rewritten.
 */
export type Tab = {
  key: string;
  session: string;
  kind: string;
  title: string;
};

/** The next generated label for `kind`: the smallest `n >= 1` whose title no tab carries. */
export function nextTabTitle(kind: string, tabs: readonly Tab[]): string {
  const taken = new Set(tabs.map((tab) => tab.title));
  let n = 1;
  while (taken.has(`${kind} ${n}`)) n += 1;
  return `${kind} ${n}`;
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
