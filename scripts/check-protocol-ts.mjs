#!/usr/bin/env node
// Regenerate the page's copy of the console<->device wire from `device/protocol/src/lib.rs`.
//
//   node scripts/check-protocol-ts.mjs          # write the bindings
//   node scripts/check-protocol-ts.mjs --check  # fail if a committed binding is stale
//
// The generated files under `device/console/web/src/protocol/` are outputs: edit the Rust type and
// re-run this, never the .ts file. `--check` is what CI (and a reviewer) runs.
import { spawnSync } from "node:child_process";
import process from "node:process";

const check = process.argv.includes("--check");
const target = "device/console/web/src/protocol";

const exported = spawnSync(
  "cargo",
  ["test", "--manifest-path", "device/Cargo.toml", "-p", "summrise-protocol", "export_bindings"],
  { stdio: "inherit" },
);
if (exported.status !== 0) {
  console.error("check-protocol-ts: the export failed; the bindings were not regenerated");
  process.exit(2);
}

if (check) {
  const diff = spawnSync("git", ["status", "--porcelain", "--", target], { encoding: "utf8" });
  if (diff.status !== 0) {
    console.error("check-protocol-ts: git could not report the generated files");
    process.exit(2);
  }
  if (diff.stdout.trim() !== "") {
    console.error(
      "check-protocol-ts: the committed bindings are stale:\n" + diff.stdout + "\n" +
        "run `node scripts/check-protocol-ts.mjs` and commit the result",
    );
    process.exit(1);
  }
}
console.log(check ? `protocol ts: up to date (${target})` : `protocol ts: written to ${target}`);
