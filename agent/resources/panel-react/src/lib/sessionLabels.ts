// Session labels, disambiguated — ONE implementation, because there are TWO strips.
//
// WHY IT EXISTS. The live panel (2026-09-17) had 16 sessions, TEN of them labelled `pwsh`, so its tab strip
// rendered ten identical tabs truncated to `pws…` — you could not tell which one held your work. Round 167
// fixed that in TabBar. Round 170 measured a page and found the DESKTOP strip still showing `d1,
// serial:COM4, d1, …`, because the desktop renders its own tabs and the fix had been written into the
// panel's renderer instead of into a shared place. That is the same mistake the shell's control group
// records ("a per-density copy is how the two drifted before, R131"), made one round after quoting it.
//
// THE FIRST KEEPS THE BARE LABEL and repeats take a counter: `pwsh`, `pwsh 2`, `pwsh 3`. A NUMBER because
// that is the only information the row honestly has — a pty session's label is its shell, its sid is opaque,
// and two PowerShell sessions genuinely ARE interchangeable until you look inside them.
export function disambiguateLabels<T extends { label: string }>(items: T[]): string[] {
  const seen = new Map<string, number>();
  return items.map((item) => {
    const n = (seen.get(item.label) ?? 0) + 1;
    seen.set(item.label, n);
    return n === 1 ? item.label : `${item.label} ${n}`;
  });
}
