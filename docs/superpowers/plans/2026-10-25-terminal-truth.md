# The Terminal Tells the Truth — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** An operator can open an SSH or serial session from the console, the tabs survive a page change, and a terminal that stops answering says why.

**Architecture:** Console only, over the existing `terminal.*` tools. Three gaps the parity sweep found (`2026-10-25-legacy-parity-sweep.md` §1–§3): `+ New` is pty-only and a saved SSH preset calls `terminal.create` with no `auth` (which the device refuses); `App` unmounts the workspace on a rail click; `poll()`/`write` swallow every failure.

**Tech Stack:** React 19 + TypeScript, `callTool(grant, …)`, `xterm.js` (already used), the console's existing styles.

**Spec:** this plan and the sweep. Device side: `terminal.create` takes `{kind, auth?, cols, rows, …}` — read `device/device/src/mcp.rs`'s `terminal.create` schema and `device/device/src/capabilities/terminal.rs` for the exact `auth` shapes before writing the form.

## Global Constraints

- Console web files only (+ the rebuilt `dist/index.html`); no dependencies; no Rust.
- Every device call is `callTool(grant, …)`; no console route.
- **Credentials are per open and never stored**: the password/key path lives in the form's state and the request, and nothing writes it to localStorage, a URL, or a preset.
- A tab that dies says what happened in the tab, not in a console log.
- English UI and comments; `tsc --noEmit` 0; `build-web.sh` builds and `--check` is current; bundle committed.

## Review Focus

1. The SSH form sends the `auth` the device expects (password **or** key path), and the preset dialog no longer promises a credential the form never asked for.
2. Serial gets its framing fields (parity, data bits, stop bits) only when it is serial.
3. A tab survives navigating to another page and back, still showing its output, without re-attaching by hand.
4. A grant that expired, a device that went offline, and a session the device closed are three different sentences.
5. No credential reaches a preset file, `localStorage`, or a URL.

### Task 1: The connect dialog

**Files:** `device/console/web/src/TerminalWorkspace.tsx`

- [ ] **Step 1:** `+ New` opens a dialog with kind (pty/ssh/serial); ssh asks host, port (default 22), user, and **either** a password **or** a key path (one radio, both optional-but-one-required); serial asks the port path and framing; pty asks nothing.
- [ ] **Step 2:** The request sends exactly what `terminal.create` accepts for that kind (no `auth` for pty, no framing for ssh).
- [ ] **Step 3:** The saved-preset dialog keeps only non-secret fields and says so; a preset ssh entry opens prefilled and asks for the credential.

### Task 2: Tabs that survive a page change

**Files:** `device/console/web/src/App.tsx`, `TerminalWorkspace.tsx`

- [ ] **Step 1:** Lift the tab list and the active tab to `App` (or keep the workspace mounted while the page is not visible), so a rail click does not unmount the sessions.
- [ ] **Step 2:** On return, a tab whose session is still live re-attaches; one whose session is gone shows the closed sentence (Task 3) rather than an empty screen.

### Task 3: A terminal that stops answering says why

**Files:** `TerminalWorkspace.tsx`

- [ ] **Step 1:** Per-tab error state: a failed `terminal.read` marks the tab "unreachable" with the device's sentence; a failed `terminal.write` says the keystrokes were not sent instead of dropping them silently.
- [ ] **Step 2:** On a failure, probe `terminal.list` once: a session the device no longer has is "closed" (with the device's reason when it gives one), a probe that also fails is "unreachable", and a grant refusal names the grant.
- [ ] **Step 3:** Gates, commit with the rebuilt bundle.

### Task 4: Verify on the canary

- [ ] pty, ssh and serial sessions opened from the dialog; a page change and back; a session killed by the device (`terminal.close` from another tab) shows the closed sentence. Blocked while the gate route is down.
