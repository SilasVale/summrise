// THE DESIGN SCALE, PINNED AGAINST THE BUILT STYLESHEET.
//
// WHY. An audit of the real pages (device Chrome, 6 pages × 2 themes) counted what the CSS was
// actually doing: 12 distinct `border-radius` literals while the token file named three (999px
// alone 51 times, 6px 18, 8px 17, and 10px 13 — that last one WAS `--radius-sm`, written out by
// hand), gaps at 2/4/5/6/7/8/10/12 with four of those off any scale, and 73 distinct
// padding/margin values in total. The values are now named tokens (`--radius-*`, `--sp-*`), and
// this file is what keeps them named: a new rule that writes `gap: 7px` fails here instead of
// quietly starting a second scale.
//
// IT READS THE BUILT CSS, like the contrast contract — the artefact the agent embeds, not the
// sources a reader might be looking at.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

function builtCss(): string {
  return readFileSync(
    path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "panel", "panel.css"),
    "utf8",
  );
}

/** Declarations of one top-level block, by exact selector (the same helper the contrast contract
 *  uses; two copies exist because the two files measure different things and neither imports test
 *  code from the other). */
function blockOf(css: string, selector: string): string {
  // ANCHORED AT A LINE START. Unanchored, `.dtab-close` also matches inside
  // `button.dtab-close { … }` — and that earlier rule has no width, so the assertion failed
  // against a block nobody was asking about.
  const m = css.match(
    new RegExp(
      "(?:^|\\n)" + selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + "\\s*\\{([\\s\\S]*?)\\n\\}",
    ),
  );
  expect(m, `block ${selector} not found in built panel.css`).not.toBeNull();
  return m![1];
}

/** Every value written for one property, as the raw text. */
function valuesOf(css: string, prop: string): string[] {
  return [...css.matchAll(new RegExp(`(?:^|[;{\\s])${prop}:\\s*([^;}]+)`, "g"))].map((m) =>
    m[1].trim(),
  );
}

