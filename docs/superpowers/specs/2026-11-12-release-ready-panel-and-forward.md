# Release-ready: the panel and the forward feature, measured 2026-11-12

**Status: READY TO BUILD, NOT RELEASED.** The operator holds the device upgrade; nothing here ships until they say so. This file is
what a release would do, what proves it, and what still has to be true first.

## What would ship

| surface | what changes for a person | where it lives |
|---|---|---|
| **the desktop panel** | the panel the device serves — its page frame, the layout contract's head rules, the memory page, the timeline — built into `summrise-device.exe` | `agent/resources/panel-react/src` → `agent/resources/panel/{panel.js, panel.css}`, embedded by `agent/src/web/panel.rs` (`include_str!`) |
| **the forward plugin** | the rules that carry a `host:port` through an SSH hop with nobody present | `agent/src/plugins/forward/` — **merged in source, carried by NO live build** |

## What proves the panel is ready (all run 2026-11-12 on this tree)

| gate | result |
|---|---|
| `npx vitest run` (panel-react) | **120 files / 1,066 tests passed** |
| `npm run build` (panel-react) | `../panel/panel.js 696.86 kB │ gzip 196.77 kB` |
| `cargo test --test panel_sheet_freshness` | **1 passed** |
| `cargo test --test spacing_scale` | **2 passed** |
| `cargo test --test stylesheet_hygiene` | **5 passed** |

**AND ONE THING THE BUILD ITSELF WILL STOP YOU ON.** The agent's `build.rs` refuses to compile a device binary when the panel
bundle's mtime is older than its source, and it says why:

    summrise-agent build: STALE panel bundle — resources/panel-react/src is newer than resources/panel/ by 1719s …
    panel.js is embedded at compile time, so this build would ship the OLD panel.
    Rebuild first: `cd agent/resources/panel-react && npm run build`

Measured today: the rebuild produced a **byte-identical** `panel.js` (git saw no change) — the bundle was correct, its TIMESTAMP was
not. So a clean checkout can fail the device build with a stale-bundle panic that is about mtime, not content.

**THEREFORE THE FIRST STEP OF ANY DEVICE BUILD IS THE PANEL BUILD**, before `cargo`:

    cd agent/resources/panel-react && npm run build
    cd ../../.. && ./scripts/build.sh agent        # or the installer script the release runbook names

## What the fleet shows today about the forward feature

Measured 2026-11-12 through the console's own proxy against DESKTOP-9VH92BJ (`0.3.292`): the device advertises **46 tools and not one
of them is `forward.*`**. The console's `ForwardRules` page draws only when the tool exists, so on this fleet the page is correctly
absent — not broken. **The forward plugin has never run on a device.**

## What still has to be true before forward can be USED

1. **A device build that carries the plugin.** Until one is installed, `forward.list` cannot be measured, and the console's reader is
   written against the source (`{rules: [{name, listen, target, via, state: {…}}]}`) — see
   `docs/superpowers/specs/2026-11-12-device-wire-shapes.md`, where it is marked **NOT measured**.
2. **One live call to `forward.list`** on that build, recorded in the wire-shapes file, and a test named for the shape it answers.
3. **A rule that actually carries a connection** — the feature's own proof, which needs a second machine to dial through.

## How a release would go (when the operator lifts the hold)

1. `cd agent/resources/panel-react && npm run build` — the panel bundle, every time.
2. Build the installer (the release runbook's script; `scripts/build-installer.sh` is currently another workstream's file and must not
   be swept up by this one).
3. Verify the artifact: size, sha256, and that the embedded panel carries today's contract (`panel_sheet_freshness` again).
4. Stage it where the console's `/installer` reads, then let **one** machine take it — DESKTOP-9VH92BJ is the console's own host and
   the least disruptive first subject.
5. Read the new build's `/panel` on that machine, and `forward.list` through the console.
6. Only then the rest of the fleet.

## Rollback

The device keeps its previous executable beside the new one (the update's own swap), and the console's `/installer` can serve the
older artifact again by name. **The panel rolls back with the binary** — it is embedded, not fetched — so there is no second thing to
undo.

## What this note does NOT claim

- It does not claim a device build was produced today: none was.
- It does not claim the forward page works: it has never drawn a rule from a live device.
- It does not claim the panel's new pages have been seen on a device: their behaviour is proven by the panel's own tests and the
  contract gates, not by a screenshot from a shipped build.
