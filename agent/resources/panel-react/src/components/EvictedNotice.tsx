// The line that says a session was taken away, and by which rule.
//
// Drawn where the monitor banners are drawn, for the same reason: the device did something on its
// own and the operator should not have to go looking for it. Quiet by construction — a hairline
// rule and dim text, no colour of alarm — because an eviction is housekeeping, not a fault.
import { Icon } from "../ui/Icon";
import { evictedText, type EvictionNotice } from "../lib/evicted";

export function EvictedNotice({
  notice,
  onDismiss,
}: {
  notice: EvictionNotice | null;
  onDismiss?: () => void;
}) {
  // Nothing happened → nothing is drawn. (Not an empty container: a strip that reserves space for a
  // message nobody wrote is a strip that lies about having something to say.)
  if (!notice) return null;
  return (
    <div className="evicted-notice" role="status">
      <span className="evicted-text">{evictedText(notice)}</span>
      {onDismiss && (
        <button className="evicted-x" onClick={onDismiss} aria-label="Dismiss">
          <Icon name="close" />
        </button>
      )}
    </div>
  );
}
