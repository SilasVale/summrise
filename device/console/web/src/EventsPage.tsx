import { useCallback, useEffect, useState } from "react";
import { useRequestGate } from "./requests";
import { callTool, toolFailure, type Grant } from "./terminal";

/** The health half of `state.get`: the same readiness answer `/api/health` gives. */
type Health = {
  status: string;
  version: string;
  pid: number;
  poisoned: boolean;
  /** Why the document is poisoned, when the device names a reason. */
  reason?: string;
};

/** What `state.get` answers: the document (or null), its health, and the device's event tail. */
type StateAnswer = {
  state: unknown;
  health: Health;
  events: unknown[];
};

/**
 * The wire shape as an older device answers it.
 *
 * `events` is an additive field: a device that predates it returns `{state, health}` only, and that
 * device is exactly the one whose health an operator may need most. Absence is the empty tail.
 */
type StateReply = Omit<StateAnswer, "events"> & { events?: unknown };

/**
 * The device's own event tail, as a page.
 *
 * `state.get` carries the last ten lines of `events.jsonl`, the same tail the local panel shows. They
 * are the device's own account of itself — repairs, quarantines, audit lines — so the page renders
 * whatever the device wrote rather than a fixed schema: an object becomes a row with its kind and
 * its remaining keys, and a line that is not an object is shown as the text it is. The tail arrives
 * oldest first; the page reverses a copy for an operator who reads newest first.
 *
 * A poisoned state document is not an empty page: the health half still answers degraded, and the
 * events the device recorded are still shown. Only a failed call is a page with no events at all.
 */
export function EventsPage({ grant }: { grant: Grant }) {
  const [answer, setAnswer] = useState<StateAnswer | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // The newest read wins: a Refresh that supersedes the mount's read, or a read for the previous
  // grant, must not paint over the current one.
  const gate = useRequestGate();

  const read = useCallback(async () => {
    const ticket = gate.begin();
    setBusy(true);
    setError(null);
    try {
      const reply = JSON.parse(await callTool(grant, "state.get", {})) as StateReply;
      if (gate.stale(ticket)) return;
      // A missing or non-array tail is `[]`: an older device still has health worth rendering, and a
      // malformed field must not blank the page.
      setAnswer({ ...reply, events: Array.isArray(reply.events) ? reply.events : [] });
    } catch (cause) {
      if (gate.stale(ticket)) return;
      // A failed read leaves nothing to show: the previous tail belongs to the attempt that failed,
      // and keeping it under the refusal would read as the answer the device just refused.
      setAnswer(null);
      setError(toolFailure(cause));
    } finally {
      if (!gate.stale(ticket)) setBusy(false);
    }
  }, [grant, gate]);

  // A different grant is a different device's events. The old tail and its refusal belong to the
  // device this page has left, so the new grant starts with an empty page and one fresh read.
  useEffect(() => {
    gate.begin();
    setAnswer(null);
    setError(null);
    setBusy(false);
    void read();
  }, [gate, read]);

  return (
    <div>
      <div className="panel-head">
        <h2>Events</h2>
        <div className="actions">
          <button type="button" disabled={busy} onClick={() => void read()}>
            Refresh
          </button>
        </div>
      </div>
      <p className="hint">The device keeps its last ten events.</p>

      {error && <p className="error">{error}</p>}

      {answer && (
        <>
          {answer.health.poisoned && (
            <p className="error">
              The device's state document is poisoned, so its health is degraded.
            </p>
          )}
          {answer.health.poisoned && answer.health.reason && (
            <p className="hint mono">{answer.health.reason}</p>
          )}

          <dl className="facts" aria-label="Device health">
            <div>
              <dt>Status</dt>
              <dd>{answer.health.status}</dd>
            </div>
            <div>
              <dt>Version</dt>
              <dd>{answer.health.version}</dd>
            </div>
            <div>
              <dt>PID</dt>
              <dd>{answer.health.pid}</dd>
            </div>
          </dl>

          {answer.events.length === 0 ? (
            <p className="hint">The device has recorded no events.</p>
          ) : (
            <table>
              <thead>
                <tr>
                  <th>When</th>
                  <th>Kind</th>
                  <th>Fields</th>
                </tr>
              </thead>
              <tbody>
                {/* Reversed copy, never in place: the device's oldest-first order is an answer, not a draft. */}
                {[...answer.events].reverse().map((event, index) =>
                  isRecord(event) ? (
                    <tr key={index}>
                      <td>{localTime(event.ts_ms)}</td>
                      <td>{textOf(event.kind ?? event.event)}</td>
                      <td>
                        {fieldsOf(event).map((field) => (
                          <span key={field} className="mono">
                            {field}{" "}
                          </span>
                        ))}
                      </td>
                    </tr>
                  ) : (
                    <tr key={index}>
                      <td colSpan={3} className="mono">
                        {textOf(event)}
                      </td>
                    </tr>
                  )
                )}
              </tbody>
            </table>
          )}
        </>
      )}
    </div>
  );
}

/** Whether an event is one of the object lines the device writes, not a string fallback. */
function isRecord(event: unknown): event is Record<string, unknown> {
  return typeof event === "object" && event !== null && !Array.isArray(event);
}

/** An event's kind, or nothing when the line carries no string kind. */
function textOf(value: unknown): string {
  return typeof value === "string" ? value : value == null ? "" : String(value);
}

/** The operator's clock for an event's `ts_ms`; an absent or unreadable stamp is left blank. */
function localTime(ts: unknown): string {
  return typeof ts === "number" && Number.isFinite(ts) ? new Date(ts).toLocaleString() : "";
}

/**
 * Every key the device wrote except the labels the table already shows, as `key=value`.
 *
 * A tool-call line names its label `event` (begin/end); the other lines use `kind`. Both belong in
 * the Kind column, so neither is repeated among the fields.
 */
function fieldsOf(event: Record<string, unknown>): string[] {
  return Object.entries(event)
    .filter(([key]) => key !== "ts_ms" && key !== "kind" && key !== "event")
    .map(([key, value]) => `${key}=${valueText(value)}`);
}

/** A field value as text: a string stands alone, anything else is its JSON spelling. */
function valueText(value: unknown): string {
  return typeof value === "string" ? value : JSON.stringify(value) ?? String(value);
}
