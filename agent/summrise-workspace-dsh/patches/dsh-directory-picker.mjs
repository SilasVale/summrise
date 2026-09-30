#!/usr/bin/env node
/**
 * Make the in-app directory picker browse the WORKSPACE host through the fs seam, instead of this
 * machine's own filesystem.
 *
 * # Why this is needed at all (measured 2026-09-30, desktop-14rjcr8)
 *
 * The operator's words: "我的意思是做到打开工作区的地方啊，不是另外加个 workspace". They are right, and this is
 * that place: DSH's own 工作区 sidebar → add → a dialog. After `directory-picker-auto` was switched to the in-app
 * variant (`-browse`, because the native one cannot open a dialog from a session-0 service), that dialog OPENS —
 * and lists `AppData`, `Desktop`, `Documents` … because this backend imports `node:fs/promises` and browses **the
 * DSH host**, not the workspace.
 *
 * The workspace's files live on the other machine. DSH's fs seam (`ctx.fs`) is already pointed there by
 * `summrise-workspace-dsh/fs`, so the fix is to ask the seam instead of `node:fs` — and then the same dialog, in
 * the same place, with nothing added, lists `/home/zhengsaisi/…` and a picked folder becomes a workspace.
 *
 * # What it changes, and why each piece is the shape it is
 *
 *   1. `list()` reads through `ctx.fs.listDir(path, signal)` — the seam's own method — and keeps the EXACT shape
 *      the client half consumes: `{path, home, crumbs, entries, truncated}` with rows `{name, path, hidden}`.
 *      The shape is not a detail: the client renders it, and a renamed field would show as an empty browser.
 *   2. `home` is `/`, because on the workspace host that is where browsing starts — the device's `homedir()`
 *      names a directory that does not exist on the far side.
 *   3. The crumbs are built POSIX-wise here. The backend's own `ancestryCrumbs` uses `node:path`, whose win32
 *      flavour would answer `/home\\` for a POSIX path — the bug this file exists to avoid, one layer down.
 *   4. `createDirectory` REFUSES BY NAME. The workspace fs door reads only (`stat`, `lstat`, `readText`,
 *      `readBytes`, `listDir`), so creating a folder on the workspace host is not something this deployment can
 *      do — and a generic failure there would read as a bug rather than as a boundary.
 *
 * # Why a patch and not a fork
 *
 * The plugin is DSH's, and this is a deployment: the same reason `patch-dsh-workspace-registry.mjs` exists. The
 * anchors are exact strings from the published package, and the script REFUSES rather than half-applying.
 */
import { readFileSync, writeFileSync } from 'node:fs';

const TARGET = process.argv[2];
if (!TARGET) {
  console.error('usage: patch-dsh-directory-picker.mjs <path to dsh-host-directory-picker-browse/lib/index.js>');
  process.exit(2);
}

const original = readFileSync(TARGET, 'utf8');
let text = original;

/** Replace one exact block, or refuse — a patch that half-applies is worse than one that does not. */
function replaceOnce(label, from, to) {
  const at = text.indexOf(from);
  if (at < 0) {
    console.error(`REFUSED: ${label} — its anchor is not in ${TARGET}.`);
    console.error('The package changed shape; re-read it rather than guessing at a new anchor.');
    process.exit(1);
  }
  if (text.indexOf(from, at + 1) >= 0) {
    console.error(`REFUSED: ${label} — its anchor appears more than once.`);
    process.exit(1);
  }
  text = text.slice(0, at) + to + text.slice(at + from.length);
}

const LIST_FROM = `	async list(path, signal) {
		const home = homedir();`;
const LIST_TO_START = text.indexOf(LIST_FROM);
if (LIST_TO_START < 0) {
  console.error(`REFUSED: list() — its head is not in ${TARGET}.`);
  process.exit(1);
}
const CREATE_AT = text.indexOf('	async createDirectory(path, name) {', LIST_TO_START);
if (CREATE_AT < 0) {
  console.error('REFUSED: createDirectory() — not found after list(); the method order changed.');
  process.exit(1);
}

