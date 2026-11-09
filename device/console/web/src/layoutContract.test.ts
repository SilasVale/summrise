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
  rail: { console: number; panel: number };
  title: { size: number; weight: number };
  breakpoints: Record<string, number>;
  empty: { radius: number; pad: number; gap: number; textAlign: string; shadow: string };
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
      "2765217e89fa949818bad779a06c33a8fbcb256c95bf1cf333edc0830418e7a5"
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
