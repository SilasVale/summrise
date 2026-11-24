# The Page Request Gate — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** A slow answer cannot overwrite a newer one on any console page, and a button drawn by the page is the size the page meant.

**Architecture:** One small helper (`web/src/requests.ts`) that every pane uses to mark a request as the newest; a response whose ticket is stale is dropped. Plus the two CSS rules the reviews found missing (`.mini` at base, `.memory-entry`).

**Spec:** this plan. The three findings it closes are in `.superpowers/sdd/*/progress.md`: "no read request sequencing" (Runs), "read() is last-writer-wins" (Monitors), "stale-response race" (Memory) — three reviewers, one bug.

## Why a helper and not three refs

Each page has the same shape: a `read()` that awaits `callTool`, then sets state. When a user refreshes, clicks a tag, or changes the device while an answer is in flight, the older answer can land last and paint the older view. A per-page `useRef<number>` fixes one page; a helper makes the rule the default and gives the next page nothing to remember.

## Global Constraints

- Console web only (+ the rebuilt `dist/index.html`); no dependencies; English.
- No behaviour change other than dropping stale answers: the newest request always wins, and an error from a stale request is dropped too (it would describe a view the operator already left).
- `tsc --noEmit` 0 errors; `build-web.sh` builds and the bundle is committed; `--check` current.

## Review Focus

1. The ticket is a ref, not state (no extra render, no stale closure).
2. A superseded answer drops **both** its data and its error.
3. Unmount/device change invalidates in-flight work (so a late answer cannot set state after the page is gone).
4. The three pages that had the bug now use it, and their existing guards are not duplicated into a mess.
5. The CSS: `.mini` has a base rule that the `.tools` variant still overrides; `.memory-entry` has a rule.

### Task 1: The gate and the two rules

**Files:**
- Create: `device/console/web/src/requests.ts`
- Modify: `device/console/web/src/RunsPage.tsx`, `MonitorsPage.tsx`, `MemoryPage.tsx`, `app.css`

**Interfaces:**
- `useRequestGate(): { begin(): number; stale(ticket: number): boolean }` — `begin()` returns a new ticket and invalidates every earlier one; `stale(ticket)` is true when `ticket` is no longer the newest (callers drop the answer).
- The pages call `const gate = useRequestGate();` then `const ticket = gate.begin(); … if (gate.stale(ticket)) return;`.

- [ ] **Step 1:** Write `requests.ts` with the hook and a short doc comment stating the rule.
- [ ] **Step 2:** Use it in the three pages at every `callTool`-then-set-state site (reads, history, status polls).
- [ ] **Step 3:** `app.css`: base `.mini` rule (the `.tools .mini` rule keeps its own size), and a `.memory-entry` rule that the page's markup actually uses.
- [ ] **Step 4:** Gates, commit with the rebuilt bundle.
