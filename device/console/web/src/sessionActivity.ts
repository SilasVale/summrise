/**
 * One session's slice of the console's live activity.
 *
 * The console's live feed is machine-wide: a `/events` snapshot is the newest lines for every
 * machine. A running call is announced with the same bounded arguments as its finished row — the
 * proxy's `started` frame carries `evidence` — so a running call can be scoped by
 * `evidence.session` too. A finished call joins a session the same way; the machine's own
 * `session.opened`/`session.closed` rows carry their session in `session`. A call that names no
 * session, including every snapshot row read back from the audit file (which keeps no session or
 * evidence), is **withheld and counted**, never attributed to the active tab, because the record
 * cannot prove it belongs there.
 *
 * The feed is capped at the page's live window and keeps no per-session history. This module draws
 * only the lines it is given: an absent older call is absent, not invented. The rail's footer says
 * so; the data layer must not quietly fill the gap.
 *
 * A row is a fact, not a progress bar. The console never measures a running call's percentage or
 * output, so the only running fact is that it is running; a finished call's rows come from its
 * bounded evidence and the sentence the console wrote.
 */
import type { LiveLine, StartedCall, TxnReceipt } from "./liveActivity";
import type { SpineOutcome } from "./sessionSpine";
import { typedText, WRITE_BURST_MS } from "./terminalInput";
import { tokenLabel } from "./tokenFact";

/**
 * Who a row belongs to, in the record's own terms.
 *
 * `caller` is the console's fact — `console` or a client — and it is not a claim about a person or
 * a model. A token names the credential a client presented; it is the best label the page has, and
 * the tooltip says what it is not. `Session` is the machine's own session opcode, not a call.
 */
export type Actor = {
  label: string;
  kind: "you" | "client" | "session" | "unknown";
  title: string;
};

/** One row the rail can draw; `facts` are the record's own strings, never a sentence invented here. */
export type SessionRow = {
  /** Stable within one list; a coalesced typed line keeps the id of its first write. */
  id: string;
  /** The console's clock for the newest fact in the row. */
  atMs: number;
  actor: Actor;
  tone: "running" | "ok" | "failed" | "session";
  title: string;
  facts: string[];
  /** The text to search the active xterm for; only a finished write with typed input has one. */
  jump?: string;
  /**
   * The record's own class of the row, when it published one. The spine reads it first; the fields
   * below are the fallback an older console row carries.
   */
  rowKind?: string;
  /** The transaction the row belongs to, from the row itself. */
  txn?: string | null;
  /** The transaction's declared purpose, on a begin row. */
  purpose?: string | null;
  /** A goal this row records: the sentence, or `null` when it cleared the goal. */
  goal?: { text: string } | null;
  /** The device's receipt, on a transaction's end row. */
  receipt?: TxnReceipt | null;
  /** What the row says happened, in the spine's closed vocabulary. */
  outcome: SpineOutcome;
};

const YOU: Actor = {
  label: "You",
  kind: "you",
  title: "This console made the call on your operator session."
};

const SESSION_ACTOR: Actor = {
  label: "Session",
  kind: "session",
  title: "The machine's own session event; it is not a call made by you or a client."
};

const UNATTRIBUTED: Actor = {
  label: "Unattributed",
  kind: "unknown",
  title: "The record names no caller, so this row is attributed to neither you nor a client."
};

/** A client whose token the page does not know, or a client with no token record at all. */
function clientActor(label: string | null): Actor {
  return {
    label: label ?? "Client",
    kind: "client",
    title:
      label === null
        ? "A client made this call with a signed grant; the record names no token, so it is not proof of a person or a model."
        : `A client made this call with the token ${label}; a token is not proof of a person or a model.`
  };
}

/**
 * The session a row names: its own `session` field first (a session event's payload is the
 * authority), then the call's `evidence.session`.
 *
 * An empty string is missing, not a session named `""`: the console writes no empty id, and a row
 * that carried one must be withheld like any other sessionless row.
 */
export function sessionOf(v: {
  session?: string;
  evidence?: Record<string, unknown>;
}): string | null {
  if (typeof v.session === "string" && v.session !== "") return v.session;
  const evidence = v.evidence;
  if (!evidence || typeof evidence !== "object" || Array.isArray(evidence)) return null;
  const session = evidence.session;
  return typeof session === "string" && session !== "" ? session : null;
}

/**
 * The actor a call's record proves.
 *
 * `caller: "console"` is the operator's own page, so the row says **You**; `caller: "client"` is a
 * client, named by its token when the page knows it and its short id when it does not. Absent means
 * the record cannot say, and the row says **Unattributed** rather than guessing a side. No label or
 * tooltip claims an AI: a token is not proof of a person or a model.
 */
