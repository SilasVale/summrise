#!/usr/bin/env node
// The architecture gate: workspace dependencies point one way.
//
// The architecture of record (docs/design/architecture-of-record-2026-11-12.md, I8) says
// `core <- bus <- {device, console} <- surfaces`: `core` names no crate above it, `bus` names only
// `core`, and no surface imports another surface. Until this file existed, nothing checked it.
//
// `cargo metadata --no-deps` is the source of truth. Every `packages[].dependencies` entry whose
// `name` is another workspace package becomes one edge; dev-, build- and target-specific
// dependencies count too, because a test that links an upper layer is still an edge in the tree.
// The table below is the complete allow-list: an edge not in it is the failure this gate exists for.
//
//   node scripts/check-architecture.mjs
//   node scripts/check-architecture.mjs --metadata path/to/metadata.json
//   node scripts/check-architecture.mjs --self-test
//
// Exit 0: every edge is in the table and every crate has a rule.
// Exit 1: an upward edge, a crate with no rule, or a rule that names a crate that is gone.
// Exit 2: the graph could not be read (cargo missing, bad JSON, bad arguments) — not a verdict.
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const MANIFEST = "device/Cargo.toml";

// ─── THE LAYERING TABLE ──────────────────────────────────────────────────────────────────────────
// One row per workspace package. `mayUse` is the complete set of workspace packages that crate may
// name in any Cargo dependency table, on any target. A string allows the edge everywhere; an object
// with `kinds` narrows it ("normal", "dev", "build"), which is how a test-only edge stays test-only.
//
// READ FROM THE MANIFESTS ON 2026-11-12, and that is why this is not exactly the sketch in the
// architecture doc. `device` already names `ui`, `mesh` and `release`, and `console` names `ui` and
// `mesh`; the doc's `core <- bus <- {device, console} <- surfaces` does not show those lower leaves.
// `device` and `release` also dev-depend on `launch` (both parse a value with the launcher's own
// type in tests); those permissions are dev-only so a shipped dependency would fail.
//
// The one exception the brief states — `launch` may depend on `device` — is encoded even though no
// manifest declares it today: `launch`'s workspace dependencies are `core` and `winsec`, and the only
// edge between the two runs the other way (`device` -> `launch`, dev-only). It is left visible here
// rather than silently inferred. No entry allows `summrise-console`: nothing may depend on the console.
const RULES = {
  // The kernel: decisions as values, no I/O, and no crate above it.
  "summrise-core": { mayUse: [] },

  // The journal/dispatch layer: core only. It has no workspace dependency today; the permission is
  // the target's, so the bus can take core without touching this table.
  "summrise-bus": { mayUse: ["summrise-core"] },

  // The lower leaves. They name no workspace crate; an edge out of one is an inversion.
  "summrise-winsec": { mayUse: [] },
  "summrise-winsys": { mayUse: [] },
  "summrise-ui": { mayUse: [] },

  // Vendored, and a member only because Cargo auto-includes a path dependency under the workspace
  // root. It must stay a leaf: the app depends on the PTY, never the PTY on the app.
  "portable-pty": { mayUse: [] },

  // The transport and the signed release document.
  "summrise-mesh": { mayUse: ["summrise-core"] },
  "summrise-release": {
    mayUse: ["summrise-core", { name: "summrise-launch", kinds: ["dev"] }]
  },

  // The supervisor. It names no surface: a supervisor that imports the app it starts is the upward
  // edge this gate exists to forbid. It runs the device by path and argv, not by type.
  "summrise-launch": {
    mayUse: ["summrise-core", "summrise-winsec"]
  },

  // The app. Its own manifest declares all of these: the bus and core below it, the shared tokens,
  // the mesh identity, the release document, the OS boundary, the vendored PTY, and `launch` for
  // the sidecar test only.
  "summrise-device": {
    mayUse: [
      "summrise-bus",
      "summrise-core",
      "summrise-mesh",
      "summrise-release",
      "summrise-ui",
      "summrise-winsec",
      "summrise-winsys",
      "portable-pty",
      { name: "summrise-launch", kinds: ["dev"] }
    ]
  },

  // The console: core and bus (the control plane's target), the OS boundary, the page's shared
  // shell, the mesh readiness probe, and the page crate it implements the reads for. The manifest
  // names core, mesh and ui today; bus, winsec and winsys are the brief's permissions and have no
  // edge yet.
  "summrise-console": {
    mayUse: [
      "summrise-bus",
      "summrise-console-page",
      "summrise-core",
      "summrise-mesh",
      "summrise-ui",
      "summrise-winsec",
      "summrise-winsys"
    ]
  },

  // The page role: the built assets, the icon and the live stream, handed only the reads its
  // handler needs. It names no other workspace crate; that is the boundary the split exists for.
  "summrise-console-page": { mayUse: [] },

  // The desktop shell: the OS boundary only. It is a surface, not a library the app links.
  "summrise-shell": { mayUse: ["summrise-winsys"] }
};

