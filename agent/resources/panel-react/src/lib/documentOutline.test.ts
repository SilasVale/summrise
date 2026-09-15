// THE DOCUMENT OUTLINE, AS A CONTRACT.
//
// WHY. Measured on the real pages (device Chrome, 6 pages): the ONLY `<h1>`s in the whole product
// were the side rail's labels — "Sessions" and "Plugins", at 13px — sitting ABOVE page titles that
// were `<h2>` at 17px. So the outline was inverted, and three pages (Terminal, Browser, Memory) had
// no heading at all: a screen-reader user could not discover where they were or jump to the page.
//
// WHAT IS PINNED, and why each half matters:
//   * exactly ONE `<h1>` per page, and it NAMES the page — the thing a screen reader announces;
//   * no level is skipped on the way down (h1 -> h3), which is what makes an outline navigable;
//   * the rail's labels are NOT headings (they answer "what is this list", not "what page is this");
//   * the panel's three regions are LANDMARKS (nav / aside / main) — the desktop density had them
//     and the panel density was four plain divs, so there was nothing to jump between.
//
// The page's name may be visually hidden (`.sr-only`) where the design carries it another way — a
// terminal page cannot spend a row on a title — which is exactly why this is a test rather than a
// screenshot check.
import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

// This file lives in src/lib, so ONE level up is src/ — two would land on the package root,
// where neither components/ nor styles/ exists (the first version of this test failed that way).
const SRC = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function read(rel: string): string {
  return readFileSync(path.join(SRC, rel), "utf8");
}

/** Every PAGE component and the name its h1 must carry. */
const PAGES: Array<[string, string]> = [
  ["components/HistoryPage.tsx", "History"],
  ["components/BrowserPage.tsx", "Browser"],
  ["components/MemoryPage.tsx", "Memory"],
  ["components/PluginsPage.tsx", "Plugins"],
  ["components/SettingsPage.tsx", "Settings"],
];

/** The per-session VIEWS. They are not pages and own no h1, but they render headings, and a level
 *  skipped inside one is the same defect (measured: the Path view rendered h2 … h4 until round 39). */
const VIEWS = ["components/TrajectoryView.tsx", "components/PathView.tsx", "components/RunStrip.tsx"];

/** Components that are SECTIONS of a page: they carry a heading and must not own an h1. */
const SECTIONS: Array<[string, string]> = [
  ["components/ArchivePage.tsx", "Sessions"],
  ["components/ActivityPage.tsx", "Runs"],
];

