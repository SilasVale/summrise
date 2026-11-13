import { describe, expect, it } from "vitest";
import type { AuditLine, MachineReadAnswer, MachineRow } from "./api";
import {
  EMPTY_MACHINE,
  machineCallRows,
  machineContribution,
  machineCounts,
  machineRecordSentence,
  machineSentence,
  mergeMachineRows
} from "./machineRows";

const device = "d".repeat(64);
const name = "Alpha";

function row(over: Partial<MachineRow> = {}): MachineRow {
  return {
    seq: 10,
    ts: "2026-11-05T09:00:00.000Z",
    type: "call.finished",
    payload_bytes: 42,
    trace: "console-1",
    tool: "files.read",
    ok: true,
    door: "panel",
    ...over
  };
}

function answer(over: Partial<MachineReadAnswer> = {}): MachineReadAnswer {
  return {
    device,
    journal: true,
    oldest_seq: 1,
    last_seq: 10,
    rows: [],
    more: false,
    read_at_ms: 1,
    unknown: false,
    ...over
  };
}

function line(over: Partial<AuditLine> = {}): AuditLine {
  return {
    ts_ms: Date.parse("2026-11-05T09:00:00.000Z"),
    kind: "mcp",
    device_id: device,
    status: 200,
    tool: "files.read",
    ...over
  };
}

describe("machineCallRows", () => {
  it("draws a call's outcome and not its started half, nor a session fact", () => {
    const rows = machineCallRows(
      answer({
        rows: [
          row({ seq: 12, type: "call.finished", tool: "files.list" }),
          row({ seq: 11, type: "call.started", tool: "files.list", ok: undefined }),
          row({ seq: 10, type: "session.opened", tool: undefined, ok: undefined }),
          row({ seq: 9, type: "plugin.loaded", tool: undefined, ok: undefined })
        ]
      }),
      device,
      name
    );
    expect(rows.map((entry) => entry.machine_seq)).toEqual([12]);
  });

  it("reads exactly like the console's row for the same call", () => {
    const [ran] = machineCallRows(answer({ rows: [row({ seq: 8 })] }), device, name);
    expect(ran.kind).toBe("mcp");
    expect(ran.status).toBe(200);
    expect(ran.ok).toBe(true);
    expect(ran.device_id).toBe(device);
    expect(ran.tool).toBe("files.read");
    expect(ran.trace).toBe("console-1");
    expect(ran.sentence).toBe("files.read ran on Alpha");

    const [refused] = machineCallRows(
      answer({
        rows: [
          row({
            seq: 7,
            type: "call.denied",
            ok: false,
            tool: "files.write",
            reason: "denied"
          })
        ]
      }),
      device,
      name
    );
    expect(refused.status).toBe(424);
    expect(refused.ok).toBe(false);
    expect(refused.sentence).toBe("files.write was refused on Alpha (denied)");
  });

  it("carries the door and the machine's own position", () => {
    const [mesh] = machineCallRows(answer({ rows: [row({ door: "mesh" })] }), device, name);
    expect(mesh.door).toBe("mesh");
    expect(mesh.machine_seq).toBe(10);
  });

  it("draws nothing the console's own door wrote, or a row that names no door", () => {
    // A `console` row is a call the console proxied or one of the console's own reads of the machine
    // (the catalogue, the join, this read). The console's record is its authority; the journal is
    // drawn for the doors the console cannot have.
    expect(machineCallRows(answer({ rows: [row({ door: "console" })] }), device, name)).toEqual([]);
    expect(machineCallRows(answer({ rows: [row({ door: undefined })] }), device, name)).toEqual([]);
  });

  it("draws nothing from a read that could not be made, or a machine with no journal", () => {
    expect(machineCallRows(answer({ unknown: true }), device, name)).toEqual([]);
    expect(machineCallRows(answer({ journal: false }), device, name)).toEqual([]);
    expect(machineCallRows(null, device, name)).toEqual([]);
  });
});

describe("mergeMachineRows", () => {
  const proxied = line({
    ts_ms: Date.parse("2026-11-05T09:00:05.000Z"),
    tool: "files.read",
    status: 200,
    trace: "console-1"
  });
  const deviceOnly = line({
    ts_ms: Date.parse("2026-11-05T09:00:03.000Z"),
    tool: "screen.capture",
    status: 200,
    trace: "machine-9",
    machine_seq: 9,
    door: "panel"
  });

  it("keeps a machine row the console's record does not hold", () => {
    const merged = mergeMachineRows([proxied], [deviceOnly]);
    expect(merged.map((entry) => entry.tool)).toEqual(["files.read", "screen.capture"]);
  });

  it("drops the machine's copy of a call the console already drew, by trace", () => {
    const copy = line({
      ts_ms: Date.parse("2026-11-05T09:00:06.000Z"),
      tool: "files.read",
      status: 200,
      trace: "console-1",
      machine_seq: 11
    });
    expect(mergeMachineRows([proxied], [copy])).toEqual([proxied]);
  });

  it("keeps a machine-only call when both rows carry different traces", () => {
    // Two different traces name two different calls, whatever their instants, tools and statuses
    // say: the composite fallback must not drop a real call because an unrelated console row
    // happened to land on the same millisecond.
    const consoleRow = line({ trace: "console-1" });
    const machineCall = line({ trace: "machine-1", machine_seq: 6, door: "panel" });
    expect(mergeMachineRows([consoleRow], [machineCall])).toEqual([consoleRow, machineCall]);
  });

  it("drops a machine row with no trace against a traced console row on the same key", () => {
    const consoleRow = line({ trace: "console-1" });
    const machineCopy = { ...line({ trace: undefined }), machine_seq: 7, door: "console" };
    expect(mergeMachineRows([consoleRow], [machineCopy])).toEqual([consoleRow]);
  });

  it("falls back to the composite key when only the machine's row carries a trace", () => {
    // An audit line from before the trace existed has no trace; the machine's copy does. Matching on
    // the trace alone would draw the same call twice.
    const consoleRow = line({ trace: undefined });
    const machineCopy = {
      ...line({ trace: "machine-8", machine_seq: 8, door: "console" })
    };
    expect(mergeMachineRows([consoleRow], [machineCopy])).toEqual([consoleRow]);
  });

  it("falls back to the merge's composite key for a row written before traces", () => {
    const untraced = line({ trace: undefined });
    const copy = { ...untraced, machine_seq: 12, door: "console" };
    expect(mergeMachineRows([untraced], [copy])).toEqual([untraced]);
  });

  it("stays newest first after the merge", () => {
    const older = line({ ts_ms: 1_000, tool: "a", trace: "m-1", machine_seq: 1 });
    const newer = line({ ts_ms: 3_000, tool: "b", trace: "c-2" });
    const middle = line({ ts_ms: 2_000, tool: "c", trace: "m-2", machine_seq: 2 });
    const merged = mergeMachineRows([newer], [older, middle]);
    expect(merged.map((entry) => entry.ts_ms)).toEqual([3_000, 2_000, 1_000]);
  });

  it("returns the console's window untouched when nothing was added", () => {
    const consoleLines = [proxied];
    expect(mergeMachineRows(consoleLines, [])).toBe(consoleLines);
  });
});

