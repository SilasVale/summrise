/**
 * The `ctx.fs` provider backed by a summrise agent.
 *
 * # What it is for
 *
 * A DSH running on one machine, with its workspace on another. Without this, the workspace is
 * SPLIT-BRAINED: `glob` and `grep` go through the subprocess seam and hit the workspace host, while
 * `read` and `edit` go through `ctx.fs` and hit the machine the DSH runs on — so a search finds a
 * file the harness then cannot open. This is the other half.
 *
 * # Mounting it
 *
 * One implementation per service, so the stock provider goes:
 *
 * ```yaml
 * - id: fs
 *   disabled: true
 * ```
 *
 * # What it does NOT do yet
 *
 * WRITES. `writeText` and `editText` throw rather than half-work. A write needs three things this
 * backend does not have: a content version to guard staleness (the one `stat` can afford is a
 * metadata token — size and mtime — and a guard built on it would reject and accept the wrong
 * things), an atomic publish, and the per-path serialization that keeps the guard from being
 * decoration. A backend that wrote without them would corrupt files quietly, which is worse than one
 * that says it cannot.
 *
 * `watch` throws too, which the seam allows: *"unsupported providers reject without polling."* A
 * watcher that silently degraded to polling would be a caller believing it had invalidations it
 * does not have.
 */

import { FileSystem, FsError, FsTargetKey, FsVersion as makeVersion } from '@deepseek-ai/dsh-fs';

import {
  WorkspaceFsError,
  fileUrlFor,
  fsOnAgent,
  normalizePath,
  pathContains,
  targetFor,
} from './fs-transport.js';

/** How a DSH deployment names this provider in a profile. */
export const name = 'summrise-workspace-fs';

/** The chunk `streamText` reads at a time: large enough to be worth a round trip, small enough to
 * keep a window rather than a file in memory. */
const STREAM_CHUNK = 256 * 1024;

/** The cap a whole-file read gets when the caller names none. */
const DEFAULT_READ_CAP = 8 * 1024 * 1024;

function resolveConfig(config) {
  const env = typeof process !== 'undefined' ? process.env ?? {} : {};
  const port = config.port ?? (env.SUMMRISE_WORKSPACE_PORT ? Number(env.SUMMRISE_WORKSPACE_PORT) : 22);
  return {
    endpoint: config.endpoint ?? env.SUMMRISE_WORKSPACE_ENDPOINT ?? undefined,
    token: config.token ?? env.SUMMRISE_WORKSPACE_TOKEN,
    host: config.host ?? env.SUMMRISE_WORKSPACE_HOST,
    user: config.user ?? env.SUMMRISE_WORKSPACE_USER,
    port: Number.isFinite(port) ? port : 22,
    password: config.password ?? env.SUMMRISE_WORKSPACE_PASSWORD,
    keyPath: config.keyPath ?? config.key_path ?? env.SUMMRISE_WORKSPACE_KEY_PATH,
    /** The directory a relative path resolves against. */
    root: config.root ?? env.SUMMRISE_WORKSPACE_ROOT ?? '/',
  };
}

/** Turn the transport's refusal into the seam's typed error, keeping the CODE. */
function asFsError(cause) {
  if (cause instanceof WorkspaceFsError) return new FsError(cause.message, cause.code, { cause });
  return cause;
}

export default class SummriseWorkspaceFileSystem extends FileSystem {
  __config;

  constructor(ctx, config = {}) {
    super(ctx);
    this.__config = resolveConfig(config ?? {});
  }

  /** The connection facts, for a diagnostic that must not print the password. */
  describe() {
    const { endpoint, host, user, port, root } = this.__config;
    return { endpoint, host, user, port, root };
  }

  __requireHost() {
    const { host, user } = this.__config;
    if (!host || !user) {
      throw new FsError(
        'summrise-workspace-fs has no workspace host: set `host` and `user` in the profile entry\'s config, or SUMMRISE_WORKSPACE_HOST / SUMMRISE_WORKSPACE_USER',
        'FS_IO_ERROR',
      );
    }
  }

  /** The path a target names. The key IS the path — see `targetFor`. */
  __pathOf(target) {
    if (typeof target === 'string') return normalizePath(target, this.__config.root);
    if (target && typeof target.targetKey === 'string') return target.targetKey;
    throw new FsError('a resolved target is required', 'FS_IO_ERROR');
  }

  async __ask(op, path, extra) {
    this.__requireHost();
    try {
      return await fsOnAgent({
        endpoint: this.__config.endpoint,
        token: this.__config.token,
        host: this.__config.host,
        user: this.__config.user,
        port: this.__config.port,
        password: this.__config.password,
        keyPath: this.__config.keyPath,
        op,
        path,
        ...extra,
      });
    } catch (cause) {
      throw asFsError(cause);
    }
  }

  /** The seam's type vocabulary, from the host's. `symlink` belongs to `lstat` alone. */
  static __seamType(kind) {
    if (kind === 'file') return 'file';
    if (kind === 'directory') return 'directory';
    return 'other';
  }

  /**
   * A metadata freshness token, and it is NOT a content hash.
   *
   * `stat` cannot afford one — hashing means reading the file — so this is size and mtime. It is
   * enough to notice the changes `stat` exists to notice, and it is deliberately NOT what a write
   * guard should use: a rebuild that lands the same size inside the same second would pass it. The
   * content version the agent computes on a read is `sha256:…`.
   */
  static __metadataVersion(info) {
    return makeVersion(`stat:${info?.size ?? 0}:${info?.mtime ?? 0}`);
  }

