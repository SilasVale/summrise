/* tslint:disable */
/* eslint-disable */

/**
 * `anyCommandRunning(sessions)` — is ANY session holding a command in flight, for the DEVICE mark.
 *
 * `!!sessions?.some(…)`: an absent or null list is `false`, and anything that is NOT a list is a
 * throw in the JavaScript (`{}.some` is not a function) — so it is an error here too rather than a
 * silent `false`, which would turn a caller's mistake into "nothing is running".
 */
export function any_command_running(sessions: any): boolean;

/**
 * `archiveEntries(payload)` — `GET /api/sessions` → entries, or a THROW.
 *
 * The throw is the point and it is why this function returns `Result`: `[]` from a body the panel
 * did not understand would render as "this device has recorded no sessions", which is a claim
 * about the DEVICE drawn from a response the panel failed to read. The two sentences are the
 * TypeScript's own, word for word — `useDeviceRead` carries a fold's message out to the operator
 * as the reason a read is unreadable.
 */
export function archive_entries(payload: any): any;

/**
 * `badgeIcon(count, urgent, baseHref)` — the favicon for this much attention, as a data URL.
 *
 * `count` arrives as a JS number rather than a `usize` because the TypeScript's own tests are the
 * subject: `count <= 0` and `count > 9` are the two tests, and a `-1` or a `2.5` reaches them.
 */
export function badge_icon(count: number, urgent: boolean, base_href: any): string;

/**
 * `deviceLiveness(input)` — the device as a whole: reachable, holding questions, or busy.
 *
 * NO `failed` HERE, AND THAT IS A DECISION RATHER THAN AN OMISSION: a device-level failure would
 * have to pick WHICH session's last command to blame and say nothing about which, and the rail is
 * the one mark that is always on screen — a light that is on most of the time means nothing.
 */
export function device_liveness(input: any): string;

/**
 * `disambiguateLabels(items)` — one label per item, numbered where they collide.
 *
 * THE INPUT IS AN `Array` AND A NON-ARRAY IS REFUSED, which is a deliberate narrowing of the
 * TypeScript rather than an accident: its `for (const item of items)` accepts any ITERABLE, so
 * `disambiguateLabels("ab")` walks the string's characters there and answers `["1·undefined", …]`.
 * That is not a shape this panel produces (both call sites pass a `Session[]`), and reproducing it
 * would mean writing a rule for an input nobody has — so the port throws instead, loudly, in the
 * one case the TypeScript would have silently invented labels for.
 */
export function disambiguate_labels(items: any): Array<any>;

/**
 * `groupOperation(events, boundaries)` — the timeline's events and its run boundaries folded into
 * one group per run, plus the unattributed bucket.
 *
 * Order-independent: both inputs may arrive in any order (the hook accumulates them across polls),
 * so extents are computed with min/max rather than by position, and a `run/begin` that arrives
 * after its own events still registers the run.
 *
 * `pushRow` sorts a group's rows by `ts_ms` after every push; a single stable sort of the whole
 * list at the end is the same order (each push appends, and a stable sort of an already-sorted
 * list preserves what it had), so that is what happens here.
 */
export function group_operation(events: any, boundaries: any): any;

/**
 * `livenessOf(input)` — THE PRECEDENCE, in one place.
 *
 * `reachable` is about the TRANSPORT, not the entity: a session on a dead connection cannot be
 * answered even if a question is outstanding, so it is `off` and the mark must not claim otherwise.
 * `waiting` outranks `working` because a question DECAYS if it is not seen while work continues;
 * `failed` sits between activity and quiet because it is a fact about what ALREADY HAPPENED.
 *
 * EVERY TEST IS THE JAVASCRIPT'S TRUTHINESS — `!input.reachable`, `input.pending`, `input.active`,
 * `input.failed` — and not `=== true`, which is why the fields are read as `JsValue` and asked with
 * `is_truthy`. A `1`, a `"no"` and an object are all the JavaScript's answers, and a port that
 * compared to `true` would answer differently for every one of them.
 */
export function liveness_of(input: any): string;

/**
 * `operationRows(...)` — every row of the timeline, in the same groups `groupOperation` builds,
 * in the order the groups are rendered: "grouped by run, oldest group first, the unattributed
 * bucket last and separate".
 *
 * It takes the GROUPS rather than the raw events, because the grouping has exactly one
 * implementation above: the rows a reader sees cannot disagree with the counts a strip shows.
 * The TypeScript wrapper keeps its `(events, boundaries)` signature for its own callers and makes
 * the same two calls this does.
 */
export function operation_rows(groups: any): any;

/**
 * `parseBootHistory(j)` — never throws, and never invents a value.
 */
export function parse_boot_history(j: any): any;

/**
 * `parseEvicted(detail)` — the notice, or `null` for anything this build cannot describe.
 *
 * `null` and not a throw: this runs inside an event handler, and a frame this build cannot use must
 * be a frame it says nothing about rather than a listener that raises. The three refusals are the
 * TypeScript's own and each is a strict test:
 *
 *   * `d.ev !== "session-evicted"` — the frame's own name, so a listener registered for one event
 *     cannot turn a different one into a notice about something that did not happen;
 *   * a `cause` that is neither `"idle"` nor `"cap"` — the two rules the device enforces, and a
 *     third one invented by a newer device is not a line this build knows how to write;
 *   * NO SESSIONS LEFT after the rows are filtered — a notice that says nothing was taken is worse
 *     than no notice, and it is what an empty `sessions` array would produce.
 */
