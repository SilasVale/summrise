import { useCallback, useEffect, useRef, useState } from "react";
import type { Device } from "./api";
import { copyText } from "./copy";
import { DevicePicker } from "./DevicePicker";
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
import { useLiveTools } from "./useLiveTools";

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
    setPlugins([]);
    void reload();
    return () => {
      ticket.current += 1;
    };
  }, [reload]);

  return { plugins, loading, error, reload };
}

/**
 * Plugins: add a capability the built-ins do not have, and keep it healthy. A plugin is a directory
 * whose `plugin.json` names an MCP server; installing runs that code with this machine's privileges,
 * so the trust sentence leads and the path is validated before Add and run exists.
 */
export function PluginsPage({
  devices,
  device,
  onChoose,
  onMachine
}: {
  devices: Device[];
  device: Device | null;
  onChoose: (id: string) => void;
  onMachine: () => void;
}) {
  const live = useLiveTools(device?.id ?? null);
  const known = !live.loading && live.error === null;
  const supported = live.tools.some((tool) => tool.name === "plugins.list");
  const remote = usePlugins(device?.id ?? null, known && supported);
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<PluginRecord | null>(null);
  const [logsFor, setLogsFor] = useState<PluginRecord | null>(null);
  const [busyNamespace, setBusyNamespace] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [added, setAdded] = useState<string | null>(null);

  const restart = async (plugin: PluginRecord) => {
    setBusyNamespace(plugin.namespace);
    setActionError(null);
    try {
      await restartPlugin(device?.id ?? "", plugin.namespace);
      await remote.reload();
    } catch (cause) {
      setActionError(pluginFailure(cause).message);
    } finally {
      setBusyNamespace(null);
    }
  };

  const offline = !device?.online || live.offline;
  const subtitle =
    known && supported
      ? `${remote.plugins.length} plugin${remote.plugins.length === 1 ? "" : "s"}`
      : "Install, watch and restart the capabilities this machine adds.";

  return (
    <div className="page plugins-page">
      <header className="page-head">
        <div>
          <h1>Plugins</h1>
          <p className="page-sub">{subtitle}</p>
        </div>
        {/* THE EMPTY LIST IS THE ONE PLACE THE ACTION BELONGS WHEN THERE IS NO LIST: the empty
            state below carries its own Add plugin, and rendering this header control too put two
            identical primary buttons on a page with nothing in it. */}
        {device && known && supported && (remote.loading || remote.error !== null || remote.plugins.length > 0) && (
          <div className="page-actions">
            <button type="button" className="primary" onClick={() => setAdding(true)}>
              Add plugin
            </button>
          </div>
        )}
      </header>

      {!device ? (
        <section className="empty-state">
          <h2>Choose a machine</h2>
          <DevicePicker devices={devices} onChoose={onChoose} />
        </section>
      ) : live.loading && live.tools.length === 0 ? (
        <div className="skeleton-list" aria-label="Reading the plugin list" role="status">
          {[0, 1, 2].map((row) => (
            <div className="skeleton-row" key={row}>
              <span className="skeleton skeleton-name" />
              <span className="skeleton skeleton-line" />
            </div>
          ))}
        </div>
      ) : known && !supported ? (
        <section className="empty-state">
          <h2>Older build</h2>
          <p className="hint">This machine needs a newer Summrise to host plugins.</p>
          <button type="button" onClick={onMachine}>
            Open Machine
          </button>
        </section>
      ) : offline || (live.error && live.tools.length === 0) ? (
        <div className="notice" role="status">
          <span>
            {device
              ? `${device.name ?? "This machine"} is not connected. The console keeps trying; open the machine's panel if it stays offline.`
              : "The console could not read this machine."}
          </span>
          <button
            type="button"
            onClick={() => {
              live.retry();
              void remote.reload();
            }}
          >
            Retry
          </button>
        </div>
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
            <article className="plugin-row" key={plugin.namespace}>
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
                {plugin.path && (
                  <p className="plugin-path mono" title={plugin.path}>
                    {plugin.path}
                  </p>
                )}
                {plugin.state === "failed" && plugin.last_error && (
                  <p className="plugin-error">{plugin.last_error}</p>
                )}
              </div>
              <div className="plugin-actions">
                {plugin.state === "failed" && (
                  <button
                    type="button"
                    disabled={busyNamespace === plugin.namespace}
                    onClick={() => void restart(plugin)}
                  >
                    {busyNamespace === plugin.namespace ? "Restarting…" : "Restart"}
                  </button>
                )}
                <details className="row-menu">
                  <summary aria-label={`Actions for ${plugin.name}`} title={`Actions for ${plugin.name}`}>
                    ⋯
                  </summary>
                  <div className="row-menu-pop">
                    <button type="button" onClick={() => setLogsFor(plugin)}>
                      Logs
                    </button>
                    {plugin.state !== "failed" && (
                      <button
                        type="button"
                        disabled={busyNamespace === plugin.namespace}
                        onClick={() => void restart(plugin)}
                      >
                        {busyNamespace === plugin.namespace ? "Restarting…" : "Restart"}
                      </button>
                    )}
                    <button type="button" className="danger" onClick={() => setRemoving(plugin)}>
                      Remove
                    </button>
                  </div>
                </details>
              </div>
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

      {adding && device && (
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

      {removing && device && (
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

      {logsFor && device && (
        <LogsSheet deviceId={device.id} plugin={logsFor} onClose={() => setLogsFor(null)} />
      )}
    </div>
  );
}

/// The path's one truthful state before Add and run exists: idle, checking, a manifest, or a refusal.
type Check =
  | { state: "idle" }
  | { state: "checking" }
  | { state: "valid"; summary: PluginSummary }
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

  const validate = async () => {
    const wanted = path.trim();
    if (wanted === "") {
      setCheck({ state: "idle" });
      return;
    }
    const mine = ++ticket.current;
    setCheck({ state: "checking" });
    setError(null);
    try {
      const summary = await inspectPlugin(deviceId, wanted);
      if (mine === ticket.current) setCheck({ state: "valid", summary });
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

  const add = async () => {
    if (check.state !== "valid" || adding) return;
    setAdding(true);
    setError(null);
    try {
      const answer = await addPlugin(deviceId, path.trim());
      onAdded(answer.plugin);
    } catch (cause) {
      setError(pluginFailure(cause).message);
    } finally {
      setAdding(false);
    }
  };

  const valid = check.state === "valid";
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
        {check.state === "valid" && (
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

  const remove = async () => {
    setBusy(true);
    setError(null);
    try {
      await removePlugin(deviceId, plugin.namespace);
      onRemoved();
    } catch (cause) {
      setError(pluginFailure(cause).message);
    } finally {
      setBusy(false);
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
