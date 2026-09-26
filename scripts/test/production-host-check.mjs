// production-host-check.mjs — WHERE THE DEPLOYMENT'S HOSTNAMES MAY APPEAR, AND WHERE THEY MAY NOT.
//
// WHY THIS EXISTS (round 46 of the standing goal). The operator asked twice (round 95, then again this week) to get
// the deployment's domain out of a PUBLIC repository. Measured before touching anything: **493 occurrences across 118 files**,
// and three quarters of it is infrastructure this repository genuinely IS — the update channel the agent ships with,
// the CDN the release scripts smoke-test, the worker's routing suffix, the installer templates. A blind scrub breaks
// the update path for real devices; a rename is a project with DNS in it.
//
// SO THE RULE IS ENFORCED RATHER THAN REMEMBERED, the shape `retired-colours-check.mjs` already uses: the files that
// legitimately name a production host are DECLARED below with a reason, and any OTHER file that mentions one fails this
// gate. The list may only shrink; a new entry needs a sentence saying why the deployment's hostname belongs in the
// tree. Every entry is a debt with an owner, not a permission.
//
// Run: node scripts/test/production-host-check.mjs
import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const HOST = /saisi\.online/;

// file (or directory prefix) : why this one may name a production host
const ALLOWED = [
  // ── the release + distribution path: these MUST name the real host to do their job ──
  ["scripts/", "cut, publish, smoke and audit a release against the live CDN"],
  ["index/", "the CDN worker and its landing page ARE the download site"],
  ["agent/deploy/", "installer templates and their docs: the URL a customer installs from"],
  ["agent/summrise-agent-npm/", "the npm package's README and CLI defaults name the update channel"],
  ["proxies/", "the satellite workers' routes and their operational README"],
  // ADDED 2026-09-24, AND THE COMMIT SAYS WHY — which is the gate's own instruction for a genuine
  // need ("that is a conversation, not an edit"). The relay's worker config names the host twice by
  // necessity: the route pattern that hands it /files/*, and PUBLIC_BASE, which is what it mints
  // one-time download URLs from. The alternative was to move both to the Cloudflare dashboard, which
  // would make declared config manual config — the opposite of what AGENTS.md asks for after a
  // cutover. `relay/` had been undeclared in git entirely until the previous day.
  ["relay/", "the file relay's OWN worker config: its /files/* route and PUBLIC_BASE must name the host to mint one-time download URLs (D6 cut the relay out of the CDN worker)"],
  // ── the worker's own configuration ──
  ["gateway/wrangler", "the deployment's own routes and vars"],
  ["gateway/src/http.ts", "the CONSOLE_HOST allowed-origins list, which is the deployment's identity"],
  ["gateway/src/device-fetch.ts", "the DEVICE_HOST_SUFFIX fallback — the rule that decides what a device hostname is"],
  ["gateway/src/plugins/devices.ts", "the INDEX_WORKER_URL / INSTALL_SOURCE fallbacks"],
  ["gateway/ui/", "the console's own API client defaults"],
  ["gateway/scripts/", "the console's build + sync scripts"],
  ["gateway/public/", "the console's Source Viewer mirror, generated from the files above"],
  // ── docs the operator reads ──
  ["README.md", "the front door documents the shipped install command"],
  ["AGENTS.md", "the release section documents the shipped install command"],
  ["agent/AGENTS.md", "same, on the agent side"],
  ["docs/agents/ideas.md", "rows 21 and 23: the question, its measurements, and the decision"],
  // `docs/agents/inventory.md` WAS HERE UNTIL ITS ONE MENTION WAS REWORDED (round 47). The entry is gone rather than
  // kept for a file that no longer needs it: an allowance that matches nothing is a stale debt, and the gate will say
  // so the moment the file mentions a host again. The list may only shrink — this is it shrinking.
  ["docs/agents/design-ledger.md", "the long form's record of this decision"],
  // THE FIRST TIME THIS LIST WENT UP, AND THE TWO GATES THAT FORCED IT (round 273). `ledger-budget-check.mjs` now ENFORCES a
  // 400,000-byte ceiling that this ledger had crossed, so 134 KB of early rounds had to move somewhere; and THIS gate counts
  // FILES while the debt it tracks is really OCCURRENCES, so a file being SPLIT reads here as the debt growing. IT DID NOT
  // GROW, and the gate prints both numbers: **407 occurrences in 111 files before the split and 407 in 111 after it** — the
  // same three URLs, character for character, in the same narrative. The alternative was to reword a round's own record to
  // satisfy a ratchet, which is the tail wagging the dog, or to name the new file so that a WIDENED PREFIX covered it — which
  // is the same growth with the evidence hidden, and is exactly what this ratchet exists to prevent. So the constant below
  // goes up once, visibly, and the reason is written here rather than inferred.
  ["docs/agents/ledger-early-rounds.md", "the SAME record, in the file the round-273 ceiling split moved it to"],
  // ── the agent's own runtime defaults and the config it ships with ──
  ["agent/config.yaml", "the embedded config a fresh install starts from: the update channel and console URL"],
  ["agent/src/bootstrap.rs", "the embedded-config default and the tests that pin what a fresh install gets"],
  ["agent/src/main.rs", "startup logging/registration that names the configured console"],
  ["agent/src/register.rs", "self-registration against the configured console; its URL is the deployment's"],
  ["agent/src/web/mod.rs", "status/registration surfaces that report the configured URLs"],
  ["agent/src/tunnel.rs", "the ingress host a device's tunnel must join (round 45 made the proxy URL configurable)"],
  ["agent/src/plugins/update/tools.rs", "the update plugin: the manifest URL, its tests, and the channel it reports"],
  ["agent/src/plugins/system/tools.rs", "the console_url fallback for tools that call home"],
  ["agent/src/plugins/memory/sanitize.rs", "a redaction test whose INPUT is a URL carrying the host"],
  ["agent/src/plugins/design/tools.rs", "design-tool fixtures that upload to the configured console"],
  ["agent/scripts/", "the sweeps' emitted probes fetch the device's panel through the configured host"],
  ["agent/resources/panel-react/scripts/", "the panel's mock agent, which answers with the deployment's shape"],
  ["agent/tests/fixtures/", "wire fixtures shared by the Rust and panel tests: the deployment's own values"],
  // ── the worker's other configured defaults ──
  ["gateway/src/auth.ts", "a comment explaining why a device panel is same-site with the console"],
  ["gateway/src/channels.ts", "the AI channel defaults (oracle/zen-us/zen-go) a deployment ships with"],
  ["gateway/src/upstream.ts", "the upstream provider defaults those channels point at"],
  ["gateway/src/plugins/translate.ts", "the translate plugin's exit default"],
  ["gateway/src/store/", "store defaults that name the deployment's own hosts"],
  [".github/workflows/release.yml", "the release job that must reach the live CDN and gateway"],
  // ── tests whose SUBJECT is the hostname rule ──
  // NAMED INDIVIDUALLY, NOT AS A DIRECTORY (round 49). Five files in `gateway/test` used the production host only as an
  // INPUT (a device row, a tunnel name) and now use the reserved test domain `summrise.test`; the fifteen below assert the
  // deployment's own identity — a default, an allowlist, the suffix rule — so they keep it, and a NEW test file cannot
  // inherit the allowance by living in the same directory.
  // THE THIRD KIND OF HOST HAS TWO FACES (rounds 117-121), and each entry below says WHICH FACE it is, because a generic
  // reason is the kind that stops being true without anybody noticing.
  // Face 1: a test whose SUBJECT is the shipped default — migrating it would delete the thing under test.
  ["gateway/test/devices-validate.test.mjs", "face 1 — its first test is named \"default suffix\" and passes {} on purpose: the shipped default IS the subject"],
  // Face 2: a base URL the PRODUCT chooses (the relay/exit endpoints), which those tests exist to pin.
  ["gateway/test/gateway.test.mjs", "face 2 — the relay and exit BASE URLs the product chooses; the tests exist to pin them"],
  ["gateway/test/registry.test.mjs", "face 2 — the DEFAULTS of usProxyBase/museResponsesExit; moving them is a design change, not a fixture one"],
  ["gateway/test/summrise-cli.test.mjs", "face 2 — the gateway base the CLI defaults to (SUMMRISE_GATEWAY), which the test asserts"],
  ["agent/resources/panel-react/src", "panel fixtures and tests that render device rows"],
];

