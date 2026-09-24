// ONE READ OF A DEVICE ROUTE — the loop thirteen modules hand-rolled, and the four
// places it had drifted.
//
// WHY THIS MODULE EXISTS. Every surface that reads a route off the device asks the
// same four questions, and each had been answered a different way:
//
//   * WHAT DOES A FAILURE DO TO THE VALUE IN HAND? The guard
//     `if (deviceRefused(j)) { setFailed(true); return; }` was byte-identical in four
//     files and co-conditioned in a fifth, and one of the five (`useAgentVitals`) had
//     drifted to `!j` — which an EMPTY OBJECT passes, because `{}` is truthy, so a
//     refusal carrying fields would have been read as a sample. The rule is one rule,
//     and it is also the panel's oldest discipline: a read that FAILED must not render
//     as a device that is EMPTY (or, here, as a chart that went quiet). So a failure
//     keeps the last good value and reports the read as `"unreadable"` — the two facts
//     stay distinguishable at every caller, and `reduce` is never shown a refusal at all.
//
//   * HOW FAST MAY THIS RUN? The cadence floor disagreed with no rule:
//     `Math.max(10_000, …)` in `useVitalsSeries`, `Math.max(5_000, …)` in `useMonitors`,
//     and NONE AT ALL in `useBootHistory`, `useAgentVitals` and `UpdateCard` — where a
//     caller passing a small `intervalMs` turned the panel into a poller. `Math.max`
//     over a stated floor is now the module's rule, and a caller that omits `everyMs`
//     gets NO timer at all rather than a default cadence nobody asked for.
//
//   * MAY A LATE REPLY STILL WRITE? The unmount guard had three idioms (a local
//     `let alive` in five files, a `useRef` plus a mount effect in two, an `aliveRef`
//     in one — and the remaining loop sites had NO guard at all). One idiom, here: a reply
//     that settles after the card is gone must not touch state, because the last frame the
//     operator saw is the last frame there is.
//
//   * AND WHO WINS WHEN TWO READS OVERLAP? "Only the NEWEST read may write" existed in
//     two of the thirteen (`useSessionArchive` is where it was written down), so the
//     other eleven could let a slow reply land on top of a newer one and rewind the
//     value. Here it is unconditional: every read takes a sequence number and only the
//     one still current may write.
//
// A caller states the route, the fold, the value to start from and the cadence. The four rules
// above are stated once, here, and NINE readers inherit them: the five migrated first
// (`useVitalsSeries`, `useBootHistory`, `useAgentVitals`, `useMonitors`, `UpdateCard`) and the four
// migrated next (`useSessionArchive`, `useOperationRuns`, `DeviceLogsCard`, `ConnectCard`).
//
// AND THE PARAGRAPH THAT USED TO SIT HERE NAMED SIX REMAINING SITES, TWO OF WHICH WERE WRONG. It
// said the six "still hand-roll the same shape"; measured, only FOUR of them ever did — the four
// just named — and the other two are not this loop at all:
//
//   * `useSSE.ts` is NOT a loop of this shape. It is a STREAM consumer: an event/byte reader with
//     its own reconnect, frame-decoding and gap-backfill rules, where a failure is not "keep the
//     last value" but "the stream is broken, say so and resume from the last frame". It is not
//     migrating, and calling it a copy of this loop made the stream look like a poll.
//
//   * `EvidenceDrawer.tsx` is NOT on this seam. It reads through `fetch` with an EXPLICIT `apiBase`
//     and a `token` PROP — the desktop shell's transport, handed in by its host — while this module
//     goes through `callApi`'s module-level transport. Migrating it would change WHICH TRANSPORT
//     ANSWERS, which is a behaviour change, not a migration.
//
// THE SITES STILL TO MIGRATE ARE THE ONES THAT DO FIT THIS SEAM, and they are named here as
// measured rather than left to be rediscovered: `useCommandEvents.ts` reads one route per session
// (`/api/sessions/{sid}`) on mount and on events, keeps the last good events, owns a read state and
// its own watermark/ordering guard — it fits, and it needs one thing this module does not express
// yet: a RESET when the path's subject changes, so a session switch cannot draw the previous
// session's events while the new read is in flight. `usePlugins.ts` reads a route with keep-last and
// a per-route error string, but it reads TWO routes (a once-gated spec plus a status) and carries
// actions, so it is not one read of one route. Neither is in this change's scope.
//
// AND THE REST OF THE PANEL'S READERS WERE MEASURED TOO, so "still to migrate" is a list rather
// than an impression: `useSessions.ts` and `TerminalPane.tsx` read a TOOL through `callTool`, not a
// route through `callApi`, so this seam does not describe them; `SettingsPage.tsx` and
// `ConnModal.tsx` read once to SEED EDITABLE state (a form, a picker's saved list) and report a
// failure in their own status line, so there is no single folded value to hand over.
import { useCallback, useEffect, useRef, useState } from "react";
import { callApi, deviceRefused } from "../lib/api";
import type { ReadState } from "../lib/readState";

