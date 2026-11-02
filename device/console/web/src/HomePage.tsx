import { useEffect, useState } from "react";
import type { AuditLine, Device } from "./api";
import { DeviceCard } from "./DeviceCard";
import { copyText } from "./copy";
import { age, shortId } from "./format";
import { listPlugins } from "./pluginApi";
import { useLiveTools } from "./useLiveTools";

/** A one-time enrollment code and the command that redeems it. */
export type Enrollment = { code: string; expires_at_ms: number } | null;

type FleetProps = {
  devices: Device[];
  selectedId: string | null;
  code: Enrollment;
  enrollCommand: string;
  busy: boolean;
  onOpen: (id: string) => void;
  onRename: (device: Device) => void;
  onRemove: (device: Device) => void;
  onDiagnostics: (device: Device) => void;
  onAddMachine: () => void;
  onDismissCode: () => void;
  onNotice: (sentence: string) => void;
};

/**
 * Home before a machine is selected: the fleet. One row per machine, one accent for the action that
 * adds one, and the enrollment command when it exists. This is the previous Devices page kept because
 * it is the only way in — restyled to the hub's shell, not given a second dashboard.
 */
function FleetHome(props: FleetProps) {
  const online = props.devices.filter((device) => device.online).length;
  return (
    <div className="page home-page">
      <header className="page-head">
        <div>
          <h1>Machines</h1>
          <p className="page-sub">
            {props.devices.length} machine{props.devices.length === 1 ? "" : "s"} · {online} online
          </p>
        </div>
        <div className="page-actions">
          <button type="button" className="primary" disabled={props.busy} onClick={props.onAddMachine}>
            Add a machine
          </button>
        </div>
      </header>

      {props.code && (
        <section className="enroll" aria-label="Enrollment command">
          <p className="hint">
            This code works once, for 15 minutes. On the Windows machine, run this in an elevated
            Command Prompt, then restart the <code>SummriseLaunch</code> service.
          </p>
          <pre className="code">{props.enrollCommand}</pre>
          <div className="actions">
            <button
              type="button"
              onClick={() =>
                void copyText(props.enrollCommand).then((ok) =>
                  props.onNotice(ok ? "Enrollment command copied." : "Could not copy the command.")
                )
              }
            >
              Copy command
            </button>
            <button type="button" onClick={props.onDismissCode}>
              Dismiss
            </button>
          </div>
          <p className="hint">Expires at {new Date(props.code.expires_at_ms).toLocaleTimeString()}.</p>
        </section>
      )}

      {props.devices.length === 0 ? (
        <section className="empty-state">
          <h2>No machines yet</h2>
          <p className="hint">
            A machine appears here after it enrolls with this console. Run the command once on the
            Windows machine, and it dials out to this console.
          </p>
        </section>
      ) : (
        <div className="fleet-list">
          {props.devices.map((device) => (
            <DeviceCard
              key={device.id}
              device={device}
              selected={props.selectedId === device.id}
              onSelect={() => props.onOpen(device.id)}
              onRename={() => props.onRename(device)}
              onRemove={() => props.onRemove(device)}
              onDiagnostics={() => props.onDiagnostics(device)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

type PluginCount = number | null;

/// How many plugins the selected machine hosts. `known` is false while the tool list is unread or
/// unreadable; an older build has no `plugins.list` at all and counts 0, which is the truth for it.
function usePluginCount(deviceId: string | null, known: boolean, present: boolean): PluginCount {
  const [count, setCount] = useState<PluginCount>(null);
  useEffect(() => {
    if (deviceId === null || !known) {
      setCount(null);
      return;
    }
    if (!present) {
      setCount(0);
      return;
    }
    let live = true;
    setCount(null);
    listPlugins(deviceId)
      .then((plugins) => {
        if (live) setCount(plugins.length);
      })
      .catch(() => {
        // A plugin read that fails leaves the count unknown; Home does not invent a zero.
        if (live) setCount(null);
      });
    return () => {
      live = false;
    };
  }, [deviceId, known, present]);
  return count;
}

function plural(count: number, noun: string, many = `${noun}s`): string {
  return `${count} ${count === 1 ? noun : many}`;
}

/**
 * The hub, once a machine is chosen: is it running, where is the door, what can it do, what next.
 * One status line, the endpoint, one accent, one counts line and the last three calls. Identity is
 * the header's; this page does not repeat it, and the catalogue is Capabilities' job, not a tile grid.
 */
function HubHome(
  props: FleetProps & {
    device: Device;
    audit: AuditLine[];
    onConnect: () => void;
    onCapabilities: () => void;
    onPlugins: () => void;
    onActivity: () => void;
  }
) {
  const device = props.device;
  const name = device.name ?? shortId(device.id);
  const live = useLiveTools(device.id);
  // A stale catalogue still names the tool surface, so the plugin read is allowed; an empty list
  // after a failed read knows nothing and must not be read as "an older build".
  const known = live.tools.length > 0 || (!live.loading && live.error === null);
  const pluginsTool = live.tools.some((tool) => tool.name === "plugins.list");
  const plugins = usePluginCount(device.id, known, pluginsTool);
  const [copied, setCopied] = useState(false);
  const endpoint = `${location.origin}/mcp/${device.id}`;

  const offline = !device.online || live.offline;
  // The status line is state only. Counts have one home: the counts line, where they are a link.
  const statusLine = [offline ? "Offline" : "Running", device.version ? `v${device.version}` : null]
    .filter(Boolean)
    .join(" · ");
  const capabilities = new Set(live.tools.map((tool) => tool.capability)).size;
  const calls = props.audit.filter((line) => line.device_id === device.id).slice(0, 3);
  const notice = offline
    ? `${name} is not connected. The console keeps trying; open the machine's panel if it stays offline.`
    : live.error
      ? "The machine is running. The console could not read its tool list, so these counts may be stale."
      : null;

  const copyEndpoint = async () => {
    if (offline) return;
    if (await copyText(endpoint)) {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } else {
      props.onNotice("Could not copy the endpoint: the browser refused the clipboard.");
    }
  };

  return (
    <div className="page home-page">
      <header className="page-head">
        <div>
          <h1>Home</h1>
          <p className="status-line">{statusLine}</p>
        </div>
      </header>

      {notice && (
        <div className="notice" role="status">
          <span>{notice}</span>
          <button type="button" onClick={() => live.retry()}>
            Retry
          </button>
        </div>
      )}

      <section className="home-hero" aria-label="The endpoint">
        <p className="hint">This is the door your AI clients connect to.</p>
        <div className="endpoint-row">
          <code className={"endpoint mono" + (offline ? " dim" : "")}>{endpoint}</code>
          <button
            type="button"
            disabled={offline}
            title={offline ? "The machine is not connected" : undefined}
            onClick={() => void copyEndpoint()}
          >
            {copied ? "Copied" : "Copy"}
          </button>
        </div>
        <div className="home-actions">
          <button type="button" className="primary" onClick={props.onConnect}>
            Connect a client
          </button>
          <button type="button" className="link" onClick={props.onPlugins}>
            Add a capability
          </button>
        </div>
      </section>

      <div className="home-lower">
        {offline ? (
          <p className="hint">Tool counts are unavailable while the machine is offline.</p>
        ) : live.error && live.tools.length === 0 ? (
          <p className="hint">Tool counts are unavailable until the console can read the machine's list.</p>
        ) : (
          <button type="button" className="counts-line" onClick={props.onCapabilities}>
            {plural(live.tools.length, "tool")} · {plural(capabilities, "capability", "capabilities")}
            {plugins !== null ? ` · ${plural(plugins, "plugin")}` : ""}
            {live.stale ? " · stale" : ""} <span aria-hidden="true">→</span>
          </button>
        )}

        <section className="home-recent" aria-labelledby="recent-calls">
          <div className="home-recent-head">
            <h2 id="recent-calls">Recent calls</h2>
            <button type="button" className="link" onClick={props.onActivity}>
              All activity <span aria-hidden="true">→</span>
            </button>
          </div>
          {calls.length === 0 ? (
            <p className="hint">No calls yet.</p>
          ) : (
            <ul className="recent-calls">
              {calls.map((line, index) => (
                <li key={`${line.ts_ms}-${index}`}>
                  <span className="mono">{line.tool ?? line.detail ?? "call"}</span>
                  <span className={"chip " + (line.status >= 400 ? "chip-bad" : "chip-ok")}>
                    {line.status}
                  </span>
                  <span className="recent-age" title={new Date(line.ts_ms).toLocaleString()}>
                    {age(line.ts_ms)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </div>
  );
}

export type HomePageProps = FleetProps & {
  /// The machine in context. Null is the fleet zero state; a chosen machine is the hub.
  device: Device | null;
  audit: AuditLine[];
  onConnect: () => void;
  onCapabilities: () => void;
  onPlugins: () => void;
  onActivity: () => void;
};

export function HomePage(props: HomePageProps) {
  return props.device ? <HubHome {...props} device={props.device} /> : <FleetHome {...props} />;
}
