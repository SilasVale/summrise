import { useCallback, useEffect, useRef, useState } from "react";
import { callApi, callTool } from "../lib/api";

// Session state (migrated from panel.js) — the xterm instance + render
// cursor live OUTSIDE React state (imperative, heavy); React tracks only the
// session list + metadata. Those live in TerminalPane, as refs; this file used
// to claim a `runtimes` ref map held them, which was never true and is gone.

/** Map the agent's snake_case pending_approval onto the camelCase shape React
 *  uses. One helper because THREE call sites need it (first sight, revive,
 *  refresh) and three hand-written mappings would drift — the panel would then
 *  show a prompt for a command the agent had already released. */
/** Grants arrive as a plain array of first words. Anything that is not a
 *  non-empty string is dropped rather than rendered: the UI must never show a
 *  grant it could not revoke by the same string. */
/** A goal is a non-empty string or nothing. An empty or whitespace-only value
 *  from the server would render as a blank objective, which is worse than no
 *  objective: it looks like one was set. */
function mapGoal(s: any): string | null {
  const g = s?.goal;
  return typeof g === "string" && g.trim().length > 0 ? g : null;
}

/** The plan arrives as an array of step labels; same filtering discipline as
 *  grants, so the UI never renders a step it could not match back. */
function mapPlan(s: any): string[] {
  const p = s?.plan;
  if (!Array.isArray(p)) return [];
  return p.filter(
    (x: unknown): x is string => typeof x === "string" && x.length > 0,
  );
}

function mapGrants(s: any): string[] {
  const g = s?.approval_grants;
  if (!Array.isArray(g)) return [];
  return g.filter(
    (x: unknown): x is string => typeof x === "string" && x.length > 0,
  );
}

/** A question the device is holding open for a person to answer.
 *
 *  `expiresAtMs` is an ABSOLUTE wall-clock deadline, not the device's remaining
 *  budget. The device reports `expires_in_ms` — a countdown that SHRINKS on
 *  every read — and a component that keeps that number while also accumulating
 *  its own elapsed time counts the same seconds twice: with the old 60 s block
 *  a 2 s poll made the display fall at roughly double speed, which is fatal at
 *  the gate's real ~15-minute TTL (the operator would be told the question had
 *  minutes left when it had half an hour, or the reverse). Converting ONCE, at
 *  the edge where the wire shape is read, leaves the display with one honest
 *  number that only the clock moves. */
export interface PendingApproval {
  id: string;
  command: string;
  expiresAtMs: number;
}

export function mapPending(s: any): PendingApproval | null {
  const p = s?.pending_approval;
  if (!p || typeof p.id !== "string") return null;
  const budget =
    typeof p.expires_in_ms === "number" && Number.isFinite(p.expires_in_ms)
      ? Math.max(0, p.expires_in_ms)
      : 0;
  return {
    id: p.id,
    command: typeof p.command === "string" ? p.command : "",
    expiresAtMs: Date.now() + budget,
  };
}

/** How many sessions are holding a question for the operator right now.
 *
 *  Keyed on `pendingApproval`, NEVER on `approvalRequired`: the gate being armed
 *  is a standing posture (every command will ask), while a pending approval is
 *  an actual decision waiting. A badge driven by "armed" would make every armed
 *  session shout permanently — and an indicator that is always on is one nobody
 *  reads. Closed tombstones are excluded: their question is history. */
export function pendingApprovalCount(sessions: Session[]): number {
  return sessions.filter((s) => !s.closed && s.pendingApproval).length;
}

