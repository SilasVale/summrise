# Test Hardening — the Sampled Lists and the Unpinned Derivation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Two tests stop being samples: the authorize list covers the whole registry, and the memory id derivation is pinned to a literal, with the cases the reviews asked for.

**Architecture:** No production code changes. Two test files: `device/device/src/http.rs` (the authorize test iterates `summrise_core::registry::CAPABILITIES` instead of a hand-written list that has already drifted twice) and `device/device/src/capabilities/memory.rs` (a literal hash, a third duplicate, export-vs-search id equality, and an update-vs-delete race).

**Spec:** this plan; the findings are the ledger's "minor" lines from the Task 1/2 reviews of `2026-10-25-memory-entry-ids` and the final review's "authorize test subset omits the new tools".

## Global Constraints

- Test-only changes. A production line that changes for a test is a finding, not a convenience.
- The pinned hash must be computed **independently** of the implementation (e.g. `printf '%s' '<line>' | sha256sum`), not by printing what the code produces — otherwise the test pins the bug.
- Repository language is English.
- Gates: `cargo test -p summrise-device`, `cargo clippy -p summrise-device --all-targets -- -D warnings`, `cargo fmt --all -- --check`.

## Review Focus

1. The authorize test fails if a registry tool is *not* allowed — not just that 38 names are allowed.
2. The pinned id is a literal that appears nowhere else; changing the hash in the code makes it fail.
3. The `-3` case asserts three distinct ids and that deleting the middle one leaves the other two searchable by their own ids.
4. The race asserts exactly one of two concurrent writers wins and the loser is a typed refusal (never both succeed, never a corrupt store).

### Task 1: The two tests

**Files:**
- Modify: `device/device/src/http.rs` (the authorize tests)
- Modify: `device/device/src/capabilities/memory.rs` (its test module)

- [ ] **Step 1:** Replace the authorize test's literal tool list with the registry's tools, and add the mirrored negative: a tool name that is not in the registry is refused with the same shape as before.
- [ ] **Step 2:** Pin `legacy_id` to a literal for a known line (computed with `sha256sum`, not by running the code), and keep the existing `-2` assertion.
- [ ] **Step 3:** Add the `-3` duplicate case and the export-vs-search id equality assertion.
- [ ] **Step 4:** Add an update-vs-delete race on one id: two threads, one updates and one deletes; assert exactly one succeeded, the other is `NoSuchEntry`, and the store replays consistently.
- [ ] **Step 5:** Gates, commit.
