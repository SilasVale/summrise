# The Files Page — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** An operator browses, reads, edits and uploads files inside the device's files root, from the console.

**Architecture:** One console page in the rail (after Devices, before Terminal — files are the thing an operator reaches for first on a device) over the four existing tools (`files.list`, `files.read`, `files.write`, `files.mkdir`), through the same grant path as every other pane. No device or console route changes.

**Tech Stack:** React 19 + TypeScript, `callTool(grant, …)`, `useRequestGate` for its reads, the console's existing styles.

**Spec:** this plan; the device side is `device/device/src/capabilities/files.rs` and the schemas in `device/device/src/mcp.rs`.

## The shapes (verified in the code, not guessed)

- `files.list {path}` → `[{name, kind: "file"|"directory"|"other", len}]`, sorted, ≤1000 entries; `""` is the root. A larger directory is a refusal.
- `files.read {path, encoding?: "utf8"|"base64"}` → the text, or base64 for a file that is not text; ≤1 MiB, and the utf8 read *fails* on a binary file (the refusal is the signal).
- `files.write {path, content, encoding?}` replaces the file's bytes; the parent must exist; ≤1 MiB decoded.
- `files.mkdir {path}` creates one directory; its parent must exist.
- There is **no delete tool**. The page says so rather than offering a button the device will refuse.

## Global Constraints

- Console web files only (+ the rebuilt `dist/index.html`); no dependencies; no Rust.
- Every device call is `callTool(grant, …)`; no console route.
- The three states as Memory/Monitors/Runs: no device, no grant, view.
- Reads go through `useRequestGate` (the newest answer wins; a stale error is dropped).
- English UI and comments; 1 MiB shown as the limit where it matters.
- Gates: `tsc --noEmit` 0 errors; `build-web.sh` builds and `--check` is current; bundle committed.

## Review Focus

1. A binary file is a fact, not an error page: the utf8 refusal offers the base64 read (or a download), and the page does not show raw bytes as mojibake.
2. Path handling cannot escape the root: the page only ever sends paths built from listed entries and the current directory; no `..` is synthesised.
3. Saving warns that it replaces the whole file, and stays disabled until something changed.
4. An upload larger than 1 MiB is refused in the page with that sentence, before the device.
5. A directory that is too large to list shows the device's sentence, not an empty list.

### Task 1: The Files page

**Files:**
- Create: `device/console/web/src/FilesPage.tsx`
- Modify: `device/console/web/src/App.tsx`, `Rail.tsx`, `app.css`

- [ ] **Step 1:** Page, rail entry (after Devices, before Terminal), App branch mirroring Memory.
- [ ] **Step 2:** Breadcrumb + listing: directories first (click to descend), files (click to open), sizes; a ".." entry only when the current path is not the root.
- [ ] **Step 3:** Viewer/editor: utf8 text in a textarea with Save; on a utf8 refusal, offer "Open as base64" and a Download button (Blob from base64); Save disabled until dirty, with the "replaces the file" note.
- [ ] **Step 4:** New folder (`files.mkdir`) and Upload (FileReader → base64 → `files.write`, refusing >1 MiB in the page first).
- [ ] **Step 5:** Gates, then commit with the rebuilt bundle.

### Task 2: Verify on the canary

- [ ] List the root, open a text file, edit and save it, re-read it, create a folder, upload a small file. Record the evidence in the commit. (Blocked while the gate route is down.)
