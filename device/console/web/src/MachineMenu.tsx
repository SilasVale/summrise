import { useEffect, useRef } from "react";
import type { Device } from "./api";
import { shortId } from "./format";

/**
 * THE HEADER IS ONE CONTROL. `[● workshop · online · v0.3.246 ▾]` is the machine and the picker at
 * once: its menu holds All machines, every machine, and Add a machine. A name beside a `<select>` that
 * repeats that name is the old rail's grammar — many controls, one fact.
 *
 * **A SWITCHER, NOT A SECOND FLEET TABLE.** The menu used to draw each machine's tightest disk, the
 * day's client calls and the last absence under its name — Home's board in miniature, with the
 * counters Activity owns and an unlabelled `I: 23.9 GiB free`. The question this control answers is
 * only "which machine?", so a row is the dot, the name, the state word and the build; the fleet's
 * verdict is Home's table and the counters are Activity's (audit-43 F10).
 */
export function MachineMenu({
  devices,
  chosen,
  onChoose,
  onAddMachine
}: {
  devices: Device[];
  chosen: Device | null;
  onChoose: (id: string | null) => void;
  onAddMachine: () => void;
}) {
  const ref = useRef<HTMLDetailsElement>(null);

  // A menu closes when the operator clicks anywhere else. `<details>` does not do that by itself.
  useEffect(() => {
    const onDown = (event: MouseEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) ref.current.removeAttribute("open");
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, []);

  const pick = (action: () => void) => {
    ref.current?.removeAttribute("open");
    action();
  };
  const label = chosen ? chosen.name ?? shortId(chosen.id) : "All machines";

  return (
    <details
      className="machine-menu"
      ref={ref}
      onKeyDown={(event) => {
        if (event.key === "Escape") ref.current?.removeAttribute("open");
      }}
    >
      <summary aria-label={chosen ? `Machine: ${label}` : "All machines"}>
        {chosen && (
          <span
            className={"dot " + (chosen.online ? "dot-ok" : "dot-off")}
            aria-hidden="true"
          />
        )}
        <span className="machine-menu-label">{label}</span>
        {chosen && (
          // **THE BUILD KEEPS ITS OWN ELEMENT BECAUSE THE PHONE KEEPS THE BUILD.** Below 640 px the
          // status word is hidden (the dot already carries it) while the version stays: an offline
          // Device page at 390 used to name no build anywhere, because this whole span was hidden.
          // The word is the fleet's `online`/`offline`, not the app's `Running`: one reachability
          // vocabulary on the shell, the attention rows and the table (audit-43 F2).
          <span className="machine-menu-meta">
            <span className="machine-menu-state">{chosen.online ? "online" : "offline"}</span>
            {chosen.version ? (
              <span className="machine-menu-version">{` · v${chosen.version}`}</span>
            ) : null}
          </span>
        )}
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="m6 9 6 6 6-6" />
        </svg>
      </summary>
      <div className="machine-pop" aria-label="Machines">
        <button
          type="button"
          aria-current={!chosen ? "true" : undefined}
          onClick={() => pick(() => onChoose(null))}
        >
          All machines
        </button>
        {devices.map((device) => {
          const name = device.name ?? shortId(device.id);
          return (
            <button
              key={device.id}
              type="button"
              aria-current={chosen?.id === device.id ? "true" : undefined}
              onClick={() => pick(() => onChoose(device.id))}
            >
              <span className={"dot " + (device.online ? "dot-ok" : "dot-off")} aria-hidden="true" />
              <span className="machine-pop-name">{name}</span>
              <span className="machine-pop-meta">
                {device.online ? "online" : "offline"}
                {device.version ? ` · v${device.version}` : ""}
              </span>
            </button>
          );
        })}
        <div className="machine-pop-sep" />
        <button type="button" onClick={() => pick(onAddMachine)}>
          Add a machine
        </button>
      </div>
    </details>
  );
}
