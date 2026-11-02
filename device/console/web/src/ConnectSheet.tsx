import { useCallback, useEffect, useMemo, useState } from "react";
import { api, type Created, type Device, type LiveTool, type TokenMeta } from "./api";
import { groupByCapability } from "./capabilities";
import { copyText } from "./copy";
import { message } from "./format";
import { FleetTokens } from "./FleetTokens";
import { Modal } from "./Modal";
import { Sheet } from "./Sheet";
import { useLiveTools } from "./useLiveTools";
import { useToolNames } from "./useToolNames";

export type ConnectSection = "endpoint" | "clients" | "tokens";

const SECTION_LABEL: Record<ConnectSection, string> = {
  endpoint: "Endpoint",
  clients: "Clients",
  tokens: "Tokens"
};

const SECTIONS: readonly ConnectSection[] = ["endpoint", "clients", "tokens"];

const TOKEN_EXPIRIES = [
  { seconds: 0, label: "Never" },
  { seconds: 30 * 86400, label: "30 days" },
  { seconds: 90 * 86400, label: "90 days" },
  { seconds: 365 * 86400, label: "365 days" }
];

const CLIENT_IDS = ["dsh", "claude", "codex", "cursor"] as const;
type ClientId = (typeof CLIENT_IDS)[number];
const CLIENT_LABELS: Record<ClientId, string> = {
  dsh: "DSH",
  claude: "Claude",
  codex: "Codex",
  cursor: "Cursor"
};

/** The client blocks: one endpoint and one `<token>` placeholder per client. */
function clientConfig(client: ClientId, url: string): string {
  if (client === "dsh") {
    return [
      "- id: mcp-summrise",
      "  name: '@deepseek-ai/dsh-mcp-client'",
      "  config:",
      "    serverName: summrise",
      "    transport: streamable-http",
      `    url: ${url}`,
      "    headers:",
      "      Authorization: Bearer <token>"
    ].join("\n");
  }
  if (client === "claude") {
    return `claude mcp add --transport http summrise ${url} \\
  --header "Authorization: Bearer <token>"`;
  }
  if (client === "codex") {
    return [
      `# set SUMMRISE_TOKEN = <token>`,
      "[mcp_servers.summrise]",
      `url = "${url}"`,
      'bearer_token_env_var = "SUMMRISE_TOKEN"'
    ].join("\n");
  }
  return JSON.stringify(
    {
      mcpServers: {
        summrise: { url, headers: { Authorization: "Bearer <token>" } }
      }
    },
    null,
    2
  );
}

function lastUsed(entry: TokenMeta): string {
  return entry.last_used_ms == null
    ? "never used"
    : `last used ${new Date(entry.last_used_ms).toLocaleDateString()}`;
}

function expiry(entry: TokenMeta): string {
  return entry.expires_at_ms == null
    ? "never expires"
    : `expires ${new Date(entry.expires_at_ms).toLocaleDateString()}`;
}

/**
 * Connect: hand the AI a door and a credential. Three sections in one sheet — the endpoint, the
 * client configuration for DSH/Claude/Codex/Cursor, and the token list plus its create form. The
 * title follows the entry point (`Connect a client` from Home/Device/account, `Tokens` from the
 * account menu); the token value is shown once and closing the sheet forgets it.
 */
