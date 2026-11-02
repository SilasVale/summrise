import type { Page } from "./Rail";

/** The rail's destinations, in the order the page type declares them. Kept here so a link parser
 * does not import the React component just to read a list. */
const PAGES: readonly Page[] = ["devices", "terminal", "screen", "files", "runs", "system", "audit"];

/** The fragment for a surface: `#files`, `#runs`, and so on. */
export function surfaceHash(page: Page): string {
  return "#" + page;
}

/** Read a surface fragment. A run link (`#run/...`) is not a surface and is read by run-link.ts. */
export function parseSurfaceHash(hash: string): Page | null {
  const match = /^#([a-z]+)$/.exec(hash);
  if (!match) return null;
  return (PAGES as readonly string[]).includes(match[1]) ? (match[1] as Page) : null;
}
