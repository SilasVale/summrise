import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type Created, type Device, type LiveTool, type TokenMeta } from "./api";
import { groupByCapability } from "./capabilities";
import { copyText } from "./copy";
import { exactMoment, isoDate, lastHeardMs, message, seenAgo, shortId } from "./format";
import { Modal } from "./Modal";
import { Sheet } from "./Sheet";
import { useLiveTools } from "./useLiveTools";
import { useToolCatalogue, type CatalogueTool } from "./useToolCatalogue";
import { mergeTokenRows, scopeLabel, tokenScope, type TokenScope } from "./tokenScopes";

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

/**
 * One absolute moment on the sheet: the ISO day a person reads, with the exact time in its title.
 *
 * The token sheet's only absolute dates share this element. `toLocaleDateString()` put the browser's
 * order in the cell — `11/4/2026` in en-US, another order in every other locale — which is not a
 * date an operator can read without knowing the browser's locale first.
 */
function Moment({ ts }: { ts: number }) {
  return (
    <time dateTime={new Date(ts).toISOString()} title={exactMoment(ts)}>
      {isoDate(ts)}
    </time>
  );
}

/**
 * One machine and its silence, for a sentence about that machine alone.
 *
 * The moment and the wording are the fleet row's own: `lastHeardMs` picks the newest evidence and
 * `seenAgo` keeps the minutes, so no surface can date a silence differently from the row behind it.
 * A machine the console has never heard has no age to state, and the console's existing wording for
 * it is just "offline" — the sheet does not invent "never seen".
 */
function offlineMachine(device: Device): string {
  const name = device.name || shortId(device.id);
  const heard = lastHeardMs(device);
  return heard === null
    ? `${name} (offline)`
    : `${name} (offline, last seen ${seenAgo(heard)})`;
}

/**
 * One machine in the all-machines coverage sentence, with the presence the fleet read serves.
 *
 * The list names the state of every machine because the token covers every enrolled machine, and
 * the state is a statement about the scope, not a filter of it. The one-machine form above states
 * only the state that is news.
 */
