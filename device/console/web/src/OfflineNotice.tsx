import { seenAgo } from "./format";

/**
 * The machine's own state, above whatever the page does.
 *
 * EVERY DEVICE PAGE CAN BE LEGITIMATELY EMPTY, and only one of the reasons is the page's own: a
 * machine that is not connected was, until this sentence existed, reported by the failed call that
 * happened to run first — "the console never reached the device" beside a request the operator never
 * made. A person reading that has to work out whether the console, the permission or the machine is
 * the fault, and the one thing that would answer it (the machine is down) was on another page.
 *
 * The notice is a control's neighbour rather than a toast: it stays while the condition does, and it
 * names the surface it is stopping through `detail`, because "No shell can start" is wrong on Files
 * and "Nothing can be read" is wrong on Terminal.
 */
export function OfflineNotice({
  lastSeenMs,
  detail
}: {
  /** When the console last heard from the machine; `null`/absent when it never has. */
  lastSeenMs?: number | null;
  /** The sentence for this surface, after the machine's state: what cannot happen until it returns. */
  detail: string;
}) {
  const lastSeen = typeof lastSeenMs === "number" ? seenAgo(lastSeenMs) : null;
  return (
    <p className="offline-notice" role="status">
      <strong>This machine is offline.</strong>{" "}
      {lastSeen ? `Last seen ${lastSeen}. ` : ""}
      {detail}
    </p>
  );
}
