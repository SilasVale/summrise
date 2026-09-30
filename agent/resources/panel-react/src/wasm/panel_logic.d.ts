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
 * `attemptAge(atMs, nowMs)` — the age of an ACT, where `checkedAge` is the age of a READING. Same
 * units, no verb: the sentence around it already says what happened.
 */
export function attempt_age(at_ms: any, now_ms: any): string;

/**
 * `attentionSteps(steps)` — worth a second look, worst first.
 *
 * `bg` ranks with `running`: both are "not finished", and neither is a problem to draw the eye. The
 * order is `(rank, index)`, and both keys are unique per step, so the comparator is a total order
 * and the sort does not depend on the engine's stability.
 */
export function attention_steps(steps: any): Array<any>;

/**
 * `badgeIcon(count, urgent, baseHref)` — the favicon for this much attention, as a data URL.
 *
 * `count` arrives as a JS number rather than a `usize` because the TypeScript's own tests are the
 * subject: `count <= 0` and `count > 9` are the two tests, and a `-1` or a `2.5` reaches them.
 */
export function badge_icon(count: number, urgent: boolean, base_href: any): string;

/**
 * `cardState(card)` — the same derivation, over the command card's own fields. It is a separate
 * export because two callers hold a card and not the three arguments, and folding it into one means
 * every call site builds an object to pass three values.
 */
export function card_state(card: any): object;

/**
 * `checkedAge(checkedAt, nowMs)` — "checked 12s ago", or null when there is no time to weigh.
 *
 * ZERO IS NOT A TIME, and the formatter is the last place before the screen: `epoch 0` renders as
 * "checked 497204h ago", which is a claim about a device that simply has not answered.
 */
export function checked_age(checked_at: any, now_ms: any): any;

/**
 * `derivePath(rounds, controlEvents)` — the steps, their summary, and the round id → index map.
 *
 * `controlEvents` DEFAULTS to `[]` in the TypeScript, and a `JsValue::UNDEFINED` in is that same
 * empty list — so a caller that omits it gets a path with every step owned by the agent, which is
 * what "no handoff" means.
 */
export function derive_path(rounds: any, control_events: any): object;

/**
 * `deviceLiveness(input)` — the device as a whole: reachable, holding questions, or busy.
 *
 * NO `failed` HERE, AND THAT IS A DECISION RATHER THAN AN OMISSION: a device-level failure would
 * have to pick WHICH session's last command to blame and say nothing about which, and the rail is
 * the one mark that is always on screen — a light that is on most of the time means nothing.
 */
export function device_liveness(input: any): string;

/**
 * `diagnoseUpdate(log)` — the four-way verdict over `summrise-update.log`.
 *
 * PRESENCE IS DECIDED PER KIND, not by position: a receipt and a start can be interleaved over
 * several updates, and the question is only whether each kind appears at all. Ordering by line index
 * would answer a different question ("was the LAST thing a receipt?") and would report `cli-only` for
 * a device whose most recent update succeeded.
 */
export function diagnose_update(log: any): object;

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
 * `groupEvents(events)` — the live trail's CARDS.
 *
 * A `command/start` while the previous command never ended closes it as `interrupted`, so a
 * mid-stream start cannot orphan a card that would otherwise read "running" forever.
 */
export function group_events(events: any): Array<any>;

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
 * `groupRounds(events)` — the trajectory's ROUNDS, which are `derivePath`'s input.
 *
 * A round is ENDED by a `command/end` OR by a terminal status, and **the LAST marker in the round
 * wins** — a backgrounded command can later log `closed`. A superseded round is sealed AS-IS (the
 * raw view: what the log says), which is where this deliberately disagrees with `groupEvents`.
 */
export function group_rounds(events: any): Array<any>;

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
 * Does the recent series say something worth interrupting an operator for?
 *
 * `samples` is the device's series, oldest first; `nowMs` the caller's clock. The rule reads only
 * the last `window_ms`, needs `min_samples` of evidence, and fires only when at least two thirds of
 * those readings sit in the dial's `crit` band.
 *
 * SILENCE IS THE DEFAULT, and it is the honest one: a device this panel cannot see a series for (a
 * host that reports no vitals, an agent that just started) produces no notice, because "I have not
 * looked" and "nothing is wrong" are different facts and a chip that conflates them is worse than no
 * chip.
 */
export function load_notice(samples: any, now_ms: any, window_ms: any, min_samples: any): any;

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
 * `parseAttempt(v)` — the launch record, or null. A record without a POSITIVE time, a `from` and a
 * `to` is not one: half a record would render as "updated from to at Invalid Date".
 */
export function parse_attempt(v: any): any;

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
 * `parseUpdateStatus(j)` — read `/api/update`. Never throws; a body this build cannot use is the
 * empty state, which renders as "unknown" rather than as "current".
 */
