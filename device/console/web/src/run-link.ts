/** A run named in the URL: which machine it ran on and which record to open. */
export type RunLink = { deviceId: string; runId: string };

/**
 * The URL fragment Activity writes when it opens a run: `#run/<device>/<run>`, both percent-encoded.
 * A fragment, not a path: the console is one file with no router, and a fragment is not sent to the
 * console on reload.
 */
export function runLink(deviceId: string, runId: string): string {
  return "#run/" + encodeURIComponent(deviceId) + "/" + encodeURIComponent(runId);
}

/** Read a run link; anything that is not exactly one device and one run is not a link. */
export function parseRunLink(hash: string): RunLink | null {
  const match = /^#run\/([^/]+)\/([^/]+)$/.exec(hash);
  if (!match) return null;
  try {
    return { deviceId: decodeURIComponent(match[1]), runId: decodeURIComponent(match[2]) };
  } catch {
    // A malformed escape is a malformed link, not a crash on boot.
    return null;
  }
}