export function actorOf(
  line: Pick<LiveLine, "caller" | "token_id">,
  names: ReadonlyMap<string, string>
): Actor {
  if (line.caller === "console") return YOU;
  if (line.caller === "client") {
    return clientActor(line.token_id ? tokenLabel(line.token_id, names) : null);
  }
  return UNATTRIBUTED;
}

/** Whether a line is a call, which is what a sessionless row is counted as. */
function isCall(line: LiveLine): boolean {
  return line.kind === "mcp" || line.kind === "poll" || line.tool !== undefined;
}

/** A non-empty string in one call's bounded evidence. */
function evidenceText(
  evidence: Record<string, unknown> | undefined,
  key: string
): string | null {
  const value = evidence?.[key];
  return typeof value === "string" && value !== "" ? value : null;
}

/** The string arguments in one call's bounded evidence. */
function evidenceList(
  evidence: Record<string, unknown> | undefined,
  key: string
): string[] {
  const value = evidence?.[key];
  return Array.isArray(value)
    ? value.filter((entry): entry is string => typeof entry === "string")
    : [];
}

/** `npm test -- --run`, as the caller wrote it; null when the evidence names no command. */
function commandLine(evidence: Record<string, unknown> | undefined): string | null {
  const command = evidenceText(evidence, "command");
  if (command === null) return null;
  const args = evidenceList(evidence, "args");
  return [command, ...args].join(" ");
}

/** What a person typed in one write, or null when the evidence carries no keystroke. */
function typedInputOf(evidence: Record<string, unknown> | undefined): string | null {
  const input = evidence?.input;
  if (typeof input !== "string") return null;
  // The console's excerpt trims each write, so a space keystroke arrives as the empty string. It is
  // still text a person typed (a word separator), not a missing keystroke; the caller decides
  // whether a line made only of spaces is worth a card.
  return input === "" ? " " : typedText(input);
}

/**
 * The title a call earns: what was typed, then the command and its arguments, then a path, then the
 * console's own sentence. The order is the reader's: a typed line is the thing a person recognises,
 * a command is next, and a generic sentence is the fallback rather than a label that hides evidence.
 */
function titleOf(
  line: LiveLine,
  typed: string | null
): string {
  if (typed !== null) return typed;
  const command = commandLine(line.evidence);
  if (command !== null) return command;
  const path = evidenceText(line.evidence, "path");
  if (path !== null) return path;
  return line.sentence;
}

/** The record's own facts for a finished row: the tool, a run's exit, the console's reason, the measured time. */
function factsOf(line: LiveLine): string[] {
  const facts: string[] = [];
  if (typeof line.tool === "string" && line.tool !== "") facts.push(line.tool);
  if (typeof line.evidence?.exit_code === "number") {
    facts.push(`exit ${line.evidence.exit_code}`);
  }
  if (line.evidence?.timed_out === true) facts.push("timed out");
  if (typeof line.detail === "string" && line.detail !== "") facts.push(line.detail);
  if (typeof line.ms === "number") facts.push(`${line.ms} ms`);
  return facts;
}

/** A finished row's tone: the machine's own session event, a failure, or an answer. */
function toneOf(line: LiveLine): SessionRow["tone"] {
  if (line.kind === "session") return "session";
  return line.ok === false || line.status >= 400 ? "failed" : "ok";
}

/**
 * What a row says happened, in the spine's closed vocabulary.
 *
 * The published row kind wins. An older console row carries no kind, so the tool it names is the
 * one fact left: the transaction's own tools answer their declaration, and everything else is the
 * call's own outcome. A kind this build cannot place is `unknown`, which the rail draws neutrally
 * and counts.
 */
export function outcomeOf(line: LiveLine): SpineOutcome {
  if (line.kind === "session") return "session";
  const kind = line.row_kind;
  if (kind !== undefined) {
    switch (kind) {
      case "txn.started":
        return "working";
      case "txn.committed":
        return "committed";
      case "txn.aborted":
        return "aborted";
      case "call.denied":
        return "refused";
      case "goal.set":
      case "goal.cleared":
      case "call":
      case "call.started":
      case "call.finished":
      case "session.opened":
      case "session.closed":
        return line.ok === false || line.status >= 400 ? "failed" : "ok";
      default:
        return "unknown";
    }
  }
  switch (line.tool) {
    case "txn.begin":
      return "working";
    case "txn.commit":
      return "committed";
    case "txn.abort":
      return "aborted";
    default:
      return line.ok === false || line.status >= 400 ? "failed" : "ok";
  }
}

