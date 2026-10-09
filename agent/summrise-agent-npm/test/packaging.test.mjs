// THE PACKAGING SUITE — what must be true of the TARBALL, as opposed to what the CLI decides.
//
// ── WHY THIS FILE EXISTS AND WHAT IT REPLACED (landing 4b, the cutover) ───────────────────────────
//
// This package's suite used to be `cli.test.mjs`: 68 cases over the pure decisions exported by
// `bin/summrise.js` — the PowerShell generators, the quoting rules, the update guards, the status
// report. Every one of those decisions is Rust now (`agent/summrise-cli`, 146 tests of its own) and
// they are pinned answer-by-answer against the corpus the TypeScript left behind
// (`agent/summrise-cli/parity/expected.json`, 162 cases). Re-testing them HERE would be a second
// implementation of the same assertions, which is the shape this repository keeps paying for.
//
// What nothing checked was the OTHER half: whether the thing npm actually uploads can run. That is
// not a decision and it is not Rust's business — it is a claim about `package.json`, `files[]` and
// `required-in-tgz.txt`, and the device's only symptom of getting it wrong is that `summrise` is
// not a command (or is the previous release's command).
//
// ── THE CONSTRAINT THAT SHAPES EVERY CASE BELOW ───────────────────────────────────────────────────
//
// **NO CASE MAY REQUIRE A BUILT ARTIFACT TO EXIST.** This suite runs in CI's `pack-chain` job, which
// has no cargo-xwin, and on a fresh checkout where every `*.exe` here is gitignored and absent. So
// each case reads a RULE (the manifest, the required list, the tree) rather than a binary: "is the
// bin target something npm will pack" is checkable with nothing built, and it is the question whose
// answer decides whether the tarball is installable.
import test from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const pkg = require("../package.json");
const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");

/** `required-in-tgz.txt`: one path per line, `#` comments and blanks ignored. The ONE owner of the
 *  list — `scripts/publish-release.sh`, `.github/workflows/release.yml` and CI's pack gate all read
 *  this file rather than restating it. */
function requiredInTgz() {
  const text = fs.readFileSync(path.join(ROOT, "required-in-tgz.txt"), "utf8");
  return text
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l && !l.startsWith("#"));
}

const files = pkg.files;

test("the CLI is the Rust binary, not a compiled .js", () => {
  // THE CUTOVER'S OWN FACT, and the reason this suite exists at all: the npm `bin` is a cargo
  // artifact. `bin/summrise.js` was tsc output of `src/summrise.ts`, both are deleted, and a
  // package whose `bin` pointed back at a `.js` would ship a CLI nothing compiles any more.
  assert.equal(
    pkg.bin.summrise,
    "bin/summrise.exe",
    "the npm bin must be the Rust CLI — `cargo xwin build -p summrise-cli` output staged by " +
      "scripts/build.sh. MEASURED before this was chosen: npm's shim (cmd-shim 8.0.0, bundled " +
      "with npm 11.19.0) reads the bin target, finds no shebang, and generates a .cmd that runs " +
      "the file DIRECTLY — no `node` in between — so a native .exe is deliverable by npm.",
  );
  assert.ok(
    pkg.bin.summrise.endsWith(".exe"),
    `a non-.exe bin target is the generated-JS arrangement this landing removed: ${pkg.bin.summrise}`,
  );
  for (const gone of ["src/summrise.ts", "bin/summrise.js", "tsconfig.json"]) {
    assert.ok(
      !fs.existsSync(path.join(ROOT, gone)),
      `${gone} is back. The TypeScript CLI and its tsc emit were deleted with the cutover and ` +
        `nothing regenerates them; a stale copy left on this box would be a second, unbuilt CLI ` +
        `beside the one that ships. Delete it.`,
    );
  }
});

test("the bin target is inside files[], because npm packs nothing else", () => {
  // `files[]` is an ALLOWLIST: a bin target outside it is a command npm links to a file the tarball
  // does not contain. The install still succeeds and prints `added 1 package`; `summrise` is simply
  // not there.
  assert.ok(
    files.includes(pkg.bin.summrise),
    `files[] must declare ${pkg.bin.summrise}, or the tarball ships a bin it does not contain: ` +
      `${JSON.stringify(files)}`,
  );
});

test("no files[] entry is a DIRECTORY", () => {
  // THE ROUND-278 LESSON, kept as a rule rather than as a comment: `files: ["bin/"]` passed a
  // "does the entry exist" check while the CLI itself was missing, because the directory existed.
  // Every entry here must name a file, so a check on it is a check on the file.
  const dirs = files.filter((f) => f.endsWith("/"));
  assert.deepEqual(
    dirs,
    [],
    `these files[] entries are directories, so a gate over them can pass with the file inside ` +
      `missing — name the files: ${JSON.stringify(dirs)}`,
  );
});

