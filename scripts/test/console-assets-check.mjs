// console-assets-check.mjs — A CONSOLE SOURCE CHANGE OWES ITS BUILT ASSETS, AND A REBUILD MUST PROVE IT.
//
// WHY THIS EXISTS (round 178 of the standing goal, from a gap round 177 measured). The console's built assets under
// `gateway/public/assets/` are TRACKED, hashed, and referenced by `gateway/public/index.html`. Round 172 and 173 changed
// console source and left those assets at the old revision, and NOTHING noticed: the PANEL has a guard for exactly this
// (`agent/build.rs` refuses to compile when `resources/panel-react/src` is newer than `resources/panel/`, because that bundle
// is embedded at compile time), the console embeds nothing, and CI's `ui` job rebuilds the same new file and never asks
// whether the tree it built from was consistent.
//
// THE CHECK IS A REBUILD, and the assertion is that a rebuild changes NOTHING that git tracks. In the good case it is a no-op
// on the tree; in the bad case the diff it leaves behind IS the fix, which is the whole reason to prefer this shape over an
// mtime comparison — round 146 recorded what an mtime guard costs when a file is RESTORED rather than edited.
//
// IT NEEDS `gateway/ui` DEPENDENCIES and takes about a second; it refuses to run on a dirty tree, so the diff it produces
// cannot be confused with somebody's work.
//
// Run: node scripts/test/console-assets-check.mjs
import { execFileSync } from "node:child_process";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "") + "/..";
const git = (...a) => execFileSync("git", a, { cwd: ROOT, encoding: "utf8" }).trim();

const dirty = git("status", "--porcelain");
if (dirty) {
  console.error(
    `FAIL the working tree is not clean, so a rebuild's diff could not be told from your work:\n${dirty}\n` +
      `Commit or stash first — this check rebuilds the console in place.`,
  );
  process.exit(1);
}

const before = git("status", "--porcelain");
try {
  execFileSync("npm", ["run", "build"], { cwd: `${ROOT}/gateway/ui`, stdio: "pipe" });
} catch (e) {
  console.error(
    `FAIL the console build itself failed (rc=${e.status ?? 1}) — that is a different problem, and it should be fixed first:\n` +
      String(e.stdout || "").slice(-500),
  );
  process.exit(1);
}
const after = git("status", "--porcelain");

if (after !== before) {
  console.error(
    `console-assets: the tracked assets did NOT match the source — a rebuild changed them:\n${after}\n\n` +
      `Commit the rebuild's output. The assets under gateway/public/assets/ are tracked and referenced by index.html, so a\n` +
      `freshly checked-out tree would otherwise serve a console built from older source.`,
  );
  process.exit(1);
}
console.log("console-assets: a rebuild of the console changes nothing git tracks — the committed assets match the source");
