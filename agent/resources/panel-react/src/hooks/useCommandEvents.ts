import { useEffect, useMemo, useRef, useState } from "react";
import { useDeviceRead } from "./useDeviceRead";
import { stripAnsi } from "../lib/ansi";
import type { ReadState } from "../lib/readState";

// Command event stream for one session (round-admin-ui Task 4): polls
// GET /api/sessions/{sid} — the audit JSONL (command/start → output →
// command/end, per session_log.rs SessionEvent) — and groups the raw events
// into command cards. The grouping mirrors the agent's round-99/100 terminal
// markers (session_log.rs recover_interrupted): a command is ENDED by
// command/end OR by a status of "backgrounded" / "closed" / "exited:N" — a
// backgrounded command never logs command/end, and a session that dies logs
// "closed" / "exited:<code>" instead. Other status values ("opened", …) are
// session-level and do not end a command.
//
// The cards feed the command card stream + details panel; all rendering
// downstream is TEXT-ONLY (never innerHTML).
//
// THE READ ITSELF IS `useDeviceRead`'S NOW (see its header): the read at mount, the ordering guard,
// the unmount guard, keep-last-on-failure and the cadence floor were hand-rolled here and are not any
// more. What this file keeps is the route, the fold, the three-word read state and the event-driven
// refreshes — and the two options the module grew for it (`enabled`, `resetKey`).

export interface CommandEvent {
  seq: number;
  ts: number;
  kind: string;
  command?: string;
  text?: string;
  exit_code?: number | null;
  reason?: string | null;
  status?: string | null;
  duration_ms?: number | null;
  /** command/start only: WHY the agent ran this, in its own words (optional). */
  intent?: string | null;
  /** command/start only: the alternatives it says it passed over. */
  considered?: string[] | null;
  /** command/start only: the 1-based plan step this command advances. */
  plan_step?: number | null;
  /** command/start only: the run this command was executed under, as the AI
   *  named it via `run_begin`. Present on the wire since runs were introduced;
   *  undeclared here until round 30, which is why the trail silently dropped an
   *  attribution the device had already recorded.
   *
   *  A LABEL, NEVER A CREDENTIAL (`runs.rs` pins this twice): it says which
   *  execution the AI CLAIMED this belonged to, and it is presented as that
   *  claim — never as a verified grouping. */
  run_id?: string | null;
}

export interface CommandCard {
  /** `c-<start seq>` — stable across polls (selection survives re-fetch). */
  id: string;
  seq: number;
  command: string;
  /** Accumulated output text (tail-capped, see MAX_OUTPUT_CHARS). */
  output: string;
  /** Unix seconds of command/start. */
  startedAt: number;
  ended: boolean;
  exitCode: number | null;
  reason: string | null; // marker / idle / timeout / interrupted / backgrounded / closed / exited:N
  durationMs: number | null;
}

// Per-card accumulation cap: a long-running command (tail -f, loops, binary
// streams) can produce unbounded output — the audit file itself caps each
// chunk at 4 KiB but not the total while the session is open. Keep the card's
// memory bounded; the TAIL wins (the newest output is what the operator
// needs to see; the audit file on disk still holds the head).
const MAX_OUTPUT_CHARS = 1_000_000;
// round-128: tail cap for the raw event array (see the fold below).
const MAX_RAW_EVENTS = 20_000;
const TRUNC_MARK = "\n…[output truncated — older lines dropped]…\n";
/** The value before the first read of a subject. ONE object, held here rather than built per render:
 *  the module hands this exact identity back on a `resetKey` change, and that is how the fold below
 *  recognises the first reply of a NEW session (see `reduce`). */
const NO_EVENTS: CommandEvent[] = [];

/** Map a status event's value to a command end, or null if session-level.
 *  Exported for the trajectory view (useTrajectory) — the round state uses
 *  the same round-99/100 terminal markers. */
export function terminalStatus(st: string): { exitCode: number | null; reason: string } | null {
  if (st === "backgrounded" || st === "closed") return { exitCode: null, reason: st };
  if (st.startsWith("exited:")) {
    const code = Number(st.slice("exited:".length));
    return { exitCode: Number.isFinite(code) ? code : null, reason: st };
  }
  return null;
}

