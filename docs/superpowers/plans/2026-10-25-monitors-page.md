# The Monitors Page — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An operator sees the reachability they configured, and can add, probe and remove a target, from the console.

**Architecture:** One console page in the rail (after Memory, before Audit) that uses the existing `monitor.*` tools through the same grant path as the other panes. No device or console route changes: `monitor.list`, `monitor.status {id}`, `monitor.add`, `monitor.remove {id}` and `monitor.probe {id}` already exist and are audited.

**Tech Stack:** React 19 + TypeScript, `callTool(grant, name, args)` from `./terminal`, the console's existing `.panel` / `.picker` / `.card` styles.

**Spec:** this plan; the device side is `device/device/src/capabilities/monitor.rs` and the schemas in `device/device/src/mcp.rs` (`monitor.add`: `{kind: tcp|http, host, port, path?, expect?, interval_ms}` with `interval_ms >= 5000`).

## Global Constraints

- Only the console page changes: `device/console/web/src/*` (plus the rebuilt `dist/index.html`). No Rust, no new dependencies.
- Every device call goes through `callTool(grant, …)`; no console route, no token handling in the page.
- The page has the same three states as Memory: no device, no grant (hint), and the view.
- Polling stops when the page is not the visible one or the grant changes.
- Gates: `npx tsc --noEmit` zero errors; `./scripts/build-web.sh` succeeds and `--check` reports current; bundle committed.
- English UI and comments.

## Review Focus

1. A target that is down renders as a fact with its `last_status`/`since`, not as an error page.
2. `monitor.add` with `kind: http` sends `path`/`expect`; with `tcp` it sends neither (the device refuses extra fields).
3. Remove is confirmed, and a removed target leaves the list.
4. The poll does not stack (a slow device does not queue probes), and it is cleared on unmount.
5. A failure surfaces the device's sentence, not a blank list.

---

### Task 1: The Monitors page

**Files:**
- Create: `device/console/web/src/MonitorsPage.tsx`
- Modify: `device/console/web/src/App.tsx`, `Rail.tsx`, `app.css` (page styles if needed)

**Interfaces:**
- Consumes: `callTool(grant, "monitor.list", {})` → `[{id, kind, host, port, path?, interval_ms?}]`; `callTool(grant, "monitor.status", {id})` → `{up_now, since_ms, up_pct, drops, last_status}`; `callTool(grant, "monitor.add", {kind, host, port, path?, expect?, interval_ms})`; `callTool(grant, "monitor.remove", {id})`; `callTool(grant, "monitor.probe", {id})`.
- Produces: `MonitorsPage({ grant })`.

- [ ] **Step 1:** Add the page and the rail entry; no device → hint with the device picker (like Memory), no grant → the same hint.
- [ ] **Step 2:** List targets with their status: up/down dot, `host:port` (+`path` for http), `since`, `up_pct`, `drops`, `last_status`; per-card Probe now and Remove (confirm).
- [ ] **Step 3:** Add form: kind, host, port, interval_ms (default 60000, min 5000), path/expect only for http; on success, refresh.
- [ ] **Step 4:** Poll `monitor.status` for each target every 15 s while mounted; clear on unmount.
- [ ] **Step 5:** Gates — `tsc --noEmit`, `build-web.sh`, commit with the rebuilt bundle.

### Task 2: Verify on the canary — done 2026-10-25 (console 0.3.37, device 0.3.30)

The page's call path, run through the device's MCP with the install token:

```
add:         {"host":"127.0.0.1","id":"127.0.0.1:19000","interval_ms":5000,"kind":"tcp","port":19000}
status:      {"drops":0,"last_status":null,"since_ms":…,"up_now":true,"up_pct":100}
probe:       {"latency_ms":0,"ok":true,"ts":…}
list:        [{"id":"127.0.0.1:19000",…,"summary":{…,"up_now":true,"up_pct":100}}]
remove:      {"removed":true}
list-after:  []
```
