# Capability Hub: the product, the console and the plan

> **Status: plan of record, 2026-11-02.** This replaces the proposal of the same date. That draft
> answered "what could this become"; this one answers "what we ship, what we delete, and how we know
> it works". It is the spec the implementation plan
> (`docs/superpowers/plans/2026-11-02-capability-hub-v1.md`) argues from.
>
> Every decision here was made by the Lead under the operator's instruction to decide without asking.
> The operator's own words were: 梳理核心价值 · 苹果的产品风格 · 用户体验好 · 断舍离 · 功能可用也要好用.

---

## 0. The decision in one page

**Summrise is the capability hub for AI on a real machine:** one MCP endpoint, capabilities that work
the moment it is installed, plugins when one capability is missing, and a console that shows every
call and lets the operator take the keyboard back.

Three decisions define this revision:

1. **The hub is the product; the console is its window.** The hub is not a new service. It is the
   device runtime that already executes 50 tools, plus a plugin host that lets it execute more. The
   console stops being a device control panel with a hub bolted on: its first screen is the hub, and
   the machine workspace is one destination inside it.
2. **Five destinations, not seven.** Home · Capabilities · Plugins · Activity · Device. Devices and
   Connect stop being places: the fleet is Home's zero state and the machine picker lives in the
   header; Connect is a verb, so it is a focused sheet opened from the actions that need it. Deletion
   is not a later phase; it is the first commit.
3. **A plugin is an MCP server, installed from a directory we trust.** No marketplace, no sandbox, no
   signatures, no permission policy. Installing a plugin runs that plugin's code with the operator's
   privileges; the product says so in the same sentence that offers the button. A plugin adds tools
   to the one endpoint; it does not add a second model of authorization.

What this revision cut from the proposal, and why:

| Cut | Why |
|---|---|
| A standalone `summrise` CLI with `up`, `plugin add`, `doctor` | The Windows installer, the service and the console already are the one path. A second path would have to stay correct forever; the hub earns a CLI when a headless install earns one. `v1.1`. |
| `plugins` as a separate page from Capabilities | The operator's question is "what can my AI do", not "which binary provides it". Plugins are a source badge and a management sheet, not a second catalogue. |
| Git and npm plugin sources in v1 | Both add a runtime dependency (git/npm) and a failure mode (network, lockfiles) that the first plugin authors do not need. A directory is the whole contract in v1. `v1.1`. |
| A permission/scopes UI for plugins | The token already names tools and destructive consent. A second policy layer before adoption is a tax with no payer. Installing is the trust decision; the token is the runtime brake. |
| The seven-surface console as the first screen | It teaches an icon rail before it teaches what the AI can do. Replaced by Home + one Device destination. |
| Memory, Monitors, Events and Security as pages | They were already out of navigation. Their components are deleted, not preserved "for later": a dead page is a second product nobody maintains. |

What this revision adds:

- A **plugin host** in the device runtime: manifest, supervisor, MCP stdio client, namespaced tools,
  crash restart, bounded logs, and `plugins.*` tools the console (and any MCP client) can drive.
- A **live tool catalogue** for the console: `GET /fleet/:id/tools` reads the selected device's
  actual `tools/list` (built-ins + plugins), so the UI, the token mint form and the Capabilities page
  cannot drift from what the device advertises.
- A **Home** that states the hub's three facts in one screen: it is running, here is the door, here is
  what the AI can do; and the two actions that matter: Connect a client, Add a capability.
- **Try it**: every tool can be called from the catalogue with a form generated from its schema,
  without writing a client. This is the product demo and the debugging tool at once.
- **A deletion pass**: one navigation, one page shell, no status strip, no dead pages, no duplicate
  "connect" dialogs.

---

## 1. Core value

### 1.1 The promise

> **Give your AI hands on a real machine — one endpoint, capabilities that work out of the box, and a
> window that shows what happened and lets you take back the keyboard.**

The first half is table stakes in 2026: many projects expose a shell to an agent. The second half is
the product. A capability hub that cannot be watched is a remote shell with branding; a console that
cannot extend the hub is a monitoring page. Summrise is the two halves in one runtime.

### 1.2 The three promises, in the order the operator feels them

1. **Hands, immediately.** Terminal, files, browser, screen, runs, machine facts and state are real
   on install. No plugin, no config file, no Node runtime. The first successful call is minutes away,
   not an afternoon.
2. **One door, extendable.** The AI sees one MCP endpoint. A missing capability is one plugin away,
   and the plugin's tools appear behind the same door, under the same token, in the same record.
3. **A window you can close.** Every call leaves a row: what, on which machine, under which token,
   with what result. The operator opens the same machine's terminal or screen and takes over. Revoke
   is one action; the workspace is the brake, not a permission dialog after the fact.

### 1.3 Who it is for

A developer or operator who runs AI clients that need to do work rather than describe it, on Windows
machines they own or are responsible for. They already speak MCP (DSH, Claude, Codex, Cursor, a
custom client) or can paste a configuration. They do not want to build a terminal, a browser driver
and a screen capture for every client, and they do not want a cloud VM between the agent and the
machine the work belongs to.

**Not for:** teams that need multi-tenant policy and compliance evidence; people who want a hosted
sandbox; people who want an agent orchestrator with tasks, plans and teams. Those are different
products, and pretending otherwise is how this one loses its edge.

### 1.4 The job to be done

> "Let my agent do real work on this machine, through one endpoint I can point any client at, and let
> me see and undo what it did — without building the hands myself or forking the runtime to add one."

### 1.5 Why this product, not the alternatives