// THE LIST MAY ONLY SHRINK, AND THAT SENTENCE HAD NO GATE (round 155). It was written twice in this file and nothing enforced
// it: adding an entry passed silently, which is a debt that could grow while the comment claimed otherwise. The ratchet is
// the sentence made mechanical — it goes DOWN whenever an entry leaves, and UP only in a commit that says why, which is what
// the failure message below has always asked for and what round 273 did ONCE (42 → 43). The "UP never" this comment used to
// say was stronger than the code it describes: the growth branch exists and prints its own condition, and a rule a reader
// cannot follow is worse than no rule. THE FIRST UP HAD A CAUSE WORTH KNOWING: `ledger-budget-check` now enforces the
// ledger's 400,000-byte ceiling, the ledger was over it, and the split that fixed that added a FILE — while the debt this
// list tracks is OCCURRENCES. Two gates, one counting files and one counting bytes, and the ledger cannot satisfy both
// without this constant moving.
const MAX_ALLOWED = 43;
if (ALLOWED.length > MAX_ALLOWED) {
  console.error(
    `production-host: the declared list GREW to ${ALLOWED.length} from ${MAX_ALLOWED}. This list is a debt with owners, not ` +
      `a permission: it may only shrink. Take the hostname out of the file, or change this constant in a commit that says why.`,
  );
  process.exit(1);
}
if (ALLOWED.length < MAX_ALLOWED) {
  console.error(
    `production-host: the declared list SHRANK to ${ALLOWED.length} from ${MAX_ALLOWED} — lower MAX_ALLOWED to ` +
      `${ALLOWED.length} in this commit, so the ratchet keeps the ground it gained.`,
  );
  process.exit(1);
}


