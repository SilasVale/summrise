import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { Device } from "./api";
import { copyText } from "./copy";
import {
  addPlugin,
  inspectPlugin,
  listPlugins,
  pluginFailure,
  pluginLogs,
  removePlugin,
  restartPlugin,
  type PluginFailure,
  type PluginRecord,
  type PluginSummary
} from "./pluginApi";
import { Sheet } from "./Sheet";
import { toolsReadState } from "./toolsRead";
import type { LiveToolsState } from "./useLiveTools";

const LOG_LINES = 200;
const GUIDE_URL = "https://github.com/SilasVale/summrise/blob/main/docs/PLUGINS.md";

const STATE_LABEL: Record<PluginRecord["state"], string> = {
  running: "Running",
  starting: "Starting",
  stopped: "Stopped",
  failed: "Failed"
};

function stateClass(state: PluginRecord["state"]): string {
  if (state === "running") return "chip-ok";
  if (state === "starting") return "chip-warn";
  if (state === "failed") return "chip-bad";
  return "";
}

function toolsText(count: number): string {
  return `${count} tool${count === 1 ? "" : "s"}`;
}

/// The plugin list for one machine, read through the operator's own `/mcp` session.
function usePlugins(deviceId: string | null, enabled: boolean) {
  const [plugins, setPlugins] = useState<PluginRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [ownerDeviceId, setOwnerDeviceId] = useState<string | null>(null);
  const ticket = useRef(0);

  const reload = useCallback(async () => {
    if (deviceId === null || !enabled) {
      setPlugins([]);
      setLoading(false);
      setError(null);
      return;
    }
    const mine = ++ticket.current;
    setLoading(true);
    setError(null);
    try {
      const answer = await listPlugins(deviceId);
      if (mine === ticket.current) setPlugins(answer);
    } catch (cause) {
      if (mine === ticket.current) setError(pluginFailure(cause).message);
    } finally {
      if (mine === ticket.current) setLoading(false);
    }
  }, [deviceId, enabled]);

  useEffect(() => {
    setOwnerDeviceId(enabled ? deviceId : null);
    setPlugins([]);
    void reload();
    return () => {
      ticket.current += 1;
    };
  }, [deviceId, enabled, reload]);

  // The next device renders before this effect resets the stored answer. Only expose the answer
  // to its owner, including its error and loading state, from that very first render.
  const owned = ownerDeviceId === deviceId;
  return {
    plugins: owned ? plugins : [],
    loading: owned ? loading : enabled,
    error: owned ? error : null,
    ownerDeviceId,
    reload
  };
}

/**
 * Plugins: add a capability the built-ins do not have, and keep it healthy. A plugin is a directory
 * whose `plugin.json` names an MCP server; installing runs that code with this machine's privileges,
 * so the trust sentence leads and the path is validated before Add and run exists.
 *
 * This is the plugin half of the Tools tab; the tab's `<h2>` and the catalogue live in
 * `ToolsTab.tsx`. The offline notice is the Device shell's, above both sections, so this one does
 * not repeat it.
 */
