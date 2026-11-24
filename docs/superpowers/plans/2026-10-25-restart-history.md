# The Restart History — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** The console says how the device last came back — which swap was applied or rolled back, and how often the launcher has repaired it from the baseline — from the two records the launcher already writes.

**Architecture:** Extends the `update` object the device already reports in `host.facts` (Task 1 of `2026-10-25-update-state`) with `last_swap` and `repairs`, read best-effort from `<install>/swap.result.json` and `<install>/repair.log`; the device card renders them beside the three generations. No new tool, no new route, no new frame.

**Tech Stack:** Rust (`device/src/host.rs`), React + TypeScript (`DeviceCard.tsx`). On top of `5672183`.

**Spec:** this plan; the writers are `SwapResult` and `append_repair_record` in `device/launch/src/lib.rs`, and the paths are `swap_result`/`repair_log` in `device/launch/src/paths.rs`.

## The shapes (verified in the writer, not guessed)

- `<install>/swap.result.json` → `SwapResult { outcome: "applied"|"rolled_back"|"refused", version?, sha256?, at_ms, reason? }`, rewritten by each swap.
- `<install>/repair.log` → one JSON line per restore: `{ ts_ms, kind: "repaired", from: "baseline"|"previous", sha256 }`.
- Both are the **launcher's** records, read for display: the same ruling as the sidecars — no signature verification here, and the comment says so.

## Global Constraints

- `host.facts().update` gains `last_swap` (object or null) and `repairs` (array, at most the newest 5), without changing the three generation keys.
- Best-effort: a missing or torn file is null / empty, never a failed call; a huge repair log is read from the tail, not into memory whole, or capped by the same "newest 5" rule.
- The card renders these only when present; the existing row is unchanged when both are absent.
- No new dependencies on either side; English.
- Gates: `cargo test -p summrise-device`, clippy `-D warnings`, fmt; `tsc --noEmit`; `build-web.sh` with the committed bundle.

## Review Focus

1. A `refused` swap is not an update: the card must not read "applied".
2. A torn `swap.result.json` is null, and the three generations still report.
3. The repair count is honest when the log is large or truncated mid-line (a partial last line is skipped, not counted).
4. No restart is invented: only what the two files hold.

### Task 1: The device's restart facts

**Files:**
- Modify: `device/device/src/host.rs`

**Interfaces:**
- Produces: `update.last_swap = {"outcome","version"?,"at_ms","reason"?}|null`; `update.repairs = [{"at_ms","from","sha256"}]` (newest first, ≤5).

- [ ] **Step 1:** Failing tests: a swap result parses (`applied` and `rolled_back`); a refused one keeps its reason; a torn swap file is null while the generations report; a repair log with three lines yields three records newest-first, a partial last line is skipped, and a 10-line log yields 5.
- [ ] **Step 2:** Implement both readers best-effort; keep the existing keys and their tests.
- [ ] **Step 3:** Gates; commit.

### Task 2: The card

**Files:**
- Modify: `device/console/web/src/DeviceCard.tsx`, `api.ts`

- [ ] **Step 1:** One more row (or the same row, extended): "last swap applied 0.3.30 · 2 h ago" / "last swap rolled back from 0.3.30: <reason>" / "repaired twice from the previous generation". Omit when absent; never say "updated" for a refusal.
- [ ] **Step 2:** Gates, commit.

### Task 3: Verify on the canary

- [ ] Read the rows against the canary's real `swap.result.json` and `repair.log`. Blocked while the gate route is down.
