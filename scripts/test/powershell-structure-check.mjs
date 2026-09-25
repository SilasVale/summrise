// powershell-structure-check.mjs — THE POWER SHELL THAT RUNS AS ADMINISTRATOR ON A CUSTOMER'S MACHINE IS NEVER PARSED HERE.
//
// WHY THIS EXISTS (rounds 28, 31, 32). `agent/deploy/*.ps1` is the installer, the tunnel repair and the integrity
// library — the code that runs AS ADMINISTRATOR on a customer's box. No `pwsh` exists on this machine, so not one
// line of it is ever EXECUTED here: all three of those rounds said "unexecuted here" in their own reports. And
// `scripts/test/script-syntax.bash` walks `git ls-files '*.sh' '*.bash'` — 27 files, with `.ps1` NOT among them.
// What those rounds did instead was a brace/paren census BY HAND against the previous version, which is exactly the
// manual check that stops happening. So the defect that can reach a customer is the one nothing here can see: an
// edit that leaves a brace, a paren or a bracket unbalanced, or a string unterminated — which `pwsh` rejects in a
// second.
//
// WHAT IT IS: a structural scan, NOT a parser. It answers ONE question — do `{}`, `()` and `[]` balance OUTSIDE
// single-quoted strings, here-strings and comments, and INSIDE every `$( … )` subexpression a double-quoted string
// carries. That last region is the one this check used to document as a hole about itself, and it is the region
// that matters most here: the launcher lines this installer WRITES are built out of `"… $(Get-Thing) …"`, and a
// `$( … )` is CODE with its own braces and parens, not text. The stripping is what makes it correct rather than a
// false alarm: the installer writes launcher scripts whose bodies contain braces (`'… { exit }; …'`,
// summrise-online-setup.ps1:443), and the integrity tests carry JSON in single-quoted strings
// (`'{"version":"1.2.364"}'`), so a naive count fails on correct code. What it therefore cannot see is listed at
// the bottom of this file, and the snippets it is judged by are the `SELF_TEST` table below — a scanner whose skip
// rules were never themselves tested is the instrument this suite trusts least. `installer_integrity.rs` keeps its
// own pins; this is a DIFFERENT instrument — syntax SHAPE, not wiring — and neither weakens nor duplicates the other.
//
// Run: node scripts/test/powershell-structure-check.mjs
import { readdirSync, readFileSync } from "node:fs";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";
const DIR = "agent/deploy";
// A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN, and this suite has paid for that lesson already (round 170
// measured it for script-syntax.bash: an empty file list printed "0 files parse" and exited 0). 7 `.ps1` files
// live under agent/deploy today; the floor leaves room for a deliberate removal and none for a collapse.
const FLOOR = 5;

const OPEN = { "{": "}", "(": ")", "[": "]" };
const CLOSE = { "}": "{", ")": "(", "]": "[" };

/** Every `.ps1` under `dir`, recursively, sorted. A directory WALK rather than `git ls-files`, so a file that
 *  cannot be read is a FAILURE here rather than an absence — the one thing this check must never do is pass
 *  quietly over something it did not look at. */
function ps1Files(dir) {
  const out = [];
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = `${dir}/${e.name}`;
    if (e.isDirectory()) out.push(...ps1Files(p));
    else if (e.name.endsWith(".ps1")) out.push(p);
  }
  return out.sort();
}

/** One left-to-right pass, skipping whole the things that make a bracket not-code: `#` comments, `<# … #>`
 *  comments, here-strings, single-quoted strings and double-quoted strings — EXCEPT that a `$( … )` inside a
 *  double-quoted string is code, and is scanned as such (see `doubleQuoted`). Returns the problems found, each
 *  naming the line it starts on. */
