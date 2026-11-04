/**
 * The Rust/WASM logic seam.
 *
 * The console page is React for rendering; the decisions are Rust. `device/console-logic` compiles
 * to one wasm module, and this file is the only place the page knows that: it decodes the module
 * bytes once, instantiates them, and exports the functions the components import. There is no fetch
 * and no second artifact — the bytes are inlined into the single HTML the console binary embeds.
 *
 * Top-level await is deliberate: a component must never render with half-loaded logic. The module is
 * 21 KB and compiles in single-digit milliseconds; the bundle waits for it, as it waits for its own
 * code.
 */
import init, { age as wasmAge, retry_sentence, short_id } from "./wasm/console_logic";
import { CONSOLE_LOGIC_WASM_BASE64 } from "./wasm/console_logic_bg";

/** Decode the build-time base64 into the bytes wasm-bindgen instantiates from. */
function bytes(base64: string): Uint8Array {
  const binary = atob(base64);
  const out = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) out[index] = binary.charCodeAt(index);
  return out;
}

await init({ module_or_path: bytes(CONSOLE_LOGIC_WASM_BASE64) });

/** The first twelve UTF-16 code units of an id, with an ellipsis only when something was cut. */
export function shortId(id: string): string {
  return short_id(id);
}

/** How long ago a device was last seen. `null` is the page's "never". */
export function age(ms: number | null): string {
  return wasmAge(ms === null ? Number.NaN : ms, Date.now());
}

/**
 * How long before `nowMs` a timestamp was.
 *
 * The same formatter as [`age`], with the clock supplied instead of read: a paused page stops at the
 * moment it was paused, and its age column must stop with it.
 */
export function ageAt(ms: number | null, nowMs: number): string {
  return wasmAge(ms === null ? Number.NaN : ms, nowMs);
}

/** The one sentence both surfaces use for a link that came back. */
export function retrySentence(attempts: number, reason: string): string {
  return retry_sentence(attempts, reason);
}
