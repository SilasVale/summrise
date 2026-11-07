import type { DeviceUpdate, HostFacts } from "./api";
import { exactMoment, seenAgo } from "./format";

/**
 * THE DEVICE'S OWN ACCOUNT OF ITS LAST START, AND THE ONE UPDATE FACT THAT IS NEW.
 *
 * The console cannot tell a machine that slept from one an update broke: it knows only that frames
 * stopped. So it says the silence (`offline · last seen 7m ago`) and, beside it, the device's own
 * account of when the running process began. This module builds that line from the section the
 * device sent — the verdict's `detail`, the updater's last answer — and the newest entry in the
 * host frame's `restarts` journal, which is when this process started. It adds only the age and the
 * one word the operator uses for an applied release ("updated").
 *
 * THE START IS THE FACT THE CHIPS CANNOT STATE. The fleet row's chip and the header's chip carry
 * the running version; they cannot say when it began. So a start sentence is drawn with its age
 * even when its version repeats the chip:
 *
 *   started 0.3.286 · 24m ago                    a machine that restarted without changing version
 *   started 0.3.277 · 2h ago (updated 12m ago)   an answer that arrived inside this start
 *   update to 0.3.276 refused                    the updater's refusal, inside this start
 *   update to 0.3.276 rolled back to 0.3.275     the updater's rollback, inside this start
 *   refused: nothing was installed               the device's own current sentence
 *
 * Without a journal moment the console cannot age the start, and a repeated version has nothing
 * new to say, so it draws no line rather than invent a time. A start naming a version the surface
 * does not show is the contradiction the operator needs, and it is kept as written.
 *
 * NO SENTENCE ATTACHES AN UPDATE TO A START IT PREDATES. `last_update.at_ms` belongs to a process
 * (the one that folded the answer in); the newest entry in the host frame's `restarts` journal is
 * when the running process started. An answer before that moment is history — the running version
 * proves it took — so the start line says nothing about it. The Machine tab still shows the answer,
 * labelled as before this start (`UpdateStoryLine`). Without either moment the console cannot prove
 * the answer belongs to this start, and says nothing about the update rather than guess.
 *
 * THE SAME JOURNAL NAMES THE CHANNEL. The newest restart records the release the running process
 * started from; the launcher's current generation is the fallback for a device whose journal
 * predates the field. `updateChannel` is that one rule for both surfaces, so the fleet row and the
 * Machine line cannot disagree about which channel the "behind" comparison is about.
 */
export type StartStoryContext = {
  /** When the running process started, from the host frame's newest restart entry. */
  startedAtMs?: number | null;
  /** The version the surface shows beside this line, so the line does not repeat it for nothing. */
  shownVersion?: string | null;
  /**
   * True when the surface has its own version column or badge beside this sentence (Home's fleet
   * row). The row then states only when the start was; a start naming a different version is still
   * the contradiction that must be kept.
   */
  versionElsewhere?: boolean;
  /** The clock the age is read against; the components leave it to `Date.now()`. */
  nowMs?: number;
  /**
   * True when the page draws the update card beside this line (the Machine tab's `Updates`).
   *
   * **THE CARD OWNS THE UPDATE ANSWER THERE.** The card states the applied version first when it is
   * the running one, and it dates the updater's last answer; a parenthetical `(updated 12m ago)`
   * beside the start would say the same thing twice. So on that page the line states the running
   * version and when the start was — the update answer itself stays on the card.
   */
  updateShown?: boolean;
};

/** The sentence a surface draws, and the exact instant its `title` should carry when it is a start. */
export type StartStory = {
  text: string;
  title: string | null;
};

export function startedStory(
  update: DeviceUpdate | null | undefined,
  context: StartStoryContext = {}
): StartStory | null {
  const detail = update?.verdict?.detail?.trim();
  if (!detail) return null;
  const last = update?.last_update ?? null;

  // The device's own current sentence. The running app writes one when it folds the updater's
  // answer in without restarting; the console shows it as written. The one exception is an applied
  // sentence that names the version the surface already shows: its new fact is when, so it is aged
  // instead of repeated.
  if (!detail.startsWith("started ")) {
    const updatedTo = /^updated to\s+(\S+)/.exec(detail)?.[1] ?? null;
    if (updatedTo !== null && updatedTo === context.shownVersion) {
      // The card states this answer in its own row when the page draws one; the line has nothing
      // to add, so it adds nothing rather than repeating the age beside the card's result.
      if (context.updateShown) return null;
      return last && insideStart(last.at_ms, context)
        ? { text: `updated ${seenAgo(last.at_ms, context.nowMs)}`, title: null }
        : null;
    }
    return { text: detail, title: null };
  }

  // A start sentence is drawn even when its version is the one the chip shows: the chip states what
  // runs, never when it started. The journal's moment is the fact that dates it, and the title
  // carries the exact instant behind the age.
  const startVersion = detail.slice("started ".length).trim();
  const startedAt = startMoment(context.startedAtMs);
  const clause = context.updateShown
    ? null
    : updateClause(last, update?.rolled_back_to, context);
  // The version the surface's own column or badge already states; a start with no version at all
  // is the same "nothing to add here" for a surface that has its own copy.
  const repeatsSurface =
    startVersion === "" || startVersion === (context.shownVersion ?? null);
  let base: string | null;
  if (startedAt !== null) {
    const age = seenAgo(startedAt, context.nowMs);
    // **ONE VERSION PER ROW, AND ONE PER TAB.** A surface with its own version column (Home) states
    // the build there and only the age here; the Device shell has no such column, so its start line
    // carries the version on every tab — including Machine, where the update card draws the
    // updater's answer and the line above it is the only sentence that places what runs in time.
    base =
      context.versionElsewhere === true && repeatsSurface
        ? age
        : `started ${startVersion === "" ? age : `${startVersion} · ${age}`}`;
  } else {
    // No journal: never borrow another moment for this start. A differing version is still the
    // device's sentence; a repeat of the chip has nothing new to say.
    base =
      startVersion !== "" && startVersion !== (context.shownVersion ?? null) ? detail : null;
  }
  const text = base ? (clause ? `${base} (${clause})` : base) : clause;
  if (!text) return null;
  return { text, title: startedAt !== null ? exactMoment(startedAt) : null };
}