export function ConnectSheet({
  device,
  devices,
  initialSection = "endpoint",
  onClose
}: {
  device: Device | null;
  devices: Device[];
  initialSection?: ConnectSection;
  onClose: () => void;
}) {
  const [section, setSection] = useState<ConnectSection>(initialSection);
  const [copied, setCopied] = useState<string | null>(null);
  const [copiedClient, setCopiedClient] = useState<ClientId | null>(null);
  const [client, setClient] = useState<ClientId>("dsh");
  const [notice, setNotice] = useState<string | null>(null);
  const live = useLiveTools(device?.id ?? null);
  const fallback = useToolNames();
  const endpoint = device ? `${location.origin}/mcp/${device.id}` : `${location.origin}/mcp/all`;
  const title = initialSection === "tokens" ? "Tokens" : "Connect a client";
  const offline = device?.online === false;

  const copy = async (what: string, text: string) => {
    if (await copyText(text)) {
      setCopied(what);
      setNotice(null);
      window.setTimeout(() => setCopied((current) => (current === what ? null : current)), 2000);
    } else {
      setNotice("Could not copy: the browser refused the clipboard.");
    }
  };

  const copyConfig = async (id: ClientId, text: string) => {
    if (await copyText(text)) {
      setCopiedClient(id);
      setNotice(null);
    } else {
      setNotice("Could not copy: the browser refused the clipboard.");
    }
  };

  return (
    <Sheet title={title} onClose={onClose}>
      <div className="connect-sheet">
        <div className="connect-tabs" role="tablist" aria-label="Connect sections">
          {SECTIONS.map((entry) => (
            <button
              key={entry}
              type="button"
              role="tab"
              aria-selected={section === entry}
              onClick={() => {
                setSection(entry);
                setCopied(null);
                setCopiedClient(null);
                setNotice(null);
              }}
            >
              {SECTION_LABEL[entry]}
            </button>
          ))}
        </div>

        {notice && <p className="error">{notice}</p>}

        {section === "endpoint" && (
          <div role="tabpanel" aria-label="Endpoint">
            <p className="hint">
              Point an MCP client at this URL with a bearer token created in Tokens.
            </p>
            <div className="endpoint-row">
              <code className={"endpoint mono" + (offline ? " dim" : "")}>{endpoint}</code>
              <button
                type="button"
                disabled={offline}
                title={offline ? "The machine is not connected" : undefined}
                onClick={() => void copy("endpoint", endpoint)}
              >
                {copied === "endpoint" ? "Copied" : "Copy"}
              </button>
            </div>
            <p className="hint">
              A bearer token is the credential the client sends with every request. It names the
              tools it may call, and revoking it stops the next call.
            </p>
            {!device && (
              <p className="hint">
                This endpoint spans the fleet: every tool takes a required <code>device</code>{" "}
                argument — a device id, or a name unique in the token's scope.
              </p>
            )}
            <details className="connect-config">
              <summary>Local loopback or through the console?</summary>
              <p className="hint">
                On the machine itself, its loopback endpoint needs no token. Through this console the
                URL above needs one and is reachable from anywhere the console is. Both reach the
                same runtime, the same tools and the same record.
              </p>
            </details>
          </div>
        )}

        {section === "clients" && (
          <div role="tabpanel" aria-label="Clients">
            <div className="connect-tabs" role="tablist" aria-label="Client">
              {CLIENT_IDS.map((id) => (
                <button
                  key={id}
                  type="button"
                  role="tab"
                  aria-selected={client === id}
                  onClick={() => {
                    setClient(id);
                    setCopiedClient(null);
                  }}
                >
                  {CLIENT_LABELS[id]}
                </button>
              ))}
            </div>
            <p className="hint">
              The token is created in Tokens and shown once; paste it where <code>&lt;token&gt;</code>{" "}
              is.
            </p>
            {copiedClient === client ? (
              <p className="plugin-valid" role="status">
                Copied. It is not shown again.
              </p>
            ) : (
              <pre className="code scroll">{clientConfig(client, endpoint)}</pre>
            )}
            <div className="connect-actions">
              <button
                type="button"
                onClick={() => void copyConfig(client, clientConfig(client, endpoint))}
              >
                {copiedClient === client ? "Copied" : "Copy configuration"}
              </button>
              <button type="button" onClick={() => setSection("tokens")}>
                Create a token
              </button>
            </div>
          </div>
        )}

        {section === "tokens" && (
          <div role="tabpanel" aria-label="Tokens">
            <TokensSection
              device={device}
              devices={devices}
              liveTools={live.tools}
              fallbackNames={fallback}
              endpoint={endpoint}
              offline={offline}
            />
          </div>
        )}
      </div>
    </Sheet>
  );
}