| Alternative | What it gives | What it costs |
|---|---|---|
| A general MCP server per capability | one tool, one process, one config per client | the operator assembles and watches N servers; no record, no brake |
| A cloud sandbox / cloud IDE | isolation, easy provisioning | the work is not on the machine the work belongs to; no real browser, screen or files |
| A desktop agent (computer use) | a screen and a mouse | slow, fragile, unauditable, no terminal-grade precision |
| A remote desktop | a person watching a person | the agent is not the worker; nothing is recorded as a call |
| **Summrise** | **real capabilities + one endpoint + a record and a brake** | **Windows-first; one operator, not a tenant model** |

The moat is not the tool list; it is the pairing of **real hands** with **reversibility**: the same
runtime that executes the call writes the row, and the same console that shows the row can take the
keyboard. A competitor can copy the tools; copying the trust surface means rebuilding the product.

### 1.6 How we know it works

Read from the world, not the repository:

- minutes and steps from a clean machine to a first successful MCP call;
- share of sessions that end with a recorded call rather than an unattributed action;
- plugin installs that reach a healthy `tools/list`, and the time to get there;
- calls that needed the operator to take over, and whether the takeover worked;
- crashes/restarts per device per week, and whether the hub came back with the same tools;
- tokens revoked after a mistaken call (a brake that is never used is a brake nobody believes in).

No vanity metric (installs, stars) displaces these.

---

## 2. The model

```
Capability  ── declares ──▶  Tool  ── called by ──▶  Call  ── recorded as ──▶  Record
     │                                                                    │
     └── built-in (Rust, in-process) or plugin (MCP server, child process)┘
                                    on a Device, reached through Reachability
```

- **Capability** — a named group of tools with one job: `terminal`, `files`, `browser`, `runs`,
  `screen`, `host`, or a plugin's namespace. The unit the catalogue organizes; the unit a plugin adds.
- **Tool** — one callable operation with a name, a description, an input schema and annotations
  (destructive or not). The unit `tools/list` advertises and a token scopes.
- **Device** — a real machine running the runtime: `summrise-launch` supervises `summrise-device`.
- **Session** — the live object a stateful tool returns (a pseudo-terminal, a browser page, a run).
  It is not a fourth kind of capability; it is why some capabilities are worth keeping open.
- **Call / Record** — one `tools/call`, and the audit row it leaves. The record is the product's
  memory and the operator's evidence.
- **Reachability** — how the operator gets to the device: the console from anywhere, the panel on the
  machine. A device dials out; nothing listens inbound by default.

`PRODUCT.md` keeps its four nouns (Device, Session, Record, Reachability) and gains **Capability** as
the hub's organizing object. The five are a hierarchy, not five equal things.

---

## 3. Design principles

The operator asked for the Apple register. These are the constraints that register reduces to here;
each one is checkable, and a change that violates one is wrong even if it looks good.

1. **The product is the subject.** A screen shows the machine, the capability or the call before it
   explains anything about itself. Chrome recedes: no dashboard of widgets about the dashboard.
2. **One primary action per screen.** The accent marks the action the operator should take. If two
   things are orange, one of them is wrong. Secondary actions are quiet; destructive ones are never
   the accent fill.
3. **Progressive disclosure is the navigation.** Fleet → hub → capability → tool → call. A first visit
   shows the machines; the hub appears once one is chosen; a tool's form appears when the operator
   asks to try it. No page explains a feature the operator cannot see yet.
4. **One type scale, one spacing rhythm.** Seven sizes, 13 px body; space comes from a named scale.
   Rank comes from scale and space, not from colour, boxes or decoration.
5. **States are visible without hovering.** Focus, hover, current, disabled, empty, loading, waiting,
   offline, failed. Every state has words and a next action; a state that needs a tooltip is not
   shown.
6. **Motion explains change, and nothing else.** 120–180 ms, ease-out, no entrance choreography. With
   `prefers-reduced-motion`, the product is complete.
7. **Copy is the interface.** Verb-first buttons, sentence case, no jargon before it is defined, no
   policy language. An error names the next action; an empty state teaches the first step.
8. **Deletion is part of the design.** Every addition names what it replaces. The default answer to
   "and also" is no. A page nobody can reach is not a feature waiting; it is code that will rot.
9. **Measured, not argued.** Contrast and type are a Rust test; layout is the layout gate; every page
   is rendered at 360/390/800/1280 in light and dark and read. A change that moves a page nobody
   meant to touch is a finding.
10. **Trust is stated, not implied.** "Installing a plugin runs its code with this machine's
    privileges" appears where the install happens, not only in a doc.
11. **The keyboard is a first-class surface.** Every action reachable by Tab; a visible focus ring;
    Enter runs the focused action; Escape closes the sheet; the first Tab stop is Skip to content.
12. **Honest about time and distance.** "seen 12s ago", "this may still be running", "the device did
    not answer". Never a spinner where a sentence belongs.

---

## 4. The first run

The promise is judged in the first five minutes; design the whole path, not just the console.

1. **Install** — `SummriseSetup-<version>.exe`. The service starts, the device listens on loopback.
2. **Open the panel** — `http://127.0.0.1:18080/`. It says the machine is running, and gives one
   action: enrol it with the console. The operator pastes the console URL and a one-time code.
3. **Open the console** — the machine appears in the fleet, online. The operator opens it.
4. **Home** — one sentence of state: running, version, 50 tools, 0 plugins. The endpoint is on the
   screen with Copy. Two actions: **Connect a client** and **Add a capability**.
5. **Connect** — a sheet: the endpoint, then the client selector (DSH / Claude / Codex / Cursor) with
   a copy-paste block; "Create token" names the tools and the destructive consent. The token is shown
   once.
6. **First call** — the AI calls `terminal.create` and `terminal.write`; Activity shows the rows; the
   operator opens Device → Terminal and sees the session. The loop is closed.

