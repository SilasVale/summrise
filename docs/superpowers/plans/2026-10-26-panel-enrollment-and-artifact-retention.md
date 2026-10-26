# Enrolling from the panel, and one day of installer artifacts

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** let the person standing at the machine enroll it from the local panel — console URL and the
one-time code, no command line — and stop the installer workflow's artifacts from filling the
account's Actions storage.

**Architecture:** The panel is a loopback HTML page with no login; the enrollment is a `POST` the page
makes to the device's own HTTP API. The CSRF/loopback proof is a per-boot 32-byte random token injected
into the panel HTML only, required in a header on the POST, with the peer address checked loopback. The
enrollment itself is the existing `enroll::enroll` (identity signature, one-time code, console token),
so the panel adds a surface, not a second protocol.

**Tech Stack:** Rust (device `http.rs`, `panel.html`), the existing `enroll` module; GitHub Actions YAML.

**Spec:** the operator's question "没有界面操作的方式吗"; the console's enrollment UI generates the
code (`App.tsx` "Add a device"), and this is the device-side half. `cs.yml`/`ci.yml` do not test the
panel; the Rust server tests do.

## Global Constraints

- **The panel token is per boot and never leaves the loopback page.** 32 bytes from `OsRng`, 64 hex,
  injected at `__PANEL_TOKEN__` in the served HTML (a `<meta>` tag); it must not appear in
  `/api/panel`, in the diagnostics text, in any response body, or in a log line.
- **The enrollment endpoint is local-only in depth:** the server binds loopback already; the handler
  also refuses a non-loopback peer (the same check `/api/panel` makes), requires the panel token in
  `x-summrise-panel`, and requires a JSON body. A cross-site page cannot read the token or set the
  header without a CORS preflight the device never grants.
- **The enrollment protocol is the existing one.** No new console route, no change to the registration
  signature, no new token format. The panel is a caller of `enroll::enroll`.
- **Errors are the device's sentences.** The panel shows `EnrollError`'s Display; it invents no text.
  Success returns the device id and says the `SummriseLaunch` service must be restarted for the
  running app to open the channel (the console link is built at boot).
- **A second enrollment refuses without `force`** (the existing `TokenConflict`); the form's "Replace
  an existing enrollment" maps to `force`.
- English UI/comments. `cargo test -p summrise-device`, clippy, fmt; `npm test` untouched; the
  console bundle does not change. Check every command's exit code directly.

## Review Focus

