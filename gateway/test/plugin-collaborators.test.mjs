// ── registry.ts's DIRECTORY CONTRACT is a CLAIM, and it has been wrong twice ──
//
// The header lists plugins/'s EXCLUSIVE collaborator modules by name — "device-
// proxy.ts (devices only), translate-vision.ts + model-route.ts (translate only),
// models-probe.ts (admin only)" — and states the rule that makes the list
// checkable: a module with two live consumers is NOT a private collaborator, it
// belongs in src/ as foundation.
//
// The file then records its own history: round-184 found models-probe.ts MISSING
// from that list while satisfying the rule, and notes "this is the second time in
// this log that the claim, not the code, was the thing out of date". A rule that
// mechanical plus a list that is hand-written is exactly the pair this loop keeps
// finding: the rule can DERIVE the list, so the list does not have to be trusted.
//
// This derives it. The plugin set comes from index.ts's own imports (that is what
// registers a plugin), so neither the rule nor the list is restated here.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const ROOT = fileURLToPath(new URL("..", import.meta.url));

const read = (rel) => readFileSync(join(ROOT, rel), "utf8");

/** The plugins index.ts imports — its imports ARE the registration. */
function pluginSet() {
  const src = read("src/index.ts");
  return new Set(
    [...src.matchAll(/from "\.\/plugins\/([a-z-]+)\.ts"/g)].map((m) => m[1]),
  );
}

/** `plugins/*.ts` minus the framework minus the registered plugins. */
function collaboratorFiles() {
  const plugins = pluginSet();
  return readdirSync(join(ROOT, "src", "plugins"))
    .filter((f) => f.endsWith(".ts"))
    .map((f) => f.replace(/\.ts$/, ""))
    .filter((f) => f !== "registry" && !plugins.has(f))
    .sort();
}

/** How many files import `plugins/<name>.ts` (relative or rooted). */
function consumers(name, self) {
  const src = join(ROOT, "src");
  const out = [];
  const walk = (dir) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, e.name);
      if (e.isDirectory()) { walk(p); continue; }
      if (!e.name.endsWith(".ts")) continue;
      if (p === self) continue;
      const t = readFileSync(p, "utf8");
      if (
        t.includes(`from "./${name}.ts"`) ||
        t.includes(`from "./plugins/${name}.ts"`) ||
        t.includes(`from "../plugins/${name}.ts"`)
      ) out.push(p);
    }
  };
  walk(src);
  return out;
}

/** The names in registry.ts's DIRECTORY CONTRACT, read as data. */
function listedCollaborators() {
  const header = read("src/plugins/registry.ts").split("*/")[0];
  //  rather than : a name at the end of a comment line is followed by
  // the continuation's  before the  — which is exactly how this parser missed
  // models-probe.ts on its first run. Parsing prose is where these instruments break.
  return [...header.matchAll(/\b([a-z][a-z-]*)\.ts\b(?=[\s*]*[+(])/g)]
    .map((m) => m[1])
    .sort();
}

test("registry: the DIRECTORY CONTRACT lists every collaborator and nothing else", () => {
  const derived = collaboratorFiles();
  const listed = listedCollaborators();
  assert.deepEqual(
    listed,
    derived,
    "src/plugins/registry.ts's DIRECTORY CONTRACT no longer names exactly the modules " +
      "that are collaborators by its OWN rule (in plugins/, not the framework, not a " +
      "registered plugin). Round-184 found models-probe.ts missing from this same list " +
      "while satisfying the rule — the list is a claim, and this is the assertion that " +
      "makes it checkable. Add the missing name (or move the module to src/ if it has " +
      "two consumers).",
  );
});

test("registry: every collaborator has exactly one consumer, every plugin at least one", () => {
  for (const name of collaboratorFiles()) {
    const n = consumers(name, join(ROOT, "src", "plugins", `${name}.ts`)).length;
    assert.equal(
      n,
      1,
      `plugins/${name}.ts has ${n} consumers. The contract's rule: "A module with two ` +
        `live consumers is NOT a private collaborator: it belongs in src/ as foundation." ` +
        `Zero consumers means it is dead and should be deleted.`,
    );
  }
  for (const name of pluginSet()) {
    const n = consumers(name, join(ROOT, "src", "plugins", `${name}.ts`)).length;
    assert.ok(
      n >= 1,
      `plugins/${name}.ts is registered by index.ts but nothing else imports it — it is ` +
        `listed as a plugin while being unreachable`,
    );
  }
});

test("ARCHITECTURE.md's plugins/ row accounts for every file in the directory", () => {
  // The Directory contracts row enumerates `plugins/`, so it is the SECOND place
  // that lists the same set — and round 249 found it disagreeing with the derived
  // one: it said "9 route plugins", listed `device-proxy`, `model-route` and
  // `registry` among them, and OMITTED `models-probe` entirely, which is the same
  // omission round 184 found in registry.ts's header. Round 217's test covers the
  // header; nothing covered the row. Both are read as data here.
  const row = read("../docs/ARCHITECTURE.md")
    .split("\n")
    .find((l) => l.startsWith("| `gateway/src/plugins/`"));
  assert.ok(row, "ARCHITECTURE.md still has a `gateway/src/plugins/` contract row");

  // THE CONTRACT IS THE ENUMERATION, NOT THE WHOLE ROW — and this distinction is the
  // whole test. The first version asked "does the row mention X?" and COULD NOT FAIL:
  // the row's own correction note says "while OMITTING `models-probe`", so deleting
  // `models-probe` from the LIST left the word present in the PROSE about the defect
  // and the check stayed green. That is round 215's exact instrument defect ("a
  // landed-check grepping a string still present in a comment"), reproduced here on
  // the first attempt. So: strip the note, then read the two parenthesised lists.
  const contract = row.replace(/\*\*Corrected round \d+:.*?\*\*/s, "");
  // `registry` is legitimately named OUTSIDE the parentheses ("the **registry framework**"),
  // so the check is against the whole note-stripped cell rather than against the two lists —
  // which is only safe BECAUSE the note is stripped: measured, removing `models-probe` from
  // the collaborator list leaves the name nowhere in `contract`.
  assert.ok(contract.includes("route plugins"), `the row's contract did not parse: ${contract}`);

  const onDisk = [...new Set([...collaboratorFiles(), ...pluginSet(), "registry"])].sort();
  const missing = onDisk.filter((f) => !new RegExp(`(^|[^a-z-])${f}([^a-z-]|$)`).test(contract));
  assert.deepEqual(
    missing,
    [],
    `ARCHITECTURE.md's plugins/ row does not account for: ${missing.join(", ")}. The row ` +
      `enumerates that directory, and plugins/ holds ${onDisk.length} .ts files \u2014 every one ` +
      `of them must appear in the row's plugin list or its collaborator list. (This is round ` +
      `184's defect in a second artifact: round 217's test derives the set for registry.ts's ` +
      `header, and nothing read this row.)`,
  );
});


