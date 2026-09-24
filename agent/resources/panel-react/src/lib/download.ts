// HANDING BYTES TO THE BROWSER IS NOT SESSION STATE.
//
// `useSessions.ts` owns sessions: fetching rows, tombstoning, sweeping, the poll cadence. It also carried six
// lines of anchor plumbing inside `exportSession` — build a Blob, `document.createElement("a")`, set `href` from
// `URL.createObjectURL`, set `download`, `click()`, `revokeObjectURL`. That is why the twentieth exploration
// counted it among the hook's accumulated concerns.
//
// The distinction matters: the hook's `window.addEventListener` and `setInterval` calls ARE its job — it polls
// and it listens for cross-panel changes. Clicking an anchor is not. This module is the one place the panel
// touches an anchor, and it is testable without rendering anything.
export interface DownloadTarget {
  href: string;
  download: string;
  click: () => void;
}

/**
 * Hand `blob` to the browser as a download named `filename`.
 *
 * The object URL is REVOKED SYNCHRONOUSLY after the click. That is safe because `click()` dispatches the
 * navigation synchronously for a download, and it is necessary because the URL keeps the whole Blob alive
 * until revoked — the export path can build megabytes, so leaking one per export is a real cost.
 *
 * `document` is injected so a test can assert the click without a DOM, and so this file stays the only place
 * that names one.
 */
export function downloadBlob(
  filename: string,
  blob: Blob,
  doc: Pick<Document, "createElement"> = document,
): DownloadTarget {
  const url = URL.createObjectURL(blob);
  const a = doc.createElement("a") as unknown as DownloadTarget;
  a.href = url;
  a.download = filename;
  try {
    a.click();
  } finally {
    // `finally`, so a click that throws still releases the Blob.
    URL.revokeObjectURL(url);
  }
  return a;
}
