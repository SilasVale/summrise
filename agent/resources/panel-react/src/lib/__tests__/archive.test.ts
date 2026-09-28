// lib/archive — the pure half of the session archive.
//
// The page tests (components/__tests__/ArchivePage.test.tsx) cover what an
// operator sees; this file covers the decisions that page is not allowed to make
// up. Three of them are the panel's documented discipline and each has a real
// incident behind it:
//
//   * a response the panel does not UNDERSTAND is not an empty archive — it
//     throws, because "0 sessions" is a claim about the device (the round-113
//     class of bug: a failed read tombstoning things that were fine);
//   * an absent field is ABSENCE — never 0, never a stand-in word;
//   * the order is DERIVED from the last recorded event's stamp, because
//     `list_sessions` walks the directory and directory order is neither
//     chronological nor stable.
//
// AND THE FIRST DESCRIBE BLOCK NOW TESTS RUST (P2, 2026-09-28). `archiveEntries` moved to
// `agent/resources/panel-logic/src/archive.rs`, and these assertions were NOT rewritten to suit it:
// every expected value and every `toThrow` below is the one that was written against the
// TypeScript, and the only edit is `await` — the call is a promise now because the wasm is fetched
// at the first call rather than at page load. That is criterion ① of the migration plan: the tests
// that were the JS version's evidence are the Rust version's evidence too, including the sentences
// a bad body produces. The corpus behind them (121 payload shapes, both implementations, compared
// on values AND messages) is in the commit message.
import { describe, it, expect } from "vitest";
import {
  ARCHIVE_PAGE,
  archiveClock,
  archiveEntries,
  lastEventWords,
  newestFirst,
  pageOf,
  type ArchiveEntry,
} from "../archive";

const entry = (
  sid: string,
  ts: number | null,
  extra: Partial<NonNullable<ArchiveEntry["last"]>> = {},
): ArchiveEntry => ({
  sid,
  // Identity is absent by default here: the fixtures are about ordering and
  // wording, and a record written before the device kept identity is the case
  // those tests must keep working for.
  identity: null,
  last: ts == null && Object.keys(extra).length === 0
    ? null
    : { kind: "status", ts, status: "closed", exitCode: null, reason: null, ...extra },
});

describe("archiveEntries — reading the device's manifest", () => {
  it("maps id + the folded last event", async () => {
    const rows = await archiveEntries({
      ok: true,
      sessions: [
        { id: "s-1", state: { kind: "command/end", ts: 1700, exit_code: 0, reason: null, status: null } },
      ],
    });
    expect(rows).toHaveLength(1);
    expect(rows[0].sid).toBe("s-1");
    expect(rows[0].last).toEqual({ kind: "command/end", ts: 1700, status: null, exitCode: 0, reason: null });
  });

  it("THROWS on a response it does not understand, instead of reporting an empty device", async () => {
    // An agent that is too old, a proxy that answered HTML, a 200 shell: none of
    // these is "this device has recorded no sessions".
    await expect(archiveEntries(null)).rejects.toThrow();
    await expect(archiveEntries("<html>")).rejects.toThrow();
    await expect(archiveEntries({ ok: false, error: "boom" })).rejects.toThrow();
    await expect(archiveEntries({ ok: true })).rejects.toThrow();
  });

  it("accepts a genuinely empty manifest as empty — the two are different answers", async () => {
    expect(await archiveEntries({ ok: true, sessions: [] })).toEqual([]);
  });

  it("drops a row it cannot NAME rather than rendering it under a fabricated id", async () => {
    const rows = await archiveEntries({
      ok: true,
      sessions: [
        { state: { kind: "status", ts: 1 } },               // no id
        { id: "   ", state: { kind: "status", ts: 2 } },     // blank id
        { id: 42, state: { kind: "status", ts: 3 } },        // not a string
        { id: "kept", state: { kind: "status", ts: 4, status: "closed" } },
      ],
    });
    expect(rows.map((r) => r.sid)).toEqual(["kept"]);
  });

  it("keeps a session whose folded state the device did not write, with no state invented", async () => {
    const rows = await archiveEntries({ ok: true, sessions: [{ id: "bare" }, { id: "empty-state", state: {} }] });
    expect(rows.map((r) => r.last)).toEqual([null, null]);
  });

  it("rejects a stamp or exit code of the wrong type instead of coercing it", async () => {
    const [row] = await archiveEntries({
      ok: true,
      sessions: [{ id: "s", state: { kind: "command/end", ts: "1700", exit_code: "0", reason: 7, status: "" } }],
    });
    expect(row.last).toEqual({ kind: "command/end", ts: null, status: null, exitCode: null, reason: null });
  });
});