A developer who only wants the endpoint can skip 5's client blocks and copy the URL and a token from
the same sheet. No account, no second config file.

---

## 5. The console: information architecture

### 5.1 Destinations

| Destination | The one job | Primary action |
|---|---|---|
| **Home** | Say what this hub is and what it can do; offer the next step. | Connect a client |
| **Capabilities** | Show every tool the AI can call, by capability, with its schema. | Try a tool |
| **Plugins** | Install, watch, restart and remove the capabilities that are not built in. | Add plugin |
| **Activity** | Show what was called, by which token, with what result. | (filter; no creation) |
| **Device** | Put the operator's hands on the machine: Terminal · Screen · Files · Runs · Machine. | per tab |

**Fleet and Connect are not destinations.** The fleet is Home before a machine is selected, and the
header picker afterwards. Connect is a sheet opened from Home, Device and the account menu; it is a
verb with a focused job (endpoint, client config, tokens), not a place to browse.

**Settings / About** live in the account menu at the rail's foot: version, console URL, docs, sign
out. They do not earn a destination; nothing there is a daily job.

### 5.2 The shell

- **Rail (≥768 px):** a 200 px labelled sidebar. 20 px icons, 13 px labels, one accent for the
  current item. Labels are not optional at this width: an icon-only column is a memory test, and the
  product's own audit found ten icons that led back to the page the reader was on.
- **Rail (640–767 px):** a 56 px icon rail, labels on hover/focus and as accessible names.
- **<640 px:** a bottom tab bar with five items, 20 px icon + 11 px label. Home is the first tab; the
  account menu moves to the header. The content gets the full width; no 56 px left gutter. The bar
  does not wrap, and there is no status strip beneath it.
- **Header:** one control, not a name plus a picker: `[● workshop · Running · v0.3.246 ▾]`, whose
  menu lists All machines, every machine, and Add a machine. With no machine it reads
  `[All machines ▾]`. On mobile the header is one 44 px row: picker, title, overflow menu.
- **No bottom status strip.** It duplicated the fleet count, the session and the version. The count
  is Home's job, the session is the account menu's, the version is About's.
- **Content width:** 1120 px max, 24 px page padding (16 on phone). One `h1` per page.

### 5.3 The map from today

| Today | Tomorrow |
|---|---|
| Devices page (rail) | Home's zero state + the header picker |
| Terminal / Screen / Files / Runs / Machine (five rail items) | Device → five tabs, one destination |
| Activity | Activity (unchanged in content; new shell) |
| Connect AI client dialog, Fleet tokens dialog | Connect sheet: Endpoint · Clients · Tokens |
| Bottom status strip | deleted; facts move to Home / account menu |
| Memory / Monitors / Events / Security components | deleted from the tree |
| "Work on this" / "Connect AI client" on cards | **Open** (accent) / **Connect** (quiet) |

### 5.4 The URL

- `#home`, `#capabilities`, `#capabilities/<tool>`, `#plugins`, `#activity`, `#device/terminal`,
  `#device/screen`, `#device/files`, `#device/runs`, `#device/machine`.
- The selected device id survives a reload (localStorage) and is not in the URL; a deep link to a
  device-specific surface without a selected device opens the picker.
- A run deep link (`#run/<device>/<id>`) keeps working and selects Device → Runs.

---

## 6. Screen specifications

### 6.1 Home

**Purpose:** answer "is it running, where is the door, what can it do, what should I do next" in one
screen, without repeating the shell.

- **No second header.** The shell's machine control owns identity; Home starts with one status line:
  `Running · v0.3.246 · 50 tools · 2 plugins`.
- **Endpoint:** the console's MCP URL in one code row with Copy. Offline: the line says why, the
  endpoint is dimmed, and Copy is disabled rather than copying a door that does not open.
- **One accent:** **Connect a client** (opens the sheet). **Add a capability** is a quiet link to
  Plugins. The empty calls state does not repeat Connect.
- **Counts:** one line — `50 tools · 8 capabilities · 2 plugins →` — opens Capabilities. No tile grid:
  the catalogue is the catalogue.
- **Recent calls:** the last three, as single-line rows (tool, result, relative time); "All activity →".
  Empty: "No calls yet." pointing at the same Connect button.
- **Notices:** at most one banner, and only for the states that need the operator now: offline, or a
  catalogue the console could not read ("The machine is running. The console could not read its tool
  list, so these counts may be stale." + Retry). Version drift lives on Machine; a failed plugin lives
  on Plugins.
- **Zero state (no machine selected):** the fleet, one row per machine (name, status, build, last
  seen). The whole row opens the machine; the row's overflow holds Rename, Remove and Copy diagnostics.
  One accent only: **Add a machine**, in the page head (the empty state uses the same action, not a
  second primary). "Reconnected after 2 attempts" is the most a row says about the past; the full
  failure sentence is not the card's job.

### 6.2 Capabilities

**Purpose:** show exactly what the AI can call and let the operator prove it.

- **Header:** "Capabilities", count "50 tools · 8 capabilities", search field (filters name,
  description, capability). No source segmented control: the source is a badge, not a mode.
- **Body:** one section per capability: name, source badge, `12 tools · 3 can change the machine`
  (a summary, not a chip on every row), one-line description, then tool rows. A tool row is a button:
  monospace name, one-line description, chevron. Built-ins first, then plugins, each alphabetically;
  within the built-ins, the operator's order (`terminal`, `files`, `screen`, `browser`, `runs`,
  `host`, `plugins`, then the rest).
- **Detail sheet:** name, capability, source, description, annotations (`Read-only`, `Destructive`,
  `Idempotent`, `Open world` only when supplied), the input schema summarised as a field list, and
  **Try it**. The `Destructive` chip appears here, not on every list row.
