# P0 — the inventory, the baseline, and the toolchain proof

The first step of `2026-09-28-the-product-moves-to-rust.md`, on branch
`feat/the-product-moves-to-rust`. **It changes no product code, adds no gate, adds no test and adds
no CI job.** Three deliverables, in the plan's own order:

| | deliverable | where |
|---|---|---|
| ① | which exports are LOGIC and which are RENDERING | this file, below |
| ② | the numbers P2 will be judged against | this file, below |
| ③ | `wasm-pack` working here | [`wasm-pack-proof/`](wasm-pack-proof/README.md) |

Everything here is reproducible from this directory: `inventory.mjs` prints the list and its counts,
`sizes.sh` prints the byte sizes, `first-render.mjs` prints the paint timings.

**Nothing in this P0 is a plan for P2.** It is the material P2 picks from.

---

## ① THE LIST — what is LOGIC, and what is RENDERING

### The rule, applied per EXPORT

> **LOGIC** if it computes, parses, derives, validates, formats or decides.
> **RENDERING** if it only places what it was given.

**A third class exists, and the plan's two words do not name it — so it is named here, because P2
will otherwise mistake it for a target.** An export that only talks to a browser or network API
(`fetch`, `document`, `window`, `localStorage`, `navigator`, `Notification`, canvas, `postMessage`)
neither computes nor places: it is the **BOUNDARY**, and it can never move to Rust. It is the seam
the wasm sits behind. `lib/api.ts`'s `callApi`, `lib/theme.ts`'s `setTheme` and every `useXxx` hook
that wires state to a subscription are boundary code, and a list that called them "rendering" would
send P2 looking for a JSX tree that is not there.

The three sides are reported separately in the tables. **A file can hold all three** — that is the
normal case and the reason this is classified by export rather than by file.

### How the list was produced

```bash
node docs/superpowers/p0/inventory.mjs .        # prints both tables and the counts
```

`inventory.mjs` holds the classification as DATA and re-reads every export out of the sources, so a
file that gains an export cannot silently leave the list, and **the count and the list cannot drift
apart** — change a classification and the number changes with it. It also throws if a name is
classified that the file does not actually export.

**Excluded from the list: the test files.** `*.test.ts(x)`, `__tests__/`, `test-setup.ts` and
`test-utils/` are neither logic nor rendering — they are the evidence, and P2's criterion ① (the
same input, two versions, the same output) is built out of them. They are counted in ② instead.

### The counts

| UI | LOGIC exports | files | other exports (rendering + boundary) |
|---|---|---|---|
| panel `agent/resources/panel-react/src` | **156** | **46** | 102 |
| console `gateway/ui/src` | **31** | **8** | 22 |
| **total** | **187** | **54** | 124 |

One caveat on the panel's 156: `humanIdle` is *defined* in `lib/duration.ts` and re-exported by
`lib/evicted.ts` and `lib/idleSessions.ts`, so 156 export entries are **154 distinct definitions**.
The table counts what each file exports, because that is what a caller imports.

---


### The panel — agent/resources/panel-react/src

