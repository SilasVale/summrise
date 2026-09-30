#!/usr/bin/env node
/**
 * Make `dsh-workspace`'s path canon the FS SEAM's, not the host's.
 *
 * # The wall this removes (measured 2026-09-30, on the official providers)
 *
 * `dsh-workspace` decides what a workspace path IS with the host's own filesystem:
 *
 *     import { mkdir, realpath, stat } from "node:fs/promises";       // lib/index.js:2
 *     async function realpathNormalize(path) {
 *       if (!fullyQualifiedWorkspacePath(path)) throw new TypeError(…);
 *       return await realpath(path);                                  // lib/index.js:48
 *     }
 *
 * and its own doc comment says that is deliberate: "This is the ONE uniqueness canon of the package —
 * workspace paths are stored canonicalized, uniqueness is string equality of canonicalized paths … and
 * attach-time session `cwd` checks go through the same canon."
 *
 * That is coherent while the workspace lives on the harness host. It is not coherent with the OFFICIAL
 * remote providers, whose whole point is that the files live on ANOTHER machine: `@deepseek-ai/dsh-fs-ssh`
 * serves `ctx.fs` from a POSIX SSH host, and `dsh-workspace` then asks the LOCAL disk whether that path
 * exists. Measured on `zss` with `dsh-ssh`/`dsh-fs-ssh`/`dsh-subprocess-ssh`/`dsh-sandbox-ssh` all active
 * and the remote helper running:
 *
 *     workspace/create {path: "/home/zhengsaisi/summrise"}
 *       -> workspace/invalid-path: ENOENT: no such file or directory, realpath '/home/zhengsaisi/summrise'
 *
 * The path exists — on the workspace host. Nothing could be created and no session could attach, so the
 * UI had no way to be used at all.
 *
 * # What it changes
 *
 * The seam already has the right primitive, and its own type declarations say so: "a consumer never
 * manufactures a key, it receives one from `resolve()`", and "the local backend passes a realpath".
 * So the registry canonicalizes through `ctx.fs.resolve()` and asks `ctx.fs.stat()` whether the result is
 * a directory — which is the remote answer under the SSH providers and the local one under `dsh-fs-local`.
 *
 * A missing seam falls back to the host functions, so a profile with no `ctx.fs` behaves exactly as before.
 * `mkdir` is deliberately NOT rerouted: the seam has no directory creation (the workspace door is
 * read-only), and `initializeDefault` only runs for the default workspace on an empty install.
 *
 * # Usage
 *
 *     node dsh-workspace-canon.mjs <path to dsh-workspace/lib/index.js>
 *     node dsh-workspace-canon.mjs --dsh <DSH install dir>
 */
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

function resolveTarget(argv) {
  const flag = argv.indexOf('--dsh');
  if (flag >= 0 && argv[flag + 1] !== undefined) return join(argv[flag + 1], 'node_modules/@deepseek-ai/dsh-workspace/lib/index.js');
  return argv[2];
}

const TARGET = resolveTarget(process.argv);
if (TARGET === undefined) {
  console.error('usage: node dsh-workspace-canon.mjs <dsh-workspace/lib/index.js>');
  console.error('       node dsh-workspace-canon.mjs --dsh <DSH install dir>');
  process.exit(2);
}
if (!existsSync(TARGET)) {
  console.error(`REFUSED: no such file: ${TARGET}`);
  process.exit(2);
}

const original = readFileSync(TARGET, 'utf8');

/** Only the patched file carries this. */
const MARKER = 'PATCHED: the workspace canon is the fs seam';
if (original.includes(MARKER)) {
  console.log('already patched');
  process.exit(0);
}