export function PluginList({
  device,
  live,
  offline,
  onMachine
}: {
  device: Device;
  live: LiveToolsState;
  /// The machine's state from the shell (`deviceOnline === false` or the failed read's own answer).
  offline: boolean;
  onMachine: () => void;
}) {
  const known = !live.loading && live.error === null;
  const read = toolsReadState(live);
  const supported = live.tools.some((tool) => tool.name === "plugins.list");
  const remote = usePlugins(device.id, known && supported);
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<PluginRecord | null>(null);
  const [logsFor, setLogsFor] = useState<PluginRecord | null>(null);
  const [busyNamespaces, setBusyNamespaces] = useState<ReadonlySet<string>>(() => new Set<string>());
  const restarting = useRef(new Set<string>());
  const [actionError, setActionError] = useState<string | null>(null);
  const [added, setAdded] = useState<string | null>(null);
  const [contextDeviceId, setContextDeviceId] = useState(device.id);
  const epoch = useRef(0);

  useLayoutEffect(() => () => {
    epoch.current += 1;
    restarting.current.clear();
  }, [device.id]);

  // Reset before rendering a sheet with another machine: its effects must never combine B's id
  // with A's checked path or namespace. A sent mutation still completes on its original machine.
  if (contextDeviceId !== device.id) {
    setContextDeviceId(device.id);
    setAdding(false);
    setRemoving(null);
    setLogsFor(null);
    setBusyNamespaces(new Set<string>());
    setActionError(null);
    setAdded(null);
  }

  const ownsRow = (plugin: PluginRecord) =>
    remote.ownerDeviceId === device.id && remote.plugins.includes(plugin);

  const restart = async (plugin: PluginRecord) => {
    if (!ownsRow(plugin) || restarting.current.has(plugin.namespace)) return;
    const mine = epoch.current;
    restarting.current.add(plugin.namespace);
    setBusyNamespaces(new Set(restarting.current));
    setActionError(null);
    try {
      await restartPlugin(device.id, plugin.namespace);
      if (mine !== epoch.current) return;
      await remote.reload();
    } catch (cause) {
      if (mine === epoch.current) setActionError(pluginFailure(cause).message);
    } finally {
      if (mine === epoch.current) {
        restarting.current.delete(plugin.namespace);
        setBusyNamespaces(new Set(restarting.current));
      }
    }
  };

  const subtitle =
    known && supported
      ? remote.loading && remote.plugins.length === 0
        ? "Reading the plugin list…"
        : remote.error !== null && remote.plugins.length === 0
          ? "Plugin list unavailable."
          : `${remote.plugins.length} plugin${remote.plugins.length === 1 ? "" : "s"}`
      : "Install, watch and restart the capabilities this machine adds.";

  return (
    <div className="plugin-management">
      <div className="tools-section-head tools-plugin-head">
        <p className="page-sub">{subtitle}</p>
        {/* THE EMPTY LIST IS THE ONE PLACE THE ACTION BELONGS WHEN THERE IS NO LIST: the empty
            state below carries its own Add plugin, and rendering this header control too put two
            identical primary buttons on a page with nothing in it. */}
        {known && supported && (remote.loading || remote.error !== null || remote.plugins.length > 0) && (
          <div className="tools-section-actions">
            <button type="button" className="primary" onClick={() => setAdding(true)}>
              Add plugin
            </button>
          </div>
        )}
      </div>

      {read === "reading" ||
        (live.loading && live.error === null && !offline && remote.error === null && remote.plugins.length === 0) ? (
        <div className="skeleton-list" aria-label="Reading the plugin list" role="status">
          {[0, 1, 2].map((row) => (
            <div className="skeleton-row" key={row}>
              <span className="skeleton skeleton-name" />
              <span className="skeleton skeleton-line" />
            </div>
          ))}
        </div>
      ) : offline && remote.plugins.length === 0 ? (
        <p className="hint">The console has no plugin list from this machine yet.</p>
      ) : live.error !== null && remote.plugins.length === 0 ? (
        // THE TOOLS READ OWNS THE ONE RETRY AND THE ONE FAILURE SENTENCE. This section names only
        // what the failure costs here and points at that notice, rather than restating the failure
        // in a second sentence (audit-43 F9).
        <p className="hint">Plugins need the tool list; Retry above.</p>
      ) : known && !supported ? (
        <section className="empty-state">
          <h3>Older build</h3>
          <p className="hint">This machine needs a newer Summrise to host plugins.</p>
          <button type="button" onClick={onMachine}>
            Open Machine
          </button>
        </section>
      ) : remote.loading && remote.plugins.length === 0 ? (
        <div className="skeleton-list" aria-label="Reading the plugin list" role="status">
          {[0, 1, 2].map((row) => (
            <div className="skeleton-row" key={row}>
              <span className="skeleton skeleton-name" />
              <span className="skeleton skeleton-line" />
            </div>
          ))}
        </div>
      ) : remote.error ? (
        <div className="notice" role="status">
          <span>{remote.error}</span>
          <button type="button" onClick={() => void remote.reload()}>
            Retry
          </button>
        </div>
      ) : remote.plugins.length === 0 ? (
        <section className="empty-state">
          <p className="hint">No plugins yet. Add one when a built-in cannot do the job.</p>
          <button type="button" className="primary" onClick={() => setAdding(true)}>
            Add plugin
          </button>
        </section>
      ) : (
        <div className="plugin-list">
          {remote.plugins.map((plugin) => (
            <article className="card plugin-row" key={plugin.namespace}>
              <div className="plugin-main">
                <div className="plugin-head">
                  <span className="plugin-name">{plugin.name}</span>
                  <span className={"chip " + stateClass(plugin.state)}>
                    {STATE_LABEL[plugin.state]}
                  </span>
                </div>
                <p className="plugin-meta">
                  <span className="mono">{plugin.namespace}</span>
                  {plugin.version ? ` · v${plugin.version}` : ""} · {toolsText(plugin.tools)}
                </p>
                {plugin.description && <p className="hint">{plugin.description}</p>}
                {plugin.state === "failed" && plugin.last_error && (
                  <p className="plugin-error">{plugin.last_error}</p>
                )}
              </div>
              <aside className="plugin-inspection" aria-label={`${plugin.name} configuration and actions`}>
                {plugin.path && (
                  <div className="plugin-source">
                    <span className="hint">Source directory</span>
                    <p className="plugin-path mono" title={plugin.path}>{plugin.path}</p>
                  </div>
                )}
              <div className="plugin-actions">
                {plugin.state === "failed" && (
                  <button
                    type="button"
                    disabled={busyNamespaces.has(plugin.namespace)}
                    onClick={() => void restart(plugin)}
                  >
                    {busyNamespaces.has(plugin.namespace) ? "Restarting…" : "Restart"}
                  </button>
                )}
                <details className="row-menu">
                  <summary aria-label={`Actions for ${plugin.name}`} title={`Actions for ${plugin.name}`}>
                    ⋯
                  </summary>
                  <div className="row-menu-pop">
                    <button type="button" onClick={() => { if (ownsRow(plugin)) setLogsFor(plugin); }}>
                      Logs
                    </button>
                    {plugin.state !== "failed" && (
                      <button
                        type="button"
                        disabled={busyNamespaces.has(plugin.namespace)}
                        onClick={() => void restart(plugin)}
                      >
                        {busyNamespaces.has(plugin.namespace) ? "Restarting…" : "Restart"}
                      </button>
                    )}
                    <button type="button" className="danger" onClick={() => { if (ownsRow(plugin)) setRemoving(plugin); }}>
                      Remove
                    </button>
                  </div>
                </details>
              </div>
              </aside>
            </article>
          ))}
        </div>
      )}

      {actionError && (
        <div className="notice" role="status">
          <span>{actionError}</span>
          <button type="button" onClick={() => setActionError(null)}>
            Dismiss
          </button>
        </div>
      )}
      {added && (
        <div className="notice" role="status">
          <span>{added}</span>
          <button type="button" onClick={() => setAdded(null)}>
            Dismiss
          </button>
        </div>
      )}

      {adding && (
        <AddPluginSheet
          deviceId={device.id}
          onClose={() => setAdding(false)}
          onAdded={(plugin) => {
            setAdding(false);
            setAdded(`${plugin.name} added with ${toolsText(plugin.tools)}.`);
            void remote.reload();
          }}
        />
      )}

      {removing && (
        <RemovePluginSheet
          deviceId={device.id}
          plugin={removing}
          onClose={() => setRemoving(null)}
          onRemoved={() => {
            setRemoving(null);
            setAdded(`${removing.name} removed. Its tools are gone from this machine.`);
            void remote.reload();
          }}
        />
      )}

      {logsFor && (
        <LogsSheet deviceId={device.id} plugin={logsFor} onClose={() => setLogsFor(null)} />
      )}
    </div>
  );
}

