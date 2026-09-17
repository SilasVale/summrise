// The approval gate's two settled/urgent surfaces and the waiting badges —
// read from the BUILT stylesheet, the bytes the agent embeds via include_str!.
// A hand-edit of panel.css that diverges from src/styles/ must not pass, and
// neither must a "small cleanup" that quietly turns the retired row back into a
// live-looking prompt or makes a waiting mark differ from its neighbours only
// by animation (the incident this repo already has on record).
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

function builtCss(): string {
  return readFileSync(
    path.join(
      path.dirname(fileURLToPath(import.meta.url)),
      "..", "..", "..", "..", "panel", "panel.css",
    ),
    "utf8",
  );
}

function escapeRe(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Exact selector's declaration block, or null when absent (so a renamed
 *  selector fails loudly instead of silently comparing "" to ""). */
function blockOf(css: string, selector: string): string | null {
  const m = css.match(new RegExp(escapeRe(selector) + "\\s*\\{([^}]*)\\}"));
  return m ? m[1] : null;
}

/** Everything a user with `prefers-reduced-motion` still sees. */
function withoutAnimation(block: string): string[] {
  return block
    .split(";")
    .map((d) => d.trim())
    .filter((d) => d && !/^animation(-duration|-name)?\s*:/.test(d))
    .sort();
}

function expectNoRawHex(block: string, selector: string): void {
  const hex = block.match(/#[0-9a-fA-F]{3,8}\b/g);
  expect(hex, `${selector} hardcodes ${hex?.join(", ")} — use a token`).toBeNull();
}

/** The gate's own source: a class name in the stylesheet proves the rule exists, not that the
 *  markup uses it (the round-27 lesson about helpers versus call sites). */
function ApprovalGateSource(): string {
  return readFileSync(
    path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "components", "ApprovalGate.tsx"),
    "utf8",
  );
}

describe("the retired question (0 s)", () => {
  it("is demoted to a quiet chip, NOT the live prompt's warning treatment", () => {
    const css = builtCss();
    const expired = blockOf(css, ".approval-expired");
    expect(expired, ".approval-expired missing from the built panel.css").not.toBeNull();
    expect(expired!).toContain("var(--surface-chip)");
    expect(expired!).toMatch(/border:\s*1px solid var\(--line\)/);
    // --chrome-ink-dim, not --muted: --muted measured 4.40 on this surface
    // (the repo's own recorded near miss on the count chips).
    expect(expired!).toContain("color: var(--chrome-ink-dim)");
    // The warning wash belongs to a LIVE question. On a settled row it would
    // read as "still your move" — the exact misreading the row exists to stop.
    expect(expired!).not.toContain("--warn");
    expectNoRawHex(expired!, ".approval-expired");

    // ...and the live prompt keeps the treatment that was taken away.
    const prompt = blockOf(css, ".approval-prompt")!;
    expect(prompt).toContain("var(--warn)");
    expect(prompt).toContain("var(--warn-soft)");
  });
});

describe("the gate's screen-reader plumbing", () => {
  it("has a clipped (not display:none) utility for the static description", () => {
    // display:none would drop the text out of the accessibility tree entirely,
    // so the alertdialog's aria-describedby would resolve to nothing.
    // The utility is GENERAL now (round 34): it was named after this one feature while three pages
    // had no accessible name at all, so it lives in base.css as `.sr-only` and the gate uses it.
    const sr = blockOf(builtCss(), ".sr-only");
    expect(sr, ".sr-only missing — a display:none node is dropped by screen readers").not.toBeNull();
    expect(sr!).toMatch(/position:\s*absolute/);
    expect(sr!).toMatch(/clip:\s*rect\(0 0 0 0\)/);
    // …and the gate must actually use it, or the description resolves to nothing.
    expect(ApprovalGateSource(), "the gate must use the shared utility").toContain('className="sr-only"');
    expect(sr!).not.toMatch(/display:\s*none/);
  });

  it("says the LAST MINUTE in words, in the readable danger weight", () => {
    const urgent = blockOf(builtCss(), ".approval-urgent");
    expect(urgent, ".approval-urgent missing — the urgent state would be colour-only").not.toBeNull();
    expect(urgent!).toContain("var(--danger-on-soft)");
    // Never animation alone: a state whose only difference is motion vanishes
    // for anyone with prefers-reduced-motion.
    expect(urgent!).not.toMatch(/animation\s*:/);
  });
});

