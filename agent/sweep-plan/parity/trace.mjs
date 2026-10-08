// Run ONE emitted sweep bundle against the stub browser and return the plan-shaped trace of what it
// asked the browser to do. This is the JavaScript side of the parity harness — the PRE-CHANGE payload,
// executed, rather than a second reading of its source.
//
//   node trace.mjs <tree> <tool> <passes|->  →  { tool, passes, trace }
//
// `<tree>` is a checkout of the pre-change commit (compare.mjs makes a worktree of it), `<passes>` is
// `-` for "the flag was absent".
import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const nativeRequire = createRequire(import.meta.url);
const stubPath = nativeRequire.resolve("./stub-browser.cjs");

/** One case, as a value rather than as printed JSON — compare.mjs calls this directly. */
export function trace(tree, tool, passes, workdir) {
  // `null` is "the flag was absent" and `""` is "the flag was given empty" — different inputs, so they
  // get different work directories rather than the same one by accident.
  const slug = passes === null ? "absent" : passes === "" ? "empty" : passes.replace(/[^\w,]/g, "_");
  const tmp = join(workdir, `${tool}-${slug}`);
  mkdirSync(tmp, { recursive: true });
  // The three payloads read a fixture path or directory; the plan does not depend on its CONTENTS, only
  // on it existing (a missing harness file aborts the run before the trace is complete).
  writeFileSync(join(tmp, "panel-harness.html"), '<meta name="summrise-harness-build" content="0-stub">');
  writeFileSync(join(tmp, "index.html"), "<html></html>");
  writeFileSync(join(tmp, "installer.html"), "<html></html>");
  writeFileSync(join(tmp, "npm-only.html"), "<html></html>");

  const emitter = join(tree, "agent", "scripts", `${tool}-design-sweep.mjs`);
  const args = [emitter, "--emit"];
  if (passes !== null) args.push(`--passes=${passes}`);
  const code = execFileSync(process.execPath, args, {
    encoding: "utf8",
    env: { ...process.env, SUMMRISE_LANDING_OUT: tmp },
    maxBuffer: 64 * 1024 * 1024,
  });

  process.env.SUMMRISE_BROWSER_HELPER = stubPath;
  process.env.SUMMRISE_PANEL_HARNESS = join(tmp, "panel-harness.html");
  process.env.SUMMRISE_SWEEP_ROOT = tmp;
  process.env.SUMMRISE_SWEEP_REPORT = join(tmp, "report.json");

  const stub = nativeRequire(stubPath);
  // A FRESH RECORDING per case: the harness runs every case in one process, and a shared trace would
  // splice one case's events into the next.
  stub.reset();
  // THE PAYLOAD IS THE DEVICE'S PROGRAM: CommonJS, `require` for its builtins, and an async IIFE that
  // the caller does not await — which is why the stub's `close()` is the completion signal.
  // eslint-disable-next-line no-new-func
  // `process.exit` is REPLACED, not passed through: the payload's own catch calls it on a fatal error,
  // and a harness that silently died on case 4 of 90 would report a partial run as a comparison.
  const fakeProcess = {
    ...process,
    exit(code) { throw new Error(`the emitted payload exited ${code}`); },
  };
  new Function(
    "require", "module", "exports", "__dirname", "__filename", "process", "console", "Buffer",
    "setTimeout", "clearTimeout", code,
  )(
    (spec) => (spec === stubPath ? stub : nativeRequire(spec)),
    { exports: {} }, {}, tmp, join(tmp, "emitted.js"), fakeProcess,
    { log() {}, error() {} }, Buffer, setTimeout, clearTimeout,
  );

  return { stub, tmp, code };
}

/** Wait for the emitted program to call `close()`, or give up loudly. */
export async function awaitTrace(stub, ms = 60000) {
  const session = stub.state;
  let timer;
  const timeout = new Promise((r) => { timer = setTimeout(r, ms); });
  const closed = await Promise.race([session.finished.then(() => true), timeout.then(() => false)]);
  clearTimeout(timer);
  if (!closed) {
    throw new Error(
      `the emitted payload never finished: ${session.trace.length} browser call(s) recorded, then it ` +
        `stopped — a truncated trace compared against a plan would pass for the wrong reason`,
    );
  }
  return planEvents(session.trace);
}

/**
 * THE PLAN-SHAPED PART OF A TRACE: navigate, resize, emulate media, set the SPA hash, reload.
 *
 * ONE normalization, and it is about naming rather than about tolerance: a `hash` whose argument is a
 * `[theme, hash]` pair is reduced to its hash, because the console sets the theme through
 * `localStorage` and the hash in a single `evaluate` and it is the hash that names the surface. The
 * sequence is otherwise compared EXACTLY — the Rust plan carries `set_viewport` per surface precisely
 * so that "resized once per width" and "resized before every page" are different plans and stay
 * different here.
 */
export function planEvents(trace) {
  const keep = [];
  for (const e of trace) {
    if (e.t === "viewport") keep.push({ t: "viewport", width: e.width, height: e.height });
    else if (e.t === "media") keep.push({ ...e });
    else if (e.t === "goto") keep.push({ t: "goto", url: e.url });
    else if (e.t === "reload") keep.push({ t: "reload" });
    else if (e.t === "hash") {
      const h = Array.isArray(e.hash) ? e.hash[e.hash.length - 1] : e.hash;
      keep.push({ t: "hash", hash: h });
    }
  }
  return keep;
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [tree, tool, passes] = process.argv.slice(2);
  // The CLI form spells "absent" as `-`, because a bare empty argument is indistinguishable from a
  // missing one in a shell.
  const { stub } = trace(tree, tool, passes === "-" ? null : passes, process.env.SUMMRISE_PARITY_TMP || "/tmp/sweep-plan-parity");
  const events = await awaitTrace(stub);
  process.stdout.write(JSON.stringify({ tool, passes, events }));
}
