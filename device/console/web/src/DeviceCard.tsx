import type { ChannelActivity, DeviceUpdate, HostFacts } from "./api";
import type { LastAttempt } from "./attempt";
import { presenceText, shortId } from "./format";
import { startedStory, processStartMs } from "./updateStory";

export type Device = {
  id: string;
  name: string | null;
  online: boolean;
  version: string | null;
  /** When the console last saw a frame, or absent when it has seen none since it started. */
  last_frame_ms?: number | null;
  /** When the current silence started, from the newest still-open `device.left`. */
  last_left_ms?: number | null;
  /** The registry's last channel open: older evidence than either of the two above. */
  last_seen_ms?: number | null;
  /**
   * The AI's calls the local calendar day holds for this machine, and how many failed. Home does
   * not draw it — the day's counts are Activity's noun — but the fleet read still carries it and
   * the page passes the row unchanged.
   */
  today?:
    | {
        client_calls: number;
        failed: number;
        since_ms?: number | null;
        bounded?: boolean;
        scan_limit?: number | null;
      }
    | null;
  /**
   * The attempt before the current channel. The row does not draw it: it is a count and a reason
   * with no moment of its own, and a sentence about it read as today's fault (C9). The field stays
   * because the page passes the fleet's row unchanged, and a test pins that the row ignores it.
   */
  last_attempt?: LastAttempt | null;
  /**
   * The audit's join/leave tally for this machine's channel, over the window the absence read uses.
   * The row does not draw it — the same C9 rule as `last_attempt`: a count with no moment of its own
   * reads as a fault happening now, and the Machine tab is where the sentence that explains it
   * belongs. The field stays because the page passes the fleet's row unchanged.
   */
  channel_activity?: ChannelActivity | null;
  host?: HostFacts | null;
  /** The device's own update story, absent when it never sent one. */
  update?: DeviceUpdate | null;
};

type Props = {
  device: Device;
  selected: boolean;
  onSelect: () => void;
  onRename: () => void;
  onRemove: () => void;
  onDiagnostics: () => void;
  /**
   * **THE ROW'S TWO NEW DOORS (the hundred-and-eighty-first list).** The console can now watch a service on a machine and read what
   * its AI learned, and neither was reachable from the fleet table — the page a person uses to pick a machine. The menu is where a
   * row's management already lives, so the doors go there rather than into a new column.
   */
  onOpenTab?: (tab: "machine" | "memory") => void;
};

/**
 * One machine in Home's fleet table: Machine / Status / Version / Started, and the overflow.
 *
 * The row is a real `<tr>` because the page draws a table: the four facts line up column-wise for
 * a fleet comparison, and the status dot sits with the word it colours. The name is the row's
 * link (the button's accessible name is the machine), and the overflow holds the three management
 * actions. There is no per-row Connect, no accent on a row, and no client-call counter: how much a
 * machine worked is Activity's noun, while "is it here, what does it run, when did it start" is
 * the fleet's. A machine behind the staged installer is named in Home's Needs-attention block, not
 * twice on its row.
 */
export function DeviceCard({
  device,
  selected,
  onSelect,
  onRename,
  onRemove,
  onDiagnostics,
  onOpenTab
}: Props) {
  const label = device.name ?? shortId(device.id);
  // **THE AGE IS THE NEWEST EVIDENCE THE CONSOLE HAS.** A frame proves presence and wins; a console
  // that restarted mid-outage still has the audit's newest open `device.left`; the registry's
  // `last_seen_ms` is the last channel open, older than either. The row states the same moment the
  // attention line does, in the same words: `presenceText` is that one phrase (audit-43 F2).
  const state = presenceText(device);
  // NO RECONNECT SENTENCE. `last_attempt` is a count and a reason with no moment of its own: both
  // online rows said "Reconnected after 1 attempt — …" at 10:07 about the 09:41/09:43 restarts, and
  // the sentence read as a fault that was happening now. `/fleet` carries no time for the attempt,
  // and the last frame is after the reconnect rather than at it, so any date here would be borrowed
  // from a different fact. The row draws neither the count nor the reason; Copy diagnostics keeps
  // the attempt as a record and dates the moment the console last heard the machine.
  // The device's own account of its last start, beside the console's account of its silence. The two
  // lines are independent on purpose: the console knows frames stopped, not that the machine slept,
  // and the device knows what it started and when, not when the console last heard it. The start is
  // drawn even when its version repeats the chip: the chip states what runs, never when it began,
  // and that moment is what places the boot against the silence. An update that belongs to
  // this start is aged beside it; an answer from before the start is left as history elsewhere.
  const started = startedStory(device.update, {
    startedAtMs: processStartMs(device),
    shownVersion: device.version,
    // The row's Version cell already states the build, so Started states only when. A start naming
    // a different version is still drawn as the contradiction it is.
    versionElsewhere: true
  });
  return (
    <tr className={"fleet-row" + (selected ? " is-current" : "")}>
      <td className="fleet-machine">
        <button type="button" className="fleet-open" aria-label={`Open ${label}`} onClick={onSelect}>
          <span className="fleet-name">{label}</span>
          <span className="fleet-chevron" aria-hidden="true">›</span>
        </button>
      </td>
      <td className="fleet-status">
        <span className={"dot " + (device.online ? "dot-ok" : "dot-off")} aria-hidden="true" />
        <span className={"fleet-state" + (device.online ? " ok" : "")}>{state}</span>
      </td>
      <td className="fleet-version">
        {device.version ? (
          <span className="fleet-build mono">v{device.version}</span>
        ) : (
          <span className="fleet-empty" aria-label="No version reported">—</span>
        )}
      </td>
      <td className="fleet-started">
        {/* **THE PHONE'S CARD HAS NO HEADER TO LABEL THIS AGE.** Below 640 px the table becomes a
            card and `thead` is `display:none`, so the cell read as a second bare `… ago` under the
            status age (audit-43 F3). The word is the header the phone does not draw; the desktop
            table hides it, because its `Started` column header already labels the cell. */}
        <span className="fleet-started-label">Started</span>
        {started ? (
          <span className="fleet-verdict" title={started.title ?? started.text}>
            {started.text}
          </span>
        ) : (
          <span className="fleet-empty" aria-label="No start reported">—</span>
        )}
      </td>
      <td className="fleet-actions">
        <details className="row-menu">
          <summary aria-label={`More about ${label}`} title={`More about ${label}`}>⋯</summary>
          <div className="row-menu-pop">
            <button type="button" onClick={onDiagnostics}>
              Copy diagnostics
            </button>
            {onOpenTab !== undefined && (
              <>
                <button type="button" onClick={() => onOpenTab("machine")}>
                  Watch a service
                </button>
                <button type="button" onClick={() => onOpenTab("memory")}>
                  Read its memory
                </button>
              </>
            )}
            <button type="button" onClick={onRename}>
              Rename
            </button>
            <button type="button" className="danger" onClick={onRemove}>
              Remove
            </button>
          </div>
        </details>
      </td>
    </tr>
  );
}
