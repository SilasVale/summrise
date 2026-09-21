//! THE WIRE VOCABULARY — the strings the DEVICE writes and the INTERFACES read.
//!
//! WHY THIS MODULE EXISTS (round 44 of the standing goal). Every string below crosses the wire: the device writes it,
//! and the panel (or the console, or a model over MCP) switches on it. Until now each end spelled them independently —
//! `"monitor-change"` in `monitor.rs` and again in the panel's `useMonitors`, `"crashed"`/`"replaced"` in `runstate.rs`
//! and again in `bootNotice.ts`, the command end reasons in `exec.rs` and again in `stateFromEnd`. **Two hand-written
//! copies of one fact is the exact shape this objective exists to remove**, and the copies were kept in step only by
//! tests that read ONE side each.
//!
//! WHAT CHANGES WITH THIS FILE: the strings are stated once, here. `contract_vocabulary_snapshot` writes two artifacts
//! from them — `agent/contract-vocabulary.json` (the machine-readable face) and
//! `agent/resources/panel-react/src/lib/contract.gen.ts` (the panel's typed copy) — and
//! `scripts/test/contract-vocabulary-check.mjs` proves that the Rust sources, both artifacts and the interfaces all
//! still agree. A string that drifts in any one of those places fails CI by name.
//!
//! WHAT DOES NOT LIVE HERE: the mapping from a reason to a *state* (`stateFromEnd`), or from a boot kind to a tone
//! (`bootNotice`) — those are the interfaces' own decisions, and they belong to the surface that renders them. This
//! module is only the vocabulary both ends must SPELL the same way.

/// The `ev` field of every control frame the device pushes on `/api/events/term`. The panel receives each one as a
/// `vale-<ev>` window event, so a typo on either side is a state that silently never arrives.
///
/// MECHANICAL, NOT REMEMBERED: `grep -rhoP '"ev"\s*:\s*"\K[a-z-]+' agent/src --include=*.rs` is where this list comes
/// from, and the gate runs the same scan — a new `"ev"` literal anywhere in the agent fails until it is listed here.
pub const FRAMES: &[&str] = &[
    "sessions-changed",
    "term-output",
    "session-evicted",
    "playwright-changed",
    "monitor-change",
];

/// How the run BEFORE this one ended — `/api/status.last_boot_kind` and `/api/boots[].kind`.
///
/// NOT A SECOND COPY: `runstate::BootKind::as_str` is the enum's own spelling, and `boot_kinds_match_the_enum` below
/// fails if this list and the enum ever disagree. The list exists so the JSON artifact can carry it.
/// FIVE, NOT FOUR: this list was first written with `crashed` as the last entry and `machine-restart` was missing
/// entirely — a kind the device has emitted since it learned to tell "the host rebooted" from "the process died". The
/// test below compares this against `BootKind::ALL`, which is how it was caught inside the same round.
pub const BOOT_KINDS: &[&str] = &[
    "first-run",
    "clean-exit",
    "replaced",
    "machine-restart",
    "crashed",
];

/// Why a command ended, as recorded on `command/end` and switched on by the panel's `stateFromEnd`.
///
/// `exited` is a PREFIX, not a value: the device writes `exited:<code>` with its own number, which is why
/// [`EXITED_PREFIX`] is separate. The panel's own type comment lists the same seven (`marker / idle / timeout /
/// interrupted / backgrounded / closed / exited:N`) — that comment is what this constant makes checkable.
pub const END_REASONS: &[&str] = &[
    "marker",
    "idle",
    "timeout",
    "interrupted",
    "backgrounded",
    "closed",
];
pub const EXITED_PREFIX: &str = "exited:";

