/**
 * The session spine: the record drawn as a session → transaction → call document.
 *
 * The page does not invent this shape and it does not read a call's arguments to find it. Every
 * value here comes from a **row field** the console or the device published — `txn`, `purpose`,
 * `goal`, `receipt`, `actor`, `outcome` — so two rows describing one call cannot group differently
 * on two surfaces, and a purpose is never recovered by guessing from a command line.
 *
 * Reading order is the record's: oldest first. The rail draws it newest first; this module returns
 * the document, not the viewport. A transaction is one group; its header is the purpose (a claim,
 * printed with its author); its body is the rows the device observed; its foot is either the
 * device's receipt or the plain fact that nothing ended it.
 *
 * Rows that carry no transaction (a session boundary, an ungrouped call) stay rows. A row this
 * build cannot classify is **unknown**: it is drawn neutrally and counted, never dropped and never
 * given a meaning the record did not carry.
 */
import type { Actor } from "./sessionActivity";
import type { TxnReceipt } from "./liveActivity";

/** The classes a row can belong to, as the row's own fields answer them. */
export type SpineOutcome =
  | "working"
  | "ok"
  | "failed"
  | "refused"
  | "committed"
  | "aborted"
  | "session"
  | "unknown";

/** One row, reduced to the fields the spine reads. */
export type SpineRow = {
  id: string;
  /** When the record wrote it. */
  atMs: number;
  /** Whose words the row carries: a call's caller, or the author of a claim. */
  actor: Actor;
  /**
   * The record's own class of the row, when it published one (`txn.started`, `goal.set`,
   * `call.denied`, …). Absent on older console rows; the fields below then classify it.
   */
  rowKind?: string;
  /** The transaction the row belongs to, from the row itself. */
  txn?: string | null;
  /** The transaction's declared purpose, on its begin (or its receipt's) row. */
  purpose?: string | null;
  /** A goal this row records: the sentence, or `null` when it cleared the goal. */
  goal?: { text: string } | null;
  /** The device's receipt, on the transaction's end row. Its counts are the device's. */
  receipt?: TxnReceipt | null;
  /**
   * What the row says happened, in the closed vocabulary above — or any other string a record
   * published. A value the spine cannot place is what makes the row unknown, so it is kept as the
   * string it was rather than widened to a meaning.
   */
  outcome?: SpineOutcome | string;
  /** The row as the card draws it. Optional: the spine never reads a title to group. */
  title?: string;
  facts?: readonly string[];
  jump?: string;
  /** The card's tone, when a view already assigned one. */
  tone?: "running" | "ok" | "failed" | "session";
};

/** One transaction, as the record describes it. */
export type SpineGroup = {
  /** The transaction's id, when a row names one. */
  txn: string | null;
  /** The caller's declared purpose; `null` when no row in this window retained it. */
  purpose: string | null;
  /** Who declared it: the author printed beside the claim, never an implied "the operator". */
  author: Actor;
  /** `working` until an end row says otherwise; `unrecorded` when no begin was retained. */
  state: "working" | "committed" | "aborted" | "unrecorded";
  /** Whether the record left it open. The page never closes a group on the record's behalf. */
  open: boolean;
  /** Whether the begin row is in this window. */
  beginsHere: boolean;
  /** Whether the end row is in this window. */
  endsHere: boolean;
  startedAtMs: number;
  endedAtMs: number | null;
  /** The goal in effect where the transaction began, when a row carried it. */
  goalAtStart: { text: string } | null;
  /** The rows the device observed under it, oldest first. */
  calls: SpineRow[];
  /** The device's receipt, when the end row carried one. */
  receipt: TxnReceipt | null;
};

/** A divider where the record changed its goal. */
export type SpineDivider = {
  id: string;
  atMs: number;
  /** The goal now in effect; `null` when the record cleared it. */
  goal: { text: string } | null;
  /** Who wrote that goal: a divider carries its author too. */
  author: Actor;
};

/** One entry of the document, in reading order. */
export type SpineEntry =
  | { kind: "txn"; group: SpineGroup }
  | { kind: "goal"; divider: SpineDivider }
  | { kind: "row"; row: SpineRow; neutral: boolean };

/** The document, and what the page may say about it without inventing a total. */
export type SessionSpine = {
  /** Oldest first, as the record wrote it. */
  entries: SpineEntry[];
  groups: SpineGroup[];
  dividers: SpineDivider[];
  /** The goal in effect at the end of this window, or `null` when the record leaves none. */
  goalNow: { text: string } | null;
  counts: {
    /** Rows given to the spine. */
    rows: number;
    /** Call rows in the window (not boundaries, not unknown rows). */
    calls: number;
    groups: number;
    /** Rows this build could not classify. Visible, and counted here. */
    unknown: number;
    /** Groups the record left open. */
    open: number;
  };
};

