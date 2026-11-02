import { useEffect, useState } from "react";
import { api, type Created, type Device, type TokenMeta } from "./api";
import { copyText } from "./copy";
import { message } from "./format";
import { Modal } from "./Modal";
import { ToolPicker } from "./ToolPicker";
import { useToolNames } from "./useToolNames";

/** The client block for the aggregate endpoint: one server, the token, and the `device` argument. */
function fleetConfig(token: string): string {
  return [
    `- id: mcp-summrise`,
    `  name: '@deepseek-ai/dsh-mcp-client'`,
    `  config:`,
    `    serverName: summrise`,
    `    transport: streamable-http`,
    `    url: ${location.origin}/mcp/all`,
    `    headers:`,
    `      Authorization: Bearer ${token}`,
  ].join("\n");
}

/**
 * The fleet-level permanent tokens: one credential for several devices.
 *
 * This is the surface the per-device Connect card cannot carry — that card is opened for one device,
 * and the token list it shows filters on that device. A fleet token has an empty `device_id` and a
 * non-empty `device_ids`; it is accepted only on `/mcp/all`, where every tool takes a required
 * `device` argument. The create answer is the only time the plaintext exists.
 */
export function FleetTokens({
  devices,
  tokens,
  onChanged,
}: {
  devices: Device[];
  tokens: TokenMeta[];
  onChanged: () => void;
}) {
  const tools = useToolNames();
  const [name, setName] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [chosen, setChosen] = useState<Set<string>>(new Set(tools));
  const [destructive, setDestructive] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [created, setCreated] = useState<Created | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<TokenMeta | null>(null);
  const [revokeBusy, setRevokeBusy] = useState(false);
  const [revokeError, setRevokeError] = useState<string | null>(null);

  // Everything selected to begin with, as the per-device form does: a half-selected tool list is the
  // reason a first call fails with "tool not permitted". A served catalogue replaces the fallback.
  useEffect(() => {
    setChosen(new Set(tools));
  }, [tools]);

  const fleet = tokens.filter((entry) => (entry.device_ids?.length ?? 0) > 0);
  const names = new Map(
    devices.map((device) => [device.id, device.name ?? device.id.slice(0, 12)]),
  );

  const toggleDevice = (id: string) => {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setSelected(next);
  };

  const copy = async (what: string, text: string) => {
    if (await copyText(text)) {
      setCopied(what);
      window.setTimeout(
        () => setCopied((current) => (current === what ? null : current)),
        2000,
      );
    }
  };

  const create = async () => {
    setBusy(true);
    setError(null);
    setCopied(null);
    try {
      setCreated(
        await api.createToken({
          name: name.trim(),
          device_ids: [...selected],
          tools: tools.filter((tool) => chosen.has(tool)),
          allow_destructive: destructive,
        }),
      );
      onChanged();
    } catch (cause) {
      setError(message(cause));
    } finally {
      setBusy(false);
    }
  };

  const revoke = async () => {
    if (!revoking) return;
    setRevokeBusy(true);
    setRevokeError(null);
    try {
      await api.revokeToken(revoking.id);
      setRevoking(null);
      onChanged();
    } catch (cause) {
      setRevokeError(message(cause));
    } finally {
      setRevokeBusy(false);
    }
  };

  const reset = () => {
    setCreated(null);
    setError(null);
    setName("");
    setSelected(new Set());
    setDestructive(false);
  };

  return (
    <section className="panel">
      <div className="panel-head">
        <h2>Fleet tokens</h2>
      </div>
      <p className="hint">
        One permanent token for several devices. It is accepted only on{" "}
        <code>/mcp/all</code>; every tool there takes a required{" "}
        <code>device</code> argument (a device id, or a name unique in the
        token&apos;s scope). A device token is refused there, and a fleet token
        is refused on a single-device endpoint.
      </p>

      {created ? (
        <div className="connect">
          <p className="hint">
            This is the only time the token is shown: the console stores only
            its hash. Copy it into the client configuration now; a lost value
            means creating another.
          </p>
          <div className="grant-row">
            <code className="mono">{created.token}</code>
            <button
              type="button"
              onClick={() => void copy("secret", created.token)}
            >
              {copied === "secret" ? "Copied" : "Copy token"}
            </button>
          </div>
          <pre className="config">{fleetConfig(created.token)}</pre>
          <div className="modal-actions">
            <button
              type="button"
              onClick={() => void copy("config", fleetConfig(created.token))}
            >
              {copied === "config" ? "Copied" : "Copy client config"}
            </button>
            <button type="button" className="primary" onClick={reset}>
              Create another
            </button>
          </div>
          <p className="hint">
            Behind Cloudflare Access, add the two <code>CF-Access-*</code>{" "}
            headers for this deployment&apos;s service token; the
            endpoint&apos;s policy decides whether they are needed.
          </p>
        </div>
      ) : (
        <div className="connect">
          <div className="grant-row">
            <label htmlFor="fleet-token-name">Name</label>
            <input
              id="fleet-token-name"
              value={name}
              placeholder="e.g. dsh"
              onChange={(event) => setName(event.target.value)}
            />
          </div>

          <fieldset className="tools">
            <legend>
              Devices ({selected.size} of {devices.length})
            </legend>
            <div className="tool-grid">
              {devices.map((device) => (
                <label key={device.id} className="check">
                  <input
                    type="checkbox"
                    checked={selected.has(device.id)}
                    onChange={() => toggleDevice(device.id)}
                  />
                  {device.name ?? device.id.slice(0, 12)}
                </label>
              ))}
            </div>
          </fieldset>

          <ToolPicker names={tools} chosen={chosen} onChange={setChosen} />

          <label className="check">
            <input
              type="checkbox"
              checked={destructive}
              onChange={(event) => setDestructive(event.target.checked)}
            />
            Allow destructive tools
          </label>

          {error && <p className="error">{error}</p>}

          <div className="modal-actions">
            <button
              type="button"
              className="primary"
              disabled={
                busy ||
                name.trim() === "" ||
                selected.size === 0 ||
                chosen.size === 0
              }
              onClick={() => void create()}
            >
              {busy ? "Creating…" : "Create fleet token"}
            </button>
          </div>
        </div>
      )}

      <section className="token-list" aria-label="Existing fleet tokens">
        <h3>Existing fleet tokens</h3>
        {fleet.length === 0 ? (
          <p className="hint">
            No fleet tokens. One keeps a multi-device client configuration
            working across restarts; create one above.
          </p>
        ) : (
          <ul>
            {fleet.map((entry) => (
              <li key={entry.id}>
                <div>
                  <strong>{entry.name}</strong>{" "}
                  <span className="muted">
                    {new Date(entry.created_at_ms).toLocaleDateString()}
                  </span>
                </div>
                <div>
                  <span className="muted">
                    {(entry.device_ids ?? [])
                      .map((id) => names.get(id) ?? id.slice(0, 12))
                      .join(", ")}
                  </span>{" "}
                  · <span className="muted">{entry.tools.length} tools</span> ·{" "}
                  <span className="muted">
                    {entry.allow_destructive
                      ? "destructive allowed"
                      : "read-only"}
                  </span>{" "}
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
      </section>

      {revoking && (
        <Modal
          title={`Revoke "${revoking.name}"?`}
          onClose={() => {
            if (!revokeBusy) setRevoking(null);
          }}
        >
          <p className="hint">
            Any client using this token stops on its next request, and the token
            cannot be recovered. The devices themselves are unaffected.
          </p>
          {revokeError && <p className="error">{revokeError}</p>}
          <div className="modal-actions">
            <button
              type="button"
              disabled={revokeBusy}
              onClick={() => setRevoking(null)}
            >
              Cancel
            </button>
            <button
              type="button"
              className="primary"
              disabled={revokeBusy}
              onClick={() => void revoke()}
            >
              {revokeBusy ? "Revoking…" : "Revoke token"}
            </button>
          </div>
        </Modal>
      )}
    </section>
  );
}
