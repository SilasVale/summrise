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
// A caller states the route, the fold, the value to start from, the cadence — and, since the change
// that migrated `useCommandEvents`, whether the read is LIVE at all (`enabled`) and what a CHANGE OF
// SUBJECT is (`resetKey`). The four rules above are stated once, here, and the readers that inherit
// them are these — NAMED RATHER THAN COUNTED, deliberately: this sentence said NINE, then TEN, and the
// NUMBER is the part that drifts when a round adds one (the lesson AGENTS.md records for its own gate
// count). The exclusions, which are not readers of this loop, are the two paragraphs below.
//
//   * migrated first: `useVitalsSeries`, `useBootHistory`, `useAgentVitals`, `useMonitors`,
//     `UpdateCard`;
//   * migrated next: `useSessionArchive`, `useOperationRuns`, `DeviceLogsCard`, `ConnectCard`;
//   * migrated with the two options named above, which exist because they needed them:
//     `useCommandEvents` (one session's audit trail) and `usePlugins` (TWO reads — a once-gated spec
//     plus a status — migrated separately in the same round, and the reason `enabled` is a caller's
//     word for "there is nothing to read yet" rather than a route nobody may build).
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
// THE SITES THAT FIT THIS SEAM — the list this header exists to keep honest — ARE ACCOUNTED FOR, and
// each is named with what it needed, because "still to migrate" is a claim that has to be RE-MEASURED
// rather than inherited:
//
//   * `useCommandEvents.ts` — one route per session (`/api/sessions/{sid}`), read at mount and on
//     events, keeping the last good events and owning a read state, a per-session watermark and its
//     own ordering guard — IS MIGRATED. It is the caller `resetKey` was added FOR: a subject that can
//     CHANGE, whose previous value must not survive the switch by even one frame.
//
//   * `usePlugins.ts` — TWO routes (a once-gated spec plus a status), and it carries actions, so it
//     is two reads on this seam rather than one read of one route — is migrating separately in the
//     same round, and it is where `enabled` is shared. Its record is its own header, not this one.
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
  /** Called ONLY with a body the device actually sent — never with a refusal — and NEVER "pure" in
   *  the strict sense, which this contract used to claim and the review caught: a cursor-carrying
   *  reader (`useOperationRuns`) advances a caller-held ref from the reply it is folding, and the
   *  module never granted that. Exactly two things are guaranteed about a `reduce`:
   *
   *    * it sees a body the device SENT (a refusal and a throw are the module's, not the fold's);
   *    * if it THROWS, the read is reported as `"unreadable"`, the last good value is kept, and the
   *      exception never reaches React's render — which is what lets a strict caller say "a body I
   *      cannot use is a FAILED read" (`useSessionArchive`, `DeviceLogsCard`) rather than folding it
   *      into a default that would render as a device which is empty.
   *
   *  Anything else it does to the caller's own state is the caller's business. */
  reduce: (previous: T, body: unknown) => T;
  /** The value before the first successful read. */
  initial: T;
  /** How often to read. Omitted = read once at mount, and on `refresh`. */
  everyMs?: number;
  /** The smallest cadence this read may run at. Default 5_000. */
  floorMs?: number;
  /** Is this read LIVE? `false` = no read at mount, no timer, and `refresh()` does nothing. A
   *  caller that has no subject yet (no session selected) says so with this rather than by
   *  building a route it must not fetch.
   *
   *  ADDED WITH ITS CALLER, the same rule the function form of `path` above records: it is here
   *  because `useCommandEvents` is handed `sid: string | null` and has to answer "there is nothing
   *  to read yet" without asking the device about a session id it does not have. The no-op
   *  `refresh` is the half that is easy to get wrong — a caller's event listener (`visibilitychange`,
   *  a push from the agent) fires while the read is off, and a refresh that still went through would
   *  fetch the very route the caller said not to build. It also takes NO sequence number on the way
   *  out: `seqRef` is the ordering guard's whole state, so a no-op that bumped it would abandon a
   *  read the caller had legitimately started (see `refresh` below). */
  enabled?: boolean; // default true
  /** A value whose CHANGE means the thing being read is a DIFFERENT thing. On a change the value
   *  goes back to `initial`, the read state back to `"reading"`, and any in-flight read is
   *  abandoned (its reply must not land under the new subject). It is applied DURING RENDER, not
   *  in an effect: an effect would leave one frame of the previous subject on screen, which for a
   *  session switch reads as the new session's own trail. `resetKey` is the session id in the
   *  caller that needs this.
   *
   *  ADDED WITH ITS CALLER TOO (`useCommandEvents`), not in anticipation of one. Two things it does
   *  NOT do, both worth stating because a caller will assume them: it does not compare by VALUE
   *  (it is an ordinary dependency, so a caller passes a stable value — a session id — and never a
   *  fresh object per render), and it does not by itself re-read. The re-read comes from the read
   *  effect, which depends on this key as well: a new subject is read at once, exactly as it is at
   *  mount, rather than at the next tick — a caller that switched subjects would otherwise show an
   *  empty surface for up to a full cadence.
   *
   *  AND A FRESH OBJECT PER RENDER IS A LOOP, NOT A SLOWDOWN: the reset is applied during render, so
   *  a key that differs on every render would set state on every render. The ref is advanced BEFORE
   *  the state is set (see the reset below), so the one re-render the reset causes finds the key
   *  unchanged and stops — a caller cannot hang the panel, but it can make it re-render forever, and
   *  a session id is the value this was built for. */
  resetKey?: unknown;
}

