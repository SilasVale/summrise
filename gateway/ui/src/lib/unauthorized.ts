/**
 * The 401 SEAM — a leaf module, so the data layer never reaches into a React context.
 *
 * What this replaced: `client.ts` imported `notifyUnauthorized` from `../contexts/AuthContext.tsx`,
 * and `AuthContext.tsx` imported `api` from `./client.ts`. The cycle was real, and it was also a
 * layering inversion — the API client, which knows nothing about React, depended on a context module
 * that renders nothing and exists only to hold the session — resolved by a module-level `let` that the
 * provider wrote and the client read. Both sides now point DOWN at this file instead:
 *
 *     client.ts ─┐
 *                ├─► lib/unauthorized.ts      (this file: the seam)
 *     AuthContext ┘
 *
 * The mutable is still here, deliberately. This is composition-root wiring — exactly one writer (the
 * provider, which owns what "signed out" means) and one reader (the client, which is the only place
 * that learns a session died) — and a callback registered once is the smallest honest form of it.
 * What changes is that the dependency now travels in one direction and the file is named for the job.
 *
 * WHAT IT MEANS WHEN NOTHING IS REGISTERED, which is why this comment exists at all: the render smokes
 * in gateway/ui mount the bundle WITHOUT the provider, so their 401s are a silent no-op and always
 * were. That is correct for a smoke whose subject is a page, and it is worth knowing before someone
 * reads a green smoke as "the 401 path is covered" — it is not, and no test here covers it.
 */
let onUnauthorized: (() => void) | null = null;

/** Called by AuthProvider while it is mounted; the provider owns what being signed out MEANS. */
export function setUnauthorizedHandler(handler: (() => void) | null): void {
  onUnauthorized = handler;
}

/** Called by the API client when a request answers 401. No-op when no provider is mounted. */
export function notifyUnauthorized(): void {
  onUnauthorized?.();
}
