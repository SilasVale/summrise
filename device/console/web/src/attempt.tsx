/**
 * The reason a console link's last attempt failed, as the machine's offline notice states it.
 *
 * `last_attempt.reason` is the device's own sentence about why the attempt before the last one
 * failed. The page adds the count and the punctuation and never rewrites the reason: a proxy's "HTTP
 * error: 502 Bad Gateway" and a channel's "the console accepted the channel and then dropped it" are
 * different diagnoses, and a paraphrase would merge them into one healthy-looking reconnect.
 *
 * THE FLEET ROW DOES NOT USE THIS. It drew the same sentence with no date, so a 09:41 reconnect read
 * as a fault at 10:07; an attempt carries no moment of its own, so the row now says nothing about it
 * and Copy diagnostics keeps the record. What is left here is the hub's offline notice, where the
 * machine is not connected *now* and the attempt is why the console is still trying.
 *
 * Where the reason is drawn it is cut to one line with an ellipsis, and the element's title carries
 * the whole sentence, so a reason longer than a phone-wide row is still reachable.
 */

export type LastAttempt = { attempts: number; reason?: string | null };

/** "1 attempt" / "3 attempts" — the count in the page's own words. */
export function attemptCount(attempts: number): string {
  return `${attempts} attempt${attempts === 1 ? "" : "s"}`;
}

/**
 * The device's reason, or `""` when it gave none. A zero-attempt note is not a failure to report:
 * `last_attempt` only arrives after a reconnect, but the count is what decides whether one is.
 */
export function attemptReason(attempt: LastAttempt | null | undefined): string {
  if (!attempt || attempt.attempts <= 0 || typeof attempt.reason !== "string") return "";
  return attempt.reason.trim() === "" ? "" : attempt.reason;
}

export function AttemptReason({ reason }: { reason: string }) {
  return (
    <span className="attempt-reason" title={reason}>
      {reason}
    </span>
  );
}