describe("waiting badges", () => {
  it("`off` is distinguished by the DASH, not by fading", () => {
    // THE ONE SILHOUETTE NOTHING HAD EVER PHOTOGRAPHED. A tombstone exists only after an operator presses the tab's
    // × and confirms, so the page sweep — which photographs pages and never presses — has never seen it, and it is
    // the state whose whole design is a judgement call: `off` is a DASHED ring rather than a dimmed one, because a
    // fade is what drops a mark under the 3:1 a graphic needs against the chrome.
    // Verified on the rendered panel first (round 10 of the standing goal): border-style dashed, transparent fill,
    // 1px, ink on the settled chrome 3.78:1 — and the tab's own background takes ~195ms to settle out of the active
    // fill, which is why a probe that measures immediately reads the transition instead of the state.
    const css = builtCss();
    const off = blockOf(css, '.mark[data-live="off"]');
    expect(off, '.mark[data-live="off"] missing').not.toBeNull();
    expect(off!).toMatch(/border:\s*[\d.]+px\s+dashed/);
    expect(off!).toContain("var(--mark-ink)");
    // NOT a fill: a filled `off` would read as `working` at a glance, which is the one confusion this state must
    // never cause — a closed session is not a busy one.
    expect(off!).toMatch(/background:\s*transparent/);
    // AND NOBODY ELSE IS DASHED. If another state picks up a dash, two states share a silhouette and `off` stops
    // being distinguishable with the colour stripped out — which is the whole claim of the mark language.
    for (const other of ["idle", "working", "waiting"]) {
      const block = blockOf(css, `.mark[data-live="${other}"]`);
      expect(block, `.mark[data-live="${other}"] missing`).not.toBeNull();
      expect(block!, `${other} must not be dashed as well`).not.toMatch(/dashed/);
    }
  });

  it("the waiting mark is a SHAPE, not just another colour — in every place that draws one", () => {
    const css = builtCss();
    // ONE RULE DRAWS THE MARK NOW. Three tests used to live here — the panel dot, the desktop rail's dot and a
    // second element in the tab strip — and each asserted its own copy of the diamond. They were asserting the
    // same design three times, which is also how three copies come to disagree. `lib/liveness.ts` decides the
    // state and `.mark[data-live=…]` draws it, so this is one assertion about the one rule every place uses.
    const waiting = blockOf(css, '.mark[data-live="waiting"]');
    const working = blockOf(css, '.mark[data-live="working"]');
    const idle = blockOf(css, '.mark[data-live="idle"]');
    const off = blockOf(css, '.mark[data-live="off"]');
    for (const [name, block] of [["waiting", waiting], ["working", working], ["idle", idle], ["off", off]] as const) {
      expect(block, `.mark[data-live="${name}"] missing — a state with no silhouette is a state nobody can see`)
        .not.toBeNull();
    }
    // The COLOUR is --warn-ink, not the --state-warn fill this used to pin: the fill measures 2.63 on the dark
    // card against the 3:1 a mark needs (round 126). Colour alone cannot carry waiting-vs-working — at this size
    // they share a band — so the shape is asserted WITH it.
    expect(waiting!).toContain("var(--warn-ink)");
    expect(waiting!).toMatch(/rotate\(45deg\)/);
    // FOUR STATES, FOUR SILHOUETTES, with animation removed: a user with prefers-reduced-motion must still be
    // able to tell them apart. This is the whole claim of the mark language, in one line.
    const shapes = [waiting!, working!, idle!, off!].map(withoutAnimation);
    expect(new Set(shapes).size, `two states draw the same shape:\n${shapes.join("\n---\n")}`).toBe(4);
    expectNoRawHex(waiting!, '.mark[data-live="waiting"]');
  });

  it("the tab's mark carries the STATE, and the lane rides along as its ink", () => {
    const css = builtCss();
    // The lane names the ink instead of painting the dot — one mark per session with two channels, which is
    // what lets a waiting tab say so without a second element.
    const lane = blockOf(css, ".tab-dot");
    expect(lane, ".tab-dot missing").not.toBeNull();
    expect(lane!).toContain("--mark-ink");
    expect(blockOf(css, '.tab-dot[data-kind="ssh"]')).toContain("var(--lane-ds)");
    expect(blockOf(css, '.tab-dot[data-kind="serial"]')).toContain("var(--lane-or)");
    // AND THE SECOND ELEMENT IS GONE. `.tab-wait` used to sit beside the dot; if it comes back, a session
    // holding a question draws two marks again and the strip stops answering "which one needs me" at a glance.
    expect(blockOf(css, ".tab-wait"), ".tab-wait is back — the dot carries the state now").toBeNull();
  });

  it("the device-level chip uses the chip surface and a token mark", () => {
    const css = builtCss();
    const chip = blockOf(css, ".waiting-chip");
    const mark = blockOf(css, ".waiting-mark");
    expect(chip, ".waiting-chip missing").not.toBeNull();
    expect(mark, ".waiting-mark missing").not.toBeNull();
    expect(chip!).toContain("var(--surface-chip)");
    expect(chip!).toContain("color: var(--chrome-ink-dim)");
    expect(mark!).toContain("var(--warn-ink)");
    expect(mark!).toMatch(/rotate\(45deg\)/);
    expectNoRawHex(chip!, ".waiting-chip");
    expectNoRawHex(mark!, ".waiting-mark");
  });
});