/**
 * The sentence alone, for callers that only draw text. It is exactly `startedStory(...).text`, so
 * there is one source of truth for the line and its title.
 */
export function startedLine(
  update: DeviceUpdate | null | undefined,
  context: StartStoryContext = {}
): string | null {
  return startedStory(update, context)?.text ?? null;
}

/**
 * The channel the running process started from, or null when neither fact names one.
 *
 * THE JOURNAL IS THE CLOSER FACT. `host.restarts[0]` is this process's own entry: it says the
 * release the running app started from, which is the channel the "running v…" half of the update
 * line describes. The launcher's `host.update.current.channel` is the generation it trusts on disk,
 * and it is the fallback only because a build whose journal predates the channel field still has the
 * sidecar. On the measured 84T the sidecar was null and the journal said `canary`; the console read
 * only the sidecar and printed "channel not reported" about a machine whose own panel said
 * `Channel canary`.
 *
 * A blank string is not a channel: a field the device wrote empty is a field it did not report.
 */
export function updateChannel(device: {
  host?: Pick<HostFacts, "restarts" | "update"> | null;
}): string | null {
  return (
    namedChannel(device.host?.restarts?.[0]?.channel) ??
    namedChannel(device.host?.update?.current?.channel)
  );
}

function namedChannel(value: unknown): string | null {
  return typeof value === "string" && value.trim() !== "" ? value.trim() : null;
}

/**
 * The moment the running process started, from the host frame's own boot journal.
 *
 * `host.restarts` is the device's record of its own boots, newest first; the newest entry is this
 * process. The machine's `uptime_s` is not that fact — it is how long the host has been up — and a
 * line built from it could attach an old answer to a start it predates.
 */
export function processStartMs(device: {
  host?: { restarts?: { at_ms?: number }[] | null } | null;
}): number | null {
  return startMoment(device.host?.restarts?.[0]?.at_ms);
}

function startMoment(at: number | null | undefined): number | null {
  return typeof at === "number" && Number.isFinite(at) && at > 0 ? at : null;
}

function insideStart(atMs: number, context: StartStoryContext): boolean {
  const startedAt = startMoment(context.startedAtMs);
  // Both moments must exist. A missing start is not "probably after": rule (c) forbids borrowing
  // one start's moment for another start's answer.
  return startedAt !== null && atMs >= startedAt;
}

/**
 * The updater's last answer as the line's own sentence, or null when it belongs to another start.
 *
 * `refused` and `rolled_back` are the device's own words and are used unchanged; `applied` is the
 * one the page reads as "updated" because that is what the operator calls a release that took over,
 * and it is aged rather than dated absolutely: the question is "did the 0.3.277 update take", and
 * "12m ago" answers when. An outcome this console does not know returns null, so a newer device's
 * word draws no guessed sentence.
 */
function updateClause(
  last: NonNullable<DeviceUpdate["last_update"]> | null,
  rolledBackTo: string | undefined,
  context: StartStoryContext
): string | null {
  if (!last || !insideStart(last.at_ms, context)) return null;
  switch (last.outcome) {
    case "refused":
      return last.to ? `update to ${last.to} refused` : "update refused";
    case "applied":
      // **AN APPLY THAT IS THE START IS SAID ONCE.** The measured 9VH and 84T both carry
      // `last_update.at_ms == restarts[0].at_ms`: the process began when the release took over.
      // `(updated 2h ago)` beside `started 2h ago` is one moment written twice, so the start alone
      // is the fact. An answer that arrived inside a longer-lived process is still its own age.
      if (last.at_ms === startMoment(context.startedAtMs)) return null;
      return `updated ${seenAgo(last.at_ms, context.nowMs)}`;
    case "rolled_back":
      if (last.to) {
        return rolledBackTo
          ? `update to ${last.to} rolled back to ${rolledBackTo}`
          : `update to ${last.to} rolled back`;
      }
      return rolledBackTo ? `rolled back to ${rolledBackTo}` : "rolled back";
    default:
      return null;
  }
}
