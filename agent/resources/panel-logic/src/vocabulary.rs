//! The wire VOCABULARY this crate has to know, in ONE place.
//!
//! WHY THIS MODULE EXISTS. `boot.rs` moved the restart history's parse into Rust and needed the
//! five boot kinds; its own header said what that cost and what the fix was — *"`KINDS` used to
//! live in the TypeScript, and it is a copy of `BOOT_KINDS` in `lib/contract.gen.ts`, which is
//! GENERATED from `agent/src/vocabulary.rs`. It is still a copy after this move — of the same list,
//! in a third language. The right fix is one Rust home for the vocabulary that both crates depend
//! on (or a generator that emits this file too); it is named in the commit message rather than done
//! here, because it touches the contract generator and the gate that watches it."*
//!
//! THE SECOND FAMILY THAT NEEDED IT IS WHAT MADE IT WORTH DOING: `parse_last_boot` reads
//! `last_boot_kind` and validates it against the SAME five strings. A third copy of one list is
//! where a drift starts being likely rather than possible, so the list moved here and `boot.rs`
//! reads it from here.
//!
//! ── WHAT THIS IS NOT, AND IT MATTERS ────────────────────────────────────────────────────────────
//!
//! IT IS STILL A COPY, and this module does not pretend otherwise. The authority is
//! `agent/src/vocabulary.rs`, which the contract generator reads to emit
//! `agent/resources/panel-react/src/lib/contract.gen.ts`; the wasm crate is NOT in the agent's
//! workspace (its Cargo.toml says why: a wasm cdylib must never be built for
//! x86_64-pc-windows-msvc), so it cannot `use` that module directly. What changed is the COUNT:
//! one copy inside this crate instead of one per family.
//!
//! THE DRIFT IS CAUGHT BY A GATE, NOT BY THIS COMMENT. `agent/tests/contract_vocabulary.rs` reads
//! `agent/contract-vocabulary.json` and checks the strings the panel and the agent both spell — the
//! five boot kinds among them. A value changed in `vocabulary.rs` without changing it here fails
//! there, which is the arrangement the third-language copy needs in order to be safe.

/// The kinds the device writes into `last_boot_kind` and into a boot record's `kind`.
///
/// The order is the TypeScript's (`lib/contract.gen.ts`'s `BOOT_KINDS`, emitted from
/// `vocabulary.rs`) and it is kept because a `find` over a list is order-sensitive only for
/// duplicate entries — of which there are none — and because a reader comparing the two files should
/// not have to diff them.
pub const BOOT_KINDS: [&str; 5] = [
    "first-run",
    "clean-exit",
    "replaced",
    "machine-restart",
    "crashed",
];

/// `BOOT_KINDS.find((k) => k === raw) ?? null` — the kind, or nothing.
///
/// A STRICT MEMBERSHIP TEST, which is the TypeScript's own: an unrecognised kind is NOT the nearest
/// one and not a default. `boot.rs` renders it as "unrecorded" and `parseLastBoot` answers
/// `kind: null`, and both are the same refusal to put another kind's words under this one's name.
pub fn boot_kind(raw: &str) -> Option<&'static str> {
    BOOT_KINDS.iter().copied().find(|k| *k == raw)
}

/// The ways a command END that the device names, in the contract's order (`END_REASONS` in
/// `lib/contract.gen.ts`, generated from `agent/src/vocabulary.rs`).
///
/// MOVED HERE 2026-09-29 for `path.rs`'s `stateFromEnd`, which reads these strings to decide a state.
/// The order and the spellings are the contract's, byte for byte, and `contract_vocabulary.rs` now
/// pins this list against the source of truth — so a value changed in `agent/src/vocabulary.rs`
/// without changing it here fails a gate rather than drifting silently. That is the arrangement the
/// module header PROMISED for `BOOT_KINDS` and did not have; this list carries it, and the promise is
/// now true because two lists are covered rather than one.
pub const END_REASONS: [&str; 6] = [
    "marker",
    "idle",
    "timeout",
    "interrupted",
    "backgrounded",
    "closed",
];

/// The prefix the device puts before a numeric exit (`exited:3`) — `EXITED_PREFIX` in
/// `lib/contract.gen.ts`, from the same source of truth. `stateFromEnd` checks a reason starts with
/// it; the number after it is the device's, not this crate's.
pub const EXITED_PREFIX: &str = "exited:";

/// `END_REASONS.includes(raw)` — a strict membership test, exactly as the TypeScript's
/// `(END_REASONS as readonly string[]).includes(reason)` is: a reason the device invented is not the
/// nearest one, it is an ending this build cannot name.
pub fn end_reason(raw: &str) -> Option<&'static str> {
    END_REASONS.iter().copied().find(|k| *k == raw)
}
