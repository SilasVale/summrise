import type { AuditLine } from "./api";

/**
 * What the audit table is narrowing by. `device` is an exact id and `""` means every device; `tool`
 * is empty for every tool; `status` is one of the two outcomes the operator can name.
 */
export type AuditFilter = { device: string; tool: string; status: "all" | "ok" | "failed" };

/**
 * Whether the exchange failed, in the operator's sense.
 *
 * The HTTP status is not enough. A device's JSON-RPC error is returned to the caller as HTTP 200 (an
 * MCP client treats a non-2xx as a transport failure and would lose the error's code), and the console
 * records that 200 with `detail: "jsonrpc error -32002"` — so a refused tool call was a green 200 in
 * the table. The derived outcome is the one the status filter and the row's warning read.
 */
export function auditFailed(line: AuditLine): boolean {
  return line.status >= 400 || (line.detail ?? "").startsWith("jsonrpc error");
}

/**
 * The lines the filter keeps, in the order they arrived.
 *
 * The tool field matches the tool name or the detail. The detail is searched for every line, not only
 * the tool-less ones: a device error carries its JSON-RPC code there, and that is how an operator finds
 * "the calls the device refused".
 */
export function filterAudit(lines: AuditLine[], filter: AuditFilter): AuditLine[] {
  const tool = filter.tool.trim().toLowerCase();
  if (filter.device === "" && tool === "" && filter.status === "all") return lines;
  return lines.filter((line) => {
    if (filter.device !== "" && line.device_id !== filter.device) return false;
    if (tool !== "") {
      const inTool = (line.tool ?? "").toLowerCase().includes(tool);
      const inDetail = (line.detail ?? "").toLowerCase().includes(tool);
      if (!inTool && !inDetail) return false;
    }
    if (filter.status === "ok" && auditFailed(line)) return false;
    if (filter.status === "failed" && !auditFailed(line)) return false;
    return true;
  });
}
