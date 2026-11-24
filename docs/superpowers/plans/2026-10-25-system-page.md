# The System Page — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** An operator sees what is running on a device, ends a process deliberately, and tests whether a service is reachable — from the console.

**Architecture:** One console page in the rail (after Runs, before Audit) over the three existing `system.*` tools, through the same grant path as every other pane. No device or console route changes.

**Tech Stack:** React 19 + TypeScript, `callTool(grant, …)`, `useRequestGate`, the console's existing styles.

**Spec:** this plan; the device side is `device/device/src/capabilities/system.rs` and the schemas in `device/device/src/mcp.rs`.

## The shapes (verified in the code, not guessed)

- `system.process_list {name?}` → `[{pid, name, memory_kb}]` (name filters case-insensitively).
- `system.process_kill {pid | name}` → `{killed: [pid…], failed: [{pid, reason}]}`; it is destructive and audited; naming a `pid` or a `name`, not both.
- `system.net_test {host, port, timeout_ms?}` → `{addresses: [..], connected: string|null, latency_ms: number|null, error: string|null}`; the timeout defaults to 5 s and the device caps it at 60 s.
- These are the tools that end a process **on purpose**: the page must never make a kill one click away from a typo — the confirm names the process it is about to end.

## Global Constraints

- Console web files only (+ the rebuilt `dist/index.html`); no dependencies; no Rust.
- Every device call is `callTool(grant, …)`; no console route.
- The three states as Memory/Monitors/Runs: no device, no grant, view.
- Reads go through `useRequestGate`.
- English UI and comments.
- Gates: `tsc --noEmit` 0 errors; `build-web.sh` builds and `--check` is current; bundle committed.

## Review Focus

1. The kill confirm names the pid **and** the process name, and the row shows both — a pid reused by a different process must not be endable by inertia.
2. A kill that partly fails shows the failures (`{pid, reason}`), not a success sentence.
3. The filter is a device-side filter (`system.process_list {name}`), not a client-side one that hides processes the device would kill by name.
4. `net_test` shows the resolved addresses, the connected address and latency, and the device's `error` when nothing connected — a closed port is a result, not a page error.
5. No update/restart section is invented; when the device learns to report update state, that is its own design.

### Task 1: The System page

**Files:**
- Create: `device/console/web/src/SystemPage.tsx`
- Modify: `device/console/web/src/App.tsx`, `Rail.tsx`, `app.css`

- [ ] **Step 1:** Page, rail entry (after Runs, before Audit), App branch mirroring Runs.
- [ ] **Step 2:** Processes: a filter box + Refresh (gated reads), a table of pid/name/memory, and a Kill button per row opening a confirm that names pid and process; the result reports killed and failed.
- [ ] **Step 3:** Network: host, port, timeout (default 5000, 1..60000), a Test button, and a result block (addresses, connected + latency, or the device's error).
- [ ] **Step 4:** Gates, then commit with the rebuilt bundle.

### Task 2: Verify on the canary

- [ ] List the device's processes, filter by name, run a net test against a port that is open and one that is closed, and kill a process the page started (`runs.exec` a `ping` and end it). Record the evidence in the commit. (Blocked while the gate route is down.)