- **Try it (in the sheet):** one field per top-level schema property (`string`, `number`, `integer`,
  `boolean`, `enum`, arrays as JSON); required marked; description as help; a **Raw JSON** toggle for
  nested types. The form's states are part of the spec:
  - blank optional field = omit the key; blank required field = error on press and focus it
    (`Add a value for "path".`);
  - a boolean is `Not sent / true / false`, never a bare checkbox;
  - a property the summariser cannot render forces Raw JSON (`This tool takes a nested object; edit
    the JSON.`), and the toggle refuses to switch back with `Fix the JSON before switching back.`;
  - **Run** is pinned in the sheet's footer, is the only accent, and becomes `Running… 12s` with
    `This can take up to two minutes.`;
  - the result is formatted JSON when it parses, capped at 40vh / 400 lines with **Show all** and
    **Copy**, and errors are shown verbatim;
  - no answer after two minutes: `No answer after 2 minutes. The call may still be running; check
    Activity.`;
  - a destructive tool shows the confirmation inline above the arguments, with the exact JSON that
    will run and the button `Run once`; success adds `Recorded in Activity.` with a link.
- **States:** loading (three skeleton rows, not a spinner over the page); device offline (one banner,
  Retry, no empty list); no tools (the device is running a build that advertises none — say so and
  point at Machine); no search matches ("No tool matches \"…\"." + "Clear the search."); a stale
  catalogue is labelled, not hidden.
- **Rule:** the page reads the device's live `tools/list`; the static registry is a fallback only,
  and the page must never show a tool the device would refuse as unknown.

### 6.3 Plugins

**Purpose:** add a capability the built-ins do not have, and keep it healthy.

- **Header:** "Plugins", count, **Add plugin** (accent).
- **List:** one row per plugin: name, namespace/version, state chip (`Running`, `Starting`,
  `Stopped`, `Failed`), tool count, source path (truncated, title has the full path); actions
  `Logs`, `Restart`, `Remove` in an overflow menu. A failed row is a real row: name, `Stopped 2m ago`,
  one-line reason, a visible **Restart**, and Logs in the overflow.
- **Add sheet:** a path field that is validated on blur; under it, either the manifest summary
  (`hello · v0.1.0 · 2 tools · runs with this machine's privileges`) or the failure and its action
  (`No plugin.json in that folder.` + `Open the guide`). **Add and run** (accent) stays disabled until
  the path is valid. Trust copy leads with the consequence: `This runs the plugin's code with this
  machine's privileges. Install only code you trust.` Then a disclosure `What is a plugin?` for the
  definition and the manifest keys.
- **Logs sheet:** loaded when the sheet opens, last 200 lines, mono 12 px / 1.5, scrolled to the last
  line when the plugin failed; **Copy**; **Refresh** only while `Starting`; empty: "No output yet." +
  "The plugin has not written anything since it started."
- **Empty state:** "No plugins yet. Add one when a built-in cannot do the job." + the primary action.
  No manifest example on the page; it belongs in the guide.
- **Older device:** if `plugins.list` is not in the catalogue, say "This machine needs a newer
  Summrise to host plugins." with **Open Machine**; do not show an Add button that will fail.
- **Remove:** confirmation names the plugin and says calls to its tools will fail; removing does not
  delete the operator's source directory (v1 registers a path).
- **States:** loading, offline, failed, starting.

### 6.4 Activity

**Purpose:** the record — what was called, by which token, with what result.

Keep the existing table and filters (time window, token, device, tool; CSV export; row detail;
deep link to Runs). The shell gives it one `h1` and the new spacing. Fix the record's own reading:

- the table has no nested `max-height`; while a row is open the page scrolls, not a 17 rem box;
- the whole row toggles its record; one chevron, not a `Details` button per row;
- the device column shows the machine's name (the id in `title`), never a 64-hex string;
- there is no `KIND` column (`mcp` on every row is not information) and no "20 of 20 lines" caption;
- headers are sentence case at 12 px muted, not uppercase micro-labels;
- relative time with the exact date in `title`, grouped by day when the window spans more than one;
- the record is a `10rem minmax(0, 1fr)` grid, max 42 rem, values left-aligned, mono only for
  code-shaped values; Export CSV moves to the overflow.
- Empty state: "No calls in this window." with a **Last week** action when the window is narrower
  than all time. The subtitle "Every call this console recorded…" is deleted: the table is the
  sentence.

### 6.5 Connect (sheet)

**Purpose:** hand the AI a credential and a configuration.

- Three sections, one at a time, in a segmented control: **Endpoint**, **Clients**, **Tokens**. The
  title follows the entry point: `Connect a client` (Endpoint, Clients) or `Tokens` (list + create);
  same sheet, explicit entries.
- **Endpoint:** the console's MCP URL for this device, Copy, and one sentence on what a bearer is.
  A disclosure explains local loopback vs console endpoint.
- **Clients:** DSH / Claude / Codex / Cursor tabs; each shows a config block with `<token>` as a
  placeholder and Copy. After Copy the block collapses to `Copied. It is not shown again.`
- **Tokens:** the existing token table (name, scope summary, created, expires, last used; rotate,
  revoke) and **Create token** with a name, presets first (`Read-only`, `All tools`, `Choose tools…`)
  with a live summary `12 tools · 3 can change the machine`, the full grid only behind
  `Choose tools`, and destructive consent. The created value is shown once in a copy block with a
  warning; closing the sheet forgets it.
- The sheet is wide on desktop (720 px), full-screen on phone; Escape closes; focus returns to the
  trigger.

### 6.6 Device

**Purpose:** the operator's hands; the brake.