  // ── the pure operations: no round trip, which is what keeps every other call to one ──────────

  async resolve(path, opts) {
    const target = targetFor(path, opts?.cwd ?? this.__config.root);
    return { targetKey: FsTargetKey(target.targetKey), displayPath: target.displayPath };
  }

  processPath(target) {
    return this.__pathOf(target);
  }

  fileUrl(target) {
    return fileUrlFor(this.__pathOf(target));
  }

  contains(parent, child) {
    return pathContains(this.__pathOf(parent), this.__pathOf(child));
  }

  /**
   * Absent, and the seam's own words for it: a backend that never confines reports `undefined`, which
   * is the honest fact the tool layer reads to advertise its escalation fields.
   */
  get sandboxMode() {
    return undefined;
  }

  // ── one round trip each ──────────────────────────────────────────────────────────────────────

  async stat(target, signal) {
    const answer = await this.__ask('stat', this.__pathOf(target), { signal });
    if (answer.info === null || answer.info === undefined) return undefined;
    const info = answer.info;
    return {
      version: SummriseWorkspaceFileSystem.__metadataVersion(info),
      type: SummriseWorkspaceFileSystem.__seamType(info.kind),
      size: info.kind === 'file' ? info.size : undefined,
    };
  }

  async lstat(path, opts, signal) {
    const answer = await this.__ask('lstat', normalizePath(path, opts?.cwd ?? this.__config.root), {
      signal,
    });
    if (answer.info === null || answer.info === undefined) return undefined;
    const info = answer.info;
    return {
      version: SummriseWorkspaceFileSystem.__metadataVersion(info),
      // `symlink` is what lstat exists to report: a consumer with a trust-boundary rule rejects the
      // PATH before `resolve` follows it.
      type: info.kind === 'symlink' ? 'symlink' : SummriseWorkspaceFileSystem.__seamType(info.kind),
      size: info.kind === 'file' ? info.size : undefined,
    };
  }

  async readText(target, signal) {
    const answer = await this.__ask('readText', this.__pathOf(target), {
      signal,
      max_bytes: DEFAULT_READ_CAP,
    });
    return answer.text;
  }

  async readBytes(target, signal, maxBytes) {
    const answer = await this.__ask('readBytes', this.__pathOf(target), {
      signal,
      offset: 0,
      length: maxBytes,
    });
    return Uint8Array.from(Buffer.from(answer.bytes_b64 ?? '', 'base64'));
  }

  async readByteRange(target, range, signal) {
    const answer = await this.__ask('readBytes', this.__pathOf(target), {
      signal,
      offset: range.offset,
      length: range.length,
    });
    return Uint8Array.from(Buffer.from(answer.bytes_b64 ?? '', 'base64'));
  }

  /**
   * A file's text, chunk by chunk.
   *
   * A WINDOW, NOT A FILE: each chunk is one `readBytes` at an offset, so a large file never has more
   * than one chunk of itself in this process. The loop stops when a chunk comes back shorter than it
   * asked for, which is what end-of-file looks like from here.
   */
  async *streamText(target, signal) {
    const path = this.__pathOf(target);
    let offset = 0;
    for (;;) {
      const answer = await this.__ask('readBytes', path, {
        signal,
        offset,
        length: STREAM_CHUNK,
      });
      const chunk = Buffer.from(answer.bytes_b64 ?? '', 'base64');
      if (chunk.length > 0) {
        // Decoded per chunk, as the seam requires: the BACKEND owns cross-chunk UTF-8 decoding, and
        // a multi-byte character straddling a boundary must not become two replacement characters.
        yield chunk.toString('utf8');
        offset += chunk.length;
      }
      if (chunk.length < STREAM_CHUNK) return;
    }
  }

  async listDir(target, signal) {
    const path = this.__pathOf(target);
    const answer = await this.__ask('listDir', path, { signal });
    const entries = Array.isArray(answer.entries) ? answer.entries : [];
    return entries.map((entry) => {
      const child = normalizePath(`${path}/${entry.name}`, '/');
      return {
        name: entry.name,
        type: SummriseWorkspaceFileSystem.__seamType(entry.kind),
        target: { targetKey: FsTargetKey(child), displayPath: child },
        version: SummriseWorkspaceFileSystem.__metadataVersion(entry),
        size: entry.kind === 'file' ? entry.size : undefined,
      };
    });
  }

  // ── refused, by name ─────────────────────────────────────────────────────────────────────────

  async writeText() {
    throw new FsError(
      'this workspace is read-only: summrise-workspace-fs does not implement writeText yet, and a write without a content version guard, an atomic publish and a per-path lock would corrupt files quietly',
      'FS_IO_ERROR',
    );
  }

  async editText() {
    throw new FsError(
      'this workspace is read-only: summrise-workspace-fs does not implement editText yet, and an edit without a content version guard would apply to a file that changed under it',
      'FS_IO_ERROR',
    );
  }

  async watch() {
    // The seam: "unsupported providers reject without polling." Silently degrading to a poll would
    // be a caller believing it had invalidations it does not have.
    throw new FsError(
      'summrise-workspace-fs does not implement watch: this backend has no invalidation channel for a remote host, and it will not pretend a poll is one',
      'FS_IO_ERROR',
    );
  }
}
