import type { Session } from "./api";
import { copyText } from "./copy";

const DOCS_URL = "https://github.com/SilasVale/summrise/tree/main/docs";

/**
 * The account menu at the rail's foot (and in the header below 640 px). Settings and About do not
 * earn a destination — nothing here is a daily job — but the version, the console's URL and the way
 * out belong to the operator's session, so they live here.
 */
export function AccountMenu({
  session,
  consoleVersion,
  onSignOut,
  onConnect,
  onTokens,
  onNotice
}: {
  session: Session;
  consoleVersion: string | null;
  onSignOut: () => void;
  onConnect: () => void;
  onTokens: () => void;
  onNotice: (sentence: string) => void;
}) {
  const identity = session.email
    ? session.email
    : session.via === "access"
      ? "signed in through Cloudflare Access"
      : "signed in with the operator token";
  return (
    <details className="account">
      <summary aria-label="Account menu" title="Account">
        <span className="account-mark" aria-hidden="true">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="12" cy="8" r="3.5" />
            <path d="M5 20a7 7 0 0 1 14 0" />
          </svg>
        </span>
        <span className="account-label">Account</span>
      </summary>
      <div className="account-pop">
        <p className="hint account-identity">{identity}</p>
        <p className="account-meta">
          Console <span className="mono">{consoleVersion ?? "…"}</span>
        </p>
        <button type="button" onClick={onConnect}>
          Connect a client
        </button>
        <button type="button" onClick={onTokens}>
          Tokens
        </button>
        <button
          type="button"
          onClick={() =>
            void copyText(location.origin).then((ok) =>
              onNotice(ok ? "Console URL copied." : "Could not copy the console URL.")
            )
          }
        >
          Copy console URL
        </button>
        <a href={DOCS_URL} target="_blank" rel="noreferrer">
          Documentation
        </a>
        <button type="button" onClick={onSignOut}>
          Sign out
        </button>
      </div>
    </details>
  );
}
