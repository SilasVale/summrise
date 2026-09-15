// WHICH SESSIONS IS NOBODY USING? — the panel's half of the session story.
//
// WHY (round 41). The device has a 16-session cap and a 15-minute idle sweeper; measured on the
// operator's own box, SIXTEEN sessions were alive, eleven of them silent for more than five hours
// and several for eleven. The device now announces every eviction it performs (round 37) and the
// panel draws it — but until now the operator could only WATCH sessions accumulate and then be
// taken away. This is the rule behind offering to close them first, on their terms.
//
// THE THRESHOLD IS NOT ARBITRARY: the device's own idle TTL is 15 minutes, and the measured
// distribution on the device is bimodal — sessions in use (seconds of silence) and sessions
// forgotten (hours, distributed around 5-11h). One hour sits in the empty middle by a wide margin,
// so an offer at one hour cannot catch a session somebody is working in.
//
// CLOSED SESSIONS ARE NOT CANDIDATES: they have no shell to release (the device closed them when
// they exited), and counting them would make the offer's number wrong.

import type { Session } from "../hooks/useSessions";

/** Silence past which a session is worth offering to close. See the note above for why an hour. */
export const IDLE_OFFER_MS = 60 * 60 * 1000;

/** The sessions nobody is using: live, and silent for longer than the threshold. */
export function idleSessions(sessions: Session[], thresholdMs: number = IDLE_OFFER_MS): Session[] {
  return sessions.filter((s) => !s.closed && !s.savedOnly && s.idleMs > thresholdMs);
}

/** ONE LINE for the offer: how many, and the longest silence among them. */
export function idleOfferText(candidates: Session[]): string {
  if (candidates.length === 0) return "";
  const longest = Math.max(...candidates.map((s) => s.idleMs));
  const names = candidates.map((s) => s.label || s.sid);
  const shown = names.slice(0, 3).join(", ") + (names.length > 3 ? ` +${names.length - 3} more` : "");
  return `${candidates.length} session${candidates.length === 1 ? "" : "s"} idle for up to ${humanIdle(
    longest,
  )} — ${shown}`;
}

function humanIdle(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  return `${Math.floor(s / 3600)}h${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}m`;
}