export interface Session {
  sid: string;
  label: string;
  kind: string;
  closed: boolean;
  savedOnly: boolean;
  active: boolean;
  /** HOW LONG THE DEVICE SAYS THIS SESSION HAS BEEN SILENT (ms).
   *
   *  The device's `terminal_list` row carried NO time of any kind until round 37 added `idle_ms`
   *  (the same round that found sixteen sessions silent for up to eleven hours behind a dead idle
   *  sweeper). `firstSeenAt` below is still only "when this panel first saw the row" — that was
   *  never the session's age — but idle time is now a FACT the device measured, which is what lets
   *  the panel offer to close what nobody is using. */
  idleMs: number;
  /**
   * A COMMAND IS IN FLIGHT IN THIS SESSION — the device's own `busy` flag, which its execute wait-loop sets around
   * the command it is waiting for (round 28 of the standing goal).
   *
   * THE PANEL ALREADY DERIVES THIS, for the one session whose trail it has loaded ("a trailing command/start with
   * no end: still running"), and could not for any other: the rail and the other tabs decided "working" from
   * `idleMs`, which is OUTPUT RECENCY — so a command that runs silently read as idle in exactly the case this
   * device exists for. The device has known all along; the row says so now.
   */
  commandRunning: boolean;
  /** THE EXIT CODE OF THE LAST COMMAND THIS SESSION FINISHED, when the shell's marker reported one
   *  (round 96). `null`/absent means NO CODE WAS OBSERVED — no command yet, or the last wait ended
   *  without a marker (a timeout or a partial read), or an ssh/serial session, which has no marker
   *  injection at all.
   *
   *  WHY IT IS ON THE WIRE RATHER THAN DERIVED HERE. The panel can read a failure out of the audit
   *  trail — `cardState` maps an exit code to ok/fail — but only for the session whose trail is
   *  loaded, so the rail and every other tab could not say it. `liveness.ts` names this exact hole:
   *  "'Failed' is not here because no field reports it per session — inventing a state would put a
   *  shape on the screen that nothing can ever mean." The device has reported one since round 96.
   *
   *  THREE STATES, AND THE DEVICE CLEARS IT WHEN A NEW COMMAND IS WRITTEN, so a surface must not
   *  keep its own memory of a failure: a row that is absent means "nothing to say", not "fine". */
  lastExitCode?: number | null;
  /** WHEN THIS PANEL FIRST SAW THE SESSION — not when it opened.
   *
   *  This field was called `openedAt` and rendered as the session's AGE, which
   *  the panel cannot know: the device's `terminal_list` row carries no OPEN
   *  timestamp (`agent/src/tools/terminal/mod.rs` — it reports `idle_ms` since
   *  round 37, which is a different fact), so the value is
   *  `Date.now()` at the moment the panel discovered the row. A page reload
   *  therefore stamped every already-running session as "now", and the 30 s sweep
   *  did the same for each new row — a session open for hours read as seconds old.
   *
   *  The value is useful and TRUE as "first seen by this panel"; only the LABEL
   *  was false. Renamed rather than removed so every consumer had to be
   *  reconsidered, which is how the two render sites were found. */
  firstSeenAt: number;
  closedAt: number | null;
  /** A PERSON holds this session's keyboard (control handoff). Server-owned
   *  state mirrored here: the agent refuses `terminal_execute` while it is set,
   *  so the panel must SHOW it or the operator cannot tell why the AI stopped. */
  heldByHuman: boolean;
  /** The session is ARMED: every execute waits for a decision. Server-owned. */
  approvalRequired: boolean;
  /** The command currently blocked at the gate, if any. Present only while a
   *  decision is actually being waited for — the agent clears it on every exit
   *  path, so a rendered prompt is always a live question. */
  pendingApproval: PendingApproval | null;
  /** First words allowed without asking. Server-owned and derived from commands
   *  the operator approved, so the panel's job is to SHOW them: a grant nobody
   *  can see is one nobody can judge or revoke, and these decide what runs. */
  approvalGrants: string[];
  /** What the operator asked this session to achieve, if they said. Server-owned:
   *  the goal is the anchor a run is judged against, so the panel must show the
   *  stored value rather than whatever was last typed. */
  goal: string | null;
  /** The AGENT's declared plan: what it intends to do, in order. Distinct from
   *  `goal`, which is the operator's. Showing both is what makes a divergence
   *  visible — the plan says five steps, the path shows three plus two nobody
   *  announced. */
  plan: string[];
}

// REMOVED 2026-09-24 (the panel exploration's F2): `interface SessionRuntime` and
// `const runtimes = new Map<string, SessionRuntime>()`. The map was DECLARED, RETURNED from this hook
// and never written or read by anything — `term`, `fit`, `container`, `renderedBytes`, `needSync` and
// `sseDirty` had no producers at all — while the file header described it as the design ("The term
// object is attached to a ref map so the render cycle never re-creates it"). The xterm instance and the
// render cursor actually live in TerminalPane, where they are refs. It shipped in the bundle, and no
// gate could see it: `exports-check` reads EXPORTS, and this was a module-level const.

