// workflow-yaml-check — GITHUB HAS TO BE ABLE TO PARSE THE FILE, OR NOTHING IN IT RUNS AT ALL.
//
// MEASURED, round 273, and this is the largest outage in this repository's history. Three gates were wired into `ci.yml` by
// appending their command lines UNDER the previous step's `run:` instead of starting a new `- name:` / `run:` pair:
//
//       - name: production hosts are declared
//         run: node scripts/test/production-host-check.mjs
//         node scripts/test/http-route-header-check.mjs      <-- no `run:` key, and YAML has no such shape
//         node scripts/test/skill-frontmatter-check.mjs
//         node scripts/test/agents-snippet-check.mjs
//
// The file stopped parsing. GitHub answered EVERY push with a completed run carrying ZERO jobs and `conclusion: failure` —
// which is indistinguishable, in a count, from a real red, and which has NO LOG TO OPEN, because no job ever started:
//
//   commit       jobs  conclusion  created == updated
//   8ca63398      11   success     no      <- the last commit CI actually ran
//   e92fbf32       0   failure     yes     <- the first orphaned line
//   ... 35 commits, all 0 jobs, all "failure"
//
// **AND NOTHING HERE COULD SEE IT, WHICH IS THE PART WORTH FIXING.** `workflow-shell-check.mjs` owns "the workflows' shell
// parses"; its header says "the extraction goes through the YAML PARSER rather than text-slicing", and its implementation
// is a hand-rolled `run:` regex whose own comment admits "Using a real parser would be better". A line with no `run:` key is
// not a `run:` block, so all three orphans were skipped. `all-gates.bash` derives its list from the same file by regex and
// ran all 61 commands happily — locally green, remotely dead. **The instrument read the file as TEXT; the other end reads
// it as YAML**, which is AGENTS.md's "run the command the other end runs" one level down.
//
// HOW THIS CHECK DECIDES, AND WHY IT IS TWO INSTRUMENTS. The authority is a REAL YAML PARSER (`python3 -c "import yaml"`),
// because approximating a parser is what caused this. But the runner is not guaranteed to carry PyYAML, and a gate that
// refuses to run is a gate somebody deletes — so there is a portable structural fallback, and IT IS DIFFERENTIALLY PROVEN
// against the real parser rather than asserted: `--differential` replays every revision of every workflow file in this
// repository's history through both and reports every disagreement. Run it when you change the fallback.
//
// WHAT THE FALLBACK CANNOT SEE, said here rather than discovered later: it looks for the shape that broke this file — a
// line that is neither a mapping key, nor a sequence item, nor more indented than the key above it — so a YAML error that
// IS one of those three shapes (a duplicate key, a tab, a bad flow collection) passes it. That is why the real parser runs
// first wherever one exists, and why the line below says which instrument decided.
//
// Run: node scripts/test/workflow-yaml-check.mjs
//      node scripts/test/workflow-yaml-check.mjs --differential
import { readdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { pathToFileURL } from "node:url";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";
const DIR = `${ROOT}/.github/workflows`;

// THE MODULE IS IMPORTABLE WITHOUT RUNNING THE CHECK (measured: it was not, and importing `structuralVerdict` to test the
// fallback against the real broken file printed the pass line and called `process.exit` on the way). Anything a reader
// imports must not have side effects, or the only way to test it is to copy it.
const isMain = process.argv[1] && pathToFileURL(realpathSync(process.argv[1])).href === import.meta.url;

/** The real parser, when this host has one. Returns null when it does not — NEVER a silent pass. */
function realParserVerdict(path) {
  try {
    execFileSync("python3", ["-c", "import yaml,sys; yaml.safe_load(open(sys.argv[1]))", path], { stdio: "pipe" });
    return { ok: true, why: "" };
  } catch (e) {
    if (e.code === "ENOENT" || /ModuleNotFoundError|ImportError/.test(String(e.stderr))) return null; // no parser here
    const msg = String(e.stderr || e.message).split("\n").filter((l) => l.trim()).slice(-1)[0] || "does not parse";
    return { ok: false, why: msg.trim() };
  }
}

/** The portable fallback. See the header for what it cannot see, and `--differential` for how it is held to account. */
export function structuralVerdict(text) {
  const lines = text.split("\n");
  let lastKeyIndent = 0;
  let blockScalarIndent = -1; // indent of the key whose value is a `|` / `>` block, or -1
  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    if (!raw.trim()) continue;
    const indent = raw.search(/\S/);
    const content = raw.trim();
    if (content.startsWith("#")) continue;
    if (content === "---" || content === "...") continue;
    // A block scalar swallows every line more indented than the key that opened it.
    if (blockScalarIndent >= 0) {
      if (indent > blockScalarIndent) continue;
      blockScalarIndent = -1;
    }
    if (content.startsWith("-")) {
      const after = content.replace(/^-\s*/, "");
      if (after.includes(":")) lastKeyIndent = indent;
      continue;
    }
    if (content.includes(":")) {
      lastKeyIndent = indent;
      if (/:\s*[|>][-+0-9]*\s*$/.test(content)) blockScalarIndent = indent;
      continue;
    }
    if (indent > lastKeyIndent) continue; // a plain or quoted scalar continuing on the next line
    return {
      ok: false,
      line: i + 1,
      why: `line ${i + 1} is indented like the key above it and has no ":" — YAML reads it as a key and gives up`,
    };
  }
  return { ok: true, why: "" };
}

