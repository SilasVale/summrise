import { describe, expect, it } from "vitest";
import type {
  AuditLine,
  Device,
  MachineDayAnswer,
  MachineReadAnswer,
  MachineRow
} from "./api";
import {
  FLEET_MACHINES,
  fleetContribution,
  fleetDay,
  fleetDayNote,
  fleetJournal,
  fleetMachineRows,
  fleetPlan,
  fleetSentence,
  mergeFleetRows,
  type FleetMachineRead
} from "./fleetRecord";

/**
 * **THE FLEET'S RECORD (round 13).** The ledger's next item: a fleet window draws only the
 * console's rows because no single journal answers for it, and its day counts only the console's
 * audit. The page now fans out, bounded, to the online machines and merges what they say. These
 * tests pin the five facts the merge and the sentences must keep apart: a machine that could not
 * be asked is not one whose journal was empty; a fleet larger than the cap is named; the console's
 * row wins the same call; a journal-less machine is complete, not missing; and a read in flight is
 * not a "not counted".
 */

function device(id: string, over: Partial<Device> = {}): Device {
  return { id, name: null, online: true, version: null, last_seen_ms: null, ...over };
}

function machineRow(seq: number, over: Partial<MachineRow> = {}): MachineRow {
  return {
    seq,
    ts: "2026-11-13T10:00:00.000Z",
    type: "call.finished",
    tool: "host.facts",
    ok: true,
    door: "loopback",
    ...over
  };
}

