#!/usr/bin/env node
/**
 * Stop the session controller from `mkdir`-ing the workspace path on the HOST.
 *
 * # The wall this removes (measured 2026-09-30, on the official providers)
 *
 * `dsh-api-session-controller` ensures the session's cwd exists before composing the agent:
 *
 *     try {
 *       await mkdir(cwd, { recursive: true });        // lib/index.js:445 — node:fs/promises, the HOST's disk
 *     } catch (error) {
 *       throw new Error(`failed to ensure project directory "${cwd}": ${String(error)}`, { cause: error });
 *     }
 *
 * With the official remote providers the cwd names a directory on ANOTHER machine, so on `zss`:
 *
 *     session/create {workspaceId: …}
 *       -> gateway/internal: failed to create session …: failed to ensure project directory
 *          "/home/zhengsaisi/summrise": Error: EACCES: permission denied, mkdir '/home…'
 *
 * # What it changes
 *
 * The call is a defensive ensure, not a request: the workspace registry has already proved the directory
 * exists, and the fs seam is the thing that knows WHERE. So the check moves to the seam —
 * `ctx.fs.resolve()` then `ctx.fs.stat()` — and the host `mkdir` runs only when the seam does not report a
 * directory. A profile with no `ctx.fs` keeps exactly the old behaviour, and a genuinely missing LOCAL
 * directory is still created.
 *
 * The seam has no directory creation (the workspace door is read-only), so a MISSING remote directory is
 * still a failure — but it is now a failure with the right cause: the path the workspace names is gone,
 * which is worth saying rather than papering over with a host-side mkdir that cannot possibly help.
 *
 * # Usage
 *
 *     node dsh-session-cwd.mjs <path to dsh-api-session-controller/lib/index.js>
 *     node dsh-session-cwd.mjs --dsh <DSH install dir>
 */
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

function resolveTarget(argv) {
  const flag = argv.indexOf('--dsh');
  if (flag >= 0 && argv[flag + 1] !== undefined) return join(argv[flag + 1], 'node_modules/@deepseek-ai/dsh-api-session-controller/lib/index.js');
  return argv[2];
}

const TARGET = resolveTarget(process.argv);
if (TARGET === undefined) {
  console.error('usage: node dsh-session-cwd.mjs <dsh-api-session-controller/lib/index.js>');
  console.error('       node dsh-session-cwd.mjs --dsh <DSH install dir>');
  process.exit(2);
}
if (!existsSync(TARGET)) {
  console.error(`REFUSED: no such file: ${TARGET}`);
  process.exit(2);
}

const original = readFileSync(TARGET, 'utf8');

/** Only the patched file carries this. */
const MARKER = 'PATCHED: the cwd is ensured through the fs seam';
if (original.includes(MARKER)) {
  console.log('already patched');
  process.exit(0);
}

const FROM = `\t\t\tawait mkdir(cwd, { recursive: true });`;
const TO = `\t\t\t// ${MARKER}. The registry already proved this directory exists; the seam is what knows where,
\t\t\t// and mkdir here would run on the harness host — where a remote workspace's path does not exist
\t\t\t// (measured: EACCES mkdir '/home...' on zss). The host mkdir stays for the local case.
\t\t\tconst seamFs = this.ctx.fs;
\t\t\tconst cwdIsDirectory = seamFs === void 0 ? false : (await seamFs.stat(await seamFs.resolve(cwd)))?.type === "directory";
\t\t\tif (!cwdIsDirectory) await mkdir(cwd, { recursive: true });`;

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
console.log(`  ${MARKER}`);
console.log(`  bytes: ${original.length} -> ${patched.length}`);
