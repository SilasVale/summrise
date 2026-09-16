// NO ORPHAN MODULES.
//
// Measured in round 74: 93 production modules under src/, and every one of them is imported by
// something. That is worth protecting, because the failure it prevents is silent — a module left
// behind by a refactor, or one whose only consumer was pruned (rounds 71-73 kept finding the
// leftovers of prunes: citations to documents that no longer exist). Nothing fails when a component
// stops being rendered; the bundle simply carries it, and a reader cannot tell it is dead.
//
// The two files that nothing imports are ENTRY POINTS, and their exemptions are EARNED rather than
// asserted: this test also checks that the thing which loads them still does. An exemption list that
// nothing verifies is how a dead module gets a permanent alibi.
import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SRC = path.dirname(path.dirname(fileURLToPath(import.meta.url))); // …/src
const PANEL = path.dirname(SRC); // …/panel-react

/** Every file under src/, recursively. */
function walk(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const full = path.join(dir, name);
    if (statSync(full).isDirectory()) walk(full, out);
    else out.push(full);
  }
  return out;
}

function productionModules(): string[] {
  return walk(SRC).filter((f) => {
    const base = path.basename(f);
    return (
      /\.(ts|tsx)$/.test(base) &&
      !base.includes(".test.") &&
      !base.endsWith(".d.ts")
    );
  });
}

function importedBy(target: string, all: string[]): string | null {
  const stem = path.basename(target).replace(/\.(ts|tsx)$/, "");
  const pat = new RegExp(
    `(?:from|import)\\s*\\(?\\s*["'\`][^"'\`]*/?${stem.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(?:\\.(?:ts|tsx|js|jsx))?["'\`]`,
  );
  for (const other of all) {
    if (other === target) continue;
    if (pat.test(readFileSync(other, "utf8"))) return other;
  }
  return null;
}

/** Files nothing imports because something other than an import loads them. The test below proves
 *  that "something" still exists, so these cannot become an alibi for a module nobody uses. */
const ENTRY_POINTS = ["main.tsx", "test-setup.ts"];

describe("module reachability", () => {
  it("every production module is imported by something, apart from the two entry points", () => {
    const all = walk(SRC);
    const orphans = productionModules()
      .filter((f) => importedBy(f, all) === null)
      .map((f) => path.relative(SRC, f))
      .filter((rel) => !ENTRY_POINTS.includes(rel))
      .sort();
    expect(orphans).toEqual([]);
  });

  it("the exemptions are EARNED: the bundle entry is declared, and the test setup is wired", () => {
    // Measured, not assumed: the panel has NO index.html (the Rust side serves the HTML and the
    // bundle separately), so an HTML check would have been a fiction. `main.tsx` is the LIBRARY
    // ENTRY declared in vite.config.ts — that is the thing that would have to change for it to
    // become an orphan.
    const vite = readFileSync(path.join(PANEL, "vite.config.ts"), "utf8");
    expect(vite, "vite.config.ts must declare src/main.tsx as the build entry").toMatch(
      /entry:\s*["']src\/main\.tsx["']/,
    );
    // test-setup.ts is wired by vitest.config.ts — a different file from vite.config.ts, which is
    // exactly why assuming the wiring is how this kind of exemption rots.
    const vitest = readFileSync(path.join(PANEL, "vitest.config.ts"), "utf8");
    expect(vitest, "vitest.config.ts must point at src/test-setup.ts").toMatch(/test-setup/);
  });
});
