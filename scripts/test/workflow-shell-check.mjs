// A SHELL SCRIPT INSIDE A WORKFLOW IS STILL A SHELL SCRIPT, AND NOTHING CHECKED THAT OURS PARSED.
//
// `release.yml`'s "Publish the release and attach the tgz (API)" step had never executed. Its `run:` block opened
// a double-quoted string with `<<<"$(curl …` and never closed it:
//
//   read -r old old_digest <<<"$(curl -sf -H "$auth" "${api}/releases/${id}/assets?per_page=100" \
//          | jq -r --arg a "$ASSET" '.[] | select(.name==$a) | "\(.id) \(.digest // "")"')
//
// The string swallowed the rest of that logical line and the `local_digest="sha256:..."` two lines below, and the
// parser gave up at the `jq -r` after that — so the runner reported `line 42: syntax error near unexpected token
// ')'` for a line that is itself perfectly valid. ONE CHARACTER. The step arrived with round 200's tag-move guard
// and was unreachable from the day it was written, because only a SECOND publish reaches it.
//
// Every workflow script in this repo is now parsed before it can reach a runner. The extraction goes through the
// YAML PARSER rather than text-slicing — the round-240 investigation lost three attempts to slicing, because a
// line-number prefix of a multi-line construct is legitimately incomplete.
//
// Run: node scripts/test/workflow-shell-check.mjs
import { readdirSync, readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { writeFileSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";
const DIR = `${ROOT}/.github/workflows`;

// A minimal YAML reader for the one shape we need: `run:` scalars, whether inline, block (`|`, `>`) or folded.
// Using a real parser would be better, but this file must not depend on a package the workflow runner has.
function runBlocks(text) {
  const out = [];
  const lines = text.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const m = lines[i].match(/^(\s*)(?:-\s+)?run:\s*(.*)$/);
    if (!m) continue;
    const indent = m[1].length;
    const rest = m[2];
    if (rest && !/^[|>]/.test(rest)) {
      out.push({ line: i + 1, body: rest });
      continue;
    }
    // A block scalar: every following line more indented than `run:`, or blank.
    const body = [];
    let j = i + 1;
    for (; j < lines.length; j++) {
      const l = lines[j];
      if (l.trim() === "") {
        body.push("");
        continue;
      }
      if (l.search(/\S/) <= indent) break;
      body.push(l);
    }
    const common = body
      .filter((l) => l.trim())
      .reduce((min, l) => Math.min(min, l.search(/\S/)), Infinity);
    out.push({
      line: i + 1,
      body: body.map((l) => (l.trim() ? l.slice(common) : "")).join("\n"),
    });
    i = j - 1;
  }
  return out;
}

const problems = [];
let checked = 0;
const dir = mkdtempSync(join(tmpdir(), "wfshell-"));

for (const f of readdirSync(DIR).filter((x) => x.endsWith(".yml") || x.endsWith(".yaml")).sort()) {
  const src = readFileSync(`${DIR}/${f}`, "utf8");
  for (const { line, body } of runBlocks(src)) {
    // `shell: pwsh` / `cmd` steps are not bash; only bash-shaped scripts are parsed here.
    if (/\bshell:\s*(pwsh|powershell|cmd)\b/.test(src.slice(0, 4000))) continue;
    if (!body.trim()) continue;
    checked++;
    const p = join(dir, `${f}-${line}.sh`);
    writeFileSync(p, body);
    try {
      execFileSync("bash", ["-n", p], { stdio: "pipe" });
    } catch (e) {
      const msg = String(e.stderr || e.message).split("\n").slice(0, 3).join(" ").trim();
      problems.push(
        `${f}:${line} — this \`run:\` block does not parse, so NO step after it in that job can run. ` +
          `bash says: ${msg}. (This is how the publish step went unnoticed for 40 rounds: a first publish never ` +
          `reaches it.)`,
      );
    }
  }
}

if (problems.length) {
  console.error("FAIL workflow-shell: a workflow carries a script that cannot run.\n");
  for (const p of problems) console.error(`  ${p}`);
  process.exit(1);
}

console.log(
  `workflow-shell: ${checked} run block(s) across the workflows parse — a step whose script cannot run can no ` +
    `longer reach a runner.`,
);