1. **Token scope.** The HTML is the only place the token appears (plus the app's memory). The
   `/api/panel` JSON and the "Copy diagnostics" text do not contain it; the response body never echoes
   the console token.
2. **Local-only.** A request without the header, with a wrong header, or with a non-loopback peer is
   refused before any enrollment work; a JSON-less POST is refused by the extractor. The check order
   (peer → token → parse) is deliberate.
3. **The sentence and the restart.** A refused code/signature/clock conflict is the console's own
   sentence; a success names the device id and states that the service must be restarted. The panel
   never claims the link is live before that restart.
4. **Re-enrollment is explicit.** Without `force` the existing registration is kept; with it the new
   console/token replaces the old; the form's warning says what that means.
5. **The artifact change is only retention.** `retention-days: 1` on the installer artifact; tag
   releases still attach the installer to the release, which is storage outside Actions artifacts.

---

### Task 1: The panel's enroll form

**Files:**
- Modify: `device/device/src/http.rs` (`AppState`, `local_panel`, the route, the handler, tests)
- Modify: `device/device/src/panel.html` (the section, form, script, a little CSS)
- Modify: `README.md` (Onboarding: the panel path, beside the CLI path)

**Interfaces:**
- `AppState` gains `data: PathBuf`, `install: PathBuf` (from `Config`) and `panel_token: String`
  (64 lowercase hex, minted once in `router_with`).
- `GET /` serves the HTML with `<meta name="summrise-panel" content="<64 hex>">` in `<head>`.
- `POST /api/enroll`:
  - headers: `x-summrise-panel: <64 hex>`; body JSON
    `{"console_url": string, "code": string, "force"?: bool, "allow_plaintext"?: bool}`;
  - loopback peer only (else 403 `{"error":"loopback_only"}`);
  - wrong/absent token 403 `{"error":"unauthorized"}`; malformed JSON 400/422 as axum decides;
  - calls `crate::enroll::enroll(&EnrollInvocation { console_url, code, data: app.data, force:
    request.force.unwrap_or(false), allow_plaintext: request.allow_plaintext.unwrap_or(false) })`;
  - success 200 `{"id":"<device id>"}` (no console token);
  - `EnrollError` → 400 with `{"error": <Display sentence>}` (the console/IO variants included; the
    operator reads the sentence; no second text).
- The panel form (ids for the tests): `enroll-console`, `enroll-code`, `enroll-force`,
  `enroll-plaintext`, the submit button, and `enroll-status` (`role="status"`). On success:
  `Enrolled <id>. Restart the SummriseLaunch service, then reload this panel.`; on failure: the
  `error` sentence. Disable the button while the request is in flight.

- [ ] **Step 1: Tests first**

In `http.rs`'s test module, using the existing server harness (bind a loopback port):

1. `GET /` carries a `<meta name="summrise-panel" content="…">` whose value is 64 hex, and the enroll
   form's ids; two separately built servers have different tokens.
2. `GET /api/panel` JSON does not contain the token string.
3. `POST /api/enroll` without `x-summrise-panel` → 403; with a wrong token → 403; both with no
   console.json written.
4. With the token and a mock console (`POST /devices` echoing the request's `id` and returning
   `{"id": <that id>, "token": <64 hex>}`), a valid body → 200 `{id}`, and `<data>/console.json` and
   `<data>/console-token.json` exist.
5. The same when `<data>/console-token.json` already exists: without `force` the response is 400 and
   the old file is unchanged; with `force: true` it is replaced.
6. A non-loopback `ConnectInfo` (insert the extension in a `oneshot` request to the router) is 403
   even with the right token.

- [ ] **Step 2: Implement the endpoint and the state**

Add the fields and the mint (reuse the helper module's pattern: `rand_core::{OsRng, RngCore}`,
32 bytes, lowercase hex; do not import helper's private `mint`). Add the route and handler; use the
existing `constant_time_eq` over the decoded header. Build the URL/code validation through `enroll`
(do not duplicate it).

- [ ] **Step 3: The panel**

Add the section under **Console link** with the form and the script; the fetch reads the meta token and
sends it in `x-summrise-panel`. Keep the panel's existing style tokens; add only the field/button rules
it lacks. The panel stays loopback read-only otherwise. Update the README's "Onboarding a machine" with
the panel path (open `http://127.0.0.1:<port>/`, paste, Enroll, restart the service).

- [ ] **Step 4: Verify and commit**

```bash
source ~/.cargo/env
cargo test --manifest-path device/Cargo.toml -p summrise-device
cargo clippy --manifest-path device/Cargo.toml -p summrise-device --all-targets -- -D warnings
cargo fmt --manifest-path device/Cargo.toml --all -- --check
```

Commit: `device: the panel can enrol this machine with a code`.

### Task 2: One day of installer artifacts

**Files:**
- Modify: `.github/workflows/installer.yml` (the `upload-artifact` step)

**Interfaces:** none (workflow only).

- [ ] **Step 1: Change the retention**

Add `retention-days: 1` to the `actions/upload-artifact@v4` step that uploads
`SummriseSetup-*.exe`, with a comment: the release asset is the durable copy; the run artifact is a
convenience and does not need to sit in the account's Actions storage for 90 days (the quota warning of
2026-10-26). Do not change the tag-release upload.

- [ ] **Step 2: Verify and commit**

YAML parses (a `python -c "import yaml"` load is enough; no workflow runs on a PR path change), the
diff is one line plus the comment. Commit:
`ci: the installer's run artifact lives one day, not ninety`.

## Self-review

- Spec coverage: the operator's UI-enrollment question (the panel form), the Actions storage warning
  (the artifact retention).
- Review Focus: token scope, local-only gates, sentences/restart, explicit re-enrollment, retention.
- Deliberate limits: no hot-reload of the console link (restart required), no panel login (loopback
  is the trust boundary), no new console route.
