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

/** `20261102T113000.000Z`: basic ISO 8601 in UTC, milliseconds kept, filename-safe. */
function compactIso(ms: number): string {
  const at = new Date(ms);
  const pad = (value: number, width = 2) => String(value).padStart(width, "0");
  return (
    String(at.getUTCFullYear()) +
    pad(at.getUTCMonth() + 1) +
    pad(at.getUTCDate()) +
    "T" +
    pad(at.getUTCHours()) +
    pad(at.getUTCMinutes()) +
    pad(at.getUTCSeconds()) +
    "." +
    pad(at.getUTCMilliseconds(), 3) +
    "Z"
  );
}

/**
 * One audit line per line, oldest first, with no header row.
 *
 * The page's newest-first window is the present at the top; a file is a history, so it is written
 * the other way round. The order is by `ts_ms`, stable for equal moments, because the window's own
 * order is a presentation choice and a file's order should be the record's.
 */
export function auditJsonl(lines: AuditLine[]): string {
  if (lines.length === 0) return "";
  return (
    lines
      .slice()
      .sort((left, right) => left.ts_ms - right.ts_ms)
      .map((line) => JSON.stringify(line))
      .join("\n") + "\n"
  );
}

/**
 * The window's filename, named by the moments it actually spans. `to` is one millisecond past the
 * newest line because a line exactly at `to` is outside the range the file proves.
 */
export function auditJsonlName(lines: AuditLine[]): string {
  if (lines.length === 0) return "summrise-audit-empty.ndjson";
  const moments = lines.map((line) => line.ts_ms);
  const from = Math.min(...moments);
  const to = Math.max(...moments) + 1;
  return `summrise-audit-${compactIso(from)}_to_${compactIso(to)}.ndjson`;
}
