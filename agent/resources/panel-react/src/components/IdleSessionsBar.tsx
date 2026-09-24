// The offer: sessions nobody is using, closable in one move — with the count and the silence
// spelled out, and a confirmation, because closing a session ends a shell.
//
// DESIGN. It is drawn where the crowding is felt (both densities mount it beside the tab strip,
// like the eviction notice) and it is QUIET: one line, chrome text, a single action. It renders
// NOTHING when there is nothing to offer — a permanent "close idle sessions" button on a device
// with two active sessions is an invitation to close something somebody is using.
import { useEffect, useState } from "react";
import { Icon } from "../ui/Icon";
import { idleOfferText } from "../lib/idleSessions";
import type { Session } from "../hooks/useSessions";

export function IdleSessionsBar({
  candidates,
  onClose,
}: {
  candidates: Session[];
  /** Close ONE session — the same call the tab's own × makes, so there is one close path.
   *
   *  ASYNC ON PURPOSE, and typed that way since 2026-09-24. It was `(sid: string) => void` while both
   *  callers pass `closeSession`, which is an I/O call — TypeScript allows an async function where a
   *  `void`-returning one is expected, so the promise was dropped, the loop below could not await it,
   *  and the comment there claimed a sequencing the code did not do. */
  onClose: (sid: string) => Promise<void>;
}) {
  const [confirming, setConfirming] = useState(false);
  const [closing, setClosing] = useState(false);
  // "HIDE UNTIL THE LIST CHANGES" HAS TO MEAN IT. The dismiss button carried that label from the
  // start and its handler only cancelled the confirmation, so clicking it left the bar exactly where
  // it was — a control that promises something and does nothing (found by measuring the surface,
  // round 42). The offer is keyed on WHICH sessions it is offering: hiding holds until the set
  // changes, and a new idle session brings it back.
  const signature = candidates.map((s) => s.sid).join(",");
  const [hiddenFor, setHiddenFor] = useState<string | null>(null);
  useEffect(() => {
    // A different set of candidates is new information: un-hide.
    setHiddenFor((h) => (h !== null && h !== signature ? null : h));
  }, [signature]);
  if (candidates.length === 0 || hiddenFor === signature) return null;

  const closeAll = async () => {
    setClosing(true);
    // Sequential, and the list is the SNAPSHOT the button was pressed with: closing a session is a
    // real I/O call, and firing sixteen at once to save milliseconds is how a UI lies about what
    // happened if one of them fails. The awaits are what make `closing` below mean anything — without
    // them the flag was cleared before the first close had even reached the device. The caller
    // re-renders with a fresh list after each one; this loop walks the sessions the OFFER named, which
    // is what the operator agreed to.
    for (const s of candidates) await onClose(s.sid);
    setClosing(false);
    setConfirming(false);
  };

  return (
    <div className="idle-bar" role="status">
      <span className="idle-text">{idleOfferText(candidates)}</span>
      {confirming ? (
        <>
          <button
            className="btn btn-mini"
            onClick={() => setConfirming(false)}
            disabled={closing}
          >
            Keep them
          </button>
          <button
            className="btn btn-danger btn-mini"
            onClick={closeAll}
            disabled={closing}
          >
            {closing ? "Closing…" : `Close ${candidates.length}`}
          </button>
        </>
      ) : (
        <button
          className="btn btn-ghost btn-mini"
          onClick={() => setConfirming(true)}
        >
          Close them
        </button>
      )}
      <button
        className="idle-x"
        aria-label="Hide until the list changes"
        title="Hide until the list changes"
        onClick={() => {
          setConfirming(false);
          setHiddenFor(signature);
        }}
      >
        <Icon name="close" />
      </button>
    </div>
  );
}
