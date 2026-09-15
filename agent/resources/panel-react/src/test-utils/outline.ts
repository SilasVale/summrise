// The outline assertions, shared by the page tests.
//
// WHY A HELPER AND NOT JUST A SOURCE SCAN. A component may have several render branches (the
// Browser page has a desktop-mode placeholder and a pane), so counting `<h1>`s in the SOURCE says
// nothing about what a person actually gets: the first version of this check failed on
// BrowserPage for having two — one per branch, of which exactly one renders. What matters is the
// RENDERED document, so the rule lives here and each page's own test (which already knows how to
// mount that page) calls it.
import { expect } from "vitest";

/** Exactly one `<h1>`, it names the page, and it comes FIRST in the document. */
export function expectOneH1(container: HTMLElement, name: string): void {
  const h1s = [...container.querySelectorAll("h1")];
  expect(h1s.length, `the page must have exactly one <h1> (found ${h1s.length})`).toBe(1);
  expect(h1s[0].textContent?.trim()).toBe(name);
  // …and nothing outranks it: a heading BEFORE the page's own name is the inverted outline the
  // rail's labels used to create.
  const first = container.querySelector("h1,h2,h3,h4,h5,h6");
  expect(first?.tagName.toLowerCase(), "the page's name must be the first heading").toBe("h1");
}

/** No heading level deeper than one step below its predecessor — h1 -> h3 breaks an outline.
 *
 *  The FIRST heading is not judged: a fragment mounted on its own legitimately starts below h1
 *  (ArchivePage is a section of History and starts at h2), and only the caller knows what encloses
 *  it. Judging it here is how the first version of this helper failed two section tests for being
 *  what they are. */
export function expectNoSkippedLevel(container: HTMLElement): void {
  const levels = [...container.querySelectorAll("h1,h2,h3,h4,h5,h6")].map((h) =>
    Number(h.tagName[1]),
  );
  for (let i = 1; i < levels.length; i++) {
    expect(
      levels[i] - levels[i - 1],
      `h${levels[i - 1]} -> h${levels[i]} skips a level`,
    ).toBeLessThanOrEqual(1);
  }
}

/** A SECTION of a page: it carries a heading, and it must NOT own one (the page does). */
export function expectSectionHeading(container: HTMLElement, name: string): void {
  expect(
    container.querySelectorAll("h1").length,
    "a section must not carry an <h1> — the page owns that (History renders both halves)",
  ).toBe(0);
  const h2s = [...container.querySelectorAll("h2")];
  expect(h2s.map((h) => h.textContent?.trim())).toContain(name);
}