/** The goal a row records, when the row carries one: a set sentence, or `null` for a clear. */
function goalOfLine(line: LiveLine): { text: string } | null | undefined {
  if (line.goal === undefined) return undefined;
  if (line.goal === null) return null;
  return line.goal.text !== "" ? { text: line.goal.text } : undefined;
}

/**
 * The actor identity two writes must share to be one typed line.
 *
 * Only proven identities coalesce: the console has one actor, and a client is its token. A row with
 * no caller, or a client with no token record, has no identity to compare — joining two of them
 * would draw one line for two actors the record cannot tell apart.
 *
 * Exported because the session card's last-input line joins by the same proof. The rail and the card
 * must agree on what "one actor" means, or the same two writes would be one line in one place and
 * two in the other.
 */
export function writeActorKey(line: Pick<LiveLine, "caller" | "token_id">): string | null {
  if (line.caller === "console") return "console";
  if (
    line.caller === "client" &&
    typeof line.token_id === "string" &&
    line.token_id !== ""
  ) {
    return `client\u0000${line.token_id}`;
  }
  return null;
}

/** One row from a finished line, or null when the line is a control-only write with nothing to show. */
function rowFromLine(
  line: LiveLine,
  names: ReadonlyMap<string, string>,
  id: string
): SessionRow | null {
  const typed = typedInputOf(line.evidence);
  // A control-only write is the terminal's own traffic (a focus report, an arrow key), not a
  // command. It keeps its session for scoping but earns no card.
  if (line.tool === "terminal.write" && typed === null) return null;
  const goal = goalOfLine(line);
  return {
    id,
    atMs: line.ts_ms,
    actor: line.kind === "session" ? SESSION_ACTOR : actorOf(line, names),
    tone: toneOf(line),
    title: titleOf(line, typed),
    facts: factsOf(line),
    ...(typed !== null ? { jump: typed } : {}),
    ...(line.row_kind !== undefined ? { rowKind: line.row_kind } : {}),
    ...(typeof line.txn === "string" && line.txn !== "" ? { txn: line.txn } : {}),
    ...(typeof line.purpose === "string" && line.purpose !== ""
      ? { purpose: line.purpose }
      : {}),
    ...(goal !== undefined ? { goal } : {}),
    ...(line.receipt !== undefined ? { receipt: line.receipt } : {}),
    outcome: outcomeOf(line)
  };
}

/**
 * One row from a running call.
 *
 * The proxy publishes only what it already knows: the tool, the caller, the token and the bounded
 * arguments. There is no output and no progress yet, so a running row shows the command it can
 * prove and says running; it gets no jump, because the xterm holds the text only once the write
 * finishes.
 */
function rowFromRunning(call: StartedCall, names: ReadonlyMap<string, string>): SessionRow | null {
  const typed = typedInputOf(call.evidence);
  if (call.tool === "terminal.write" && (typed === null || typed.trim() === "")) return null;
  const title = typed ?? commandLine(call.evidence) ?? evidenceText(call.evidence, "path") ?? call.tool;
  return {
    id: `running:${call.id}`,
    atMs: call.at_ms,
    actor: actorOf(call, names),
    tone: "running",
    title,
    // The title is the tool when the evidence names nothing else; repeating it as a fact would draw
    // the same word twice. The fact earns its place only when the title is something else.
    facts: title === call.tool ? [] : [call.tool],
    ...(typeof call.txn === "string" && call.txn !== "" ? { txn: call.txn } : {}),
    outcome: "working"
  };
}

/**
 * The rows for one active device and session, newest first, and how many sessionless calls on that
 * device were withheld.
 *
 * A line from another machine or another session is not this session's business — it is neither
 * drawn nor counted. A sessionless call on the active machine is a hole the rail cannot place; it
 * is counted so the page can say so, and never attributed. An empty list yields no rows: the
 * console has no per-session history, and this module will not manufacture one.
 */
