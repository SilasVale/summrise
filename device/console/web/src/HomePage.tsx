import type { Device } from "./api";
import { DeviceCard } from "./DeviceCard";
import { copyText } from "./copy";

/** A one-time enrolment code and the command that redeems it. */
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
 * adds one, and the enrolment command when it exists. This is the previous Devices page kept because
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
            A machine appears here after it enrols with this console. Run the command once on the
            Windows machine, and it dials out to this console.
          </p>
          {/* The same action as the page head, so an empty fleet has one primary action, not two. */}
          <button type="button" className="link" disabled={props.busy} onClick={props.onAddMachine}>
            Add a machine
          </button>
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

export type HomePageProps = FleetProps & {
  /// The machine in context. Null is the fleet zero state; the hub is built on top of this in C3.
  device: Device | null;
};

export function HomePage(props: HomePageProps) {
  return <FleetHome {...props} />;
}
