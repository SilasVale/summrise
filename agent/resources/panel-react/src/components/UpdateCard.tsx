// UpdateCard — "is this device current, and can I do something about it?"
//
// WHY IT EXISTS. The device has been updatable since npm became the single channel, but only
// from OUTSIDE it: `vale update` over a terminal, or an AI calling `agent_update`. An
// operator looking at the panel — the surface they actually have open — could not tell
// whether the box was on yesterday's build, and the one place that came close
// (`DeviceLogsCard`) DIAGNOSES an update after the fact from a log file. This card answers
// before and offers the action.
//
// WHAT IT IS NOT: a second installer. The button POSTs the SAME tool an AI would call
// (`/api/tools/agent_update`), so the sha256 gate, the host pin, the rollback pin, the busy
// marker and the swap are the code that already ships — and `/api/update` reports state with
// those same rules (see `plugins::update::update_status`). This card only renders them.
//
// HONEST STATES, the panel's discipline, and here they matter more than usual because the
// wrong one costs an operator trust in the update channel:
//   * "no update channel configured" — a purely local install, NOT a failure;
//   * "the release server did not answer" — unknown, and never drawn as "up to date";
//   * "pinned by vale rollback" — an update may exist that this device will REFUSE;
//   * "already in flight" — the busy marker, so a second click cannot race the first.
import { useCallback, useEffect, useRef, useState } from "react";
import { callApi } from "../lib/api";

export interface UpdateStatus {
  current: string;
  channel: string | null;
  latest: string | null;
  updateAvailable: boolean;
  pinnedTo: string | null;
  busy: boolean;
  /** Text when the channel could not be read; null when it answered (or was never asked). */
  error: string | null;
  /** THE LAST UPDATE THIS DEVICE LAUNCHED, or null when it never has.
   *
   *  The device records it at the moment it hands the swap script to WMI (`record_update_attempt`), which is the
   *  only moment anything can say for certain that an update STARTED — the log narrates what the script did
   *  afterwards, and it is written by two programs. This is the fact; the logs card's log reading is the
   *  narration, and stays the fallback for a device that reports the field as absent. */
  lastAttempt: { atMs: number; from: string; to: string } | null;
  /** WHEN THE DEVICE LAST ASKED ITS CHANNEL — epoch milliseconds, or null when it never has.
   *
   *  The device reports this (`checked_at`, from `update_status`), and this panel used to drop it: the card
   *  said "1.2.435 available" with no age on it, while the device answers from a 30-second cache and may have
   *  failed its last check entirely. A version claim without a time is a claim the reader cannot weigh — the
   *  same reason the vitals window and the restart list carry their span. */
  checkedAt: number | null;
}

const EMPTY_UPDATE: UpdateStatus = {
  current: "",
  channel: null,
  latest: null,
  updateAvailable: false,
  pinnedTo: null,
  busy: false,
  error: null,
  checkedAt: null,
  lastAttempt: null,
};

const str = (v: unknown): string | null => (typeof v === "string" && v ? v : null);

/** Read `/api/update`. Never throws; a body this build cannot use is the empty state, which
 *  renders as "unknown" rather than as "current". */
export function parseUpdateStatus(j: unknown): UpdateStatus {
  const b = (j ?? {}) as Record<string, unknown>;
  return {
    current: str(b.current) ?? "",
    channel: str(b.channel),
    latest: str(b.latest),
    updateAvailable: b.update_available === true,
    pinnedTo: str(b.pinned_to),
    busy: b.busy === true,
    error: str(b.error),
    // A NUMBER, NOT A STRING: the wire sends epoch ms, and `0` is not a time anyone can weigh (it reads as 1970),
    // so a non-positive or non-finite value is null — the same "absent, never zero" rule the vitals and boot
    // records follow.
    checkedAt:
      typeof b.checked_at === "number" && Number.isFinite(b.checked_at) && b.checked_at > 0
        ? b.checked_at
        : null,
    lastAttempt: parseAttempt(b.last_attempt),
  };
}

/** The launch record, or null. A record without a POSITIVE time, a `from` and a `to` is not one — the device
 *  refuses such a body too (`last_update_attempt`), and this refuses it again because the two ends of a wire
 *  contract drift independently: half a record would render as "updated from to at Invalid Date". */
export function parseAttempt(v: unknown): { atMs: number; from: string; to: string } | null {
  if (!v || typeof v !== "object") return null;
  const a = v as Record<string, unknown>;
  const atMs = a.at_ms;
  if (typeof atMs !== "number" || !Number.isFinite(atMs) || atMs <= 0) return null;
  const from = typeof a.from === "string" ? a.from : "";
  const to = typeof a.to === "string" ? a.to : "";
  if (!from && !to) return null;
  return { atMs, from, to };
}