describe("machineCounts", () => {
  it("says the journal was read only when the machine keeps one and answered for it", () => {
    const read = (over: Partial<MachineReadAnswer>) =>
      machineCounts({ state: "read", error: null, answer: answer(over) }, 0);
    expect(read({}).journal).toBe(true);
    // An older machine's refusal is not a record read.
    expect(read({ unknown: true }).journal).toBe(false);
    // A machine that keeps no journal answered, but there is no window to claim.
    expect(read({ journal: false }).journal).toBe(false);
    // A failed read and a fleet window claim nothing either.
    expect(machineCounts({ state: "failed", error: "offline", answer: null }, 0).journal).toBe(false);
    expect(EMPTY_MACHINE.journal).toBe(false);
  });
});

describe("machineSentence", () => {
  it("says nothing when the machine contributed no row", () => {
    expect(machineSentence(EMPTY_MACHINE)).toBeNull();
    expect(machineSentence(machineCounts({ state: "read", error: null, answer: answer() }, 0))).toBeNull();
  });

  it("counts the calls only the machine's record holds", () => {
    expect(machineSentence({ ...EMPTY_MACHINE, contributed: 2 })).toBe(
      "2 of these calls are on the machine's record, not this console's."
    );
    expect(machineSentence({ ...EMPTY_MACHINE, contributed: 1 })).toBe(
      "1 of these calls is on the machine's record, not this console's."
    );
  });

  it("says why the journal was not read", () => {
    expect(
      machineSentence(
        machineCounts({ state: "failed", error: "the machine is not connected", answer: null }, 0)
      )
    ).toBe("The machine's own journal was not read: the machine is not connected.");
  });

  it("says a machine does not offer the journal rather than drawing an empty one", () => {
    expect(
      machineSentence(
        machineCounts({ state: "read", error: null, answer: answer({ unknown: true }) }, 0)
      )
    ).toBe("The machine's own journal was not read: this machine does not offer it.");
  });

  it("carries the paging facts the footer draws", () => {
    const counts = machineCounts(
      {
        state: "read",
        error: null,
        answer: answer(),
        more: true,
        paged: true,
        readingOlder: true,
        olderError: "the machine is not connected"
      },
      3
    );
    expect(counts).toMatchObject({
      contributed: 3,
      more: true,
      paged: true,
      readingOlder: true,
      olderError: "the machine is not connected"
    });
    // A caller that predates paging is not a paging machine.
    expect(machineCounts({ state: "read", error: null, answer: answer() }, 1)).toMatchObject({
      more: false,
      paged: false,
      readingOlder: false,
      olderError: null
    });
  });
});

describe("machineContribution", () => {
  it("keeps only the machine rows the console's window does not hold", () => {
    const consoleRow = line({ trace: "console-1", tool: "files.read" });
    const machineDuplicate = line({ trace: "console-1", tool: "files.read", door: "console" });
    const machineOnly = line({ trace: "panel-1", tool: "screen.capture", door: "panel" });
    const rows = machineContribution([consoleRow], [machineDuplicate, machineOnly]);
    expect(rows.map((row) => row.trace)).toEqual(["panel-1"]);
  });

  it("is the same set the merge draws", () => {
    const consoleRow = line({ trace: "console-1" });
    const machineOnly = line({ trace: "mesh-1", door: "mesh" });
    const merged = mergeMachineRows([consoleRow], [consoleRow, machineOnly]);
    expect(merged).toHaveLength(2);
    expect(machineContribution([consoleRow], [consoleRow, machineOnly])).toHaveLength(1);
  });
});

describe("machineRecordSentence", () => {
  it("names the fact an audit row cannot", () => {
    const row = line({ machine_seq: 44, door: "panel" });
    expect(machineRecordSentence(row)).toBe(
      "This call is only in the machine's own journal; the console never proxied it. It arrived by the panel."
    );
  });

  it("says nothing on a row the console drew", () => {
    expect(machineRecordSentence(line())).toBeNull();
  });

  it("leaves an audit line's own position to the read-back's sentence", () => {
    // Round 9: a proxied line now carries `machine_seq`, but it is not a journal-drawn row (no
    // `door`) — its sentence is the read-back's, not "only in the machine's journal".
    expect(machineRecordSentence(line({ machine_seq: 814 }))).toBeNull();
  });
});
