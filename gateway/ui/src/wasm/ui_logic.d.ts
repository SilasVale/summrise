/* tslint:disable */
/* eslint-disable */

/**
 * THE BARE PREFIX (`or/` and `or` are one channel).
 *
 * `String(prefix ?? "")` — the JavaScript coerces ANY value (a number, `null`, `undefined`) rather
 * than only accepting a string, so the input stays a `JsValue` and the coercion is the ENGINE's
 * `ToString`, not Rust's formatter. That is this migration's standing rule: ask the engine for a
 * value's text, never `format!` — a `key` spelled `1e+21` on one side and
 * `1000000000000000000000` on the other is a different key.
 *
 * **AND THIS FUNCTION CLAIMED THAT RULE WHILE BREAKING IT (found 2026-09-29, by the test that wired
 * it).** The body was `JsString::from(prefix)`, which is an UPCAST and not a coercion: it reinterprets
 * the value as a string without converting it, so `as_string()` answers `None` for a number, a `null`
 * and an `undefined`, and every one of them became `""`. The TypeScript's `String(5)` is `"5"`. The
 * console's `test/lane.test.mjs` pins exactly that case, which is why it was found the moment the
 * function was reachable rather than the day a caller sent a number.
 *
 * The `?? ""` half is `is_null() || is_undefined()`: `String(null)` is `"null"`, and the TypeScript
 * asked for the empty string.
 */
export function bare_prefix(prefix: any): string;

/**
 * The words for that signal, given the provider's reason when it has one.
 *
 * `c.ok ? t("overview.healthOk") : c.reason || t("overview.healthDown")` — the `reason` wins over the
 * generic line, because a provider that says WHY is more useful than a label that says WHAT. The
 * `||` is a TRUTHINESS test on the reason as well: an empty string is no reason, and the TypeScript
 * falls through to the generic line for it.
 *
 * **AND IT RETURNS THE REASON AS IT ARRIVED, WHICH IS NOT A STRING AND IS THE TYPESCRIPT'S OWN
 * BEHAVIOUR.** `c.reason || t(…)` returns the VALUE: the first version of this port stringified it
 * through the engine and answered `"[object Object]"` where the JavaScript answers `{}`. The return
 * type has always said `string` while the body could hand back anything truthy, and the differential
 * is what turned that into a fact — so the port is faithful and the cast stays in the wrapper.
 */
export function channel_label(channel: any, t: Function): any;

/**
 * The signal for a channel: `ok` is the provider's own answer, anything else is a failure with a
 * reason the row shows.
 *
 * **TRUTHINESS, NOT `=== true`** — the TypeScript is `ok ? "ok" : "err"`, so a `1` is `ok` there and
 * `false` here would be a divergence on the same input.
 */
export function channel_signal(ok: any): string;

/**
 * A DIAL'S TONE, from "how many of N are well" — the same question the channels tile and the devices
 * tile both ask, and they used to answer it differently.
 *
 * `known` is separate from `total` because "we have not asked yet" is not "none are healthy" — the
 * same distinction `deviceState.ts` records for a probe has not answered.
 *
 * **BOTH COUNTS ARRIVE AS VALUES, NOT NUMBERS, AND THAT IS THE SECOND DIVERGENCE THE DIFFERENTIAL
 * FOUND.** `ok === total` is JavaScript's STRICT equality and `ok > 0` is a RELATIONAL comparison,
 * and they disagree on everything that is not a number: for `ok = true, total = 1` the first is
 * `false` and the second is `true`, so the TypeScript answers `warn` — and a port that took two
 * `f64` parameters turned that into `1 === 1` and answered `ok`. Reading the two through the
 * operators the TypeScript actually wrote is the whole fix; see `strictly_equals` and `to_number`.
 */
export function health_tone(known: any, ok: any, total: any): string;

/**
 * The lane class for a channel prefix, with its trailing slash ignored.
 */
export function lane_class(prefix: any): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly bare_prefix: (a: any) => [number, number];
    readonly channel_label: (a: any, b: any) => [number, number, number];
    readonly channel_signal: (a: any) => [number, number];
    readonly health_tone: (a: any, b: any, c: any) => [number, number];
    readonly lane_class: (a: any) => [number, number];
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
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
