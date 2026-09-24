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
  // AND THE ONE FAILURE THIS LAYER STILL HAS WORDS FOR IS KEPT HERE. `useDeviceRead` reports every
  // failed read as one of three words and never hands the exception back (see its `reduce` doc), so
  // the sentence for a body the FOLD refused is written by the fold that raised it — the only
  // failure at this end that carries a message — and cleared before each attempt, so a later
  // transport failure cannot inherit the previous body's words.
  const specNoteRef = useRef("");

  const {
    data: spec,
    read: specRead,
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
      // THE THROW IS THE HOOK'S, THE FAILURE PATH IS THE MODULE'S: `useDeviceRead` catches it and
      // reports the read as `"unreadable"`, keeping the last good registry — the same outcome this
      // hook's own `throw` produced, with the module owning the question. A refusal never reaches
      // this fold at all (the module folds it to `"unreadable"` first), which is the other half of
      // the same rule: neither a refusal nor a body it cannot use is ever read as "no plugins".
      const plugins = (body as { plugins?: unknown } | null)?.plugins;
      if (!Array.isArray(plugins)) {
        specNoteRef.current = SPEC_BODY_UNUSABLE;
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

  // THE PAGE'S TWO SENTENCES, FROM THE TWO READ STATES. The texts are the ones the two `catch`
  // blocks used to set, so nothing the operator reads changes. The spec one keeps its `inventory: `
  // prefix for the failure this layer diagnoses itself (the fold's — see `specNoteRef`); a refusal
  // or a transport failure carries no message this end can see any more, and gets the sentence a
  // message-less failure always got.
  //
  // AND SAID OUT LOUD, NOT AS A PERMANENT "Loading…". The id this page gates on (`specLoaded`) stays
  // false through a failure, so the surface must not imply progress that stopped; the sentence below
  // is what the page prints instead.
  const specError =
    specRead === "unreadable"
      ? specNoteRef.current
        ? `inventory: ${specNoteRef.current}`
        : "inventory could not be read"
      : "";

  // A REFUSAL IS NOT THIS HOOK'S TO SPELL ANY MORE. This read used to ask `deviceRefused(res)` and
  // throw `new Error(res?.error || "status failed")`, so a refusal was reported as `status: <the
  // device's own words>`. `useDeviceRead` asks that question now — `lib/api.ts` owns the predicate
  // and the module never folds a refusal — so a refused status read is reported the module's way:
  // `"unreadable"`, with the last good status kept, and the sentence a failure WITHOUT a message
  // always got. The device's own error string is not lost anywhere it mattered: a start/stop ACTION
  // still lands it verbatim in `log` (see `runAction`).
  const statusError =
    statusRead === "unreadable" ? "status poll failed" : "";

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
      // This attempt's words, not the previous attempt's (see `specNoteRef`).
      specNoteRef.current = "";
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
