import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { callApi } from "../lib/api";
import { useDeviceRead } from "./useDeviceRead";

// Plugin inventory + playwright-mcp control (round-admin-ui Task 6).
//
// Data sources, per the design spec
// (docs/superpowers/specs/2026-08-15-agent-admin-ui-design.md):
//   GET /api/plugins/status              — playwright running state, read on
//                                          mount and on every refresh (the
//                                          5 s poll was removed in round 163)
//   GET /api/spec                        — the plugin registry (names and
//                                          descriptions come from the agent,
//                                          not a hardcoded list)
//   POST /api/plugins/playwright/start   — spawn playwright-mcp
//   POST /api/plugins/playwright/stop    — stop it
//
// Every start/stop attempt lands in `log` — the verbatim agent error body on
// failures — so the playwright card doubles as a startup log. All rendering
// downstream is TEXT-ONLY (React text nodes, never innerHTML).

interface PlaywrightStatus {
  running: boolean;
  port?: number;
  /** WHEN THE INSTANCE STARTED — and the device OMITS THIS on its healthy
   *  EXTERNAL branch, which its own comment calls the production path
   *  (`agent/src/plugins/playwright/manager.rs`: the SummrisePlaywright task hosts
   *  the instance, so it outlives the agent that reported it). A consumer must
   *  therefore treat an absent value as "not reported" rather than substituting
   *  a clock: `started_at ?? Date.now()` rendered "up 0s" for an instance that
   *  had been running for days. */
  started_at?: number;
  healthy?: boolean;
  /** The instance is hosted OUTSIDE this agent (the scheduled task). On the wire
   *  since the external branch was written and NOT DECLARED here until now — the
   *  same shape as `run_id` in round 30: a field the device records and the
   *  panel's type silently drops. */
  external?: boolean;
}

/** dsh StateDot states — success | warn | error | ongoing (design spec). */
type PluginState = "success" | "warn" | "error" | "ongoing";

interface PluginRow {
  name: string;
  displayName: string;
  description: string;
  enabled: boolean;
  state: PluginState;
  stateLabel: string;
  /** stage-n: number of MCP tools the plugin registers (from /api/spec). */
  toolCount?: number;
  /** Live playwright detail; only set on the playwright row. */
  playwright?: PlaywrightStatus;
}

interface LogLine {
  ts: string;
  text: string;
  error: boolean;
}

interface SpecPlugin {
  name: string;
  displayName: string;
  description: string;
  /** Full tool definitions from /api/spec — only the COUNT is surfaced
   *  (PluginRow.toolCount); the schemas themselves are dropped in the row
   *  map so state stays light. */
  tools?: { name: string }[];
}

const MAX_LOG = 50;

/** The spec fold's own words for the one failure IT diagnoses. A constant, so the message it throws
 *  and the sentence the page reports cannot drift apart. */
const SPEC_BODY_UNUSABLE = "the spec route answered without a plugins list";

