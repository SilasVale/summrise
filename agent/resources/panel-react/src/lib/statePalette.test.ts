// ============================================================================
// Discrete state palette — the visual contract for the SIX AI/command states.
//
// WHY THIS FILE EXISTS. "running" and "ok" used to be the SAME colour
// (`var(--accent)`) and differed ONLY by the `cmd-pulse` animation. The panel
// also honours `prefers-reduced-motion: reduce`, which sets `animation: none`
// on .cmd-dot, .traj-ev-dot and .plug-dot. So for anyone with that setting —
// a standard accessibility preference, not an edge case — "the AI is still
// working" and "the AI has finished" rendered IDENTICALLY, in three separate
// components at once.
//
// The defect was invisible to every existing gate: the tests were green, the
// stylesheet was valid, and the two rules LOOKED different when read side by
// side (one had an animation).
//
// So the contract pinned here is deliberately narrow and mechanical:
//
//     two states that a user must be able to tell apart may not differ ONLY
//     by animation.
//
// That is exactly the property whose absence caused the bug, and it is the
// one thing a future edit is most likely to break — by "simplifying" two
// nearby rules back into one shared colour.
// ============================================================================

import { PATH_STATES } from "./path";
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** The built stylesheet — the artifact the agent embeds via include_str!.
 *  Read from the BUILD, like the tab-visibility pin: a hand-edit of panel.css
 *  that diverges from src/styles/ must not pass silently. */
function builtCss(): string {
  return readFileSync(
    path.join(
      path.dirname(fileURLToPath(import.meta.url)),
      "..", "..", "..", "panel", "panel.css",
    ),
    "utf8",
  );
}

