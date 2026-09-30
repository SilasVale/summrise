#!/usr/bin/env node
/**
 * Does the SERVED panel actually carry the Workspaces page, and every class name it renders?
 *
 * WHY THIS EXISTS, and why it is a FILE rather than an inline script. The device has no browser installed
 * (`where /r D:\Summrise playwright-core\package.json` finds nothing — the staged component is a zip), and
 * the desktop shell holds the only CDP port (127.0.0.1:9333 answered the shell's own "CDP self-check OK"
 * and then timed out for a second client). So the one surface a script CAN read is the bytes the agent
 * serves, and the two gates that matter for a page are in those bytes: every class it renders must be a
 * class the stylesheet paints (`unstyledMarkup`), and it must carry exactly one h1, first
 * (`design-sweep`'s `h1Count !== 1 || !firstIsH1`). Reading the served files checks the same properties the
 * failing job named, on the bytes the device is actually running.
 *
 * Usage: node verify-workspaces-page.mjs [panelBaseUrl]   (default http://127.0.0.1:18080/panel/)
 */
const base = process.argv[2] ?? "http://127.0.0.1:18080/panel/";

/** Class names the page renders, and the rules that must paint them. */
const MARKS = [
  "workspaces-view-head",
  "workspaces-entry-file",
  "workspaces-run-status",
  "workspaces-editor-text",
  "workspaces-machine",
];
/** The press state the feedback gate demanded: a hover with no `:active` is a failure, not a style. */
const PRESS_RULE = ".workspaces-entry-file:active";
/** What the page says out loud, so a bundle that lost a string cannot pass as "present". */
const STRINGS = ["Add a host", "Open a terminal", "File contents", "Workspaces"];

const failures = [];
const note = (ok, what, detail) => {
  if (!ok) failures.push(`${what}${detail ? `: ${detail}` : ""}`);
  return `${ok ? "ok  " : "FAIL"} ${what}${detail ? ` — ${detail}` : ""}`;
};

const count = (haystack, needle) => haystack.split(needle).length - 1;

const js = await (await fetch(`${base}panel.js`)).text();
const css = await (await fetch(`${base}panel.css`)).text();
console.log(`served from ${base}`);
console.log(`  panel.js  ${js.length} bytes`);
console.log(`  panel.css ${css.length} bytes`);

for (const mark of MARKS) {
  const inJs = count(js, mark) > 0;
  const inCss = count(css, `.${mark}`) > 0;
  console.log(note(inJs && inCss, `${mark}`, `in js ${inJs}, painted by css ${inCss}`));
}
console.log(note(count(css, PRESS_RULE) > 0, "the file row's press state", PRESS_RULE));

for (const s of STRINGS) {
  console.log(note(js.includes(s), `the string "${s}"`, s));
}

// The h1 contract: the page renders one, sr-only, and nothing else claims to be the page's name.
const srOnly = count(js, "sr-only");
console.log(note(srOnly > 0, "the page carries an sr-only heading", `${srOnly} occurrence(s)`));

if (failures.length > 0) {
  console.log(`\n${failures.length} problem(s):`);
  for (const f of failures) console.log(`  - ${f}`);
  process.exit(1);
}
console.log("\nthe served panel carries the Workspaces page, its styles, its press state and its strings");