describe("newestFirst — the order the route does not give", () => {
  it("sorts by the last recorded event, newest first", () => {
    const out = newestFirst([entry("a", 100), entry("b", 300), entry("c", 200)]);
    expect(out.map((e) => e.sid)).toEqual(["b", "c", "a"]);
  });

  it("is TOTAL and stable — read_dir order is neither, and a list that reshuffles cannot be clicked", () => {
    const ids = (rows: ArchiveEntry[]) => rows.map((e) => e.sid);
    // Same input in a different directory order → the same output order.
    const one = newestFirst([entry("x", 5), entry("y", 5), entry("z", 5)]);
    const two = newestFirst([entry("z", 5), entry("x", 5), entry("y", 5)]);
    expect(ids(one)).toEqual(ids(two));
  });

  it("sorts a session with NO recorded stamp last, rather than guessing when it ran", () => {
    const out = newestFirst([entry("no-stamp", null), entry("stamped", 1)]);
    expect(out.map((e) => e.sid)).toEqual(["stamped", "no-stamp"]);
  });

  it("does not mutate the array it was given", () => {
    const input = [entry("a", 1), entry("b", 2)];
    newestFirst(input);
    expect(input.map((e) => e.sid)).toEqual(["a", "b"]);
  });
});

describe("pageOf — the bounded page", () => {
  it("holds rows back and reports how many", () => {
    const rows = Array.from({ length: 167 }, (_, i) => entry(`s-${i}`, 167 - i));
    const { shown, more } = pageOf(rows, ARCHIVE_PAGE);
    expect(shown).toHaveLength(ARCHIVE_PAGE);
    expect(more).toBe(167 - ARCHIVE_PAGE);
  });

  it("holds nothing back when the list fits — `more: 0`, not a negative count", () => {
    const { shown, more } = pageOf([entry("a", 1)], ARCHIVE_PAGE);
    expect(shown).toHaveLength(1);
    expect(more).toBe(0);
  });

  it("is bounded even when the window is asked for more rows than exist", () => {
    const { shown, more } = pageOf([entry("a", 1)], 10_000);
    expect(shown).toHaveLength(1);
    expect(more).toBe(0);
  });
});

describe("lastEventWords — the log's own vocabulary", () => {
  it("renders a status event VERBATIM (opened / closed / exited:3 are the payload)", () => {
    for (const s of ["opened", "closed", "exited:3", "backgrounded"]) {
      expect(lastEventWords({ kind: "status", ts: 1, status: s, exitCode: null, reason: null })?.label).toBe(s);
    }
  });

  it("keeps exit 0 a VALUE, and 'no exit code recorded' the ABSENCE of one", () => {
    expect(lastEventWords({ kind: "command/end", ts: 1, status: null, exitCode: 0, reason: null })?.label).toBe("exit 0");
    // The absence is not a word in the exit code's place: it is a different
    // sentence, and it must not read as a code of its own.
    const absent = lastEventWords({ kind: "command/end", ts: 1, status: null, exitCode: null, reason: null });
    expect(absent?.label).toBe("command ended");
    expect(absent?.label).not.toContain("exit");
  });

  it("states what the RECORD shows for a command that never ended, not why", () => {
    const w = lastEventWords({ kind: "command/start", ts: 1, status: null, exitCode: null, reason: null });
    expect(w?.label).toContain("no end recorded");
    // "crashed" / "interrupted" would be a cause, which the file does not state.
    expect(w?.label).not.toMatch(/crash|interrupt|kill/i);
  });

  it("says nothing at all when the device folded nothing", () => {
    expect(lastEventWords(null)).toBeNull();
    expect(lastEventWords({ kind: "", ts: null, status: null, exitCode: null, reason: null })).toBeNull();
  });

  it("never invents a status value the device did not write", () => {
    const w = lastEventWords({ kind: "status", ts: 1, status: null, exitCode: null, reason: null });
    expect(w?.label).toBe("status");
  });
});

describe("archiveClock", () => {
  it("draws nothing for an absent stamp", () => {
    expect(archiveClock(null)).toBeNull();
  });

  it("is an ABSOLUTE stamp — never a relative claim about the reader's clock", () => {
    const out = archiveClock(1_700_000_000);
    expect(out).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/);
    expect(out).not.toMatch(/ago|just now|yesterday/i);
  });

  it("draws nothing for a stamp that is not a real time", () => {
    expect(archiveClock(Number.NaN)).toBeNull();
  });
});
