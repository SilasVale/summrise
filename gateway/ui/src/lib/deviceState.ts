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
// WHAT THIS IS NOT: a device whose status is STALE. `checked_at` says when the gateway last probed, and a row could
// report "checked 40 minutes ago"; distinguishing that from fresh is a second question, and it is not answered here.

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
export function deviceIsUp(status: DeviceStatusLike | undefined): boolean {
  return !!status?.agent_up;
}

/** A status row: what it is called, which of the three states it is in, and the word for it. */
interface DeviceSignal {
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

function signalOf(value: boolean | undefined, t: Translate, yes: DeviceKey, no: DeviceKey): { signal: Signal; state: string } {
  if (value === undefined) return { signal: "off", state: t("devices.notChecked") };
  return value ? { signal: "ok", state: t(yes) } : { signal: "err", state: t(no) };
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