| file | LOGIC (migration candidates) | RENDERING / BOUNDARY (stays TS) |
|---|---|---|
| `App.tsx` | — | `App` |
| `components/ActivityPage.tsx` | — | `ActivityPage` |
| `components/ApprovalGate.tsx` | `firstWord` | `ApprovalGate` |
| `components/ArchivePage.tsx` | — | `ArchivePage` |
| `components/BootChip.tsx` | — | `BootChip` |
| `components/BrowserPage.tsx` | — | `BrowserPage` |
| `components/CommandCard.tsx` | `fmtDuration` | `CopyButton` · `CommandCard` · `CommandStream` |
| `components/ConnectCard.tsx` | — | `ConnectCard` |
| `components/ConnModal.tsx` | — | `ConnModal` |
| `components/ContextRail.tsx` | — | `ContextRail` |
| `components/DesktopShell.tsx` | — | `DesktopShell` |
| `components/DetailsPanel.tsx` | — | `DetailsPanel` |
| `components/DeviceHealthCard.tsx` | — | `DeviceHealthCard` |
| `components/DeviceLogsCard.tsx` | — | `DeviceLogsCard` |
| `components/EmbeddedBrowserPane.tsx` | — | `EmbeddedBrowserPane` |
| `components/ErrorBoundary.tsx` | — | `ErrorBoundary` |
| `components/EvictedNotice.tsx` | — | `EvictedNotice` |
| `components/EvidenceDrawer.tsx` | — | `EvidenceDrawer` |
| `components/GettingStarted.tsx` | — | `GettingStarted` |
| `components/GoalBar.tsx` | — | `GoalBar` |
| `components/HistoryPage.tsx` | — | `HistoryPage` |
| `components/IconRail.tsx` | `PAGE_ICONS` | `IconRail` |
| `components/IdleSessionsBar.tsx` | — | `IdleSessionsBar` |
| `components/LoadChip.tsx` | — | `LoadChip` |
| `components/MemoryPage.tsx` | — | `MemoryPage` |
| `components/MonitorAlerts.tsx` | — | `MonitorAlerts` |
| `components/MonitorChip.tsx` | — | `MonitorChip` |
| `components/MonitorsCard.tsx` | — | `MonitorsCard` |
| `components/NotificationsCard.tsx` | — | `NotificationsCard` |
| `components/PanelApp.tsx` | — | `PanelApp` |
| `components/PathView.tsx` | — | `PathView` |
| `components/PluginsPage.tsx` | — | `PluginsPage` |
| `components/RestartHistoryCard.tsx` | — | `RestartHistoryCard` |
| `components/RunStrip.tsx` | `clock` | `RunGroupHead` · `RunStrip` |
| `components/SessionControl.tsx` | — | `SessionControl` |
| `components/SettingsPage.tsx` | — | `SettingsPage` |
| `components/Shell.tsx` | `Page` · `PAGES` · `PAGE_LABELS` | `Shell` |
| `components/Sparkline.tsx` | — | `Sparkline` |
| `components/StatusBar.tsx` | — | `StatusBar` |
| `components/TabBar.tsx` | `SessionView` | `StripMore` · `TabBar` |
| `components/TerminalPane.tsx` | — | `TerminalPane` |
| `components/TerminalWorkspace.tsx` | `CommandEvents` | `TerminalWorkspace` |
| `components/TrajectoryView.tsx` | — | `TrajectoryView` |
| `components/UpdateCard.tsx` | `UpdateStatus` · `parseUpdateStatus` · `parseAttempt` · `attemptAge` · `checkedAge` | `useUpdateStatus` · `UpdateCard` |
| `components/ViewSwitch.tsx` | — | `ViewSwitch` |
| `components/VitalsDial.tsx` | — | `VitalsDial` |
| `components/WaitingChip.tsx` | — | `WaitingChip` |
| `hooks/useActiveTabVisible.ts` | — | `useActiveTabVisible` |
| `hooks/useAgentVitals.ts` | `AgentVitals` · `BootKind` · `LastBoot` · `parseLastBoot` · `fmtUptime` | `useAgentVitals` |
| `hooks/useAiActivityPulse.ts` | `PULSE_MS` | `useAiActivityPulse` |
| `hooks/useAttention.ts` | `humanMs` | `useNotifyPermission` · `useAttention` · `useAttentionTitle` · `useAttentionNotifications` |
| `hooks/useBootHistory.ts` | `BootRecord` · `BootHistory` · `EMPTY_BOOT_HISTORY` · `parseBootHistory` | `useBootHistory` |
| `hooks/useCommandEvents.ts` | `CommandEvent` · `CommandCard` · `terminalStatus` · `groupEvents` | `useCommandEvents` |
| `hooks/useDesktopCommands.ts` | — | `useDesktopCommands` |
| `hooks/useDeviceActivity.ts` | `WORKING_MS` | `useDeviceActivity` |
| `hooks/useDeviceRead.ts` | `DeviceReadOptions` · `DeviceRead` | `useDeviceRead` |
| `hooks/useEvicted.ts` | — | `useEvictedNotice` |
| `hooks/useMonitors.ts` | `MonitorTarget` · `Monitors` · `EMPTY_MONITORS` · `parseMonitors` · `MonitorAlert` · `parseMonitorChange` · `fmtSince` · `unstableTargets` · `downTargets` | `useMonitors` · `useMonitorAlerts` |
| `hooks/useNow.ts` | — | `useNow` |
| `hooks/useOperationRuns.ts` | — | `useOperationRuns` |
| `hooks/usePlugins.ts` | — | `usePlugins` |
| `hooks/useSessionArchive.ts` | — | `useSessionArchive` |
| `hooks/useSessions.ts` | `PendingApproval` · `mapPending` · `pendingApprovalCount` · `Session` | `useSessions` |
| `hooks/useSSE.ts` | — | `useSSE` |
| `hooks/useStripOverflow.ts` | — | `useStripOverflow` |
| `hooks/useTrajectory.ts` | `TrajRound` · `groupRounds` | `useTrajectory` |
| `hooks/useVitalsSeries.ts` | `VitalsSeries` · `EMPTY_SERIES` · `parseVitalsSeries` | `useVitalsSeries` |
| `lib/agentVersion.ts` | `releaseVersion` · `releaseVersionLabel` | — |
| `lib/ansi.ts` | `stripAnsi` | — |
| `lib/api.ts` | `deviceRefused` | `initTransport` · `getHost` · `getToken` · `callApi` · `callTool` |
| `lib/archive.ts` | `ArchiveEntry` · `ARCHIVE_PAGE` · `archiveEntries` · `newestFirst` · `pageOf` · `archiveClock` · `lastEventWords` | — |
| `lib/attention.ts` | `stateKey` · `shouldNotify` · `AttentionItem` · `BASE_TITLE` · `attentionFrom` · `titleFor` · `badgeIcon` · `attentionSummary` | `inDesktopWindow` |
| `lib/boot.ts` | — | `computeBoot` |
| `lib/bootNotice.ts` | `bootKindLabel` · `isCrash` · `REPLACED_NOTICE_SECS` · `bootNotice` | — |
| `lib/browserAction.ts` | `actionVerdict` | — |
| `lib/clipboard.ts` | — | `copyText` |
| `lib/contract.gen.ts` | `FRAMES` · `BOOT_KINDS` · `END_REASONS` · `EXITED_PREFIX` · `Frame` · `BootKind` · `EndReason` | — |
| `lib/download.ts` | — | `DownloadTarget` · `downloadBlob` |
| `lib/duration.ts` | `humanIdle` | — |
| `lib/embeddedBridge.ts` | — | `EMBEDDED_MEMBERS` · `BROWSER_MEMBERS` · `DESKTOP_MEMBERS` · `embeddedBridge` · `browserBridge` · `desktopBridge` |
| `lib/embeddedNav.ts` | — | `shouldAcceptNavPush` |
| `lib/evicted.ts` | `EvictionNotice` · `parseEvicted` · `evictedText` · `humanIdle` | — |
| `lib/gettingStarted.ts` | `GETTING_STARTED_VERSION` · `GETTING_STARTED_KEY` · `STEPS` · `GETTING_STARTED_LEAD` · `GETTING_STARTED_REOPEN` · `shouldShowGuide` | — |
| `lib/idleSessions.ts` | `IDLE_OFFER_MS` · `idleSessions` · `idleOfferText` · `humanIdle` | — |
| `lib/lagMarkers.ts` | `pruneLagMarkers` | — |
| `lib/liveness.ts` | `Liveness` · `URGENCY` · `SILHOUETTE` · `MOVES` · `livenessOf` · `deviceLiveness` · `sessionWaiting` · `sessionActive` · `anyCommandRunning` · `sessionLiveness` · `sessionFailed` | — |
| `lib/monitorMark.ts` | `monitorModifier` · `monitorMarkClass` | — |
| `lib/notify.ts` | `NotifyPermission` · `NotificationLike` · `NotificationCtor` · `readPermission` · `permissionHint` | `DeviceNotifier` |
| `lib/particles.ts` | — | `startParticleField` |
| `lib/path.ts` | `PATH_STATES` · `PathState` · `stateFromEnd` · `cardState` · `PathStep` · `PathSummary` · `SessionPath` · `derivePath` · `summarizePath` · `attentionSteps` | — |
| `lib/readState.ts` | `ReadState` | — |
| `lib/recipe.ts` | `RECIPE_MARKER` · `RECIPE_TAG` · `suggestedTitle` · `buildRecipe` · `recipeWarnings` | — |
| `lib/runs.ts` | `OperationEvent` · `RunBoundary` · `ActivityRow` · `RunGroup` · `groupOperation` · `operationRows` · `groupCount` · `RUN_STATE_LABEL` · `runStateNote` | — |
| `lib/sessionLabels.ts` | `disambiguateLabels` | — |
| `lib/sessionViews.ts` | `pruneSessionViews` | — |
| `lib/spark.ts` | `LOAD_MIN_SAMPLES` · `LOAD_WINDOW_MS` · `sparkSegments` · `seriesStats` · `loadNotice` | — |
| `lib/terminalAdopt.ts` | `MAX_ADOPT_PAGES` · `adoptNeedsAnotherPage` · `adoptPageExceeded` · `WRITE_SLICE_CHARS` · `splitWriteSlices` | — |
| `lib/theme.ts` | — | `getTheme` · `setTheme` · `toggleTheme` · `onThemeChange` |
| `lib/trailRead.ts` | `trailReadNotice` | — |
| `lib/updateDiagnosis.ts` | `UpdateDiagnosis` · `diagnoseUpdate` | — |
| `lib/useAck.ts` | — | `useAck` |
| `ui/Icon.tsx` | `IconName` | `Icon` · `BrandMark` |

