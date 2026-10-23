export type AuditLine = {
  ts_ms: number;
  kind: string;
  device_id?: string | null;
  tool?: string | null;
  detail?: string | null;
  status: number;
};

export function AuditTable({ lines }: { lines: AuditLine[] }) {
  if (lines.length === 0) return <p className="hint">No audit lines yet.</p>;
  return (
    <div className="scroll">
      <table>
        <thead>
          <tr><th>when</th><th>kind</th><th>device</th><th>tool</th><th>status</th></tr>
        </thead>
        <tbody>
          {lines.map((line, index) => (
            <tr key={`${line.ts_ms}-${index}`}>
              <td className="mono">{new Date(line.ts_ms).toLocaleTimeString()}</td>
              <td>{line.kind}</td>
              <td className="mono">{line.device_id ? line.device_id.slice(0, 12) + "…" : ""}</td>
              <td className="mono">{line.tool ?? line.detail ?? ""}</td>
              <td><span className={"chip " + (line.status < 400 ? "chip-ok" : "chip-bad")}>{line.status}</span></td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
