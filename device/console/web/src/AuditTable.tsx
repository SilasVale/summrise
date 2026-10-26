import { Fragment, useMemo, useState } from "react";
import type { AuditLine, Device } from "./api";
import { filterAudit, type AuditFilter } from "./auditFilter";

/**
 * How many lines the page asks the console for. `/audit?limit=` accepts up to 1000; 200 is enough
 * history for the table's filters without turning the poll into a full audit export.
 */
export const AUDIT_WINDOW = 200;

const EMPTY: AuditFilter = { device: "", tool: "", status: "all" };

/** An id long enough to be worth shortening; the line's own detail shows the whole one. */
function shortId(id: string): string {
  return id.length > 12 ? id.slice(0, 12) + "…" : id;
}

function rowKey(line: AuditLine, index: number): string {
  return `${line.ts_ms}-${index}`;
}

export function AuditTable({ lines, devices }: { lines: AuditLine[]; devices: Device[] }) {
  const [filter, setFilter] = useState<AuditFilter>(EMPTY);
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());
  const shown = useMemo(() => filterAudit(lines, filter), [lines, filter]);

  const fleet = new Set(devices.map((device) => device.id));
  const retired = Array.from(
    new Set(
      lines
        .map((line) => line.device_id)
        .filter((id): id is string => id != null && id !== "" && !fleet.has(id))
    )
  ).sort();

  const hasFilter = filter.device !== "" || filter.tool.trim() !== "" || filter.status !== "all";

  const clear = () => {
    setFilter(EMPTY);
    setOpen(new Set());
  };

  const toggle = (key: string) => {
    setOpen((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  if (lines.length === 0) return <p className="hint">No audit lines yet.</p>;

  return (
    <div>
      <div className="audit-toolbar">
        <label className="audit-field">
          <span>Device</span>
          <select
            value={filter.device}
            onChange={(event) => setFilter({ ...filter, device: event.target.value })}
          >
            <option value="">All devices</option>
            {devices.map((device) => (
              <option key={device.id} value={device.id}>
                {device.name ?? shortId(device.id)}
              </option>
            ))}
            {retired.map((id) => (
              <option key={id} value={id}>
                {shortId(id)}
              </option>
            ))}
          </select>
        </label>
        <label className="audit-field tool">
          <span>Tool or detail</span>
          <input
            type="search"
            value={filter.tool}
            placeholder="filter by tool or detail"
            onChange={(event) => setFilter({ ...filter, tool: event.target.value })}
          />
        </label>
        <label className="audit-field">
          <span>Status</span>
          <select
            value={filter.status}
            onChange={(event) =>
              setFilter({ ...filter, status: event.target.value as AuditFilter["status"] })
            }
          >
            <option value="all">All</option>
            <option value="ok">Succeeded</option>
            <option value="failed">Failed</option>
          </select>
        </label>
        {hasFilter && (
          <button type="button" onClick={clear}>
            Clear
          </button>
        )}
      </div>

      <p className="hint audit-caption">
        {shown.length} of {lines.length} lines
        {lines.length >= AUDIT_WINDOW && (
          <>
            {" "}
            — this page shows the newest {lines.length} lines; the console keeps more.
          </>
        )}
      </p>

      {shown.length === 0 ? (
        <p className="hint">No audit lines match.</p>
      ) : (
        <div className="scroll">
          <table>
            <thead>
              <tr>
                <th>when</th>
                <th>kind</th>
                <th>device</th>
                <th>tool</th>
                <th>status</th>
                <th>details</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((line, index) => {
                const key = rowKey(line, index);
                const isOpen = open.has(key);
                return (
                  <Fragment key={key}>
                    <tr>
                      <td className="mono">{new Date(line.ts_ms).toLocaleTimeString()}</td>
                      <td>{line.kind}</td>
                      <td className="mono">
                        {line.device_id ? shortId(line.device_id) : ""}
                      </td>
                      <td className="mono">{line.tool ?? line.detail ?? ""}</td>
                      <td>
                        <span className={"chip " + (line.status < 400 ? "chip-ok" : "chip-bad")}>
                          {line.status}
                        </span>
                      </td>
                      <td>
                        <button
                          type="button"
                          className="mini"
                          aria-expanded={isOpen}
                          onClick={() => toggle(key)}
                        >
                          {isOpen ? "Hide" : "Details"}
                        </button>
                      </td>
                    </tr>
                    {isOpen && (
                      <tr>
                        <td colSpan={6}>
                          <dl className="facts">
                            <div>
                              <dt>Timestamp</dt>
                              <dd className="mono">{new Date(line.ts_ms).toISOString()}</dd>
                            </div>
                            <div>
                              <dt>Kind</dt>
                              <dd className="mono">{line.kind}</dd>
                            </div>
                            <div>
                              <dt>Device</dt>
                              <dd className="mono">{line.device_id ?? "—"}</dd>
                            </div>
                            <div>
                              <dt>Tool</dt>
                              <dd className="mono">{line.tool ?? "—"}</dd>
                            </div>
                            <div>
                              <dt>Detail</dt>
                              <dd className="mono">{line.detail ?? "—"}</dd>
                            </div>
                            <div>
                              <dt>Status</dt>
                              <dd>{line.status}</dd>
                            </div>
                          </dl>
                        </td>
                      </tr>
                    )}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
