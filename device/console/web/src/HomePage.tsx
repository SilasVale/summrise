import { useEffect, useState } from "react";
import { LiveNow } from "./LiveNow";
import type { LiveLine } from "./liveActivity";
import type { AuditLine, Device } from "./api";
import { DeviceCard } from "./DeviceCard";
import { copyText } from "./copy";
import { shortId } from "./format";
import { listPlugins } from "./pluginApi";
import { useLiveTools } from "./useLiveTools";
import { ActivityTimeline } from "./ActivityTimeline";
import { mergeActivity } from "./activityMerge";

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

/** Midnight in the console's own timezone; the hub's timeline is today's, not the audit window's. */
function startOfToday(now = Date.now()): number {
  const date = new Date(now);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}

/**
 * The hub, once a machine is chosen: is it running, what is it doing now, what did it do today.
 * One status line, one Connect action, the Now band, this machine's timeline and one counts line.
 * Identity is the header's; this page does not repeat it, and the catalogue is Capabilities' job,
 * not a tile grid. The endpoint lives in Connect, so this page never names the URL.
 */
function HubHome(
  props: FleetProps & {
    device: Device;
    audit: AuditLine[];
    onConnect: () => void;
    onCapabilities: () => void;
    onActivity: () => void;
    /** The console's own build, from /health; the chip next door is the device's. */
    consoleVersion?: string | null;
    /** The live activity stream, newest first. Absent in tests and before the stream opens. */
    liveLines?: LiveLine[];
    liveState?: "connecting" | "live" | "offline";
    /** Open a machine's Terminal tab, where an AI's session can be watched. */
    onWatchTerminal?: (deviceId: string) => void;
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
  // The page's subject is the machine: the stream's lines are the newest of the same audit, and
  // both the Now band and the timeline drop every other machine's. The timeline is today's; older
  // days belong to Activity, behind the All activity link.
  const liveLines = (props.liveLines ?? []).filter((line) => line.device_id === device.id);
  const todayStart = startOfToday();
  const timelineLines = mergeActivity(
    props.audit.filter((line) => line.device_id === device.id),
    liveLines
  )
    .filter((line) => line.ts_ms >= todayStart)
    .sort((a, b) => b.ts_ms - a.ts_ms);

  const offline = !device.online || live.offline;
  // **THE MACHINE CHIP ALREADY SAYS "Running · v…".** Repeating it here spent the page's one subtitle
  // line on a fact the header had just stated; the console's own build is the fact the chip cannot
  // carry, and the one a person needs when a page and a device disagree about what is deployed.
  const statusLine = [
    offline ? "Offline" : "Running",
    props.consoleVersion ? `console v${props.consoleVersion}` : null
  ]
    .filter(Boolean)
    .join(" · ");
  const capabilities = new Set(live.tools.map((tool) => tool.capability)).size;
  const notice = offline
    ? `${name} is not connected. The console keeps trying; open the machine's panel if it stays offline.`
    : live.error
      ? "The machine is running. The console could not read its tool list, so these counts may be stale."
      : null;

  return (
    <div className="page home-page">
      <header className="page-head">
        <div>
          <h1>Home</h1>
          <p className="page-sub">{statusLine}</p>
        </div>
        <div className="page-actions">
          <button type="button" onClick={props.onConnect}>
            Connect a client
          </button>
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

      <LiveNow
        lines={liveLines}
        devices={props.devices}
        state={props.liveState ?? "connecting"}
        onAll={props.onActivity}
        onWatch={props.onWatchTerminal}
      />

      <section className="panel home-timeline" aria-label="This machine's activity">
        <ActivityTimeline
          lines={timelineLines}
          devices={props.devices}
          onWatch={props.onWatchTerminal}
          empty={`No calls from ${name} today. When an AI client calls this machine, its work appears here.`}
        />
      </section>

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
    </div>
  );
}

export type HomePageProps = FleetProps & {
  /// The machine in context. Null is the fleet zero state; a chosen machine is the hub.
  device: Device | null;
  audit: AuditLine[];
  onConnect: () => void;
  onCapabilities: () => void;
  onActivity: () => void;
  /** The console's own build, from /health; the machine chip next door is the device's. */
  consoleVersion?: string | null;
  /** The live activity stream, newest first. Absent in tests and before the stream opens. */
  liveLines?: LiveLine[];
  liveState?: "connecting" | "live" | "offline";
  /** Open a machine's Terminal tab, where an AI's session can be watched. */
  onWatchTerminal?: (deviceId: string) => void;
};

export function HomePage(props: HomePageProps) {
  return props.device ? <HubHome {...props} device={props.device} /> : <FleetHome {...props} />;
}
