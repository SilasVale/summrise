/**
 * The filesystem transport: ask a summrise agent about a workspace host's files.
 *
 * THIS FILE IMPORTS NOTHING FROM DSH, for the same reason its subprocess sibling does not — the
 * decisions are testable without a DSH install, and `fs.js` is the thin class that wires them in.
 *
 * # The codes travel as codes
 *
 * The agent answers a filesystem failure with one of `dsh-fs`'s own `FsErrorCode` tokens
 * (`FS_TOO_LARGE`, `FS_NOT_TEXT`, `FS_NOT_FOUND`, …). This module CARRIES that token rather than
 * re-deriving one from the message: the seam requires callers to branch on the code, and a
 * translation layer that re-read the prose would undo the thing the agent was changed to guarantee.
 */

import { DEFAULT_ENDPOINT } from './transport.js';

/** Raised when the agent refuses a filesystem operation. `code` is a `dsh-fs` `FsErrorCode`. */
export class WorkspaceFsError extends Error {
  constructor(message, code) {
    super(message);
    this.name = 'WorkspaceFsError';
    this.code = code;
  }
}

/**
 * One filesystem operation on a workspace host.
 *
 * @param options - the connection, the `op`, the path, and whatever that op needs.
 * @returns the agent's answer, unshaped — each op reads the fields it asked for.
 */
export async function fsOnAgent(options) {
  const {
    endpoint = DEFAULT_ENDPOINT,
    token,
    host,
    user,
    port = 22,
    password,
    keyPath,
    op,
    path,
    fetchImpl = globalThis.fetch,
    signal,
    ...rest
  } = options;

  const body = { op, path, host, user, port };
  if (password) body.password = password;
  if (keyPath) body.key_path = keyPath;
  // Whatever else the op needs (`max_bytes`, `offset`, `length`) rides along verbatim: this layer
  // does not decide what an op accepts, because a second copy of that list is a second thing to
  // keep in step with the route.
  for (const [key, value] of Object.entries(rest)) {
    if (value !== undefined) body[key] = value;
  }

  const headers = { 'content-type': 'application/json' };
  if (token) headers.authorization = `Bearer ${token}`;

  let response;
  try {
    response = await fetchImpl(`${endpoint.replace(/\/+$/, '')}/api/workspace/fs`, {
      method: 'POST',
      headers,
      body: JSON.stringify(body),
      signal,
    });
  } catch (cause) {
    if (signal?.aborted) throw signal.reason ?? cause;
    throw new WorkspaceFsError(
      `could not reach the agent at ${endpoint}: ${cause?.message ?? cause}`,
      'FS_IO_ERROR',
    );
  }

  if (!response.ok) {
    throw new WorkspaceFsError(
      `the agent answered HTTP ${response.status} for /api/workspace/fs`,
      'FS_IO_ERROR',
    );
  }

  let answer;
  try {
    answer = await response.json();
  } catch (cause) {
    throw new WorkspaceFsError(
      `the agent's answer was not JSON: ${cause?.message ?? cause}`,
      'FS_IO_ERROR',
    );
  }
  if (answer?.ok !== true) {
    throw new WorkspaceFsError(
      typeof answer?.error === 'string' ? answer.error : 'the agent refused the operation',
      typeof answer?.code === 'string' ? answer.code : 'FS_IO_ERROR',
    );
  }
  return answer;
}

/**
 * Normalise a path into the one spelling this backend uses as a target key.
 *
 * PURE ARITHMETIC, AND THAT IS THE POINT. The seam warns that "resolve-then-operate costs a remote
 * backend two round trips per tool call", and resolution is the one step that does not need one: a
 * POSIX path has exactly one spelling, so `resolve` here costs nothing and every other operation
 * costs exactly one round trip.
 *
 * The rules are POSIX's, and only the ones a workspace needs: `.` segments go, `..` segments pop,
 * repeated slashes collapse, and a relative path is resolved against the base.
 *
 * `..` AT THE ROOT IS THE ROOT, because that is what the host does with it. An earlier version of
 * this kept the `..` in the output, reasoning that inventing a different path would be worse than
 * passing one on — and the reasoning was backwards: `resolve('/..')` would then answer `/..` while
 * READING `/..` reads the root, so the target key would name a file it is not. The seam requires the
 * same file to yield the same key, and the only spelling that means what the caller meant is `/`.
 *
 * @param path - the path to normalise.
 * @param base - the directory a relative path is resolved against.
 */
export function normalizePath(path, base) {
  if (typeof path !== 'string' || path.length === 0) {
    throw new WorkspaceFsError('a path is required', 'FS_IO_ERROR');
  }
  // A WINDOWS-SPELLED ABSOLUTE PATH NAMES THE SAME FILE, and this backend has to read it that way.
  // The DSH that drives this provider is on Windows while the workspace is POSIX, so the host's own
  // layers — and the model, which is told the platform — write an absolute path as
  // `D:\home\zhengsaisi\summrise`. Measured 2026-09-30 in a live session on desktop-14rjcr8: every
  // `resolve()` arrived in that spelling, so a backend that insisted on the leading slash answered
  // `/D:\home\...`, which is not a path on any machine. Stripping the drive and flipping the
  // separators is the whole translation: the drive letter carries no information here, because the
  // workspace root is on the far side of the seam and only one filesystem is addressable.
  const windowsAbsolute = /^[A-Za-z]:[\\/]/.test(path);
  const corrected = windowsAbsolute ? path.slice(2).replace(/\\/g, '/') : path;
  const absolute = corrected.startsWith('/') ? corrected : `${base ?? '/'}/${corrected}`;
  const out = [];
  for (const segment of absolute.split('/')) {
    if (segment === '' || segment === '.') continue;
    if (segment === '..') {
      // At the root there is nowhere to go, and that is POSIX's answer rather than this layer's
      // choice: the host resolves `/..` to `/`, so any other spelling here would be a target key
      // that does not name the file it is used to open.
      if (out.length > 0) out.pop();
      continue;
    }
    out.push(segment);
  }
  const joined = `/${out.join('/')}`;
  // A trailing slash survives only as the root itself; every other path has one spelling.
  return joined === '/' ? '/' : joined.replace(/\/+$/, '');
}

/**
 * Whether `child` is `parent` or something inside it.
 *
 * PURE, and segment-wise rather than a string prefix: `/work/a` must not be said to contain
 * `/work/ab`, which a `startsWith` would claim.
 */
export function pathContains(parent, child) {
  if (parent === child) return true;
  const base = parent === '/' ? '/' : `${parent}/`;
  return child.startsWith(base);
}

/**
 * The `file:` URI for a workspace path.
 *
 * Encoded per segment so a name with a space or a `#` survives, while the separators stay
 * separators — which is what makes the result a URI rather than a string that looks like one.
 */
export function fileUrlFor(path) {
  const encoded = path
    .split('/')
    .map((segment) => encodeURIComponent(segment))
    .join('/');
  return `file://${encoded}`;
}

/**
 * Split an incoming path into the two shapes the seam distinguishes.
 *
 * @returns the target key and the path to display, which are the same string here — a workspace path
 *   is absolute, stable and readable, so there is nothing to hide behind an opaque key.
 */
export function targetFor(path, base) {
  const normalized = normalizePath(path, base);
  return { targetKey: normalized, displayPath: normalized };
}
