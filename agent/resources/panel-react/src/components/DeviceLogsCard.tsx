// DeviceLogsCard — the device's OWN logs, and the update verdict they answer.
//
// WHY THIS EXISTS. `GET /api/logs` was built to let a remote client see why the
// agent behaved oddly "without asking someone to open files (or guessing a path
// and cat-ing it over a PTY)" — and then NOTHING consumed it. Same shape as
// `/api/sessions` before the Archive page: a served route with no reader.
//
// The concrete thing it unblocks is the question the release docs answer with a
// four-way table: after `summrise update` the connection ALWAYS drops for ~10s, and
// that drop is the documented signature of a successful swap — which makes it
// useless as evidence, because a command that never arrived looks identical. The
// only honest answer is the log, and reading it meant a Get-Content on the box.
//
// It is a DIAGNOSTIC, not a control: nothing here updates anything. The update
// still happens through the CLI or the console.
import { useState } from "react";
import { useDeviceRead } from "../hooks/useDeviceRead";
import { diagnoseUpdate, type UpdateDiagnosis } from "../lib/updateDiagnosis";

interface LogFile {
  name: string;
  /** FALSE means the agent never wrote this file — a different fact from an
   *  empty one, and the route distinguishes them on purpose. */
  present: boolean;
  log: string;
}

/** What ONE body from `GET /api/logs` yields: the list and the directory it came
 *  from. One read value, not two pieces of state set from inside the fold — `dir`
 *  is part of the same answer, so it travels with it. */
interface LogsRead {
  /** `null` = no successful read yet (reading, or the read failed). */
  logs: LogFile[] | null;
  dir: string;
}

const NO_LOGS_YET: LogsRead = { logs: null, dir: "" };

const VERDICT_TONE: Record<UpdateDiagnosis["verdict"], string> = {
  "cli-swap-launched": "ok",
  "rust-swap": "ok",
  "cli-only": "warn",
  "never-arrived": "warn",
  "no-log": "quiet",
};

export function DeviceLogsCard() {
  const [open, setOpen] = useState<string | null>(null);

  // THE ONE-SHOT READ IS `useDeviceRead`'s (see its header): the mount read, the refusal guard,
  // the unmount guard. No `everyMs` — this is a diagnostic, read once.
  const { data, read } = useDeviceRead<LogsRead>({
    path: "/api/logs",
    // A DEVICE THAT ANSWERED "NO" DID NOT GIVE US LOGS. The route's contract is
    // `ok:true` plus a `logs` array; anything else is a failure to report, not an
    // empty result to draw. Checking only the rejection meant a `{ok:false}` body
    // fell through to an empty list, which renders exactly like a healthy device
    // that has written nothing — a claim about the device made from a response
    // that refused to make it. `useDeviceRead` never folds a refusal, so half of
    // that rule is now structural; the other half is this THROW, which the module
    // catches and reports as `"unreadable"` instead of folding `[]`.
    reduce: (_previous, body) => {
      const r = body as { logs?: unknown; dir?: unknown } | null;
      if (!Array.isArray(r?.logs)) {
        throw new Error("device logs: response carries no logs array");
      }
      return {
        logs: r.logs as LogFile[],
        dir: typeof r?.dir === "string" ? r.dir : "",
      };
    },
    initial: NO_LOGS_YET,
  });

  const { logs, dir } = data;
  // A FAILED read says so. Rendering "no logs" for an unreachable device would be
  // a claim about the device, and it is not one we can make. Driven by the READ
  // state, never by an empty array standing in for a failure.
  const failed = read === "unreadable";

  const update = logs?.find((l) => l.name === "summrise-update.log");
  const d = diagnoseUpdate(update ? update.log : null);

  return (
    <div className="settings-section">
      <h2>Device logs</h2>
      {failed ? (
        <p className="muted">
          The device did not answer, so its logs could not be read. This is not
          the same as a device with no logs.
        </p>
      ) : logs === null ? (
        <p className="muted">Reading the device's logs…</p>
      ) : (
        <>
          <p
            className={`device-logs-verdict device-logs-verdict-${VERDICT_TONE[d.verdict]}`}
            data-verdict={d.verdict}
          >
            {d.summary}
          </p>
          {d.receipt && <p className="device-logs-receipt">{d.receipt}</p>}
          {dir && <p className="muted device-logs-dir">Read from {dir}</p>}
          <div className="device-logs-list">
            {logs.map((l) => (
              <div key={l.name}>
                <button
                  type="button"
                  className="device-logs-toggle"
                  onClick={() =>
                    setOpen((cur) => (cur === l.name ? null : l.name))
                  }
                  aria-expanded={open === l.name}
                >
                  {l.name}
                  {/* An ABSENT file is named as absent. The route reports the
                      difference deliberately; flattening it here would throw
                      that away. */}
                  {!l.present && (
                    <span className="device-logs-absent">not written yet</span>
                  )}
                </button>
                {open === l.name && l.present && (
                  <pre className="device-logs-tail">{l.log}</pre>
                )}
                {open === l.name && !l.present && (
                  <p className="muted">
                    This device has never written {l.name}, so there is nothing
                    to show.
                  </p>
                )}
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
