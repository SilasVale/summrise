# The console's HTTP client, its grant form, and one set of text helpers

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** put the console's two untested security-surface pieces — the HTTP client every page goes
through and the grant form that mints what the device checks — under test, and remove the five copies
of the same `message` helper and the three variants of "short device id".

**Architecture:** `api.ts` is the console's whole HTTP surface (token, Access, JSON errors); a test
mocks `fetch` and walks every endpoint. `GrantForm` is the only place a grant is minted; a test mocks
`./api` and the `/tools` catalogue. The helpers move to `format.ts` with their own tests.

**Tech Stack:** React 19 + TypeScript, vitest/jsdom/RTL.

**Spec:** the sweep's untested-pages note and the round-3/4 standards reviews (the `message`/`shortId`
duplication); `api.ts` and `GrantForm.tsx` have no tests, and the grant form is where the console's
authorization begins.

## Global Constraints

- **Tasks 1–2 are test-only.** No production change in `api.ts` or `GrantForm.tsx`; if a test exposes a
  real defect, report it and leave the file alone.
- **No network.** Every `fetch` is mocked and restored (`vi.stubGlobal` + `vi.unstubAllGlobals` in
  `afterEach`, or an explicit restore); no test leaves a global mock or timer behind.
- **Task 3 is behaviour-preserving except one rule:** a truncated id ends in `…` only when it is
  actually truncated (DeviceCard already did this unconditionally; the other display sites did not
  ellipsise at all). A protocol name (ConnectCard's `serverName`) is not a display name and keeps its
  raw slice.
- English UI/comments. `npm run typecheck` 0; `npm test` green (152 at the base); `./scripts/build-web.sh`
  with its exit code checked (never a piped tail) and the committed bundle in agreement.

## Review Focus

1. **The client's split is honest.** `NoAnswer` only for a body that is not JSON (the edge answered); a
   non-2xx JSON body is `HttpError` with the body's `error` or the status text; the token header exists
   only once `setToken` ran; `content-type` only when there is a body.
2. **The form cannot mint nothing.** An empty selection refuses locally with its sentence and no
   `api.grant`; a selection sends exactly those names, the lifetime and the destructive flag; a
   `/tools` failure leaves the fallback list usable and everything selected.
3. **One helper, one rule.** Every display site uses `shortId`; `shortId` truncates to twelve code
   units and adds `…` only then; `message` is the `Error`/string rule the five pages already share;
   the protocol name is untouched.
4. **No leaked mock.** A test that silences or replaces a global restores it, including on failure.

---

### Task 1: The HTTP client's tests

**Files:**
- Create: `device/console/web/src/api.test.ts` (test-only)

**Interfaces:**
- `api` (the object), `HttpError`, `NoAnswer`, `setToken`, `getToken`, `meshStatus`,
  `meshReadiness` — all already exported.
- Mock `globalThis.fetch`; a helper `answer({status?, statusText?, text?, json?})` returns the
  Response-like object the client reads (`ok`, `status`, `statusText`, `text()`).

- [ ] **Step 1: Write the tests**

Cover at least:

1. `setToken`/`getToken`: empty before, the value after, empty again after `setToken(null)`;
2. headers: no token → no `authorization`; a token → `Bearer <token>`; a GET has no `content-type`; a
   POST with a body does;
3. a JSON 200 is parsed; a non-JSON 200 is `NoAnswer`; a non-2xx JSON `{error: "..."}` is `HttpError`
   with that code; a non-2xx empty/non-JSON body falls back to `statusText`;
4. each endpoint's path and method: `session` `/session`; `signIn` POST `/auth` with the credential;
   `devices` `/fleet` (not `/devices`); `audit()` `/audit?limit=20`, `audit(5)` limit 5; `rename` POST
   `/fleet/<id>/name`; `remove` DELETE `/fleet/<id>`; `connections`; `saveConnection`; `removeConnection`;
   `enroll` POST; `grant` POST `/grants` with `{device_id, tools, expires_in_s, allow_destructive}`;
5. `meshStatus` `/mesh`; `meshReadiness` success, `HttpError` with `no_mesh_broker` → `{configured:
   false}`, any other `HttpError` → `{error: code}`, and a non-HTTP error → `{error:
   "mesh_unavailable"}`;
6. the request body is `JSON.stringify` of what the caller passed (assert `init.body`).

- [ ] **Step 2: Run, then commit**

`npx vitest run src/api.test.ts`, then the whole suite and typecheck. The bundle does not change.
Commit: `test: the console's HTTP client is pinned endpoint by endpoint`.

### Task 2: The grant form's tests

**Files:**
- Create: `device/console/web/src/GrantForm.test.tsx` (test-only)

**Interfaces:**
- `GrantForm({deviceId, onGranted})`; the real component's fallback `TOOL_NAMES` (38 names) and the
  `/tools` fetch in its mount effect.
- Mock `./api` with `vi.mock`: `api.grant` a `vi.fn()`, `getToken` a `vi.fn(() => "token")`; keep the
  real `TOOL_NAMES` export by `importOriginal`. Mock `globalThis.fetch` for `/tools` and restore it.

- [ ] **Step 1: Write the tests**

Cover at least:

1. the fallback renders 38 tool checkboxes, all checked, and the legend says `Tools (38 of 38)`;
2. a successful `/tools` answer (two tools) replaces the list, selects both, and the `/tools` request
   carried `Authorization: Bearer token`; a failed `/tools` leaves the fallback usable;
3. **None** then **Grant access** → `Pick at least one tool.` and `api.grant` was not called;
4. unchecking one tool updates the count; **All** restores everything;
5. a grant sends `api.grant(deviceId, [...chosen], lifetime, destructive)` exactly (change the
   lifetime to 60 and untick destructive), and `onGranted` receives the envelope plus the same names;
6. an `api.grant` rejection shows its message; the button returns from `Signing…`.

- [ ] **Step 2: Run, then commit**

`npx vitest run src/GrantForm.test.tsx`, then the whole suite and typecheck. The bundle does not change.
Commit: `test: the grant form mints exactly what was selected`.

### Task 3: One `shortId` and one `message`

**Files:**
- Create: `device/console/web/src/format.ts`, `device/console/web/src/format.test.ts`
- Modify: `App.tsx`, `DevicePage.tsx`, `DevicePicker.tsx`, `DeviceCard.tsx`, `AuditTable.tsx`,
  `ConnectCard.tsx` (display ids), `FilesPage.tsx`, `MemoryPage.tsx`, `MonitorsPage.tsx`,
  `RunsPage.tsx`, `SystemPage.tsx` (the `message` copies), `DevicePage.test.tsx` (the unnamed id's
  expectation)

**Interfaces:**
- ```ts
  /** The first twelve characters of an id, with an ellipsis only when something was cut. */
  export function shortId(id: string): string;
  /** A failed call's sentence: an Error's message, or the value as a string. */
  export function message(cause: unknown): string;
  ```

- [ ] **Step 1: Pure tests first**

`format.test.ts`: a 12-character id is unchanged; a 13-character id is the first 12 plus `…`; a short
id has no ellipsis; `message(new Error("x"))` is `"x"`, `message("x")` is `"x"`, `message(42)` is `"42"`.

- [ ] **Step 2: Replace the copies**

- `AuditTable.shortId` and `DeviceCard.shortId` are deleted; their callers import `shortId`.
- The `id.slice(0, 12)` in `App.tsx` (five headings/notices), `DevicePage.tsx`, `DevicePicker.tsx`
  and `ConnectCard.tsx`'s display `name` become `shortId(id)`. **`ConnectCard`'s `serverName` stays a
  raw slice** — it is a protocol name, not a display string.
- The five local `message` functions are deleted; their callers import the shared one.
- `DevicePage.test.tsx`'s unnamed-device expectation becomes `0123456789ab…`; if any other test asserts
  a truncated id without the ellipsis, update it and say so.

- [ ] **Step 3: Verify and commit**

`npm run typecheck`, `npm test`, build with the exit code checked, bundle committed.
Commit: `console: one short id and one error sentence, not eleven copies`.

## Self-review

- Spec coverage: the HTTP client and the grant form (the two security surfaces without tests), and the
  duplication the standards reviews named.
- Review Focus: headers/errors (Task 1), empty selection and exact grant (Task 2), the helper rule and
  the protocol name (Task 3).
- Deliberate change: truncated ids now say they were truncated (an ellipsis); the unnamed-device test
  records it.