function escapeRe(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Declaration block for an EXACT selector, as ONE ARM of a rule.
 *
 *  COMMA LISTS COUNT (round 25 of the standing goal). The vocabulary is written as one shared rule —
 *  `.cmd-dot[data-state="fail"], .traj-ev-dot[data-state="fail"], .plug-dot[data-state="error"] { … }` — because
 *  three families rendering one vocabulary is the point. This matcher required the selector to be followed
 *  immediately by `{`, so five existing contracts reported the rule MISSING and read a null body: a lookup that
 *  cannot see the shape the sheet is written in fails in the direction that looks like a real defect. It now
 *  matches the selector where it sits — at a rule boundary, possibly with an arm after it.
 */
function blockOf(css: string, selector: string): string | null {
  const m = css.match(new RegExp("(?:^|[}\\n,])\\s*" + escapeRe(selector) + "\\s*(?:,[^{]*)?\\{([^}]*)\\}"));
  return m ? m[1] : null;
}

/** Declarations that survive with animation switched off — i.e. everything a
 *  user with `prefers-reduced-motion` still sees. This is the whole point:
 *  if two states are identical here, they are identical for that user. */
function withoutAnimation(block: string): string[] {
  return block
    .split(";")
    .map((d) => d.trim())
    .filter((d) => d && !/^animation(-duration|-name)?\s*:/.test(d))
    .sort();
}

/** Each entry is one component's "in progress" state against its "settled
 *  successfully" state. All three carried the identical defect, so all three
 *  are pinned — fixing one and missing the others is how they drifted. */
const PAIRS: Array<{ component: string; active: string; done: string }> = [
  { component: "command card", active: ".cmd-dot", done: ".cmd-dot" },
  { component: "trajectory rail", active: ".traj-ev-dot", done: ".traj-ev-dot" },
  { component: "plugin page", active: ".plug-dot", done: ".plug-dot" },
];

/** State values differ per component: the plugin page uses
 *  ongoing/success, the other two use running/ok. */
const STATES: Record<string, { active: string; done: string }> = {
  ".cmd-dot": { active: "running", done: "ok" },
  ".traj-ev-dot": { active: "running", done: "ok" },
  ".plug-dot": { active: "ongoing", done: "success" },
};

describe("discrete state palette", () => {
  describe.each(PAIRS)("$component", ({ active }) => {
    const { active: aState, done: dState } = STATES[active];
    const selA = `${active}[data-state="${aState}"]`;
    const selD = `${active}[data-state="${dState}"]`;

    it(`${aState} and ${dState} differ WITHOUT relying on animation`, () => {
      const css = builtCss();
      const a = blockOf(css, selA);
      const d = blockOf(css, selD);

      // Absent rules must fail as themselves, not as "" vs "".
      expect(a, `${selA} missing from built panel.css`).not.toBeNull();
      expect(d, `${selD} missing from built panel.css`).not.toBeNull();

      const aStatic = withoutAnimation(a!);
      const dStatic = withoutAnimation(d!);

      expect(
        aStatic,
        `${selA} and ${selD} are IDENTICAL once animation is removed.\n` +
          `A user with prefers-reduced-motion cannot tell "${aState}" from ` +
          `"${dState}". Give them different COLOUR or different SHAPE — ` +
          `animation alone is not a state channel.`,
      ).not.toEqual(dStatic);
    });
  });

  it("the four VERDICT states agree across both renderers", () => {
    // The command stream and the trajectory rail are two renderers of one
    // vocabulary, so a verdict must not mean one colour in one place and
    // another colour in the other. `muted` is excluded on purpose — see the
    // next test for why its density differs.
    const css = builtCss();
    for (const s of ["running", "ok", "fail", "warn"]) {
      const a = blockOf(css, `.cmd-dot[data-state="${s}"]`);
      const b = blockOf(css, `.traj-ev-dot[data-state="${s}"]`);
      expect(a, `.cmd-dot ${s} missing`).not.toBeNull();
      expect(b, `.traj-ev-dot ${s} missing`).not.toBeNull();
      const colour = (block: string) =>
        (block.match(/background\s*:\s*([^;]+)/) || [, "<none>"])[1].trim();
      expect(colour(b!), `.traj-ev-dot ${s} disagrees with .cmd-dot ${s}`)
        .toBe(colour(a!));
    }
  });

  it("EVERY state has a visual channel in the BUILT sheet", () => {
    // THE TEST THAT WAS MISSING, and its absence is why round 31 shipped a state
    // with no dot. `bg` was added to `PathState`, returned by `cardState`, and
    // rendered as `data-state` — while `data-state="bg"` occurred ZERO times in
    // every stylesheet AND in the built artifact. The dot did not render
    // UNSTYLED, it rendered NOTHING: base `.cmd-dot` sets only size and
    // border-radius, so an unmatched state is an invisible 8px circle. The
    // round-31 tests asserted words and counts, which is exactly the layer where
    // the bug was not.
    //
    // Read the BUILT css (the tracked artifact the agent embeds), not src: a rule
    // that never made it through the build is not a channel.
    const css = builtCss();
    for (const c of [".cmd-dot", ".traj-ev-dot"]) {
      for (const state of PATH_STATES) {
        const b = blockOf(css, `${c}[data-state="${state}"]`);
        expect(
          b,
          `${c}[data-state="${state}"] has NO rule, so a step in that state \
           renders an invisible dot. Every PathState needs a channel — add it to \
           src/styles/components.css beside its siblings.`,
        ).not.toBeNull();
      }
    }
  });

  it("no two states collapse onto one colour in either renderer", () => {
    // Colour alone cannot carry five states in this palette (three of the five
    // hues share one red-orange band), which is why SHAPE carries part of the
    // difference too — but no two of the five may share a colour, or the shape
    // work would be doing all the lifting on its own.
    const css = builtCss();
    for (const c of [".cmd-dot", ".traj-ev-dot"]) {
      // THE LIST COMES FROM THE SOURCE, NOT FROM THIS TEST. It used to be a
      // hardcoded five names, so the state added in round 31 (`bg`) sat OUTSIDE
      // the guarantee this test exists to state — and it had no CSS rule at all
      // while this test passed. A contract that enumerates its own subjects
      // cannot notice a new one.
      const seen = PATH_STATES.map((s) => {
        const b = blockOf(css, `${c}[data-state="${s}"]`);
        expect(b, `${c}[data-state="${s}"] missing`).not.toBeNull();
        return (b!.match(/background\s*:\s*([^;]+)/) || [, "<none>"])[1].trim();
      });
      const painted = seen.filter((v) => v !== "transparent");
      expect(new Set(painted).size, `${c} duplicate state colours: ${seen.join(" | ")}`)
        .toBe(painted.length);
    }
  });

  it("muted reads as ABSENCE, tailored to each renderer's density", () => {
    // The one deliberate asymmetry, pinned so it stays a decision:
    //   command card — muted is rare (a command ended without a verdict), so
    //                  it is a HOLLOW ring: "nothing concluded here".
    //   trajectory   — muted is the MAJORITY (every raw output line), so it is
    //                  a quiet FILLED dot: texture, not a marker.
    const css = builtCss();
    const cardMuted = blockOf(css, `.cmd-dot[data-state="muted"]`)!;
    expect(cardMuted, "card muted should be unfilled").toContain("transparent");
    expect(cardMuted, "card muted should keep a visible outline")
      .toMatch(/box-shadow\s*:\s*inset/);

    const railMuted = blockOf(css, `.traj-ev-dot[data-state="muted"]`)!;
    expect(railMuted, "rail muted should be a quiet FILLED dot, not a ring")
      .not.toContain("transparent");
    // THE TOKEN CHANGED IN ROUND 33, THE RULE IT GUARDS DID NOT. This asserted `--ds-neutral` (the old "texture, not a
    // marker" fill) and it was right to until the rendered measurement said what that texture cost: rgb(212,212,216)
    // on the light surface is **1.42:1**, where a graphic needs 3 — an INVISIBLE dot, and an invisible dot carries no
    // state at all. `--state-muted` is the token the three sibling muted rings already use (4.83 light / 6.5 dark), so
    // the assertion is now the thing its own message always said: not a VERDICT colour. A neutral is still required,
    // and the check below names the three verdicts rather than one blessed token, so the next edit cannot swap
    // legibility for a hue that means something else.
    expect(railMuted, "rail muted must not borrow a verdict colour")
      .not.toMatch(/--(state-)?(ok|fail|warn)\b/);
    expect(railMuted, "rail muted should paint a neutral").toMatch(/--(state-)?muted|--ds-neutral/);
  });

  it("the reduced-motion premise still holds for every state dot", () => {
    // This test documents WHY the rule above is necessary. If a future edit
    // drops these selectors from the reduced-motion block, the premise
    // changes — and that should be a deliberate decision, not a silent one.
    const css = builtCss();
    // EVERY reduced-motion block, not the first one. Round 77 moved the main block to the END of the
    // cascade — a media query adds no specificity, so it only wins where it comes after the rules it
    // overrides, and the desktop sheet is concatenated last — which left this regex matching the one
    // small `.gs-card` block and failing. The PREMISE is what matters here ("the state dots are
    // covered"); which block covers them is the stylesheet's business, not this test's.
    const blocks = [...css.matchAll(/@media\s*\(prefers-reduced-motion:\s*reduce\)\s*\{([\s\S]*?)\n\}/g)].map((m) => m[1]);
    expect(blocks.length, "the prefers-reduced-motion block is gone").toBeGreaterThan(0);
    const media = blocks.join("\n");
    for (const sel of [".cmd-dot", ".traj-ev-dot", ".plug-dot"]) {
      expect(media, `${sel} must still honour reduced motion`).toContain(sel);
    }
    expect(media, "the blocks must actually disable animation").toMatch(/animation\s*:\s*none/);
  });
});

describe("the verdict vocabulary", () => {
  // THE TWO RENDERERS ARE ONE VOCABULARY, and the check that said so only compared COLOUR. Measured (round 25 of
  // the standing goal): `.cmd-dot[data-state="warn"]` carried a halo and `.traj-ev-dot[data-state="warn"]` did not,
  // and `ok` / `fail` / `warn` were three FILLS a reader had to know by heart — under `prefers-reduced-motion`
  // `running` lost its pulse and became indistinguishable from `warn`. The shapes carry the verdict now.
  const shapeOf = (css: string, family: string, state: string) => {
    // EVERY RULE THAT MATCHES, in sheet order, the way the cascade resolves them: a selector may be one arm of a
    // comma list shared by three families, which is exactly how the vocabulary is written.
    const rules = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => ({
      sels: m[1].trim().split(",").map((x) => x.trim()),
      body: m[2],
    }));
    // the two halves the specificity model needs: what the STATE says, and what the BASE says
    const arm = rules.filter((r) => r.sels.includes(`${family}[data-state="${state}"]`)).map((r) => r.body).join("\n");
    const baseBlock = rules.filter((r) => r.sels.includes(family)).map((r) => r.body).join("\n");
    // THE STATE'S OWN DECLARATIONS OVERRIDE THE BASE'S, which is what specificity does: `.plug-dot[data-state="warn"]`
    // is (0,2,0) and `.plug-dot` is (0,1,0), so the arm wins even though the base rule sits LATER in the sheet.
    // Taking the last textual declaration — my first attempt at this — read the base's `background: transparent`
    // over the arm's fill and reported every arm as a ring, hiding four live defects. A shape check that disagrees
    // with the cascade reports on a stylesheet nobody is looking at.
    const prop = (name: string) => {
      const from = (text: string) => {
        const m = new RegExp(`(?:^|[;{\\s])${name}\\s*:\\s*([^;}]+)`).exec(text);
        return m ? m[1].trim() : "";
      };
      return from(arm) || from(baseBlock);
    };
    const bg = prop("background") || prop("background-color");
    const shadow = prop("box-shadow");
    const filled = !!bg && !/transparent|none/.test(bg);
    // A MARK THAT IS BOTH A FILL AND A RING IS NEITHER, and it gets its own kind so it cannot hide (round 45 of the
    // standing goal). Until now `inset` won outright, so a rule that set a background and inherited an inset shadow
    // computed as `ring` — distinct from a solid, and therefore passing. The console's marks check found this by
    // mutation in round 44; the panel's copy of the same expression had the same hole, latent, because no panel
    // state does both today. One vocabulary, two checks, and now one rule about what a silhouette may be.
    const kind =
      /inset/.test(shadow) && filled ? "ring+fill" : /inset/.test(shadow) ? "ring" : filled && !!shadow && !/none/.test(shadow) ? "halo" : filled ? "solid" : "empty";
    return [prop("border-radius") || "0", /rotate/.test(prop("transform")) ? "rotated" : "upright", kind].join("|");
  };

  it("never draws a mark that is a FILL inside a RING", () => {
    // `ring+fill` IS NOT IN THE VOCABULARY: solid, ring, halo and empty are the four, and a mark that is two of them
    // is neither — a green fill inside a grey inset ring reads as "slightly thicker dot", which is how the panel's
    // `.plug-dot[error]` got a stray halo past a sheet-level check in round 25. Every state of every family that
    // carries `data-state`, so a new one cannot arrive already broken.
    const css = builtCss().replace(/\/\*[\s\S]*?\*\//g, "");
    const families = [".cmd-dot", ".traj-ev-dot", ".plug-dot"];
    // THE PLUGIN DOT USES ITS OWN NAMES for the same five states (`success`/`error`/`ongoing`), which is why this
    // list carries both vocabularies — and why the first version of this test reported only `warn` while four arms
    // were broken.
    const states = ["ok", "warn", "fail", "running", "muted", "success", "error", "ongoing"];
    const bad: string[] = [];
    for (const family of families) {
      for (const state of states) {
        const sig = shapeOf(css, family, state);
        if (sig.includes("ring+fill")) bad.push(`${family}[data-state="${state}"]`);
      }
    }
    expect(
      bad,
      `\n${bad.length} mark(s) are a fill inside a ring. Add \`box-shadow: none\` for a fill, or drop the background\n` +
        `for a ring — the vocabulary is solid / ring / halo / empty:\n  ${bad.join("\n  ")}\n`,
    ).toEqual([]);
  });

  it("gives every verdict its own shape, and gives both renderers the SAME one", () => {
    const css = builtCss().replace(/\/\*[\s\S]*?\*\//g, "");
    const STATES = ["ok", "warn", "fail", "running"];
    for (const s of STATES) {
      const a = shapeOf(css, ".cmd-dot", s);
      const b = shapeOf(css, ".traj-ev-dot", s);
      // `running` IS IN THIS LOOP ON PURPOSE, AND IT WAS DEFENDED THE HARD WAY (round 33). `eventDotState` cannot
      // return it today — every event goes through `stateFromEnd(true, …)` — so the rule looked unreachable and I
      // deleted it. Four tests here refused, and they are right: EVERY state in the vocabulary must have a channel in
      // EVERY renderer, so a state that becomes reachable later cannot draw an invisible dot. An unreachable STATE is
      // the deriver's fact; the CHANNEL is the sheet's obligation. (The mark-coverage note's "no surface rendered
      // running" is therefore true and is telling us about the deriver, not about a missing rule.)
      expect(a, `.traj-ev-dot[${s}] renders differently from .cmd-dot[${s}] — one vocabulary, two renderers`).toBe(b);
    }
    // `muted` IS EXCLUDED ON PURPOSE, and the next test pins why: in the command card it is rare and hollow, in the
    // trajectory it is the majority and a quiet fill. A density decision outranks uniformity, and a check that
    // insisted otherwise would be arguing with a documented choice rather than protecting one.
    const shapes = STATES.map((s) => [s, shapeOf(css, ".cmd-dot", s)] as const);
    for (const [s, sh] of shapes) expect(sh, `.cmd-dot[${s}] renders as nothing`).not.toMatch(/empty$/);
    expect(
      new Set(shapes.map(([, sh]) => sh)).size,
      `two verdicts share a shape — colour alone is not a state channel:\n  ${shapes.map(([s, sh]) => `${s}: ${sh}`).join("\n  ")}`,
    ).toBe(STATES.length);
  });
});