/** What a group's header says, and whose claim it is. */
export type GroupHeader = {
  purpose: string;
  /**
   * Who declared it, or `null` when no begin row is in this window. An orphan group has no
   * declaration to attribute: the first call's actor is not the author of a purpose the record
   * did not retain, so the page prints no author rather than borrowing one.
   */
  author: string | null;
  authorKind: Actor["kind"] | null;
  owner: "you" | "client" | "session" | "unattributed";
  /** The badge says `claim` only when an author is retained; otherwise it says so. */
  badge: "claim" | "unattributed";
  /** Whose claim it is, in words a tooltip can use. Never "the operator's instruction". */
  claim: string;
  state: SpineGroup["state"];
  open: boolean;
};

/** The classes the console and the device publish today. Anything else is unknown. */
const KNOWN_ROW_KINDS = new Set([
  "session.opened",
  "session.closed",
  "goal.set",
  "goal.cleared",
  "txn.started",
  "txn.committed",
  "txn.aborted",
  "call",
  "call.denied",
  "call.started",
  "call.finished"
]);

/** The outcomes the spine can place. Anything else is unknown. */
const KNOWN_OUTCOMES = new Set<string>([
  "working",
  "ok",
  "failed",
  "refused",
  "committed",
  "aborted",
  "session"
]);

type RowClass = "begin" | "end" | "goal" | "call" | "unknown";

/**
 * Whether the record says the row failed or was refused.
 *
 * A refused `txn.begin` still carries `purpose` (the console copies the row's fields from the
 * arguments before it learns the credential lacks the tool) and has no receipt and no row kind.
 * Its class comes from the failure, not the field it happens to carry: an attempt the machine
 * never ran must not draw a group that opened and is working.
 */
function failedOrRefused(row: SpineRow): boolean {
  return row.outcome === "failed" || row.outcome === "refused" || row.tone === "failed";
}

/**
 * Whether the row is a session boundary.
 *
 * The counts' `calls` says "not boundaries": a session opened or closed is a row the rail draws,
 * but it is not a call under a transaction, and counting it would make the window's call count
 * larger than the calls it holds.
 */
function isBoundary(row: SpineRow): boolean {
  return (
    row.rowKind === "session.opened" ||
    row.rowKind === "session.closed" ||
    row.outcome === "session"
  );
}

/**
 * What one row is, from its own fields.
 *
 * The published row kind wins when there is one. Older console rows carry no kind, so the fields
 * they do carry answer instead: a receipt (or a declared ending) closes a group, a failure or
 * refusal is a call that did not happen, a purpose opens a group, a goal is a divider, and a plain
 * outcome is a call. Failure is read **before** purpose, because a refused begin's evidence still
 * carries the purpose it would have declared.
 */
function classify(row: SpineRow): RowClass {
  const kind = row.rowKind;
  if (kind === "txn.started") return "begin";
  if (kind === "txn.committed" || kind === "txn.aborted") return "end";
  if (kind === "goal.set" || kind === "goal.cleared") {
    return row.goal !== undefined ? "goal" : "unknown";
  }
  if (kind === "call" || kind === "call.denied" || kind === "call.started" || kind === "call.finished") {
    return row.outcome === "unknown" ? "unknown" : "call";
  }
  if (kind === "session.opened" || kind === "session.closed") return "call";
  if (kind !== undefined) return KNOWN_ROW_KINDS.has(kind) ? "call" : "unknown";

  if (row.receipt != null || row.outcome === "committed" || row.outcome === "aborted") {
    return "end";
  }
  // Before purpose: a call the machine refused carries the purpose it never got to declare.
  if (failedOrRefused(row)) return "call";
  if (row.purpose != null) return "begin";
  if (row.goal !== undefined) return "goal";
  if (row.outcome !== undefined && KNOWN_OUTCOMES.has(row.outcome)) return "call";
  return "unknown";
}

/** The sentence a call's outcome earns, in the record's vocabulary. */
function stateOfEnd(row: SpineRow): SpineGroup["state"] {
  if (row.rowKind === "txn.aborted" || row.outcome === "aborted") return "aborted";
  if (row.rowKind === "txn.committed" || row.outcome === "committed") return "committed";
  const state = row.receipt?.state;
  if (state === "aborted" || state === "committed" || state === "working") return state;
  return "unrecorded";
}

/** The transaction a row names, from its own field or its receipt. */
function txnOf(row: SpineRow): string | null {
  if (typeof row.txn === "string" && row.txn !== "") return row.txn;
  const receipt = row.receipt?.txn;
  return typeof receipt === "string" && receipt !== "" ? receipt : null;
}

