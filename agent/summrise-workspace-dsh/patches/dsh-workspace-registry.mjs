#!/usr/bin/env node
/**
 * Make `@deepseek-ai/dsh-workspace` validate a workspace path through the fs seam
 * instead of only through the DSH host's own filesystem.
 *
 * # Why this is needed at all (measured 2026-09-30, desktop-14rjcr8)
 *
 * DSH 0.2.0-rc.1's workspace registry imports `{ mkdir, realpath, stat }` from
 * `node:fs/promises` and validates a workspace's cwd against **the DSH host**. A
 * workspace whose files live on the WORKSPACE host therefore cannot be attached:
 *
 *     session/workspace-attach-failed: … its cwd '/home/zhengsaisi/summrise' does
 *     not resolve, so it cannot be validated
 *
 * That check is the one thing standing between "ctx.fs is remote" and a remote
 * workspace a person can actually open, and it is not something a backend can
 * satisfy from its own side: the call never reaches the backend. `realpathNormalize`
 * throws first, and `fullyQualifiedWorkspacePath` refuses a POSIX path outright.
 *
 * # What it changes, and why each piece is the shape it is
 *
 *   1. `attachSession` falls back to `ctx.fs` when the host's realpath fails:
 *      `resolve()` then `stat()`, accepting the target when it is a directory.
 *   2. The registry's narrow `host` view gains `fs`, because that view is what a
 *      `Workspace` receives — it has no ctx of its own.
 *   3. `fs` joins the registry's `static inject`. Cordis refuses `ctx.fs` otherwise
 *      ("cannot get property \"fs\" without inject"), which is how the requirement
 *      was found rather than assumed.
 *
 * # Safety
 *
 * Every anchor must appear EXACTLY ONCE, the patched source is compiled with
 * `node --check` BEFORE the original is replaced, and anything unexpected aborts
 * with exit 1 leaving the file untouched. Re-running is a no-op. This is the same
 * shape as this deployment's other DSH patches (`~/patch-dsh-*.sh`).
 *
 * # Usage
 *
 *     node dsh-workspace-registry.mjs [--dsh <dir>]
 *
 * `<dir>` defaults to `D:\Summrise\components\dsh` (the device's staged DSH).
 */

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, readFileSync, unlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const argDir = (() => {
  const i = process.argv.indexOf('--dsh');
  return i >= 0 ? process.argv[i + 1] : 'D:\\Summrise\\components\\dsh';
})();

const FILE = join(argDir, 'node_modules', '@deepseek-ai', 'dsh-workspace', 'lib', 'index.js');
const MARKER = 'SUMMRISE_REMOTE_WORKSPACE_PATCH';

/** Replace `anchor` with `replacement`, insisting the anchor is unique. */
function replaceOnce(source, anchor, replacement, what) {
  const count = source.split(anchor).length - 1;
  if (count !== 1) {
    console.error(`ANCHOR "${what}" matched ${count} times (expected 1) -- aborting, file untouched`);
    process.exit(1);
  }
  return source.replace(anchor, replacement);
}

if (!existsSync(FILE)) {
  console.error(`not found: ${FILE} -- pass --dsh <dir> pointing at the staged DSH`);
  process.exit(1);
}

let source = readFileSync(FILE, 'utf8');
if (source.includes(MARKER)) {
  console.log('already patched');
  process.exit(0);
}

