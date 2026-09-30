#!/usr/bin/env node
/**
 * Make `dsh-workspace` treat a POSIX absolute path as a fully qualified WORKSPACE path.
 *
 * # Why this is needed at all (measured 2026-09-30, desktop-14rjcr8)
 *
 * `fullyQualifiedWorkspacePath` asks the HOST's platform what "absolute" means:
 *
 *     if (platform !== "win32") return posix.isAbsolute(path);
 *     const root = win32.parse(path).root;
 *     return win32.isAbsolute(path) && root !== "\\" && root !== "/";
 *
 * DSH runs on Windows here and the workspace's files live on a POSIX host, so the answer is wrong for
 * every path the operator can actually pick. The end of the flow the operator uses — choose a folder in
 * the "add workspace" dialog — answered:
 *
 *     workspace/invalid-path: cannot create a Workspace at "/home/zhengsaisi/summrise":
 *     Workspace path is not fully qualified: '/home/zhengsaisi/summrise'
 *
 * The dialog could LIST the Linux filesystem (that is `dsh-directory-picker.mjs`) and still could not
 * CREATE anything from it, which is a flow that looks finished and is not.
 *
 * # What it changes
 *
 * A POSIX absolute path is fully qualified, full stop. That is not a relaxation for convenience: a
 * workspace whose path is `/home/...` is by construction one whose files are on a POSIX host — the only
 * way this deployment produces such a path — and `win32.parse("/home/x").root` being `"/"` says nothing
 * about whether the path is usable. Drive-rooted paths keep the rules they had.
 *
 * # How it survives a DSH upgrade
 *
 * `start-dsh.ps1` re-applies every patch in this directory before each launch, and this one is
 * idempotent BY MARKER — the comment below, which only the patched file carries.
 */
import { readFileSync, writeFileSync } from 'node:fs';

const TARGET = process.argv[2];
if (!TARGET) {
  console.error('usage: patch-dsh-workspace-path.mjs <path to dsh-workspace/lib/index.js>');
  process.exit(2);
}

const original = readFileSync(TARGET, 'utf8');
const MARKER = 'PATCHED: a POSIX absolute path is fully qualified';
if (original.includes(MARKER)) {
  console.log('already patched');
  process.exit(0);
}

const FROM = `function fullyQualifiedWorkspacePath(path, platform = process.platform) {
	if (platform !== "win32") return posix.isAbsolute(path);
	const root = win32.parse(path).root;
	return win32.isAbsolute(path) && root !== "\\\\" && root !== "/";
}`;
const TO = `function fullyQualifiedWorkspacePath(path, platform = process.platform) {
	// PATCHED: a POSIX absolute path is fully qualified. A workspace spelled /home/... is one whose files are on
	// a POSIX host — the only way this deployment produces such a path — so asking the HOST's platform whether it
	// is absolute answers a different question than the one being asked.
	if (posix.isAbsolute(path)) return true;
	if (platform !== "win32") return false;
	const root = win32.parse(path).root;
	return win32.isAbsolute(path) && root !== "\\\\" && root !== "/";
}`;

if (!original.includes(FROM)) {
  console.error(`REFUSED: the anchor is not in ${TARGET}.`);
  console.error('The package changed shape; re-read it rather than guessing at a new anchor.');
  process.exit(1);
}
if (original.indexOf(FROM) !== original.lastIndexOf(FROM)) {
  console.error('REFUSED: the anchor appears more than once.');
  process.exit(1);
}

const patched = original.replace(FROM, TO);
writeFileSync(TARGET, patched, 'utf8');
console.log(`patched: ${TARGET}`);
console.log('  fullyQualifiedWorkspacePath -> a POSIX absolute path is qualified');
console.log(`  bytes: ${original.length} -> ${patched.length}`);
