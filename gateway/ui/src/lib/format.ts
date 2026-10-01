/** Shared display formatters.
 *
 *  ── RUST SINCE 2026-09-30 (block ③), AND IT IS THE CONSOLE'S LAST PIECE OF LOGIC ───────────────
 *
 *  `maskToken` is `gateway/ui-logic/src/lib.rs` now. The differential is 30 corpus cases with 0
 *  divergences and every arm reached (6 empty answers, 21 masks, 3 raises); `test/format.test.mjs`
 *  runs UNCHANGED against it.
 *
 *  WHAT REMAINS IN TYPESCRIPT under `src/`, AND WHY EACH ONE IS A DECISION RATHER THAN AN OMISSION:
 *
 *    * `lib/keyNames.ts` — a VOCABULARY LIST (nine provider key names). P0's rule is "LOGIC if it
 *      computes, parses, derives, validates, formats or decides"; a constant list does none of those,
 *      and its whole value is being ONE declaration that `test/byok.test.mjs` compares against the
 *      worker's own `USER_KEY_NAMES`. Moving it would turn a module constant into a call that
 *      allocates an array per render, and the crate would hold a list the console's test checks
 *      against a WORKER's list.
 *    * `i18n.ts` — the dictionary and `t`. THE SEAM IS ALREADY DRAWN AROUND IT: two migrated families
 *      (`channelState.ts`, `deviceState.ts`) take `t` as a CALLBACK, because the WORD for a state
 *      belongs to the console's dictionary and Rust decides WHICH state. Moving the dictionary into
 *      the crate would reverse that seam — the crate would call itself through a JS shim — and put a
 *      boundary crossing plus a string allocation on every `t()` of every render, for 728 keys of
 *      DATA. It stays, and this paragraph is the reason.
 *
 *  So the console's LOGIC is either in the crate or named here. */
import { logic } from "../wasm/consoleLogic.ts";

/** Mask a secret for display: `vk-1ab…cdef`.
 *
 *  THE ANSWER CAN CONTAIN A LONE SURROGATE, which is why the crate builds it through the engine:
 *  `tok[0]` is the first UTF-16 UNIT, so a token beginning with an emoji is masked by a string that
 *  is half a character — and a Rust `String` is UTF-8 and cannot hold one. */
export function maskToken(tok: string | undefined | null): string {
  return logic().mask_token(tok) as string;
}
