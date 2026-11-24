import { describe, expect, it } from "vitest";
import { auditCsv, auditCsvName, auditJsonl, auditJsonlName } from "./audit-export";
import type { AuditLine } from "./api";

const line: AuditLine = {
  ts_ms: Date.UTC(2026, 10, 2, 11, 30, 0),
  kind: "mcp",
  device_id: "device-1",
  token_id: "token-1",
  tool: "runs.exec",
  detail: 'run "a,b" exited 0',
  status: 200
};

describe("audit export", () => {
  it("writes one quoted row per line and escapes quotes", () => {
    const csv = auditCsv([line], () => "workshop", () => "operator");
    const rows = csv.trimEnd().split("\r\n");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toBe('"timestamp","kind","device","device_id","token","token_id","tool","status","detail"');
    expect(rows[1]).toBe(
      '"2026-11-02T11:30:00.000Z","mcp","workshop","device-1","operator","token-1","runs.exec","200","run ""a,b"" exited 0"'
    );
  });

  it("leaves an absent id empty and names the file to the second", () => {
    const csv = auditCsv([{ ...line, device_id: null, token_id: null }], () => "", () => "");
    const row = csv.trimEnd().split("\r\n")[1];
    expect(row).toBe(
      '"2026-11-02T11:30:00.000Z","mcp","","","","","runs.exec","200","run ""a,b"" exited 0"'
    );
    expect(auditCsvName(new Date(2026, 10, 2, 9, 5, 7))).toBe("summrise-audit-20261102-090507.csv");
  });
});

describe("the JSON Lines window", () => {
  const early = { ...line, ts_ms: Date.UTC(2026, 10, 2, 11, 30, 0), tool: "early" };
  const late = { ...line, ts_ms: Date.UTC(2026, 10, 2, 11, 30, 2), tool: "late" };
  const middle = { ...line, ts_ms: Date.UTC(2026, 10, 2, 11, 30, 1), tool: "middle" };

  it("writes one audit line per line, oldest first, and no header row", () => {
    // The page holds the server's newest-first window; a file is a history, so the window is
    // written the other way round. It must not be reordered by anything but the moment.
    const text = auditJsonl([late, early, middle]);
    expect(text.endsWith("\n")).toBe(true);
    const lines = text.trimEnd().split("\n").map((entry) => JSON.parse(entry));
    expect(lines.map((entry) => entry.tool)).toEqual(["early", "middle", "late"]);
    expect(lines[0]).toMatchObject({ ts_ms: early.ts_ms, kind: "mcp" });
    expect(text.startsWith("{")).toBe(true);
  });

  it("names the window by the moments it actually spans", () => {
    expect(auditJsonlName([late, early, middle])).toBe(
      "summrise-audit-20261102T113000.000Z_to_20261102T113002.001Z.ndjson"
    );
    expect(auditJsonl([])).toBe("");
  });
});
