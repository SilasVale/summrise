// react-jsx: no React import needed
import type { Session } from "../hooks/useSessions";
import type { AgentVitals } from "../hooks/useAgentVitals";
import { VitalsDial } from "./VitalsDial";
import { WaitingChip } from "./WaitingChip";
import { BootChip } from "./BootChip";
import { LoadChip } from "./LoadChip";
import type { VitalsSeries } from "../hooks/useVitalsSeries";

/** A reading, or an em dash while the agent has not reported one. Never 0 for
 *  "unknown": cpu_pct is absent on the FIRST sample by design (it is a server-side
 *  delta), and printing 0% there would be a lie the operator cannot see through. */
const reading = (v: number | null): string => (v === null ? "—" : `${Math.round(v)}%`);

export function StatusBar({ sessions, status, sseState, vitals, identity, recentCrashes, vitalsSeries }: {
  sessions: Session[];
  status: string;
  sseState: "connected" | "down" | "connecting";
  /** WHICH MACHINE this is. The panel is served BY the device, so the host it was
   *  reached on IS its identity — and with one front door over several devices that
   *  is the first thing the chrome should answer. Optional: callers without it
   *  render exactly as before. */
  identity?: string;
  /** Optional, so the strip still renders for callers with no vitals yet: the
   *  instrument is an addition to this line, not a precondition for it. */
  vitals?: AgentVitals;
  /** The device's 24 h crash count, when the shell has polled the restart history.
   *  Optional for the same reason: the chip renders without it. */
  recentCrashes?: number | null;
  /** The vitals SERIES, for the sustained-load chip. Optional: without it that chip is
   *  simply absent, which is the same thing it renders when the load is fine. */
  vitalsSeries?: VitalsSeries | null;
}) {
  const live = sessions.filter((s) => !s.closed).length;
  return (
    <div id="statusbar">
      {identity && (
        <>
          <span className="instrument-identity" title={identity}>{identity}</span>
          <span className="instrument-divider" aria-hidden="true" />
        </>
      )}
      {vitals && (
        <span className="instrument">
          <VitalsDial cpu={vitals.cpu} mem={vitals.mem} />
          <span className="instrument-reading">
            <span className="instrument-label">CPU</span>
            <span className="instrument-value">{reading(vitals.cpu)}</span>
          </span>
          <span className="instrument-reading">
            <span className="instrument-label">Mem</span>
            <span className="instrument-value">{reading(vitals.mem)}</span>
          </span>
          {vitals.uptime && (
            <>
              <span className="instrument-divider" aria-hidden="true" />
              <span className="instrument-reading">
                <span className="instrument-label">Up</span>
                <span className="instrument-value">{vitals.uptime}</span>
              </span>
            </>
          )}
          {/* The release the device is RUNNING. The desktop strip has always shown
              it; the panel never did, which meant the density you actually work in
              was the one that could not tell you what was deployed. */}
          {vitals.release && (
            <>
              <span className="instrument-divider" aria-hidden="true" />
              <span className="instrument-reading">
                <span className="instrument-label">Ver</span>
                <span className="instrument-value">{vitals.release}</span>
              </span>
            </>
          )}
          <span className="instrument-divider" aria-hidden="true" />
        </span>
      )}
      <span id="status" className={status.startsWith("error") || status.startsWith("open failed") ? "error" : ""}>{status}</span>
      <span id="session-count" className={live ? "" : "hidden"}>{live} session{live === 1 ? "" : "s"}</span>
      {/* HOW THE PREVIOUS RUN ENDED — a fault that outlives its boot, or the news of
          the restart the operator just triggered. Renders nothing for a first run, a
          clean exit, or a restart old enough that the uptime reading beside it already
          says the same thing. See lib/bootNotice.ts. */}
      <BootChip
        lastBoot={vitals?.lastBoot}
        uptimeSecs={vitals?.uptimeSecs}
        recentCrashes={recentCrashes}
      />
      {/* "It has been like this for a while" — the one fact the dial's two instantaneous
          numbers cannot carry. Renders nothing unless the rule in lib/spark.ts says so. */}
      <LoadChip series={vitalsSeries} />
      {/* The device-level answer to "is anything waiting for me?" — see
          WaitingChip. Renders nothing at zero. */}
      <WaitingChip sessions={sessions} />
      {sseState === "down" && <span id="sse-status">reconnecting…</span>}
    </div>
  );
}