/** "checked 12s ago" — the age of the device's answer, in the panel's own vocabulary.
 *
 *  The unit follows the size of the number because the reader's question changes with it: seconds matter while an
 *  update is being applied, minutes when the card is idle. Anything over an hour is stated in hours, and the exact
 *  timestamp rides along in the title so a reader who needs the clock time has it. */
/** "2m ago" — the age of an ACT, where `checkedAge` is the age of a READING. Same units, no verb: the sentence
 *  around it already says what happened ("the swap was handed over 2m ago"), and repeating "checked" there would
 *  describe the wrong event. */
export function attemptAge(atMs: number | null, nowMs: number): string {
  const age = checkedAge(atMs, nowMs);
  return age ? age.replace(/^checked /, "") : "at an unknown time";
}

export function checkedAge(checkedAt: number | null, nowMs: number): string | null {
  // ZERO IS NOT A TIME. The mapper already refuses non-positive values, and this refuses them again because the
  // formatter is the last place before the screen: `epoch 0` renders as "checked 497204h ago", which is a claim
  // about a device that simply has not answered that question.
  if (checkedAt === null || !Number.isFinite(checkedAt) || checkedAt <= 0) return null;
  const secs = Math.max(0, Math.round((nowMs - checkedAt) / 1000));
  if (secs < 90) return `checked ${secs}s ago`;
  const mins = Math.round(secs / 60);
  if (mins < 90) return `checked ${mins}m ago`;
  return `checked ${Math.round(mins / 60)}h ago`;
}

/** The update state, re-read on an interval (the release channel is cached device-side, so
 *  this is cheap) and after every action. */
