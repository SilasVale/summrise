/**
 * Which per-session views are still worth keeping.
 *
 * WHY THIS IS A FUNCTION AND NOT AN EFFECT BODY. `App` holds `sessionViews` — the main-area view
 * (terminal | trajectory) chosen per session — and its only writer ADDS: `changeView` spreads the
 * record and sets one key. The session list, meanwhile, caps closed tombstones at 32, so every session
 * an operator ever opened a trajectory in left its sid in that record for the life of the page (found
 * 2026-09-24 by the panel exploration). The prune needs the live sids, which only the list knows, so it
 * runs in an effect — but the decision itself is pure, and pure decisions in this panel get tested.
 *
 * THE IDENTITY RETURN IS PART OF THE CONTRACT, not an optimisation. The effect depends on the session
 * list, which changes on every poll; returning a fresh object whenever nothing was pruned would
 * re-render `App` once per poll for ever. Unchanged in, SAME OBJECT out.
 */
export function pruneSessionViews<V>(
  views: Record<string, V>,
  liveSids: Iterable<string>,
): Record<string, V> {
  const live = new Set(liveSids);
  const keys = Object.keys(views);
  const kept = keys.filter((sid) => live.has(sid));
  if (kept.length === keys.length) return views;
  const next: Record<string, V> = {};
  for (const sid of kept) next[sid] = views[sid];
  return next;
}
