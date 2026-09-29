// Strip ANSI escape sequences for plain-text rendering. The live terminal
// interprets these via xterm.js; text surfaces (trajectory timeline, command
// cards) must not print them raw — raw bytes rendered as text showed cursor
// moves ([2J[H), SGR color runs ([93m), OSC titles and semantic-prompt
// markers (]133;D;) as visible garbage (round-134).
// THE FOUR PATTERNS MOVED WITH THE FUNCTION. They are quoted here because they are the RULE, and a
// reader comparing this file to `events.rs`'s scanner wants to see that the Rust arm for arm is the
// same four — CSI (`ESC [` params final), OSC (to BEL, to ST, or to end of input), DCS (ESC [ P^_ …
// ST) and SINGLE (ESC [ =>78 6MNOc]) — and that the DCS rule SHARES its `[` with CSI, so the
// alternation backtracks between them.

/** Remove ANSI escapes; any stray ESC that survived pattern matching is
 *  dropped too, so control bytes can never reach the DOM as text. */
import { logic } from "../wasm/panelLogic";

export function stripAnsi(s: string | undefined): string {
  return logic().strip_ansi(s);
}
