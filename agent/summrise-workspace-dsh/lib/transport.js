/**
 * The transport half of the workspace subprocess seam: talk to a summrise agent, and shape what
 * comes back into the two things the seam reads — offset-based output readers and an outcome.
 *
 * THIS FILE IMPORTS NOTHING FROM DSH, deliberately. The seam's own types live in a package this one
 * peers with, so a module that imported them could not be tested without a DSH install; everything
 * with a decision in it lives here instead, and `index.js` is the thin class that wires it in.
 *
 * # What this is NOT allowed to do
 *
 * The seam promises that no shell layer exists — every argv element is passed through unquoted. That
 * promise is kept by the AGENT, which puts the argv and the working directory in a frame on the
 * SSH channel's stdin rather than in the exec string. This module's whole job is to hand the agent
 * the argv faithfully: it does no quoting, no joining, and no trimming.
 */

/** The agent's default loopback address. */
export const DEFAULT_ENDPOINT = 'http://127.0.0.1:18080';

/**
 * The connection fields to send with a request — the ONE definition of the rule, imported by the fs
 * transport rather than repeated there.
 *
 * NO HOST IS A LEGITIMATE CONFIGURATION: the agent resolves the request's path against its mapping
 * store and uses the connection registered for it (the workspace registry). That is why a provider
 * can be configured with nothing but an endpoint and a token.
 *
 * HALF A CONNECTION IS STILL A MISTAKE, and it is named HERE rather than at the far end: a host
 * without a user is a half-written config, not a delegation, and the failure belongs where the
 * mistake was made — not as an SSH auth error thirty seconds later.
 */
export function connectionFields({ host, user, port = 22 } = {}) {
  if (!host && !user) return {};
  if (!host) throw new Error('summrise-workspace: a connection needs a host as well as a user');
  if (!user) throw new Error('summrise-workspace: a connection needs a user as well as a host');
  return { host, user, port };
}

/**
 * THE MACHINE'S ROOT MAPPING — `path: "/"` against the connection this provider was configured with — or `null`
 * when it has no machine of its own.
 *
 * WHY THIS EXISTS. A mapping is what makes a path resolvable, and the in-app directory picker browses through the
 * fs seam: without a mapping, "add workspace" refuses every path. Something has to be the moment at which "the
 * machine" is added, and the panel used to be it — the operator retired that ("不是另外加个 workspace"). The
 * moment is the CONFIG: a provider that was told a host IS that machine. A delegated provider (no host) has no
 * machine of its own and registers nothing — the registry decides, which is what a registry is for.
 *
 * `"/"` rather than the host's home directory: the mapping is a PREFIX, so the root covers every path on the
 * machine, and a more specific mapping can still override a subtree.
 */
export function machineRootMapping({ host, user, port = 22 } = {}) {
  if (!host || !user) return null;
  return { path: '/', connection_id: `ssh:${user}@${host}:${port}` };
}

/**
 * Register it, through the same route the doors resolve by. Registering is idempotent on the agent's side, so a
 * second call is harmless; the providers still call it once per instance.
 *
 * A REFUSAL IS NOT THROWN: this runs before a request that has its own error to report, and a registry that
 * cannot be told is not a reason to fail the call it precedes. The mapping it returns is what was sent, or null.
 */
export async function registerMachineRoot({
  endpoint = DEFAULT_ENDPOINT,
  token,
  host,
  user,
  port,
  fetchImpl = globalThis.fetch,
} = {}) {
  const mapping = machineRootMapping({ host, user, port });
  if (!mapping) return null;
  try {
    const res = await fetchImpl(`${endpoint}/api/workspace/register`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', authorization: `Bearer ${token}` },
      body: JSON.stringify(mapping),
    });
    return res && res.ok ? mapping : null;
  } catch {
    return null;
  }
}

/** Where the staged argv helper lives on a workspace host. Mirrors the agent's own default. */
export const DEFAULT_HELPER = '~/.summrise/bin/summrise-exec-argv';

/** Raised when the caller asked for a stdio disposition this provider does not implement. */
export class UnsupportedStdioError extends Error {
  constructor(message) {
    super(message);
    this.name = 'UnsupportedStdioError';
  }
}

/** Raised when the agent answered, but refused — as opposed to a transport failure. */
export class WorkspaceExecError extends Error {
  constructor(message, code) {
    super(message);
    this.name = 'WorkspaceExecError';
    this.code = code;
  }
}

/**
 * Refuse any stdio shape this provider does not implement, by name.
 *
 * WHY IT THROWS RATHER THAN APPROXIMATES: a `'pipe'` stream silently downgraded to a collected
 * buffer is a caller reading a live protocol from a dead one — it fails later, somewhere else, as a
 * parse error. `'inherit'` is worse: silently collected, the child's diagnostics never reach the
 * harness's own stream and the operator sees nothing.
 *
 * @param spec - the seam's spawn spec.
 */