test("every required-in-tgz.txt entry is declared in files[]", () => {
  // THE DIRECTION NOTHING ELSE CHECKS. `scripts/test/build-pins.bash` asserts the other one (every
  // files[] entry is named in required-in-tgz.txt). This is its complement and it fails at a
  // different moment: a file required by the release gates but EXCLUDED from the pack is a tarball
  // that cannot satisfy its own release check — the failure lands in the release job, after the
  // version is bumped, not in the pull request.
  for (const entry of requiredInTgz()) {
    assert.ok(
      files.includes(entry),
      `required-in-tgz.txt requires ${entry} and files[] does not pack it — npm pack cannot ` +
        `satisfy the release gate. Either add it to files[] or take it off the required list.`,
    );
  }
});

test("no boxed COMPONENT artefact is in the package", () => {
  // The package is ~6.7 MB and carries NO component: `setup` FETCHES cloudflared (54 MB), the
  // playwright bundle (31 MB) and the electron runtime, verifying each against the release
  // manifest's sha256 pin. A boxed copy here would be a package nobody can upload, and — worse —
  // one whose component is older than the pin that describes it.
  for (const artefact of [
    "cloudflared.exe",
    "summrise-playwright.zip",
    "electron-win32-x64.zip",
  ]) {
    assert.ok(
      !files.includes(artefact),
      `${artefact} is a FETCHED component, not a packaged one — it must not be in files[]`,
    );
    assert.ok(
      !requiredInTgz().includes(artefact),
      `${artefact} is a FETCHED component, not a packaged one — it must not be required in the tgz`,
    );
  }
});

test("the launcher and the agent exe are both shipped", () => {
  // A scheduled task whose action is a program the package does not carry is Task Scheduler's
  // `0x2`, five minutes later, forever. `summrise-launch.exe` is what both tasks run and
  // `summrise-agent.exe` is the service.
  for (const exe of ["summrise-agent.exe", "summrise-launch.exe"]) {
    assert.ok(files.includes(exe), `the package must ship ${exe}: ${JSON.stringify(files)}`);
    assert.ok(
      requiredInTgz().includes(exe),
      `required-in-tgz.txt must require ${exe}, or the release gates cannot see it go missing`,
    );
  }
});

test("the desktop shell's sources and icons are shipped", () => {
  // FOUR FILES BECAUSE ONE REQUIRES THE NEXT (landing 6): `main.js` requires
  // `./summrise_url_policy`, which requires `./summrise_url_policy_bg.wasm`. A tarball carrying the
  // JavaScript and not the module starts a shell that dies with `Cannot find module`.
  for (const f of [
    "summrise-desktop-electron/src/main.js",
    "summrise-desktop-electron/src/preload.js",
    "summrise-desktop-electron/src/summrise_url_policy.js",
    "summrise-desktop-electron/src/summrise_url_policy_bg.wasm",
    "summrise-desktop-electron/icon.png",
    "summrise-desktop-electron/icon.ico",
  ]) {
    assert.ok(files.includes(f), `the package must ship ${f}`);
    assert.ok(requiredInTgz().includes(f), `required-in-tgz.txt must require ${f}`);
  }
});

test("the package is Windows x64 only", () => {
  // It carries PE binaries and a `bin` whose shim story is measured on Windows. Installing it on a
  // platform it cannot run is the failure npm's `os`/`cpu` fields exist to refuse.
  assert.deepEqual(pkg.os, ["win32"], "the package ships Windows binaries");
  assert.deepEqual(pkg.cpu, ["x64"], "and x64 ones");
});

test("the version is a plain semver", () => {
  // Load-bearing three ways: `release.yml` refuses a tag that disagrees with it, the CLI's own
  // `--version` acceptance check parses it, and `updateWouldNotMove` compares it against the
  // device's release marker. A range or a build suffix is not a version any of those can read.
  assert.match(
    pkg.version,
    /^\d+\.\d+\.\d+$/,
    `package.json's version must be a plain dotted triple, got ${JSON.stringify(pkg.version)}`,
  );
});

test("the build script compiles no CLI", () => {
  // The package's `npm run build` is the electron shell's emit (and the copy of the committed wasm
  // pair). It used to also compile `src/summrise.ts` into `bin/summrise.js`; that half is gone, and
  // a script that still named it would fail with ENOENT on a source that no longer exists.
  const build = pkg.scripts.build;
  // `-p tsconfig.json` AND NOT THE BARE NAME, because the electron compile legitimately names
  // `../summrise-desktop-electron/tsconfig.json` — a check for `tsconfig.json` matches that too and
  // failed on the first run of this very case. The needle has to be the form the deleted compile
  // used: this package's OWN project file, which no longer exists.
  for (const dead of ["bin/summrise.js", "summrise-fresh-bin", "-p tsconfig.json"]) {
    assert.ok(
      !build.includes(dead),
      `the build script still names ${dead}, which the cutover deleted: ${build}`,
    );
  }
  assert.ok(
    build.includes("summrise-desktop-electron"),
    "the build script's remaining job is the electron shell's emit",
  );
});
