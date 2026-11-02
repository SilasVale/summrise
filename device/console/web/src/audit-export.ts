import type { AuditLine } from "./api";

/** Resolve the device/token column: a name when known, the short id otherwise, empty for absent. */
export type AuditName = (id: string | null | undefined) => string;

/**
 * The audit table as CSV, for the view the operator is actually looking at. Every field is quoted
 * and inner quotes doubled, so a detail containing a comma, a quote or a newline cannot shift the
 * columns. Timestamps are ISO 8601 in UTC; the ids are beside the names so a spreadsheet can join.
 */
export function auditCsv(lines: AuditLine[], deviceName: AuditName, tokenName: AuditName): string {
  const rows = [
    ["timestamp", "kind", "device", "device_id", "token", "token_id", "tool", "status", "detail"],
    ...lines.map((line) => [
      new Date(line.ts_ms).toISOString(),
      line.kind,
      deviceName(line.device_id),
      line.device_id ?? "",
      tokenName(line.token_id),
      line.token_id ?? "",
      line.tool ?? "",
      String(line.status),
      line.detail ?? ""
    ])
  ];
  const cell = (value: string) => '"' + value.replace(/"/g, '""') + '"';
  return rows.map((row) => row.map(cell).join(",")).join("\r\n") + "\r\n";
}

/** A stable, human-readable filename: the operator can tell two exports apart in a downloads folder. */
export function auditCsvName(at: Date): string {
  const pad = (value: number) => String(value).padStart(2, "0");
  return (
    "summrise-audit-" +
    String(at.getFullYear()) +
    pad(at.getMonth() + 1) +
    pad(at.getDate()) +
    "-" +
    pad(at.getHours()) +
    pad(at.getMinutes()) +
    pad(at.getSeconds()) +
    ".csv"
  );
}
