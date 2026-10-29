# Remove the Console's 60-Second Grants Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Delete the console's 60-second per-device grant layer. The operator's permanent token is the only MCP credential: `/mcp/<device-id>` and `/mcp/all` authenticate it directly, and the device's console channel keeps its own registration token.

**Why now:** The operator's model is one operator with permanent tokens (`srm_…` with `device_ids`). The grants were a second, short-lived credential in front of the same decision: they add a signing key, a seed file, a `/grants` form, a token→grant exchange, a second expiry, and a docs/runbook path — for a scoping the permanent token already carries.

**Architecture:** The console's MCP gate becomes: bearer token (the operator's permanent API token, `device_ids` scoped) → the `device` the tool names is allowed by that token → the call. The device channel (the console's WebSocket to the device) authenticates with the device's registration token, as it does today; the grant envelope disappears from the wire.

**Tech Stack:** Rust (`summrise-console`, `summrise-device`), axum, rmcp; the existing test harnesses.

## Global Constraints

- No new credential, no new key, no new file: the removal is net-negative in moving parts.
- One permanent token still cannot reach a device outside its `device_ids`; the refusal is an audit line, not a 500.
- The device's own loopback surfaces (`/api/panel`, `/api/enroll`, `/api/terminal`) keep the panel token; they never used grants.
- The `/` React page and `/next` keep their routes; only the grant form/page goes.
- The legacy console must not be touched: this is the product's console only.

## Review Focus

- A token scoped to device A naming device B is refused, and the refusal is audited.
- A revoked/expired token is refused before any device call.
- The device channel still reconnects and answers after the grant path is gone.
- The `/grants` form and the grant key/seed are removed from code, scripts (`publish-canary.bash`), installer argv, `RELEASE.md`, and `CONNECT-MCP.md` in the same change that stops accepting grants.

---

### Task 1: The console's MCP gate takes the permanent token

**Files:** `device/console/src/http.rs`, `device/console/src/mcp.rs`, `device/console/src/auth.rs`, `device/console/tests/console.rs`.

- [ ] **Step 1: The failing tests** — `/mcp/<id>` with a permanent token and no grant: allowed for the token's device, 403 for a foreign device; a grant envelope is rejected (no longer a credential); the audit line for the refusal.
- [ ] **Step 2: Run (fails).**
- [ ] **Step 3: Implement** the gate: the bearer token → the token's `device_ids` → the path/`device` check; remove the grant verification from the request path.
- [ ] **Step 4: Green; the suite; clippy.**
- [ ] **Step 5: Commit.**

### Task 2: Delete the grant machinery

**Files:** `device/console/src/grants.rs` (delete), `http.rs` (`/grants`, `create_grant`, `sign_token_grant`, the `GrantForm` page), `config.rs` (`load_grant_key`, `--grant-key-file`), `http/page.rs` (the grant page), the tests.

- [ ] **Step 1: The failing tests** — `/grants` is gone (404); the console starts with no grant key file; the page has no grant form.
- [ ] **Step 2: Run (fails).**
- [ ] **Step 3: Delete** the module, routes, config, page, and the `SUMMRISE_CONSOLE_GRANT_KEY` reads.
- [ ] **Step 4: Green; clippy.**
- [ ] **Step 5: Commit.**

### Task 3: The device's console channel stops verifying grants

**Files:** `device/device/src/mcp.rs` (the `Ctx`/`Granted`/`verify_grant` path), `device/device/src/console.rs`, the tests.

- [ ] **Step 1: The failing test** — a console channel call without a grant still authorizes as the console's channel (the device's own token); a grant envelope is not required.
- [ ] **Step 2: Run (fails).**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Green; clippy.**
- [ ] **Step 5: Commit.**

### Task 4: Scripts, docs, installer and the release

**Files:** `scripts/publish-canary.bash`, `scripts/verify-release.sh`, `docs/RELEASE.md`, `docs/CONNECT-MCP.md`, `device/installer/summrise.nsi`, `device/launch/**` (the `--grant-key-file` argv).

- [ ] **Step 1:** Remove `SUMMRISE_CONSOLE_GRANT_KEY` and `--grant-key-file` everywhere; update the runbook and the connect guide to the permanent-token flow.
- [ ] **Step 2:** `grep -rn "grant" scripts/ docs/RELEASE.md docs/CONNECT-MCP.md device/installer device/launch` is empty except the audit's historical lines.
- [ ] **Step 3:** Build 0.3.50, publish, deploy to the fleet; `/mcp/all` with the permanent token works; a foreign-device call is 403.
- [ ] **Step 4: Commit.**

## Self-Review

- **The operator's request:** the 60-second grant layer is gone; the permanent token is the only MCP credential.
- **The device channel** keeps its registration token — the removal is the MCP client's grant, not the device's identity.
- **The review focus** has a test or a grep in each task.