export function parse_update_status(j: any): object;

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
 * min / avg / max over the known readings, or `null` when there are none. Never invents a zero for
 * an empty series.
 */
export function series_stats(values: any): any;

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
 * Path data for an SVG polyline, one entry per RUN of known values.
 *
 * `values` is oldest-first (the device's order). `width`/`height` are the drawing box; the path is
 * scaled to fill it, with the series' own min/max as the vertical range — a sparkline's job is the
 * shape, and a fixed 0-100 axis would flatten every real series into a straight line. A run of ONE
 * known value draws a dot-sized segment rather than nothing, so a single reading is still visible.
 *
 * Returns an EMPTY list when there is nothing to draw (all values absent): an empty chart must be an
 * ABSENT chart, never a flat line at zero.
 */
export function spark_segments(values: any, width: any, height: any): Array<any>;

/**
 * `stateFromEnd(ended, exitCode, reason)` — THE ONE DERIVATION OF A COMMAND'S STATE.
 *
 * `reason` is the STATUS string the trail carries (`backgrounded`, `closed`, `interrupted`,
 * `exited:3`), which is why feeding it through unchanged is what makes the views agree.
 */
export function state_from_end(ended: any, exit_code: any, reason: any): object;

/**
 * `stripAnsi(s)` — the four escape rules, then a sweep for any ESC that survived.
 *
 * THE SCAN, IN THE ORDER THE REGEX ALTERNATION READS:
 *
 *   1. `CSI`     ESC `[` [0-9;?]* [ -/]* [@-~]   — SGR colours, cursor moves, `\x1b[2J`
 *   2. `OSC`     ESC `]` [^BEL ESC]* (BEL | ESC `\` | end-of-input) — titles, `]133;D;`
 *   3. `DCS`     ESC `[P^_] … ESC `\`  — device-control strings
 *   4. `SINGLE`  ESC [ `=` `>` 7 8 6 M N O c ]  — the one-byte escapes
 *
 * and then every remaining ESC is dropped, so a control byte can never reach the DOM as text.
 */
export function strip_ansi(input: any): string;

/**
 * `summarizePath(steps)` — how much work, how much of it failed, and how long it took.
 *
 * `commandMs` is a FLOOR, not a total: a backgrounded or still-running step has no duration, so
 * `untimed` counts them and the view says "at least" instead of implying a total it cannot know.
 */
export function summarize_path(steps: any): object;

/**
 * `terminalStatus(st)` — the marker rule, or nothing for a status that ends nothing.
 *
 * `exited:<n>` is the one that carries a code, and the code is the DEVICE's: a non-numeric tail is
 * `NaN`, `Number.isFinite` says no, and the answer is a reason with no exit code.
 */
export function terminal_status(status: string): object | undefined;

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
    readonly attempt_age: (a: any, b: any) => [number, number];
    readonly attention_steps: (a: any) => any;
    readonly badge_icon: (a: number, b: number, c: any) => [number, number];
    readonly card_state: (a: any) => [number, number, number];
    readonly checked_age: (a: any, b: any) => any;
    readonly derive_path: (a: any, b: any) => [number, number, number];
    readonly device_liveness: (a: any) => [number, number, number, number];
    readonly diagnose_update: (a: any) => [number, number, number];
    readonly disambiguate_labels: (a: any) => [number, number, number];
    readonly group_events: (a: any) => [number, number, number];
    readonly group_operation: (a: any, b: any) => [number, number, number];
    readonly group_rounds: (a: any) => [number, number, number];
    readonly liveness_of: (a: any) => [number, number, number, number];
    readonly load_notice: (a: any, b: any, c: any, d: any) => [number, number, number];
    readonly operation_rows: (a: any) => [number, number, number];
    readonly parse_attempt: (a: any) => any;
    readonly parse_boot_history: (a: any) => [number, number, number];
    readonly parse_evicted: (a: any) => [number, number, number];
    readonly parse_last_boot: (a: any) => [number, number, number];
    readonly parse_monitor_change: (a: any) => [number, number, number];
    readonly parse_monitors: (a: any) => [number, number, number];
    readonly parse_update_status: (a: any) => any;
    readonly parse_vitals_series: (a: any) => [number, number, number];
    readonly series_stats: (a: any) => [number, number, number];
    readonly session_active: (a: any, b: number) => [number, number, number];
    readonly session_failed: (a: any) => [number, number, number];
    readonly session_liveness: (a: any, b: number) => [number, number, number, number];
    readonly session_waiting: (a: any) => [number, number, number];
    readonly spark_segments: (a: any, b: any, c: any) => [number, number, number];
    readonly state_from_end: (a: any, b: any, c: any) => [number, number, number];
    readonly strip_ansi: (a: any) => [number, number];
    readonly summarize_path: (a: any) => any;
    readonly terminal_status: (a: number, b: number) => any;
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