**156 LOGIC exports across 46 files** (of 102 files exporting anything; the other 102 exports are rendering or boundary).

### The console — gateway/ui/src

| file | LOGIC (migration candidates) | RENDERING / BOUNDARY (stays TS) |
|---|---|---|
| `api/client.ts` | `ApiError` · `ProviderView` · `ModelFacets` · `ProbeResult` · `ProviderModelDraft` · `Me` · `RouteInfo` · `HealthChannel` · `User` · `Device` · `DeviceStatus` · `RegKeyInfo` | `api` |
| `components/ui.tsx` | — | `PageHeader` · `Card` · `Badge` · `CopyButton` · `Modal` · `Empty` |
| `contexts/AuthContext.tsx` | — | `AuthProvider` · `useAuth` |
| `contexts/ToastContext.tsx` | — | `ToastProvider` · `useToast` |
| `i18n.ts` | `TranslationKey` · `t` | `setLang` · `useTranslation` |
| `lib/baseUrl.ts` | — | `clientBase` |
| `lib/channelState.ts` | `channelSignal` · `channelLabel` · `healthTone` | — |
| `lib/deviceState.ts` | `tunnelKnownDown` · `deviceIsUp` · `agentSignal` · `tunnelSignal` · `CONSOLE_POLL_MS` · `deviceTally` · `DeviceTally` · `Signal` | — |
| `lib/deviceUpdate.ts` | `rememberedUpdateAttempt` · `updateControl` | `rememberUpdateAttempt` · `forgetUpdateAttempt` |
| `lib/format.ts` | `maskToken` | — |
| `lib/keyNames.ts` | `KEY_NAMES` | — |
| `lib/lane.ts` | `barePrefix` · `laneClass` | — |
| `lib/particles.ts` | — | `startParticleField` |
| `lib/theme.ts` | — | `getTheme` · `toggleTheme` |
| `lib/unauthorized.ts` | — | `setUnauthorizedHandler` · `notifyUnauthorized` |
| `lib/useAck.ts` | — | `useAck` |

