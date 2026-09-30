/**
 * The `ctx.subprocess` provider backed by a summrise agent.
 *
 * # What it is for
 *
 * A DSH running on one machine, with its workspace on another. `glob`, `grep` and anything else that
 * spawns a process then run **on the workspace host**, over the agent's SSH connection — so the code
 * that is searched is the code that is there, and the ripgrep that searches it is the host's own.
 *
 * # It is one implementation of a service the host plane holds ONE of
 *
 * Mounting this means the stock provider must go, in `$DSH_HOME/profiles/<profile>/cordis.patch.yml`:
 *
 * ```yaml
 * - id: subprocess
 *   disabled: true
 * ```
 *
 * # The seam's promise, and who keeps it
 *
 * The seam guarantees no shell layer: every argv element is passed as an unquoted value. This class
 * keeps that by **handing the argv to the agent unchanged** — it does no quoting, no joining, no
 * trimming. The agent carries the argv and the working directory in a frame on the SSH channel's
 * stdin, because the exec string `sshd` receives goes to the login shell. Neither side may start
 * being clever about the other's values.
 */

import {
  SubprocessExecutableNotFoundError,
  SubprocessRuntime,
} from '@deepseek-ai/dsh-subprocess';

import {
  DEFAULT_ENDPOINT,
  DEFAULT_HELPER,
  UnsupportedStdioError,
  assertSupportedStdio,
  connectionFields,
  execOnAgent,
  registerMachineRoot,
  makeCollectedReader,
  normalizeOutcome,
} from './transport.js';

export { UnsupportedStdioError } from './transport.js';

/** How a DSH deployment names this provider in a profile. */
export const name = 'summrise-workspace';

/**
 * Read the connection this provider runs commands on.
 *
 * The constructor argument is the profile entry's `config:`; the environment is the fallback,
 * because a provider whose only configuration path is a file the caller has not edited yet is a
 * provider that reports "no host" instead of working. The environmental spelling is documented here
 * rather than discovered: `SUMMRISE_WORKSPACE_HOST`, `..._USER`, `..._PORT`, `..._PASSWORD`,
 * `..._KEY_PATH`, `..._ENDPOINT`, `..._TOKEN`, `..._HELPER`.
 */
function resolveConfig(config) {
  const env = typeof process !== 'undefined' ? process.env ?? {} : {};
  const port = config.port ?? (env.SUMMRISE_WORKSPACE_PORT ? Number(env.SUMMRISE_WORKSPACE_PORT) : 22);
  return {
    endpoint: config.endpoint ?? env.SUMMRISE_WORKSPACE_ENDPOINT ?? DEFAULT_ENDPOINT,
    token: config.token ?? env.SUMMRISE_WORKSPACE_TOKEN,
    host: config.host ?? env.SUMMRISE_WORKSPACE_HOST,
    user: config.user ?? env.SUMMRISE_WORKSPACE_USER,
    port: Number.isFinite(port) ? port : 22,
    password: config.password ?? env.SUMMRISE_WORKSPACE_PASSWORD,
    keyPath: config.keyPath ?? config.key_path ?? env.SUMMRISE_WORKSPACE_KEY_PATH,
    helper: config.helper ?? env.SUMMRISE_WORKSPACE_HELPER ?? DEFAULT_HELPER,
    // THE ROOT A LOOKUP RUNS IN. On a delegated provider (no host configured) the agent resolves a request's
    // PATH against its mapping store, so a request that carries no path at all is refused
    // `workspace/unknown-path` — which is what every executable lookup did, because `resolveExecutable` passed
    // `undefined` as its cwd. `/` is the honest default: it is what "add this machine" registers, and a provider
    // pinned to a subtree should say so here.
    root: config.root ?? env.SUMMRISE_WORKSPACE_ROOT ?? '/',
    timeoutMs: config.timeoutMs ?? config.timeout_ms,
  };
}