const files = readdirSync(DIR)
  .filter((x) => x.endsWith(".yml") || x.endsWith(".yaml"))
  .sort()
  .map((f) => `${DIR}/${f}`);

// ── the differential: every revision of every workflow, both instruments, every disagreement printed ──
if (isMain && process.argv.includes("--differential")) {
  const shas = execFileSync("git", ["log", "--format=%H", "--", ".github/workflows"], { cwd: ROOT, encoding: "utf8" })
    .split("\n")
    .filter(Boolean);
  let compared = 0;
  const disagreements = [];
  for (const sha of shas) {
    let names;
    try {
      names = execFileSync("git", ["ls-tree", "-r", "--name-only", sha, ".github/workflows"], { cwd: ROOT, encoding: "utf8" })
        .split("\n")
        .filter((n) => n.endsWith(".yml") || n.endsWith(".yaml"));
    } catch {
      continue;
    }
    for (const n of names) {
      let text;
      try {
        text = execFileSync("git", ["show", `${sha}:${n}`], { cwd: ROOT, encoding: "utf8", maxBuffer: 1 << 26 });
      } catch {
        continue;
      }
      const tmp = `/tmp/wfyaml-${sha.slice(0, 8)}-${n.split("/").pop()}`;
      writeFileSync(tmp, text);
      const real = realParserVerdict(tmp);
      if (real === null) {
        console.error("workflow-yaml: no real parser on this host, so the differential cannot run");
        process.exit(3);
      }
      const mine = structuralVerdict(text);
      compared++;
      if (real.ok !== mine.ok) disagreements.push({ sha: sha.slice(0, 9), n, real, mine });
    }
  }
  if (disagreements.length) {
    console.error(`FAIL workflow-yaml: the fallback and the real parser disagree on ${disagreements.length} of ${compared}:\n`);
    for (const d of disagreements.slice(0, 20)) {
      console.error(`  ${d.sha} ${d.n}: real=${d.real.ok ? "parses" : "REJECTS"} fallback=${d.mine.ok ? "parses" : "REJECTS"}`);
      if (!d.real.ok) console.error(`      real: ${d.real.why}`);
      if (!d.mine.ok) console.error(`      fallback: ${d.mine.why}`);
    }
    process.exit(1);
  }
  console.log(
    `workflow-yaml: the fallback agrees with the real parser on all ${compared} workflow revision(s) in this ` +
      `repository's history — both directions, zero disagreements.`,
  );
  process.exit(0);
}

// ── the check itself: the real parser where one exists, the fallback where it does not ──
if (isMain) {
  const problems = [];
  let byReal = 0;
  let byFallback = 0;
  for (const path of files) {
    const real = realParserVerdict(path);
    const name = path.slice(ROOT.length + 1);
    if (real === null) {
      byFallback++;
      const mine = structuralVerdict(readFileSync(path, "utf8"));
      if (!mine.ok) problems.push(`${name}: ${mine.why} (structural fallback — no YAML parser on this host)`);
    } else {
      byReal++;
      if (!real.ok) {
        problems.push(
          `${name}: GITHUB CANNOT PARSE THIS FILE, SO NO JOB IN IT WILL EVER START — every push gets a run with ZERO ` +
            `jobs and conclusion "failure", with no log to open. The parser says: ${real.why}`,
        );
      }
    }
  }

  if (problems.length) {
    console.error("FAIL workflow-yaml: a workflow file is not valid YAML.\n");
    for (const p of problems) console.error(`  ${p}`);
    console.error(
      "\n  The shape that caused this once (round 273): a command written on the line BELOW a step's `run:` instead of in " +
        "its own `- name:` / `run:` pair. Three rounds did it, CI was dead for ~35 commits, and the local suite stayed green " +
        "because it reads this file as TEXT.",
    );
    process.exit(1);
  }

  console.log(
    `workflow-yaml: ${files.length} workflow file(s) parse — ${byReal} by the real YAML parser, ${byFallback} by the ` +
      `structural fallback. A file GitHub cannot parse can no longer reach main unnoticed.`,
  );
}
