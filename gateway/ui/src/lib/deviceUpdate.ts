// deviceUpdate.ts — WHAT THIS CONSOLE MAY SAY AND DO ABOUT ONE DEVICE'S UPDATE, decided in one place.
//
// WHY A MODULE. The Devices page already owns the fleet's version VERDICT (`verdict = st?.update`, guarded by
// `device-verdict-check`), and this is the step after it: the verdict says whether a newer build exists, and this
// says whether anything may be PRESSED. Those are different questions with different inputs — the verdict needs
// `update_available`, the control needs `busy`, `error`, `pinned_to` and `last_attempt` as well — and the moment
// they are two `if`s in a JSX expression, the third one is written by hand somewhere else.
//
// THE SPINE, and it is one sentence: THE DEVICE'S OWN ANSWER DECIDES WHETHER THE CONTROL EXISTS. `st.update` is
// set by the gateway only inside `if (res.ok)` of its probe of the device's `/api/status`, so a device that did
// not answer has NO verdict — and no verdict means nothing may be claimed or pressed. This module never derives
// a version comparison of its own; the console's one comparison stays in `views/DevicesPanel.tsx` as the guarded
// fallback, where `device-verdict-check` holds it (`console-derivation-check` fails a comparison anywhere else).
//
// THE THREE STATES THE BRIEF NAMES, as they are computed here:
//
//   offline     → `none`. There is no control because there is no answer. The tri-state gate is
//                 `agent.signal === "err"` ("asked, and it is not answering") and NEVER `!agentUp`: a device
//                 nobody has asked yet is ALSO falsy, and a boolean test would print a refusal about every row
//                 before the first poll returned. `agentSignal` is required rather than optional for that reason —
//                 a caller cannot pass the boolean by mistake.
//   up to date  → `current` (the version chip). It says nothing and does nothing, which IS the sentence.
//   in flight   → `inflight`. One sentence, not a button and not a spinner: a second press is what the device's
//                 own marker exists to refuse, and a spinner would be this console claiming to know when the
//                 device comes back.
//
// AND TWO STATES THE BRIEF'S THREE DO NOT COVER, both of which used to render as "up to date" — the lie this
// control exists to remove:
//
//   `unchecked` — the device answered, but `error` says its RELEASE SERVER did not. The device sets
//                 `update_available: false` in that case, so a chip showing only a version reads as "you are
//                 current" when the truth is "nobody asked successfully". `tools.rs` says it in one line:
//                 "NOT AVAILABLE WHEN NOBODY ANSWERED: an unreachable channel must never read as up to date".
//   `held`      — `pinned_to` is set: a human ran `summrise rollback` on the device. The field was forwarded and
//                 read by NOTHING, so a device held on a version was indistinguishable from a current one. The
//                 control does not offer the update, and it never sends `force` — on the device `force` DELETES
//                 `.rollback-pin`, so the override is not something this surface may do silently.

// ── RUST SINCE 2026-09-30 (block ③), THE THIRD MODULE THROUGH THE CONSOLE'S SEAM ──────────────────
//
// `updateControl` and the two-hour window that dates it are `gateway/ui-logic/src/lib.rs` now,
// transliterated: the same truthiness tests, the same `typeof latest === "string"` (a NON-EMPTY string
// rather than a truthy one), the same `error`-outranks-`pinned_to` precedence, the same `?? Date.now()`
// arm, and the same key order on every result. The differential is 147,600 corpus cases with 0
// divergences and every arm of the state machine reached; `test/device-update.test.mjs` runs UNCHANGED.
//
// WHAT STAYS HERE, AND IT IS P0's THIRD CLASS RATHER THAN AN OMISSION: the localStorage half. It talks
// to the platform, and the crate has no localStorage. `rememberedUpdateAttempt` asks the crate for the
// window (`dark_window_ms`) instead of keeping a second copy of the number — the one thing on this side
// that did change.
import type { Signal } from "./deviceState.ts";
import { logic } from "../wasm/consoleLogic.ts";