export default class SummriseWorkspaceRuntime extends SubprocessRuntime {
  /** @type {ReturnType<typeof resolveConfig>} */
  __config;

  constructor(ctx, config = {}) {
    super(ctx);
    this.__config = resolveConfig(config ?? {});
  }

  /** The connection facts, for a diagnostic that must not print the password. */
  describe() {
    const { endpoint, host, user, port } = this.__config;
    return { endpoint, host, user, port };
  }

  /**
   * THE SAME RULE THE TRANSPORT APPLIES, asked here so a misconfiguration fails before a request is
   * built. NO host at all is not a misconfiguration any more: it delegates the choice to the agent's
   * mapping store, which is what lets one provider serve workspaces on several hosts.
   */
  __requireHost() {
    connectionFields(this.__config);
  }

  /**
   * THE MOMENT "THE MACHINE" IS ADDED, once per provider instance (see `machineRootMapping`): a provider that was
   * configured with a host registers that machine's root, so the paths a picker browses resolve. A delegated
   * provider registers nothing, and the registry decides — which is what a registry is for.
   */
  async __ensureMachineRoot() {
    if (this.__rootDone) return;
    this.__rootDone = true;
    await registerMachineRoot({
      endpoint: this.__config.endpoint,
      token: this.__config.token,
      host: this.__config.host,
      user: this.__config.user,
      port: this.__config.port,
    });
  }

  async __request(argv, cwd, stdoutCap, stderrCap, signal) {
    this.__requireHost();
    await this.__ensureMachineRoot();
    return execOnAgent({
      endpoint: this.__config.endpoint,
      token: this.__config.token,
      host: this.__config.host,
      user: this.__config.user,
      port: this.__config.port,
      password: this.__config.password,
      keyPath: this.__config.keyPath,
      helper: this.__config.helper,
      timeoutMs: this.__config.timeoutMs,
      argv,
      cwd,
      stdoutCap,
      stderrCap,
      signal,
    });
  }

  /**
   * Look the command up ON THE WORKSPACE HOST.
   *
   * The lookup runs `command -v -- "$1"` with the name as a POSITIONAL PARAMETER, never interpolated
   * into the script text — the script is a constant and the value is data. That is the one place this
   * provider involves a shell at all, and it is a lookup rather than an execution: the command the
   * caller eventually runs still crosses as a frame.
   */
  async resolveExecutable(command, _env, signal) {
    if (typeof command !== 'string' || command.length === 0) {
      throw new SubprocessExecutableNotFoundError('no command was given to resolve');
    }
    if (command.includes('/')) {
      // A caller that named a path has already resolved it; the target will agree or fail at exec.
      return command;
    }
    let answer;
    try {
      answer = await this.__request(
        ['/bin/sh', '-c', 'command -v -- "$1"', 'sh', command],
        // THE LOOKUP NEEDS A PATH OF ITS OWN (see `root` above): without one, a delegated provider refuses every
        // lookup before the shell is ever reached.
        this.__config.root,
        4096,
        4096,
        signal,
      );
    } catch (cause) {
      throw new SubprocessExecutableNotFoundError(
        `could not look up ${JSON.stringify(command)} on the workspace host: ${cause?.message ?? cause}`,
      );
    }
    const found = answer.stdout.toString('utf8').trim();
    if (found.length === 0) {
      throw new SubprocessExecutableNotFoundError(
        `${JSON.stringify(command)} was not found on the workspace host`,
      );
    }
    // `command -v` may answer with a shell builtin's name; a path is what the seam wants.
    return found.includes('/') ? found.split('\n')[0] : command;
  }

  /**
   * The workspace host's shell facts.
   *
   * Reported as POSIX, because that is what the staged helper is: a POSIX `execve` shim. A host that
   * cannot run it is a host this provider cannot drive, and saying so here is better than a spawn
   * failing later with a missing-file error nobody can place.
   */
  async terminalEnvironment(_signal) {
    return { platform: 'linux', defaultShell: '/bin/sh' };
  }

