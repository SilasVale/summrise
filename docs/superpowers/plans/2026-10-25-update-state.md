# The Device's Update State — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** The console's device card says which version is running, which one is staged, and which one is kept for a rollback — from the baseline generations the launcher already writes.

**Architecture:** The device reads the launcher's three baseline sidecars (`<install>/baseline/{current,previous,pending}/baseline.json`) inside its existing best-effort `host.facts`, which already travels on the channel to the console; the device card renders them. No new tool, no new route, no new frame.

**Tech Stack:** Rust (`device/src/host.rs`, `serde_json`), React + TypeScript (`DeviceCard.tsx`).

**Spec:** this plan; the launcher's layout is `device/launch/src/paths.rs` and the sidecar is `Sidecar { version, channel, sha256, signature }` in `device/launch/src/lib.rs`.

## What the facts mean, and what they do not

- `current` is the generation the launcher trusts; `pending` is a pair staged and not yet proven (an update in flight); `previous` is the pair a failed update returns to.
- These are **what the launcher wrote**, read for display. The device does not verify the sidecar's signature here — the launcher did that before it wrote the pair, and this is a fact about a file, not a trust decision. Say so in the code comment, or a later reader will think the device vouched for the version.
- Best-effort, like every other fact: a missing or malformed sidecar is `null`, never a failed call.

## Global Constraints

- The device's `host.facts` gains an `update` object; nothing else on the wire changes.
- The console renders it only when present; a device that reports no `update` renders exactly as it does today.
- No new dependencies on either side.
- English; `cargo test -p summrise-device`, clippy `-D warnings`, fmt; `tsc --noEmit`; `build-web.sh` with the committed bundle.

## Review Focus

1. A malformed or half-written sidecar yields `null` for that generation and the rest still report — the facts must not fail because one file is mid-write.
2. The install root comes from the running executable's own path, not from a hard-coded `C:\SummriseInstall`.
3. The card distinguishes "running" from "staged" without claiming the swap happened.
4. Nothing about the read is trusted: no signature check is implied, and the comment says so.

### Task 1: The device's update facts

**Files:**
- Modify: `device/device/src/host.rs` (+ its tests)
- Test: the same file's `mod tests`

**Interfaces:**
- Produces: `host.facts()["update"] = {"current": {"version","channel"}|null, "previous": …|null, "pending": …|null}`.
- Produces: `fn update_state(install: &Path) -> serde_json::Value` (the readable seam the tests drive).

- [ ] **Step 1:** Write the failing tests: a temp install with all three sidecars parses; one malformed sidecar is `null` while the others report; a missing baseline directory is three nulls; no install dir (a test passing a path that does not exist) never panics.
- [ ] **Step 2:** Implement `update_state`, wire it into `facts()` using `std::env::current_exe().parent()`, and keep it best-effort.
- [ ] **Step 3:** Gates; commit.

### Task 2: The card

**Files:**
- Modify: `device/console/web/src/DeviceCard.tsx` (the `HostFactsBlock`), `api.ts` (`HostFacts.update`)

- [ ] **Step 1:** A row that reads "staged 0.3.30 (canary) · current 0.3.29 · previous 0.3.28", omitting the parts that are null, and rendering nothing when `update` is absent.
- [ ] **Step 2:** Gates (`tsc`, `build-web.sh`), commit.

### Task 3: Verify on the canary

- [ ] Read the card's update row against the canary's real baseline directories; stage (not apply) a release and watch `pending` appear. Blocked while the gate route is down.
