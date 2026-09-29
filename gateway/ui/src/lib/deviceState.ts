// deviceState.ts — IS THIS DEVICE'S AGENT ANSWERING? ONE derivation, and "not checked" is not "down".
//
// WHY (round 35 of the standing goal). The console derived this twice — `deviceStatuses[d.name]?.agent_up` in the
// Devices page and `status[d.name]?.agent_up` in the Overview — and BOTH collapsed three states into two:
//
//     const st = deviceStatuses[d.name];
//     const agentUp = !!st?.agent_up;
//     { label: t("devices.statusAgent"), ok: agentUp, err: !agentUp, state: agentUp ? online : offline }
//
// With no status entry at all — a device the console HAS NEVER ASKED ABOUT, because the poll has not run or the
// gateway has not answered — `agentUp` is false, `err` is true, and the row paints a RED dot and the word
// "offline". That is a claim about a device nobody has checked, and it is the loudest claim the page can make.
//
// The `.sig-dot.off` state existed in the stylesheet for exactly this and had NO PRODUCER anywhere: every row was
// built `ok` or `err`. A mark language with a state nothing renders is a vocabulary with a hole in it.
//
// So the fact is here, with three outcomes, and both components read it:
//
//     undefined status      -> off   "not checked"   — nothing has been asked yet
//     agent_up: false       -> err   "offline"       — asked, and the agent is not answering
//     agent_up: true        -> ok    "online"
//
// HOW OLD IS THIS, AND HOW OLD CAN IT BE — ANSWERED, not left open. An earlier version of this note said a row could
// be "checked 40 minutes ago" and that freshness was a second question. It is not: the worker probes each device's
// own /api/status through its tunnel behind a 30-second in-isolate cache (DEVICE_PROBE_TTL_MS in
// `summrise-gate/src/plugins/mcp.ts`, mirrored for the console's code viewer at
// `gateway/public/code/files/summrise-gate/src/plugins/mcp.ts`), the console polls every 60s (`CONSOLE_POLL_MS` in this file), and `checked_at` is THAT
// probe's timestamp — not the newer `agent_update` check's, which is a different field on the same row. So the
// worst-case age of an `agent_up` reading is about a minute, a stale row cannot masquerade as a fresh one, and no
// fourth "stale" state is needed. A test in `test/device-state.test.mjs` reads the mirror and fails if the TTL this
// note cites stops matching the source.

import { logic } from "../wasm/consoleLogic.ts";

// ── RUST SINCE 2026-09-29 (block ③), the console's biggest remaining module ──────────────────────
//
// The five functions below are `gateway/ui-logic/src/lib.rs` now, transliterated: the same tri-state
// (an ABSENT status is "not checked", `false` is "offline", `true` is "online"), the same
// `tunnel_up === false` strictness, the same `devices ?? []`, and the same four fields on each
// signal row in the TypeScript's key order. The translator `t` still crosses the boundary as a
// callback — the WORD for a state belongs to the console's dictionary, so the crate decides WHICH
// state and the dictionary says the word — and `test/device-state.test.mjs` (27 assertions) runs
// unchanged against it.
type Signal = "ok" | "err" | "off";

/** The shape both components hold: whatever the gateway's status endpoint answered for one device. */
interface DeviceStatusLike {
  agent_up?: boolean;
  tunnel_up?: boolean;
}

/**
 * IS THE AGENT ANSWERING. `false` for a device with no status entry, which is why a caller that needs to say
 * "offline" rather than "not checked" must read the SIGNAL below instead of this boolean.
 */
/** THE TUNNEL IS KNOWN DOWN — the tri-state rule, in one place (round 128 of the standing goal).
 *
 *  `openPanel` refused to open a device's page when `st.tunnel_up === false`, written by hand at the call site. The test is
 *  right — ABSENT IS NOT DOWN, which is the whole point of this module — but a second hand-written `=== false` is a place
 *  that can become `!st.tunnel_up` and start refusing devices nobody has checked. The rule belongs here, next to the signal
 *  that renders it. */
export function tunnelKnownDown(status?: { tunnel_up?: boolean } | null): boolean {
  return logic().tunnel_known_down(status);
}

export function deviceIsUp(status: DeviceStatusLike | undefined): boolean {
  return logic().device_is_up(status);
}