export function usePlugins(active: boolean) {
  const [busy, setBusy] = useState<"start" | "stop" | null>(null);
  const [actionError, setActionError] = useState("");
  const [log, setLog] = useState<LogLine[]>([]);
  const busyRef = useRef(busy);
  busyRef.current = busy;

  // TWO READS, TWO ERRORS. These were one `loadError`, and the status fetch's
  // success cleared whatever the SPEC fetch had just set — so the failure this
  // hook most needed to report was wiped microseconds later by an unrelated
  // success, and the page went on saying "Loading inventory…". My own first fix
  // introduced that shape and the test caught it; a single cell cannot carry two
  // independent facts. Each read owns its own state now (`specRead`/`statusRead`
  // below), so the two facts are independent BY CONSTRUCTION rather than by two
  // `useState` cells that had to be kept apart, and the two sentences further
  // down keep them independent at this end as well.
  //
  // THE TWO READS ARE `useDeviceRead`'S (see its header): the mount read, the refusal guard, the
  // unmount guard, the ordering guard and keep-last-on-failure. Neither passes `everyMs` — round
  // 163 removed the poll — so both routes are read on mount and on `refresh()`, which is what this
  // hook did by hand. What stays here is the shape this hook has always had: the registry read ONCE
  // (the `specLoaded` gate in `refresh`), the status read on every refresh, and the two sentences.
  //
  // AND `enabled: active` IS THE HOOK'S OTHER GATE. `App` calls this hook with `connected`, and it
  // stays mounted through a disconnect — so `active` is what used to stop the read before `callApi`
  // was reached, both before the call and again after each await. The module owns it now:
  // `enabled: false` means no read at mount, no timer, and a `refresh` that does nothing, and
  // turning it back on IS the read a reconnect needs. The post-await re-checks go with it: the write
  // guards are the module's (unmount and ordering) and the panel draws the connection form while it
  // is disconnected, so a reply still in flight can only land in a state nobody is looking at.
  //
  // AND THE WORDS FOR A FAILED READ COME BACK FROM THE MODULE. `useDeviceRead` reports every failed read
  // as one of three words — and, beside it, `reason`: the sentence it caught (see its `DeviceRead` doc),
  // which is the DEVICE's own `error` for a refusal, the transport's message for a throw, and the FOLD's
  // own message when the fold is what refused. This hook therefore keeps NO note of its own: the sentence
  // for a body the fold refused is still written by the fold (the `throw` below, whose constant keeps the
  // message and the page's copy of it from drifting), and it reaches the page through `reason`. The
  // clearing the deleted note did by hand is the module's too — `reason` is `""` after a success and is
  // overwritten by the next failure — so a later transport failure cannot inherit the previous body's
  // words.

  const {
    data: spec,
    read: specRead,
    reason: specReason,
    refresh: refreshSpec,
  } = useDeviceRead<SpecPlugin[]>({
    path: "/api/spec",
    enabled: active,
    reduce: (_previous, body) => {
      // AN UNUSABLE BODY IS A FAILURE, NOT A SILENT NO-OP. This used to be
      // `if (Array.isArray(...)) { ... }` with NO else: a 200 whose body the
      // panel cannot use (a proxy's error page, an empty reply) fell straight
      // through, set nothing, and left `specLoaded` false — the same permanent
      // "Loading inventory…" the catch was fixed for, reached without a
      // throw. Routing it through the ONE failure path means both kinds are
      // reported and there is a single place that decides what a failed read
      // says.
      //
      // THE THROW IS THE HOOK'S, THE FAILURE PATH IS THE MODULE'S: `useDeviceRead` catches it, reports the
      // read as `"unreadable"` and carries this message out as `reason` — which is what the page prints,
      // with the last good registry kept. The same outcome this hook's own `throw` produced, with the
      // module owning the question (and the sentence no longer having to be re-printed one layer up). A
      // refusal never reaches this fold at all (the module folds it to `"unreadable"` first), which is the
      // other half of the same rule: neither a refusal nor a body it cannot use is ever read as "no
      // plugins".
      const plugins = (body as { plugins?: unknown } | null)?.plugins;
      if (!Array.isArray(plugins)) {
        throw new Error(SPEC_BODY_UNUSABLE);
      }
      return (plugins as SpecPlugin[]).filter(
        (p) => p && typeof p.name === "string",
      );
    },
    initial: [],
  });

  const {
    data: playwright,
    read: statusRead,
    reason: statusReason,
    refresh: refreshStatus,
  } = useDeviceRead<PlaywrightStatus | null>({
    path: "/api/plugins/status",
    enabled: active,
    // A BODY WITHOUT A `playwright` OBJECT IS A SUCCESS WITH NOTHING TO SHOW — the card's "first
    // status poll still pending" is `null`, and a real status replaces it. A failed READ keeps the
    // last good one either way, exactly as the hand-written `setPlaywright` did (it was never
    // called from the catch).
    reduce: (_previous, body) => {
      const res = body as { playwright?: PlaywrightStatus } | null;
      return res?.playwright && typeof res.playwright === "object"
        ? res.playwright
        : null;
    },
    initial: null,
  });

  // `specLoaded` IS THE SPEC READ'S STATE — the same fact the old boolean carried, derived instead
  // of set, so the public shape below does not change. It is what `refresh` asks (see the gate
  // there): the registry has loaded exactly when the read last ended `"ok"`.
  const specLoaded = specRead === "ok";

  // THE PAGE'S TWO SENTENCES, FROM THE TWO READ STATES AND THE MODULE'S `reason`. The texts are the ones
  // the two `catch` blocks used to set, so nothing the operator reads changes — and the words the failure
  // carried now come back with it instead of being reduced to the read state. The spec one keeps its
  // `inventory: ` prefix, which is the shape the hand-written throw produced for the fold's own message
  // (this layer still diagnoses that one body) and is now also how a refusal's `error` reaches the page.
  //
  // AND SAID OUT LOUD, NOT AS A PERMANENT "Loading…". The id this page gates on (`specLoaded`) stays
  // false through a failure, so the surface must not imply progress that stopped; the sentence below
  // is what the page prints instead.
  const specError =
    specRead === "unreadable"
      ? specReason
        ? `inventory: ${specReason}`
        : "inventory could not be read"
      : "";

  // A REFUSAL IS NOT THIS HOOK'S TO SPELL ANY MORE — AND ITS WORDS ARE NOT LOST WITH THE JOB. This read
  // used to ask `deviceRefused(res)` and throw `new Error(res?.error || "status failed")`, so a refusal
  // was reported as `status: <the device's own words>`. `useDeviceRead` asks that question now
  // (`lib/api.ts` owns the predicate and the module never folds a refusal) and hands the device's `error`
  // back as `reason`, so a refused status read is again reported in the device's own words — with the
  // last good status kept, and with the message-less sentence a failure that carried no words always got.
  // The device's own error string still lands verbatim in `log` for a start/stop ACTION (see `runAction`).
  const statusError =
    statusRead === "unreadable"
      ? statusReason
        ? `status: ${statusReason}`
        : "status poll failed"
      : "";

  // One status+spec refresh: the registry is static per agent process, so the
  // spec fetch runs once and `specLoaded` gates it.
  //
  // A FAILED SPEC FETCH IS NOT "TRANSIENT — RETRY NEXT TICK", which is what this
  // said and what its `catch` used to do. THERE IS NO TICK: the 5 s poll was removed
  // in round 163 (see the effect below) and `specLoaded` is the only thing that
  // re-arms the fetch, so a failure left it FALSE FOREVER — the inventory
  // rendered "Loading inventory…" permanently, with no error, and the only
  // recoveries were a tab refocus or a `playwright-changed` event. The status
  // fetch in the SAME hook does the right thing (its failure is reported);
  // this is the twin rule applied to one branch and not the other, inside one
  // function. It now reports the failure so the page can say what happened.
  const refresh = useCallback(async () => {
    // NO `active` CHECK HERE: the read that must not dial is the read the module was TOLD is off
    // (`enabled: active` above), which no-ops this call and takes no sequence number doing it. A
    // second gate in this closure could only disagree with the one the module mirrors per render.
    // THE `specLoaded` GATE, ASKED AT THE CALL SITE. The read's state is the id now, so "has it
    // loaded" is `read === "ok"` — and this is also what keeps a FAILED spec read retried: it
    // reports `"unreadable"`, which is not `"ok"`, so the next refocus or event asks again. That is
    // what the paragraph above requires, and why a failure is reported rather than silently retried.
    if (specRead !== "ok") {
      // NO NOTE TO CLEAR BEFORE THE ATTEMPT ANY MORE: the module's `reason` is reported with the settle
      // it was caught in (and is `""` after a success), so a refused retry carries the refusal's own
      // words rather than inheriting the previous body's — the clearing the deleted note did by hand,
      // done by the module.
      await refreshSpec();
    }
    await refreshStatus();
  }, [specRead, refreshSpec, refreshStatus]);

  // round-163: the 5s status POLL is gone. The playwright status refreshes
  // on mount, after every start/stop action (already), on the agent-pushed
  // `playwright-changed` SSE event, and on tab refocus.
  //
  // AND THE MOUNT READ IS THE MODULE'S OWN. `useDeviceRead` reads once when it is rendered, and that
  // read IS the `refresh()` this effect used to make at mount; asking here as well would dial both
  // routes twice per page load. The same is true of a RECONNECT: `enabled` is one of the module's
  // read-effect dependencies, so turning it back on re-arms that effect and reads both routes at
  // once — which is why this effect has nothing left to do but wire the events.
  useEffect(() => {
    if (!active) return;
    const onChange = () => {
      refresh();
    };
    window.addEventListener("summrise-playwright-changed", onChange);
    document.addEventListener("visibilitychange", onChange);
    return () => {
      window.removeEventListener("summrise-playwright-changed", onChange);
      document.removeEventListener("visibilitychange", onChange);
    };
  }, [active, refresh]);

  const pushLog = useCallback((text: string, error: boolean) => {
    const ts = new Date().toLocaleTimeString();
    setLog((prev) => [...prev.slice(-(MAX_LOG - 1)), { ts, text, error }]);
  }, []);

  const runAction = useCallback(
    async (which: "start" | "stop") => {
      if (busyRef.current) return;
      setBusy(which);
      try {
        const res = await callApi(`/api/plugins/playwright/${which}`, {
          method: "POST",
        });
        setActionError("");
        // NOT "ok". A start/stop whose reply carried no status is a call whose
        // outcome the panel did NOT learn, and logging it `ok` reports success the
        // device never claimed. Every current device path sends `status`, so this
        // arm is reached only when the reply is empty or unreadable — precisely
        // when a verdict must not be invented.
        const status =
          res && typeof res === "object" && typeof res.status === "string"
            ? res.status
            : "(no status in the reply)";
        pushLog(`${which} → ${status}`, false);
      } catch (e: any) {
        setActionError(e?.message || `${which} failed`);
        pushLog(`${which} FAILED: ${e?.message || "unknown error"}`, true);
      } finally {
        setBusy(null);
        // Re-read NOW: with no poll (round 163) nothing else would re-read after
        // an action, so the row would keep showing the pre-action state until a
        // refocus or a `playwright-changed` event.
        refresh();
      }
    },
    [pushLog, refresh],
  );

  const start = useCallback(() => runAction("start"), [runAction]);
  const stop = useCallback(() => runAction("stop"), [runAction]);

  // Inventory rows: registry plugins with live playwright state. Non-
  // playwright plugins run in-process → success. Playwright maps the four
  // StateDot states: running → ongoing, last action failed → error,
  // stopped → warn.
  /** THE PLAYWRIGHT ROW'S STATE AND ITS LABEL, PAIRED ONCE (round 127 of the standing goal).
   *
   *  The ladder was written twice in this file — once inside `rows`, once in `playwrightRow` — with the same three states,
   *  the same three labels and the same order, differing only in what the CALLER does when none of them applies (a spec row
   *  falls back to `warn`; the card is `null` while its first status poll is still pending). The pair is the fact; the
   *  fallback is the caller's. Writing the pair twice is how `Running` becomes `running` in one place and `Stopped` becomes
   *  `Paused` in the other without anybody noticing. */
  function playwrightState(
    running: boolean | undefined,
    actionError: unknown,
  ): { state: "ongoing" | "error" | "warn"; stateLabel: string } | null {
    if (running) return { state: "ongoing", stateLabel: "Running" };
    if (actionError) return { state: "error", stateLabel: "Error" };
    return null;
  }

  const rows = useMemo<PluginRow[]>(
    () =>
      spec.map((p) => {
        const base = {
          name: p.name,
          displayName: p.displayName,
          description: p.description,
          enabled: true,
          // stage-n: tools count badge (terminal=25, memory=6, …)
          toolCount: Array.isArray(p.tools) ? p.tools.length : undefined,
        };
        if (p.name === "playwright") {
          const pw = playwright ?? undefined; // PluginRow.playwright is `?`, not nullable
          const live = playwrightState(playwright?.running, actionError);
          if (live) return { ...base, ...live, playwright: pw };
          return {
            ...base,
            state: "warn" as const,
            stateLabel: "Stopped",
            playwright: pw,
          };
        }
        return { ...base, state: "success" as const, stateLabel: "Loaded" };
      }),
    [spec, playwright, actionError],
  );

  // The playwright card is driven by the live status DIRECTLY — independent
  // of /api/spec, so the control card still works if the registry fetch
  // failed. null = first status poll still pending.
  const playwrightRow: PluginRow | null = useMemo(() => {
    const base = {
      name: "playwright",
      displayName: "Playwright",
      description: "playwright-mcp browser automation",
      enabled: true,
    };
    const live = playwrightState(playwright?.running, actionError);
    if (live)
      return { ...base, ...live, playwright: playwright ?? { running: false } };
    if (playwright !== null)
      return {
        ...base,
        state: "warn" as const,
        stateLabel: "Stopped",
        playwright,
      };
    return null;
  }, [playwright, actionError]);

  // One line for the caller, in the order the reads happen: the inventory is
  // the page's body, the status is a row inside it.
  return {
    rows,
    specLoaded,
    playwright,
    playwrightRow,
    loadError: specError || statusError,
    busy,
    log,
    start,
    stop,
  };
}
