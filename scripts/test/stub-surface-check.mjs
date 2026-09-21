// stub-surface-check.mjs — THE TWO BACKENDS OF ONE MODULE MUST OFFER THE SAME SURFACE.
//
// WHY THIS EXISTS (round 148 of the standing goal). Round 146 found that the agent's DEFAULT feature config had not compiled
// for a long time: `plugins/terminal/tools/exec.rs` calls `term_note_exit_code` in three places, the REAL terminal backend
// defines it, and the STUB — the backend compiled when the `terminal` feature is off — did not. Nothing feature-independent
// called it when it was added, which is exactly why it went unnoticed until a caller appeared in a third place.
//
// A configuration nobody builds is a configuration that is broken, and a surface with a hole in it breaks in ONE
// configuration only. This compares the two: every `pub async fn` the real backend offers must be offered by the stub, or
// declared here with a reason.
//
// WHAT IT DOES NOT CHECK: the bodies (a stub answers "nothing here" on purpose), the other modules, or whether a stub's
// answer is truthful — only that a caller cannot fail to COMPILE because it picked the wrong backend.
//
// Run: node scripts/test/stub-surface-check.mjs
import { readFileSync } from "node:fs";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";
const REAL = "agent/src/tools/terminal/mod.rs";
const STUB = "agent/src/tools/terminal/stub.rs";

const fns = (p) => new Set([...readFileSync(`${ROOT}/${p}`, "utf8").matchAll(/pub async fn (\w+)/g)].map((m) => m[1]));
const real = fns(REAL);
const stub = fns(STUB);

/** Names the stub deliberately does not mirror, each with the reason it cannot or should not. */
const DECLARED = {
  sweep_idle:
    "its return type is defined inside the feature-gated module, so a stub mirror cannot name it without moving the type — " +
    "and no feature-independent caller exists",
};

if (real.size < 20) {
  console.error(`FAIL read only ${real.size} method(s) from the real backend — the tree moved, so this proves nothing`);
  process.exit(1);
}

const findings = [];
for (const name of real) {
  if (!stub.has(name) && !DECLARED[name]) {
    findings.push(
      `${REAL} offers \`${name}\` and ${STUB} does not — a caller that compiles with the feature on will fail to compile ` +
        `without it, in a configuration CI may not build. Mirror it, or declare it here with the reason it cannot be`,
    );
  }
}
for (const name of stub) {
  if (!real.has(name)) {
    findings.push(`${STUB} offers \`${name}\` and ${REAL} does not — the two surfaces have drifted apart`);
  }
}
if (findings.length) {
  console.error(`stub-surface: ${findings.length} difference(s) between the backends:\n  ` + findings.join("\n  "));
  process.exit(1);
}
console.log(
  `stub-surface: ${real.size} method(s) in the real backend, ${stub.size} in the stub, ` +
    `${Object.keys(DECLARED).length} declared exception(s) — a caller cannot pick the wrong backend`,
);
