import { useEffect, useState } from "react";
import { callTool, type Grant } from "./terminal";

/** The part of a `monitor.list` target the chip reads; the Monitors page owns the full shape. */
type Target = { summary?: { up_now: boolean | null } | null };

type Report = { grant: Grant; down: number; total: number };

/// How often the chip re-reads the device's targets. The device probes on its own timer; this only
/// refreshes the fact the top bar shows.
const POLL_MS = 20_000;

/**
 * The one thing the top bar says about monitors: how many are down, wherever the operator is.
 *
 * It is a read of the grant the console already holds, not a route or a notification. Unknown is not
 * zero: a refused or failed list, a target with no summary, or an empty list renders nothing, and a
 * down target renders the fact without an error banner. A report belongs to the grant it was read
 * with, so a late answer from an old grant cannot paint the new one.
 */
export function MonitorChip({ grant, onOpen }: { grant: Grant; onOpen: () => void }) {
  const [report, setReport] = useState<Report | null>(null);

  useEffect(() => {
    let live = true;
    let inFlight = false;

    const read = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        const targets = JSON.parse(await callTool(grant, "monitor.list", {})) as Target[];
        if (!live || !Array.isArray(targets)) return;
        const down = targets.filter((target) => target?.summary?.up_now === false).length;
        setReport({ grant, down, total: targets.length });
      } catch {
        // The chip never becomes an error banner: a refresh that failed leaves the last fact alone.
      } finally {
        inFlight = false;
      }
    };

    void read();
    const timer = setInterval(() => void read(), POLL_MS);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [grant]);

  const current = report?.grant === grant ? report : null;
  if (!current || current.down === 0) return null;

  return (
    <button
      type="button"
      className="chip chip-down"
      title={`${current.down} of ${current.total} monitors down — open Monitors`}
      onClick={onOpen}
    >
      {current.down} monitor{current.down === 1 ? "" : "s"} down
    </button>
  );
}
