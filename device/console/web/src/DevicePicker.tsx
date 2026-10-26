import type { Device } from "./api";

/**
 * A device list for a page that needs one: the rail can be the first thing an operator clicks, and
 * "no device selected" is a dead end when the list is one panel away.
 */
export function DevicePicker({ devices, onChoose }: { devices: Device[]; onChoose: (id: string) => void }) {
  if (devices.length === 0) {
    return <p className="hint">No devices are enrolled yet. Add one from the Devices page.</p>;
  }
  return (
    <ul className="picker">
      {devices.map((device) => (
        <li key={device.id}>
          <span className={"dot " + (device.online ? "dot-ok" : "dot-off")} />
          <strong>{device.name ?? device.id.slice(0, 12)}</strong>
          {device.version && <span className="chip">{device.version}</span>}
          <button type="button" className="mini" onClick={() => onChoose(device.id)}>
            Use
          </button>
        </li>
      ))}
    </ul>
  );
}