const NEW_LIST = `	async list(path, signal) {
		// PATCHED: the WORKSPACE host's filesystem, through the fs seam, not this machine's.
		const home = "/";
		// PATCHED: a POSIX absolute path. The backend's own fullyQualified is win32's notion, and it refuses
		// /home/zhengsaisi on this deployment — the same wall workspace/create hits ("Workspace path is not
		// fully qualified"). The workspace host is POSIX by construction: the helper is a POSIX execve shim.
		if (path !== void 0 && !path.startsWith("/")) throw new DirectoryPickerError("directory-unreadable", path, \`cannot list "\${path}": not an absolute path on the workspace host\`);
		const target = path ?? home;
		let rows;
		try {
			rows = await this.ctx.fs.listDir(target, signal);
		} catch (error) {
			signal?.throwIfAborted();
			throw new DirectoryPickerError("directory-unreadable", target, \`cannot list \${target}: \${messageOf(error)}\`);
		}
		const entries = [];
		let truncated = false;
		for (const row of rows) {
			signal?.throwIfAborted();
			// DIRECTORIES ONLY, as the host path was: the browser shows what can be entered. The seam's rows are
			// its OWN shape — name, type, target, version, size — so the field is type and not the door's kind:
			// two wrong guesses (kind, then the value) each answered entries: [] on the device, which is what
			// an empty browser looks like from the inside.
			if (row.type !== "directory") continue;
			if (entries.length === this.config.maxEntries) {
				truncated = true;
				break;
			}
			entries.push({
				name: row.name,
				path: \`\${target.replace(/\\/$/, "")}/\${row.name}\`,
				hidden: row.name.startsWith(".")
			});
		}
		return {
			path: target,
			home,
			crumbs: posixCrumbs(target),
			entries,
			truncated
		};
	}
`;
text = text.slice(0, LIST_TO_START) + NEW_LIST + text.slice(CREATE_AT);

const CREATE_FROM = `	async createDirectory(path, name) {`;
const CREATE_END = text.indexOf('\n	}', text.indexOf(CREATE_FROM)) + 3;
if (CREATE_END < 3) {
  console.error('REFUSED: createDirectory() — its body has no end.');
  process.exit(1);
}
text =
  text.slice(0, text.indexOf(CREATE_FROM)) +
  `	async createDirectory(path, name) {
		// PATCHED: refused BY NAME. The workspace fs door reads only, so a folder cannot be created on the
		// workspace host from here — and a generic failure would read as a bug rather than as a boundary.
		throw new DirectoryPickerError(
			"directory-create-failed",
			path,
			\`cannot create "\${name}" in \${path}: the workspace host's fs door reads only (stat, lstat, readText, readBytes, listDir)\`
		);
	}
` +
  text.slice(CREATE_END);

// the POSIX crumbs helper, beside the backend's own (which uses node:path and would answer /home\ for /home/x)
// THE INJECTION, without which `ctx.fs` is not reachable at all: cordis refuses a service access that is not
// declared, and the error it throws ("cannot get property \"fs\" without inject") names the fix. Neither this
// class nor its base declares any injection, so adding one here is additive rather than a replacement.
const CLASS_ANCHOR = `var BrowseDirectoryPicker = class extends DirectoryPicker {
	config;`;
if (!text.includes(CLASS_ANCHOR)) {
  console.error('REFUSED: the picker class — its head is not in ' + TARGET + '.');
  process.exit(1);
}
text = text.replace(
  CLASS_ANCHOR,
  `var BrowseDirectoryPicker = class extends DirectoryPicker {
	/** PATCHED IN: the fs seam, which is where the workspace host lives. */
	static inject = ["fs"];
	config;`,
);

const HELPER_ANCHOR = 'function ancestryCrumbs(target) {';
if (!text.includes(HELPER_ANCHOR)) {
  console.error('REFUSED: ancestryCrumbs() — not found, so the POSIX helper has nowhere to go.');
  process.exit(1);
}
text = text.replace(
  HELPER_ANCHOR,
  `/** PATCHED IN: crumbs for a POSIX path, built POSIX-wise. \`node:path\`'s win32 flavour answers \`/home\\\\\` here. */
function posixCrumbs(target) {
	const parts = target.split("/").filter((s) => s.length > 0);
	const crumbs = [{ name: "/", path: "/", hidden: false }];
	let current = "";
	for (const part of parts) {
		current += \`/\${part}\`;
		crumbs.push({ name: part, path: current, hidden: part.startsWith(".") });
	}
	return crumbs;
}

${HELPER_ANCHOR}`,
);

writeFileSync(TARGET, text, 'utf8');
console.log(`patched: ${TARGET}`);
console.log(`  list()            -> ctx.fs.listDir, POSIX crumbs, directories only`);
console.log(`  createDirectory() -> a refusal by name`);
console.log(`  bytes: ${original.length} -> ${text.length}`);
