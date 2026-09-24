#!/usr/bin/env node
// exports-check.mjs — A MODULE'S PUBLIC SURFACE SHOULD BE WHAT SOMEBODY USES.
//
// WHY THIS EXISTS (round 21 of the standing goal). The objective says prune whatever stops earning its place, and
// the previous round pruned eighteen tokens nothing read. The same question was never asked of the CODE. Measured:
// 305 exports in the panel and 43 in the console, of which
//
//     67 + 9  were referenced NOWHERE in the other files
//    34 + 0  were referenced only by tests
//
// and triaging the 67 found the distinction that makes this check worth having:
//
//   * 24 of them were used INSIDE their own file — the `export` keyword was the only thing that made them public.
//     Nothing breaks when it goes, and the module stops promising something it never offered. (A constant like
//     `URGENT_MS` is the file's business; exporting it invites a second copy elsewhere.)
//   * 3 were used nowhere at all: the panel's `hasTransport()` (nothing called it), the console's `<StatusChip>`
//     (nothing rendered it) and its `getLang()` (nothing called it — the dictionary reads `lang` directly).
//
// `<StatusChip>` is the reason this file exists rather than the deletion alone: its CSS family stayed behind, 35
// lines of `.chip` / `.chip .dot` / four tone rules that nothing could render — INCLUDING a `.chip.off .dot` using
// the very `--text-faint` ink the console's marks gate had already caught at 2.46:1 on the live `.sig-dot.off`. The
// defect existed twice and only the live copy was fixed, because nothing connected "this component is gone" to
// "these rules can never match".
//
// TESTS COUNT AS USE. An export only a test reads is a deliberate testing seam on this codebase (34 of them in the
// panel), not dead weight, and a check that called them dead would be turned off within a round.
//
// WHAT IT CANNOT SEE: a member reached dynamically (`mod[name]`, a string-built import). Nothing in either UI does
// that today; if something starts, name it in ALLOWED with the reason.
import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

const UIS = [
  ["panel", "agent/resources/panel-react/src"],
  ["console", "gateway/ui/src"],
];

/** Deliberately public despite no importer. Empty, and each entry would need its reason. */
const ALLOWED = new Set([]);

const sourceFiles = (dir) => {
  const out = [];
  const walk = (d) => {
    const full = path.join(ROOT, d);
    if (!existsSync(full) || !statSync(full).isDirectory()) return;
    for (const n of readdirSync(full)) {
      if (n === "node_modules" || n === "dist" || n === "build") continue;
      const f = path.join(full, n);
      if (statSync(f).isDirectory()) walk(path.join(d, n));
      else if (/\.(ts|tsx)$/.test(n) && !/\.d\.ts$/.test(n)) out.push(f);
    }
  };
  walk(dir);
  return out;
};

const failures = [];
let exportsSeen = 0;

for (const [ui, dir] of UIS) {
  const files = sourceFiles(dir);
  // COMMENTS ARE STRIPPED FIRST, the lesson `stub-surface-check` and `retired-colours-check` already
  // record, and this gate needed it for a reason of its own: it counts a bare `\bname\b` ANYWHERE as a
  // use, so PROSE kept dead exports alive. `useSessionEvents` survived six audits on two comments that
  // merely mentioned it, and `evicted.ts`'s `humanIdle` was matched by an unrelated same-named local in
  // another file. Stripping also stops a commented-out `export` being counted as a declaration, which
  // the scan below would otherwise do. Found 2026-09-24 by the panel exploration.
  const stripComments = (s) => s.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");
  const texts = new Map(files.map((f) => [f, stripComments(readFileSync(f, "utf8"))]));
  let seen = 0;

  for (const [file, text] of texts) {
    if (/\.test\./.test(file)) continue;
    for (const m of text.matchAll(/^export\s+(?:async\s+)?(?:function|const|class|enum|type|interface)\s+([A-Za-z_][\w]*)/gm)) {
      const name = m[1];
      if (ALLOWED.has(name)) continue;
      seen++;
      let elsewhere = 0;
      let own = 0;
      for (const [other, body] of texts) {
        const found = [...body.matchAll(new RegExp("\\b" + name + "\\b", "g"))].length;
        if (other === file) own += Math.max(0, found - 1); // minus the declaration itself
        else elsewhere += found;
      }
      const where = path.relative(ROOT, file);
      if (elsewhere === 0 && own === 0) failures.push(`${ui}: ${name} (${where}) is exported and used NOWHERE — delete it`);
      else if (elsewhere === 0) failures.push(`${ui}: ${name} (${where}) is exported but used only inside its own file — drop the \`export\``);
    }
  }

  // A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN: the two UIs had 305 and 43 exports when this was written.
  if (seen < 20) {
    console.error(`exports-check: FAILED — the ${ui} scan found ${seen} exports, which is too few to be reading it`);
    process.exit(1);
  }
  exportsSeen += seen;
}

if (failures.length) {
  console.error("exports-check: FAILED — public surface nothing uses:");
  for (const f of failures) console.error("  " + f);
  console.error("\n  An export is a PROMISE that somebody outside needs this. If nobody does, it is either the file's");
  console.error("  own business (drop the keyword) or nobody's (delete it). A test-only use is a seam and counts as use.");
  process.exit(1);
}
console.log(`exports-check: ok — ${exportsSeen} exports across both UIs, every one used outside the file that declares it`);
