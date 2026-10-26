import { useEffect, useState } from "react";
import { api, type Device } from "./api";
import { copyText } from "./copy";
import { shortId } from "./format";
import { TOOL_NAMES } from "./GrantForm";
import { Modal } from "./Modal";

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

/** Lowercase hex of the UTF-8 bytes, the encoding the endpoint's bearer uses. */
function hexOf(text: string): string {
  return Array.from(new TextEncoder().encode(text), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

/**
 * The endpoint and the credential an MCP client needs, minted once and shown once.
 *
 * The card is the page's half of `docs/CONNECT-MCP.md`: it mints a grant through the operator session
 * the page already holds, then shows the URL, the `Authorization` header and a client configuration.
 * It never stores the grant: closing the dialog loses it, and minting another does not revoke the
 * first — the grant model has no revocation, only expiry.
 */
export function ConnectCard({ device, onClose }: { device: Device; onClose: () => void }) {
  const [tools, setTools] = useState<string[]>(TOOL_NAMES);
  const [lifetime, setLifetime] = useState(3600);
  const [destructive, setDestructive] = useState(false);
  const [issued, setIssued] = useState<Issued | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);

  // The registry is the list, as it is for the grant form; the fallback keeps the card usable when
  // the console cannot answer, and the mint's own `unknown_tool` refusal is the last word.
  useEffect(() => {
    let live = true;
    api
      .tools()
      .then((list) => {
        if (!live || list.length === 0) return;
        setTools(list.map((tool) => tool.name));
      })
      .catch(() => {
        /* keep the fallback */
      });
    return () => {
      live = false;
    };
  }, []);

  const name = device.name ?? shortId(device.id);
  const endpoint = `${location.origin}/mcp/${device.id}`;

  const mint = async () => {
    setBusy(true);
    setError(null);
    setCopied(null);
    try {
      const envelope = await api.grant(device.id, tools, lifetime, destructive);
      const expires = (envelope as { grant?: { expires_at_ms?: number } }).grant?.expires_at_ms;
      const header = `Bearer ${hexOf(JSON.stringify(envelope))}`;
      const serverName = `summrise-${device.id.slice(0, 12)}`;
      const config = [
        `- id: mcp-${serverName}`,
        `  name: '@deepseek-ai/dsh-mcp-client'`,
        `  config:`,
        `    serverName: ${serverName}`,
        `    transport: streamable-http`,
        `    url: ${endpoint}`,
        `    headers:`,
        `      Authorization: ${header}`
      ].join("\n");
      setIssued({
        endpoint,
        header,
        config,
        expiresAt: expires ?? Date.now() + lifetime * 1000
      });
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const copy = async (what: "endpoint" | "header" | "config", text: string) => {
    if (await copyText(text)) {
      setCopied(what);
      window.setTimeout(() => setCopied((current) => (current === what ? null : current)), 2000);
    }
  };

  return (
    <Modal title={`Connect an AI client to ${name}`} onClose={onClose}>
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
    </Modal>
  );
}
