// THE DEAD-STYLE RATCHET.
//
// WHY. The panel's stylesheet is 4k lines and nothing had ever checked it for rules no component can
// render — measured, 22 classes and 148 declaration lines were dead, some of them large leftovers of
// the browser pane that the embedded view replaced. They are gone now; this keeps them from coming
// back, because dead CSS is not free: it is what a reader has to search through to find the rule
// that actually applies.
//
// THE TEST CALLS THE TOOL INSTEAD OF REIMPLEMENTING IT. `scripts/prune-dead-css.py` owns the
// question "can this be pruned?" — including the part that is easy to get wrong: a class name can be
// ASSEMBLED at runtime (`className={`browser-action${cls}`}`, `notify-state is-${permission}`), so
// "the literal never appears" is not evidence of death. A reimplementation here would drift from the
// tool that does the pruning; a caller cannot.
import { describe, it, expect } from "vitest";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

describe("the stylesheet", () => {
  it("has nothing left that no component can render", () => {
    const out = execFileSync("python3", [path.join(ROOT, "scripts", "prune-dead-css.py"), "--json"], {
      cwd: ROOT,
      encoding: "utf8",
    });
    // The tool prints a human summary first and the JSON last; take the last JSON line.
    const line = out
      .split("\n")
      .reverse()
      .find((l) => l.trim().startsWith("{"));
    expect(line, `no JSON from the pruner:\n${out}`).toBeTruthy();
    const report = JSON.parse(line!) as { wouldRemove: string[]; rules: number; lines: number };
    // A NEW dead class fails here with the command that fixes it, rather than being discovered by
    // whoever next reads 4k lines of CSS.
    expect(
      report.wouldRemove,
      `dead CSS — run: python3 scripts/prune-dead-css.py --write`,
    ).toEqual([]);
    expect(report.rules).toBe(0);
  });
});