const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";
const tracked = execSync("git ls-files", { encoding: "utf8", cwd: ROOT })
  .split("\n")
  .filter(Boolean);

let offenders = [];
const byFile = new Map();
// READ THE WORKING TREE, NOT HEAD. The first version used `git show HEAD:<file>`, which made the gate blind to the one
// thing it exists for: a mutation that PLANTS a new file naming the host passed, because a file that is not yet in HEAD
// cannot be read from it — the gate would have failed one commit LATE. `git ls-files` is what keeps build artifacts out;
// the content comes from disk.
for (const f of tracked) {
  let text;
  try {
    text = readFileSync(join(ROOT, f), "utf8");
  } catch {
    continue;
  }
  const hits = (text.match(/saisi\.online/g) || []).length;
  if (!hits) continue;
  byFile.set(f, hits);
  if (ALLOWED.some(([p]) => f === p || f.startsWith(p))) continue;
  offenders.push(`${f} (${hits})`);
}

const total = [...byFile.values()].reduce((a, b) => a + b, 0);
console.log(
  `production-host: ${total} occurrence(s) in ${byFile.size} file(s); ${ALLOWED.length} declared location(s)`,
);
if (offenders.length) {
  console.error(
    `\nFAIL ${offenders.length} file(s) name a production host outside the declared list:\n  ` +
      offenders.slice(0, 12).join("\n  ") +
      (offenders.length > 12 ? `\n  … and ${offenders.length - 12} more` : ""),
  );
  console.error(
    "\nEither take the hostname out of the file, or declare its path in ALLOWED in this script WITH A REASON — AND RAISE\n" +
      "MAX_ALLOWED BY ONE IN THE SAME COMMIT. BOTH STEPS, because this message used to name only the first, and a reader who\n" +
      "followed it met a SECOND refusal from the ratchet above: an instruction that cannot be carried out as written, which is\n" +
      "the shape this repository keeps finding. Round 273 is the measurement — the loop followed this line, added the entry, and\n" +
      "was stopped one commit later by the constant. The list is a debt with owners, not a permission: it may only shrink, and\n" +
      "the one UP it has ever taken (42 -> 43, round 273) was a FILE being split, not a host being added.",
  );
  process.exit(1);
}
console.log("production-host: no undeclared file names one");