**31 LOGIC exports across 8 files** (of 16 files exporting anything; the other 22 exports are rendering or boundary).

### The total

**187 logic exports across 54 files** (panel + console).

---

### What the list says about where P2 should start

Three observations that fall out of the tables and change the order of the work:

**1. The panel's logic is concentrated in `lib/`, and it is already pure.** 105 of the 156 panel
exports live in `lib/`, and 26 of the 46 files are `lib/` files. Most of them import nothing at all —
`liveness.ts` (11 exports, 224 lines), `path.ts` (10, 304), `runs.ts` (9, 512) and `archive.ts` (7,
205) are data-in/data-out over JSON. These are the cheapest and largest wins, and they are what the
plan's "统一收益最大" means concretely.

**2. The hooks are a split, not a unit.** 36 of the panel's 156 exports are in `hooks/`, and in every
case the *derivation* is separable from the *wiring*: `useMonitors.ts` exports 9 things of which 7 are
pure (`parseMonitors`, `parseMonitorChange`, `fmtSince`, `unstableTargets`, `downTargets` and two
types) and 2 are the React hooks. P2's move for a hook is therefore "the parse and the derivation go
to Rust, the `useEffect` stays and calls it" — not "the hook goes to Rust", which cannot work.

**3. The console's logic is small and already almost pure.** 31 exports across 8 files, and 12 of them
are type declarations in `api/client.ts` that describe the wire shape. The two that carry real
behaviour are `i18n.ts`'s `t` (a dictionary lookup with interpolation, over an 875-line file) and
`deviceState.ts`'s eight device-fact functions. **The console is a much smaller P2 than the panel**,
which is worth knowing before it is scheduled as though it were the same size.

