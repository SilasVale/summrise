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
 * The lane class for a channel prefix, with its trailing slash ignored.
 */
export function lane_class(prefix: any): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly bare_prefix: (a: any) => [number, number];
    readonly lane_class: (a: any) => [number, number];
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
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
