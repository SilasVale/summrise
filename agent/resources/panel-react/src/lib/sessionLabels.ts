// Session labels, disambiguated — ONE implementation, because there are TWO strips.
//
// WHY IT EXISTS. The live panel (2026-09-17) had 16 sessions, TEN of them labelled `pwsh`, so its tab strip
// rendered ten identical tabs truncated to `pws…` — you could not tell which one held your work. Round 167
// fixed that in TabBar. Round 170 measured a page and found the DESKTOP strip still showing `d1,
// serial:COM4, d1, …`, because the desktop renders its own tabs and the fix had been written into the
// panel's renderer instead of into a shared place. That is the same mistake the shell's control group
// records ("a per-density copy is how the two drifted before, R131"), made one round after quoting it.
//
// THE FIRST KEEPS THE BARE LABEL and repeats take a counter. A NUMBER because that is the only information
// the row honestly has — a pty session's label is its shell, its sid is opaque, and two PowerShell sessions
// genuinely ARE interchangeable until you look inside them.
//
// ── AND THE COUNTER LEADS, BECAUSE THE ELLIPSIS CUTS THE TAIL (2026-09-28) ────────────────────────────
//
// It was a SUFFIX — `pwsh`, `pwsh 2`, `pwsh 3` — for as long as it existed, and both strips render a label
// through `text-overflow: ellipsis`, which removes exactly the end of the string. So the one token that made
// two tabs different was the one token the renderer was guaranteed to delete first, and the disambiguation
// this file exists to provide was silently defeated whenever a label met its cap.
//
// MEASURED ON THE OPERATOR'S OWN SCREEN (their screenshot, and the strip reproduced here in a real browser):
// two ssh sessions to `stc@192.168.1.1` render as `stc@192.168.1.1` and `stc@192.168.1.1…` — the ` 2` is
// gone, and two tabs that a person must tell apart (an AI drives whichever is focused) read as one. **It is
// not a truncation defect: truncation is fine. It is that the truncation removed the DIFFERENCE.**
//
// THE MARGIN WAS 2.7px, WHICH IS WHY IT SURVIVED SO LONG. `.dtab-name` caps at 112px; `stc@192.168.1.1 2`
// needs 109.3px in this box's fallback font and 111px at the active tab's semibold weight. A font a few
// percent wider — the operator's Chinese Windows resolves `--font` to Microsoft YaHei, which is listed
// before `Segoe UI` — crosses the cap, and whether two tabs can be told apart stops being a property of the
// panel and becomes a property of the machine it is opened on. **A distinguisher that survives only in the
// font it was measured in is not a distinguisher.**
//
// A PREFIX IS IMMUNE TO THE MECHANISM, not to this instance of it: whatever the cap, whatever the font,
// whatever the label, the ellipsis can only ever eat the part that is NOT the counter. It also fixes the
// case this file was never asked about — two labels that differ only in their tail (`stc@192.168.1.100`
// and `stc@192.168.1.101`, which truncate to the same string and were never duplicates by this function's
// own test) — because the prefix differs before any truncation happens.
//
// `·` IS THIS REPOSITORY'S SEPARATOR (the status strip, the archive row, the memory card all use it), so
// the counter reads as a MARK ON the label rather than as part of the name: `2·stc@192.168.1.1`.
export function disambiguateLabels<T extends { label: string }>(items: T[]): string[] {
  const seen = new Map<string, number>();
  return items.map((item) => {
    const n = (seen.get(item.label) ?? 0) + 1;
    seen.set(item.label, n);
    return n === 1 ? item.label : `${n}·${item.label}`;
  });
}