function finishCard(start: CommandEvent, outputs: string[], ended: boolean, exitCode: number | null, reason: string | null, durationMs: number | null): CommandCard {
  // Raw SSE bytes carry ANSI/OSC control sequences — strip for text cards.
  let output = stripAnsi(outputs.join(""));
  if (output.length > MAX_OUTPUT_CHARS) output = TRUNC_MARK + output.slice(-MAX_OUTPUT_CHARS);
  return {
    id: `c-${start.seq}`,
    seq: start.seq,
    command: start.command ?? "",
    output,
    startedAt: start.ts,
    ended,
    exitCode,
    reason,
    durationMs,
  };
}

/** Group a session's raw audit events (in seq order) into command cards. */
export function groupEvents(events: CommandEvent[]): CommandCard[] {
  const cards: CommandCard[] = [];
  let start: CommandEvent | null = null;
  let outputs: string[] = [];

  for (const ev of events) {
    switch (ev.kind) {
      case "command/start": {
        // A new start while the previous command never ended — close it as
        // interrupted so it can't stay "running" forever (recovery appends
        // interrupted server-side, but a mid-stream start must not orphan
        // the prior card).
        if (start) cards.push(finishCard(start, outputs, true, null, "interrupted", null));
        start = ev;
        outputs = [];
        break;
      }
      case "output": {
        if (start && ev.text) outputs.push(ev.text);
        break;
      }
      case "command/end": {
        if (start) {
          cards.push(finishCard(start, outputs, true, ev.exit_code ?? null, ev.reason ?? null, ev.duration_ms ?? null));
          start = null;
          outputs = [];
        }
        break;
      }
      case "status": {
        if (!start || !ev.status) break;
        const term = terminalStatus(ev.status);
        if (term) {
          // Status ends carry no duration_ms — derive it from the event ts
          // (round-58 unit: ms).
          cards.push(finishCard(start, outputs, true, term.exitCode, term.reason, (ev.ts - start.ts) * 1000));
          start = null;
          outputs = [];
        }
        break;
      }
    }
  }
  // A trailing start with no end: still running (or the agent died before
  // recovery appended interrupted) — surface it as a LIVE card.
  if (start) cards.push(finishCard(start, outputs, false, null, null, null));
  return cards;
}

/**
 * How the last completed read of a session's audit log went — one of the three `ReadState`s.
 * The VOCABULARY lives in `lib/readState.ts` (one type for the three facts, shared with the
 * session archive and the views that draw an empty trail); what it means HERE is:
 *
 *   "reading"     — no completed read yet for this sid (initial, or just switched)
 *   "ok"          — the last read SUCCEEDED. Note this says nothing about how
 *                   many events came back: a successful read of an empty file is
 *                   "ok" with zero events, and that is a DIFFERENT fact from a
 *                   read that failed. Collapsing the two is how an unreadable
 *                   session renders as "this session recorded nothing".
 *   "unreadable"  — nothing readable is established for this sid: the device
 *                   says there is no readable record for it (`found:false` —
 *                   retention, another data dir, a stale id), or the read failed
 *                   and NO read of this sid has ever succeeded. A failure AFTER
 *                   a good read keeps "ok": the audit log is append-only, so the
 *                   events already in hand are still true.
 *
 * EVERY VIEW THAT RENDERS AN EMPTY TRAIL NEEDS THIS, not just the archive. It
 * said the live views "ignore it, exactly as they ignore the failed poll" — and
 * that is what they did, so they told the operator a session had run nothing
 * while the read was still in flight or had failed outright. `lib/trailRead.ts`
 * owns the wording; the field is REQUIRED on the slice `App` hands to them so a
 * mount cannot forget it again.
 *
 * THESE THREE WORDS ARE DERIVED, NOT STORED. The loop hands this reader the module's coarser `read`
 * (three words about the last READ), and the mapping below turns it into the three words above
 * (about what may be CLAIMED for this session) — see it for the two places they differ and for what
 * the module can and cannot say on its own.
 */