**And one finding that is an argument FOR the migration rather than a task in it:** `lib/contract.gen.ts`
is *generated* — its header says `Source of truth: agent/src/vocabulary.rs`, refreshed by
`SUMMRISE_REFRESH_CONTRACT=1 cargo test contract_vocabulary_snapshot` and checked by
`contract-vocabulary-check.mjs`. **The panel's control-frame vocabulary is already owned by Rust and
mirrored into TypeScript by a generator.** The direction of that mirror is the whole migration in
miniature, and it is the existing proof that this repository can hold one fact in Rust and consume it
in the UI.

---

## ② THE BASELINE — the numbers P2 is judged against

Every number below has the command that produced it. Two of them are quoted from a tool that reports
in decimal `kB` (vite) and are **not** used: the byte counts come from `stat -c%s`, and are
cross-checked against a real HTTP fetch.

### The numbers

| | panel | console |
|---|---|---|
| **first-load payload, raw** | **952,231 B** | **380,754 B** |
| **first-load payload, gzipped** | **271,487 B** | **112,718 B** |
| **first contentful paint (median of 5)** | **80 ms** | **108 ms** |
| **tests that pass** | **906** (112 files) | **50** |
| build exit code | 0 | 0 |
| test exit code | 0 | 0 |

### The commands

**Build and test — the commands CI runs, from each UI's own directory:**

```bash
cd agent/resources/panel-react && npm run build && npm test   # -> 112 files, 906 tests, exit 0
cd gateway/ui                && npm run build && npm test     # -> 50 tests, 0 fail,   exit 0
```

(The console's `npm run build` is `tsc -b && vite build && prune-stale-assets` — **not**
`tsc --noEmit`, which is the check that once let six type errors through. The panel's `npm test` is
`vitest run`; its reporter line is `Tests  906 passed`.)

**Byte sizes — `stat -c%s` for raw, `gzip -9` for gzipped, and `curl -w '%{size_download}'` against a
real loopback fetch as an independent check:**

