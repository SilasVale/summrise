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

/** The custom properties a theme block declares: `--name: value`.
 *
 *  COMMENTS ARE STRIPPED FIRST, and that is not tidiness. A comment that mentions a token in prose —
 *  "--surface: code/output panes, log bodies…" — is indistinguishable from a declaration to a naive
 *  pattern, and because the LAST match wins it silently replaced the real value: `--surface` resolved
 *  to that sentence, `parseColour` returned null, and a pair measured NaN. Every pair this file
 *  judges is judged from these values, so a prose line could change what the whole contract sees.
 *  (Found in round 86 while pinning `--danger-on-soft`; the same helper is the one the main sweep
 *  uses to resolve both sides of all 36 pairs.) */
function tokensIn(css: string, selector: string): Record<string, string> {
  css = css.replace(/\/\*[\s\S]*?\*\//g, "");
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
      "white on a scrim over a screenshot: judged by the WORST base rule instead (see the probe suite), worst case 5.74",
    // These three are NOT exempt any more: the earlier note claimed they sat over a screenshot
    // thumbnail, and that was simply wrong — `.browser-action` paints `--chrome-bg-3`, so the wash
    // composites over a chrome surface. Their base is named and measured like any other pair.
    // .monitor-chip.is-flapping is judged by the RENDERED sweep, not here, and that is a limitation
    // worth naming rather than papering over: its colour and its surface are declared in different
    // rules (the documented blind spot above), and both sides go through another var() — --state-warn
    // resolves to --warn, --surface-chip to --ds-neutral-100 — so a static pair here resolves to
    // nothing and is skipped. I added the entry, mutated the token back, and watched the test pass:
    // coverage that cannot fail. Removed, with the reason. The rendered sweep measures it on every run
    // because the harness fixture now produces a flapping target, and it measured 2.80 in dark before
    // round 109's fix and 9.13 after.
    ".browser-action-badge.ok": "--chrome-bg-3",
    ".browser-action-badge.err": "--chrome-bg-3",
    ".browser-action-badge.run": "--chrome-bg-3",
    ".browser-ev-toggle.active":
      "a chip inside the evidence drawer, over the embedded browser surface rather than the page background",
  };

  it("--danger-on-soft clears AA on every surface it lands on, including the ones it cannot reach", () => {
    // ROUND 86. The pair sweep above only judges rules that declare BOTH `color` and `background`.
    // `--danger-on-soft` is used as a TEXT colour in five rules and declares its own background in
    // one, so the other four were unchecked — and one of them, `#modal-status.error`, cannot be
    // RENDERED by the harness either: the connect modal only appears when the panel is disconnected,
    // and the fixture keeps it connected even with every API call failing. Measured here instead,
    // pair by pair, with the surfaces taken from the sheet:
    //
    //     modal surface     7.20 light / 7.26 dark      (the unreachable state)
    //     danger-soft wash  6.38 light / 7.58 dark      (composited over the theme's --bg)
    //     tab background    7.52 light / 7.01 dark      (.tab-close:hover)
    //
    // A state a harness cannot produce is not a reason to leave its colours unmeasured; it is a
    // reason to measure them somewhere else.
    const css = builtCss();
    const light = tokensIn(css, ":root");
    const dark = { ...light, ...tokensIn(css, 'body[data-theme="dark"]') };
    const cases = [
      ["#modal-status.error", "--danger-on-soft", "--surface"],
      [".tab-close:hover", "--danger-on-soft", "--chrome-bg-2"],
    ];
    let checked = 0;
    const THEMES: Array<[string, Record<string, string>]> = [["light", light], ["dark", dark]];
    for (const [label, fgName, bgName] of cases) {
      for (const [theme, tokens] of THEMES) {
        const fg = parseColour(tokens[fgName]);
        const bg = parseColour(tokens[bgName]);
        expect(fg, `${label}: ${fgName} must resolve`).toBeTruthy();
        expect(bg, `${label}: ${bgName} must resolve`).toBeTruthy();
        held(label, theme, fg!, bg!);
        checked++;
      }
    }
    // The wash is translucent, so it is composited over the theme's page background — the same rule
    // the main sweep uses, rather than a second reading of it.
    // `--bg` is itself `var(--ds-neutral-50)` in the light theme, so one level of var() is resolved
    // before compositing — an unresolved name parses to null and the maths would throw.
    const resolve = (tokens: Record<string, string>, name: string): string => {
      const raw = tokens[name] ?? "";
      const asVar = /^var\((--[a-z0-9-]+)\)$/.exec(raw.trim());
      return asVar ? (tokens[asVar[1]] ?? raw) : raw;
    };
    for (const [theme, tokens] of THEMES) {
      const fg = parseColour(resolve(tokens, "--danger-on-soft"));
      const wash = parseColour(resolve(tokens, "--danger-soft"));
      const page = parseColour(resolve(tokens, "--bg"));
      expect(wash && page, `${theme}: --danger-soft and --bg must both resolve`).toBeTruthy();
      const stack = compositeStack([wash!, page!]);
      held(`--danger-soft wash (${theme})`, theme, fg!, stack);
      checked++;
    }
    expect(checked, "this check must actually measure something").toBeGreaterThanOrEqual(6);

    function held(label: string, theme: string, fg: ReturnType<typeof parseColour>, bg: ReturnType<typeof parseColour>) {
      const ratio = contrastRatio(fg!, bg!);
      expect(
        ratio,
        `${label} (${theme}): ${ratio.toFixed(2)} — needs 4.5`,
      ).toBeGreaterThanOrEqual(4.5);
    }
  });

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
      const hint = NOT_JUDGEABLE[rule.selector];
      // A REASON means "this static view cannot judge it"; a TOKEN names the base to measure
      // against (the surface comes from a SIBLING class, which no single rule can see). A named
      // base is still measured — a wrong claim fails here, and the rendered sweep on a device is
      // the authority that would contradict it.
      const namedBase = hint && hint.startsWith("--") ? hint : null;
      if (hint && !namedBase) continue;
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
        const baseToken = namedBase ?? "--bg";
        const base = parseColour(tokens[baseToken] ?? "#ffffff") ?? { r: 255, g: 255, b: 255 };
        const composited = bg.a !== undefined && bg.a < 1 ? compositeStack([bg], base) : bg;
        checked++;
        const ratio = contrastRatio(fg, composited);
        const need = aaThreshold(12, 400); // the sheet's small-text floor: 4.5
        if (ratio < need) {
          failures.push(
            `${rule.selector} [${theme}] ${rule.color} on ${rule.background}` +
              `${namedBase ? ` over ${namedBase}` : ""} = ${ratio.toFixed(2)} (needs ${need})`,
          );
        }
      }
    }

    // A sweep that read nothing is not a sweep that found nothing: this floor is the measured count
    // (36 pairs across two themes) minus room for the allowlist, so a broken resolver fails loudly.
    expect(checked, "the sweep must actually measure something").toBeGreaterThan(24);
    // 7 entries: 4 named BASES (measured against them) and 3 reasons (irreducibly contextual).
    expect(Object.keys(NOT_JUDGEABLE).length, "every entry needs its reason or its base").toBe(7);
    expect(
      failures,
      `${failures.length} colour pair(s) under AA — pick the next step up in the same family ` +
        `(the quiet tokens are quiet on purpose; --chrome-ink-dim passes on every surface):\n  ` +
        failures.join("\n  "),
    ).toEqual([]);
  });

  // ── THE MARKS: A FILL IS A GRAPHIC, AND WCAG ASKS 3:1 OF IT (round 130) ────────────────────────
  //
  // WHY THIS EXISTS. Rounds 126-127 fixed ten marks across the panel — .monitor-mark, .waiting-mark,
  // .rail-dot[data-state=waiting], .cmd-dot[data-state=warn] and the three session dots — every one of
  // which had been painting itself with a FILL token (--state-warn, --accent) that measures 2.53-2.96
  // on the dark chrome. They were found by the rendered graphics probe and verified on the page, which
  // is the right way to FIND such a defect and the wrong way to KEEP it fixed: nothing in CI would
  // notice a regression, because the pair sweep above only judges rules that declare BOTH `color` and
  // `background`, and a mark declares only the second.
  //
  // So they are listed here with the surface each one actually lands on, judged at 3:1 — the same
  // manual-pair idiom round 86 introduced for #modal-status.error, and for the same reason: the sweep
  // cannot reach these, and "the sweep cannot reach it" is not a reason to leave a colour unmeasured.
  //
  // The dark chip surface is TRANSLUCENT (rgba(255,255,255,0.07) over the chrome), so it is composited
  // rather than read — the same treatment the wash gets above.
  it("every state mark is painted by an INK token, and clears 3:1 where it lands", () => {
    const css = builtCss();
    // The rule body for an EXACT selector: "sel {" with the space, so `.monitor-mark` cannot match
    // `.monitor-mark.is-flapping {` by prefix. `blockOf` lives in approvalSurfaces.test.ts and is not
    // exported, so this stays local rather than reaching across files for it.
    const blockOf = (cssText: string, sel: string): string | null => {
      const i = cssText.indexOf(sel + " {");
      if (i < 0) return null;
      const j = cssText.indexOf("}", i);
      return j < 0 ? null : cssText.slice(i, j + 1);
    };
    const resolve = (tokens: Record<string, string>, name: string): string => {
      const raw = tokens[name] ?? "";
      const asVar = /^var\((--[a-z0-9-]+)\)$/.exec(raw.trim());
      const inner = asVar && asVar[1];
      return inner ? (tokens[inner] ?? raw) : raw;
    };
    // The RULES, not a hand-picked pair: a token-pair check passes happily while the rule itself goes
    // back to a fill colour — which is exactly what the first version of this test did, and what a
    // mutation proved. Each rule is read from the built sheet, so a regression fails here.
    const marks: Array<[string, string]> = [
      [".boot-mark", "--surface-chip"],
      [".monitor-mark", "--surface-chip"],
      [".waiting-mark", "--surface-chip"],
      ['.rail-dot[data-state="waiting"]', "--chrome-bg-2"],
      ['.cmd-dot[data-state="warn"]', "--chrome-bg-2"],
      [".side-dot", "--chrome-bg-2"],
      [".tab-dot", "--chrome-bg-2"],
      // AND THE ONES THE FIRST LIST MISSED. Rounds 126-127 fixed ten rules; this guard started with
      // seven, so three of the fixes it exists to protect were not protected by it. Found by comparing
      // the list against the sheet rather than by trusting the list.
      ['.desktop-rail-status[data-state="waiting"] .dot', "--chrome-bg-2"],
      [".tab-wait", "--chrome-bg-2"],
      [".monitor-alert .monitor-mark", "--surface-chip"],
      ['#approval-arm .ag-dot[data-state="armed"]', "--surface"],
      [".monitor-dot.down", "--surface-chip"],
    ];
    // A FILL token is not a mark's colour. These are the ones that measured 2.53-2.96 on the dark
    // chrome before rounds 126-127; --warn-ink and --accent-ink are the tokens meant for graphics.
    const FILLS = ["--state-warn", "--state-ok", "--state-fail", "--state-running", "--accent", "--success", "--danger"];
    const light = tokensIn(css, ":root");
    const dark = { ...light, ...tokensIn(css, 'body[data-theme="dark"]') };
    const failures: string[] = [];
    let checked = 0;
    for (const [selector, surfaceName] of marks) {
      const block = blockOf(css, selector);
      expect(block, `${selector} missing from the built sheet`).not.toBeNull();
      // A mark paints with a background OR a border: .boot-mark is a TRIANGLE, drawn entirely by its
      // bottom border, and has no background at all. Either is a token; both are checked the same way.
      // background, or ANY border property — .boot-mark writes `border-bottom: 7px solid var(--warn-ink)`,
      // a shorthand with the token inside it, which a `border-*-color` pattern misses entirely.
      const paint = /(?:background|border(?:-[a-z]+)*):[^;]*var\((--[a-z0-9-]+)\)/.exec(block!);
      expect(paint, `${selector} must paint from a token`).not.toBeNull();
      const token = paint![1];
      if (FILLS.includes(token)) {
        failures.push(`${selector} paints itself with the FILL ${token}`);
      }
      for (const [theme, tokens] of [["light", light], ["dark", dark]] as Array<[string, Record<string, string>]>) {
        const ink = parseColour(resolve(tokens, token));
        const surface = parseColour(resolve(tokens, surfaceName));
        expect(ink && surface, `${selector}: ${token} and ${surfaceName} must resolve`).toBeTruthy();
        const surf = surface as NonNullable<typeof surface>;
        const inkColour = ink as NonNullable<typeof ink>;
        const base = parseColour(resolve(tokens, "--chrome-bg-2"));
        const layer = (surf.a ?? 1) < 1 && base ? compositeStack([surf, base]) : surf;
        const ratio = contrastRatio(inkColour, layer);
        checked++;
        if (ratio < 3.0) failures.push(`${selector} [${theme}] ${token} on ${surfaceName} = ${ratio.toFixed(2)} (needs 3)`);
      }
    }
    expect(checked, "this check must actually measure something").toBe(marks.length * 2);
    expect(
      failures,
      `${failures.length} mark(s) wrong — a FILL token is not a mark's colour, and 3:1 is the bar:\n  ` +
        failures.join("\n  "),
    ).toEqual([]);
  });
});
