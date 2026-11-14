"use strict";
// The tray's rows as data, so their wording, order and the version row can be tested without
// launching Electron. `main.ts` maps `action` to the one thing that row does; anything without an
// action is a label the poll fills in (version, health).
//
// THE VERSION ROW IS THE POINT: a tray that only says "Open" and "Quit" makes the running build
// invisible, and "is this the update I installed?" becomes a guessing game. The row is disabled on
// purpose — it is a fact, not a button.
function trayMenuRows(facts) {
  const version = facts && typeof facts.version === "string" ? facts.version : "";
  const health = facts && typeof facts.health === "string" ? facts.health : "";
  const rows = [
    { label: "Open", action: "open" },
    { type: "separator" },
    { label: "Open transcript", action: "transcript" },
    { label: "Open terminal", action: "terminal" },
    { type: "separator" },
    { label: version ? "Version " + version : "Version (not reported)", enabled: false },
    { label: "Check for updates\u2026", action: "update" },
    { type: "separator" },
    { label: health, enabled: false },
    { type: "separator" },
    { label: "Quit", action: "quit" },
  ];
  // An empty health line is a divider next to a divider; drop it and collapse the seam so the menu
  // never shows two separators in a row (or one at an end).
  const kept = rows.filter((row) => row.type === "separator" || row.label);
  const out = [];
  for (const row of kept) {
    if (row.type === "separator" && (out.length === 0 || out[out.length - 1].type === "separator")) continue;
    out.push(row);
  }
  while (out.length > 0 && out[out.length - 1].type === "separator") out.pop();
  return out;
}
module.exports = { trayMenuRows };
