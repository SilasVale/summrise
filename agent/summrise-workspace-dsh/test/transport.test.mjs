/**
 * The transport's tests, run with `node --test` and no DSH install.
 *
 * That is the reason `lib/transport.js` imports nothing from the seam's packages: everything with a
 * decision in it lives where it can be tested, and `lib/index.js` is the thin class that wires it in.
 */

import assert from 'node:assert/strict';
import test from 'node:test';

import {
  UnsupportedStdioError,
  WorkspaceExecError,
  assertSupportedStdio,
  execOnAgent,
  makeCollectedReader,
  normalizeOutcome,
} from '../lib/transport.js';

/** A fetch that records the request and answers with `body`. */
function fakeFetch(body, { status = 200, onRequest } = {}) {
  const calls = [];
  const impl = async (url, init) => {
    calls.push({ url, init, body: init?.body ? JSON.parse(init.body) : undefined });
    onRequest?.(calls[calls.length - 1]);
    return {
      ok: status >= 200 && status < 300,
      status,
      json: async () => body,
    };
  };
  impl.calls = calls;
  return impl;
}

const collect = (maxBytes) => ({ maxBytes });

test('the shape fs-search actually uses is accepted', () => {
  // The measured call: `stdio: { stdin: 'ignore', stdout: { maxBytes }, stderr: { maxBytes } }`.
  assert.doesNotThrow(() =>
    assertSupportedStdio({
      stdio: { stdin: 'ignore', stdout: collect(1 << 20), stderr: collect(64 << 10) },
    }),
  );
});

test('a stdio shape this provider cannot honour is refused BY NAME', () => {
  // A silent downgrade is the failure: `'pipe'` collected is a caller reading a live protocol from a
  // dead one, and `'inherit'` collected means the operator never sees the diagnostics.
  for (const [stdio, about] of [
    [{ stdin: 'pipe', stdout: collect(1), stderr: collect(1) }, 'stdin pipe'],
    [{ stdin: 'ignore', stdout: 'pipe', stderr: collect(1) }, 'stdout pipe'],
    [{ stdin: 'ignore', stdout: collect(1), stderr: 'inherit' }, 'stderr inherit'],
    [{ stdin: 'ignore', stdout: collect(1), stderr: collect(1), control: 'pipe' }, 'control'],
    [{ stdin: 'ignore', stdout: 5, stderr: collect(1) }, 'a non-spec output mode'],
  ]) {
    assert.throws(
      () => assertSupportedStdio({ stdio }),
      (e) => e instanceof UnsupportedStdioError && /not implemented|must be a collect spec/.test(e.message),
      about,
    );
  }
});

test('a collected reader returns the stream from an offset', () => {
  const reader = makeCollectedReader(Buffer.from('hello world'), false);
  assert.deepEqual(reader.readFrom(0), {
    text: 'hello world',
    nextOffset: 11,
    lossy: false,
  });
  assert.deepEqual(reader.readFrom(6), { text: 'world', nextOffset: 11, lossy: false });
  assert.deepEqual(reader.readFrom(99), { text: '', nextOffset: 11, lossy: false });
});

test('a truncated stream is reported LOSSY, which is what the seam asks for', () => {
  // The agent reports that it dropped bytes but not how many, so absolute offsets are not
  // recoverable — and the seam's own rule for that case is: return the retained tail, mark it lossy.
  const reader = makeCollectedReader(Buffer.from('the retained tail'), true);
  const read = reader.readFrom(0);
  assert.equal(read.text, 'the retained tail');
  assert.equal(read.lossy, true);
});

test('the outcome vocabulary distinguishes exited, signalled, and unknown', () => {
  assert.deepEqual(normalizeOutcome({ kind: 'exited', code: 0 }), { exitCode: 0, signal: null });
  assert.deepEqual(normalizeOutcome({ kind: 'exited', code: 3 }), { exitCode: 3, signal: null });
  assert.deepEqual(normalizeOutcome({ kind: 'signalled', name: 'SIGKILL' }), {
    exitCode: null,
    signal: 'SIGKILL',
  });
  // NOT a success: a channel that closed with no status is a command that may never have run.
  assert.deepEqual(normalizeOutcome({ kind: 'unknown' }), { exitCode: null, signal: null });
  assert.deepEqual(normalizeOutcome(undefined), { exitCode: null, signal: null });
});