export function useUpdateStatus(intervalMs = 60_000): UpdateStatus & {
  failed: boolean;
  refresh: () => Promise<void>;
} {
  const [status, setStatus] = useState<UpdateStatus>(EMPTY_UPDATE);
  const [failed, setFailed] = useState(false);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const refresh = useCallback(async () => {
    try {
      const j = await callApi("/api/update");
      if (!alive.current) return;
      if (j?.ok !== true) {
        setFailed(true);
        return;
      }
      setStatus(parseUpdateStatus(j));
      setFailed(false);
    } catch {
      if (alive.current) setFailed(true);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const t = window.setInterval(() => void refresh(), intervalMs);
    return () => window.clearInterval(t);
  }, [refresh, intervalMs]);

  return { ...status, failed, refresh };
}

type Phase = "idle" | "confirm" | "applying" | "applied" | "error";

export function UpdateCard({
  status,
  failed,
  refresh,
  /** The release the panel last saw, so a swap that happens WHILE the operator watches can
   *  be recognised as one. `useAgentVitals` supplies it. */
  runningRelease,
  /** The clock, injectable the way the vitals and monitor cards take it, so the age of the
   *  device's answer is testable instead of reading `Date.now()` inside the markup. */
  nowMs = Date.now(),
}: {
  status: UpdateStatus;
  failed?: boolean;
  refresh: () => Promise<void>;
  runningRelease?: string;
  nowMs?: number;
}) {
  const [phase, setPhase] = useState<Phase>("idle");
  const [message, setMessage] = useState("");
  const startedFrom = useRef<string>("");

  // A SWAP IS OBSERVED, NOT ANNOUNCED: the agent dies mid-update, so nothing can tell this
  // card "done" — the connection drops and the release changes. That is the signal, and it
  // is the same one the release docs tell a human to look for.
  useEffect(() => {
    if (phase !== "applying" || !runningRelease) return;
    if (startedFrom.current && runningRelease !== startedFrom.current) {
      setPhase("applied");
      setMessage(`now running ${runningRelease}`);
      void refresh();
    }
  }, [phase, runningRelease, refresh]);

  async function apply(force: boolean) {
    setPhase("applying");
    setMessage("staging the download — the agent restarts when it is applied");
    startedFrom.current = runningRelease ?? status.current;
    try {
      const r = await callApi("/api/tools/agent_update", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(force ? { force: true } : {}),
      });
      const result = r?.result ?? {};
      if (r?.ok !== true) {
        setPhase("error");
        setMessage(str(r?.error) ?? "the device refused the update");
        return;
      }
      if (result.status === "up_to_date") {
        setPhase("idle");
        setMessage("already up to date");
        void refresh();
        return;
      }
      if (result.status === "pinned") {
        setPhase("error");
        setMessage(str(result.message) ?? "the device is pinned by vale rollback");
        return;
      }
      // "upgrading": the download runs in the background and the swap kills the agent. Stay
      // in `applying` — the effect above watches for the release to change.
      setMessage(
        str(result.remote) ? `installing ${result.remote} — the panel reconnects on the new build` : "installing…",
      );
    } catch (e) {
      // The connection dropping HERE is expected during a swap; the version poll decides
      // whether it worked. Anything else is a real failure to report.
      setMessage(e instanceof Error ? e.message : "the call failed");
    }
  }

  const busy = status.busy || phase === "applying";
  const action = status.updateAvailable
    ? `Update to ${status.latest}`
    : status.latest
      ? `Reinstall ${status.current}`
      : "";

  return (
    <div className="settings-section">
      <h3>Agent update</h3>
      {failed && !status.current ? (
        <p className="muted">
          The device did not answer, so its update state could not be read. That is not the
          same as being up to date.
        </p>
      ) : (
        <>
          <p className="update-line">
            <span>running {status.current || "unknown"}</span>
            {status.latest && (
              <span className="update-latest" data-available={status.updateAvailable ? "yes" : "no"}>
                {status.updateAvailable ? `${status.latest} available` : `latest is ${status.latest}`}
              </span>
            )}
            {/* THE AGE OF THE ANSWER, which is the device's own fact (`checked_at`) and was being thrown away.
                A version claim with no time on it cannot be weighed: the device answers from a 30-second cache,
                and an update applied a minute ago still reads "1.2.403 available" until the next check lands.
                Rendered only where the device reported a time — a device that never checked says nothing here
                rather than "checked 56 years ago". */}
            {checkedAge(status.checkedAt, nowMs) && (
              <span
                className="update-checked"
                title={
                  status.checkedAt
                    ? `the device last asked its channel at ${new Date(status.checkedAt).toLocaleTimeString()}`
                    : undefined
                }
              >
                {checkedAge(status.checkedAt, nowMs)}
              </span>
            )}
          </p>

          {!status.channel && (
            <p className="muted">
              No update channel is configured on this install (<code>platform.download_url</code> is
              unset), so this device is updated by hand. That is a supported way to run it.
            </p>
          )}

          {/* THE LAST UPDATE THIS DEVICE LAUNCHED — the fact that answers "did my click do anything", which
              until now could only be inferred by opening a log file and reading a four-way verdict derived from
              two programs' text. Stated with its time, because "the swap started" and "the swap started twenty
              minutes ago and this build is still running" are different situations. */}
          {status.lastAttempt && (
            <p
              className="update-attempt"
              data-from={status.lastAttempt.from}
              data-to={status.lastAttempt.to}
            >
              Last update launched on this device: {status.lastAttempt.from || "?"} →{" "}
              {status.lastAttempt.to || "?"},{" "}
              <span
                className="update-attempt-age"
                title={`the device handed the swap script over at ${new Date(status.lastAttempt.atMs).toLocaleTimeString()}`}
              >
                {attemptAge(status.lastAttempt.atMs, nowMs)}
              </span>
            </p>
          )}

          {status.error && (
            // UNKNOWN, never "current": the release server not answering is a different fact
            // from the device being up to date, and this line is the only place that says so.
            <p className="update-warn" data-kind="unreachable">
              The release server did not answer ({status.error}) — so whether a newer build
              exists is unknown, not "no".
            </p>
          )}

          {status.pinnedTo && (
            <p className="update-warn" data-kind="pinned">
              Pinned to <strong>{status.pinnedTo}</strong> by <code>vale rollback</code>. An update
              may exist that this device will refuse until the pin is cleared on the device.
            </p>
          )}

          {busy && (
            <p className="update-warn" data-kind="busy">
              An update is already in flight on this device — a second one would race it.
            </p>
          )}

          {status.channel && action && !busy && (
            <div className="update-actions">
              {phase === "confirm" ? (
                <>
                  <button type="button" className="btn btn-danger" onClick={() => void apply(status.latest === status.current)}>
                    {status.updateAvailable ? `Yes, update to ${status.latest}` : `Yes, reinstall ${status.current}`}
                  </button>
                  <button type="button" className="btn btn-ghost" onClick={() => setPhase("idle")}>
                    Cancel
                  </button>
                </>
              ) : (
                <button type="button" className="btn" onClick={() => setPhase("confirm")}>
                  {action}
                </button>
              )}
              {!status.updateAvailable && phase === "idle" && (
                <span className="muted update-note">
                  Reinstalling downloads and re-applies the same build — the repair path for a
                  damaged install.
                </span>
              )}
            </div>
          )}

          {phase === "confirm" && (
            <p className="update-warn" data-kind="confirm">
              This device will restart the agent to apply the build. The panel reconnects by
              itself within about a minute; terminals close.
            </p>
          )}

          {message && (
            <p
              className={`update-state${phase === "error" ? " is-error" : ""}${phase === "applied" ? " is-ok" : ""}`}
              data-phase={phase}
            >
              {message}
            </p>
          )}
        </>
      )}
    </div>
  );
}