const USAGE = `usage: node scripts/check-architecture.mjs [--metadata <cargo-metadata.json>] [--self-test]`;
const TABLES = { normal: "dependencies", dev: "dev-dependencies", build: "build-dependencies" };
const KINDS = new Set(Object.keys(TABLES));

// ─── the checker ─────────────────────────────────────────────────────────────────────────────────

// Compiles the table once. A typo in the table is a checker error (exit 2), not a graph verdict.
function compileRules(table) {
  const rules = new Map();
  for (const [crate, rule] of Object.entries(table)) {
    if (!rule || !Array.isArray(rule.mayUse)) {
      throw new Error(`RULES[${JSON.stringify(crate)}] needs a mayUse array`);
    }
    const allow = new Map();
    for (const entry of rule.mayUse) {
      const spec = typeof entry === "string" ? { name: entry } : entry;
      if (!spec || typeof spec.name !== "string" || spec.name.length === 0) {
        throw new Error(`RULES[${crate}] has a permission with no name`);
      }
      if (allow.has(spec.name)) {
        throw new Error(`RULES[${crate}] lists ${spec.name} twice`);
      }
      let kinds = null;
      if (spec.kinds !== undefined) {
        if (!Array.isArray(spec.kinds) || spec.kinds.length === 0) {
          throw new Error(`RULES[${crate} -> ${spec.name}] needs a non-empty kinds array`);
        }
        kinds = new Set(spec.kinds);
        for (const kind of kinds) {
          if (!KINDS.has(kind)) {
            throw new Error(`RULES[${crate} -> ${spec.name}] has unknown kind ${JSON.stringify(kind)}`);
          }
        }
      }
      allow.set(spec.name, { kinds });
    }
    rules.set(crate, { allow });
  }
  return rules;
}

// A workspace package -> package edge. `kind` is Cargo's (`null` is a normal dependency); `target`
// is the `cfg(...)` it sits behind, if any.
function edgeOf(from, dep, manifestPath) {
  return {
    from,
    to: dep.name,
    kind: dep.kind ?? "normal",
    target: dep.target ?? null,
    manifestPath
  };
}

