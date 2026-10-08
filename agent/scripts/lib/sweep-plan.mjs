// sweep-plan — THE PLAN COMES FROM RUST.
//
// BOUNDARY (docs/superpowers/specs/2026-10-08-every-decision-is-rust-design.md §2): the platform call
// this file performs is running the `summrise-sweep-plan` binary and reading its stdout. It decides
// nothing — the tool name and the pass list are the caller's own command line, and the plan is
// whatever the binary printed. Everything a payload used to derive for itself (which surfaces are
// visited, in what order, at which density/path/viewport/theme/mode, which passes run on each, and the
// policy caps and floors) is computed there, once, at EMIT time, and then travels as DATA.
//
// WHY CARGO RUN AND NOT A PREBUILT PATH. `cargo run` REBUILDS when the crate changed, so the plan a
// payload embeds cannot be a stale one — which a fixed path would silently allow, and the failure
// would look like a design regression rather than a build one. `SUMMRISE_SWEEP_PLAN_BIN` overrides it
// for a caller that has staged the binary somewhere (a device, a release bundle).
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const REPO = fileURLToPath(new URL("../../..", import.meta.url));

/**
 * The plan for one tool and one `--passes` spec, as a JS object.
 *
 * `passes` is the flag's VALUE, and `null` means it was absent — which is not the same input as an
 * empty string, and the two tools disagree about what each means. The emitter hands over exactly what
 * it read from argv, and the binary owns the interpretation.
 *
 * @param {"panel"|"console"|"landing"} tool
 * @param {string|null} passes
 * @returns {object} the plan, as printed by the Rust
 */
export function sweepPlan(tool, passes) {
  const args = ["--tool", tool];
  if (passes !== null && passes !== undefined) args.push(`--passes=${passes}`);
  const out = planBinary().run(args);
  let plan;
  try {
    plan = JSON.parse(out);
  } catch (e) {
    throw new Error(
      `summrise-sweep-plan --tool ${tool}${passes == null ? "" : ` --passes=${passes}`} did not print a plan: ` +
        `${String(e.message).slice(0, 120)}; first bytes were ${JSON.stringify(out.slice(0, 80))}`,
    );
  }
  return plan;
}

let cached = null;
function planBinary() {
  if (cached) return cached;
  const staged = process.env.SUMMRISE_SWEEP_PLAN_BIN;
  if (staged) {
    if (!existsSync(staged)) {
      throw new Error(`SUMMRISE_SWEEP_PLAN_BIN is set to ${staged}, which does not exist`);
    }
    cached = { run: (args) => execFileSync(staged, args, { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 }) };
    return cached;
  }
  const manifest = join(REPO, "agent", "Cargo.toml");
  if (!existsSync(manifest)) {
    throw new Error(
      `the sweep plan is Rust and its crate is not beside this file (looked for ${manifest}); ` +
        `set SUMMRISE_SWEEP_PLAN_BIN to a prebuilt summrise-sweep-plan`,
    );
  }
  if (hasCargo()) {
    cached = {
      run: (args) =>
        execFileSync("cargo", ["run", "--quiet", "--manifest-path", manifest, "-p", "summrise-sweep-plan", "--", ...args], {
          encoding: "utf8",
          maxBuffer: 64 * 1024 * 1024,
          stdio: ["ignore", "pipe", "inherit"],
        }),
    };
    return cached;
  }
  // NO CARGO ON THIS PATH — AND THE PRE-COMMIT HOOK IS THE CALLER THAT MAKES THIS REAL. It assembles all
  // five artifacts on every commit, and blocking a commit because a toolchain is not on a developer's
  // PATH is a worse outcome than the staleness this fallback has to guard against. So a PREBUILT binary
  // is used, and only when it is NEWER THAN EVERY SOURCE FILE of the crate: a plan embedded from a stale
  // binary would look like a design regression rather than a build one, and that is the failure the
  // `cargo run` default exists to prevent.
  const built = join(REPO, "agent", "target", "debug", "summrise-sweep-plan");
  if (existsSync(built)) {
    const stale = staleAgainst(built);
    if (!stale) {
      cached = { run: (args) => execFileSync(built, args, { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 }) };
      return cached;
    }
    throw new Error(
      `no cargo on PATH and the prebuilt ${built} is OLDER than ${stale} — build it ` +
        `(cargo build -p summrise-sweep-plan) or put cargo on PATH; a plan embedded from a stale binary ` +
        `reports a design change that never happened`,
    );
  }
  throw new Error(
    "the sweep plan is Rust and there is neither a cargo on PATH nor a built " +
      `${built} — run \`cargo build -p summrise-sweep-plan\` (or set SUMMRISE_SWEEP_PLAN_BIN)`,
  );
}

/** Is `cargo` runnable? Asked once, and cheaply. */
let cargoAnswer = null;
function hasCargo() {
  if (cargoAnswer !== null) return cargoAnswer;
  try {
    execFileSync("cargo", ["--version"], { stdio: "ignore" });
    cargoAnswer = true;
  } catch (e) {
    cargoAnswer = false;
  }
  return cargoAnswer;
}

/** The newest source file of the crate, if it is newer than `built` — the staleness guard above. */
function staleAgainst(built) {
  const j = join;
  const crate = join(REPO, "agent", "sweep-plan");
  const t = statSync(built).mtimeMs;
  const walk = (dir) => {
    let newest = null;
    for (const name of readdirSync(dir)) {
      const full = j(dir, name);
      const st = statSync(full);
      if (st.isDirectory()) {
        const inner = walk(full);
        if (inner && (!newest || statSync(inner).mtimeMs > statSync(newest).mtimeMs)) newest = inner;
        continue;
      }
      if (!/\.(rs|toml)$/.test(name)) continue;
      if (st.mtimeMs > t && (!newest || st.mtimeMs > statSync(newest).mtimeMs)) newest = full;
    }
    return newest;
  };
  return walk(crate);
}
