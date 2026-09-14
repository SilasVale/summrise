// What the panel says about the PREVIOUS run of the agent — the one rule, in one
// place, so the two densities (panel strip and desktop footer) cannot disagree.
//
// WHY A RULE AT ALL. The device has known how the last run ended since round 254:
// `REPLACED by a restart` (an update swap or a task restart) or `CRASHED or was
// killed`. That sentence lived in `logs\startup.log` on the machine, and a field
// engineer reading a log is not the operator watching a panel. `/api/status` now ships
// it, and this file decides what it is worth saying out loud — because the two cases
// deserve OPPOSITE treatment: one is the normal consequence of updating, and one is a
// fault. Rendering them the same way is how an operator learns to ignore both.
//
// TWO TONES, TWO LIFETIMES:
//
//   crashed   A fault, and it stays true for the whole life of the current run — the
//             agent is running BECAUSE something restarted it after a bad death. So the
//             chip stays until the next clean boot clears it (the device rewrites the
//             verdict every start). It is not an alarm that needs dismissing; it is a
//             fact about the run you are in, and the uptime beside it says how long ago.
//   replaced  Normal. Worth saying only while it is NEWS — right after `vale update`,
//             the operator wants the panel to confirm the swap happened. After that the
//             uptime already answers it, and a permanent "restarted" chip would be
//             chrome that trains people to stop reading chips (the lesson
//             `WaitingChip` records for its own zero state).
//
// `first-run` and `clean-exit` say nothing: nothing happened. So does `machine-restart`
// — a device that was rebooted (or lost power) did not fault, and the whole reason that
// verdict exists apart from `crashed` is that it must NOT raise this chip: a warning
// that fires after every routine reboot is one nobody reads by the time a real crash
// arrives.
import type { BootKind, LastBoot } from "../hooks/useAgentVitals";

/** The kinds, in words. ONE vocabulary for every surface that names a verdict — the chip's
 *  hover, the history card's rows — so a kind cannot be described two ways in one panel.
 *  `null` (an unrecognised kind) says so rather than borrowing another kind's wording. */
export function bootKindLabel(kind: BootKind | null): string {
  switch (kind) {
    case "first-run":
      return "first start";
    case "clean-exit":
      return "previous run exited cleanly";
    case "replaced":
      return "replaced by a restart";
    case "machine-restart":
      return "the machine restarted";
    case "crashed":
      return "previous run crashed";
    default:
      return "unrecorded";
  }
}

/** True for the one kind that means the agent died on its own. Used by the history card to
 *  weigh a row and by the summary line to count; the chip has its own rule below. */
export const isCrash = (kind: BootKind | null): boolean => kind === "crashed";

/** How long a normal restart keeps its chip. Five minutes is measured rather than
 *  chosen: the update flow's own swap+restart is seconds, the panel polls `/api/status`
 *  every 15 s, and an operator who just clicked update looks at the panel immediately —
 *  so the chip is seen by whoever caused it, and is gone by the time the page is
 *  reopened for other work. */
export const REPLACED_NOTICE_SECS = 300;

export interface BootNotice {
  /** `warn` = a fault the operator may need to act on; `info` = news, then gone. */
  tone: "warn" | "info";
  /** The chip's text — short enough for the strip, never colour-only. */
  text: string;
  /** The full sentence from the device, for the hover. */
  title: string;
}

/** The chip to render, or `null` for "nothing worth saying".
 *
 *  `uptimeSecs` is required for the `replaced` case and may be null: without a
 *  trustworthy "how long ago", a normal restart cannot be told from a stale one, and
 *  the rule then says nothing rather than guessing.
 *
 *  `recentCrashes` is the device's own 24 h count from `/api/boots`. It never changes
 *  WHETHER the chip appears — it is the same verdict either way — and it only ever adds a
 *  sentence to the hover: "this happened once" and "this keeps happening" are different
 *  situations, and an operator staring at the chip is exactly who needs to know which.
 */
export function bootNotice(
  lastBoot: LastBoot | null | undefined,
  uptimeSecs: number | null | undefined,
  recentCrashes?: number | null,
): BootNotice | null {
  if (!lastBoot) return null;
  switch (lastBoot.kind) {
    case "crashed": {
      const base = `${lastBoot.detail}\n\nThe agent is running now — this is how the run before it ended.`;
      const repeated =
        typeof recentCrashes === "number" && recentCrashes > 1
          ? `\n\n${recentCrashes} crashes in the last 24 hours — the Restarts card in Settings lists them.`
          : "";
      return { tone: "warn", text: "last run crashed", title: base + repeated };
    }
    case "replaced":
      if (typeof uptimeSecs !== "number" || uptimeSecs >= REPLACED_NOTICE_SECS) {
        return null;
      }
      return { tone: "info", text: "just restarted", title: lastBoot.detail };
    default:
      // `first-run` (nothing precedes this run) and `clean-exit` (it stopped on
      // purpose): neither is news. An unrecognised kind lands here too — a newer agent
      // inventing a verdict this build cannot explain must render as SILENCE, never as
      // a warning it does not understand.
      return null;
  }
}