// `cargo metadata --no-deps` output, reduced to the graph. `workspace_members` is authoritative when
// present: a path dependency outside the workspace (none today) must not become a node or an edge.
function graphFromMetadata(metadata) {
  if (!metadata || typeof metadata !== "object" || !Array.isArray(metadata.packages)) {
    throw new Error("the metadata has no packages[] array (is this cargo metadata --format-version 1?)");
  }
  const byId = new Map();
  for (const pkg of metadata.packages) {
    if (!pkg || typeof pkg.id !== "string" || typeof pkg.name !== "string") {
      throw new Error("packages[] has an entry without a string id and name");
    }
    if (byId.has(pkg.id)) throw new Error(`packages[] names ${pkg.id} twice`);
    byId.set(pkg.id, pkg);
  }
  const memberIds =
    Array.isArray(metadata.workspace_members) && metadata.workspace_members.length > 0
      ? metadata.workspace_members
      : metadata.packages.map((pkg) => pkg.id);
  const missing = memberIds.filter((id) => !byId.has(id));
  if (missing.length > 0) {
    throw new Error(`workspace_members names ${missing.length} package(s) absent from packages[]: ${missing[0]}`);
  }
  const members = memberIds.map((id) => byId.get(id));
  const packages = new Set(members.map((pkg) => pkg.name));
  const edges = [];
  for (const pkg of members) {
    for (const dep of Array.isArray(pkg.dependencies) ? pkg.dependencies : []) {
      if (!dep || typeof dep.name !== "string" || !packages.has(dep.name)) continue;
      // A registry dependency that happens to share a workspace crate's name is not that crate.
      if (dep.source != null) continue;
      edges.push(edgeOf(pkg.name, dep, displayPath(pkg.manifest_path)));
    }
  }
  return { packages, edges };
}

function displayPath(path) {
  if (typeof path !== "string" || path.length === 0) return null;
  const rel = relative(ROOT, path);
  return rel.length > 0 && !rel.startsWith("..") && !isAbsolute(rel) ? rel : path;
}

// The verdict. Pure: the self-test calls this on an in-memory graph, and the real run calls it on
// cargo's answer. It returns violations; it does not exit.
function checkGraph(graph, rules) {
  const violations = [];

  for (const crate of [...graph.packages].sort()) {
    if (!rules.has(crate)) violations.push({ type: "no-rule", crate });
  }
  for (const [crate, rule] of rules) {
    if (!graph.packages.has(crate)) {
      violations.push({ type: "stale-rule", crate });
      continue;
    }
    for (const [allowed, permission] of rule.allow) {
      if (!graph.packages.has(allowed)) {
        violations.push({ type: "stale-permission", crate, allowed, kinds: permission.kinds });
      }
    }
  }

  const seen = new Set();
  for (const edge of graph.edges) {
    const rule = rules.get(edge.from);
    // A crate with no rule is already one violation; its edges would only repeat that.
    if (!rule) continue;
    const key = `${edge.from}\0${edge.to}\0${edge.kind}\0${edge.target ?? ""}`;
    if (seen.has(key)) continue;
    seen.add(key);
    const permission = rule.allow.get(edge.to);
    if (!permission) {
      violations.push({
        type: "edge",
        ...edge,
        allowed: [...rule.allow.entries()].map(([name, grant]) => ({ name, kinds: grant.kinds }))
      });
    } else if (permission.kinds && !permission.kinds.has(edge.kind)) {
      violations.push({ type: "kind", ...edge, allowedKinds: permission.kinds });
    }
  }

  const order = { "no-rule": 0, "stale-rule": 1, "stale-permission": 2, kind: 3, edge: 4 };
  return violations.sort(
    (a, b) =>
      order[a.type] - order[b.type] ||
      String(a.crate ?? a.from).localeCompare(String(b.crate ?? b.from)) ||
      String(a.allowed ?? a.to).localeCompare(String(b.allowed ?? b.to))
  );
}

const exitCodeFor = (violations) => (violations.length === 0 ? 0 : 1);

function allowedText(entries) {
  if (entries.length === 0) return "no workspace crate";
  return entries
    .map(({ name, kinds }) => (kinds ? `\`${name}\` (${kindText(kinds)} only)` : `\`${name}\``))
    .join(", ");
}

function kindText(kinds) {
  return [...kinds].sort().map((kind) => `[${TABLES[kind]}]`).join(" or ");
}

