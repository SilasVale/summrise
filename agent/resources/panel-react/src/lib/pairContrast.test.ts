// EVERY colour/background pair the sheet declares IN ONE RULE, checked against AA in both themes.
//
// WHY THIS EXISTS (round 43). Three rounds in a row found the SAME defect shape, one call site at a
// time: `--muted` is fine on white (4.83) and on --bg (4.63) and fails on the softer surfaces, so it
// kept appearing wherever a quiet token met a soft wash — the trajctory badge (4.23), then the
// session offer's action button (4.40). Fixing instances teaches nothing; this checks the CLASS:
//
//   a rule that declares both `color` and `background` states its own pair, so it can be measured
//   without rendering anything — for light and for dark — and a pair under 4.5 fails here.
//
// It uses the agent's tested contrast maths rather than a second implementation (see
// agent/scripts/lib/contrast-probe.mjs, whose own suite pins the worst-gradient and alpha rules).
//
// WHAT IT CANNOT SEE, stated so nobody trusts it further than it goes: a rule that sets only a
// colour and INHERITS its background (the majority), a background that comes from a gradient, and
// anything an image or pseudo-element paints behind the text. Those need the rendered sweep on a
// device (agent/scripts/panel-render-audit.mjs), which is where all three instances above were
// actually found.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
// The `.d.ts` beside this folder declares the module's shape to TypeScript.
import {
  aaThreshold,
  compositeStack,
  contrastRatio,
  parseColour,
} from "../../../../scripts/lib/contrast-probe.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));

function builtCss(): string {
  return readFileSync(path.join(HERE, "..", "..", "..", "panel", "panel.css"), "utf8");
}

/** The custom properties a theme block declares: `--name: value`. */
function tokensIn(css: string, selector: string): Record<string, string> {
  const m = css.match(
    new RegExp(selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + "\\s*\\{([\\s\\S]*?)\\n\\}"),
  );
  expect(m, `block ${selector} not found in built panel.css`).not.toBeNull();
  const out: Record<string, string> = {};
  for (const decl of m![1].matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) out[decl[1]] = decl[2].trim();
  return out;
}

/** Resolve `var(--x)` (one level — the stylesheet nests no deeper) against a theme's tokens. */
function resolve(value: string, tokens: Record<string, string>): string | null {
  const v = value.trim();
  const varMatch = /^var\((--[\w-]+)\s*(?:,\s*([^)]+))?\)$/.exec(v);
  if (varMatch) {
    const token = tokens[varMatch[1]];
    if (token) return resolve(token, tokens);
    return varMatch[2] ? resolve(varMatch[2], tokens) : null;
  }
  // A naked value (a hex, rgb(), a named colour) is used as written.
  return v;
}

interface Rule {
  selector: string;
  color: string;
  background: string | null;
}

/** Every rule in the sheet that declares a colour, with its background when it declares one. */
function colourRules(css: string): Rule[] {
  const bare = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const out: Rule[] = [];
  for (const m of bare.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selector = m[1].trim();
    const body = m[2];
    if (selector.startsWith("@")) continue; // at-rule bodies hold rules, not declarations
    const colour = /(?:^|;)\s*color\s*:\s*([^;]+);/.exec(body);
    const bg = /(?:^|;)\s*background(?:-color)?\s*:\s*([^;]+);/.exec(body);
    if (!colour) continue;
    const value: string = colour[1];
    out.push({ selector, color: value, background: bg ? bg[1] : null });
  }
  return out;
}

describe("colour pairs declared in one rule", () => {
  /** Rules whose declared pair this STATIC view CANNOT judge, each with the reason and the number.
   *  These are not "probably fine": every one is either non-text (a different minimum applies) or
   *  drawn over something only the renderer knows. Anything not listed must pass. */
  const NOT_JUDGEABLE: Record<string, string> = {
    ".rail-btn.active":
      "the rail's GLYPH, not text: WCAG 1.4.11 applies (3.0), and it measures 3.83 light / 4.30 dark",
    ".desktop-rail-btn.active": "same glyph, other density",
    ".browser-ev-time":
      "white on a 50% black scrim over a screenshot; the base is the image, which a static read cannot see",
    ".browser-action-badge.ok": "coloured text on a wash drawn over a screenshot thumbnail",
    ".browser-action-badge.err": "same: wash over a thumbnail",
    ".browser-action-badge.run": "same: wash over a thumbnail",
    ".browser-ev-toggle.active":
      "a chip inside the evidence drawer, over the embedded browser surface rather than the page background",
  };

  it("every one of them clears AA in both themes", () => {
    const css = builtCss();
    const themes: Record<string, Record<string, string>> = {
      light: tokensIn(css, ":root"),
      dark: tokensIn(css, 'body[data-theme="dark"]'),
    };
    const failures: string[] = [];
    let checked = 0;

    for (const rule of colourRules(css)) {
      if (!rule.background) continue; // inherits its surface — the rendered sweep covers those
      // INACTIVE CONTROLS ARE EXEMPT from the contrast minimum (WCAG 1.4.3 excludes them), and the
      // sheet expresses that state with opacity rather than a dimmer token.
      if (/:disabled|\[disabled\]|\[aria-disabled/.test(rule.selector)) continue;
      if (NOT_JUDGEABLE[rule.selector]) continue;
      // A background that is not a flat colour (a gradient, an image) cannot be measured here.
      if (/gradient|url\(/.test(rule.background)) continue;

      for (const [theme, tokens] of Object.entries(themes)) {
        const fgRaw = resolve(rule.color, tokens);
        const bgRaw = resolve(rule.background, tokens);
        if (!fgRaw || !bgRaw) continue; // a dynamic value (currentColor, inherit) — not measurable
        const fg = parseColour(fgRaw);
        const bg = parseColour(bgRaw);
        if (!fg || !bg) continue;
        // A TRANSLUCENT SURFACE IS NOT A COLOUR UNTIL IT IS COMPOSITED. `--surface-chip` in dark is
        // rgba(255,255,255,0.07) and `--state-muted-soft` is rgba(127,127,127,0.12); measured raw,
        // this sweep reported .cmd-badge at 1.93 where the device measures 6.76 — 80 "failures" that
        // were one alpha bug. The in-page probe composites the ancestor stack; here the base is the
        // theme's page background, which is what a chip sits on.
        const base = parseColour(tokens["--bg"] ?? "#ffffff") ?? { r: 255, g: 255, b: 255 };
        const composited = bg.a !== undefined && bg.a < 1 ? compositeStack([bg], base) : bg;
        checked++;
        const ratio = contrastRatio(fg, composited);
        const need = aaThreshold(12, 400); // the sheet's small-text floor: 4.5
        if (ratio < need) {
          failures.push(
            `${rule.selector} [${theme}] ${rule.color} on ${rule.background} = ${ratio.toFixed(2)} (needs ${need})`,
          );
        }
      }
    }

    // A sweep that read nothing is not a sweep that found nothing: this floor is the measured count
    // (36 pairs across two themes) minus room for the allowlist, so a broken resolver fails loudly.
    expect(checked, "the sweep must actually measure something").toBeGreaterThan(24);
    expect(Object.keys(NOT_JUDGEABLE).length, "every exemption needs its reason").toBe(7);
    expect(
      failures,
      `${failures.length} colour pair(s) under AA — pick the next step up in the same family ` +
        `(the quiet tokens are quiet on purpose; --chrome-ink-dim passes on every surface):\n  ` +
        failures.join("\n  "),
    ).toEqual([]);
  });
});