describe("the document outline", () => {
  it("every page states its own name as its only h1", () => {
    for (const [file, name] of PAGES) {
      const src = read(file);
      const h1s = [...src.matchAll(/<h1[^>]*>([\s\S]*?)<\/h1>/g)].map((m) => m[1].trim());
      // "At least one" in the SOURCE, because a component may have several render branches (the
      // Browser page has two, one of which renders). "EXACTLY one" is a fact about the RENDERED
      // document, and each page's own test asserts it through src/test-utils/outline.ts.
      expect(h1s.length, `${file} must name the page with an <h1>`).toBeGreaterThanOrEqual(1);
      for (const h1 of h1s) {
        expect(
          h1.replace(/<[^>]+>/g, "").trim(),
          `${file}'s h1 must name the page`,
        ).toBe(name);
      }
    }
  });

  it("the terminal page is named where it mounts, in BOTH densities", () => {
    // The panel and the desktop mount the terminal differently, and only the panel got the hidden
    // h1 in round 34 — measured live, the desktop's landing page had NO headings at all while every
    // other page had one.
    for (const file of ["components/PanelApp.tsx", "components/DesktopShell.tsx"]) {
      expect(read(file), `${file} must name the terminal page`).toMatch(
        /<h1 className="sr-only">Terminal<\/h1>/,
      );
    }
  });

  it("no page is nested inside another main landmark", () => {
    // MEASURED live on /desktop/: `main.desktop-main` contained `main.desktop-content` — invalid
    // HTML, and one page offering a screen reader two main landmarks to choose between.
    const desktop = read("components/DesktopShell.tsx");
    expect(desktop, "the content card must not be a second <main>").not.toMatch(
      /<main className="desktop-content"/,
    );
    expect(desktop).toMatch(/<div className="desktop-content">/);
  });

  it("a section carries a heading and never an h1", () => {
    // ArchivePage and ActivityPage are the two halves of History. They were pages until round 31's
    // merge, and promoting their titles to h1 during round 34's outline pass made History render
    // TWO h1s — caught by the live measurement, not by their own tests, each of which mounts only
    // its own component.
    for (const [file, name] of SECTIONS) {
      const src = read(file);
      expect(src, `${file} must not own an <h1>`).not.toMatch(/<h1[^>]*>/);
      expect(src, `${file} must name itself with an <h2>`).toMatch(
        new RegExp(`<h2[^>]*>\\s*${name}\\s*</h2>`),
      );
    }
  });

  it("a view's top-level heading is an h2 — the page owns the h1", () => {
    // The relative-skip check below cannot see this: PathView.tsx contains ONE heading, so there is
    // no pair to compare, and only the RENDERED page showed `h1 Terminal` above `h3 Worth a look`
    // (and, at the first attempt, `h4`). A view's first heading is a section of the page.
    for (const file of VIEWS) {
      const first = /<h([1-6])[ >]/.exec(read(file));
      if (!first) continue; // a view with no headings has nothing to place
      expect(Number(first[1]), `${file}: a view's first heading must be h2 (the page is h1)`).toBe(2);
    }
  });

  it("no page skips a level on the way down", () => {
    // A heading two levels below its parent is the smell that makes an outline unusable: h1 -> h3
    // tells a screen reader a section is missing. Every page component is scanned for the sequence.
    for (const file of [...PAGES.map((p) => p[0]), ...SECTIONS.map((s) => s[0]), ...VIEWS]) {
      const src = read(file);
      const levels = [...src.matchAll(/<h([1-6])[ >]/g)].map((m) => Number(m[1]));
      let previous = 0;
      for (const level of levels) {
        expect(
          previous === 0 ? 1 : level - previous,
          `${file}: h${previous} -> h${level} skips a level`,
        ).toBeLessThanOrEqual(1);
        previous = level;
      }
    }
  });

  it("the rail's labels are NOT headings", () => {
    // They were the product's only h1s, at 13px, above every page title.
    const rail = read("components/ContextRail.tsx");
    expect(rail, "the rail labels the list; it does not own the page's heading").not.toMatch(
      /<h[12][^>]*className="side-title"/,
    );
    expect(rail).toMatch(/className="side-title"/);
  });

  it("the panel's regions are landmarks, like the desktop density's", () => {
    const shell = read("components/Shell.tsx");
    expect(shell, "the panel's rail is navigation").toMatch(/<nav id="icon-rail"/);
    expect(shell, "the side list is complementary").toMatch(/<aside id="context-rail"/);
    expect(shell, "the page is main").toMatch(/<main id="canvas-host"/);
    // …and the desktop density uses the SAME vocabulary: a nav rail and ONE main. It had an
    // <aside> rail and, further in, a second <main>.
    expect(shell, "the desktop's rail is navigation too").toMatch(/<nav className="desktop-rail"/);
    expect(shell).toMatch(/<main className="desktop-main"/);
  });

  it("the hidden-text utility is one shared rule, used by the pages that need it", () => {
    const base = read("styles/base.css");
    expect(base, "`.sr-only` must be the general utility").toMatch(/\.sr-only\s*\{/);
    // Every sr-only page name must be inside a page component that also renders something visible —
    // checked by the PAGES list above; here we assert the class is not duplicated per-feature again.
    const css = readdirSync(path.join(SRC, "styles"))
      .filter((f) => f.endsWith(".css"))
      .map((f) => read(`styles/${f}`))
      .join("\n");
    expect(css, "one hidden-text utility, not one per feature").not.toMatch(/\.approval-sr\s*\{/);
  });
});