function describe(violation) {
  if (violation.type === "no-rule") {
    return [
      `  ${violation.crate} has no rule in the layering table`,
      `      fix: add an entry to RULES at the top of scripts/check-architecture.mjs; a new crate`,
      `           is placed in a layer, never exempted from one.`
    ];
  }
  if (violation.type === "stale-rule") {
    return [
      `  RULES names ${violation.crate}, but the workspace has no package with that name`,
      `      fix: delete the entry, or restore the crate.`
    ];
  }
  if (violation.type === "stale-permission") {
    return [
      `  ${violation.crate} allows ${violation.allowed}, but the workspace has no package with that name`,
      `      fix: correct the name in RULES, or delete the permission.`
    ];
  }

  const where = violation.manifestPath ? `; ${violation.manifestPath}` : "";
  const target = violation.target ? `, target ${violation.target}` : "";
  const header = `  ${violation.from} -> ${violation.to}  ([${TABLES[violation.kind]}]${target}${where})`;
  if (violation.type === "kind") {
    return [
      header,
      `      ${violation.from} may depend on ${violation.to} only as ${kindText(violation.allowedKinds)}.`,
      `      fix: move the dependency to the matching Cargo.toml table, or widen the rule.`
    ];
  }
  return [
    header,
    `      ${violation.from} may not depend on ${violation.to}.`,
    `      allowed for ${violation.from}: ${allowedText(violation.allowed)}`,
    `      fix: remove the dependency, or add the edge to RULES with the architecture change that allows it.`
  ];
}

function printViolations(violations) {
  console.error(
    `architecture: ${violations.length} violation${violations.length === 1 ? "" : "s"} in the workspace dependency graph`
  );
  console.error("");
  for (const violation of violations) {
    for (const line of describe(violation)) console.error(line);
    console.error("");
  }
  console.error("architecture: FAILED — dependencies do not point one way");
}

// ─── the two graph sources ───────────────────────────────────────────────────────────────────────

function cargoCandidates() {
  const exe = process.platform === "win32" ? "cargo.exe" : "cargo";
  if (process.env.CARGO) return [process.env.CARGO];
  // The caller's PATH first, then the conventional install, because a shell that did not source
  // ~/.cargo/env still has a working toolchain and this gate should not fail on PATH alone.
  const cargoHome = process.env.CARGO_HOME || join(homedir(), ".cargo");
  const fallback = join(cargoHome, "bin", exe);
  return fallback === exe ? [exe] : [exe, fallback];
}

function graphFromCargo() {
  const args = ["metadata", "--manifest-path", MANIFEST, "--no-deps", "--format-version", "1"];
  const tried = [];
  for (const cargo of cargoCandidates()) {
    tried.push(cargo);
    const result = spawnSync(cargo, args, {
      cwd: ROOT,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024
    });
    if (result.error) {
      if (result.error.code === "ENOENT") continue;
      throw new Error(`cargo (${cargo}) could not be started: ${result.error.message}`);
    }
    if (result.status !== 0) {
      const stderr = (result.stderr || "").trim();
      const how = result.status === null ? `signal ${result.signal}` : `exit ${result.status}`;
      throw new Error(`cargo metadata failed (${how})${stderr ? `:\n${stderr}` : ""}`);
    }
    let metadata;
    try {
      metadata = JSON.parse(result.stdout);
    } catch (error) {
      throw new Error(`cargo metadata printed invalid JSON: ${error.message}`);
    }
    return graphFromMetadata(metadata);
  }
  throw new Error(`cargo was not found (tried ${tried.join(", ")}); put it on PATH or set CARGO`);
}

function graphFromFile(path) {
  let text;
  try {
    text = readFileSync(resolve(path), "utf8");
  } catch (error) {
    throw new Error(`cannot read ${path}: ${error.message}`);
  }
  let metadata;
  try {
    metadata = JSON.parse(text);
  } catch (error) {
    throw new Error(`${path} is not JSON: ${error.message}`);
  }
  return graphFromMetadata(metadata);
}

