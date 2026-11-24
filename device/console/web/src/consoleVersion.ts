/**
 * **WHICH BUILD IS THIS PAGE? (the hundred-and-ninety-ninth list.)** A tab left open across a deploy kept calling the routes of an older
 * page, and the audit recorded the consequences (`monitor.add`, `memory.list`, `monitor.status` refusals, all `caller: console`, all
 * from a bundle that had already been fixed). The console server stamps the page at start and `/health` reports the same version, so the
 * page can compare itself with the running build — the comparison lives in `App`, which already reads `/health`.
 */

/** The version stamped into this page's `<meta>`, or `null` on a page served by an older console (which has no such meta). */
export function pageVersion(): string | null {
  const meta = document.querySelector('meta[name="console-version"]');
  const value = meta?.getAttribute("content") ?? "";
  // The placeholder survives only when a page is opened straight from `dist/` without the server's stamp.
  if (value === "" || value === "__CONSOLE_VERSION__") return null;
  return value;
}
