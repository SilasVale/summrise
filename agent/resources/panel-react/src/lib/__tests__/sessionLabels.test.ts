// The session tab's distinguisher: two strips, one rule, and a counter that survives truncation.
//
// WHY THESE FIVE CASES AND NOT MORE. The function is nine lines of Rust now
// (`panel-logic/src/session_labels.rs`) and its whole behaviour is: a label that appears once stays
// bare, a label that appears more than once is numbered on EVERY member, and the counter LEADS the
// label because both strips render through `text-overflow: ellipsis` (which deletes the end of the
// string, i.e. exactly where a suffix would put the distinguisher).
//
// The exhaustive comparison lives in the commit that moved it — 31 corpus shapes, the deleted
// TypeScript against this crate, byte-identical, including `null`, `undefined`, a number and a
// missing property. What is pinned HERE is the behaviour a reader would have to be told, so that a
// future edit to the Rust cannot quietly change it.
import { describe, it, expect } from "vitest";
import { disambiguateLabels } from "../sessionLabels";

const s = (label: string) => ({ label, sid: `s-${label}` });

describe("disambiguateLabels", () => {
  it("returns a label that appears once unchanged", () => {
    expect(disambiguateLabels([s("pwsh"), s("bash")])).toEqual(["pwsh", "bash"]);
    expect(disambiguateLabels([])).toEqual([]);
  });

  it("numbers EVERY member of a colliding set, the first included", () => {
    // Not "the repeats get a mark": a reader who sees `2·` and no `1·` has to work out what the 2 is
    // the second OF. The mark is an ordinal over a set.
    expect(disambiguateLabels([s("pwsh"), s("pwsh"), s("pwsh")])).toEqual([
      "1·pwsh",
      "2·pwsh",
      "3·pwsh",
    ]);
  });

  it("leaves a lone label bare beside a colliding one", () => {
    expect(disambiguateLabels([s("pwsh"), s("zsh"), s("pwsh")])).toEqual(["1·pwsh", "zsh", "2·pwsh"]);
  });

  it("puts the counter BEFORE the label, where the ellipsis cannot reach it", () => {
    // The defect this ordering exists for: `stc@192.168.1.1 2` is 112.45px against a 112px cap, so
    // the ` 2` is the first thing `text-overflow: ellipsis` deletes — and the two tabs a person must
    // tell apart render as one. `.dtab-name` is the cap; `lib/sessionLabels.ts` carries the table.
    const [a, b] = disambiguateLabels([s("stc@192.168.1.1"), s("stc@192.168.1.1")]);
    expect(a.startsWith("1·")).toBe(true);
    expect(b.startsWith("2·")).toBe(true);
    // And the label itself is intact after the mark, which is the half `#1 ` (8.75px) could not buy.
    expect(a.slice(2)).toBe("stc@192.168.1.1");
  });

  it("counts the collision over the WHOLE list, not over a prefix of it", () => {
    // The two-pass rule: `counts` is a fact about the list, so a label whose duplicate appears LAST
    // is still numbered at position one. A one-pass version numbered by array position and left the
    // first member bare.
    expect(disambiguateLabels([s("pwsh"), s("zsh"), s("bash"), s("pwsh")])).toEqual([
      "1·pwsh",
      "zsh",
      "bash",
      "2·pwsh",
    ]);
  });
});