/** `1 call`, `2 calls`. */
function callCount(count: number): string {
  return `${count} ${count === 1 ? "call" : "calls"}`;
}

/**
 * Build the session's document from the rows it has.
 *
 * Nothing is manufactured: a transaction whose begin row is outside the window is an orphan group
 * that says so; a group with no end row is open and says no end was recorded; a receipt's totals
 * are printed beside the window's own count, never instead of it.
 */
export function buildSessionSpine(rows: readonly SpineRow[]): SessionSpine {
  const ordered = rows
    .map((row, index) => ({ row, index }))
    .sort((left, right) => left.row.atMs - right.row.atMs || left.index - right.index)
    .map((entry) => entry.row);

  const entries: SpineEntry[] = [];
  const groups: SpineGroup[] = [];
  const dividers: SpineDivider[] = [];
  const byTxn = new Map<string, SpineGroup>();
  let goalNow: { text: string } | null = null;
  let goalAuthor: Actor | null = null;
  let goalKnown = false;
  let unknown = 0;
  let calls = 0;

  /** A group for a txn that has no begin in this window. It is still the record's group. */
  const orphan = (txn: string, author: Actor, atMs: number, purpose: string | null): SpineGroup => {
    const group: SpineGroup = {
      txn,
      purpose,
      author,
      state: "unrecorded",
      open: false,
      beginsHere: false,
      endsHere: false,
      startedAtMs: atMs,
      endedAtMs: null,
      goalAtStart: null,
      calls: [],
      receipt: null
    };
    groups.push(group);
    byTxn.set(txn, group);
    entries.push({ kind: "txn", group });
    return group;
  };

  for (const row of ordered) {
    const rowClass = classify(row);
    const txn = txnOf(row);
    switch (rowClass) {
      case "goal": {
        const next = row.goal ?? null;
        // A divider is drawn only where the record changed: re-writing the same sentence by the
        // same author is not a new goal, and a clear when nothing is set changes nothing.
        const changed =
          !goalKnown ||
          (goalNow?.text ?? null) !== (next?.text ?? null) ||
          (goalAuthor?.label ?? null) !== row.actor.label;
        goalKnown = true;
        goalNow = next;
        goalAuthor = row.actor;
        if (changed) {
          const divider: SpineDivider = {
            id: row.id,
            atMs: row.atMs,
            goal: next,
            author: row.actor
          };
          dividers.push(divider);
          entries.push({ kind: "goal", divider });
        }
        break;
      }
      case "begin": {
        if (txn === null) {
          unknown += 1;
          entries.push({ kind: "row", row, neutral: true });
          break;
        }
        // A window can open between a transaction's first call and its begin row. The group the
        // call already created is the same record, so the begin adopts it rather than making a
        // second group for one transaction.
        const adopted = byTxn.get(txn);
        if (adopted !== undefined && !adopted.beginsHere) {
          adopted.beginsHere = true;
          adopted.startedAtMs = row.atMs;
          adopted.purpose = row.purpose ?? row.receipt?.purpose ?? adopted.purpose;
          adopted.author = row.actor;
          adopted.goalAtStart = row.goal ?? adopted.goalAtStart;
          adopted.open = !adopted.endsHere;
          if (!adopted.endsHere) adopted.state = "working";
          break;
        }
        const group: SpineGroup = {
          txn,
          purpose: row.purpose ?? row.receipt?.purpose ?? null,
          author: row.actor,
          state: "working",
          open: true,
          beginsHere: true,
          endsHere: false,
          startedAtMs: row.atMs,
          endedAtMs: null,
          goalAtStart: row.goal ?? null,
          calls: [],
          receipt: null
        };
        groups.push(group);
        byTxn.set(txn, group);
        entries.push({ kind: "txn", group });
        break;
      }
      case "end": {
        if (txn === null) {
          unknown += 1;
          entries.push({ kind: "row", row, neutral: true });
          break;
        }
        const existing = byTxn.get(txn);
        const group =
          existing ??
          orphan(txn, row.actor, row.atMs, row.receipt?.purpose ?? row.purpose ?? null);
        group.state = stateOfEnd(row);
        group.open = false;
        group.endsHere = true;
        group.endedAtMs = row.atMs;
        group.receipt = row.receipt ?? null;
        group.purpose ??= row.receipt?.purpose ?? row.purpose ?? null;
        break;
      }
      case "call": {
        // A session boundary is drawn as a row but is not a call: `counts.calls` says so.
        if (!isBoundary(row)) calls += 1;
        if (txn !== null) {
          let group = byTxn.get(txn);
          if (group === undefined) group = orphan(txn, row.actor, row.atMs, null);
          group.calls.push(row);
        } else {
          entries.push({ kind: "row", row, neutral: false });
        }
        break;
      }
      case "unknown":
      default: {
        unknown += 1;
        if (txn !== null) {
          let group = byTxn.get(txn);
          if (group === undefined) group = orphan(txn, row.actor, row.atMs, null);
          // Visible where it belongs, and counted: no meaning is assigned to it.
          group.calls.push(row);
        } else {
          entries.push({ kind: "row", row, neutral: true });
        }
        break;
      }
    }
  }

  return {
    entries,
    groups,
    dividers,
    goalNow,
    counts: {
      rows: rows.length,
      calls,
      groups: groups.length,
      unknown,
      open: groups.filter((group) => group.open).length
    }
  };
}

