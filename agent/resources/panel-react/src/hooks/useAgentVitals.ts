// Vitals for the instrument surfaces: `/api/status`, polled.
//
// ONE OWNER FOR THE POLL. The desktop status strip and the panel's instrument line
// both need cpu / mem / uptime / release, and a second copy of this fetch would be a
// second place for the `release` rule to drift — which is the defect
// `lib/agentVersion.ts` exists to prevent (there is exactly one rule for which
// version field wins, and every caller goes through it).
//
// CPU% IS A SERVER-SIDE DELTA (`agent/src/metrics.rs`): the FIRST sample has no
// previous reading to subtract from, so it is legitimately absent and appears from
// the second poll onward. Nothing here invents a zero for it — an instrument that
// reports 0% when it means "unknown" is worse than one that reports nothing.
import { useEffect, useState } from "react";
import { callApi } from "../lib/api";
import { releaseVersion } from "../lib/agentVersion";

export interface AgentVitals {
  /** THE RELAY, AS THE DEVICE REPORTS IT (round 207). `configured: false` means this device has no relay — which is the
   *  default and is NOT the same as "a relay that is broken", and the difference is the whole reason the field exists.
   *  Null until the first sample, like every other reading here. */
  relay?: {
    configured: boolean;
    connected: boolean;
    failures: number;
    lastError: string | null;
  } | null;
  /** WHERE THE DEVICE IS BOUND, AS CONFIGURED — a DIFFERENT fact from `location.host`, which is the address this browser
   *  reached it on. They are the same string on a local browser and differ the moment a relay or a tunnel is in the path,
   *  which is why Settings shows both and says which is which. Reported since 1.2.448: the bind lived only in a YAML file
   *  on the device, so an operator asking "where is that configured?" had no surface to ask. */
  host?: string;
  port?: number;
  /** WHERE THE CONFIG FILE IS, reported by the agent from the same path helper it reads the file with. The operator asked for
   *  the config file by name: every value on the Settings page comes from it, and nothing said where it was. */
  configPath?: string;
  /** Release the device reports (NOT the Cargo version — see lib/agentVersion.ts). */
  release: string;
  /** Human uptime, already formatted. Empty until the first sample. */
  uptime: string;
  /** Percentages 0–100, or null while unknown. */
  cpu: number | null;
  mem: number | null;
  /** The same uptime in seconds, RAW — `uptime` is a rendering of it, and a rule that
   *  needs to compare ("did this happen just now?") cannot compare against "2h 14m".
   *  Optional so every existing caller keeps compiling: the instrument is an
   *  addition to this strip, never a precondition for it. */
  uptimeSecs?: number | null;
  /** How the PREVIOUS run of the agent ended, when the device has a verdict on
   *  record (`/api/status`'s `last_boot` + `last_boot_kind`). Null = no verdict on
   *  record; undefined = nobody has polled yet. See lib/bootNotice.ts for the one
   *  rule that decides whether either fact is worth the operator's eye. */
  lastBoot?: LastBoot | null;
}

/** The five verdicts `runstate.rs` can reach. Kept as a union because the panel
 *  BRANCHES on it: a typo in a comparison must fail the build, not silently render
 *  nothing. An unrecognised spelling from a newer agent degrades to `kind: null`
 *  (the detail still renders) rather than being coerced into one of these. */
export type BootKind =
  "first-run" | "clean-exit" | "replaced" | "machine-restart" | "crashed";

export interface LastBoot {
  kind: BootKind | null;
  /** The operator's sentence — `logs/startup.log`'s own line, verbatim. */
  detail: string;
}

const BOOT_KINDS: BootKind[] = [
  "first-run",
  "clean-exit",
  "replaced",
  "machine-restart",
  "crashed",
];

/** Read the boot verdict off a `/api/status` body. `null` when the response carries
 *  none — which is a fact ("this device has never booted a build that records one"),
 *  not an error. The detail is what makes the verdict worth showing; a kind with no
 *  sentence is dropped rather than rendered as a bare word. */