/** The device's own answer at `/api/update`, as far as the gateway forwards it (`DeviceUpdate` in
 *  `gateway/src/plugins/mcp.ts` owns the field-by-field justification). Everything optional here is optional
 *  because an older agent does not send it — absent means "not reported", never "zero". */
interface DeviceUpdateWire {
  current?: string;
  latest?: string;
  update_available: boolean;
  pinned_to: string | null;
  /** The device's update marker exists. It is only the presence of a file, and the device itself stops trusting
   *  it after `BUSY_STALE_SECS` (600) — which is why the sentence below is dated by `last_attempt` and not by this. */
  busy?: boolean;
  /** The release server did not answer. Not a verdict about versions — the ABSENCE of one. */
  error?: string | null;
  /** When the DEVICE asked its channel, ms. The row's own `checked_at` is this console's probe time. */
  checked_at?: number;
  /** `{at_ms, from, to, launched}` — written when the device handed the swap to WMI, which is the only moment
   *  anything can say for certain that an update STARTED. A log narrates what happened next; this is the fact. */
  last_attempt?: { at_ms?: number; from?: string; to?: string; launched?: boolean } | null;
}

/** What the row may render. `kind: "none"` means exactly that — no chip, no button, no sentence.
 *
 *  THE VERSION IS NOT IN HERE, on purpose. Which version the row PRINTS is the view's single expression
 *  (`st.update?.current ?? st.version ?? d.lastVersion`); a second copy of that choice inside this module is a
 *  second answer to "what version is this device on", and the two would drift the first time one changed. What
 *  this module owns is the DECISION — whether anything may be pressed — and the facts that qualify it. */
type UpdateControl =
  | { kind: "none" }
  | { kind: "action"; to: string }
  | { kind: "inflight"; since: number | null; source: "device" | "console" }
  | { kind: "held"; pinnedTo: string; checkedAt: number | null }
  | { kind: "unchecked"; error: string; checkedAt: number | null }
  | { kind: "current"; checkedAt: number | null };

/** HOW LONG A DEVICE MAY STAY DARK AFTER AN UPDATE BEFORE THIS CONSOLE STOPS CALLING IT "IN FLIGHT".
 *
 *  THE NUMBER IS `UPDATE_DARK_WINDOW_MS` IN `gateway/ui-logic/src/lib.rs` NOW, with the measurement
 *  that justifies it (two hours, from a device that took `summrise update` and answered 502 / 530 /
 *  1033 for roughly two hours while `startup.log` showed the agent starting normally — THE SWAP HAD
 *  SUCCEEDED, SLOWLY). It is asked for here rather than copied: `logic().dark_window_ms()`. */

/* ── THE CONSOLE'S OWN MEMORY OF AN ATTEMPT ────────────────────────────────────────────────────────────────
 *
 * WHY IT EXISTS. When the swap starts, the device goes dark — and a dark device has no `st.update`, so the
 * in-flight sentence loses its source at exactly the moment it becomes the only thing on the row worth reading.
 * The device cannot answer then, so the fact has to come from the end that pressed the button.
 *
 * IT IS THE CONSOLE'S FACT, NOT A GUESS ABOUT THE DEVICE: "at T this console asked this device to update". That
 * is why it is written BEFORE the request leaves and cleared only on an answer that proves nothing started — a
 * timeout is not a refusal, and forgetting the attempt because the tunnel died mid-swap would delete the signal
 * in the one case it exists for.
 *
 * It is per-browser (localStorage) and expires with `UPDATE_DARK_WINDOW_MS`, so a console that is closed for a
 * day does not resurrect a stale sentence. */

const ATTEMPT_KEY = "summrise-console-update-attempt";

interface Attempt {
  at: number;
  to: string;
}

let cache: Record<string, Attempt> | null = null;

/** localStorage is not always there — a private window can throw on ACCESS, not only on write, and the jsdom
 *  suites render the page outside a browser. A console that cannot remember must still render. */
