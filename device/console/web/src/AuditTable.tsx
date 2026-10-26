import { Fragment, useMemo, useState } from "react";
import type { AuditLine, Device } from "./api";
import { auditFailed, filterAudit, type AuditFilter } from "./auditFilter";
import { shortId } from "./format";

/**
 * How many lines the page asks the console for. `/audit?limit=` accepts up to 1000; 200 is enough
 * history for the table's filters without turning the poll into a full audit export.
 */
export const AUDIT_WINDOW = 200;

const EMPTY: AuditFilter = { device: "", tool: "", status: "all" };

/**
 * One key per line, stable across filtering and polling.
 *
 * The key is the line's content plus its ordinal among identical lines in the FULL list, not its
 * index in the filtered one: a filter that keeps an open row must keep it open, and the five-second
 * poll returns new objects for the same lines. A content-only key would collapse two identical lines
 * into one; the ordinal is what keeps them apart. The token id is part of the content: two lines that
 * differ only in which token made the call are different facts, so a reordered poll must not move the
 * open row from one token to the other.
 */
function lineKeys(lines: AuditLine[]): Map<AuditLine, string> {
  const seen = new Map<string, number>();
  const keys = new Map<AuditLine, string>();
  for (const line of lines) {
    const base = [
      line.ts_ms,
      line.kind,
      line.device_id ?? "",
      line.token_id ?? "",
      line.tool ?? "",
      line.detail ?? "",
      line.status
    ].join("|");
    const ordinal = seen.get(base) ?? 0;
    seen.set(base, ordinal + 1);
    keys.set(line, `${base}|${ordinal}`);
  }
  return keys;
}

export function AuditTable({ lines, devices }: { lines: AuditLine[]; devices: Device[] }) {
  const [filter, setFilter] = useState<AuditFilter>(EMPTY);
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());
  const shown = useMemo(() => filterAudit(lines, filter), [lines, filter]);
  const keys = useMemo(() => lineKeys(lines), [lines]);

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
            — the page loaded the newest {lines.length} lines; older lines are not loaded.
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
                <th>token</th>
                <th>tool</th>
                <th>status</th>
                <th>details</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((line) => {
                const key = keys.get(line) ?? String(line.ts_ms);
                const isOpen = open.has(key);
                const failed = auditFailed(line);
                const deviceError = failed && line.status < 400;
                return (
                  <Fragment key={key}>
                    <tr>
                      <td className="mono">{new Date(line.ts_ms).toLocaleTimeString()}</td>
                      <td>{line.kind}</td>
                      <td className="mono">
                        {line.device_id ? shortId(line.device_id) : ""}
                      </td>
                      <td className="mono">{line.token_id ? shortId(line.token_id) : "—"}</td>
                      <td className="mono">
                        {line.tool ?? line.detail ?? ""}
                        {deviceError && (
                          <>
                            {" "}
                            <span className="chip chip-warn" title={line.detail ?? undefined}>
                              error
                            </span>
                          </>
                        )}
                      </td>
                      <td>
                        <span
                          className={
                            "chip " +
                            (line.status >= 400 ? "chip-bad" : deviceError ? "chip-warn" : "chip-ok")
                          }
                          title={
                            deviceError
                              ? "HTTP 200: the device answered with a JSON-RPC error, shown in the detail"
                              : undefined
                          }
                        >
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
                        <td colSpan={7}>
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