export function parseLastBoot(j: Record<string, unknown>): LastBoot | null {
  const detail = typeof j.last_boot === "string" ? j.last_boot.trim() : "";
  if (!detail) return null;
  const raw = typeof j.last_boot_kind === "string" ? j.last_boot_kind : "";
  const kind = BOOT_KINDS.find((k) => k === raw) ?? null;
  return { kind, detail };
}

const EMPTY_VITALS: AgentVitals = {
  release: "",
  uptime: "",
  cpu: null,
  mem: null,
  uptimeSecs: null,
  lastBoot: null,
};

/** Seconds → the shortest honest form: `45s`, `12m 30s`, `3h 05m`, `2d 4h`. */
export function fmtUptime(secs: number): string {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
  if (secs < 86400)
    return `${Math.floor(secs / 3600)}h ${String(Math.floor((secs % 3600) / 60)).padStart(2, "0")}m`;
  return `${Math.floor(secs / 86400)}d ${Math.floor((secs % 86400) / 3600)}h`;
}

export function useAgentVitals(intervalMs = 15000): AgentVitals {
  const [vitals, setVitals] = useState<AgentVitals>(EMPTY_VITALS);
  useEffect(() => {
    let alive = true;
    const tick = async () => {
      try {
        const j = await callApi("/api/status");
        // A FAILED READ MUST NOT UPDATE, which is the rule every sibling poller already follows
        // (`useMonitors`, `useVitalsSeries`, `useBootHistory`, `UpdateCard` all gate on `ok !== true`)
        // and this one did not: its guard was `!j`, which an EMPTY OBJECT passes because `{}` is
        // truthy. So a refusal carrying fields would have been read as a sample. Keep-last is the right
        // answer for a failed poll — "vitals are a nicety" — but keeping last means not reading it.
        if (!alive || j?.ok !== true) return;
        // A partial sample UPDATES ONLY WHAT IT CARRIES. cpu_pct is missing on the
        // first poll by design, and blanking the memory reading because of it would
        // make the instrument flicker between "known" and "unknown" every start.
        setVitals((prev) => {
          const next: AgentVitals = { ...prev };
          const v = releaseVersion(j);
          if (v) next.release = v;
          if (typeof j.uptime_secs === "number") {
            next.uptime = fmtUptime(j.uptime_secs);
            next.uptimeSecs = j.uptime_secs;
          }
          // CLEARED RATHER THAN KEPT, like the field below it: a relay that stops being reported must not leave the strip
          // claiming a connection. `configured: false` is a real answer and is kept as one — it renders as nothing.
          next.relay =
            j.relay && typeof j.relay === "object"
              ? {
                  configured: j.relay.configured === true,
                  connected: j.relay.connected === true,
                  failures: Number(j.relay.consecutive_failures || 0),
                  lastError:
                    typeof j.relay.last_error === "string"
                      ? j.relay.last_error
                      : null,
                }
              : null;
          // Kept while present, like cpu and mem below: an agent that does not report a bind has not moved its bind.
          if (typeof j.host === "string") next.host = j.host;
          if (typeof j.config_path === "string")
            next.configPath = j.config_path;
          if (typeof j.port === "number") next.port = j.port;
          if (typeof j.cpu_pct === "number") next.cpu = j.cpu_pct;
          if (typeof j.mem_pct === "number") next.mem = j.mem_pct;
          // THE ONE FIELD THAT IS CLEARED RATHER THAN KEPT. Every other reading here
          // keeps its last value when a sample omits it, because "the agent did not tell
          // me this time" is not news. A boot verdict is different: the device either has
          // one on record or does not, so a response that carries none means there is
          // none — keeping a stale "the last run crashed" would be the panel asserting a
          // fault that no longer exists. A FAILED poll never reaches this line (the catch
          // above keeps everything), which is the distinction that matters.
          next.lastBoot = parseLastBoot(j);
          return next;
        });
      } catch {
        /* keep the last values — vitals are a nicety, never a hard dependency */
      }
    };
    void tick();
    const t = window.setInterval(tick, intervalMs);
    return () => {
      alive = false;
      window.clearInterval(t);
    };
  }, [intervalMs]);
  return vitals;
}
