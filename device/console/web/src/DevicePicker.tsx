import type { Device } from "./api";
import { lastHeardMs, seenAgo, shortId } from "./format";
import type { FleetRead } from "./HomePage";

/**
 * A device list for a page that needs one: the rail can be the first thing an operator clicks, and
 * "no device selected" is a dead end when the list is one panel away.
 *
 * **AN EMPTY LIST IS NOT AN EMPTY FLEET.** `/fleet` can be on the wire for fifteen seconds or fail,
 * and the page held an empty list in both states; it said `No devices are enrolled yet.` for all of
 * them (audit-43 F4). The read's state is what separates "none enrolled" from "not read yet" and
 * "could not be read", so it is a prop here and only a confirmed empty read draws the enrollment
 * sentence. A list already in hand stays useful while a reread is pending or failed: it is the last
 * thing the console knew, and the shell's banner owns the failure.
 */
export function DevicePicker({
  devices,
  onChoose,
  fleetRead,
  onRetry
}: {
  devices: Device[];
  onChoose: (id: string) => void;
  /** What the latest `/fleet` read answered. Absent is a confirmed answer for the list in hand. */
  fleetRead?: FleetRead;
  /** Read `/fleet` again after a failed read: the page's Retry, not the shell's poll. */
  onRetry?: () => void;
}) {
  const fleet = fleetRead ?? { state: "ready" as const };
  if (devices.length === 0) {
    if (fleet.state === "reading") {
      // **THE PAGE RESERVES THE SHAPE, NOT A SENTENCE (the ninety-seventh list).** Measured 2026-11-11 with
      // `/fleet` held 90 s: SIX of the seven pages of this product draw the contract's `loading` atom while a
      // read is held, and this page drew `p.hint` — **the same sentence Home's skeleton carries as its
      // `aria-label`, "Reading the machine list…", in a 52 px card where three 60 px rows will arrive.**
      // The panel's own results page had this hole and the eightieth list fixed it there; the console's Device
      // page was never fixed with it, which is what a list that names ONE occurrence always leaves behind.
      //
      // The rows are this page's own: the picker draws `.picker li` at the `row` atom's inset, so the skeleton
      // reserves THEIR shape — a name and one line of facts, at the card's 8 px rhythm — and the page does not
      // jump from a one-line box to a four-row list when the read answers.
      return (
        <div className="skeleton-list picker-skeleton" aria-label="Reading the machine list" role="status">
          {[0, 1, 2].map((row) => (
            <div className="skeleton-row" key={row}>
              <span className="skeleton skeleton-name" />
              <span className="skeleton skeleton-line" />
            </div>
          ))}
        </div>
      );
    }
    if (fleet.state === "failed") {
      return (
        <div className="notice" role="status">
          <span title={fleet.reason}>The machine list could not be read.</span>
          <button type="button" onClick={() => onRetry?.()}>
            Retry
          </button>
        </div>
      );
    }
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
              aria-label={`Open ${device.name ?? shortId(device.id)}`}
              onClick={() => onChoose(device.id)}
            >
              Open
            </button>
          </li>
        );
      })}
    </ul>
  );
}
