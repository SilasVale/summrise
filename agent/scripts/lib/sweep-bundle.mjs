// sweep-bundle — assemble ONE runnable script out of REAL modules, and prove it parses before returning it.
//
// WHY THIS EXISTS (round 266). The five sweep emitters build their device-side script by putting the whole program
// inside a TEMPLATE LITERAL in their own source: `${focusPass.toString()}`, `${JSON.stringify(PROBE_SOURCE)}`,
// `${pageChecks("body")}`. That makes the emitted text a SECOND GRAMMAR — the program after one level of escaping —
// so every writer has to know which strings may contain a backtick (` it ends the host file mid-parse: 53 recorded
// incidents), which pieces survive as JSON and which are spliced as code, and which borrowed helper must also be
// embedded. Six modules exist only to compensate: four copies of a `new Function` parse guard, `assertEmbedded`, a
// bash program that walks the emitted text counting backslash runs, and a pre-commit hook.
//
// THE FIX IS NOT A BUNDLER, IT IS A FILE. A payload that lives in its own module is ordinary code: comments may
// contain anything, an import is resolved at bundle time instead of being hand-listed, and nothing is escaped
// because nothing is embedded inside a string. This module is the small, dependency-free assembler for OUR tree
// (a few dozen files, literal relative requires, no node_modules): it follows `require("./x.cjs")`, wraps each
// module in a function, emits a 20-line loader plus the `require`-calls Node itself answers, and — the part the
// landing emitter never had — COMPILES what it is about to print.
//
// IT DELIBERATELY DOES NOT USE esbuild. esbuild is present in agent/resources/panel-react/node_modules as a
// transitive dependency of vite, but `--emit` must work where the emitters run TODAY: on the device, under its
// bundled node, with no node_modules at all (that is the round-228 recipe). A bundler that only exists on CI would
// trade one broken environment for another.

/** Resolve `spec` (relative only) against the module that asked, or throw naming both. */
function resolveRelative(fromId, spec, has) {
  const base = fromId.slice(0, fromId.lastIndexOf("/") + 1);
  const joined = normalize(base + spec);
  for (const candidate of [joined, joined + ".cjs", joined + ".js", joined + ".json"]) {
    if (has(candidate)) return candidate;
  }
  throw new Error(
    `sweep-bundle: ${fromId} requires "${spec}" and no such module is in the payload ` +
      `(tried ${[joined, joined + ".cjs", joined + ".js"].join(", ")}) — a relative require inside a bundled ` +
      `payload must name a file that is part of it`,
  );
}

/** Collapse `.` and `..` segments so two spellings of one file are one module id. */
function normalize(p) {
  const out = [];
  for (const part of p.split("/")) {
    if (part === "." || part === "") continue;
    if (part === "..") out.pop();
    else out.push(part);
  }
  return out.join("/");
}

/**
 * Bundle a CommonJS payload into one self-contained script.
 *
 * THE TARGET MATTERS (round 269). Four emitters produce a script that NODE runs on the device, and the harness emitter
 * produces a `<script>` that a BROWSER runs inside the harness page — where `require` does not exist. A loader that
 * captured `const __nativeRequire = require` would throw on load there, before the fixture had stubbed anything, so the
 * target decides the preamble and whether a non-relative require is even allowed (in a browser payload it cannot be).
 *
 * @param {object} opts
 * @param {Record<string,string>} opts.modules  module id (path relative to the payload root) → source
 * @param {string} opts.entry                   the module the script should run, e.g. "landing-run.cjs"
 * @param {"node"|"browser"} [opts.target]      where the assembled script runs (default "node")
 * @returns {{ code: string, ids: string[] }}
 */
