# Console activity timeline: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The console's Activity page becomes a dated record of what the AI did — one row per action, opening to the evidence for that call — live, pausable, with the audit table still available.

**Architecture:** The console already streams every audited action on `GET /events` with a human `sentence`; this plan adds a bounded `evidence` object built at proxy time (the call's own arguments and a run's output tail), renders the record as day-grouped rows that expand in place, and keeps the existing `AuditTable` behind a `Table` toggle. No new storage: evidence lives in the event, never in the audit file.

**Tech Stack:** Rust (axum, tokio, serde_json) in `device/console`; React 19 + TypeScript in `device/console/web`; vitest + jsdom for the page; the console's own integration harness in `device/console/tests/console.rs`.

**Spec:** `docs/superpowers/specs/2026-11-05-console-live-work-design.md` (and the operator-approved preview `docs/superpowers/specs/timeline-detail.html`).

## Global Constraints

- A page shows only what the audit, the device, or the run record proves. No invented animation or placeholder data.
- The audit file's shape does not change: it still never carries a body. Evidence is live-only, in memory.
- Type scale and weights come from the tokens (`var(--fs-*)`, `var(--fw-*)`); the design-token gate must pass.
- Every opening control is a real button with `aria-expanded`; focus is visible; `prefers-reduced-motion` is honoured.
- Evidence strings are bounded: an excerpt of 64 characters per field; a run's output tail of 1600 characters.

## Review Focus

1. A row from the audit file (no evidence) opened by the operator — it must say why, not render an empty panel.
2. A very long command, path or output — it must wrap or scroll, never widen the page.
3. A call that failed — the status and the error line must be visible in the evidence, not only in the row.
4. A live event whose tool the console has no evidence template for — the panel must show the result facts only.
5. A paused timeline receiving new events — nothing may move until Resume, and the count must be right.

---

### Task 1: The page's tests describe the timeline

**Files:**
- Modify: `device/console/web/src/App.test.tsx`
- Test: the same file

**Interfaces:**
- Consumes: `ActivityTimeline` from `src/ActivityTimeline.tsx`, default view `"stream"` in `App.tsx`.
- Produces: nothing new.

- [ ] **Step 1: Run the suite and read the five failures**

Run: `cd device/console/web && npx vitest run src/App.test.tsx`
Expected: five failures, all because Activity now renders the timeline instead of the table.

- [ ] **Step 2: Switch the table-dependent tests to the Table view**

In each failing test that asserts a tool or token cell, click the `Table` tab before the assertion:
`fireEvent.click(screen.getByRole("tab", { name: "Table" }));`

- [ ] **Step 3: Run the suite**

Run: `npx vitest run src/App.test.tsx`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add device/console/web/src/App.test.tsx
git commit -m "test: Activity opens on the timeline, so the table tests open the table"
```

### Task 2: A row opens to its evidence

**Files:**
- Modify: `device/console/web/src/ActivityTimeline.tsx`
- Test: `device/console/web/src/ActivityTimeline.test.tsx` (new)

**Interfaces:**
- Consumes: `AuditLine.evidence?: Record<string, unknown>` (`src/api.ts`).
- Produces: nothing; the component is self-contained.

- [ ] **Step 1: Write the failing test**

```tsx
it("opens a row's evidence when the row is clicked", () => {
  const line = {
    ts_ms: Date.now(), kind: "mcp", status: 200, tool: "runs.exec",
    sentence: "Ran `cargo test`",
    evidence: { command: "cargo", args: ["test"], output: "PASS 441 tests" }
  };
  render(<ActivityTimeline lines={[line]} devices={[]} />);
  fireEvent.click(screen.getByRole("button", { expanded: false }));
  expect(screen.getByText("cargo test")).toBeTruthy();
  expect(screen.getByText(/PASS 441 tests/)).toBeTruthy();
});
```

- [ ] **Step 2: Run it and see it fail**

Run: `npx vitest run src/ActivityTimeline.test.tsx`
Expected: FAIL — the row does not open.

- [ ] **Step 3: Implement the openable row**

The row is a `div` with `role="button"`, `aria-expanded`, Enter/Space handling, and one open row keyed by
`${ts_ms}-${tool ?? kind}-${status}`; `Evidence` renders Command / Input / Output / Result from the
`evidence` object.

- [ ] **Step 4: Run all the page's tests**

Run: `npx vitest run`
Expected: PASS (the previous count plus one file).

- [ ] **Step 5: Commit**

```bash
git add device/console/web/src/ActivityTimeline.tsx device/console/web/src/ActivityTimeline.test.tsx
git commit -m "web: a timeline row opens to the call's evidence"
```

### Task 3: The live event carries the evidence

**Files:**
- Modify: `device/console/src/activity.rs` (done: `detail`), `device/console/src/http.rs` (`publish_call`)
- Test: `device/console/tests/console.rs`

**Interfaces:**
- Produces: an `evidence` object in each live `mcp` event: `{command?, args?, input?, output?, path?, session?, url?, host?, bytes?, timeout_ms?, display?}`.

- [ ] **Step 1: Write the failing integration test**

In `tests/console.rs`, after the existing stream tests: connect a fake device, open `/events`, proxy a
`files.read` of `notes.txt`, and assert the `activity` event contains
`"evidence":{"path":"notes.txt"}`.

- [ ] **Step 2: Run it**

Run: `cargo test -p summrise-console --test console evidence`
Expected: FAIL — no evidence field.

- [ ] **Step 3: Wire it**

`proxy_reply` builds `evidence = activity::detail(tool, arguments)`; the success branch adds
`output = run_output_tail(&reply)` for `runs.exec`; `publish_call` writes it as `event["evidence"]`.

- [ ] **Step 4: Run the console tests**

Run: `cargo test -p summrise-console`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add device/console/src/activity.rs device/console/src/http.rs device/console/tests/console.rs
git commit -m "console: a live call carries its own evidence"
```

### Task 4: Ship it and watch it work

**Files:** none; deployment and verification.

- [ ] **Step 1: Gates**

Run: `npx vitest run`, `npm run typecheck`, `scripts/build-web.sh --check`, `cargo test -p summrise-console --test tokens`, `cargo test -p summrise-console`, `cargo clippy -p summrise-console --all-targets -- -D warnings`.

- [ ] **Step 2: Build `0.3.272-uxNN`, swap the host binary, poll `/health`.**

- [ ] **Step 3: Live check.** In a real browser on `console.saisi.online`: fire `runs.exec`
  (`{"command":"cmd.exe","args":["/c","echo timeline-evidence"]}`) through `/mcp`, open Activity, click the
  new row, read the command and the output, screenshot.

- [ ] **Step 4: Commit the backlog tick.**

```bash
git commit -m "backlog: the timeline and its evidence are shipped"
```

### Task 5: Screenshots as evidence (next slice, same pattern)

Keep the last capture per device in console memory (bounded at 8, keyed by a monotonic id), serve it at
`GET /activity/shot/:device/:id`, and put `shot` in the evidence of a `screen.capture` event. The row's
panel shows the image; a restart forgets it. Test: a fake device answers `screen.capture` with a 1×1 PNG,
the live event names a shot, and the route serves bytes with `image/png`.

### Task 6: The session card (next slice)

The Terminal page renders a card above the terminal: machine, shell, age, last command, last lines, and one
`Attach` button. Auto-attach keeps its behaviour; the card is what a reader sees before the console.

### Task 7: The hub about the machine (next slice)

Home's hub drops the endpoint hero (it stays in the Connect sheet), and shows the Now band, today's
timeline for this machine, and the session preview.
