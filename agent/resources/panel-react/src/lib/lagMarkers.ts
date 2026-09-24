/**
 * Which lag markers are still worth keeping.
 *
 * WHY THIS IS A FUNCTION AND NOT A LINE IN THE SWEEP. `useSSE` keeps `lagBackfill` — a marker per
 * session meaning "the next frame must be gap-backfilled" — sets it WHOLESALE on reconnect (every
 * registered session gets one) and clears it one entry at a time as that session's frames arrive. A
 * session that died in between therefore kept its marker for the life of the page (found 2026-09-24 by
 * the panel exploration). The sweep already computes the set of sessions that could still frame, which
 * is what the prune needs — and the KEEP SET is the subtle part, so it is a pure function with a test
 * rather than one line inside a transport loop.
 *
 * THE KEEP SET IS `registered`, NOT `live`. A closed tombstone that the pane still holds can be
 * REVIVED (round-245), and a revived session must find its marker intact — pruning to live sessions
 * alone would drop markers for sessions the panel is still capable of showing.
 */
export function pruneLagMarkers<M extends { delete(key: string): unknown }>(
  markers: M,
  reachable: Iterable<string>,
): number {
  const keep = new Set(reachable);
  let dropped = 0;
  for (const sid of [...(markers as unknown as Map<string, unknown>).keys()]) {
    if (!keep.has(sid)) {
      markers.delete(sid);
      dropped += 1;
    }
  }
  return dropped;
}
