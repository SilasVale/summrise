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
  page: { max: number; pad: number; gap: number };
  card: { radius: number; pad: number; gap: number; headerGap: number };
  rail: {
    console: number;
    panel: number;
    consoleCollapsesAt: number | null;
    panelCollapsesAt: number | null;
  };
  title: { size: number; weight: number };
  breakpoints: Record<string, number>;
  empty: { radius: number; pad: number; gap: number; textAlign: string; shadow: string };
  head: { gap: number; align: string };
  motion: { reducedMotion: string; cap: string; namedAnimationsStop: boolean };
  cardTitle: { size: number; weight: number };
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
      "23e0de0ccefe0bb21fdc3a662660db40e2ca5890ff9bcdf41e114d7ae495eab9"
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
      allowed.add(value);
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
    // **AND IT COLLAPSES WHERE THE CONTRACT SAYS (the fifty-first list).** The rail is 232 px from
    // `tablet` (767) up and a TOP BAR below it, so the page's left edge is 0 at 380/480/639 — measured.
    // The panel never collapses (a device app in a window), and the contract records that difference as
    // a RULE rather than leaving the next reader to measure both surfaces.
    expect(contract.rail.consoleCollapsesAt, "the console collapses").toBe(contract.breakpoints.tablet);
    expect(contract.rail.panelCollapsesAt, "the panel never collapses").toBeNull();
    expect(source, "the rail's width is the contract's").toContain(
      `grid-template-columns: ${contract.rail.console}px minmax(0, 1fr)`
    );
    expect(source, "the rail itself is the contract's width").toContain(
      `width: ${contract.rail.console}px`
    );
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
