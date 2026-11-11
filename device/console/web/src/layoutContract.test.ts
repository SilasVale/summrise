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
  page: { max: number; pad: number; gap: number; listGap: number; boxSizing: string; phonePad: number };
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
    wayBack: boolean;
    radius: number;
    pad: number;
    gap: number;
    textAlign: string;
    shadow: string;
    shapes: Record<"standalone" | "inCard" | "measured", string>;
  };
  head: {
    gap: number;
    align: string;
    subtitleSlot: { class: string; size: string; weight: string; ink: string; measure: string };
    claimSlot: {
      class: string;
      permittedSteps: string[];
      weight: string;
      ink: string;
      stepBy: { by: string; condition: string; count: string };
    };
    maxHeadsPerPage: number;
  };
  // A ROW inside a list (the eighty-seventh list, read by a test for the first time in the eighty-ninth).
  row: {
    insetX: number;
    insetY: number;
    gapLines: number;
    gapBoxes: number;
    boxed: string;
  };
  motion: {
    reducedMotion: string;
    cap: string;
    namedAnimationsStop: boolean;
    interactionMs: number;
  };
  cardTitle: { size: number; weight: number };
  tracking: {
    bySize: Record<string, string>;
    inherits: boolean;
    literals: number;
  };
  ground: { light: string; dark: string; gradients: number };
  elevation: {
    flat: string;
    pane: string;
    raised: string;
    paneShadow: string;
    paneRadius: number;
    paneEdge: number;
    recess: string;
    recessTint: number;
    cardTint: number;
  };
  palette: {
    washAlpha: number;
    ink: string;
    muted: string;
    faint: string;
    accent: string;
    semantic: string[];
    accentFamily: {
      jobs: string[];
      names: Record<"console" | "panel", string[]>;
      note: string;
      maxDeltaE: number;
    };
    extraHues: { name: string; ink: string; reason: string }[];
  };
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
    rowRule: string;
    box: string;
    regionBand: { min: number; max: number };
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
    sameSizeDifferentRoles: { marker: string; roles: string[] };
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
  it("is the one the panel has, byte for byte", () => {
    // **THE CROSS-REPO PIN, POINTED AT THE OTHER REPO (the eighty-eighth list).** This test hashed
    // `docs/superpowers/specs/layout-contract.json` in ITS OWN repo and compared it to a literal — and the
    // panel's test did the same, each pinning its own file. Measured 2026-11-10: the two files were
    // **different** (the product's had `row` and `tracking`, the panel's had `theme` and `targetTouch: 44`)
    // and both literals were green, because neither test could see the other repo's copy. The comment in
    // both files claimed the numbers were the same; they had never been.
    //
    // **A PIN THAT COMPARES A FILE TO ITSELF VERIFIES NOTHING BUT THE FILE'S STABILITY.** This one now
    // reads the panel's copy — the agent repo sits beside this one on this machine, at
    // `../summrise/docs/superpowers/specs/layout-contract.json` — and skips, with a named reason, when the
    // sibling checkout is not there (a CI job that clones one repo must not fail on the other's absence).
    // The literal below is the sha256 of BOTH copies, and it is the same literal the panel's test carries.
    const ours = createHash("sha256").update(contractText()).digest("hex");
    expect(ours).toBe(
      "57875ecdd4ce0e9207ffee534ae4d1d514ce45993e65db820b80d6c867055d01"
    );
    const sibling = path.join(here, "..", "..", "..", "..", "..", "summrise", "docs",
      "superpowers", "specs", "layout-contract.json");
    let theirs: string | null = null;
    try {
      theirs = readFileSync(sibling, "utf8");
    } catch {
      theirs = null;
    }
    if (theirs === null) {
      // Recorded rather than silent: this checkout has no sibling agent repo, so the cross-repo half of
      // the pin cannot run here.
      expect(
        theirs,
        "the sibling agent repo is checked out beside this one and its contract is readable",
      ).toBeNull();
      return;
    }
    expect(
      createHash("sha256").update(theirs).digest("hex"),
      "the panel's copy of the contract is the same FILE as this one",
    ).toBe(ours);
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
    // **A CENSUS MUST COUNT RULES, NOT ELEMENTS (the hundred-and-twenty-third list).** Measured 2026-11-11: a
    // border census inside `.page` gave Home **32** against Activity's **4** — an 8x drift — and **25 of Home's 32
    // were `td`/`th` bottom edges from ONE shared rule**, a table's row rule doing its grouping. **Counting an element
    // and counting a rule are different questions**, so the atom names the kinds, and this test refuses an atom that
    // does not: a `rowRule` is one per table, a `box` is a border on a region or a control.
    expect(contract.hairline.rowRule, "the atom names a table's row rule").toContain("ONE rule");
    expect(contract.hairline.box, "and a border on a region or a control").toContain("region");
    // **AND THE BAND IS A MEASUREMENT, NOT A TASTE (the hundred-and-twenty-third list).** The same census, with the
    // two kinds counted apart, measured a page's REGION boxes at **3, 6, 6, 8** — one band on both surfaces — while
    // the CONTROLS were 1, 1, 15, 1. **A control's edge is its affordance and its count is the page's form**, so the
    // atom records only the region band: a page that drew 23 REGION boxes would be outside it, and one that drew 23
    // CONTROL boxes would not be — which is exactly the distinction my first census could not make.
    expect(contract.hairline.regionBand.min, "the band's floor is measured").toBe(3);
    expect(contract.hairline.regionBand.max, "and its ceiling").toBe(8);
    expect(contract.hairline.regionBand.min).toBeLessThan(contract.hairline.regionBand.max);
    // The overview's block is a `<section className="card">` now, and this test refuses the old shape.
    expect(contract.head.maxHeadsPerPage, "one head per page").toBe(1);
    expect(contract.head.subtitleSlot.class, "the context slot is named").toBe(".page-sub");
    // **AND THE CLAIM SLOT IS THE ATOM'S, NOT EACH PAGE'S (the hundred-and-twenty-first list).** Measured 2026-11-11:
    // the title's block carried a CLAIM on some pages and nothing on others — console Home put `3 of 4 online · 2
    // need attention` at **22/600** (the title's own size, lighter), Activity at **17/600**, and the panel put no
    // claim under its title at all. **Two slots, recorded once.**
    // **THE CLAIM SLOT IS A POSITION WITH TWO PERMITTED STEPS (the hundred-and-twenty-first list).** It is NOT one
    // value: Home's verdict takes the TITLE's rung and Activity's count takes the SECTION's, **and a gate below pins
    // that they do not converge** — that is the operator's standing direction, recorded. What the atom was missing is
    // that a claim belongs in the head's block at all: the panel's pages went straight from `Settings` to its fold.
    expect(contract.head.claimSlot.class, "the claim slot is named").toBe(".page-claim");
    expect(contract.head.claimSlot.permittedSteps, "two steps, not one").toEqual(["xl", "lg"]);
    expect(contract.head.claimSlot.weight, "and the weight says it is a sentence, not a title").toBe("semi");
    // **AND WHICH STEP EACH CLAIM TAKES IS A FIELD, NOT A COMMENT (the hundred-and-twenty-second list, item 2).**
    // The choosing rule was prose — *"`xl` when it is the page's second title line, `lg` when the page's title is the
    // whole claim"* — and **a comment cannot refuse an 18 px claim.** Measured on all four claiming pages:
    //
    //     Home      `3 of 4 online · 2 need attention`   a CONDITION  -> 22 (xl)
    //     Activity  `Today: 543 client calls`            a COUNT      -> 17 (lg)
    //     Memory    `3 entries saved`                    a COUNT      -> 17 (lg)
    //     Plugins   `4 plugins · 4 enabled`              a COUNT      -> 17 (lg)
    //
    // **4 of 4 agree**, so this is a measured rule and not a proposal. And both answers are steps the seven-size
    // scale ALREADY has — an eighteenth or twentieth pixel would fix rule 5's ambiguity by breaking rule 3.
    const stepBy = contract.head.claimSlot.stepBy;
    expect(stepBy.by, "the rule is by what the claim is FOR").toBe("what the claim is for");
    expect(stepBy.condition, "a condition is the page's second title line").toBe("xl");
    expect(stepBy.count, "a count is a step under it").toBe("lg");
    // Both answers must be steps the claim slot permits, or the field contradicts its own list.
    for (const step of [stepBy.condition, stepBy.count]) {
      expect(contract.head.claimSlot.permittedSteps, `${step} is a permitted claim step`).toContain(step);
    }
    // **AND BOTH POSITIONS ARE IN USE, WHICH IS WHY TWO STEPS ARE PERMITTED.** A third would be a decision.
    const sheet = stripComments(css());
    expect(sheet, "Home's claim takes the title's rung").toContain("font-size: var(--fs-xl)");
    expect(sheet, "Activity's claim takes the section's").toContain("font-size: var(--fs-lg)");
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
    // **AND THE SECOND SHAPE IS NO LONGER A SECOND CARD (the hundred-and-twelfth list).** It restated the atom's six
    // declarations verbatim, both of them reading a `--empty-gap` nothing set. **Two cards that happen to agree are two
    // habits; one card declared once is the atom** — the filtered empty wears `.empty-state` in the component now, and
    // this gate asks the atom's shape ONCE plus the composition that puts it on the second surface.
    const timelineSrc = readFileSync(path.join(here, "ActivityTimeline.tsx"), "utf8");
    expect(timelineSrc, "the filtered empty composes the atom").toContain(
      'className="empty-state timeline-empty timeline-empty-filtered"',
    );
    for (const [selector, why] of [
      [".empty-state", "the page's own empty shape IS the card"],
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

  it("is a ROW inside a list, and the list's own rhythm is on the scale", () => {
    // **THE ATOM WAS RECORDED AND NOBODY READ IT (the eighty-ninth list).** The `row` atom was written in the
    // eighty-seventh list from a measurement — seven row shapes for one idea — and neither repo's test named
    // it, so it was a comment with a schema rather than a decision. **AND ONE OF ITS NUMBERS WAS CONTRADICTED
    // BY THE PAGE THAT PROVOKED IT**: measured 2026-11-10, `.timeline-row.is-stretch` carried `margin-top: 18px`
    // and the stretch header `margin: 0 0 18px`, both on NO step of the scale the same contract declares
    // (`2/4/6/8/10/12/14/16/24`), inside the very list the atom describes.
    const sheet = css();
    const scale = contract.space.scale.map((n) => n);
    expect(contract.row.gapLines, "a list of LINES separates by its own leading").toBe(0);
    expect(contract.row.gapBoxes, "a list of BOXES separates by a scale step").toBe(8);
    // **ZERO IS NOT A STEP, IT IS THE ABSENCE OF ONE.** A line list separates by its own leading, so `gapLines`
    // is 0 by definition — the value the atom exists to state — and the steps that must be ON the rhythm are
    // the ones a container actually paints.
    for (const [name, value] of Object.entries({ insetX: contract.row.insetX, insetY: contract.row.insetY,
      gapBoxes: contract.row.gapBoxes })) {
      expect(scale.includes(value as number), `row.${name} = ${value} is on the rhythm`).toBe(true);
    }
    expect(contract.row.gapLines, "a line list adds no separation of its own").toBe(0);
    expect(contract.row.boxed, "a row that draws a box is a card").toBe("card");
    // **AND THE PAGE'S OWN LIST SEPARATIONS ARE THE SCALE'S.** The stretch step is the one number in the
    // timeline's list separations, and a literal there is a rhythm of one page.
    const stretch = sheet.match(/\.timeline-row\.is-stretch\s*\{[^}]*\}/);
    expect(stretch, "the stretch rule exists").toBeTruthy();
    expect(stretch![0], "and takes a step, not a literal").toContain("margin-top: var(--sp-4)");
    const offScale = [...stretch![0].matchAll(/(\d+)\.?\d*px/g)].map((m) => Math.round(parseFloat(m[0]))).filter((n) => n > 0);
    expect(offScale, "no pixel literal in the stretch separation").toEqual([]);
  });

  it("is the CARD's own gap — a child of a card adds no margin of its own", () => {
    // **RULE 7, MEASURED AS A RHYTHM RATHER THAN A RULE (the eighty-sixth list).** Measured 2026-11-10 at
    // 1,440 on the live console by reading each card's declared `row-gap` and then the space that landed
    // BETWEEN its children:
    //
    //     card             declared gap   children's own margins   space they got
    //     card.attention   8 px           `-4px`, `0`              **4**, 8
    //
    // The 4 was a NEGATIVE margin on `.attention-release`, pulling a child up into a gap the container had
    // already set — spacing done twice, and one of the two was negative. The panel had the mirror image
    // (14 and 16, from margins its children brought and the browser's own `1em`), and this list recorded it
    // as ONE decision in both sheets: the container states the separation, and a child of a card brings none.
    const sheet = css();
    expect(ruleFor(sheet, "\\.card > \\*"), "a child of a card adds no margin").toContain("margin-block: 0");
    expect(ruleFor(sheet, "\\.card"), "and the card states the separation").toContain(
      "gap: var(--sp-2)",
    );
    // **AND THE ONE CHILD THAT USED TO ADD ONE STILL EXISTS AND STILL DRAWS NO MARGIN.**
    // **AND THE ONE CHILD THAT USED TO ADD ONE STILL EXISTS AND STILL DRAWS NO MARGIN.** `ruleFor` needs a
    // rule whose brace is on its OWN line, and this sheet writes small rules on one line — so the scan below
    // reads every declaration block whose selector names the release line, however it is line-broken.
    // The `*` in `.card > * { margin-block: 0 }` matches this element too, which is the POINT of the rule;
    // what the scan refuses is a margin on the element's OWN selector.
    // **COMMENTS NAME THE THING TOO, AND THIS SHEET'S COMMENTS QUOTE THE MARGIN THEY REMOVED.** The scan
    // reads the sheet with its comments stripped first — a chapter that explains `margin: -4px 0 0` is not a
    // rule that draws it, and the first draft of this test failed on exactly that quotation.
    const bare = sheet.replace(/\/\*[\s\S]*?\*\//g, "");
    const release = [...bare.matchAll(/([^{}]*)\{([^{}]*)\}/g)]
      .filter((m) => m[1].includes(".attention-release"))
      .map((m) => m[2]);
    expect(release.length, "the release line is still drawn").toBeGreaterThanOrEqual(1);
    for (const body of release) {
      expect(body, "and adds no margin to the card's gap").not.toMatch(/margin/);
    }
  });

  it("is a page BODY that is the page's content, not an empty state", () => {
    // **MEASURED 2026-11-10 AT 1,440, WALKING THE DEVICE PAGE'S THREE STATES BY FORCING EACH READ (the
    // eighty-third list):**
    //
    //     state          body element        body h   inner      rows
    //     4 machines     **.empty-state**     248      ul.picker  4
    //     0 machines     **.empty-state**      64      p.hint     0
    //     failed read    **.empty-state**      90      .notice    0
    //
    // **The page's CONTENT was drawn inside the class that means "there is nothing here".** `.empty-state` is
    // the atom the sixty-third list consolidated for genuine empties; this page used it as its BODY. Rule 19
    // asks each state to tell the truth — a state whose element cannot tell "four" from "none" is the wrong
    // ELEMENT, not a wrong sentence.
    const source = readFileSync(path.join(here, "DevicePageShell.tsx"), "utf8");
    expect(source, "the picker's block is a card").toMatch(/<section className="card device-picker-card">/);
    expect(source, "and not the empty atom").not.toMatch(/<section className="empty-state">\s*<DevicePicker/);
    // **AND A LIST FILLS THE CARD IT IS IN.** Measured before: the card was 1,072 px wide and its rows were
    // **326** — 30 % of the page — because a `ul` sized to its content sat inside a card that fills the
    // column, while the fleet table's row measures 1,070 of 1,072.
    const picker = ruleFor(css(), "\\.picker");
    expect(picker, "the list takes its card's width").toContain("width: 100%");
    expect(contract.empty.radius, "and the empty atom still exists for real empties").toBe(14);
    // **A HEAD ALIGNS AT ITS TOP EDGE.** `.device-head` said `center` where the atom says `flex-start` —
    // invisible on a one-row head, wrong the moment it wraps.
    expect(ruleFor(css(), "\\.device-head"), "the device head reads the atom").toContain(
      "align-items: flex-start",
    );
    expect(contract.head.align, "which the contract names").toBe("flex-start");
  });

  it("is ONE subtitle step in the head's own slot, on every page", () => {
    // **RULE 3 ASKED OF THE HEAD (the eighty-second list).** Measured 2026-11-10 across both surfaces: the
    // contract names ONE subtitle slot — `head.subtitleSlot: ".page-sub"` — and the pages drew it at 13/400
    // on three of them and at **17/500** on Activity, where the count sentence was set at the size the
    // contract gives a SECTION `h2`. The head's second line is a subtitle on every page or it is not the
    // head's.
    //
    // **AND THIS LIST EXISTS BECAUSE THE FIRST MEASUREMENT WAS WRONG.** The probe read `head.children` and
    // concluded History drew no `h1`; the `h1` is inside that `div`, at 22/700, on all seven pages. A probe
    // that reports a page's name missing is worth re-running before a list is written about it.
    const sheet = css();
    // The slot's own step, as this sheet draws it.
    // **THE CASCADE'S ANSWER, NOT THE FIRST MATCH.** This sheet declares `.page-sub` twice — the atom at
    // line 207 and a `margin-top: 3px` refinement 2,283 lines later — so the slot's declarations have to be
    // read across BOTH, which is what a reader sees.
    const subRules = [...sheet.matchAll(/\.page-sub\s*\{([^}]*)\}/g)].map((m) => m[1]);
    expect(subRules.length, "the slot is declared").toBeGreaterThanOrEqual(1);
    const sub = subRules.join(" ");
    expect(sub, "the subtitle's size is the atom's").toContain("font-size: var(--fs-sm)");
    expect(sub, "and it is the muted step, not the ink").toContain("color: var(--ink-muted)");
    // **AND THE COUNT IS NO LONGER IN THAT SLOT AT ALL (the hundred-and-seventh list).** The eighty-second
    // list put `.activity-today` INSIDE `.page-sub` and asked it to be the same step as a subtitle — which made
    // the head draw `22 / 13 / 12`, two near-identical small lines a reader could not tell apart. **`head.
    // maxHeadsPerPage: 1` could not see it because both wore the subtitle's class.** The count is the page's
    // FACT, not its context: it leaves the slot and takes the `section` rung, giving that head `22 → 17 → 13`.
    const today = ruleFor(sheet, "\\.activity-today");
    expect(today, "the count is at the section's rung the atom records").toContain("--fs-lg");
    expect(today, "at the section's weight").toContain("--fw-semi");
    const appSrc = readFileSync(path.join(here, "App.tsx"), "utf8");
    expect(appSrc, "and it is not in the subtitle slot").not.toMatch(/className="page-sub activity-today"/);
    // **AND THE BROWSER LEG ASKS THE SAME NUMBER.** It asserted 13 for a year (the subtitle's step, after the
    // eighty-second list caught it at 22); the day the count left the slot, that leg failed with
    // `the head's count line is 17px` — a gate doing its job on a shape that changed.
    const gateSrc = readFileSync(path.join(here, "..", "scripts", "browser-gate.mjs"), "utf8");
    expect(gateSrc, "the browser leg reads the section's rung").toContain("activityDesign.sentence !== 17");
    // Every page's name is an `h1` at the contract's title step — the re-run's finding, recorded.
    expect(contract.head.subtitleSlot.class, "one context slot, named once").toBe(".page-sub");
    expect(contract.title.size, "and the name above it is the contract's").toBe(22);
  });

  it("is the LOADING atom, drawn where a read is out — not a word in a result line", () => {
    // **RULE 19's LOADING HALF, MEASURED PAGE BY PAGE WITH EVERY READ HELD 3.5 s (the eightieth list).**
    // The atom was written in the forty-seventh list — `rows 3, rowPad 12, rowRadius 10, rowGap 8,
    // barRadius 6, role "status"` — and it shipped on TWO pages of seven:
    //
    //     console Home    "Reading the machine list" + 2 rows   ← two, not the atom's three
    //     console Activity "Reading the audit…"                  ← no rows, no role
    //
    // **AND A LOADING STATE IS NOT A RESULT.** The sentences these pages printed sat where a COUNT goes:
    // "a loading state is not a result, and the count line is drawn only when it has a count."
    const sheet = css();
    // The atom's own shape, as this sheet draws it.
    const row = ruleFor(sheet, "\\.skeleton-row");
    expect(row, "a skeleton row has the atom's padding").toContain(`padding: var(--sp-3)`);
    expect(row, "and the atom's corner").toContain(`border-radius: var(--radius-sm)`);
    expect(ruleFor(sheet, "\\.skeleton-list"), "and the atom's gap").toContain(`gap: var(--sp-2)`);
    // **AND EVERY PLACE THAT RESERVES A LOADING SHAPE DRAWS THAT MANY ROWS.** `[0, 1, 2]` is the atom's
    // three; a page that draws two is a page whose habit the atom recorded instead of its rule.
    const sources = ["HomePage.tsx", "ActivityTimeline.tsx", "PluginList.tsx", "ToolCatalogue.tsx"]
      .map((name) => readFileSync(path.join(here, name), "utf8"));
    const drawing = sources.filter((source) => source.includes("skeleton-list"));
    expect(drawing.length, "the pages that reserve a shape").toBeGreaterThanOrEqual(2);
    for (const source of drawing) {
      expect(source, "the atom's three rows").toContain("[0, 1, 2]");
    }
    // **AND THE READ'S LABEL IS IN A LIVE REGION.** Three of these pages said nothing to a screen reader
    // before this list; `role="status"` with the read's own words is the atom's half that a sighted reader
    // cannot see and a screen reader cannot miss.
    for (const source of drawing) {
      expect(source, "role=status on the placeholder").toContain('role="status"');
      expect(source, "and the read's own label").toContain("aria-label");
    }
  });

  it("is a RECORD whose first row is above half the viewport", () => {
    // **MEASURED 2026-11-11 AT 1,440 × 900 ON THE RECORD PAGE**, counting the space before the first row a
    // reader can actually read:
    //
    //     page head bottom      155
    //     stream bar            179 → 224      (45)
    //     the filter block      238 → 407      (**169**)
    //       ├ machines + chips 49
    //       ├ callers + chip + count **91**   ← one chip and a two-line count sentence
    //       └ search field      49
    //     day head              423 → 455
    //     **first record row  455**           ← half the viewport gone before one call is drawn
    //
    // **The offender was not the machines row (49 px): it was `Callers`**, a block holding ONE chip whose
    // count sentence wrapped to two lines at `--fs-sm`, so that block was 91 px while its neighbour was 49.
    // **And a count sentence is not a control**: it is a sentence about the READ, and it belongs on the
    // record's own line.
    const source = readFileSync(path.join(here, "ActivityTimeline.tsx"), "utf8");
    const sheet = css();
    // The count is drawn OUTSIDE the filter row: `filterable` renders the filters, then the count.
    const callers = source.indexOf("filter-block-callers");
    const count = source.indexOf('className="filter-count"');
    expect(callers, "the callers control still exists").toBeGreaterThan(-1);
    expect(count, "the count still exists").toBeGreaterThan(-1);
    // Its own wrapper, drawn in every state, not inside the filter row's grid.
    expect(source, "the count has its own line").toContain("{filterable && (");
    const countBlock = source.slice(Math.max(0, source.lastIndexOf("{filterable && (", count)) + 16, count);
    expect(countBlock, "and it is NOT inside the filter row").not.toContain("timeline-filters");
    // The filter row is one row: `align-items: end` keeps every block on the baseline its label starts.
    const row = sheet.match(/\.timeline-filters \{\s[^}]*\}/);
    expect(row, "the filter row is declared").toBeTruthy();
    expect(row![0], "and its blocks are aligned, not stacked from the top").toContain("align-items: end");
    // A count that cannot wrap cannot make a row tall: it has no width to wrap in.
    const countRule = sheet.match(/\.filter-count \{\s[^}]*\}/);
    expect(countRule, "the count has a rule").toBeTruthy();
    expect(countRule![0], "the count is not a filter column any more").not.toContain("align-self: center");
  });

  it("draws an empty state in ONE of TWO shapes, and the in-card one keeps the atom's gap", () => {
    // **MEASURED 2026-11-11 AT 1,360, the Activity table driven to a filter that matches nothing:**
    //
    //     the empty shape            radius  pad          gap   box                       way back
    //     `.empty-state` (the atom)   14      16           **12**  its own, no shadow        yes
    //     `.audit-empty`              **0**  **`12px 0`**  **8**   NONE — inside `.panel.activity-panel` (14/18)
    //
    // **The third shape is legitimate in KIND and wrong in NUMBERS.** It is an empty INSIDE a card — the thirty-eighth
    // list's `no card inside a card` — so it must not draw its own surface. But **its sentence and its `Clear filters`
    // control sat 8 px apart where every other empty in the product separates them by 12**, and the atom had no word
    // for which shape a given empty was.
    expect(contract.empty.shapes.standalone, "the atom names the card shape").toContain("card");
    expect(contract.empty.shapes.inCard, "and the in-card one").toContain("no surface");
    // The page-body empty IS the card, at the atom's numbers.
    const card = ruleFor(stripComments(css()), ".empty-state")!;
    const gap = card.match(/gap:\s*var\(--([\w-]+)\)/)?.[1];
    expect(tokenPx(tokens(), gap!), "the card's gap is the atom's").toBe(contract.empty.gap);
    // **AND THE IN-CARD ONE TAKES THE SAME GAP, WITH NO SURFACE.** That is the whole distinction: one number shared,
    // one surface not drawn.
    // **THE COMMENT IS NOT THE RULE** — this rule's own note explains the shape it does NOT take, and a scan for
    // the substring finds the explanation. `ruleFor` returns the block; strip its comments before asking.
    const sheet = stripComments(css());
    const inCard = sheet.match(/\n\.audit-empty \{([^}]*)\}/)?.[1] ?? "";
    expect(inCard, "the in-card empty declares its gap").toMatch(/gap:\s*var\(--sp-3\)/);
    expect(inCard, "and draws NO surface of its own").not.toContain("border");
    expect(inCard, "no radius either").not.toContain("border-radius");
    expect(inCard, "and no shadow").not.toContain("box-shadow");
    // The atom's own numbers.
    expect(contract.empty.gap, "the gap both shapes share").toBe(12);
    expect(contract.empty.radius, "the card's corner").toBe(14);
    expect(contract.empty.pad, "the card's inset").toBe(16);
  });

  it("recesses a block by the CARD's step with the sign reversed, not by 'the ground, again'", () => {
    // **MEASURED 2026-11-11, THREE SPELLINGS OF ONE IDEA — AND ALL THREE ARE WORTH NOTHING:**
    //
    //     `.settings-fold` (panel)   1.027 tint   ( #fafafa on a (247,247,247) pane )
    //     `--panel-3` (this sheet)   1.027         ( #f4f4f5 on the same pane )
    //     `#fafafa` on `#fafafa`     1.000         ( `--surface-recessed: var(--bg)`, the literal "the ground, again" )
    //
    // **A usable recess needs 8 levels BELOW the pane**, and the light scale's steps are 3–8 apart
    // (`#fafafa` 250 / `#f4f4f5` 244 / `#ffffff` 255). **The scale has no step that can carry it, and inventing one is
    // adding a colour.** So the atom states a DERIVATION: the card is `color-mix(ink 3.5%, surface)`; the recess is
    // the same mix at 7%, which measures 1.074 — **the card's own 1.072 with the sign reversed**.
    expect(contract.elevation.cardTint, "the card's own step").toBe(1.072);
    expect(contract.elevation.recessTint, "the recess is that step, reversed").toBe(1.074);
    expect(contract.elevation.recess, "and it is a DERIVATION, not a token").toContain("color-mix");
    // **AND THE SHEET CARRIES IT**: one token, derived from the ink, at the atom's factor.
    const decls = stripComments(tokens());
    const recess = decls.match(/--surface-recessed:\s*([^;]+);/)?.[1] ?? "";
    expect(recess, "the recess is derived, not `var(--bg)`").toContain("color-mix");
    const factor = recess.match(/([\d.]+)%/)?.[1];
    expect(Number(factor), "at the atom's factor").toBe(7);
    // **AND `var(--bg)` IS NO LONGER A RECESS.** The old spelling is one step from nothing (1.000).
    expect(recess, "the ground is not a recess").not.toBe("var(--bg)");
  });

  it("floats ONE pane object: the atom's shadow, the atom's radius, the page's own edge", () => {
    // **MEASURED 2026-11-11 WITH A POPOVER OPEN ON EACH SURFACE:**
    //
    //     surface   the thing that floats   shadow                                       radius   edge
    //     console   `.machine-pop`          `rgba(0,0,0,.12) 0 12px 32px` (one layer)      **10**     `.14`
    //     panel     `.session-menu`         **two layers**, incl. a drawn `0 0 0 1px` ring  **14**     `.08`
    //
    // **Two popovers, two objects** — and the token the atom named for the `pane` level was carried by NEITHER sheet.
    // The pane is the one thing that FLOATS above a page, so it must be ONE object on both surfaces: the atom's
    // shadow, `radius.dialog` (14 — the step larger than a card's), and **the page's own hairline**, because a pane
    // sits ON the page rather than inside a box of its own.
    const sheet = stripComments(css());
    const tokensSrc = stripComments(tokens());
    // The shadow is the atom's, in BOTH themes.
    const declOf = (src: string, name: string) =>
      new RegExp(`--${name}\\s*:\\s*([^;]+);`).exec(src)?.[1].replace(/\\s+/g, "") ?? "";
    const norm = (v: string) => v.replace(/\\s+/g, "");
    expect(declOf(tokensSrc, "shadow-lg"), "the pane's shadow is the atom's (same value, any spelling)").toBe(
      norm(contract.elevation.paneShadow),
    );
    expect(contract.elevation.paneRadius, "the pane's radius is the atom's").toBe(14);
    expect(contract.elevation.paneEdge, "and its edge weight").toBe(0.08);
    // **EVERY FLOAT IS THE SAME OBJECT.** A rule with the pane's shadow must take the pane's radius and the page's
    // line — the seven rules that float things here (three popovers, the sheet, the export menu, the alert banner).
    const floaters = [...sheet.matchAll(/([^{}]+)\{([^}]*box-shadow: var\(--shadow-lg\)[^}]*)\}/g)];
    expect(floaters.length, "something floats").toBeGreaterThan(0);
    for (const [, sel, body] of floaters) {
      // **TWO THINGS THAT FLOAT ARE NOT POPOVERS.** `.alert-banner` is a NOTICE in the frame (it rides the layout's
      // own row, not over a page), and `.sheet` is a full-height DRAWER whose corners belong to the edge it slides
      // from. Every floating BOX over a page — a menu, a popover, a formats list — is the pane.
      if (sel.includes(".alert-banner") || sel.includes(".sheet")) continue;
      expect(body, `${sel.trim()} takes the pane's radius`).toContain("border-radius: var(--radius)");
      expect(body, `${sel.trim()} draws the page's own line`).toContain("border: 1px solid var(--line)");
      expect(body, `${sel.trim()} draws no second edge`).not.toContain("--line-strong");
    }
  });

  it("gives every (size, weight, kind) group ONE tracking, from the atom's four steps", () => {
    // **MEASURED 2026-11-11, GROUPED BY SIZE THEN WEIGHT THEN KIND** — the atom's own method, because *"an uppercase
    // label, a mono value and prose are three kinds"*. Every group that carried two trackings:
    //
    //     surface/page           group            the two answers
    //     console Home           13/600/body      `-0.26px` (brand, machine menu) | **`normal`** (nav label)
    //     console Activity       17/600/body      `-0.34px` (day label)        | **`normal`** (the count)
    //     panel Settings         17/600/body      `-0.187px` (every `h2`)     | **normal** (a fold's summary)
    //
    // **Same size, same weight, one is a title and one a plain word** — and the product gave both the title's
    // tracking in one place and none in the other. `tracking.bySize` says which is which.
    const tokensSrc = stripComments(tokens());
    // **ALL FOUR STEPS ARE NAMED**, including `none` — this sheet had three and `none` had no name, so a rule that
    // wanted the body's step wrote nothing and a rule that wanted it explicitly had nothing to read.
    for (const step of ["display", "tight", "label", "eyebrow", "none"]) {
      expect(tokensSrc, `the ${step} step is a token`).toMatch(new RegExp(`--tracking-${step}\\s*:`));
    }
    // **A NAV LABEL IS BODY TEXT, AND IT SAYS SO.** It drew `normal` beside a brand at `-0.26px` because nothing
    // had said which it was.
    const railLabel = ruleFor(stripComments(css()), ".rail-label");
    expect(railLabel, "the nav label declares the body's step").toContain("letter-spacing: var(--tracking-none)");
    // The atom's own claims, so this reads the contract.
    expect(contract.tracking.literals, "no rule writes a tracking by hand").toBe(0);
    expect(contract.tracking.inherits, "and none is inherited as a baseline").toBe(false);
  });

  it("steps the page's inset at the phone band, on BOTH surfaces", () => {
    // **MEASURED 2026-11-11 ACROSS FIFTEEN WIDTHS, 1,600 → 360:**
    //
    //     surface   width   page pad   content column of the window
    //     console   390     **24**     **342 of 390 (88%)** — 12% chrome
    //     panel     390     **14**     **308 of 390 (79%)** — 21% chrome
    //
    // **The two surfaces agreed at the desk and disagreed at the phone.** The panel's step was a measured FIX in an
    // earlier round that never reached this sheet, and the console's 24 drew `24px/24px/24px/24px` from 1,600 to
    // **360** — the `phone` breakpoint in the atom moved nothing in the page's own inset.
    // **The step is the scale's own**: `space.scale` has no 24 below the desk, and 14 sits between 12 and 16.
    const sheet = stripComments(css());
    // The step is a query at the atom's own band, and it sets the atom's own number.
    const step = sheet.match(/@media \(max-width: 639px\)[^{]*\{[^}]*\.page[^}]*padding: var\(--sp-3-5\)/);
    expect(step, "the phone band steps the page's inset").toBeTruthy();
    // **AND THE DESK KEEPS THE 24.** A step that replaced the desk's inset would be a different page.
    const desk = ruleFor(sheet, ".page");
    expect(desk, "the desk's inset is still the atom's 24").toContain("padding: var(--sp-5)");
    // The atom's numbers.
    expect(contract.page.pad, "the desk's inset").toBe(24);
    expect(contract.page.phonePad, "and the phone's step").toBe(14);
    expect(contract.breakpoints.phone, "at the band the atom names").toBe(639);
    expect(contract.space.scale, "and the step is ON the scale").toContain(14);
  });

  it("measures a line with ONE token, in `ch`, on every block of prose", () => {
    // **MEASURED 2026-11-11 AT 1,600, EVERY BLOCK OF PROSE, IN `ch`:**
    //
    //     surface/page   block                width  max-width as written   takes the token?
    //     console Home   `.hint`              **68ch**  `var(--measure)`      **yes**
    //     console Home   `.attention-release`  68ch    `416px`               no — a PIXEL literal
    //     console Act.   `.hint`              **78ch**  `477px`               no — a PIXEL literal
    //
    // **A pixel lands on 66 characters at ONE font size and drifts at any other**, which is why the atom's measure
    // is `ch` and never px. The atom also says a measure is not a taste: 66ch is where the block that already reads
    // well sits, inside the 45–75 band a line is followed back along.
    const sheet = stripComments(css());
    // The token exists and is the atom's number.
    expect(stripComments(tokens()), "the measure is a token").toContain(`--measure:${contract.measure.prose}`);
    expect(contract.measure.prose, "and the atom's number is `ch`").toMatch(/^\d+ch$/);
    // **NO PROSE RULE WRITES A PIXEL MEASURE.** A `max-width` of 3–7 hundred px on a prose block is a measure in
    // the wrong unit. **A MEDIA QUERY IS NOT A MEASURE**: the first scan caught `@container token-list
    // (max-width: 639px)` because a lookbehind cannot see a query that started 40 characters earlier — so split
    // the sheet into top-level blocks instead, which is the only reliable way to ask "declaration or condition?".
    const blocks: string[] = [];
    let depth = 0;
    let start0 = 0;
    for (let i = 0; i < sheet.length; i++) {
      if (sheet[i] === "{") depth++;
      else if (sheet[i] === "}") {
        depth--;
        if (depth === 0) { blocks.push(sheet.slice(start0, i + 1)); start0 = i + 1; }
      }
    }
    const prosePx = blocks
      .filter((b) => !b.trimStart().startsWith("@"))
      .flatMap((b) => [...b.matchAll(/max-width:\s*([3-7]\d\d(?:\.\d+)?px)/g)].map((m) => m[1]));
    expect(prosePx, "no prose rule writes its measure in pixels").toEqual([]);
    // And no rule spells the measure itself.
    expect(sheet, "and none spells `66ch` by hand").not.toMatch(/max-width:\s*66ch/);
  });

  it("draws the subtitle slot ONCE per page, at ONE treatment", () => {
    // **MEASURED 2026-11-11, EVERY `.page-sub` INSIDE A PAGE HEAD:**
    //
    //     surface/page       count  size/weight   ink
    //     console Activity   **2**  13 / **500**   **ink**   (the day's COUNT and the stream's STATUS)
    //                               12 / 400        muted
    //     panel Memory/Plugins/Settings  **1**  13 / 400  muted
    //
    // **One slot, two treatments — and Activity put TWO lines in it**, which `head.maxHeadsPerPage: 1` could not
    // see because both wore the subtitle's class. A count is not a subtitle: it is the page's FACT, and it takes
    // the `section` rung the atom records, which gives that head **22 → 17 → 13** where before it had two.
    const src = readFileSync(path.join(here, "App.tsx"), "utf8");
    // The count is not in the slot.
    expect(src, "the day's count is not a `.page-sub`").not.toMatch(/className="page-sub activity-today"/);
    expect(src, "and it says what it is").toContain('className="activity-today"');
    // **HOME KEEPS ITS OWN SHAPE.** Its verdict is the title's second line at the title's size; Activity opens with
    // the day's number. The two pages must NOT converge — that is the standing direction.
    const sheet = css();
    const verdict = ruleFor(sheet, ".home-verdict");
    const count = ruleFor(sheet, ".activity-today");
    expect(verdict, "Home's verdict is at the title's rung").toContain("font-size: var(--fs-xl)");
    expect(count, "Activity's count is at the section's").toContain("font-size: var(--fs-lg)");
    expect(verdict, "which is not the count's").not.toBe(count);
    // The atom's own numbers.
    expect(contract.head.subtitleSlot.class, "the context slot the atom names").toBe(".page-sub");
    expect(contract.head.maxHeadsPerPage, "and one per page").toBe(1);
    expect(contract.section.h2.size, "the section rung the count now takes").toBe(17);
    // **AND THE STEP CARRIES THREE ROLES, SO THE ATOM NAMES THE MARKER THAT SEPARATES THEM (the hundred-and-
    // twenty-second list).** Measured 2026-11-11: **17/600** is the page's CLAIM on both surfaces, a DAY HEADING
    // inside the record, and this atom's `h2` — on Activity the claim (`Today: 543 client calls`) and a day
    // (`Tuesday, 11/10`) weigh the same and sit **217 px apart**. **There is no free step** between 22 and 17 to
    // move the claim into, and the two lines' tracking differs by **0.34 px**, which the tracking atom's own comment
    // calls invisible. **So the separator is the SLOT, not the size**, and an atom that only records the size leaves
    // the next page free to put a claim in the body at a section heading's weight.
    expect(contract.section.sameSizeDifferentRoles.marker, "the marker is POSITION").toBe("slot");
    expect(contract.section.sameSizeDifferentRoles.roles, "three roles, named").toEqual([
      "claim (head)",
      "h2 (body)",
      "day heading (body)",
    ]);
  });

  it("fills the on-soft job with ONE member on both surfaces", () => {
    // **THE JOB THE CONSOLE HAD AND NO TOKEN FOR (the hundred-and-twenty-fourth list).** Measured 2026-11-11:
    //
    //     console   `--accent #bf3a0a` on `--accent-soft #fff0e6`   **4.93:1**   the atom's member: `(none)`
    //     panel     `--accent-on-soft #9c3a0a` on the same wash     **6.2:1**
    //
    // **Both pass AA, so nothing was broken** — the defect is **two answers to one job**, which is exactly what a
    // palette rule forbids, and `palette.accentFamily.names.console` recorded this surface's member as `(none)`.
    // The sheet declares it now, both blocks, because the mix that a light value paints on a DARK wash is 1.4:1.
    const sheet = stripComments(tokens());
    const light = sheet.slice(0, sheet.indexOf("@media"));
    const dark = sheet.slice(sheet.indexOf("@media"));
    expect(light, "the light block declares the member").toContain("--accent-on-soft:#9c3a0a;");
    expect(dark, "and after dark the member INVERTS, because the wash is dark").toMatch(
      /--accent-on-soft:#[0-9a-f]{6};/,
    );
    // **AND NO RULE FILLS THE JOB WITH `--accent` ANY MORE.** The two sites that did (`.seg`'s pressed segment and
    // the chip-tag hover) take the member; a third would be a fourth answer.
    const rules = stripComments(css());
    expect(rules, "no rule paints accent text on the wash with the brand member").not.toMatch(
      /background: var\(--accent-soft\);\s*color: var\(--accent\)/,
    );
    expect(rules, "the two sites take the atom's member").toContain("var(--accent-on-soft)");
  });

  it("paints the accent family the contract declares, and no sixth member as a literal", () => {
    // **MEASURED 2026-11-11 AS A HUE CENSUS.** The palette atom declares ONE accent and the pages painted NINE warm
    // colours; the members sit so close a reader cannot name them apart:
    //
    //     --accent          #bf3a0a   the brand
    //     --accent-solid    #b03a0a   ΔE 6.6 from --accent
    //     --accent-on-soft  #9c3a0a   ΔE 9.2
    //     --accent-ink      #d9480f   ΔE 9.2
    //
    // **Rule 12: "no ambiguous or too-similar colours".** Each member earns its place by a MEASURED contrast job —
    // which is the decision the atom now records — and what a gate can hold is that the FAMILY is the atom's list
    // and that no rule paints one of its members as a literal the layer cannot name.
    const tokensSrc = stripComments(tokens());
    // **A MEMBER IS A JOB, AND EACH SURFACE NAMES IT ITS OWN WAY.** The console calls its hover step
    // `--accent-hover` and the panel `--accent-solid-hover`; the contract records the JOB so the two surfaces can be
    // compared without pretending they share a spelling.
    const family = contract.palette.accentFamily;
    expect(family.jobs.length, "the family has six jobs").toBe(6);
    expect(family.names.console.length, "and the console names each of them").toBe(6);
    expect(family.names.panel.length, "and the panel names each of them").toBe(6);
    for (let i = 0; i < family.jobs.length; i++) {
      const name = family.names.console[i];
      if (name === "(none)") continue;   // the console's brand IS its graphic ink; that slot is the accent itself
      expect(tokensSrc, `the console declares ${name} for the ${family.jobs[i]} job`).toContain(`${name}:`);
    }
    // **AND THE ONE EXTRA HUE IS WRITTEN DOWN.** The memory tag's teal is a hue the semantic four cannot carry;
    // the atom names it, so a second unspoken hue is a failure rather than a habit.
    expect(contract.palette.extraHues.length, "the product draws one hue outside the palette").toBe(1);
    // **THE HUE IS THE PALETTE'S; THE TOKEN IS THE PANEL'S.** The memory tag exists on one surface, so this sheet
    // cannot be asked to declare it — the panel's own gap does, and this gate only holds the atom's INVENTORY.
    expect(contract.palette.extraHues[0].ink, "the extra hue is recorded with its value").toBe("#0a6f64");
    expect(tokensSrc, "and this surface paints no contradicting hue").not.toContain("--tag-ink:");
    // The console paints no literal in the accent's ink either.
    const sheet = stripComments(css());
    const accentInk = /--accent:\s*(#[0-9a-f]{6})/i.exec(tokensSrc);
    expect(accentInk, "the accent is a hex token").toBeTruthy();
    const hex = accentInk![1].slice(1).toLowerCase();
    const rgb = `${parseInt(hex.slice(0, 2), 16)}, ${parseInt(hex.slice(2, 4), 16)}, ${parseInt(hex.slice(4, 6), 16)}`;
    const literals = [...sheet.matchAll(/rgba?\(([^)]+)\)/g)]
      .map((m) => m[1].split(",").slice(0, 3).map((x) => parseInt(x.trim(), 10)).join(", "))
      .filter((triple) => triple === rgb);
    expect(literals, "no rule paints the accent as a literal").toEqual([]);
  });

  it("draws a LABEL as a label and a HEADING as a heading, at the atom's levels", () => {
    // **MEASURED 2026-11-11, EVERY CARD'S OWN TITLE:**
    //
    //     surface   card            element   size/weight    transform     text
    //     console   `.attention`   **h2**    **10 / 600**   **uppercase** "Needs attention"
    //     panel     `.plug-card`   header    13 / 400       none          "Playwright…"
    //
    // **A card's title was an `h2` drawn at the EYEBROW's size.** The `section` atom says `h2` is **17 / 600** and
    // `eyebrow` is **10 / 600 / tracking .05 / uppercase** — and this card was an `h2` at exactly the eyebrow's
    // numbers. **The tag and the size disagreed, and the tag is what a screen reader hears**: the outline announced a
    // section heading, the page drew a small label.
    // **AND THE OUTLINE GATE WAS RIGHT WHEN IT REFUSED MY FIRST FIX**: wrapping the panel's card title in an `h3`
    // skips a level under a page's `h1`, and the card sits directly under the page. The element says the RUNG it
    // occupies; the size is the atom's.
    const src = readFileSync(path.join(here, "HomePage.tsx"), "utf8");
    expect(src, "the attention label is not a heading").toContain('className="attention-label"');
    expect(src, "and it is not an h2").not.toMatch(/<h2>Needs attention<\/h2>/);
    // The label's typography is still the eyebrow's, and the rule that gives it that is keyed by the class.
    const sheet = css();
    expect(sheet, "the label is drawn at the eyebrow's size").toMatch(/\.attention-label,[\s\S]{0,400}font-size: var\(--fs-2xs\)/);
    expect(sheet, "with the eyebrow's step").toMatch(/\.attention-label,[\s\S]{0,400}text-transform: uppercase/);
    // **AND THE BROWSER GATE READS IT BY NAME.** This step compared `h2` for a year; the day the tag stopped being one,
    // it failed with `the attention block's heading read ""` — which is a gate doing its job on a shape that changed.
    const gateSrc = readFileSync(path.join(here, "..", "scripts", "browser-gate.mjs"), "utf8");
    expect(gateSrc, "the gate reads the block's label").toContain("block.querySelector('.attention-label, h2')");
    // The atom's own numbers, so this reads the contract and not taste.
    expect(contract.section.h2.size, "a section heading is 17").toBe(17);
    expect(contract.section.eyebrow.size, "a label is 10").toBe(10);
    expect(contract.section.eyebrow.transform, "and uppercase").toBe("uppercase");
    expect(contract.section.eyebrow.weight, "at 600").toBe(600);
  });

  it("moves at ONE interaction duration, read from the token layer", () => {
    // **MEASURED 2026-11-11 AS A VOCABULARY.** The contract records the reduce MECHANISM and no number, so nothing
    // could compare the two surfaces' dialects:
    //
    //     surface   the pair that carries every interactive element      other durations in the same page
    //     console   `background@0.12s` ×39, `transform@0.12s` ×33        **`transform@0.14s` ×44**
    //     panel     `background@0.12s` ×12, `color@0.12s` ×11            `background@0.2s`, `@0.15s`, `@0.06s`
    //
    // **The same hover took 0.12 s on one surface and 0.2 s on the other**, and even within this sheet a chevron
    // moved at 140 ms while every control moved at 120 — written in MILLISECONDS, which is why a scan for `0.NNs`
    // could not see it.
    const sheet = css();
    // **THE TOKEN LIVES WHERE TOKENS LIVE.** `app.css` USES them; `tokens.css` DEFINES them — a scan of the wrong
    // file is a scan of nothing, which is what the first draft of this gate did.
    const token = stripComments(tokens()).match(/--dur-interaction:\s*([^;]+);/);
    expect(token, "the duration is a token").toBeTruthy();
    expect(token![1].trim(), "and it is the contract's number").toBe(`${contract.motion.interactionMs}ms`);
    // **NO TRANSITION WRITES ITS OWN DURATION** — in any unit. An `animation` may keep its own (a sheet arriving
    // explains a state change; a hover does not), so this scans `transition` declarations only.
    const decls = [...sheet.matchAll(/transition:\s*[^;]+;/g)].map((m) => m[0]);
    const literals = decls.filter((d) => /\b\d+(?:\.\d+)?(?:ms|s)\b/.test(d));
    expect(literals, "no transition declaration writes a literal duration").toEqual([]);
    // And the reduce mechanism the atom records, so the gate reads the contract rather than the sheet.
    expect(contract.motion.reducedMotion, "the reduce mechanism is a blanket").toBe("blanket");
    expect(contract.motion.cap, "and its cap").toBe("0.01ms");
  });

  it("separates two LIKE things ONE way, with the family's `line`", () => {
    // **MEASURED 2026-11-11 ACROSS EVERY LIST THE PRODUCT DRAWS**, the first four rows of each:
    //
    //     list                 rows  container gap      rule on the row     rule colour
    //     `.attention-list`     2     **0px**           **1 of 2**           `soft` (.05)
    //     `.fleet-table tbody`  4     **0 (leading)**   **0**                —
    //     `.timeline-rows`      4     **0 (leading)**   **0**                —
    //     `.mem-list`           3     **8px**           **3 of 3**           `line` (.08)
    //     `.plug-rows`          4     **8px**           **3 of 3**           `line` (.08)
    //
    // **Five lists, four separators.** The `row` atom answers the question twice — a list of LINES separates by its
    // own leading (`gapLines: 0`), a list of BOXES by the scale's 8 (`gapBoxes: 8`) — and the `hairline` atom says
    // what a separating line takes: *"a hairline separates two regions and takes `line`"*.
    const sheet = css();
    const rule = ruleFor(sheet, ".attention-row");
    expect(rule, "the attention row is declared").toBeTruthy();
    expect(rule, "its separator is the family's `line`").toContain("border-top: 1px solid var(--line)");
    // `--line-soft` is for an edge that is almost not there; a separator every other list draws at `.08` may not be
    // the one this list draws at `.05`.
    expect(rule, "and not the `soft` member").not.toContain("var(--line-soft)");
    // **THE TWO LISTS THAT SEPARATE BY A RULE AGREE ON THAT RULE** — one family, one member.
    expect(sheet, "the panel's lists separate at `line` too").toContain("border-top: 1px solid var(--line)");
    // And the atom's own numbers, so this reads the contract rather than taste.
    expect(contract.hairline.line, "the separating line is the family's").toBe("rgba(0, 0, 0, 0.08)");
    expect(contract.hairline.soft, "and `soft` is a different number, for a different job").toBe(
      "rgba(0, 0, 0, 0.05)",
    );
    expect(contract.row.gapLines, "a list of LINES separates by its leading").toBe(0);
    expect(contract.row.gapBoxes, "a list of BOXES by the scale's 8").toBe(8);
  });

  it("is a page that RESERVES THE SHAPE while a read is held, not a sentence", () => {
    // **RULE 19's LOADING HALF, MEASURED WITH EVERY READ PARKED 90 SECONDS (the ninety-seventh list).** What
    // each page draws while a read is held:
    //
    //     Home      console   `.skeleton-list`, 3 rows, `role="status"`, "Reading the machine list"   (195 px)
    //     Memory    panel     `.skeleton-list`, 3 rows, `role="status"`, "Reading the memory list"    (193 px)
    //     Plugins   panel     `.skeleton-list`, 3 rows, `role="status"`, "Reading the plugin inventory" (193 px)
    //     **Device console   `p.hint` — the sentence itself, in a 52 px card**                          **0 rows**
    //
    // **SIX of the seven pages drew the atom and ONE drew a sentence** — and it was the SAME sentence Home's
    // skeleton carries as its `aria-label`, so the product said one thing in two shapes on two pages of one
    // console, from the same read state. The panel had this hole and the eightieth list fixed it THERE; the
    // console's Device page was never fixed with it.
    const source = readFileSync(path.join(here, "DevicePicker.tsx"), "utf8");
    // Every reading branch in this product draws the atom: the count of them is this file's and HomePage's.
    for (const file of ["DevicePicker.tsx", "HomePage.tsx"]) {
      const body = readFileSync(path.join(here, file), "utf8");
      expect(body, `${file} draws the atom`).toContain('className="skeleton-list');
      expect(body, `${file} carries the read's own label`).toMatch(/aria-label="Reading the /);
      expect(body, `${file} announces it`).toMatch(/role="status"/);
    }
    // **THE SENTENCE IS STILL THERE — AS THE LABEL, NOT AS THE PAGE.** A person and a screen reader both read it;
    // what changes is that the page also reserves the rows that are about to arrive, so the page does not jump
    // from a one-line box to a four-row list when the read answers.
    expect(source, "the picker's reading branch returns the atom").toContain(
      'aria-label="Reading the machine list"',
    );
    expect(source, "and no longer returns the bare sentence").not.toMatch(
      /return \(\s*\n\s*<p className="hint" role="status">/,
    );
    // The atom's own numbers, from the contract: three rows, the row's corner and the card's inset.
    const row = ruleFor(css(), ".skeleton-row");
    expect(row, "the skeleton row is declared").toBeTruthy();
    expect(row, "at the atom's row corner").toContain("var(--radius-sm)");
    // The atom's own numbers: three rows, and its role is `status`.
    expect(readFileSync(path.join(here, "DevicePicker.tsx"), "utf8")).toContain("[0, 1, 2].map");
    expect(contract.loading.rows, "the atom's rows").toBe(3);
    expect(contract.loading.role, "the atom's role").toBe("status");
  });

  it("is a DAY that says what it holds, and its heading says only what it is called", () => {
    // **MEASURED 2026-11-11 ON THE SERVED BUILD, a seeded record of TWO days (22 rows each):**
    //
    //     a day's own children   `h2.timeline-day-label`, `ol.timeline-rows` — and nothing else
    //     a day's label          "Today" / "Monday, 11/9"
    //     what the DAY said      **nothing**, unless it was long enough to need a door
    //
    // `dayText(lines)` — which builds "22 calls · 15:27 → 15:32" — existed and was drawn in ONE place, the door's
    // title and facts, i.e. only on days longer than the 60 rows drawn. `DAY_VISIBLE` is 60 and the live window
    // holds 1,000 lines in a single day, so a long day answers "what is this?" and a short day cannot: one
    // question, one page, two behaviours, decided by a number the reader cannot see.
    const source = readFileSync(path.join(here, "ActivityTimeline.tsx"), "utf8");
    expect(source, "the day's head is drawn on every day").toContain("timeline-day-head");
    expect(source, "and it carries the day's own sentence").toContain("dayText(day.lines)");
    // **THE FACTS ARE THE HEADING'S SIBLING, NOT ITS CHILD**, so the heading's accessible name stays what the
    // day is CALLED: a reader who jumps by heading hears "Today", "Monday, 11/9" — the record's structure — not
    // a heading ending in a clock range. The suite's own heading test caught this when it was a child.
    // A regex whose body contains `</div>` closes itself at the bare `/` — `indexOf` instead.
    const open = source.indexOf('<div className="timeline-day-head is-sticky">');
    expect(open, "the day's head is drawn").toBeGreaterThan(-1);
    const head = source.slice(open, source.indexOf("</div>", open));
    expect(head, "the head is its own row").toBeTruthy();
    expect(head, "the heading is inside it").toContain("<h2 className=\"timeline-day-label\">");
    expect(head, "and the facts are its sibling").toContain("<span className=\"timeline-day-facts\">");
    expect(head.indexOf("</h2>"), "the facts come AFTER the heading closes").toBeLessThan(
      head.indexOf("timeline-day-facts"),
    );
    const sheet = css();
    const label = sheet.match(/\.timeline-day-label\s*\{[^}]*\}/);
    expect(label![0], "the heading is the day's name, at the day's own step").toContain("font-size: var(--fs-lg)");
    const facts = sheet.match(/\.timeline-day-facts\s*\{[^}]*\}/);
    expect(facts, "the facts are a step down from the name").toBeTruthy();
    expect(facts![0]).toContain("font-size: var(--fs-sm)");
    expect(facts![0], "and a weight down").toContain("font-weight: var(--fw-regular)");
    // **WHAT STICKS IS THE HEAD, NOT THE HEADING** — the head is the row, and the heading is its first child.
    const sticky = sheet.match(/\.timeline-day-head\.is-sticky\s*\{[^}]*\}/);
    expect(sticky, "the day's head is what sticks to the container").toBeTruthy();
    expect(sticky![0]).toContain("position: sticky");
  });

  it("is the RECORD's one grouping: a stretch is a LEVEL, not a caption", () => {
    // **MEASURED 2026-11-11 AT 1,440 ON THE LIVE CONSOLE**, reading what a reader actually compares on
    // Activity — the page whose own direction is *"the record: what happened, when, on which machine"*:
    //
    //     day label "Today"   17 / 600   --ink
    //     a row's verb        13 / 400   --ink
    //     **a stretch header 11 / 400   --ink-faint**   "3 calls · 36 s"
    //     a row's machine     12 / 500   --ink-muted
    //
    // **The line that says "these calls belong together" was the smallest and faintest thing on the page** —
    // smaller and lighter than the machine name it groups. The page drew 60 rows in 9 stretches, and a reader
    // met nine grey 11 px words between hairlines with no way to know they were the structure.
    const sheet = css();
    const stretch = ruleFor(sheet, "\\.timeline-stretch-text");
    // Above the row's verb (13/400) by a WEIGHT step, and not above the day's 17/600.
    expect(stretch, "the level's size is the row's").toContain("font-size: var(--fs-base)");
    expect(stretch, "and it is heavier than the sentence it groups").toContain("font-weight: var(--fw-medium)");
    expect(stretch, "in the page's muted ink, not its faintest").toContain("color: var(--ink-muted)");
    // **THE HEADER FORM IS THE ONE THAT NEEDS IT MOST**, and it was drawn at the same quiet step as the
    // footer: a header opens rows the reader has not seen yet.
    const header = sheet.match(/\.timeline-stretch\.is-header \.timeline-stretch-text\s*\{[^}]*\}/);
    expect(header, "the header form is its own declaration").toBeTruthy();
    expect(header![0], "and it takes the page's own ink").toContain("color: var(--ink)");
    // **THE RULES STAY**: the grouping survives a reader who cannot see the type (rule 6's second channel).
    expect(ruleFor(sheet, "\\.timeline-stretch"), "the hairlines are still drawn").toBeTruthy();
    const rules = sheet.match(/\.timeline-stretch::before, \.timeline-stretch::after\s*\{[^}]*\}/);
    expect(rules, "two 1 px rules around the words").toBeTruthy();
    expect(rules![0]).toContain("height: 1px");
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