export function bundleSweep({ modules, entry, target = "node" }) {
  if (target !== "node" && target !== "browser") throw new Error(`sweep-bundle: unknown target "${target}"`);
  const has = (id) => Object.prototype.hasOwnProperty.call(modules, id);
  if (!has(entry)) throw new Error(`sweep-bundle: entry "${entry}" is not one of the payload modules`);

  // DEPENDENCIES FIRST, and a cycle is an ERROR rather than a silent half-order: a payload that requires itself
  // cannot be assembled into functions in any order, and finding that out on the device (as "x is not defined")
  // is exactly the class of failure this module exists to move to bundle time.
  const order = [];
  const state = new Map(); // id -> "visiting" | "done"
  const stack = [];
  const visit = (id) => {
    if (state.get(id) === "done") return;
    if (target === "browser") {
      // A BROWSER PAYLOAD CANNOT REQUIRE A BUILTIN, and finding that out at load time (inside a page whose fixture is
      // half-installed) is strictly worse than finding it here. Only literal relative requires are followed, so this is
      // the same scan the bundler already does — the target only decides what a failure means.
      for (const spec of requires(modules[id])) {
        if (!spec.startsWith(".")) throw new Error(`sweep-bundle: ${id} requires "${spec}" and the target is a browser, where there is no require`);
      }
    }
    if (state.get(id) === "visiting") {
      throw new Error(`sweep-bundle: require cycle — ${[...stack, id].join(" → ")}`);
    }
    state.set(id, "visiting");
    stack.push(id);
    for (const spec of relativeRequires(modules[id])) visit(resolveRelative(id, spec, has));
    stack.pop();
    state.set(id, "done");
    order.push(id);
  };
  visit(entry);

  const parts = [];
  parts.push("// generated by agent/scripts/lib/sweep-bundle.mjs — assembled from real modules; do not edit.");
  parts.push("// The payload's own module files are the source of truth; this file is a build product of --emit.");
  parts.push('"use strict";');
  if (target === "node") parts.push("const __nativeRequire = require;");
  parts.push("const __factories = {};");
  parts.push("const __exports = {};");
  parts.push("function __require(id) {");
  parts.push("  if (id in __exports) return __exports[id];");
  parts.push(
    target === "node"
      ? "  if (!(id in __factories)) return __nativeRequire(id);"
      : // A PAGE HAS NOTHING TO FALL BACK TO: an id that is not in the payload is a bundling bug, and saying so beats
        // "require is not defined" from inside a fixture that is half-installed.
        "  if (!(id in __factories)) throw new Error('the page asked for ' + id + ', which this payload does not carry');",
  );
  parts.push("  const module = { exports: {} };");
  parts.push("  __exports[id] = module.exports;");
  // THE LOOKUP IS NOT THE CALL. This line returned `__map[id][spec]` — a module ID STRING — so every relative
  // require in a payload handed back a string where an object was expected, and the failure surfaced far away as
  // "Cannot read properties of undefined (reading 'root')". It was found by RUNNING the bundle under a stub
  // browser, not by comparing the payload's pieces with the old emitter's (those were byte-identical): a seam is
  // proved by executing it. scripts/test/sweep-bundle-check.mjs now bundles a three-module fixture and RUNS it.
  parts.push(
    target === "node"
      ? "  const local = (spec) => (spec.startsWith('.') ? __require(__map[id][spec]) : __nativeRequire(spec));"
      : // unreachable for a non-relative spec: the bundle refused it. It stays a throw so a computed spec is loud.
        "  const local = (spec) => { if (!spec.startsWith('.')) throw new Error('a browser payload cannot require ' + spec); return __require(__map[id][spec]); };",
  );
  parts.push("  __factories[id](module, module.exports, local);");
  parts.push("  __exports[id] = module.exports;");
  parts.push("  return module.exports;");
  parts.push("}");
  parts.push("const __map = {};");
  for (const id of order) {
    const map = {};
    for (const spec of relativeRequires(modules[id])) map[spec] = resolveRelative(id, spec, has);
    parts.push(`__map[${JSON.stringify(id)}] = ${JSON.stringify(map)};`);
  }
  for (const id of order) {
    parts.push(`__factories[${JSON.stringify(id)}] = function (module, exports, require) {`);
    parts.push(modules[id]);
    parts.push("};");
  }
  parts.push(`__require(${JSON.stringify(entry)});`);

  const body = parts.join("\n") + "\n";
  // A PAGE HAS ONE GLOBAL SCOPE: the loader's own names must not land in it, so a browser bundle is wrapped.
  const code = target === "browser" ? `(function () {\n${body}})();\n` : body;
  assertParses(code);
  return { code, ids: order };
}

/** Every LITERAL relative require in a module's source. Computed specifiers are not followed — and that is the
 *  point: a payload whose dependencies cannot be seen at bundle time cannot be assembled honestly, so the
 *  convention is literal relative requires, which the check in scripts/test/sweep-bundle-check.mjs pins. */
function requires(src) {
  const out = [];
  const re = /\brequire\(\s*["']([^"']*)["']\s*\)/g;
  let m;
  while ((m = re.exec(src))) out.push(m[1]);
  return out;
}

function relativeRequires(src) {
  const out = [];
  const re = /\brequire\(\s*["'](\.[^"']*)["']\s*\)/g;
  let m;
  while ((m = re.exec(src))) out.push(m[1]);
  return out;
}

/** Compile without running. An emitter that prints a script nobody can parse is the failure this makes
 *  impossible — and it is the guard `landing-design-sweep.mjs` was missing while the other four emitters had it. */
export function assertParses(code) {
  try {
    // eslint-disable-next-line no-new-func
    new Function(code);
  } catch (e) {
    const line = /<anonymous>:(\d+)/.exec(String(e.stack || ""))?.[1];
    const at = line ? `\n  at emitted line ${line}: ${code.split("\n")[Number(line) - 2] ?? ""}` : "";
    throw new Error(`sweep-bundle: the assembled script does not parse — ${e.message}${at}`);
  }
  return true;
}