- A tab bar: **Terminal · Screen · Files · Runs · Machine** (URL fragment per tab). The selected
  machine's header is above; switching tabs never loses the machine. Desktop: the tab bar sits in
  the page head, not as a third band under the header. Phone: one 44 px header row (machine + `⋯`),
  then a 40 px scrollable segmented tab row; the tab's name is the page's `h1`.
- Terminal is the default tab when the operator chooses Device: the most direct way to take over.
- Each tab keeps its current content and actions, with these changes: no per-page device picker (the
  shell guarantees a device); one `h1` for the destination is the tab's name in the header, so the
  card titles inside become `h2`; the copy that told the operator to pick a machine is deleted. One
  caption line under the `h1` at most; the Files limits move to a `Limits` disclosure at the panel
  foot and the Runs help becomes field help.
- The terminal is one frame, not two: the panel's padding is 0, the canvas goes edge to edge under a
  40 px tab strip, the canvas has no border or radius of its own, and in dark it is one step below
  the page (`--term-canvas: #0f1014`) rather than the page's own colour.
- Screen keeps Browser as its second view; Files and Runs keep their forms and results; Machine keeps
  host facts, install, restart and the process list. Closing a live PTY asks `Close this session?
  The process keeps running on the machine.`
- If the device is offline, the tabs render the last known state where it is cheap (terminal
  sessions) and one banner with Retry where a read is required. No fake data.

### 6.7 Copy deck (the strings that matter)

| Context | Copy |
|---|---|
| Home, running | `Running · v0.3.246 · 50 tools · 2 plugins` |
| Home, no calls | `No calls yet.` (points at the one Connect action) |
| Connect, primary | `Connect a client` |
| Fleet, primary | `Add a machine` |
| Capabilities, empty search | `No tool matches "{query}".` + `Clear the search` |
| Capabilities, stale | `The machine is running. The console could not read its tool list, so these counts may be stale.` + `Retry` |
| Try it, destructive | `This call can change this machine and cannot be undone. Run it?` |
| Try it, time-out | `No answer after 2 minutes. The call may still be running; check Activity.` |
| Plugins, trust | `This runs the plugin's code with this machine's privileges. Install only code you trust.` |
| Plugins, empty | `No plugins yet. Add one when a built-in cannot do the job.` |
| Plugin failed | `{name} stopped. {reason}` + `Restart` / `Logs` |
| Device offline | `{name} is not connected. The console keeps trying; open the machine's panel if it stays offline.` |
| Old build | `This machine needs a newer Summrise to host plugins.` + `Open Machine` |
| Token shown | `Copy this token now. It is not shown again.` |
| Terminal, empty | `No session yet.` |

Rules: sentence case; verbs first; no "simply", "just", "please"; no "permission denied" without the
action that grants it; no sentence that describes the UI ("this page lets you…").

---

## 7. Cross-cutting craft

- **Type:** the seven committed sizes — 10, 11, 12, 13, 14, 17, 22 px — 13 px body, 1.5 line
  height for prose, 1.3 for data. System stack (`-apple-system, "Segoe UI", …`); monospace for tool
  names, paths, logs, endpoints, tokens. A new size is a change to the token and
  `device/console/tests/tokens.rs` in the same commit.
- **Space:** the committed rhythm — 2, 4, 6, 8, 10, 12, 14, 16, 24. Page 24; card 16; row 8/12. No
  ad-hoc margins.
- **Radii:** the committed set — 6, 8, 10, 14, 20, pill 999. One shadow level; no glow.
- **Accent:** the brand orange, and only for the primary action, the current rail item, and focus.
  Destructive actions are text-coloured with a red hover, never a filled red button in the normal
  path. A screen has one accent fill; when a second candidate appears, delete the weaker one rather
  than restyling it.
- **Focus:** 2 px accent ring with 2 px offset, visible on every interactive element in both themes —
  inputs, selects and textareas included; skip link is the first tab stop; sheets trap focus and
  restore it.
- **Hover and press:** the primary darkens (`--accent-hover`), the secondary raises its surface and
  border, and a card does not lift. Motion explains change, not decoration.
- **Sheets:** right-side drawer at ≥800 px (480 px wide), bottom sheet below; backdrop 40% black;
  Escape and backdrop click close (unless a call is running); the title is the sheet's `h2`.
- **Loading:** skeletons for lists, inline "Running…" for actions, no full-page spinner.
- **Errors:** inline where the action is, one sentence + one action; a failed read never renders as
  an empty state.
- **Relative time:** "just now", "12s ago", "3m ago", "2h ago", "3d ago", exact date in title.
- **Mobile:** bottom tabs, 16 px gutters, sheets full-screen, tables become rows (Activity keeps a
  horizontal scroll inside the card, never the page).

---

## 8. Architecture

```
AI client (DSH / Claude / Codex / Cursor / custom)
        │  POST /mcp/<device>        (console: bearer = token or grant)
        ▼
┌────────────────────────── Summrise console ──────────────────────────┐
│ operator page (React) · /fleet · /audit · /tokens · /mcp/<device>     │
│ GET /fleet/:id/tools  ── tools/list over the device channel           │
└───────────────────────────────┬──────────────────────────────────────┘
                                │ outbound WebSocket (device dials; grant/token per frame)
                                ▼
┌────────────────────────── Summrise device ───────────────────────────┐
│ MCP gateway: tools/list (built-ins ∪ healthy plugins) · tools/call     │
│ Registry: 50 built-in tools + dynamic plugin tools                     │
│ Router: name → in-process handler | plugin namespace                   │
│ Supervisor: spawn · initialize · health · restart · logs · stop        │
│ Built-ins: files memory terminal browser monitor runs system host      │
│            screen state plugins                                        │
└───────────────┬──────────────────────────────────┬────────────────────┘
                │ in-process                        │ child process, stdio JSON-RPC
                ▼                                   ▼
        Built-in capability                 Plugin = any MCP server
```

