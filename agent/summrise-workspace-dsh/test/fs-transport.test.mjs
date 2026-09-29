/**
 * The filesystem transport's tests, run with `node --test` and no DSH install.
 *
 * The pure half is where the decisions are — normalisation, containment, the URI — and the RPC half
 * is checked at the boundary this module owns: what goes on the wire, and that a refusal's CODE
 * survives the trip instead of being re-derived from its message.
 */

import assert from 'node:assert/strict';
import test from 'node:test';

import {
  WorkspaceFsError,
  fileUrlFor,
  fsOnAgent,
  normalizePath,
  pathContains,
  targetFor,
} from '../lib/fs-transport.js';

/** A fetch that records the request and answers with `body`. */
function fakeFetch(body, { status = 200 } = {}) {
  const calls = [];
  const impl = async (url, init) => {
    calls.push({ url, init, body: init?.body ? JSON.parse(init.body) : undefined });
    return {
      ok: status >= 200 && status < 300,
      status,
      json: async () => body,
    };
  };
  impl.calls = calls;
  return impl;
}

test('a path has exactly ONE spelling', () => {
  // Resolution is the step the seam says costs a remote backend a round trip, and it is the one step
  // that does not need one: a POSIX path normalises to itself.
  assert.equal(normalizePath('/a/b/c', '/'), '/a/b/c');
  assert.equal(normalizePath('/a//b///c', '/'), '/a/b/c');
  assert.equal(normalizePath('/a/./b', '/'), '/a/b');
  assert.equal(normalizePath('/a/x/../b', '/'), '/a/b');
  assert.equal(normalizePath('/a/b/', '/'), '/a/b');
  assert.equal(normalizePath('/', '/'), '/');
  assert.equal(normalizePath('/a/b/..', '/'), '/a');
});

test('a relative path resolves against the base, and a root stays a root', () => {
  assert.equal(normalizePath('b/c', '/work'), '/work/b/c');
  assert.equal(normalizePath('./b', '/work'), '/work/b');
  assert.equal(normalizePath('../b', '/work/a'), '/work/b');
  assert.equal(normalizePath('b', '/'), '/b');
});

test('climbing above the root lands ON the root, which is what the host does', () => {
  // POSIX: `/..` is `/`. Keeping the `..` would make resolve('/..') answer a key that READING '/..'
  // does not open — the target key would name a file it is not, and the seam requires the same file
  // to yield the same key.
  assert.equal(normalizePath('/..', '/'), '/');
  assert.equal(normalizePath('/../../a', '/'), '/a');
  assert.equal(normalizePath('/a/../../b', '/'), '/b');
});

test('an empty path is refused rather than answered as the root', () => {
  assert.throws(
    () => normalizePath('', '/'),
    (e) => e instanceof WorkspaceFsError && e.code === 'FS_IO_ERROR',
  );
});

test('containment is SEGMENT-wise, not a string prefix', () => {
  // `/work/a` must not be said to contain `/work/ab`, which a startsWith would claim.
  assert.equal(pathContains('/work', '/work/a'), true);
  assert.equal(pathContains('/work', '/work'), true);
  assert.equal(pathContains('/work/a', '/work/ab'), false);
  assert.equal(pathContains('/', '/anything'), true);
  assert.equal(pathContains('/work', '/workshop'), false);
  assert.equal(pathContains('/work/a', '/work'), false);
});

test('the file URI encodes names without breaking the separators', () => {
  assert.equal(fileUrlFor('/a/b c'), 'file:///a/b%20c');
  // A single quote is a legal path character in RFC 3986, so encodeURIComponent leaves it — and a
  // URI that escaped it would still be valid, which is why this is asserted rather than assumed.
  assert.equal(fileUrlFor("/a/it's"), "file:///a/it's");
  assert.equal(fileUrlFor('/a/b#c'), 'file:///a/b%23c');
  assert.equal(fileUrlFor('/a/b'), 'file:///a/b');
});