function coverageMachine(device: Device): string {
  return device.online
    ? `${device.name || shortId(device.id)} (online)`
    : offlineMachine(device);
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
  focusTokenId = null,
  onClose
}: {
  device: Device | null;
  devices: Device[];
  initialSection?: ConnectSection;
  /**
   * The token a record named: open the Tokens list scrolled to that row. The account menu and the
   * endpoint entry point pass none, and the list opens as it always did.
   */
  focusTokenId?: string | null;
  onClose: () => void;
}) {
  const [section, setSection] = useState<ConnectSection>(initialSection);
  const [copied, setCopied] = useState<string | null>(null);
  const [copiedClient, setCopiedClient] = useState<ClientId | null>(null);
  const [client, setClient] = useState<ClientId>("dsh");
  const [notice, setNotice] = useState<string | null>(null);
  const live = useLiveTools(device?.id ?? null);
  const registry = useToolCatalogue();
  const endpoint = device ? `${location.origin}/mcp/${device.id}` : `${location.origin}/mcp/all`;
  // The token kind this sheet's URL takes. The URL above is one door; a token of the other kind is
  // refused there, so the copy says which credential belongs here and where the other one is used.
  const doorSentence = device
    ? `This endpoint, ${endpoint}, takes a device token minted for ` +
      `${device.name || shortId(device.id)}. A fleet token is accepted only at ` +
      `${location.origin}/mcp/all.`
    : `This endpoint, ${endpoint}, takes a fleet token. A device token is accepted only at its ` +
      "own machine's URL, whose path is /mcp/<device id>.";
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
            <p className="hint">{doorSentence}</p>
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
            <p className="hint">{doorSentence}</p>
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
              registryTools={registry}
              liveError={live.error}
              liveOffline={live.offline}
              endpoint={endpoint}
              offline={offline}
              focusTokenId={focusTokenId}
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
  registryTools,
  liveError,
  liveOffline,
  endpoint,
  offline,
  focusTokenId
}: {
  device: Device | null;
  devices: Device[];
  liveTools: LiveTool[];
  registryTools: CatalogueTool[];
  liveError: string | null;
  liveOffline: boolean;
  endpoint: string;
  offline: boolean;
  focusTokenId: string | null;
}) {
  // The minted token's scope: one machine (the sheet's device) or the fleet. With no current
  // machine, the fleet is the only scope a token can be minted for.
  const [scope, setScope] = useState<TokenScope>(device ? "device" : "fleet");
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
  const [copiedConfig, setCopiedConfig] = useState(false);
  const [tokens, setTokens] = useState<TokenMeta[] | null>(null);
  const [tokensError, setTokensError] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [rotating, setRotating] = useState<TokenMeta | null>(null);
  const [rotateBusy, setRotateBusy] = useState(false);
  const [rotateError, setRotateError] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<TokenMeta | null>(null);
  const [revokeBusy, setRevokeBusy] = useState(false);
  const [revokeError, setRevokeError] = useState<string | null>(null);
  // The row a record's Manage asked for. A ref rather than an effect over the whole table: the
  // list is drawn from a fetch this component owns, so the row exists only after `tokens` answers.
  const focusedRow = useRef<HTMLTableRowElement | null>(null);

  // The catalogue the form counts: the selected machine's live list when it is in hand, otherwise
  // the registry `/tools` serves. The fleet form has no selected machine, so the registry is its
  // only catalogue — and its entries carry the same destructive flags the live list does.
  const catalogue = useMemo(
    () =>
      liveTools.length > 0
        ? liveTools
        : registryTools.map((tool) => ({
            name: tool.name,
            description: "",
            inputSchema: {},
            source: "builtin",
            capability: tool.name.split(".")[0],
            destructive: tool.destructive === true
          })),
    [liveTools, registryTools]
  );
  // The embedded fallback has names but no flags. Printing `0 can change the machine` over it was
  // a claim the page had not read: the count is stated only when every flag came from the live list
  // or the registry, and otherwise the sentence says where the count is read.
  const destructiveKnown =
    liveTools.length > 0 ||
    (registryTools.length > 0 && registryTools.every((tool) => tool.destructive !== null));
  // WHICH CATALOGUE THE COUNT IS. The machine's live list is the count when one is in hand — after
  // a failed read that is the last tool list the console read — and the console registry is the
  // count otherwise. An offline machine or one whose list could not be read must not have the
  // registry's count written as the machine's: the label names the catalogue, and the sentence
  // above the presets says why it is the fallback. The fleet form has no machine to attribute and
  // keeps its one-list-for-every-machine suffix.
  const usingLiveList = liveTools.length > 0;
  const machineOffline = offline || liveOffline;
  const listUnreadable = liveError !== null;
  const countFromRegistry = device !== null && !usingLiveList && (machineOffline || listUnreadable);
  const countFromLastLive = device !== null && usingLiveList && (machineOffline || listUnreadable);
  const registryReason = machineOffline
    ? "the machine is offline"
    : "the machine's live list could not be read";
  const lastLiveReason = machineOffline
    ? "the machine is offline"
    : "it could not be refreshed";
  const catalogueLabel = countFromRegistry
    ? ` (console registry; ${registryReason})`
    : countFromLastLive
      ? ` (last tool list the console read; ${lastLiveReason})`
      : "";
  // The token still covers an offline machine, and the one-machine form now says the state the
  // fleet form says for the same machine, with the fleet row's own last-seen evidence.
  const coverDevice = device
    ? devices.find((entry) => entry.id === device.id) ?? device
    : null;
  const coverName =
    device === null
      ? ""
      : machineOffline && coverDevice
        ? offlineMachine(coverDevice)
        : device.name || shortId(device.id);
  // A fleet token applies one tool list to every machine, and v1 refuses a plugin tool there:
  // only the device that advertises it can vouch for it. A device token still offers the whole
  // live catalogue.
  const scopedCatalogue = useMemo(
    () => (scope === "fleet" ? catalogue.filter((tool) => tool.source === "builtin") : catalogue),
    [catalogue, scope]
  );
  const groups = useMemo(() => groupByCapability(scopedCatalogue), [scopedCatalogue]);
  const allNames = useMemo(() => scopedCatalogue.map((tool) => tool.name), [scopedCatalogue]);
  const readonlyNames = useMemo(
    () => scopedCatalogue.filter((tool) => !tool.destructive).map((tool) => tool.name),
    [scopedCatalogue]
  );

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

  /**
   * Read the whole registry, newest first.
   *
   * `GET /tokens` is the whole registry and the only list the console serves (there is no
   * `/tokens/<device>` route; asking for one made every Tokens open log a 405). Every record is
   * shown, whatever machine it was minted for: a row scoped elsewhere is a real token that exists,
   * and hiding it made the list look empty when it was not. The Scope column, not a filter, says
   * which machine each row reaches. A failed read keeps the list already on screen and says why.
   */
  const refresh = useCallback(async (justCreated?: TokenMeta) => {
    try {
      const whole = await api.tokens();
      setTokens(mergeTokenRows(whole, justCreated ? [justCreated] : []));
      setTokensError(null);
    } catch (cause) {
      setTokensError(message(cause));
      if (justCreated) setTokens((previous) => mergeTokenRows(previous ?? [], [justCreated]));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Bring the token a record named into view once the list has answered. `scrollIntoView` is absent
  // in jsdom and in a runtime that cannot scroll; the row's mark still says which record this is.
  useEffect(() => {
    if (!focusTokenId) return;
    focusedRow.current?.scrollIntoView?.({ block: "center", behavior: "smooth" });
  }, [focusTokenId, tokens]);

  const toggle = (tool: string) => {
    const next = new Set(effectiveChosen);
    if (next.has(tool)) next.delete(tool);
    else next.add(tool);
    setTouched(true);
    setChosen(next);
  };

  const switchScope = (next: TokenScope) => {
    setScope(next);
    // The two scopes may offer different catalogues (the fleet hides plugin tools); starting a
    // scope fresh keeps a stale selection from carrying a tool the new scope cannot carry.
    setTouched(false);
    setChosen(new Set());
    setCreateError(null);
  };

  const create = async () => {
    if (scope === "device" && !device) return;
    setCreating(true);
    setCreateError(null);
    setCopied(false);
    setCopiedConfig(false);
    try {
      const draft = {
        name: name.trim(),
        tools: selectedTools,
        allow_destructive: destructive,
        ...(tokenExpiry > 0 ? { expires_in_s: tokenExpiry } : {})
      };
      const answer =
        scope === "fleet"
          ? await api.createToken({ ...draft, device_ids: devices.map((entry) => entry.id) })
          : await api.createToken({ ...draft, device_id: device!.id });
      setCreated(answer);
      setName("");
      await refresh(answer);
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
      setCopied(false);
      setCopiedConfig(false);
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
      // Both scopes are records in one registry, and the console revokes either by id at
      // `DELETE /tokens/<id>`. What the row's scope decides is which list it leaves and which
      // source it was read from; there is no second revoke route to choose between.
      await api.revokeToken(revoking.id);
      setRevoking(null);
      await refresh();
    } catch (cause) {
      setRevokeError(message(cause));
    } finally {
      setRevokeBusy(false);
    }
  };

  const copyId = async (id: string) => {
    if (await copyText(id)) {
      setCopiedId(id);
      window.setTimeout(() => setCopiedId((current) => (current === id ? null : current)), 2000);
    }
  };

  const reset = () => {
    setCreated(null);
    setCreateError(null);
    setName("");
    setCopied(false);
    setCopiedConfig(false);
  };

  const createdFleet = created ? tokenScope(created) === "fleet" : false;
  const createdUrl = created
    ? createdFleet
      ? `${location.origin}/mcp/all`
      : `${location.origin}/mcp/${created.device_id}`
    : "";
  const createdConfig = created ? clientConfig("dsh", createdUrl).replace("<token>", created.token) : "";
  const canCreate =
    name.trim() !== "" &&
    selectedTools.length > 0 &&
    (scope === "device" ? device !== null : devices.length > 0);

  return (
    <div className="tokens-section">
      <p className="hint">
        A token is a credential the client keeps in its configuration. The console stores only its
        hash; the value is shown once, here.
      </p>

      <p className="hint">
        A token's device scope is fixed when it is minted. Rotate keeps it; to cover different
        machines, mint a new token.
      </p>

      {!device && (
        <p className="hint">
          The endpoint <code className="mono">{endpoint}</code> spans the fleet. A token minted for
          all machines is accepted there; every tool takes a required <code>device</code> argument.
        </p>
      )}
      {device && scope === "fleet" && (
        <p className="hint">
          This token is a fleet token: use it at <code className="mono">{location.origin}/mcp/all</code>.
          It is refused at this machine's URL, <code className="mono">{endpoint}</code>.
        </p>
      )}

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
          {createdFleet && (
            <>
              <pre className="config">{createdConfig}</pre>
              <div className="connect-actions">
                <button
                  type="button"
                  onClick={() =>
                    void copyText(createdConfig).then((ok) => {
                      if (ok) setCopiedConfig(true);
                    })
                  }
                >
                  {copiedConfig ? "Copied" : "Copy client config"}
                </button>
              </div>
            </>
          )}
          <div className="connect-actions">
            <button type="button" onClick={reset}>
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

          <div className="seg" role="group" aria-label="Token scope">
            {(["device", "fleet"] as const).map((entry) => (
              <button
                key={entry}
                type="button"
                aria-pressed={scope === entry}
                disabled={entry === "device" ? device === null : devices.length === 0}
                onClick={() => switchScope(entry)}
              >
                {entry === "device" ? "this machine" : "all machines"}
              </button>
            ))}
          </div>
          {scope === "device" && device && (
            <p className="hint">
              This token will cover {coverName}.
            </p>
          )}
          {scope === "fleet" && devices.length > 0 && (
            <p className="hint">
              This token will cover the {devices.length} machine
              {devices.length === 1 ? "" : "s"} enrolled now:{" "}
              {devices.map(coverageMachine).join(", ")}.
            </p>
          )}
          {scope === "fleet" && devices.length === 0 && (
            <p className="hint">Enroll a machine before minting a token that spans the fleet.</p>
          )}

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

          {countFromRegistry && (
            <p className="hint">
              {machineOffline
                ? "The machine is offline."
                : "The machine's live list could not be read."}{" "}
              This form counts the static fallback; the console refuses a plugin tool it cannot
              verify.
            </p>
          )}

          {preset === "choose" && (
            <div className="scope-groups">
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
            {selectedTools.length} tool{selectedTools.length === 1 ? "" : "s"}
            {catalogueLabel} ·{" "}
            {destructiveKnown
              ? `${destructiveCount} can change the machine`
              : "the count of tools that can change a machine is read from a machine"}
            {scope === "fleet" ? " · one list for every machine in the token's scope" : ""}
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
            Without this, the machine refuses anything that changes it; read-only tools still work.
          </p>

          {createError && <p className="error">{createError}</p>}
          <div className="connect-actions">
            <button type="button" className="primary" disabled={creating || !canCreate} onClick={() => void create()}>
              {creating ? "Creating…" : "Create token"}
            </button>
          </div>
        </section>
      )}

      <section className="token-list" aria-label="Existing tokens">
        <h3>Existing tokens</h3>
        {tokens === null && !tokensError && <p className="hint">Loading…</p>}
        {tokens !== null && tokens.length === 0 && (
          <p className="hint">
            A token is the credential a client keeps in its configuration, and none has been minted
            yet; create one above.
          </p>
        )}
        {tokens !== null && tokens.length > 0 && (
          <div
            className="table-scroll token-table-scroll"
            role="region"
            aria-label="Token list"
            tabIndex={0}
          >
            <table className="token-table">
              <thead>
                <tr>
                  <th scope="col">Name</th>
                  <th scope="col">Scope</th>
                  <th scope="col">Tools</th>
                  <th scope="col">Expires</th>
                  <th scope="col">Created</th>
                  <th scope="col">
                    <span className="sr-only">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {tokens.map((entry) => {
                  const label = entry.name || shortId(entry.id);
                  const scope = scopeLabel(entry, devices, device?.id ?? null);
                  const focused = entry.id === focusTokenId;
                  return (
                    <tr
                      key={entry.id}
                      className={focused ? "is-managed" : undefined}
                      ref={focused ? focusedRow : undefined}
                    >
                      <td>
                        <div className="token-name">
                          <strong>{label}</strong>
                          <span className="token-id">
                            <code title={entry.id}>{shortId(entry.id)}</code>
                            <button
                              type="button"
                              className="mini"
                              aria-label={`Copy id of ${label}`}
                              onClick={() => void copyId(entry.id)}
                            >
                              {copiedId === entry.id ? "Copied" : "Copy"}
                            </button>
                          </span>
                          <span className="hint">
                            {entry.last_used_ms == null ? (
                              "never used"
                            ) : (
                              <>used <Moment ts={entry.last_used_ms} /></>
                            )}
                          </span>
                        </div>
                      </td>
                      <td>
                        <span className="chip" title={scope.title}>
                          {scope.door}
                        </span>
                        <span className="hint token-scope-reach" title={scope.title}>
                          {scope.label}
                        </span>
                      </td>
                      <td className="token-tools">
                        <span title={entry.tools.join(", ")}>
                          {entry.tools.length} tool{entry.tools.length === 1 ? "" : "s"}
                        </span>
                        <span className="hint">
                          {entry.allow_destructive ? "destructive allowed" : "read-only"}
                        </span>
                      </td>
                      <td className="token-expires">
                        <span className="token-meta-label" aria-hidden="true">Expires</span>
                        {entry.expires_at_ms == null ? (
                          "never"
                        ) : (
                          <Moment ts={entry.expires_at_ms} />
                        )}
                      </td>
                      <td className="token-created">
                        <span className="token-meta-label" aria-hidden="true">Created</span>
                        <Moment ts={entry.created_at_ms} />
                      </td>
                      <td className="token-actions">
                        <button
                          type="button"
                          className="mini"
                          onClick={() => {
                            setRotateError(null);
                            setRotating(entry);
                          }}
                        >
                          Rotate
                        </button>
                        <button
                          type="button"
                          className="mini"
                          onClick={() => {
                            setRevokeError(null);
                            setRevoking(entry);
                          }}
                        >
                          Revoke
                        </button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
        {tokensError && <p className="error">{tokensError}</p>}
      </section>

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
            The new token keeps this one's name, its fixed device scope, tools and destructive
            consent. Its value is shown once, right after — the console keeps only its hash.
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
            {tokenScope(revoking) === "fleet" ? "This token is accepted on /mcp/all. " : ""}
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
