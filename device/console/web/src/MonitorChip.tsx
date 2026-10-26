import { useEffect, useRef, useState } from "react";
import { notifyMonitors } from "./notifications";
import { callTool, ToolError, type Grant } from "./terminal";

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
 * zero: before any successful read, a refused or failed list, a target with no summary, or an empty
 * list renders nothing, and a down target renders the fact without an error banner. A report belongs
 * to the grant it was read with, so a late answer from an old grant cannot paint the new one. A failed
 * refresh keeps the last fact and retries; a grant the console or device refused is not retried at
 * all.
 */
export function MonitorChip({ grant, onOpen }: { grant: Grant; onOpen: () => void }) {
  const [report, setReport] = useState<Report | null>(null);

  // The last down count a successful read established. `null` means no read has succeeded yet, so
  // the first fact is not a transition and is never announced. A failed read leaves this untouched:
  // what the device last said is still the operator's best information.
  const previous = useRef<number | null>(null);
  // The poll restarts on `grant`, not on a parent re-render that hands over a new `onOpen`; the
  // click always runs the newest opener without making the device the price of a closure.
  const onOpenRef = useRef(onOpen);
  useEffect(() => {
    onOpenRef.current = onOpen;
  }, [onOpen]);

  useEffect(() => {
    let live = true;
    let inFlight = false;
    let timer: number | null = null;
    // A different grant is a different device's registry: its first successful read is another
    // "first", never a rise from the grant this chip used to show.
    previous.current = null;

    const stop = () => {
      if (timer === null) return;
      window.clearInterval(timer);
      timer = null;
    };

    const read = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        const targets = JSON.parse(await callTool(grant, "monitor.list", {})) as Target[];
        if (!live || !Array.isArray(targets)) return;
        const down = targets.filter((target) => target?.summary?.up_now === false).length;
        // Only a rise after a known count is a new outage. The count itself is the state; the
        // announcement belongs to the transition, once, however many polls repeat the same number.
        if (previous.current !== null && down > previous.current) {
          notifyMonitors(down, targets.length, onOpenRef.current);
        }
        previous.current = down;
        setReport({ grant, down, total: targets.length });
      } catch (cause) {
        // A refused grant is not retried: the envelope is stale or lacks the tool, and asking again
        // every 20 seconds cannot fix it. Any other failure keeps the last fact and retries.
        if (
          cause instanceof ToolError &&
          (cause.status === 403 || cause.code === -32001 || cause.code === -32002)
        ) {
          stop();
        }
      } finally {
        inFlight = false;
      }
    };

    timer = window.setInterval(() => void read(), POLL_MS);
    void read();
    return () => {
      live = false;
      stop();
    };
  }, [grant]);

  const current = report?.grant === grant ? report : null;
  if (!current || current.down === 0) return null;

  return (
    <button
      type="button"
      className="chip chip-bad chip-action"
      title={`${current.down} of ${current.total} monitors down — open Monitors`}
      onClick={onOpen}
    >
      {current.down} monitor{current.down === 1 ? "" : "s"} down
    </button>
  );
}