test('a target key IS its path, which is why resolve costs nothing', () => {
  assert.deepEqual(targetFor('/work/a.txt', '/'), {
    targetKey: '/work/a.txt',
    displayPath: '/work/a.txt',
  });
  assert.deepEqual(targetFor('a.txt', '/work'), {
    targetKey: '/work/a.txt',
    displayPath: '/work/a.txt',
  });
});

test('an op sends the connection, the path, and only what that op asked for', async () => {
  const fetchImpl = fakeFetch({ ok: true, info: { kind: 'file', size: 3, mtime: 9, mode: 420 } });
  await fsOnAgent({
    endpoint: 'http://127.0.0.1:18080/',
    host: 'h',
    user: 'u',
    port: 22122,
    op: 'stat',
    path: '/work/a.txt',
    fetchImpl,
  });
  const sent = fetchImpl.calls[0];
  assert.equal(sent.url, 'http://127.0.0.1:18080/api/workspace/fs');
  assert.deepEqual(sent.body, { op: 'stat', path: '/work/a.txt', host: 'h', user: 'u', port: 22122 });
});

test('an op-specific field rides along verbatim, and an undefined one does not', async () => {
  // This layer does not decide what an op accepts: a second list of that would be a second thing to
  // keep in step with the route.
  const fetchImpl = fakeFetch({ ok: true, bytes_b64: '' });
  await fsOnAgent({
    host: 'h',
    user: 'u',
    op: 'readBytes',
    path: '/a',
    offset: 10,
    length: 32,
    somethingUnset: undefined,
    fetchImpl,
  });
  const sent = fetchImpl.calls[0].body;
  assert.equal(sent.offset, 10);
  assert.equal(sent.length, 32);
  assert.ok(!('somethingUnset' in sent), 'an undefined field must not be sent as a null');
});

test("a refusal keeps the AGENT'S code, which is the seam's own vocabulary", async () => {
  // The whole reason the agent was changed to answer in `dsh-fs` codes: a translation layer that
  // re-read the message here would undo the guarantee.
  for (const code of ['FS_TOO_LARGE', 'FS_NOT_TEXT', 'FS_NOT_FOUND', 'FS_NOT_REGULAR_FILE']) {
    const fetchImpl = fakeFetch({ ok: false, error: 'whatever the prose says', code });
    await assert.rejects(
      () => fsOnAgent({ host: 'h', user: 'u', op: 'readText', path: '/a', fetchImpl }),
      (e) => e instanceof WorkspaceFsError && e.code === code,
      code,
    );
  }
});

test('a transport failure is FS_IO_ERROR, not a code it invented', async () => {
  const fetchImpl = async () => {
    throw new Error('connect ECONNREFUSED');
  };
  await assert.rejects(
    () => fsOnAgent({ endpoint: 'http://127.0.0.1:9', host: 'h', user: 'u', op: 'stat', path: '/a', fetchImpl }),
    (e) => e instanceof WorkspaceFsError && e.code === 'FS_IO_ERROR' && /127\.0\.0\.1:9/.test(e.message),
  );
});

test('an HTTP error is not mistaken for a refusal', async () => {
  const fetchImpl = fakeFetch({}, { status: 500 });
  await assert.rejects(
    () => fsOnAgent({ host: 'h', user: 'u', op: 'stat', path: '/a', fetchImpl }),
    (e) => e instanceof WorkspaceFsError && e.code === 'FS_IO_ERROR' && /HTTP 500/.test(e.message),
  );
});

test('a caller cancellation stays a cancellation', async () => {
  const controller = new AbortController();
  const reason = new Error('the caller gave up');
  const fetchImpl = async () => {
    controller.abort(reason);
    throw new Error('aborted');
  };
  await assert.rejects(
    () =>
      fsOnAgent({ host: 'h', user: 'u', op: 'stat', path: '/a', signal: controller.signal, fetchImpl }),
    (e) => e === reason,
  );
});
