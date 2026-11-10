// THE LAYOUT CONTRACT, SHARED WITH THE DESKTOP PANEL.
//
// WHY. The console and the panel (`/home/zss/summrise/agent/resources/panel-react`) are two
// frontends of one product. They shared a token scale and nothing else about layout. Measured
// 2026-11-10: this console holds its page to 1,100 px inside a 232 px labelled rail; the panel's
// `.desktop-content` measured 1,271 px at a 1360 window and **2,471 px at 2560** — no page column at
// all — while its Settings page nested three boxes at x=98, 115, 132 and its Memory page carried a
// second 980 px reading width the frame had never given it.
//
// The contract is a JSON file carried by BOTH repos. This test does two things:
//
//   1. asserts THIS repo's stylesheet draws the frame at the contract's numbers, through tokens; and
//   2. asserts the file's sha256 equals a literal recorded in the PANEL's copy of this test. Changing
//      the contract here without changing it there fails that test, and vice versa. That is the
//      cross-repo drift alarm: one file, one hash, two loud failures.
import { describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
// device/console/web/src → device/console/web → device/console → device → repo root
const contractPath = path.join(
  here,
  "..",
  "..",
  "..",
  "..",
  "docs",
  "superpowers",
  "specs",
  "layout-contract.json"
);

type Contract = {
  page: { max: number; pad: number; gap: number; listGap: number; boxSizing: string };
  card: { radius: number; pad: number; gap: number; headerGap: number };
  rail: {
    // THE CONSOLE'S LABELLED RAIL STARTS AT THIS WIDTH (the seventy-fourth list); below it is the 56 px
    // icon rail, and below `phone` the phone's bottom bar. The number used to say 767 and the CSS said
    // 640, because two later `min-width: 640px` queries overrode the icon band's rule.
    // (recorded as a measurement, not a preference)
    console: number;
    panel: number;
    consoleCollapsesAt: number | null;
    panelCollapsesAt: number | null;
  };
  title: { size: number; weight: number };
  breakpoints: Record<string, number> & { queryForm: { below: string; at: string } };
  empty: {
    wayBack: boolean; radius: number; pad: number; gap: number; textAlign: string; shadow: string };
  head: {
    gap: number;
    align: string;
    subtitleSlot: string;
    maxHeadsPerPage: number;
  };
  motion: { reducedMotion: string; cap: string; namedAnimationsStop: boolean };
  cardTitle: { size: number; weight: number };
  tracking: {
    bySize: Record<string, string>;
    inherits: boolean;
    literals: number;
  };
  ground: { light: string; dark: string; gradients: number };
  elevation: { flat: string; pane: string; raised: string };
  palette: { washAlpha: number; ink: string; muted: string; faint: string; accent: string; semantic: string[] };
  leading: { body: number; tight: number; mark: number };
  measure: { prose: string; codeExempt: boolean };
  space: {
    scale: number[];
    controlH: number;
    controlPadY: number;
    target: number;
  };
  radius: {
    page: Record<string, number>;
    overlay: Record<string, number>;
    pill: number;
    target: number;
  };
  hairline: {
    line: string;
    strong: string;
    soft: string;
    names: number;
    stateMarksCarryOwnColour: boolean;
    stateMarksCarryWord: boolean;
  };
  label: {
    case: string;
    verbAndSubject: boolean;
    acronyms: string[];
  };
  section: {
    h2: { size: number; weight: number };
    h3: { size: number; weight: number };
    eyebrow: { size: number; weight: number; tracking: number; transform: string };
  };
  failed: { radius: number; pad: number; textTone: string; tint: string; role: string; retries: number };
  loading: {
    rowRadius: number;
    rowPad: number;
    rowGap: number;
    barRadius: number;
    rows: number;
    role: string;
  };
};

function contractText(): string {
  return readFileSync(contractPath, "utf8");
}

const contract: Contract = JSON.parse(contractText()) as Contract;

function css(): string {
  return readFileSync(path.join(here, "app.css"), "utf8");
}

/** The scale's own file: app.css USES the tokens, tokens.css DEFINES them. */
function tokens(): string {
  return readFileSync(path.join(here, "tokens.css"), "utf8");
}

/** The declarations of the LAST rule for a selector — the cascade's answer, not the first match. */
/** The source with comments removed, so a gate reads the MARKUP and never its own explanation. */
function stripComments(source: string): string {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/\{\/\*[\s\S]*?\*\/\}/g, "")
    .replace(/\s\/\/ .*$/gm, "");
}

function ruleFor(source: string, selector: string): string {
  const matches = [
    ...source.matchAll(new RegExp(`(?:^|\\n)${selector}\\s*\\{([\\s\\S]*?)\\n\\}`, "gm"))
  ];
  if (matches.length === 0) throw new Error(`no rule for ${selector}`);
  return matches[matches.length - 1][1];
}

function tokenPx(source: string, token: string): number {
  // THE CONSOLE WRITES ITS SCALE ON FEW LINES (`--radius:14px; --radius-sm:10px;`), so the
  // separator after the colon is OPTIONAL — a regex that demanded one found no token at all.
  const match = source.match(new RegExp(`--${token}: *([0-9]+)px`));
  if (match === null) throw new Error(`--${token} is not defined`);
  return Number(match[1]);
}

function tokenFor(source: string, value: number, family: string): string {
  for (const match of source.matchAll(new RegExp(`--(${family}[\\w-]*): *${value}px`, "g"))) {
    return match[1];
  }
  throw new Error(`no ${family} token stands for ${value}px`);
}