### 8.1 Where the hub lives

The hub is the **device runtime**. It already owns the tools, the audit, the token/grant check and
the channel to the console. A plugin host is a new module in that runtime, not a second service and
not a second database.

The **console** is the window and the door: it serves the operator page, proxies MCP to each device,
mints tokens, and reads the device's live tool list for the UI. It does not execute tools and does not
own plugin state.

### 8.2 The plugin host

**Manifest** (`plugin.json`, UTF-8, no comments):

```json
{
  "id": "com.example.hello",
  "namespace": "hello",
  "name": "Hello",
  "version": "0.1.0",
  "description": "Two friendly tools, and the reference for writing a plugin.",
  "command": ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "plugin.ps1"],
  "cwd": ".",
  "env": { "HELLO_GREETING": "hi" }
}
```

| Field | Rule |
|---|---|
| `id` | required, `[a-z0-9][a-z0-9._-]{2,63}`, unique |
| `namespace` | required, `[a-z][a-z0-9-]{1,31}`, unique, not a built-in capability name |
| `name` | required, 1–64 chars, human |
| `version` | required, semver-ish `x.y.z` |
| `description` | optional, one sentence |
| `command` | required, non-empty array of strings; the executable is `command[0]` |
| `cwd` | optional, relative to the plugin directory; default the directory |
| `env` | optional object of strings; added to the inherited environment |
| `transport` | optional, must be `"stdio"` in v1 |

Unknown keys are rejected (a typo is an error, not a silent default). Validation is a pure function in
`summrise-core`, tested without spawning anything.

**Install** (`plugins.add`, MCP tool, operator authority):

1. Resolve the path (absolute; `~` not expanded in v1), canonicalise, require a directory.
2. Read and validate `plugin.json`; require `command[0]` to resolve (absolute or found on `PATH`).
3. Refuse a namespace or tool-name collision with a built-in or another live plugin.
4. Persist the record in `<data>/plugins.json` (path, manifest, installed_at_ms); start it.
5. Return the plugin state and its tool count.

Installing **registers a path**; the source stays where the developer keeps it. Removing forgets the
record and stops the process; it never deletes the operator's files. This is the v1 contract and the
UI says it.

**Lifecycle:**

```
add → validate → spawn → initialize → tools/list → Running
                              │ failure            │ ping × 3 failed / crash
                              ▼                    ▼
                           Failed ← backoff restart (1s,2s,4s,8s,16s,30s cap)
                              │                    │
                              └── manual restart ──┘
remove / shutdown → close stdin → 2s → kill
```

- Startup timeout 10 s; `ping` every 30 s with a 5 s timeout; three consecutive misses restart.
- After five starts in five minutes the plugin is `Failed` and stays down until **Restart**; its tools
  leave `tools/list` the moment it is not `Running`.
- One in-flight `tools/call` per plugin in v1 (a mutex); calls queue behind it. A call times out after
  120 s with `PLUGIN_TIMEOUT` and a sentence naming the plugin; the plugin is not killed for a slow
  call unless it stops answering pings.
- Logs: stdout/stderr of the child append to `<data>/logs/plugin-<namespace>.log`, rotated at 4 MiB
  (one `.1`); `plugins.logs` returns the last 200 lines. A malformed stdout line is logged and
  skipped; a response id nobody waits for is logged.
- Shutdown: on device exit, stop all plugins (no orphans). The supervisor owns the children.

**Protocol:** MCP `2025-06-18` over stdio, newline-delimited JSON-RPC. Requests carry ids; the client
sends `initialize` (protocolVersion, capabilities, clientInfo), then `notifications/initialized`, then
`tools/list`. `tools/call` forwards the plugin's `result` content unchanged except for the envelope.
`ping` is the health check. A plugin that speaks an older protocol is accepted if it answers
`initialize`; the negotiated version is recorded in the log.

**Tool naming and annotations:** plugin tool `X` is advertised as `<namespace>.<X>`. A name that does
not match `[A-Za-z0-9_.-]{1,64}` is refused and the plugin is marked failed with the offending name.
`tools/list` passes through `title`, `description`, `inputSchema` and the standard annotations.
Destructive is computed as `!readOnlyHint && destructiveHint.unwrap_or(true)` (the MCP default); the
audit line uses the namespaced name, so a plugin's calls are attributable without a schema change.

**Authorization:** the device's existing rule applies unchanged. An operator loopback call and a
console token are checked against the tool's destructive flag; a token that does not name the plugin
tool cannot call it. No new policy object. `plugins.*` are ordinary built-ins with their own
destructive flags (`add`, `remove`, `restart` destructive; `list`, `logs` read-only).

### 8.3 The `plugins.*` tools

| Tool | Input | Output | Destructive |
|---|---|---|---|
| `plugins.list` | `{}` | `[{id,namespace,name,version,description,state,tools,path,last_error}]` | no |
| `plugins.add` | `{path}` | `{plugin, tools}` | yes |
| `plugins.remove` | `{namespace}` | `{removed, stopped}` | yes |
| `plugins.restart` | `{namespace}` | `{plugin}` | yes |
| `plugins.logs` | `{namespace, lines?}` | `{namespace, lines: [string]}` | no |

Errors are the product's shape: `{code, message, action}` in the tool text, e.g.
`PLUGIN_NOT_FOUND` → "no plugin named hello" → `plugins.list`.

These are registry tools, so they appear in `tools/list`, in the token scope picker, and in the
Activity record with no special case. The console page calls them through the existing operator
`/mcp` proxy; the console needs no plugin-specific route.

