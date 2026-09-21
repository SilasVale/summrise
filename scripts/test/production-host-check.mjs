// production-host-check.mjs — WHERE THE DEPLOYMENT'S HOSTNAMES MAY APPEAR, AND WHERE THEY MAY NOT.
//
// WHY THIS EXISTS (round 46 of the standing goal). The operator asked twice (round 95, then again this week) to get
// `saisi.online` out of a PUBLIC repository. Measured before touching anything: **461 occurrences across 118 files**,
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
  ["agent/vale-agent-npm/", "the npm package's README and CLI defaults name the update channel"],
  ["proxies/", "the satellite workers' routes and their operational README"],
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
  ["docs/agents/inventory.md", "the audit that measured where the hosts appear"],
  ["docs/agents/design-ledger.md", "the long form's record of this decision"],
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
  ["gateway/test/", "routing, SSRF and host-allow tests: the hostname IS the input under test"],
  ["agent/resources/panel-react/src", "panel fixtures and tests that render device rows"],
];

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
    "\nEither take the hostname out of the file, or add its path to ALLOWED in this script WITH A REASON. " +
      "The list is a debt with owners, not a permission: it may only shrink.",
  );
  process.exit(1);
}
console.log("production-host: no undeclared file names one");