  /**
   * Run one argv on the workspace host.
   *
   * The handle is returned SYNCHRONOUSLY and `done` settles later, which is the seam's shape: a spawn
   * failure and a command failure are different events and only the second is an outcome.
   */
  spawn(spec) {
    assertSupportedStdio(spec);

    const controller = new AbortController();
    const forwardAbort = () => controller.abort(spec.signal?.reason);
    if (spec.signal) {
      if (spec.signal.aborted) controller.abort(spec.signal.reason);
      else spec.signal.addEventListener('abort', forwardAbort, { once: true });
    }

    // THE READERS EXIST BEFORE THE COMMAND DOES, and that is not a nicety: a consumer takes
    // `handle.collected` the moment `spawn` returns — `dsh-bash-local` reads `collected.stdout`
    // straight into the `observed` streams it hands its caller — and a handle whose
    // `collected.stdout` is `undefined` is reported as *the implementation dropped a requested
    // collect stream*. Measured 2026-09-30 on desktop-14rjcr8: that sentence is what EVERY `bash`
    // call answered, because the first version of this file filled `collected` only after `done`
    // settled. The bytes do arrive later; the READER is here now, and it answers empty until they do.
    const buffers = {
      stdout: Buffer.alloc(0),
      stderr: Buffer.alloc(0),
      stdoutOverflow: false,
      stderrOverflow: false,
    };
    const readerOver = (bytes, overflow) => ({
      readFrom: (fromByte) => makeCollectedReader(bytes(), overflow()).readFrom(fromByte),
    });
    const collected = {
      stdout: readerOver(
        () => buffers.stdout,
        () => buffers.stdoutOverflow,
      ),
      stderr: readerOver(
        () => buffers.stderr,
        () => buffers.stderrOverflow,
      ),
    };
    const done = (async () => {
      try {
        const answer = await this.__request(
          [...spec.argv],
          spec.cwd,
          spec.stdio.stdout.maxBytes,
          spec.stdio.stderr.maxBytes,
          controller.signal,
        );
        buffers.stdout = answer.stdout;
        buffers.stderr = answer.stderr;
        buffers.stdoutOverflow = answer.stdoutOverflow;
        buffers.stderrOverflow = answer.stderrOverflow;
        return normalizeOutcome(answer.status);
      } finally {
        spec.signal?.removeEventListener?.('abort', forwardAbort);
      }
    })();

    return {
      stdin: undefined,
      stdout: undefined,
      stderr: undefined,
      control: undefined,
      collected,
      done,
      // The seam's only termination verb. It aborts the request, which closes the connection, which
      // is what makes the agent's handler drop its channel, which is what makes the far side
      // terminate. Idempotent by construction: aborting an aborted controller is a no-op.
      terminate: () => controller.abort(new Error('terminated by the caller')),
      waitForExit: async (signal) => {
        if (!signal) {
          await done.catch(() => {});
          return true;
        }
        return await Promise.race([
          done.then(
            () => true,
            () => true,
          ),
          new Promise((resolve) => {
            if (signal.aborted) resolve(false);
            else signal.addEventListener('abort', () => resolve(false), { once: true });
          }),
        ]);
      },
    };
  }

  /**
   * Not implemented, and it says so rather than pretending.
   *
   * A terminal spawn is an INTERACTIVE pty with foreground signalling — `signalForeground`, activity
   * tracking, a resizable window. The agent's workspace route is a one-shot request/response, and a
   * pty is what the agent's separate terminal surface is for; wiring this one through that surface
   * is a piece of work, not a default.
   */
  async spawnTerminal() {
    throw new UnsupportedStdioError(
      'spawnTerminal is not implemented by summrise-workspace: a pty needs the agent’s terminal surface, not its one-shot workspace route',
    );
  }
}
