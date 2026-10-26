import { useEffect, useState } from "react";
import { api, type Created, type Device, type TokenMeta } from "./api";
import { copyText } from "./copy";
import { message, shortId } from "./format";
import { useToolNames } from "./useToolNames";
import { Modal } from "./Modal";
import { ToolPicker } from "./ToolPicker";

/** The lifetimes the card offers, and the sentence each one carries. */
const LIFETIMES = [
  { seconds: 900, label: "15 minutes" },
  { seconds: 3600, label: "1 hour" },
  { seconds: 86400, label: "24 hours" },
  { seconds: 604800, label: "7 days" }
];

/** What one mint produced: the two facts a client needs, and the YAML that carries them. */
type Issued = {
  endpoint: string;
  header: string;
  config: string;
  expiresAt: number;
};

type Mode = "session" | "token";

/** Lowercase hex of the UTF-8 bytes, the encoding the endpoint's bearer uses. */
function hexOf(text: string): string {
  return Array.from(new TextEncoder().encode(text), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

/** The DeepSeek Harness snippet both modes show: one endpoint, one credential, the same shape. */
function clientConfig(endpoint: string, deviceId: string, authorization: string): string {
  const serverName = `summrise-${deviceId.slice(0, 12)}`;
  return [
    `- id: mcp-${serverName}`,
    `  name: '@deepseek-ai/dsh-mcp-client'`,
    `  config:`,
    `    serverName: ${serverName}`,
    `    transport: streamable-http`,
    `    url: ${endpoint}`,
    `    headers:`,
    `      Authorization: ${authorization}`
  ].join("\n");
}

/**
 * Whether a token is still in use, in the row's muted line: the local day of its last call, or that
 * it has never made one. A token used today and one used a month ago are the same shape otherwise.
 */
function lastUsed(entry: TokenMeta): string {
  return entry.last_used_ms == null
    ? "never used"
    : `last used ${new Date(entry.last_used_ms).toLocaleDateString()}`;
}

/**
 * The endpoint and the credential an MCP client needs, minted once and shown once.
 *
 * The card is the page's half of `docs/CONNECT-MCP.md`: it mints a grant through the operator session
 * the page already holds, then shows the URL, the `Authorization` header and a client configuration.
 * It never stores the grant: closing the dialog loses it, and minting another does not revoke the
 * first — the grant model has no revocation, only expiry.
 *
 * Its other mode mints a **permanent token** instead: the console keeps only its hash, the plaintext
 * is returned once, and a client pastes it into a fixed configuration. The token is a password — the
 * list below the form carries metadata only, and revocation is the only withdrawal.
 */
export function ConnectCard({ device, onClose }: { device: Device; onClose: () => void }) {
  // The registry is the list, as it is for the grant form; the fallback keeps the card usable when
  // the console cannot answer, and the mint's own `unknown_tool` refusal is the last word.
  const tools = useToolNames();
  const [mode, setMode] = useState<Mode>("session");

  // The session-grant half. Untouched by the token mode beside it.
  const [lifetime, setLifetime] = useState(3600);
  const [destructive, setDestructive] = useState(false);
  const [issued, setIssued] = useState<Issued | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);

  // The permanent-token half: the form, the one plaintext the console returns, and the metadata list.
  const [tokenName, setTokenName] = useState("");
  const [chosen, setChosen] = useState<Set<string>>(new Set(tools));
  const [tokenDestructive, setTokenDestructive] = useState(false);
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [created, setCreated] = useState<Created | null>(null);
  const [tokenList, setTokenList] = useState<TokenMeta[] | null>(null);
  const [tokenListError, setTokenListError] = useState<string | null>(null);
  const [rotating, setRotating] = useState<TokenMeta | null>(null);
  const [rotateBusy, setRotateBusy] = useState(false);
  const [rotateError, setRotateError] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<TokenMeta | null>(null);
  const [revokeBusy, setRevokeBusy] = useState(false);
  const [revokeError, setRevokeError] = useState<string | null>(null);

  // Everything selected to begin with, exactly as the grant form does: a half-selected list is the
  // reason a first call fails with "tool not permitted". The served catalogue replaces the fallback
  // when it lands, and a failed or empty answer leaves the selection alone.
  useEffect(() => {
    setChosen(new Set(tools));
  }, [tools]);

  const name = device.name ?? shortId(device.id);
  const endpoint = `${location.origin}/mcp/${device.id}`;

  const refreshTokens = async () => {
    try {
      setTokenList(await api.tokens());
      setTokenListError(null);
    } catch (cause) {
      setTokenListError(message(cause));
    }
  };

  // The list is asked for when the mode opens, and again after a create or a revoke. It is metadata:
  // nothing here ever holds a token value except `created`, which is cleared by minting another.
  useEffect(() => {
    if (mode === "token") void refreshTokens();
  }, [mode]);

  const mint = async () => {
    setBusy(true);
    setError(null);
    setCopied(null);
    try {
      const envelope = await api.grant(device.id, tools, lifetime, destructive);
      const expires = (envelope as { grant?: { expires_at_ms?: number } }).grant?.expires_at_ms;
      const header = `Bearer ${hexOf(JSON.stringify(envelope))}`;
      setIssued({
        endpoint,
        header,
        config: clientConfig(endpoint, device.id, header),
        expiresAt: expires ?? Date.now() + lifetime * 1000
      });
    } catch (cause) {
      setError(message(cause));
    } finally {
      setBusy(false);
    }
  };

  const tokenConfig = (token: string) => clientConfig(endpoint, device.id, `Bearer ${token}`);

  const createPermanent = async () => {
    setCreating(true);
    setCreateError(null);
    setCopied(null);
    try {
      setCreated(
        await api.createToken({
          name: tokenName.trim(),
          device_id: device.id,
          tools: tools.filter((tool) => chosen.has(tool)),
          allow_destructive: tokenDestructive
        })
      );
      await refreshTokens();
    } catch (cause) {
      setCreateError(message(cause));
    } finally {
      setCreating(false);
    }
  };

  // A rotation is the create's one-time view with the old record's scope: the console answers with the
  // new plaintext, the list is read again so the same name carries the new date, and the value is
  // cleared by minting another exactly as a create's is.
  const rotate = async () => {
    if (!rotating) return;
    setRotateBusy(true);
    setRotateError(null);
    setCopied(null);
    try {
      setCreated(await api.rotateToken(rotating.id));
      setCreateError(null);
      setRotating(null);
      await refreshTokens();
    } catch (cause) {
      setRotateError(message(cause));
    } finally {
      setRotateBusy(false);
    }
  };

  const revoke = async () => {
    if (!revoking) return;
    setRevokeBusy(true);
    setRevokeError(null);
    try {
      await api.revokeToken(revoking.id);
      setRevoking(null);
      await refreshTokens();
    } catch (cause) {
      setRevokeError(message(cause));
    } finally {
      setRevokeBusy(false);
    }
  };

  const copy = async (what: string, text: string) => {
    if (await copyText(text)) {
      setCopied(what);
      window.setTimeout(() => setCopied((current) => (current === what ? null : current)), 2000);
    }
  };

  const mine = (tokenList ?? []).filter((entry) => entry.device_id === device.id);

  return (
    <Modal title={`Connect an AI client to ${name}`} onClose={onClose}>
      <div className="connect-tabs" role="tablist" aria-label="Credential">
        <button
          type="button"
          role="tab"
          aria-selected={mode === "session"}
          onClick={() => setMode("session")}
        >
          Session grant
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={mode === "token"}
          onClick={() => setMode("token")}
        >
          Permanent token
        </button>
      </div>

      {mode === "session" && (
        <div role="tabpanel" aria-label="Session grant">
          <p className="hint">
            An MCP client reaches this device through the console. Mint a grant, then put the endpoint and
            the header into the client&apos;s configuration. The grant is shown once and expires.
          </p>

          {!issued && (
            <div className="connect">
              <div className="grant-row">
                <label htmlFor="connect-lifetime">Grant expires after</label>
                <select id="connect-lifetime" value={lifetime} onChange={(event) => setLifetime(Number(event.target.value))}>
                  {LIFETIMES.map((choice) => (
                    <option key={choice.seconds} value={choice.seconds}>{choice.label}</option>
                  ))}
                </select>
              </div>
              <label className="check">
                <input type="checkbox" checked={destructive} onChange={(event) => setDestructive(event.target.checked)} />
                Allow destructive tools
              </label>
              <p className="hint">
                The grant covers every tool the device declares. Without destructive consent the device refuses{" "}
                <span className="mono">terminal.create</span>, <span className="mono">terminal.write</span>,{" "}
                <span className="mono">files.write</span>, <span className="mono">browser.navigate</span>,{" "}
                <span className="mono">runs.exec</span> and the memory writes; the read tools still work.
              </p>
              <button type="button" className="primary" disabled={busy} onClick={() => void mint()}>
                {busy ? "Signing…" : "Mint a grant and show the config"}
              </button>
              {error && <p className="error">{error}</p>}
            </div>
          )}

          {issued && (
            <div className="connect">
              <p className="hint">
                This header is the credential. It expires{" "}
                <strong>{new Date(issued.expiresAt).toLocaleString()}</strong>; anyone who has it can call the
                grant&apos;s tools on this device until then. It is not stored, and minting another does not
                revoke it.
              </p>
              <pre className="code scroll">{`URL: ${issued.endpoint}\nAuthorization: ${issued.header}`}</pre>
              <div className="connect-actions">
                <button type="button" onClick={() => void copy("endpoint", issued.endpoint)}>
                  {copied === "endpoint" ? "Copied" : "Copy endpoint"}
                </button>
                <button type="button" onClick={() => void copy("header", issued.header)}>
                  {copied === "header" ? "Copied" : "Copy header"}
                </button>
                <button type="button" onClick={() => void copy("config", issued.config)}>
                  {copied === "config" ? "Copied" : "Copy client config"}
                </button>
              </div>
              <details className="connect-config">
                <summary>DeepSeek Harness configuration</summary>
                <pre className="code scroll">{issued.config}</pre>
              </details>
              <p className="hint">
                If this console is behind Cloudflare Access, the <span className="mono">/mcp/*</span>{" "}
                application is a separate gate: the client needs whatever it requires — a bypass, or a
                service token.
              </p>
              <div className="modal-actions">
                <button type="button" onClick={() => setIssued(null)}>Mint another</button>
                <button type="button" className="primary" onClick={onClose}>Done</button>
              </div>
            </div>
          )}

          {!issued && (
            <div className="modal-actions">
              <button type="button" onClick={onClose}>Cancel</button>
            </div>
          )}
        </div>
      )}

      {mode === "token" && (
        <div role="tabpanel" aria-label="Permanent token" className="connect">
          <p className="hint">
            A permanent token is a credential the client keeps in a fixed MCP configuration: it is not
            tied to one run, and it works until you revoke it. The console stores only its hash and
            checks the device on every request.
          </p>

          {!created && (
            <div className="connect">
              <div className="grant-row">
                <label htmlFor="token-name">Token name</label>
                <input
                  id="token-name"
                  type="text"
                  placeholder="laptop-dsh"
                  maxLength={80}
                  value={tokenName}
                  onChange={(event) => setTokenName(event.target.value)}
                />
              </div>

              <ToolPicker names={tools} chosen={chosen} onChange={setChosen} />

              <label className="check">
                <input type="checkbox" checked={tokenDestructive} onChange={(event) => setTokenDestructive(event.target.checked)} />
                Allow destructive tools
              </label>

              <button
                type="button"
                className="primary"
                disabled={creating || tokenName.trim() === "" || chosen.size === 0}
                onClick={() => void createPermanent()}
              >
                {creating ? "Creating…" : "Create token"}
              </button>
              {createError && <p className="error">{createError}</p>}
            </div>
          )}

          {created && (
            <div className="connect">
              <p className="hint">
                This token is shown once and never again: the console keeps only its hash. Treat it like a
                password — anyone who has it can call this device&apos;s tools until you revoke it, so store
                it as a secret and keep it out of a commit.
              </p>
              <pre className="code scroll">{created.token}</pre>
              <div className="connect-actions">
                <button type="button" onClick={() => void copy("token", created.token)}>
                  {copied === "token" ? "Copied" : "Copy token"}
                </button>
              </div>
              <p className="hint">
                The client configuration, with the token as the bearer. The value is the credential: put
                it in a secret or an environment variable, not in the repository.
              </p>
              <pre className="code scroll">{tokenConfig(created.token)}</pre>
              <div className="connect-actions">
                <button type="button" onClick={() => void copy("token-config", tokenConfig(created.token))}>
                  {copied === "token-config" ? "Copied" : "Copy client config"}
                </button>
              </div>
              <div className="modal-actions">
                <button type="button" onClick={() => { setCreated(null); setCreateError(null); }}>Create another</button>
                <button type="button" className="primary" onClick={onClose}>Done</button>
              </div>
            </div>
          )}

          <section className="token-list" aria-label="Existing tokens">
            <h3>Existing tokens</h3>
            {tokenList === null && !tokenListError && <p className="hint">Loading…</p>}
            {tokenList !== null && mine.length === 0 && (
              <p className="hint">
                No permanent tokens for this device. One keeps a client&apos;s fixed MCP configuration
                working across restarts; create one above.
              </p>
            )}
            {mine.length > 0 && (
              <ul>
                {mine.map((entry) => (
                  <li key={entry.id}>
                    <div>
                      <strong>{entry.name}</strong>{" "}
                      <span className="muted">{new Date(entry.created_at_ms).toLocaleDateString()}</span>
                    </div>
                    <div>
                      <span className="muted">
                        {entry.tools.join(", ")}
                        {entry.allow_destructive ? " · destructive allowed" : " · read-only"}
                      </span>{" · "}
                      <span className="muted">{lastUsed(entry)}</span>{" "}
                      <button type="button" onClick={() => { setRotateError(null); setRotating(entry); }}>
                        Rotate
                      </button>{" "}
                      <button type="button" onClick={() => { setRevokeError(null); setRevoking(entry); }}>
                        Revoke
                      </button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
            {tokenListError && <p className="error">{tokenListError}</p>}
          </section>
        </div>
      )}

      {rotating && (
        <Modal title={`Rotate "${rotating.name}"?`} onClose={() => { if (!rotateBusy) setRotating(null); }}>
          <p className="hint">
            The old token stops working the moment this succeeds: every client still using it is
            refused on its next request, and the old value cannot be recovered.
          </p>
          <p className="hint">
            The new token keeps this one&apos;s name, device, tools and destructive consent. Its value
            is shown once, right after — the console keeps only its hash — so update the client&apos;s
            configuration then.
          </p>
          {rotateError && <p className="error">{rotateError}</p>}
          <div className="modal-actions">
            <button type="button" disabled={rotateBusy} onClick={() => setRotating(null)}>Cancel</button>
            <button type="button" className="primary" disabled={rotateBusy} onClick={() => void rotate()}>
              {rotateBusy ? "Rotating…" : "Rotate token"}
            </button>
          </div>
        </Modal>
      )}

      {revoking && (
        <Modal title={`Revoke "${revoking.name}"?`} onClose={() => { if (!revokeBusy) setRevoking(null); }}>
          <p className="hint">
            Any client using this token stops on its next request, and the token cannot be recovered.
            To keep the client working, create a new token and update its configuration.
          </p>
          {revokeError && <p className="error">{revokeError}</p>}
          <div className="modal-actions">
            <button type="button" disabled={revokeBusy} onClick={() => setRevoking(null)}>Cancel</button>
            <button type="button" className="primary" disabled={revokeBusy} onClick={() => void revoke()}>
              {revokeBusy ? "Revoking…" : "Revoke token"}
            </button>
          </div>
        </Modal>
      )}
    </Modal>
  );
}
