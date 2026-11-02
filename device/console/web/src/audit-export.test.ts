import { describe, expect, it } from "vitest";
import { auditCsv, auditCsvName } from "./audit-export";
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