export function assertSupportedStdio(spec) {
  const { stdin, stdout, stderr, control } = spec.stdio;
  if (stdin !== 'ignore' && stdin !== 'pipe' && typeof stdin?.data !== 'string') {
    throw new UnsupportedStdioError(`unsupported stdin disposition: ${JSON.stringify(stdin)}`);
  }
  if (stdin === 'pipe') {
    throw new UnsupportedStdioError(
      "stdin: 'pipe' is not implemented — a workspace command's stdin cannot be held open across the agent's request/response boundary",
    );
  }
  for (const [name, mode] of [
    ['stdout', stdout],
    ['stderr', stderr],
  ]) {
    if (mode === 'pipe' || mode === 'inherit') {
      throw new UnsupportedStdioError(
        `${name}: '${mode}' is not implemented — this provider collects both streams, and a silent downgrade would be a caller reading a live stream from a dead one`,
      );
    }
    if (typeof mode !== 'object' || mode === null || typeof mode.maxBytes !== 'number') {
      throw new UnsupportedStdioError(
        `${name} must be a collect spec ({ maxBytes }), got ${JSON.stringify(mode)}`,
      );
    }
  }
  if (control !== undefined) {
    throw new UnsupportedStdioError(
      "control: 'pipe' is not implemented — the agent's route has no duplex channel",
    );
  }
}

/**
 * An offset-based reader over one collected stream.
 *
 * @param bytes - the retained bytes, as a Buffer.
 * @param truncated - true when the agent dropped bytes from the HEAD of this stream.
 * @returns the reader the seam's `collected.stdout` / `collected.stderr` expect.
 */
export function makeCollectedReader(bytes, truncated) {
  const whole = bytes;
  return {
    /**
     * @param fromByte - whole-stream offset to resume from.
     */
    readFrom(fromByte) {
      const offset = Number.isFinite(fromByte) && fromByte > 0 ? Math.floor(fromByte) : 0;
      // THE COORDINATES ARE THE RETAINED WINDOW'S, AND THAT IS SAID OUT LOUD. The agent reports that
      // it dropped bytes but not HOW MANY, so a truncated stream's absolute offsets are not
      // recoverable here; what this can honour is the seam's stated behaviour for that case —
      // return the whole retained tail and mark the read lossy.
      if (truncated) {
        return {
          text: whole.toString('utf8'),
          nextOffset: whole.length,
          lossy: true,
        };
      }
      const from = Math.min(offset, whole.length);
      return {
        text: whole.subarray(from).toString('utf8'),
        nextOffset: whole.length,
        lossy: false,
      };
    },
  };
}

/**
 * Map the agent's status object onto the seam's `{ exitCode, signal }` vocabulary.
 *
 * `unknown` becomes `{ exitCode: null, signal: null }` and NOT a success: a channel can close without
 * the server ever sending a status, and a caller that read that as exit 0 would report a command
 * that never ran.
 *
 * @param status - the agent's `status` object.
 */
export function normalizeOutcome(status) {
  if (status?.kind === 'exited' && Number.isInteger(status.code)) {
    return { exitCode: status.code, signal: null };
  }
  if (status?.kind === 'signalled' && typeof status.name === 'string') {
    return { exitCode: null, signal: status.name };
  }
  return { exitCode: null, signal: null };
}

/**
 * Run one argv on a workspace host through the agent's `/api/workspace/exec`.
 *
 * @param options - endpoint, credentials, the argv and cwd, the caps, and an abort signal.
 * @returns the decoded streams, the raw status, and the overflow flags.
 */
export async function execOnAgent(options) {
  const {
    endpoint = DEFAULT_ENDPOINT,
    token,
    host,
    user,
    port = 22,
    password,
    keyPath,
    argv,
    cwd,
    stdoutCap,
    stderrCap,
    helper = DEFAULT_HELPER,
    timeoutMs,
    signal,
    fetchImpl = globalThis.fetch,
  } = options;

  const body = { ...connectionFields({ host, user, port }), argv };
  if (password) body.password = password;
  if (keyPath) body.key_path = keyPath;
  if (typeof cwd === 'string' && cwd.length > 0) body.cwd = cwd;
  if (Number.isFinite(stdoutCap)) body.stdout_cap = stdoutCap;
  if (Number.isFinite(stderrCap)) body.stderr_cap = stderrCap;
  if (helper) body.helper = helper;
  if (Number.isFinite(timeoutMs)) body.timeout_ms = timeoutMs;

  const headers = { 'content-type': 'application/json' };
  if (token) headers.authorization = `Bearer ${token}`;

  let response;
  try {
    response = await fetchImpl(`${endpoint.replace(/\/+$/, '')}/api/workspace/exec`, {
      method: 'POST',
      headers,
      body: JSON.stringify(body),
      signal,
    });
  } catch (cause) {
    // An abort is the caller's own cancellation and must stay distinguishable from a failure to
    // reach the agent — the seam's `done` rejects for provider failures and the caller reads its own
    // signal to classify a cancellation.
    if (signal?.aborted) throw signal.reason ?? cause;
    throw new WorkspaceExecError(`could not reach the agent at ${endpoint}: ${cause?.message ?? cause}`);
  }

  if (!response.ok) {
    throw new WorkspaceExecError(
      `the agent answered HTTP ${response.status} for /api/workspace/exec`,
      'http_error',
    );
  }

  let answer;
  try {
    answer = await response.json();
  } catch (cause) {
    throw new WorkspaceExecError(`the agent's answer was not JSON: ${cause?.message ?? cause}`);
  }
  if (answer?.ok !== true) {
    throw new WorkspaceExecError(
      typeof answer?.error === 'string' ? answer.error : 'the agent refused the command',
      typeof answer?.code === 'string' ? answer.code : 'unknown',
    );
  }

  return {
    stdout: Buffer.from(answer.stdout_b64 ?? '', 'base64'),
    stderr: Buffer.from(answer.stderr_b64 ?? '', 'base64'),
    status: answer.status,
    stdoutOverflow: answer.stdout_overflow === true,
    stderrOverflow: answer.stderr_overflow === true,
  };
}
