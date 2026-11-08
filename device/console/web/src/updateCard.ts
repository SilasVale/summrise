import type { DeviceUpdate, HostFacts } from "./api";
import { clockTimeSeconds, exactMoment, isoDate } from "./format";
import { processStartMs, startedLine, updateChannel } from "./updateStory";
import { versionEqual, versionRelation } from "./versions";

/**
 * THE MACHINE TAB'S UPDATE STORY, IN THE DEVICE PANEL'S UPDATES SHAPE.
 *
 * The panel answers "did the update land, and is this machine current?" with a labelled grid —
 * `Last target`, `Last outcome`, `Last result`, `Last check` — and the console answered it with one
 * grey sentence: `running v0.3.290 · channel canary · last check 01:51 · last outcome applied to
 * 0.3.290 on 00:22`. The sentence made the applied answer its last clause, repeated the running
 * version, and said the target version twice; the operator read a paragraph to find the one fact
 * the page exists to state. This module builds the grid instead.
 *
 * ONE VERSION PER ROW, THE APPLIED VERSION FIRST WHEN IT IS THE RUNNING ONE. The device's own
 * `install.version` is what runs, and the updater's last `applied` target is a second field that
 * usually names the same release. When it does, the grid's first row is `Applied 0.3.290` — the
 * answer, and the one place that version is written (`Running` and `Last target` would each restate
 * it, so neither is drawn). When it does not, the running version and the target are different
 * facts and each gets its own row, so a machine behind its last target reads `Running 0.3.280`
 * beside `Last target 0.3.290` instead of a sentence that hides the gap. A rollback's
 * `Rolled back to` also replaces `Running` when the two name the same release.
 *
 * A RECORD FROM BEFORE THIS START IS HISTORY, NOT THE LAST OUTCOME. The newest entry in the host
 * frame's boot journal is when the running process began; an updater answer older than it belongs
 * to an earlier process. It stays on the grid — it is the device's own record — with `before this
 * start` beside it, so the 9VH's two-day-old apply cannot read as what the machine just did.
 *
 * NO INVENTED ROWS. Every row is a fact the fleet carries: the device's state document (running
 * version, last target, outcome, result, check), the boot journal or launcher sidecar (channel, the
 * start moment), and the device's own verdict sentence when no swap answer was ever folded in. The
 * console has no update phase, so the panel's `Phase` row is not drawn; nothing substitutes one.
 * A record the device did not report says `not recorded`, and a channel it never named says `not
 * reported` — the page's own absent words, never a guessed value.
 */
export type UpdateRow = {
  key: string;
  /** The panel's label for the fact, so the console's card reads like the device's own. */
  label: string;
  /** The value as text. For a moment this is the console's clock; `exact` carries the instant. */
  text: string;
  /** The exact instant behind a moment row, for its `<time>` title. */
  exact?: string;
  /** True for the page's absent words (`not recorded`, `not reported`), not a device fact. */
  absent?: boolean;
};

/** The fleet fields the update story reads; the same shape `Device` and the tests carry. */
export type UpdatePageDevice = {
  /** The version the surface's chip shows, for the verdict fallback's age. */
  version?: string | null;
  update?: DeviceUpdate | null;
  host?: Pick<HostFacts, "restarts" | "update"> | null;
};

/**
 * The rows of the Machine tab's update card, in reading order.
 *
 * The card is drawn only when the device sent an update story at all; a device that never sent one
 * keeps the page's one sentence instead of a grid of its own absence (`UpdateCard`).
 *
 * `release` is the staged installer's version — the one fact that turns the running number into the
 * page's answer. `undefined` is "not read yet" and adds no clause; `null` is a console that knows it
 * has no staging (or could not read one), which is stated as `release unknown` rather than left as
 * an unstated comparison. A known release is compared once, on the row that states the version, so
 * `0.3.292 · ahead of the release 0.3.291` is the whole answer and no other row repeats either
 * number.
 */
