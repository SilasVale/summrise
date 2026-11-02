# The console workspace, next — the audits' ranking

**Date:** 2026-11-02. **Status:** working plan for the standing product goal. The last pass shipped
`0.3.219` (console), the landing, and PR #68; this file ranks what the four read-only audits found
and fixes the order of the next slices. It is not a rewrite and not a new surface.

## What is already true

- The page is React 19 for rendering and **Rust for the decisions that have no DOM**: the first
  `console-logic` wasm family (ids, ages, retry sentences) ships in `0.3.219`, with a source-stamp gate.
- The machine has a home: `HostFacts` is Machine's first section; the Devices card is a picker again.
- Post-sign-in failures have one alert region; boot can retry; fleet/audit reads fail independently.
- The React sheet is behind a Rust AA test (11 pairs × 2 themes). It caught `--ink-faint` at 2.46:1.
- The landing and the guides no longer sell the deleted approval queue.
- `Connect AI client` opens on the permanent token, destructive consent is explicit, and Fleet tokens
  is reachable from Devices.

## The audits' verdict

Four read-only audits (IA, visual system, functional usability, first minute) agreed on the thesis
and disagreed with the page's **rank**, not its craft. The findings worth carrying:

| # | Finding | Severity | Where |
|---|---|---|---|
| 1 | Terminal is not the primary object: ~11 controls above it, three "no session" sentences, sessions hidden until "Attach", terminal capped at 384px | P1 | TerminalWorkspace |
| 2 | Activity never learns which machine is selected; it is a fleet log, not the machine's record | P1 | AuditTable / App |
| 3 | Accent does six jobs (primary, nav, selection, links, chart, focus); the picker repeats three identical fills | P1 | app.css |
| 4 | The terminal hardcodes 12 hexes and paints a black canvas in light mode | P1 | TerminalWorkspace xterm theme |
| 5 | The "one palette" is two sheets: 16 of 26 shared names differ from `device/ui` | P1 | tokens.css |
| 6 | Spacing is 72% literal (150/209); weights are 450/550/600/650 literals; two off-scale rem sizes | P2 | app.css |
| 7 | The screenshot harness never renders a selected tool surface; the browser path is unproven | P1 | screenshot.mjs, no Playwright flow |
| 8 | The first screen is Devices now, but the zero-device state still needs a door and the rail labels are hidden | P2 | App / Rail |

Findings already closed this pass: the machine's home, invisible errors, stale grant copy, the
enrollment command, the token default, the landing's approval claim, `--ink-faint`, disabled/pressed
states, reduced motion, and the duplicated page name.

## Slice order

Each slice lands with tests, a build, and a rendered before/after. No slice adds a page, a framework
or a second runtime.

### 1. The terminal is the primary object

- Auto-list `terminal.list` on mount (a device with sessions shows them without an "Attach" click);
  presets are the second list, under a disclosure.
- Show the toolbar only when a tab exists; at the empty state the one filled control is `+ New`.
- One empty-state sentence, not three; delete the dashed box that repeats it.
- The terminal takes the remaining height (`1fr`, min 18rem), not `min(52vh, 24rem)`.
- Acceptance: with a chosen device, the first screenful is the session list and `+ New`; a new pty
  puts focus in the xterm; no control above it is disabled at rest.

### 2. Activity is the machine's record

- A visible `This machine / All machines` scope, defaulting to the selected machine and staying in
  sync when the rail's selection changes.
- Expanded rows carry the full device id and the run's stdout/stderr when the row is a run.
- Keep Fleet tokens on Devices; Activity stays the record.
- Acceptance: select a machine, open Activity, and the table is scoped without touching a filter;
  the scope persists across the rail.

### 3. One accent, one job

- Decide the picker's single primary: the card itself becomes the target or `Work on this` becomes
  the only filled control; `New enrollment code` is neutral when the fleet is non-empty.
- Selected state is a tint, not a second brand mark; links use the readable ink pair, not `--accent`.
- The vitals series stop using the brand accent (a chart is not an action).
- Acceptance: at most one accent-filled control per screen; desaturate the page and every state
  still reads.

### 4. One palette, one source

- Generate `device/console/web/src/tokens.css` from `device/ui` (the Rust palette), and add a Rust
  test that fails on drift.
- The xterm theme reads `--term-*` (light canvas white, dark `#131418`); no raw hex in TSX.
- Acceptance: 0/26 shared names differ; the terminal's light canvas is the page's white; the AA gate
  still passes.

### 5. Type and space, family by family

- Fold 26 literal weights to 500/600 and the two rem sizes to the scale (15.2→13, 10.4→11).
- Convert the 42 pixel-neutral spacing declarations first (screenshot diff: 0 changed pixels), then
  the ≤1.2px family page by page.
- Acceptance: the ratchet's token share rises with a measured pixel budget; no page moves without
  its own before/after.

### 6. The evidence path

- Add a Playwright flow: built page → operator-token sign-in → select device → `+ New` pty → type a
  marker → read it back through xterm. The jsdom suite cannot prove the `/mcp` shape.
- Capture the selected state of all five workspace pages, and link the workspace shot in the gallery.
- Acceptance: the flow fails on a broken `/mcp` auth shape; the gallery has one selected shot per
  surface.

### 7. Real-device verification (operator-gated)

The reachable Windows host is the operator's, as recorded in the charter. When it is offered: install
`0.3.219`, open a pty, capture the screen, restart the app, and verify the relay card. Until then the
claim stays "not verified on a real device", not "verified".

## Non-goals

- No new navigation model, no return of a context rail, no tenancy, no BYOK growth.
- No Electron, no runtime Node, no second feedback path.
- No bulk spacing rewrite: one family per commit, with a pixel proof.

## Verification for every slice

```
cd device/console/web && npm run typecheck && npm test && npm run build
cd ../../.. && ./scripts/build-web.sh --check        # includes the wasm source stamp
cargo test --manifest-path device/Cargo.toml -p summrise-console --test tokens
cargo test --manifest-path device/console-logic/Cargo.toml
node device/console/web/scripts/screenshot.mjs <tag>  # look at the gallery
node landing/check.mjs                                # when the landing moves
```

A deploy is `stage-console-deploy.bash <version>` then the host command, then `/health` must name the
version and the page bytes must match `dist/index.html`. Rollback is the `.bak-<previous>` binary.

## Deployment record (2026-11-02)

- **Console `0.3.219`** deployed to `DESKTOP-9VH92BJ`: hash `06184e5d…` matched the staged digest,
  previous `0.3.218`, backup kept, `/health` reports `version: 0.3.219` with 3 devices online, and the
  served page is 772,790 bytes — the same bytes as `device/console/web/dist/index.html`, with the
  wasm logic and the Fleet tokens entry in it.
- **Landing** deployed (`summrise-landing`, version `bbdeb932…`): live `summrise.saisi.online` says
  "Consent and audit", carries the React Devices screenshot, and contains no ninety-second-hold claim
  in either language.
- The canary mirror on the tailnet is what the host fetched from; it is a deploy-time server, not a
  product surface.