// P1-4: export downloads at most this many 1 MiB pages (see exportSession).
const MAX_EXPORT_PAGES = 16;

/** How often the panel re-lists sessions with no event to prompt it. Named once: this was a bare 30_000 inside the
 *  effect plus a "30 s" in two comments, which is how one number becomes three that can disagree. */
const SESSIONS_SWEEP_MS = 30_000;

/** ONE MAPPING FROM A DEVICE ROW TO A SESSION ROW (round 96). This 7-line object literal appeared TWICE in this file —
 *  byte-identical apart from indentation — which is the spine's defect in its most literal form: one fact (how a device
 *  row becomes a session row) written down twice, free to drift the moment one of them gains a field. It is also the
 *  place the three "duplicate-looking" controls get their three DIFFERENT facts (`held_by_human`, `approval_required`,
 *  `goal`), so a second copy is where they could quietly become one. */
/** EVERY FIELD THAT IS THE DEVICE'S TO SAY, defined once (round 97). `mapRow` builds a whole row; the two paths that
 *  REFRESH an existing one — reviving a tombstone whose sid reappeared, and syncing a hold that changed without an event —
 *  need exactly this part and must not restate it. They did: the same seven fields were written a second and a third
 *  time, so a field added to the wire would have reached a new row and neither of the refreshed ones. */
// THE TWO FIELDS THAT SAY WHETHER A COMMAND IS RUNNING, mapped in ONE place because both the
// discovery path (`mapRow`) and every refresh path (`wireFields`) must agree on them.
//
// THEY WERE MISSING FROM `wireFields` UNTIL 2026-09-24, and nothing failed: the list's own promise —
// "a field added to the wire would have reached a new row and neither of the refreshed ones" — reads
// as satisfied, because a field the list omits reaches a NEW row and neither refreshed one. So
// `idleMs` and `commandRunning` were written once at discovery and then frozen, while three consumers
// treat them as live: `sessionActive` (a panel-opened session is stamped `idleMs: 0` and reads
// "working" for ever), `anyCommandRunning` (the rail mark shows the device as NOT working during a
// silent long command — the exact blindness round 28 fixed) and `idleSessions`' offer-to-close, whose
// `!commandRunning` guard could never fire. The panel exploration found it; `session-row-check.mjs`
// disclaims precisely this property ("or whether `wireFields` is used everywhere it should be").
const liveFields = (s: any) => ({
  idleMs: typeof s.idle_ms === "number" ? s.idle_ms : 0,
  commandRunning: !!s.command_running,
});

const wireFields = (s: any) => ({
  ...liveFields(s),
  heldByHuman: !!s.held_by_human,
  approvalRequired: !!s.approval_required,
  pendingApproval: mapPending(s),
  approvalGrants: mapGrants(s),
  goal: mapGoal(s),
  plan: mapPlan(s),
  lastExitCode: typeof s.last_exit_code === "number" ? s.last_exit_code : null,
});

/** Does a refreshed row carry anything the stored one does not already have?
 *
 *  THIS LIST HAS TO STAY EXPLICIT, and the reason is a fact about one of its members:
 *  `wireFields`'s `pendingApproval` is DERIVED at map time — `mapPending` turns the device's shrinking
 *  budget into an absolute deadline — so comparing the mapped objects would report a change on every
 *  poll and pull that deadline earlier each time. The "obvious" generalisation (compare mapped objects,
 *  so a field can never be forgotten) was tried on 2026-09-24 and broke exactly that test. Hence the
 *  narrow `pendingApproval?.id` below: identity may change, the derived deadline may not.
 *
 *  AND THE LIST IS WHERE THE BUG WAS. It omitted `idleMs` and `commandRunning` while `wireFields` did
 *  too, so a refresh neither carried them nor noticed them — a row kept its discovery values for life,
 *  and three consumers read them as live (`sessionActive`, the rail's `anyCommandRunning`,
 *  `idleSessions`' offer-to-close). CARRY AND DETECT ARE TWO LISTS; fixing the bug needed both. */