```bash
docs/superpowers/p0/sizes.sh agent/resources/panel 18099 index.html panel.css vendor/xterm.css panel.js
docs/superpowers/p0/sizes.sh gateway/public        18098 index.html assets/index-B1XnjM77.css assets/index-DrYd0E19.js favicon.svg
```

| panel file | raw | gzipped | curl |
|---|---|---|---|
| `index.html` | 1,032 | 614 | 1,032 |
| `panel.css` | 282,231 | 80,849 | 282,231 |
| `vendor/xterm.css` | 5,383 | 2,037 | 5,383 |
| `panel.js` | 663,585 | 187,987 | 663,585 |
| **total** | **952,231** | **271,487** | **952,231** |

| console file | raw | gzipped | curl |
|---|---|---|---|
| `index.html` | 1,391 | 650 | 1,391 |
| `assets/index-B1XnjM77.css` | 36,272 | 7,090 | 36,272 |
| `assets/index-DrYd0E19.js` | 341,075 | 104,077 | 341,075 |
| `favicon.svg` | 2,016 | 901 | 2,016 |
| **total** | **380,754** | **112,718** | **380,754** |

**`stat` and `curl` agree to the byte on every file**, which is the point of running both: this
repository was burned by a `31140 bytes` figure that was JavaScript's `String.length` (UTF-16 units)
when the real size was 31,718 bytes. Neither number above is a `.length`.

**First render — a real Chromium, five runs each, 1280×900, route interception so no listener is
opened:**

```bash
# the panel is measured on the repository's own harness, in its URL mode (so panel.js is FETCHED):
SUMMRISE_PANEL_BUNDLE_URL=http://summrise.test/panel \
  node agent/scripts/panel-render-audit.mjs --out /tmp/p0     # exits 2 — that IS its emit path

export LD_LIBRARY_PATH=/tmp/chromium-libs/prefix/usr/lib/x86_64-linux-gnu
export SUMMRISE_CHROMIUM_PATH=~/.cache/ms-playwright/chromium-1228/chrome-linux64/chrome
node docs/superpowers/p0/first-render.mjs panel   5
node docs/superpowers/p0/first-render.mjs console 5
```

| | panel FCP | console FCP |
|---|---|---|
| samples (ms) | 88, 80, 80, 100, 76 | 116, 112, 108, 100, 96 |
| **median** | **80** | **108** |
| min / max | 76 / 100 | 96 / 116 |
| first paint | same as FCP | 72, 64, 76, 64, 64 |
| DOMContentLoaded | ~109–114 | ~75–94 |

**One trap in reproducing this from a worktree:** a fresh worktree has **no `node_modules`**, so
`npm run build` and `npm test` fail with `rc=127` in both UIs — and `all-gates` then reports three
gates failing for a reason that is environmental rather than real. Point the worktree at the main
checkout's installs (the two `package-lock.json`s are identical, and the UI sources are byte-identical
between this branch and `main`, which is what makes the sharing safe):

```bash
ln -sfn <main-checkout>/agent/resources/panel-react/node_modules agent/resources/panel-react/node_modules
ln -sfn <main-checkout>/gateway/ui/node_modules                gateway/ui/node_modules
```

**Do not commit those symlinks** — `**/node_modules/` in `.gitignore` matches a *directory* and not a
symlink, so they show up as untracked.

### What each number is NOT — because a baseline that overstates itself is worse than none- **The panel's FCP is measured on the repository's own harness** (`panel-render-audit.mjs` with a
  stubbed device API), not on a device with a live agent. It links `panel.css` and `panel.js` and
  **does not link `vendor/xterm.css`**, so it is 5,383 bytes lighter than the real page. The *payload*
  figure above is the real page's, and includes it.
- **The console's FCP is its login page** — `/api/*` answers 401, which is `render-smoke.mjs`'s own
  assumption. A signed-in view renders more.
