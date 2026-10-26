import { describe, expect, it } from "vitest";
import type { AuditLine } from "./api";
import { auditFailed, filterAudit, type AuditFilter } from "./auditFilter";

const A = "device-a";
const B = "device-b";

const lines: AuditLine[] = [
  { ts_ms: 1, kind: "tool", device_id: A, tool: "memory.export", detail: "wrote 3 entries", status: 200 },
  { ts_ms: 2, kind: "tool", device_id: B, tool: "terminal.write", detail: null, status: 403 },
  { ts_ms: 3, kind: "tool", device_id: A, tool: "terminal.list", detail: null, status: 200 }
];

const empty: AuditFilter = { device: "", tool: "", status: "all" };
const filter = (patch: Partial<AuditFilter>): AuditFilter => ({ ...empty, ...patch });

describe("filterAudit", () => {
  it("returns the same array for an empty filter", () => {
    expect(filterAudit(lines, empty)).toBe(lines);
  });

  it("narrows by device alone", () => {
    expect(filterAudit(lines, filter({ device: A }))).toEqual([lines[0], lines[2]]);
  });

  it("narrows by tool alone, case-insensitively", () => {
    expect(filterAudit(lines, filter({ tool: "TERMINAL" }))).toEqual([lines[1], lines[2]]);
  });

  it("narrows by status alone", () => {
    expect(filterAudit(lines, filter({ status: "ok" }))).toEqual([lines[0], lines[2]]);
    expect(filterAudit(lines, filter({ status: "failed" }))).toEqual([lines[1]]);
  });

  it("composes device and tool", () => {
    expect(filterAudit(lines, filter({ device: A, tool: "terminal" }))).toEqual([lines[2]]);
  });

  it("composes device and status", () => {
    expect(filterAudit(lines, filter({ device: A, status: "failed" }))).toEqual([]);
  });

  it("matches the detail with or without a tool", () => {
    const detailOnly: AuditLine = {
      ts_ms: 4,
      kind: "tool",
      device_id: A,
      tool: null,
      detail: "Uploaded a firmware image",
      status: 200
    };
    const refused: AuditLine = {
      ts_ms: 5,
      kind: "mcp",
      device_id: A,
      tool: "terminal.create",
      detail: "jsonrpc error -32002",
      status: 200
    };
    expect(filterAudit([detailOnly], filter({ tool: "firmware" }))).toEqual([detailOnly]);
    // A tool call's detail can name the outcome, and that is how an operator finds a refusal.
    expect(filterAudit([refused], filter({ tool: "32002" }))).toEqual([refused]);
  });

  it("calls a device JSON-RPC error failed although it crossed the console as HTTP 200", () => {
    const refused: AuditLine = {
      ts_ms: 5,
      kind: "mcp",
      device_id: A,
      tool: "terminal.create",
      detail: "jsonrpc error -32002",
      status: 200
    };
    expect(auditFailed(refused)).toBe(true);
    expect(auditFailed(lines[0])).toBe(false);
    expect(filterAudit([refused], filter({ status: "failed" }))).toEqual([refused]);
    expect(filterAudit([refused], filter({ status: "ok" }))).toEqual([]);
  });
});