/// The JSON artifact `contract-vocabulary-check.mjs` reads. Written by `contract_vocabulary_snapshot`.
pub fn as_json() -> serde_json::Value {
    serde_json::json!({
        "frames": FRAMES,
        "boot_kinds": BOOT_KINDS,
        "end_reasons": END_REASONS,
        "exited_prefix": EXITED_PREFIX,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// THE LIST IS A VIEW OF THE ENUM, NOT A COPY OF IT. `runstate.rs` already spells the boot kinds once (in
    /// `BootKind::as_str`); this asserts the constant above agrees with it rather than trusting two hand-written lists
    /// to stay in step — which is the whole reason this module exists.
    #[test]
    fn boot_kinds_match_the_enum() {
        let from_enum: Vec<&str> = crate::runstate::BootKind::ALL
            .iter()
            .map(|k| k.as_str())
            .collect();
        let mut mine = BOOT_KINDS.to_vec();
        let mut theirs = from_enum.clone();
        mine.sort_unstable();
        theirs.sort_unstable();
        assert_eq!(
            mine, theirs,
            "vocabulary::BOOT_KINDS and runstate::BootKind::as_str spell different boot kinds"
        );
    }

    /// A REASON THAT IS A PREFIX IS NOT A REASON. `exited:0` must not be listed as a value, or the panel would
    /// compare against a literal that never appears; and the prefix must not be one of the values either.
    #[test]
    fn the_exited_prefix_is_not_also_a_value() {
        assert!(
            EXITED_PREFIX.ends_with(':'),
            "the prefix must be recognisable as one"
        );
        assert!(
            !END_REASONS.contains(&EXITED_PREFIX.trim_end_matches(':')),
            "`exited` is listed as a value AND used as a prefix"
        );
    }

    /// Writes both artifacts, or checks them. Refresher, exactly like the tool spec's:
    /// `VALE_REFRESH_CONTRACT=1 cargo test --features terminal,keyring contract_vocabulary_snapshot`.
    #[test]
    fn contract_vocabulary_snapshot() {
        let dir = env!("CARGO_MANIFEST_DIR");
        let json_path = format!("{dir}/contract-vocabulary.json");
        let rendered = format!(
            "// The wire vocabulary: the strings the device writes and the interfaces read.\n\
             // Generated from agent/src/vocabulary.rs by\n\
             // vocabulary::tests::contract_vocabulary_snapshot.\n\
             // The panel's typed copy is resources/panel-react/src/lib/contract.gen.ts; both are checked by\n\
             // scripts/test/contract-vocabulary-check.mjs.\n\
             // Do not hand-edit: VALE_REFRESH_CONTRACT=1 cargo test contract_vocabulary_snapshot, then commit.\n{}\n",
            serde_json::to_string_pretty(&as_json()).unwrap()
        );
        let ts_path = format!("{dir}/resources/panel-react/src/lib/contract.gen.ts");
        let ts = format!(
            "// GENERATED — do not edit. Source of truth: agent/src/vocabulary.rs\n\
             // Refresh: VALE_REFRESH_CONTRACT=1 cargo test contract_vocabulary_snapshot\n\
             // Checked by scripts/test/contract-vocabulary-check.mjs, which fails by name when either end drifts.\n\n\
             /** The `ev` field of every control frame the device pushes on /api/events/term. */\n\
             export const FRAMES = {frames} as const;\n\n\
             /** How the run before this one ended (/api/status.last_boot_kind, /api/boots[].kind). */\n\
             export const BOOT_KINDS = {boots} as const;\n\n\
             /** Why a command ended. `exited` is a PREFIX: the device writes `exited:<code>`. */\n\
             export const END_REASONS = {reasons} as const;\n\
             export const EXITED_PREFIX = {prefix};\n\n\
             export type Frame = (typeof FRAMES)[number];\n\
             export type BootKind = (typeof BOOT_KINDS)[number];\n\
             export type EndReason = (typeof END_REASONS)[number];\n",
            frames = serde_json::to_string(FRAMES).unwrap(),
            boots = serde_json::to_string(BOOT_KINDS).unwrap(),
            reasons = serde_json::to_string(END_REASONS).unwrap(),
            prefix = serde_json::to_string(EXITED_PREFIX).unwrap(),
        );

        let refresh = std::env::var("VALE_REFRESH_CONTRACT").is_ok_and(|v| !v.is_empty());
        for (path, want) in [(&json_path, &rendered), (&ts_path, &ts)] {
            if refresh {
                std::fs::write(path, want).unwrap_or_else(|e| panic!("write {path}: {e}"));
                continue;
            }
            let have = std::fs::read_to_string(path).unwrap_or_else(|e| {
                panic!("{path} missing ({e}) — run VALE_REFRESH_CONTRACT=1 cargo test contract_vocabulary_snapshot")
            });
            assert_eq!(
                have.trim_end(),
                want.trim_end(),
                "{path} is stale vs agent/src/vocabulary.rs — run \
                 VALE_REFRESH_CONTRACT=1 cargo test contract_vocabulary_snapshot and commit it"
            );
        }
    }
}