/** A status row: what it is called, which of the three states it is in, and the word for it. */
interface DeviceSignal {
  /** THE ROW'S TWO FLAGS (rounds 133/144): the same answer `signal` carries, because the view spreads this row straight
   *  into a list that renders `ok`/`err` classes. They were added to `signalOf`'s return type and NOT to this interface,
   *  which `tsc -b` caught in CI while a local `tsc --noEmit` reported clean. */
  ok: boolean;
  err: boolean;
  label: string;
  signal: Signal;
  state: string;
}

/**
 * Translate one of the three outcomes. Injected so this module stays free of the i18n runtime — and typed to the
 * EXACT keys it uses, so the console's own translator (a union of every key it knows) is assignable here and a key
 * this file invents is a compile error rather than a string that renders as its own name.
 */
type DeviceKey =
  | "devices.statusAgent"
  | "devices.online"
  | "devices.offline"
  | "devices.statusTunnel"
  | "devices.tunnelUp"
  | "devices.tunnelDown"
  | "devices.notChecked";
type Translate = (key: DeviceKey) => string;

// `signalOf` MOVED WITH ITS TWO CALLERS (block ③, 2026-09-29): the three-outcome rule is
// `signal_of` in the crate, and `agentSignal`/`tunnelSignal` above are the two places that use it. It
// is a private helper, so nothing imports it and the two `ok`/`err` flags the views spread straight
// into a list now come from one object the crate builds.

/** The agent row. */
export function agentSignal(status: DeviceStatusLike | undefined, t: Translate): DeviceSignal {
  return logic().agent_signal(status, t as unknown as () => string) as unknown as DeviceSignal;
}

/** The tunnel row — the same three states, because the gateway may not have probed it either. */
export function tunnelSignal(status: DeviceStatusLike | undefined, t: Translate): DeviceSignal {
  return logic().tunnel_signal(status, t as unknown as () => string) as unknown as DeviceSignal;
}

/** How many devices are known to be up, and how many have not been asked yet. */
interface DeviceTally {
  /** Devices whose status says the agent is answering. */
  online: number;
  /** Devices whose status says the tunnel is up. */
  tunnels: number;
  /** Devices with NO status entry — not checked, which is not the same as down. */
  unchecked: number;
  total: number;
}

/**
 * THE COUNTS, IN ONE PLACE (round 38 of the standing goal). Two surfaces computed "N devices online" from the same
 * fact, each with its own `filter(...).length` — the same shape that let the per-device mark disagree a round
 * earlier. A count cannot show ambiguity per device, so the honest thing is to return BOTH numbers: `online` is
 * what is known, `unchecked` is what is not yet known, and a surface that wants to say "1 of 2" can say so while
 * another shows the unchecked count beside it.
 */
function deviceTally(devices: { name: string }[] | null | undefined, statuses: Record<string, DeviceStatusLike | undefined>): DeviceTally {
  return logic().device_tally(devices, statuses) as unknown as DeviceTally;
}

export { deviceTally };
export type { DeviceTally };
/** EXPORTED SO A SECOND MODULE CAN REQUIRE THE TRI-STATE ANSWER RATHER THAN A BOOLEAN (the update control does:
 *  `lib/deviceUpdate.ts` takes this type, not `agentUp`, because "not yet asked" and "offline" are the same
 *  `false` and only the signal tells them apart). */
export type { Signal };

/**
 * HOW OFTEN THE CONSOLE ASKS FOR DEVICE STATUS — the single value both views used to hard-code separately.
 *
 * THE WORKER'S OWN COMMENT ABOUT THIS IS WRONG, and reading it is why this constant exists. `summrise-gate`'s
 * `cachedDeviceProbe` says its 30-second cache is safe because the console "polls every 30s already" — and the
 * console polls every 60s, in `Overview.tsx` and `DevicesPanel.tsx`, each with its own literal. The behaviour is
 * fine and this comment is not: a 30s cache behind a 60s poll always answers fresh, so the cache serves a second
 * view or a manual refresh and never the console's own request. What was NOT fine is a stated reason that is false
 * in another repository, and the same number written twice here — which is what this constant removes on this side.
 *
 * The console polls every 60s. The worker's sentence was FILED in `docs/agents/ideas.md` and FIXED in round
 * 156: it now states the measured margin (a 30s cache behind a 60s poll never serves the console's own
 * request), which is the fact a reader needs before changing the number. Whether this constant should live
 * somewhere both crates read is a placement decision, still open, still the operator's.
 */
// IT STAYS HERE, DELIBERATELY: it is the console's own polling cadence and no crate function reads
// it, and a module-level `logic()` call would evaluate before any test could await the module — which
// is exactly what the first version of this move did.
export const CONSOLE_POLL_MS = 60_000;
