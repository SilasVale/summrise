import type { AuditLine } from "./api";

/**
 * What the audit table is narrowing by. `device` is an exact id and `""` means every device; `tool`
 * is empty for every tool; `status` is one of the two outcomes the console can name.
 */
export type AuditFilter = { device: string; tool: string; status: "all" | "ok" | "failed" };

/**
 * The lines the filter keeps, in the order they arrived. A line with no tool is matched on its
 * detail: the device writes some audit lines with only a sentence.
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
    if (filter.status === "ok" && line.status >= 400) return false;
    if (filter.status === "failed" && line.status < 400) return false;
    return true;
  });
}