/** The two methods, inserted where the registry's own methods begin. */
const METHODS = `	/**
	* ${MARKER}'s, not the host's: \`ctx.fs.resolve()\` canonicalizes where the files actually are.
	*
	* Under the local filesystem provider this is the same answer the host's \`realpath\` gave. Under
	* \`@deepseek-ai/dsh-fs-ssh\` it is the REMOTE host's answer — which is the only answer that can be
	* right, because the path names a directory on that machine. A profile with no \`ctx.fs\` keeps the
	* host behaviour through the fallback.
	*/
	async canonicalizePath(path) {
		const fs = this.ctx.fs;
		if (fs === void 0) return await realpathNormalize(path);
		const resolved = await fs.resolve(path);
		return resolved.displayPath ?? resolved.targetKey;
	}
	/**
	* Whether the seam (or, without one, the host) reports a directory at this canonical path.
	*
	* THE SEAM'S \`stat\` TAKES THE TARGET \`resolve\` RETURNED, NOT A PATH — its own types say a consumer
	* "never manufactures a key, it receives one from resolve()", and passing the string instead answers
	* \`invalid_type: expected object, received string\` at path \`target\` (measured). So the resolve happens
	* here too; it is a pure operation on both backends.
	*/
	async isDirectoryPath(path) {
		const fs = this.ctx.fs;
		if (fs === void 0) return (await stat(path)).isDirectory();
		const target = await fs.resolve(path);
		return (await fs.stat(target))?.type === "directory";
	}
`;

/**
 * `from` is rewritten to `to`. An entry may carry `ownedBy`: a marker proving some OTHER patch already
 * rerouted that site through the seam, in which case an absent anchor is not a changed package but a
 * finished one. Without that field an absent anchor is a refusal, because half a canon is worse than none.
 */
const EDITS = [
  // the two call shapes the registry uses for canonicalization
  { from: 'await realpathNormalize(path)', to: 'await this.canonicalizePath(path)' },
  { from: 'await realpathNormalize(header.cwd)', to: 'await this.canonicalizePath(header.cwd)' },
  // and the three directory checks
  { from: 'if (!(await stat(canonical)).isDirectory())', to: 'if (!(await this.isDirectoryPath(canonical)))' },
  // `dsh-workspace-registry.mjs` already guards the attach path with its own `viaSeam` flag, so this site is
  // finished when that flag is present and the bare anchor is gone.
  { from: 'if (!(await stat(cwd)).isDirectory())', to: 'if (!(await this.isDirectoryPath(cwd)))', ownedBy: 'viaSeam' },
  { from: 'if (!(await stat(path)).isDirectory()) {', to: 'if (!(await this.isDirectoryPath(path))) {' },
];

const INSERT_AT = '\tasync create(path, title) {';

// THE REPLACEMENTS RUN FIRST, AND THE METHODS ARE INSERTED AFTER THEM. The other order looks equivalent and
// is not: the inserted method's own fallback line contains the very anchor being replaced, so inserting first
// rewrote it into a call to itself — infinite recursion whenever no seam is mounted. The test's count of the
// rerouted sites is what caught that, which is why it counts instead of merely checking for a substring.
let patched = original;
const counts = {};
const skipped = [];
for (const { from, to, ownedBy } of EDITS) {
  const before = patched.split(from).length - 1;
  if (before === 0 && ownedBy !== undefined && patched.includes(ownedBy)) {
    skipped.push(`${from} (already rerouted by a patch carrying ${JSON.stringify(ownedBy)})`);
    continue;
  }
  counts[from] = before;
  if (before === 0) {
    console.error(`REFUSED: no occurrence of ${JSON.stringify(from)} — nothing was written.`);
    process.exit(1);
  }
  patched = patched.split(from).join(to);
}

if (!patched.includes(INSERT_AT)) {
  console.error(`REFUSED: the anchor for the inserted methods is not in ${TARGET}.`);
  process.exit(1);
}
patched = patched.replace(INSERT_AT, METHODS + INSERT_AT);

writeFileSync(TARGET, patched, 'utf8');
console.log(`patched: ${TARGET}`);
console.log(`  ${MARKER}'s: ${Object.values(counts).reduce((a, b) => a + b, 0)} call site(s) rerouted`);
for (const [from, n] of Object.entries(counts)) console.log(`    ${n}x ${from}`);
for (const s of skipped) console.log(`    skipped ${s}`);
console.log(`  bytes: ${original.length} -> ${patched.length}`);