- **The console is served gzipped by a Cloudflare Worker in production**; the local measurement serves
  the raw bytes, so its FCP is if anything pessimistic against production and optimistic against a
  slow network. It is a comparison instrument for P2, not a claim about a user's experience.
- **These are one machine's numbers.** P2's criterion is the *same instrument, before and after*, so
  the absolute values matter less than that the command is fixed and re-runnable.

### What the baseline already tells P2

The plan warns that a naive port pays an invisible cost, and the spike measured it: 8,879 gzipped
bytes — **26 % of that payload** — from one `format!("{:.3}")` on an `f64`, because `{:.3}` links
Rust's float formatter.

**Put beside the console's 112,718 gzipped bytes, that is the number to keep in view:** a single
formatting call can cost 8 % of the console's entire first-load payload. The panel's 271,487 has more
room, but 156 exports is 156 chances to pay it. **P2's criterion ② (the size delta of each move) is
not a formality; it is the measurement that decides whether the move stays.**

---

## ③ `wasm-pack` — it works. See [`wasm-pack-proof/`](wasm-pack-proof/README.md)

**`wasm-pack` 0.15.0 builds, and the output imports and runs in a real browser.** Not a stop
condition; the summary is:

```bash
cd docs/superpowers/p0/wasm-pack-proof && WASM_OPT=$(command -v wasm-opt) ./build.sh
# wasm-pack build --target web --no-opt   ->  rc=0
# wasm as emitted :  22800 bytes /  8894 gzipped
# js glue         :   6960 bytes /  2089 gzipped
# wasm optimized  :  15662 bytes /  7016 gzipped
```

Imported in Chromium from the emitted glue: `add(19, 23)` → `42`, `echo("hello")` → `"wasm:hello"`,
no page errors, exports `add`, `default`, `echo`, `initSync`.

**Two costs, both documented in that README and both relevant to P2:**

1. **Plain `wasm-pack build --target web` hangs on this box, and it is not the compile.** The crate
   compiles in 8 seconds; wasm-pack then spends 14 minutes holding a lock file while fetching
   binaryen from a GitHub release that never arrives. `--no-opt` is the working form, and wasm-opt
   runs as a separate step from an `npm i binaryen` copy.
