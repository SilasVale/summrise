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
  section: { gap: number };
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
      "2d3ee035994875a5a7741d2f6431127268630a40e68160fd6c187118739502d5"
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