/// The path's one truthful state before Add and run exists: idle, checking, a manifest, or a refusal.
type Check =
  | { state: "idle" }
  | { state: "checking" }
  | { state: "valid"; path: string; summary: PluginSummary }
  | { state: "invalid"; failure: PluginFailure };

function AddPluginSheet({
  deviceId,
  onClose,
  onAdded
}: {
  deviceId: string;
  onClose: () => void;
  onAdded: (plugin: PluginRecord) => void;
}) {
  const [path, setPath] = useState("");
  const [check, setCheck] = useState<Check>({ state: "idle" });
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ticket = useRef(0);
  const mounted = useRef(false);

  useLayoutEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      ticket.current += 1;
    };
  }, []);

  const validate = async () => {
    const wanted = path.trim();
    const mine = ++ticket.current;
    if (wanted === "") {
      setCheck({ state: "idle" });
      return;
    }
    setCheck({ state: "checking" });
    setError(null);
    try {
      const summary = await inspectPlugin(deviceId, wanted);
      if (mine === ticket.current) setCheck({ state: "valid", path: wanted, summary });
    } catch (cause) {
      if (mine === ticket.current) setCheck({ state: "invalid", failure: pluginFailure(cause) });
    }
  };

  // A pasted path must not look broken: Add and run stays disabled until the manifest is read, and
  // waiting for blur meant clicking the disabled button did nothing at all. Read after a short pause
  // as the operator types, and keep the blur check for the deliberate case.
  useEffect(() => {
    const wanted = path.trim();
    if (wanted === "") {
      setCheck({ state: "idle" });
      return;
    }
    const timer = window.setTimeout(() => {
      void validate();
    }, 300);
    return () => window.clearTimeout(timer);
    // `validate` is intentionally not a dependency: it closes over the current path and ticket.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path]);

  const checkedPath = check.state === "valid" && check.path === path.trim() ? check.path : null;
  const add = async () => {
    if (checkedPath === null || checkedPath === "" || adding) return;
    setAdding(true);
    setError(null);
    try {
      const answer = await addPlugin(deviceId, checkedPath);
      if (mounted.current) onAdded(answer.plugin);
    } catch (cause) {
      if (mounted.current) setError(pluginFailure(cause).message);
    } finally {
      if (mounted.current) setAdding(false);
    }
  };

  const valid = checkedPath !== null && checkedPath !== "";
  return (
    <Sheet
      title="Add plugin"
      onClose={onClose}
      busy={adding}
      footer={
        <>
          <button type="button" disabled={adding} onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="primary" disabled={!valid || adding} onClick={() => void add()}>
            {adding ? "Adding…" : "Add and run"}
          </button>
        </>
      }
    >
      <div className="add-plugin">
        <p className="trust">
          This runs the plugin's code with this machine's privileges. Install only code you trust.
        </p>
        <label className="field" htmlFor="plugin-path">
          <span className="field-label">Plugin directory</span>
          <input
            id="plugin-path"
            type="text"
            spellCheck={false}
            placeholder="C:\\plugins\\hello"
            value={path}
            onChange={(event) => {
              ticket.current += 1;
              setPath(event.target.value);
              setCheck({ state: "idle" });
              setError(null);
            }}
            onBlur={() => void validate()}
            onKeyDown={(event) => {
              if (event.key === "Enter") void validate();
            }}
          />
        </label>
        {path.trim() === "" && (
          <p className="hint">
            Paste the directory that contains plugin.json. Add and run becomes available after the manifest is read.
          </p>
        )}
        {check.state === "checking" && <p className="hint">Reading the manifest…</p>}
        {check.state === "valid" && valid && (
          <p className="plugin-valid" role="status">
            {check.summary.namespace} · v{check.summary.version} · runs with this machine's privileges
          </p>
        )}
        {check.state === "invalid" && (
          <div className="plugin-invalid" role="alert">
            <p>{check.failure.message}</p>
            {check.failure.code === "NO_MANIFEST" || /guide/i.test(check.failure.action ?? "") ? (
              <a href={GUIDE_URL} target="_blank" rel="noreferrer">
                Open the guide
              </a>
            ) : check.failure.action ? (
              <p className="hint">{check.failure.action}</p>
            ) : null}
          </div>
        )}
        {error && <p className="error">{error}</p>}

        <details className="plugin-about">
          <summary>What is a plugin?</summary>
          <p className="hint">
            A plugin is an MCP server: a command Summrise starts and speaks to over stdio. Its tools
            appear behind the one endpoint, under the same token and in the same record.
          </p>
          <p className="hint">
            <code>plugin.json</code> needs <code>id</code>, <code>namespace</code>, <code>name</code>,{" "}
            <code>version</code> and a non-empty <code>command</code>. <code>cwd</code>,{" "}
            <code>env</code> and <code>transport</code> (<code>"stdio"</code>) are optional. The
            source directory is not copied; removing only forgets the registration.
          </p>
        </details>
      </div>
    </Sheet>
  );
}

function RemovePluginSheet({
  deviceId,
  plugin,
  onClose,
  onRemoved
}: {
  deviceId: string;
  plugin: PluginRecord;
  onClose: () => void;
  onRemoved: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(false);

  useLayoutEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const remove = async () => {
    setBusy(true);
    setError(null);
    try {
      await removePlugin(deviceId, plugin.namespace);
      if (mounted.current) onRemoved();
    } catch (cause) {
      if (mounted.current) setError(pluginFailure(cause).message);
    } finally {
      if (mounted.current) setBusy(false);
    }
  };

  return (
    <Sheet
      title={`Remove ${plugin.name}?`}
      onClose={onClose}
      busy={busy}
      footer={
        <>
          <button type="button" disabled={busy} onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="danger" disabled={busy} onClick={() => void remove()}>
            {busy ? "Removing…" : "Remove"}
          </button>
        </>
      }
    >
      <div className="remove-plugin">
        <p>
          Calls to {plugin.name}'s tools will fail. Summrise stops the process and forgets the
          registration; the source directory stays where it is.
        </p>
        {plugin.path && <p className="mono plugin-path">{plugin.path}</p>}
        {error && <p className="error">{error}</p>}
      </div>
    </Sheet>
  );
}

function LogsSheet({
  deviceId,
  plugin,
  onClose
}: {
  deviceId: string;
  plugin: PluginRecord;
  onClose: () => void;
}) {
  const [lines, setLines] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const body = useRef<HTMLPreElement>(null);
  const ticket = useRef(0);

  const read = useCallback(async () => {
    const mine = ++ticket.current;
    setError(null);
    try {
      const answer = await pluginLogs(deviceId, plugin.namespace, LOG_LINES);
      if (mine === ticket.current) setLines(answer);
    } catch (cause) {
      if (mine === ticket.current) {
        setLines([]);
        setError(pluginFailure(cause).message);
      }
    }
  }, [deviceId, plugin.namespace]);

  useEffect(() => {
    void read();
    return () => {
      ticket.current += 1;
    };
  }, [read]);

  // A failed plugin's log is read from the end, where the reason is.
  useEffect(() => {
    if (lines !== null && plugin.state === "failed" && body.current) {
      body.current.scrollTop = body.current.scrollHeight;
    }
  }, [lines, plugin.state]);

  const text = (lines ?? []).join("\n");
  return (
    <Sheet
      title={`Logs · ${plugin.name}`}
      onClose={onClose}
      footer={
        <>
          <button
            type="button"
            disabled={lines === null}
            onClick={() =>
              void copyText(text).then((ok) => {
                if (ok) {
                  setCopied(true);
                  window.setTimeout(() => setCopied(false), 2000);
                }
              })
            }
          >
            {copied ? "Copied" : "Copy"}
          </button>
          {plugin.state === "starting" && (
            <button type="button" onClick={() => void read()}>
              Refresh
            </button>
          )}
        </>
      }
    >
      <div className="plugin-logs">
        {error && <p className="error">{error}</p>}
        <pre className="logs mono" ref={body}>
          {lines === null ? "Reading the log…" : lines.length === 0 ? "" : text}
        </pre>
        {lines !== null && lines.length === 0 && !error && (
          <div className="hint">
            <p>No output yet.</p>
            <p>The plugin has not written anything since it started.</p>
          </div>
        )}
      </div>
    </Sheet>
  );
}
