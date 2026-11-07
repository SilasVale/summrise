import type { Device } from "./api";
import { lastHeardMs, seenAgo, shortId } from "./format";

/**
 * A device list for a page that needs one: the rail can be the first thing an operator clicks, and
 * "no device selected" is a dead end when the list is one panel away.
 */
export function DevicePicker({ devices, onChoose }: { devices: Device[]; onChoose: (id: string) => void }) {
  if (devices.length === 0) {
    return <p className="hint">No devices are enrolled yet. Add one from Home.</p>;
  }
  return (
    <ul className="picker">
      {devices.map((device) => {
        // **THE STATE IS READ, NOT DECODED.** The picker used to be a coloured dot with an
        // `aria-hidden` dot and the words in `.sr-only`: choosing a machine was choosing a colour.
        // The word is visible now, and the age Home spells out is its title. The version keeps its
        // `v` so it reads as the same fact as the fleet table's column.
        const heard = lastHeardMs(device);
        const state = device.online ? "online" : "offline";
        const age =
          heard === null ? state : `${state} · ${device.online ? "seen" : "last seen"} ${seenAgo(heard)}`;
        return (
          <li key={device.id}>
            <span className={"dot " + (device.online ? "dot-ok" : "dot-off")} aria-hidden="true" />
            <strong>{device.name ?? shortId(device.id)}</strong>
            <span className={"picker-state" + (device.online ? " ok" : "")} title={age}>
              {state}
            </span>
            {device.version && <span className="chip">v{device.version}</span>}
            <button
              type="button"
              className="mini"
              aria-label={`Use ${device.name ?? shortId(device.id)}`}
              onClick={() => onChoose(device.id)}
            >
              Use
            </button>
          </li>
        );
      })}
    </ul>
  );
}