describe("the design scale", () => {
  it("every border-radius is a token, a percentage or an exempted SHAPE", () => {
    const css = builtCss();
    // A rotated square's radius is a shape, not a corner style; the brand SVG's is a percentage.
    // Exempted BY SELECTOR so a future stray `2px` cannot ride along on the exemption.
    const SHAPE_EXEMPT = [
      '.rail-dot[data-state="waiting"]',
      '.desktop-rail-status[data-state="waiting"] .dot',
      '.rail-brand svg',
      '.desktop-rail-brand svg',
    ];
    const blocks = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => ({
      sel: m[1].trim().split("\n").pop()!.trim(),
      body: m[2],
    }));
    const offenders: string[] = [];
    let seen = 0;
    for (const b of blocks) {
      const m = /(?:^|[;{\s])border-radius:\s*([^;}]+)/.exec(b.body);
      if (!m) continue;
      seen++;
      if (SHAPE_EXEMPT.some((e) => b.sel.includes(e))) continue;
      const value = m[1].trim();
      // EVERY length in the value must be a token; percentages are allowed (circles), 0 is nothing.
      const bad = [...value.matchAll(/(\d+(?:\.\d+)?)px/g)].map((x) => x[0]);
      if (bad.length || !/^(var\(--radius(-\w+)?\)|\d+(\.\d+)?%|0)(\s+(var\(--radius(-\w+)?\)|\d+(\.\d+)?%|0))*$/.test(value)) {
        offenders.push(`${b.sel} { border-radius: ${value} }`);
      }
    }
    expect(seen, "the sweep should find radius declarations at all").toBeGreaterThan(50);
    expect(
      offenders,
      "a radius literal starts a second scale — use --radius-xs|sm2|sm|(default)|lg|pill, or a % for a circle",
    ).toEqual([]);
  });

  it("every gap is a spacing token or zero", () => {
    const css = builtCss();
    const values = [
      ...valuesOf(css, "gap"),
      ...valuesOf(css, "row-gap"),
      ...valuesOf(css, "column-gap"),
    ];
    expect(values.length, "the sweep should find gap declarations at all").toBeGreaterThan(20);
    const offenders = values.filter((v) => {
      const bad = [...v.matchAll(/(\d+(?:\.\d+)?)px/g)].map((x) => x[0]);
      return bad.length > 0;
    });
    expect(
      offenders,
      "an off-scale gap is how the 73 distinct spacing values happened — use --sp-0-5|1|2|3|4|5",
    ).toEqual([]);
  });

  it("the strips that hold many sessions keep their scrollbar and a hittable close", () => {
    // MEASURED on the built panel with the operator's own 16 sessions: `#tabs` is 211px wide for
    // 1777px of tabs, and the desktop strip 911px for 1462px — two and four tabs off-screen. The
    // desktop explicitly hid the scrollbar (`scrollbar-width: none` + a hidden webkit bar), so
    // nothing said the rest existed; and every close/export control measured 12x15 (panel) and
    // 15x15 (desktop), under the 24x24 a pointer target needs (WCAG 2.5.8).
    // COMMENTS STRIPPED FIRST: the block I wrote explains the defect it fixes and therefore
    // CONTAINS the string the assertion forbids ("must not contain scrollbar-width: none") — a
    // test that judges prose instead of declarations, which is how this failed the first time.
    const css = builtCss().replace(/\/\*[\s\S]*?\*\//g, "");
    for (const sel of ["#tabs", ".desktop-tabs"]) {
      const block = blockOf(css, sel);
      expect(block, `${sel} must keep a scrollbar: it is the only sign that sessions are hidden`)
        .toContain("scrollbar-width: thin");
      expect(block, `${sel} must not hide its scrollbar`).not.toContain("scrollbar-width: none");
    }
    // No rule anywhere may hide the webkit scrollbar of a strip that overflows.
    expect(css, "a hidden webkit scrollbar is the same defect in another engine").not.toMatch(
      /\.desktop-tabs::-webkit-scrollbar\s*\{\s*display:\s*none/,
    );
    // The panel's close and export controls share ONE declaration (`.tab-export, .tab-close`), so
    // there is no lone `.tab-close` block to find — the shared rule is asserted below instead.
    for (const sel of [".dtab-close"]) {
      const block = blockOf(css, sel);
      const w = /width:\s*(\d+)px/.exec(block);
      const h = /height:\s*(\d+)px/.exec(block);
      // Assert the MATCH first: `expect(w && Number(w[1]))` hands `null` to a numeric matcher and
      // the failure reads "received object" instead of naming what is missing.
      expect(w, `${sel} must declare a width`).not.toBeNull();
      expect(h, `${sel} must declare a height`).not.toBeNull();
      expect(Number(w![1]), `${sel} must be at least 24px wide`).toBeGreaterThanOrEqual(24);
      expect(Number(h![1]), `${sel} must be at least 24px tall`).toBeGreaterThanOrEqual(24);
    }
    // The export control shares the panel's rule, so it is covered by the same block.
    expect(blockOf(css, ".tab-export, .tab-close")).toContain("width: 24px");
  });

  it("the two device-notice strips share one vertical rhythm and hittable controls", () => {
    // Measured on the built panel (round 42), both densities: `.idle-bar` 37px tall with 5px/16px
    // padding and `.evicted-notice` 30px with 6px/16px — the same family of strip, two different
    // rhythms. It also carries the dismiss control every strip needs, at the 24px a pointer needs.
    const css = builtCss().replace(/\/\*[\s\S]*?\*\//g, "");
    for (const sel of [".idle-bar", ".evicted-notice"]) {
      const block = blockOf(css, sel);
      expect(block, `${sel} padding must come from the scale`).toMatch(/padding:\s*var\(--sp-[\w-]+\)/);
      expect(block, `${sel} gap must come from the scale`).toContain("gap: var(--sp-");
    }
    const x = blockOf(css, ".idle-x");
    expect(Number(/width:\s*(\d+)px/.exec(x)![1])).toBeGreaterThanOrEqual(24);
    expect(Number(/height:\s*(\d+)px/.exec(x)![1])).toBeGreaterThanOrEqual(24);
  });

  it("the approval card keeps a readable floor and never breaks a decision mid-phrase", () => {
    // MEASURED at a 900px viewport (round 46): the card shrank to ~190px, its buttons wrapped to
    // two and three lines ("Run / it", "Always / allow / vlan") and the note became a cramped
    // column — on the one surface where a person decides whether a command runs. `min-width: 0` was
    // the cause; `min(320px, 100%)` is the floor that cannot overflow.
    const css = builtCss().replace(/\/\*[\s\S]*?\*\//g, "");
    const card = blockOf(css, ".approval-prompt");
    expect(card, "the approval card must keep a width floor").toContain("min-width: min(320px, 100%)");
    const row = blockOf(css, ".approval-actions");
    expect(row, "the row must wrap as a group").toContain("flex-wrap: wrap");
    const button = blockOf(css, ".approval-actions button");
    expect(button, "a decision label must not break mid-phrase").toContain("white-space: nowrap");
  });

  it("the session-control rows wrap instead of squeezing their siblings", () => {
    // MEASURED at a 900px viewport: the approval card (now with a 320px floor) took its width and
    // the goal chip beside it collapsed to ~30px — the objective rendered one character per line.
    // Wrapping is the fix that keeps BOTH readable; ellipsising the goal would contradict the
    // decision .goal-text documents (an objective truncated to "provision the ONU on VL…" is not
    // something anyone can act on).
    const css = builtCss().replace(/\/\*[\s\S]*?\*\//g, "");
    for (const sel of ["#canvas-top", ".desktop-term-bar"]) {
      expect(blockOf(css, sel), `${sel} must wrap rather than squeeze`).toContain("flex-wrap: wrap");
    }
  });

  it("type sizes come from the scale, and none of them is a half pixel", () => {
    const css = builtCss();
    // Sizes appear both as `font-size:` and inside the `font:` shorthand; the shorthand is why
    // this looks for the number rather than for the property.
    const sizes = new Set<string>();
    for (const v of valuesOf(css, "font-size")) for (const m of v.matchAll(/(\d+(?:\.\d+)?)px/g)) sizes.add(m[1]);
    for (const v of valuesOf(css, "font")) for (const m of v.matchAll(/(\d+(?:\.\d+)?)px/g)) sizes.add(m[1]);
    const scale = new Set(["10", "11", "12", "13", "14", "17", "22"]);
    const offenders = [...sizes].filter((s) => !scale.has(s));
    expect(
      offenders,
      "a size outside --fs-2xs|xs|sm|base|md|lg|xl is drift; note 11.5 is deliberately NOT on the scale",
    ).toEqual([]);
    expect(sizes.has("11.5"), "11.5px folded into --fs-xs (11px) in the scale pass").toBe(false);
  });
});