### 8.4 The console's live catalogue

`GET /fleet/:id/tools` (operator session):

- sends `tools/list` to the device over the channel with the operator's scope (the same mechanism
  `/mcp` already uses server-side);
- returns `{ tools: [...] }` exactly as the device advertises, plus a `source` field computed by the
  console (`builtin` when the name is in `summrise_core::registry`, else `plugin:<namespace>`) and
  the capability name for built-ins;
- 503 when the device is offline, 504 on the channel timeout, with the same recovery sentences the
  page uses elsewhere.

The static `GET /tools` stays for the fleet-token form and as a fallback. The page prefers the live
list; an older device that does not answer still renders built-ins.

`POST /tokens` validation: a tool name in the static registry is accepted as today; a name outside it
is accepted only if the device is online and advertises it (the same live read). If the device is
offline, the request is refused with an action ("connect the machine, then mint the token") rather
than minting a scope that may be a typo. Aggregate tokens keep the static registry for v1; a plugin
tool on an aggregate token is `v1.1`.

### 8.5 The UI

- One `App` owns session, devices, selected device, audit, tokens and the live tool cache. Pages are
  presentational and receive what they render.
- `useLiveTools(deviceId)` fetches `/fleet/:id/tools`, caches per device for 30 s, and is shared by
  Home, Capabilities, Connect and the token form. A failed fetch keeps the last success for the
  session and marks it stale; a stale catalogue is labelled, not hidden.
- Sheets are one component (`Sheet`) with focus trap, Escape, backdrop and scroll lock; dialogs that
  already exist migrate to it.
- The design tokens stay in `tokens.css`; the new rail, header, Home grid, capability list and sheet
  live in `app.css` with the same scale. A styling change that adds a size or a spacing value not on
  the scale fails the token test or is rejected in review.
- `capabilities.ts` (pure): grouping, filtering, destructive computation, schema summarising, example
  values. Tested with vitest; no React in the tests.
- The screenshot harness grows Home, Capabilities, a tool sheet, Plugins (empty and populated) and
  Connect; the gallery is read at 1280 and 390 in both themes.

---

## 9. Trust and access

- **The network boundary is the only access rule.** Loopback needs no token; a non-loopback bind
  needs one bearer token; the console's endpoint needs a token or a grant.
- **Installing a plugin is trusting its code.** It runs as the device's user (SYSTEM when the service
  runs as SYSTEM), with that user's filesystem and network. There is no sandbox, signature or
  registry in v1. The UI says this where the install happens; the docs say it twice.
- **The token is the runtime brake.** It names the tools and whether destructive ones are allowed;
  a plugin tool is in the same scope list as a built-in. Revocation is immediate for new calls.
- **The record is the evidence.** Every call, including a denied one and a plugin call, leaves a begin
  and an end row; a plugin failure is recorded as the tool's failure, not as an unlogged event.
- **The plugin log is the operator's, not the plugin's.** It lives under the data directory, is
  bounded, and is read through `plugins.logs`; a plugin cannot write the audit.
- **No policy language.** A refusal names the consent flag or the next command; it never says
  "permission denied" and stops.

---

## 10. Scope

### v1 (this plan)

- Plugin host: manifest, install/remove/restart/list/logs, stdio client, supervision, health, bounded
  logs; built-ins ∪ plugin tools in `tools/list`; routing and authorization; audit unchanged.
- `plugins.*` tools and a shipped example plugin (`examples/plugins/hello`, PowerShell) that proves
  the contract on Windows.
- Console: live `/fleet/:id/tools`, token-mint validation against it, the new five-destination React
  shell, Home, Capabilities + Try it, Plugins, Connect sheet, Activity in the new shell, Device tabs.
- Deletion: the old rail, the status strip, Memory/Monitors/Events/Security components, the
  duplicate connect dialogs, the per-page device pickers inside Device.
- Docs: this spec, PRODUCT.md, README, QUICKSTART, CONNECT-MCP, a plugin authoring guide; the
  implementation plan.
- Deploy: the console to `DESKTOP-9VH92BJ`, verified by the runbook; the device runtime to the
  canary machine only if the plugin host can be proven and rolled back. The console alone must not
  ship a page that requires a device feature no device has: the UI degrades on old builds.

### v1.1 (named, not built)

- Git and npm plugin sources; plugin update.
- `summrise` CLI for headless installs and `plugin` verbs.
- Aggregate-token scopes over plugin tools.
- Plugin idle stop and per-plugin resource caps.

### Not this product

- marketplace, registry, signing, sandboxing, WASM plugins;
- permission profiles, approvals queue, observe mode, audit export for compliance;
- multi-tenant, teams, roles, agent orchestration, task boards, chat;
- Linux/macOS device runtime (the protocol is portable; the shell, service and screen are not v1);
- fleet management beyond the operator's handful of machines.

---

## 11. Acceptance

A reviewer can check each line; "it compiles" is not a line.

**Device**

1. `plugins.add` on `examples/plugins/hello` starts it, `tools/list` includes `hello.greet`, and
   `tools/call` returns the plugin's answer.
2. A token that names `hello.greet` calls it; a token that does not is refused with the same denial
   shape as a built-in.
3. Killing the plugin process makes its tools disappear from `tools/list`, restarts it, and they come
   back; the log has both facts.
4. A plugin with an invalid manifest, a colliding namespace or a colliding tool name is refused with
   a sentence naming the field or the collision, and nothing is spawned.
5. The device exits; no plugin process survives.
6. A slow plugin call times out with `PLUGIN_TIMEOUT`; the device stays healthy.

**Console**

