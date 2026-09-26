// ci-command-table-check.mjs — THE TABLE THAT SAYS WHAT CI RUNS MUST MATCH WHAT CI RUNS.
//
// WHY THIS EXISTS (round 151 of the standing goal). AGENTS.md carries a table of "the exact commands, by working directory",
// and round 146 proved it can drift: the table claimed the agent's checks run in "both feature sets" while the workflow ran
// ONE, and the missing configuration was broken at the time. A record that describes the checks is worth exactly as much as
// its agreement with them.
//
// TWO DIRECTIONS, because they fail differently:
//
//   A. DOC → CI   every check command the table names must appear in ci.yml. This is the direction that drifted: a table
//                 promising a check nobody runs reads as coverage that does not exist.
//   B. CI → DOC   every check-shaped step a job runs must be named somewhere in AGENTS.md, or declared below with a reason.
//                 A step nobody wrote down is one nobody can run by hand.
//
// WHAT IT DOES NOT CHECK: the prose, the working directories (the table states them, the workflow enforces them), or steps
// that are not checks — installs, uploads, deployments and one-off setup are out of scope by the pattern below.
//
// Run: node scripts/test/ci-command-table-check.mjs
import { readFileSync } from "node:fs";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";
const ci = readFileSync(`${ROOT}/.github/workflows/ci.yml`, "utf8");
const agents = readFileSync(`${ROOT}/AGENTS.md`, "utf8");

/** A CHECK is a command that answers "is this end well?" — not an install, a build artifact upload or a deploy. */
const IS_A_CHECK = /^(npm run (typecheck|lint|format:check|build|test)|npm test|cargo (test|clippy|fmt|check)\b)/;

/** Steps that are checks but deliberately not in the table, each with the reason it is not a per-end command. */
const DECLARED = {
  "cargo build -p summrise-agent --features terminal --bin summrise-agent":
    "it produces the binary the e2e step drives, so it is part of that step rather than a per-end check",
  "cargo xwin check -p summrise-agent --target x86_64-pc-windows-msvc --features terminal,keyring":
    "a cross-target CHECK whose command the Build section gives in full; the table is about the four ends' suites",
  "node scripts/test/skill-frontmatter-check.mjs": "every skill under .agents/skills/ must be one the agent can LOAD: a broken "
    + "frontmatter block fails SILENTLY by leaving the catalog (round 170)",
  "node scripts/test/http-route-header-check.mjs": "the device's own HTTP surface: every route the header NAMES must exist in the "
    + "Pattern table route_of resolves (rounds 146-150)",
  "node scripts/test/gate-mutations-check.mjs": "the audit itself; AGENTS.md points at the ledger for the gates it drives",
  "node scripts/test/stub-surface-check.mjs": "same: a gate, recorded in the ledger with its mutation",
  "bash scripts/design-sweep-ci.bash": "the design job's entry point, which the Build and design sections describe",
};

const findings = [];

// ── A. every check command the TABLE names must exist in the workflow
const tableStart = agents.indexOf("| where | CI runs | and NOT |");
const tableEnd = agents.indexOf("\n\n", tableStart);
const table = agents.slice(tableStart, tableEnd);
const claimed = new Set([...table.matchAll(/`([^`]+)`/g)].map((m) => m[1]).filter((c) => IS_A_CHECK.test(c.trim())));
for (const cmd of claimed) {
  // the table writes the command; the workflow writes it too, possibly inside a multi-line run block
  if (!ci.includes(cmd)) {
    findings.push(
      `AGENTS.md's table names \`${cmd}\` and NO step in ci.yml runs it — a record promising a check that does not exist ` +
        `(round 146's drift, in the direction that reads as coverage)`,
    );
  }
}

// ── A2. AND THE SCOPED INSTRUCTION FILE'S COMMANDS, because it inherits NONE of the root's gates (round 124).
// `agent/AGENTS.md` is the hottest file in this repository (792 commits when this was written) and it was carrying
// `cargo test` / `cargo clippy --all-targets` with no `-p` -- a SUBSET of what CI runs -- and `cargo fmt --all`, the
// MUTATING form, while CI runs `-- --check`. Nothing was checking it, because a scoped instruction file is read by
// whoever works in that scope, which is exactly when a wrong command costs most.
//
// AND THE FIRST VERSION OF THIS BLOCK FAILED ON A CLEAN TREE, which is worse than no gate: it blocked every commit until
// somebody weakened it. The cause is the line below that strips a TRAILING COMMENT -- `cargo fmt --all -- --check   # note`
// is not the string CI contains, and the extractor kept the note. **The bite alone is half a proof; the clean tree passing
// is the other half** (rounds 124 and 109 are the two halves of that lesson).
const agentAgents = readFileSync(`${ROOT}/agent/AGENTS.md`, "utf8");
const scoped = new Set(
  [...agentAgents.matchAll(/^([a-z][^\n`]*)$/gm)]
    .map((m) => m[1].replace(/\s+#.*$/, "").trim())
    .filter((c) => IS_A_CHECK.test(c)),
);
for (const raw of scoped) {
  const bare = raw.replace(/^cd [^&]+&&\s*/, "").trim();
  if (!ci.includes(bare) && !ci.includes(raw)) {
    findings.push(
      `agent/AGENTS.md names \`${raw}\` and NO step in ci.yml runs it — a scoped instruction file inherits none of its ` +
        `parent's gates (round 123), which is how the hottest file in the repository came to promise a different check`,
    );
  }
}

// ── B. every check-shaped step must be named in AGENTS.md or declared
const steps = [...ci.matchAll(/^\s+(?:- )?run: (.+)$/gm)].map((m) => m[1].trim());
const blocks = [...ci.matchAll(/run: \|\n((?:\s{10,}.*\n)+)/g)].flatMap((m) =>
  m[1]
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean),
);
for (const cmd of [...steps, ...blocks]) {
  if (!IS_A_CHECK.test(cmd) || DECLARED[cmd]) continue;
  const bare = cmd.replace(/\s+/g, " ");
  if (!agents.includes(cmd) && !agents.includes(bare)) {
    findings.push(`ci.yml runs \`${cmd}\` and AGENTS.md does not name it — a step nobody can run by hand`);
  }
}

if (claimed.size < 4) {
  console.error(`FAIL read only ${claimed.size} command(s) from the table — its shape changed, so this proves nothing`);
  process.exit(1);
}
if (findings.length) {
  console.error(`ci-command-table: ${findings.length} disagreement(s):\n  ` + findings.join("\n  "));
  process.exit(1);
}
console.log(
  `ci-command-table: the table names ${claimed.size} check(s) and ci.yml runs every one of them; ` +
    `${Object.keys(DECLARED).length} step(s) declared as not-per-end`,
);
