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
  /** Close ONE session — the same call the tab's own × makes, so there is one close path. */
  onClose: (sid: string) => void;
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
    // Sequential, and the list is recomputed by the caller after each close: closing a session is
    // a real I/O call, and firing sixteen at once to save milliseconds is how a UI lies about what
    // happened if one of them fails.
    for (const s of candidates) onClose(s.sid);
    setClosing(false);
    setConfirming(false);
  };

  return (
    <div className="idle-bar" role="status">
      <span className="idle-text">{idleOfferText(candidates)}</span>
      {confirming ? (
        <>
          <button className="btn btn-mini" onClick={() => setConfirming(false)} disabled={closing}>
            Keep them
          </button>
          <button className="btn btn-danger btn-mini" onClick={closeAll} disabled={closing}>
            {closing ? "Closing…" : `Close ${candidates.length}`}
          </button>
        </>
      ) : (
        <button className="btn btn-ghost btn-mini" onClick={() => setConfirming(true)}>
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
