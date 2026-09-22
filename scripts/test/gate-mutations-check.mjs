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

/** Each case: the gate, the file it reads, and the EXACT break that gate was proven with.
 *
 *  ONE GATE IS AUDITED ELSEWHERE, and this is where that is declared rather than silently missing:
 *  `console-assets-check` REBUILDS the console, so it needs `gateway/ui` dependencies — which this job does not install (its
 *  first placement failed here with "Cannot find type definition file for 'vite/client'", which is a missing dependency and
 *  not a stale asset). Its proof is therefore the inconsistency it was written for (rounds 177-178) and the `ui` job, which is
 *  the only place it can run at all.
 *
 *  The owed `sweep-fixture-dupes-check` case arrived in round 107, and it needed no new shape after all: a fixture key on
 *  ONE line duplicated in place is a plain from/to replacement. (The earlier note assumed the `/api/health` block, which is
 *  multi-line — the wrong anchor made the shape look impossible.)
 *
 *  A case may carry `also`: further edits applied with the primary and restored with it, which is what let round 109
 *  represent `wire-field-check`'s second branch — a field planted in a hook AND in the harness while absent from every Rust
 *  and TypeScript source. That shape was owed for one round and paid for with four lines of the runner. */
const CASES = [
  {
    gate: "scripts/test/sweep-bundle-check.mjs",
    file: "agent/scripts/lib/sweep-bundle.mjs",
    why: "the browser target's preamble reaches for the native require — the harness payload would throw on load, in a page whose fixture is half-installed",
    from: `  if (target === "node") parts.push("const __nativeRequire = require;");`,
    to: `  parts.push("const __nativeRequire = require;");`,
  },
  {
    gate: "scripts/test/sweep-bundle-check.mjs",
    file: "agent/scripts/lib/sweep-bundle.mjs",
    why: "the loader's relative require returns the module id instead of calling __require (the defect round 266 shipped and its own gate caught)",
    // RE-PAIRED (round 269): the loader line moved inside a per-target ternary when the browser target arrived, so the
    // anchor had to move with it — the same "the mutation's anchor is GONE" the meta-gate reports, twice now, each time
    // because code moved rather than because the gate stopped working.
    from: `      ? "  const local = (spec) => (spec.startsWith('.') ? __require(__map[id][spec]) : __nativeRequire(spec));"`,
    to: `      ? "  const local = (spec) => (spec.startsWith('.') ? __map[id][spec] : __nativeRequire(spec));"`,
  },
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
  {
    gate: "scripts/test/production-host-check.mjs",
    file: "gateway/test/cors.test.mjs",
    why: "a file that just came OFF the declared-hosts list spells the deployment's host again — the regression a shrinking list can suffer",
    from: 'const AI = "https://console.vale.test";',
    to: 'const AI = "https://console.vale.test"; // https://agent.saisi.online',
  },
  {
    gate: "scripts/test/build-pins.bash",
    file: ".github/workflows/ci.yml",
    why: "a gate is written and never invoked, so it guards nothing",
    from: "      - name: every console wire field has a producer\n        run: node scripts/test/console-wire-field-check.mjs\n",
    to: "",
  },

  {
    gate: "scripts/test/sweep-fixture-dupes-check.mjs",
    // THE PAYLOAD MODULE, WHERE THE TABLE MOVED (round 268): the emitter holds no fixture markup any more, so a
    // mutation planted there was never read and the gate looked unproven.
    file: "agent/scripts/lib/sweep/console-run.cjs",
    why: "a sweep's fixture table answers one endpoint TWICE, so the last key silently wins and a page renders what nobody meant it to",
    from: "  '/api/version': { version: '1.0.106' },\n",
    to: "  '/api/version': { version: '1.0.106' },\n  '/api/version': { version: '9.9.9' },\n",
  },

  {
    gate: "scripts/test/wire-field-check.mjs",
    file: "agent/resources/panel-react/src/hooks/useAgentVitals.ts",
    also: [
      {
        file: "agent/scripts/panel-render-audit.mjs",
        from: "      running: P.get('pwrun') === '1',",
        to: "      running: P.get('pwrun') === '1', stub_only_field: 1,",
      },
    ],
    why: "a field a fixture carries and NO producer sends — it renders in the harness and would be undefined on a device",
    from: "export function useAgentVitals",
    to: "const _stub = (j: any) => j.stub_only_field;\nexport function useAgentVitals",
  },

  {
    gate: "scripts/test/console-wire-field-check.mjs",
    file: "gateway/ui/src/views/DevicesPanel.tsx",
    also: [
      {
        // THE SWEEP'S FIXTURE IS IN ITS PAYLOAD MODULE NOW (round 268) — this anchor moved with it.
        file: "agent/scripts/lib/sweep/console-run.cjs",
        from: "tunnel_up: true,",
        to: "tunnel_up: true, only_in_the_sweep: true,",
      },
    ],
    why: "a field only the SWEEP's fixture carries — it renders in the sweep and is undefined against the deployed worker",
    from: "const verdict = st?.update;",
    to: "const verdict = st?.update; const _s = st?.only_in_the_sweep; void _s;",
  },

  {
    gate: "scripts/test/one-derivation-check.mjs",
    file: "agent/resources/panel-react/src/components/MonitorChip.tsx",
    why: "a mark's CSS state spelled by hand in a component instead of by lib/monitorMark.ts (round 129's clause)",
    from: '<span className={monitorMarkClass("flapping")} aria-hidden="true" />',
    to: '<span className="monitor-mark is-flapping" aria-hidden="true" />',
  },

  {
    gate: "scripts/test/device-verdict-check.mjs",
    file: "gateway/ui/src/views/DevicesPanel.tsx",
    why: "the old comparison HOISTED out of the guarded fallback — the console answers for devices it cannot speak about (round 134)",
    from: "              const outdated = verdict\n                ? verdict.update_available\n                : !!d.lastVersion && !!install?.version && d.lastVersion !== install.version;",
    to: "              const outdated = !!d.lastVersion && !!install?.version && d.lastVersion !== install.version || !!verdict?.update_available;",
  },

  {
    gate: "scripts/test/wire-field-check.mjs",
    file: "agent/resources/panel-react/src/hooks/useAgentVitals.ts",
    also: [
      {
        file: "agent/scripts/panel-render-audit.mjs",
        from: "      running: P.get('pwrun') === '1',",
        to: "      running: P.get('pwrun') === '1', only_a_comment_field: 1,",
      },
      {
        file: "agent/src/vocabulary.rs",
        from: "pub const EXITED_PREFIX",
        to: "// only_a_comment_field: a comment is not a producer\npub const EXITED_PREFIX",
      },
    ],
    why: "a field carried by the harness and read by a hook, whose only 'producer' is a COMMENT in the Rust (round 135)",
    from: "export function useAgentVitals",
    to: "const _c = (j: any) => j.only_a_comment_field;\nexport function useAgentVitals",
  },

  {
    gate: "scripts/test/stub-surface-check.mjs",
    file: "agent/src/tools/terminal/stub.rs",
    why: "the stub loses a method the real backend offers — the round-146 defect, which broke ONE configuration for a long time",
    from: "    pub async fn term_permit_count(&self, _sid: &str) -> usize {\n        0\n    }\n",
    to: "",
  },

  {
    gate: "scripts/test/ci-command-table-check.mjs",
    file: ".github/workflows/ci.yml",
    why: "a check the table names stops being run by CI — the round-146 drift, in the direction that reads as coverage",
    from: "          cargo test -p vale-agent --features terminal,keyring\n",
    to: "",
  },

  {
    gate: "scripts/test/production-host-check.mjs",
    file: "scripts/test/production-host-check.mjs",
    why: "the declared list GROWS — the sentence 'the list may only shrink' had no gate until round 155",
    from: "const MAX_ALLOWED = 41;",
    to: "const MAX_ALLOWED = 9;",
  },

  {
    gate: "scripts/test/contract-vocabulary-check.mjs",
    file: "agent/contract-vocabulary.json",
    why: "the artifact claims a value the device never writes — the state that silently never renders (round 156's clause)",
    from: '"frames": [',
    to: '"frames": [\n    "a-state-nobody-emits",',
  },

  {
    gate: "scripts/test/contract-vocabulary-check.mjs",
    file: "agent/resources/panel-react/src/lib/bootNotice.ts",
    why: "an interface stops reading a value the device still emits — vocabulary spoken to nobody (round 157's symmetric clause)",
    from: '"machine-restart"',
    to: '"machine-restart-renamed"',
  },

  {
    gate: "scripts/test/console-derivation-check.mjs",
    file: "gateway/ui/src/views/Models.tsx",
    why: "a second copy of the prefix rule appears in the console — the round-172 defect, one call site at a time",
    from: "barePrefix(prefix)",
    to: 'prefix.replace(/\\/$/, "")',
  },

  {
    gate: "scripts/test/console-derivation-check.mjs",
    file: "gateway/ui/src/views/Overview.tsx",
    why: "a SECOND version comparison appears — a second verdict, where the device is the one that answers (round 175's rule)",
    // anchored on the SHORTEST unambiguous span: the longer version of this anchor compared equal by eye and failed by
    // machine, which is the invisible-character lesson this repository has recorded twice (a non-breaking space in a
    // selector, a typographic quote in a message). The mutation is the same either way.
    from: "d.lastVersion ||",
    to: "d.lastVersion !== st.version ? \"outdated\" : d.lastVersion ||",
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
  const paths = [c.file, ...(c.also ?? []).map((a) => a.file)];
  const originals = new Map(paths.map((f) => [f, readFileSync(`${ROOT}/${f}`, "utf8")]));
  const anchors = [[c.file, c.from], ...(c.also ?? []).map((a) => [a.file, a.from])];
  const broken = anchors.find(([f, from]) => !originals.get(f).includes(from));
  if (broken) {
    console.error(`FAIL ${c.gate}: its mutation's anchor is GONE from ${broken[0]} — re-pair it, or the gate is unproven`);
    findings++;
    continue;
  }
  const path = `${ROOT}/${c.file}`;
  const original = originals.get(c.file);
  if (!original.includes(c.from)) {
    console.error(`FAIL ${c.gate}: its mutation's anchor is GONE from ${c.file} — re-pair it, or the gate is unproven`);
    findings++;
    continue;
  }
  // THE RUNNER FOLLOWS THE FILE: this list holds `.mjs` gates and `.bash` ones, and the first version fed every `gate`
  // to `node` — which made `build-pins.bash` red before any mutation and the audit reported it as "the tree must be
  // green", a true sentence about a false premise.
  const runner = c.gate.endsWith(".bash") ? "bash" : "node";
  const before = run(runner, [c.gate]);
  if (before !== 0) {
    console.error(`FAIL ${c.gate} is RED before any mutation (rc=${before}) — the tree must be green for this to mean anything`);
    findings++;
    continue;
  }
  try {
    writeFileSync(path, original.replace(c.from, c.to));
    for (const a of c.also ?? []) writeFileSync(`${ROOT}/${a.file}`, originals.get(a.file).replace(a.from, a.to));
    if (c.emit) emit(c.emit);
    const after = run(runner, [c.gate]);
    if (after === 0) {
      console.error(`FAIL ${c.gate} did NOT bite: ${c.why} (it passed with the break in place)`);
      findings++;
    } else {
      console.log(`ok   ${c.gate} bites — ${c.why}`);
    }
  } finally {
    for (const [f, text] of originals) writeFileSync(`${ROOT}/${f}`, text);
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
