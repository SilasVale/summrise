import type { AuditLine, Device, TokenMeta } from "./api";
import { AuditTable } from "./AuditTable";

/**
 * The console's failed authentications, read-only.
 *
 * These are the requests that never reached an action: a bad operator token, a missing or expired
 * session, a bad device token, a failed registration proof, an unknown or revoked API token. They
 * live in their own file rather than the audit because an unauthenticated caller can generate them
 * at will; the page renders that file with the audit's table, which already has the right columns.
 */
export function SecurityPage({
  lines,
  devices,
  tokens
}: {
  lines: AuditLine[];
  devices: Device[];
  tokens: TokenMeta[];
}) {
  return (
    <section className="panel">
      <div className="panel-head">
        <h2>Security</h2>
      </div>
      <p className="hint">
        Failed authentications are recorded here, separately from the audit. The log is rate-limited
        to 100/minute, and a suppressed N line means N refusals were not written.
      </p>
      <AuditTable
        lines={lines}
        devices={devices}
        tokens={tokens}
        empty="No failed authentications yet."
        emptyFiltered="No failed authentication matches this filter."
      />
    </section>
  );
}
