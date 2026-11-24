// Read-only author-scope proof for the actual gate callbacks. No proof/skip constructor is exported.
let nextSequence = 0;
const prefix = 'contrast: unsupported paint text-fill provenance author scope';
const failure = (reason, cause) => new Error(`${prefix} ${reason}`, cause ? { cause } : undefined);
const positiveId = (value) => Number.isSafeInteger(value) && value > 0;
const counts = () => ({ nodes: 0, openRoots: 0, uaRoots: 0, closedRoots: 0, unknown: 0 });

export async function withAuthorPaintScope(page, read) {
  if (typeof read !== 'function') throw failure('requires an actual reader');
  const sequence = ++nextSequence;
  const media = async () => {
    let active;
    try { active = await page.evaluate(() => matchMedia('(forced-colors: active)').matches); }
    catch (error) { throw failure('media state unavailable', error); }
    if (typeof active !== 'boolean') throw failure('media state unknown');
    return active;
  };
  const beforeMedia = await media();
  if (!beforeMedia) {
    const value = await read(null);
    const afterMedia = await media();
    if (afterMedia !== beforeMedia) throw failure('media changed during normal reader');
    return { value, evidence: { version: 1, method: 'not-required-normal', sequence, beforeMedia, afterMedia } };
  }

  let session, documentRoot, value, primaryError, before, after, afterMedia;
  let documentEpoch = 0, boundEpoch = null;
  const observed = counts(), closed = new Set(), unknown = new Set();
  const markUnknown = (reason, stats) => { unknown.add(reason); stats.unknown += 1; };
  const inspect = (node, stats, expectedShadow = false, ancestors = new Set()) => {
    if (!node || typeof node !== 'object' || Array.isArray(node) || ancestors.has(node)) {
      markUnknown('unreadable or cyclic node descriptor', stats); return;
    }
    const branch = new Set(ancestors); branch.add(node);
    stats.nodes += 1;
    if (!positiveId(node.backendNodeId) || !Number.isInteger(node.nodeType) || node.nodeType < 1 || node.nodeType > 12) {
      markUnknown('invalid node identity', stats); return;
    }
    if (expectedShadow || node.shadowRootType !== undefined) {
      if (node.nodeType !== 11) markUnknown('invalid shadow node type', stats);
      if (node.shadowRootType === 'closed') { closed.add(node.backendNodeId); stats.closedRoots += 1; }
      else if (node.shadowRootType === 'open') stats.openRoots += 1;
      else if (node.shadowRootType === 'user-agent') stats.uaRoots += 1;
      else markUnknown('unknown shadow root type', stats);
    }
    if (node.childNodeCount !== undefined && (!Number.isSafeInteger(node.childNodeCount) || node.childNodeCount < 0)) markUnknown('invalid child count', stats);
    if ([1, 9, 11].includes(node.nodeType) && node.children === undefined && node.childNodeCount !== 0) markUnknown('incomplete child descriptors', stats);
    if (Array.isArray(node.children) && node.childNodeCount !== undefined && node.children.length !== node.childNodeCount) markUnknown('incomplete child list', stats);
    for (const key of ['children', 'shadowRoots', 'pseudoElements']) {
      if (node[key] === undefined) continue;
      if (!Array.isArray(node[key])) { markUnknown(`unreadable ${key}`, stats); continue; }
      for (const child of node[key]) inspect(child, stats, key === 'shadowRoots', branch);
    }
    for (const key of ['contentDocument', 'templateContent']) {
      if (node[key] !== undefined) inspect(node[key], stats, false, branch);
    }
    if (['IFRAME', 'FRAME'].includes(node.nodeName) && !node.contentDocument) markUnknown('inaccessible frame document', stats);
  };
  const descriptorEvent = (node, expectedShadow = false) => {
    // This handler must remain synchronous: no unresolved inspection may cross the post barrier.
    try { inspect(node, observed, expectedShadow); }
    catch { markUnknown('event descriptor inspection failed', observed); }
  };
  const assertClean = () => {
    if (closed.size) throw failure('closed authored shadow root');
    if (unknown.size) throw failure(`unknown or incomplete scope: ${[...unknown].join(', ')}`);
    if (boundEpoch !== null && documentEpoch !== boundEpoch) throw failure('document epoch changed');
  };
  const command = async (name, args) => {
    try { return await session.send(name, args); }
    catch (error) { throw failure(`protocol ${name} unavailable`, error); }
  };
  const snapshot = async () => {
    const result = await command('DOM.getDocument', { depth: -1, pierce: true });
    const root = result?.root;
    if (!root || root.nodeType !== 9 || !positiveId(root.backendNodeId)) throw failure('unreadable root document');
    const stats = counts();
    try { inspect(root, stats); }
    catch (error) { throw failure('snapshot inspection failed', error); }
    assertClean();
    return { documentBackendNodeId: root.backendNodeId, documentNodeId: root.nodeId, epoch: documentEpoch, ...stats };
  };
  const sameLiveRoot = async () => {
    try {
      return await documentRoot.evaluate((root) => root !== null && root === document.documentElement && root.isConnected);
    } catch (error) { throw failure('live document binding unavailable', error); }
  };

  try {
    try { session = await page.context().newCDPSession(page); }
    catch (error) { throw failure('CDP unavailable', error); }
    session.on('DOM.shadowRootPushed', ({ root }) => descriptorEvent(root, true));
    session.on('DOM.setChildNodes', ({ nodes }) => {
      if (!Array.isArray(nodes)) { markUnknown('unreadable setChildNodes', observed); return; }
      for (const node of nodes) descriptorEvent(node);
    });
    session.on('DOM.childNodeInserted', ({ node }) => descriptorEvent(node));
    session.on('DOM.pseudoElementAdded', ({ pseudoElement }) => descriptorEvent(pseudoElement));
    session.on('DOM.documentUpdated', () => { documentEpoch += 1; });
    await command('DOM.enable', { includeWhitespace: 'all' });
    before = await snapshot();
    boundEpoch = before.epoch;
    try { documentRoot = await page.evaluateHandle(() => document.documentElement); }
    catch (error) { throw failure('live document handle unavailable', error); }
    if (!await sameLiveRoot()) throw failure('missing or stale live document root');
    assertClean();
    if (await media() !== beforeMedia) throw failure('media changed before reader');

    const authorPaintScope = Object.freeze({ version: 1, method: 'cdp-pierced-dom', documentRoot, sequence });
    // The actual reader receives a live node reference, never a caller-supplied admission flag.
    value = await read(authorPaintScope);

    after = await snapshot(); // The response follows synchronous DOM event-descriptor latches.
    if (after.documentBackendNodeId !== before.documentBackendNodeId) throw failure('root document identity changed');
    if (!await sameLiveRoot()) throw failure('stale live document root after reader');
    afterMedia = await media();
    if (afterMedia !== beforeMedia) throw failure('media changed during reader');
    assertClean();
  } catch (error) {
    primaryError = error;
  }

  // Cleanup must finish before any provisional value can be returned, even after a reader error.
  const cleanupErrors = [];
  if (documentRoot) {
    try { await documentRoot.dispose(); }
    catch (error) { cleanupErrors.push(error); }
  }
  if (session) {
    try { await session.detach(); }
    catch (error) { cleanupErrors.push(error); }
  }
  if (cleanupErrors.length) throw failure('cleanup failed; provisional result discarded', new AggregateError(primaryError ? [primaryError, ...cleanupErrors] : cleanupErrors));
  if (primaryError) throw primaryError;
  assertClean();
  return {
    value,
    evidence: {
      version: 1, method: 'cdp-pierced-dom', sequence, beforeMedia, afterMedia,
      before, after, events: observed, documentEpoch, closedObserved: closed.size,
      unknownObserved: unknown.size, cleanup: 'disposed-and-detached',
    },
  };
}
