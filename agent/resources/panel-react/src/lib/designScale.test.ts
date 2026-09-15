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