/**
 * READ the audit log of one session and return the RAW events (in seq order) together with the read
 * state above — the read `useCommandEvents` below (and, through it, the trajectory timeline) is
 * built on.
 *
 * THE LOOP IS `useDeviceRead`'S (see its header): the read at mount, the ordering guard, the unmount
 * guard, keep-last-on-failure and the cadence floor were hand-rolled here and are not any more. What
 * this reader still states for itself:
 *
 *   * IT DOES NOT POLL, AND `pollMs` IS STILL INERT — round 163's decision, kept. There is no
 *     timer: the reader is refetched when the session produces output and when the tab regains
 *     focus (the effect below), which is what the original did and what `pollMs` never affected.
 *     A migration of this reader's first cut passed `everyMs: pollMs` and silently re-added the
 *     timer; that is the shape to refuse, because the parameter's doc says in as many words that it
 *     has no effect on anything.
 *
 *   * NO SUBJECT, NO READ. `sid === ""` DISABLES the loop (`enabled`) rather than building
 *     `/api/sessions/`: nothing is selected, so nothing is asked. The empty string is the ONE
 *     spelling of "no session" this reader takes — see `useCommandEvents` below, which is where the
 *     `string | null` the panel actually holds is converted on the way in. The same option pair
 *     carries the switch (`resetKey: sid`), which resets the value, the read state and any in-flight
 *     read during render.
 *
 *   * A FAILED read (tunnel blip, agent restarting) keeps the last good events instead of blanking
 *     the stream (same stance as useSessions' poll) — while the READ STATE stays this reader's own
 *     question, because "the trail could not be read" and "the session recorded nothing" are the two
 *     facts its views exist to keep apart.
 */