export function sessionActivity(
  lines: readonly LiveLine[],
  running: readonly StartedCall[],
  deviceId: string,
  session: string,
  names: ReadonlyMap<string, string>
): { rows: SessionRow[]; withheld: number } {
  let withheld = 0;
  const scoped: LiveLine[] = [];
  for (const line of lines) {
    if (line.device_id !== deviceId) continue;
    const owner = sessionOf(line);
    if (owner === null) {
      if (isCall(line)) withheld += 1;
      continue;
    }
    if (owner !== session) continue;
    scoped.push(line);
  }

  type Entry = { row: SessionRow; write: { key: string; atMs: number } | null };
  const entries: Entry[] = [];
  // The newest write card per proven actor: one typed line can be split by the wire, and a read or
  // another actor's row in between must not cut the command into fragments.
  const openWrites = new Map<string, Entry>();
  // A write can reach both the snapshot and the stream; the same instant and text is one keystroke.
  const seenWrites = new Set<string>();
  const ordered = [...scoped].sort((left, right) => left.ts_ms - right.ts_ms);
  for (const line of ordered) {
    const typed = typedInputOf(line.evidence);
    if (line.tool === "terminal.write" && typed === null) {
      // An input-less write is the terminal's own traffic (Enter, an arrow, xterm's focus report).
      // It earns no card, as before, and it ends the actor's open line: without that boundary the
      // 2 s bound would join two commands a person typed separately. A write that proves no actor
      // cannot say whose line it ends, so it ends them all rather than guessing.
      const actor = writeActorKey(line);
      if (actor === null) openWrites.clear();
      else openWrites.delete(actor);
      continue;
    }
    if (typed !== null) {
      const raw = line.evidence?.input;
      const key = `${line.ts_ms}\u0000${line.tool ?? ""}\u0000${line.caller ?? ""}\u0000${
        line.token_id ?? ""
      }\u0000${typeof raw === "string" ? raw : ""}`;
      if (seenWrites.has(key)) continue;
      seenWrites.add(key);
    }
    const id = `line:${line.ts_ms}:${entries.length}`;
    const row = rowFromLine(line, names, id);
    if (row === null) continue;
    const key = typed !== null ? writeActorKey(line) : null;
    const previous = key !== null ? openWrites.get(key) : undefined;
    if (
      key !== null &&
      typed !== null &&
      previous !== undefined &&
      previous.write !== null &&
      line.ts_ms - previous.write.atMs <= WRITE_BURST_MS
    ) {
      // One typed line split by the wire: join the pieces rather than drawing a fragment per write.
      // The card keeps its first id and takes the newest moment, so it does not remount per key.
      previous.row.atMs = line.ts_ms;
      previous.row.title += typed;
      previous.row.jump = `${previous.row.jump ?? ""}${typed}`;
      previous.write.atMs = line.ts_ms;
      continue;
    }
    const entry: Entry = {
      row,
      write: key !== null ? { key, atMs: line.ts_ms } : null
    };
    entries.push(entry);
    if (key !== null) openWrites.set(key, entry);
  }

  for (const call of running) {
    if (call.device_id !== deviceId) continue;
    const owner = sessionOf(call);
    if (owner === null) {
      withheld += 1;
      continue;
    }
    if (owner !== session) continue;
    const row = rowFromRunning(call, names);
    if (row !== null) entries.push({ row, write: null });
  }

  // A line made only of spaces has no command to draw. It was a real keystroke for coalescing, but a
  // card for it would be a blank.
  const visible = entries.filter(
    (entry) => entry.write === null || entry.row.title.trim() !== ""
  );
  visible.sort((left, right) => right.row.atMs - left.row.atMs);
  return { rows: visible.map((entry) => entry.row), withheld };
}

/**
 * A door as `terminal.list` names it: the holder that opened the session, or wrote its goal.
 *
 * It is attribution, not identity. The device writes `{kind, token_id}`; this page prints a label
 * and never treats the label as a person or a model.
 */
export type SessionHolder = {
  kind?: string;
  token_id?: string | null;
};

/** A session's goal, as `terminal.list` answers it. */
export type SessionGoal = {
  text: string;
  by?: SessionHolder;
  at_ms?: number;
};

/**
 * One row of `terminal.list`, reduced to the fields the session card reads.
 *
 * `goal` is optional **and the key's presence is the fact**: an absent key is a device that does not
 * answer a goal, an explicit `null` is a device that says the session has none. The page must not
 * turn one into the other.
 */
export type SessionListEntry = {
  id: string;
  kind: string;
  started_at_ms?: number;
  holder?: SessionHolder;
  goal?: SessionGoal | null;
};

/**
 * A door in words a person reads beside a goal, never proof of a person or a model.
 *
 * `names` is the operator's own naming of token ids, so a client's goal is attributed to the token
 * the operator knows rather than to a guess.
 */
export function holderLabel(
  holder: SessionHolder | undefined,
  names: ReadonlyMap<string, string>
): string {
  if (!holder || typeof holder.kind !== "string" || holder.kind === "") {
    return "an unrecorded author";
  }
  const id = typeof holder.token_id === "string" && holder.token_id !== "" ? holder.token_id : null;
  switch (holder.kind) {
    case "console":
      // The console's own operator session is the one caller the page can name as the reader.
      return id === null || id === "operator" ? "you" : tokenLabel(id, names);
    case "token":
      return id === null ? "a client" : tokenLabel(id, names);
    case "install":
      return "the install door";
    case "panel":
      return "the panel";
    case "grant":
      return "a grant";
    case "mesh":
      return "a paired machine";
    default:
      return "an unrecorded author";
  }
}