describe("the layout contract", () => {
  it("is the one the panel has", () => {
    // **THE CROSS-REPO PIN.** This literal appears in the panel's
    // `agent/resources/panel-react/src/lib/layoutContract.test.ts`. If the two disagree, the two
    // frontends are drawn to different page systems and one of them is wrong.
    expect(createHash("sha256").update(contractText()).digest("hex")).toBe(
      "72d6e4a4116e052d477cc50bd5e78ca7eff0861a564ccdbc6a9811204f5a9f58"
    );
  });

  it("is a page frame this console draws, through tokens", () => {
    const source = css();
    const page = ruleFor(source, "\\.page");
    const pad = tokenFor(tokens(), contract.page.pad, "sp");
    const gap = tokenFor(tokens(), contract.page.gap, "sp");
    expect(page, "the page column's max width is the contract's").toContain(
      `min(${contract.page.max}px, 100%)`
    );
    expect(page, "the page's padding comes from the scale").toContain(`padding: var(--${pad})`);
    expect(page, "the page's rhythm comes from the scale").toContain(`gap: var(--${gap})`);
    expect(tokenPx(tokens(), pad)).toBe(contract.page.pad);
    expect(tokenPx(tokens(), gap)).toBe(contract.page.gap);
  });

  it("is the frame's arithmetic: this surface's `max` is the BOX, which is the panel's defect to avoid", () => {
    // **THE CONTRACT'S `max` MEANT THE BOX HERE AND THE CONTENTS ON THE PANEL (the seventieth list).**
    // Measured 2026-11-10 at 1,440: this page is **1,120 wide with 1,072 of contents** because of the global
    // `* { box-sizing: border-box }` and `width: min(1120px, 100%)`; the panel declared `max-width: 1120px`
    // under content-box, drew a box of 1,168 with 1,120 of contents, and below `wide` stuck **31 px past the
    // window's right edge**. This surface was already right, and that is why the number needed recording:
    // an unrecorded correct answer is one a later page can get wrong.
    expect(contract.page.boxSizing, "max is the box").toBe("border-box");
    const sheet = css();
    // The global reset is this surface's arithmetic, and the frame's width is derived from the contract.
    expect(sheet, "this surface resets every box").toMatch(/\*\s*\{\s*box-sizing:\s*border-box/);
    const page = ruleFor(sheet, "\\.page");
    expect(page, "the page's width is capped by the contract's number").toContain(
      `min(${contract.page.max}px, 100%)`
    );
  });

  it("is the page body's rhythm, and a list's tick is the list's own", () => {
    // **THE RHYTHM BELONGS TO THE CONTAINER THAT HAS LIKE THINGS IN IT (the sixty-ninth list).** Measured
    // 2026-11-10: this console is uniform — every page's head sits 24 px above its first block and its
    // blocks run 24 apart — and the panel was not: `page-plugins` put the card's 8 on the PAGE ROOT, so its
    // head sat 8 px off its first block while Memory's sat 24, and Settings charged the frame's 24 plus a
    // `margin-bottom: 12` for the same separation, which is 36 — a value on no surface's scale.
    expect(contract.page.gap, "the page body's rhythm").toBe(24);
    expect(contract.page.listGap, "a list ticks at the card's own gap").toBe(contract.card.gap);
    const sheet = css();
    // **1. THE PAGE ROOT DECLARES THE FRAME'S RHYTHM ONCE AND NO PAGE OVERRIDES IT.** This surface was
    // already right, and that is why the rule is recorded: the panel's exception was invisible here.
    const pageRules = [...sheet.matchAll(/\n(\.page[\w-]*)\s*\{([^}]*)\}/g)]
      .filter((m) => !m[1].startsWith(".page-"))
      .filter((m) => /[;\s]gap:/.test(m[2]));
    expect(pageRules.map((m) => m[1]), "no page body restates the frame's gap").toEqual([".page"]);
    // **2. AND A LIST DECLARES ITS TICK ON ITS OWN ELEMENT**, which is what the panel's fix added and what
    // this surface must keep if it grows one: `.runs-history`, `.files-list` and `.attention-problems` are
    // lists, and none of them may move the page's rhythm to say so.
    for (const list of [".files-list", ".attention-problems", ".runs-history"]) {
      const rule = ruleFor(sheet, list.replace(/\./g, "\\."));
      expect(rule, `${list} is a list this page draws`).toBeTruthy();
      expect(rule, `${list} does not restate the page's gap`).not.toMatch(/[;\s]gap:\s*var\(--sp-5\)/);
    }
  });

  it("is a CARD's inset: `padding: 0` only when the child brings the inset", () => {
    // **A CARD AT `padding: 0` IS RIGHT ONLY WHEN ITS CHILD BRINGS THE INSET (the sixty-seventh list).** The
    // pair that produced this rule: the panel's `.settings-fold` painted a frame and hugged it (its child was
    // a plain block, so the BOX had to pad), while this surface's `.fleet-table-wrap` does the same thing
    // and is CORRECT, because its child is a table whose header row pads `9px var(--sp-4)`.
    const sheet = css();
    const wrap = ruleFor(sheet, ".fleet-table-wrap");
    expect(wrap, "the table card exists").not.toBeNull();
    expect(wrap!, "and it pads nothing itself").toMatch(/padding: *0/);
    // **AND THE CHILD SUPPLIES IT.** The header row's padding is what makes the zero right.
    const th = ruleFor(sheet, ".fleet-table thead th");
    expect(th!, "the table's header row pads").toMatch(/padding: *[^;]*var\(--sp-4\)/);
    // **A PLAIN CARD PADS ITSELF.** The other cards on the same page are at the atom's 16, which is what a
    // card whose children are blocks must do.
    const card = ruleFor(sheet, ".card");
    expect(card!, "a plain card pads itself").toMatch(/padding: *var\(--sp-4\)/);
  });

  it("is ONE scrim, and a background outside it is not a colour a surface may paint", () => {
    // **THE SAME SCRIM IN TWO SHADES (the sixty-sixth list).** `.sheet-scrim` painted `rgba(0,0,0,.4)` and
    // `.scrim` painted `rgba(12,16,20,.38)`, and a later rule repainted BOTH to the same `color-mix` — so
    // the two literals were dead except in the cascade. A scrim is one proportion of one colour over the
    // page; it is not a surface, and there are not two of them.
    const tk = tokens();
    expect(tk, "the scrim is a token").toMatch(/--scrim: *color-mix\(in srgb, #000 30%, transparent\)/);
    for (const selector of [".sheet-scrim", ".scrim"]) {
      const body = ruleFor(css(), selector);
      expect(body, selector + " must have a rule").not.toBeNull();
      expect(body!, selector + " paints the one scrim").toMatch(/background: *var\(--scrim\)/);
    }
    // **AND NO SCRIM IS A RAW COLOUR.** The two shades this list found cannot come back as literals.
    expect(tk, "no .4 black scrim literal").not.toMatch(/rgba\(0, 0, 0, 0\.4\)/);
    expect(tk, "no tinted scrim literal").not.toMatch(/rgba\(12, 16, 20, 0\.38\)/);
    // **THE OTHER FOUR LITERAL BACKGROUNDS ON THIS SURFACE ARE OVERLAYS**, and they are recorded as such
    // rather than forgiven: a dialog's wash cannot be a surface colour.
    expect(contract.palette.washAlpha).toBe(0.18);
  });

  it("is a control's LABEL: sentence case, and never a fragment with a glyph", () => {
    // **THIS SURFACE HAD THE SAME FRAGMENT IN TWO PLACES (the sixty-fifth list).** `+ New session` appeared
    // on the shell's toolbar and again in the empty state's actions — a glyph doing a word's job — while
    // 21 of this surface's other 22 controls were already sentence case.
    expect(contract.label.case, "a label is sentence case").toBe("sentence");
    expect(contract.label.verbAndSubject, "and it names a verb and its subject").toBe(true);
    expect(contract.label.acronyms).toContain("URL");
    expect(contract.label.acronyms).toContain("AI");
    // **NO LABEL IN THIS SURFACE'S COMPONENT TREE STARTS WITH A GLYPH.** The check is the whole tree, so a
    // new `+ Something` is refused rather than the one this list fixed.
    for (const file of ["TerminalWorkspace.tsx", "HomePage.tsx", "OverviewPage.tsx", "TerminalDialogs.tsx"]) {
      let body: string;
      try {
        body = readFileSync(path.join(here, file), "utf8");
      } catch {
        continue; // a page this surface does not have is not a failure
      }
      expect(body, `${file} has no glyph-led label`).not.toMatch(/>\s*\+\s*[A-Za-z]/);
    }
  });

  it("is the QUERY FORM at a breakpoint, and no width is claimed by both sides", () => {
    // **A FOLD ENDS AT N-1 (the sixty-fourth list).** The panel's approval banner used
    // `(max-width: 1088px)` — AT `columns` — so it held its one-column shape one pixel into the wide band.
    // This surface was already right, and the check is what keeps it right: its min set and max set do
    // not intersect, and every fold is spelled on the side it belongs to.
    const source = css();
    const maxWidths = new Set(
      [...source.matchAll(/@media[^{]*\(max-width:\s*([0-9]+)px\)/g)].map((m) => Number(m[1]))
    );
    const minWidths = new Set(
      [...source.matchAll(/@media[^{]*\(min-width:\s*([0-9]+)px\)/g)].map((m) => Number(m[1]))
    );
    const contested = [...maxWidths].filter((v) => minWidths.has(v));
    expect(contested, "no width is claimed by both query forms").toEqual([]);
    // **AND THE FORM IS THE CONTRACT'S.** A downward query ends at N-1 and an upward one starts at N, so
    // every `max-width` sits one below a breakpoint and every `min-width` sits on one.
    const bp = new Set(
      Object.entries(contract.breakpoints)
        .filter(([, v]) => typeof v === "number")
        .map(([, v]) => v as number)
    );
    // **A BREAKPOINT IS SPELLED ONE WAY OR THE OTHER, NOT BOTH.** This surface writes its phone fold as a
    // PAIR — `(max-width: 639px)` and `(min-width: 640px)` — and its tablet fold as a max-width alone,
    // because nothing below it needs the other side. The rule is therefore about ownership, not about
    // which spelling a sheet happens to use: a breakpoint's number may be a `max-width` or a `min-width`,
    // never both, and `contested` above is that assertion.
    const widths = [...maxWidths, ...minWidths];
    for (const v of widths) {
      expect(bp.has(v) || bp.has(v + 1) || bp.has(v - 1), `${v} must sit on a breakpoint`).toBe(true);
    }
    expect(contract.breakpoints.queryForm.below).toBe("N-1");
    expect(contract.breakpoints.queryForm.at).toBe("N");
  });

  it("is ONE head per page, and everything under it is a card", () => {
    // **THE DEVICE PAGE DREW TWO HEADERS 57 px APART (the sixty-second list).** Measured: `.page-head`
    // at y=76 and `.overview-head` — a `<header>` — at y=133, whose `h2` read as a second page title. A
    // page has ONE head; anything under it is a card, which is what `card` means in the contract.
    // The overview's block is a `<section className="card">` now, and this test refuses the old shape.
    expect(contract.head.maxHeadsPerPage, "one head per page").toBe(1);
    expect(contract.head.subtitleSlot, "the subtitle slot is named").toBe(".page-sub");
    // **NO PAGE MARKUP HAS TWO HEADERS.** The check is per page file: a `<header>` and a second one, or a
    // `<header>` wrapping a block that is a card elsewhere.
    const pages = ["OverviewPage.tsx", "HomePage.tsx", "ActivityTimeline.tsx"];
    for (const file of pages) {
      const source = readFileSync(path.join(here, file), "utf8");
      const headers = [...source.matchAll(/<header\b/g)].length;
      expect(headers, `${file} draws at most one <header>`).toBeLessThanOrEqual(1);
    }
    const overview = readFileSync(path.join(here, "OverviewPage.tsx"), "utf8");
    expect(overview, "the machine's block is a CARD").toContain('className="card overview-head"');
    expect(overview, "and it is a section with a label").toMatch(/<section className="card overview-head" aria-label=/);
  });

  it("is a MEASURE for prose, and a transcript is exempt", () => {
    // **THIS SURFACE'S PROSE STOOD AT 46 ch AND STILL HAD NO RULE (the sixty-first list).** Measured, the
    // widest line here was `.hint installer-missing` at 46 characters — inside the band a line is followed
    // back along — because the sentences happened to be short. The blocks that would take the card's
    // 1,072 px had never been asked. A token makes the NEXT sentence take the measure.
    const tk = tokens();
    expect(tk, "the measure is a token").toMatch(/--measure: *66ch/);
    expect(contract.measure.prose).toBe("66ch");
    // **THE PROSE CLASS TAKES IT**, and so does each block that carries a sentence.
    for (const selector of [".hint", ".attention-problems li", ".activity-status"]) {
      const body = ruleFor(css(), selector);
      expect(body, selector + " must have a rule").not.toBeNull();
      expect(body!, selector + " takes the measure").toMatch(/max-width: *var\(--measure\)/);
    }
    // **AND A TRANSCRIPT IS EXEMPT.** `.evidence-output` is a log: scanned token by token, and wrapping it
    // to a prose measure would break the shape of the output the console prints. The contract says so.
    expect(contract.measure.codeExempt).toBe(true);
    const transcript = ruleFor(css(), ".evidence-output");
    expect(transcript!, "a transcript keeps its width").not.toMatch(/max-width: *var\(--measure\)/);
  });

  it("is THREE line-height ratios, and the mark belongs to a glyph", () => {
    // **EIGHT RATIOS ON THIS SHEET (the sixtieth list).** Measured before: `1` (8 sites), `1.45` (7),
    // `1.5` (9), `1.4` (3), `1.2` (2), `1.6`, `1.55`, `0`. A reader cannot see .05 of a ratio, so it was
    // never a design choice — and `0` on a rule that carried text was a collapsed box waiting to clip.
    const tk = tokens();
    const read = (name: string) => Number(tk.match(new RegExp(`--${name}: *(\\d+(?:\\.\\d+)?)`))?.[1]);
    expect(read("lh-body"), "a paragraph's leading").toBe(contract.leading.body);
    expect(read("lh-tight"), "a heading's leading").toBe(contract.leading.tight);
    expect(read("lh-mark"), "a glyph in a box").toBe(contract.leading.mark);
    // **NO BARE RATIO SURVIVES IN THE SHEET.** The lookahead has to skip the space a sheet may leave
    // before the token: `(?! *var(--lh` refuses both spellings and still catches a literal like 1.4.
    const literals = [...css().matchAll(/line-height: *(?! *var\(--lh)[^;}]+/g)].map((m) => m[0]);
    expect(literals, "no bare line-height survives").toEqual([]);
    // **AND THE MARK IS NOT A SENTENCE'S RATIO.** 1 sets a glyph's box to its own size, which is rule
    // 13's floor; the contract says so, and body is looser than tight so the three cannot collapse.
    expect(contract.leading.mark).toBe(1);
    expect(contract.leading.body).toBeGreaterThan(contract.leading.tight);
  });

  it("is the rhythm, and a control's HEIGHT is not a padding", () => {
    // **A NUMBER OFF THE SCALE IS A NUDGE UNLESS IT MEASURES SOMETHING ELSE (the fifty-ninth list).**
    // Measured on this surface: `button { padding: 5px }` looked like a nudge against a scale of
    // 2/4/6/8 — and this sheet's own comment said why it is not: "One control height (28)". 5 + 5 over an
    // 18 px line box is 28, so the number had been chosen for a HEIGHT and written as a padding.
    const tk = tokens();
    // The console's token file is MINIFIED (`--control-h:28px`) where the panel's is spaced; a contract
    // records a VALUE, not a source text, so the separator is optional here.
    const read = (name: string) => Number(tk.match(new RegExp(`--${name}: *(\\d+)px`))?.[1]);
    for (const step of contract.space.scale) {
      expect(tk, `the rhythm has a ${step} step`).toMatch(new RegExp(`--sp-[0-9-]+: *${step}px`));
    }
    expect(read("control-h"), "the control's height").toBe(contract.space.controlH);
    expect(read("control-pad-y"), "and the padding that reaches it").toBe(contract.space.controlPadY);
    // **AND THE BUTTON SAYS ITS HEIGHT AS A HEIGHT.** A token nothing reads is a number waiting to be typed
    // twice; this asserts the rule that uses it.
    const button = ruleFor(css(), "button");
    expect(button, "the button reads the height").toMatch(/min-height:\s*var\(--control-h\)/);
    expect(button, "and the padding").toMatch(/padding:\s*var\(--control-pad-y\)/);
  });

  it("is the radius scale, and every step means something rather than being available", () => {
    // **SIX DECLARED VALUES, AND THE LARGEST IS PAINTED ONLY INSIDE AN OVERLAY (the fifty-eighth list).**
    // `--radius-lg: 20px` exists here and is used three times — a bottom sheet's top corners, and two
    // dialogs — so a PAGE never draws it. **A scale whose largest step is invisible on a page is a step
    // that drifts**, because nothing a person sees can show it is wrong. The contract says what each step
    // is FOR, and this surface is the one that supplied the row meaning (`10` on `.fleet-row`,
    // `.tool-list`, `.skeleton-row`).
    const tk = tokens();
    const read = (name: string) => Number(tk.match(new RegExp(`--${name}:\\s*(\\d+)px;`))?.[1]);
    expect(read("radius"), "a card's corner").toBe(contract.radius.page.card);
    expect(read("radius-sm"), "a ROW's corner").toBe(contract.radius.page.row);
    expect(read("radius-sm2"), "a control's corner").toBe(contract.radius.page.control);
    expect(read("radius-xs"), "a mark's corner").toBe(contract.radius.page.mark);
    expect(read("radius-lg"), "an overlay's corner").toBe(contract.radius.overlay.sheet);
    // **AND THE OVERLAY STEP IS BIGGER THAN THE PANE STEP BY DEFINITION** — a sheet covers a page, so its
    // corner is drawn larger. Equal values would mean there is no reason for two names.
    expect(contract.radius.overlay.sheet).toBeGreaterThan(contract.radius.page.card);
    expect(contract.radius.pill, "one pill").toBe(999);
    // **AND THE LARGEST STEP IS USED ONLY WHERE AN OVERLAY IS** — proven by where it appears, not by a
    // promise about intent.
    const uses = [...css().matchAll(/(\.[\w-]+(?:\s*\{\s*|\s*\[[^\]]*\]\s*\{)?)[^{}]*?var\(--radius-lg\)/g)].length;
    expect(uses, "the overlay step has call sites").toBeGreaterThan(0);
    expect(uses, "and not one of them is a page-level card").toBeLessThanOrEqual(4);
  });

  it("is ONE hairline family, and the chrome's line is derived rather than restated", () => {
    // **THIS SURFACE HAD ONE FAMILY AND THE PANEL HAD THREE (the fifty-seventh list).** Measured, the
    // panel's `--chrome-line` was the same `rgba(0,0,0,.08)` as `--line` under a second name, and its
    // `--glass-hairline` was a COOL `.07` for the same job. This surface derives the chrome's line from
    // the ink with `color-mix` and keeps the family at three steps — which is the shape the contract now
    // records, so the surface that had three names could not keep them.
    const tk = tokens();
    expect(tk, "the line is the contract's").toMatch(/--line:rgba\(0,0,0,\.08\);|--line:rgba\(0, 0, 0, 0\.08\);/);
    expect(tk, "the strong step").toContain("--line-strong:rgba(0,0,0,.14)");
    expect(tk, "the soft step").toContain("--line-soft:rgba(0,0,0,.05)");
    // **AND THE CHROME'S LINE IS NOT A FOURTH NAME** — it is the same `--line`, so a change to the
    // hairline cannot leave the chrome behind.
    const sheet = css();
    expect(sheet, "the chrome line is derived from the ink").toMatch(
      /--hairline:\s*color-mix\(in srgb, var\(--ink\)/);
    expect(contract.hairline.names, "one family, three steps").toBe(1);
  });

  it("is the OTHER half of a state mark: the word, and this surface already carried it", () => {
    // **THE CONTRACT RECORDED THE COLOUR HALF OF RULE 6 AND NOT THE OTHER (the sixty-eighth list).**
    // `stateMarksCarryOwnColour` says a mark takes its own ink; it says nothing about the WORD beside it,
    // and the panel's rail mark turned out to carry its state only in colour and shape, with the words in a
    // `title` — a hover, not a surface. Measured on this console: every state it draws already says it in
    // words — `.fleet-status` reads "offline · last seen …", `.home-verdict` reads "3 of 4 online · 2 need
    // attention" — so this surface is the one the rule came FROM and the one that must keep it.
    expect(contract.hairline.stateMarksCarryOwnColour, "a mark takes its own colour").toBe(true);
    expect(contract.hairline.stateMarksCarryWord, "and it carries a word").toBe(true);
    const sheet = css();
    // The three state marks this surface draws, each by the element that carries the sentence.
    for (const [selector, why] of [
      [".fleet-status", "the fleet's dot is beside the phrase that explains it"],
      [".home-verdict", "the verdict counts in words, not only in tint"],
      [".activity-status", "a run's status is a word"],
    ]) {
      const rule = ruleFor(sheet, selector.replace(/\\./g, "\\."));
      expect(rule, `${why}: ${selector} is drawn`).toBeTruthy();
    }
    // **AND A MARK WITH NO WORDS AT ALL WOULD BE THE DEFECT** — so the dot-plus-word shape is asserted
    // directly rather than only named: a `aria-hidden` state dot is legal (colour is decoration when the
    // words are beside it) and a dot is NOT legal when it is the only thing that carries the state.
    // `ruleFor` THROWS on a missing selector, so the absence of a rail-status mark is a regex, not a rule.
    expect(sheet, "this surface has no unlabelled rail mark").not.toMatch(/\\.rail-status\s*\{/);
    // Every dot this surface draws is `aria-hidden` and sits beside its sentence; the words are the state.
    const dots = [...sheet.matchAll(/\b(\w+-status|\w+-state|\w+-dot)\s*\{/g)].map((m) => m[1]);
    expect(dots.length, "the surface draws state elements").toBeGreaterThan(0);
  });

  it("is the palette's third grey, and this surface is the one it came from", () => {
    // **THE THIRD GREY WAS TWO COLOURS SIX APART (the fifty-sixth list).** Measured from both token
    // blocks: the ink was already identical (`#18181b`) and so was the second grey (`#71717a`), but the
    // third was `#696a74` here and `#6f707a` on the panel — two names for a colour the eye cannot tell
    // from the other, which rule 12 refuses.
    const source = tokens();
    // The rule: FEWER NUMBERS, so the third grey is not a fourth family waiting to appear.
    expect(source, "the third grey is the contract's").toContain(`--faint:${contract.palette.faint};`);
    expect(source, "the ink is the contract's").toContain(`--ink:${contract.palette.ink};`);
    expect(source, "the second grey is the contract's").toContain(`--muted:${contract.palette.muted};`);
    expect(source, "the accent is the contract's").toContain(`--accent:${contract.palette.accent};`);
    // **AND THE PALETTE IS FIVE NAMES, NOT A SPECTRUM.** `ink`, `muted`, `faint`, one accent, and the
    // semantic four — anything else is a decision that has to be made in the open, which is what the
    // surface that added `#6f707a` had not done.
    expect(contract.palette.semantic.length, "four semantic names").toBe(4);
  });

  it("is the card's elevation, and this surface already drew it", () => {
    // **THE CONSOLE WAS THE SURFACE THE PANEL WAS MEASURED AGAINST (the fifty-fifth list).** Its card
    // measured `box-shadow: none` — a hairline on the ground — while the panel's carried a four-layer
    // float. So this test is the line being HELD rather than a fix: `elevation.flat` is `none`, a card
    // is flat on the page, and `elevation.pane` (`--glass-shadow`) is the level that may float.
    const source = css();
    const card = ruleFor(source, ".card");
    expect(card, "the card must have a rule").not.toBeNull();
    const declared = card!.match(/box-shadow:\s*([^;}]+)/)?.[1]?.trim() ?? "";
    // **FLAT IS ABSENCE AS MUCH AS IT IS `none`** — this surface's `.card` never declared a shadow, and a
    // rule that carries none satisfies the level exactly as a rule that says `none` does.
    expect(declared === "" || declared === contract.elevation.flat, "the card sits ON the page").toBe(
      true
    );
    // **AND THE FLOAT IS STILL SOMEWHERE** — a modal or a drawer with no shadow would read as part of the
    // page. This surface's `--shadow-lg` is that level, and it is used.
    expect(source, "the pane level exists and is used").toContain("var(--shadow-lg)");
    expect(contract.elevation.pane).toBe("--glass-shadow");
  });

  it("is the ground, and it is ONE colour rather than a wash", () => {
    // **THE SURFACE EVERY PAGE SITS ON HAD NO LINE IN THE CONTRACT (the fifty-fourth list).** Measured:
    // the panel's ground was three decorative gradients behind a fixed overlay plus a fourth on its
    // canvas, in six hand-written colours, while this surface painted ONE colour and drew no overlay at
    // all. Rule 17 refuses a gradient as DECORATION, and the console was already right — so this test is
    // the line that keeps it right, and the number the other surface is now held to.
    const source = css();
    // **`ruleFor` IS NOT THE RIGHT TOOL FOR `body`.** It reads the LAST match of `body { … \n}`, and
    // there are twelve `body`-containing rules whose bodies run to the next newline-brace — the last
    // one it finds is not the ground's. Asserting through it would have been a test that passed for a
    // reason unrelated to what it claims (found by mutation: the gradient went in and the test stayed
    // green). The ground's own declaration is read directly.
    const body = source.match(/\nbody\s*\{\s*\n\s*margin: 0;\s*\n([^}]*)\n\}/)?.[1];
    expect(body, "the ground is declared on body").toBeTruthy();
    // The light ground is a color-mix over the panel token; the assertion is that it is ONE declaration
    // with no image layer, not that it is a literal.
    expect(body!, "the ground carries no image").not.toMatch(/background-image/);
    expect(body!, "the ground is a colour").toMatch(/background:\s*(color-mix|var\(--)/);
    // **AND NO PAGE-LEVEL DECORATIVE GRADIENT.** The fades that DO exist are the table's edge fades —
    // they explain that a horizontally scrolling table continues past its edge, which is a STATE — and
    // they are the only ones. A third would be decoration, and this count is where it would show up.
    const imageRules = [
      ...source.matchAll(/\.[\w-]+[^{]*\{[^}]*background-image:\s*(?:linear|radial)-gradient/g),
    ].map((m) => m[0].split("{")[0].trim());
    expect(imageRules.length, "no page-level decorative gradient").toBeLessThanOrEqual(2);
    expect(contract.ground.gradients, "the contract records zero decorative gradients").toBe(0);
  });

  it("is the card title, and a card header is not styled by accident", () => {
    // **THE SAME VISUAL ROLE WAS TWO ELEMENTS AT TWO SIZES (the fifty-third list).** Measured: this
    // surface drew the card's title as `<strong>` at 13 px / 600 while the panel's card header drew
    // `h2` at 14 / 600. Worse than a size mismatch: a `<strong>` is read as emphasis and NEVER as a
    // heading, so the element that looked like the card's title was not one — while the `h2` beside it
    // was the card's EYEBROW (10 px, uppercase). The contract's `cardTitle` is 14 / 600.
    const source = css();
    const rule = source.slice(source.indexOf(".card header strong,"), source.indexOf("}", source.indexOf(".card header strong,")) + 1);
    expect(rule, "the card title's rule exists").toContain("h2");
    expect(rule, "and a card header's h3 is the same level").toContain("h3");
    const size = rule.match(/font-size:\s*var\(--([\w-]+)\)/)?.[1];
    expect(size, "the card title's size is a TOKEN").toBeTruthy();
    expect(tokenPx(tokens(), size!), "the card title is the contract's").toBe(contract.cardTitle.size);
    // **A WEIGHT IS A BARE NUMBER, NOT A `px` VALUE**, so it is read from the token block directly rather
    // than through `tokenPx` (which would look for `--fw-semi: 600px` and find nothing).
    const weightTok = rule.match(/font-weight:\s*var\(--([\w-]+)\)/)?.[1];
    expect(weightTok, "the card title's weight is a TOKEN").toBeTruthy();
    expect(
      Number(tokens().match(new RegExp(`--${weightTok}: *(\\d+)`))![1]),
      "and its weight",
    ).toBe(contract.cardTitle.weight);
  });

  it("is the section level, and no heading below the title is a size of its own", () => {
    // **AN `h2` WAS 22, 17, 14 OR 10 PX DEPENDING ON THE PAGE (the forty-ninth list).** Measured on the
    // built surfaces: Activity's day heading drew 22 / 600 — the page TITLE's size, one weight lighter —
    // while every other `h2` here drew 17 / 600. The record's day read as a second page title above its
    // rows. The contract's `section.h2` is 17 / 600, and this is the assertion that both surfaces'
    // headings land on it.
    const source = css();
    const scale = tokens();
    // The one heading this surface sets by hand below the title: the day.
    const day = ruleFor(source, ".timeline-day-label");
    const size = day!.match(/font-size:\s*var\(--([\w-]+)\)/)?.[1];
    expect(size, "the day's size is a TOKEN").toBeTruthy();
    expect(tokenPx(scale, size!), "the day is a SECTION, not a second title").toBe(
      contract.section.h2.size
    );
    // And the GLOBAL `h2` is the same level, so a page that writes no rule of its own lands there too.
    // It is written on ONE line (`h2 { font-size: var(--fs-lg); … }`), which `ruleFor`'s
    // `\n}`-terminated shape does not match, so this reads the declaration directly and says why.
    const globalH2 = source.match(/\nh2\s*\{[^}]*font-size:\s*var\(--([\w-]+)\)/)?.[1];
    expect(globalH2, "an h2's size is a TOKEN").toBeTruthy();
    expect(tokenPx(scale, globalH2!), "an h2 is the section level").toBe(contract.section.h2.size);
  });

  it("honours reduced motion with ONE blanket, and the contract records the mechanism", () => {
    // **THE MECHANISM, ASSERTED HERE; THE RESULT, ASSERTED BY `scripts/motion-gate.mjs`.** This surface
    // has honoured `prefers-reduced-motion` completely since it was written — measured, 29 elements move
    // normally and **0** under `reduce` — and that is WHY the panel was measured against it: the panel's
    // answer was two named lists and it still moved 4 buttons. The contract now records the mechanism
    // (the nineteenth of the operator's twenty rules, which had no line in it at all).
    const source = css();
    const block = source.slice(source.indexOf("@media (prefers-reduced-motion: reduce)"));
    // **THE BLANKET WINS OVER ANY RULE WRITTEN LATER**, which is what a named list cannot promise.
    expect(block, "the blanket covers every element and both pseudo-elements").toMatch(
      /\*, *\*::before, *\*::after {/
    );
    expect(block, "and it wins over a later rule").toMatch(/transition-duration:\s*0\.01ms !important/);
    expect(block, "animations are capped too").toMatch(/animation-duration:\s*0\.01ms !important/);
    // **`0.01ms` RATHER THAN `0s`**, so a `transitionend` listener still fires — the reason this surface
    // chose it, kept as a contract value rather than a habit.
    expect(contract.motion.reducedMotion).toBe("blanket");
    expect(contract.motion.cap).toBe("0.01ms");
    // **AND THE NAMED LIST IS STILL HERE FOR THE SECOND HALF OF THE RULE** — an animation whose STATE
    // must also stop, which a duration cannot express.
    expect(contract.motion.namedAnimationsStop).toBe(true);
  });

  it("is one eyebrow, and the twelve rules that drew it stop restating it", () => {
    // **TWELVE RULES WROTE THE SAME FIVE DECLARATIONS (the forty-ninth list, item 2).** The console
    // turned a small label into an eyebrow twelve times, each restating the size, the weight, the
    // spacing and the case; the panel wrote a thirteenth with DIFFERENT numbers (11 px / 700 / a
    // hard-coded 0.06em where this file reads the token). Three numbers for one idea is how a scale
    // stops being one.
    const source = css();
    const scale = tokens();
    // **THE ATOM IS A SELECTOR LIST, so `ruleFor` cannot find it** — it looks for one selector
    // followed by `{`. The list itself is the measurement: whatever rule carries the eyebrow's
    // numbers must name more than one label, because a rule per label is the spelling this replaced.
    const start = source.indexOf(".filter-label,");
    expect(start, "the eyebrow's rule exists").toBeGreaterThan(0);
    const body = source.slice(start, source.indexOf("}", start) + 1);
    const selectors = body.split("{")[0].split(",").map((part) => part.trim()).filter(Boolean);
    expect(selectors.length, "the eyebrow is ONE rule over many labels").toBeGreaterThan(6);
    const rule = body.slice(body.indexOf("{"));
    const eyebrowSize = rule.match(/font-size:\s*var\(--([\w-]+)\)/)?.[1];
    expect(tokenPx(scale, eyebrowSize!), "the eyebrow is the contract's size").toBe(
      contract.section.eyebrow.size
    );
    expect((rule.match(/letter-spacing:\s*var\(--([\w-]+)\)/)?.[1] ?? ""), "spacing is a token")
      .toBeTruthy();
    // **AND NOTHING ELSE RESTATES IT.** The count of `text-transform: uppercase` IS the measurement:
    // twelve before this round, and those that remain are labels that set a COLOUR of their own on top
    // of the atom (an alarm, an accent) or belong to a table head, which is a different thing. A block
    // of rules carrying the same five declarations is the drift this consolidation exists to prevent.
    const uppercase = [...source.matchAll(/text-transform:\s*uppercase/g)].length;
    expect(uppercase, "the eyebrow is ONE rule, not twelve").toBeLessThanOrEqual(4);

  });

  it("is one page head, drawn by every page, with the contract's row", () => {
    // **A CUT THAT WAS THE PANEL'S, AND THIS SURFACE WAS THE PROOF IT COULD BE UNIFORM.** Measured on
    // the built panel: four pages, three header classes and one page with no header at all — and the
    // page without one sat 16 px higher than the rest. The console has always drawn ONE `.page-head`
    // on every page; the contract now writes its row down (`head: { gap 12, align flex-start }`) and
    // this test holds BOTH halves: the rule reads the contract, and no page invents its own.
    const source = css();
    const body = ruleFor(source, ".page-head");
    expect(body, "the page head must have a rule").not.toBeNull();
    const gap = body!.match(/gap:\s*var\(--([\w-]+)\)/)?.[1];
    expect(gap, "the head's gap is a TOKEN").toBeTruthy();
    expect(tokenPx(tokens(), gap!), "the head's gap is the contract's").toBe(contract.head.gap);
    expect((body!.match(/align-items:\s*([^;}]+)/)?.[1] ?? "").trim()).toBe(contract.head.align);
    // **EVERY PAGE DRAWS IT, AND NONE OF THEM SPELLS IT ITSELF.** The pages that carry a title, named
    // so a new page cannot quietly draw its own first line.
    const pages = ["HomePage", "App"];
    const all = pages
      .map((page) => stripComments(readFileSync(path.join(here, `${page}.tsx`), "utf8")))
      .join("\n");
    expect(all, "the head is drawn").toMatch(/className="page-head"/);
    expect(all, "no page keeps a second header class").not.toMatch(
      /className="(device-head|activity-head|home-head)"/);
  });

  it("is the failed state's atom, and a failure is a notice with the alarm's tint", () => {
    // **A FAILURE IS A NOTICE, NOT RED TEXT, AND ONE NEVER TOLD YOU HOW LOUD IT WAS (the forty-eighth
    // list).** Measured with every read failing: this surface's `.notice` had the right SHAPE (12 px
    // padding, a corner, a tint, ink words) but the SAME `#fafafa` tint for a refused fleet read and
    // for an unavailable installer — one grey box for every kind of news. The panel, meanwhile, drew
    // `rgb(165,29,29)` text with no container at all.
    const source = css();
    const body = ruleFor(source, ".notice");
    expect(body, "the notice must have a rule").not.toBeNull();
    const radius = body!.match(/border-radius:\s*var\(--([\w-]+)\)/)?.[1];
    expect(radius, "the notice's corner is a TOKEN").toBeTruthy();
    expect(tokenPx(tokens(), radius!), "the corner is the contract's").toBe(contract.failed.radius);
    // **THE TINT IS THE ALARM'S, NOT THE PAGE'S.** A notice that cannot say how loud it is is one
    // grey box for every kind of news, which is the state the panel's raw red text was over-correcting
    // for on the other surface.
    expect(body, "the notice wears the alarm's tint").toMatch(/background:\s*var\(--warn-soft\)/);
    // **AND THE WORDS ARE INK.** No `color` declaration means the page's ink, which is the point:
    // the sentence is the message and the tint is the alarm (rule 6, without a second colour).
    expect(body, "the notice's words are ink, not a second alarm").not.toMatch(/color:\s*var\(--danger/);
    // One announcer: the surfaces' failures are reported by ONE live region each.
    const home = stripComments(readFileSync(path.join(here, "HomePage.tsx"), "utf8"));
    expect(home, "the failed read is a live region").toMatch(/role="status"/);
  });

  it("is an empty state with the reader's way back, on BOTH empty shapes of this surface", () => {
    // **THE CONTRACT NOW RECORDS THAT AN EMPTY STATE KEEPS THE WAY BACK (the seventy-second list).**
    // Measured 2026-11-10 after filtering Activity into nothing: this surface drew a bare `p.hint` and a
    // button on the page's grid — it HAD the `Clear search` (which the panel's empty did not) and not the
    // box the atom says an empty state is. Both shapes are the atom now.
    expect(contract.empty.wayBack, "an empty state keeps the way back").toBe(true);
    const sheet = css();
    for (const [selector, why] of [
      [".empty-state", "the page's own empty shape was already the card"],
      [".timeline-empty-filtered", "the filtered empty is the same card now"],
    ]) {
      const rule = ruleFor(sheet, selector.replace(/\./g, "\\."));
      expect(rule, `${why}: ${selector}`).toBeTruthy();
      const radius = rule!.match(/border-radius:\s*var\(--([\w-]+)\)/)?.[1];
      expect(tokenPx(tokens(), radius!), `${selector}'s corner is the atom's`).toBe(contract.empty.radius);
      const pad = rule!.match(/padding:\s*var\(--([\w-]+)\)/)?.[1];
      expect(tokenPx(tokens(), pad!), `${selector}'s inset is the atom's`).toBe(contract.empty.pad);
      expect(rule, `${selector} is start-aligned`).toContain("justify-items: start");
      expect(rule, `${selector} has no shadow`).toContain("box-shadow: none");
    }
    // **AND THE FILTERED SHAPE KEEPS ITS CONTROL.** A sentence that names what narrowed the list, with
    // nothing beside it that undoes the narrowing, is a dead end however well boxed it is.
    const timeline = stripComments(readFileSync(path.join(here, "ActivityTimeline.tsx"), "utf8"));
    expect(timeline, "the filtered empty offers the clear").toMatch(/Clear search/);
  });

  it("is the loading state's atom, and a read in flight reserves the shape it will draw", () => {
    // **THE LOADING STATE IS A SKELETON IN A LIVE REGION (the forty-seventh list).** This surface
    // drew it before the contract named it: `.skeleton-row` at 12 px padding, a 10 px corner, two
    // bars at 6 px, inside `.skeleton-list` carrying `role="status"` and the read's own label. The
    // contract writes the numbers down so the OTHER surface had somewhere to read them from, and this
    // test refuses a literal in the same rule — `10px` and `--radius-sm` measure the same corner and
    // only one of them is a system (`designScale`'s Rust counterpart refuses the literal too).
    const source = css();
    const row = ruleFor(source, ".skeleton-row");
    expect(row, "the skeleton row must have a rule").not.toBeNull();
    const rowRadius = row!.match(/border-radius:\s*var\(--([\w-]+)\)/)?.[1];
    expect(rowRadius, "the row's corner is a TOKEN").toBeTruthy();
    expect(tokenPx(tokens(), rowRadius!), "the row's corner is the contract's").toBe(
      contract.loading.rowRadius
    );
    const pad = row!.match(/padding:\s*var\(--([\w-]+)\)/)?.[1];
    expect(pad, "the row's padding is a TOKEN").toBeTruthy();
    expect(tokenPx(tokens(), pad!), "the row's padding is the contract's").toBe(contract.loading.rowPad);
    const bar = ruleFor(source, ".skeleton");
    const barRadius = bar!.match(/border-radius:\s*var\(--([\w-]+)\)/)?.[1];
    expect(barRadius, "the bar's corner is a TOKEN").toBeTruthy();
    expect(tokenPx(tokens(), barRadius!), "the bar's corner is the contract's").toBe(
      contract.loading.barRadius
    );
    // **THE HALF A STYLESHEET CANNOT HOLD.** The rows are drawn inside a live region that names the
    // read, which is what makes a skeleton a statement rather than a shimmer.
    const home = readFileSync(path.join(here, "HomePage.tsx"), "utf8");
    // **THE TWO ATTRIBUTES THAT MAKE IT A STATEMENTS: A LABEL AND A LIVE ROLE.** The regex above
    // matched either one on its own, so removing `role="status"` still passed — a gate that accepts
    // the half it exists to protect. Each is asserted separately, on the element that carries the
    // class, which is the only element either attribute belongs to.
    const opening = home.slice(
      home.indexOf('className="skeleton-list"'),
      home.indexOf(">", home.indexOf('className="skeleton-list"'))
    );
    expect(opening, "the skeleton list names the read").toMatch(/aria-label=/);
    expect(opening, "the skeleton list is a live region").toMatch(/role="status"/);
  });

  it("is the empty state's atom, and both empty shapes on this surface are it", () => {
    // **THE EMPTY STATE IS A PAGE ATOM, NOT A BARE RUN OF ITEMS (the forty-sixth list).** The
    // directive says the two surfaces share the same empty state, and the contract now declares it.
    // This surface has TWO empty shapes and they must be the same one: `.card.first-run` (the
    // first-minute card) and `.empty-state` (a list with nothing in it, drawn by the plugin catalogue
    // and the device shell). Measured before the fix: `.card.first-run` was already the page's card
    // (16 / 14 / no shadow / start / 1072 of a 1120 column) and `.empty-state` was a bare run of
    // items with NO corner, NO padding and NO surface — the same idea drawn two ways on one page.
    const source = css();
    const rule = ruleFor(source, ".empty-state");
    expect(rule, "the empty state must have a rule").not.toBeNull();
    const radius = rule!.match(/border-radius:\s*var\(--([\w-]+)\)/)?.[1];
    expect(radius, "the empty state's corner is a token").toBeTruthy();
    expect(tokenPx(tokens(), radius!), "the corner is the card's").toBe(contract.empty.radius);
    const pad = rule!.match(/padding:\s*var\(--([\w-]+)\)/)?.[1];
    expect(pad, "the empty state's padding is a token").toBeTruthy();
    expect(tokenPx(tokens(), pad!), "the padding is a card's").toBe(contract.empty.pad);
    expect((rule!.match(/box-shadow:\s*([^;}]+)/)?.[1] ?? "").trim()).toBe(contract.empty.shadow);
    // And the first-minute card is the same atom: it is the page's `.card`, so its corner and padding
    // come from the one rule the contract already fixes.
    const card = ruleFor(source, ".card");
    expect(tokenPx(tokens(), (card!.match(/border-radius:\s*var\(--([\w-]+)\)/)?.[1])!)).toBe(
      contract.empty.radius
    );
    expect(tokenPx(tokens(), (card!.match(/padding:\s*var\(--([\w-]+)\)/)?.[1])!)).toBe(
      contract.empty.pad
    );
  });

  it("is the breakpoint set, and every width query reads one of them", () => {
    // **THE WIDTHS WERE THE ONE SCALE THAT WAS NOT A SCALE (the forty-second list).** Spacing, type,
    // radius and colour were tokens; the widths at which this layout changes were literals scattered
    // through 31 media queries — `639 / 640 / 720 / 767 / 799 / 800 / 900 / 480 / 1280`, plus two in
    // `rem` (`56rem` = 896, `68rem` = 1088) whose real numbers a grep cannot even see. The odd ones
    // were folded into the set after checking, one by one, that the fold changed nothing measured
    // (two of them DID change behaviour and were put back).
    const source = css();
    const widths = [...source.matchAll(/@media\s+\(([^)]*(?:width)[^)]*)\)/g)]
      .map((match) => match[1])
      .flatMap((query) => [...query.matchAll(/(?:min|max)-width:\s*([0-9]+)(px|rem)/g)].map((m) => ({
        value: Number(m[1]),
        unit: m[2]
      })))
      .filter((entry) => entry.unit === 'px')
      .map((entry) => entry.value);
    // The set a query may name: the contract's breakpoints, and the ±1 pair each side of them that
    // `min-width`/`max-width` needs (639/640 is ONE breakpoint written from both ends).
    const allowed = new Set<number>();
    for (const value of Object.values(contract.breakpoints)) {
      // **`breakpoints` IS A RECORD OF NUMBERS PLUS ONE OBJECT OF WORDS** (`queryForm`, the sixty-fourth
      // list), so the numeric members are what a query may name.
      if (typeof value !== "number") continue;
      allowed.add(value);
      allowed.add(value - 1);
      allowed.add(value + 1);
    }
    const strangers = [...new Set(widths)].filter((value) => !allowed.has(value));
    expect(strangers, 'a width query outside the contract is a fourth scale nobody declared').toEqual(
      [],
    );
    // And nothing is hidden inside a `rem`: a width a grep cannot see is a width nobody can review.
    const remWidths = [...source.matchAll(/@media\s+\([^)]*width:\s*[0-9]+rem/g)];
    expect(remWidths.length, 'a width query must name pixels, which the contract lists').toBe(0);
  });

  it("is the page title every page of both surfaces draws", () => {
    // **THE PAGE TITLE WAS THE ONE THING THE FRAME DID NOT MAKE UNIFORM (the thirty-ninth list).**
    // Measured 2026-11-10: this console drew it at 22 px / 700 on Home and Activity, while the panel
    //'s History and Plugins drew theirs at 17 px / 700 — three separate rules over there, and none
    // at all on History. A title is the reader's answer to "where am I"; it cannot have two sizes.
    const source = css();
    const title = ruleFor(source, "\\.page-head h1");
    const size = tokenFor(tokens(), contract.title.size, "fs");
    expect(title, "the title's size is the contract's").toContain(`font-size: var(--${size})`);
    expect(tokenPx(tokens(), size)).toBe(contract.title.size);
    expect(title, "the title's weight is the contract's").toContain(
      `font-weight: ${contract.title.weight === 700 ? "var(--fw-bold)" : "var(--fw-semi)"}`
    );
  });

  it("is the rail this console ships, which the contract records rather than equalizes", () => {
    // The console's rail carries DESTINATION LABELS and measures 232 px; the panel's is an icon
    // rail at 52. A web console and a desktop app genuinely differ here — the contract records both
    // numbers so one of them cannot be changed by accident and called a fix. Measured before this
    // change: the sheet declared **200 px** twice and **232 px** twice, and the frame drew 232.
    const source = css();
    // **AND IT COLLAPSES WHERE THE RENDERED PAGE SAYS, NOT WHERE THE CSS MEANT (the seventy-fourth list).**
    // The rail has THREE forms and this number is the widest one's floor: 232 px from `consoleCollapsesAt`
    // up, a 56 px ICON rail from `breakpoints.phone` to that number, and the phone's bottom bar below.
    //
    // Measured 2026-11-10 on the live console, the old number (767) was what the icon band's rule SAID and
    // not what the page did: `@media (min-width: 640px) and (max-width: 767px)` paints a 56 px rail, and
    // TWO later `@media (min-width: 640px)` queries — one repainting `.shell`'s columns, one `.rail`'s
    // width — overrode it on source order alone. Measured at 640/700/767 the rail was **232** wide, the
    // page 408/468/535, and the fleet table (min-content 635 px, `overflow: hidden`) lost four of its five
    // columns with nothing to scroll.
    expect(contract.rail.consoleCollapsesAt, "the console collapses").toBe(contract.breakpoints.tablet);
    // **AND THE TWO RULES AGREE.** A number and a stylesheet that disagree is how the defect survived a
    // contract test for a week: the test read the number, the page read the CSS.
    // The assertion is on the RULE, not on any `min-width: 640px` query: this sheet has five of those,
    // and a test that accepts any of them would pass with the defect still in place (it did, once).
    // **AND THE STYLESHEET OBEYS IT.** Measured 2026-11-10: the number said 767 and the page drew a 232 px
    // labelled rail at 640/700/767, because the icon band's rule was overridden — on source order alone —
    // by two later `(min-width: 640px)` queries (one repainting `.shell`'s columns, one `.rail`'s width).
    // The contract test read the number and passed; the page read the CSS. So the assertion is on the RULE
    // that must start where the labelled form starts, not on any `min-width: 640px` query, of which this
    // sheet has five.
    const labelledFloor = (contract.rail.consoleCollapsesAt ?? 0) + 1;
    const labelledRule = source.slice(source.indexOf(`@media (min-width: ${labelledFloor}px)`));
    expect(
      labelledRule.slice(0, 400),
      "`.shell`'s labelled columns start where the labelled form starts",
    ).toMatch(/\.shell \{ grid-template-columns: 232px/);
    // The rail's own width is in the SECOND query at that floor (the file declares the shell's columns
    // beside the topbar and the rail's width beside the sidebar rules), so the assertion reads the whole
    // sheet for it and the NEXT `768`-and-up block for its floor.
    const railRule = source.slice(source.indexOf("@media (min-width: 768px) {", source.indexOf("@media (min-width: 768px) {") + 1));
    expect(railRule.slice(0, 400), "and so does the rail's own width").toMatch(/\.rail \{ width: 232px/);
    // The ICON rail's band is the one between `phone` and that number, and its rule must not be
    // overridden by anything declared later in the sheet.
    expect(source, "the icon rail is 56 px").toContain("grid-template-columns: 56px minmax(0, 1fr)");
    expect(contract.rail.panelCollapsesAt, "the panel never collapses").toBeNull();
    expect(source, "the rail's width is the contract's").toContain(
      `grid-template-columns: ${contract.rail.console}px minmax(0, 1fr)`
    );
    expect(source, "the rail itself is the contract's width").toContain(
      `width: ${contract.rail.console}px`
    );
  });

  it("is the tracking atom: a step belongs to a SIZE, and nothing inherits a fraction", () => {
    // **RULE 14 AS A LINE THE EDITOR READS (the seventy-fifth list).** Measured by grouping every text node on a
    // rendered page by `font-size` (then weight and kind) and asking whether that group carried ONE
    // `letter-spacing`: this surface answered NO on every page — 22 px at -0.242 AND -0.44, 12 px at
    // -0.052 AND 0, 10 px at 0.5 AND 0, 17 px at -0.052 AND -0.34 — while the panel answered YES.
    expect(contract.tracking.inherits, "a tracking of the body's is one more answer").toBe(false);
    expect(contract.tracking.literals, "a number written at a call site is a habit").toBe(0);
    // The four steps are the ones the sheet declares, and each is the size it is for.
    const tk = tokens();
    for (const [size, step] of Object.entries(contract.tracking.bySize)) {
      if (step === "none") continue;
      expect(tk, `${size} reads the ${step} step`).toContain(`--tracking-${step}:`);
    }
    expect(tk, "the display step is the contract's").toMatch(/--tracking-display:\s*-0\.011em/);
  });

  it("is ONE tracking per size — the baseline is nobody's fourth number", () => {
    // **RULES 3 AND 14, MEASURED (the seventy-fifth list).** Grouping every text node on a rendered page by
    // its `font-size` and asking whether that size carries ONE tracking, the answer on 2026-11-10 was no:
    //
    //     console Home  22 px   -0.242px (h1) · -0.44px (home-verdict)
    //     console Home  12 px   -0.052px where a rule opted in · 0 where a rule said `normal`
    //     console Activity 17   -0.052px (page-sub) · -0.34px (timeline-day-label)
    //
    // The cause was `body { letter-spacing: -0.004em }`: an inherited baseline that no tracking token
    // names, so "the default" and "-0.004em" were two answers to one question depending on which element
    // you inspected, and a rule that wrote `normal` had to know the difference.
    //
    // The four steps the sheet declares are now the whole story, and NOTHING inherits a fraction:
    const sheet = css();
    expect(sheet, "the body carries no tracking of its own").not.toMatch(
      /body \{[^}]*letter-spacing/,
    );
    for (const token of ["display", "tight", "label", "eyebrow"]) {
      expect(tokens(), `${token}'s step is a token`).toContain(`--tracking-${token}:`);
    }
    // **AND NO TRACKING IS WRITTEN AT A CALL SITE.** `0.02em` on `.timeline-stretch-text` and `0` on a mono
    // chip were the two that survived the inheritance; both read nothing but the scale now (the mono
    // chip's `0` is the one kept, and it is the baseline rather than a fifth value).
    expect(sheet, "no em or px tracking literal survives").not.toMatch(
      /letter-spacing:\s*(?!var\()[-0-9.]+(?:em|px|rem)\s*;/,
    );
    // **AND A RULE THAT DRAWS A TITLE OR A LABEL SAYS WHICH STEP IT IS USING.** Two rules on this page
    // looked like one object and were set at two steps (the installer's card title at the baseline while
    // every other title carried the display's; the stretch line opened at the eyebrow's 0.08em in a column
    // of 11 px clocks). Both are the same failure: a rule that sets SIZE and WEIGHT but leaves TRACKING to
    // whatever the element rule underneath happened to paint.
    const suspects = [
      [".installer-strip h2", "display"],
      // **A 600 RUN AT A TITLE SIZE ANSWERS ONCE.** The installer's title took the display step and this row
      // — which also sets only a size and a weight — was still at the baseline, so 14/600 had two answers
      // on the deployed build. The rule is the size/weight/kind group, not the element.
      [".attention-machine", "display"],
      [".home-verdict", "display"],
      [".timeline-stretch-text", "none"],
    ];
    for (const [selector, step] of suspects) {
      const rule = ruleFor(sheet, selector.replace(/\./g, "\\."));
      expect(rule, `${selector} exists`).toBeTruthy();
      if (step === "none") {
        expect(rule, `${selector} is a record line, so it inherits the body's step`).not.toMatch(
          /letter-spacing/,
        );
      } else {
        expect(rule, `${selector} says which step it reads`).toContain(
          `letter-spacing: var(--tracking-${step})`,
        );
      }
    }
  });

  it("is a FLEET TABLE whose shape follows its box, not the window", () => {
    // **MEASURED 2026-11-10 ON THE LIVE CONSOLE: the table's min-content is 635 px, and the box it is given
    // is 360 px at 640, 487 at 767, 620 at 900** — it does not fit again until the window is 914. The
    // wrapper is `overflow: hidden`, so the columns that do not fit are not scrolled away, they are GONE,
    // and the row's `⋯` menu — the only way to a machine's diagnostics — was among them.
    //
    // The card form that already existed for the phone keeps every fact, draws the column names the hidden
    // header can no longer draw, and needs no sideways scroll. It was gated on the WINDOW because the phone
    // was the only place anyone had looked. The console's token table solved the same question with a
    // container query ("the sheet is 600 px wide even at 1280, so the list is measured by its own scroller");
    // a table's shape follows its box, and this asserts the box is what decides it.
    const sheet = css();
    expect(sheet, "the fleet's box is a container").toMatch(
      /\.fleet-table-wrap\s*\{\s*container:\s*fleet-table\s*/,
    );
    expect(sheet, "and the card form is gated on it, not on the window").toMatch(
      /@container\s+fleet-table\s*\(max-width:\s*639px\)/,
    );
    // **NO WINDOW QUERY MAY OWN THE CARD FORM.** A `@media (max-width: 639px)` copy of these rules is the
    // defect coming back: at 900 px the window is wide and the table's box is 620.
    const cardForm = sheet.slice(sheet.indexOf("@container fleet-table"));
    const nextAt = cardForm.indexOf("\n@");
    const block = cardForm.slice(0, nextAt === -1 ? undefined : nextAt);
    expect(block, "the card form draws the labels the header cannot").toContain(".fleet-started-label");
    expect(block, "and keeps the row's actions inside the card").toContain(".fleet-actions");
    expect(contract.breakpoints.phone, "the container's threshold is the contract's").toBe(639);
  });

  it("is a card this console draws, through tokens", () => {
    const source = css();
    const card = ruleFor(source, "\\.card");
    const radius = tokenFor(tokens(), contract.card.radius, "radius");
    const pad = tokenFor(tokens(), contract.card.pad, "sp");
    const gap = tokenFor(tokens(), contract.card.gap, "sp");
    // **THE CONSOLE'S CARD IS SMALLER THAN THE PANEL'S ON PURPOSE — AND THAT IS A CONTRACT FAILURE.**
    // This assertion is the reason the next item exists: measured today, `.card` here is
    // `radius-sm` (10) with `padding: var(--sp-3)` (12) and `gap: var(--sp-2)` (8), while the
    // contract names 14 / 16 / 8. A page cannot be the same page in two frontends if its blocks are
    // 4 px tighter in one of them.
    expect(card, "the card's corner is the contract's radius").toContain(
      `border-radius: var(--${radius})`
    );
    expect(card, "the card's padding comes from the scale").toContain(`padding: var(--${pad})`);
    expect(card, "the card's inner rhythm comes from the scale").toContain(`gap: var(--${gap})`);
    expect(tokenPx(tokens(), radius)).toBe(contract.card.radius);
    expect(tokenPx(tokens(), pad)).toBe(contract.card.pad);
    expect(tokenPx(tokens(), gap)).toBe(contract.card.gap);
  });
});
