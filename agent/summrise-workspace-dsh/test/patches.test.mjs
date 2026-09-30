/**
 * The patch programs' tests, run with `node --test` and no DSH install.
 *
 * Each patch edits a PUBLISHED package by exact string anchor, so the three things worth testing are the
 * three ways it can go wrong: it must apply to the shape it was written for, it must not apply twice, and
 * it must REFUSE — writing nothing — when the package has changed shape underneath it. A fixture carrying
 * the same anchors as the real file is what makes those testable without a DSH tree.
 *
 * The fourth test is the one this file exists for: a blanket replacement of `realpathNormalize` would also
 * rewrite the function's OWN body and its export, leaving a package that still parses and can never fall
 * back to the host. The fixture pins the count, so that edit cannot pass.
 */

import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

const PATCH = new URL('../patches/dsh-workspace-canon.mjs', import.meta.url).pathname;

/** The anchors, exactly as `@deepseek-ai/dsh-workspace@0.2.0-rc.1` spells them. */
const FIXTURE = `import { mkdir, realpath, stat } from "node:fs/promises";
async function realpathNormalize(path) {
	if (!fullyQualifiedWorkspacePath(path)) throw new TypeError(\`Workspace path is not fully qualified: '\${path}'\`);
	return await realpath(path);
}
class WorkspaceRegistry {
	async attachSession(sessionId) {
		let cwd;
		cwd = await realpathNormalize(header.cwd);
		if (!(await stat(cwd)).isDirectory()) throw new Error("not a directory");
	}
	async create(path, title) {
		const canonical = await realpathNormalize(path);
		if (!(await stat(canonical)).isDirectory()) throw new Error("not a directory");
	}
	async resolveByPath(path) {
		const canonical = await realpathNormalize(path);
		return canonical;
	}
	async indexHeader(header) {
		const path = await realpathNormalize(header.cwd);
		if (!(await stat(path)).isDirectory()) {
			return;
		}
	}
}
export { realpathNormalize };
`;

function withFixture(body) {
  const dir = mkdtempSync(join(tmpdir(), 'dsh-canon-test-'));
  const file = join(dir, 'index.js');
  try {
    writeFileSync(file, FIXTURE, 'utf8');
    return body(file, dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function runPatch(file) {
  try {
    return { code: 0, out: execFileSync(process.execPath, [PATCH, file], { encoding: 'utf8' }) };
  } catch (error) {
    return { code: error.status ?? 1, out: String(error.stdout ?? '') + String(error.stderr ?? '') };
  }
}

test('applies to the published shape and reports every call site it rerouted', () => {
  withFixture((file) => {
    const { code, out } = runPatch(file);
    assert.equal(code, 0, out);
    assert.match(out, /7 call site\(s\) rerouted/);
    const patched = readFileSync(file, 'utf8');
    // the seam is what the registry asks now
    assert.match(patched, /const fs = this\.ctx\.fs;/);
    assert.match(patched, /fs\.resolve\(path\)/);
    assert.match(patched, /fs\.stat\(path\)\)\?\.type === "directory"/);
    assert.match(patched, /PATCHED: the workspace canon is the fs seam/);
  });
});

test('reroutes the call sites and NOT the function it falls back to', () => {
  withFixture((file) => {
    assert.equal(runPatch(file).code, 0);
    const patched = readFileSync(file, 'utf8');
    // the five sites
    assert.equal(patched.split('await this.canonicalizePath(path)').length - 1, 2);
    assert.equal(patched.split('await this.canonicalizePath(header.cwd)').length - 1, 2);
    assert.equal(patched.split('await this.isDirectoryPath(').length - 1, 3);
    // and the fallback path survives: the free function's body plus the seam-less arm of the new method
    assert.equal(patched.split('return await realpathNormalize(path);').length - 1, 1);
    assert.equal(patched.split('return await realpath(path);').length - 1, 1);
    assert.match(patched, /export \{ realpathNormalize \};/);
  });
});

test('is idempotent: a second run changes nothing', () => {
  withFixture((file) => {
    assert.equal(runPatch(file).code, 0);
    const once = readFileSync(file, 'utf8');
    const second = runPatch(file);
    assert.equal(second.code, 0);
    assert.match(second.out, /already patched/);
    assert.equal(readFileSync(file, 'utf8'), once);
  });
});

test('refuses — and writes nothing — when the package changed shape', () => {
  withFixture((file) => {
    writeFileSync(file, 'export const somethingElse = 1;\n', 'utf8');
    const { code, out } = runPatch(file);
    assert.notEqual(code, 0);
    assert.match(out, /REFUSED/);
    assert.equal(readFileSync(file, 'utf8'), 'export const somethingElse = 1;\n');
  });
});

test('the patched file still parses', () => {
  withFixture((file) => {
    assert.equal(runPatch(file).code, 0);
    execFileSync(process.execPath, ['--check', file]);
  });
});