const wireFieldsChanged = (
  existing: any,
  fresh: ReturnType<typeof wireFields>,
): boolean =>
  existing.heldByHuman !== fresh.heldByHuman ||
  existing.approvalRequired !== fresh.approvalRequired ||
  (existing.lastExitCode ?? null) !== fresh.lastExitCode ||
  existing.pendingApproval?.id !== fresh.pendingApproval?.id ||
  existing.approvalGrants.join("\u0000") !==
    fresh.approvalGrants.join("\u0000") ||
  existing.goal !== fresh.goal ||
  existing.plan.join("\u0000") !== fresh.plan.join("\u0000") ||
  existing.idleMs !== fresh.idleMs ||
  existing.commandRunning !== fresh.commandRunning;

const mapRow = (s: any) => ({
  sid: s.id,
  label: s.label || s.id,
  kind: s.kind || "pty",
  closed: false,
  savedOnly: false,
  active: false,
  // `idleMs` and `commandRunning` arrive through `wireFields` below — the same mapper the refresh
  // paths use, which is the point: they used to be written here only, and frozen at their first
  // value for the life of the row.
  firstSeenAt: Date.now(),
  closedAt: null,
  ...wireFields(s),
});

export function useSessions(connected: boolean) {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [activeSid, setActiveSid] = useState<string | null>(null);
  const [status, setStatusState] = useState("");
  // round-94: a live mirror of activeSid for async callbacks (closeSession
  // awaits terminal_close, during which the user can activate another tab —
  // the closed-over activeSid was stale and stomped that activation).
  const activeRef = useRef<string | null>(null);
  activeRef.current = activeSid;

  // round-163: the 3s terminal_list POLL is gone. The list refreshes on:
  // connect (initial), an agent-pushed `sessions-changed` SSE event (emitted
  // by terminal_open/close), tab refocus, and A SLOW BACKGROUND SWEEP
  // (SESSIONS_SWEEP_MS below).
  //
  // THE FOURTH WAS MISSING FROM THIS LIST UNTIL ROUND 50, and the way it went missing is the point: round 163 wrote
  // three triggers and was RIGHT, round 245 added the sweep and documented it at the interval itself — and this list,
  // forty lines above, quietly became incomplete. A reader deciding whether a refresh would arrive would have
  // believed it. During an SSE outage the UI shows "reconnecting" anyway; the sweep is what covers the gaps.
  useEffect(() => {
    if (!connected) return;
    const tick = async () => {
      // round-245 (terminal-display audit HIGH-1): the refresh contract was
      // ONE fire-and-forget terminal_list after each sessions-changed event.
      // A transient failure (tunnel blip, agent mid-restart) swallowed the
      // event and the AI-opened session NEVER appeared — no later event
      // exists to retry it (the agent emits sessions-changed only on
      // open/close/death). Retry the list once after a short delay.
      for (let attempt = 0; attempt < 2; attempt++) {
        let list: any = null;
        try {
          // round-113: a FAILED poll (tunnel blip, agent restarting) used to
          // return [] and tombstone EVERY open session — the heartbeat then
          // skipped them and the agent's 15-min sweeper reaped them while the
          // user watched. Only a SUCCESSFUL list may mark sessions gone.
          list = await callTool("terminal_list");
          if (!Array.isArray(list)) return; // tool error surfaced as non-array → give up
        } catch {
          // Transient failure — retry once, then give up (the background
          // sweep below still covers the gap).
          if (attempt === 0) {
            await new Promise((r) => setTimeout(r, 1200));
            continue;
          }
          return;
        }
        const seen = new Set(list.map((s: any) => s.id));
        setSessions((prev) => {
          const next = [...prev];
          for (const s of list as any[]) {
            const existing = next.find((x) => x.sid === s.id);
            if (!existing) {
              next.push(mapRow(s));
            } else if (existing.closed) {
              // round-245 (terminal-display audit HIGH-1): REVIVE a tombstone
              // whose sid reappears live. A fast AI session (open → one
              // command → exit) used to be tombstoned by a list that raced
              // the agent's close emit, and NOTHING ever revived it — the tab
              // sat dead forever (activate() refuses closed entries). A live
              // reappearance means the session is real: un-tombstone it.
              const revived = {
                ...existing,
                closed: false,
                closedAt: null,
                ...wireFields(s),
              };
              next[next.indexOf(existing)] = revived;
            } else if (wireFieldsChanged(existing, wireFields(s))) {
              // The hold is server-owned and can change WITHOUT a sessions-changed
              // event (this panel's own control button, or another client).
              // Syncing it here is what keeps the indicator honest. Placed AFTER
              // the revive branch on purpose: a closed tombstone whose hold
              // differs must still be REVIVED, not merely have its flag synced.
              next[next.indexOf(existing)] = { ...existing, ...wireFields(s) };
            }
          }
          // Mark gone sessions closed (retained history shows as tombstone).
          // round-88: a session that died server-side (PTY exit, SSH drop,
          // serial error) must ALSO release focus — the R86 close-switching
          // only covered the ✕ path, so a dead active tab kept the blinking
          // cursor and swallowed keystrokes.
          // review #6: immutable pass — the old loop MUTATED objects still
          // referenced by the previous state array (breaks memo/batching
          // contracts; React 18 double-invoke shows stale tabs).
          let deadActive = false;
          const next2 = next.map((x) => {
            if (!seen.has(x.sid) && !x.savedOnly) {
              if (x.active) deadActive = true;
              return {
                ...x,
                closed: true,
                closedAt: x.closedAt || Date.now(),
                active: false,
              };
            }
            return x;
          });
          let out = next2;
          if (deadActive) {
            const nextLive = out.find((s) => !s.closed);
            if (nextLive) {
              setActiveSid(nextLive.sid);
              out = out.map((x) =>
                x.sid === nextLive.sid ? { ...x, active: true } : x,
              );
            } else setActiveSid(null);
          }
          // round-117: cap the tombstone count — every session the device
          // ever hosted (PTY/SSH churn, other clients) accumulated forever,
          // growing the tab bar and the per-tick O(m) scan. Keep the newest
          // 32 closed entries; older ones are dropped (their history is
          // still readable server-side via the sessions dir).
          const closed = out.filter((s) => s.closed);
          if (closed.length > 32) {
            const drop = new Set(
              closed.slice(0, closed.length - 32).map((s) => s.sid),
            );
            return out.filter((s) => !drop.has(s.sid));
          }
          return out;
        });
        return; // success — done
      }
    };
    tick();
    const onChange = () => {
      tick();
    };
    window.addEventListener("summrise-sessions-changed", onChange);
    document.addEventListener("visibilitychange", onChange);
    // round-245 (HIGH-1): a slow background sweep (SESSIONS_SWEEP_MS) that ONLY ADDS live
    // sessions the panel has never seen — the safety net when both the
    // event-driven refetch AND its retry failed. It must never tombstone
    // (tombstoning is the event path's job, where the agent's close emit
    // proves the session died).
    const sweep = window.setInterval(async () => {
      try {
        const list = await callTool("terminal_list");
        if (!Array.isArray(list)) return;
        setSessions((prev) => {
          // Only add never-seen live sessions + auto-activate the newest when
          // nothing is active — never tombstone here (that is the event
          // path's job, where the agent's close emit proves death).
          const missing = (list as any[]).filter(
            (s) => !prev.some((x) => x.sid === s.id),
          );
          const next = [...prev];
          for (const s of missing) {
            next.push(mapRow(s));
          }
          if (
            !prev.some((x) => x.active) &&
            next.some((x) => !x.closed && x.active === false)
          ) {
            const liveTail = next.filter((x) => !x.closed);
            const target = liveTail[liveTail.length - 1];
            if (target) {
              setActiveSid(target.sid);
              return next.map((x) =>
                x.sid === target.sid ? { ...x, active: true } : x,
              );
            }
          }
          return next;
        });
      } catch {
        /* transient — next sweep */
      }
    }, SESSIONS_SWEEP_MS);
    return () => {
      window.removeEventListener("summrise-sessions-changed", onChange);
      document.removeEventListener("visibilitychange", onChange);
      window.clearInterval(sweep);
    };
  }, [connected]);

  // FAST POLL while the gate is armed — SEPARATE from the effect above, because
  // it must be able to start and stop as the mode changes without tearing down
  // the event listeners.
  //
  // WHAT IT IS FOR, now that the question lives for the gate's full TTL
  // (~15 minutes) instead of a one-minute block: DISCOVERY, not rescue. The 30 s
  // background sweep would still find a question eventually, but "eventually" is
  // up to 30 s of the operator staring at a session that is already waiting —
  // and the decision is theirs to make in the first seconds. 2 s also notices
  // RETIREMENT promptly, so an expired question clears instead of sitting there.
  // (The old comment here justified 2 s as "well inside the agent's one-minute
  // window"; that window is gone, and the reason above is the one that survived.)
  //
  // Gated on being armed, so an idle panel still polls nothing — the same "poll
  // only when needed" discipline as round-163, which removed a 3 s poll in
  // favour of events.
  const armed = sessions.some((s) => s.approvalRequired);
  useEffect(() => {
    if (!armed) return;
    // Re-fires the SAME event the agent's SSE pushes, rather than calling the
    // refresh directly: the listener above already owns the retry, the tombstone
    // and the revive rules, and a second call path would be a second copy of them.
    const fast = window.setInterval(() => {
      window.dispatchEvent(new CustomEvent("summrise-sessions-changed"));
    }, 2000);
    return () => window.clearInterval(fast);
  }, [armed]);

  const setStatus = useCallback((msg: string) => setStatusState(msg), []);

  // A KEYSTROKE THAT DID NOT LAND must say so. `TerminalPane` dispatches `summrise-write-failed` when a
  // `terminal_write` rejects, and until round 94 NOTHING LISTENED: the panel swallowed the failure to
  // keep its write chain alive, which is right, and then said nothing at all — so an operator typing
  // into a session whose agent had gone away saw their keystrokes vanish with no explanation. The
  // status line already carries failures ("open failed: …"), and this is one of them.
  useEffect(() => {
    const onWriteFailed = () =>
      setStatusState(
        "error: keystrokes could not be sent — this session may be gone",
      );
    window.addEventListener("summrise-write-failed", onWriteFailed);
    return () =>
      window.removeEventListener("summrise-write-failed", onWriteFailed);
  }, []);

  const openSession = useCallback(
    async (
      kind: string,
      target: string,
      extra: Record<string, unknown> = {},
    ) => {
      try {
        const sid = await callTool("terminal_open", {
          kind,
          target,
          rows: 30,
          cols: 120,
          ...extra,
        });
        if (typeof sid !== "string" || !sid)
          throw new Error("terminal_open returned no sid");
        setSessions((prev) => {
          // round-131: rebuild the entry UNCONDITIONALLY — the old
          // `prev.some(...) return prev` guard let a 3s poll tick (which
          // registered the session with active:false between the server's
          // open and this setSessions) skip the activation, leaving the pane
          // display:none (round-86 bug class). Filtering any existing entry
          // also clears a stale tombstone from a reordered poll response.
          const label =
            kind === "ssh"
              ? target.split("@").pop() || target
              : kind === "serial"
                ? target.split("?")[0]
                : target || "shell";
          // round-86: the new session is the ACTIVE one — the old active:false
          // + setActiveSid(sid) never set the session's own flag, so the pane
          // stayed display:none (blank terminal area).
          return [
            ...prev
              .filter((s) => s.sid !== sid)
              .map((s) => ({ ...s, active: false })),
            {
              sid,
              label,
              kind,
              closed: false,
              savedOnly: false,
              active: true,
              idleMs: 0,
              commandRunning: false,
              lastExitCode: null,
              firstSeenAt: Date.now(),
              closedAt: null,
              heldByHuman: false,
              approvalRequired: false,
              pendingApproval: null,
              approvalGrants: [],
              goal: null,
              plan: [],
            },
          ];
        });
        setActiveSid(sid);
        return sid;
      } catch (e: any) {
        setStatusState(`open failed: ${e.message}`);
        throw e;
      }
    },
    [],
  );

  const closeSession = useCallback(
    async (sid: string) => {
      // round-83: a transient close failure must NOT mark the session closed —
      // the old catch(() => {}) swallowed the error and the tab wedged
      // (closed class, onClick disabled, SSE still streaming). On failure keep
      // it open and surface the error.
      try {
        await callTool("terminal_close", { session_id: sid });
        setSessions((prev) => {
          const next = prev.map((s) =>
            s.sid === sid ? { ...s, closed: true, closedAt: Date.now() } : s,
          );
          // round-86: closing the ACTIVE session must switch to the next live
          // one — the old code left activeSid on the dead tab (stale output,
          // unclickable, typing went nowhere).
          // round-94: read the LIVE activeSid — the user may have activated
          // another tab while terminal_close was in flight; only switch if the
          // closed session is still the active one.
          if (activeRef.current === sid) {
            const nextLive = next.find((s) => !s.closed && s.sid !== sid);
            if (nextLive) {
              setActiveSid(nextLive.sid);
              return next.map((s) => ({
                ...s,
                active: s.sid === nextLive.sid,
              }));
            }
            // round-88: no live session left — the closed one must NOT stay
            // active (it kept its pane visible with a blinking cursor while
            // no tab was highlighted).
            setActiveSid(null);
            return next.map((s) => ({ ...s, active: false }));
          }
          return next;
        });
      } catch (e: any) {
        setStatusState(`close failed — session still open: ${e.message}`);
      }
    },
    [activeSid],
  );

  const activate = useCallback((sid: string) => {
    // round-117: a CLOSED (tombstone) session must not become active —
    // round-113 unmounted closed panes, so activating one blanks the whole
    // terminal area (every mounted pane hidden; with no live sessions the
    // blank is permanent). The round-86 "review closed history" intent died
    // with round-113; a closed tab click is now a no-op.
    setSessions((prev) => {
      const target = prev.find((s) => s.sid === sid);
      if (!target || target.closed) return prev;
      setActiveSid(sid);
      return prev.map((s) => ({ ...s, active: s.sid === sid }));
    });
  }, []);

  const exportSession = useCallback(
    (sid: string) => {
      // review #7: ONE read returns at most 1 MiB (the spill cap tail-clamps)
      // — long sessions exported as a truncated slice with no marker. Page
      // the retained history with the returned END cursor.
      // P1-4 (export backpressure): bound the download (a 64 MiB Blob build
      // froze the tab on huge AI sessions) and SAY SO — a truncation marker
      // goes into the file tail plus a status-line prompt.
      (async () => {
        try {
          const parts: string[] = [];
          let offset = 0;
          let truncated = false;
          for (let i = 0; i < MAX_EXPORT_PAGES; i++) {
            const r: any = await callTool("terminal_read", {
              session_id: sid,
              offset,
              clean: true,
            });
            const text = (r && r.text) || "";
            if (!text) break;
            parts.push(text);
            const end = Number(r.end ?? 0);
            if (!Number.isFinite(end) || end <= offset) break;
            offset = end;
            if (i === MAX_EXPORT_PAGES - 1) {
              // Loop exhausted with the cursor still advancing — probe once to
              // tell "stopped exactly at the end" from "more history pending".
              try {
                const probe: any = await callTool("terminal_read", {
                  session_id: sid,
                  offset,
                  clean: true,
                });
                if (probe && probe.text) truncated = true;
              } catch {
                /* probe failed — treat the export as complete */
              }
            }
          }
          if (truncated)
            parts.push(
              `\n…[export truncated at ${MAX_EXPORT_PAGES} MiB — read the full log via terminal_read offset ${offset}]…\n`,
            );
          const blob = new Blob([parts.join("")], { type: "text/plain" });
          const a = document.createElement("a");
          a.href = URL.createObjectURL(blob);
          a.download = `${sid}.log`;
          a.click();
          URL.revokeObjectURL(a.href);
          if (truncated)
            setStatusState(
              `export truncated at ${MAX_EXPORT_PAGES} MiB — the file tail says where to continue reading`,
            );
        } catch {
          setStatusState("export failed");
        }
      })();
    },
    [setStatusState],
  );

  /** Hand the session's keyboard to a person, or back to the AI.
   *
   *  The agent owns this state; the response is the authority, so the local flag
   *  is set from what the SERVER reports rather than from what was requested.
   *  A failed call leaves the flag alone and surfaces the error — a button that
   *  flipped optimistically would tell the operator they hold a keyboard the
   *  agent is still driving, which is worse than showing nothing. */
  const setControl = useCallback(async (sid: string, human: boolean) => {
    try {
      const r = await callApi(
        `/api/sessions/${encodeURIComponent(sid)}/control`,
        {
          method: "POST",
          body: JSON.stringify({ holder: human ? "human" : "ai" }),
        },
      );
      const held = !!r?.held_by_human;
      setSessions((prev) =>
        prev.map((s) => (s.sid === sid ? { ...s, heldByHuman: held } : s)),
      );
      setStatusState(
        held ? "you have the keyboard" : "AI may drive this session",
      );
      return held;
    } catch (e: any) {
      setStatusState(`control failed: ${e?.message ?? e}`);
      throw e;
    }
  }, []);

  /** Arm or disarm the approval gate for a session.
   *
   *  Like `setControl`, the SERVER's answer is the authority: a button that
   *  flipped optimistically would tell the operator the gate is armed while the
   *  agent still runs commands unattended — the one direction of error that
   *  matters here. */
  const setApproval = useCallback(async (sid: string, required: boolean) => {
    try {
      const r = await callApi(
        `/api/sessions/${encodeURIComponent(sid)}/control`,
        {
          method: "POST",
          body: JSON.stringify({ approval_required: required }),
        },
      );
      const on = !!r?.approval_required;
      setSessions((prev) =>
        prev.map((s) => (s.sid === sid ? { ...s, approvalRequired: on } : s)),
      );
      setStatusState(
        on ? "approval required for this session" : "approval gate off",
      );
      return on;
    } catch (e: any) {
      setStatusState(`approval mode failed: ${e?.message ?? e}`);
      throw e;
    }
  }, []);

  /** Answer a pending approval.
   *
   *  `decided: false` means there was nothing left to decide — the agent gave up
   *  or another client answered first. That is NOT success, so the status says
   *  so rather than leaving the operator believing their click landed. */
  const decideApproval = useCallback(
    async (sid: string, id: string, approve: boolean, grant = false) => {
      try {
        const r = await callApi(
          `/api/sessions/${encodeURIComponent(sid)}/approval`,
          {
            method: "POST",
            body: JSON.stringify({ id, approve, grant }),
          },
        );
        if (!r?.decided) {
          setStatusState("that request was already resolved");
          return false;
        }
        // Grants come back on the decision's own response, so the list updates
        // immediately rather than one poll later.
        if (Array.isArray(r?.approval_grants)) {
          const g: string[] = r.approval_grants;
          setSessions((prev) =>
            prev.map((s) => (s.sid === sid ? { ...s, approvalGrants: g } : s)),
          );
        }
        setStatusState(
          approve
            ? grant
              ? "approved, and remembered"
              : "approved"
            : "refused",
        );
        return true;
      } catch (e: any) {
        setStatusState(`decision failed: ${e?.message ?? e}`);
        throw e;
      }
    },
    [],
  );

  /** State the session's goal, or clear it with an empty string.
   *
   *  The SERVER's stored value is what lands in state, so a goal that was trimmed
   *  or capped comes back as what is actually in force rather than as what was
   *  typed. */
  const setGoal = useCallback(async (sid: string, goal: string) => {
    try {
      const r = await callApi(
        `/api/sessions/${encodeURIComponent(sid)}/control`,
        {
          method: "POST",
          body: JSON.stringify({ goal }),
        },
      );
      const stored =
        typeof r?.goal === "string" && r.goal.trim() ? r.goal : null;
      setSessions((prev) =>
        prev.map((s) => (s.sid === sid ? { ...s, goal: stored } : s)),
      );
      setStatusState(stored ? "goal set" : "goal cleared");
      return stored;
    } catch (e: any) {
      setStatusState(`goal failed: ${e?.message ?? e}`);
      throw e;
    }
  }, []);

  /** Revoke one grant, or all of them. The SERVER's list is the answer, so a
   *  revoke that did not land cannot leave the panel showing it as gone. */
  const revokeGrants = useCallback(async (sid: string, grant?: string) => {
    try {
      const r = await callApi(
        `/api/sessions/${encodeURIComponent(sid)}/grants`,
        {
          method: "POST",
          body: JSON.stringify(grant === undefined ? { all: true } : { grant }),
        },
      );
      const g: string[] = Array.isArray(r?.approval_grants)
        ? r.approval_grants
        : [];
      setSessions((prev) =>
        prev.map((s) => (s.sid === sid ? { ...s, approvalGrants: g } : s)),
      );
      setStatusState(
        grant === undefined
          ? "all allowances revoked"
          : `no longer allowing ${grant}`,
      );
      return g;
    } catch (e: any) {
      setStatusState(`revoke failed: ${e?.message ?? e}`);
      throw e;
    }
  }, []);

  return {
    sessions,
    activeSid,
    status,
    setStatus,
    openSession,
    closeSession,
    activate,
    exportSession,
    setControl,
    setApproval,
    decideApproval,
    revokeGrants,
    setGoal,
  };
}
