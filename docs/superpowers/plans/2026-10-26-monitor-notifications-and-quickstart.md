# Monitor notifications, and a quick start that is true

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** the operator learns about a monitor going down without staring at the tab, and a new operator
gets from an installed device to an AI client holding a permanent token without reading four documents.

**Architecture:** the console's top bar already polls `monitor.list` every 20 seconds for its down
chip. Notifications hang off that one poll: a browser `Notification` fires when the down count rises
after the first successful read, only with an explicit preference and granted permission. The
preference and permission live in one small module; the Monitors page gains the card that asks. The
quick start is a single document that links the existing ones and states the commands and the exact
config.

**Tech Stack:** React 19 + TS + vitest/jsdom (`device/console/web`); Markdown under `docs/`.

**Spec:** `docs/superpowers/specs/2026-10-25-legacy-parity-sweep.md` ("Monitor alerts … browser
notifications (`NotificationsCard`) are not built: the permission prompt is its own decision") and the
operator's repeated questions about the token/MCP path.

## Global Constraints

- **Permission is requested only from a click**, never on mount or a timer. The card's button is the
  gesture; `Notification.requestPermission()` is never called anywhere else.
- **Opt-in, default off.** A preference in `localStorage` (`summrise.notify.monitors`) must be
  explicitly turned on by the card. Permission granted without the preference means no notification.
  An absent/unsupported/denied permission is quiet: no throw, no console error, no banner.
- **No notification on the first successful read**, and none when the count is unchanged or falls.
  Only a rise after a known previous count fires, once per rise. A refused grant stops the poll as
  today and must not fire.
- **The click opens Monitors.** The notification's body names the count and the total; clicking it
  focuses the window and runs the same `onOpen` the chip uses. No navigation without a click.
- **The quick start must match the code.** Every command and config line is one that exists:
  `summrise-device --enroll`, the panel form, `srm_` tokens, the `POST /mcp/<device-id>` endpoint, the
  install token at `<data>/token`. Link, do not duplicate, the long-form docs; if a fact changes, the
  linked doc is the source.
- English; `npm run typecheck`, `npm test`, `./scripts/build-web.sh` (exit code checked, bundle
  committed); `docs/` only for Task 2.

## Review Focus

1. **The permission prompt is honest.** It happens on the card's button, only there; the card renders
   the four states (unsupported, default, granted/on, granted/off, denied) in plain sentences and
   never promises a notification it cannot deliver.
2. **The transition, not the state.** First read: no notification. A fall then a rise: one
   notification. A rise to the same count: none. A refused/failed poll: none; the last fact stands.
3. **One preference, one grammar.** `notifyMonitors(down, total, onOpen)` is the only place a
   `Notification` is constructed; the chip holds no permission logic; the card holds no polling.
4. **The click is the navigation.** The notification's click focuses the window and calls the same
   `onOpen`; the in-page chip is unchanged.
5. **The quick start's claims are checkable.** Each step names the screen/flag it uses; the
   troubleshooting table's statuses/codes are the console's own (`401`, `403`/`-32001`, `404`
   `no_such_device`, the Access 302).

---

### Task 1: Browser notifications for the down chip

**Files:**
- Create: `device/console/web/src/notifications.ts` + `notifications.test.ts`
- Create: `device/console/web/src/NotificationsCard.tsx` + `.test.tsx`
- Modify: `device/console/web/src/MonitorChip.tsx` + `.test.tsx`
- Modify: `device/console/web/src/MonitorsPage.tsx` + `.test.tsx` (the card at the top)
- Modify: `device/console/web/src/app.css` only if a class is new

**Interfaces:**
- `notifications.ts`:
  - `type Permission = "unsupported" | "default" | "granted" | "denied"`;
  - `permission(): Permission` (reads `window.Notification`; absent → `"unsupported"`);
  - `requestPermission(): Promise<Permission>` (the click gesture; resolves the browser's answer;
    unsupported → `"unsupported"`);
  - `enabled(): boolean` (the stored preference, default `false`; off when unsupported/denied);
  - `setEnabled(on: boolean): void` (localStorage; a failed write is ignored — the session still
    works);
  - `notifyMonitors(down: number, total: number, onOpen: () => void): void` — constructs a
    `Notification` **only** when permission is granted and `enabled()`; title
    `"Summrise — N monitor(s) down"`, body `"N of total monitors down. Click to open Monitors."`;
    `onclick` focuses the window and calls `onOpen`; a constructor throw is swallowed.
- `NotificationsCard`: a section with `role="region" aria-label="Monitor notifications"`; a button
  whose label follows the state (`"Enable notifications"`, `"Notifications are on"` / `"Turn off"`),
  the denied sentence, the unsupported sentence; `MonitorsPage` renders it above the target list.
- `MonitorChip`: keep the poll and `report`; add a `useRef<number | null>` of the last down count;
  after each successful list: if `previous !== null && down > previous`, call
  `notifyMonitors(down, targets.length, onOpen)`; set previous. A failed/refused read leaves it.

- [ ] **Step 1: Module tests first**

`notifications.test.ts` with a fake `globalThis.Notification` (class with a static
`requestPermission`/`permission`, a constructor spy, `close`): unsupported when absent; permission
mapping; `requestPermission` resolves and never throws; `enabled()` default false and after
`setEnabled(true)`; `notifyMonitors` constructs only with granted+enabled; title/body exact;
`onclick` focuses (spy `window.focus`) and calls `onOpen`; a throwing constructor is swallowed.
`localStorage.clear()` in `beforeEach`; restore the original `Notification`.

- [ ] **Step 2: Card tests first**

The four states, the click calling `requestPermission` and then rendering on, the off toggle, the
denied/unsupported sentences, and that nothing is requested on mount.

- [ ] **Step 3: Chip tests first**

With the notifications module mocked: first read with a down target → no `notifyMonitors`; a rise
(0→1) after a successful up read → one call with `(1, total, onOpen)`; a fall then a rise → one more;
an unchanged count → none; a refused list → none and the poll stops (existing assertion).

- [ ] **Step 4: Implement, wire the card, verify**

`MonitorsPage` renders `<NotificationsCard />`; the chip imports `notifyMonitors`; the module guards
every browser API. `npm run typecheck`, `npm test`, build (exit code checked), bundle committed.
Commit: `console: monitors announce a new outage when the operator asked`.

### Task 2: The quick start

**Files:**
- Create: `docs/QUICKSTART.md`
- Modify: `README.md` ("Where to read next" links it)
- Modify: `docs/CONNECT-MCP.md` (a one-line pointer at the top)

**Content (each fact already true in the tree):**
1. **Install** — one line plus a link to `README.md#install-from-a-release`; the local panel appears at
   `http://127.0.0.1:18080/` (18090 for the canary), loopback only.
2. **Enroll** — console → Devices → **Add a device** → **New enrollment code**; then either the panel's
   **Enrol this machine** form (console URL + code) or
   `summrise-device --enroll <console-url> <code>`; restart `SummriseLaunch` after either (the console
   link is built at boot); the console shows the device online.
3. **Permanent token** — console → the device → **Connect an AI client** → **Permanent token**;
   name/tools/consent → **Create token**; the `srm_…` value is shown once: store it as a secret.
4. **MCP config** — the exact DSH YAML with the literal `srm_…` placeholder and
   `https://console.saisi.online/mcp/<device-id>`; a generic HTTP client line; note the same endpoint
   accepts a 1-hour grant (hex) instead, for a session.
5. **Check it** — one `tools/list`/`ping` example via curl (or "the AI client lists the tools");
   expected: only the token's tools.
6. **Revoke** — the card's **Existing tokens** → **Revoke**; the next request is 401; the token is
   never shown again.
7. **Local only (no console)** — the install token at `<data>/token` (Windows
   `C:\ProgramData\Summrise\token`), the panel's **Use this machine** section, the loopback MCP URL,
   and that the install token is the owner credential.
8. **When it does not work** — a table: 302 to Cloudflare Access (`/mcp` Access app), 401
   (`srm_` revoked/wrong device/expired grant), 403 with `-32001` (tool not in the token), the console
   card offline (device not restarted after enroll / no outbound access), banner/queue facts.

Keep it under ~140 lines; link `CONNECT-MCP.md` for the grant form and `RELEASE.md` for deployment.

- [ ] **Step 1: Write and verify links** (`grep` each anchor; every code block uses a command/flag that
  exists in the tree).
- [ ] **Step 2: Commit** `docs: the five-minute path from an installed device to an AI client`.

## Self-review

- Spec: the sweep's remaining notification item; the operator's token/MCP questions answered in one
  place.
- Review Focus: permission gesture, transition rule, one constructor, click navigation, checkable
  quick start.
- Deliberate: no notification for a monitor that flips while the tab is closed (the poll is the app's
  only fact), no service-worker push (the console is not a push server), no per-target names in the
  body (the chip counts; the page has the detail).

## Review rulings (2026-10-26)

- The first successful read is a fact, not a transition: it never notifies. The chip's `previous`
  ref resets with the grant, so a different device's first read is another first.
- A rise fires once; unchanged counts and falls say nothing. A failed read leaves the last fact and
  the ref alone.
- Permission is asked only in the card's click handler. `permission()`/`enabled()`/`notifyMonitors`
  never prompt; a storage write the browser refuses is a session-only preference.
- `notifyMonitors` is the only `Notification` constructor; its click focuses and calls the chip's
  latest `onOpen` through a ref, so a parent re-render neither restarts the poll nor changes where a
  notification goes.
- QUICKSTART's commands and codes are the tree's: the enroll flags, `srm_`, the restart, the
  `503 retry_after_ms` and the `404 no_such_device` paths were checked against the source.
