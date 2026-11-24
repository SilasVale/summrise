"use strict";
// The tray's rows as data, so their wording, order and the version row can be tested without
// launching Electron. `main.ts` maps `action` to the one thing that row does; anything without an
// action is a label the poll fills in (version, health).
//
// TWO SURFACES, TWO WINDOWS: the two device pages (transcript = /activity, panel) and the two panes
// of the thin host window (Browser, Harness). There is no "Open" row: a row that means "focus
// whatever is already there" is the tray icon's own click, and the terminal is gone with the SPA.
//
// THE VERSION ROW IS THE POINT: a tray that only says "Browser" and "Quit" makes the running build
// invisible, and "is this the update I installed?" becomes a guessing game. The row is disabled on
// purpose — it is a fact, not a button.
function trayMenuRows(facts) {
  const version = facts && typeof facts.version === "string" ? facts.version : "";
  const health = facts && typeof facts.health === "string" ? facts.health : "";
  const rows = [
    { label: "Open transcript", action: "transcript" },
    { label: "Open panel", action: "panel" },
    { label: "Browser", action: "browser" },
    { label: "Harness", action: "harness" },
    // The installed Rust shell, window only: one explicit click, never a boot path. It is a FALLBACK
    // beside the panes, not a fifth surface — the label says "window only" so nobody expects the tray
    // the full shell used to own.
    { label: "Open fallback shell (window only)", action: "fallback" },
    { type: "separator" },
    { label: version ? "Version " + version : "Version (not reported)", enabled: false },
    { label: "Check for updates\u2026", action: "update" },
    { type: "separator" },
    { label: health, enabled: false },
    { type: "separator" },
    { label: "Quit Summrise", action: "quit" },
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
