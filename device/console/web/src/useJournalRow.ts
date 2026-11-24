import { useEffect, useState } from "react";
import { api, type MachineReadAnswer, type MachineRow } from "./api";
import { readFailureSentence } from "./joinMarks";

/**
 * **READING ONE ROW BACK FROM THE MACHINE (round 9).**
 *
 * A proxied call's audit line now names the machine's own sequence for its outcome half
 * (`machine_seq`). That is a pointer: `journal.read` with `before_seq = seq + 1, limit = 1` answers
 * exactly the row at that position, or nothing when the journal has rotated past it. The opened
 * record uses it to show the machine's own copy of the call, not only the console's.
 *
 * Three facts stay apart, as everywhere else in this round:
 *
 * 1. **Empty is not missing.** A `journal.read` window with no row at that sequence means the row
 *    rotated away; the call still happened. The sentence says so.
 * 2. **`unknown` is not empty.** An older machine that does not offer the read did not say the row is
 *    gone; it said it cannot answer.
 * 3. **A failed read is not a claim.** The console's own sentence is shown instead.
 */

export type JournalRowState = {
  /** `idle` when no device/sequence was asked for; `reading` while the one small read is out. */
  state: "idle" | "reading" | "read" | "failed";
  /** The machine's row at that sequence, when the answer holds it. */
  row: MachineRow | null;
  answer: MachineReadAnswer | null;
  /** The console's sentence for a failed read, or null. */
  error: string | null;
};

const NOTHING: JournalRowState = { state: "idle", row: null, answer: null, error: null };

/**
 * How long a failed read is remembered before opening the record asks again.
 *
 * A success needs no expiry: a sequence names one row forever, and a row the journal rotated stays
 * rotated. A failure is about the moment — the machine was offline, the console restarted — so the
 * page must be able to ask again without a reload.
 */
export const JOURNAL_ROW_RETRY_MS = 30_000;

/** Per `(device, seq)`, for the life of the page: a retained row cannot change under its sequence. */
const cache = new Map<string, JournalRowState>();
/** When each failed read was recorded, so a failure can age out while a success cannot. */
const failedAt = new Map<string, number>();
const pending = new Map<string, Promise<void>>();

/** Test seam: this is module state, and a test must start from a clean console. */
export function resetJournalRows() {
  cache.clear();
  failedAt.clear();
  pending.clear();
}

function keyOf(deviceId: string, seq: number, trace: string | null): string {
  // **THE TRACE IS PART OF THE QUESTION.** The bus has no per-journal epoch yet (it is target
  // design), so a reset can make an old audit line's `seq` name a different call's row. A row is only
  // this row when the sequence *and* the trace agree; the key keeps a cached answer from one call
  // from answering another that happens to carry the same sequence.
  return `${deviceId}\u0000${seq}\u0000${trace ?? ""}`;
}

/**
 * Read one machine row back by its sequence.
 *
 * The read is one row wide and happens only when a record is opened, so a page that lists a thousand
 * calls costs nothing until the operator asks about one. The answer is remembered: a sequence names
 * one row forever, so a repeated open is not a second request.
 */
export function useJournalRow(
  deviceId: string | null,
  seq: number | null,
  trace: string | null = null
): JournalRowState {
  const [version, setVersion] = useState(0);
  const key = deviceId === null || seq === null ? null : keyOf(deviceId, seq, trace);

  useEffect(() => {
    if (deviceId === null || seq === null || key === null) return;
    // **A READ ALREADY OUT IS WAITED ON, NOT REPEATED.** The cache says `reading` as soon as the
    // request starts, so a remount must find the promise before the cache's own early return;
    // otherwise it would show "Reading…" forever.
    const inFlight = pending.get(key);
    if (inFlight !== undefined) {
      let live = true;
      void inFlight.then(() => {
        if (live) setVersion((value) => value + 1);
      });
      return () => {
        live = false;
      };
    }
    const cached = cache.get(key);
    if (cached !== undefined) {
      const failed = failedAt.get(key);
      // A failure is remembered for a moment, not forever: the machine may be back by the next open.
      if (cached.state !== "failed" || failed === undefined || Date.now() - failed < JOURNAL_ROW_RETRY_MS) {
        return;
      }
      cache.delete(key);
      failedAt.delete(key);
    }
    cache.set(key, { state: "reading", row: null, answer: null, error: null });
    setVersion((value) => value + 1);
    const read = api
      .journalRead(deviceId, { before: seq + 1, limit: 1 })
      .then((answer: MachineReadAnswer) => {
        // The row must be the one asked for: a journal that rotated — or was reset, where a sequence
        // can name another call — must not answer with a different call's row.
        const row =
          answer.unknown || !answer.journal
            ? null
            : (answer.rows.find(
                (entry) =>
                  entry.seq === seq &&
                  (trace === null ||
                    (typeof entry.trace === "string" && entry.trace === trace))
              ) ?? null);
        cache.set(key, { state: "read", row, answer, error: null });
        setVersion((value) => value + 1);
      })
      .catch((cause: unknown) => {
        cache.set(key, {
          state: "failed",
          row: null,
          answer: null,
          error: readFailureSentence(cause)
        });
        failedAt.set(key, Date.now());
        setVersion((value) => value + 1);
      })
      .finally(() => {
        pending.delete(key);
      });
    pending.set(key, read);
    // `version` is how the answer reaches this render; `key` is the question's own identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deviceId, seq, version]);

  if (key === null) return NOTHING;
  return cache.get(key) ?? NOTHING;
}

/**
 * The opened record's sentence for one read-back, or null while it is still being read.
 *
 * It states the machine's own facts: the row's kind, its tool and outcome, the door it arrived by,
 * and how large its payload was — the same summary `journal.read` gives, never the payload.
 */
export function journalRowSentence(state: JournalRowState, seq: number): string | null {
  if (state.state === "reading" || state.state === "idle") return null;
  if (state.state === "failed") {
    const reason = (state.error ?? "the console could not ask").replace(/\.$/, "");
    return `The machine's own row ${seq} could not be read: ${reason}.`;
  }
  if (state.answer?.unknown === true) {
    return `The machine's own row ${seq} could not be read: this machine does not offer its journal.`;
  }
  if (state.answer !== null && !state.answer.journal) {
    return `The machine's own row ${seq} could not be read: this machine keeps no journal.`;
  }
  if (state.row === null) {
    return `The machine no longer holds this call at row ${seq}; its journal has rotated past it or been reset.`;
  }
  const row = state.row;
  // The machine's own clock for the row, as it wrote it: the console's ts is when the exchange
  // finished, and the two are the point of reading the machine's copy at all.
  const facts: string[] = [
    typeof row.ts === "string" && row.ts !== "" ? `${row.type} at ${row.ts}` : row.type
  ];
  if (typeof row.tool === "string" && row.tool !== "") {
    const outcome =
      row.ok === true ? "ok" : row.ok === false ? `refused${row.reason ? `: ${row.reason}` : ""}` : null;
    facts.push(outcome === null ? row.tool : `${row.tool} ${outcome}`);
  }
  if (typeof row.door === "string" && row.door !== "") facts.push(`arrived by ${row.door}`);
  if (typeof row.payload_bytes === "number") facts.push(`payload ${row.payload_bytes} B`);
  return `The machine's own row ${seq}: ${facts.join(", ")}.`;
}