function TokensSection({
  device,
  devices,
  liveTools,
  fallbackNames,
  endpoint,
  offline
}: {
  device: Device | null;
  devices: Device[];
  liveTools: LiveTool[];
  fallbackNames: string[];
  endpoint: string;
  offline: boolean;
}) {
  const catalogue = useMemo(
    () =>
      liveTools.length > 0
        ? liveTools
        : fallbackNames.map((name) => ({
            name,
            description: "",
            inputSchema: {},
            source: "builtin",
            capability: name.split(".")[0],
            destructive: false
          })),
    [liveTools, fallbackNames]
  );
  const groups = useMemo(() => groupByCapability(catalogue), [catalogue]);
  const allNames = useMemo(() => catalogue.map((tool) => tool.name), [catalogue]);
  const readonlyNames = useMemo(
    () => catalogue.filter((tool) => !tool.destructive).map((tool) => tool.name),
    [catalogue]
  );

  const [name, setName] = useState("");
  const [preset, setPreset] = useState<"all" | "readonly" | "choose">("all");
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [touched, setTouched] = useState(false);
  const [tokenExpiry, setTokenExpiry] = useState(0);
  const [destructive, setDestructive] = useState(false);
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [created, setCreated] = useState<Created | null>(null);
  const [copied, setCopied] = useState(false);
  const [tokens, setTokens] = useState<TokenMeta[] | null>(null);
  const [tokensError, setTokensError] = useState<string | null>(null);
  const [rotating, setRotating] = useState<TokenMeta | null>(null);
  const [rotateBusy, setRotateBusy] = useState(false);
  const [rotateError, setRotateError] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<TokenMeta | null>(null);
  const [revokeBusy, setRevokeBusy] = useState(false);
  const [revokeError, setRevokeError] = useState<string | null>(null);

  const effectiveChosen = touched ? chosen : new Set(allNames);
  const selectedTools =
    preset === "all"
      ? allNames
      : preset === "readonly"
        ? readonlyNames
        : Array.from(effectiveChosen).filter((tool) => allNames.includes(tool));
  const destructiveCount = catalogue.filter(
    (tool) => tool.destructive && selectedTools.includes(tool.name)
  ).length;

  const refresh = useCallback(async () => {
    try {
      setTokens(await api.tokens());
      setTokensError(null);
    } catch (cause) {
      setTokensError(message(cause));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const toggle = (tool: string) => {
    const next = new Set(effectiveChosen);
    if (next.has(tool)) next.delete(tool);
    else next.add(tool);
    setTouched(true);
    setChosen(next);
  };

  const create = async () => {
    if (!device) return;
    setCreating(true);
    setCreateError(null);
    setCopied(false);
    try {
      setCreated(
        await api.createToken({
          name: name.trim(),
          device_id: device.id,
          tools: selectedTools,
          allow_destructive: destructive,
          ...(tokenExpiry > 0 ? { expires_in_s: tokenExpiry } : {})
        })
      );
      setName("");
      await refresh();
    } catch (cause) {
      // The console's additive sentence is the recovery copy; its code is the fallback.
      setCreateError(message(cause));
    } finally {
      setCreating(false);
    }
  };

  const rotate = async () => {
    if (!rotating) return;
    setRotateBusy(true);
    setRotateError(null);
    try {
      setCreated(await api.rotateToken(rotating.id));
      setRotating(null);
      await refresh();
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
      await refresh();
    } catch (cause) {
      setRevokeError(message(cause));
    } finally {
      setRevokeBusy(false);
    }
  };

  const mine = device
    ? (tokens ?? []).filter((entry) => entry.device_id === device.id)
    : [];

  return (
    <div className="tokens-section">
      {device ? (
        <>
          <p className="hint">
            A token is a credential the client keeps in its configuration. The console stores only
            its hash; the value is shown once, here.
          </p>

          {created ? (
            <div className="created-token">
              <p className="token-warning">Copy this token now. It is not shown again.</p>
              <div className="endpoint-row">
                <code className="endpoint mono">{created.token}</code>
                <button
                  type="button"
                  onClick={() =>
                    void copyText(created.token).then((ok) => {
                      if (ok) setCopied(true);
                    })
                  }
                >
                  {copied ? "Copied" : "Copy token"}
                </button>
              </div>
              <div className="connect-actions">
                <button type="button" onClick={() => setCreated(null)}>
                  Create another
                </button>
              </div>
            </div>
          ) : (
            <section className="token-create" aria-labelledby="create-token">
              <h3 id="create-token">Create a token</h3>
              <div className="field">
                <label htmlFor="token-name">Name</label>
                <input
                  id="token-name"
                  value={name}
                  maxLength={80}
                  placeholder="laptop-dsh"
                  onChange={(event) => setName(event.target.value)}
                />
              </div>

              <div className="seg" role="group" aria-label="Tool scope">
                {(["all", "readonly", "choose"] as const).map((entry) => (
                  <button
                    key={entry}
                    type="button"
                    aria-pressed={preset === entry}
                    onClick={() => setPreset(entry)}
                  >
                    {entry === "all" ? "All tools" : entry === "readonly" ? "Read-only" : "Choose tools…"}
                  </button>
                ))}
              </div>

              {preset === "choose" && (
                <div className="scope-groups">
                  {offline && (
                    <p className="hint">
                      The machine is offline. The list below is the static fallback; the console
                      refuses a plugin tool it cannot verify.
                    </p>
                  )}
                  {groups.map((group) => (
                    <fieldset className="scope-group" key={group.name}>
                      <legend>{group.name}</legend>
                      {group.tools.map((entry) => (
                        <label className="check mono" key={entry.name}>
                          <input
                            type="checkbox"
                            checked={effectiveChosen.has(entry.name)}
                            onChange={() => toggle(entry.name)}
                          />
                          {entry.name}
                        </label>
                      ))}
                    </fieldset>
                  ))}
                </div>
              )}

              <p className="hint">
                {selectedTools.length} tool{selectedTools.length === 1 ? "" : "s"} ·{" "}
                {destructiveCount} can change the machine
              </p>

              <div className="grant-row">
                <label htmlFor="token-expiry">Expires</label>
                <select
                  id="token-expiry"
                  value={tokenExpiry}
                  onChange={(event) => setTokenExpiry(Number(event.target.value))}
                >
                  {TOKEN_EXPIRIES.map((choice) => (
                    <option key={choice.seconds} value={choice.seconds}>
                      {choice.label}
                    </option>
                  ))}
                </select>
              </div>

              <label className="check">
                <input
                  type="checkbox"
                  checked={destructive}
                  onChange={(event) => setDestructive(event.target.checked)}
                />
                Allow destructive tools
              </label>
              <p className="hint">
                Without this, the machine refuses anything that changes it; read-only tools still
                work.
              </p>

              {createError && <p className="error">{createError}</p>}
              <div className="connect-actions">
                <button
                  type="button"
                  className="primary"
                  disabled={creating || name.trim() === "" || selectedTools.length === 0}
                  onClick={() => void create()}
                >
                  {creating ? "Creating…" : "Create token"}
                </button>
              </div>
            </section>
          )}

          {!created && (
            <section className="token-list" aria-label="Existing tokens">
              <h3>Existing tokens</h3>
              {tokens === null && !tokensError && <p className="hint">Loading…</p>}
              {tokens !== null && mine.length === 0 && (
                <p className="hint">
                  No permanent tokens for this device. One keeps a client's fixed configuration
                  working across restarts.
                </p>
              )}
              {mine.length > 0 && (
                <ul>
                  {mine.map((entry) => (
                    <li key={entry.id}>
                      <div>
                        <strong>{entry.name}</strong>{" "}
                        <span className="muted">
                          {new Date(entry.created_at_ms).toLocaleDateString()}
                        </span>
                      </div>
                      <div>
                        <span className="muted">
                          {entry.tools.length} tools ·{" "}
                          {entry.allow_destructive ? "destructive allowed" : "read-only"}
                        </span>{" "}
                        · <span className="muted">{expiry(entry)}</span> ·{" "}
                        <span className="muted">{lastUsed(entry)}</span>{" "}
                        <button
                          type="button"
                          onClick={() => {
                            setRotateError(null);
                            setRotating(entry);
                          }}
                        >
                          Rotate
                        </button>{" "}
                        <button
                          type="button"
                          onClick={() => {
                            setRevokeError(null);
                            setRevoking(entry);
                          }}
                        >
                          Revoke
                        </button>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
              {tokensError && <p className="error">{tokensError}</p>}
            </section>
          )}
        </>
      ) : (
        <p className="hint">
          The endpoint <code className="mono">{endpoint}</code> spans the fleet. Create a fleet token
          below; every tool on it takes a required <code>device</code> argument.
        </p>
      )}

      <details
        className={"fleet-token-disclosure" + (device === null ? " fleet-only" : "")}
        open={device === null}
      >
        <summary>Fleet tokens</summary>
        <FleetTokens devices={devices} tokens={tokens ?? []} embedded onChanged={() => void refresh()} />
      </details>

      {rotating && (
        <Modal
          title={`Rotate "${rotating.name}"?`}
          onClose={() => {
            if (!rotateBusy) setRotating(null);
          }}
        >
          <p className="hint">
            The old token stops working the moment this succeeds: every client still using it is
            refused on its next request, and the old value cannot be recovered.
          </p>
          <p className="hint">
            The new token keeps this one's name, device, tools and destructive consent. Its value is
            shown once, right after — the console keeps only its hash.
          </p>
          {rotateError && <p className="error">{rotateError}</p>}
          <div className="modal-actions">
            <button type="button" disabled={rotateBusy} onClick={() => setRotating(null)}>
              Cancel
            </button>
            <button type="button" className="primary" disabled={rotateBusy} onClick={() => void rotate()}>
              {rotateBusy ? "Rotating…" : "Rotate token"}
            </button>
          </div>
        </Modal>
      )}

      {revoking && (
        <Modal
          title={`Revoke "${revoking.name}"?`}
          onClose={() => {
            if (!revokeBusy) setRevoking(null);
          }}
        >
          <p className="hint">
            Any client using this token stops on its next request, and the token cannot be
            recovered. To keep the client working, create a new token and update its configuration.
          </p>
          {revokeError && <p className="error">{revokeError}</p>}
          <div className="modal-actions">
            <button type="button" disabled={revokeBusy} onClick={() => setRevoking(null)}>
              Cancel
            </button>
            <button type="button" className="primary" disabled={revokeBusy} onClick={() => void revoke()}>
              {revokeBusy ? "Revoking…" : "Revoke token"}
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}