function useSessionEventsWithState(
  sid: string,
  pollMs = 2000,
): { events: CommandEvent[]; readState: ReadState; firstSeq: number } {
  // WHERE THE RECORD BEGINS, as the device reports it. The trail is trimmed to
  // ~2000 lines when a session closes, so a long session's head is discarded by
  // design — `firstSeq > 1` is the only signal, and a viewer that ignores it
  // presents a trimmed trail as the whole story.
  //
  // IT IS NOT RESET ON A SWITCH, deliberately: that is what this reader did before the loop moved
  // (the render-time reset cleared the events, the read state and the watermark, not this), and the
  // new session's first reply replaces it. Stated because the reset block that used to sit below
  // made the omission visible, and the module cannot make it for a value it does not hold.
  const [firstSeq, setFirstSeq] = useState(1);
  // IS THERE A READABLE RECORD FOR THIS ID? The device answers the question the client used to have
  // to hedge: `found:false` means there is no readable record for this id (retention, another data
  // dir, a stale id), which is NOT the same as a session that recorded nothing. Without it the
  // viewer would draw an empty trail for a session whose file is simply gone. It is STATE because
  // the render reads it (the mapping below), and it is only authoritative when the field is
  // PRESENT — an older agent omits it, and then the HTTP-level success is all we know.
  //
  // LIKE `firstSeq`, IT IS NOT RESET ON A SWITCH — and unlike `firstSeq` that needs saying, because
  // this one is READ during render. Every render that reads it has either just folded a reply for
  // the CURRENT subject (which is what sets it) or is deciding "unreadable" without consulting it
  // (a failure before any success of this subject), so a previous session's value is never drawn.
  const [found, setFound] = useState(true);
  // WHERE THE LAST ACCEPTED REPLY LEFT THE WATERMARK. The audit log is append-only and `seq` is
  // per-session monotonic (seeded from the file's max seq on agent restart), so the watermark only
  // ever moves forward while a file is appended. A ref, because it is read inside the fold and
  // written there — and because the module's subject reset is what re-bases it, see `reduce`.
  const lastSeqRef = useRef(0);
  // HAS ANY READ OF THIS SUBJECT EVER SUCCEEDED? A failed later poll must not overwrite a known-good
  // state (and with an empty-but-readable session, `events.length` cannot answer this — hence a flag
  // of its own). It is MIRRORED FROM THE MODULE'S `read` rather than set at the call site, because
  // the module is where the fact lives now: `"ok"` means its last settle was a successful fold, and
  // `"reading"` means NO READ OF THIS SUBJECT HAS SETTLED — the state a `resetKey` change puts it
  // back to, which is what clears this flag for the new subject in the same render. `"unreadable"`
  // leaves it ALONE on purpose: erasing the memory of an earlier success is exactly what would make
  // a later poll failure render as a session that recorded nothing.
  const sawOkRef = useRef(false);

  // THE LOOP IS `useDeviceRead`'S (see its header): mount read, ordering guard, unmount guard,
  // keep-last-on-failure and the cadence floor. What is left here is this reader's own — the route
  // built per read, the fold, the two facts the fold publishes beside the events, and the
  // event-driven refreshes below.
  //
  // round-138 IS NOT ANSWERED HERE ANY MORE, AND ITS RECORD STAYS. A slow poll for the OLD session
  // used to resolve after the user switched and land the old session's events under the new tab,
  // poisoning its seq watermark (the new session's polls then always returned "nothing new"). That
  // check lived here as a `sidRef` re-check after the `await`; it is TWO of the module's rules now —
  // the ordering guard drops a reply that is no longer the newest, and `resetKey: sid` abandons in
  // flight exactly the reply a switch makes stale. The copy is gone on purpose: a second check here
  // would be a second answer to a question the module has already answered.
  const { data: events, read, refresh } = useDeviceRead<CommandEvent[]>({
    // THE ROUTE IS BUILT AT READ TIME, from the sid in hand — a captured string would pin the FIRST
    // session forever and every later switch would keep reading the session it started on. (The
    // module resolves a function path per read, which is why the function form exists.)
    path: () => `/api/sessions/${encodeURIComponent(sid)}`,
    reduce: (prev, body) => {
      const res = body as {
        found?: unknown;
        first_seq?: unknown;
        events?: unknown;
      } | null;
      // THIS BODY IS A COMPLETED READ — the module already decided that: a refusal and a throw never
      // reach a `reduce` at all (see `useDeviceRead`), so there is no `deviceRefused` check here,
      // and `found` below is a fact about a session rather than about the transport.
      const foundNow = typeof res?.found === "boolean" ? res.found : true;
      // Absent on an older agent: default to 1, which claims nothing.
      const fs = typeof res?.first_seq === "number" ? res.first_seq : 1;
      // BOTH ARE PUBLISHED BEFORE THE "NOTHING NEW" GUARD BELOW, which is about re-rendering rather
      // than about whether the device answered anything new: a quiet session whose record appeared
      // (or vanished) must still be able to say so.
      setFound(foundNow);
      setFirstSeq((prevSeq) => (prevSeq === fs ? prevSeq : fs));
      const evs: CommandEvent[] = res && Array.isArray(res.events) ? res.events : [];
      // THE MODULE OWNS THE SUBJECT RESET, AND THIS IS HOW THIS FOLD LEARNS OF IT. On a `resetKey`
      // change the module puts the value back to `initial` — the very object this reader passed in —
      // so a fold that is handed it is the FIRST fold of a NEW subject and the watermark must start
      // from zero. The new session's seqs are lower (seq is per-session), so a watermark kept from
      // the previous session would make every reply look like "nothing new" and its trail would
      // never appear. No `sid` diff here: the module already decided, and this reads its decision.
      if (prev === NO_EVENTS) lastSeqRef.current = 0;
      let maxSeq = 0;
      for (const e of evs) maxSeq = Math.max(maxSeq, e.seq || 0);
      // `<=` not `===`: a degraded response (empty/malformed events, transient blip that returns a
      // 200 shell) must NOT rewind the watermark and blank the stream — the audit log only grows.
      // AND NOTHING NEW MEANS THE SAME OBJECT BACK: the module writes what this returns straight
      // into its state, so returning `prev` itself is the render-skipping contract (a fresh array
      // every 2 s would re-render the cards, the trajectory and everything around them for no new
      // fact). The two facts above are their own state and re-render only when one changed.
      if (maxSeq <= lastSeqRef.current) return prev; // nothing new
      lastSeqRef.current = maxSeq;
      // round-128/129: tail-cap the RAW events — a chatty long-lived
      // session (serial console ~11.5KB/s → ~40MB/hour) grew browser
      // memory unbounded. The cap respects ROUND BOUNDARIES: a command
      // still running at the cut point must keep its command/start, or
      // groupEvents drops the whole live card (output without a start is
      // discarded) and the trajectory relabels it '(session)'.
      let tail = evs;
      if (tail.length > MAX_RAW_EVENTS) {
        const cut = tail.length - MAX_RAW_EVENTS;
        // Walk back to the newest command/start at or before the cut
        // (keep a running round's start so the live card isn't orphaned).
        let start = cut;
        for (let i = cut; i >= 0; i--) {
          if (tail[i].kind === "command/start") { start = i; break; }
        }
        tail = tail.slice(start);
        // round-131: if the anchored round STILL exceeds the cap (one
        // command running for the whole window — no newer start ever
        // appears), drop the round's OLDEST output events, keeping the
        // start (groupEvents/groupRounds need it for the live card).
        // The audit file on the server still holds the full history.
        if (tail.length > MAX_RAW_EVENTS) {
          const keep = tail.filter((e, i) => i === 0 || i >= tail.length - MAX_RAW_EVENTS);
          tail = keep;
        }
      }
      return tail;
    },
    // The identity the module hands back on a subject change — ONE object, held here rather than
    // built per render, because that identity is how the fold above learns the subject changed.
    initial: NO_EVENTS,
    // NO CADENCE, WHICH IS ROUND 163'S DECISION RESTORED. This migration's first version passed
    // `everyMs: pollMs` (with a 2 s floor), which RE-ADDED the timer round 163 removed: the reader
    // would have gone back to dialling the device every 2 s for every open session page, for a value
    // the original's own doc calls inert — "Not 'poll': there is no timer — the 5 s cadence was
    // removed in round 163, and `pollMs` survives only as an effect dependency, so a caller-supplied
    // value has no effect on anything." A read at mount plus the event-driven refreshes below IS this
    // reader's cadence, and the `status` events that carry no output are what the timer once stood in
    // for. `pollMs` therefore stays exactly what it was: a parameter with no effect on anything.
    //
    // NO SUBJECT, NO READ: no read at mount and `refresh()` (which the listeners below call
    // unconditionally) does nothing. And `resetKey` is the switch: value, read state and in-flight
    // read are reset DURING RENDER on a sid change — clearing the events, re-basing the watermark and
    // reporting "reading" synchronously, so no frame shows the old session's trail under the new
    // session's tab and the new session's lower seqs are not skipped.
    enabled: sid !== "",
    resetKey: sid,
  });

  // THE MODULE'S `read`, IN THIS READER'S WORDS. The link is a mirror, not a copy: reading the
  // module's transitions is what tells the flag above which subject it belongs to.
  if (read === "reading") sawOkRef.current = false;
  else if (read === "ok") sawOkRef.current = true;

  // WHAT THE SURFACE MAY CLAIM about this session's trail, which is the question its views ask (see
  // the `ReadState` doc above). The module's three words answer a narrower one — how the last read
  // went — and the two places they are not enough are exactly the two rules this reader has always
  // had: `found:false` is a SUCCESSFUL read that establishes nothing readable (so the last success's
  // answer is overridden), and a failure must keep the last success's answer when there WAS one.
  //
  // THE MODULE CANNOT SAY "NO READ OF THIS SUBJECT HAS EVER SUCCEEDED" ON ITS OWN — checked, not
  // assumed: a failed settle reports `"unreadable"` whether or not an earlier one succeeded, so the
  // word is one word for two facts. What it CAN say, and does, is which transition it just made
  // (`"reading"` → nothing has settled; `"ok"` → a fold succeeded), which is all the flag above
  // needs; the distinction is carried by the module's transitions rather than by a second state
  // machine beside them.
  const readState: ReadState =
    read === "reading"
      ? "reading"
      : read === "unreadable" && !sawOkRef.current
        ? "unreadable"
        : found
          ? "ok"
          : "unreadable";

  // THE EVENT-DRIVEN REFRESHES, which are this reader's own and are its ONLY cadence: the module
  // takes no `everyMs` here (see the call above), so "because this session just produced output,
  // look now" and "because the tab came back" are the two reasons this reader ever re-reads.
  useEffect(() => {
    // Nothing to watch while nothing is selected: the module is disabled in that state anyway
    // (`enabled: sid !== ""`), and this keeps the old shape — no listeners, no tick.
    if (!sid) return; // nothing to poll; clearing already happened on switch
    // round-163 TOOK THE TIMER OUT OF HERE, and that stays in view because it was a real decision:
    // the audit log refetches when THIS session produced terminal output (debounced) or the tab
    // regained focus, and a silent command still lands its cards on the next output burst. Both
    // paths are below and both are unchanged. WHAT THE TIMER ONCE STOOD IN FOR — a `status` event
    // carrying no output, so no burst ever arrives to fetch it — is not restored here: re-adding a
    // cadence to fix it is a different change from this migration, and round 163 made the call.
    let timer: number | undefined;
    // SPA audit MED-3: the pure trailing-edge debounce STARVED under
    // continuous output (every burst byte-stream re-armed the 1s timer —
    // during builds/serial floods the cards froze). Max-wait pattern: a
    // pending refresh older than 5s runs immediately.
    let armedAt = 0;
    const onOutput = (e: Event) => {
      const from = (e as CustomEvent).detail?.sid;
      if (from !== sid) return; // another session's output
      const now = Date.now();
      if (!armedAt) armedAt = now;
      if (timer) window.clearTimeout(timer);
      if (now - armedAt >= 5000) {
        armedAt = 0;
        timer = undefined;
        void refresh();
        return;
      }
      timer = window.setTimeout(() => {
        armedAt = 0;
        timer = undefined;
        void refresh();
      }, 1000);
    };
    const onVisible = () => { if (document.visibilityState === "visible") void refresh(); };
    window.addEventListener("summrise-term-output", onOutput);
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.removeEventListener("summrise-term-output", onOutput);
      document.removeEventListener("visibilitychange", onVisible);
      if (timer) window.clearTimeout(timer);
    };
    // `sid` IS A DEPENDENCY because the listener above compares against the session it was armed
    // for: the comparison used to read a `sidRef` mirror, and a closure over the current `sid`
    // answers the same question without a ref that exists only to be re-pointed each render. The
    // effect re-registers on a switch, which is what the old one did too (`[sid, pollMs]`).
    //
    // AND `pollMs` IS STILL IN THE LIST, exactly as inert as it was there: it is not a cadence (see
    // the call above), it is a parameter this signature keeps because removing it is a wider change
    // than correcting the sentence describing it — and leaving it OUT of the dependencies would be
    // the half-measure, a parameter documented as "an effect dependency" that is not one.
  }, [refresh, sid, pollMs]);

  return { events, readState, firstSeq };
}

