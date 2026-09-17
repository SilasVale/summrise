// unstyledMarkup.test.ts — THE INVERSE OF THE DEAD-CSS RATCHET, and it exists because the first one cannot see
// this direction.
//
// WHY (round 248). `deadStyles.test.ts` asks "is there a rule no component can render?" — CSS that nothing uses.
// Nothing asked the opposite question: **is there a class in the markup that no rule matches?** Round 245 deleted
// `.tab-wait` while `DesktopShell` kept rendering it, so for a day the desktop strip showed a session holding a
// question as NOTHING AT ALL. The dead-CSS tool cannot see that (the rule was gone, so there was nothing to
// report), the unit tests did not assert that element, and only the browser sweep could have caught it — which is
// a long way from the line that broke it.
//
// IT IS A RATCHET, NOT A CLEAN SWEEP. Sixteen literal classes are unstyled today and they are listed rather than
// hidden: the count may fall and may not grow. Triage — a rule, a deletion, or a documented reason per entry — is
// work for whoever touches each surface, and this file's job is to stop the number rising while that happens.
//
// WHAT IT DELIBERATELY DOES NOT DO: judge ASSEMBLED class names. `className={`browser-action${cls}`}` cannot be
// resolved statically, and guessing would produce exactly the false positives that make a gate ignorable — the
// same caveat `prune-dead-css.py` carries in the other direction. Only double-quoted literals are read.
import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SRC = path.join(HERE, "..");
const BUILT_CSS = path.join(HERE, "..", "..", "..", "panel", "panel.css");

/** The frozen list. Each entry is a class rendered in markup that no rule in the built sheet matches. */
const KNOWN_UNSTYLED = [
  // TRIAGED (round 249): `tab-confirm`, `dtab-confirm` and `tab-confirm-hint` left this list by getting rules —
  // the two-step close had no layout of its own, so `close? [Close] [Cancel]` flowed inline inside a 30px strip.
  // TRIAGED (round 250): `settings-row-bar` and `settings-input-narrow` got the rules their names promised —
  // both were rendered as a bare <div> and a default-sized number field — and `settings-input` was DELETED from
  // the markup, because the sheet's generic input rule already styles every one of those elements and a class
  // that matches no rule is a name a reader has to check. Ten remain.
  "activity-row-dur", "archive-row-identity", "archive-row-kind", "device-logs-file", "health-stat",
  "monitor-expect", "monitor-fact", "path-step-run-label", "traj-ev-kind", "update-current",
];

function tsxFiles(dir: string, out: string[] = []): string[] {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, e.name);
    if (e.isDirectory()) tsxFiles(full, out);
    else if (e.name.endsWith(".tsx")) out.push(full);
  }
  return out;
}

describe("the markup's class names", () => {
  it("has no NEW class that the stylesheet cannot paint", () => {
    const css = readFileSync(BUILT_CSS, "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
    const defined = new Set([...css.matchAll(/\.(-?[_a-zA-Z][\w-]*)/g)].map((m) => m[1]));

    const used = new Map<string, string>();
    for (const file of tsxFiles(SRC)) {
      const src = readFileSync(file, "utf8");
      for (const m of src.matchAll(/className="([^"{}]+)"/g)) {
        for (const cls of m[1].trim().split(/\s+/)) {
          if (cls && !used.has(cls)) used.set(cls, path.relative(SRC, file));
        }
      }
    }

    // A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN: the panel's components carry hundreds of literal classes.
    expect(used.size, `only ${used.size} literal class names found — this test is reading the wrong thing`)
      .toBeGreaterThan(300);

    const unstyled = [...used.keys()].filter((c) => !defined.has(c)).sort();
    const grown = unstyled.filter((c) => !KNOWN_UNSTYLED.includes(c));
    expect(
      grown,
      `${grown.length} class name(s) are rendered but no rule matches them. Each is a mark the reader can see in` +
        ` the JSX and not on the screen:\n  ` +
        grown.map((c) => `${c}  (${used.get(c)})`).join("\n  ") +
        `\n\nGive it a rule, delete it from the markup, or add it to KNOWN_UNSTYLED with the reason it stays.`,
    ).toEqual([]);
  });
});
