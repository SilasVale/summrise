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
// `gateway/public/code/files/summrise-gate/src/plugins/mcp.ts`), the console polls every 30s, and `checked_at` is THAT
// probe's timestamp — not the newer `agent_update` check's, which is a different field on the same row. So the
// worst-case age of an `agent_up` reading is about a minute, a stale row cannot masquerade as a fresh one, and no
// fourth "stale" state is needed. A test in `test/device-state.test.mjs` reads the mirror and fails if the TTL this
// note cites stops matching the source.

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
  return status?.tunnel_up === false;
}

export function deviceIsUp(status: DeviceStatusLike | undefined): boolean {
  return !!status?.agent_up;
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

function signalOf(value: boolean | undefined, t: Translate, yes: DeviceKey, no: DeviceKey): {
  // THE ROW'S TWO FLAGS COME FROM HERE TOO (round 133). The view built them at the call site —
  // `{ ...agent, ok: agent.signal === "ok", err: agent.signal === "err" }` — twice, which is the signal re-derived one
  // step after it was derived, and a place where a fourth signal value would be silently absent from both flags.
 signal: Signal; ok: boolean; err: boolean; state: string } {
  if (value === undefined) return { signal: "off", ok: false, err: false, state: t("devices.notChecked") };
  return value
    ? { signal: "ok", ok: true, err: false, state: t(yes) }
    : { signal: "err", ok: false, err: true, state: t(no) };
}

/** The agent row. */
export function agentSignal(status: DeviceStatusLike | undefined, t: Translate): DeviceSignal {
  return { label: t("devices.statusAgent"), ...signalOf(status?.agent_up, t, "devices.online", "devices.offline") };
}

/** The tunnel row — the same three states, because the gateway may not have probed it either. */
export function tunnelSignal(status: DeviceStatusLike | undefined, t: Translate): DeviceSignal {
  return { label: t("devices.statusTunnel"), ...signalOf(status?.tunnel_up, t, "devices.tunnelUp", "devices.tunnelDown") };
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
  const list = devices ?? [];
  let online = 0, tunnels = 0, unchecked = 0;
  for (const d of list) {
    const st = statuses[d.name];
    if (st === undefined) unchecked++;
    if (st?.agent_up) online++;
    if (st?.tunnel_up) tunnels++;
  }
  return { online, tunnels, unchecked, total: list.length };
}

export { deviceTally };
export type { DeviceTally };

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
 * The console polls every 60s. The worker's sentence is filed in `docs/agents/ideas.md` for whoever edits it.
 */
export const CONSOLE_POLL_MS = 60_000;
