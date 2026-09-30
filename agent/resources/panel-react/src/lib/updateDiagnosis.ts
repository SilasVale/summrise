// updateDiagnosis — read `summrise-update.log` and answer "did the update take?".
//
// THE QUESTION THIS EXISTS FOR. A device update goes: the CLI writes an
// `update requested X -> Y` receipt, hands a PowerShell swap script to WMI, and
// the connection drops ~10s mid-swap. That drop is the DOCUMENTED signature of a
// successful swap — which makes it useless as evidence, because a transport
// failure that never delivered the command looks identical from the caller's
// side. Observed on d1: the update returned a connection error, was read as "the
// swap is running", and had never reached the device at all.
//
// The repo's own answer is a FOUR-WAY table over `summrise-update.log`, and until now
// reading it meant a `Get-Content` on the box. The device already serves the file
// (`GET /api/logs`) and nothing consumed it. This turns the tail into the verdict.
//
// The table, verbatim from the release docs:
//
//   receipt | `update start` | what it means
//   --------+----------------+-----------------------------------------------
//   present | absent         | the CLI reached the device, the swap never launched
//   present | present        | the CLI's swap launched; check copy/restart below
//   absent  | present        | the swap was launched by `agent_update` (Rust)
//   absent  | absent         | the command never reached the device at all
//
// PURE ON PURPOSE: no fetch, no React, no clock. The four-way logic is the part
// worth testing, and every input is a string.
//
// ── RUST SINCE 2026-09-30 (block ②), WITH `UpdateCard`'s READERS ─────────────────────────────────
//
// The whole derivation is `agent/resources/panel-logic/src/update.rs` now: the four-way table, the
// two hand-rolled regexes (`/copy ok\s*=\s*(true|false)/i`, `/task restarted/i`), the `log.trim()`
// and `log.split(/\r?\n/)` calls — including the fact that they are METHOD CALLS, so a number raises
// here exactly as it raised before. `components/UpdateCard.tsx`'s `parseUpdateStatus`, `parseAttempt`,
// `checkedAge` and `attemptAge` are in the same module, because they are the other end of the same
// question: the log says what HAPPENED, the device's answer says what is TRUE NOW.
//
// THE DIFFERENTIAL IS 422 CASES WITH 0 DIVERGENCES — the original TypeScript against the Rust, on the
// VALUES, the KEY ORDER and whether each side RAISED — and every arm of the five verdicts is reached.
// `src/lib/__tests__/updateDiagnosis.test.ts` and `components/__tests__/UpdateCard.test.tsx` run
// UNCHANGED against it.
import { logic } from "../wasm/panelLogic";

type UpdateVerdict =
  "cli-swap-launched" | "cli-only" | "rust-swap" | "never-arrived" | "no-log";

export interface UpdateDiagnosis {
  verdict: UpdateVerdict;
  /** One sentence an operator can act on, in product vocabulary. */
  summary: string;
  /** Whether an `update requested` receipt is the LAST thing of its kind. */
  receipt: string | null;
  /** The last completed swap's outcome lines, when the script got that far. */
  copyOk: boolean | null;
  restarted: boolean | null;
}

/**
 * The four-way verdict.
 *
 * PRESENCE IS DECIDED PER KIND, not by position: a receipt and a start can be
 * interleaved with other lines and with each other over several updates, and the
 * question is only whether each kind appears at all. Ordering them by line index
 * would answer a different question ("was the LAST thing a receipt?") and would
 * report `cli-only` for a device whose most recent update succeeded.
 *
 * An empty/absent log is its own verdict — `no-log` — NOT `never-arrived`: on a
 * device that has never been updated there is nothing to have arrived, and
 * collapsing the two would tell an operator their update was lost when none was
 * attempted.
 *
 * THE TYPE IS WIDER THAN THE WIRE (`string | null | undefined`): a log line the
 * device did not answer with is `null`, and the crate answers `no-log` for both.
 */
export function diagnoseUpdate(log: string | null | undefined): UpdateDiagnosis {
  return logic().diagnose_update(log) as UpdateDiagnosis;
}