export function parse_evicted(detail: any): any;

/**
 * `parseLastBoot(j)` — the last boot, or `null` when the body does not describe one.
 */
export function parse_last_boot(j: any): any;

/**
 * `parseMonitorChange(detail)` — one `monitor-change` frame → one alert, or `null`.
 *
 * `null` and not a throw: this runs inside an event handler, and a frame this build cannot use must
 * not become an exception in a listener — nor a banner about something that did not happen.
 */
export function parse_monitor_change(detail: any): any;

/**
 * `parseMonitors(j)` — `GET /api/monitors`'s body → the monitor list.
 *
 * Never throws, and never invents a value: a body this build cannot use is an EMPTY list, a target
 * with no id is DROPPED (a target the panel cannot name is not one it can address), and a probe or
 * a transition with no usable stamp is dropped too — it cannot be placed on the time axis, and the
 * alternative is a chart drawn from guesswork.
 */
export function parse_monitors(j: any): any;

/**
 * `parseVitalsSeries(j)` — the series, or the empty one.
 *
 * THE OUTPUT KEYS ARE THE TYPESCRIPT'S (`tsMs`, `intervalSecs`, `spanSecs`, `memTotalMb`) and not
 * the wire's (`ts_ms`, `interval_secs`, `span_secs`, `mem_total_mb`). The wire names are read; the
 * camelCase names are what the panel's readers destructure, and a port that returned the wire's
 * spelling would type-check against `VitalsSeries` only if the interface were changed too — which
 * is a change to the panel, not a migration of it.
 */
export function parse_vitals_series(j: any): any;

/**
 * `sessionActive(session, workingMs)` — is the SESSION working, not "is the device busy".
 *
 * THE DEVICE'S ANSWER FIRST: `command_running` is the manager's own busy flag, so it is true for the
 * WHOLE life of a command, including the silent minutes that output recency cannot see. Recency is
 * the second signal, for work that is not a command through this path — and it is `typeof idleMs ===
 * "number"`, a TYPE test, so a session whose `idle_ms` arrived as a string is not a reading.
 */
export function session_active(session: any, working_ms: number): boolean;

/**
 * `sessionFailed(session)` — DID THIS SESSION'S LAST COMMAND FAIL, the device's own exit code.
 *
 * ABSENT IS NOT FAILURE and not success: `lastExitCode` is `null` when the device observed no code
 * at all, which is a third state a mark must not turn into either answer. NON-ZERO IS A FAILURE,
 * with no judgement about which codes deserve it — the command cards have called every non-zero exit
 * "Failed (exit N)" since they existed.
 */
export function session_failed(session: any): boolean;

/**
 * `sessionLiveness(session, workingMs)` — ONE DERIVATION FOR EVERY SURFACE.
 *
 * Nothing about a session's own mark needs the device: connectivity is the RAIL's fact, so a
 * disconnected device does not make every session `off` — which is what a CLOSED session means.
 */
export function session_liveness(session: any, working_ms: number): string;

/**
 * `sessionWaiting(session)` — CAN THIS SESSION STILL ANSWER, the ONE predicate the mark, the tab's
 * title and its aria-label all read.
 *
 * A CLOSED session's row keeps its data — the tombstone is the same record — so a question that
 * expired with the session it belonged to survives in `pendingApproval`. Without the `closed` half,
 * the desktop tab's title says "waiting for your approval" about a tab that cannot be answered at
 * all, which is the disagreement between the two densities this model exists to stop.
 */
export function session_waiting(session: any): boolean;

/**
 * `titleFor(items, base, tab)` — the tab title, or the base when there is nothing to say.
 *
 * `items.length` IS A NON-NEGATIVE INTEGER BELOW 2^32, which is the one place this file may use
 * Rust's formatter for a number: `String(n)` and `{}` agree on every such value (they diverge at
 * 1e21 and on `-0`, neither of which an array length can be). The crate's rule — ask the ENGINE,
 * never Rust's formatter — is about values a device can send, and this one cannot be sent at all.
 */
export function title_for(items: any, base: string, tab: boolean): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly any_command_running: (a: any) => [number, number, number];
    readonly archive_entries: (a: any) => [number, number, number];
    readonly badge_icon: (a: number, b: number, c: any) => [number, number];
    readonly device_liveness: (a: any) => [number, number, number, number];
    readonly disambiguate_labels: (a: any) => [number, number, number];
    readonly group_operation: (a: any, b: any) => [number, number, number];
    readonly liveness_of: (a: any) => [number, number, number, number];
    readonly operation_rows: (a: any) => [number, number, number];
    readonly parse_boot_history: (a: any) => [number, number, number];
    readonly parse_evicted: (a: any) => [number, number, number];
    readonly parse_last_boot: (a: any) => [number, number, number];
    readonly parse_monitor_change: (a: any) => [number, number, number];
    readonly parse_monitors: (a: any) => [number, number, number];
    readonly parse_vitals_series: (a: any) => [number, number, number];
    readonly session_active: (a: any, b: number) => [number, number, number];
    readonly session_failed: (a: any) => [number, number, number];
    readonly session_liveness: (a: any, b: number) => [number, number, number, number];
    readonly session_waiting: (a: any) => [number, number, number];
    readonly title_for: (a: any, b: number, c: number, d: number) => [number, number];
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
