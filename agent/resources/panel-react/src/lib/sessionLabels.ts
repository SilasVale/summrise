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
//
// ── THE WHOLE COLLISION IS NUMBERED, AND `·` WAS CHOSEN BY MEASUREMENT ────────────────────────────────
//
// A LONE `2` IS NOT A COUNTER. An independent review of the rendered strip made the point that numbering
// only the repeats makes the mark a MUTATION of one label rather than an ordinal over a set: a reader who
// sees `2·` and no `1·` has to work out what the 2 is the second OF. So a label that appears more than once
// is numbered on EVERY member, the first included; a label that appears once stays bare, because there is
// no set for it to be a member of.
//
// AND THE SEPARATOR IS NOT A TASTE CHOICE — IT IS THE ONLY ONE THAT FITS. The same review objected, fairly,
// that `·` is an identifier character, so `2·stc@192.168.1.1` can be misread as the username `2·stc`. Its
// preferred mark was `#`, which is already this repository's duplicate mark (`lib/runs.ts`). MEASURED IN THE
// DEVICE'S OWN FONT at the inactive tab's weight, against the 112px cap:
//
//     stc@192.168.1.1      100.98px   fits
//     stc@192.168.1.1 2    112.45px   OVER by 0.45 — this is the defect
//     2·stc@192.168.1.1    111.73px   fits, and so does 1·stc@192.168.1.1
//     1 stc@192.168.1.1    112.45px   OVER by 0.45
//     #1 stc@192.168.1.1   120.75px   OVER by 8.75
//     #1·stc@192.168.1.1   120.03px   OVER by 8.03
//
// `#1 ` costs 8.75px and puts BOTH labels over the cap, so it does not remove a truncation — it ADDS one,
// and the one it adds eats the tail of the address on the tab that reads perfectly well today. `·` is the
// narrowest mark available and the only form measured that keeps the whole address readable. The ambiguity
// the review named is real, and it is recorded here rather than argued away; what buys it back is the
// counter LEADING — a reader who mis-parses the mark still sees two different strings, which is the whole
// of what this function is for.
export function disambiguateLabels<T extends { label: string }>(items: T[]): string[] {
  // TWO PASSES, because "does this label collide" is a fact about the WHOLE list that the first pass cannot
  // know. The one-pass version numbered a repeat by its position in the array, which is why only the repeats
  // carried a mark at all.
  const counts = new Map<string, number>();
  for (const item of items) counts.set(item.label, (counts.get(item.label) ?? 0) + 1);
  const seen = new Map<string, number>();
  return items.map((item) => {
    const n = (seen.get(item.label) ?? 0) + 1;
    seen.set(item.label, n);
    return counts.get(item.label) === 1 ? item.label : `${n}·${item.label}`;
  });
}