export function updateRows(
  device: UpdatePageDevice,
  nowMs = Date.now(),
  release: string | null | undefined = undefined
): UpdateRow[] {
  const update = device.update ?? null;
  const last = update?.last_update ?? null;
  const verdict = update?.verdict ?? null;
  const detail = verdict?.detail?.trim() ?? "";
  const running = trimmed(update?.current);
  const target = trimmed(last?.to);
  // An applied answer names the version that took over. Only `applied` claims a release landed; a
  // refusal or a rollback names what was attempted and what the device returned to.
  const applied = last?.outcome === "applied" ? target : null;
  const startedAtMs = processStartMs(device);
  const rolledBack = last?.outcome === "rolled_back" ? trimmed(update?.rolled_back_to) : null;
  const beforeStart =
    startedAtMs !== null &&
    last !== null &&
    Number.isFinite(last.at_ms) &&
    last.at_ms < startedAtMs;
  const rows: UpdateRow[] = [];

  if (last) {
    const appliedRow =
      applied !== null && running !== null && versionEqual(applied, running);
    if (appliedRow) {
      // THE APPLIED VERSION IS THE ONE RUNNING: the answer leads, and it is the only row that states
      // this version. `Running` and `Last target` would each repeat it. The row spells the running
      // version, because that is the field the header's chip shows and the two must not look like
      // different releases (`0.3.290` and `0.3.290.0` are not).
      rows.push({ key: "applied", label: "Applied", text: versionText(running, release) });
    } else {
      if (running !== null && !(rolledBack !== null && versionEqual(running, rolledBack))) {
        rows.push({ key: "running", label: "Running", text: versionText(running, release) });
      }
      if (target !== null) rows.push({ key: "target", label: "Last target", text: target });
    }
    // **AN APPLIED ROW THAT LEADS ALREADY SAYS THE OUTCOME.** `Last outcome applied` directly under
    // `Applied 0.3.291` repeated the same word for the same swap; the row is kept only when it
    // carries something the leading row does not — another outcome, or this answer's place before
    // the running start.
    if (!(appliedRow && last.outcome === "applied" && !beforeStart)) {
      rows.push({
        key: "outcome",
        label: "Last outcome",
        text: outcomeWord(last.outcome) + (beforeStart ? " · before this start" : "")
      });
    }
    if (rolledBack !== null) {
      // When the rollback returned to the running version there is no `Running` row above (it would
      // repeat this one), so the verdict rides here: the card must still answer "is it current?".
      const rollbackIsRunning = running !== null && versionEqual(running, rolledBack);
      rows.push({
        key: "rolled_back_to",
        label: "Rolled back to",
        text: rollbackIsRunning ? versionText(rolledBack, release) : rolledBack
      });
    }
    rows.push(momentRow("result", "Last result", last.at_ms));
  } else if (verdict !== null && detail !== "") {
    // **THE VERDICT IS THE RESULT WHEN NO SWAP ANSWER WAS FOLDED IN.** A device whose only update
    // result is its last start's sentence (the measured 84T's `started 0.3.280`) must not read as a
    // device with no record: the verdict is drawn as the device wrote it, aged only when the journal
    // proves when the start was.
    if (running !== null) {
      rows.push({ key: "running", label: "Running", text: versionText(running, release) });
    }
    rows.push({
      key: "verdict",
      label: "Verdict",
      text:
        `${verdict.outcome} · ` +
        (startedLine(update, { startedAtMs, shownVersion: device.version, nowMs }) ?? detail)
    });
  } else {
    // A story with no record: the panel's rows still say what is absent, so the operator learns the
    // device has no answer rather than meeting an empty card.
    if (running !== null) {
      rows.push({ key: "running", label: "Running", text: versionText(running, release) });
    }
    rows.push({ key: "target", label: "Last target", text: "not recorded", absent: true });
    rows.push({ key: "outcome", label: "Last outcome", text: "not recorded", absent: true });
    rows.push({ key: "result", label: "Last result", text: "not recorded", absent: true });
  }

  rows.push(checkRow(update?.last_check?.at_ms, last?.at_ms));
  const channel = updateChannel(device);
  rows.push({
    key: "channel",
    label: "Channel",
    text: channel ?? "not reported",
    absent: channel === null
  });
  return rows;
}

/**
 * The running version plus the card's one comparison with the staged release.
 *
 * An unread installer adds nothing (`undefined`); a console that read it and found no staging says
 * `release unknown`; a known release is compared once. The `current` clause does not repeat the
 * number — the running version at the start of the row is the whole fact — while "behind"/"ahead"
 * must name the release, because that is the number the running one is being placed against.
 */
function versionText(
  running: string,
  release: string | null | undefined
): string {
  if (release === undefined) return running;
  if (release === null) return `${running} · release unknown`;
  const relation = versionRelation(running, release);
  if (relation === "current") return `${running} · current (the release)`;
  if (relation === "behind") return `${running} · behind the release ${release}`;
  if (relation === "ahead") return `${running} · ahead of the release ${release}`;
  // A value that is not a dotted version cannot be compared; name it rather than claim a side.
  return `${running} · release ${release}`;
}

/** A row for a moment in the console's clock, with the exact instant for its title. */
function momentRow(key: string, label: string, atMs: number | null | undefined): UpdateRow {
  if (typeof atMs === "number" && Number.isFinite(atMs) && atMs > 0) {
    return {
      key,
      label,
      text: `${isoDate(atMs)} ${clockTimeSeconds(atMs)}`,
      exact: exactMoment(atMs)
    };
  }
  return { key, label, text: "not recorded", absent: true };
}

/**
 * The check row, with the one qualification a missing record needs.
 *
 * **A CHECK THE DEVICE DID NOT RECORD IS NOT A MACHINE THAT NEVER CHECKED.** The measured 84T had
 * an applied 0.3.291 and `lastCheck: null`: the installer/launcher path that put the release on
 * the machine wrote no check. Beside that apply, a bare `not recorded` read as "never looked", so
 * the row names the order the device does prove — the record is absent before this update. With no
 * result to bound it, the row keeps the page's plain absent words.
 */
function checkRow(checkAtMs: number | null | undefined, resultAtMs: number | null | undefined): UpdateRow {
  const row = momentRow("check", "Last check", checkAtMs);
  if (!row.absent) return row;
  if (typeof resultAtMs === "number" && Number.isFinite(resultAtMs) && resultAtMs > 0) {
    return {
      key: "check",
      label: "Last check",
      text: "not recorded before this update",
      absent: true
    };
  }
  return row;
}

/** The device's outcome word, with `rolled_back` spelled the way the page reads it. */
function outcomeWord(outcome: string): string {
  return outcome === "rolled_back" ? "rolled back" : outcome;
}

/** A non-blank string, trimmed, or null. A field the device wrote empty is a field it did not send. */
function trimmed(value: string | null | undefined): string | null {
  return typeof value === "string" && value.trim() !== "" ? value.trim() : null;
}