2. **The two `wasm-opt` flags are a property of the code, not of the toolchain** — measured both
   ways. The spike's module *is* refused without them (`memory.copy operations require bulk memory
   operations`); this minimal crate is *accepted* without them, byte-identically. Pass them always.

---

## The two decisions this P0 was asked to make

### Where `wasm32-unknown-unknown` is declared

**In a `rust-toolchain.toml` inside each wasm crate — not in the repository root's.**

The root file is the release pipeline's byte-identity guarantee for `summrise-agent.exe`, and its own
comment says a change there must be a deliberate, visible commit. **A wasm target does not affect that
exe, but editing the file that guarantees it, in order to build something that is not the exe, is how
a guarantee stops being one.** So the root file is untouched, and `wasm-pack-proof/rust-toolchain.toml`
declares the target next to the thing that needs it.

This works because **rustup resolves the nearest toolchain file walking up from the working
directory** — measured: from inside the crate, `rustup show active-toolchain` answers
`1.98.1-x86_64-unknown-linux-gnu (overridden by '/tmp/rustmig/wasm-pack-proof/rust-toolchain.toml')`,
and from `agent/` it still resolves to the root's. **The channel is deliberately the same 1.98.1**, so
one compiler version builds the exe and the wasm and a toolchain bump stays one commit at the root.

**And it is what makes a fresh checkout work.** The root file declares only
`x86_64-pc-windows-msvc`; `wasm32-unknown-unknown` is installed on this box because somebody ran
`rustup target add` by hand, which no clone reproduces. A target listed in a toolchain file is one
rustup ensures. *(Verified: the resolution above, and that the target is installed for 1.98.1.
Not verified: the auto-install on a machine that lacks it — that would mean removing an installed
target from a toolchain this box shares with the agent build.)*

### Framework, or none — for a UI that DOES have a component tree

**None. And the reasoning is different from the spike's, which is why it is stated rather than
inherited.**

The spike answered "no framework" for the landing page because that page *has* no component tree,
no routing and no state graph — a framework there would have been a large dependency for a document.
**The panel and the console do have those things**, so the spike's reason does not transfer and the
question has to be answered again. It comes out the same way, for a different reason:

1. **The plan already fixes the method, and it is not a rewrite.** *"`wasm-pack build --target web` →
   a `.js` glue + a `.wasm`, imported from React like any module. React, Radix and Tailwind stay
   exactly as they are; only logic moves."* A framework would mean rebuilding the component tree in
   Rust — which the plan explicitly rules out, because *"rewriting would stop new features for
   months, and React can call wasm"*.
2. **A framework here would be a SECOND component tree, not a replacement for the first.** The 46
   panel files that hold logic are mostly not components at all — 26 of them are `lib/`, and 9 of the
   46 are components holding exactly one pure helper each (`fmtDuration`, `firstWord`, `clock`). There
   is no tree to move.
3. **The measured floors are the wrong shape for this job.** Sycamore 25,275 gz · Leptos 33,394 gz ·
   Yew 57,068 gz. Against the console's entire 112,718 gz payload, Yew alone would be **half of it
   again** — and each of those numbers buys a component tree that the migration is not allowed to
   use. (`dioxus-web` is `0.8.0-alpha.1` and is not a basis for a decision, as the brief says.)
4. **What a framework would actually have to replace is React's state, and React's state is not the
   logic.** The hooks are where the boundary lives; criterion ③ of P2 — *the wasm loads only when it
   is needed* — is a statement about a module fetch, which is exactly what `--target web` glue gives
   and what a framework's monolithic bundle makes harder.

**What would change this answer:** if a UI ever needed to render *without* React — a second consumer
of the same component tree — the trade flips, because then the framework is not a duplicate tree but
the only one. Nothing in the plan asks for that.

---

## What the source contradicted in the brief

Four things, all measured rather than argued:

1. **"`wasm-opt` 132 refuses Rust 1.98's output without the two flags" — conditionally true.** It
   refuses *the spike's* output and accepts *this crate's*, at the same wasm-opt version, with the
   same declared target features. The variable is whether the compiled code contains a `memory.copy`
   (spike: yes; `add`/`echo`: no). See ③ above and the crate README's table. **The practical
   consequence is the reverse of the brief's implication:** the flags are not a large-crate
   workaround, they are a per-edit lottery, so pass them unconditionally.

2. **`wasm-pack` is not installed here, and `cargo install` is the way in — not the release tarball.**
   The brief describes the wasm32 target as already added and the toolchain as proven, but
   `wasm-pack` itself was absent. The GitHub release asset (20 MB) was still crawling after 25
   minutes; `cargo install wasm-pack --locked` from crates.io finished in **44 seconds**. GitHub
   releases are effectively unusable from this box, crates.io is not.

3. **The panel and the console are byte-identical between this branch and `main`** —
   `git diff --stat main...HEAD` is empty for both directories. So the baseline is not
   branch-specific, and P2's "before" numbers are also `main`'s. Worth stating because the brief
   implies the branch already carries UI work.

4. **`lib/contract.gen.ts` is generated from `agent/src/vocabulary.rs`.** The brief treats the panel
   as TypeScript that needs to become Rust; part of it already *is* Rust, mirrored by a generator and
   policed by `contract-vocabulary-check.mjs`. That is a working precedent for the whole migration
   sitting in the tree already.

**And one thing the brief got right that is worth confirming:** the main checkout was at `502ddbf2`
and `feat/the-product-moves-to-rust` at `46fa0a3f`; the worktree `/tmp/rustmig` was already on the
branch, and it carried one uncommitted change (`scripts/test/all-gates.bash`, another agent's work on
the gate migration). **It was left untouched and is not part of this commit.**
