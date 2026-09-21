// gate-mutations-check.mjs — EVERY GATE, BROKEN ON PURPOSE, ON EVERY PUSH.
//
// WHY THIS EXISTS (round 105 of the standing goal). This repository's standard is that no gate is ASSUMED to bite: each is
// proven by breaking the thing it guards. That proof has always been a manual act, and round 104 showed what that costs —
// re-auditing six gates produced TWO false alarms, and both were my MUTATIONS rather than the gates: appending a comment
// to the query definition left `harness-fixture-check`'s pattern matching (correctly), and `(st as any)?.field` has `)` for
// a receiver so `console-wire-field-check`'s scan cannot see it (correctly). The fifth time in that session a non-biting
// mutation was about the mutation.
//
// So the mutations live HERE, in one place, paired with the gate each must fail — and this runs in CI. `harness-fixture-check`
// keeps its mutations inside itself for the same reason; this file is that idea for the gates whose logic reads files
// directly.
//
// SAFETY: it edits tracked files, so it REFUSES to start unless the working tree is clean, and it restores every file in a
// `finally`. A gate that cannot be broken is reported as a finding — that is the whole point.
//
// Run: node scripts/test/gate-mutations-check.mjs
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";
const git = (...a) => execFileSync("git", a, { cwd: ROOT, encoding: "utf8" }).trim();

const dirty = git("status", "--porcelain");
if (dirty) {
  console.error(
    `FAIL the working tree is not clean, so this check cannot tell a mutation from your work:\n${dirty}\n` +
      `Commit or stash first — this script edits tracked files and restores them.`,
  );
  process.exit(1);
}

/** Each case: the gate, the file it reads, and the EXACT break that gate was proven with. */
const CASES = [
  {
    gate: "scripts/test/session-row-check.mjs",
    file: "agent/resources/panel-react/src/hooks/useSessions.ts",
    why: "a device field read outside the mapping section",
    from: "const seen = new Set(list.map((s: any) => s.id));",
    to: "const seen = new Set(list.map((s: any) => s.id + String(s.last_exit_code)));",
  },
  {
    gate: "scripts/test/one-derivation-check.mjs",
    file: "agent/resources/panel-react/src/components/CommandCard.tsx",
    why: "a second mapping from an ending to a state",
    from: "  const st = cardState(card);",
    to: '  const st = card.reason === "backgrounded" ? ({ state: "warn" } as never) : cardState(card);',
  },
  {
    gate: "scripts/test/wire-field-check.mjs",
    file: "agent/resources/panel-react/src/hooks/useAgentVitals.ts",
    why: "a field the device harness and every fixture never speak",
    from: "export function useAgentVitals",
    to: "const _planted = (j: any) => j.last_boot_verdict;\nexport function useAgentVitals",
  },
  {
    gate: "scripts/test/console-wire-field-check.mjs",
    file: "gateway/ui/src/views/DevicesPanel.tsx",
    why: "a console field nobody produces (the round-100 shape, in the plain spelling the scan can see)",
    from: "const verdict = st?.update;",
    to: "const verdict = st?.update; const _p = st?.last_boot_verdict; void _p;",
  },
  {
    gate: "scripts/test/gateway-device-field-check.mjs",
    file: "gateway/src/plugins/mcp.ts",
    why: "the gateway forwards a field the device never spells",
    from: 'j.last_boot_kind === "crashed"',
    to: 'j.last_boot_verdict === "crashed"',
  },
  {
    gate: "scripts/test/device-verdict-check.mjs",
    file: "gateway/ui/src/views/DevicesPanel.tsx",
    why: "the console stops reading the device's own verdict",
    from: "const verdict = st?.update;",
    to: "const verdict = undefined;",
  },
  {
    gate: "scripts/test/harness-fixture-check.mjs",
    file: "agent/scripts/panel-render-audit.mjs",
    why: "the query is parsed ONCE again, which is what made the page answer 'crashed' with the flag in its URL",
    from: `  var P = { get: function (n) { return new URLSearchParams(location.search).get(n); },
            has: function (n) { return new URLSearchParams(location.search).has(n); } };`,
    to: "  var P = new URLSearchParams(location.search);",
    emit: "agent/scripts/panel-render-audit.mjs",
  },
  {
    gate: "scripts/test/contract-vocabulary-check.mjs",
    file: "agent/resources/panel-react/src/lib/contract.gen.ts",
    why: "one artifact's vocabulary diverges from the other's",
    from: '"machine-restart"',
    to: '"machine-reboot"',
  },
];

const run = (cmd, args) => {
  try {
    execFileSync(cmd, args, { cwd: ROOT, stdio: "pipe" });
    return 0;
  } catch (e) {
    return e.status ?? 1;
  }
};

/** The harness emitter's OWN convention is exit 2 on success (`rc=2 (2=ok)`, printed by every caller), so its exit code is
 *  not a pass/fail signal and this must not read it as one — the first version did, and the audit died on the one case
 *  that has to re-emit. */
const emit = (script) => {
  try {
    execFileSync("node", [script], { cwd: ROOT, stdio: "pipe" });
  } catch (e) {
    if ((e.status ?? 1) !== 2) throw e;
  }
};

let findings = 0;
for (const c of CASES) {
  const path = `${ROOT}/${c.file}`;
  const original = readFileSync(path, "utf8");
  if (!original.includes(c.from)) {
    console.error(`FAIL ${c.gate}: its mutation's anchor is GONE from ${c.file} — re-pair it, or the gate is unproven`);
    findings++;
    continue;
  }
  const before = run("node", [c.gate]);
  if (before !== 0) {
    console.error(`FAIL ${c.gate} is RED before any mutation (rc=${before}) — the tree must be green for this to mean anything`);
    findings++;
    continue;
  }
  try {
    writeFileSync(path, original.replace(c.from, c.to));
    if (c.emit) emit(c.emit);
    const after = run("node", [c.gate]);
    if (after === 0) {
      console.error(`FAIL ${c.gate} did NOT bite: ${c.why} (it passed with the break in place)`);
      findings++;
    } else {
      console.log(`ok   ${c.gate} bites — ${c.why}`);
    }
  } finally {
    writeFileSync(path, original);
    if (c.emit) emit(c.emit);
  }
}

const restored = git("status", "--porcelain");
if (restored) {
  console.error(`FAIL the tree is DIRTY after restoring:\n${restored}\nRestore it by hand before doing anything else.`);
  process.exit(1);
}
if (findings) {
  console.error(`\ngate-mutations: ${findings} gate(s) unproven — a gate that cannot fail is worse than no gate`);
  process.exit(1);
}
console.log(`\ngate-mutations: ${CASES.length} gate(s) broken on purpose and every one of them bit`);