// 1. the fallback inside attachSession — the WHOLE validation block, because the
//    original `throw` and its closing brace are part of what is being replaced
const attachAnchor = [
  '\t\t\tlet cwd;',
  '\t\t\ttry {',
  '\t\t\t\tcwd = await realpathNormalize(header.cwd);',
  '\t\t\t} catch (error) {',
  '\t\t\t\tthrow new Error(`cannot attach session \'${sessionId}\' to workspace \'${this.record.path}\': its cwd \'${header.cwd}\' does not resolve, so it cannot be validated`, { cause: error });',
  '\t\t\t}',
  '\t\t\tif (!(await stat(cwd)).isDirectory()) throw new Error(`cannot attach session \'${sessionId}\' to workspace \'${this.record.path}\': its cwd \'${header.cwd}\' is not a directory`);',
].join('\n');
source = replaceOnce(
  source,
  attachAnchor,
  [
    '\t\t\t// SUMMRISE_REMOTE_WORKSPACE_PATCH: a workspace host is not this host.',
    '\t\t\tlet cwd;',
    '\t\t\tlet viaSeam = false;',
    '\t\t\ttry {',
    '\t\t\t\tcwd = await realpathNormalize(header.cwd);',
    '\t\t\t} catch (localError) {',
    '\t\t\t\tconst seam = typeof this.host.fs === "function" ? this.host.fs() : void 0;',
    '\t\t\t\tif (!seam) throw new Error(`cannot attach session \'${sessionId}\' to workspace \'${this.record.path}\': its cwd \'${header.cwd}\' does not resolve, so it cannot be validated`, { cause: localError });',
    '\t\t\t\tlet target;',
    '\t\t\t\ttry { target = await seam.resolve(header.cwd); } catch (error) {',
    '\t\t\t\t\tthrow new Error(`cannot attach session \'${sessionId}\' to workspace \'${this.record.path}\': its cwd \'${header.cwd}\' does not resolve, so it cannot be validated`, { cause: localError });',
    '\t\t\t\t}',
    '\t\t\t\tconst info = await seam.stat(target);',
    '\t\t\t\tif (!info || info.type !== "directory") throw new Error(`cannot attach session \'${sessionId}\' to workspace \'${this.record.path}\': its cwd \'${header.cwd}\' is not a directory`);',
    '\t\t\t\tcwd = target.targetKey;',
    '\t\t\t\tviaSeam = true;',
    '\t\t\t}',
    '\t\t\tif (!viaSeam && !(await stat(cwd)).isDirectory()) throw new Error(`cannot attach session \'${sessionId}\' to workspace \'${this.record.path}\': its cwd \'${header.cwd}\' is not a directory`);',
  ].join('\n'),
  'attachSession cwd validation',
);

// 2. the narrow host view a Workspace receives gains the seam
source = replaceOnce(
  source,
  '\t\trememberSessionPath: (id, path) => {\n\t\t\tthis.sessionPaths.set(id, path);\n\t\t\tthis.invalidSessionPaths.delete(id);\n\t\t}\n\t};',
  '\t\trememberSessionPath: (id, path) => {\n\t\t\tthis.sessionPaths.set(id, path);\n\t\t\tthis.invalidSessionPaths.delete(id);\n\t\t},\n\t\t// SUMMRISE_REMOTE_WORKSPACE_PATCH: the fs seam. Lazy on purpose -- the field\n\t\t// initializer runs after super(ctx), and reading it late keeps that independent.\n\t\tfs: () => this.ctx.fs\n\t};',
  'registry host view',
);

// 3. and it must be INJECTED, not merely read
source = replaceOnce(
  source,
  'static inject = ["storageDomain", "sessionPersistence"];',
  '// SUMMRISE_REMOTE_WORKSPACE_PATCH: cordis refuses ctx.fs without it --\n\t// "cannot get property \\"fs\\" without inject".\n\tstatic inject = ["storageDomain", "sessionPersistence", "fs"];',
  'registry inject list',
);

// compile BEFORE replacing, with a .js name: the package is type:module and
// `node --check` refuses an unknown extension (that refusal is why this line exists)
const tmp = FILE.replace(/index\.js$/, 'index.summrise-patched.js');
writeFileSync(tmp, source);
try {
  execFileSync(process.execPath, ['--check', tmp], { stdio: 'pipe' });
} catch (error) {
  console.error(`node --check FAILED -- aborting, file untouched: ${String(error.stderr || error.message).slice(0, 400)}`);
  unlinkSync(tmp);
  process.exit(1);
}
copyFileSync(FILE, `${FILE}.bak-before-remote-workspace`);
writeFileSync(FILE, source);
console.log(`patched ${FILE}`);