// REMOVED 2026-09-24 (the panel exploration): `useSessionEvents(sid, pollMs)`, a two-line forwarder
// to `useSessionEventsWithState(...).events`. Its doc called it a compatibility shim — "kept as its own
// export so adding the read state could not change what an existing consumer sees" — and that consumer
// no longer exists: nothing in src/ or the tests called it, and its only mentions were PROSE in two
// comments. `exports-check` counted those words as uses, which is why it survived six rounds of audits;
// the check strips comments before counting now.

/**
 * Command card stream for one session: READ the audit log (the shared read above) and
 * group the raw events into cards. A FAILED read keeps the last good cards
 * instead of blanking the stream.
 *
 * NOTHING HERE IS POLLED *BY THIS FUNCTION*, which is what the two sentences above used to deny
 * ("Cards update every poll", "a FAILED poll"): the schedule belongs to the read above, whose loop
 * is `useDeviceRead`'s. The 5 s cadence was removed in round 163 and the reader is now polled at the
 * 2 s `pollMs` names, on top of the SSE-driven path in `App` and the reader's own effect — so
 * `pollMs` IS the cadence again rather than an inert dependency, and it is passed straight through.
 */
export function useCommandEvents(sid: string | null, pollMs = 2000) {
  // round-128: the raw events are exposed so the trajectory view reuses THIS
  // read instead of mounting a second one (double fetch every 2s). `readState`
  // is the third thing the same read knows (see `ReadState`) — the archive
  // viewer needs it to tell an empty trail from an unreadable one.
  //
  // `null` BECOMES `""` HERE, ONCE, at the public entry point: the panel holds "no session
  // selected" as `null` (see `App`), the reader takes the empty string — and no route is built
  // from it, because the reader disables the loop in exactly that state (`enabled`). Converting
  // here rather than deep inside keeps one spelling on each side of the seam.
  const { events, readState, firstSeq } = useSessionEventsWithState(sid ?? "", pollMs);
  const cards = useMemo(() => groupEvents(events), [events]);
  return { cards, events, readState, firstSeq };
}