/** The claim badge's owner, from the author's own kind. */
function ownerOf(author: Actor): GroupHeader["owner"] {
  if (author.kind === "you") return "you";
  if (author.kind === "client") return "client";
  if (author.kind === "session") return "session";
  return "unattributed";
}

/** Whether the spine cannot place this row, so a surface draws it neutrally. */
export function isUnknownRow(row: SpineRow): boolean {
  return classify(row) === "unknown";
}

/**
 * What a group's header says.
 *
 * The purpose is the caller's claim and is printed with its author. The badge is `claim` only when
 * the begin row — and so the author — is in this window. An orphan group's first call is not its
 * declaration: the actor who happened to make that call is never printed as "by" the purpose, and
 * the badge says no author was retained. Nothing here may call the purpose the operator's
 * instruction: a client's sentence is the client's.
 */
export function groupHeader(group: SpineGroup): GroupHeader {
  if (!group.beginsHere) {
    return {
      purpose: group.purpose ?? "Purpose not retained in this window",
      author: null,
      authorKind: null,
      owner: "unattributed",
      badge: "unattributed",
      claim: "no author retained in this window",
      state: group.state,
      open: group.open
    };
  }
  const owner = ownerOf(group.author);
  const claim =
    owner === "you"
      ? "the operator's claim"
      : owner === "client"
        ? "the client's claim"
        : owner === "session"
          ? "the session's claim"
          : "an unattributed claim";
  return {
    purpose: group.purpose ?? "Purpose not retained in this window",
    author: group.author.label,
    authorKind: group.author.kind,
    owner,
    badge: "claim",
    claim,
    state: group.state,
    open: group.open
  };
}

/** How long the device observed the transaction running, in words. */
function durationWords(ms: number): string | null {
  if (!Number.isFinite(ms) || ms < 0) return null;
  const seconds = Math.round(ms / 1_000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  if (minutes < 60) return rest === 0 ? `${minutes}m` : `${minutes}m ${rest}s`;
  const hours = Math.floor(minutes / 60);
  const restMinutes = minutes % 60;
  return restMinutes === 0 ? `${hours}h` : `${hours}h ${restMinutes}m`;
}

/**
 * The sentence under a group: what this window holds, and what the device said about the whole
 * transaction.
 *
 * The two figures are never added or averaged. A window with two of the receipt's nine calls says
 * "2 calls in this window · receipt: 9 calls", because the page can count only what it can see and
 * the device's totals are the device's. An open group says **no end recorded** and is never closed
 * here; a group whose receipt was not retained says so instead of implying a total.
 */
export function groupEnd(group: SpineGroup): { open: boolean; sentence: string } {
  const calls = group.calls.filter((row) => classify(row) === "call" && !isBoundary(row)).length;
  const inWindow = `${callCount(calls)} in this window`;
  if (group.open) {
    return { open: true, sentence: `${inWindow} · no end recorded` };
  }
  if (group.state === "unrecorded" && group.receipt === null) {
    return { open: false, sentence: `${inWindow} · receipt not retained` };
  }
  const observed = group.receipt?.observed;
  if (observed === undefined || typeof observed.calls !== "number") {
    return {
      open: false,
      sentence: `${inWindow} · ${group.state} · receipt not retained`
    };
  }
  const parts = [
    inWindow,
    group.state,
    `receipt: ${callCount(observed.calls)}`
  ];
  if (typeof observed.failures === "number") {
    parts.push(`${observed.failures} ${observed.failures === 1 ? "failure" : "failures"}`);
  }
  if (typeof observed.refusals === "number") {
    parts.push(`${observed.refusals} refused`);
  }
  const duration =
    typeof observed.duration_ms === "number" ? durationWords(observed.duration_ms) : null;
  if (duration !== null) parts.push(duration);
  return { open: false, sentence: parts.join(" · ") };
}
