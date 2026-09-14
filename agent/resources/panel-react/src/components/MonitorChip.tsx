// MonitorChip — "a host you asked this device to watch is down", on the strip.
//
// WHY IT IS NOT PART OF THE CARD. The operator who asked for a watch is by definition not
// staring at the Settings page: the chip is how the answer reaches them wherever they are in
// the panel. It names the TARGET and for HOW LONG, because "192.168.1.1:22 down 4m" is the
// whole fact and "1 monitor down" would send them looking for which one.
//
// SILENT BY DEFAULT, like every other chip here: no targets, no probes yet, or everything up
// renders nothing at all. Several down targets collapse to one chip with the first name and a
// count — a strip is a line, not a list, and the card is one click away.
import { downTargets, fmtSince, type Monitors } from "../hooks/useMonitors";

export function MonitorChip({ monitors, nowMs = Date.now() }: { monitors?: Monitors | null; nowMs?: number }) {
  if (!monitors || monitors.targets.length === 0) return null;
  const down = downTargets(monitors, nowMs);
  if (down.length === 0) return null;
  const [first] = down;
  const more = down.length - 1;
  const text =
    `${first.target.host}:${first.target.port} down ${fmtSince(first.sinceMs)}` +
    (more > 0 ? ` (+${more})` : "");
  const title =
    down
      .map((d) => `${d.target.host}:${d.target.port} — down for ${fmtSince(d.sinceMs)}`)
      .join("\n") + "\n\nThe Reachability card in Settings shows the probe history.";
  return (
    <span className="monitor-chip" title={title} data-down={down.length}>
      <span className="monitor-mark" aria-hidden="true" />
      {text}
    </span>
  );
}
