/* tslint:disable */
/* eslint-disable */

/**
 * How long ago a device was last seen, in the fleet's one vocabulary.
 *
 * `ms` is NaN for "never" (the page maps its null to NaN at the seam), because a wasm-bindgen
 * `Option<f64>` would answer `None` for a legitimate 0 as well.
 */
export function age(ms: number, now_ms: number): string;

/**
 * The one sentence both surfaces use for a link that came back.
 */
export function retry_sentence(attempts: number, reason: string): string;

/**
 * The first twelve UTF-16 code units of an id, with an ellipsis only when something was cut.
 *
 * JS `id.slice(0, 12)` counts UTF-16 units, not Unicode scalars. A device id is 64 hex characters,
 * so the distinction never matters in practice; it is still implemented exactly, because a mirror
 * that only matches on the easy input is a guess.
 */
export function short_id(id: string): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly age: (a: number, b: number) => [number, number];
    readonly retry_sentence: (a: number, b: number, c: number) => [number, number];
    readonly short_id: (a: number, b: number) => [number, number];
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
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
