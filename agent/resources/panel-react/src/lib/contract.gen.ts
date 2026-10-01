// GENERATED — do not edit. Source of truth: agent/src/vocabulary.rs
// Refresh: SUMMRISE_REFRESH_CONTRACT=1 cargo test contract_vocabulary_snapshot
// Checked by agent/tests/contract_vocabulary.rs, which fails BY NAME when either end drifts.

/** The `ev` field of every control frame the device pushes on /api/events/term. */
export const FRAMES = ["sessions-changed","term-output","session-evicted","playwright-changed","monitor-change"] as const;

/** How the run before this one ended (/api/status.last_boot_kind, /api/boots[].kind).
*  THE ARRAY IS NOT EMITTED ANY MORE, AND THAT IS A MOVE, NOT A RETIREMENT — the same
*  one `END_REASONS` took below. Its only reader was `lib/bootNotice.ts`'s `isKnownKind`,
*  and the boot notice is `panel-logic/src/boot_notice.rs` now, where the membership
*  test reads the crate's OWN `vocabulary.rs`. A list nobody imports is a promise
*  nobody keeps, and `agent/tests/exports_check.rs` refuses one. The TYPE stays: two
*  panel signatures take it. */
export type Frame = (typeof FRAMES)[number];
export type BootKind = "first-run" | "clean-exit" | "replaced" | "machine-restart" | "crashed";

// `END_REASONS`, `EXITED_PREFIX` and `EndReason` ARE NOT EMITTED ANY MORE, AND THAT IS
// A MOVE, NOT A RETIREMENT (2026-09-29, P2). `lib/path.ts` stateFromEnd was their only
// reader and it is `panel-logic/src/path.rs` now, where the list and the table it keys
// live in ONE crate — and `agent/tests/contract_vocabulary.rs` compares that copy
// against `contract-vocabulary.json` on every run. The JSON still carries both: it is
// the contract the gate reads.
