// ONE RULE, TWO PACKAGES, NO SHARED MODULE.
//
// "Which of the device's two version fields wins" is implemented on BOTH sides:
//
//   panel    agent/resources/panel-react/src/lib/agentVersion.ts   releaseVersion()
//   gateway  gateway/src/plugins/mcp.ts                            wireVersion()
//
// They cannot share code — different packages, different deploy targets — and BOTH have been broken:
// round-304 on the gateway side (the console read the frozen Cargo `version`, showed v1.0.145 forever, and the
// outdated badge never cleared), and the panel's own comment records the other direction (its desktop shell read
// v1.2.354 while Settings reported v1.0.145 for the SAME device). The panel names the lesson:
//
//   Two copies of a rule is what let them disagree.
//
// This gate does not merge them. It holds them to ONE rule and requires that each side NAMES the other, so a
// change on one side is visible from the other — which is the part neither had.
//
// Run: node scripts/test/device-version-rule-check.mjs
import { decomment } from "./lib/decomment.mjs";
import { readFileSync } from "node:fs";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";
const PANEL = "agent/resources/panel-react/src/lib/agentVersion.ts";
const GATEWAY = "gateway/src/plugins/mcp.ts";

// RAW, because the cross-references this checks live in COMMENTS — decomment erases the thing being checked.
// (The first version decomposed both and reported each side as naming nothing.)
const panelRaw = readFileSync(`${ROOT}/${PANEL}`, "utf8");
const gatewayRaw = readFileSync(`${ROOT}/${GATEWAY}`, "utf8");
// Decommented, because the precedence check reads CODE and a comment could mention either name in any order.
const panel = decomment(panelRaw);
const gateway = decomment(gatewayRaw);

const problems = [];

// 1. Each side must still implement the rule, under a name, so a reader can find it.
if (!/export function releaseVersion\(/.test(panel)) {
  problems.push(`${PANEL}: \`releaseVersion\` is gone — the panel's half of the rule has no name.`);
}
if (!/export function wireVersion\(/.test(gateway)) {
  problems.push(
    `${GATEWAY}: \`wireVersion\` is gone. This was two inline lines until round 236; without a name the ` +
      `console's half of the rule cannot be pointed at, tested, or found by the other side.`,
  );
}

// 2. Both must prefer `release`, and only then fall back to `version`. Order is the whole rule: `version` is the
//    FROZEN Cargo protocol version, so reading it first is what showed v1.0.145 forever.
function prefersRelease(body, label) {
  // READ THE BRANCHES, NOT THE FIRST MENTION. The first version of this used `indexOf` over the whole body,
  // which found the TypeScript annotation (`as { release?: unknown; version?: unknown }`) — so it measured
  // DECLARATION order and stayed green when the two `return`s were swapped. A mutation proved it: reversing
  // the branches in `wireVersion` left this gate passing.
  const branches = [...body.matchAll(/if \(([^)]*)\)/g)].map((m) => m[1]);
  const first = branches.find((b) => b.includes("release") || b.includes("version"));
  if (!first || !first.includes("release")) {
    problems.push(
      `${label}: its first version branch does not test \`release\`. \`version\` is the frozen Cargo ` +
        `protocol version (1.0.x); reading it first is the round-304 defect that showed v1.0.145 forever.`,
    );
  }
}
function bodyOf(src, name) {
  const i = src.indexOf(`export function ${name}(`);
  if (i < 0) return "";
  const open = src.indexOf("{", i);
  let depth = 0;
  for (let j = open; j < src.length; j++) {
    if (src[j] === "{") depth++;
    else if (src[j] === "}") {
      depth--;
      if (depth === 0) return src.slice(open + 1, j);
    }
  }
  return "";
}
prefersRelease(bodyOf(panel, "releaseVersion"), PANEL);
prefersRelease(bodyOf(gateway, "wireVersion"), GATEWAY);

// 3. Each side must NAME the other. This is the part that was missing, and the only part that makes a change on
//    one side visible from the other without a shared module.
if (!panelRaw.includes("gateway/src/plugins/mcp.ts")) {
  problems.push(
    `${PANEL}: no longer names \`gateway/src/plugins/mcp.ts\`. The console applies the same rule to the same ` +
      `device; if this file does not say so, the next change here is invisible from there.`,
  );
}
if (!gatewayRaw.includes("agent/resources/panel-react/src/lib/agentVersion.ts")) {
  problems.push(
    `${GATEWAY}: no longer names the panel's \`agentVersion.ts\`. Same reason, other direction.`,
  );
}

if (problems.length) {
  console.error("FAIL device-version-rule: the two halves of this rule have drifted apart.\n");
  for (const p of problems) console.error(`  ${p}`);
  process.exit(1);
}

console.log(
  "device-version-rule: both halves prefer `release` over the frozen `version`, and each names the other.",
);
