// GENERATED — do not edit. Source of truth: agent/src/vocabulary.rs
// Refresh: VALE_REFRESH_CONTRACT=1 cargo test contract_vocabulary_snapshot
// Checked by scripts/test/contract-vocabulary-check.mjs, which fails by name when either end drifts.

/** The `ev` field of every control frame the device pushes on /api/events/term. */
export const FRAMES = ["sessions-changed","term-output","session-evicted","playwright-changed","monitor-change"] as const;

/** How the run before this one ended (/api/status.last_boot_kind, /api/boots[].kind). */
export const BOOT_KINDS = ["first-run","clean-exit","replaced","machine-restart","crashed"] as const;

/** Why a command ended. `exited` is a PREFIX: the device writes `exited:<code>`. */
export const END_REASONS = ["marker","idle","timeout","interrupted","backgrounded","closed"] as const;
export const EXITED_PREFIX = "exited:";

export type Frame = (typeof FRAMES)[number];
export type BootKind = (typeof BOOT_KINDS)[number];
export type EndReason = (typeof END_REASONS)[number];
