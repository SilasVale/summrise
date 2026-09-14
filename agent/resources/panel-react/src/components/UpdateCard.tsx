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
}

export const EMPTY_UPDATE: UpdateStatus = {
  current: "",
  channel: null,
  latest: null,
  updateAvailable: false,
  pinnedTo: null,
  busy: false,
  error: null,
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
  };
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
}: {
  status: UpdateStatus;
  failed?: boolean;
  refresh: () => Promise<void>;
  runningRelease?: string;
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
            <span className="update-current">running {status.current || "unknown"}</span>
            {status.latest && (
              <span className="update-latest" data-available={status.updateAvailable ? "yes" : "no"}>
                {status.updateAvailable ? `${status.latest} available` : `latest is ${status.latest}`}
              </span>
            )}
          </p>

          {!status.channel && (
            <p className="muted">
              No update channel is configured on this install (<code>platform.download_url</code> is
              unset), so this device is updated by hand. That is a supported way to run it.
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