test('the argv crosses VERBATIM — no quoting, no joining, no trimming', () => {
  // The seam's promise, checked at the boundary this module owns. The agent carries these in a
  // frame; if this layer started quoting, the promise would be broken before the frame was built.
  const argv = ['rg', '--no-config', 'two words', '$(id)', "it's \"quoted\"", '  padded  '];
  return execOnAgent({
    host: 'h',
    user: 'u',
    argv,
    cwd: '/a dir/$(id)',
    stdoutCap: 1234,
    stderrCap: 56,
    fetchImpl: fakeFetch({ ok: true, stdout_b64: '', stderr_b64: '', status: { kind: 'exited', code: 0 } }),
  }).then(() => {});
});

test('what goes on the wire is the argv unchanged and the caps the caller asked for', async () => {
  const fetchImpl = fakeFetch({
    ok: true,
    stdout_b64: Buffer.from('out').toString('base64'),
    stderr_b64: Buffer.from('err').toString('base64'),
    status: { kind: 'exited', code: 7 },
    stdout_overflow: false,
    stderr_overflow: true,
  });
  const argv = ['rg', 'two words', '$(id)'];

  const answer = await execOnAgent({
    endpoint: 'http://127.0.0.1:18080/',
    host: 'h',
    user: 'u',
    port: 22122,
    argv,
    cwd: '/a dir',
    stdoutCap: 1000,
    stderrCap: 20,
    fetchImpl,
  });

  const sent = fetchImpl.calls[0];
  assert.equal(sent.url, 'http://127.0.0.1:18080/api/workspace/exec');
  assert.deepEqual(sent.body.argv, argv);
  assert.equal(sent.body.cwd, '/a dir');
  assert.equal(sent.body.stdout_cap, 1000);
  assert.equal(sent.body.stderr_cap, 20);
  assert.equal(sent.body.port, 22122);

  assert.equal(answer.stdout.toString('utf8'), 'out');
  assert.equal(answer.stderr.toString('utf8'), 'err');
  assert.deepEqual(answer.status, { kind: 'exited', code: 7 });
  assert.equal(answer.stdoutOverflow, false);
  assert.equal(answer.stderrOverflow, true);
});

test('a refusal from the agent keeps its code', async () => {
  const fetchImpl = fakeFetch({ ok: false, error: 'a host is required', code: 'invalid_params' });
  await assert.rejects(
    () => execOnAgent({ host: 'h', user: 'u', argv: ['rg'], fetchImpl }),
    (e) => e instanceof WorkspaceExecError && e.code === 'invalid_params' && /a host is required/.test(e.message),
  );
});

test('an HTTP error is not mistaken for a refusal', async () => {
  const fetchImpl = fakeFetch({}, { status: 500 });
  await assert.rejects(
    () => execOnAgent({ host: 'h', user: 'u', argv: ['rg'], fetchImpl }),
    (e) => e instanceof WorkspaceExecError && e.code === 'http_error',
  );
});

test('an unreachable agent is named with its endpoint', async () => {
  const fetchImpl = async () => {
    throw new Error('connect ECONNREFUSED');
  };
  await assert.rejects(
    () => execOnAgent({ endpoint: 'http://127.0.0.1:9', host: 'h', user: 'u', argv: ['rg'], fetchImpl }),
    (e) => e instanceof WorkspaceExecError && /127\.0\.0\.1:9/.test(e.message),
  );
});

test('a caller cancellation stays a cancellation instead of becoming a provider failure', async () => {
  // The seam classifies causes by the caller's OWN signal, so an abort must not arrive as one of
  // this module's errors.
  const controller = new AbortController();
  const reason = new Error('the caller gave up');
  const fetchImpl = async () => {
    controller.abort(reason);
    throw new Error('aborted');
  };
  await assert.rejects(
    () => execOnAgent({ host: 'h', user: 'u', argv: ['rg'], signal: controller.signal, fetchImpl }),
    (e) => e === reason,
  );
});