function store(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

function readAll(): Record<string, Attempt> {
  if (cache) return cache;
  const s = store();
  if (!s) return (cache = {});
  try {
    const parsed: unknown = JSON.parse(s.getItem(ATTEMPT_KEY) || "{}");
    cache = parsed && typeof parsed === "object" ? (parsed as Record<string, Attempt>) : {};
  } catch {
    cache = {};
  }
  return cache;
}

function writeAll(next: Record<string, Attempt>): void {
  cache = next;
  const s = store();
  if (!s) return;
  try {
    s.setItem(ATTEMPT_KEY, JSON.stringify(next));
  } catch {
    /* a full or forbidden store is not a reason to fail a request that is already on its way */
  }
}

/** Record that THIS console asked this device to update. Called before the request leaves, on purpose. */
export function rememberUpdateAttempt(name: string, to: string, at = Date.now()): void {
  writeAll({ ...readAll(), [name]: { at, to } });
}

/** Drop the record — only for an answer that PROVES nothing started (`up_to_date`, `pinned`, a refusal). */
export function forgetUpdateAttempt(name: string): void {
  const all = { ...readAll() };
  delete all[name];
  writeAll(all);
}

/** This console's record for one device, or null when it never asked or the record is older than the window.
 *
 *  THE STORE'S OWN GUARD STAYS EXACTLY AS IT WAS — `typeof hit.at !== "number" || hit.at <= 0` — and it is
 *  deliberately NOT the crate's `positive()`: the two differ on `Infinity`, where this guard accepts the
 *  record and the derivation then rejects it. The module's own note below records why both exist: the store's
 *  filter is an OPTIMISATION, and the rule belongs to the derivation. Only the WINDOW moved, and it is asked
 *  for rather than copied. */
export function rememberedUpdateAttempt(name: string, now = Date.now()): Attempt | null {
  const hit = readAll()[name];
  if (!hit || typeof hit.at !== "number" || hit.at <= 0) return null;
  return now - hit.at < logic().dark_window_ms() ? hit : null;
}

/* ── THE DERIVATION ──────────────────────────────────────────────────────────────────────────────────────── */

/** What `updateControl` takes. NOT EXPORTED, and `exports-check` is why: nobody outside names this shape — the
 *  view passes an object literal and the compiler infers it — so the keyword would be a promise about a public
 *  surface with nobody on the other end of it. It goes back on the day a second caller has to assemble one. */
interface UpdateInputs {
  /** `st.update` — the device's OWN answer. Absent whenever the gateway's probe did not reach it. */
  update?: DeviceUpdateWire | null;
  /** The tri-state agent signal from `agentSignal()`. REQUIRED: see the header for why a boolean is the wrong
   *  input and why making it optional would invite one. */
  agentSignal: Signal;
  /** `rememberedUpdateAttempt(name)` — this console's own record, if it has one. */
  remembered: { at: number; to: string } | null;
  now?: number;
}

/** The one function every render of this control goes through.
 *
 *  THE ARMS AND THEIR ORDER ARE THE CRATE'S NOW (`update_control` in `gateway/ui-logic/src/lib.rs`,
 *  where each one carries the reason it is tested where it is). What is worth repeating here is the
 *  half a reader of THIS file needs: a device that did not answer has no `st.update` at all — the
 *  gateway sets it only inside `if (res.ok)` — and the sentence about it comes from `agent.signal ===
 *  "err"` (the tri-state "asked, and it is not answering"), never from `!agentUp`, which is ALSO true
 *  for a device nobody has asked about yet.
 *
 *  THE WINDOW IS APPLIED IN BOTH PLACES, on purpose: here in the derivation, and in
 *  `rememberedUpdateAttempt`'s store filter as an optimisation. A caller handing this function a record
 *  it read some other way must not be able to resurrect a sentence about a swap that finished
 *  yesterday. */
export function updateControl(input: UpdateInputs): UpdateControl {
  return logic().update_control(
    input.update,
    input.agentSignal,
    input.remembered,
    input.now,
  ) as UpdateControl;
}
