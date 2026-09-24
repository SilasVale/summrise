import { describe, it, expect } from "vitest";
import { pruneSessionViews } from "./sessionViews";

// THE VIEW RECORD USED TO GROW FOR THE LIFE OF THE PAGE (found 2026-09-24 by the panel exploration).
// `App`'s only writer ADDS a key, while the session list caps closed tombstones at 32 — so every
// session an operator ever opened a trajectory in stayed in the record after the session itself was
// dropped. These three properties are the whole contract, and the second is the one that keeps the
// effect from re-rendering `App` once per poll for ever.
describe("pruneSessionViews", () => {
  it("drops the views of sessions that are gone, and keeps the rest", () => {
    const views = { a: "terminal", b: "trajectory", c: "terminal" };
    expect(pruneSessionViews(views, ["a", "c"])).toEqual({
      a: "terminal",
      c: "terminal",
    });
  });

  it("returns THE SAME OBJECT when nothing was pruned — identity, not an equal copy", () => {
    const views = { a: "terminal", b: "trajectory" };
    // The effect depends on the session list, which changes on every poll. A fresh object here is a
    // re-render of App on every poll, for ever, which is why this is asserted by identity.
    expect(pruneSessionViews(views, ["a", "b"])).toBe(views);
    // …and the same when the record is empty or when it gains a session it has no view for.
    const empty = {};
    expect(pruneSessionViews(empty, [])).toBe(empty);
    expect(pruneSessionViews(views, ["a", "b", "new-session"])).toBe(views);
  });

  it("keeps a REVIVED session's view, because revival puts its sid back in the list", () => {
    // round-245 revives a tombstone whose sid reappears live; the prune has already run by then, so
    // what this pins is the ordering that matters: a sid in the list is never pruned, whatever
    // happened to it before.
    const views = { a: "trajectory" };
    expect(pruneSessionViews(views, ["a"])).toBe(views);
    expect(pruneSessionViews(views, [])).toEqual({});
  });
});
