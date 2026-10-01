/* @ts-self-types="./panel_logic.d.ts" */

/**
 * `actionVerdict(a)` — the badge's word and the device's own sentence, or nothing.
 * @param {any} action
 * @returns {any}
 */
export function action_verdict(action) {
    const ret = wasm.action_verdict(action);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * True when the response proves more history exists past what we rendered.
 * @param {any} resp
 * @param {any} rendered
 * @param {any} advanced
 * @returns {boolean}
 */
export function adopt_needs_another_page(resp, rendered, advanced) {
    const ret = wasm.adopt_needs_another_page(resp, rendered, advanced);
    return ret !== 0;
}

/**
 * Page bound: never chain more than this many reads (wedged-server guard).
 * @param {any} page
 * @param {any} max_pages
 * @returns {boolean}
 */
export function adopt_page_exceeded(page, max_pages) {
    const ret = wasm.adopt_page_exceeded(page, max_pages);
    return ret !== 0;
}

/**
 * `anyCommandRunning(sessions)` — is ANY session holding a command in flight, for the DEVICE mark.
 *
 * `!!sessions?.some(…)`: an absent or null list is `false`, and anything that is NOT a list is a
 * throw in the JavaScript (`{}.some` is not a function) — so it is an error here too rather than a
 * silent `false`, which would turn a caller's mistake into "nothing is running".
 * @param {any} sessions
 * @returns {boolean}
 */
export function any_command_running(sessions) {
    const ret = wasm.any_command_running(sessions);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return ret[0] !== 0;
}

/**
 * `archiveEntries(payload)` — `GET /api/sessions` → entries, or a THROW.
 *
 * The throw is the point and it is why this function returns `Result`: `[]` from a body the panel
 * did not understand would render as "this device has recorded no sessions", which is a claim
 * about the DEVICE drawn from a response the panel failed to read. The two sentences are the
 * TypeScript's own, word for word — `useDeviceRead` carries a fold's message out to the operator
 * as the reason a read is unreadable.
 * @param {any} payload
 * @returns {any}
 */
export function archive_entries(payload) {
    const ret = wasm.archive_entries(payload);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * `attemptAge(atMs, nowMs)` — the age of an ACT, where `checkedAge` is the age of a READING. Same
 * units, no verb: the sentence around it already says what happened.
 * @param {any} at_ms
 * @param {any} now_ms
 * @returns {string}
 */
export function attempt_age(at_ms, now_ms) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.attempt_age(at_ms, now_ms);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * `attentionSteps(steps)` — worth a second look, worst first.
 *
 * `bg` ranks with `running`: both are "not finished", and neither is a problem to draw the eye. The
 * order is `(rank, index)`, and both keys are unique per step, so the comparator is a total order
 * and the sort does not depend on the engine's stability.
 * @param {any} steps
 * @returns {Array<any>}
 */
export function attention_steps(steps) {
    const ret = wasm.attention_steps(steps);
    return ret;
}

/**
 * `badgeIcon(count, urgent, baseHref)` — the favicon for this much attention, as a data URL.
 *
 * `count` arrives as a JS number rather than a `usize` because the TypeScript's own tests are the
 * subject: `count <= 0` and `count > 9` are the two tests, and a `-1` or a `2.5` reaches them.
 * @param {number} count
 * @param {boolean} urgent
 * @param {any} base_href
 * @returns {string}
 */
export function badge_icon(count, urgent, base_href) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.badge_icon(count, urgent, base_href);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * The kinds, in words. ONE vocabulary for every surface that names a verdict — the chip's hover, the
 * history card's rows — so a kind cannot be described two ways in one panel. `null` (an unrecognised
 * kind) says so rather than borrowing another kind's wording.
 * @param {any} kind
 * @returns {string}
 */
export function boot_kind_label(kind) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.boot_kind_label(kind);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * The chip to render, or `null` for "nothing worth saying".
 *
 * `uptime_secs` is required for the `replaced` case and may be null: without a trustworthy "how long
 * ago", a normal restart cannot be told from a stale one, and the rule then says nothing rather than
 * guessing.
 *
 * `recent_crashes` is the device's own 24 h count from `/api/boots`. It never changes WHETHER the
 * chip appears — it is the same verdict either way — and it only ever adds a sentence to the hover:
 * "this happened once" and "this keeps happening" are different situations, and an operator staring
 * at the chip is exactly who needs to know which.
 * @param {any} last_boot
 * @param {any} uptime_secs
 * @param {any} recent_crashes
 * @param {any} replaced_notice_secs
 * @returns {any}
 */
export function boot_notice(last_boot, uptime_secs, recent_crashes, replaced_notice_secs) {
    const ret = wasm.boot_notice(last_boot, uptime_secs, recent_crashes, replaced_notice_secs);
    return ret;
}

/**
 * Render the recipe body.
 *
 * The shape is deliberate: a machine-readable marker line, the outcome summary (so a recipe that
 * half-failed is honest about it rather than presenting itself as a known-good procedure), then the
 * commands one per line in order.
 * @param {any} path
 * @param {any} input
 * @param {any} marker
 * @param {any} tag
 * @returns {object}
 */
export function build_recipe(path, input, marker, tag) {
    const ret = wasm.build_recipe(path, input, marker, tag);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * `cardState(card)` — the same derivation, over the command card's own fields. It is a separate
 * export because two callers hold a card and not the three arguments, and folding it into one means
 * every call site builds an object to pass three values.
 * @param {any} card
 * @returns {object}
 */
export function card_state(card) {
    const ret = wasm.card_state(card);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * `checkedAge(checkedAt, nowMs)` — "checked 12s ago", or null when there is no time to weigh.
 *
 * ZERO IS NOT A TIME, and the formatter is the last place before the screen: `epoch 0` renders as
 * "checked 497204h ago", which is a claim about a device that simply has not answered.
 * @param {any} checked_at
 * @param {any} now_ms
 * @returns {any}
 */
export function checked_age(checked_at, now_ms) {
    const ret = wasm.checked_age(checked_at, now_ms);
    return ret;
}

/**
 * `derivePath(rounds, controlEvents)` — the steps, their summary, and the round id → index map.
 *
 * `controlEvents` DEFAULTS to `[]` in the TypeScript, and a `JsValue::UNDEFINED` in is that same
 * empty list — so a caller that omits it gets a path with every step owned by the agent, which is
 * what "no handoff" means.
 * @param {any} rounds
 * @param {any} control_events
 * @returns {object}
 */
export function derive_path(rounds, control_events) {
    const ret = wasm.derive_path(rounds, control_events);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * `deviceLiveness(input)` — the device as a whole: reachable, holding questions, or busy.
 *
 * NO `failed` HERE, AND THAT IS A DECISION RATHER THAN AN OMISSION: a device-level failure would
 * have to pick WHICH session's last command to blame and say nothing about which, and the rail is
 * the one mark that is always on screen — a light that is on most of the time means nothing.
 * @param {any} input
 * @returns {string}
 */
export function device_liveness(input) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ret = wasm.device_liveness(input);
        var ptr1 = ret[0];
        var len1 = ret[1];
        if (ret[3]) {
            ptr1 = 0; len1 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred2_0 = ptr1;
        deferred2_1 = len1;
        return getStringFromWasm0(ptr1, len1);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * `diagnoseUpdate(log)` — the four-way verdict over `summrise-update.log`.
 *
 * PRESENCE IS DECIDED PER KIND, not by position: a receipt and a start can be interleaved over
 * several updates, and the question is only whether each kind appears at all. Ordering by line index
 * would answer a different question ("was the LAST thing a receipt?") and would report `cli-only` for
 * a device whose most recent update succeeded.
 * @param {any} log
 * @returns {object}
 */
export function diagnose_update(log) {
    const ret = wasm.diagnose_update(log);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * `disambiguateLabels(items)` — one label per item, numbered where they collide.
 *
 * THE INPUT IS AN `Array` AND A NON-ARRAY IS REFUSED, which is a deliberate narrowing of the
 * TypeScript rather than an accident: its `for (const item of items)` accepts any ITERABLE, so
 * `disambiguateLabels("ab")` walks the string's characters there and answers `["1·undefined", …]`.
 * That is not a shape this panel produces (both call sites pass a `Session[]`), and reproducing it
 * would mean writing a rule for an input nobody has — so the port throws instead, loudly, in the
 * one case the TypeScript would have silently invented labels for.
 * @param {any} items
 * @returns {Array<any>}
 */
export function disambiguate_labels(items) {
    const ret = wasm.disambiguate_labels(items);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * `groupEvents(events)` — the live trail's CARDS.
 *
 * A `command/start` while the previous command never ended closes it as `interrupted`, so a
 * mid-stream start cannot orphan a card that would otherwise read "running" forever.
 * @param {any} events
 * @returns {Array<any>}
 */
export function group_events(events) {
    const ret = wasm.group_events(events);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

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
 * @param {any} events
 * @param {any} boundaries
 * @returns {any}
 */
export function group_operation(events, boundaries) {
    const ret = wasm.group_operation(events, boundaries);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * `groupRounds(events)` — the trajectory's ROUNDS, which are `derivePath`'s input.
 *
 * A round is ENDED by a `command/end` OR by a terminal status, and **the LAST marker in the round
 * wins** — a backgrounded command can later log `closed`. A superseded round is sealed AS-IS (the
 * raw view: what the log says), which is where this deliberately disagrees with `groupEvents`.
 * @param {any} events
 * @returns {Array<any>}
 */
export function group_rounds(events) {
    const ret = wasm.group_rounds(events);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * HOW LONG A SILENCE LASTED — `45s`, `4m`, `1h 04m`.
 *
 * THE SHAPE IS THE PANEL'S DOMINANT ONE, with the seconds branch kept: under a minute, seconds are the
 * useful unit and `0m` is a lie about a 45-second silence.
 * @param {any} ms
 * @returns {string}
 */
export function human_idle(ms) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.human_idle(ms);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * `humanMs(ms)`.
 *
 * `Math.max(0, Math.round(ms / 1000))` — the coercion, `Math.round`'s half-toward-+INFINITY (which is
 * not `f64::round`), and `Math.max`'s NaN propagation, in that order.
 * @param {any} ms
 * @returns {string}
 */
export function human_ms(ms) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.human_ms(ms);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * ONE LINE for the offer: how many, and the longest silence among them.
 * @param {any} candidates
 * @returns {any}
 */
export function idle_offer_text(candidates) {
    const ret = wasm.idle_offer_text(candidates);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * The sessions nobody is using: live, silent for longer than the threshold, and NOT holding a command.
 *
 * CLOSED SESSIONS ARE NOT CANDIDATES: they have no shell to release (the device closed them when they
 * exited), and counting them would make the offer's number wrong. `savedOnly` is the same kind of
 * exclusion.
 * @param {any} sessions
 * @param {any} threshold_ms
 * @returns {Array<any>}
 */
export function idle_sessions(sessions, threshold_ms) {
    const ret = wasm.idle_sessions(sessions, threshold_ms);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * True for the one kind that means the agent died on its own. Used by the history card to weigh a row
 * and by the summary line to count; the chip has its own rule.
 * @param {any} kind
 * @returns {boolean}
 */
export function is_crash(kind) {
    const ret = wasm.is_crash(kind);
    return ret !== 0;
}

/**
 * The keys whose markers should be dropped: every key that is NOT in `reachable`.
 *
 * `new Set(reachable)` is the ITERABLE protocol, so an array, a string, a Set and a Map all work and a
 * non-iterable raises — which the corpus carries.
 * @param {any} keys
 * @param {any} reachable
 * @returns {Array<any>}
 */
export function lag_markers_to_drop(keys, reachable) {
    const ret = wasm.lag_markers_to_drop(keys, reachable);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

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
 * @param {any} input
 * @returns {string}
 */
export function liveness_of(input) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ret = wasm.liveness_of(input);
        var ptr1 = ret[0];
        var len1 = ret[1];
        if (ret[3]) {
            ptr1 = 0; len1 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred2_0 = ptr1;
        deferred2_1 = len1;
        return getStringFromWasm0(ptr1, len1);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

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
 * @param {any} samples
 * @param {any} now_ms
 * @param {any} window_ms
 * @param {any} min_samples
 * @returns {any}
 */
export function load_notice(samples, now_ms, window_ms, min_samples) {
    const ret = wasm.load_notice(samples, now_ms, window_ms, min_samples);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * The class list for a monitor mark. `flapping` wins over `up`: a target that is up now but has been
 * dropping is the thing the chip exists to say.
 *
 * `["monitor-mark", modifier, extra].filter(Boolean).join(" ")` — TRUTHINESS drops a falsy `extra`
 * (an empty string, `null`, `0`), and the join goes through the ENGINE because an `extra` can carry
 * anything a wire string can, a lone surrogate included.
 * @param {any} state
 * @param {any} extra
 * @returns {any}
 */
export function monitor_mark_class(state, extra) {
    const ret = wasm.monitor_mark_class(state, extra);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * THE MODIFIER ALONE — the same state, for the elements that carry the family's vocabulary without
 * being the mark itself.
 *
 * `MonitorAlerts` renders `<div className={"monitor-alert " + …}>` around a `<span className=
 * "monitor-mark …">`, and both describe ONE fact: this alert is up or down. Round 126 gave the mark
 * the derivation and left the container spelling the words by hand — the gate this module is guarded
 * by found it on its next run, which is what it is for.
 *
 * THE COMPARISON IS STRICT, and anything that is not `"up"` or `"down"` is `is-flapping` — which is
 * what the TypeScript's chained ternary does with a value that is neither.
 * @param {any} state
 * @returns {string}
 */
export function monitor_modifier(state) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.monitor_modifier(state);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * `operationRows(...)` — every row of the timeline, in the same groups `groupOperation` builds,
 * in the order the groups are rendered: "grouped by run, oldest group first, the unattributed
 * bucket last and separate".
 *
 * It takes the GROUPS rather than the raw events, because the grouping has exactly one
 * implementation above: the rows a reader sees cannot disagree with the counts a strip shows.
 * The TypeScript wrapper keeps its `(events, boundaries)` signature for its own callers and makes
 * the same two calls this does.
 * @param {any} groups
 * @returns {any}
 */
export function operation_rows(groups) {
    const ret = wasm.operation_rows(groups);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * `parseAttempt(v)` — the launch record, or null. A record without a POSITIVE time, a `from` and a
 * `to` is not one: half a record would render as "updated from to at Invalid Date".
 * @param {any} v
 * @returns {any}
 */
export function parse_attempt(v) {
    const ret = wasm.parse_attempt(v);
    return ret;
}

/**
 * `parseBootHistory(j)` — never throws, and never invents a value.
 * @param {any} j
 * @returns {any}
 */
export function parse_boot_history(j) {
    const ret = wasm.parse_boot_history(j);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

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
 * @param {any} detail
 * @returns {any}
 */
export function parse_evicted(detail) {
    const ret = wasm.parse_evicted(detail);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * `parseLastBoot(j)` — the last boot, or `null` when the body does not describe one.
 * @param {any} j
 * @returns {any}
 */
export function parse_last_boot(j) {
    const ret = wasm.parse_last_boot(j);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * `parseMonitorChange(detail)` — one `monitor-change` frame → one alert, or `null`.
 *
 * `null` and not a throw: this runs inside an event handler, and a frame this build cannot use must
 * not become an exception in a listener — nor a banner about something that did not happen.
 * @param {any} detail
 * @returns {any}
 */
export function parse_monitor_change(detail) {
    const ret = wasm.parse_monitor_change(detail);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * `parseMonitors(j)` — `GET /api/monitors`'s body → the monitor list.
 *
 * Never throws, and never invents a value: a body this build cannot use is an EMPTY list, a target
 * with no id is DROPPED (a target the panel cannot name is not one it can address), and a probe or
 * a transition with no usable stamp is dropped too — it cannot be placed on the time axis, and the
 * alternative is a chart drawn from guesswork.
 * @param {any} j
 * @returns {any}
 */
export function parse_monitors(j) {
    const ret = wasm.parse_monitors(j);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * `parseUpdateStatus(j)` — read `/api/update`. Never throws; a body this build cannot use is the
 * empty state, which renders as "unknown" rather than as "current".
 * @param {any} j
 * @returns {object}
 */
export function parse_update_status(j) {
    const ret = wasm.parse_update_status(j);
    return ret;
}

/**
 * `parseVitalsSeries(j)` — the series, or the empty one.
 *
 * THE OUTPUT KEYS ARE THE TYPESCRIPT'S (`tsMs`, `intervalSecs`, `spanSecs`, `memTotalMb`) and not
 * the wire's (`ts_ms`, `interval_secs`, `span_secs`, `mem_total_mb`). The wire names are read; the
 * camelCase names are what the panel's readers destructure, and a port that returned the wire's
 * spelling would type-check against `VitalsSeries` only if the interface were changed too — which
 * is a change to the panel, not a migration of it.
 * @param {any} j
 * @returns {any}
 */
export function parse_vitals_series(j) {
    const ret = wasm.parse_vitals_series(j);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * What the operator should be told about the current state, in the panel's own voice — the settings
 * card renders this verbatim, and it is the ONLY place the difference between "you never turned it
 * on", "the browser said no" and "this browser cannot" is explained.
 * @param {any} state
 * @returns {string}
 */
export function permission_hint(state) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.permission_hint(state);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * Which per-session views are still worth keeping.
 *
 * THE IDENTITY RETURN IS PART OF THE CONTRACT, not an optimisation. The caller's effect depends on the
 * session list, which changes on every poll; returning a fresh object whenever nothing was pruned
 * would re-render `App` once per poll for ever. **Unchanged in, SAME OBJECT out.**
 * @param {any} views
 * @param {any} live_sids
 * @returns {any}
 */
export function prune_session_views(views, live_sids) {
    const ret = wasm.prune_session_views(views, live_sids);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * `readPermission(ctor, permission)` — the browser's answer, read defensively.
 *
 * A page in an insecure context has no `Notification` at all, and that is a STATE, not an error: the
 * caller passes the constructor it found (or nothing) and the permission string it read (or nothing),
 * because reading `Notification.permission` is the BOUNDARY this module deliberately does not cross.
 *
 * `permission ?? …` IS NULLISH COALESCING, not truthiness: an EMPTY STRING is a value the caller read
 * and is passed through to the strict comparison below, where it answers `default`.
 * @param {any} ctor
 * @param {any} permission
 * @returns {string}
 */
export function read_permission(ctor, permission) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.read_permission(ctor, permission);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * Steps that make a recipe questionable — surfaced in the save form so the operator is not silently
 * saving a broken procedure as a good one.
 * @param {any} steps
 * @returns {Array<any>}
 */
export function recipe_warnings(steps) {
    const ret = wasm.recipe_warnings(steps);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * `releaseVersion(status)` — the npm release if the device sent one, else the frozen protocol version,
 * else nothing.
 *
 * `(status ?? {})` IS THE FIRST THING THAT HAPPENS, and it is why a nullish status answers `""` rather
 * than raising. Everything else is read off a BOXED value, because `(5).release` is `undefined` in
 * JavaScript — a primitive has no properties but is not an error either.
 * @param {any} status
 * @returns {string}
 */
export function release_version(status) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.release_version(status);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * `v1.2.354`, or `v?` when the device reported neither — never a bare "v".
 * @param {any} status
 * @returns {string}
 */
export function release_version_label(status) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.release_version_label(status);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * min / avg / max over the known readings, or `null` when there are none. Never invents a zero for
 * an empty series.
 * @param {any} values
 * @returns {any}
 */
export function series_stats(values) {
    const ret = wasm.series_stats(values);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * `sessionActive(session, workingMs)` — is the SESSION working, not "is the device busy".
 *
 * THE DEVICE'S ANSWER FIRST: `command_running` is the manager's own busy flag, so it is true for the
 * WHOLE life of a command, including the silent minutes that output recency cannot see. Recency is
 * the second signal, for work that is not a command through this path — and it is `typeof idleMs ===
 * "number"`, a TYPE test, so a session whose `idle_ms` arrived as a string is not a reading.
 * @param {any} session
 * @param {number} working_ms
 * @returns {boolean}
 */
export function session_active(session, working_ms) {
    const ret = wasm.session_active(session, working_ms);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return ret[0] !== 0;
}

/**
 * `sessionFailed(session)` — DID THIS SESSION'S LAST COMMAND FAIL, the device's own exit code.
 *
 * ABSENT IS NOT FAILURE and not success: `lastExitCode` is `null` when the device observed no code
 * at all, which is a third state a mark must not turn into either answer. NON-ZERO IS A FAILURE,
 * with no judgement about which codes deserve it — the command cards have called every non-zero exit
 * "Failed (exit N)" since they existed.
 * @param {any} session
 * @returns {boolean}
 */
export function session_failed(session) {
    const ret = wasm.session_failed(session);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return ret[0] !== 0;
}

/**
 * `sessionLiveness(session, workingMs)` — ONE DERIVATION FOR EVERY SURFACE.
 *
 * Nothing about a session's own mark needs the device: connectivity is the RAIL's fact, so a
 * disconnected device does not make every session `off` — which is what a CLOSED session means.
 * @param {any} session
 * @param {number} working_ms
 * @returns {string}
 */
export function session_liveness(session, working_ms) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ret = wasm.session_liveness(session, working_ms);
        var ptr1 = ret[0];
        var len1 = ret[1];
        if (ret[3]) {
            ptr1 = 0; len1 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred2_0 = ptr1;
        deferred2_1 = len1;
        return getStringFromWasm0(ptr1, len1);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * `sessionWaiting(session)` — CAN THIS SESSION STILL ANSWER, the ONE predicate the mark, the tab's
 * title and its aria-label all read.
 *
 * A CLOSED session's row keeps its data — the tombstone is the same record — so a question that
 * expired with the session it belonged to survives in `pendingApproval`. Without the `closed` half,
 * the desktop tab's title says "waiting for your approval" about a tab that cannot be answered at
 * all, which is the disagreement between the two densities this model exists to stop.
 * @param {any} session
 * @returns {boolean}
 */
export function session_waiting(session) {
    const ret = wasm.session_waiting(session);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return ret[0] !== 0;
}

/**
 * `shouldAcceptNavPush(m)`.
 *
 * Each `===` is STRICT and on the RAW value, so a numeric `inputValue` equal to a numeric
 * `valueAtFocus` answers true — and the comparisons are in the source's order, because each one is a
 * separate reason to follow the push.
 * @param {any} m
 * @returns {boolean}
 */
export function should_accept_nav_push(m) {
    const ret = wasm.should_accept_nav_push(m);
    return ret !== 0;
}

/**
 * `stored !== GETTING_STARTED_VERSION` — a STRICT comparison, so `null` (nothing stored, a fresh
 * install) opens the guide, and so does a stored value from any other version.
 * @param {any} stored
 * @param {any} version
 * @returns {boolean}
 */
export function should_show_guide(stored, version) {
    const ret = wasm.should_show_guide(stored, version);
    return ret !== 0;
}

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
 * @param {any} values
 * @param {any} width
 * @param {any} height
 * @returns {Array<any>}
 */
export function spark_segments(values, width, height) {
    const ret = wasm.spark_segments(values, width, height);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * Split text into per-frame write slices (pure, unit-tested).
 * @param {any} text
 * @param {any} size
 * @returns {Array<any>}
 */
export function split_write_slices(text, size) {
    const ret = wasm.split_write_slices(text, size);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * `stateFromEnd(ended, exitCode, reason)` — THE ONE DERIVATION OF A COMMAND'S STATE.
 *
 * `reason` is the STATUS string the trail carries (`backgrounded`, `closed`, `interrupted`,
 * `exited:3`), which is why feeding it through unchanged is what makes the views agree.
 * @param {any} ended
 * @param {any} exit_code
 * @param {any} reason
 * @returns {object}
 */
export function state_from_end(ended, exit_code, reason) {
    const ret = wasm.state_from_end(ended, exit_code, reason);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

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
 * @param {any} input
 * @returns {string}
 */
export function strip_ansi(input) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.strip_ansi(input);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * Title shown in the save form, derived from the path so the operator usually only has to confirm
 * it. Uses the FIRST command (what the run was about) and the step count.
 * @param {any} path
 * @returns {any}
 */
export function suggested_title(path) {
    const ret = wasm.suggested_title(path);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * `summarizePath(steps)` — how much work, how much of it failed, and how long it took.
 *
 * `commandMs` is a FLOOR, not a total: a backgrounded or still-running step has no duration, so
 * `untimed` counts them and the view says "at least" instead of implying a total it cannot know.
 * @param {any} steps
 * @returns {object}
 */
export function summarize_path(steps) {
    const ret = wasm.summarize_path(steps);
    return ret;
}

/**
 * `terminalStatus(st)` — the marker rule, or nothing for a status that ends nothing.
 *
 * `exited:<n>` is the one that carries a code, and the code is the DEVICE's: a non-numeric tail is
 * `NaN`, `Number.isFinite` says no, and the answer is a reason with no exit code.
 * @param {string} status
 * @returns {object | undefined}
 */
export function terminal_status(status) {
    const ptr0 = passStringToWasm0(status, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.terminal_status(ptr0, len0);
    return ret;
}

/**
 * `titleFor(items, base, tab)` — the tab title, or the base when there is nothing to say.
 *
 * `items.length` IS A NON-NEGATIVE INTEGER BELOW 2^32, which is the one place this file may use
 * Rust's formatter for a number: `String(n)` and `{}` agree on every such value (they diverge at
 * 1e21 and on `-0`, neither of which an array length can be). The crate's rule — ask the ENGINE,
 * never Rust's formatter — is about values a device can send, and this one cannot be sent at all.
 * @param {any} items
 * @param {string} base
 * @param {boolean} tab
 * @returns {string}
 */
export function title_for(items, base, tab) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(base, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.title_for(items, ptr0, len0, tab);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * The sentence a view shows while a read is in flight, or when it failed — or `null` when the caller's
 * own empty state is TRUE and may be shown.
 *
 * THE COMPARISONS ARE STRICT, so anything that is not exactly `"ok"` or `"unreadable"` takes the
 * in-flight branch — which is the safe direction: a state this build has never heard of is not a
 * licence to claim the session ran nothing.
 * @param {any} read
 * @returns {any}
 */
export function trail_read_notice(read) {
    const ret = wasm.trail_read_notice(read);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_boolean_get_5b446f51afd21013: function(arg0) {
            const v = arg0;
            const ret = typeof(v) === 'boolean' ? v : undefined;
            return isLikeNone(ret) ? 0xFFFFFF : ret ? 1 : 0;
        },
        __wbg___wbindgen_is_falsy_16bd49b68658263e: function(arg0) {
            const ret = !arg0;
            return ret;
        },
        __wbg___wbindgen_is_function_1f9d30630b8b1d3d: function(arg0) {
            const ret = typeof(arg0) === 'function';
            return ret;
        },
        __wbg___wbindgen_is_null_e343b7d08827ba72: function(arg0) {
            const ret = arg0 === null;
            return ret;
        },
        __wbg___wbindgen_is_object_3c45d4f2dde4e749: function(arg0) {
            const val = arg0;
            const ret = typeof(val) === 'object' && val !== null;
            return ret;
        },
        __wbg___wbindgen_is_undefined_8865fb403f8fe9d8: function(arg0) {
            const ret = arg0 === undefined;
            return ret;
        },
        __wbg___wbindgen_jsval_eq_02babf21faa37971: function(arg0, arg1) {
            const ret = arg0 === arg1;
            return ret;
        },
        __wbg___wbindgen_number_get_2e0e7dee9f701a71: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'number' ? obj : undefined;
            getDataViewMemory0().setFloat64(arg0 + 8 * 1, isLikeNone(ret) ? 0 : ret, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, !isLikeNone(ret), true);
        },
        __wbg___wbindgen_string_get_0380ccaa2f57f0d9: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'string' ? obj : undefined;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_throw_41e9ee4f547fc59a: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg___wbindgen_typeof_e777a26e115d416b: function(arg0) {
            const ret = typeof arg0;
            return ret;
        },
        __wbg_apply_a910804df6e1e433: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.apply(arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_call_187d372bd5fdd4aa: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.call(arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_call_6137034ef55c9d0f: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.call(arg1);
            return ret;
        }, arguments); },
        __wbg_done_b41a1d26cdb37fb6: function(arg0) {
            const ret = arg0.done;
            return ret;
        },
        __wbg_from_296ca31f8d0f1c52: function(arg0) {
            const ret = Array.from(arg0);
            return ret;
        },
        __wbg_get_31af05bd4842a84f: function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.get(arg0, arg1);
            return ret;
        }, arguments); },
        __wbg_get_464ae6d03ecb8ac7: function(arg0, arg1) {
            const ret = arg0.get(arg1);
            return ret;
        },
        __wbg_get_658f6698067d9515: function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.get(arg0, arg1);
            return ret;
        }, arguments); },
        __wbg_get_6c896e0571ddae51: function(arg0, arg1) {
            const ret = arg0[arg1 >>> 0];
            return ret;
        },
        __wbg_get_unchecked_288889d017702237: function(arg0, arg1) {
            const ret = arg0[arg1 >>> 0];
            return ret;
        },
        __wbg_instanceof_Object_67a83cdc00c5d141: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Object;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_isArray_2b41c29f43a3fb12: function(arg0) {
            const ret = Array.isArray(arg0);
            return ret;
        },
        __wbg_isArray_e15a2ff68ffdbef2: function(arg0) {
            const ret = Array.isArray(arg0);
            return ret;
        },
        __wbg_iterator_e3c31c892080e444: function() {
            const ret = Symbol.iterator;
            return ret;
        },
        __wbg_keys_440be172f17c0265: function(arg0) {
            const ret = Object.keys(arg0);
            return ret;
        },
        __wbg_length_d4bdea10311bd9cf: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_localeCompare_90de64421aef2322: function(arg0, arg1, arg2, arg3, arg4) {
            const ret = arg0.localeCompare(getStringFromWasm0(arg1, arg2), arg3, arg4);
            return ret;
        },
        __wbg_new_28744009d011f847: function() {
            const ret = new Map();
            return ret;
        },
        __wbg_new_2f0c455872a7873c: function(arg0, arg1, arg2, arg3) {
            const ret = new RegExp(getStringFromWasm0(arg0, arg1), getStringFromWasm0(arg2, arg3));
            return ret;
        },
        __wbg_new_343a093a3c2ffb4e: function(arg0, arg1) {
            const ret = new Error(getStringFromWasm0(arg0, arg1));
            return ret;
        },
        __wbg_new_617a8cdb8bb1130e: function() {
            const ret = new Object();
            return ret;
        },
        __wbg_new_ee2291f50781bf1d: function() {
            const ret = new Array();
            return ret;
        },
        __wbg_next_33784799010f1bbe: function(arg0) {
            const ret = arg0.next;
            return ret;
        },
        __wbg_next_f4aac29c42af995c: function() { return handleError(function (arg0) {
            const ret = arg0.next();
            return ret;
        }, arguments); },
        __wbg_push_2baf45db356cf468: function(arg0, arg1) {
            const ret = arg0.push(arg1);
            return ret;
        },
        __wbg_set_145a351398b48c65: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = Reflect.set(arg0, arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_set_6ae97e73113c4f0b: function(arg0, arg1, arg2) {
            const ret = arg0.set(arg1, arg2);
            return ret;
        },
        __wbg_static_accessor_GLOBAL_266715b9d96ba635: function() {
            const ret = typeof global === 'undefined' ? null : global;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_GLOBAL_THIS_10fb7dc1ae063179: function() {
            const ret = typeof globalThis === 'undefined' ? null : globalThis;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_SELF_0b583911f537483a: function() {
            const ret = typeof self === 'undefined' ? null : self;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_WINDOW_d7f903d1508cbdc4: function() {
            const ret = typeof window === 'undefined' ? null : window;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_toString_aad181a510c306d8: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.toString(arg1);
            return ret;
        }, arguments); },
        __wbg_value_f3c585ee8f5ba40c: function(arg0) {
            const ret = arg0.value;
            return ret;
        },
        __wbindgen_generic_0000000000000001: function(arg0) {
            // Cast intrinsic for `F64 -> Externref`.
            const ret = arg0;
            return ret;
        },
        __wbindgen_generic_0000000000000002: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./panel_logic_bg.js": import0,
    };
}

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (!module.ok) {
            throw new Error(`failed to fetch Wasm: ${module.status} ${module.statusText} fetching '${module.url}'`);
        }

        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('panel_logic_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