7. `GET /fleet/:id/tools` returns the same tool names as the device's `tools/list`; an offline device
   returns 503 with a recovery sentence.
8. Minting a token with a plugin tool succeeds when the device is online and is refused offline.

**Page**

9. Five destinations; no dead rail link; every page has one `h1` and fits 360/390/800/1280 without a
   horizontal scrollbar.
10. Home shows the endpoint, counts, recent calls and two actions; with no machine it shows the
    fleet.
11. A destructive tool cannot be run from Try it without the confirmation; its result and its Activity
    row appear.
12. An old device (no `plugins.*`) renders Plugins with the "newer Summrise" sentence and no Add
    button.
13. The token value is shown once, is not in the DOM after the sheet closes, and never appears in the
    audit.

**Deploy**

14. `scripts/build-web.sh --check` passes; the console stage script accepts the tree; the deployed
    `/health` names the new version and the served page is the committed `dist/index.html` byte for
    byte; the real-device terminal check prints `REAL_TERMINAL_OK`.
15. The real-device plugin check (add, call, remove) passes against the canary, or the device half is
    explicitly deferred with the console UI degrading as in 12.

---

## 12. Delivery

| Milestone | Outcome | Gate |
|---|---|---|
| **M0 Plan** | this spec + the implementation plan + PRODUCT.md revision | review of this document |
| **M1 Device** | plugin host, `plugins.*`, routing, audit, example plugin | `cargo test -p summrise-device -p summrise-core`; the six device acceptance lines |
| **M2 Console** | live catalogue, token validation, new shell, Home, Capabilities, Try it, Plugins, Connect, Device tabs | `cargo test -p summrise-console`; `npm run typecheck && npm test && npm run build`; screenshots |
| **M3 Docs** | PRODUCT/README/QUICKSTART/CONNECT/plugin guide, exact and current | copy audit against the code |
| **M4 Deploy** | console on the canary, verified; device only with a rollback | the deploy runbook + acceptance 14/15 |

The implementation plan carries the task-level file map, interfaces and tests. Tasks are dispatched
to subagents; the Lead reviews the diff, runs the gates and deploys.

---

## 13. Deployment and rollback

- **Console** — the verified path: `scripts/stage-console-deploy.bash <version> --mirror-dir
  /tmp/canary-mirror`, then the host fetches `deploy-console-<version>.cmd`, verifies SHA-256, stops
  the task, backs up, swaps, starts, and prints `/health`. Verify `/health`, the served page's size
  against `dist/index.html`, and `scripts/real-device-check.ps1`. Rollback is the backup binary and
  the same three task commands (`docs/console-deploy.md`).
- **Device** — only through a signed release document or a supervised swap with a backup. The
  launcher owns the binary; a manual copy while the service runs is forbidden. If the canary device
  cannot be updated safely in this round, the console ships with the old-build degradation and the
  device half is the next release; the product must not claim a feature no device runs.
- **Gate** — stage refuses a dirty tree, a `HEAD` that does not contain `origin/main`, or a stale
  page. Build from a clean tree that contains current `origin/main`.
- **After** — keep the last two backups, delete older `summrise-console.exe.bak-*`, and record the
  measured facts in the runbook.

---

## 14. Risks

| Risk | What it would do | Mitigation |
|---|---|---|
| **Cold start** | a hub with no plugins is "just another MCP server" | the 50 built-ins carry v1; plugins are the escape hatch, never the headline; Home sells the built-ins first |
| **Trust** | a plugin run as SYSTEM is a remote-code-execution feature wearing a UI | say it plainly at install; keep the token brake; no marketplace; the example is source the operator can read |
| **Windows-only plugins** | an npm/git ecosystem does not exist for this host | v1 plugins are directories with any command; PowerShell example; docs state the platform |
| **The console swallows the hub** | seven surfaces return as ten | five destinations are a tested invariant; a new rail item needs a deletion in the same change |
| **Two sources of truth for tools** | the page offers a tool the device refuses | the live catalogue is the page's read; the static registry is fallback only; acceptance 7/12 |
| **Protocol drift** | an MCP server that speaks a newer protocol fails | negotiate `2025-06-18`, accept the server's answer, log the version; a failed handshake is a sentence, not a crash |
| **Deploy asymmetry** | console ships hub UI, devices do not have the host | degrade on old builds (acceptance 12); device release separate and explicit |

---

## 15. Decision log

1. **The hub is the product; the console is its window.** The runtime already is the hub's execution
   layer; no second service, database or config file.
2. **Five destinations.** Home, Capabilities, Plugins, Activity, Device. Fleet is Home's zero state;
   Connect is a sheet. A destination must own a daily job.
3. **A plugin is an MCP server registered by path.** No copy, no marketplace, no sandbox; installing
   is trusting. The path stays the developer's; removal forgets it.
4. **`plugins.*` are built-in tools, not a second API.** The console, any MCP client and the tests
   drive the same surface; the audit and the token scope need no new model.
5. **The live catalogue is the console's source of truth per device.** The static registry is a
   fallback and the built-in flag, not the list the page renders.
6. **No CLI in v1.** The installer + service + console is the one path; a second path is a second
   thing to keep correct. `v1.1`.
7. **Destructive follows MCP's defaults.** `!readOnlyHint && destructiveHint.unwrap_or(true)`; a
   plugin that omits annotations is treated as destructive.
8. **The console and the device deploy separately.** The page degrades on old builds; no deploy
   claims a device feature it cannot verify.
9. **Deletion precedes addition.** The old rail, the status strip, the dead pages and the duplicate
   dialogs go in the same change that introduces the new shell.
10. **Every screen is measured.** One `h1`, one accent, the scale, the layout gate, the screenshots,
    the contrast test — a styling change without them is a guess.