function page(deviceId: string, over: Partial<MachineReadAnswer> = {}): MachineReadAnswer {
  return {
    device: deviceId,
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

function dayAnswer(
  deviceId: string,
  loopback: { calls: number; failed: number } = { calls: 0, failed: 0 },
  over: Partial<MachineDayAnswer> = {}
): MachineDayAnswer {
  return {
    device: deviceId,
    journal: true,
    day_start_ms: 1_000,
    day_end_ms: 2_000,
    oldest_seq: 1,
    last_seq: 10,
    scanned: 5,
    more: false,
    calls: {
      console: { calls: 0, failed: 0 },
      panel: { calls: 0, failed: 0 },
      mesh: { calls: 0, failed: 0 },
      loopback,
      unknown: { calls: 0, failed: 0 }
    },
    read_at_ms: 1,
    unknown: false,
    ...over
  };
}

function read(
  d: Device,
  over: { page?: Partial<FleetMachineRead["page"]>; day?: Partial<FleetMachineRead["day"]> } = {}
): FleetMachineRead {
  return {
    device: d,
    page: { state: "read", answer: page(d.id), error: null, ...over.page },
    day: { state: "read", answer: dayAnswer(d.id), error: null, ...over.day }
  };
}

function consoleCall(over: Partial<AuditLine> = {}): AuditLine {
  return {
    ts_ms: 1_000,
    kind: "mcp",
    status: 200,
    ok: true,
    device_id: "A",
    tool: "files.read",
    ...over
  };
}

describe("fleetPlan", () => {
  it("asks the online machines in order, at most the cap, and keeps the rest", () => {
    const devices = Array.from({ length: 10 }, (_, index) => device(`D${index}`));
    const plan = fleetPlan(devices);
    expect(plan.asked.map((entry) => entry.id)).toEqual([
      "D0",
      "D1",
      "D2",
      "D3",
      "D4",
      "D5",
      "D6",
      "D7"
    ]);
    expect(plan.beyond.map((entry) => entry.id)).toEqual(["D8", "D9"]);
    expect(plan.offline).toEqual([]);
    expect(plan.asked.length).toBe(FLEET_MACHINES);
  });

  it("asks every online machine when the fleet is smaller than the cap", () => {
    const plan = fleetPlan([device("A"), device("B", { online: false }), device("C")]);
    expect(plan.asked.map((entry) => entry.id)).toEqual(["A", "C"]);
    expect(plan.beyond).toEqual([]);
    expect(plan.offline.map((entry) => entry.id)).toEqual(["B"]);
  });

  it("asks nothing of an empty fleet", () => {
    expect(fleetPlan([])).toEqual({ asked: [], beyond: [], offline: [] });
  });
});

describe("fleetMachineRows", () => {
  it("draws the calls each machine's journal holds and keeps the machine's own name", () => {
    const a = device("A", { name: "DESKTOP-A" });
    const rows = fleetMachineRows([
      read(a, { page: { answer: page("A", { rows: [machineRow(3), machineRow(2, { door: "panel" })] }) } })
    ]);
    expect(rows.map((row) => [row.device_id, row.machine_seq])).toEqual([
      ["A", 3],
      ["A", 2]
    ]);
    expect(rows[0]?.sentence).toContain("DESKTOP-A");
  });

  it("draws nothing for a machine that did not answer, does not offer it, or keeps no journal", () => {
    const rows = fleetMachineRows([
      read(device("A"), { page: { state: "failed", answer: null, error: "boom" } }),
      read(device("B"), { page: { answer: page("B", { unknown: true }) } }),
      read(device("C"), { page: { answer: page("C", { journal: false }) } })
    ]);
    expect(rows).toEqual([]);
  });

  it("reports the fleet's journal only when a machine answered for one", () => {
    expect(fleetJournal([read(device("A"))])).toBe(true);
    expect(fleetJournal([read(device("A"), { page: { answer: page("A", { journal: false }) } })])).toBe(
      false
    );
    expect(fleetJournal([read(device("A"), { page: { answer: page("A", { unknown: true }) } })])).toBe(
      false
    );
  });
});

describe("fleetDay", () => {
  it("adds the answered machines' direct calls and their failures", () => {
    const day = fleetDay(
      [
        read(device("A"), { day: { answer: dayAnswer("A", { calls: 2, failed: 1 }) } }),
        read(device("B"), { day: { answer: dayAnswer("B", { calls: 3, failed: 0 }) } })
      ],
      fleetPlan([device("A"), device("B")])
    );
    expect(day).toEqual({ calls: 5, failed: 1, more: false, unread: 0 });
  });

  it("counts a cut machine as a floor, and names every kind of machine it could not ask", () => {
    const plan = fleetPlan([device("A"), device("B"), device("C", { online: false })]);
    const reads: FleetMachineRead[] = [
      read(device("A"), { day: { answer: dayAnswer("A", { calls: 1, failed: 0 }, { more: true }) } }),
      read(device("B"), { day: { state: "failed", answer: null, error: "boom" } })
    ];
    const day = fleetDay(reads, plan);
    expect(day).toEqual({ calls: 1, failed: 0, more: true, unread: 2 });
  });

  it("does not count a journal-less machine as unread: its day is the console's own record", () => {
    const plan = fleetPlan([device("A"), device("B")]);
    const day = fleetDay(
      [
        read(device("A"), { day: { answer: dayAnswer("A", { calls: 2, failed: 0 }) } }),
        read(device("B"), { day: { answer: dayAnswer("B", { calls: 0, failed: 0 }, { journal: false }) } })
      ],
      plan
    );
    expect(day).toEqual({ calls: 2, failed: 0, more: false, unread: 0 });
  });

  it("has no number when no machine answered", () => {
    const plan = fleetPlan([device("A"), device("B", { online: false })]);
    expect(
      fleetDay([read(device("A"), { day: { state: "failed", answer: null, error: "boom" } })], plan)
    ).toBeNull();
  });
});

describe("fleetDayNote", () => {
  it("says how many machines' direct calls are missing", () => {
    const plan = fleetPlan([device("A"), device("B"), device("C", { online: false })]);
    expect(
      fleetDayNote(
        [
          read(device("A"), { day: { answer: dayAnswer("A") } }),
          read(device("B"), { day: { state: "failed", answer: null, error: "boom" } })
        ],
        plan
      )
    ).toBe("The direct calls of 2 machines were not counted.");
  });

  it("says nothing when every machine answered", () => {
    const plan = fleetPlan([device("A"), device("B")]);
    expect(fleetDayNote([read(device("A")), read(device("B"))], plan)).toBeNull();
  });

  it("is silent while a read is in flight: counting is not the same as not counted", () => {
    const plan = fleetPlan([device("A")]);
    expect(fleetDayNote([read(device("A"), { day: { state: "reading", answer: null } })], plan)).toBeNull();
  });

  it("says a fleet with no online machine was not counted at all", () => {
    const plan = fleetPlan([device("A", { online: false }), device("B", { online: false })]);
    expect(fleetDayNote([], plan)).toBe(
      "The direct calls were not counted: no machine in this fleet is online."
    );
  });

  it("says nothing for an empty fleet", () => {
    expect(fleetDayNote([], { asked: [], beyond: [], offline: [] })).toBeNull();
  });
});

describe("fleetSentence", () => {
  it("names the calls the machines' records added", () => {
    const plan = fleetPlan([device("A"), device("B")]);
    expect(fleetSentence([read(device("A")), read(device("B"))], plan, 7)).toBe(
      "7 of these calls are on the machines' records, not this console's."
    );
  });

  it("is silent when the machines added nothing and every record was read", () => {
    const plan = fleetPlan([device("A")]);
    expect(fleetSentence([read(device("A"))], plan, 0)).toBeNull();
  });

  it("names the machines whose record was not read, with why, and the ones beyond the cap", () => {
    const devices = Array.from({ length: 10 }, (_, index) =>
      device(`D${index}`, { name: `DESKTOP-${index}` })
    );
    const plan = fleetPlan(devices);
    const reads = [
      read(devices[0]!, { page: { state: "failed", answer: null, error: "boom" } }),
      read(devices[2]!, { page: { answer: page("D2", { unknown: true }) } }),
      read(devices[8]!, { page: { answer: page("D8") } })
    ];
    expect(fleetSentence(reads, plan, 0)).toBe(
      "The journal of 2 machines was not read: DESKTOP-0 (did not answer), DESKTOP-2 (does not " +
        "offer its journal). 2 more machines were not asked (the page reads at most 8)."
    );
  });

  it("keeps the count and the reason for an offline machine, and lists at most three names", () => {
    const plan = fleetPlan([
      device("A", { name: "DESKTOP-A" }),
      device("B", { name: "DESKTOP-B", online: false }),
      device("C", { name: "DESKTOP-C", online: false }),
      device("D", { name: "DESKTOP-D", online: false }),
      device("E", { name: "DESKTOP-E", online: false })
    ]);
    expect(fleetSentence([read(device("A", { name: "DESKTOP-A" }))], plan, 0)).toBe(
      "The journal of 4 machines was not read: DESKTOP-B (offline), DESKTOP-C (offline), " +
        "DESKTOP-D (offline) and 1 more."
    );
  });

  it("does not call an in-flight read a record that was not read", () => {
    const plan = fleetPlan([device("A"), device("B")]);
    expect(
      fleetSentence(
        [read(device("A")), read(device("B"), { page: { state: "reading", answer: null } })],
        plan,
        0
      )
    ).toBeNull();
  });
});

describe("mergeFleetRows", () => {
  it("keeps the console's row for a call the machines also recorded", () => {
    const consoleRow = consoleCall({ trace: "t-1", ts_ms: 2_000 });
    const machine = machineRow(4, { trace: "t-1", ts: "2026-11-13T10:00:02.000Z" });
    const rows = fleetMachineRows([
      read(device("A"), { page: { answer: page("A", { rows: [machine] }) } })
    ]);
    expect(fleetContribution([consoleRow], rows)).toEqual([]);
    expect(mergeFleetRows([consoleRow], rows)).toEqual([consoleRow]);
  });

  it("draws a call only the machine holds, in time order", () => {
    const consoleRow = consoleCall({
      ts_ms: Date.parse("2026-11-13T10:00:04.000Z"),
      trace: "t-1"
    });
    const machine = machineRow(4, { trace: "t-2", ts: "2026-11-13T10:00:03.000Z" });
    const rows = fleetMachineRows([
      read(device("A"), { page: { answer: page("A", { rows: [machine] }) } })
    ]);
    expect(mergeFleetRows([consoleRow], rows).map((row) => row.ts_ms)).toEqual([
      Date.parse("2026-11-13T10:00:04.000Z"),
      Date.parse("2026-11-13T10:00:03.000Z")
    ]);
    expect(mergeFleetRows([consoleRow], rows)[1]?.trace).toBe("t-2");
  });

  it("does not merge two machines' untraced rows just because their clocks agree", () => {
    const at = "2026-11-13T10:00:00.000Z";
    const consoleRow = consoleCall({ device_id: "A", ts_ms: Date.parse(at) });
    const rows = fleetMachineRows([
      read(device("A"), {
        page: { answer: page("A", { rows: [machineRow(1, { ts: at, tool: "files.read" })] }) }
      }),
      read(device("B"), {
        page: { answer: page("B", { rows: [machineRow(1, { ts: at, tool: "files.read" })] }) }
      })
    ]);
    expect(fleetContribution([consoleRow], rows).map((row) => row.device_id)).toEqual(["B"]);
  });

  it("draws one machine row once, however many pages repeat its sequence", () => {
    const one = machineRow(1);
    const rows = fleetMachineRows([read(device("A"), { page: { answer: page("A", { rows: [one, one] }) } })]);
    expect(mergeFleetRows([], rows).length).toBe(1);
  });
});
