import { useEffect, useRef } from "react";
import { compactAge } from "./age";
import type { Device, HostFacts } from "./api";
import { ageAt, shortId } from "./format";

/**
 * THE HEADER IS ONE CONTROL. `[● workshop · Running · v0.3.246 ▾]` is the machine and the picker at
 * once: its menu holds All machines, every machine, and Add a machine. A name beside a `<select>` that
 * repeats that name is the old rail's grammar — many controls, one fact.
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
  // The absence is an age, so it is read against the clock of this render, not of the module.
  const now = Date.now();

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
          <span className="machine-menu-meta">
            {chosen.online ? "Running" : "Offline"}
            {chosen.version ? ` · v${chosen.version}` : ""}
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
          const line = machineLine(device, now);
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
                {device.online ? "Running" : "Offline"}
                {device.version ? ` · v${device.version}` : ""}
              </span>
              {/* The row's grid is the fleet row's: dot, name, meta, then a muted subline that
                  spans from the name to the end. `.fleet-attempt` is that subline's rule. */}
              {line && <span className="fleet-attempt">{line}</span>}
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

/**
 * One muted line under a machine's name: the disk that fills first and the last absence, side by
 * side so comparing four machines is four rows rather than four Machine pages.
 *
 * Each half is optional and an unknown half is left out, never zeroed: a machine that has sent no
 * host facts has no disk, and a console that has not seen this device drop omits `last_outage`
 * entirely. Two unknowns therefore draw no line rather than an empty or invented one.
 */
function machineLine(device: Device, now: number): string | null {
  const facts: string[] = [];
  const disk = tightestDisk(device.host?.disks);
  if (disk) facts.push(`${diskLabel(disk.name)} ${gib(disk.free_bytes)} free`);
  if (device.last_outage) {
    facts.push(
      `back ${ageAt(device.last_outage.back_at_ms, now)} after ${compactAge(device.last_outage.away_ms)}`
    );
  }
  return facts.length === 0 ? null : facts.join(" · ");
}

type Disk = { name: string; total_bytes: number; free_bytes: number };

/**
 * The disk with the lowest free percentage among those with a positive total: a huge disk with a few
 * percent left fills before a small one with half left. The Machine page's vitals chart makes the
 * same choice, so the two surfaces cannot name different disks.
 */
function tightestDisk(disks: HostFacts["disks"]): Disk | null {
  let tightest: Disk | null = null;
  let tightestRatio = Number.POSITIVE_INFINITY;
  for (const disk of disks ?? []) {
    if (!disk || typeof disk.name !== "string") continue;
    const { total_bytes: total, free_bytes: free } = disk;
    if (typeof total !== "number" || !Number.isFinite(total) || total <= 0) continue;
    if (typeof free !== "number" || !Number.isFinite(free) || free < 0) continue;
    const ratio = free / total;
    if (ratio < tightestRatio) {
      tightestRatio = ratio;
      tightest = disk;
    }
  }
  return tightest;
}

/** `C:\` reads `C:`; `/` stays `/`; a volume's label is already its own name. */
function diskLabel(name: string): string {
  const trimmed = name.replace(/[\\/]+$/, "");
  return trimmed.length > 0 ? trimmed : name;
}

/** Free space at a glance: whole GiB when it is whole, one decimal when it is not. */
function gib(bytes: number): string {
  const value = bytes / 1024 ** 3;
  const rounded = Math.round(value * 10) / 10;
  return `${Number.isInteger(rounded) ? rounded.toFixed(0) : rounded.toFixed(1)} GiB`;
}