function scan(src, file) {
  const problems = [];
  const stack = []; // every open bracket, with the line it was opened on
  const n = src.length;
  let i = 0;
  let line = 1;
  const skipTo = (j) => {
    for (let k = i; k < j; k++) if (src[k] === "\n") line++;
    i = j;
  };
  const fail = (at, why) => problems.push(`${file}:${at} — ${why}`);

  /** A SINGLE-QUOTED STRING: `''` is an escaped quote, and everything else is literal — INCLUDING braces, which
   *  is how the installer writes launcher scripts and how the integrity tests carry JSON manifests. Returns false
   *  when it is never closed. */
  function singleQuoted() {
    const at = line;
    let j = i + 1;
    let closed = false;
    while (j < n) {
      if (src[j] === "'") {
        if (src[j + 1] === "'") { j += 2; continue; } // an escaped quote, still inside
        closed = true;
        break;
      }
      j++;
    }
    if (!closed) {
      fail(at, "a single-quoted string is never closed — `pwsh` would reject the file");
      return false;
    }
    skipTo(j + 1);
    return true;
  }

  /** A DOUBLE-QUOTED STRING: a backtick escapes the NEXT character (`` `" `` is a literal quote, which the
   *  integrity tests use to carry JSON) and `""` is an escaped quote.
   *
   *  `$( … )` INSIDE IT IS NOT TEXT. PowerShell evaluates it, so its braces and parens belong to the file's
   *  structure exactly like brackets outside the string — and a launcher line is where they are written. The
   *  subexpression's `(` joins the SAME stack, its body is scanned as code, and it ends at the `)` that closes it;
   *  a `$( … )` inside that body nests the same way. A plain `$name` — and `${name}`, whose braces are part of the
   *  NAME, not a group — is a variable reference and stays opaque. */
  function doubleQuoted() {
    const at = line;
    skipTo(i + 1); // the opening quote
    while (i < n) {
      const c = src[i];
      if (c === "`") { skipTo(Math.min(i + 2, n)); continue; } // the backtick escapes whatever follows
      if (c === '"') {
        if (src[i + 1] === '"') { skipTo(i + 2); continue; } // an escaped quote, still inside
        i++;
        return true;
      }
      if (c === "$" && src[i + 1] === "(") {
        stack.push({ ch: "(", line });
        skipTo(i + 2);
        if (!code(stack.length)) return false; // the subexpression, ended by the `)` closing this `(`
        continue;
      }
      if (c === "\n") line++;
      i++;
    }
    fail(at, "a double-quoted string is never closed — `pwsh` would reject the file");
    return false;
  }

  /** CODE — outside every string, and inside a `$( … )` subexpression. `subDepth` is the stack depth at which the
   *  subexpression's own `(` was pushed, or null outside one: a `)` arriving at that depth ends the subexpression
   *  and hands control back to the string that opened it. Returns false when a string or comment ran away, which
   *  stops the scan — `fail` has already recorded why. */
  function code(subDepth) {
    while (i < n) {
      const c = src[i];
      const at = line;

      // A COMMENT IS NOT CODE: `#` to end of line, and `<# … #>` — which the installer's own header (line 2) uses.
      if (c === "<" && src[i + 1] === "#") {
        const end = src.indexOf("#>", i + 2);
        if (end < 0) {
          fail(at, "`<#` opens a block comment that is never closed with `#>`, so `pwsh` would reject the file");
          return false;
        }
        skipTo(end + 2);
        continue;
      }
      if (c === "#") {
        const end = src.indexOf("\n", i);
        skipTo(end < 0 ? n : end);
        continue;
      }

      // A HERE-STRING IS NOT CODE EITHER: `@'` / `@"`, opened at the end of a line and closed by `'@` / `"@` at the
      // START of one (PowerShell 5.1's rule, and these scripts are `#Requires -Version 5.1`). Its body is raw text —
      // an installer writing a config puts exactly that in one — and a body holding an odd number of quotes would
      // otherwise desynchronize every line below it.
      if (c === "@" && (src[i + 1] === "'" || src[i + 1] === '"') && /^[^\S\n]*\r?\n/.test(src.slice(i + 2))) {
        const q = src[i + 1];
        const end = src.indexOf(`\n${q}@`, i + 2);
        if (end < 0) {
          fail(at, `\`@${q}\` opens a here-string that is never closed by \`${q}@\` at the start of a line`);
          return false;
        }
        skipTo(end + 3);
        continue;
      }

      if (c === "'") { if (!singleQuoted()) return false; continue; }
      if (c === '"') { if (!doubleQuoted()) return false; continue; }

      // OUTSIDE a string a backtick escapes the next character too: `` `{ `` is a literal brace, not a group, and a
      // backtick at the end of a line is a continuation.
      if (c === "`") {
        skipTo(Math.min(i + 2, n));
        continue;
      }

      if (OPEN[c]) {
        stack.push({ ch: c, line: at });
        i++;
        continue;
      }
      if (CLOSE[c]) {
        // THE SUBEXPRESSION ENDS HERE: its own `(` is on top, so it is consumed and the string resumes.
        if (c === ")" && stack.length === subDepth) {
          stack.pop();
          i++;
          return true;
        }
        const top = stack.pop();
        if (!top) fail(at, `\`${c}\` closes nothing — there is one closer too many`);
        else if (top.ch !== CLOSE[c]) fail(at, `\`${c}\` closes the \`${top.ch}\` opened at line ${top.line}`);
        i++;
        continue;
      }

      if (c === "\n") line++;
      i++;
    }
    return true;
  }

  code(null);

  for (const o of stack) fail(o.line, `\`${o.ch}\` is opened here and never closed — \`pwsh\` would reject the file`);
  return problems;
}