export interface DeviceRead<T> {
  data: T;
  read: ReadState;
  /** Read now. Resolves when the read settles; NEVER rejects. WHEN THE READ IS DISABLED
   *  (`enabled: false`) this DOES NOTHING AND RESOLVES — a caller reads this member rather than the
   *  option's own doc, which is where the rule was first written and, until a review said so, the
   *  only place it was. */
  refresh: () => Promise<void>;
}

/** The floor a cadence gets when the caller states none. Deliberately NOT exported: it
 *  is this module's answer to "how fast may a device read run", not a knob. */
const DEFAULT_FLOOR_MS = 5_000;

export function useDeviceRead<T>(opts: DeviceReadOptions<T>): DeviceRead<T> {
  const {
    path,
    reduce,
    initial,
    everyMs,
    floorMs = DEFAULT_FLOOR_MS,
    enabled = true,
    resetKey,
  } = opts;
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
  // The other reader that had a version of it — `useCommandEvents`' post-await `sid` re-check (round
  // 138) — asked a different question (a session SWITCH, not a stale reply), and that question is
  // this module's now too: the guard below drops the reply, and `resetKey` abandons in flight the
  // moment the subject changes.
  const seqRef = useRef(0);
  // IS THIS READ LIVE? Mirrored like `path`, so the stable `refresh` reads it as it is NOW: a
  // caller turns the read off and back on (no session selected, then one selected), and a closure
  // that captured the first value could never be told.
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  // A DIFFERENT SUBJECT IS A DIFFERENT VALUE, AND THE SWITCH IS SETTLED DURING RENDER. An effect
  // would run after the frame that still holds the previous subject — one frame of the OLD session's
  // trail drawn as the new session's own — so the value, the read state and the in-flight read are
  // all reset here, where React is already computing the frame that will be committed. The value is
  // put back to the very `initial` the caller passed in, object identity included: that identity is
  // what lets a caller's `reduce` recognise the first fold of a new subject (see `useCommandEvents`).
  // The key is remembered rather than diffed against a render-count, so React's double render in
  // development sees the change once, and it is compared by identity like any other dependency.
  const keyRef = useRef(resetKey);
  if (keyRef.current !== resetKey) {
    keyRef.current = resetKey;
    dataRef.current = initial;
    // AND THE OLD SUBJECT'S REPLY IS ABANDONED, not merely out of date: a read still in flight was
    // built from the route of the subject that is gone, and letting it land would write the previous
    // subject's value under the new one. The sequence number is what every write already checks.
    seqRef.current++;
    setData(initial);
    setRead("reading");
  }
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
    // A DISABLED READ READS NOTHING — AND IT MUST NOT BORROW THE GUARD'S SEQUENCE NUMBER TO SAY SO.
    // `seqRef` is the ordering guard's entire state, so a no-op that still did `++seqRef` would
    // abandon a read the caller had legitimately started (`enabled` can be turned off while one is
    // in flight) and then return as if nothing had happened. The check is first for that reason, not
    // for tidiness.
    if (!enabledRef.current) return;
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
    // `resetKey` IS A DEPENDENCY, AND THAT IS THE SECOND HALF OF ITS CONTRACT. The reset above is
    // synchronous — the value cannot survive a switch by even one frame — but the new subject still
    // has to be READ, and waiting for the next tick would leave the surface empty for up to a full
    // cadence after every switch. Re-running this effect reads at once, exactly as it does at mount,
    // and re-arms the timer for the new subject.
    //
    // A DISABLED READ DOES NOT RUN AT ALL: no read at mount, no timer. Turning it back on re-arms
    // this effect, which IS the read the caller asked for by turning it on — a caller that
    // deactivated the read while it had no subject (and left `refresh` alone, because its listener
    // is harmless) gets the mount read the moment it has one.
    if (!enabled) return;
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
  }, [refresh, everyMs, floorMs, enabled, resetKey]);

  return { data, read, refresh };
}