// ─── self-test ───────────────────────────────────────────────────────────────────────────────────
// An in-memory graph only: no cargo, no file, no temp directory. It first accepts a clean graph built
// from the table's own permissions, then adds one deliberate upward edge (`core` -> `bus`) and
// asserts the same checker rejects it with exit code 1.
function runSelfTest(rules) {
  const packages = new Set(rules.keys());
  const edges = [
    ["summrise-bus", "summrise-core"],
    ["summrise-mesh", "summrise-core"],
    ["summrise-release", "summrise-core"],
    ["summrise-release", "summrise-launch", "dev"],
    ["summrise-launch", "summrise-core"],
    ["summrise-launch", "summrise-winsec"],
    ["summrise-device", "summrise-bus"],
    ["summrise-device", "summrise-core"],
    ["summrise-device", "summrise-mesh"],
    ["summrise-device", "summrise-release"],
    ["summrise-device", "summrise-ui"],
    ["summrise-device", "summrise-winsec"],
    ["summrise-device", "summrise-winsys"],
    ["summrise-device", "portable-pty"],
    ["summrise-device", "summrise-launch", "dev"],
    ["summrise-console", "summrise-core"],
    ["summrise-console", "summrise-console-page"],
    ["summrise-console", "summrise-mesh"],
    ["summrise-console", "summrise-ui"],
    ["summrise-shell", "summrise-winsys"]
  ].map(([from, to, kind = "normal"]) => ({ from, to, kind, target: null, manifestPath: null }));

  const clean = checkGraph({ packages, edges }, rules);
  if (exitCodeFor(clean) !== 0) {
    console.error("self-test: FAILED — the clean graph was rejected:");
    for (const violation of clean) for (const line of describe(violation)) console.error(line);
    return 1;
  }

  const upward = { from: "summrise-core", to: "summrise-bus", kind: "normal", target: null, manifestPath: null };
  const dirty = checkGraph({ packages, edges: [...edges, upward] }, rules);
  if (exitCodeFor(dirty) !== 1) {
    console.error(`self-test: FAILED — one upward edge produced exit code ${exitCodeFor(dirty)}, expected 1`);
    return 1;
  }
  if (!dirty.some((violation) => violation.type === "edge" && violation.from === "summrise-core" && violation.to === "summrise-bus")) {
    console.error("self-test: FAILED — the rejection did not name core -> bus");
    return 1;
  }

  console.log("self-test: ok — clean graph accepted (exit 0); core -> bus rejected (exit 1)");
  return 0;
}

// ─── the command ────────────────────────────────────────────────────────────────────────────────

function parseArgs(argv) {
  let metadata = null;
  let selfTest = false;
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--self-test") {
      selfTest = true;
    } else if (arg === "--metadata") {
      const value = argv[index + 1];
      if (value === undefined) throw new Error(`--metadata needs a path\n${USAGE}`);
      metadata = value;
      index += 1;
    } else if (arg.startsWith("--metadata=")) {
      metadata = arg.slice("--metadata=".length);
      if (metadata.length === 0) throw new Error(`--metadata needs a path\n${USAGE}`);
    } else {
      throw new Error(`unknown argument ${JSON.stringify(arg)}\n${USAGE}`);
    }
  }
  if (selfTest && metadata) throw new Error(`--self-test and --metadata are alternatives\n${USAGE}`);
  return { metadata, selfTest };
}

function main(argv) {
  try {
    if (argv.includes("--help") || argv.includes("-h")) {
      console.log(USAGE);
      return 0;
    }
    const options = parseArgs(argv);
    const rules = compileRules(RULES);
    if (options.selfTest) return runSelfTest(rules);

    const graph = options.metadata ? graphFromFile(options.metadata) : graphFromCargo();
    const violations = checkGraph(graph, rules);
    if (violations.length > 0) {
      printViolations(violations);
      return 1;
    }
    const pairs = new Set(graph.edges.map((edge) => `${edge.from}\0${edge.to}`));
    console.log(
      `architecture: ok — ${graph.packages.size} workspace packages, ${pairs.size} workspace dependency edges, 0 upward`
    );
    return 0;
  } catch (error) {
    console.error(`architecture: ${error.message}`);
    return 2;
  }
}

process.exitCode = main(process.argv.slice(2));