export interface DeviceReadOptions<T> {
  /** The route to read. A function when the route carries a cursor that advances per read.
   *
   *  DELETED AND RESTORED, deliberately, and the record is here so it is not deleted a third
   *  time. The first version accepted `string | (() => string)` for the cursor readers, but
   *  neither of the two that need it (`useOperationRuns`, `useSessionArchive`) migrated in that
   *  round, so the form's only consumer was a test — a hypothetical seam by this repo's own rule
   *  (one adapter is hypothetical, two is real) — and it was removed. It returns in the change
   *  that gives it a production caller: `useOperationRuns` asks for what it has not seen
   *  (`since_ms` from its own cursor ref), and that cursor ADVANCES per read. A captured string
   *  would pin the first cursor forever and re-request the same window, so the function is
   *  resolved AT READ TIME, never at hook-call time. */
  path: string | (() => string);
  /** Pure. Called ONLY with a body the device actually sent. Never with a refusal. */
  reduce: (previous: T, body: unknown) => T;
  /** The value before the first successful read. */
  initial: T;
  /** How often to read. Omitted = read once at mount, and on `refresh`. */
  everyMs?: number;
  /** The smallest cadence this read may run at. Default 5_000. */
  floorMs?: number;
}

export interface DeviceRead<T> {
  data: T;
  read: ReadState;
  /** Read now. Resolves when the read settles; NEVER rejects. */
  refresh: () => Promise<void>;
}

/** The floor a cadence gets when the caller states none. Deliberately NOT exported: it
 *  is this module's answer to "how fast may a device read run", not a knob. */
const DEFAULT_FLOOR_MS = 5_000;

export function useDeviceRead<T>(opts: DeviceReadOptions<T>): DeviceRead<T> {
  const { path, reduce, initial, everyMs, floorMs = DEFAULT_FLOOR_MS } = opts;
  const [data, setData] = useState<T>(initial);
  const [read, setRead] = useState<ReadState>("reading");
  // The value IN HAND, mirrored for the fold. `refresh` is stable (see below), so it
  // cannot read `data` from its own closure — and the fold runs OUTSIDE React's updater,
  // which keeps a throwing `reduce` a FAILED READ (handled by the catch below) rather
  // than an exception thrown during render. Every parse function in this panel is total
  // by design; a read loop that could not survive a caller's reducer is not a seam worth
  // handing out.
  const dataRef = useRef<T>(initial);
  // The route and the fold as they are NOW, not as they were when this hook first ran: a
  // caller may hand in an inline `reduce` (a new function every render). Without this, a fresh
  // `refresh` identity every render would re-arm the interval effect on every render — a
  // self-inflicted poll storm. The same ref is what makes a FUNCTION path work: it is called at
  // read time (below), so a caller's cursor ref is read per read instead of pinned at mount.
  const pathRef = useRef(path);
  pathRef.current = path;
  const reduceRef = useRef(reduce);
  reduceRef.current = reduce;
  // ONLY THE NEWEST READ MAY WRITE. Two overlapping reads — a mount read plus a refresh, or
  // a focus-driven refresh landing on top of the interval's — must not let the slower one
  // land last and rewind the value. The rule was first written down in `useSessionArchive`
  // (one of only two readers in the panel that had it); it lives here now and that reader
  // keeps a pointer instead of a second copy (its local `inFlightRef` went with its loop).
  // The other reader that had a version of it — `useCommandEvents`' post-await `sid` re-check —
  // is a different question (a session SWITCH, not a stale reply) and still answers it itself.
  const seqRef = useRef(0);
  // UNMOUNTED IS NOT A PLACE TO WRITE. Declared before the read effect so the flag is set
  // for the mount read; cleared in the cleanup, which React runs last-in-first-out, i.e.
  // after the read effect's own cleanup.
  const aliveRef = useRef(false);
  useEffect(() => {
    aliveRef.current = true;
    return () => {
      aliveRef.current = false;
    };
  }, []);

  const refresh = useCallback(async () => {
    const seq = ++seqRef.current;
    try {
      // RESOLVED HERE, not when the hook was called. A cursor-carrying caller (see `path`'s
      // doc) advances its ref between reads, and a route captured once would ask for the same
      // window forever.
      const route = pathRef.current;
      const body = await callApi(typeof route === "function" ? route() : route);
      if (!aliveRef.current || seq !== seqRef.current) return;
      // A REFUSAL IS NOT A BODY. `deviceRefused` (`lib/api.ts`) is the panel's one
      // predicate for "the device said no", and `reduce` is never shown a refusal: a
      // refused read must not render as a device that is empty, and the two facts are
      // only distinguishable if the fold never sees the failure at all.
      if (deviceRefused(body)) {
        setRead("unreadable");
        return;
      }
      const fold = reduceRef.current;
      const next = fold(dataRef.current, body);
      dataRef.current = next;
      setData(next);
      setRead("ok");
    } catch {
      // KEEP THE LAST GOOD VALUE. A missed poll is not a device that went quiet — blanking
      // the surface would be this panel asserting it — so a failed read (a refusal or a
      // throw) leaves the value alone and says only that the read is `"unreadable"`, which
      // the caller draws in its own words.
      if (!aliveRef.current || seq !== seqRef.current) return;
      setRead("unreadable");
    }
  }, []);

  useEffect(() => {
    // A read at mount, always: a surface with no data and no timer still has to ask. And
    // `refresh` reads immediately wherever it is called from, timer or no timer.
    void refresh();
    // NO TIMER UNLESS ONE WAS ASKED FOR. With `everyMs` omitted this reads once at mount
    // (and on every `refresh` the caller makes) — an omitted cadence is not a default one.
    if (everyMs === undefined) return;
    // AND THE CADENCE IS FLOORED. `Math.max(floorMs, everyMs)` is this module's one rule
    // for how fast a read may run, so a caller that computed a nonsense cadence — or was
    // handed one by a malformed reply — cannot turn the panel into a poller.
    const t = window.setInterval(() => void refresh(), Math.max(floorMs, everyMs));
    return () => window.clearInterval(t);
  }, [refresh, everyMs, floorMs]);

  return { data, read, refresh };
}