// ── THE SCANNER'S OWN FIXTURES. pwsh is not installed here, so these snippets are the only place the rules above
// are stated as VERDICTS — and the instrument had never been tested, only its subject. A skip rule one character
// too greedy reports "balanced" over precisely the edit this check exists to catch. The two rows that carry the
// weight are the first two: the SAME brace must FAIL inside a `$( … )` and PASS inside a single-quoted string,
// because the installer writes launcher scripts and JSON manifests that hold braces, and a scanner that cannot tell
// those apart fails correct code and gets reverted within a week.
const SELF_TEST = [
  ["a `{` opened inside a `$( … )` in a double-quoted string", '"$(Get-X { )"', false],
  ["braces that are TEXT — the JSON the integrity tests carry in `'…'`", '\'{"a":1}\'', true],
  ["`${name}` is a variable NAME, not a group", '"${name}"', true],
  ["a balanced `$( … )` carrying a group", '"$(Get-X -A { 1 })"', true],
  ["a `$( … )` nested inside another one", '"$(Get-X -A "$(Get-Y)")"', true],
  ["a backtick still escapes inside the string: `` `$( `` is literal", '"`$(Get-X { )"', true],
  ["a `)` inside a single-quoted string does not end the subexpression", '"$(Get-X -A \')\')"', true],
  ["a `#` comment's braces are not code", "# { and (\n( )\n", true],
];

let fixtureFailures = 0;
// A TABLE THAT LOST ITS ROWS IS NOT A TABLE THAT PASSED — the same rule as `FLOOR` above, for the instrument
// rather than the subject: the four verdicts this check is defined by are the minimum, and an empty table would
// otherwise print "0 scanner fixture(s) agree" and exit 0.
if (SELF_TEST.length < 4) {
  console.error(`FAIL powershell-structure: only ${SELF_TEST.length} scanner fixture(s) — the table has been emptied, so nothing states what the skip rules mean any more`);
  process.exit(1);
}
for (const [what, snippet, clean] of SELF_TEST) {
  const got = scan(snippet, "<fixture>");
  if ((got.length === 0) !== clean) {
    fixtureFailures++;
    console.error(
      `FAIL powershell-structure self-test: ${what} — expected ${clean ? "PASS" : "FAIL"}, got ` +
        `${clean ? "FAIL" : "PASS"}${got.length ? ` (${got.join("; ")})` : ""}`,
    );
  }
}
if (fixtureFailures) {
  console.error(
    `FAIL powershell-structure: ${fixtureFailures} of ${SELF_TEST.length} fixture(s) disagree — the scanner no longer\n` +
      `follows the rules this file states, so its verdict on the deploy .ps1 files proves nothing.`,
  );
  process.exit(1);
}

const problems = [];
let found;
try {
  found = ps1Files(`${ROOT}/${DIR}`);
} catch (e) {
  console.error(`FAIL powershell-structure: cannot walk ${DIR} (${e.code || e.message}) — a scan that cannot read its subject proves nothing`);
  process.exit(1);
}

let checked = 0;
for (const abs of found) {
  const rel = abs.slice(ROOT.length + 1); // absolute on disk, repo-relative in every message
  let src;
  try {
    src = readFileSync(abs, "utf8");
  } catch (e) {
    // LOUD, NOT SILENT: a file this check cannot read is a FAILURE, never an absence from the count.
    problems.push(`${rel} — CANNOT BE READ (${e.code || e.message}); a file this check cannot read must never count as a pass`);
    continue;
  }
  checked++;
  problems.push(...scan(src, rel));
}

// PROBLEMS BEFORE THE FLOOR, and the order is load-bearing: when the walk found files but could not READ them,
// every one of them landed in `problems` while `checked` stayed 0 — and the floor's "read only 0 file(s)" fired
// first and buried the reason. A loud failure that hides its own cause is half a loud failure.
if (problems.length) {
  console.error(
    `FAIL powershell-structure: ${problems.length} structural problem(s) in ${checked} file(s). This code runs AS ADMINISTRATOR on a\n` +
      `customer's machine, no \`pwsh\` on this box ever parses it, and \`script-syntax.bash\` does not walk \`.ps1\`:\n`,
  );
  for (const p of problems) console.error(`  ${p}`);
  process.exit(1);
}
if (checked < FLOOR) {
  console.error(`FAIL powershell-structure: read only ${checked} .ps1 file(s) under ${DIR}, expected at least ${FLOOR} — the scan is reading the wrong thing`);
  process.exit(1);
}
console.log(
  `powershell-structure: ${checked} .ps1 file(s) under ${DIR} balance {} () [] outside single-quoted strings, here-strings\n` +
    `and comments — and inside the \`$( … )\` subexpressions a double-quoted string carries. ${SELF_TEST.length} scanner fixture(s) agree.`,
);

// WHAT THIS CANNOT SEE, so the next reader does not mistake it for a parser: a `$( … )` inside a double-quoted
// HERE-STRING (`@" … "@`), whose body is still skipped whole — `pwsh` evaluates those too, the deploy files carry
// none today, and a here-string is where the installer puts text it does NOT want interpolated, so the two
// mistakes are not symmetric. A `$( … )` in a plain double-quoted string is OFF this list now: it is scanned by
// `doubleQuoted`, its brackets counted on the same stack as the file's own, and the `SELF_TEST` table above fails
// if that stops being true. What also remains invisible is anything semantic — a misspelled cmdlet, a wrong
// parameter, a missing `param` block; and whether the file would RUN. `pwsh` answers those, and it is not installed
// here, which is the whole reason this check exists at the level it does.
